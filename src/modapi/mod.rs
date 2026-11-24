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
    /// Block id to the plugin that tints it.
    tinted: HashMap<String, usize>,
    /// Entity type id to the plugin that draws it.
    entities: HashMap<String, usize>,
    /// Mod packet channel to the plugin that reads it.
    channels: HashMap<String, usize>,
    /// Tints asked so far, by modded table generation and state id.
    tints: HashMap<(u32, u16), torch_mod_api::Tint>,
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
                for block in &registration.tinted_blocks {
                    out.tinted.entry(block.clone()).or_insert(index);
                }
                for kind in &registration.entities {
                    out.entities.entry(kind.clone()).or_insert(index);
                }
                for channel in &registration.channels {
                    out.channels.entry(channel.clone()).or_insert(index);
                }
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

/// Entity rigs a payload asked to redraw, taken by `render` each frame.
static REDRAW_ENTITIES: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// A mod packet from the server: to the plugin reading its channel, if
/// one does; the rigs it names are asked for again.
pub fn on_payload(channel: &str, data: &[u8]) {
    let redraw = {
        let mut plugins = PLUGINS.lock();
        let Some(&index) = plugins.channels.get(channel) else {
            return;
        };
        let Some(loaded) = plugins.loaded.get_mut(index) else {
            return;
        };
        match loaded.runtime.payload(channel, data) {
            Ok(redraw) => redraw,
            Err(e) => {
                loaded.failures += 1;
                if loaded.failures <= 3 {
                    crate::log_warn!("modapi", "{}: payload on {channel}: {e}", loaded.name);
                }
                return;
            }
        }
    };
    store::redraw(&redraw.block_entities);
    REDRAW_ENTITIES.lock().extend(redraw.entities);
}

/// Entity ids whose rigs a payload changed since the last call.
pub(crate) fn take_entity_redraws() -> Vec<i32> {
    std::mem::take(&mut *REDRAW_ENTITIES.lock())
}

/// Whether a plugin draws entities of type `kind` (`mymod:biplane`).
pub fn draws_entity(kind: &str) -> bool {
    PLUGINS.lock().entities.contains_key(kind)
}

/// The rig for an entity, from the plugin that registered its type; `None`
/// when none did.
fn entity(view: &torch_mod_api::EntityView) -> Option<Result<torch_mod_api::Rig, String>> {
    let mut plugins = PLUGINS.lock();
    let index = *plugins.entities.get(&view.kind)?;
    let input = match serde_json::to_vec(view) {
        Ok(bytes) => bytes,
        Err(e) => return Some(Err(e.to_string())),
    };
    let loaded = plugins.loaded.get_mut(index)?;
    Some(
        loaded
            .runtime
            .entity(&input)
            .map_err(|e| format!("{}: {e}", loaded.name)),
    )
}

/// The tint a plugin gives a modded block state (`Plugin::block_tint`), or
/// `None` when no plugin tints its block. Asked once per state and kept.
pub fn block_tint(state: u16, block: &str, props: &[(&str, &str)]) -> Option<torch_mod_api::Tint> {
    let mut plugins = PLUGINS.lock();
    let index = *plugins.tinted.get(block)?;
    let key = (crate::blocks::modded::generation(), state);
    if let Some(tint) = plugins.tints.get(&key) {
        return Some(*tint);
    }
    let view = torch_mod_api::BlockStateView {
        block: block.to_owned(),
        props: props
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    };
    let input = serde_json::to_vec(&view).ok()?;
    let loaded = plugins.loaded.get_mut(index)?;
    let tint = match loaded.runtime.block_tint(&input) {
        Ok(tint) => tint,
        Err(e) => {
            crate::log_warn!("modapi", "{}: tint of {block}: {e}", loaded.name);
            torch_mod_api::Tint::None
        }
    };
    plugins.tints.insert(key, tint);
    Some(tint)
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
        match &rig.parts[0].animation {
            torch_mod_api::Animation::Spin {
                axis,
                degrees_per_tick,
                ..
            } => {
                assert_eq!(*axis, [0.0, 0.0, 1.0]);
                assert_eq!(*degrees_per_tick, 18.0);
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
    fn the_example_plugin_tints_its_leaves() {
        let mut runtime = runtime::Runtime::load("example", EXAMPLE).unwrap();
        assert_eq!(runtime.registration.tinted_blocks, ["example:leaves"]);
        let mut tint = |props: serde_json::Value| {
            let view = serde_json::json!({"block": "example:leaves", "props": props});
            runtime.block_tint(view.to_string().as_bytes()).unwrap()
        };
        assert_eq!(tint(serde_json::json!({})), torch_mod_api::Tint::Foliage);
        assert_eq!(
            tint(serde_json::json!({"snowy": "true"})),
            torch_mod_api::Tint::Color(0xFF_FFFF)
        );
    }

    #[test]
    fn the_example_plugin_draws_its_drone_entity() {
        let mut runtime = runtime::Runtime::load("example", EXAMPLE).unwrap();
        assert_eq!(runtime.registration.entities, ["example:drone"]);
        let view =
            serde_json::json!({"type": "example:drone", "id": 7, "width": 1.0, "height": 0.5});
        let rig = runtime.entity(view.to_string().as_bytes()).unwrap();
        assert_eq!(rig.parts.len(), 1);
        let torch_mod_api::Geometry::Cuboid { from, to, .. } = &rig.parts[0].geometry else {
            panic!("{:?}", rig.parts[0].geometry);
        };
        // Its type's size, in sixteenths about its feet.
        assert_eq!((*from, *to), ([-8.0, 0.0, -8.0], [8.0, 8.0, 8.0]));
    }

    #[test]
    fn a_payload_reaches_the_plugin_and_names_what_to_redraw() {
        let mut runtime = runtime::Runtime::load("example", EXAMPLE).unwrap();
        assert_eq!(runtime.registration.channels, ["example:paint"]);
        // Entity 7 painted red: VarInt 7, then the int 0xFFFF0000.
        let mut data = vec![7u8];
        data.extend(0xFFFF_0000u32.to_be_bytes());
        let redraw = runtime.payload("example:paint", &data).unwrap();
        assert_eq!(redraw.entities, [7]);
        // The plugin kept it: the drone's rig is red now.
        let view =
            serde_json::json!({"type": "example:drone", "id": 7, "width": 1.0, "height": 0.5});
        let rig = runtime.entity(view.to_string().as_bytes()).unwrap();
        let torch_mod_api::Geometry::Cuboid { tint, .. } = &rig.parts[0].geometry else {
            panic!("{:?}", rig.parts[0].geometry);
        };
        assert_eq!(*tint, 0xFFFF_0000);
    }

    #[test]
    fn a_module_without_the_exports_is_refused() {
        assert!(runtime::Runtime::load("junk", b"\0asm\x01\0\0\0").is_err());
    }
}
