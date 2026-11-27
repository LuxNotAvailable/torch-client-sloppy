//! The Torch mod API, version 2 (`docs/MOD_API.md`): what a ported mod's
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
pub const API_VERSION: i32 = 2;

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
    /// Menu type ids (`mymod:press`) whose screens this plugin draws
    /// ([`Plugin::screen`]): Java's `MenuScreens.register`.
    #[serde(default)]
    pub screens: Vec<String>,
    /// Item ids (`mymod:custom_egg`) whose tinted layers this plugin
    /// colours ([`Plugin::item_tints`]): Java's `ItemColor`.
    #[serde(default)]
    pub tinted_items: Vec<String>,
    /// Fluid ids (`mymod:honey`, the source fluid) whose looks this plugin
    /// gives ([`Plugin::fluid`]): NeoForge's `IClientFluidTypeExtensions`.
    #[serde(default)]
    pub fluids: Vec<String>,
    /// Of `block_entities`, the types that simulate on the client every
    /// tick ([`Plugin::tick`]): Java's `tick()` running client-side.
    #[serde(default)]
    pub ticking: Vec<String>,
    /// Block entity types that move entities standing on or in them
    /// ([`Plugin::entity_motion`]): a belt, a conveyor.
    #[serde(default)]
    pub moves_entities: Vec<String>,
    /// Keys the mod binds ([`Plugin::key`]): Java's `KeyMapping`s. They
    /// show in the client's Controls, rebindable.
    #[serde(default)]
    pub keys: Vec<KeyBinding>,
}

/// A key a mod binds: Java's `KeyMapping`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// Its name, Java's (`keybind.sophisticatedbackpacks.open_backpack`):
    /// the options save it by this, the language names it.
    pub name: String,
    /// The default key, as the options file writes it (`key.keyboard.b`,
    /// `key.mouse.middle`); empty for none.
    #[serde(default)]
    pub default: String,
    /// Its Controls category, a translation key or plain text
    /// (`keygroup.sophisticatedbackpacks`).
    #[serde(default)]
    pub category: String,
}

/// A bound key pressed ([`Plugin::key`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyView {
    /// The binding's name.
    pub name: String,
    /// The menu type open (`minecraft:generic_9x3`, a mod's;
    /// `minecraft:inventory` for the player's own), or `None` in the world.
    #[serde(default)]
    pub screen: Option<String>,
    /// In a menu, the slot under the mouse (Java's `getSlotUnderMouse`).
    #[serde(default)]
    pub slot: Option<SlotView>,
}

/// A menu slot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SlotView {
    /// Its menu slot index (Java's `Slot.index`).
    pub index: u32,
    /// What it holds.
    #[serde(default)]
    pub item: Option<ItemView>,
    /// When it's one of the player's own slots, its index in the player's
    /// inventory (Java's `Slot.getSlotIndex`): hotbar 0-8, main 9-35,
    /// armor 36-39 (feet first), offhand 40.
    #[serde(default)]
    pub inventory_slot: Option<u32>,
}

/// The local player, at the start of this tick ([`host::player`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerView {
    /// The feet's position.
    pub pos: [f64; 3],
    /// Java's `getYRot` and `getXRot`, in degrees.
    pub yaw: f32,
    pub pitch: f32,
    /// The selected hotbar slot, 0-8.
    pub selected: u32,
    #[serde(default)]
    pub main_hand: Option<ItemView>,
    #[serde(default)]
    pub off_hand: Option<ItemView>,
    pub sneaking: bool,
    /// What the crosshair is on (Java's `Minecraft.hitResult`, misses as
    /// `None`).
    #[serde(default)]
    pub hit: Option<Hit>,
}

