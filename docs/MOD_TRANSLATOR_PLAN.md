# Mod translator plan

A tool that takes a modpack's mod jars and turns what it can into data this
client loads, so a modded server (Create on NeoForge, for example) looks and
collides right. It goes with `MULTIVERSION_PLAN.md`, which gets the client
onto the server; this gets the mods' content into the client.

## What a mod jar holds, and how far each part goes

| Part | Example (Create) | Translatable | How |
|---|---|---|---|
| Assets: textures, block and item models, blockstates, lang, sounds | cogwheel textures and models | Yes, fully | Mount the jar's `assets/<modid>/` as a pack under the mod's namespace |
| Registry names and ids, tags, recipes | `create:shaft` and its id | Already arrives | NeoForge's frozen registry sync (`client/neoforge.rs`), carried through the 1.21.1 hop |
| Static data defined in code: block properties and their value order, collision shapes, entity sizes, entity model geometry, block entity types | the shaft's `axis` property and shape, a train model | Yes, by running the mod once | The dumper (step 2) |
| Mod network payloads | config sync, contraption packets | Skipped generically | Already skipped. Reading one needs that mod's codec; acting on it needs its logic |
| Block entity NBT | kinetic speed, belt contents | Readable | Plain NBT, already received. Showing it needs logic |
| Client code: renderers, animations, screens, HUD | spinning shafts, contraptions, Ponder | Not mechanically | Agent ports against a mod-port API and conversion docs (step 4) |

## Why modded blocks are stone today

A server syncs block registry names, not block states. A state's network id
comes from the block's property definitions in Java: the property names, and
each property's values in the order the code declares them (enum order, an
int range, `true` then `false`). A blockstates JSON lists the combinations
but not that order. Without it a modded state can't be told apart, so the
client draws and collides every one as stone (`blocks::info::stand_in`).

## Pipeline

### 1. Asset import

- Input: a mods folder (or one jar). Output: one pack per mod, or a merged
  pack, in the client's storage, mounted after vanilla.
- Needs namespace-aware block model lookup (`CLEANUP_CHECKLIST.md` item 16).
- 1.21.1 mods use pack format 34: item models through `models/item`
  `overrides` and `custom_model_data`, from before 1.21.4's item model
  definitions. This needs the old-pack conversion in `MULTIVERSION_PLAN.md`
  section 4.9.
- Fully automatic. No mod code runs.

### 2. The dumper

A small Java mod, loaded with the modpack into a headless NeoForge server
(the same idea as `tools/DumpGameData.java` for vanilla). It walks every
registry after load and writes JSON:

- every block state in network id order, with its properties
- collision and outline shapes per state
- block entity types
- entity types and sizes
- entity model geometry, from each model's `createBodyLayer`
- item properties the client predicts with (stack size, food, tool tiers)

Driven by a script like `tools/new_version.py`: official NeoForge installer,
the user's mods folder, run, collect. It needs a JDK and nothing else. The
output is a bundle keyed by the mod list's hash (or by server), which the
client loads when that server's registry sync matches it.

### 3. Client loaders

- Block states from the dump replace the stand-in for that server: real
  models from step 1, real shapes for collision and culling. That also ends
  the "stuck inside a stone block" desync next to non-full modded blocks.
- Modded entities render as static models from the dumped geometry and the
  imported textures, without animation.
- Unknown modded content keeps today's fallbacks.

### 4. Behaviour: agent ports

Mod client code calls into Minecraft's client classes everywhere, so it
can't be run or translated mechanically (embedding a JVM would need most of
Minecraft's client API rebuilt underneath, and can't run on Android or
web). Instead, the tool writes the docs a port needs into the mod's
conversion directory and an agent ports the Java, batch by batch. Four
things make that reliable.

**A mod-port API in the client.** A port needs fixed places to land, or
each one reaches into internals and breaks on every refactor. Today
vanilla's equivalents are spread over `blockentities/render`,
`entities/models`, `gui` and `client/packets` with no extension points.
The API:

- block entity renderer hook: state in, draw list parts out (`draw`)
- entity renderer and model hook: dumped geometry plus a pose function
- mod payload handler, registered by channel with its codec, feeding state
  into the world or session (Create's contraption and train data arrive
  this way, so codecs are ported too)
- screens and HUD elements: `gui` widgets behind a stable interface
- a per-tick client logic hook

**Conversion docs, one per Java API area.** Each pairs a vanilla example
with its existing Torch port (`reference/` holds the vanilla side):

- `PoseStack` / `VertexConsumer` to draw lists
- `BlockEntityRenderer` and `EntityRenderer` to the hooks
- `AbstractContainerScreen` and HUD layers to `gui`
- `StreamCodec` / `CustomPacketPayload` to `AzBuf` and the payload handler
- `Mth`, `RandomSource` to `util::mth`, `util::javarandom`

The tool copies the relevant ones into the conversion directory with the
mod's own class list, so the agent works from local context.

**Scope control.** A big mod's client side is hundreds of classes (Create's
Ponder alone is large). The tool lists the mod's client classes, sorts
them by kind (renderer, model, screen, payload, other) and hands the agent
one bounded batch at a time. A class that fits no API kind is flagged in
`PORTED.md`, not guessed at.

**Verification.** Without checks, ported code compiles and still looks
wrong:

- build and tests per batch: codecs round-trip, models match the dumped
  geometry
- side-by-side screenshots: a real modded client and Torch at the same
  spot, positioned by hand
- a `PORTED.md` per mod: what is done, skipped and why, and the mod version
  it was ported from, so a mod update is a diff against that version

**Constraints.**

- Licence: a code port is a derivative work, so each mod's licence decides
  whether its port can live in this repo or stays local, like converted
  assets.
- Compiled-in ports mean a rebuild per modpack. Loading ports into a
  shipped client (for example as WASM plugins) is worth building once
  ports are many or must ship separately.

## Jar or source

The dumper runs jars and the assets come from them. For ports the agent
needs readable Java: a mod's source directory as given, or the jar
decompiled (Vineflower, as `tools/new_version.py` does for vanilla).

## Distribution

The tool runs locally on jars the user already has and writes a local
cache. Converted mod assets don't ship with the client: mod licences vary.

## Order

1. Finish the 1.21.1 entity and item translation (`MULTIVERSION_PLAN.md`
   section 4).
2. Namespace-aware block models (checklist item 16).
3. Asset import (step 1).
4. Dumper and client loaders (steps 2 and 3).
5. The mod-port API and the conversion docs (step 4).
6. Pilot: an agent ports Create's kinetics against them, which tests the
   docs before porting at scale.

After 3 and 4, a Create world looks right standing still; after 6, its
machines move.
