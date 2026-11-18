# Architecture

Map for finding code. Start here, then the target module's `mod.rs` `//!` doc.
Searches skip data files (`.ignore`); use `rg --no-ignore` to reach them.

## Flow

Network (`eagler`/azalea, `protocol` hops) feeds `client` (packets, tick,
`bus`). `client` updates the session and world. `play` predicts player
actions. `entities` and `blockentities` turn state into models; `terrain`
meshes chunk sections (on `client`'s worker). `renderer` (Bevy/wgpu,
`MC_RENDERER=wgpu`) or `vk` (native Vulkan, desktop default) draws them.
`gui` draws screens and HUD and talks back through `client::bus`.

## `src/`

| Module | Owns | Start at |
|---|---|---|
| `lib.rs` | crate root, `run()`, shared statics | `run` |
| `audio` | sound engine, categories, music, ambience, subtitles | `mod.rs` |
| `blockentities` | block entity models (`render/`) and client sim (`sim/`) | `mod.rs` |
| `blocks` | blockstate registry, model baking, per-state `info` | `info.rs`, `bake.rs` |
| `client` | connection, packet handlers, game tick, GUI bus, chunk meshing worker, dimension types, sky timelines, server post effects; the frame's `View` (cameras, fog) renderers read | `tick.rs`, `bus.rs`, `packets.rs`, `view.rs` |
| `draw` | the frame's non-terrain draw lists as plain data (entity meshes and instances, particle sprites, env parts) the game fills and `vk` reads | `mod.rs` |
| `diag` | logging, profiler (`prof_span!`), perf reports, watchdog | `perf_report.rs` |
| `eagler` | Eaglercraft websocket transport, server query | `mod.rs` |
| `entities` | entity state, kind facts, models (`models/` by family), rigs, posing | `mod.rs`, `facts.rs` |
| `gui` | every screen, HUD, forms, options (incl. `antialias`), click GUI | `screens.rs`, `form.rs`, `option_codec.rs` |
| `items` | item model definitions, dispatch, icons, meshes | `model.rs`, `dispatch.rs` |
| `lighting` | client light engine (sky + block), light thread, lightmap colours and levels (`lightmap`) | `level.rs`, `engine.rs` |
| `mobile` | touch controls (`mobile_ui`) | `mod.rs` |
| `modules` | click-GUI modules (ESP, zoom, freecam, ...) and their store | `list.rs`, `registry.rs` |
| `platform` | OS/web/Android abstraction: env, storage, http, input, time | `mod.rs` |
| `play` | player actions: interaction, placement, riding, vehicles, commands | `interaction.rs`, `placement.rs` |
| `protocol` | multi-version support: packet tables, id remaps, hops | `hop.rs` (recipe at top) |
| `renderer` | Bevy/wgpu renderer: terrain, entities, sky, post, shader-pack draw | `systems.rs`, `terrain.rs` |
| `resourcepacks` | pack list, mounting, Modrinth, Torch Default pack | `mod.rs`, `TORCH_DEFAULT_PACK.md` |
| `shaderpack` | Iris/OptiFine pack loading: properties, preprocess, programs; compiling to WGSL (`compile/`, `install`) for both renderers; pack block ids and vertex data; uniforms (`uniforms/`, `inputs`, `frame_state`) | `pipeline.rs`, `compile/mod.rs`, `install.rs` |
| `terrain` | section meshing for both renderers: block geometry, light, CTM, vertex layout, cave visibility, block animations; streaming section changes to the renderer and its listeners (`stream`) | `mod.rs`, `mesh.rs`, `stream.rs` |
| `text` | fonts, glyph layout, text components | `mod.rs` |
| `util` | vanilla-parity helpers (`mth`, `javarandom`), particles, packs, zip | `mth.rs` |
| `vk` | native Vulkan renderer and `vkview` | `mod.rs`, `live.rs` |
| `voice` | Simple Voice Chat client | `mod.rs` |

## Outside `src/`

| Path | What |
|---|---|
| `shaders/*.slang` | Vulkan shaders (`build.rs` compiles them) |
| `src/**/*.wgsl` | wgpu shaders, next to their Rust |
| `tools/` | generators (`gen_*.py`, `protogen.py`), asset and build scripts |
| `vendor/` | patched crates; read `vendor/PATCHES.md` first |
| `crates/torch-opus` | safe Opus bindings |
| `reference/minecraft-<ver>/` | decompiled vanilla, for parity checks (git-ignored) |
| `android/`, `web/`, `fastlane/` | platform shells and store metadata |

## Data files (skip unless the task is about their values)

`src/generated_items.rs`, `src/generated_block_sounds.rs`,
`src/entities/models/**/*_anim.rs`, `src/entities/models/aquatic/animation/`,
`src/protocol/data/*.json`.
