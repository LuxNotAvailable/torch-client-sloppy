use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use crate::blockentities::feed::BlockEntityData;
use crate::blockentities::sim::Anim;
use crate::blockentities::{BeRegistry, BeState, Submit};
use crate::entities::Carried;

pub fn register(registry: &mut BeRegistry) {
    registry.add_submitter(BlockEntityKind::Vault, submit);
}

fn submit(st: &BeState, out: &mut Submit) {
    let (BlockEntityData::Vault(data), Anim::Vault(vault)) = (&*st.data, &st.anim) else {
        return;
    };
    out.item(
        Carried::GroundItemCluster,
        &data.display_item,
        pose(vault.angle(st.partial)),
    );
}

fn pose(spin: f32) -> Transform {
    Transform::from_xyz(0.5, 0.4, 0.5)
        * Transform::from_rotation(Quat::from_rotation_y(spin.to_radians()))
}

#[cfg(test)]
pub(super) fn test_states(_out: &mut Vec<BeState>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_item_floats_at_the_cage_centre_and_turns_about_y() {
        let at = pose(90.0);
        assert!((at.translation - Vec3::new(0.5, 0.4, 0.5)).length() < 1e-6);
        assert!((at.rotation * Vec3::X - Vec3::NEG_Z).length() < 1e-5);
    }
}
