use std::f32::consts::PI;

use azalea_core::direction::Direction;
use azalea_registry::builtin::BlockEntityKind;

use crate::blockentities::models;
use crate::blockentities::sim::Anim;
use crate::blockentities::{BeRegistry, BeSpec, BeState, GeomKey};
use crate::entities::geom::{BakedModel, LayerDef, PartState};

pub fn register(registry: &mut BeRegistry) {
    registry.add(
        BlockEntityKind::Bell,
        BeSpec::model("bell", key, layer, texture, setup),
    );
}

fn key(_st: &BeState) -> Option<u64> {
    Some(GeomKey::new().finish())
}

fn layer(_st: &BeState) -> LayerDef {
    models::bell()
}

fn texture(_st: &BeState) -> String {
    "entity/bell/bell_body".to_string()
}

fn swing(ticks: f32) -> f32 {
    (ticks / PI).sin() / (4.0 + ticks / 3.0)
}

fn rotation(direction: Option<Direction>, ticks: f32) -> (f32, f32) {
    let base = swing(ticks);
    match direction {
        Some(Direction::North) => (-base, 0.0),
        Some(Direction::South) => (base, 0.0),
        Some(Direction::East) => (0.0, -base),
        Some(Direction::West) => (0.0, base),
        _ => (0.0, 0.0),
    }
}

fn setup(model: &BakedModel, parts: &mut [PartState], st: &BeState) {
    let (x_rot, z_rot) = match &st.anim {
        Anim::Bell(bell) => rotation(bell.shake_direction(), bell.ticks(st.partial)),
        _ => (0.0, 0.0),
    };
    let body = &mut parts[model.id("bell_body")];
    body.x_rot = x_rot;
    body.z_rot = z_rot;
}

#[cfg(test)]
pub(super) fn test_states(out: &mut Vec<BeState>) {
    for facing in ["north", "south", "east", "west"] {
        for attachment in ["floor", "ceiling", "single_wall", "double_wall"] {
            out.push(super::test_state(
                "bell",
                &[("facing", facing), ("attachment", attachment)],
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swing_axis_follows_the_click_direction() {
        let ticks = 10.0;
        let base = swing(ticks);
        assert_eq!(rotation(Some(Direction::North), ticks), (-base, 0.0));
        assert_eq!(rotation(Some(Direction::West), ticks), (0.0, base));
        assert_eq!(rotation(Some(Direction::Up), ticks), (0.0, 0.0));
        assert_eq!(rotation(None, ticks), (0.0, 0.0));
    }

    #[test]
    fn the_swing_decays() {
        assert!(swing(2.0).abs() > swing(48.0).abs());
    }

    #[test]
    fn the_texture_is_on_disk() {
        let st = super::super::test_state("bell", &[("facing", "north")]);
        let file = crate::assets_root().join(format!("textures/{}.png", texture(&st)));
        assert!(file.exists());
    }
}
