use azalea_core::direction::Direction;
use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use crate::blockentities::feed::BlockEntityData;
use crate::blockentities::{BeRegistry, BeState, Submit};
use crate::entities::Carried;

pub fn register(registry: &mut BeRegistry) {
    registry.add_submitter(BlockEntityKind::BrushableBlock, submit);
}

fn submit(st: &BeState, out: &mut Submit) {
    let BlockEntityData::Brushable(brushable) = &*st.data else {
        return;
    };
    let dust_progress: u32 = st.state.prop("dusted").parse().unwrap_or(0);
    let Some(hit) = brushable.hit_direction else {
        return;
    };
    if dust_progress == 0 || brushable.item.is_empty() {
        return;
    }
    let face = hit.normal();
    out.set_light(out.light_at([st.pos[0] + face.x, st.pos[1] + face.y, st.pos[2] + face.z]));
    out.item(
        Carried::FixedItem,
        &brushable.item,
        pose(hit, dust_progress),
    );
}

fn translations(direction: Direction, completion_state: u32) -> Vec3 {
    let mut at = Vec3::new(0.5, 0.0, 0.5);
    let completion_offset = completion_state as f32 / 10.0 * 0.75;
    match direction {
        Direction::East => at.x = 0.73 + completion_offset,
        Direction::West => at.x = 0.25 - completion_offset,
        Direction::Up => at.y = 0.25 + completion_offset,
        Direction::Down => at.y = -0.23 - completion_offset,
        Direction::North => at.z = 0.25 - completion_offset,
        Direction::South => at.z = 0.73 + completion_offset,
    }
    at
}

fn pose(direction: Direction, dust_progress: u32) -> Transform {
    let east_west = matches!(direction, Direction::East | Direction::West);
    let turn: f32 = 75.0 + if east_west { 90.0 } else { 0.0 } + 11.0;
    Transform::from_xyz(0.0, 0.5, 0.0)
        * Transform::from_translation(translations(direction, dust_progress))
        * Transform::from_rotation(Quat::from_rotation_y(turn.to_radians()))
        * Transform::from_scale(Vec3::splat(0.5))
}

#[cfg(test)]
pub(super) fn test_states(_out: &mut Vec<BeState>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_item_rises_out_of_the_brushed_face_as_the_dust_clears() {
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-5;
        assert!(near(
            translations(Direction::Up, 0),
            Vec3::new(0.5, 0.25, 0.5)
        ));
        assert!(near(
            translations(Direction::Up, 4),
            Vec3::new(0.5, 0.55, 0.5)
        ));
        assert!(near(
            translations(Direction::East, 2),
            Vec3::new(0.88, 0.0, 0.5)
        ));
        assert!(near(
            translations(Direction::West, 2),
            Vec3::new(0.1, 0.0, 0.5)
        ));
        assert!(near(
            translations(Direction::North, 2),
            Vec3::new(0.5, 0.0, 0.1)
        ));
        assert!(near(
            translations(Direction::South, 2),
            Vec3::new(0.5, 0.0, 0.88)
        ));
        assert!(near(
            translations(Direction::Down, 2),
            Vec3::new(0.5, -0.38, 0.5)
        ));
    }

    #[test]
    fn the_pose_adds_the_half_block_lift_and_halves_the_item() {
        let at = pose(Direction::Up, 0);
        assert!((at.translation - Vec3::new(0.5, 0.75, 0.5)).length() < 1e-6);
        assert_eq!(at.scale, Vec3::splat(0.5));
    }
}
