#!/usr/bin/env python3
"""Generate `src/play/placement_rules.rs` from the game itself.

Reads what `tools/DumpGameData.java` prints (see `gen_push_reactions.py` for
how to run it) and writes, per block, which vanilla `canSurvive` /
`mayPlaceOn` rule placement prediction checks (`play::survival::Rule`), the
items that place a block of another name, and the standing/wall pairs of
`StandingAndWallBlockItem`s:

    python3 gen_placement_rules.py game.txt > <crate>/src/play/placement_rules.rs

The first class in a block's class chain (most derived first) with an entry
in RULES decides its rule. A tag of None means `supports_<block id>`.
"""

import sys

# Vanilla block class -> (Rule variant, tag or None for supports_<id>).
RULES = {
    "CropBlock": ("Crop", "supports_crops"),
    "PitcherCropBlock": ("Crop", "supports_crops"),
    "StemBlock": ("BelowTag", None),
    "NetherWartBlock": ("BelowTag", "supports_nether_wart"),
    "NetherFungusBlock": ("BelowTag", None),
    "NetherRootsBlock": ("BelowTag", None),
    "NetherSproutsBlock": ("BelowTag", "supports_nether_sprouts"),
    "WitherRoseBlock": ("BelowTag", "supports_wither_rose"),
    "AzaleaBlock": ("BelowTag", "supports_azalea"),
    "MangrovePropaguleBlock": ("BelowTag", "supports_mangrove_propagule"),
    "DryVegetationBlock": ("BelowTag", "supports_dry_vegetation"),
    "BambooSaplingBlock": ("BelowTag", "supports_bamboo"),
    "CactusFlowerBlock": ("CactusFlower", None),
    "SeagrassBlock": ("Seagrass", None),
    "LeafLitterBlock": ("SturdyBelow", None),
    "SmallDripleafBlock": ("SmallDripleaf", None),
    "MushroomBlock": ("Mushroom", None),
    "LilyPadBlock": ("Unpredicted", None),
    "VegetationBlock": ("BelowTag", "supports_vegetation"),
    "BaseRailBlock": ("RigidBelow", None),
    "DiodeBlock": ("RigidBelow", None),
    "BasePressurePlateBlock": ("PressurePlate", None),
    "RedstoneWireBlock": ("RedstoneWire", None),
    "CandleBlock": ("CenterBelow", None),
    "MossyCarpetBlock": ("MossyCarpet", None),
    "CarpetBlock": ("NotAirBelow", None),
    "SnowLayerBlock": ("Snow", None),
    "SeaPickleBlock": ("SeaPickle", None),
    "BaseCoralWallFanBlock": ("SturdyBehind", None),
    "BaseCoralPlantTypeBlock": ("SturdyBelow", None),
    "CakeBlock": ("SolidBelow", None),
    "CandleCakeBlock": ("SolidBelow", None),
    "StandingSignBlock": ("SolidBelow", None),
    "BannerBlock": ("SolidBelow", None),
    "WallSignBlock": ("SolidBehind", None),
    "WallBannerBlock": ("SolidBehind", None),
    "CeilingHangingSignBlock": ("CenterAbove", None),
    "WallHangingSignBlock": ("WallHangingSign", None),
    "LadderBlock": ("SturdyBehind", None),
    "TripWireHookBlock": ("SturdyBehind", None),
    "AmethystClusterBlock": ("SturdyBehind", None),
    "ShelfMushroomBlock": ("SturdyBehind", None),
    "CocoaBlock": ("Cocoa", None),
    "WallTorchBlock": ("SturdyBehind", None),
    "RedstoneWallTorchBlock": ("SturdyBehind", None),
    "BaseTorchBlock": ("CenterBelow", None),
    "HangingRootsBlock": ("SturdyAbove", None),
    "HangingMossBlock": ("SturdyAbove", None),
    "SporeBlossomBlock": ("SporeBlossom", None),
    "SugarCaneBlock": ("SugarCane", None),
    "CactusBlock": ("Cactus", None),
    "BigDripleafBlock": ("BigDripleaf", None),
    "DoorBlock": ("Door", None),
    "FrogspawnBlock": ("Unpredicted", None),
    "VineBlock": ("Unpredicted", None),
    "MultifaceBlock": ("Unpredicted", None),
    "GrowingPlantBlock": ("Unpredicted", None),
    "ChorusFlowerBlock": ("Unpredicted", None),
    "ChorusPlantBlock": ("Unpredicted", None),
    "BubbleColumnBlock": ("Unpredicted", None),
    "FireBlock": ("Unpredicted", None),
    "SoulFireBlock": ("Unpredicted", None),
}

