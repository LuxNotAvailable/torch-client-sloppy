//! The Torch mod API, version 1 (`docs/MOD_API.md`): what a ported mod's
//! client code is written against. A port implements [`Plugin`] and calls
//! [`export_plugin!`]; built for `wasm32-unknown-unknown`, it becomes the
//! `plugin.wasm` in its mod's `.torchmod` package.
//!
//! A plugin isn't called per frame. It describes what to draw (a [`Rig`]:
//! parts with transforms and animations), and the client plays the
//! animations natively until something changes.
//!
//! The client uses these same types to read what a plugin returns.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use serde_json::Value;

/// The API version this SDK builds plugins for.
pub const API_VERSION: i32 = 1;

/// What a plugin handles, returned once from `init`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Registration {
    /// Block entity type ids (`create:simple_kinetic`) whose visuals this
    /// plugin draws.
    #[serde(default)]
    pub block_entities: Vec<String>,
    /// Of those, the types whose block's own model the plugin draws instead
    /// (a shaft that spins): the client leaves the block's static model out.
    #[serde(default)]
    pub replaces_block_model: Vec<String>,
    /// Sprites the plugin draws that no block model uses (a fluid's
    /// `create:fluid/honey_still`), so the client puts them in its atlas.
    #[serde(default)]
    pub sprites: Vec<String>,
    /// Block ids (`mymod:maple_leaves`) whose tinted faces this plugin
    /// colours ([`Plugin::block_tint`]).
    #[serde(default)]
    pub tinted_blocks: Vec<String>,
    /// Entity type ids (`mymod:biplane`) this plugin draws
    /// ([`Plugin::entity`]).
    #[serde(default)]
    pub entities: Vec<String>,
    /// The mod's own packet channels (`mymod:sync_state`) this plugin
    /// reads ([`Plugin::payload`]).
    #[serde(default)]
    pub channels: Vec<String>,
}

/// What a payload changed: the rigs to ask for again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Redraw {
    /// Entity ids whose [`Plugin::entity`] rigs change.
    #[serde(default)]
    pub entities: Vec<i32>,
    /// Block entity positions whose [`Plugin::block_entity`] rigs change.
    #[serde(default)]
    pub block_entities: Vec<[i32; 3]>,
}

/// An entity as the plugin sees it, when it appears.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityView {
    /// Its type id (`mymod:biplane`).
    #[serde(rename = "type")]
    pub kind: String,
    /// The server's entity id (stable while it's in view).
    pub id: i32,
    /// The type's size, as the mod declares it (`EntityDimensions`).
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
    /// The game time, in ticks, when this view was made.
    #[serde(default)]
    pub time: i64,
    /// Its synced entity data the client could read, by data index
    /// (`{"8": 3, "9": [0.0, 1.5, 0.0]}`): numbers, booleans, strings,
    /// positions, vectors, UUIDs (hex), poses and NBT. The client asks for
    /// the rig again when it changes. Read the mod's `EntityDataAccessor`
    /// ids for the indices.
    #[serde(default)]
    pub data: Value,
}

/// A block state, for hooks that depend on the state alone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockStateView {
    /// Its block's id (`mymod:maple_leaves`).
    pub block: String,
    #[serde(default)]
    pub props: BTreeMap<String, String>,
}

impl BlockStateView {
    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props.get(name).map(String::as_str)
    }
}

/// Where a block's tinted faces (model faces with a `tintindex`) take their
/// colour: Java's `BlockColor`. The biome sources blend by position as
/// vanilla's do; a fixed colour is the same everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tint {
    /// No tint: the texture as it is.
    #[default]
    None,
    /// The biome's grass colour (grass blocks, ferns).
    Grass,
    /// The biome's foliage colour (most leaves, vines).
    Foliage,
    /// The biome's dry foliage colour (leaf litter).
    DryFoliage,
    /// The biome's water colour.
    Water,
    /// A fixed colour, `0xRRGGBB` (birch and spruce leaves use these).
    Color(u32),
}

