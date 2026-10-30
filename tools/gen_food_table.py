#!/usr/bin/env python3
"""Regenerate the food table in `src/gui/appleskin.rs` (`food_values`) from
the decompiled `Foods.java` in `reference/minecraft-<version>/`.

    python3 tools/gen_food_table.py           # rewrite the table
    python3 tools/gen_food_table.py --check   # exit 1 if it is out of date

Each `FoodProperties NAME = ...` gives the item `name` (lower-cased): its
`.nutrition(n)` and `.saturationModifier(m)`, or `stew(n)`, which is
nutrition `n` with a modifier of 0.6.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(__file__))
from generated import replace_region  # noqa: E402

CRATE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FOODS = os.path.join(CRATE, "reference", "minecraft-26.3", "net", "minecraft",
                     "world", "food", "Foods.java")
TARGET = os.path.join(CRATE, "src", "gui", "appleskin.rs")

# `Foods.stew`: `nutrition(n).saturationModifier(0.6F)`.
STEW_MODIFIER = 0.6


def main():
    src = open(FOODS, encoding="utf-8").read()
    stew = re.search(r"stew\(final int nutrition\)\s*\{[^}]*saturationModifier\(([0-9.]+)F\)", src)
    if not stew or float(stew.group(1)) != STEW_MODIFIER:
        sys.exit("Foods.stew changed; update STEW_MODIFIER")
    rows = []
    for m in re.finditer(r"FoodProperties ([A-Z_]+) = ([^;]+);", src):
        name, expr = m.group(1).lower(), m.group(2)
        if s := re.match(r"stew\((\d+)\)", expr):
            nutrition, modifier = int(s.group(1)), STEW_MODIFIER
        else:
            n = re.search(r"\.nutrition\((\d+)\)", expr)
            mod = re.search(r"\.saturationModifier\(([0-9.]+)F\)", expr)
            if not (n and mod):
                sys.exit(f"can't read {m.group(1)}: {expr}")
            nutrition, modifier = int(n.group(1)), float(mod.group(1))
        rows.append((name, nutrition, modifier))
    body = "".join(f'        "{name}" => ({n}, {mod:g}),\n' for name, n, mod in sorted(rows))
    body = re.sub(r", (\d+)\),", r", \1.0),", body)
    sys.exit(replace_region(TARGET, "food values", body, check="--check" in sys.argv))


if __name__ == "__main__":
    main()
