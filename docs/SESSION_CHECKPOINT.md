# Session checkpoint: Android build, sound, built-in pack (2026-11-22)

For the next session or agent. Rules that still hold:
- The user gave full control of the Android build: as fast as possible on
  the Pixel 10 Pro (PowerVR DXT-48-1536), looking identical to PC.
- Keep Android and PC separate:
  - Android-only code sits behind `cfg(target_os = "android")`, `MOBILE`
    or `MC_OS_ANDROID`.
  - Every difference goes in `docs/ANDROID_VS_PC.md`, same commit.
  - Findings that could help PC go in `docs/PC_CANDIDATES.md` only. Don't
    apply them to PC until the user asks.
- Only the built-in pack runs on Android, so pack-engine paths it never
  touches may be skipped there.
- Pushed to `main` (LuxNotAvailable fork). The working tree was clean at
  `4d73676`.

## Done this session

| Commit | What |
|---|---|
| 1b0670e and earlier | Android terrain path: vertex input, CULL_LIST, pipelined render thread, ... (55 -> 113 fps without a pack, RD 32) |
| 94f4420 | Android settings: Greedy Meshing and Near-First hidden (the tuner skips both); Supersampling removed (a saved `ssaa` reads as FXAA); only the built-in pack is offered or loaded, with no folder button. Pack stretch fixed (targets at the upright size). Pack crash fixed (gbuffers_water at 40 B/px; colortex6 is now RGBA16F with an 11/11/10 key encoding). Pack quads stored per quad (−645 MB). Per-Face Culling buffers made lazily (−430 MB). Both builds: no pack terrain draws on a frame without a cull |
| 0cbdd31, c6e15a7 | Sound on Android: rodio/cpal through Oboe (AAudio); `libc++abi` linked in `build.rs`; the stream closes on background and reopens on resume; Speaker button hidden; voice chat still off. The sound set (375 MB) downloads on first run |
| f3cd5c6 | Pack: occlusion from the pack's own depth (`hiz_reduce_pack`, `pack_pyramid`); early depth test on gbuffers_water; `vk::pack::tiled` (merged render passes, CLEAR/DONT_CARE ops; pixel-identical on desktop) |
| 1ab2f66 | colortex2 as RGBA16UI on Android (`TexelWords`, `texel_at`, `iris_uint(colortex2)`); integer attachments never blend; shadow cascade culls over CPU lists (`shadow_list`) |
| 4d73676 | Pack Hi-Z build uses one gather per 2x2 (2.3 -> 1.4 ms) |
| e257b4a | No pack: drawn turned straight into the swapchain, no rotation pass (GPU 8.88 -> 8.20 ms, ~116-122 fps). Pack: Hi-Z from depthtex1 with the hand masked, no depthtex2; shadow depth clears deferred; pipeline cache saved after pack setup and on pause (4.6 s -> 0.2 s). Both: shadow cascade count race fixed. Compositor rotation tried: much worse |

## Pack GPU per frame (Pixel, RD 32, standing at spawn)

| Step | GPU ms |
|---|---|
| First working pack | 95.6 |
| Early-Z water | 73.1 |
| Occlusion from the pack's depth | 68.5 -> 31.4 (in-run A/B) |
| Tiled passes | 32.6 -> 31.4 |
| Shadow cascade lists | 31.5 -> 31.35 |
| RGBA16UI texel target | terrain −7%, deferred −4%, −128 MiB |
| Hi-Z gather | about 30.7 |
| Hi-Z from depthtex1 + hand mask, shadow clears | about 30.6 |