/// A block entity as the plugin sees it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockEntityView {
    /// Its type id (`create:simple_kinetic`).
    #[serde(rename = "type")]
    pub kind: String,
    pub pos: [i32; 3],
    /// Its block's id (`create:shaft`).
    pub block: String,
    /// Its block state's properties.
    #[serde(default)]
    pub props: BTreeMap<String, String>,
    /// Its data as the server sent it: NBT as JSON (compounds are objects,
    /// lists and arrays are arrays, every number is a number).
    #[serde(default)]
    pub nbt: Value,
    /// The game time, in ticks, when this view was made.
    #[serde(default)]
    pub time: i64,
    /// The game time its NBT arrived. A view can be made again later with
    /// the same NBT (a neighbour changed): time-based state in the NBT (an
    /// angle, an item's position) is as of this tick, not `time`.
    #[serde(default)]
    pub nbt_time: i64,
}

impl BlockEntityView {
    /// A property of its block state.
    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props.get(name).map(String::as_str)
    }

    /// A number at a path through the NBT (`["Network", "Speed"]`).
    pub fn nbt_f64(&self, path: &[&str]) -> Option<f64> {
        let mut at = &self.nbt;
        for key in path {
            at = at.get(key)?;
        }
        at.as_f64()
    }
}

/// What to draw for one block entity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rig {
    #[serde(default)]
    pub parts: Vec<Part>,
}

/// One drawn piece: geometry, placed by a static transform, then moved by
/// an animation. Units are sixteenths of a block from the block's corner;
/// angles are degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub geometry: Geometry,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub animation: Animation,
    /// A texture moving across the part's quads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll: Option<Scroll>,
}

/// Java's `SpriteShiftEntry` with `shiftUVScrolling`: the part's quads drawn
/// with sprite `from` draw from sprite `to` instead, the same texel offset
/// into it, moved down `to` by `offset` of its height:
/// `offset = fract(per_tick * ticks + phase) * span`. `to` is usually
/// taller than `from` (a strip of it repeated), so the moved rect stays
/// inside it; past its end the host wraps to its top. Declare `to` in
/// [`Registration::sprites`]: no model names it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scroll {
    pub from: String,
    pub to: String,
    /// Cycles a tick; negative runs up.
    pub per_tick: f32,
    /// Cycles at tick 0.
    #[serde(default)]
    pub phase: f32,
    /// The share of `to`'s height one cycle moves through.
    pub span: f32,
}

impl Part {
    /// A model with no transform or animation.
    pub fn model(model: impl Into<String>) -> Part {
        Part {
            geometry: Geometry::Model {
                model: model.into(),
                x: 0,
                y: 0,
                uvlock: false,
            },
            transform: Transform::default(),
            animation: Animation::None,
            scroll: None,
        }
    }

    pub fn with_transform(mut self, transform: Transform) -> Part {
        self.transform = transform;
        self
    }

    pub fn with_animation(mut self, animation: Animation) -> Part {
        self.animation = animation;
        self
    }

    /// An item (`minecraft:iron_ingot`, `create:cogwheel`) with no
    /// transform or animation.
    pub fn item(item: impl Into<String>) -> Part {
        Part {
            geometry: Geometry::Item {
                item: item.into(),
                flat: None,
            },
            transform: Transform::default(),
            animation: Animation::None,
            scroll: None,
        }
    }

