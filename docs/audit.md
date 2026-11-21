# Audit

2026-11-21 · 657 rs · 19 tools/ scripts · 20 vendor/azalea/codegen/

The 2026-11-17 audit's items are all done: the orphans are deleted, the
`lerp` copies use `mth`, and the profile folder is now `acquired-profiles/`.

## Dupes

Identical function bodies (6+ lines; generated, `_anim` and model files skipped).
Merged 2026-11-21: `unwind` (now `mth::unwind_degrees`), `player_facing`
(now `interaction::player_direction`), `last_word_start` (now
`commands::last_word_index`).

Kept: `terrain/cave.rs` / `scene/occlusion.rs` `index` and
`entities/blockmodel.rs` / `blockentities/text.rs` `finish`. Each is ~12 lines
over different types; sharing them needs a trait for little gain.

Not dupes: `mth::wrap_degrees` / `wrap_degrees_f64` (vanilla keeps both float
widths), timeline `lerp` trait, environment `lerp`, `items/mesh.rs` `remap`.

## Dead code

`cargo check` is warning-free (desktop and `mobile_ui`). Removed 2026-11-21:
`audio::audible_at`, `DisplayTransforms::get_named`, and `Retired::Entities` /
`Retired::Particles` (entity and particle scenes update in place now).

## Orphans

None. Every tracked `src/**/*.rs` is reachable from `main.rs` / `lib.rs` / `bin/`.

## Dead-code allows

68, each with a `reason`: feature- or platform-gated items (touch controls,
perf_report, profiling, packed assets), test-only helpers, and kept vanilla
parity (full `*_anim.rs` sets, `PartDefinition` methods). Spot-checked clean.
Watch these groups; they hold code nothing calls yet:

- `shaderpack/` ×7: "the native pack rewrite picks it up"
- `voice/mod.rs:271`, `voice/wire.rs:449`: voice screen and mic not built
- `text/mod.rs:12`: end poem and credits not drawn

## Hygiene

- `src/vk/pack/screen.rs:841`: the only real TODO left (odd-frame parity variant).
- Done 2026-11-21: vendored `azalea-client` uses `azalea_registry::data::Enchantment`
  (in `vendor/PATCHES.md`); `IMMERSIVEVFX_FEATURES.md` moved to `docs/`;
  `Modelfile.64k` deleted.
- `dist/`, `keystore/`, `screenshot-dump/`, `reference/`, `acquired-profiles/`: gitignored, as expected.

## Scripts

All 19 `tools/` scripts active (codegen + asset pipeline). `tools/generated.py`
is the region-replacement engine for the gen_* tools. Vendor scripts: azalea protocol codegen.

## Priority

Nothing open above Low: the pack TODO and the shaderpack/voice allows wait on their features.

Low, long-term: split `scene/` and `vk/` into their own crates for a hard
boundary (from the retired `token-optimization.md`; its other items are done).
