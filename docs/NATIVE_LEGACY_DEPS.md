# Native renderer: what it still takes from the legacy renderer

Reference for 10.4 stage 2–3. Snapshot 2026-11-18 (after b31a5a9). Line numbers drift: search for the symbol, and use the `±` range around it.

Format: **symbol** · defined at `file:line` · used at `file:line` (vk side).

## 1. Hooks the other way (legacy code calls into `vk`)

These keep the native renderer fed. They must move to the game-side code that survives.

| What | Caller (legacy) | Callee (vk) |
|---|---|---|
| Terrain sections, after meshing | `renderer/systems.rs:193` in `ChunkIndex::store_section` (182–245) | `terrain_dump::section` |
| Terrain cleared | `renderer/systems.rs:297` in `ChunkIndex::clear` (295–308) | `terrain_dump::cleared` |
| Column dropped | `renderer/systems.rs:311` in `ChunkIndex::drop_column` (309–322) | `terrain_dump::column_dropped` |
| Block atlas texels | `renderer/systems.rs:369` in `setup` (354–420) | `terrain_dump::atlas` |
| Entity/env/particle capture plugin | `renderer/entity_material.rs:314` in `EntityMaterialPlugin::build` (303–320) | `entity_dump::plugin` |
| Live renderer plugin | `renderer/systems.rs:2604` in `run` (2533–2700) | `live::LivePlugin` |
| Backend choice | `renderer/systems.rs:2357` `wgpu_settings` (2347–2378), `:2386` `resolve_backend` (2379–2420) | `live::wanted` |
| Pack compile switch | `renderer/packdraw/mod.rs:248` `native_renderer` (246–250) | `live::wanted` |
| Pack changed | `gui/shaderpacks.rs:147, 157, 449` | `live::pack_changed` |
| Entity dump gate | `entities/mod.rs:915` | `terrain_dump::folder` |

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

| Data | Defined | Used (vk) |
|---|---|---|
| `systems::WorldCamera` + `GlobalTransform` + `Projection::Perspective` | `renderer/systems.rs:44` | `live.rs:697–715` `world_camera`; `entity_dump.rs:976, 983, 1450` |
| `DistanceFog` (`FogFalloff::Linear`) on the world camera | Bevy (`bevy_pbr`) | `entity_dump.rs:907, 983` |
| `panorama::PanoramaCamera` + `Camera::is_active` (menu?) | `renderer/panorama.rs:29` | `live.rs:461–466` |
| `panorama` lens constants (copied) | `renderer/panorama.rs` | `vk/panorama.rs:18–25` |
| `inventory_figure::{FigureCamera, FigureImage, ready, FIGURE_LAYER}` + ortho `ScalingMode::Fixed` | `renderer/inventory_figure.rs:19, 106, 112, 117` | `live.rs:662–695` `figure_view`; `entity_dump.rs:1189–1199` |
| `hand::{HandCamera, HandGlint, HAND_LAYER}` | `renderer/hand.rs:16, 832, 848` | `entity_dump.rs:201–220, 786, 987, 994–995` |
| `frame_view::FrameView`, `view_bob::current_warp` | `renderer/frame_view.rs:14`; `renderer/view_bob.rs:134` | `entity_dump.rs:989, 1223` |
| `antialias::{AaMode, SSAA_SCALES}` (GUI option) | `renderer/antialias.rs:21, 78` | `live.rs:485–495` |
| `post_effects::active()` (server flags) | `renderer/post_effects.rs:28` | `entity_dump.rs:959`; carried in `vk/env.rs:57` |

## 4. Entities, sky, overlays (`entity_dump.rs`, from Bevy meshes)

The whole capture reads Bevy render components: `Mesh3d`, `MeshMaterial3d<_>`, `GlobalTransform`, `InheritedVisibility`, `RenderLayers`, `MeshTag`, `Assets<Mesh>`, `Assets<Image>`, `AssetEvent`. That is the biggest thing to replace.

