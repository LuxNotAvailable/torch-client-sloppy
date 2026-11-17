# Vendored crates

Patched upstream crates, wired in through `[patch.crates-io]` (or path deps)
in `Cargo.toml`. Read this before opening `vendor/`; read the crate itself only
when debugging it.

| Crate | Upstream | Why vendored | Changed files |
|---|---|---|---|
| `azalea/` | azalea (git) | Real fork. Protocol, physics, inventory and registry changes the client needs. | ~120 files; see below |
| `bevy_core_pipeline/` | 0.18.1 | PowerVR (Pixel 10 Pro) aborts in `vkCreateComputePipelines` on the mip-generation downsample-depth shader. Fork skips that pipeline. | `src/experimental/mip_generation/mod.rs` (`create_downsample_depth_pipelines`) |
| `simdnbt/` | crates.io | Bounds checks: overflow-safe length math, `skip` checks before moving. Malformed NBT can't read past the buffer. | `src/common.rs` (`read_with_u32_length`), `src/reader.rs` (`ensure_can_read`, `skip`) |
| `bevy_camera/` | 0.18.1 | Reason not recorded. Diff against the crates.io 0.18.1 source to find it. | unknown |

## azalea

Treat as our code. History: `git log -- vendor/azalea`. Main areas:

- `azalea-protocol`: packet changes (`c_add_entity` modded types,
  `c_move_entity_pos_rot`, `c_update_advancements`), bitsets.
- `azalea-physics`: scaffolding collision, ladders, crawl
  (`collision/world_collisions.rs`, `collision/blocks.rs`).
- `azalea-inventory`: components (paintings, bundles, trims), click
  prediction (`operations.rs`, `slot.rs`).
- `azalea-block`, `azalea-registry`: modded IDs past `MAX_STATE`, regenerated
  tables (`tags/`, `builtin.rs`, `data.rs`).
- `azalea-entity`: metadata, inventory, particles.
- `azalea-client`: `handle_walk` no longer stops a sprint; the vanilla stop
  rules in `local_player_ai_step` do (`plugins/movement.rs`).
- `codegen/`: Python generators for the tables above. Edit these, then
  regenerate; never edit generated tables by hand.

Large generated files (skip unless the task is about their data):
`azalea-physics/src/collision/blocks.rs`, `azalea-registry/src/tags/*.rs`,
`azalea-inventory/src/default_components/generated.rs`,
`azalea-block/src/generated.rs`, `azalea-language/src/en_us.json`.
