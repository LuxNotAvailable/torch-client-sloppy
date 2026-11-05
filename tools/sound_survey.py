#!/usr/bin/env python3
"""Surveys which sounds the client plays against what vanilla's client plays.

Most sounds reach the client as `ClientboundSound*` packets and play through
one generic path. The rest vanilla's client plays itself: its own footsteps,
block hits, the UI, ambience, music, level events and so on. This lists, for
each vanilla source file that plays sounds on the client, the sound events it
names that nothing in `src/` mentions, and every sound id written in `src/`
that the sound index doesn't define (a typo or a renamed event).

Usage:
    tools/sound_survey.py [--all] [SOUNDS_JSON]

SOUNDS_JSON defaults to the newest installed index (see check_subtitles.py).
Vanilla source is the newest reference/minecraft-*/ decompile. A file counts
as client-played if it lives under net/minecraft/client/ or calls
playLocalSound; --all lists every file that names a sound event.
"""

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_subtitles import newest_sounds  # noqa: E402

REPO = Path(__file__).resolve().parent.parent

REGISTER = re.compile(r'(\w+)\s*=\s*register\w*\(\s*"([a-z0-9_./]+)"')
REFERENCE = re.compile(r"SoundEvents\.(\w+)")
LITERAL = re.compile(r'"(?:minecraft:)?([a-z0-9_]+(?:\.[a-z0-9_]+)+)"')
SOUNDISH = ("ambient.", "block.", "entity.", "item.", "music.", "music_disc.",
            "ui.", "weather.", "event.", "enchant.", "particle.", "intentionally_empty")


def looks_like_other_key(s: str) -> bool:
    """Translation keys, block id numbers and file names share the prefixes."""
    parts = s.split(".")
    return ("minecraft" in parts or any(p.isdigit() for p in parts)
            or parts[-1] in ("wgsl", "properties", "components", "durability", "unbreakable", "huge"))


def newest_reference() -> Path:
    found = sorted(p for p in REPO.glob("reference/minecraft-*") if (p / "net").is_dir())
    if not found:
        sys.exit("no reference decompile")
    return found[-1]


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    every = "--all" in sys.argv
    sounds = set(json.loads((Path(args[0]) if args else newest_sounds()).read_text(encoding="utf-8")))
    ref = newest_reference()

    events = {}
    for name, sound in REGISTER.findall((ref / "net/minecraft/sounds/SoundEvents.java").read_text(encoding="utf-8")):
        events[name] = sound

    ours = {}
    for path in sorted((REPO / "src").rglob("*.rs")):
        for sound in LITERAL.findall(path.read_text(encoding="utf-8", errors="replace")):
            ours.setdefault(sound, set()).add(path.relative_to(REPO).as_posix())

    print(f"vanilla: {ref.name}; {len(events)} SoundEvents fields, {len(sounds)} events in the index")
    print(f"ours: {sum(1 for s in ours if s in sounds)} known events mentioned in src/\n")

    unknown = sorted(s for s in ours if s not in sounds and s.startswith(SOUNDISH)
                     and not looks_like_other_key(s))
    print("## Sound ids in src/ the index doesn't define\n")
    for sound in unknown:
        print(f"- `{sound}` in {', '.join(sorted(ours[sound]))}")
    if not unknown:
        print("(none)")

    print("\n## Vanilla client sounds nothing in src/ mentions\n")
    missing_total = 0
    for path in sorted((ref / "net/minecraft").rglob("*.java")):
        text = path.read_text(encoding="utf-8", errors="replace")
        rel = path.relative_to(ref).as_posix()
        client = rel.startswith("net/minecraft/client/") or "playLocalSound" in text
        if not (client or every):
            continue
        named = {events[n] for n in REFERENCE.findall(text) if n in events}
        missing = sorted(s for s in named if s not in ours)
        if missing:
            missing_total += len(missing)
            print(f"### {rel}\n")
            for sound in missing:
                print(f"- `{sound}`")
            print()
    print(f"{missing_total} missing across the files listed; {len(unknown)} unknown ids")
    return 0


if __name__ == "__main__":
    sys.exit(main())
