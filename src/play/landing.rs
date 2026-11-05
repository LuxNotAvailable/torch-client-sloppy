//! The local player's landing sounds. Vanilla's client runs
//! `Entity.checkFallDamage` for its own player, so the block's `fallOn`, and
//! the fall damage sounds `LivingEntity.causeFallDamage` plays, are heard
//! there first; the server plays them to everyone else.

use azalea::block::BlockState;

use crate::audio::SoundCategory;

/// `Attributes.SAFE_FALL_DISTANCE`'s base.
const SAFE_FALL_DISTANCE: f64 = 3.0;

/// Where the player landed and how far they fell.
pub(crate) struct Landing {
    pub at: [f32; 3],
    pub fall_distance: f64,
    /// The block under the feet (`getOnPosLegacy`, 0.2 below).
    pub on: BlockState,
    /// `abilities.mayfly`: such players take no fall damage.
    pub may_fly: bool,
    /// Jump Boost's amplifier: each level adds one block of safe fall.
    pub jump_boost: Option<i32>,
    /// `isSuppressingBounce`: sneaking, so slime blocks hurt as usual.
    pub sneaking: bool,
}

/// `LivingEntity.calculateFallDamage` for a player.
fn fall_damage(fall_distance: f64, modifier: f64, jump_boost: Option<i32>) -> i32 {
    let safe = SAFE_FALL_DISTANCE + jump_boost.map_or(0.0, |a| f64::from(a + 1));
    ((fall_distance + 1.0e-6 - safe) * modifier).floor() as i32
}

/// The block's `fallOn`, and the sounds it leads to.
pub(crate) fn landed(l: &Landing) {
    let play = |event: &str, volume: f32, pitch: f32| {
        crate::audio::play_at(event, SoundCategory::Players, l.at, volume, pitch);
    };
    let id = crate::util::block_model::block_id(l.on);
    let modifier = match id {
        "hay_block" => 0.2,
        "honey_block" => {
            play("block.honey_block.slide", 1.0, 1.0);
            0.2
        }
        "slime_block" if !l.sneaking => 0.0,
        // `PowderSnowBlock.fallOn` plays the fall sound by distance alone and
        // deals no damage.
        "powder_snow" => {
            if l.fall_distance >= 4.0 {
                let event = if l.fall_distance < 7.0 {
                    "entity.player.small_fall"
                } else {
                    "entity.player.big_fall"
                };
                play(event, 1.0, 1.0);
            }
            return;
        }
        _ => 1.0,
    };
    if l.may_fly {
        return;
    }
    let damage = fall_damage(l.fall_distance, modifier, l.jump_boost);
    if damage <= 0 {
        return;
    }
    // `getFallDamageSound`, then `playBlockFallSound` (honey plays its own
    // fall sound the same way).
    play(
        if damage > 4 {
            "entity.player.big_fall"
        } else {
            "entity.player.small_fall"
        },
        1.0,
        1.0,
    );
    if !l.on.is_air() {
        let group = crate::util::block_model::sound_group(id);
        play(group.fall_event, group.volume * 0.5, group.pitch * 0.75);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_past_three_blocks_hurt_and_jump_boost_adds_safe_distance() {
        assert_eq!(fall_damage(3.0, 1.0, None), 0);
        assert_eq!(fall_damage(4.0, 1.0, None), 1);
        assert_eq!(fall_damage(8.5, 1.0, None), 5);
        assert_eq!(fall_damage(4.0, 1.0, Some(0)), 0);
        assert_eq!(fall_damage(20.0, 0.2, None), 3);
    }
}
