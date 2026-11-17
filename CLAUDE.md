# torch-client

Rust Minecraft client (Bevy + native Vulkan). Rules: `STYLE_GUIDE.md`.

## Context rules
- Start at `docs/ARCHITECTURE.md`, then the target module's `mod.rs` `//!` doc.
- Don't read `vendor/`. See `vendor/PATCHES.md`; open a crate only to debug it.
- Don't read data files unless the task is about their values:
  `src/generated_*.rs`, `*_anim.rs`, `entities/models/aquatic/animation/`,
  `protocol/data/*.json`. `.ignore` keeps them out of searches; use
  `rg --no-ignore` to reach them.
- Big files (2K+ lines: `vk/terrain.rs`, `renderer/systems.rs`, `gui/slots.rs`,
  `gui/options.rs`, `protocol/hop.rs`, ...): grep for the symbol and read
  that range, never the whole file.
- Protocol: recipe at top of `src/protocol/hop.rs`.
- Vanilla parity: decompiled source in `reference/minecraft-<ver>/` (git-ignored,
  so search it with an explicit path or `--no-ignore`).
