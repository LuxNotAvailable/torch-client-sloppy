# Android specifics: where the phone build should differ

The renderer was built like a desktop GPU-driven renderer and then ported.
Phones (tile-based GPUs: PowerVR, Mali, Adreno) favour different methods.
This file lists where the Android build should take its own path, why,
and how to test each change. Measurements: `docs/ANDROID_PERF.md`.

## What the Pixel 10 Pro shows

- Terrain is **geometry-bound**: ~6.5–7.5 ms GPU whatever the Render Scale
  (100 / 70 / 50% cost the same). Cost follows vertices and how they're
  fetched, not pixels.
- CPU `renderer system` takes **~7–9 ms**: half of a 60 fps frame before
  the GPU starts.
- The Hi-Z build costs **~2.2 ms**, and it forces depth to be stored to
  memory, which a tiler would otherwise keep on chip and discard.

## Likely causes, most likely first

1. **Vertex pulling.** The terrain shader reads packed quads from bindless
   storage buffers and unpacks them itself. Desktop GPUs handle that
   well; mobile GPUs are often much slower at it than fixed-function
   vertex input, which is prefetched and cached. It fits a
   geometry-bound cost that ignores resolution.
2. **Compute between graphics passes.** The cull compute, the Hi-Z build
   and the draw-count readback make a tiler flush its tiles and stall
   between passes. Bedrock culls on the CPU and draws each chunk's buffer
   directly: no compute, no indirect draws, no pyramid.
3. **CPU overhead from Bevy.** ECS schedules, change detection, the mesh
   bridge (`draw/bridge.rs`) and per-frame scene syncs add up. Past
   examples of this class: the entity mesh table that grew all session,
   beacon beams rebuilt every frame, particle batch meshes built and never
   used.
4. **Culling gaps.** Sections beside the camera skipped occlusion until
   `972672a`. More gaps like it may exist.
5. **Thermals and pacing.** Nothing adapts yet; clocks drop after minutes
   and the frame rate sags with them.

## Planned Android path

### 1. CPU-culled terrain (test first)

A terrain mode for phones:

- fixed-function vertex input (a vertex buffer and attribute layout, not
  storage-buffer pulling);
- culling on the CPU: frustum plus the cave visibility graph, which
  already runs on the CPU (`terrain/cave.rs`);
- one plain indexed draw per visible section (or per bucket), no
  `DrawIndexedIndirectCount`;
- no Hi-Z pyramid, so depth can stay transient (`storeOp = DONT_CARE`,
  lazily allocated).

Keep the GPU-driven path on desktop. Make the mode switchable, so one run
can A/B both.

**Test:** in-run A/B on the Pixel, same scene and camera (`vkview`
`--camera game`, or the game with the toggle), comparing terrain GPU ms
and CPU frame ms. If terrain drops clearly, causes 1 and 2 are confirmed
and phones default to this path.

### 2. A CPU profile of one phone frame

Find where the 7–9 ms goes. Builds aren't debuggable, so write
`profiles/` to external files (`/sdcard/Android/data/com.torch.client/files`)
or build a debuggable APK, then trace with Perfetto or simpleperf.
Targets: Bevy schedule overhead, bridge copies, scene syncs, per-frame
allocations.

### 3. Tile-friendly frame

- Depth transient when Hi-Z is off.
- `loadOp = CLEAR` / `DONT_CARE` everywhere; nothing stored that isn't
  read later.
- Rotation, upscale, FXAA and post merged into one full-screen pass.

### 4. Thermal adaptation

Read `AThermal_getThermalHeadroom`. Lower Render Scale or render distance
before the device throttles, and pace with FIFO at a steady 60 or 30
(Swappy or `VK_GOOGLE_display_timing`): a steady 30 beats a jittery 45.

## Rules for Android changes

- Build game APKs with `--features mobile_ui` (touch controls).
- Measure with in-run A/B tests; separate runs are too noisy.
- Leave out stretches at 30 fps in logs: a menu was open (GUI frame cap).
- Record results in `docs/ANDROID_PERF.md`.
