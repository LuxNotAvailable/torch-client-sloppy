# Torch Client

[Torch](https://torchclient.com) is a Minecraft client rebuilt from the ground up in Rust: no JVM, and by the original project's own figures a third of the memory and twice the frame rate of the vanilla Java client. This repository is an unofficial fork of it ([torch-client/torch-client](https://github.com/torch-client/torch-client)).
Please go use and contribute to the original ^^^

> [!WARNING]
> This is a SLOPPY SLOPPY abomination of vibe coded mess mixed with also sloppy human coded mess
> This is in no way reflective of the original project, I also do not condone the use of AI as a crutch for people
> who dont know how to program in the first place. That being said, this is a rather guilty pleasure project of mine to see
> just how capable these slop machines have come so far. This is NOT a serious project. 

## What it plays

- **Minecraft Java Edition 26.3**, natively.
- **26.1 to 26.1.2 and 1.21.11 servers**, through a translation layer. Pick the version from the menu on the Multiplayer screen.
- **Accounts:** a Microsoft account, or an offline name for servers that allow it.
- **Game assets:** downloaded from Mojang's servers on first launch. None are shipped in this repository.

## Build from source

The pinned nightly toolchain in `rust-toolchain.toml` is installed by `rustup` on the first build.

```sh
git clone https://github.com/LuxNotAvailable/torch-client-sloppy.git
cd torch-client-sloppy
cargo build --release          # desktop: target/release/torch-client
tools/build_linux.sh           # Linux
tools/build_web.sh             # browser
tools/build_android.sh         # Android APK
```

Run `torch-client --help` for the command line:
- `-u <name>` sets the player name.
- `-a <host>` joins a server directly.
- `--dir` shows where settings and assets are kept.
- `--reset`, `--reset-config` and `--reset-assets` clear them.

Optional features (the click GUI, touch controls, profilers) are listed in [CONTRIBUTING.md](.github/CONTRIBUTING.md#feature-system).

### Performance reports

```sh
cargo build --release --features perf_report
```

This build adds a **Profile** button to the pause menu. It records ten seconds of play and writes a report to `profiles/` in the client's folder. The report covers frame rate and lows, every timed stage, GPU passes, memory and CPU use, and the machine's hardware and settings. It comes as a `.txt` to read and a `.json` to compare across machines.

## Contributing

See [CONTRIBUTING.md](.github/CONTRIBUTING.md).

## License

[GNU Affero General Public License v3.0 or later](LICENSE)

This project vendors modified source from:
- azalea: [vendor/azalea/LICENSE.md](vendor/azalea/LICENSE.md)
- bevy_camera: [vendor/bevy_camera/LICENSE-MIT](vendor/bevy_camera/LICENSE-MIT) or [vendor/bevy_camera/LICENSE-APACHE](vendor/bevy_camera/LICENSE-APACHE)
- simdnbt: [vendor/simdnbt/LICENSE](vendor/simdnbt/LICENSE)

Torch is not affiliated with Mojang Studios or Microsoft.
and Torch-Sloppy is not affiliated with Torch.
