# The mod API

How a converted mod's behaviour runs in this client. A mod's client code
(renderers, animations, payload handlers, screens) is ported to
Rust against this API, compiled to WebAssembly, and shipped inside its
`.torchmod` package as `plugin.wasm` (`TORCHMOD_FORMAT.md`). The client
loads it at run time in a sandbox. Nothing is compiled into the client, so
a package works for anyone whose client has the API version it targets.

The API is mod-neutral: it describes what any mod's client code needs (a
block entity's moving parts, its data, its neighbours), not one mod's
features. `docs/porting/` tells an agent how to port Java to it.

## Pieces

| Piece | Where | Role |
|---|---|---|
| Guest SDK | `crates/torch-mod-api` | The types and the `export_plugin!` macro a port is written against. Builds for `wasm32-unknown-unknown`. |
| Host | `src/modapi/` | Loads each package's `plugin.wasm` (wasmi, an interpreter: it runs on every platform the client does), feeds it data, draws what it returns. |
| Ports | `ports/<modid>/`, kept outside the repo (git-ignored) | One crate per mod; `torch-convert` builds it into the package. |
| Porting docs | `docs/porting/`, kept outside the repo | For the agent: the workflow, Java-to-API mappings, checks. |

## Design: describe once, animate natively

A plugin is never called per frame. It's called when something it draws
changes (a block entity appears, its data or block state changes, a
neighbour changes), and it answers with a description: the parts to draw,
each with a transform and an animation the client plays every frame in
native code (a spin at so many degrees per tick, a swing, a slide). A
spinning shaft costs one call when its speed changes, not one per frame.
This keeps the interpreter off the hot path and makes the API declarative,
which is also easier to port to and to check.

## Version 1

`api: 1` in a package manifest. The host refuses a plugin whose
`torch_api()` is newer than it knows.

### Handlers

A plugin registers handlers in `init`:

| Handler | Registered for | Called | Returns |
|---|---|---|---|
| `block_entity` | block entity type ids (`create:kinetic`) | when one appears, its data (NBT) or its block state changes, a face neighbour's block state changes, or a modded block entity appears beside it | a `Rig` |
| `tick` | of those, the types in `ticking` | every client tick (20 a second) while the player is within 96 blocks | a `Rig`, after advancing the plugin's own state for that block entity |
| `entity_motion` | block entity types in `moves_entities` (`mymod:belt`) | every tick the local player stands on or in one | a `Motion`: `move_by` (moved with collision), then `steps` in order (`move`, `offset` (`setPos` relative, no collision), `velocity`, `on_ground`, `reset_fall`), then `velocity` |
| `entity` | entity type ids in `entities` (`mymod:biplane`) | when an entity of the type appears, and when its data or a `payload` changes it | a `Rig`, in the entity's own frame |
| `payload` | the mod's packet channels in `channels` (`mymod:sync_state`) | each play-phase packet the server sends on one | a `Redraw`: entity ids and block entity positions whose rigs to ask for again |
| `block_tint` | block ids in `tinted_blocks` (`mymod:maple_leaves`) | once per block state, then kept | a `Tint`: `none`, `grass`, `foliage`, `dry_foliage`, `water` (the biome's colour, blended by position) or `color` (`0xRRGGBB`) |
| `fluid` | source fluid ids in `fluids` (`mymod:honey`; its flowing form asks as it) | once per fluid, then kept | a `FluidLook` (`still`, `flowing`: block atlas sprites; `tint`: `0xRRGGBB`), or `null` to leave it to the client |
| `item_tints` | item ids in `tinted_items` (`mymod:custom_egg`) | once per item, then kept | the colours of its tint indices (`tintindex`), `0xRRGGBB` each |
| `screen` | menu type ids in `screens` (`mymod:press`) | when the menu opens, and when its data slots or slot count change | a `Screen`, or `null` for the client's plain grid |

The registration also lists:

- `replaces_block_model`: of those types, the ones whose block's own
  model the plugin draws instead (a spinning shaft). The client leaves
  those blocks' static models out of the terrain. A type not listed keeps
  its block model, and the plugin's parts draw over it (a tank's fluid).
