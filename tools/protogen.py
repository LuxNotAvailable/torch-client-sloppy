#!/usr/bin/env python3
"""Generate the per-version protocol tables in `src/protocol/data/`.
`tools/new_version.py` fills `reference/` and runs this.

    python3 protogen.py packets [--source PATH] [--version NAME] [--output PATH]
    python3 protogen.py remap --from NAME --to NAME [--output PATH]
    mkdir -p /tmp/gen && cd /tmp/gen
    java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar          --reports --output /tmp/gen
    cp /tmp/gen/reports/{registries,blocks}.json        <crate>/reference/minecraft-<version>/generated/reports/
    python3 protogen.py packets [--source PATH] [--version NAME] [--output PATH]
    --version 26.3
    --source  <crate>/reference/minecraft-<version>
    --output  <crate>/src/protocol/data/packets-<version>.json
    protocol number   <source>/version.json  ("protocol_version")
    packet names      <source>/net/minecraft/network/protocol/**/*PacketTypes.java
    packet ids        <source>/net/minecraft/network/protocol/<phase>/<Phase>Protocols.java
    `SERVERBOUND_SET_CREATIVE_MODE_SLOT`. The modifier is a runtime
    `CodecModifier`, not a condition, so the packet still takes exactly one id.
    clientbound game id 0. The id belongs to the **delimiter**, which is the
    packet that goes on the wire; `ClientboundBundlePacket` is a container that
    never does. Naming the id after the first argument would put `bundle` where
    `bundle_delimiter` belongs, which azalea's own generated table
    (`azalea-protocol/src/packets/game/mod.rs`) and vanilla both disagree with.
    registry ids     <source>/generated/reports/registries.json
    block state ids  <source>/generated/reports/blocks.json
    serializer ids   <source>/net/minecraft/network/syncher/EntityDataSerializers.java
"""

import argparse
import json
import re
import sys
from pathlib import Path

CRATE = Path(__file__).resolve().parent.parent

PHASES = [
    ("handshake", "handshake", "HandshakeProtocols"),
    ("status", "status", "StatusProtocols"),
    ("login", "login", "LoginProtocols"),
    ("configuration", "configuration", "ConfigurationProtocols"),
    ("game", "game", "GameProtocols"),
]

DECL = re.compile(
    r"PacketType<\s*([A-Za-z0-9_.]+)\s*>\s+([A-Z0-9_]+)\s*=\s*"
    r"create(Clientbound|Serverbound)\(\s*\"([a-z0-9_/.]+)\"\s*\)"
)
TEMPLATE = re.compile(r"ProtocolInfoBuilder\.([A-Za-z]+)\(\s*ConnectionProtocol\.")

class Fail(Exception):
    pass

def read(path):
    if not path.is_file():
        raise Fail(f"missing {path}")
    return path.read_text(encoding="utf-8")

def packet_names(protocol_dir):
    by_constant = {}
    by_class = {}
    files = sorted(protocol_dir.glob("*/*PacketTypes.java"))
    if not files:
        raise Fail(f"no *PacketTypes.java under {protocol_dir}")
    for path in files:
        owner = path.stem
        for packet_class, constant, _flow, resource in DECL.findall(read(path)):
            key = f"{owner}.{constant}"
            if key in by_constant:
                raise Fail(f"{key} declared twice")
            by_constant[key] = resource
            by_class.setdefault(packet_class, resource)
    return by_constant, by_class

