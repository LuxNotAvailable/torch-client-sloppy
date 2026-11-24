#!/usr/bin/env python3
"""Builds the standalone Torch Mod Converter folder.

    python tools/converter_app/build_dist.py [--out dist/TorchConverter]

The folder:

    TorchConverter/
      Torch Converter.exe      the window (crates/torch-converter, Rust)
      app/                     port_agent.py, port_agent_model.py (porting only)
      bin/                     torch-convert, torch-port-check (built here)
      workspace/               what the porting agent works in: docs, the
                               mod API crate, ports/ (a git repo on first use)
      data/                    the dump server, decompiled mods, logs,
                               settings; vanilla-index.txt (asset names only)
      mods_in/  mods_out/      default input and output folders
      README.txt

Rebuilding over an existing folder keeps data/, mods_in/, mods_out/ and the
workspace's ports and git history; tools, app and docs are replaced.
"""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
HERE = Path(__file__).resolve().parent
EXE = ".exe" if os.name == "nt" else ""
BUILD = HERE / "build" / "target"

README = """Torch Mod Converter
===================

Turns NeoForge mod jars into .torchmod packages for the Torch client, and
can port mods' client renderers with a local model.

Start: double-click "Torch Converter.exe". Put jars in mods_in/ (or pick
any folder), check the Mods tab, press Start; packages land in mods_out/.

Needs:
- Converting: Java 21+ (a JDK: java and javac). The first run downloads
  the official NeoForge server installer and asks for Minecraft's EULA.
- Porting renderers (optional, slow): Python 3.10+, git, ripgrep (rg), Rust (rustup) with
  `rustup target add wasm32-unknown-unknown`, Ollama and Qwen Code
  (`npm install -g @qwen-code/qwen-code`). The first port sets up the
  model (app/port_agent_model.py).

The window lists every stage, step and command as it runs; the raw output
is underneath. Logs of porting runs: data/port/logs/.

Sharing: a .torchmod carries the mod's own assets; check the mod's
licence before giving one to anyone.
"""


def log(msg):
    print(f"[build-dist] {msg}", flush=True)


def cargo_build(crate):
    log(f"building {crate.name}")
    env = dict(os.environ, CARGO_TARGET_DIR=str(BUILD))
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=crate, env=env, check=True)
    return BUILD / "release" / f"{crate.name}{EXE}"


def copy_tree(src, dst, ignore=("target", "Cargo.lock", ".qwen")):
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst, ignore=shutil.ignore_patterns(*ignore))


def vanilla_index(out):
    """Vanilla asset paths (names only), for torch-port-check."""
    src = ROOT / "reference" / "minecraft-26.3" / "assets" / "minecraft"
    if not src.is_dir():
        log("no reference/minecraft-26.3 assets: the checker won't check minecraft: names")
        return
    lines = []
    for sub in ("models", "textures", "items", "blockstates"):
        for p in sorted((src / sub).rglob("*")):
            if p.is_file():
                lines.append(p.relative_to(src).as_posix())
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    log(f"vanilla index: {len(lines)} names")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=str(ROOT / "dist" / "TorchConverter"))
    a = ap.parse_args()
    out = Path(a.out).resolve()
    out.mkdir(parents=True, exist_ok=True)

    bin_dir = out / "bin"
    bin_dir.mkdir(exist_ok=True)
    for crate in ("torch-convert", "torch-port-check"):
        shutil.copy2(cargo_build(ROOT / "crates" / crate), bin_dir)

    app = out / "app"
    app.mkdir(exist_ok=True)
    for f in (ROOT / "tools" / "port_agent.py", ROOT / "tools" / "port_agent_model.py"):
        shutil.copy2(f, app / f.name)
    # The window is Rust now: drop the old Python one and its launchers.
    for old in (app / "torch_converter.pyw", out / "Torch Converter.bat", out / "torch-converter.sh"):
        if old.exists():
            old.unlink()
    shutil.copy2(cargo_build(ROOT / "crates" / "torch-converter"), out / f"Torch Converter{EXE}")

    ws = out / "workspace"
    (ws / "docs" / "porting").mkdir(parents=True, exist_ok=True)
    for f in ("MOD_API.md", "TORCHMOD_FORMAT.md"):
        shutil.copy2(ROOT / "docs" / f, ws / "docs" / f)
    for f in (ROOT / "docs" / "porting").glob("*.md"):
        shutil.copy2(f, ws / "docs" / "porting" / f.name)
    copy_tree(ROOT / "crates" / "torch-mod-api", ws / "crates" / "torch-mod-api")
    # Ports: the template and the shipped ones; a workspace's own work stays.
    for port in sorted((ROOT / "ports").iterdir()):
        dst = ws / "ports" / port.name
        if port.is_dir() and (port.name == "example" or not dst.exists()):
            copy_tree(port, dst)
    (ws / ".gitignore").write_text("target/\n.qwen/\n", encoding="utf-8")

    data = out / "data"
    data.mkdir(exist_ok=True)
    vanilla_index(data / "vanilla-index.txt")
    for d in ("mods_in", "mods_out"):
        (out / d).mkdir(exist_ok=True)
    (out / "README.txt").write_text(README, encoding="utf-8")
    log(f"done: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