/// A crosshair target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hit {
    /// `BlockHitResult`: the block, the face hit, and the point.
    Block {
        pos: [i32; 3],
        face: mc::direction::Direction,
        at: [f64; 3],
    },
    /// `EntityHitResult`: the entity's network id, and the point.
    Entity { id: i32, at: [f64; 3] },
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
    /// The open screen changes: the client asks [`Plugin::screen`] again
    /// (state the server sent it, as a storage's settings).
    #[serde(default)]
    pub screen: bool,
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
    /// Sprites swapped on the part's quads, `from` to `to` (Java's
    /// `SpriteShiftEntry` with no motion, a `Material` retexture): each
    /// quad drawn from a `from` sprite draws the same texels of `to`.
    /// Declare a `to` no model names in [`Registration::sprites`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub retexture: BTreeMap<String, String>,
    /// An earlier part (its index in the rig) this one hangs from: placed
    /// in that part's frame, after its animation and transform, so it
    /// follows the parent's motion with its own on top (a jointed arm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
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
            retexture: BTreeMap::new(),
            parent: None,
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
            retexture: BTreeMap::new(),
            parent: None,
        }
    }

    /// A joint ([`Geometry::Joint`]) at `transform`.
    pub fn joint(transform: Transform) -> Part {
        Part::of(Geometry::Joint).with_transform(transform)
    }

    /// A part with this geometry and nothing else.
    pub fn of(geometry: Geometry) -> Part {
        Part {
            geometry,
            ..Part::model("")
        }
    }

    /// Hangs this part from rig part `parent` ([`Part::parent`]).
    pub fn with_parent(mut self, parent: usize) -> Part {
        self.parent = Some(parent);
        self
    }

    /// Swaps sprite `from` for `to` ([`Part::retexture`]).
    pub fn with_retexture(mut self, from: impl Into<String>, to: impl Into<String>) -> Part {
        self.retexture.insert(from.into(), to.into());
        self
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
        /// The sprite repeated a block at a time instead of stretched: the
        /// box cut at block edges, each piece's faces showing the sprite's
        /// texels under it, as a block model's element would (Create's
        /// `renderFluidBox` on a tall tank). `uv` is ignored.
        #[serde(default)]
        tile: bool,
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
    /// Nothing drawn: a frame for other parts to hang from
    /// ([`Part::parent`]), as a joint of a jointed arm.
    Joint,
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
    /// Turns about `axis` through `pivot` by an angle keyed in time: `keys`
    /// are `[tick, degrees]` (game ticks) in time order, straight between
    /// them, held before the first and after the last. A one-off turn (a
    /// valve handle's) is two keys; an eased one, a key a tick. With
    /// [`Part::parent`], each joint of an arm turns this way.
    Turn {
        axis: [f32; 3],
        #[serde(default = "centre")]
        pivot: [f32; 3],
        keys: Vec<[f32; 2]>,
    },
}

/// An item stack.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemView {
    /// Its id (`minecraft:stone`, `mymod:custom_egg`).
    pub item: String,
    /// Its size (1 where a hook depends on the item's type alone).
    #[serde(default)]
    pub count: u32,
}

/// The local player, as a block entity that moves entities sees it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MovingEntity {
    /// Its feet, in blocks.
    pub pos: [f64; 3],
    /// `getDeltaMovement`, blocks a tick.
    pub velocity: [f64; 3],
    /// Its bounding box's width and height, in blocks.
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub on_ground: bool,
    #[serde(default)]
    pub sneaking: bool,
    /// What the player is pressing: Java's `zza` (forward, +1) and `xxa`
    /// (left, +1).
    #[serde(default)]
    pub input: [f32; 2],
}

/// An entity standing on (or in) a block entity of a type in
/// `moves_entities`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityMotionView {
    pub block_entity: BlockEntityView,
    pub entity: MovingEntity,
}

/// What a block entity does to an entity this tick, in Java's order:
/// `move_by` (one `entity.move(MoverType.SELF, by)`, with collision), then
/// each of `steps`, then `velocity` (`setDeltaMovement`). All optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_by: Option<[f64; 3]>,
    /// More of what Java does, in order: several moves, a `setPos` nudge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<[f64; 3]>,
}

/// One thing a movement handler does to the entity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// `entity.move(MoverType.SELF, v)`: moved with collision.
    Move([f64; 3]),
    /// `entity.setPos(getX() + x, getY() + y, getZ() + z)`: moved from where
    /// the moves before left it, without collision.
    Offset([f64; 3]),
    /// `entity.setDeltaMovement(v)`.
    Velocity([f64; 3]),
    /// `entity.setOnGround(b)`.
    OnGround(bool),
    /// `entity.fallDistance = 0`.
    ResetFall,
}

