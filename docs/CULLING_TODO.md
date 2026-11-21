# Culling TODO

Gaps found with the Overdraw and Freeze Culling debug views (Debug
Options). Check each item with Freeze Culling on: freecam behind the frozen
view and see what's still drawn.

## Checklist

- [x] 1. Away-face culling for entities
- [x] 2. Away-face culling for non-standard blocks
- [x] 3. Max torch distance
- [x] 4. Max sign distance
- [x] 5. Max entity distance
- [x] 6. Away-face culling for foliage
- [x] 7. Per-plane away-face ranges
- [x] 8. Fast chest distance

Tick an item here once all of its steps below are done.

Every option lives in Video Settings > Advanced Performance, each one
switchable, so they can be A/B tested within one run.

## What exists now

The terrain cull pass is `cull` in `shaders/terrain.slang`.

- **Frustum, cave flood fill and Hi-Z** (`shaders/cull.slang`): per
  section (16³).
- **Away-face buckets** (`CULL_BUCKETS`): the opaque layer only. `bucket()`
  in `src/vk/terrain.rs` sorts each section's solid quads into +x, -x, +y,
  -y, +z and -z by normal. A bucket is skipped when the camera is fully
  behind the section's plane for that axis. Quads on no axis go to bucket
  6, which is always drawn.
- **GPU back-face culling**: on for terrain solid, cutout and translucent.
  For entities it's on only in some pipelines (see below).

## 1. Away-face culling for entities

Mobs, item frames, armour stands, paintings, block entities.

Now:
- Entity pipelines in `src/vk/entities.rs` default to
  `vk::CullModeFlags::NONE`. The comment there says faces draw both sides,
  as `entity.wgsl` did, and the back is shaded by the opposite light. Only
  the `entity_solid` / `entity_blend` variants built with `stream_with(...,
  BACK)` cull back faces, and only when the option asks for it.
- The entity cull (`shaders/entity.slang`, around line 75) tests a whole
  entity's bounds: frustum and Hi-Z only. There's no per-face or per-part
  test.

Done, toggle "Entity Away-Face Culling":
- [x] Instances with an opaque material get `draw::KIND_CULL`: with no
      alpha holes, a closed model's back faces never show. (Vanilla's
      default `entityCutout` draws both sides; its `entitySolid` and
      `entityCutoutCull` models cull.)
- [x] The entity cull writes solid instances to two lists, two-sided and
      back-culled (`cull_entities`, push `params.z`), drawn with `solid`
      and `solid_culled`. Entity Back Faces: Hidden still culls them all.
      Commands: two-sided, back-culled, then blended at rank.
- [ ] Cutout models (most mobs): cull per part where the texture region
      has no transparent texels. Needs the texture's alpha at mesh build.
- [x] Skipping item frames and paintings seen from behind: dropped. On
      glass, fences or bars their backs are visible, and vanilla draws
      them.

## 2. Away-face culling for non-standard blocks

Lanterns, chests, signs, fences, torches, rails, model blocks.

Now:
- **The cutout layer isn't bucketed.** In the cull pass the cutout layer
  is one command per section (`s.ranges.y` after the buckets), so every
  cutout quad in a visible section is drawn. Lanterns, torches and the
  other model blocks on the cutout layer get no away-face culling at all.
- On the opaque layer, quads whose normal isn't exactly on an axis go to
  bucket 6 (no axis), which is always drawn. The test is `bucket()`'s
  integer cross product, so any tilted or rotated model element counts.
- Chests, signs and other block entities are drawn by the entity path, so
  item 1 covers them.

To do:
- [x] Bucket the cutout layer the same way as solid: six axis buckets plus no
      axis, before the torch range. The axis counts are u16 pairs in
      `Slot.ranges.zw` and `spare.y`; the no-axis count is the rest. A count
      over 16 bits folds the section's cutout into no-axis. Wall signs,
      standing signs at 0°/90°/180°/270°, hanging signs and lanterns now
      cull their away faces.
- [x] Standing signs at 22.5°, 45° and 67.5°: in the octant buckets
      (item 6).
- [x] Two-sided cutout quads: not needed. The mesher emits each side as its
      own quad with its own normal, and the cutout pipeline culls back faces
      anyway, so a skipped bucket only drops faces that never showed.
- [x] Bucket 6 (tilted quads in the solid layer): covered by Per-Face
      Culling (item 7), which tests every quad exactly. Without it they stay
      always drawn; rare in the solid layer.

## 3. Max torch distance

Cull every torch beyond a set distance: torches, wall torches, soul,
copper and redstone torches, and their flame and smoke particles.

Done:
- [x] Mesher: `RenderedBlock.torch` (any block name ending in "torch")
      sends a torch's cutout quads to a torch range closing each section's
      cutout indices (`MeshBuf.torch_count`). Greedy merging leaves that
      tail in place.
- [x] `Slot.buckets[7]` holds the torch index count. The cull shader drops
      it from the cutout command when the section's nearest point is past
      the reach. The reach squared is `CullView.flags.y` (0: no limit).
- [x] Flame, smoke and redstone dust stop past the same reach
      (`torch_past_reach` in `util/particles/ambient.rs`, from the block
      centre).
- [x] Option: Video Settings > Advanced Performance, "Torch Distance" (16, 32, 48,
      64 blocks or Unlimited, the default), Low impact. It needs no
      re-mesh: the cull reads it every frame.
- [x] Lighting isn't affected: block light is baked into vertices.
- [ ] The shadow pass keeps every torch (its `flags.y` is 0).
- [ ] Old terrain dumps (vkview) carry no torch count, so they keep all
      torches.