    pub fn with_scroll(mut self, scroll: Scroll) -> Part {
        self.scroll = Some(scroll);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Geometry {
    /// A block model by id (`create:block/shaft`), turned like a blockstate
    /// variant: `x` and `y` in quarter turns.
    Model {
        model: String,
        #[serde(default)]
        x: u8,
        #[serde(default)]
        y: u8,
        #[serde(default)]
        uvlock: bool,
    },
    /// A whole block state's model.
    Block {
        block: String,
        #[serde(default)]
        props: BTreeMap<String, String>,
    },
    /// A box with one sprite on the chosen faces.
    Cuboid {
        from: [f32; 3],
        to: [f32; 3],
        /// A sprite id (`minecraft:block/water_still`).
        texture: String,
        /// Faces drawn, as bits by direction: down 1, up 2, north 4,
        /// south 8, west 16, east 32.
        #[serde(default = "all_faces")]
        faces: u8,
        /// ARGB multiplied into the sprite.
        #[serde(default = "white")]
        tint: u32,
        /// The part of the sprite each face shows, in sixteenths; the
        /// whole sprite when left out.
        #[serde(default)]
        uv: Option<[f32; 4]>,
    },
    /// Text in the game's font, as a sign draws it: in the part's XY
    /// plane, readable from `+Z`, each line centred on the origin and the
    /// lines centred as a block, one font pixel a sixteenth (a line is 10
    /// high). Scale it with the part's transform.
    Text {
        /// Lines, with `§` formatting codes (colours, bold, ...).
        lines: Vec<String>,
        /// ARGB for text with no colour code.
        #[serde(default = "black")]
        color: u32,
        /// A drop shadow, as the HUD draws text.
        #[serde(default)]
        shadow: bool,
        /// Full bright, as glowing sign text.
        #[serde(default)]
        glowing: bool,
    },
    /// An item's model as `ItemDisplayContext.FIXED` draws it (item frames,
    /// Create's belts): its `fixed` display transform, centred on the
    /// part's origin, the block's corner before the part's transform (so
    /// use a pivot of 0). Lit like the rest of the rig.
    Item {
        /// Its id (`minecraft:iron_ingot`).
        item: String,
        /// Used in place of the part's transform when the item's model is
        /// a flat sprite (`item/generated`), where Java checks
        /// `BakedModel.isGui3d()`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flat: Option<Transform>,
    },
}

fn all_faces() -> u8 {
    63
}

fn white() -> u32 {
    0xFFFF_FFFF
}

fn black() -> u32 {
    0xFF00_0000
}

/// Scale about `pivot`, rotate about `pivot` (X, then Y, then Z), then
/// translate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    #[serde(default = "centre")]
    pub pivot: [f32; 3],
    #[serde(default)]
    pub rotate: [f32; 3],
    #[serde(default)]
    pub translate: [f32; 3],
    #[serde(default = "one")]
    pub scale: [f32; 3],
}

fn centre() -> [f32; 3] {
    [8.0; 3]
}

fn one() -> [f32; 3] {
    [1.0; 3]
}

impl Default for Transform {
    fn default() -> Transform {
        Transform {
            pivot: centre(),
            rotate: [0.0; 3],
            translate: [0.0; 3],
            scale: one(),
        }
    }
}

/// Motion the client plays every frame, from the game time in ticks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Animation {
    #[default]
    None,
    /// angle = phase + degrees_per_tick * ticks, about `axis` through `pivot`.
    Spin {
        axis: [f32; 3],
        #[serde(default = "centre")]
        pivot: [f32; 3],
        degrees_per_tick: f32,
        #[serde(default)]
        phase: f32,
    },
    /// angle = amplitude * sin(2 pi * ticks / period_ticks + phase).
    Swing {
        axis: [f32; 3],
        #[serde(default = "centre")]
        pivot: [f32; 3],
        amplitude: f32,
        period_ticks: f32,
        #[serde(default)]
        phase: f32,
    },
    /// Moves to `offset` and back, a cosine ping-pong over `period_ticks`.
    Slide {
        offset: [f32; 3],
        period_ticks: f32,
        #[serde(default)]
        phase: f32,
    },
    /// Moves in straight legs: `points` are `[tick, x, y, z]` (game ticks,
    /// an offset in sixteenths) in time order. Before the first it sits at
    /// the first, after the last at the last (an item stopping at a
    /// belt's end).
    Path { points: Vec<[f32; 4]> },
}

/// What a port implements.
pub trait Plugin {
    /// What this plugin handles. Called once, before anything else.
    fn init(&mut self) -> Registration;

    /// The visuals of a block entity of a type it registered.
    fn block_entity(&mut self, view: &BlockEntityView) -> Rig;

