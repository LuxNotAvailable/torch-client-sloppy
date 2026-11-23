//! Writes one `.torchmod` (`docs/TORCHMOD_FORMAT.md`) from a jar and the
//! dump: the jar's assets and the dump's entries in its namespaces.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Env;
use crate::jar::ModJar;

pub const FORMAT: u32 = 1;

fn namespace_of(name: &str) -> &str {
    name.split_once(':').map_or("minecraft", |(ns, _)| ns)
}

/// How many blocks lead the dump under `minecraft:`: the game's own. A
/// `minecraft:` block after them was added by a mod (a backport).
fn vanilla_run(dump: &Value) -> usize {
    dump["blocks"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .take_while(|b| {
                    b["name"]
                        .as_str()
                        .is_some_and(|n| n.starts_with("minecraft:"))
                })
                .count()
        })
        .unwrap_or(0)
}

/// The dump's blocks in `namespaces`, and the `minecraft:` ones past
/// vanilla's that the jar `claims`, with `facts` and `shapes` cut down to
/// what they use and renumbered.
fn blocks(dump: &Value, namespaces: &BTreeSet<String>, claims: &BTreeSet<String>) -> Option<Value> {
    let vanilla = vanilla_run(dump);
    let all_facts = dump["facts"].as_array()?;
    let all_shapes = dump["shapes"].as_array()?;
    let mut facts = Vec::new();
    let mut fact_ids: HashMap<u64, usize> = HashMap::new();
    // Shapes 0 (empty) and 1 (full cube) keep their indices.
    let mut shapes: Vec<Value> = all_shapes.iter().take(2).cloned().collect();
    let mut shape_ids: HashMap<u64, usize> =
        (0..shapes.len() as u64).map(|i| (i, i as usize)).collect();
    let mut out = Vec::new();
    for (index, block) in dump["blocks"].as_array()?.iter().enumerate() {
        let name = block["name"].as_str()?;
        let claimed = index >= vanilla && claims.contains(name);
        if !namespaces.contains(namespace_of(name)) && !claimed {
            continue;
        }
        let mut block = block.clone();
        for state in block["states"].as_array_mut()? {
            let row = state.as_array_mut()?;
            let old = row.first()?.as_u64()?;
            let new = match fact_ids.get(&old) {
                Some(&id) => id,
                None => {
                    let mut fact = all_facts.get(old as usize)?.clone();
                    for key in ["collision", "outline"] {
                        if let Some(shape) = fact[key].as_u64() {
                            let id = *shape_ids.entry(shape).or_insert_with(|| {
                                shapes.push(
                                    all_shapes.get(shape as usize).cloned().unwrap_or(json!([])),
                                );
                                shapes.len() - 1
                            });
                            fact[key] = json!(id);
                        }
                    }
                    facts.push(fact);
                    fact_ids.insert(old, facts.len() - 1);
                    facts.len() - 1
                }
            };
            row[0] = json!(new);
        }
        out.push(block);
    }
    if out.is_empty() {
        return None;
    }
    Some(json!({"format": 1, "blocks": out, "facts": facts, "shapes": shapes}))
}

/// The dump's connected textures whose sheet the jar ships.
fn connected(dump: &Value, jar: &ModJar) -> Vec<Value> {
    let ships = |id: &str| {
        let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
        let key = format!("assets/{ns}/textures/{path}.png");
        jar.assets.iter().any(|(k, _)| *k == key)
    };
    let mut out: Vec<Value> = dump["connected_textures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|ct| ct["connected"].as_str().is_some_and(ships))
        .cloned()
        .collect();
    out.sort_by_key(|ct| ct.to_string());
    out
}

fn named(dump: &Value, key: &str, namespaces: &BTreeSet<String>) -> Vec<Value> {
    dump[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| {
            e["name"]
                .as_str()
                .is_some_and(|n| namespaces.contains(namespace_of(n)))
        })
        .cloned()
        .collect()
}

fn file_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The package for `jar`, or `None` when it has nothing the client uses
/// (a library with no assets, blocks, entities or items).
pub fn write(env: &Env, jar: &ModJar, dump: &Value, out: &Path) -> Result<Option<PathBuf>, String> {
    let Some(main) = jar.mods.first() else {
        return Ok(None);
    };
    let namespaces = jar.namespaces();
    let blocks = blocks(dump, &namespaces, &jar.minecraft_blockstates);
    let entities = named(dump, "entities", &namespaces);
    let items = named(dump, "items", &namespaces);
    let connected = connected(dump, jar);
    if jar.assets.is_empty() && blocks.is_none() && entities.is_empty() && items.is_empty() {
        return Ok(None);
    }
    // The running version the dump saw beats a placeholder in the toml.
    let version = dump["mods"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["id"].as_str() == Some(main.id.as_str()))
        .and_then(|m| m["version"].as_str())
        .unwrap_or(&main.version)
        .to_owned();
    let manifest = json!({
        "format": FORMAT,
        "mod": {"id": main.id, "version": version, "name": main.name, "license": jar.license},
        "mods": jar.mods.iter().map(|m| json!({"id": m.id, "version": m.version})).collect::<Vec<_>>(),
        "namespaces": namespaces,
        "source": {"file": jar.file, "sha256": jar.sha256},
        "game": {"minecraft": "1.21.1", "loader": "neoforge", "loader_version": env.neoforge},
        "dependencies": jar.dependencies,
        "converter": concat!("torch-convert ", env!("CARGO_PKG_VERSION")),
        "api": 0,
    });
    let path = out.join(format!(
        "{}-{}.torchmod",
        file_safe(&main.id),
        file_safe(&version)
    ));
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        z.start_file(name, options).map_err(|e| e.to_string())?;
        z.write_all(bytes).map_err(|e| e.to_string())
    };
    let pretty = |v: &Value| serde_json::to_vec_pretty(v).unwrap_or_default();
    add("torchmod.json", &pretty(&manifest))?;
    if let Some(blocks) = &blocks {
        add(
            "blocks.json",
            &serde_json::to_vec(blocks).unwrap_or_default(),
        )?;
    }
    if !entities.is_empty() {
        add("entities.json", &pretty(&json!(entities)))?;
    }
    if !items.is_empty() {
        add(
            "items.json",
            &serde_json::to_vec(&json!(items)).unwrap_or_default(),
        )?;
    }
    if !connected.is_empty() {
        add("connected.json", &pretty(&json!(connected)))?;
    }
    let mut seen = BTreeSet::new();
    for (key, bytes) in &jar.assets {
        if seen.insert(key.as_str()) {
            add(key, bytes)?;
        }
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(Some(path))
}
