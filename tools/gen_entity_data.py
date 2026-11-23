#!/usr/bin/env python3
"""Entity data layouts for one game version, from its decompiled source.

    py -3 tools/gen_entity_data.py 1.21.1 [--source PATH] [--output PATH]

Writes `src/protocol/data/entity-data-<version>.json`:

    {"version": "1.21.1",
     "serializers": ["BYTE", "INT", ...],              # by network id
     "classes": {"Cow": {"parent": "Animal",
                         "fields": [["DATA_X", "BYTE"], ...]}},
     "types": {"minecraft:cow": {"id": 23, "class": "Cow"}}}

A field's index is its class's position past the last index its superclass
chain used (`SynchedEntityData.defineId` and `ClassTreeIdRegistry`), in the
order the class declares its `defineId` fields. Classes are keyed by simple
name; nested classes (`Display.TextDisplay`) by their own name.

    registry ids     <source>/generated/reports/registries.json
    entity classes   <source>/net/minecraft/world/entity/EntityType.java
    fields           <source>/net/minecraft/**/*.java
"""

import argparse
import json
import re
import sys
from pathlib import Path

CRATE = Path(__file__).resolve().parent.parent

CLASS = re.compile(r"\bclass\s+(\w+)(?:<[^{]*?>)?\s+extends\s+([\w.]+)")
DEFINE = re.compile(
    r"(\w+)\s*=\s*SynchedEntityData\.defineId\(\s*([\w.]+)\.class\s*,\s*"
    r"EntityDataSerializers\.(\w+)\s*\)"
)
SERIALIZER = re.compile(r"registerSerializer\(([A-Z0-9_]+)\)")
REGISTER = re.compile(r'register\(\s*"([a-z0-9_]+)"\s*,')

# Entity types built by a factory method rather than `Class::new`.
FACTORIES = {
    "boatFactory": "Boat",
    "chestBoatFactory": "ChestBoat",
    "raftFactory": "Raft",
    "chestRaftFactory": "ChestRaft",
}


class Fail(Exception):
    pass


def simple(name):
    return name.rsplit(".", 1)[-1]


def scan(source):
    """Superclass and `defineId` fields per class, over every source file."""
    parents, fields = {}, {}
    for path in (source / "net" / "minecraft").rglob("*.java"):
        text = path.read_text(encoding="utf-8", errors="replace")
        if "defineId" not in text and "extends" not in text:
            continue
        for child, parent in CLASS.findall(text):
            parents.setdefault(child, simple(parent))
        for field, owner, serializer in DEFINE.findall(text):
            fields.setdefault(simple(owner), []).append([field, serializer])
    return parents, fields


def entity_classes(source):
    """Entity type name -> the class its builder makes."""
    # 26.3: `EntityType<Cow> COW = register(EntityTypeIds.COW, ...)` in
    # `EntityTypes.java`; the field's type names the class.
    types = source / "net/minecraft/world/entity/EntityTypes.java"
    if types.is_file():
        text = types.read_text(encoding="utf-8")
        found = re.findall(
            r"EntityType<([\w.]+)>\s+\w+\s*=\s*register\(\s*EntityTypeIds\.(\w+)", text
        )
        return {constant.lower(): simple(cls) for cls, constant in found}
    text = (source / "net/minecraft/world/entity/EntityType.java").read_text(encoding="utf-8")
    out = {}
    matches = list(REGISTER.finditer(text))
    for i, m in enumerate(matches):
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        call = text[m.end() : end]
        found = re.search(r"Builder\.<(\w+)>", call) or re.search(
            r"(\w+)::(?:new|create)\b", call
        )
        if found:
            out[m.group(1)] = found.group(1)
            continue
        factory = re.search(r"(\w+Factory)\(", call)
        if factory and factory.group(1) in FACTORIES:
            out[m.group(1)] = FACTORIES[factory.group(1)]
    return out


def generate(source, version, out):
    info = json.loads((source / "version.json").read_text(encoding="utf-8"))
    if info.get("id") != version:
        raise Fail(f"{source}/version.json is {info.get('id')!r}, asked for {version!r}")
    registries = json.loads(
        (source / "generated/reports/registries.json").read_text(encoding="utf-8")
    )
    type_ids = {
        name: e["protocol_id"]
        for name, e in registries["minecraft:entity_type"]["entries"].items()
    }
    serializers = SERIALIZER.findall(
        (source / "net/minecraft/network/syncher/EntityDataSerializers.java").read_text(
            encoding="utf-8"
        )
    )
    parents, fields = scan(source)
    classes_of = entity_classes(source)

    types, missing = {}, []
    needed = set()
    for name, id in sorted(type_ids.items(), key=lambda e: e[1]):
        cls = classes_of.get(name.removeprefix("minecraft:"))
        if cls is None:
            missing.append(name)
            continue
        types[name] = {"id": id, "class": cls}
        while cls is not None and cls not in needed:
            needed.add(cls)
            cls = parents.get(cls)

    classes = {
        cls: {"parent": parents.get(cls), "fields": fields.get(cls, [])}
        for cls in sorted(needed)
    }
    table = {
        "version": version,
        "serializers": serializers,
        "classes": classes,
        "types": types,
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(table, indent=1) + "\n", encoding="utf-8")
    print(f"gen_entity_data: {version}: {len(types)} types, {len(classes)} classes -> {out}")
    if missing:
        print(f"  no class found for: {', '.join(missing)}")
    roots = sorted({c for c in classes if classes[c]["parent"] is None})
    print(f"  root classes: {', '.join(roots)}")


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("version")
    ap.add_argument("--source")
    ap.add_argument("--output")
    args = ap.parse_args()
    source = Path(args.source) if args.source else CRATE / "reference" / f"minecraft-{args.version}"
    out = (
        Path(args.output)
        if args.output
        else CRATE / "src/protocol/data" / f"entity-data-{args.version}.json"
    )
    try:
        generate(source, args.version, out)
    except Fail as e:
        print(f"gen_entity_data: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
