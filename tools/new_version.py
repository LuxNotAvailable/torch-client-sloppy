#!/usr/bin/env python3
"""Bring a Minecraft version into `reference/` and generate its tables.

    py -3 tools/new_version.py vanilla 1.21.1 [--remap-to 1.21.11] [--remap-from V]
    py -3 tools/new_version.py neoforge 1.21.1 [--branch 1.21.1]

`vanilla VER` fills `reference/minecraft-VER/` the way the existing trees
look, then runs `protogen.py`:

1. Mojang's manifest gives the client jar, server jar, the client mappings
   (obfuscated versions only, up to 1.21.11) and the client libraries.
   Each download is checked against the manifest's SHA-1. Official jars
   only, never ones found on the PC.
2. The server jar's data generator writes `generated/reports/`.
3. An obfuscated client jar is renamed to Mojang's names with
   AutoRenamingTool (`--reverse`: the mappings run named to obfuscated).
4. Vineflower decompiles the classes; the jar's other files (assets, data,
   `version.json`) are copied as they are.
5. `protogen.py packets`, then `remap` for each `--remap-to` / `--remap-from`.

`neoforge VER` shallow-clones NeoForge's branch for that game version
(`1.21.1`, or `26.3.x`) into `reference/neoforge-VER/`: source patches and
NeoForge's own code, no decompile needed.

Downloads and tool runs go in `--work` (default `target/new_version/VER`);
reruns reuse verified files. AutoRenamingTool and Vineflower are pinned by
SHA-256. Needs a JDK at least as new as the version's `javaVersion`, and
git for `neoforge`. `--force` replaces an existing reference tree.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import ssl
import subprocess
import sys
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

CRATE = Path(__file__).resolve().parent.parent
TOOLS = Path(__file__).resolve().parent
MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
NEOFORGE_GIT = "https://github.com/neoforged/NeoForge.git"

# (url, sha256): pinned, so a changed upload fails instead of running.
VINEFLOWER = (
    "https://repo1.maven.org/maven2/org/vineflower/vineflower/1.12.0/vineflower-1.12.0.jar",
    "1dfcfe974395734fa467ce620661c7623d05ba83670de0529b1fbd63ff548b9d",
)
RENAMER = (
    "https://maven.neoforged.net/releases/net/neoforged/AutoRenamingTool/2.0.18/"
    "AutoRenamingTool-2.0.18-all.jar",
    "64ece9706e5b111cebcb662d4eab56ba6b9b9b3b4bee1766382352fe02a6a7f3",
)


class Fail(Exception):
    pass


def log(message):
    print(f"new_version: {message}", file=sys.stderr)


def fetch_json(url):
    with urllib.request.urlopen(url) as r:
        return json.load(r)


def digest(path, algorithm):
    h = hashlib.new(algorithm)
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def download(url, path, algorithm, expected):
    """Fetch `url` to `path` unless a file with the expected hash is there."""
    if path.is_file() and digest(path, algorithm) == expected:
        return path
    path.parent.mkdir(parents=True, exist_ok=True)
    log(f"downloading {url}")
    partial = path.with_name(path.name + ".part")
    try:
        with urllib.request.urlopen(url) as r, open(partial, "wb") as f:
            shutil.copyfileobj(r, f)
    except urllib.error.URLError as e:
        if not isinstance(e.reason, ssl.SSLError):
            raise
        # Python's own OpenSSL rejects some chains the OS accepts (seen with
        # maven.neoforged.net); curl uses the OS's TLS, still verifying.
        log(f"{e.reason.reason}; retrying with curl")
        subprocess.run(["curl", "-fsSL", "-o", str(partial), url], check=True)
    got = digest(partial, algorithm)
    if got != expected:
        partial.unlink()
        raise Fail(f"{url}: {algorithm} {got}, expected {expected}")
    partial.replace(path)
    return path


def version_json(version):
    entry = next((v for v in fetch_json(MANIFEST)["versions"] if v["id"] == version), None)
    if entry is None:
        raise Fail(f"no version {version} in Mojang's manifest")
    return fetch_json(entry["url"])


def java_major():
    out = subprocess.run(["java", "-version"], capture_output=True, text=True).stderr
    match = re.search(r'version "(\d+)(?:\.(\d+))?', out)
    if not match:
        raise Fail(f"cannot read the Java version from: {out.strip()}")
    major = int(match.group(1))
    return int(match.group(2)) if major == 1 else major


def run(args, cwd=None):
    log("running " + " ".join(str(a) for a in args[:4]) + (" ..." if len(args) > 4 else ""))
    subprocess.run([str(a) for a in args], check=True, cwd=cwd)


def libraries(meta, work):
    """The client's library jars (no natives), for the decompiler's context."""
    out = []
    for lib in meta.get("libraries", []):
        artifact = lib.get("downloads", {}).get("artifact")
        if not artifact or "natives" in lib.get("name", ""):
            continue
        path = work / "libraries" / artifact["path"]
        out.append(download(artifact["url"], path, "sha1", artifact["sha1"]))
    return out


def reports(server, work, dest):
    """Data generator reports into `dest/generated/reports`."""
    gen = work / "datagen"
    if gen.exists():
        shutil.rmtree(gen)
    run_dir = work / "datagen-run"
    run_dir.mkdir(parents=True, exist_ok=True)
    run(
        [
            "java",
            "-DbundlerMainClass=net.minecraft.data.Main",
            "-jar",
            server,
            "--reports",
            "--output",
            gen,
        ],
        cwd=run_dir,
    )
    if not (gen / "reports" / "registries.json").is_file():
        raise Fail(f"the data generator wrote no reports/registries.json under {gen}")
    target = dest / "generated" / "reports"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(gen / "reports", target)


def named_jar(client, mappings, libs, work):
    """The client jar with Mojang's names; the jar itself when unobfuscated."""
    if mappings is None:
        return client
    out = work / "client-named.jar"
    renamer = download(RENAMER[0], work / "tools" / Path(RENAMER[0]).name, "sha256", RENAMER[1])
    args = ["java", "-jar", renamer, "--input", client, "--output", out]
    args += ["--names", mappings, "--reverse"]
    args += ["--ann-fix", "--ids-fix", "--src-fix", "--record-fix"]
    for lib in libs:
        args += ["--lib", lib]
    run(args)
    with zipfile.ZipFile(out) as z:
        if "net/minecraft/network/protocol/game/GameProtocols.class" not in z.namelist():
            raise Fail(f"{out} has no GameProtocols class; the rename did not apply")
    return out


