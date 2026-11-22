# PC candidates from the Android work

Findings from optimizing the Android build that could also help the PC
build. None is applied to PC: each waits for a go-ahead. Android's version
of each is in `docs/ANDROID_VS_PC.md`; numbers in `docs/ANDROID_PERF.md`.

| Finding | Android change | PC gain (estimate) |
|---|---|---|
| Pack quads stored per vertex slot: 3 of every 4 entries unused | one 16-byte entry per quad (`QUAD_UNITS`, `iris_vk_quad`) | 3/4 of `terrain pack quads` under a pack (645 MB at RD 32 on the phone) |
| Per-Face Culling's filtered index buffers exist with the option off | made on first use (`grow_filtered`) | ~13 MB a pool (1.6 M indices x 4 bytes x 2) while off |