def split_args(text, start):
    depth = 0
    args = []
    current = start + 1
    i = current
    while i < len(text):
        c = text[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                args.append(text[current:i].strip())
                return args, i + 1
            depth -= 1
        elif c == "," and depth == 0:
            args.append(text[current:i].strip())
            current = i + 1
        elif c == '"':
            i = text.index('"', i + 1)
        i += 1
    raise Fail("unbalanced parentheses in a protocol builder chain")

def template_bodies(source):
    out = []
    for match in TEMPLATE.finditer(source):
        method = match.group(1)
        if "erverbound" in method:
            direction = "serverbound"
        elif "lientbound" in method:
            direction = "clientbound"
        else:
            raise Fail(f"cannot tell the direction of ProtocolInfoBuilder.{method}")
        args, _ = split_args(source, match.end() - 1)
        if len(args) != 2:
            raise Fail(f"ProtocolInfoBuilder.{method} took {len(args)} arguments, expected 2")
        out.append((direction, args[1]))
    return out

def scan(body, by_constant, by_class, where):
    names = []
    for match in re.finditer(r"\.(addPacket|withBundlePacket)\(", body):
        call = match.group(1)
        args, _ = split_args(body, match.end() - 1)
        if call == "addPacket":
            if len(args) not in (2, 3):
                raise Fail(f"{where}: addPacket took {len(args)} arguments")
            key = args[0]
            if key not in by_constant:
                raise Fail(f"{where}: unknown packet type {key}")
            names.append(by_constant[key])
        else:
            if len(args) != 3:
                raise Fail(f"{where}: withBundlePacket took {len(args)} arguments")
            delimiter = re.fullmatch(r"new\s+([A-Za-z0-9_]+)\(\s*\)", args[2])
            if not delimiter:
                raise Fail(f"{where}: cannot read a delimiter out of {args[2]!r}")
            packet_class = delimiter.group(1)
            if packet_class not in by_class:
                raise Fail(f"{where}: no packet type declares {packet_class}")
            names.append(by_class[packet_class])
    return names

CLIENT_REGISTRIES = [
    "attribute",
    "block",
    "block_entity_type",
    "data_component_type",
    "entity_type",
    "game_event",
    "item",
    "menu",
    "mob_effect",
    "particle_type",
    "potion",
    "sound_event",
    "villager_profession",
    "villager_type",
    "command_argument_type",
]

# Entries renamed between versions: old name -> new name, per id space.
# Applied both ways. Add one only after the hole report (and the decompile)
# shows the rename; the report suggests likely ones.
RENAMES = {
    "block": {"minecraft:chain": "minecraft:iron_chain"},  # 1.21.9
    "item": {"minecraft:chain": "minecraft:iron_chain"},  # 1.21.9
    "sound_event": {
        # Merged into the plain goat's sound (GoatAi).
        "minecraft:entity.goat.screaming.horn_break": "minecraft:entity.goat.horn_break",
        # LeashFenceKnotEntity: dropping, then placing a knot.
        "minecraft:entity.leash_knot.break": "minecraft:item.lead.untied",
        "minecraft:entity.leash_knot.place": "minecraft:item.lead.tied",
    },
    "entity_data_serializer": {"OPTIONAL_UUID": "OPTIONAL_LIVING_ENTITY_REFERENCE"},
    "command_argument_type": {"minecraft:color": "minecraft:team_color"},  # 26.3
}

# Prefixes a whole space dropped: `generic.armor` became `armor` (1.21.2).
RENAME_PREFIXES = {
    "attribute": ("minecraft:generic.", "minecraft:player.", "minecraft:zombie."),
}

SERIALIZERS = "net/minecraft/network/syncher/EntityDataSerializers.java"

def reports(source, name):
    path = source / "generated" / "reports" / f"{name}.json"
    if not path.is_file():
        raise Fail(
            f"missing {path}; run the server jar's data generator, see this script's header"
        )
    return json.loads(read(path))

def registry_ids(source, registry):
    key = f"minecraft:{registry}"
    data = reports(source, "registries")
    if key not in data:
        raise Fail(f"{source} has no {key} registry")
    return {name: e["protocol_id"] for name, e in data[key]["entries"].items()}

def block_state_ids(source):
    out = {}
    for block, info in reports(source, "blocks").items():
        for state in info["states"]:
            props = tuple(sorted(state.get("properties", {}).items()))
            out[(block, props)] = state["id"]
    return out

def block_defaults(source):
    """Each block's default state properties."""
    out = {}
    for block, info in reports(source, "blocks").items():
        state = next((s for s in info["states"] if s.get("default")), info["states"][0])
        out[block] = dict(state.get("properties", {}))
    return out

def renamed(name, renames, prefixes, forward):
    """The names `name` may have been renamed to (or from, going back)."""
    out = [renames[name]] if name in renames else []
    for prefix in prefixes:
        if forward and name.startswith(prefix):
            out.append("minecraft:" + name[len(prefix):])
        elif not forward and name.startswith("minecraft:"):
            out.append(prefix + name[len("minecraft:"):])
    return out

def landing_by_name(source, target, renames, prefixes=(), forward=True):
    """Where each source name lands: itself, or a rename. A rename may share
    its target (two entries merged)."""
    out = {}
    for name in source:
        if name in target:
            out[name] = (target[name], True)
            continue
        hit = next((n for n in renamed(name, renames, prefixes, forward) if n in target), None)
        if hit is not None:
            out[name] = (target[hit], False)
    return out

def landing_by_state(source, target, renames, target_defaults):
    """States land exactly, or on the closest state of the same block: shared
    properties kept, new ones at the block's default. A fallback landing may
    share its target with another state (a property was dropped)."""
    by_block = {}
    for (block, props), id in target.items():
        by_block.setdefault(block, []).append((dict(props), id))
    out = {}
    for key, _ in source.items():
        block, props = key
        new_block = block if block in by_block else renames.get(block)
        if new_block not in by_block:
            continue
        exact = target.get((new_block, props))
        if exact is not None:
            out[key] = (exact, True)
            continue
        old = dict(props)
        default = target_defaults.get(new_block, {})

        def score(candidate):
            props, id = candidate
            shared = sum(1 for k, v in old.items() if props.get(k) == v)
            # New properties, and values the target lacks, at the default.
            unmatched = sum(1 for k, v in props.items() if old.get(k) != v and default.get(k) == v)
            return (shared, unmatched, -id)

        out[key] = (max(by_block[new_block], key=score)[1], False)
    return out

def suggest_rename(name, unmatched_targets):
    """A likely new name for an entry with no counterpart, or None."""
    ns, _, path = name.partition(":")
    for new in unmatched_targets:
        new_ns, _, new_path = new.partition(":")
        if new_ns != ns:
            continue
        if new_path.endswith("_" + path) or path.endswith("." + new_path):
            return new
    return None

def serializer_ids(source):
    text = read(source / SERIALIZERS)
    names = re.findall(r"registerSerializer\(([A-Z0-9_]+)\)", text)
    if not names:
        raise Fail(f"no registerSerializer calls in {source / SERIALIZERS}")
    return {name: i for i, name in enumerate(names)}

def runs(source, landings, what):
    """`landings` maps a source name to (target id, exact). Two exact
    landings on one target are a bug; a fallback may share."""
    by_id = {}
    for name, id in source.items():
        if id in by_id:
            raise Fail(f"{what}: id {id} is claimed by two entries")
        by_id[id] = name

    out = []
    previous = ()
    taken = {}
    for id in range(len(by_id)):
        name = by_id.get(id)
        if name is None:
            raise Fail(f"{what}: the source version has a hole at id {id}")
        landing, exact = landings.get(name, (None, True))
        if landing is not None and exact:
            if landing in taken:
                raise Fail(f"{what}: ids {taken[landing]} and {id} both land on {landing}")
            taken[landing] = id
        offset = None if landing is None else landing - id
        if (offset,) != previous:
            out.append([id, offset])
            previous = (offset,)
    return out

def hole_report(space, old_ids, new_ids, landings, limit=12):
    """Lines naming the forward holes, with a rename guess where one fits."""
    holes = sorted((n for n in old_ids if n not in landings), key=old_ids.get)
    if not holes:
        return []
    if space == "block_state":
        blocks = sorted({b for b, _ in holes})
        lines = [f"    {len(blocks)} block(s) with no states in the target: {', '.join(blocks[:limit])}"]
        return lines + (["    ..."] if len(blocks) > limit else [])
    unmatched = sorted(set(new_ids) - {n for n in new_ids if n in old_ids})
    lines = []
    for name in holes[:limit]:
        guess = suggest_rename(name, unmatched)
        lines.append(f"    {name}" + (f"  (renamed to {guess}?)" if guess else ""))
    if len(holes) > limit:
        lines.append(f"    ... {len(holes) - limit} more")
    return lines

def generate_remap(old_source, old_version, new_source, new_version, out):
    def meta(source, version):
        info = json.loads(read(source / "version.json"))
        if info.get("id") != version:
            raise Fail(f"{source}/version.json is {info.get('id')!r}, asked for {version!r}")
        return {"version": version, "protocol": info["protocol_version"]}

    old_meta, new_meta = meta(old_source, old_version), meta(new_source, new_version)
    if old_meta["protocol"] == new_meta["protocol"]:
        raise Fail("both versions speak the same protocol; there is nothing to remap")

    sources = [(r, registry_ids) for r in CLIENT_REGISTRIES]
    sources.append(("block_state", block_state_ids))
    sources.append(("entity_data_serializer", serializer_ids))

    table = {"from": old_meta, "to": new_meta, "forward": {}, "back": {}}
    identity = []
    notes = {}
    for name, reader in sources:
        if reader is registry_ids:
            old_ids, new_ids = reader(old_source, name), reader(new_source, name)
        else:
            old_ids, new_ids = reader(old_source), reader(new_source)
        if name == "block_state":
            renames = RENAMES.get("block", {})
            back_renames = {new: old for old, new in renames.items()}
            forward_land = landing_by_state(old_ids, new_ids, renames, block_defaults(new_source))
            back_land = landing_by_state(new_ids, old_ids, back_renames, block_defaults(old_source))
        else:
            renames = RENAMES.get(name, {})
            back_renames = {new: old for old, new in renames.items()}
            prefixes = RENAME_PREFIXES.get(name, ())
            forward_land = landing_by_name(old_ids, new_ids, renames, prefixes, True)
            back_land = landing_by_name(new_ids, old_ids, back_renames, prefixes, False)
        forward = runs(old_ids, forward_land, name)
        back = runs(new_ids, back_land, f"{name} (reversed)")
        table["forward"][name] = {"count": len(old_ids), "runs": forward}
        table["back"][name] = {"count": len(new_ids), "runs": back}
        if forward == [[0, 0]] and len(old_ids) == len(new_ids):
            identity.append(name)
        fallback = sum(1 for _, exact in forward_land.values() if not exact)
        notes[name] = (
            len(old_ids) - len(forward_land),
            fallback,
            hole_report(name, old_ids, new_ids, forward_land),
        )

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(table, indent=2) + "\n", encoding="utf-8")
    print(
        f"protogen: {old_meta['version']} ({old_meta['protocol']})"
        f" -> {new_meta['version']} ({new_meta['protocol']}) -> {out}"
    )
    for name, _ in sources:
        forward = table["forward"][name]
        note = "identity" if name in identity else f"{len(forward['runs'])} runs"
        holes = sum(1 for _, offset in table["back"][name]["runs"] if offset is None)
        holes = f", {holes} back with no counterpart" if holes else ""
        lost, fallback, lines = notes[name]
        lost = f", {lost} forward with no counterpart" if lost else ""
        how = "by nearest state" if name == "block_state" else "by rename"
        fallback = f", {fallback} {how}" if fallback else ""
        print(f"  {name:<24} {forward['count']:>6} ids  {note}{holes}{lost}{fallback}")
        for line in lines:
            print(line)

