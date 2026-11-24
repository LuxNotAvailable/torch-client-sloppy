//! The mod API's host (`docs/MOD_API.md`): runs the `plugin.wasm` a
//! converted mod's package carries (its ported client code) and draws what
//! it describes. Plugins answer when something changes (`store::tick`);
//! the client plays their animations every frame (`render`).
//!
//! Version 1 handles block entity visuals. The guest side, and the types
//! both sides share, are `crates/torch-mod-api`.

mod render;
mod runtime;
pub mod screens;
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
    /// Menu type id to the plugin that draws its screen.
    screens: HashMap<String, usize>,
    /// Item id to the plugin that tints it.
    tinted_items: HashMap<String, usize>,
    /// Item tints asked so far.
    item_tints: HashMap<String, Vec<u32>>,
    /// Fluid id to the plugin that gives its look.
    fluids: HashMap<String, usize>,
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
                for menu in &registration.screens {
                    out.screens.entry(menu.clone()).or_insert(index);
                }
                for item in &registration.tinted_items {
                    out.tinted_items.entry(item.clone()).or_insert(index);
                }
                for fluid in &registration.fluids {
                    out.fluids.entry(fluid.clone()).or_insert(index);
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

/// Modded entities' synced data the client could read, by entity id, then
/// data index (`"8"`): what `EntityView::data` carries.
static ENTITY_DATA: std::sync::LazyLock<
    Mutex<HashMap<i32, serde_json::Map<String, serde_json::Value>>>,
> = std::sync::LazyLock::new(Default::default);

/// A modded entity's data values arrived (the protocol hop reads the plain
/// kinds): kept, and its rig asked for again.
pub fn on_entity_data(id: i32, values: Vec<(u8, serde_json::Value)>) {
    {
        let mut data = ENTITY_DATA.lock();
        let entry = data.entry(id).or_default();
        for (index, value) in values {
            entry.insert(index.to_string(), value);
        }
    }
    REDRAW_ENTITIES.lock().push(id);
}

/// What the client knows of a modded entity's data.
pub(crate) fn entity_data(id: i32) -> serde_json::Value {
    ENTITY_DATA
        .lock()
        .get(&id)
        .map_or(serde_json::Value::Null, |m| {
            serde_json::Value::Object(m.clone())
        })
}

/// Forgets an entity's data (it left).
pub(crate) fn forget_entity(id: i32) {
    ENTITY_DATA.lock().remove(&id);
}

/// Forgets the data of entities the client no longer tracks.
pub(crate) fn keep_entity_data(tracked: impl Fn(i32) -> bool) {
    ENTITY_DATA.lock().retain(|id, _| tracked(*id));
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

/// A modded menu's screen, from the plugin that registered its menu type;
/// `None` when none did or it left it to the client.
fn screen(view: &torch_mod_api::ScreenView) -> Option<torch_mod_api::Screen> {
    let mut plugins = PLUGINS.lock();
    let index = *plugins.screens.get(&view.menu)?;
    let input = serde_json::to_vec(view).ok()?;
    let loaded = plugins.loaded.get_mut(index)?;
    match loaded.runtime.screen(&input) {
        Ok(screen) => screen,
        Err(e) => {
            loaded.failures += 1;
            if loaded.failures <= 3 {
                crate::log_warn!("modapi", "{}: screen of {}: {e}", loaded.name, view.menu);
            }
            None
        }
    }
}

/// A fluid's look from the plugin that registered it (`Plugin::fluid`);
/// `None` when none did or it left it to the client. Callers keep it.
pub fn fluid(fluid: &str) -> Option<torch_mod_api::FluidLook> {
    let mut plugins = PLUGINS.lock();
    let index = *plugins.fluids.get(fluid)?;
    let view = torch_mod_api::FluidView {
        fluid: fluid.to_owned(),
    };
    let input = serde_json::to_vec(&view).ok()?;
    let loaded = plugins.loaded.get_mut(index)?;
    loaded.runtime.fluid(&input).unwrap_or_else(|e| {
        crate::log_warn!("modapi", "{}: look of {fluid}: {e}", loaded.name);
        None
    })
}

/// The colours a plugin gives an item's tint indices (`Plugin::item_tints`),
/// or `None` when no plugin tints it. Asked once per item and kept.
pub fn item_tints(item: &str) -> Option<Vec<u32>> {
    let mut plugins = PLUGINS.lock();
    let index = *plugins.tinted_items.get(item)?;
    if let Some(tints) = plugins.item_tints.get(item) {
        return Some(tints.clone());
    }
    let view = torch_mod_api::ItemView {
        item: item.to_owned(),
    };
    let input = serde_json::to_vec(&view).ok()?;
    let loaded = plugins.loaded.get_mut(index)?;
    let tints = loaded.runtime.item_tints(&input).unwrap_or_else(|e| {
        crate::log_warn!("modapi", "{}: tints of {item}: {e}", loaded.name);
        Vec::new()
    });
    plugins.item_tints.insert(item.to_owned(), tints.clone());
    Some(tints)
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
    fn the_example_plugin_gives_its_honey_a_look() {
        let mut runtime = runtime::Runtime::load("example", EXAMPLE).unwrap();
        assert_eq!(runtime.registration.fluids, ["example:honey"]);
        let look = runtime
            .fluid(br#"{"fluid": "example:honey"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(look.still, "minecraft:block/water_still");
        assert_eq!(look.tint, 0xF0_A030);
    }

    #[test]
    fn the_example_plugin_colours_its_egg() {
        let mut runtime = runtime::Runtime::load("example", EXAMPLE).unwrap();
        assert_eq!(runtime.registration.tinted_items, ["example:drone_egg"]);
        let tints = runtime
            .item_tints(br#"{"item": "example:drone_egg"}"#)
            .unwrap();
        assert_eq!(tints, [0x3C_44AA, 0xF9_801D]);
    }

    #[test]
    fn the_example_plugin_lays_out_its_press_screen() {
        let mut runtime = runtime::Runtime::load("example", EXAMPLE).unwrap();
        assert_eq!(runtime.registration.screens, ["example:press"]);
        // Half done; opened at the press at (1, 64, -2).
        let packed: i64 = ((1i64 & 0x3FF_FFFF) << 38) | ((-2i64 & 0x3FF_FFFF) << 12) | 64;
        let view = torch_mod_api::ScreenView {
            menu: "example:press".into(),
            title: "Press".into(),
            extra: packed.to_be_bytes().to_vec(),
            data: vec![50, 100],
            slots: 38,
        };
        let input = serde_json::to_vec(&view).unwrap();
        let screen = runtime.screen(&input).unwrap().unwrap();
        assert_eq!((screen.width, screen.height), (176, 166));
        assert_eq!(screen.slots, [Some([56, 35]), Some([116, 35])]);
        // The arrow, half full.
        assert_eq!(screen.background[1].width, 12);
        assert_eq!(screen.texts[0].text, "1 64 -2");
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
