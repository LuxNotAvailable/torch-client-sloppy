use azalea::entity::dimensions::EntityDimensions;
use azalea_registry::builtin::{BlockEntityKind, EntityKind};
use bevy::prelude::*;

use crate::blockentities::feed::BlockEntityData;
use crate::blockentities::sim::Anim;
use crate::blockentities::sim::spawner::{Spin, trial_spin_speed};
use crate::blockentities::{BeRegistry, BeState, Submit};

const BASE_SCALE: f32 = 0.53125;

pub fn register(registry: &mut BeRegistry) {
    registry.add_submitter_many(
        &[BlockEntityKind::MobSpawner, BlockEntityKind::TrialSpawner],
        submit,
    );
}

fn submit(st: &BeState, out: &mut Submit) {
    let (BlockEntityData::Spawner(data), Anim::Spawner(spin)) = (&*st.data, &st.anim) else {
        return;
    };
    let Some(kind) = data.display else {
        return;
    };
    if data.trial && trial_spin_speed(st.state.prop("trial_spawner_state")).is_none() {
        return;
    }
    out.entity(kind, pose(spin, st.partial, scale(kind)));
}

fn scale(kind: EntityKind) -> f32 {
    let dimensions = EntityDimensions::from(kind);
    BASE_SCALE / dimensions.width.max(dimensions.height).max(1.0)
}

fn pose(spin: &Spin, partial: f32, scale: f32) -> Transform {
    Transform::from_xyz(0.5, 0.4, 0.5)
        * Transform::from_rotation(Quat::from_rotation_y(spin.angle(partial).to_radians()))
        * Transform::from_xyz(0.0, -0.2, 0.0)
        * Transform::from_rotation(Quat::from_rotation_x((-30.0_f32).to_radians()))
        * Transform::from_scale(Vec3::splat(scale))
}

#[cfg(test)]
pub(super) fn test_states(_out: &mut Vec<BeState>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_mobs_keep_the_base_scale_and_large_ones_shrink_to_a_block() {
        assert_eq!(scale(EntityKind::Silverfish), BASE_SCALE);
        assert_eq!(scale(EntityKind::Ghast), BASE_SCALE / 4.0);
    }

    #[test]
    fn the_mob_hangs_below_the_pivot_and_takes_the_scale() {
        let at = pose(&Spin::default(), 0.0, 0.5);
        assert!((at.translation - Vec3::new(0.5, 0.2, 0.5)).length() < 1e-6);
        assert_eq!(at.scale, Vec3::splat(0.5));
    }
}
