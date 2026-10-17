use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::Vec3;

use crate::blockentities::feed::BlockEntityData;
use crate::blockentities::sim::Anim;
use crate::blockentities::{BeRegistry, BeState, Submit};

const VIEW_DISTANCE: f32 = 68.0;

pub fn register(registry: &mut BeRegistry) {
    registry.add_submitter(BlockEntityKind::Piston, submit);
}

fn submit(st: &BeState, out: &mut Submit) {
    let (BlockEntityData::Piston(moving), Anim::Piston(progress)) = (&*st.data, &st.anim) else {
        return;
    };
    let corner = Vec3::new(st.pos[0] as f32, st.pos[1] as f32, st.pos[2] as f32);
    if st
        .camera
        .position
        .distance_squared(corner + Vec3::splat(0.5))
        >= VIEW_DISTANCE * VIEW_DISTANCE
    {
        return;
    }
    let feet = corner + Vec3::new(0.5, 0.0, 0.5);
    for (state, offset) in moving.draws(progress.at(st.partial)).into_iter().flatten() {
        out.block(state, feet + Vec3::from_array(offset));
    }
}

#[cfg(test)]
pub(super) fn test_states(_out: &mut Vec<BeState>) {}
