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

## Options (rough order of payoff)

- **Render scale**: draw the world at 50–75% and upscale in the rotation pass, which is free since that pass samples anyway. Make it dynamic from frame time or thermal headroom (`AThermal_getThermalHeadroom`).
- **Merge passes at the end**: rotation + upscale + FXAA + post in one full-screen pass.
- **Tile-friendly attachments**: depth `LAZILY_ALLOCATED`/transient, `storeOp = DONT_CARE`, `loadOp = CLEAR`/`DONT_CARE` everywhere. No depth store unless Hi-Z reads it.
- **Hi-Z on mobile**: off, or built from a ¼-res depth every 2nd frame. PowerVR HSR already removes opaque overdraw, so measure cull savings against the 2.2 ms.
- **Opaque first, cutout last**: keep HSR effective. Fast leaves (opaque), and cutout only near.
- **Fewer triangles**: greedy meshing (3.9), far LOD (11.1). Tiler cost scales with geometry.
- **Half precision**: `half`/RelaxedPrecision in fragment shaders (lighting, fog, atlas math).
- **ASTC atlas**: cuts texture bandwidth roughly 4–8×.
- **Shading rate**: `VK_KHR_fragment_shading_rate` 2×2 on far terrain and sky, if the device offers it.
- **Frame pacing**: FIFO at 60 or 30, with Swappy or `VK_GOOGLE_display_timing`. Steady 30 beats jittery 45.
- **CPU and power**: reuse command buffers for static passes, fewer barriers, ADPF performance hints (`APerformanceHintManager`).

## Measure first

- In-game GPU timing on Android: `MC_NATIVE_GPU_TIMING` can't reach the APK. Add an option or debug toggle.
- Profiles are unreadable: the APK isn't debuggable. Write `profiles/` to external files (`/sdcard/Android/data/com.torch.client/files`), or build debuggable.
- Arm/IMG tooling: PVRTune for tiler, HSR and bandwidth counters.
