# Mod conversion checklist

What converting a NeoForge 1.21.1 mod for the Torch client covers today,
and what it doesn't. The goal is a one-shot machine: jars in, packages out
(`crates/torch-converter`), every mod working in the client without hand
work. Go down the list, fix, tick.

Legend: `[x]` done, `[~]` partial (the note says what's missing), `[ ]`
missing. **Source** says where the information comes from: the jar's
**assets**, the **dump** (`crates/torch-convert/dumper/TorchDump.java`,
run inside a NeoForge server), or a **port** (client Java rewritten
against the mod API, `docs/porting/AGENT_GUIDE.md`). **Client** is where
the Torch client uses it. A field dumped but unused counts as missing:
the eating sound was exactly that.

Last reviewed: 2026-11-24, against Create 6.0.10 and a 46-jar Create
pack.

## 1. Input and packaging

- [x] Jars from a folder, nested jars (`META-INF/jarjar`) two deep.
- [x] Mod metadata from `neoforge.mods.toml` or `mods.toml`, both `[[mods]]`
  tables and inline `mods = [{...}]`; licence; required dependencies.
- [x] Choose per jar: convert, load only (`--with`), skip; client-only and
  performance mods suggested as skip (window: Mods tab).
- [x] Official NeoForge server installed and checked against its published
  SHA-256; EULA asked in the window.
- [x] One `.torchmod` per jar (`docs/TORCHMOD_FORMAT.md`), shareable
  without the modpack.
- [x] Progress for every stage and command (`--progress`, the window).
- [ ] Missing dependencies: a jar whose required mod isn't in the input
  fails the dump server; the window should name what's missing before
  starting.
- [ ] A dump server crash: point at the mod in `logs/latest.log` and offer
  to skip it, instead of a generic failure.
- [~] Licence: recorded in the manifest and `PORTED.md`; nothing warns when
  an All Rights Reserved mod's package is about to be shared.

## 2. Assets

Kept from each jar: `blockstates`, `models`, `items`, `textures` (with
`.mcmeta` animations), `lang`.

- [x] Block and item models, blockstates, textures, animated textures,
  translations.
- [x] NeoForge `obj` and `composite` model loaders rewritten to plain
  models.
- [x] 1.21.4 item definitions (`items/<id>.json`) written for 1.21.1 item
  models.
- [~] Other model loaders (`neoforge:separate_transforms`,
  `item_layers`, `fluid_container`, mods' own) are left as they are: the
  client draws what it can of the JSON.
- [ ] `render_type` in model JSON (cutout, translucent): not read, so modded
  glass and ice draw cutout (see 3).
- [ ] Emissive faces (`neoforge_data` light values on model elements).
- [ ] Item model `overrides` with mod predicates (a modded bow's pull, a
  charged state): only the base model shows.
- [ ] **Sounds:** `sounds.json` and `.ogg` files aren't kept, so no modded
  sound event can play (machines, mobs, music discs, custom eat sounds).
- [ ] **Particles:** `particles/*.json` and their textures aren't kept.
- [ ] Fonts, shaders, `atlases/*.json` (non-block sprite atlases), GUI
  sprites (`textures/gui/sprites` are kept as textures, but nothing draws
  modded screens).
- [ ] Armor and equipment layers (`textures/models/armor`): kept, but
  nothing draws modded armor on players (see 4).

## 3. Blocks

- [x] Ids and every state, kept compact on the client; server-synced
  numbering.
- [x] Models per state (blockstate variants and multipart).
- [x] Collision and outline shapes (physics, the selection box). Source:
  dump.
- [x] Light emitted, light blocked, solid render, occlusion. Source: dump.
- [x] Mining: destroy time, needs a tool. Source: dump.
- [x] Friction (ice-like blocks). Source: dump.
- [x] Names in block info (lang).
- [x] Create-style connected textures (`connected.json`).
- [~] Block entities: drawn only by a port's plugin (Create's are partly
  ported; other mods none).
- [~] Baked-model wrappers that add geometry from code (Create's pipes):
  only through a port.
- [ ] **Block sounds** (break, step, place, hit, fall): dumped per block,
  unused; every modded block sounds like stone
  (`util::block_model::sound_group` looks up vanilla names).
- [ ] **Colour tints** (modded grass, leaves, water-like blocks): block
  colour handlers are client code; nothing dumped, nothing ported, so
  tinted faces draw grey. Needs either a dump of a tint *kind* per block
  (grass, foliage, water, constant colour) or a port hook.
- [ ] **Translucency**: modded states never draw in the translucent layer
  (`dumped_visual` sets `translucent: false`). Read `render_type` from
  the model, or dump `ItemBlockRenderTypes` (client-side: needs a heuristic
  or a port).
- [ ] Random offset (flowers, grass): dumped as `offset`, unused.
- [ ] Speed factor (soul sand) and jump factor (honey): dumped, unused.
- [ ] Emissive rendering, cull groups (leaves culling each other).
- [ ] Modded fluids: dumped per state (`fluid`), but fluid textures and
  tints live in client code (`IClientFluidTypeExtensions`); modded fluid
  blocks don't draw.
- [ ] Mining and placing particles for modded blocks (unverified: they may
  use the model's `particle` texture already).
- [~] Placement: no client prediction for modded block items (the server
  places them after a round trip); `replaceable` is dumped, unused.

## 4. Items

- [x] Names from the server's registry sync; translated names from lang.
- [x] Inventory icons and held-item models.
- [x] Use: eat and drink animation, use time, "can't eat when full",
  generic eat and drink sounds, crumbs. Source: dump (`use_animation`,
  `use_ticks`, `food`).
- [x] No barrier ghost placement for modded items.
- [ ] **Custom consume sounds**: the generic eat/drink sound plays; dump
  the item's eating and drinking sound events (needs sounds, see 2).
- [ ] **Item tints** (colour handlers: dyed items, spawn eggs, potions of
  modded types): client code; nothing dumped.
- [ ] Durability bars: `max_damage` dumped, unused (a modded tool shows no
  bar unless the server patched it).
- [ ] Max stack size: dumped, unused (client-side stacking predictions).
- [ ] Tool behaviour: mining speed with modded tools (the client's dig time
  prediction), attack damage and attribute tooltips. Needs a dump of tool
  and attribute components.
- [ ] Rarity name colours, enchantment glint defaults, other default
  components (`DataComponents` defaults): dump them generally rather than
  one at a time.
- [ ] Equippable items: modded armor and curios worn on players.
- [ ] Items drawn by code (`BlockEntityWithoutLevelRenderer`: Create's
  wrench, goggles, potato cannon): only the base model.
- [~] Items on display in block entities: belts and depots ported for
  Create; others need ports.

## 5. Entities

- [x] Types from the server, named by their registry id.
- [~] Drawn as a fixed magenta placeholder box (0.6 by 1.8); the dump's
  real size (`entities.json`) isn't read.
- [ ] Entity models and renderers (vanilla-style, GeckoLib, Create's
  contraptions and trains): needs an entity side to the mod API and ports.
- [ ] Hitboxes from the dumped size, for targeting and the F3+B overlay.
- [ ] Contraptions (Create bearings, pistons, gantries, trains): not drawn.

## 6. Sounds

- [ ] Modded sound events (block sounds, machine loops, mob sounds,
  consume sounds): no sound assets in packages, no registry of modded
  sound events on the client. Keep `sounds.json` and `sounds/`, and
  play `ns:event` through the same path vanilla sounds use.
- [ ] Block sound types from the dump (3).

## 7. Particles, screens, HUD

- [ ] Modded particle types (assets and the client factories: code).
- [ ] Modded container screens (Create's schematic table, backpacks):
  menus open with no screen. Needs a generic slot-based fallback from the
  menu's slots, then ports for real layouts.
- [ ] Creative tabs for modded items.
- [ ] HUD overlays and tooltips drawn by code (goggles' stress readout).

## 8. Networking

- [x] NeoForge handshake, registry sync, modded block, item and entity ids
  across the 1.21.1 hop.
- [~] Modded item components: an unreadable one ends that stack's
  component walk (the item and count survive; later components are
  lost).
- [ ] Custom payloads (mods' own packets): ignored. Things they carry
  (Create's contraption data, schematic progress) never arrive.
- [~] Extensible enums: logged, packets naming them may not read.

## 9. Client code (ports)

- [~] Block entity renderers: Create 31 of 100 types done or partial
  (`ports/create/PORTED.md`); no other mod ported.
- [~] Unattended porting (`tools/port_agent.py`): inventory from the Java,
  one gated session per renderer, patch out. Passes the gates; quality
  still needs review and an in-game look.
- [ ] Entity renderers, item renderers, screens, particles, overlays:
  no API side yet (`docs/MOD_API.md` is block entities only).
- [ ] An agent run started by the window per converted mod by default,
  with a time budget.

## 10. Toward one shot

- [x] One window: folders in and out, mod list, live progress, copy to the
  client.
- [x] Guardrails on ported code (`crates/torch-port-check`).
- [ ] A report per package: what converted, what fell back (stone sounds,
  untinted faces, placeholder entities, unported renderers), so gaps are
  visible without playing.
- [ ] A generic default-components dump for items and blocks (one change
  closes several rows in 3 and 4).
- [ ] In-game smoke check: load each package in the client headless, place
  every block and give every item, log errors and missing models.
- [ ] Regression pack: a fixed set of jars converted on every change to
  the converter, diffed against the last packages.
