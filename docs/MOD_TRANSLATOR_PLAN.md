# Mod translator plan

A standalone tool, `torch-convert` (`crates/torch-convert`), turns a mod jar
into a `.torchmod` package (`TORCHMOD_FORMAT.md`) that this client loads, so
a modded server (Create on NeoForge, for example) looks and collides right.
It goes with `MULTIVERSION_PLAN.md`, which gets the client onto the server;
this gets the mods' content into the client.

Decided 2026-11-23:

- No conversion ships with the client. The client reads no jars and runs
  no mod code; it only loads packages from `<storage>/mods/`.
- One package per mod, independent of any modpack, so a converted mod can
  be handed to someone else. The numbering that depends on the whole
  modpack (state ids) is worked out at join time from the server's
  registry sync.
- Ported behaviour goes in the package too (`plugin.wasm` against the mod
  API, step 4), so a package never needs a client rebuild.

## What a mod jar holds, and how far each part goes

| Part | Example (Create) | Translatable | How |
|---|---|---|---|
| Assets: textures, block and item models, blockstates, lang | cogwheel textures and models | Yes, fully | Copied into the package under the mod's namespaces |
| Registry names and ids, tags, recipes | `create:shaft` and its id | Already arrives | NeoForge's frozen registry sync (`client/neoforge.rs`), carried through the 1.21.1 hop |
| Static data defined in code: block properties and their value order, collision shapes, entity sizes, block entity types | the shaft's `axis` property and shape | Yes, by running the mod once | The dumper (step 2), inside `torch-convert` |
| Entity model geometry | a train model | Needs client classes | A client-side dump run (not built) |
| Mod network payloads | config sync, contraption packets | Skipped generically | Already skipped. Reading one needs that mod's codec; acting on it needs its logic |
| Block entity NBT | kinetic speed, belt contents | Readable | Plain NBT, already received. Showing it needs logic |
| Client code: renderers, animations, screens, HUD | spinning shafts, contraptions, Ponder | Not mechanically | Agent ports against a mod-port API and conversion docs (step 4) |

## Why modded blocks are stone without a package

A server syncs block registry names, not block states. A state's network id
comes from the block's property definitions in Java: the property names, and
each property's values in the order the code declares them (enum order, an
int range, `true` then `false`). A blockstates JSON lists the combinations
but not that order. Without it a modded state can't be told apart, so the
client draws and collides every one as stone (`blocks::info::stand_in`).

## Using it

    torch-convert setup                      # installs NeoForge, asks for the EULA
    torch-convert convert MODS_DIR [--skip NAME]... [--with JAR|DIR]... --out DIR

`convert` runs every jar given (`--with` ones load but aren't packaged)
once in a headless NeoForge server, then writes one package per jar.
Client-only mods crash a dedicated server and must be skipped. Copy the
packages to the client's `<storage>/mods/`. Build the tool with
`cargo build --release --manifest-path crates/torch-convert/Cargo.toml`; it
needs a JDK 21 or newer at run time.

First run (2026-11-23): 46 jars of the user's Create modpack (79 minus
client-only, perf and UI mods, and ToadLib, which is client-only) gave 39
packages, 24 MB. The dump: 4208 blocks, 186,970 states, 151,454 of them
modded; Create alone is 643 blocks, 31,389 states.

## Pipeline

### 1. Assets