- `sprites`: textures the plugin's cuboids use that no block model does
  (a fluid's still sprite), so the client puts them in its atlas.
- `tinted_blocks`: blocks whose tinted model faces (`tintindex`) the
  plugin colours: Java's `BlockColor`. Without one, a modded block with
  tinted faces gets foliage, grass or water by its name.
- `fluids`: fluids whose looks the plugin gives: NeoForge's
  `IClientFluidTypeExtensions`. The client draws a modded fluid block
  (one the dump says renders invisible and holds a fluid) as vanilla draws
  water: heights, flow and faces, joining only the same fluid, in the
  translucent layer, tinted. Without a plugin it looks for the mod's
  textures by name (`fluid/<name>_still`, `block/<name>_flow`,
  `block/fluid/<name>/still`, ...), and a fluid with none doesn't draw.
- `tinted_items`: items whose tinted layers the plugin colours: Java's
  `ItemColor`, for its default stack. The colours become constant tints
  on every model in the item's definition that names none (icons, held
  items). Spawn eggs need no plugin: the converter writes their colours.

`tick` is Java's client-side `tick()`. A plugin keeps its own state per
block entity (keyed by `pos`) and advances it each tick: items gliding
along a belt, an arm reaching for its target, a handle turning. The client
blends each part between its last two ticks by the partial tick, as Java's
renderers lerp `prevX` to `x`, so motion is smooth between ticks. Keep the
same parts in the same order while only their transforms change; a
different part list (an item arriving) draws afresh. `block_entity` is still
called when the server's data arrives: treat it as Java's `read`, and let
the next ticks carry on from it, as Java's client does between syncs.

To check a port's simulation against a real server, run the client with
`MC_MODAPI_TRACE=<file>`: each modded block entity view made from the
server's data (a `BlockEntityView`, with `time`) is appended to `file`, one
JSON line each. `torch-port-check --trace <file>` replays it: for each
ticking type it reads one update, ticks to the next, and reports how far
the parts drifted from what the next update draws.

`entity_motion` is a mod's movement handler for the local player
(Create's `BeltMovementHandler`). Players move themselves on the client, so
without it a belt carries everything but the player. It gets an
`EntityMotionView` (`block_entity`, plus `entity`: `pos`, `velocity`,
`width`, `height`, `on_ground`, `sneaking`); the client applies the answer
before the player's next move. Other entities need nothing: the server
moves them.

`payload` gets the channel and the raw bytes the mod's server side wrote;
`torch_mod_api::buf::Reader` reads them as Java's `FriendlyByteBuf` does
(VarInts, strings, block positions, UUIDs, network NBT as JSON). A plugin
keeps what a packet says in its own state (the plugin value lives for the
session) and reads it when the client asks for the rigs the `Redraw`
names. Payloads reach the plugin in arrival order, on the network thread.

`entity` gets an `EntityView` (`{"type": ..., "id": ..., "width": ...,
"height": ..., "time": ..., "data": {...}}`; the size is the type's own). Its parts are in
the entity's frame: the origin at its feet, `+Z` the way it faces,
sixteenths of a block. The client places the rig at the entity every frame
and turns it by the entity's yaw; animations play as on block entities. An
entity a plugin draws loses the placeholder box. `data` holds the synced
entity data the client could read, by data index (numbers, booleans,
strings, positions, vectors, UUIDs, poses, NBT; item stacks, text and
particles are left out); a change asks for the rig again. State a mod
sends in its own packets comes through `payload`.

`block_tint` gets a `BlockStateView` (`{"block": ..., "props": {...}}`); it
can't depend on position or time, since the answer is kept per state.

`screen` is Java's `MenuScreens.register` plus the screen class. It gets a
`ScreenView` (`{"menu": ..., "title": ..., "extra": [...], "data": [...],
"slots": 38}`): `extra` is the data NeoForge opened the menu with (the
buffer the mod's `IContainerFactory` reads, often a block position; read it
with `buf::Reader`), `data` the menu's `ContainerData` slots by index (32;
unsent ones read 0), `slots` how many slots the server sent. It answers a
`Screen`, an `AbstractContainerScreen` in GUI pixels from its top left:

- `width`, `height`: `imageWidth`, `imageHeight`; the client centres it.
- `background`: `renderBg`'s blits in order, each `{"texture":
  "mymod:textures/gui/press.png", "x", "y", "u", "v", "width", "height",
  "texture_width": 256, "texture_height": 256}`. A GUI sprite is its file
  (`mymod:textures/gui/sprites/arrow.png`). Progress bars are blits sized
  from `data`, as `renderBg` computes them.
- `slots`: each menu slot's `x`, `y` (`Slot.x`/`y`) by slot index; `null`
  hides one. Player slots left off the end sit where vanilla puts them (8
  across, `height - 82` down). `player_start`: where the player's 36 start,
  when not last.
- `title_at`, `inventory_label_at` (default `[8, 6]` and
  `[8, height - 94]`), `hide_title`, `hide_inventory_label`, and `texts`
  (`renderLabels`'s other strings: `text`, `x`, `y`, `color`, `shadow`).

The client draws the blits, then the slots and items, then the labels, and
handles clicks as on vanilla menus. Shift-click isn't predicted (the mod's
`quickMoveStack` runs on the server; its answer moves the items). A menu
type no plugin registers opens as a plain grid: its slots nine to a row
over the inventory. Textures load on demand, up to 8 per screen, each
squeezed into a 256-pixel cell.

Planned for later versions: screen buttons and widgets, HUD elements,
item renderers.

### What a plugin gets: `BlockEntityView`

```json
{"type": "create:simple_kinetic", "pos": [10, 64, -3],
 "block": "create:shaft", "props": {"axis": "y", "waterlogged": "false"},
 "nbt": {"Speed": 32.0, "Network": {"Id": 12}}, "time": 120345}
