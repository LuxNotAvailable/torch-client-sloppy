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

## Build and packaging

| | PC | Android |
|---|---|---|
| Entry | `main.rs` → `start_from_cli` | `android_main` in `lib.rs` (`vkview_apk`: the viewer instead) |
| Bevy features | `x11`, `wayland` | `android-native-activity` |
| Features | defaults | `--features mobile_ui` (cargo-apk ignores metadata features) |
| Sound, voice | `audio`, `voice` | off (`build.rs`: no rodio backend) |
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
| Pipeline cache | temp dir | `storage::dir()/native-pipelines.bin` |
| Window | resizable, Fullscreen and size options | native size tracked each frame (`track_native_window_size`); no Fullscreen or window-size options |
| Present mode | `AutoVsync` | `AutoNoVsync` |
| Cursor | grabbed / shown | no-op |
| Input, GUI | mouse and keyboard | touch controls and phone layout (`mobile_ui`: chat, HUD, screens, options, slots, keyboard) |
| Exit | normal | `Activity.finish()` through JNI |
| Diagnostics | — | `log_frame_rate`; logcat; `/proc` CPU and memory metrics (also Linux); `smaps_rollup` read on its own thread (`diag::runtime`) |
| Draw-count readbacks | yes | vkview only (`terrain::STATS_READBACK`) |

## Renderer

| | PC | Android |
|---|---|---|
| Terrain pools | 1.6 M vertices, 3.2 M indices | 1.2 M vertices, 2.4 M indices |
| Terrain cull | loads the whole slot | `CULL_LAZY`: ranges and band words only for kept slots (same commands) |
| Hi-Z | every frame | every second frame (`HIZ_EVERY_OTHER`); level 1 from one `GatherRed` per 2 × 2 (`hiz_reduce` params.y, same values; vkview Hi-Z 2.2 -> 1.24 ms) |
| Detail atlas tiles | up to 32 px | `TILE_PX` |
| Terrain bookkeeping (`vk::terrain::MOBILE`) | every remesh re-runs the cave flood fill (every 10 frames) and re-ranks translucent layers; the translucent re-sort scans every layer each frame | a remesh that keeps its cave visibility keeps its visible bit and rank; the re-sort scans only on a new eye block or a budget-cut pass. Same output |
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

## Option defaults

| Option | PC | Android |
|---|---|---|
| Half-Rate Occlusion | off | on |
| Greedy Meshing | as set | no effect (packing off under vertex input) |
