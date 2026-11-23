//! Blockstate registry and model baking. Per-state facts: `info`. Not
//! lighting or meshing.

use crate::platform::time::Instant;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

pub mod ambient;
pub mod bake;
pub mod facing;
pub mod info;
pub mod modded;
pub mod rand;
pub mod rotation;
pub mod shape;
pub mod state;
pub mod tags;

#[cfg(test)]
pub(crate) mod tests;

pub use bake::{BakedBlock, BakedModel, BakedQuad};
use state::{ModelRef, Rot, StateDef};

type ModelKey = (String, Rot);

pub struct Blocks {
    defs: &'static HashMap<String, StateDef>,
    models: RwLock<HashMap<ModelKey, Arc<BakedModel>>>,
    baked: RwLock<HashMap<u16, Arc<BakedBlock>>>,
    /// Modded states' bakes, for one `modded::generation`: their ids mean
    /// other blocks on another server.
    modded_baked: RwLock<(u32, HashMap<u16, Arc<BakedBlock>>)>,
    atlas_rows: u32,
    missing_defs: AtomicU32,
    missing_textures: AtomicU32,
}

static REGISTRY: OnceLock<Blocks> = OnceLock::new();

/// Every blockstate definition: vanilla's by name (`stone`), a mod's or
/// pack's by id (`create:shaft`).
fn definitions() -> &'static HashMap<String, StateDef> {
    static DEFS: OnceLock<HashMap<String, StateDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        let started = Instant::now();
        let defs = read_definitions(
            crate::assets_root(),
            crate::items::model::shared(),
            &crate::platform::assets::namespaces(),
        );
        println!(
            "[BlockModels] {} blockstates parsed in {:.1} ms",
            defs.len(),
            started.elapsed().as_secs_f32() * 1000.0
        );
        defs
    })
}

fn read_definitions(
    root: &std::path::Path,
    assets: &crate::items::model::Assets,
    namespaces: &[String],
) -> HashMap<String, StateDef> {
    let mut defs = HashMap::new();
    let mut read = |dir: std::path::PathBuf, namespace: Option<&str>| {
        let files = crate::platform::assets::read_dir(&dir);
        if files.is_empty() && namespace.is_none() {
            eprintln!("[BlockModels] cannot read {}", dir.display());
        }
        for path in files {
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let name = match namespace {
                Some(ns) => format!("{ns}:{stem}"),
                None => stem.to_string(),
            };
            let Some(json) = assets.json(&format!("blockstates/{name}.json")) else {
                eprintln!("[BlockModels] {name}.json did not parse");
                continue;
            };
            match StateDef::parse(&json) {
                Some(def) => {
                    defs.insert(name, def);
                }
                None => eprintln!("[BlockModels] {name}.json has no variants/multipart"),
            }
        }
    };
    read(root.join("blockstates"), None);
    if let Some(beside) = root.parent() {
        for ns in namespaces {
            read(beside.join(ns).join("blockstates"), Some(ns));
        }
    }
    defs
}

/// The textures mods' block models use, as atlas keys (`create:block/shaft`).
/// Vanilla's are already in the atlas from `textures/block`.
pub fn mod_sprites() -> Vec<String> {
    mod_sprites_of(definitions(), crate::items::model::shared())
}

fn mod_sprites_of(
    defs: &HashMap<String, StateDef>,
    assets: &crate::items::model::Assets,
) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut seen = std::collections::HashSet::new();
    for def in defs.values() {
        for reference in def.models() {
            if !seen.insert(reference.model.as_str()) {
                continue;
            }
            let resolved = assets.model(&reference.model);
            for value in resolved.textures.values() {
                let path = crate::items::model::strip_namespace(value);
                if !path.starts_with('#') && path.contains(':') {
                    out.insert(path.to_string());
                }
            }
        }
    }
    out.into_iter().collect()
}

pub fn registry() -> &'static Blocks {
    REGISTRY.get_or_init(Blocks::load)
}

pub fn cache_bytes() -> (u64, usize, usize) {
    REGISTRY.get().map(Blocks::cache_bytes).unwrap_or((0, 0, 0))
}

impl Blocks {
    fn load() -> Blocks {
        let atlas_rows = crate::ATLAS_ROWS
            .get()
            .copied()
            .unwrap_or(crate::terrain::TILE_PX);
        Blocks {
            defs: definitions(),
            models: RwLock::new(HashMap::new()),
            baked: RwLock::new(HashMap::new()),
            modded_baked: RwLock::new((0, HashMap::new())),
            atlas_rows,
            missing_defs: AtomicU32::new(0),
            missing_textures: AtomicU32::new(0),
        }
    }

    pub fn cache_bytes(&self) -> (u64, usize, usize) {
        let models = self.models.read();
        let quad = std::mem::size_of::<bake::BakedQuad>() as u64;
        let bytes = models.values().map(|m| m.quads.len() as u64 * quad).sum();
        (bytes, self.baked.read().len(), models.len())
    }

    pub fn baked(&self, state_id: u16, name: &str, props: &HashMap<&str, &str>) -> Arc<BakedBlock> {
        if state_id > azalea::block::BlockState::MAX_STATE {
            let generation = modded::generation();
            {
                let cache = self.modded_baked.read();
                if cache.0 == generation
                    && let Some(hit) = cache.1.get(&state_id)
                {
                    return hit.clone();
                }
            }
            let built = Arc::new(self.build(name, props));
            let mut cache = self.modded_baked.write();
            if cache.0 != generation {
                *cache = (generation, HashMap::new());
            }
            cache.1.insert(state_id, built.clone());
            return built;
        }
        if let Some(hit) = self.baked.read().get(&state_id) {
            return hit.clone();
        }
        let built = Arc::new(self.build(name, props));
        self.baked.write().insert(state_id, built.clone());
        built
    }

    pub fn atlas_rows(&self) -> u32 {
        self.atlas_rows
    }

    fn build(&self, name: &str, props: &HashMap<&str, &str>) -> BakedBlock {
        let Some(def) = self.defs.get(name) else {
            self.missing_defs.fetch_add(1, Ordering::Relaxed);
            return BakedBlock::default();
        };
        let mut out = BakedBlock::default();
        for slot in def.select(props) {
            let part = bake::bake_slot(slot, |r| self.model(r));
            out.is_solid |= part.choices.iter().all(|m| m.occludes);
            out.full_cube |= part.choices.iter().all(|m| m.full_cube);
            out.ambient_occlusion &= part.choices.iter().all(|m| m.ambient_occlusion);
            out.randomized |= part.choices.len() > 1;
            out.parts.push(part);
        }
        out
    }

    fn model(&self, reference: &ModelRef) -> Arc<BakedModel> {
        let key = (reference.model.clone(), reference.rot);
        if let Some(hit) = self.models.read().get(&key) {
            return hit.clone();
        }
        let built = Arc::new(bake::bake_model(
            crate::items::model::shared(),
            &reference.model,
            reference.rot,
            self.atlas_rows,
        ));
        if built.missing_textures > 0 {
            self.missing_textures
                .fetch_add(built.missing_textures, Ordering::Relaxed);
        }
        self.models.write().insert(key, built.clone());
        built
    }

    #[cfg(test)]
    pub fn definition_count(&self) -> usize {
        self.defs.len()
    }

    #[cfg(test)]
    pub fn definition_for_test(&self, name: &str) -> Option<&StateDef> {
        self.defs.get(name)
    }

    #[cfg(test)]
    pub fn misses(&self) -> (u32, u32) {
        (
            self.missing_defs.load(Ordering::Relaxed),
            self.missing_textures.load(Ordering::Relaxed),
        )
    }
}
