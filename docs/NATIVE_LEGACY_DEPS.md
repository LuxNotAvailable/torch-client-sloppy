# Native renderer: what it still takes from the legacy renderer

Reference for 10.4 stage 2–3. Snapshot 2026-11-18 (after b31a5a9). Line numbers drift: search for the symbol, and use the `±` range around it.

Format: **symbol** · defined at `file:line` · used at `file:line` (vk side).

## 1. Hooks the other way (legacy code calls into `vk`)

These keep the native renderer fed. They must move to the game-side code that survives.

| What | Caller (legacy) | Callee (vk) |
|---|---|---|
| Terrain sections, after meshing | moved: `terrain/stream.rs` `finish` | `terrain_dump::section` |
| Terrain cleared | moved: `terrain/stream.rs` `finish` | `terrain_dump::cleared` |
| Column dropped | moved: `terrain/stream.rs` `finish` | `terrain_dump::column_dropped` |
| Block atlas texels | moved: `lib.rs` `run`, after the atlas is built | `terrain_dump::atlas` |
| Entity/env/particle capture plugin | `renderer/entity_material.rs:314` in `EntityMaterialPlugin::build` (303–320) | `entity_dump::plugin` |
| Live renderer plugin | `renderer/systems.rs:2604` in `run` (2533–2700) | `live::LivePlugin` |
| Backend choice | `renderer/systems.rs:2357` `wgpu_settings` (2347–2378), `:2386` `resolve_backend` (2379–2420) | `live::wanted` |
| Pack compile switch | `renderer/packdraw/mod.rs:248` `native_renderer` (246–250) | `live::wanted` |
| Pack changed | `gui/shaderpacks.rs:147, 157, 449` (game side already) | `live::pack_changed` |
| Entity dump gate | `entities/mod.rs:915` (game side already) | `terrain_dump::folder` |

## 2. Terrain

| Data | Defined | Used (vk) |
|---|---|---|
| `mesh::PackedVertex` (vertex layout) | moved: `terrain/mesh.rs` | `terrain.rs:42, 1664`; `packed.rs:27`; `greedy.rs:16`; `terrain_dump.rs:19` |
| `mesh::MeshBuf` | moved: `terrain/mesh.rs` | `terrain_dump.rs:19` |
| `mesh::MATERIAL_LEAF` | moved: `terrain/mesh.rs` | `packed.rs:166, 227` |
| `mesh::leaf_options()` | moved: `terrain/mesh.rs` | `terrain.rs:3246` |
| `visgraph::VisibilitySet` | moved: `terrain/visgraph.rs` | `terrain.rs:43`; `terrain_dump.rs:20` |
| `occlusion::flood_fill`, `section_y` | moved: `terrain/cave.rs` | `terrain.rs:2079, 2098` |
| `ATLAS_COLS` | moved: `terrain/mod.rs` | `terrain.rs:3267, 3343, 3424–3425` |
| `block_anim::current`, `tick_now` | moved: `terrain/block_anim.rs` | `terrain.rs:3269–3271, 3325, 3332` |
| `terrain_pool::hiz::disabled` (env toggle) | `renderer/terrain_pool/hiz.rs:738` | `live.rs:1250` |
| `terrain_pool::TerrainOps` (cleared, unused) | `renderer/terrain_pool/mod.rs:169` | `live.rs:457` |
| `terrain_pool::TerrainTextures` (atlas id) | `renderer/terrain_pool/mod.rs:205` | `entity_dump.rs:988, 1219` |
| `terrain_pool::TerrainParams` (`water_alpha`) | `renderer/terrain_pool/mod.rs:181` | `entity_dump.rs:990, 1224` |
| `terrain::LightmapPixels` | `renderer/terrain.rs:36` (±15) | `entity_dump.rs:653, 982` |

## 3. Cameras and view (`live.rs`, from the Bevy ECS)

Done (step 5): the cameras are plain `bevy_camera` entities (no `Camera3d`, `Msaa`, `ClusterConfig`, tonemapping or render-world extraction). The sky sets the world camera's `sky::WorldFog`; `view_sync` copies it to `View::fog`. The view-bob warp reaches `vk` only through `View::warp`.

