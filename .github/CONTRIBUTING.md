# Contributing

## Issues

Use the Bug, Crash or Suggestion form on the issues page. Search existing issues first.

## Building

```sh
cargo build --release          # desktop
tools/build_web.sh             # browser
tools/build_android.sh         # Android APK
```

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
tools/build_web.sh --features webgpu
```

| Feature | Default | What it does |
| --- | --- | --- |
| `asset_download` | on | Downloads the game's assets from Mojang on first run. |
| `multiversion` | on | Lets the client join servers on other protocol versions. |
| `full_font` | on | Draws every Unicode character instead of a missing-glyph box. |
| `skins` | on | Player skins and capes. Without it, everyone gets one of the built-in default skins. |
| `online_mode` | on | Microsoft account sign-in, for servers that require it. |
| `shader_support` | on | Loads Iris/OptiFine-style shader packs from a `shaders/` folder. |
| `audio` | on | Sound. Desktop only (does nothing on the web or Android); downloads about 332 MB of sounds on first run, which can be skipped. |
| `voice` | on | Simple Voice Chat on servers that run it: voices heard at the players speaking, and the microphone by push-to-talk (Caps Lock) or voice activation. Desktop only; settings under Options > Voice Chat. |
| `resourcepacks` | on | A resource packs screen (Options > Resource Packs...) for loading packs from a `resourcepacks/` folder. |
| `explore_modrinth` | off | Browsing and downloading resource packs from Modrinth. Turns on `resourcepacks`. |
| `eagler` | off | Joins Eaglercraft servers over `wss://`. |
| `webgpu` | off | Browser build only. Uses WebGPU instead of WebGL2, with no fallback. |
| `mobile_ui` | off | On-screen touch controls. The Android build turns this on itself. |
| `click_gui` | off | A Right Shift menu with a switch and settings for every module. |
| `hud_editor` | on | A screen (Options > Edit HUD...) for dragging, scaling and hiding HUD elements, saved to `hud.json`. |
| `profiling` | off | Span profiler. Press F9 in game to save a flamegraph. |
| `deadlock-detection` | off | Names the threads involved when the client freezes on a lock. |
| `alloc_diag` | off | Heap usage rows on the F3 screen. |
| `budget` | off | Per-frame timing breakdown. |

The default features are what ships. Each has an in-game toggle for players who don't want it, so building without them (`--no-default-features`) isn't supported. The off-by-default features above are for development and diagnostics; add them on top of the defaults.
