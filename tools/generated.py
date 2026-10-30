"""Shared by the `gen_*` tools that fill a marked region of a Rust file.

A region sits between two comment lines:

    // @generated begin <name>: tools/gen_<...>.py
    ...
    // @generated end <name>

Everything between them belongs to the tool; edit the tool, not the region.
"""

import re


def replace_region(path, name, body, check=False):
    """Puts `body` between the markers named `name` in `path`.

    With `check`, writes nothing and returns 1 when the region differs, so a
    caller can `sys.exit` with the result.
    """
    src = open(path, encoding="utf-8").read()
    pattern = re.compile(
        rf"(^[ \t]*// @generated begin {re.escape(name)}:[^\n]*\n)(.*?)(^[ \t]*// @generated end {re.escape(name)}\n)",
        re.S | re.M,
    )
    m = pattern.search(src)
    if not m:
        raise SystemExit(f"{path}: no '@generated begin {name}' region")
    if m.group(2) == body:
        print(f"{path}: {name} up to date")
        return 0
    if check:
        print(f"{path}: {name} is out of date; run the tool to regenerate it")
        return 1
    open(path, "w", encoding="utf-8").write(src[:m.start(2)] + body + src[m.end(2):])
    print(f"{path}: {name} regenerated")
    return 0