def generate(source, version, out):
    meta = json.loads(read(source / "version.json"))
    protocol = meta.get("protocol_version")
    if not isinstance(protocol, int):
        raise Fail(f"{source}/version.json has no integer protocol_version")
    if meta.get("id") != version:
        raise Fail(f"{source}/version.json is {meta.get('id')!r}, asked for {version!r}")

    protocol_dir = source / "net" / "minecraft" / "network" / "protocol"
    by_constant, by_class = packet_names(protocol_dir)

    table = {"version": version, "protocol": protocol}
    for key, directory, stem in PHASES:
        text = read(protocol_dir / directory / f"{stem}.java")
        phase = {"serverbound": [], "clientbound": []}
        seen = set()
        for direction, body in template_bodies(text):
            if direction in seen:
                raise Fail(f"{stem}: two {direction} templates")
            seen.add(direction)
            phase[direction] = scan(body, by_constant, by_class, f"{stem}/{direction}")
        table[key] = phase

    for key, _, _ in PHASES:
        for direction, names in table[key].items():
            if len(set(names)) != len(names):
                raise Fail(f"{key}/{direction} registers a packet twice")
    if not table["game"]["clientbound"]:
        raise Fail("no clientbound game packets")
    if table["game"]["clientbound"][0] != "bundle_delimiter":
        raise Fail(
            "game clientbound id 0 is "
            f"{table['game']['clientbound'][0]}, expected bundle_delimiter"
        )

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(table, indent=2) + "\n", encoding="utf-8")
    total = sum(len(d) for k, _, _ in PHASES for d in table[k].values())
    print(f"protogen: {version} (protocol {protocol}), {total} packets -> {out}")
    for key, _, _ in PHASES:
        counts = {d: len(n) for d, n in table[key].items()}
        print(f"  {key:<14} {counts['serverbound']:>3} serverbound  {counts['clientbound']:>3} clientbound")