def decompile(jar, libs, work, dest):
    out = work / "decompiled"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    vineflower = download(
        VINEFLOWER[0], work / "tools" / Path(VINEFLOWER[0]).name, "sha256", VINEFLOWER[1]
    )
    # Long names: Vineflower 1.12 reads `-sef=1` as a file to decompile.
    args = ["java", "-Xmx4G", "-jar", vineflower, "-log=WARN", "--skip-extra-files=1"]
    args += [f"--thread-count={max(1, (os.cpu_count() or 2) - 1)}"]
    args += [f"-e={lib}" for lib in libs]
    args += ["--folder", jar, out]
    run(args)
    if not any(out.rglob("GameProtocols.java")):
        raise Fail(f"Vineflower wrote no GameProtocols.java under {out}")
    shutil.copytree(out, dest, dirs_exist_ok=True)


def resources(client, dest):
    """Everything in the client jar that isn't a class or signature."""
    with zipfile.ZipFile(client) as z:
        for info in z.infolist():
            name = info.filename
            if info.is_dir() or name.endswith(".class") or name.startswith("META-INF/"):
                continue
            target = (dest / name).resolve()
            if not target.is_relative_to(dest.resolve()):
                raise Fail(f"{client} holds an entry outside the tree: {name}")
            target.parent.mkdir(parents=True, exist_ok=True)
            with z.open(info) as src, open(target, "wb") as f:
                shutil.copyfileobj(src, f)