# Blocks whose placement also writes the block above (DoublePlantBlock and
# DoorBlock `setPlacedBy`, on the client too) or the bed head.
UPPER_HALF = {"DoorBlock", "DoublePlantBlock"}
NO_UPPER_HALF = {"PitcherCropBlock", "SmallDripleafBlock", "TallSeagrassBlock"}
BED = {"AbstractBedBlock"}


def main():
    classes, items = {}, {}
    with open(sys.argv[1], encoding="utf-8", errors="ignore") as dump:
        for line in dump:
            parts = line.split()
            if parts[:1] == ["class"]:
                classes[parts[1]] = parts[2:]
            elif parts[:1] == ["blockitem"]:
                items[parts[1]] = parts[2:]
    if not classes or not items:
        sys.exit("no blocks were read; is this the DumpGameData output?")

    by_rule = {}
    upper, beds = [], []
    for block, chain in sorted(classes.items()):
        for c in chain:
            if c in RULES:
                rule, tag = RULES[c]
                if rule in ("BelowTag", "Crop"):
                    rule = '%s("%s")' % (rule, tag or "supports_" + block)
                by_rule.setdefault(rule, []).append(block)
                break
        if any(c in UPPER_HALF for c in chain) and not any(c in NO_UPPER_HALF for c in chain):
            upper.append(block)
        if any(c in BED for c in chain):
            beds.append(block)

    renamed = sorted((i, v[0]) for i, v in items.items() if i != v[0])
    walls = sorted((v[0], v[2], v[3] == "up") for v in items.values() if len(v) >= 4)

    w = sys.stdout.write
    w("//! Generated by `tools/gen_placement_rules.py` from `tools/DumpGameData.java`.\n\n")
    w("use super::survival::Rule;\n\n")
    w("/// The `canSurvive` rule of a block, by id (`None`: it survives anywhere).\n")
    w("pub(super) fn rule(id: &str) -> Option<Rule> {\n    Some(match id {\n")
    for rule in sorted(by_rule):
        w("        " + "\n        | ".join('"%s"' % b for b in by_rule[rule]))
        w(" => Rule::%s,\n" % rule)
    w("        _ => return None,\n    })\n}\n\n")
    w("/// Items whose block has another name (`createBlockItemWithCustomItemName`).\n")
    w("pub(super) fn item_block(item: &str) -> Option<&'static str> {\n    Some(match item {\n")
    for i, b in renamed:
        w('        "%s" => "%s",\n' % (i, b))
    w("        _ => return None,\n    })\n}\n\n")
    w("/// `StandingAndWallBlockItem`: the wall block, and whether the standing\n")
    w("/// block hangs (attaches up) rather than stands.\n")
    w("pub(super) fn wall_block(id: &str) -> Option<(&'static str, bool)> {\n    Some(match id {\n")
    for b, wall, up in walls:
        w('        "%s" => ("%s", %s),\n' % (b, wall, "true" if up else "false"))
    w("        _ => return None,\n    })\n}\n\n")
    w("/// Blocks whose placement also writes their upper half above.\n")
    w("pub(super) fn places_upper_half(id: &str) -> bool {\n    matches!(\n        id,\n        ")
    w("\n            | ".join('"%s"' % b for b in upper))
    w("\n    )\n}\n\n")
    w("/// Beds: placement also writes the head, one block along `facing`.\n")
    w("pub(super) fn is_bed(id: &str) -> bool {\n    matches!(\n        id,\n        ")
    w("\n            | ".join('"%s"' % b for b in beds))
    w("\n    )\n}\n")


main()
