# The `.torchmod` package

A converted mod, as `torch-convert` (`crates/torch-convert`) writes it and
the client loads it from `<storage>/mods/`. One package per mod. It holds
only what the client reads, never the mod's Java, so a package converted
once works on any client and in any modpack that has that mod, and can be
handed to someone else.

The client never converts: it reads no jars and runs no mod code. Every
step that needs the mod's code happens in `torch-convert`.

## Layout

A zip archive, `<modid>-<version>.torchmod`:

| Path | Holds |
|---|---|
| `torchmod.json` | the manifest (below) |
| `assets/<namespace>/blockstates/`, `models/`, `items/`, `textures/`, `lang/` | the mod's own assets, from its jar |
| `assets/<namespace>/items/<item>.json` (written) | for each item a 1.21.1 jar gives only `models/item/<item>.json`: a 1.21.4 item definition pointing at that model |
| `blocks.json` | the mod's blocks: properties, states, shapes, behaviour |
| `entities.json` | the mod's entity types and sizes |
| `items.json` | the mod's items: stack size, damage, block |
| `connected.json` | Create-style connected textures whose sheets the mod ships |
| `plugin.wasm` | ported client code against the mod API (`MOD_API.md`), built by `torch-convert --ports` |
| `PORTED.md` | what was ported, skipped and why (with `plugin.wasm`) |

Only the namespaces the manifest lists may appear under `assets/`, and
never `minecraft`: a mod package can't restyle vanilla.

Models are plain block models: `torch-convert` rewrites those that need a
NeoForge loader. A `neoforge:composite` model becomes one with every
child's elements. A `neoforge:obj` model becomes `torch_quads`, a list of
free quads beside (or instead of) `elements`:

```json
"torch_quads": [{"texture": "#side", "vertices": [[x, y, z, u, v], ...4]}]
```

Positions are in sixteenths of a block, UVs in sixteenths of the texture;
a triangle repeats its last corner. The variant's rotation turns them; they
cull against nothing.

## `torchmod.json`

```json
{
  "format": 1,
  "mod": {"id": "create", "version": "6.0.6", "name": "Create", "license": "MIT"},
  "namespaces": ["create"],
  "mods": [{"id": "create", "version": "6.0.6"}, {"id": "ponder", "version": "1.0.82"}],
  "source": {"file": "create-1.21.1-6.0.6.jar", "sha256": "..."},
  "game": {"minecraft": "1.21.1", "loader": "neoforge", "loader_version": "21.1.264"},
  "dependencies": ["flywheel", "ponder"],
  "converter": "torch-convert 0.1.0",
  "api": 0
}
```

- `format`: this document's version. The client refuses formats it
  doesn't know.
- `mods`: every mod in the jar, its own and nested (jar-in-jar) ones.
- `source.sha256`: the jar it came from, so a package can be matched to a
  mod download.
- `game`: the version the mod was run on. Its blocks' state order is that
  version's.
- `dependencies`: mod ids that had to load for the dump. They need their
  own packages only if they add blocks.
- `api`: the mod API version `plugin.wasm` targets (`MOD_API.md`); `0`
  means no plugin.

## `blocks.json`

The dumper's format 1 (`crates/torch-convert/dumper/TorchDump.java`), cut
down to the mod's own blocks: those in its `namespaces`, and `minecraft:`
blocks past vanilla's that it ships a blockstate for (a backport's
`minecraft:pale_oak_log`). The client draws a `minecraft:` block its own
game version has as that vanilla block; it carries no assets for them.

- `blocks`: in the mod's registry order. Each has `name`, `properties`
  (`[name, [values in declared order]]`), `friction`, `speed_factor`,
  `jump_factor`, `explosion_resistance`, `sound`, `default` (the default
  state's index), `block_entity` (its block entity type, if it has one)
  and `states` in `getPossibleStates` order, each `[fact, value index per
  property]`.
- `facts`: shared per-state behaviour rows: `collision` and `outline`
  (indices into `shapes`, or `null` where the mod's shape needed a real
  level), `light`, `light_block`, `occludes`, `solid_render`, `render`
  (`MODEL`, `INVISIBLE`, `ENTITYBLOCK_ANIMATED`), `destroy_time`,
  `needs_tool`, `air`, `replaceable`, `block_entity`, `push`,
  `random_ticks`, `offset`, `fluid`.
- `shapes`: box lists, `[x0, y0, z0, x1, y1, z1]` in block units. Index 0
  is empty, 1 the full cube.

## `connected.json`

Create's connected textures (its addons' too), as `CTSpriteShifter` made
them:

```json
[{"original": "create:block/andesite_casing",
  "connected": "create:block/andesite_casing_connected",
  "type": "create:omnidirectional", "sheet": 8}]
```

A face showing `original` takes a tile of the `connected` sheet
(`sheet` x `sheet` tiles, row by row), picked by `type` from which
neighbours are the same block: Create's `AllCTTypes` (`horizontal`,
`horizontal_kryppers`, `vertical`, `omnidirectional`, `roof`, `roof_stair`,
`cross`, `rectangle`). A package lists the ones whose sheet it ships.

## How the client uses it

A server's registry sync names every block in its own id order. Vanilla's
come first; each modded block's states follow the ones before it. The
client adds up each modded block's state count from the packages, in the
server's order, which gives every modded state its id. A server block no
package has ends the count there: its states and every block's after it
stay stand-ins, and the log names the missing block.