/// A fluid, by its id (`mymod:honey`; its flowing form asks as the source).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FluidView {
    pub fluid: String,
}

/// How a fluid draws in the world: `getStillTexture`,
/// `getFlowingTexture` and `getTintColor`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FluidLook {
    /// Block atlas sprites (`mymod:block/honey_still`).
    pub still: String,
    pub flowing: String,
    /// `0xRRGGBB` the textures are multiplied by; white when left out.
    #[serde(default = "white_rgb")]
    pub tint: u32,
}

fn white_rgb() -> u32 {
    0xFF_FFFF
}

/// A mod's menu screen as the plugin sees it: when it opens, and again
/// whenever its container data or slot count changes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenView {
    /// The menu type id (`mymod:press`).
    pub menu: String,
    /// The screen's title, as plain text.
    #[serde(default)]
    pub title: String,
    /// The extra data NeoForge opened the menu with (what the mod's
    /// `IContainerFactory` reads; a block position, often): read it with
    /// [`buf::Reader`]. Empty for a plain open.
    #[serde(default)]
    pub extra: Vec<u8>,
    /// The menu's data slots (`ContainerData`, synced by
    /// `ClientboundContainerSetDataPacket`) by index.
    #[serde(default)]
    pub data: Vec<i32>,
    /// How many slots the server sent, the player's 36 included.
    #[serde(default)]
    pub slots: usize,
    /// The window in GUI pixels (Java's `getGuiScaledWidth`/`Height`):
    /// screens that size themselves to it (a tall storage) read it.
    #[serde(default)]
    pub window: [i32; 2],
}

/// A menu screen: Java's `AbstractContainerScreen`. Coordinates are GUI
/// pixels from the screen's top left corner (`leftPos`, `topPos`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    /// `imageWidth` and `imageHeight`; the client centres the screen.
    pub width: i32,
    pub height: i32,
    /// `renderBg`'s blits, drawn in order under the slots.
    #[serde(default)]
    pub background: Vec<Blit>,
    /// Each menu slot's `x` and `y` (the slot's top left, inside its
    /// border), by slot index; `null` hides one (an inactive slot).
    #[serde(default)]
    pub slots: Vec<Option<[i32; 2]>>,
    /// Where the player's 36 slots start among the menu's; the last 36
    /// when left out.
    #[serde(default)]
    pub player_start: Option<usize>,
    /// `titleLabelX`/`Y`; `[8, 6]` when left out.
    #[serde(default)]
    pub title_at: Option<[i32; 2]>,
    /// `inventoryLabelX`/`Y`; `[8, height - 94]` when left out.
    #[serde(default)]
    pub inventory_label_at: Option<[i32; 2]>,
    /// Hide the title or the inventory label (a `renderLabels` that skips
    /// them).
    #[serde(default)]
    pub hide_title: bool,
    #[serde(default)]
    pub hide_inventory_label: bool,
    /// `renderLabels`'s other text, over the slots.
    #[serde(default)]
    pub texts: Vec<ScreenText>,
    /// The screen's buttons (`addRenderableWidget`), over the background
    /// and under the slots ([`Plugin::screen_input`]).
    #[serde(default)]
    pub buttons: Vec<ScreenButton>,
    /// Drawn over the slots and their items, in order: slot overlays and
    /// highlights (`renderSlotOverlays`), ghost items, a tab's contents.
    #[serde(default)]
    pub overlay: Vec<Draw>,
    /// Text boxes (`EditBox`): the client edits them and reports each
    /// change and Enter ([`ScreenInput::Text`], [`ScreenInput::Submit`]).
    #[serde(default)]
    pub text_fields: Vec<TextField>,
    /// Scroll bars: the wheel over `area` or a drag of the thumb moves
    /// `value` and reports it ([`ScreenInput::Scroll`]); the plugin
    /// answers the screen scrolled (its slots moved, say).
    #[serde(default)]
    pub scrolls: Vec<ScrollBar>,
    /// Menu slots whose clicks are the plugin's, not the server's (Java
    /// handles them on the client: a filter's ghost slots): a click on one
    /// is a [`ScreenInput::SlotClick`] and moves nothing.
    #[serde(default)]
    pub plugin_slots: Vec<u32>,
}