def protogen(*args):
    run([sys.executable, TOOLS / "protogen.py", *args])


def vanilla(args):
    version = args.version
    dest = CRATE / "reference" / f"minecraft-{version}"
    work = Path(args.work) if args.work else CRATE / "target" / "new_version" / version
    work = work.resolve()
    if dest.exists():
        if not args.force:
            raise Fail(f"{dest} exists; pass --force to replace it")
        shutil.rmtree(dest)

    meta = version_json(version)
    needed = meta.get("javaVersion", {}).get("majorVersion", 0)
    if java_major() < needed:
        raise Fail(f"{version} needs Java {needed}; `java` on PATH is {java_major()}")

    downloads = meta["downloads"]
    client = download(
        downloads["client"]["url"], work / "client.jar", "sha1", downloads["client"]["sha1"]
    )
    server = download(
        downloads["server"]["url"], work / "server.jar", "sha1", downloads["server"]["sha1"]
    )
    mappings = None
    if "client_mappings" in downloads:
        m = downloads["client_mappings"]
        mappings = download(m["url"], work / "client_mappings.txt", "sha1", m["sha1"])
    libs = libraries(meta, work)

    staging = work / "reference"
    if staging.exists():
        shutil.rmtree(staging)
    staging.mkdir(parents=True)
    reports(server, work, staging)
    decompile(named_jar(client, mappings, libs, work), libs, work, staging)
    resources(client, staging)
    info = json.loads((staging / "version.json").read_text(encoding="utf-8"))
    if info.get("id") != version:
        raise Fail(f"the client jar's version.json is {info.get('id')!r}, not {version!r}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.move(str(staging), str(dest))
    log(f"{dest} ready (protocol {info.get('protocol_version')})")

    protogen("packets", "--version", version)
    for other in args.remap_to:
        protogen("remap", "--from", version, "--to", other)
    for other in args.remap_from:
        protogen("remap", "--from", other, "--to", version)


def neoforge_branch(version):
    """NeoForge keeps one branch per game version, named `1.21.1` or
    `26.3.x`; most releases are not tagged."""
    out = subprocess.run(
        ["git", "ls-remote", "--heads", NEOFORGE_GIT],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    heads = {line.rsplit("refs/heads/", 1)[-1] for line in out.splitlines()}
    for branch in (version, f"{version}.x"):
        if branch in heads:
            return branch
    raise Fail(f"NeoForge has no branch {version} or {version}.x")


def neoforge(args):
    dest = CRATE / "reference" / f"neoforge-{args.version}"
    if dest.exists():
        if not args.force:
            raise Fail(f"{dest} exists; pass --force to replace it")
        shutil.rmtree(dest)
    branch = args.branch or neoforge_branch(args.version)
    run(["git", "clone", "--depth", "1", "--branch", branch, NEOFORGE_GIT, dest])
    log(f"{dest} ready (NeoForge branch {branch}; `git pull` there updates it)")


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    modes = ap.add_subparsers(dest="mode", required=True)

    v = modes.add_parser("vanilla", help="decompile a game version and generate its tables")
    v.add_argument("version")
    v.add_argument("--remap-to", action="append", default=[], metavar="VER")
    v.add_argument("--remap-from", action="append", default=[], metavar="VER")
    v.add_argument("--work")
    v.add_argument("--force", action="store_true")

    n = modes.add_parser("neoforge", help="clone NeoForge's source for a game version")
    n.add_argument("version")
    n.add_argument("--branch")
    n.add_argument("--force", action="store_true")

    args = ap.parse_args()
    try:
        if args.mode == "vanilla":
            vanilla(args)
        else:
            neoforge(args)
    except Fail as e:
        log(str(e))
        return 1
    except subprocess.CalledProcessError as e:
        log(f"{e.cmd[0]} failed with exit code {e.returncode}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