## 4. Max sign distance

Skip sign text (standing, wall and hanging signs) beyond a set distance.

Done: since 26.3 the board and post are baked terrain, so only the text is
a block entity, already cut at vanilla's 64 blocks. "Sign Distance" (Video
Settings > Advanced Performance: 16, 32, 48 blocks or Vanilla) lowers that cutoff,
in `sync_block_entities` (`src/blockentities/mod.rs`).

- [x] Drop sign text beyond the limit.
- [x] Option, Low impact.
- [ ] Boards beyond the limit: they're terrain, so this needs the same
      per-section range as torches (item 3).

## 5. Max entity distance

Skip entities beyond a set distance: mobs, item entities, item frames,
armour stands and block entities.

Done: `render_reach` in `src/entities/mod.rs` ports vanilla's
`shouldRenderAtSqrDistance` and its per-type overrides: the mean box edge
× 64 × view scale. Players and arrows get ×10, item frames 16 × 64, and
phantoms, tridents and end crystals have no limit. The view scale is
vanilla's: render distance / 8, clamped to 1..2.5, times the new option.
Block entities already had vanilla's per-renderer limits (64 by default).

- [x] Distance test per entity (CPU, before the rig is shown).
- [x] Size scaling, as vanilla.
- [x] Option: Video Settings > Advanced Performance, "Entity Distance" (50%–200%,
      steps of 25%, Medium impact).
- [ ] Optional: exempt the player's vehicle and passengers. Vanilla
      doesn't, so this is left out for now.

## 6. Away-face culling for foliage

Tall grass, flowers, saplings, crops, ferns, dead bushes, sugar cane: the
cross-shaped plant models.

Done (with item 2's turned signs), toggle "Angled Face Culling":
- [x] The cutout layer's vertical faces sit in a ring of buckets: each
      axis, then two octants to the next axis (either side of the
      diagonal). `bucket_cutout` / `cutout_bucket` in `src/vk/terrain.rs`.
- [x] An octant is skipped when the camera is behind the section for both
      its edge directions (the box's back plane for each). Sound: a normal
      in the octant is a non-negative mix of the two.
- [x] Ring order keeps the seen buckets one arc, so few commands. Past four
      runs the last one stretches (drawn, just not culled).
- [x] `Slot` grew to 144 bytes (`more`) for the counts, as u16 pairs.
- [ ] Measure: a plains or forest scene, Angled Face Culling on and off in
      one run.

## 7. Per-plane away-face ranges

Away faces near the player still drew: the bucket test was per section,
so in a section the eye's x, y or z falls inside, both buckets for that
axis drew.

Done (coarse bands), toggle "Exact Face Culling":
- [x] `band_sort` (`src/vk/terrain.rs`) sorts each solid axis bucket into
      four 4-block plane bands, far side first, after greedy merging (a
      merged record's plane via `packed::unpack`).
- [x] The counts through the first three bands, as u16 pairs, in
      `Slot.more.zw` and `bands` (the `Slot` is now 176 bytes). A plane off
      the section or a count past 16 bits leaves the bucket unbanded: the
      old section test.
- [x] Cull shader: each bucket splits into a seen and a hidden part at the
      eye's band. +a and -a store the same way, so an axis pair's seen faces
      touch: still at most four commands.
- [x] 16 bands, one per block plane: exact for faces on the block grid. The
      counts past `more.zw` sit at `SLOT_BANDS` (the `Slot` is 320 bytes),
      read only when needed.
- [x] Per-Face Culling (toggle): `face_filter` in `shaders/terrain.slang`,
      a compute pass after the cull. One group per drawn solid or cutout
      command tests each quad's first triangle against the cull eye and
      compacts the kept quads into the pool's filtered indices at the
      command's own offset, then rewrites its count. Exact for every quad:
      plants, turned signs, model blocks. Off under shader packs (their
      shadow pass reads the pools with commands of its own).
- [x] Measured (in-run A/B/C, 2026-11-21): a net loss, so it's off by
      default, labelled "(Slower!)" with a warning tooltip. MX250 laptop, RD
      12: the pass costs 1.6 ms against about 1.1 ms of drawing saved. Pixel
      10 Pro: it cuts the terrain draw by 1.8 ms but the pass costs about
      3 ms, and the GPU clocks up under it (passes it doesn't touch ran 25%
      faster), hiding the work.
- [x] The suite without it: the laptop's all-passes GPU 7.37 to 7.08 ms
      (-4%), terrain draw -5%; the Pixel's terrain draw 6.87 to 5.71 ms
      (-17%), all passes 14.03 to 12.76 ms. The cull pass costs the same.
- [ ] Cutout axis buckets aren't banded yet.

## 8. Fast chest distance

Done already, at every distance: `src/blockentities/bake.rs` (after
BetterBlockEntities) bakes closed chests, trapped chests, ender chests and
shulker boxes into the section mesh, their entity textures packed into the
block atlas (`TEXTURE_DIRS`). They get every terrain cull: buckets, bands,
Per-Face Culling, Hi-Z, greedy merging. Only a chest whose lid moves goes
back to its rig, handed back to the baked copy once it settles.

- [x] Baked model per chest kind and facing, meshed with the terrain.
- [x] Double chests and open lids (a moving lid is drawn by the rig).
- [x] No distance option needed: baking is cheaper than the rig at any
      range.

## Verify

For each item, use Overdraw + Freeze Culling at a fixed spot. Compare
screenshots before and after, and the perf report's terrain GPU pass time
(Debug Options > perf report) with the option on and off within one run.
