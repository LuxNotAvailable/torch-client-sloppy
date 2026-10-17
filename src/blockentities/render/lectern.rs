use azalea_core::direction::Direction;
use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use crate::blockentities::render::chest::facing_y_rot;
use crate::blockentities::render::enchant;
use crate::blockentities::{BeRegistry, BeSpec, BeState, GeomKey};
use crate::blocks::facing;
use crate::entities::geom::{BakedModel, PartState};

pub fn register(registry: &mut BeRegistry) {
    registry.add(
        BlockEntityKind::Lectern,
        BeSpec::model("lectern_book", key, enchant::layer, enchant::texture, setup)
            .with_transform(transform),
    );
}

const OPENNESS: f32 = 1.5;
const PAGE_FLIP_1: f32 = 0.1;
const PAGE_FLIP_2: f32 = 0.9;

fn key(st: &BeState) -> Option<u64> {
    (st.state.prop("has_book") == "true").then(|| GeomKey::new().finish())
}

fn setup(model: &BakedModel, parts: &mut [PartState], _st: &BeState) {
    enchant::pose_book(model, parts, OPENNESS, PAGE_FLIP_1, PAGE_FLIP_2);
}

fn transform(st: &BeState) -> Transform {
    let placed = st.state.facing.unwrap_or(Direction::South);
    let y_rot = facing_y_rot(Some(facing::clockwise(placed)));
    let rotation =
        Quat::from_rotation_y(-y_rot.to_radians()) * Quat::from_rotation_z(67.5_f32.to_radians());
    Transform {
        translation: Vec3::new(0.5, 1.0625, 0.5) + rotation * Vec3::new(0.0, -0.125, 0.0),
        rotation,
        scale: Vec3::ONE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_stocked_lectern_draws() {
        let stocked =
            super::super::test_state("lectern", &[("facing", "north"), ("has_book", "true")]);
        let empty =
            super::super::test_state("lectern", &[("facing", "north"), ("has_book", "false")]);
        assert!(key(&stocked).is_some());
        assert!(key(&empty).is_none());
    }

    #[test]
    fn the_book_texture_is_on_disk() {
        let st = super::super::test_state("lectern", &[("facing", "north"), ("has_book", "true")]);
        let file = crate::assets_root().join(format!("textures/{}.png", enchant::texture(&st)));
        assert!(file.exists(), "missing {}", enchant::texture(&st));
    }

    #[test]
    fn the_book_rests_on_the_stand() {
        for facing in ["north", "south", "east", "west"] {
            let st =
                super::super::test_state("lectern", &[("facing", facing), ("has_book", "true")]);
            let at = transform(&st).translation;
            assert!(at.y > 0.9375, "{facing}: {at:?}");
            assert!(
                (0.0..=1.0).contains(&at.x) && (0.0..=1.0).contains(&at.z),
                "{facing}: {at:?}"
            );
        }
    }
}
