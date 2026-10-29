//! The nether portal's swirl and nausea's warp: `LocalPlayer`'s
//! `handlePortalTransitionEffect` and `tickSpinningEffect`, with nausea's
//! blend (`MobEffectInstance.BlendState`). The renderer reads the result to
//! fade in the portal overlay and squash the world's view.

use std::sync::Mutex;

use azalea::prelude::*;
use azalea_core::position::BlockPos;
use azalea_registry::builtin::{BlockKind, MobEffect};

/// Nausea's blend: in over 150 ticks, out over 20, starting 60 ticks before
/// the effect ends (`setBlendDuration(150, 20, 60)`).
const NAUSEA_BLEND_IN: f32 = 150.0;
const NAUSEA_BLEND_OUT: f32 = 20.0;
const NAUSEA_OUT_ADVANCE: i32 = 60;

#[derive(Default)]
struct Warp {
    portal: f32,
    portal_old: f32,
    nausea: f32,
    nausea_old: f32,
    spin: f32,
    spin_speed: f32,
    /// The nausea the server last sent (amplifier, duration) and the tick it
    /// arrived, to count the duration down as vanilla's client does.
    nausea_seen: Option<((i32, i32), u64)>,
    ticks: u64,
    /// When the last tick ran, for this warp's own partial tick.
    ticked_at: Option<std::time::Instant>,
}

static WARP: Mutex<Option<Warp>> = Mutex::new(None);

/// One frame's portal and nausea strengths and the spin angle in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub portal: f32,
    pub nausea: f32,
    pub angle: f32,
}

pub(crate) fn sample(_partial: f32) -> Sample {
    let warp = WARP.lock().unwrap();
    let Some(w) = warp.as_ref() else {
        return Sample::default();
    };
    // The game ticks on its own thread, so the frame's partial tick can run
    // a tick ahead of or behind these values; measure from this warp's own
    // last tick instead, so the spin never steps back.
    let partial = w
        .ticked_at
        .map_or(1.0, |at| (at.elapsed().as_secs_f32() / 0.05).min(1.0));
    let lerp = |old: f32, new: f32| old + (new - old) * partial;
    Sample {
        portal: lerp(w.portal_old, w.portal),
        nausea: lerp(w.nausea_old, w.nausea),
        angle: w.spin + partial * w.spin_speed,
    }
}

pub(crate) fn reset() {
    if let Ok(mut warp) = WARP.try_lock() {
        *warp = None;
    }
}

/// Whether the player's box touches a nether portal block.
fn in_portal(bot: &Client) -> bool {
    let (Ok(pos), Ok(world)) = (bot.position(), bot.world()) else {
        return false;
    };
    let world = world.read();
    let (lo, hi) = (
        [pos.x - 0.3, pos.y, pos.z - 0.3],
        [pos.x + 0.3, pos.y + 1.8, pos.z + 0.3],
    );
    for x in lo[0].floor() as i32..=hi[0].floor() as i32 {
        for y in lo[1].floor() as i32..=hi[1].floor() as i32 {
            for z in lo[2].floor() as i32..=hi[2].floor() as i32 {
                if world
                    .get_block_state(BlockPos::new(x, y, z))
                    .is_some_and(|s| BlockKind::from(s) == BlockKind::NetherPortal)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// One game tick of the portal and nausea effects.
pub(crate) fn tick(bot: &Client) {
    let mut guard = WARP.lock().unwrap();
    let w = guard.get_or_insert_with(Warp::default);
    w.ticks += 1;
    w.ticked_at = Some(std::time::Instant::now());

    // `handlePortalTransitionEffect`.
    w.portal_old = w.portal;
    let step = if in_portal(bot) {
        if w.portal == 0.0 {
            let pitch = 0.8 + crate::audio::random_below(1000) as f32 / 1000.0 * 0.4;
            crate::audio::play(
                "block.portal.trigger",
                crate::audio::category::SoundCategory::Ambient,
                0.25,
                pitch,
            );
        }
        0.0125
    } else if w.portal > 0.0 {
        -0.05
    } else {
        0.0
    };
    w.portal = (w.portal + step).clamp(0.0, 1.0);

    // Nausea's blend, against a duration counted down here.
    let nausea = bot
        .component::<azalea::entity::ActiveEffects>()
        .ok()
        .and_then(|e| e.get(MobEffect::Nausea).map(|d| (d.amplifier, d.duration)));
    let has = match nausea {
        None => {
            w.nausea_seen = None;
            false
        }
        Some(sent) => {
            let since = match w.nausea_seen {
                Some((seen, at)) if seen == sent => w.ticks - at,
                _ => {
                    w.nausea_seen = Some((sent, w.ticks));
                    0
                }
            };
            let (_, duration) = sent;
            // -1 lasts forever.
            duration < 0 || duration - since as i32 > NAUSEA_OUT_ADVANCE
        }
    };
    w.nausea_old = w.nausea;
    let target = if has { 1.0 } else { 0.0 };
    let per_tick = 1.0 / if has { NAUSEA_BLEND_IN } else { NAUSEA_BLEND_OUT };
    w.nausea += (target - w.nausea).clamp(-per_tick, per_tick);

    // `tickSpinningEffect`.
    if w.portal > 0.0 || w.nausea > 0.0 {
        w.spin_speed = (w.portal * 20.0 + w.nausea * 7.0) / (w.portal + w.nausea);
        w.spin += w.spin_speed;
    } else {
        w.spin_speed = 0.0;
    }
}

/// The squash `renderLevel` puts on the world's projection: rotate about
/// (0, √½, √½), stretch x by `1 / skew`, rotate back. `None` when there is
/// no effect.
pub fn view_warp(sample: Sample) -> Option<bevy::math::Mat4> {
    use bevy::math::{Mat4, Vec3};
    let intensity = sample.portal.max(sample.nausea);
    if intensity <= 0.0 {
        return None;
    }
    let mut skew = 5.0 / (intensity * intensity + 5.0) - intensity * 0.04;
    skew *= skew;
    let axis = Vec3::new(0.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    let angle = sample.angle.to_radians();
    Some(
        Mat4::from_axis_angle(axis, angle)
            * Mat4::from_scale(Vec3::new(1.0 / skew, 1.0, 1.0))
            * Mat4::from_axis_angle(axis, -angle),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_effect_no_warp() {
        assert!(view_warp(Sample::default()).is_none());
    }

    #[test]
    fn a_full_portal_squashes_the_view() {
        let m = view_warp(Sample { portal: 1.0, nausea: 0.0, angle: 0.0 }).unwrap();
        // skew = (5/6 - 0.04)^2; x stretches by its inverse.
        let skew = (5.0f32 / 6.0 - 0.04).powi(2);
        assert!((m.x_axis.x - 1.0 / skew).abs() < 1e-4, "{m:?}");
    }
}