/// Something drawn on a screen's overlay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Draw {
    Blit(Blit),
    /// `GuiGraphics.fill`: a rectangle of `color` (ARGB).
    Fill {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
    },
    /// An item as a slot draws it (`renderItem` and its count), at its
    /// top left; a count of 1 shows none.
    Item {
        item: String,
        #[serde(default = "one_item")]
        count: u32,
        x: i32,
        y: i32,
    },
}

fn one_item() -> u32 {
    1
}

/// A text box (`EditBox`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextField {
    /// Its id, the plugin's own (`search`).
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Its text. The client keeps what the player types; a different
    /// `text` in a new answer replaces it (a cleared search).
    #[serde(default)]
    pub text: String,
    /// `setMaxLength`; 32 when 0.
    #[serde(default)]
    pub max_length: usize,
    /// Grey text shown while it's empty (`setHint`).
    #[serde(default)]
    pub hint: String,
    /// Vanilla's black box with a grey border (`setBordered`); off for a
    /// field drawn on the plugin's own background.
    #[serde(default)]
    pub bordered: bool,
}

/// A scroll bar: a thumb on a track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollBar {
    /// Its id, the plugin's own (`rows`).
    pub id: String,
    /// The track, in GUI pixels from the screen's top left.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Where it is, 0 to `max` steps (rows, say).
    pub value: i32,
    pub max: i32,
    /// Where the wheel scrolls it (the panel it moves); the track when
    /// left out.
    #[serde(default)]
    pub area: Option<[i32; 4]>,
    /// The thumb's look, drawn at its place (blits at `0, 0` are its top
    /// left); vanilla's creative scroller when empty.
    #[serde(default)]
    pub thumb: Vec<Blit>,
    /// The thumb's height in pixels; 15 (the creative scroller's) when 0.
    #[serde(default)]
    pub thumb_height: i32,
}

/// A button on a screen: Java's `Button`, `ImageButton` or a widget that
/// acts as one (a tab, a toggle).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenButton {
    /// Its id, the plugin's own (`sort`, `tab_2`).
    pub id: String,
    /// Where it is, in GUI pixels from the screen's top left.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Its look: blits (screen coordinates, like `background`); with none,
    /// vanilla's button with `label` on it.
    #[serde(default)]
    pub look: Vec<Blit>,
    /// Its look under the cursor; `look` when empty.
    #[serde(default)]
    pub hover: Vec<Blit>,
    #[serde(default)]
    pub label: String,
    /// Lines shown under the cursor.
    #[serde(default)]
    pub tooltip: Vec<String>,
    /// The id the client sends as vanilla's container button click
    /// (`menu.clickMenuButton`, `ServerboundContainerButtonClickPacket`)
    /// when pressed, for a menu whose server side reads those.
    #[serde(default)]
    pub menu_button: Option<i32>,
}

/// Input on a plugin's screen ([`Plugin::screen_input`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenEvent {
    /// The screen as `screen` last saw it.
    pub screen: ScreenView,
    pub input: ScreenInput,
}

/// What happened on a screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScreenInput {
    /// A button pressed (after the client sends its `menu_button`).
    Button {
        id: String,
        #[serde(default)]
        right: bool,
        #[serde(default)]
        shift: bool,
    },
    /// A text field's text changed (`EditBox.setResponder`).
    Text { id: String, text: String },
    /// Enter in a text field.
    Submit { id: String, text: String },
    /// A scroll bar moved to `value`.
    Scroll { id: String, value: i32 },
    /// A click on one of its `plugin_slots` (`slotClicked` handled on the
    /// client).
    SlotClick {
        slot: u32,
        #[serde(default)]
        right: bool,
        #[serde(default)]
        shift: bool,
    },
    /// A key pressed with no text field focused (`keyPressed`): `key` as
    /// the options name it (`key.keyboard.escape`). Answer `consumed`
    /// to keep the client from acting on it (Escape closing the screen).
    Key {
        key: String,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        ctrl: bool,
    },
}