| Data | Defined | Used (vk) |
|---|---|---|
| Mesh/image cache from `AssetEvent<Mesh/Image>` | Bevy | `entity_dump.rs:459–545` `Cache`, `capture` (545–600), `mesh_data` (490–528), `image_data` (530–543), `repeats` (472–481) |
| `entity_material::EntityMaterial` (per-entity params) | `renderer/entity_material.rs:124, 144` | `entity_dump.rs:44, 603–617` `Drawn`, `:974` |
| `entity_material::GLINT_ITEM` (glint image handle) | `renderer/entity_material.rs:247` | `entity_dump.rs:882`; `live.rs:724` |
| `rig_parts::{RigParts, ATTRIBUTE_PART}` (GPU pose rows, part attribute) | `renderer/rig_parts.rs:21, 56` | `entity_dump.rs:516, 975, 1197` |
| `player_model::LocalRigRoot` | `renderer/player_model.rs:1997` | `entity_dump.rs:1001` |
| `tethers::WeatherLayer` | `renderer/tethers/mod.rs:196` | `entity_dump.rs:615` |
| `blockentities::{BeRigNode, BeamNode}`, `particles::draw::ParticleBatch` (tags) | `src/blockentities`, `src/util/particles/draw.rs` | `entity_dump.rs:611–616` |
| `sky::{SkyPart, SkyPartMaterial, SKY_SCALE}` | `renderer/sky.rs:16, 39, 194` | `entity_dump.rs:164, 620–627, 651, 980` |
| `clouds::CloudLayer` + `StandardMaterial` | `renderer/clouds.rs:140` | `entity_dump.rs:629–633, 650, 979` |
| `screen_effects::Overlay` (screen overlays) | `renderer/screen_effects.rs:36` | `entity_dump.rs:167–175, 656, 986` |
| `overlays::{BlockOutlineMarker, BreakOverlayMarker, HitboxMarker, ChunkBorderMarker, EspMarker, OUTLINE_WIDTH}` | `renderer/overlays.rs:21, 36, 67, 77, 92, 583` | `entity_dump.rs:176–199, 837` |
| `glint::GlintMaterial` | `renderer/glint.rs:10, 45` | `entity_dump.rs:204, 666, 996` |
| `weather::{ASSETS, SHEETS, NATIVE}` | `renderer/weather.rs:155–161` | `entity_dump.rs:808–812`; `live.rs:618` |
| `screenshot::request_numbered` (vkview reference frame) | `renderer/screenshot.rs:16` | `entity_dump.rs:1208` |
| Particles: `util::particles::{LiveParticles, ParticleAtlas}`, `live_sprites` | `src/util/particles` | `entity_dump.rs:1443–1460` `write_particles` |
| `write` system (all of the above) | — | `entity_dump.rs:968–1235`; `write_env` 646–966 |

## 5. Shader packs (`vk/pack/*`)

The pack front end lives in `renderer/packdraw`; the native side compiles and runs what it produces. It keeps all of it, but it must move out of `renderer/`.

| Data | Defined | Used (vk) |
|---|---|---|
| `packdraw::compile::{compile_frame, with_vulkan_terrain}` | `renderer/packdraw/compile/mod.rs:197`; `compile/preludes.rs:10` | `pack/mod.rs:25–26`; `pack/bindless.rs:494–499` |
| `packdraw::compile::{EntityKind, ScreenStage, Step, geometry_alpha}` | `compile/kinds.rs:54, 108`; `compile/mod.rs:112, 142` | `pack/screen.rs:16, 755` |
| `packdraw::install::{install, Installed, InstalledProgram, Module}` | `renderer/packdraw/install.rs:32, 37, 57, 107` | `pack/mod.rs:18`; `pack/screen.rs:17, 788`; `pack/targets.rs:163` |
| `packdraw::sources::{Depth, Stub, TextureSource}` | `renderer/packdraw/sources.rs:6, 12, 22` | `pack/screen.rs:18` |
| `packdraw::uniforms::{BlockSpec, FrameValues, History, UniformBytes, MatrixSet}` | `uniforms/mod.rs:32, 69, 81`; `uniforms/values.rs:64, 77` | `pack/screen.rs:19, 937` |
| `packdraw::usage::mip_targets` | `renderer/packdraw/usage.rs:166` | `pack/targets.rs:171` |
| `packdraw::inputs::Inputs` (+ `from_json`) | `renderer/packdraw/inputs.rs:85, 167` | `pack/screen.rs:156`; `live.rs:179, 908`; `viewer.rs:457`; `entity_dump.rs:1451` |
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

1. Move the shared data types out of `renderer/`. Done for `mesh`, `visgraph`, the `occlusion` flood fill, `block_anim` data, `packvertex` and `dimension` (and `timeline`, which `dimension` needs). Still to do: the `packdraw` front end. It isn't a plain move: it uses wgpu types (`BlendState`, `TextureViewDimension`), `RenderDevice` buffers in `uniforms`, `terrain_pool` constants, `images::CustomTexture`, and `inputs` reads the Bevy ECS (step 3).
2. Move section 1's hooks into the mesher and asset loading, so `terrain_dump` no longer depends on `ChunkIndex` or `setup`.
3. Replace the cameras (section 3) with a plain game-side `View` resource: eye, rotation, fov, fog, hand/figure matrices.
4. Replace `entity_dump` capture (section 4): the game hands draw lists to the native renderer directly instead of spawning Bevy meshes. This is the biggest step.
5. Replace screenshots and GUI images (section 6) with Torch's own image types. Then drop `bevy_render`, `bevy_pbr`, `bevy_core_pipeline` and wgpu.
