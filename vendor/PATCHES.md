# Vendored crates

Patched upstream crates, wired in through `[patch.crates-io]` (or path deps)
in `Cargo.toml`. Read this before opening `vendor/`; read the crate itself only
when debugging it.

| Crate | Upstream | Why vendored | Changed files |
|---|---|---|---|
| `azalea/` | azalea (git) | Real fork. Protocol, physics, inventory and registry changes the client needs. | ~120 files; see below |
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
- `azalea-client`: `ItemUseSlow`, set by the game each tick, scales input
  and blocks starting a sprint while an item is in use (`plugins/movement.rs`).
- `azalea-client`: closing a container empties the cursor, as the closed
  menu's carried stack goes with it (`plugins/inventory/mod.rs`).
- `azalea-client`: `queue_held_instant_mine` breaks instant blocks in the
  same tick while the game sets `HOLD_TO_MINE` (`plugins/mining.rs`).
- `azalea-client`: `ON_LOCAL_BREAK` is called with each predicted break
  before the world changes, so every local break plays its effects
  (`plugins/mining.rs`).
- `azalea-client`: the mining speed lookup uses
  `azalea_registry::data::Enchantment`, not the deprecated alias
  (`plugins/mining.rs`).
- `codegen/`: Python generators for the tables above. Edit these, then
  regenerate; never edit generated tables by hand.

Large generated files (skip unless the task is about their data):
`azalea-physics/src/collision/blocks.rs`, `azalea-registry/src/tags/*.rs`,
`azalea-inventory/src/default_components/generated.rs`,
`azalea-block/src/generated.rs`, `azalea-language/src/en_us.json`.
