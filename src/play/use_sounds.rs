//! Sounds of this player using a block by hand. The server plays a block's
//! `useWithoutItem` sound to everyone but the player using it
//! (`level.playSound(player, ...)`); vanilla's client hears its own by
//! running the same code, so this plays what that code would.

use azalea::block::BlockState;
use azalea_core::position::BlockPos;
use azalea_registry::builtin::ItemKind;

use crate::audio::SoundCategory;
use crate::generated_block_sounds::{UseKind, use_sounds};

/// The block at `pos` (as it was before the use) was used by hand.
/// `may_build` is `Abilities.mayBuild`, which the comparator needs.
pub(crate) fn block_used(state: BlockState, pos: BlockPos, may_build: bool) {
    let info = crate::blocks::info::info(state);
    let id = crate::util::block_model::block_id(state);
    let at = [pos.x as f32 + 0.5, pos.y as f32 + 0.5, pos.z as f32 + 0.5];
    let play = |event: &str, volume: f32, pitch: f32| {
        crate::audio::play_at(event, SoundCategory::Blocks, at, volume, pitch);
    };
    // Doors, trapdoors and fence gates: `nextFloat() * 0.1 + 0.9`.
    let swing_pitch = || crate::audio::random_unit() * 0.1 + 0.9;

    if let Some(sounds) = use_sounds(id) {
        match sounds.kind {
            UseKind::Door | UseKind::Trapdoor | UseKind::FenceGate => {
                if !sounds.by_hand {
                    return;
                }
                let opening = !info.flag("open");
                play(
                    if opening { sounds.on } else { sounds.off },
                    1.0,
                    swing_pitch(),
                );
            }
            // `ButtonBlock.useWithoutItem`: a pressed button does nothing.
            UseKind::Button => {
                if !info.flag("powered") {
                    play(sounds.on, 1.0, 1.0);
                }
            }
        }
        return;
    }
    match id {
        // `LeverBlock.playSound`, pitched by the state after the pull.
        "lever" => {
            let powered = !info.flag("powered");
            play("block.lever.click", 0.3, if powered { 0.6 } else { 0.5 });
        }
        // `ComparatorBlock.useWithoutItem`, pitched by the mode after.
        "comparator" if may_build => {
            let subtract = crate::blocks::info::property(state, "mode") != Some("subtract");
            play(
                "block.comparator.click",
                0.3,
                if subtract { 0.55 } else { 0.5 },
            );
        }
        _ => {}
    }
}

/// An item used on a block that didn't take the click itself
/// (`Item.useOn`): `face` is the clicked side, `player` the feet.
pub(crate) fn item_used_on(
    item: ItemKind,
    state: BlockState,
    pos: BlockPos,
    face: azalea_core::direction::Direction,
    above_face: Option<BlockState>,
    player: [f32; 3],
) {
    let id = crate::util::block_model::block_id(state);
    let info = crate::blocks::info::info(state);
    let center = |p: BlockPos| [p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5];
    match item {
        // `FlintAndSteelItem.useOn`: lights an unlit campfire, candle or
        // candle cake, else sets fire in front of the face.
        ItemKind::FlintAndSteel => {
            let lightable = !info.flag("lit")
                && !info.flag("waterlogged")
                && (crate::blocks::tags::has(state, "campfires")
                    || crate::blocks::tags::has(state, "candles")
                    || crate::blocks::tags::has(state, "candle_cakes"));
            let at = if lightable {
                Some(pos)
            } else {
                // `BaseFireBlock.canBePlacedAt`, as far as an air cell goes.
                above_face
                    .is_some_and(|s| s.is_air())
                    .then(|| pos + face.normal())
            };
            if let Some(at) = at {
                let pitch = crate::audio::random_unit() * 0.4 + 0.8;
                crate::audio::play_at(
                    "item.flintandsteel.use",
                    SoundCategory::Blocks,
                    center(at),
                    1.0,
                    pitch,
                );
            }
        }
        // `ShearsItem.useOn`: a growing vine or kelp tip stops growing.
        ItemKind::Shears => {
            let growing_tip = matches!(
                id,
                "kelp" | "weeping_vines" | "twisting_vines" | "cave_vines"
            );
            if growing_tip && crate::blocks::info::property(state, "age") != Some("25") {
                crate::audio::play_at(
                    "block.growing_plant.crop",
                    SoundCategory::Blocks,
                    center(pos),
                    1.0,
                    1.0,
                );
            }
        }
        // `BeehiveBlock.useItemOn`: bottling a full hive.
        ItemKind::GlassBottle => {
            let full = crate::blocks::info::property(state, "honey_level") == Some("5");
            if full && matches!(id, "beehive" | "bee_nest") {
                crate::audio::play_at("item.bottle.fill", SoundCategory::Blocks, player, 1.0, 1.0);
            }
        }
        _ => {}
    }
}

/// An item used in the air (`Item.use`): `water_source` is whether the
/// look meets a water source first, for a bottle.
pub(crate) fn item_used(item: ItemKind, water_source: bool, player: [f32; 3]) {
    match item {
        // `SpyglassItem.use`.
        ItemKind::Spyglass => {
            crate::audio::play_at(
                "item.spyglass.use",
                SoundCategory::Players,
                player,
                1.0,
                1.0,
            );
        }
        // `BottleItem.use` on water.
        ItemKind::GlassBottle if water_source => {
            crate::audio::play_at("item.bottle.fill", SoundCategory::Neutral, player, 1.0, 1.0);
        }
        _ => {}
    }
}

/// The player stopped using an item (`releaseUsing`, `finishUsingItem`).
pub(crate) fn item_released(item: ItemKind, player: [f32; 3]) {
    if item == ItemKind::Spyglass {
        crate::audio::play_at(
            "item.spyglass.stop_using",
            SoundCategory::Players,
            player,
            1.0,
            1.0,
        );
    }
}