/// What a plugin does with screen input.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenAnswer {
    /// The screen from now on (a toggled button, scrolled slots); `None`
    /// keeps it.
    #[serde(default)]
    pub screen: Option<Screen>,
    /// The input was the plugin's: the client doesn't act on it.
    #[serde(default)]
    pub consumed: bool,
    /// Close the screen (as Escape would).
    #[serde(default)]
    pub close: bool,
}

impl ScreenAnswer {
    /// A new screen.
    pub fn screen(screen: Screen) -> ScreenAnswer {
        ScreenAnswer {
            screen: Some(screen),
            ..ScreenAnswer::default()
        }
    }

    /// Consumed, nothing else changed.
    pub fn consumed() -> ScreenAnswer {
        ScreenAnswer {
            consumed: true,
            ..ScreenAnswer::default()
        }
    }

    /// This answer with `screen` as the screen from now on.
    pub fn with_screen(mut self, screen: Option<Screen>) -> ScreenAnswer {
        self.screen = screen;
        self
    }
}

/// One `GuiGraphics.blit`: a `width` x `height` piece of `texture` from
/// (`u`, `v`), drawn at (`x`, `y`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Blit {
    /// A texture id (`mymod:textures/gui/press.png`). A GUI sprite
    /// (`blitSprite`) is its file: `mymod:textures/gui/sprites/arrow.png`.
    pub texture: String,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub u: f32,
    #[serde(default)]
    pub v: f32,
    pub width: i32,
    pub height: i32,
    /// The texture's size in UV units (`blit`'s `textureWidth`/`Height`).
    #[serde(default = "texture_size")]
    pub texture_width: i32,
    #[serde(default = "texture_size")]
    pub texture_height: i32,
}

fn texture_size() -> i32 {
    256
}

/// A line of text on a screen (`GuiGraphics.drawString`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScreenText {
    pub text: String,
    pub x: i32,
    pub y: i32,
    /// `0xRRGGBB`; vanilla's label grey when left out.
    #[serde(default = "label_grey")]
    pub color: u32,
    #[serde(default)]
    pub shadow: bool,
}

fn label_grey() -> u32 {
    0x404040
}

/// What a port implements.
pub trait Plugin {
    /// What this plugin handles. Called once, before anything else.
    fn init(&mut self) -> Registration;

    /// The visuals of a block entity of a type it registered.
    fn block_entity(&mut self, view: &BlockEntityView) -> Rig;

    /// One client tick of a block entity of a type it registered in
    /// `ticking`: advance the plugin's own state for it (keyed by
    /// `view.pos`; Java's `tick()` on the client) and answer the rig as it
    /// stands now. Called 20 times a second while the player is near; the
    /// client blends each part's transform between the last two ticks, as
    /// Java's renderers lerp by partial ticks. Return the same parts in the
    /// same order while only their transforms move; a changed part list
    /// draws afresh. `block_entity` is still called when data arrives:
    /// treat it as Java's `read`.
    fn tick(&mut self, view: &BlockEntityView) -> Rig {
        self.block_entity(view)
    }

    /// What a block entity of a type in `moves_entities` does to the local
    /// player standing on or in it, this tick (Java's movement handler
    /// for it, client-side: the player moves itself).
    fn entity_motion(&mut self, view: &EntityMotionView) -> Motion {
        let _ = view;
        Motion::default()
    }

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

    /// The colours of an item it registered in `tinted_items`, by tint
    /// index (a model face's `tintindex`), each `0xRRGGBB`. Asked once per
    /// item and kept, so it can't depend on the stack's data; an index past
    /// the end stays untinted.
    fn item_tints(&mut self, item: &ItemView) -> Vec<u32> {
        let _ = item;
        Vec::new()
    }

    /// The look of a fluid it registered in `fluids`. Asked once per
    /// fluid and kept; `None` leaves it to the client's guess from the
    /// mod's texture names.
    fn fluid(&mut self, fluid: &FluidView) -> Option<FluidLook> {
        let _ = fluid;
        None
    }

