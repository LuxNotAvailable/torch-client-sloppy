#!/usr/bin/env python3
"""Checks the subtitle keys in a sound index's definitions against a
language file, for porting a new game version.

Every sound event in sounds.json may name a `subtitle` translation key; the
client shows it through `audio::subtitles`. A key missing from the language
file shows raw, so run this after updating either side.

Usage:
    tools/check_subtitles.py [SOUNDS_JSON] [LANG_JSON]

SOUNDS_JSON defaults to the newest index installed under the client's asset
directory (%LOCALAPPDATA%/torch-client/assets/sounds/<index>.json);
LANG_JSON to the newest reference/minecraft-*/assets/minecraft/lang/en_us.json.
Exits 1 if any key is missing.
"""

import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def newest_sounds() -> Path:
    base = Path(os.environ.get("LOCALAPPDATA", Path.home() / ".local/share"))
    found = sorted(
        (base / "torch-client/assets/sounds").glob("*.json"),
        key=lambda p: int(p.stem) if p.stem.isdigit() else -1,
    )
    if not found:
        sys.exit("no installed sound index; pass SOUNDS_JSON")
    return found[-1]


def newest_lang() -> Path:
    found = sorted(REPO.glob("reference/minecraft-*/assets/minecraft/lang/en_us.json"))
    if not found:
        sys.exit("no reference en_us.json; pass LANG_JSON")
    return found[-1]


def main() -> int:
    sounds_path = Path(sys.argv[1]) if len(sys.argv) > 1 else newest_sounds()
    lang_path = Path(sys.argv[2]) if len(sys.argv) > 2 else newest_lang()
    sounds = json.loads(sounds_path.read_text(encoding="utf-8"))
    lang = json.loads(lang_path.read_text(encoding="utf-8"))

    keys = {}
    silent = []
    for event, body in sounds.items():
        key = body.get("subtitle")
        if key:
            keys.setdefault(key, []).append(event)
        else:
            silent.append(event)
    missing = sorted(k for k in keys if k not in lang)

    print(f"sounds: {sounds_path}")
    print(f"lang:   {lang_path}")
    print(f"{len(sounds)} events, {len(keys)} subtitle keys, "
          f"{len(silent)} events without a subtitle")
    for key in missing:
        print(f"  missing {key} (used by {', '.join(sorted(keys[key])[:3])})")
    print("all subtitle keys translate" if not missing else f"{len(missing)} missing")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
