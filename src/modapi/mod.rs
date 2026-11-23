//! The mod API's host (`docs/MOD_API.md`): runs the `plugin.wasm` a
//! converted mod's package carries (its ported client code) and draws what
//! it describes. Plugins answer when something changes (`store::tick`);
//! the client plays their animations every frame (`render`).
//!
//! Version 1 handles block entity visuals. The guest side, and the types
//! both sides share, are `crates/torch-mod-api`.

mod render;
mod runtime;
pub mod store;

use std::collections::HashMap;

use parking_lot::Mutex;

pub use render::plugin;

struct Loaded {
    name: String,
    runtime: runtime::Runtime,
    /// Calls that failed, logged up to a few.
    failures: u32,
}

#[derive(Default)]
struct Plugins {
    loaded: Vec<Loaded>,
    /// Block entity type id to the plugin that draws it.
    block_entities: HashMap<String, usize>,
    /// Types whose block model a plugin draws instead.
    replaces_block_model: std::collections::HashSet<String>,
    /// Sprites plugins draw that block models don't use.
    sprites: Vec<String>,
}

/// Whether a plugin draws `block_entity`'s block in place of its model.
pub fn replaces_block_model(block_entity: &str) -> bool {
    PLUGINS.lock().replaces_block_model.contains(block_entity)
}

/// Sprites plugins declared, for the block atlas.
pub fn declared_sprites() -> Vec<String> {
    PLUGINS.lock().sprites.clone()
}

static PLUGINS: std::sync::LazyLock<Mutex<Plugins>> = std::sync::LazyLock::new(Default::default);

/// Loads packages' plugins, as `(package, plugin.wasm)`. A plugin that
/// doesn't load is logged and left out; the first to register a block
/// entity type draws it.
pub fn load(plugins: Vec<(String, Vec<u8>)>) {
    let mut out = Plugins::default();
    for (name, wasm) in plugins {
        match runtime::Runtime::load(&name, &wasm) {
            Ok(runtime) => {
                let index = out.loaded.len();
                let registration = &runtime.registration;
                for kind in &registration.block_entities {
                    out.block_entities.entry(kind.clone()).or_insert(index);
                }
                out.replaces_block_model
                    .extend(registration.replaces_block_model.iter().cloned());
                out.sprites.extend(registration.sprites.iter().cloned());
                crate::log_info!(
                    "modapi",
                    "{name}: plugin loaded, draws {} block entity type(s)",
                    runtime.registration.block_entities.len()
                );
                out.loaded.push(Loaded {
                    name,
                    runtime,
                    failures: 0,
                });
            }
            Err(e) => crate::log_warn!("modapi", "{name}: plugin not loaded: {e}"),
        }
    }
    *PLUGINS.lock() = out;
}

/// The rig for a block entity, from the plugin that registered its type;
/// `None` when none did.
fn block_entity(
    view: &torch_mod_api::BlockEntityView,
    near: runtime::Near,
) -> Option<Result<torch_mod_api::Rig, String>> {
    let mut plugins = PLUGINS.lock();
    let index = *plugins.block_entities.get(&view.kind)?;
    let loaded = plugins.loaded.get_mut(index)?;
    let input = match serde_json::to_vec(view) {
        Ok(bytes) => bytes,
        Err(e) => return Some(Err(e.to_string())),
    };
    match loaded.runtime.block_entity(&input, near) {
        Ok(rig) => Some(Ok(rig)),
        Err(e) => {
            loaded.failures += 1;
            if loaded.failures > 3 {
                // Logged enough: the rest draw nothing, quietly.
                return Some(Ok(torch_mod_api::Rig::default()));
            }
            Some(Err(format!("{}: {e}", loaded.name)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template port (`ports/example`), built for wasm32.
    const EXAMPLE: &[u8] = include_bytes!("testdata/example.wasm");

    #[test]
    fn the_example_plugin_registers_and_spins_its_block() {
        let mut runtime = runtime::Runtime::load("example", EXAMPLE).unwrap();
        assert_eq!(runtime.registration.block_entities, ["example:spinner"]);
        let view = serde_json::json!({"type": "example:spinner", "pos": [0, 64, 0],
            "block": "example:wheel", "props": {"axis": "z"}, "nbt": {"Speed": 60.0}});
        let rig = runtime
            .block_entity(view.to_string().as_bytes(), runtime::Near::new())
            .unwrap();
        assert_eq!(rig.parts.len(), 1);
        assert_eq!(
            rig.parts[0].geometry,
            torch_mod_api::Geometry::Model {
                model: "example:block/wheel".into(),
                x: 0,
                y: 0,
                uvlock: false
            }
        );
        match rig.parts[0].animation {
            torch_mod_api::Animation::Spin {
                axis,
                degrees_per_tick,
                ..
            } => {
                assert_eq!(axis, [0.0, 0.0, 1.0]);
                assert_eq!(degrees_per_tick, 18.0);
            }
            other => panic!("{other:?}"),
        }
        // A broken view doesn't trap: the plugin answers an empty rig.
        let rig = runtime
            .block_entity(b"{not json", runtime::Near::new())
            .unwrap();
        assert!(rig.parts.is_empty());
    }

    #[test]
    fn a_module_without_the_exports_is_refused() {
        assert!(runtime::Runtime::load("junk", b"\0asm\x01\0\0\0").is_err());
    }
}
