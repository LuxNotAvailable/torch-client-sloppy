use azalea_core::direction::Direction;
use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use super::chest::facing_y_rot;
use crate::blockentities::feed::BlockEntityData;
use crate::blockentities::{BeRegistry, BeState, Submit};
use crate::entities::Carried;

const ITEM_SIZE: f32 = 0.25;
const ALIGN_ITEMS_TO_BOTTOM: f32 = -0.25;
const SLOT_SPACING: f32 = 0.3125;

pub fn register(registry: &mut BeRegistry) {
    registry.add_submitter(BlockEntityKind::Shelf, submit);
}

fn submit(st: &BeState, out: &mut Submit) {
    let BlockEntityData::Shelf(shelf) = &*st.data else {
        return;
    };
    let carried = if shelf.align_to_bottom {
        Carried::ShelfItemBottom
    } else {
        Carried::ShelfItemCentered
    };
    let y_rot = y_rot(st.state.facing);
    for (slot, item) in shelf.slots.items.iter().enumerate() {
        out.item(carried, item, pose(slot, y_rot, shelf.align_to_bottom));
    }
}

fn y_rot(facing: Option<Direction>) -> f32 {
    match facing {
        Some(Direction::Up | Direction::Down) | None => 180.0,
        horizontal => -facing_y_rot(horizontal),
    }
}

fn pose(slot: usize, y_rot: f32, align_to_bottom: bool) -> Transform {
    let offset = Vec3::new(
        (slot as f32 - 1.0) * SLOT_SPACING,
        if align_to_bottom {
            ALIGN_ITEMS_TO_BOTTOM
        } else {
            0.0
        },
        -0.25,
    );
    Transform::from_xyz(0.5, 0.5, 0.5)
        * Transform::from_rotation(Quat::from_rotation_y(y_rot.to_radians()))
        * Transform::from_translation(offset)
        * Transform::from_scale(Vec3::splat(ITEM_SIZE))
}

#[cfg(test)]
pub(super) fn test_states(_out: &mut Vec<BeState>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vertical_or_missing_facing_turns_the_shelf_half_way() {
        assert_eq!(y_rot(Some(Direction::Up)), 180.0);
        assert_eq!(y_rot(None), 180.0);
        assert_eq!(y_rot(Some(Direction::South)), 0.0);
        assert_eq!(y_rot(Some(Direction::East)), -270.0);
    }

    #[test]
    fn slots_are_spaced_along_the_shelf_and_pushed_to_its_back() {
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-5;
        let south = |slot| pose(slot, y_rot(Some(Direction::South)), false).translation;
        assert!(near(south(0), Vec3::new(0.5 - SLOT_SPACING, 0.5, 0.25)));
        assert!(near(south(1), Vec3::new(0.5, 0.5, 0.25)));
        assert!(near(south(2), Vec3::new(0.5 + SLOT_SPACING, 0.5, 0.25)));
    }

    #[test]
    fn aligned_items_drop_to_the_bottom_and_every_item_is_quartered() {
        let aligned = pose(1, 0.0, true);
        assert!((aligned.translation.y - 0.25).abs() < 1e-6);
        assert_eq!(aligned.scale, Vec3::splat(ITEM_SIZE));
    }
}