    /// The visuals of an entity of a type it registered in `entities`.
    /// Parts are in the entity's own frame: the origin at its feet, `+Z`
    /// the way it faces, sixteenths of a block; the client places the rig
    /// at the entity and turns it by its yaw every frame.
    fn entity(&mut self, view: &EntityView) -> Rig {
        let _ = view;
        Rig::default()
    }

    /// A packet on one of its `channels`, as the mod's server side wrote it
    /// (read it with [`buf::Reader`]). Keep what it says; the client asks
    /// again for the rigs it names.
    fn payload(&mut self, channel: &str, data: &[u8]) -> Redraw {
        let _ = (channel, data);
        Redraw::default()
    }

    /// The tint of a block it registered in `tinted_blocks`. Asked once per
    /// block state and kept, so it can't depend on position or time.
    fn block_tint(&mut self, block: &BlockStateView) -> Tint {
        let _ = block;
        Tint::None
    }
}

/// A block in the world, as `block_at` reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockAt {
    pub block: String,
    #[serde(default)]
    pub props: BTreeMap<String, String>,
}

pub mod buf;

/// Calls into the client.
pub mod host {
    use super::BlockAt;

    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "torch")]
    unsafe extern "C" {
        fn log(ptr: *const u8, len: usize);
        fn block_at(x: i32, y: i32, z: i32, out: *mut u8, cap: usize) -> usize;
    }

    /// A line in the client's log, tagged with the mod.
    pub fn log_line(text: &str) {
        #[cfg(target_arch = "wasm32")]
        // SAFETY: the host reads `len` bytes from `ptr`, which `text` owns.
        unsafe {
            log(text.as_ptr(), text.len())
        };
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("{text}");
    }

    /// The block at a world position (for connection logic). `None` off the
    /// wasm host, or where the world isn't loaded.
    pub fn block(x: i32, y: i32, z: i32) -> Option<BlockAt> {
        #[cfg(target_arch = "wasm32")]
        {
            let mut buf = vec![0u8; 256];
            // SAFETY: the host writes at most `cap` bytes into `buf`.
            let mut len = unsafe { block_at(x, y, z, buf.as_mut_ptr(), buf.len()) };
            if len > buf.len() {
                buf.resize(len, 0);
                len = unsafe { block_at(x, y, z, buf.as_mut_ptr(), buf.len()) };
            }
            if len == 0 || len > buf.len() {
                return None;
            }
            serde_json::from_slice(&buf[..len]).ok()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (x, y, z);
            None
        }
    }
}

/// The exports the host calls. Used by [`export_plugin!`]; not for ports.
#[doc(hidden)]
pub mod abi {
    /// A result handed to the host: its buffer leaked, packed as
    /// `ptr << 32 | len`. The host frees it with `torch_free`.
    pub fn pack(bytes: Vec<u8>) -> i64 {
        let mut bytes = bytes.into_boxed_slice();
        let (ptr, len) = (bytes.as_mut_ptr() as usize as i64, bytes.len() as i64);
        std::mem::forget(bytes);
        (ptr << 32) | len
    }

    pub fn alloc(len: i32) -> i32 {
        let mut bytes = vec![0u8; len.max(0) as usize].into_boxed_slice();
        let ptr = bytes.as_mut_ptr() as usize as i32;
        std::mem::forget(bytes);
        ptr
    }

    /// # Safety
    /// `ptr` and `len` came from [`alloc`] or [`pack`], once.
    pub unsafe fn free(ptr: i32, len: i32) {
        if len <= 0 {
            return;
        }
        let slice = std::ptr::slice_from_raw_parts_mut(ptr as usize as *mut u8, len as usize);
        // SAFETY: the caller hands back a buffer this module leaked.
        drop(unsafe { Box::from_raw(slice) });
    }

