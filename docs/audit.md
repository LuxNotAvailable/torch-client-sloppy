# Audit

2026-11-17 · 669 rs · 21 tools/ scripts · 20 vendor/azalea/codegen/

## Dupes

| Where | Dup of | Fix |
|-------|--------|-----|
| `src/util/interp.rs` `fn lerp(t:f64,a:f64,b:f64)` | `mth::lerp_f64` (t is f32 there) | add f64-t overload to mth |
| `src/renderer/mesh.rs:137` `fn lerp(a,b,t)` | `mth::lerp(t,a,b)` | use mth |

Not dupes (legit repeated names): `clamp`×12, `normalize`×5, `Target`×4, timeline `lerp` trait, environment `lerp` — all domain-specific.

## Orphans (not in any mod.rs)

| File | Lines | What | Do |
|------|-------|------|----|
| `src/gui/gui_material.rs` | 86 | Bevy GUI material, unreferenced | del or wire |
| `src/play/tutorial.rs` | 267 | Tutorial steps (Move, FindTree, Punch…), references `gui::toast::TutorialSlot` | del or wire |
| `src/renderer/packmaterial.rs` | 379 | Terrain pack material, refs `shaderpack::pipeline` | del or wire |

~732 lines dead.

## Dead-code allows

3 total: `gui/toast.rs:85`, `gui/option_codec.rs:283`, `diag/mod.rs:13`. Clean.

## Hygiene

- `aquired-profiles/` → typo, rename `acquired-profiles/` + fix `.gitignore`
- `dist/`, `keystore/`, `screenshot-dump/`, `reference/`, `web-output/` — all gitignored, expected

## Scripts

All 21 `tools/` scripts active (codegen + asset pipeline). `tools/generated.py` = region-replacement engine for gen_* tools, not a generated artifact. Vendor scripts: azalea protocol codegen.

## Priority

1. **High** — del/wire 3 orphans (732 lines)
2. **Med** — dedupe 2 `lerp` copies via mth
3. **Low** — rename `aquired-profiles/`
