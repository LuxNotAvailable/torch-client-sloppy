//! What a mod jar holds that a package needs: its mods (from
//! `neoforge.mods.toml`, nested jar-in-jar ones too), licence,
//! dependencies, and its own assets.

use std::collections::BTreeSet;
use std::io::{Cursor, Read, Seek};
use std::path::Path;

/// The folders under `assets/<namespace>/` a package carries.
pub const KEPT: &[&str] = &["blockstates", "models", "items", "textures", "lang"];

pub struct ModInfo {
    pub id: String,
    pub version: String,
    pub name: String,
}

pub struct ModJar {
    pub file: String,
    pub sha256: String,
    /// The jar's mods, its own first, then nested jars'.
    pub mods: Vec<ModInfo>,
    pub license: Option<String>,
    /// Required mod ids from outside the jar.
    pub dependencies: Vec<String>,
    /// `(assets/<namespace>/..., bytes)`, only kept folders, never `minecraft`.
    pub assets: Vec<(String, Vec<u8>)>,
    /// `minecraft:` blocks it ships a blockstate for (`pale_oak_log` from a
    /// backport): the ones it may claim when they aren't vanilla's.
    pub minecraft_blockstates: BTreeSet<String>,
}

impl ModJar {
    pub fn read(path: &Path) -> Result<ModJar, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut out = ModJar {
            sha256: crate::setup::sha256(&bytes),
            file: file.clone(),
            mods: Vec::new(),
            license: None,
            dependencies: Vec::new(),
            assets: Vec::new(),
            minecraft_blockstates: BTreeSet::new(),
        };
        read_into(Cursor::new(bytes), &mut out, 0).map_err(|e| format!("{file}: {e}"))?;
        let own: BTreeSet<&str> = out.mods.iter().map(|m| m.id.as_str()).collect();
        let mut deps: Vec<String> = out
            .dependencies
            .iter()
            .filter(|d| {
                !own.contains(d.as_str()) && !["minecraft", "neoforge"].contains(&d.as_str())
            })
            .cloned()
            .collect();
        deps.sort();
        deps.dedup();
        out.dependencies = deps;
        Ok(out)
    }

    /// The namespaces its blocks, items and assets live in: its mods' ids
    /// and its asset folders' names.
    pub fn namespaces(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = self.mods.iter().map(|m| m.id.clone()).collect();
        for (key, _) in &self.assets {
            if let Some(ns) = key.split('/').nth(1) {
                out.insert(ns.to_owned());
            }
        }
        out.remove("minecraft");
        out
    }
}

fn read_into<R: Read + Seek>(reader: R, out: &mut ModJar, depth: u32) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let mut manifest_version = None;
    let mut nested = Vec::new();
    let mut toml_text = None;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let mut read = || -> Result<Vec<u8>, String> {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| format!("{name}: {e}"))?;
            Ok(bytes)
        };
        if name == "META-INF/neoforge.mods.toml"
            || (name == "META-INF/mods.toml" && toml_text.is_none())
        {
            toml_text = Some(String::from_utf8_lossy(&read()?).into_owned());
        } else if name == "META-INF/MANIFEST.MF" {
            let text = String::from_utf8_lossy(&read()?).into_owned();
            manifest_version = text
                .lines()
                .find_map(|l| l.strip_prefix("Implementation-Version:"))
                .map(|v| v.trim().to_owned());
        } else if depth < 2 && name.starts_with("META-INF/jarjar/") && name.ends_with(".jar") {
            nested.push(read()?);
        } else if kept(&name) {
            let bytes = read()?;
            out.assets.push((name, bytes));
        } else if let Some(stem) = name
            .strip_prefix("assets/minecraft/blockstates/")
            .and_then(|rest| rest.strip_suffix(".json"))
        {
            out.minecraft_blockstates
                .insert(format!("minecraft:{stem}"));
        }
    }
    if let Some(text) = toml_text {
        mods_toml(&text, manifest_version.as_deref(), out, depth == 0)?;
    }
    for bytes in nested {
        // A nested jar that isn't a mod (a plain library) has nothing to add.
        let _ = read_into(Cursor::new(bytes), out, depth + 1);
    }
    Ok(())
}

fn kept(key: &str) -> bool {
    let mut parts = key.splitn(4, '/');
    parts.next() == Some("assets")
        && parts
            .next()
            .is_some_and(|ns| ns != "minecraft" && !ns.is_empty())
        && parts.next().is_some_and(|folder| KEPT.contains(&folder))
        && parts.next().is_some_and(|rest| !rest.is_empty())
}

fn mods_toml(
    text: &str,
    jar_version: Option<&str>,
    out: &mut ModJar,
    top: bool,
) -> Result<(), String> {
    let root: toml::Value = toml::from_str(text).map_err(|e| format!("mods.toml: {e}"))?;
    if top {
        out.license = root
            .get("license")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
    }
    let str_of =
        |v: &toml::Value, key: &str| v.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    for m in root
        .get("mods")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let Some(id) = str_of(m, "modId") else {
            continue;
        };
        let version = match str_of(m, "version") {
            Some(v) if v.contains("${") => jar_version.unwrap_or("unknown").to_owned(),
            Some(v) => v,
            None => "unknown".to_owned(),
        };
        let name = str_of(m, "displayName").unwrap_or_else(|| id.clone());
        out.mods.push(ModInfo { id, version, name });
    }
    if let Some(deps) = root.get("dependencies").and_then(|v| v.as_table()) {
        for list in deps.values().filter_map(|v| v.as_array()) {
            for dep in list {
                let required = match dep.get("type").and_then(|v| v.as_str()) {
                    Some(t) => t.eq_ignore_ascii_case("required"),
                    None => dep
                        .get("mandatory")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                };
                if required && let Some(id) = str_of(dep, "modId") {
                    out.dependencies.push(id);
                }
            }
        }
    }
    Ok(())
}
