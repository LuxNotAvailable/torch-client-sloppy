# Android vs PC: every difference between the builds

The reference for agents: what the Android build does differently from the
desktop (PC) build, and where. Keep the two from leaking into each other.

**Rules**
- A change that adds, removes or changes an Android-only path, default or
  feature updates this file in the same commit.
- Android-only behaviour goes behind `cfg(target_os = "android")`, the
  `mobile_ui` feature, or a runtime mode that is off on PC. Never change a
  shared default to suit the phone.
- Mobile render paths live in their own files (`src/vk/terrain_fixed.rs`,
  `shaders/terrain_mobile.slang`), so PC code stays readable without them.
- Why the phone differs, and what to try next: `docs/ANDROID_SPECIFICS.md`.
  Measurements: `docs/ANDROID_PERF.md`.
- Android findings that could help PC go in `docs/PC_CANDIDATES.md`, not
  into PC code, until asked.

## Build and packaging

| | PC | Android |
|---|---|---|
| Entry | `main.rs` → `start_from_cli` | `android_main` in `lib.rs` (`vkview_apk`: the viewer instead) |
| Bevy features | `x11`, `wayland` | `android-native-activity` |
| Features | defaults | `--features mobile_ui` (cargo-apk ignores metadata features) |
| Sound | rodio through cpal's desktop backend | rodio through cpal's Oboe backend (AAudio); Oboe's static libc++ needs `libc++abi` linked (`build.rs`); rodio is also listed in the Android dependency table |
| Voice chat | `voice` | off (`build.rs`; no microphone permission) |
| Resource packs | `resourcepacks` | off (`build.rs`) |
| Assets | download / folder | packed into the APK (`cfg(packed_assets)`, `platform::assets::archive`) |
| Allocator | mimalloc (or `tracked`) | system |
| TLS | rustls default roots | `android_tls_config` (webpki roots) |
| Profile | `release` / `linux` | `android` (release, stripped) |
| Built-in shader pack | compiled plain | compiled with `-D MC_OS_ANDROID` |
| Pack OS macro | `MC_OS_WINDOWS` etc. | `MC_OS_ANDROID` (`shaderpack/features.rs`) |

## Platform

| | PC | Android |
|---|---|---|
| Env switches | process env | `env.txt` in the APK, then one pushed to `/sdcard/Android/data/com.torch.client/files/` (wins). Read with `platform::env::var`, never `std::env::var` |
| Launch target | CLI args | `launch.txt` (address, username): a pushed one in the external files folder wins over the APK's |
| Bench app | — | `tools/bench-apk` (`com.torch.bench`): the game beside the real install, for `run.sh` benches |
| Storage, assets root | `%LOCALAPPDATA%` / temp | the package's data dir (`ANDROID_APP`) |
| Pipeline cache | temp dir; saved on exit | `storage::dir()/native-pipelines.bin`; also saved once a pack is set up and on pause (`live.rs`), since an Android app is rarely closed cleanly: the built-in pack's setup is ~4.6 s cold, 0.2 s warm |
| Window | resizable, Fullscreen and size options | native size tracked each frame (`track_native_window_size`); no Fullscreen or window-size options |
| Present mode | `AutoVsync` | `AutoNoVsync` |
| Render thread | pipelined above ~200 fps, in step below | always pipelined (`draw_frame`); `MC_NATIVE_PIPELINE` (now read through `platform::env`) overrides both |
| Cursor | grabbed / shown | no-op |
| Input, GUI | mouse and keyboard | touch controls and phone layout (`mobile_ui`: chat, HUD, screens, options, slots, keyboard) |
| Exit | normal | `Activity.finish()` through JNI |
| Background | - | sound stops and its stream closes on suspend (`audio::suspend`, `engine::Command::Suspend`), reopens on resume (`platform::notify`) |
| Diagnostics | — | `log_frame_rate`; logcat; `/proc` CPU and memory metrics (also Linux); `smaps_rollup` read on its own thread (`diag::runtime`) |
| Draw-count readbacks | yes | vkview only (`terrain::STATS_READBACK`) |

## Renderer