`torch-convert` copies the jar's own namespaces' `blockstates`, `models`,
`items`, `textures` and `lang` (nested jar-in-jar mods' too), never
`minecraft`, never sounds. The client mounts each package's assets below
the resource packs (`resourcepacks/mods.rs`), only under the namespaces its
manifest lists. Block models are namespace-aware (checklist item 16):
blockstates load as `create:shaft`, models and textures keep their
namespace, and the textures mods' block models use join the block atlas
under their ids (`blocks::mod_sprites`).

Still to do:

- 1.21.1 mods use pack format 34: item models through `models/item`
  `overrides` and `custom_model_data`, from before 1.21.4's item model
  definitions. `torch-convert` should rewrite them to item definitions.
- Models drawn only in code stay empty: block entity renderers (Create's
  factory gauge, Big Cannons' barrels: 82 blocks in the first run), dynamic
  models (Copycats+: 67) and custom loaders (backpacks: 6). These need
  ports (step 4).

Connected textures: Create defines them in code (`AllSpriteShifts`), but
`CTSpriteShifter` caches each as `<texture>-><sheet>+<type>` even on a
server. The dumper loads every mod's `*SpriteShift*` class and reads that
cache by reflection (555 in the first run: Create, Create Deco, Diesel
Generators, Design-n-Decor, Create Additions, Aeronautics); packages carry
them in `connected.json`, and the client's CTM engine (`terrain/ctm.rs`)
runs Create's tile picks (`AllCTTypes`) on texture-matched faces, joining
the same block (modded blocks get their own kinds). Create's finer rules
(a vault's size picking its medium or large sheet, casings joining encased
blocks) aren't followed: the first sheet listed wins, and only the same
block joins.

The cut sheets take the block atlas from 5.5k to 17k sprites. At 16 tiles
wide and 32 px tiles (PC), that was 34k px tall, past the GPU's image
limit, and every texture sampled garbage; the atlas is 64 tiles wide now
(`terrain::ATLAS_COLS`, `terrain.slang`).

Done: `torch-convert` bakes `neoforge:obj` models to `torch_quads` (free
quads, which the client bakes like element faces, turned with the variant)
and flattens `neoforge:composite` ones into one model with every child's
elements (`models.rs`). First run: 65 OBJ and 13 composite models; of
3685 modded blocks, 156 still can't draw (above) and 182 are invisible by
design. Names use the mod's namespace (`block.create.belt`) and its
`lang/en_us.json`.

### 2. The dumper

`crates/torch-convert/dumper/TorchDump.java`, embedded in the tool and
compiled with plain `javac` against the installed server's libraries
(NeoForge's patched server jar first: it holds only the patched classes).
It dumps on `ServerAboutToStartEvent` and halts before a world loads:

- every block with its properties (values in declared order) and its
  states in `getPossibleStates` order
- per state: collision and outline shapes, light, occlusion, render shape,
  hardness, fluid and the rest (`TORCHMOD_FORMAT.md`)
- block entity types, entity types and sizes, items

`torch-convert` cuts the dump down to each jar's namespaces for its
package.

### 3. Client loaders

Block states (`blocks/modded.rs`): the packages' `blocks.json` merge at
startup. When a NeoForge server syncs its block registry, its modded blocks
follow vanilla's in its own order; adding up each one's state count from
the packages gives every modded state's id, and the hops' shift past
vanilla gives this client's. A server block no package has stops the count
there, logged by name. Named states answer `blocks::info` (name,
properties, friction, hardness), draw with their mod's model, and give
azalea's physics and picking their dumped shapes (`MODDED_SHAPES`).

A modpack has more states than a `BlockState`'s `u16` holds past
vanilla's (about 29,800 room against 151,454 in the first run). Each modded
id a chunk names gets the next compact id instead, both ways, through
azalea's `MODDED_IDS`; palettes and packets keep the server's ids. Only the
states in view need ids.

A mod can add blocks under `minecraft:` (VanillaBackport adds 222, pale
oak and the rest, from wire index 2570 of 4208). `torch-convert` gives them
to the jar that ships their blockstate; the client draws one its game
version has as that vanilla block (the state sharing the most
properties), decoded straight to it.

Limits:

- lighting still treats modded states as stone (`lighting::props::of`)
- block entity renderers' parts (`ENTITYBLOCK_ANIMATED`) draw only their
  static model, when the mod has one
- modded entities and items have their data packaged but nothing reads it
  yet

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
  whether its package can be shared.
- Ports load at run time: each is a `plugin.wasm` in its package, built
  against a versioned mod API (the manifest's `api`), run in a sandbox by
  the client. Never compiled into the client, so a package needs no
  client rebuild and works for anyone with a client that has that API.
  The agent's port and its build live in `torch-convert`'s world, not the
  client's.

## Jar or source

The dumper runs jars and the assets come from them. For ports the agent
needs readable Java: a mod's source directory as given, or the jar
decompiled (Vineflower, as `tools/new_version.py` does for vanilla).

## Distribution

The tool runs locally on jars the user already has. Packages don't ship
with the client. A package carries the mod's assets (and later a port of
its code), so handing one to someone else redistributes the mod's work:
the manifest records the mod's licence, which decides whether that's
allowed.

## Order

1. Finish the 1.21.1 entity and item translation (`MULTIVERSION_PLAN.md`
   section 4). Done apart from recipes and teleport smoothing.
2. Namespace-aware block models (checklist item 16). Done.
3. `torch-convert` with assets, the dumper and the package format; the
   client's package loader and block states. Done; waiting on a run on
   the user's server.
4. Lighting for modded states; old (format 34) item models rewritten to
   item definitions in `torch-convert`.
5. The mod-port API, its WASM host in the client, and the conversion docs
   (step 4).
6. Pilot: an agent ports Create's kinetics against them, which tests the
   docs before porting at scale.

After 3 and 4, a Create world looks right standing still; after 6, its
machines move.