| Data | Defined | Used (vk) |
|---|---|---|
| `systems::WorldCamera` + `GlobalTransform` + `Projection::Perspective` | replaced: `client::view::View::world` (filled by `renderer/view_sync.rs`) | `live.rs:697–715` `world_camera`; `entity_dump.rs:976, 983, 1450` |
| `DistanceFog` (`FogFalloff::Linear`) on the world camera | replaced: `View::fog` (from `renderer/sky.rs` `WorldFog`) | `entity_dump.rs:907, 983` |
| `panorama::PanoramaCamera` + `Camera::is_active` (menu?) | replaced: `View::menu` | `live.rs:461–466` |
| `panorama` lens constants (copied) | `renderer/panorama.rs` | `vk/panorama.rs:18–25` |
| `inventory_figure::{FigureCamera, FigureImage, ready, FIGURE_LAYER}` + ortho `ScalingMode::Fixed` | replaced: `View::{figure, figure_ready}`, `client::view::FIGURE_LAYER` | `live.rs:662–695` `figure_view`; `entity_dump.rs:1189–1199` |
| `hand::{HandCamera, HandGlint, HAND_LAYER}` | replaced: `View::hand_*`, `client::view::HAND_LAYER`; the hand and its glints are `draw::Env::{hand, hand_glints}` (filled by `hand.rs` `fill_draw_list`) | `entity_dump.rs` `write_env` |
| `frame_view::FrameView`, `view_bob::current_warp` | replaced: `View::warp` | `entity_dump.rs:989, 1223` |
| `antialias::{AaMode, SSAA_SCALES}` (GUI option) | moved: `gui/antialias.rs` (wgpu cameras: `renderer/antialias.rs`) | `live.rs:485–495` |
| `post_effects::active()` (server flags) | moved: `client/post_effects.rs` | `entity_dump.rs:959`; carried in `vk/env.rs:57` |

## 4. Entities, sky, overlays (`entity_dump.rs`, from Bevy meshes)

Done (step 4): `entity_dump` reads only `draw` lists, `View` and Bevy `Image`s. The game-side collectors (`renderer/entity_draw.rs`, `hand.rs`, `sky.rs`, `clouds.rs`, `overlays.rs`, `screen_effects.rs`, `tethers/precipitation.rs`) still read the Bevy entity trees, and leave `renderer/` with them in step 5.

