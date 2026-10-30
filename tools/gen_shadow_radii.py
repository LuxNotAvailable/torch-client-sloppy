#!/usr/bin/env python3
"""Regenerate the `shadowRadius` table in `src/renderer/shadows.rs` from the
decompiled client in `reference/minecraft-<version>/`.

    python3 tools/gen_shadow_radii.py           # rewrite the table
    python3 tools/gen_shadow_radii.py --check   # exit 1 if it is out of date

Each entity type's renderer comes from `EntityRenderers.java`. Its radius is
the `this.shadowRadius = ...F` it assigns, or else the float a constructor
hands to `this(...)` or `super(...)`, following `extends` up the class chain.
A radius written as `<n>F * <param>` (the giant's) is multiplied by the
float the registration passes. Only non-zero radii are written; the match
falls back to 0, which casts no shadow.
"""

import glob
import os
import re
import sys

sys.path.insert(0, os.path.dirname(__file__))
from generated import replace_region  # noqa: E402

CRATE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENTITY = os.path.join(CRATE, "reference", "minecraft-26.3", "net", "minecraft",
                      "client", "renderer", "entity")
TARGET = os.path.join(CRATE, "src", "renderer", "shadows.rs")

FLOAT = r"(?<![\w.])([0-9]+\.[0-9]+)F"

# Renderers whose constructor float is something other than the shadow, so
# the rule above would misread it.
NO_SHADOW = {
    # `ThrownItemRenderer(context, 1.0F, false)`: the item's scale.
    "ThrownItemRenderer",
}


def sources():
    return {
        os.path.basename(f)[:-5]: open(f, encoding="utf-8").read()
        for f in glob.glob(os.path.join(ENTITY, "**", "*.java"), recursive=True)
    }


def delegations(src, cls):
    """The arguments of each constructor's leading `this(...)`/`super(...)`."""
    for ctor in re.finditer(rf"\b{cls}\s*\([^)]*\)\s*\{{", src):
        body = src[ctor.end():]
        m = re.match(r"\s*(?:this|super)\s*\(", body)
        if not m:
            continue
        depth, i = 1, m.end()
        while depth:
            depth += {"(": 1, ")": -1}.get(body[i], 0)
            i += 1
        yield body[m.end():i - 1]


def radius(files, cls, scale, depth=0):
    src = files.get(cls)
    if src is None or depth > 6 or cls in NO_SHADOW:
        return None
    m = re.search(rf"this\.shadowRadius\s*=\s*{FLOAT}", src)
    if m:
        return float(m.group(1))
    # A constructor may pass the radius on through `this(...)` to another
    # that hands it to `super(...)`: the first float found decides.
    for args in delegations(src, cls):
        scaled = re.search(rf"{FLOAT}\s*\*\s*\w+", args)
        if scaled and scale is not None:
            return float(scaled.group(1)) * scale
        floats = re.findall(FLOAT, args)
        if floats:
            return float(floats[-1])
    parent = re.search(rf"class\s+{cls}(?:<[^{{]*?>)?\s+extends\s+(\w+)", src)
    return radius(files, parent.group(1), scale, depth + 1) if parent else None


def camel(name):
    return "".join(p.capitalize() for p in name.lower().split("_"))


def main():
    files = sources()
    registry = files["EntityRenderers"]
    rows, missing = [], []
    for m in re.finditer(
        r"register\(EntityTypes\.(\w+),\s*(?:context\s*->\s*new\s+(\w+)\(([^)]*)\)|(\w+)::new)",
        registry,
    ):
        etype, cls = m.group(1), m.group(2) or m.group(4)
        reg_float = re.search(FLOAT, m.group(3) or "")
        r = radius(files, cls, float(reg_float.group(1)) if reg_float else None)
        if r is None:
            missing.append(f"{etype} ({cls})")
        elif r > 0:
            rows.append((camel(etype), r))
    if missing and "--verbose" in sys.argv:
        print("no shadow (radius 0):", ", ".join(missing))
    body = "".join(f"        K::{name} => {r:g},\n" for name, r in sorted(rows))
    body = re.sub(r"=> (\d+),", r"=> \1.0,", body)
    sys.exit(replace_region(TARGET, "shadow radii", body, check="--check" in sys.argv))


if __name__ == "__main__":
    main()