Remaining, cool phone, side B (ms):
- terrain group 7.2 (about 4.6 is geometry, about 2 is fragment)
- deferred 5.6 (about 2 is reads and writes; about 3.5 is lighting, and the shadow taps aren't it)
- composite2 3.8 (fog resolve, sky colour per pixel)
- water 2.9
- composite 1.7
- final 1.3
- composite1 1.1
- entity cull 1.1
- Hi-Z 1.4 (every second frame)
- depth copies 0.9
- block animation copy 0.9
- post/rotation 0.7

The HUD showed 24–27 fps with the pack, against about 113 without.

## Tried and dropped (details in docs/ANDROID_PERF.md)

- **"37 ms of attachment loads and stores" was a wrong estimate.**
  Skipping the depth loads broke depth testing. Tiled passes are worth
  about 1 ms.
- **Texel cache (deferred_a/b).** It runs on the Pixel now, but is slower:
  claims 2.6 + lookups 5.4 against 5.6 ms per pixel. The hash atomics are
  dear on PowerVR.
- **Splitting a quad's shadow taps across its 4 pixels** made it slower:
  masked lanes still run every iteration.
- **Caching the shadow matrix per pixel** was slower: the driver already
  hoists it.
- Cutting deferred's taps from 8 to 2, or `TEXEL_DENOISE` 1, moved it only
  about 5%.

## PowerVR facts (in docs/ANDROID_VS_PC.md)

- A colour image over 4 bytes a pixel is padded to power-of-two sides:
  2856x1280 takes 4096x2048.
- A pass with more than 32 bytes a pixel of colour outputs fails at full
  resolution: `ERROR_OUT_OF_HOST_MEMORY`, and `DMABUFHEAPS vframe-secure`
  in logcat.
- A fragment shader with side effects tests depth late unless it declares
  `[earlydepthstencil]`.
- Back-to-back runs throttle about 30%. Use in-run A/Bs, or compare against
  an untouched pass (`final`).

## Bench workflow

- **Server.** `target/bench-server`: the official 26.3 jar, offline,
  creative, RCON. Start it with `java -Xmx4G -jar server.jar nogui` from
  that folder. Time, weather and mob spawning are locked.
- **Bench app.** `tools/bench-apk/run.sh` builds and installs
  `com.torch.bench` (the real game stays untouched) and joins via
  `adb reverse`. It takes:
  - `SECS`, `RD`, `NO_BUILD=1`;
  - `PACK=builtin`;
  - `OPTS='"shaderRenderScale": 70'` (extra options.json fields);
  - `PACKVALS='WATER_REFLECTIONS=0;...'` (pack options);
  - extra `KEY=VALUE` lines for `env.txt`.
- **Debug switches.** All are listed in `platform/env.rs`:
  - `MC_PACK_AB=tiled|hiz|shadowlist`: an in-run A/B that logs
    `pack <f> A/B: gpu A x, B y`;
  - `MC_VK_MEMORY=1`: the biggest GPU allocations;
  - `MC_PACK_SKIP=<programs>`;
  - `MC_PACK_TILED=1`: the tiled path on desktop, for image compares;
  - `MC_PACK_TILED_OFF=merge,store,load,clear`;
  - `MC_VK_ROBUST=1`;
  - `MC_PACK_SHOW`.
- **Desktop image check.** `vkview --terrain target/pixel-dump4 --pack
  builtin --camera -40,80,296,0,-10 --headless --step 0.05 --frames 60
  --size 1920x864 --screenshot x.png` (set `MINECRAFT_ASSETS` and
  `MINECRAFT_DATA` to `reference/`).
- **Screenshots.** Take them only while `com.torch.bench` is in the
  foreground (check `topResumedActivity`) to avoid capturing the user's
  home screen.
- **Reverting experiments.** Remove the exact lines you added. Don't
  `git checkout` files that hold uncommitted work.

## Open items / next steps

1. Done: Hi-Z gather checked visually (both sides match, foliage noise
   only); the real game APK rebuilt and installed at e257b4a. Its first
   launch offers the 375 MB sound download.
2. Logcat lags the A/B side flips by about a second: label screenshots by
   a forced side (no A/B) or take many and compare all pairs.
3. Pack speedups still to try:
   - Under a pack the frame is still drawn upright and turned (0.5-0.7
     ms): have the pack's `final` write turned into the swapchain (its
     fragment reads by SV_Position, so read by uv under MC_OS_ANDROID),
     then overlays and the GUI turned as in the no-pack path.
   - Pack setup (~4.6 s cold) runs on the render thread: move pipeline
     creation to a worker, or keep the old frame showing meanwhile.
   - Pack terrain through vertex input, and slimmer varyings: pack
     normal/material/foliage into one flat uint, drop color.a if always 1
     (geometry about 4.6 ms).
   - Profile deferred's 3.5 ms of lighting (`surface_light`,
     sky_ambient/direct_light, cell_res) and composite2's fog `resolve` and
     sky colour.
4. Earlier backlog:
   - translucent pass on the slim interface;
   - block animation copy spikes;
   - a moving-camera bench (flood fill about 10 ms per section crossing);
   - a Bedrock comparison, which needs the user to load a comparable world.
