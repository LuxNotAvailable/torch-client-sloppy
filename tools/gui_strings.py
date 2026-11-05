#!/usr/bin/env python3
"""Finds English shown in the GUI that doesn't go through translation.

Lists string literals in src/gui that look like text a player reads, and for
each the vanilla translation keys whose English value matches it exactly
(from the newest reference lang/en_us.json), or our own torch.* key from
src/gui/lang/en_us.json. Literals with no match need a torch.* key.

The GUI draws text through `gui::tooltip::localize`, which takes the English
as the message id, so a string only needs a key somewhere to translate:
--write gives every unmatched one a torch.<file>.<slug> key in
src/gui/lang/en_us.json (keeping keys already there), for language files and
resource packs to translate.

Usage:
    tools/gui_strings.py            # report
    tools/gui_strings.py --json     # the same as JSON, for scripted edits
    tools/gui_strings.py --write    # add torch.* keys for unmatched strings

Run it after a version port to catch new hard-coded text, or after vanilla
renames a key.
"""

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GUI = REPO / "src/gui"
TORCH_LANG = GUI / "lang/en_us.json"

LITERAL = re.compile(r'(?<![\w!])"((?:[^"\\]|\\.)*)"')
# The F3 debug panels, which vanilla doesn't translate either.
SKIP_FILES = {"src/gui/hud.rs"}
# Calls whose literal arguments are code, not text.
SKIP_LINE = re.compile(
    r"log_(info|debug|warn|error|trace)!|prof_span!|\.sprite|sprite\(|key\(|"
    r"assert|panic!|expect\(|#\[|Name::new|tr\(|label!\(|translate\(|"
    r"has_translation\(|include_str!|concat!|env!|reason =|debug_assert|unreachable!|log_[a-z]+!\($"
)
# A call whose arguments run onto the next lines: a string starting one of
# those lines belongs to it.
CALL_OPEN = re.compile(r"(log_[a-z]+!|assert[a-z_]*!|expect|panic!|unreachable!|warn!|allow|expect)\(\s*$|^\s*\w+!\(\s*$")


def newest_lang() -> dict:
    found = sorted(REPO.glob("reference/minecraft-*/assets/minecraft/lang/en_us.json"))
    if not found:
        sys.exit("no reference en_us.json")
    return json.loads(found[-1].read_text(encoding="utf-8"))


def looks_like_text(s: str) -> bool:
    if len(s.strip()) < 3 or not re.search(r"[A-Za-z]", s) or "{" in s or "\\" in s:
        return False
    if re.fullmatch(r"[a-z0-9_./:#-]+", s):  # ids, keys, paths, sprites
        return False
    return s[0].isupper() or " " in s.strip()


def main() -> int:
    vanilla = newest_lang()
    ours = json.loads(TORCH_LANG.read_text(encoding="utf-8")) if TORCH_LANG.exists() else {}
    by_value = {}
    for key, value in vanilla.items():
        by_value.setdefault(value, []).append(key)
    for key, value in ours.items():
        by_value.setdefault(value, []).insert(0, key)

    found = {}
    for path in sorted(GUI.rglob("*.rs")):
        if path.relative_to(REPO).as_posix() in SKIP_FILES:
            continue
        in_tests = False
        in_call = False
        for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if "#[cfg(test)]" in line:
                in_tests = True
            continued = in_call and line.lstrip().startswith(('"', '&format', '"'))
            if CALL_OPEN.search(line) or (SKIP_LINE.search(line) and line.rstrip().endswith("(")):
                in_call = True
            elif line.rstrip().endswith((");", "),", ")")):
                in_call = False
            if in_tests or continued or line.lstrip().startswith("//") or SKIP_LINE.search(line):
                continue
            for s in LITERAL.findall(line):
                if looks_like_text(s):
                    found.setdefault(s, []).append(f"{path.relative_to(REPO).as_posix()}:{n}")

    report = {s: {"keys": by_value.get(s, []), "at": at} for s, at in sorted(found.items())}
    if "--write" in sys.argv:
        out = dict(ours)
        taken = set(out)
        for text, r in report.items():
            if r["keys"]:
                continue
            stem = Path(r["at"][0].split(":")[0]).stem
            slug = re.sub(r"[^a-z0-9]+", "_", text.lower()).strip("_")[:40] or "text"
            key = f"torch.{stem}.{slug}"
            n = 2
            while key in taken:
                key = f"torch.{stem}.{slug}_{n}"
                n += 1
            taken.add(key)
            out[key] = text
        text = json.dumps(dict(sorted(out.items())), indent=2, ensure_ascii=False)
        TORCH_LANG.write_text(text + "\n", encoding="utf-8")
        print(f"{len(out)} torch.* keys in {TORCH_LANG.relative_to(REPO)}")
        return 0
    if "--json" in sys.argv:
        print(json.dumps(report, indent=1))
        return 0
    unmatched = [s for s, r in report.items() if not r["keys"]]
    for s, r in report.items():
        keys = ", ".join(r["keys"][:3]) or "(no key)"
        print(f"{s!r}: {keys}  [{len(r['at'])}x, {r['at'][0]}]")
    print(f"\n{len(report)} literals, {len(report) - len(unmatched)} with a key, {len(unmatched)} without")
    return 0


if __name__ == "__main__":
    sys.exit(main())
