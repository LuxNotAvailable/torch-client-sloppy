# PC candidates from the Android work

Findings from optimizing the Android build that could also help the PC
build. Android's version of each is in `docs/ANDROID_VS_PC.md`; numbers in
`docs/ANDROID_PERF.md`. New findings wait here for a go-ahead before they
reach PC code.

## Applied to PC (2026-11-22, on request: nothing that changes the picture or breaks outside packs)

Checked on desktop (vkview, two cameras): the built-in pack and the five
test packs in `config/shaders` (MakeUp, Sildur's, TrueLightFX, miniature,
E-LITE) render pixel-identical to before; without a pack only the scene's
usual run-to-run noise differs.

Laptop (MX250, 1920x1080, RD 12, the bench server's spawn; runs alternated
old and new builds):

| | Before | After |
|---|---|---|
| Torch Vibrant: fps / GPU | 29.9 / 34.9 ms | 40.2 / 25.7 ms |
| gbuffers_water | 7.50 ms | 1.60 ms |
| gbuffers_terrain / cutout | 2.89 / 2.04 ms | 1.81 / 1.76 ms |
| Pack occlusion alone (in-run `MC_PACK_AB=hiz`) | 27.5 ms | 25.7 ms |
| No pack: GPU | 8.55, 8.30 ms | 8.28, 8.12 ms |
| No pack: CPU terrain record | 1.74, 2.15 ms | 0.89, 1.17 ms |

Screenshots across the A/B's sides match apart from foliage sway.

| Change | Gain |
|---|---|
| Pack quads: one 16-byte entry per quad, at vertex / 4 (`QUAD_UNITS` = 4; `iris_vk_quad`; the GLSL prelude's `vk_pack_stream` for outside packs) | 3/4 of `terrain pack quads` under any pack; ~134 MiB at the vkview dump |
| Per-Face Culling's filtered index buffers made on first use (`grow_filtered`) | ~85 MiB at the vkview dump while off |
| Terrain cull loads ranges and bands only for kept slots (`CULL_LAZY`); Hi-Z level 1 by one `GatherRed` per 2 x 2 | same commands, same pyramid |
| Remesh keeps a section's visible bit and rank when its cave visibility is unchanged; dense-array cave flood fill (`fill_dense`) | CPU (~40 -> ~10 ms per fill on the phone) |
| Occlusion culling under a pack: depthtex2 (the depth before the hand, any pack) builds the pyramid (`pack_pyramid`) | large at long render distances (the phone: 68.5 -> 31.4 ms) |
| gbuffers_water drops a fragment behind depthtex1 after its alpha test (exact: the `LESS_OR_EQUAL` test would drop it after the shader) | hidden water no longer shaded |
| shadowtex1 not cleared when the shadow depth copy overwrites it | one shadow-map clear a frame |
| Pipeline cache saved once a pack is set up | a crash or kill no longer costs a cold pack compile |

## Not applied

| Finding | Why not on PC |
|---|---|
| Shadow cascade culls over CPU lists (`shadow_list`) | the side planes ignore an outside pack's shadow distortion: E-LITE lost shadow casters (27 pixels) |
| Translucent re-sort only on a new eye block | an older order within the block blends a little differently (MakeUp: 2 pixels, 1/255) |
| `[earlydepthstencil]` on gbuffers_water | an alpha-tested texel would write depth (resource packs with clear glass); PC drops hidden fragments in the shader instead |
| colortex2 as RGBA16UI | no gain: PC's RGBA16 is already 8 bytes (Android's fell back to 16). Would fix blended clouds and entities blending into the texel words (composite2 reads them), a visible change, so it waits |
| depthtex2 left out when unsampled | PC's pack occlusion reads it |
| Tiled render passes (`vk::pack::tiled`) | tile-GPU win; laptop and desktop GPUs aren't tiled |
| Half-rate occlusion, colortex6 as RGBA16F, render scale, turned drawing | change the picture, or phone-only |

## Bugs found on the way

- Fixed: the GLSL prelude indexed terrain slots 128 bytes apart after the
  culling suite (cc3caf0) grew them to 320, so every outside pack lost the
  device (`ERROR_DEVICE_LOST`). `pack_terrain_reads_the_slot_stride` now
  checks the prelude too.
- Open: with Near-First on, the no-pack terrain path loses the device on
  the laptop's MX250 (from 609b90a, when Near-First arrived; off by
  default).