| | PC | Android |
|---|---|---|
| Terrain pools | 1.6 M vertices, 3.2 M indices | 1.2 M vertices, 2.4 M indices |
| Pool memory | Per-Face Culling's filtered indices made with each pool | a 4-byte stand-in until Per-Face Culling is first on (`grow_filtered`; ~9 MB a pool, 430 MB at RD 32); vertex ranges rounded to whole quads |
| Terrain cull | loads the whole slot; one thread per slot | `CULL_LAZY`: ranges and band words only for kept slots; `CULL_LIST`: one thread per candidate from `cull_list` (CPU: cave-visible and in the cull frustum, plus every translucent layer), written to a host-visible buffer per frame in flight. Same commands; near-first is off |
| Hi-Z | every frame | every second frame (`HIZ_EVERY_OTHER`); level 1 from one `GatherRed` per 2 × 2 (`hiz_reduce` params.y, same values; vkview Hi-Z 2.2 -> 1.24 ms) |
| Detail atlas tiles | up to 32 px | `TILE_PX` |
| Terrain bookkeeping (`vk::terrain::MOBILE`) | every remesh re-runs the cave flood fill (every 10 frames) and re-ranks translucent layers; the translucent re-sort scans every layer each frame | a remesh that keeps its cave visibility keeps its visible bit and rank; the re-sort scans only on a new eye block or a budget-cut pass; the flood fill indexes dense arrays on its grid instead of hashing (`fill_dense`, ~40 -> ~10 ms per fill at RD 32). Same output |
| Turned display | none (no pre-rotation) | without a pack every pass draws turned straight into the swapchain (`PreRotation`; the GUI through `gui.slang`'s view_proj, in y-up clip space; screenshots turned back on read, `upright_pixels`). Under a pack, drawn upright and turned by one pass (`Live::upright`). `MC_PACK_AB=turn` A/Bs it |
| Terrain path | GPU-driven, packed + greedy, pulled vertices | GPU-driven, **vertex input** (`FixedMode::Input`): plain vertices, packing and greedy off (`live::terrain_options`). `MC_NATIVE_TERRAIN=pulled` restores the PC path |

### Fixed terrain (mobile path, under test)

Switched by `MC_NATIVE_TERRAIN=fixed` (or `fixed_ab`) in the game, or
`vkview --fixed` / `--fixed-ab`. `input` / `input_ab` (`--input`,
`--input-ab`) keep the GPU cull and Hi-Z and only draw solid and cutout
through the fixed vertex input; translucent stays pulled (its one indirect
draw spans pools), and the pool buffers use `Use::VertexInputRead` in both
passes so no barrier splits them. `input` is the Android game default
(vkview takes the flags explicitly); the rest are off on both builds.
Pixel results in `ANDROID_PERF.md` (`input` wins ~5% of frame).

| | GPU-driven (PC) | Fixed (mobile) |
|---|---|---|
| Vertices | 20-byte `PackedVertex` pulled from a storage buffer by bindless index; packed and greedy-merged records | the same vertex through fixed-function input (binding 0, stride 20; `terrain_fixed::ATTRIBUTES`) |
| Packing, greedy | on | off (needs plain vertices) |
| Vertex shader | `terrain_vertex` (`shaders/terrain.slang`) | `terrain_vertex_fixed` (`shaders/terrain_mobile.slang`) |
| Vertex-to-fragment | `VertexOut`: uv, colour (4), world, slot, flags, merged (15 values) | `MobileOut`: uv, colour (3), world, flags (9 values); `*_mobile` fragment entries widen it and run the shared `terrain_shade`. The texture-index debug view colours every slot alike |
| Culling | compute: caves, frustum, Hi-Z, face buckets and bands, cutout rings | CPU (`terrain_fixed::cull`): caves, frustum, solid axis buckets, torch reach |
| Draws | one `DrawIndexedIndirectCount` per pool and kind | one `vkCmdDrawIndexed` per bucket run, first instance = slot |
| Translucent | one indirect draw, ranked far to near | direct draws in the same rank order |
| Hi-Z, counts readback, reset counts, face filter | yes | none |
| Pool vertex buffers | `STORAGE_BUFFER` | also `VERTEX_BUFFER` (only when the mode is on) |
| Falls back to GPU-driven | — | under a shader pack (`cull_only`) and in the overdraw view |

`fixed_ab` flips every two seconds: A GPU-driven, B fixed, both on plain
vertices, so the A/B measures vertex fetch plus culling, not packing.

## Shader packs

| | PC | Android |
|---|---|---|
| Packs offered | the built-in one and the shaders folder's | the built-in one only (`shaderpack::discover::EXTERNAL`, `allowed`); a saved outside pack runs as none; no Open Shader Pack Folder button |
| Pack quads (`terrain pack quads`) | 16 bytes per vertex slot (quad q at the section's first vertex + q) | 16 bytes per quad, at vertex / 4 (`vk::terrain::QUAD_UNITS`; `iris_vk_quad` under `MC_OS_ANDROID`): a quarter of the memory (861 -> 215 MB at RD 32) |
| colortex2 (texel target) | RGBA16 (UNORM, the words / 65535) | RGBA16UI: the words as integers (`TexelWords`, `texel_at`, `iris_uint(colortex2)` over a `Texture2D<uint4>` alias of the bindless array). 8 bytes where an exact float format needs 16; RGBA16 UNORM fails `usable` there. Precompiled programs may draw to integer targets (`compile::compile_frame`); integer attachments never blend (`pipeline.rs`) |
| colortex6 (reflection claim) | RGBA32F: the key's high and low 16 bits, alpha, 1 | RGBA16F: key - 1 in 11, 11 and 10 bits, 1 + alpha (`refl_claim_pixel` / `refl_claim_key` / `refl_claim_alpha` in `lib/water.slang`) |
| Texel cache | on | off (the PowerVR driver aborts on the depth texture its compute passes read); `deferred` looks shadows up per pixel |
| Shadow cascade culls | one thread per slot, whole slots loaded | `CULL_LIST` + `CULL_LAZY` over a CPU list per cascade (`shadow_list`: live sections inside the cascade's sides; a translucent one left out keeps its zeroed command) |
| Occlusion culling under a pack | none (the terrain only culls, so no pyramid; `pyramid_view` cleared) | the pack's opaque depth (depthtex1) builds the pyramid for the next cull, hand pixels masked out by colortex2's hand flag (alpha word 2) as open sky: `PackScreen::hiz_source` (`HizSource`), `TerrainScene::pack_pyramid`, `hiz_reduce_pack` (0..1 GL depth to reversed, `params.z - params.w * d`, rows flipped, one gather per 2 x 2 plus one `GatherAlpha` of the texel target; its clear reads as open sky). Half-Rate Occlusion applies; a scaled pack's pyramid is accepted at its own size. Assumes the built-in pack's colortex2 layout |
| depthtex2 (depth before the hand) | made for every pack, copied each frame | not made (`PackTargets::new(.., no_hand = false)`): the built-in pack never samples it; a program that did would read the stub |
| gbuffers_water depth test | after the shader (its reflection claims are side effects) | before it (`[earlydepthstencil]` under `MC_OS_ANDROID`): hidden water isn't shaded. An alpha-0 texel would still write depth (vanilla's translucent textures have none) |
| Pack render passes | each pass alone, LOAD and STORE, clears as transfers | `vk::pack::tiled`: consecutive draw passes into the same attachments share a render pass; clears become CLEAR at first use (late transfer clears when read first); full-screen unblended passes without discard don't load; images nothing later uses (and not kept across frames) aren't stored. The shadow depths' clears wait too: shadowtex0's joins the shadow pass, shadowtex1's is dropped when the shadow depth copy overwrites it. Pixel-identical (desktop, `MC_PACK_TILED=1`) |

PowerVR (Pixel 10 Pro) limits the Android pack must keep:
- A colour image over 4 bytes a pixel is padded to power-of-two sides:
  2856 x 1280 RGBA16F takes 4096 x 2048 (64 MiB, 2.3x). 4-byte formats
  and depth are tight.
- A pass whose colour outputs pass 32 bytes a pixel fails at full
  resolution: present returns `ERROR_OUT_OF_HOST_MEMORY` and logcat shows
  `DMABUFHEAPS: Unable to allocate from DMA-BUF heap: vframe-secure`
  (gbuffers_water at 40 bytes did; 32 runs). Keep every pass at 32 or
  under.

Both builds: a pack draws no terrain on a frame whose cull didn't run
(`TerrainScene::culled`, e.g. while the terrain pipelines compile), since
its commands would be stale or never written. Pack targets take the
upright frame's size (`live::update_pack`): on a turned display the
swapchain is portrait, and portrait targets stretched the picture.

## Options

| Option | PC | Android |
|---|---|---|
| Half-Rate Occlusion | off | on |
| Greedy Meshing | Video Settings | hidden: no packed records to merge (tuner skips it too) |
| Near-First | Advanced Performance | hidden: the cull is never near-first (tuner skips it too) |
| Antialiasing | Off, FXAA, MSAA, Supersampling | no Supersampling (`antialias::SUPERSAMPLE`); a saved `ssaa` reads as FXAA |
| Shader packs | any | the built-in one |
| Window Size, Fullscreen, Open Client Folder | shown | hidden |
| Music & Sounds | volumes and a Speaker button | volumes only: the system picks the speaker and follows headphones; a saved speaker is ignored |
