//! Sounds of this player using a block by hand. The server plays a block's
//! `useWithoutItem` sound to everyone but the player using it
//! (`level.playSound(player, ...)`); vanilla's client hears its own by
//! running the same code, so this plays what that code would.

use azalea::block::BlockState;
use azalea_core::position::BlockPos;

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
