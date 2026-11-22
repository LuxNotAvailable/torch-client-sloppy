# Android performance (Pixel 10 Pro, PowerVR DXT-48-1536)

Native renderer, 2026-11-18. Data: game logcat (`native renderer: … ms/frame`), vkview terrain dump on the Pixel.

## Measured

| What | Value |
|---|---|
| Game, RD 6, cap 60 | 30–59 fps; dips to 30–43 with CPU flat |
| Render thread CPU | ~6 ms: record+submit 4.5, submit+present 2.4, terrain record 1.3, renew scenes 1.0, env 0.9 |
| GPU, vkview terrain, 2856×1280 | terrain 8.4 ms, Hi-Z levels 1–6 2.2, translucent 1.1, cull 0.3 |
| Fixed draw counts (no `drawIndirectCount`) | +1.1 ms GPU (75 vs 82 fps) |

CPU fits 60 fps. Dips with flat CPU mean the **GPU is the limit**: full native res, Hi-Z compute, the extra rotation pass.

## Bottlenecks

1. **Full native resolution**: 3.66 MP every frame. Fill and bandwidth dominate on a tiler.
2. **Hi-Z build**: 2.2 ms of compute. It also forces depth to be stored to memory, where a tiler could discard it on chip.
3. **Pre-rotation pass**: one extra full-screen write and read (upright image, then swapchain).
4. **Cutout (alpha test)**: `discard` turns off PowerVR's hidden-surface removal for leaves and grass, so overdraw costs full price.
5. **Compute between graphics passes**: cull, Hi-Z and counts readback make the tiler flush and stall.
6. **Thermals**: no adaptation. Clocks drop after minutes, and so does fps.

## Measured 2026-11-21 (in-run A/B, steady phases only)

- **Occlusion culling**: off costs terrain +4.6 ms against Hi-Z's 2.5 ms; keep it.
- **Render Scale 100/70/50%**: GPU 13.2-14.4 / 13.5-14.0 / 13.6-14.3 ms. Terrain
  stays ~7.5 ms at every scale: it is geometry-bound (vertices, tile binning),
  not fill-bound. Hi-Z gets 1.2-1.8 ms cheaper, the post upscale costs 0.8-1.0 ms more.
- **Leaves Fancy vs Fast**: terrain 6.59 vs 6.33-6.50 ms; cutout isn't the cost.
- **Near-first terrain** (cull walks sections nearest first; `vkview
  --near-first-ab`, `dump5`, `--camera game`, off / on): Pixel terrain
  2.21 / 2.37 and 2.16 / 2.18 ms, cull 0.42 / 0.49 ms. MX250 laptop terrain
  3.15 / 3.17 ms, cull 0.175 / 0.206 ms (0.255 before sorting by pool first).
  No gain: HSR and Hi-Z already drop what it would save, and the cull's
  scattered slot reads cost more. Kept as Advanced Performance > Near-First,
  off by default.
- **Hi-Z every other frame** (`vkview --hiz-ab`, `dump5`, `--camera game`,
  every frame / every second): Pixel GPU total 5.55 / 4.31 ms, Hi-Z
  2.16 / 1.06 ms, terrain 2.20 / 2.20 ms. MX250 laptop total 4.47-4.51 /
  4.49-4.50 ms (its Hi-Z is only 0.25 ms). Static camera: pop-in while
  turning is unmeasured. Advanced Performance > Half-Rate Occlusion,
  on by default on Android only.
- **Quarter-res Hi-Z** (level 1 from one depth texel per 2 x 2; `vkview
  --hiz-quarter-ab`, every-frame builds, full / quarter): Pixel Hi-Z
  2.08 / 1.30 ms, terrain 5.20 / 5.23 ms (no visible over-cull, static
  camera). MX250 laptop Hi-Z 0.26 / 0.15 ms, total unchanged. Not
  conservative (1 px gaps), so Advanced Performance > Quarter-Res Occlusion
  ships off.

## Measured 2026-11-22: fixed terrain and vertex input

