#!/usr/bin/env python3
"""Regenerate the firework explosion outlines in
`src/util/particles/fireworks.rs` from the decompiled
`FireworkParticles.java` in `reference/minecraft-<version>/`.

    python3 tools/gen_firework_shapes.py           # rewrite the outlines
    python3 tools/gen_firework_shapes.py --check   # exit 1 if out of date

`Starter.CREEPER_PARTICLE_COORDS` and `STAR_PARTICLE_COORDS` become
`CREEPER_COORDS` and `STAR_COORDS`, as `f32` pairs.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(__file__))
from generated import replace_region  # noqa: E402

CRATE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE = os.path.join(CRATE, "reference", "minecraft-26.3", "net", "minecraft",
                      "client", "particle", "FireworkParticles.java")
TARGET = os.path.join(CRATE, "src", "util", "particles", "fireworks.rs")

SHAPES = [("CREEPER_PARTICLE_COORDS", "CREEPER_COORDS", "The creeper face's outline"),
          ("STAR_PARTICLE_COORDS", "STAR_COORDS", "The star's outline")]


def f32(text):
    """A double literal as Rust writes the nearest f32: shortest round trip."""
    import struct
    value = struct.unpack("f", struct.pack("f", float(text)))[0]
    for digits in range(1, 10):
        short = f"{value:.{digits}g}"
        if struct.unpack("f", struct.pack("f", float(short)))[0] == value:
            return short if "." in short or "e" in short else short + ".0"
    return repr(value)


def main():
    src = open(SOURCE, encoding="utf-8").read()
    out = []
    for java, rust, doc in SHAPES:
        if out:
            out.append("\n")
        m = re.search(rf"{java}\s*=\s*new double\[\]\[\]\{{(.*?)\}};", src, re.S)
        if not m:
            sys.exit(f"{java} not found")
        pairs = re.findall(r"\{\s*(-?[0-9.]+),\s*(-?[0-9.]+)\s*\}", m.group(1))
        out.append(f"/// {doc} (`FireworkParticles.Starter.{java}`).\n")
        out.append(f"pub(super) const {rust}: [[f32; 2]; {len(pairs)}] = [\n")
        out += [f"    [{f32(x)}, {f32(y)}],\n" for x, y in pairs]
        out.append("];\n")
    sys.exit(replace_region(TARGET, "firework shapes", "".join(out), check="--check" in sys.argv))


if __name__ == "__main__":
    main()