def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    modes = ap.add_subparsers(dest="mode", required=True)

    packets = modes.add_parser("packets", help="packet-id tables from decompiled source")
    packets.add_argument("--version", default="26.3")
    packets.add_argument("--source")
    packets.add_argument("--output")

    remap = modes.add_parser("remap", help="one version's ids into another's")
    remap.add_argument("--from", dest="old", required=True)
    remap.add_argument("--to", dest="new", required=True)
    remap.add_argument("--source-from", dest="old_source")
    remap.add_argument("--source-to", dest="new_source")
    remap.add_argument("--output")

    args = ap.parse_args()
    tree = lambda version: CRATE / "reference" / f"minecraft-{version}"
    data = CRATE / "src" / "protocol" / "data"

    try:
        if args.mode == "packets":
            source = Path(args.source) if args.source else tree(args.version)
            out = Path(args.output) if args.output else data / f"packets-{args.version}.json"
            generate(source, args.version, out)
        else:
            old_source = Path(args.old_source) if args.old_source else tree(args.old)
            new_source = Path(args.new_source) if args.new_source else tree(args.new)
            if args.output:
                out = Path(args.output)
            else:
                numbers = [
                    json.loads((s / "version.json").read_text())["protocol_version"]
                    for s in (old_source, new_source)
                ]
                out = data / f"remap-{numbers[0]}-{numbers[1]}.json"
            generate_remap(old_source, args.old, new_source, args.new, out)
    except Fail as e:
        print(f"protogen: {e}", file=sys.stderr)
        return 1
    return 0

if __name__ == "__main__":
    sys.exit(main())