`vkview`, `dump5`, camera at the dump's centre (`--camera
-40,80,296,0,-10`; bounds -256,-64,80 to 176,102,512, printed by vkview),
2856x1280, 3600 frames. GPU ms per frame.

In-run A/Bs, both sides on plain vertices (packing off):

| A/B | A terrain | B terrain | A total | B total |
|---|---|---|---|---|
| `--input-ab` (A pulled, B vertex input; both GPU cull + Hi-Z) | 3.39 | 3.01 | 6.06 | 5.45 |
| `--fixed-ab` (A GPU-driven, B CPU cull, no Hi-Z) | 3.43 | 5.43 | 6.16 | 6.08 |

Separate runs against the shipped default (packed + greedy), alternated:

| Run | terrain | frame avg |
|---|---|---|
| default | 3.37, 3.38 | 5.91, 6.03 |
| `--input` | 3.01, 3.24 | 5.54, 5.76 |

- Vertex input under the GPU cull: terrain -11%, frame -10% in-run; about
  -5% frame against the packed default. The pool buffers must be one
  `Use::VertexInputRead` across both passes: a `VertexInput` then
  `VertexRead` barrier before the translucent pass cost +2.4 ms.
- Fixed (CPU cull, no Hi-Z) ties on total: terrain +2 ms, offset by no
  Hi-Z build, cull or readback. Its CPU side is unmeasured here.
- An earlier run from z = 0 (outside the dump, looking over the edge) gave
  fixed 18.1 vs 10.5 ms; that camera saw far more geometry. Use the centre.

## Game bench (2026-11-22, `tools/bench-apk/run.sh`)

The game as `com.torch.bench` against a local vanilla 26.3 server
(`target/bench-server`: offline, creative, seed -4172144997902289642,
view distance 32) through `adb reverse`; RD 32, uncapped, standing at
spawn, steady state after chunks load (~35.7k section layers, 47 pools).

| Step | fps | renderer system (CPU) | terrain record | GPU |
|---|---|---|---|---|
| Baseline (vertex input default) | ~55 | 12.9 | 6.9 (rank+order 4.3, sort translucent 1.6) | 14.7 |
| `MOBILE` terrain bookkeeping | ~68 | 8.1 | 1.0 | 13.4 |
| Hi-Z level 1 by gathers | ~76 | 7.0 | 1.0 | 11.75 (Hi-Z 2.6 -> 1.4) |

Baseline cause: the world remeshes ~35 sections a second while standing
still; each remesh re-ran the cave flood fill (~40 ms on the phone, up to
9 times per 2 s) and the translucent ranking (~4 ms), and the re-sort
scanned all 35k layers every frame.

GPU at 68 fps: terrain 6.1, Hi-Z 1-6 2.6, terrain cull 1.2, entity counts
readback 1.0, translucent 0.7, post 0.5.

## Options (rough order of payoff)

- **Render scale**: draw the world at 50–75% and upscale in the rotation pass, which is free since that pass samples anyway. Make it dynamic from frame time or thermal headroom (`AThermal_getThermalHeadroom`).
- **Merge passes at the end**: rotation + upscale + FXAA + post in one full-screen pass.
- **Tile-friendly attachments**: depth `LAZILY_ALLOCATED`/transient, `storeOp = DONT_CARE`, `loadOp = CLEAR`/`DONT_CARE` everywhere. No depth store unless Hi-Z reads it.
- **Hi-Z on mobile**: every 2nd frame and quarter-res are done (above). PowerVR HSR already removes opaque overdraw, so measure cull savings against the 2.2 ms.
- **Opaque first, cutout last**: keep HSR effective. Fast leaves (opaque), and cutout only near.
- **Fewer triangles**: greedy meshing (3.9), far LOD (11.1). Tiler cost scales with geometry.
- **Half precision**: `half`/RelaxedPrecision in fragment shaders (lighting, fog, atlas math).
- **ASTC atlas**: cuts texture bandwidth roughly 4–8×.
- **Shading rate**: `VK_KHR_fragment_shading_rate` 2×2 on far terrain and sky, if the device offers it.
- **Frame pacing**: FIFO at 60 or 30, with Swappy or `VK_GOOGLE_display_timing`. Steady 30 beats jittery 45.
- **CPU and power**: reuse command buffers for static passes, fewer barriers, ADPF performance hints (`APerformanceHintManager`).

## Measure first

- In-game GPU timing on Android: done. Push an `env.txt` holding `MC_NATIVE_GPU_TIMING=1` to `/sdcard/Android/data/com.torch.client/files/`, restart the app, and grep logcat for `native renderer: gpu`. Pushed values override the APK's bundled `env.txt`.
- Profiles are unreadable: the APK isn't debuggable. Write `profiles/` to external files (`/sdcard/Android/data/com.torch.client/files`), or build debuggable.
- Arm/IMG tooling: PVRTune for tiler, HSR and bandwidth counters.
