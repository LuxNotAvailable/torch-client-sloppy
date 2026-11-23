# Porting a mod's client code: the agent's guide

You are porting one Minecraft mod's client-side Java (its renderers and
animations) to Rust, against the Torch mod API. Your output is a crate in
`ports/<modid>/` that builds to `plugin.wasm`, which `torch-convert` puts
into the mod's `.torchmod` package. Read this whole file first, then
`docs/MOD_API.md` (the API you write against) and `JAVA_TO_API.md` (how
Java renderer code maps onto it).

This is a general tool. Nothing here is specific to one mod: the API
describes block entity visuals in general (parts, transforms,
animations), and a port translates whatever a mod's renderers do into
that.

## What a port is, and isn't

The client never runs Java. The mod's server logic still runs on the
server; its blocks, models, textures and shapes were already converted
from data (the package's assets and `blocks.json`). What's left, and what
you port, is the client code that draws things the data can't describe:
moving parts, things drawn from a block entity's state (a tank's fluid
level, a gauge's needle), parts that depend on neighbours.

A port is **declarative**. The client calls your plugin when something
changes (a block entity appears, its NBT or block state changes), and you
return a `Rig`: parts, each a geometry, a static transform and an
animation. You never draw a frame; the client plays the animations. So:

- Java that runs per frame (`render(be, partialTicks, poseStack, ...)`)
  becomes a function of the block entity's state that returns parts plus
  motion. Find what changes per frame (usually an angle from time and a
  speed) and express it as an `Animation` (`spin`, `swing`, `slide`).
- Anything that can't be expressed (particles, per-frame random flicker,
  item rendering inside a block, entities) is out of scope for API v1: list
  it as skipped in `PORTED.md`, don't approximate it with hacks.

## Inputs

1. **The mod's Java source.** If the mod has public source for the jar's
   version, use that. Otherwise decompile the jar with Vineflower (pinned
   in `tools/new_version.py`):

       java -jar vineflower-1.12.0.jar -log=WARN --folder <mod.jar> <out-dir>

2. **The mod's assets**, in its `.torchmod` (or the jar): the models
   (`assets/<modid>/models/block/...`) your parts name.
3. **What the server sends.** Your plugin only sees the NBT the server
   puts in the block entity's update packet. In Java that's the block
   entity's `getUpdateTag` (often `saveAdditional` or a `write(tag,
   clientPacket)` with `clientPacket == true`). Read it to learn the NBT
   keys and types. A field the renderer reads but the server never sends
   (client-only state) isn't available: work out whether it's derivable
   from what is sent, or skip it.

## Workflow

1. **Inventory.** List the mod's client renderers: classes implementing
   `BlockEntityRenderer`, registered in `BlockEntityRenderers.register` (or
   a mod's own registration helper, e.g. Registrate's `.renderer(...)`).
   Note each one's block entity type id(s): the string in its
   `BlockEntityType` registration (`create:simple_kinetic`). Write the list
   into `PORTED.md` (template below) before porting anything.
2. **Batch.** Port a few renderers at a time, simplest first. A renderer
   that only rotates one model is a good first one.
3. **Port one renderer** (see `JAVA_TO_API.md`):
   - Find the models it draws and their ids.
   - Find each part's placement (the `PoseStack` calls before drawing it)
     and turn it into a `Transform`.
   - Find what moves with time and turn it into an `Animation`.
   - Find which block state properties and NBT values it reads, and read
     them from `BlockEntityView` (`prop`, `nbt_f64`, `nbt`).
4. **Register** the type id in `init`, and dispatch on `view.kind` in
   `block_entity`.
5. **Test** in the crate: a unit test per renderer that builds a
   `BlockEntityView` like the server's (types and keys from step 3 of
   Inputs) and checks the parts, transforms and animation values. Check
   numbers against the Java formulas (angles per tick, offsets in
   sixteenths).
6. **Build** for wasm32 (`cargo build --release --target
   wasm32-unknown-unknown` in the crate), then package it:

       torch-convert convert <mods> --ports ports --out <dir>

7. **Record** in `PORTED.md` what's done and skipped, then hand over for
   an in-game check: the user positions the camera and compares
   screenshots with the real modded client.

## Rules

- Start from `ports/example/` (copy it): its `Cargo.toml` (own workspace,
  `cdylib`, size-optimized release) and the `export_plugin!` shape.
- No `unsafe`, no threads, no file or network access (the sandbox has
  none). Keep dependencies to `torch-mod-api` and small pure crates.
- Deterministic: the same view gives the same rig. Time-based motion goes
  in `Animation`, never in the static transform. `view.time` is for
  phases that depend on when the block entity was made, if the Java does
  that.
- Units: model units are sixteenths of a block from the block's corner;
  angles are degrees; animation speeds are per tick (20 ticks a second).
- Plugins are called again when something changes, so read state fresh
  every call; don't cache across calls unless it's pure data (tables).
- Comments: short. Say what Java behaviour a function reproduces, naming
  the Java class and method (`KineticBlockEntityRenderer.getAngleForBe`),
  so a reviewer can check it against the source.
- Licence: a port is a derivative of the mod's code. Check the mod's
  licence before a port is shared or committed; record it in `PORTED.md`.

## When the API is missing something

Don't work around it inside the plugin. Write it in `PORTED.md` under
"Needs from the API" with the Java it would serve (one line each). API
additions are made in `crates/torch-mod-api` and `src/modapi` together,
with a version bump, by whoever maintains the API.

## `PORTED.md` template

```markdown
# <Mod name> port

- Mod: <id> <version>, jar sha256 <...>
- Licence: <...> (port may be shared: yes/no)
- API: 1

## Renderers

| Java class | Block entity types | Status | Notes |
|---|---|---|---|
| `...ShaftRenderer` | `create:simple_kinetic` | done | spin about the axis property |
| `...BeltRenderer` | `create:belt` | skipped | items on belts need item rendering |

## Needs from the API

- <what, and the Java it would serve>
```

## Checks before handing over

- `cargo test` passes in the crate; every ported renderer has a test.
- `cargo build --release --target wasm32-unknown-unknown` passes with no
  warnings.
- `PORTED.md` lists every renderer from the inventory, each done or
  skipped with a reason.
