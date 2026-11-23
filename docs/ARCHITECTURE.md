# Architecture

Map for finding code. Start here, then the target module's `mod.rs` `//!` doc.
Searches skip data files (`.ignore`); use `rg --no-ignore` to reach them.

**Two builds, one tree.** PC (desktop) and Android share this code. Every
Android-only path sits behind `cfg(target_os = "android")`, `mobile_ui`,
`vk::terrain::MOBILE` or the built-in pack's `MC_OS_ANDROID`, and every
difference is listed in `docs/ANDROID_VS_PC.md` (read it before touching
platform-split code).

## Flow

Network (`eagler`/azalea, `protocol` hops) feeds `client` (packets, tick,
`bus`). `client` updates the session and world. `play` predicts player
actions. `entities` and `blockentities` turn state into models; `terrain`
meshes chunk sections (on `client`'s worker). `vk` (native Vulkan) draws them;
`scene` holds the game's Bevy-side scene (cameras, models, sky) that fills `draw`;
`app` sets up Bevy and runs input and the frame view.
`gui` draws screens and HUD and talks back through `client::bus`.

## `src/`

| Module | Owns | Start at |
|---|---|---|
| `lib.rs` | crate root, `run()`, shared statics | `run` |
| `app` | Bevy app setup (`systems::run`), input and keybind actions, intents, the frame's `FrameView`, app state, screenshots | `systems.rs`, `input.rs` |
| `audio` | sound engine (rodio/cpal; on Android through Oboe, its stream closed on suspend and reopened on resume), categories, music, ambience, subtitles, output devices (desktop) | `mod.rs`, `engine.rs` |
| `blockentities` | block entity models (`render/`) and client sim (`sim/`) | `mod.rs` |
| `blocks` | blockstate registry (mods' namespaces too), model baking, per-state `info`; modded states named by packages, with compact ids (`modded.rs`) | `info.rs`, `bake.rs` |
| `client` | connection, packet handlers, game tick, GUI bus, chunk meshing worker, dimension types, sky timelines, server post effects, the NeoForge handshake (`neoforge.rs`); the frame's `View` (cameras, fog) renderers read | `tick.rs`, `bus.rs`, `packets.rs`, `view.rs` |
| `draw` | the frame's non-terrain draw lists as plain data (entity meshes and instances, particle sprites, env parts) the game fills and `vk` reads | `mod.rs` |
| `diag` | logging, profiler (`prof_span!`), perf reports, watchdog | `perf_report.rs` |
| `eagler` | Eaglercraft websocket transport, server query | `mod.rs` |
| `entities` | entity state, kind facts, models (`models/` by family), rigs, posing | `mod.rs`, `facts.rs` |
| `gui` | every screen, HUD, forms, options (incl. `antialias`), click GUI | `screens.rs`, `form.rs`, `option_codec.rs` |
| `items` | item model definitions, dispatch, icons, meshes | `model.rs`, `dispatch.rs` |
| `modapi` | the mod API host: loads packages' `plugin.wasm` (wasmi), feeds them modded block entities, draws their rigs (`docs/MOD_API.md`) | `mod.rs`, `store.rs`, `render.rs` |
| `lighting` | client light engine (sky + block), light thread, lightmap colours and levels (`lightmap`) | `level.rs`, `engine.rs` |
| `mobile` | touch controls (`mobile_ui`) | `mod.rs` |
| `modules` | click-GUI modules (ESP, zoom, freecam, ...) and their store | `list.rs`, `registry.rs` |
| `platform` | OS/web/Android abstraction: env (every `MC_*` variable, `env.rs`), storage, http, input, time, lifecycle notices (`notify`) | `mod.rs`, `env.rs` |
| `play` | player actions: interaction, placement, riding, vehicles, commands | `interaction.rs`, `placement.rs` |
| `protocol` | multi-version support: packet tables, id remaps, hops; registry tables a modded server syncs (`registries.rs`) and NeoForge's payload codecs (`neoforge.rs`) | `hop.rs` (recipe at top) |
| `scene` | the Bevy-side scene: cameras, entity and hand models, anims, skins, sky, clouds, weather, tethers, overlays, world text, block atlas; fills `draw` and `View` (`view_sync`) | `mod.rs`, `entity_draw.rs`, `hand.rs` |
| `resourcepacks` | pack list, mounting, Modrinth, Torch Default pack; converted mods' `.torchmod` packages from `<storage>/mods` (`mods.rs`, `docs/TORCHMOD_FORMAT.md`) | `mod.rs`, `TORCH_DEFAULT_PACK.md` |
| `shaderpack` | Iris/OptiFine pack loading: properties, preprocess, programs; compiling GLSL packs to WGSL (the native terrain prelude, `compile/preludes.rs`, reads `vk::terrain`'s slots and pack quads), and binding the built-in pack's precompiled SPIR-V (`compile/`, `install`, `spirv`); which packs are offered (`discover`: the built-in one only on Android); pack block ids and vertex data; uniforms (`uniforms/`, `inputs`, `frame_state`) | `pipeline.rs`, `compile/mod.rs`, `install.rs` |
| `terrain` | section meshing for both renderers: block geometry, light, CTM, vertex layout, cave visibility, block animations; streaming section changes to the renderer and its listeners (`stream`); the renderer's terrain inputs (`textures`) and occlusion switch (`hiz`) | `mod.rs`, `mesh.rs`, `stream.rs` |
| `text` | fonts, glyph layout, text components | `mod.rs` |
| `util` | vanilla-parity helpers (`mth`, `javarandom`), particles, packs, zip | `mth.rs` |
| `vk` | native Vulkan renderer and `vkview`: the game's frame (`live.rs`; on a turned display, drawn turned straight into the swapchain, or upright and turned once under a pack), terrain (`terrain.rs`: GPU cull, Hi-Z occlusion, also from a pack's depth, `pack_pyramid`), shader packs (`pack/`: targets, screen passes, bindless, `tiled` render passes for tile GPUs); the mobile fixed terrain path (`terrain_fixed.rs`) | `mod.rs`, `live.rs`, `pack/mod.rs` |
| `voice` | Simple Voice Chat client | `mod.rs` |

## Outside `src/`

| Path | What |
|---|---|
| `shaders/*.slang` | Vulkan shaders (`build.rs` compiles them) |
| `src/shaderpack/builtin/pack/**/*.slang` | the built-in shader pack, in Slang; `build.rs` compiles it to SPIR-V behind the engine prelude in `builtin/iris/` |
| `tools/` | generators (`gen_*.py`, `protogen.py`; `new_version.py` brings a game or NeoForge version into `reference/`), asset and build scripts; `bench-apk/` (the Android game bench, beside the real app) and `vkview-apk/` (the viewer as an Android app) |
| `vendor/` | patched crates; read `vendor/PATCHES.md` first |
| `crates/torch-opus` | safe Opus bindings |
| `crates/torch-mod-api` | the mod API's SDK: the types ports return and the client reads, host calls, `export_plugin!` (`docs/MOD_API.md`) |
| `ports/` | ported mods' client code, one crate each, built to `plugin.wasm` (`docs/porting/AGENT_GUIDE.md`); `example/` is the template, `create/` the first real port |
| `crates/torch-convert` | the standalone mod converter: jar to `.torchmod` (`docs/MOD_TRANSLATOR_PLAN.md`); its own workspace, never built into the client |
| `reference/minecraft-<ver>/` | decompiled vanilla, for parity checks (git-ignored) |
| `android/`, `web/`, `fastlane/` | platform shells and store metadata |
| `docs/` | `ANDROID_VS_PC.md` (every build difference), `ANDROID_SPECIFICS.md` (why the phone differs), `ANDROID_PERF.md` (Pixel measurements), `PC_CANDIDATES.md` (Android findings for PC: applied, and why the rest wasn't), `SESSION_CHECKPOINT.md` (the Android work's state and next steps), `MULTIVERSION_PLAN.md` (what 1.21.1 and NeoForge 1.21.1 support needs, and what can be generated), `MOD_TRANSLATOR_PLAN.md` (turning a modpack's jars into models, shapes and data this client loads), `TORCHMOD_FORMAT.md` (the package), `MOD_API.md` (ported mods' API), `porting/` (how an agent ports a mod) |

## Data files (skip unless the task is about their values)

`src/generated_items.rs`, `src/generated_block_sounds.rs`,
`src/entities/models/**/*_anim.rs`, `src/entities/models/aquatic/animation/`,
`src/protocol/data/*.json`.
