//! Converted mods: each `.torchmod` package in `<storage>/mods`
//! (`docs/TORCHMOD_FORMAT.md`, written by `crates/torch-convert`) mounts its
//! assets below the resource packs and hands its blocks to
//! `blocks::modded`. The client only loads packages: it reads no jars and
//! runs no mod code.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::platform::assets::archive::Layer;
use crate::util::pack::Source;

pub(crate) fn dir() -> PathBuf {
    crate::platform::storage::dir().join("mods")
}

/// The package format this client reads.
const FORMAT: u64 = 1;

/// The folders under `assets/<namespace>/` a package may hold.
const KEPT: &[&str] = &["blockstates", "models", "items", "textures", "lang"];

/// What one package gives the client.
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) layer: Layer,
    /// `blocks.json`, when it has one.
    pub(crate) blocks: Option<String>,
    /// `connected.json`: Create-style connected textures.
    pub(crate) connected: Option<String>,
    /// `plugin.wasm`: ported client code (`docs/MOD_API.md`).
    pub(crate) plugin: Option<Vec<u8>>,
}

/// Every readable package in `dir`, in name order.
pub(crate) fn packages(dir: &Path) -> Vec<Package> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("torchmod"))
        })
        .collect();
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match package(&file, &name) {
            Ok(package) => {
                crate::log_info!(
                    "mods",
                    "{name}: {} asset files{}",
                    package.layer.len(),
                    if package.blocks.is_some() {
                        ", blocks"
                    } else {
                        ""
                    }
                );
                out.push(package);
            }
            Err(e) => crate::log_warn!("mods", "{name} is not loaded: {e}"),
        }
    }
    out
}

/// Whether `key` (`assets/<namespace>/<folder>/...`) is an asset the
/// package may give: one of its own namespaces, never `minecraft`.
fn kept(key: &str, namespaces: &[String]) -> bool {
    let mut parts = key.splitn(4, '/');
    parts.next() == Some("assets")
        && parts
            .next()
            .is_some_and(|ns| ns != "minecraft" && namespaces.iter().any(|n| n == ns))
        && parts.next().is_some_and(|folder| KEPT.contains(&folder))
        && parts.next().is_some_and(|rest| !rest.is_empty())
}

fn package(path: &Path, name: &str) -> Result<Package, String> {
    let source = Source::open(path, "")?;
    let manifest: Value = source
        .read_text("/torchmod.json")
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or("it has no readable torchmod.json")?;
    match manifest["format"].as_u64() {
        Some(FORMAT) => {}
        other => return Err(format!("package format {other:?} isn't {FORMAT}")),
    }
    let namespaces: Vec<String> = manifest["namespaces"]
        .as_array()
        .ok_or("its manifest lists no namespaces")?
        .iter()
        .filter_map(|n| n.as_str().map(str::to_owned))
        .collect();
    let mut bytes = Vec::new();
    let mut ranges = Vec::new();
    for file in source.files() {
        let key = file.trim_start_matches('/');
        if !kept(key, &namespaces) {
            continue;
        }
        let Some(data) = source.read(&file) else {
            continue;
        };
        ranges.push((key.to_owned(), bytes.len()..bytes.len() + data.len()));
        bytes.extend_from_slice(&data);
    }
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    Ok(Package {
        name: name.to_owned(),
        layer: ranges
            .into_iter()
            .map(|(key, range)| (key, &bytes[range]))
            .collect(),
        blocks: source.read_text("/blocks.json"),
        connected: source.read_text("/connected.json"),
        plugin: source.read("/plugin.wasm"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &[u8] = br#"{"format": 1, "mod": {"id": "gizmos", "version": "1.0"},
        "namespaces": ["gizmos"], "api": 0}"#;

    #[test]
    fn only_a_packages_own_namespaces_mount() {
        let own = ["create".to_owned()];
        assert!(kept("assets/create/blockstates/shaft.json", &own));
        assert!(kept("assets/create/textures/block/belt/belt.png", &own));
        assert!(!kept("assets/other/blockstates/shaft.json", &own));
        assert!(!kept(
            "assets/minecraft/textures/block/stone.png",
            &["minecraft".to_owned()]
        ));
        assert!(!kept("assets/create/sounds/cogs.ogg", &own));
        assert!(!kept("assets/create/textures", &own));
        assert!(!kept("torchmod.json", &own));
    }

    #[test]
    fn a_package_mounts_its_assets_and_hands_over_its_blocks() {
        let dir = crate::util::pack::fixture("torchmod-packages");
        crate::util::zip::write_test_archive(
            &dir.join("gizmos-1.0.torchmod"),
            &[
                ("torchmod.json", MANIFEST),
                ("assets/gizmos/blockstates/gizmo.json", b"{}".as_slice()),
                ("assets/minecraft/blockstates/stone.json", b"{}".as_slice()),
                ("blocks.json", br#"{"format": 1}"#.as_slice()),
            ],
        );
        crate::util::zip::write_test_archive(
            &dir.join("old.torchmod"),
            &[(
                "torchmod.json",
                br#"{"format": 99, "namespaces": []}"#.as_slice(),
            )],
        );
        // Jars aren't packages: the client never reads them.
        crate::util::zip::write_test_archive(
            &dir.join("gizmos-1.0.jar"),
            &[("assets/gizmos/blockstates/gizmo.json", b"{}".as_slice())],
        );
        let packages = packages(&dir);
        assert_eq!(packages.len(), 1);
        let keys: Vec<&str> = packages[0].layer.keys().map(String::as_str).collect();
        assert_eq!(keys, ["assets/gizmos/blockstates/gizmo.json"]);
        assert_eq!(packages[0].blocks.as_deref(), Some(r#"{"format": 1}"#));
    }
}