| Data | Defined | Used (vk) |
|---|---|---|
| Mesh/image cache from `AssetEvent<Mesh/Image>` | meshes: replaced by `draw::bridge::Meshes` (game side; `MeshData::slot` keeps a rebuilt asset's GPU index). Images stay Bevy `Image`s; `vk` keeps their pixel cache (`Cache`, `image_data`, `repeats`) | `entity_dump.rs` `Cache`, `capture` |
| `entity_material::EntityMaterial` (per-entity params) | replaced: `draw::Entities` (filled by `renderer/entity_draw.rs`) | `entity_dump.rs` `write` |
| `entity_material::GLINT_ITEM` (glint image handle) | `renderer/entity_material.rs:247` | `entity_dump.rs:882`; `live.rs:724` |
| `rig_parts::{RigParts, ATTRIBUTE_PART}` (GPU pose rows, part attribute) | rows: `draw::Entities::rows`; `ATTRIBUTE_PART` read by `draw::bridge` | — |
| `player_model::LocalRigRoot` | moved to the collector (`renderer/entity_draw.rs`) | — |
| `tethers::WeatherLayer` | moved to the collector | — |
| `blockentities::{BeRigNode, BeamNode}`, `particles::draw::ParticleBatch` (tags) | moved to the collector | — |
| `sky::{SkyPart, SkyPartMaterial, SKY_SCALE}` | replaced: `draw::Env::sky` (filled by `renderer/sky.rs` `update_sky`; no Bevy sky entities), `draw::SKY_SCALE` | `entity_dump.rs` `write_env` |
| `clouds::CloudLayer` + `StandardMaterial` | replaced: `draw::Env::clouds` (filled by `renderer/clouds.rs`) | `entity_dump.rs` `write_env` |
| `screen_effects::Overlay` (screen overlays) | replaced: `draw::Env::screen` (view space; `write_env` applies the hand projection) | `entity_dump.rs` `write_env` |
| `overlays::{BlockOutlineMarker, BreakOverlayMarker, HitboxMarker, ChunkBorderMarker, EspMarker, OUTLINE_WIDTH}` | replaced: `draw::Env::overlays` (filled by `overlays::fill_draw_list`). ESP and structure-block bounds keep Bevy meshes; `vk` never drew them | `entity_dump.rs` `write_env` |
| `glint::GlintMaterial` | moved to `hand.rs` `fill_draw_list` | — |
| `weather::{ASSETS, SHEETS, NATIVE}` | replaced: `draw::Env::weather` (filled by `tethers/precipitation.rs`); `NATIVE` stays (set by `live.rs`) | `live.rs:618` |
| `screenshot::request_numbered` (vkview reference frame) | `renderer/screenshot.rs:16` | `entity_dump.rs:1208` |
| Particles: `util::particles::{LiveParticles, ParticleAtlas}`, `live_sprites` | replaced: `draw::Particles` (filled by `util/particles/draw.rs` `fill_draw_list`) | `entity_dump.rs` `write_particles` |
| `write` system | reads `draw::{Entities, Env, Particles}`, `View`, the lightmap pixels, `TerrainTextures`/`TerrainParams`, `GLINT_ITEM`, `screenshot` | `entity_dump.rs` |

## 5. Shader packs (`vk/pack/*`)

The pack front end lives in `renderer/packdraw`; the native side compiles and runs what it produces. It keeps all of it, but it must move out of `renderer/`.

| Data | Defined | Used (vk) |
|---|---|---|
| `packdraw::compile::{compile_frame, with_vulkan_terrain}` | moved: `shaderpack/compile/` | `pack/mod.rs:25–26`; `pack/bindless.rs:494–499` |
| `packdraw::compile::{EntityKind, ScreenStage, Step, geometry_alpha}` | moved: `shaderpack/compile/` | `pack/screen.rs:16, 755` |
| `packdraw::install::{install, Installed, InstalledProgram, Module}` | moved: `shaderpack/install.rs` (Bevy systems: `renderer/packdraw/sync.rs`) | `pack/mod.rs:18`; `pack/screen.rs:17, 788`; `pack/targets.rs:163` |
| `packdraw::sources::{Depth, Stub, TextureSource}` | moved: `shaderpack/sources.rs` | `pack/screen.rs:18` |
| `packdraw::uniforms::{BlockSpec, FrameValues, History, UniformBytes, MatrixSet}` | moved: `shaderpack/uniforms/` (wgpu buffer: `renderer/packdraw/uniforms.rs`) | `pack/screen.rs:19, 937` |
| `packdraw::usage::mip_targets` | moved: `shaderpack/usage.rs` | `pack/targets.rs:171` |
| `packdraw::inputs::Inputs` (+ `from_json`) | moved: `shaderpack/inputs.rs` (gathered by `renderer/packdraw/gather.rs`) | `pack/screen.rs:156`; `live.rs:179, 908`; `viewer.rs:457`; `entity_dump.rs:1451` |
| `dimension::Dimension` | moved: `client/dimension.rs` | `pack/mod.rs:17`; `pack/bindless.rs:499` |
| `packvertex::{BlockIds, current, set_current}` | moved: `shaderpack/packvertex.rs` | `live.rs:651–658`; `pack/screen.rs:1953` |
| `packvertex::{player_shadow, set_native_player_shadow}` | moved: `shaderpack/packvertex.rs` | `live.rs:972, 1002`; `entity_dump.rs:1030` |

## 6. GUI textures, screenshots (Bevy images)

| Data | Defined | Used (vk) |
|---|---|---|
| `gui::pool::GuiTextures` (atlas, unihex handles) + `Assets<Image>` | `src/gui/pool.rs` | `live.rs:745–752` `gui_atlas`, `729–743` `image_data` |
| `gui::atlas_writes::AtlasWrites` | `src/gui/atlas_writes.rs` | `live.rs:447–453` |
| Bevy `Screenshot` → native `NativeCapture` → `ScreenshotCaptured` | Bevy `bevy_render` | `live.rs:469–477`, `573`, `1014–1050` `take_captures` |
| `TextureFormat` → `vk::Format` | Bevy `bevy_render` | `live.rs:733–739`; `entity_dump.rs:531` |

## Replacement order (suggested)

1. Move the shared data types out of `renderer/`. Done: `mesh`, `visgraph`, the `occlusion` flood fill, `block_anim` data, `packvertex`, `dimension` (+ `timeline`), and the `packdraw` front end (`compile`, `install`, `sources`, `usage`, `images` as `shaderpack/textures.rs`). Pack blends and texture dimensions are now pack-side types; wgpu converts in `renderer/packdraw/pipelines.rs`. `ShadowCullMode` and `SLOT_TABLE_BINDING` live in `shaderpack::compile`.
   Still pointing back into `renderer/`, to go with step 3: `shaderpack` uses `packdraw::uniforms` (`MatrixSet`, `unsupplied`, `builtin_type`, `resolve_input`) and `terrain_pool::ORIGIN_ROW` (wgpu `direct` prelude branch only).
2. Move section 1's hooks into the mesher and asset loading, so `terrain_dump` no longer depends on `ChunkIndex` or `setup`. Done: `terrain::stream` takes the world's section changes (budgets, dedup) and fires every listener (the dump and the block entity caches); `ChunkIndex` only applies them to wgpu slots. The atlas goes to `terrain_dump::atlas` from `lib.rs`. The plugin and backend rows stay with `renderer::run` until the app setup leaves `renderer/` (step 5).
3. Replace the cameras (section 3) with a plain game-side `View` resource: eye, rotation, fov, fog, hand/figure matrices. Done: `client::view::View`, filled each frame in `PostUpdate` (`ViewSystems`, after transforms) by `renderer/view_sync.rs`; `vk::live` and `entity_dump` read only it. The Antialiasing option is `gui::antialias`, the server's post effects `client::post_effects`. The pack uniforms and inputs moved to `shaderpack` (`uniforms/`, `inputs`, `endflash`, and `frame_state` for render scale, shadow rate, centre depth and the Hi-Z view); `ORIGIN_ROW` is in `terrain`. `shaderpack` no longer uses `renderer/`.
   Still in `renderer/`: the camera systems that fill `View`, and `gather_inputs` (it reads `renderer::environment`, `FrameView` and the input states). Both leave with the app setup (step 5).
4. Replace `entity_dump` capture (section 4): the game hands draw lists to the native renderer directly instead of spawning Bevy meshes. This is the biggest step.
   In batches; wgpu may break meanwhile. Textures stay Bevy `Image` handles (`bevy_image`, not the renderer); meshes and materials go. Lists live in `draw` (`EntityVertex`, `EntityInstance`, `ParticleSprite`, `EnvPart`; filled in `draw::DrawSystems`).
   - Batch 1, done: the types moved to `draw`; `renderer/lightmap.rs` is `lighting::lightmap`; particles fill `draw::Particles`.
   - Batch 2, done: sky, clouds and weather fill `draw::Env` with game-side meshes (`draw::MeshData`); `write_env` takes those beside the remaining Bevy meshes. The wgpu path no longer draws the sky or clouds. Producers stay in `renderer/` until step 5 (they read `environment` and `FrameView`).
   - Batch 3, done: screen overlays (`draw::Env::screen`) and world overlays (`draw::Env::overlays`: outline, cracks, chunk borders, hitboxes). wgpu no longer draws them.
   - Batch 4, done: the hand and held-item glints fill `draw::Env`; their Bevy meshes go through `draw::bridge` (moved from `vk`'s capture). `write_env` reads no Bevy meshes or materials now. The hand's entity tree stays until step 5.
   - Batch 5, done: entities fill `draw::Entities` (`renderer/entity_draw.rs`); `vk` dropped its mesh cache, and its live registry keys meshes by `MeshData::slot`. Still in `vk` from `renderer/`: `LightmapPixels`, `TerrainTextures`/`TerrainParams`, `GLINT_ITEM`, `screenshot` (sections 2, 6).
5. Remove wgpu (order: wgpu deletion first, then materials, cameras and fog, images and screenshots, then `Cargo.toml` and the folder moves).
   - Part 1, done: Vulkan is the only renderer (the Renderer option is gone, `vk::live::wanted` is always true). Deleted: the wgpu GUI pass (`gui/draw.rs`; `gui/pool.rs` keeps the CPU frame), `post`, `ssaa`, `antialias` (cameras), `post_effects`, `esp_material`. ESP is not drawn until it reaches the draw lists.
   - Part 2, done: deleted `terrain_pool` (wgpu terrain, GPU cull, Hi-Z), `packdraw` (wgpu pack draw; `gather.rs` is now `renderer/pack_inputs.rs`), `renderer/terrain.rs` and `block_anim.rs` (the wgpu lightmap and animation images). `TerrainParams`, `TerrainTextures` and `LightmapPixels` are plain resources in `terrain::textures`; the occlusion switch is `terrain::hiz`. `ChunkIndex` keeps its CPU section index for entity occlusion. The HUD's terrain bytes read 0 until `vk` reports them. Pack data only the wgpu path read (ids, blends, custom images, Iris segments) is kept under `allow(dead_code)` for the pack rewrite; under Vulkan, packs see no held-item or vehicle ids yet.
   - Part 3, done: materials are plain data. `draw::{Material3d, SimpleMaterial, Alpha}` replace `MeshMaterial3d`, `StandardMaterial` and `AlphaMode`; `EntityMaterial` and `GlintMaterial` are plain assets (no bind groups or shaders); `RigParts` keeps only the CPU rows. The end portal's wgpu material is gone, so end portals and gateways draw nothing until `vk` gets their shader.
6. Replace screenshots and GUI images (section 6) with Torch's own image types. Then drop `bevy_render`, `bevy_pbr`, `bevy_core_pipeline` and wgpu.
