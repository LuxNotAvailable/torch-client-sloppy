#!/usr/bin/env python3
"""Runs `tools/DumpGameData.java` against the official server jar.

Downloads the version's server jar from Mojang's manifest, checks its SHA-1,
unpacks the bundler's game jar and libraries, compiles the dumper and prints
its output, which the gen_*.py generators read:

    tools/dump_game_data.py 26.3 WORK_DIR > game.txt
    python3 tools/gen_block_sounds.py game.txt > src/generated_block_sounds.rs

WORK_DIR holds the download and build; reruns reuse a verified jar. Needs a
JDK (javac and java) on PATH.
"""

import hashlib
import json
import os
import subprocess
import sys
import urllib.request
import zipfile
from pathlib import Path

MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
TOOLS = Path(__file__).resolve().parent


def fetch_json(url):
    with urllib.request.urlopen(url) as r:
        return json.load(r)


def server_jar(version, work):
    entry = next((v for v in fetch_json(MANIFEST)["versions"] if v["id"] == version), None)
    if entry is None:
        sys.exit(f"no version {version} in the manifest")
    server = fetch_json(entry["url"])["downloads"]["server"]
    jar = work / f"server-{version}-bundler.jar"
    if not jar.is_file() or hashlib.sha1(jar.read_bytes()).hexdigest() != server["sha1"]:
        print(f"downloading {server['url']}", file=sys.stderr)
        with urllib.request.urlopen(server["url"]) as r:
            data = r.read()
        if hashlib.sha1(data).hexdigest() != server["sha1"]:
            sys.exit("server jar SHA-1 mismatch")
        jar.write_bytes(data)
    return jar


def unpack(bundler, work):
    """The bundler keeps the game jar under META-INF/versions and its
    libraries under META-INF/libraries."""
    out = work / "bundle"
    jars = []
    with zipfile.ZipFile(bundler) as z:
        for name in z.namelist():
            if name.endswith(".jar") and name.startswith(("META-INF/versions/", "META-INF/libraries/")):
                z.extract(name, out)
                jars.append(out / name)
    if not jars:
        sys.exit("no jars in the bundler")
    return jars


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    version, work = sys.argv[1], Path(sys.argv[2]).resolve()
    work.mkdir(parents=True, exist_ok=True)
    classpath = os.pathsep.join(str(j) for j in unpack(server_jar(version, work), work))
    classes = work / "classes"
    classes.mkdir(exist_ok=True)
    subprocess.run(["javac", "-cp", classpath, "-d", str(classes), str(TOOLS / "DumpGameData.java")],
                   check=True)
    # The game writes logs and caches to the working directory.
    run = work / "run"
    run.mkdir(exist_ok=True)
    subprocess.run(["java", "-cp", classpath + os.pathsep + str(classes), "DumpGameData"],
                   check=True, cwd=run, stderr=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