```

- `nbt`: the block entity's data as the server sent it, NBT as JSON:
  compounds are objects, lists and arrays are arrays, numbers are numbers
  (their NBT type is dropped), strings are strings.
- `props`: the block state's properties; `block` its id.
- `time`: the game time, in ticks, when the view was made.
- `nbt_time`: the game time its `nbt` arrived. A view is made again with
  the same NBT when a neighbour changes, so time-based state in the NBT
  (an angle, an item's position) is as of `nbt_time`, not `time`.

### What a plugin answers: `Rig`

```json
{"parts": [
  {"geometry": {"model": {"model": "create:block/shaft"}},
   "transform": {"pivot": [8, 8, 8], "rotate": [0, 0, 0]},
   "animation": {"spin": {"axis": [0, 1, 0], "pivot": [8, 8, 8],
                          "degrees_per_tick": 9.6, "phase": 0}}}
]}
```

A `Part` is geometry, a static transform, and an animation. Units are
sixteenths of a block (model units) from the block's corner; angles are
degrees.

Geometry:

| Kind | Fields | Use |
|---|---|---|
| `model` | `model` (a block model id), `x`, `y` (quarter turns, as a blockstate variant), `uvlock` | the mod's partial models: shafts, cogs, lids, arms |
| `block` | `block`, `props` | a whole block state's model (a carried block, a display) |
| `cuboid` | `from`, `to`, `texture` (a sprite id), `faces` (which of down, up, north, south, west, east), `tint` (ARGB), `uv` (optional rect in sixteenths) | simple boxes: a tank's fluid, a gauge needle |
| `text` | `lines` (with `§` codes), `color` (ARGB), `shadow`, `glowing` | text as a sign draws it, readable from `+Z`, one font pixel a sixteenth: displays, nixie tubes, labels |
| `item` | `item` (an item id), `flat` (optional transform) | an item as Java's `ItemDisplayContext.FIXED` draws it: items on belts, depots, displays |

An `item` is the item's model with its `fixed` display transform, centred
on the part's origin: the block's corner before the transform, so give
the transform a pivot of 0. `flat`, when given, replaces the transform
for an item whose model is a flat sprite (`item/generated`): Java's
renderers check `BakedModel.isGui3d()` and lay sprites down, and the
plugin can't see the model. A modded item draws from its package's
assets.

Transform, applied to the geometry in this order: scale about `pivot`,
rotate about `pivot` (X, then Y, then Z), translate.

Animation, played after the transform:

| Kind | Fields | Motion |
|---|---|---|
| `none` | | static |
| `spin` | `axis`, `pivot`, `degrees_per_tick`, `phase` | angle = `phase` + `degrees_per_tick` x ticks |
| `swing` | `axis`, `pivot`, `amplitude`, `period_ticks`, `phase` | angle = `amplitude` x sin(2 pi (ticks / `period_ticks`) + `phase`) |
| `slide` | `offset`, `period_ticks`, `phase` | moves to `offset` and back, a smooth (cosine) ping-pong |
| `path` | `points` (`[tick, x, y, z]`, game ticks and an offset) | straight legs between the points; held at the first before it and at the last after (an item gliding to a belt's end) |

Ticks are the client's game time with the partial tick, so motion is
smooth between server updates.

Scroll (optional, `scroll`): a texture moving across the part, Java's
`SpriteShiftEntry` with `shiftUVScrolling` (a belt's surface). Quads drawn
with sprite `from` draw from sprite `to` instead, at the same texel offset,
moved down `to` by `fract(per_tick x ticks + phase) x span` of its height.

| Field | Meaning |
|---|---|
| `from`, `to` | sprite ids; `to` is usually a taller strip, so the moved rect stays in it (past its end the host wraps to its top) |
| `per_tick` | cycles a tick; negative runs up |
| `phase` | cycles at tick 0 |
| `span` | the share of `to`'s height one cycle moves through |

No model names a scroll target, so declare `to` in `sprites`. The atlas
keeps a mod sprite taller than wide (and not animated) as stacked squares;
the host cuts quads at their edges and rebuilds the mesh per quarter texel
of offset.

### Helpers: vanilla's and Catnip's code in Rust

`use torch_mod_api::prelude::*;` brings in the API types and Rust copies
of the Java a client renderer or tick calls, with Java's behaviour (the
sine table, `Math.round`, `Direction` tie-breaks, `java.util.Random`), so
a port renames calls instead of rewriting them:

| Module | Java |
|---|---|
| `mc::mth` | `Mth` (`Mth::lerp`, `Mth::sin`, `Mth::wrap_degrees`, ...) |
| `mc::java` | `Math.round`/`signum`/`floorMod` where Rust differs (`JMath`), `java.util.Random` and `RandomSource.create(seed)` (`JavaRandom`), `String.hashCode` |
| `mc::direction` | `Direction`, `Direction.Axis`/`AxisDirection`/`Plane`, `Rotation`, `Mirror` |
| `mc::vec` | `Vec3`, `BlockPos`, `AABB` |
| `mc::pose` | `PoseStack`, JOML's `Quaternionf`, `com.mojang.math.Axis` (`MathAxis`), Flywheel's `TransformStack`, Catnip's `SuperByteBuffer`/`CachedBuffers`; ends in a `Part` |
| `mc::nbt` | `CompoundTag`/`ListTag` getters over `view.nbt` with Java's defaults, `NbtUtils`, Catnip's `NBTHelper`, `ItemStack` |
| `mc::color` | `FastColor.ARGB32`/`ABGR32`, Catnip's `Color` |
| `catnip` | `LerpedFloat` and `Chaser`, `PhysicalFloat`, `AngleHelper`, `VecHelper`, `DirectionHelper`, `Pointing`, `Iterate`, `AnimationFunctions`, `AnimationTickHolder` |

Names: `Class.methodName(a)` is `Class::method_name(a)`, `v.methodName(a)`
is `v.method_name(a)`. Java overloads by argument type get one name per
form: `translate(x, y, z)`/`translate_vec(v)`, `add(v)`/`add_xyz(x, y, z)`,
`rotate(q)`/`rotate_by(radians, axis)`/`rotate_degrees(degrees, axis)`,
`scale(f)`/`scale_xyz(x, y, z)`, `wrap_degrees` (float, double) and
`wrap_degrees_i` (int). `move` is a keyword: `AABB::move_by`,
`Motion::move_entity`.

Views read like Java's objects: `view.block_pos()`,
`view.direction("facing")` (`getValue(FACING)`), `view.axis("axis")`,
`view.flag("powered")`, `view.nbt.get_float("Speed")`,
`entity.get_bounding_box()`. A movement handler builds its `Motion` with
`m.move_entity(v)`, `m.set_delta_movement(v)`, `m.offset_pos(v)`,
`m.set_on_ground(b)` and `m.reset_fall_distance()`, in Java's order.

### Host functions (imports, module `torch`)

| Function | Does |
|---|---|
| `log(ptr, len)` | writes a UTF-8 line to the client log, tagged with the mod |
| `block_at(x, y, z, out_ptr, out_cap) -> len` | the block at a position as JSON (`{"block": ..., "props": {...}}`), for connection logic; writes nothing and returns the needed length if `out_cap` is too small. v1 sees the blocks within 2 of the block entity (a copy made before the call); farther ones return 0 |

### ABI

Data crosses as UTF-8 JSON in the plugin's memory.

| Export | Signature | Does |
|---|---|---|
| `torch_api` | `() -> i32` | the API version the plugin was built for |
| `torch_alloc` | `(len: i32) -> i32` | a buffer the host writes input into |
| `torch_free` | `(ptr: i32, len: i32)` | frees a buffer from `torch_alloc` or a result |
| `torch_init` | `() -> i64` | the `Registration` JSON, as `ptr << 32 \| len` |
| `torch_block_entity` | `(ptr: i32, len: i32) -> i64` | a `BlockEntityView` in, a `Rig` out (same packing) |
| `torch_entity` | `(ptr: i32, len: i32) -> i64` | an `EntityView` in, a `Rig` out |
| `torch_payload` | `(ptr: i32, len: i32) -> i64` | the channel's length (u16, big-endian), the channel, then the data in; a `Redraw` out |
| `torch_block_tint` | `(ptr: i32, len: i32) -> i64` | a `BlockStateView` in, a `Tint` out |
| `torch_tick` | `(ptr: i32, len: i32) -> i64` | a `BlockEntityView` in, the `Rig` after one tick out |
| `torch_entity_motion` | `(ptr: i32, len: i32) -> i64` | an `EntityMotionView` in, a `Motion` out |
| `torch_fluid` | `(ptr: i32, len: i32) -> i64` | a `FluidView` (`{"fluid": ...}`) in, a `FluidLook` or `null` out |
| `torch_item_tints` | `(ptr: i32, len: i32) -> i64` | an `ItemView` (`{"item": ...}`) in, an array of colours out |
| `torch_screen` | `(ptr: i32, len: i32) -> i64` | a `ScreenView` in, a `Screen` or `null` out |

All but `torch_api`, `torch_alloc`, `torch_free` and `torch_init` are
optional: a plugin built before a hook existed lacks its export.

The SDK writes all of these; a port only implements the `Plugin` trait.

### Limits

- A call that runs past its fuel (a step budget) or traps is abandoned:
  the block entity draws nothing, and the client logs it once.
- A plugin can't reach the file system, the network or other plugins.

## Where the data comes from

A modded block entity's type and data reach the client in chunk packets
and `block_entity_data`. azalea can't hold a modded block entity type, so
the last protocol hop takes modded ones out of those packets before azalea
reads them and hands them to `modapi` with the server's type id
(`protocol/v775.rs`).

A modded menu opens through NeoForge's `neoforge:advanced_open_screen`
payload; the 1.21.1 hop turns it into a plain `open_screen` and keeps its
extra data for the screen (`protocol/v767.rs`). azalea holds a modded menu
as `Menu::Modded`, sized by the first full content packet. Native-protocol (26.3) NeoForge servers don't go
through a hop yet; their modded block entities aren't captured.
