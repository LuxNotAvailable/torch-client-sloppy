# PC candidates from the Android work

Findings from optimizing the Android build that could also help the PC
build. None is applied to PC: each waits for a go-ahead. Android's version
of each is in `docs/ANDROID_VS_PC.md`; numbers in `docs/ANDROID_PERF.md`.

| Finding | Android change | PC gain (estimate) |
|---|---|---|
| Pack quads stored per vertex slot: 3 of every 4 entries unused | one 16-byte entry per quad (`QUAD_UNITS`, `iris_vk_quad`) | 3/4 of `terrain pack quads` under a pack (645 MB at RD 32 on the phone) |
| Per-Face Culling's filtered index buffers exist with the option off | made on first use (`grow_filtered`) | ~13 MB a pool (1.6 M indices x 4 bytes x 2) while off |
| No occlusion culling under a shader pack: the terrain only culls, so the pyramid is never built (`pyramid_view` cleared under `cull_only`) | the pack's depth before the hand builds it (`hiz_reduce_pack`, `pack_pyramid`) | likely large at high render distance: the phone went 68.5 -> 31.4 ms |
| gbuffers_water's reflection claims are side effects, so its depth test runs after the shader: hidden water is fully shaded | `[earlydepthstencil]` on the water fragment | the phone's water pass 29.9 -> 11.8 ms; desktop GPUs also test late for side effects |
| Pack attachments LOAD/STORE every pass, clears as transfers | `vk::pack::tiled` | ~1 ms on the phone (tile GPU); probably nothing on desktop GPUs, low priority |
