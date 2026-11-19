# Contributing

## Issues

Use the Bug, Crash or Suggestion form on the issues page. Search existing issues first.

## Building

Windows, Linux and Android only. The renderer is Vulkan (`src/vk/`), so there is no macOS or browser build.

```sh
cargo build --release          # desktop
tools/build_android.sh         # Android APK
cargo apk build --release      # Android APK, through cargo-apk
```

`cargo apk` reads its Android settings from `[package.metadata.android]` in `Cargo.toml`, which uses the same SDK levels as `tools/build_android.sh`. Signing keys never go in `Cargo.toml`. Set `CARGO_APK_RELEASE_KEYSTORE` and `CARGO_APK_RELEASE_KEYSTORE_PASSWORD`, or the same with your profile's name in place of `RELEASE`. A dev build without them is signed with the Android debug key. Keep keystores in the git-ignored `keystore/` folder.

### Shaders

The renderer's shaders are Slang, in `shaders/*.slang`. `build.rs` compiles every file with entry points to SPIR-V 1.3 with `slangc`, so a build needs it:

- Install the [Vulkan SDK](https://vulkan.lunarg.com/), which ships `slangc` and sets `VULKAN_SDK`. `build.rs` looks for `$VULKAN_SDK/Bin/slangc`, then `slangc` on the `PATH`.
- Or point `SLANGC` at a `slangc` binary.
- The shaders are written against Slang 2026.1. Another version builds with a warning.

Files without entry points are shared modules, imported by the others. Shaders must stay valid for Vulkan 1.1 (`spirv-val --target-env vulkan1.1`), and fragment shaders `discard` only in the entry point, since some PowerVR drivers reject it inside called functions.

Shader packs (Iris/OptiFine GLSL) are a separate path: `src/shaderpack/` translates them at run time.

### Validation layers

Debug builds turn on the Khronos validation layer; in a release build set `MC_VK_VALIDATION=1`. The layer comes with the Vulkan SDK. Without it, the client logs `vk: validation asked for, but the Khronos layer isn't installed` and runs without it.

Validation errors and warnings are printed to the log. A change to `src/vk/` or `shaders/` should add none; check with the `vkview` viewer (`cargo run --release --bin vkview`) as well as in game. `MC_VK_EXTENSION_PATH=1` and `MC_VK_NO_INDIRECT_COUNT=1` force the Vulkan 1.1 and no-`drawIndirectCount` paths on a desktop GPU, to test what older devices run.

Optional features are listed under [Feature system](#feature-system). If your change touches code behind a feature, build with that feature on (for example `--features click_gui`), since code behind a feature that is off is not compiled at all.

## Pull requests

- Keep each PR to one change.
- For anything large you want to work on, preferably open an issue first before writing the code describing what you will add.
- Run `rustfmt` and match the style of the surrounding code.
- Keep changes under `vendor/` small.
- No including Mojang's assets in the source tree.
- Code between `// @generated begin` and `// @generated end` is written by the `tools/gen_*.py` script the marker names, from the decompiled game in `reference/`. Change the script and rerun it rather than editing the region; `--check` reports a table that has fallen out of date.

## Feature system

Parts of the client are Cargo features. These will be cleaned up in the future as they become more solid.

```sh
cargo build --release --features click_gui
```

| Feature | Default | What it does |
| --- | --- | --- |
| `asset_download` | on | Downloads the game's assets from Mojang on first run. |
| `multiversion` | on | Lets the client join servers on other protocol versions. |
| `full_font` | on | Draws every Unicode character instead of a missing-glyph box. |
| `skins` | on | Player skins and capes. Without it, everyone gets one of the built-in default skins. |
| `online_mode` | on | Microsoft account sign-in, for servers that require it. |
| `shader_support` | on | Loads Iris/OptiFine-style shader packs from a `shaders/` folder. |
| `audio` | on | Sound. Desktop only (does nothing on Android); downloads about 332 MB of sounds on first run, which can be skipped. |
| `voice` | on | Simple Voice Chat on servers that run it: voices heard at the players speaking, and the microphone by push-to-talk (Caps Lock) or voice activation. Desktop only; settings under Options > Voice Chat. |
| `resourcepacks` | on | A resource packs screen (Options > Resource Packs...) for loading packs from a `resourcepacks/` folder. |
| `explore_modrinth` | off | Browsing and downloading resource packs from Modrinth. Turns on `resourcepacks`. |
| `eagler` | off | Joins Eaglercraft servers over `wss://`. |
| `vulkan` | on | The renderer. Needs `slangc` to build (see [Shaders](#shaders)). |
| `mobile_ui` | off | On-screen touch controls. The Android build turns this on itself. |
| `click_gui` | off | A Right Shift menu with a switch and settings for every module. |
| `hud_editor` | on | A screen (Options > Edit HUD...) for dragging, scaling and hiding HUD elements, saved to `hud.json`. |
| `profiling` | off | Span profiler. Press F9 in game to save a flamegraph. |
| `deadlock-detection` | off | Names the threads involved when the client freezes on a lock. |
| `alloc_diag` | off | Heap usage rows on the F3 screen. |
| `budget` | off | Per-frame timing breakdown. |
| `perf_report` | on | A Profile button in the pause menu: records 10 seconds of play and writes `profiles/profile-<time>.txt` and `.json` to the client's folder, with frame rate and lows, every timed stage and span, GPU passes, meshes and images uploaded, draws, live GPU objects, a memory breakdown (heap, asset data, terrain, sounds, GPU), counters, process use and the machine's hardware and settings. |

The default features are what ships. Each has an in-game toggle for players who don't want it, so building without them (`--no-default-features`) isn't supported. The off-by-default features above are for development and diagnostics; add them on top of the defaults.