    /// A payload as the host passes it: the channel's length (two bytes,
    /// big-endian), the channel, then the data.
    pub fn split_payload(input: &[u8]) -> Option<(&str, &[u8])> {
        let len = u16::from_be_bytes(input.get(..2)?.try_into().ok()?) as usize;
        let channel = std::str::from_utf8(input.get(2..2 + len)?).ok()?;
        Some((channel, input.get(2 + len..)?))
    }

    /// # Safety
    /// `ptr` and `len` name a live buffer from [`alloc`].
    pub unsafe fn input<'a>(ptr: i32, len: i32) -> &'a [u8] {
        // SAFETY: as the caller promises.
        unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len.max(0) as usize) }
    }
}

/// Exports a [`Plugin`] for the host: `export_plugin!(MyPlugin::default())`.
#[macro_export]
macro_rules! export_plugin {
    ($make:expr) => {
        thread_local! {
            static PLUGIN: ::std::cell::RefCell<Box<dyn $crate::Plugin>> =
                ::std::cell::RefCell::new(Box::new($make));
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_api() -> i32 {
            $crate::API_VERSION
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_alloc(len: i32) -> i32 {
            $crate::abi::alloc(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_free(ptr: i32, len: i32) {
            // SAFETY: the host frees only what this module handed out.
            unsafe { $crate::abi::free(ptr, len) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_init() -> i64 {
            let registration = PLUGIN.with(|p| p.borrow_mut().init());
            $crate::abi::pack(::serde_json::to_vec(&registration).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_block_entity(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let rig = match ::serde_json::from_slice::<$crate::BlockEntityView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().block_entity(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable block entity view: {e}"));
                    $crate::Rig::default()
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&rig).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_entity(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let rig = match ::serde_json::from_slice::<$crate::EntityView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().entity(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable entity view: {e}"));
                    $crate::Rig::default()
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&rig).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_payload(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the payload into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let redraw = match $crate::abi::split_payload(input) {
                Some((channel, data)) => PLUGIN.with(|p| p.borrow_mut().payload(channel, data)),
                None => $crate::Redraw::default(),
            };
            $crate::abi::pack(::serde_json::to_vec(&redraw).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_block_tint(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let tint = match ::serde_json::from_slice::<$crate::BlockStateView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().block_tint(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable block state view: {e}"));
                    $crate::Tint::None
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&tint).unwrap_or_default())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rig_reads_as_the_spec_writes_it() {
        let rig: Rig = serde_json::from_str(
            r#"{"parts": [{"geometry": {"model": {"model": "create:block/shaft"}},
                           "transform": {"pivot": [8, 8, 8], "rotate": [0, 0, 0]},
                           "animation": {"spin": {"axis": [0, 1, 0], "pivot": [8, 8, 8],
                                                  "degrees_per_tick": 9.6, "phase": 0}}}]}"#,
        )
        .unwrap();
        let part = &rig.parts[0];
        assert_eq!(
            part.geometry,
            Geometry::Model {
                model: "create:block/shaft".into(),
                x: 0,
                y: 0,
                uvlock: false
            }
        );
        assert!(
            matches!(part.animation, Animation::Spin { degrees_per_tick, .. } if degrees_per_tick == 9.6)
        );
        assert_eq!(part.transform.scale, [1.0; 3]);
        let none: Part =
            serde_json::from_str(r#"{"geometry": {"block": {"block": "minecraft:stone"}}}"#)
                .unwrap();
        assert_eq!(none.animation, Animation::None);
    }

    #[test]
    fn a_view_reads_numbers_through_its_nbt() {
        let view: BlockEntityView = serde_json::from_str(
            r#"{"type": "create:simple_kinetic", "pos": [1, 2, 3], "block": "create:shaft",
                "props": {"axis": "y"}, "nbt": {"Speed": 32.0, "Network": {"Id": 12}}, "time": 5}"#,
        )
        .unwrap();
        assert_eq!(view.prop("axis"), Some("y"));
        assert_eq!(view.nbt_f64(&["Speed"]), Some(32.0));
        assert_eq!(view.nbt_f64(&["Network", "Id"]), Some(12.0));
        assert_eq!(view.nbt_f64(&["Missing"]), None);
    }
}
