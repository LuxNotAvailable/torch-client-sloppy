#!/usr/bin/env python3
"""Surveys which sounds the client plays against what vanilla's client plays.

Most sounds reach the client as `ClientboundSound*` packets and play through
one generic path. The rest the client must play itself:

- code under net/minecraft/client/ (UI, ambience, music, sound instances);
- `playLocalSound` calls, which only ever play on the client (block
  `animateTick`s, level events);
- `playSound(player, ...)` calls in shared code: the server sends the sound to
  everyone but `player`, so the acting player's own client plays it when it
  runs the same code (placing, using doors and trapdoors, item use);
- `player.playSound(...)` / `entity.playSound(...)`, which play locally for
  the local player.

For each call site the tool collects the `SoundEvents` it names, and lists
those nothing in `src/` mentions. Sites that pick their sound at run time
(`getPlaceSound()`, a variable) are listed as dynamic for a manual look.
It also lists every sound id written in `src/` that the sound index doesn't
define.

Usage:
    tools/sound_survey.py [--all] [SOUNDS_JSON]

SOUNDS_JSON defaults to the newest installed index (see check_subtitles.py).
Vanilla source is the newest reference/minecraft-*/ decompile. --all also
lists covered call sites.
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
CALL = re.compile(r"(\b[\w.()]+?)?\.?\b(playSound|playLocalSound|playPlayerSound)\s*\(")
# First arguments that name the acting entity: the call plays for it alone.
ACTOR = re.compile(r"^\s*(player|context\.getPlayer\(\)|useContext\.getPlayer\(\)|"
                   r"ctx\.getPlayer\(\)|user|livingEntity|owner|holder)\s*$")
# Where an actor or `entity.playSound` can be the local player: item and block
# use, and the player and living-entity code it runs through.
PLAYER_CODE = ("net/minecraft/world/item/", "net/minecraft/world/level/block/",
               "net/minecraft/world/entity/player/", "net/minecraft/world/entity/LivingEntity.java",
               "net/minecraft/world/entity/Entity.java", "net/minecraft/world/entity/vehicle/",
               "net/minecraft/world/entity/projectile/", "net/minecraft/world/inventory/")


# Sites checked by hand: they run only on the server (an isClientSide
# guard, a ServerLevel cast, or entity.playSound on an entity that's never the
# local player), so the server's packet covers them. Recheck on each port.
SERVER_ONLY = {
    "world/entity/Entity.java": "leash tying runs in the server's interact",
    "world/entity/projectile/arrow/ThrownTrident.java": "Entity.playSound on a trident plays nothing client-side",
    "world/entity/projectile/EyeOfEnder.java": "behind !isClientSide",
    "world/entity/projectile/FishingHook.java": "server fishing tick (sendParticles)",
    "world/entity/projectile/ShulkerBullet.java": "ServerLevel cast",
    "world/level/block/entity/BarrelBlockEntity.java": "the openers counter runs on the server",
    "world/level/block/entity/BeaconBlockEntity.java": "behind !isClientSide",
    "world/level/block/ShelfBlock.java": "neighbour updates and slot swaps run on the server",
    "client/sounds/SoundEngine.java": "intentionally_empty is a placeholder",
    "client/sounds/SoundPreviewHandler.java": "the options screen's sound preview",
}


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


def call_args(text: str, open_at: int) -> str:
    """The argument text of the call whose `(` is at `open_at`."""
    depth = 0
    for i in range(open_at, len(text)):
        c = text[i]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return text[open_at + 1:i]
    return text[open_at + 1:]


def first_arg(args: str) -> str:
    depth = 0
    for i, c in enumerate(args):
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif c == "," and depth == 0:
            return args[:i]
    return args


def client_heard(rel: str, receiver: str, method: str, args: str) -> str | None:
    """Why the client plays this call itself, or None if the server's packet
    covers it."""
    if rel.startswith("net/minecraft/client/"):
        return "client"
    if method == "playLocalSound":
        return "local"
    if method == "playPlayerSound":
        return "actor"
    if method == "playSound" and rel.startswith(PLAYER_CODE):
        if receiver and receiver not in ("level", "this.level()", "level()", "serverLevel",
                                         "this.level", "world", "clientLevel"):
            # entity.playSound(...): Entity.playSound plays on whichever side
            # runs it, so the local player's own runs are client-heard.
            return "entity"
        first = first_arg(args)
        if ACTOR.match(first):
            return "actor"
    return None


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

    missing, dynamic, covered = [], [], 0
    for path in sorted((ref / "net/minecraft").rglob("*.java")):
        text = path.read_text(encoding="utf-8", errors="replace")
        rel = path.relative_to(ref).as_posix()
        whole_file = rel.startswith("net/minecraft/client/")
        sites = []
        for m in CALL.finditer(text):
            receiver = (m.group(1) or "").rstrip(".")
            call_text = call_args(text, m.end() - 1)
            why = client_heard(rel, receiver, m.group(2), call_text)
            if why:
                sites.append((text.count("\n", 0, m.start()) + 1, why, call_text))
        if whole_file:
            # Client code also picks sounds outside the call (sound instances).
            named = {events[n] for n in REFERENCE.findall(text) if n in events}
            sites.append((0, "client", " ".join(f"SoundEvents.{n}" for n in REFERENCE.findall(text))))
        for line, why, call_text in sites:
            named = sorted({events[n] for n in REFERENCE.findall(call_text) if n in events})
            if not named and line:
                dynamic.append((rel, line, why, " ".join(call_text.split())[:90]))
                continue
            gaps = [s for s in named if s not in ours]
            if gaps:
                missing.append((rel, line, why, gaps))
            elif named:
                covered += 1
                if every:
                    missing.append((rel, line, why + ", covered", named))

    print("\n## Client-heard sounds nothing in src/ mentions\n")
    print("Kinds: client = client code, local = playLocalSound, actor = playSound for the")
    print("acting player, entity = Entity.playSound run by the local player.\n")
    total = skipped = 0
    for rel, line, why, gaps in missing:
        if rel.removeprefix("net/minecraft/") in SERVER_ONLY and "covered" not in why:
            skipped += 1
            continue
        total += len(gaps)
        where = f"{rel.removeprefix('net/minecraft/')}" + (f":{line}" if line else "")
        print(f"- {where} ({why}): " + ", ".join(f"`{g}`" for g in gaps))

    print("\n## Dynamic sites (the sound is chosen at run time; check by hand)\n")
    for rel, line, why, call_text in dynamic:
        print(f"- {rel.removeprefix('net/minecraft/')}:{line} ({why}): `{call_text}`")

    print(f"\n{total} missing events at {len(missing) - skipped} sites "
          f"({skipped} server-only skipped); {covered} covered sites; "
          f"{len(dynamic)} dynamic sites; {len(unknown)} unknown ids")
    return 0


if __name__ == "__main__":
    sys.exit(main())
