//! The input folder's jars: their mod ids and what to do with each.

use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    Convert,
    LoadOnly,
    Skip,
}

impl Action {
    pub fn next(self) -> Action {
        match self {
            Action::Convert => Action::LoadOnly,
            Action::LoadOnly => Action::Skip,
            Action::Skip => Action::Convert,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Convert => "Convert",
            Action::LoadOnly => "Load only",
            Action::Skip => "Skip",
        }
    }
}

pub struct Jar {
    pub path: PathBuf,
    pub name: String,
    pub mods: Vec<String>,
    pub suggested: Action,
    pub why: String,
    pub action: Action,
}

/// File name fragments of mods the dump server can't load (client-only)
/// or doesn't need (performance), with why: they start as Skip.
const SKIP_HINTS: &[(&str, &str)] = &[
    ("sodium", "client-only renderer"),
    ("iris-", "client-only shaders"),
    ("distanthorizons", "client-only LOD renderer"),
    ("immediatelyfast", "client-only renderer"),
    ("entityculling", "client-only"),
    ("dynamic-fps", "client-only"),
    ("asyncparticles", "client-only"),
    ("gpubooster", "client-only"),
    ("gpushift", "client-only"),
    ("chloride", "client-only"),
    ("hdr_mod", "client-only"),
    ("skinlayers3d", "client-only"),
    ("chat_heads", "client-only"),
    ("invmove", "client-only"),
    ("mousetweaks", "client-only"),
    ("appleskin", "client HUD"),
    ("jade", "client HUD"),
    ("xaero", "client map"),
    ("jei-", "recipe viewer"),
    ("justenoughresources", "recipe viewer"),
    ("mezz_config", "recipe viewer library"),
    ("polymorph", "recipe UI"),
    ("toadlib", "client-only library (crashes the server)"),
    ("modernfix", "performance only"),
    ("ferritecore", "performance only"),
    ("c2me", "performance only"),
    ("lithium-", "performance only"),
    ("alternate_current", "performance only"),
    ("packetfixer", "networking only"),
    ("zfastnoise", "performance only"),
];

/// The jars in `folder`, sorted, with suggested actions.
pub fn scan(folder: &Path) -> Vec<Jar> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")))
        .collect();
    paths.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let lower = name.to_lowercase();
            let mods = mod_ids(&path);
            let hint = SKIP_HINTS.iter().find(|(frag, _)| lower.contains(frag));
            let (suggested, why) = match (hint, mods.is_empty()) {
                (Some((_, why)), _) => (Action::Skip, (*why).to_owned()),
                (None, true) => (Action::Skip, "no NeoForge mod in it".to_owned()),
                (None, false) => (Action::Convert, String::new()),
            };
            Jar {
                path,
                name,
                mods,
                suggested,
                why,
                action: suggested,
            }
        })
        .collect()
}

/// The jar's own mod ids, from `neoforge.mods.toml` or `mods.toml`.
pub fn mod_ids(jar: &Path) -> Vec<String> {
    let Ok(file) = std::fs::File::open(jar) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return Vec::new();
    };
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Ok(mut f) = zip.by_name(name) {
            let mut text = String::new();
            if f.read_to_string(&mut text).is_ok() {
                return toml_mod_ids(&text);
            }
        }
    }
    Vec::new()
}

/// `modId`s of the jar's own mods: the top level (an inline
/// `mods = [{...}]`) and `[[mods]]` tables, never `[[dependencies...]]`.
pub fn toml_mod_ids(toml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut own = true;
    for line in toml.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            own = t.starts_with("[[mods]]");
            continue;
        }
        if own {
            out.extend(values(t, "modId"));
        }
    }
    out
}

/// Every `key = "v"` or `key = 'v'` in `line`.
fn values(line: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find(key) {
        let after = rest[i + key.len()..].trim_start();
        rest = &rest[i + key.len()..];
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        if let Some(end) = after[1..].find(quote) {
            out.push(after[1..1 + end].to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_mods_count_and_dependencies_dont() {
        let table = "modLoader = \"javafml\"\n[[mods]]\nmodId = \"create\"\n[[dependencies.create]]\nmodId = \"flywheel\"\n";
        assert_eq!(toml_mod_ids(table), ["create"]);
        let inline = "loaderVersion = '[1,)'\nmods = [\n\t{ modId = 'mr_create_balancedrecipes', version = '1.0.1' },\n]\n";
        assert_eq!(toml_mod_ids(inline), ["mr_create_balancedrecipes"]);
    }

    #[test]
    fn actions_cycle() {
        assert_eq!(Action::Convert.next().next().next(), Action::Convert);
    }
}
