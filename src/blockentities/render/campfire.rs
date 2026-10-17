use azalea_core::direction::Direction;
use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use super::chest::facing_y_rot;
use crate::blockentities::feed::BlockEntityData;
use crate::blockentities::{BeRegistry, BeState, Submit};
use crate::entities::Carried;

const SIZE: f32 = 0.375;

pub fn register(registry: &mut BeRegistry) {
    registry.add_submitter(BlockEntityKind::Campfire, submit);
}

fn submit(st: &BeState, out: &mut Submit) {
    let BlockEntityData::Campfire(fire) = &*st.data else {
        return;
    };
    for (slot, item) in fire.items.iter().enumerate() {
        out.item(Carried::FixedItem, item, pose(slot, st.state.facing));
    }
}

fn pose(slot: usize, facing: Option<Direction>) -> Transform {
    let turns = (slot + (facing_y_rot(facing) / 90.0) as usize) % 4;
    let angle = -(turns as f32 * 90.0);
    Transform::from_xyz(0.5, 0.44921875, 0.5)
        * Transform::from_rotation(Quat::from_rotation_y(angle.to_radians()))
        * Transform::from_rotation(Quat::from_rotation_x(90.0_f32.to_radians()))
        * Transform::from_xyz(-0.3125, -0.3125, 0.0)
        * Transform::from_scale(Vec3::splat(SIZE))
}

#[cfg(test)]
pub(super) fn test_states(_out: &mut Vec<BeState>) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot_axis(slot: usize, facing: Direction) -> Vec3 {
        pose(slot, Some(facing)).rotation * Vec3::X
    }

    #[test]
    fn slots_step_a_quarter_turn_and_facing_offsets_them() {
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-5;
        assert!(near(slot_axis(0, Direction::South), Vec3::X));
        assert!(near(slot_axis(1, Direction::South), Vec3::Z));
        assert!(near(slot_axis(0, Direction::West), Vec3::Z));
        assert!(near(slot_axis(3, Direction::East), Vec3::NEG_X));
        assert!(near(slot_axis(2, Direction::North), Vec3::X));
    }

    #[test]
    fn the_food_lies_flat_on_the_grate() {
        let at = pose(0, Some(Direction::South));
        assert_eq!(at.scale, Vec3::splat(SIZE));
        assert!((at.translation.y - 0.44921875).abs() < 1e-6);
        assert!((at.rotation * Vec3::Z - Vec3::NEG_Y).length() < 1e-5);
    }
}
