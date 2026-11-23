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
| Client code: renderers, animations, screens, HUD | spinning shafts, contraptions, Ponder | No | Hand ports per mod (step 4) |

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

### 4. Behaviour, by hand

Mod client code calls into Minecraft's client classes everywhere, so it
can't be run or translated generically. The options:

- **Embed a JVM** and run mod client code. It would need most of
  Minecraft's client API rebuilt underneath. Not realistic, and impossible
  on Android and web.
- **Hand ports in Rust** for the mods that matter, the way vanilla is done
  now. First candidate: Create's kinetics (rotation from block entity NBT
  plus the dumped models).
- **A plugin system** (for example WASM) so ports ship apart from the
  client. Worth it only once several ports exist.

## Jar or source

The tool needs only jars: the dumper runs them and the assets come from
them. A mod's source helps only for hand ports, as a reference, the way
`reference/` serves vanilla parity.

## Distribution

The tool runs locally on jars the user already has and writes a local
cache. Converted mod assets don't ship with the client: mod licences vary.

## Order

1. Finish the 1.21.1 entity and item translation (`MULTIVERSION_PLAN.md`
   section 4).
2. Namespace-aware block models (checklist item 16).
3. Asset import (step 1).
4. Dumper and client loaders (steps 2 and 3).
5. Create kinetics port, if moving machines are wanted (step 4).

After 3 and 4, a Create world looks right standing still.