    /// The screen of a menu type it registered in `screens`. `None` leaves
    /// it to the client's plain grid of slots.
    fn screen(&mut self, view: &ScreenView) -> Option<Screen> {
        let _ = view;
        None
    }

    /// Input on its screen: a button, a text field, a scroll bar, a key.
    /// Tell the server with [`host::send_to_server`]; answer the screen as
    /// it now looks, whether the input was consumed, or close it.
    fn screen_input(&mut self, event: &ScreenEvent) -> ScreenAnswer {
        let _ = event;
        ScreenAnswer::default()
    }

    /// A key it registered in `keys`, pressed: Java's `consumeClick` loop
    /// (in the world) or a screen's `keyPressed`. Tell the server with
    /// [`host::send_to_server`].
    fn key(&mut self, view: &KeyView) {
        let _ = view;
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
pub mod catnip;
pub mod mc;

/// Everything a port usually needs: the API types, vanilla's helpers
/// ([`mc`]) and Catnip's ([`catnip`]).
pub mod prelude {
    pub use crate::catnip::{
        AngleHelper, AnimationFunctions, AnimationTickHolder, Chaser, DirectionHelper, Force,
        Iterate, LerpedFloat, PhysicalFloat, Pointing, VecHelper,
    };
    pub use crate::mc::StateProps;
    pub use crate::mc::color::{Abgr32, Argb32, Color};
    pub use crate::mc::direction::{Axis, AxisDirection, Direction, Mirror, Plane, Rotation};
    pub use crate::mc::java::{JMath, JavaRandom, RandomSource, string_hash};
    pub use crate::mc::mth::Mth;
    pub use crate::mc::nbt::{CompoundTag, ItemStack, ListTag, NBTHelper, NbtUtils};
    pub use crate::mc::pose::{
        CachedBuffers, MathAxis, Pose, PoseStack, Quaternion, SuperByteBuffer, TransformStack,
    };
    pub use crate::mc::vec::{AABB, BlockPos, Vec3};
    pub use crate::*;
}

/// Calls into the client.
pub mod host {
    use super::BlockAt;

    #[cfg(target_arch = "wasm32")]
    #[link(wasm_import_module = "torch")]
    unsafe extern "C" {
        fn log(ptr: *const u8, len: usize);
        fn block_at(x: i32, y: i32, z: i32, out: *mut u8, cap: usize) -> usize;
        fn send(channel: *const u8, channel_len: usize, data: *const u8, data_len: usize);
        #[link_name = "player"]
        fn player_import(out: *mut u8, cap: usize) -> usize;
    }

    #[cfg(not(target_arch = "wasm32"))]
    thread_local! {
        static PLAYER: std::cell::RefCell<Option<super::PlayerView>> = const { std::cell::RefCell::new(None) };
    }

    /// The local player as this tick began: where it is, what it holds,
    /// what it looks at. `None` before it joins a world.
    pub fn player() -> Option<super::PlayerView> {
        #[cfg(target_arch = "wasm32")]
        {
            let mut buf = vec![0u8; 512];
            // SAFETY: the host writes at most `cap` bytes into `buf`.
            let mut len = unsafe { player_import(buf.as_mut_ptr(), buf.len()) };
            if len > buf.len() {
                buf.resize(len, 0);
                len = unsafe { player_import(buf.as_mut_ptr(), buf.len()) };
            }
            if len == 0 || len > buf.len() {
                return None;
            }
            serde_json::from_slice(&buf[..len]).ok()
        }
        #[cfg(not(target_arch = "wasm32"))]
        PLAYER.with(|p| p.borrow().clone())
    }

    /// Off the wasm host (tests): what [`player`] answers.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_player(player: Option<super::PlayerView>) {
        PLAYER.with(|p| *p.borrow_mut() = player);
    }

    /// A packet to the mod's server side on `channel` (its own namespace,
    /// or one of its `channels`), as Java's `PacketDistributor.sendToServer`
    /// writes it: `data` is the payload's body ([`crate::buf::Writer`]).
    /// Sent after the current call.
    pub fn send_to_server(channel: &str, data: &[u8]) {
        #[cfg(target_arch = "wasm32")]
        // SAFETY: the host reads the two buffers, which the caller owns.
        unsafe {
            send(channel.as_ptr(), channel.len(), data.as_ptr(), data.len())
        };
        #[cfg(not(target_arch = "wasm32"))]
        SENT.with(|s| s.borrow_mut().push((channel.to_owned(), data.to_vec())));
    }

    #[cfg(not(target_arch = "wasm32"))]
    thread_local! {
        static SENT: std::cell::RefCell<Vec<(String, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Off the wasm host (tests): what [`send_to_server`] sent, taken.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn take_sent() -> Vec<(String, Vec<u8>)> {
        SENT.with(|s| std::mem::take(&mut *s.borrow_mut()))
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
        pub extern "C" fn torch_tick(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let rig = match ::serde_json::from_slice::<$crate::BlockEntityView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().tick(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable block entity view: {e}"));
                    $crate::Rig::default()
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&rig).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_entity_motion(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let motion = match ::serde_json::from_slice::<$crate::EntityMotionView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().entity_motion(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable entity motion view: {e}"));
                    $crate::Motion::default()
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&motion).unwrap_or_default())
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

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_fluid(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let look = match ::serde_json::from_slice::<$crate::FluidView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().fluid(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable fluid view: {e}"));
                    None
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&look).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_item_tints(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let tints = match ::serde_json::from_slice::<$crate::ItemView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().item_tints(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable item view: {e}"));
                    Vec::new()
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&tints).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_screen_input(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the event into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let answer = match ::serde_json::from_slice::<$crate::ScreenEvent>(input) {
                Ok(event) => PLUGIN.with(|p| p.borrow_mut().screen_input(&event)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable screen event: {e}"));
                    $crate::ScreenAnswer::default()
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&answer).unwrap_or_default())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_key(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            match ::serde_json::from_slice::<$crate::KeyView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().key(&view)),
                Err(e) => $crate::host::log_line(&format!("unreadable key view: {e}")),
            }
            $crate::abi::pack(b"null".to_vec())
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn torch_screen(ptr: i32, len: i32) -> i64 {
            // SAFETY: the host wrote the view into a buffer from torch_alloc.
            let input = unsafe { $crate::abi::input(ptr, len) };
            let screen = match ::serde_json::from_slice::<$crate::ScreenView>(input) {
                Ok(view) => PLUGIN.with(|p| p.borrow_mut().screen(&view)),
                Err(e) => {
                    $crate::host::log_line(&format!("unreadable screen view: {e}"));
                    None
                }
            };
            $crate::abi::pack(::serde_json::to_vec(&screen).unwrap_or_default())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_player_reads_as_the_host_writes_it() {
        let player: PlayerView = serde_json::from_str(
            r#"{"pos": [1.5, 64.0, -2.5], "yaw": 90.0, "pitch": 10.0, "selected": 2,
                "main_hand": {"item": "minecraft:iron_pickaxe", "count": 1},
                "sneaking": false,
                "hit": {"kind": "block", "pos": [1, 63, -3], "face": "up", "at": [1.5, 64.0, -2.5]}}"#,
        )
        .unwrap();
        assert_eq!(
            player.hit,
            Some(Hit::Block {
                pos: [1, 63, -3],
                face: mc::direction::Direction::Up,
                at: [1.5, 64.0, -2.5]
            })
        );
        assert_eq!(player.off_hand, None);
        let entity = serde_json::to_value(Hit::Entity { id: 7, at: [0.0; 3] }).unwrap();
        assert_eq!(entity["kind"], "entity");
    }

    #[test]
    fn a_screen_fills_java_s_defaults() {
        let screen: Screen = serde_json::from_str(
            r#"{"width": 176, "height": 166,
                "background": [{"texture": "m:textures/gui/press.png",
                                "x": 0, "y": 0, "width": 176, "height": 166}],
                "slots": [[56, 17], null],
                "texts": [{"text": "50%", "x": 80, "y": 60}]}"#,
        )
        .unwrap();
        assert_eq!(screen.background[0].texture_width, 256);
        assert_eq!(screen.slots, vec![Some([56, 17]), None]);
        assert_eq!(screen.texts[0].color, 0x404040);
        assert_eq!(screen.title_at, None);
    }

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
