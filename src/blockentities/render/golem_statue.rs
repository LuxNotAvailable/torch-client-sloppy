use std::f32::consts::PI;

use azalea_core::direction::Direction;
use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use crate::blockentities::models;
use crate::blockentities::render::chest::facing_y_rot;
use crate::blockentities::{BeRegistry, BeSpec, BeState, GeomKey};
use crate::entities::geom::{BakedModel, LayerDef, PartState};
use crate::entities::models::monsters::copper_golem;

const POSES: [&str; 4] = ["standing", "sitting", "running", "star"];

pub fn register(registry: &mut BeRegistry) {
    registry.add(
        BlockEntityKind::CopperGolemStatue,
        BeSpec::model("copper_golem_statue", key, layer, texture, setup).with_transform(transform),
    );
}

fn pose(st: &BeState) -> &'static str {
    let name = st.state.prop("copper_golem_pose");
    POSES
        .iter()
        .copied()
        .find(|pose| *pose == name)
        .unwrap_or(POSES[0])
}

fn weathering(st: &BeState) -> &'static str {
    let block = st.state.block.as_str();
    ["exposed", "weathered", "oxidized"]
        .into_iter()
        .find(|state| block.contains(state))
        .unwrap_or("")
}

fn key(st: &BeState) -> Option<u64> {
    Some(GeomKey::new().str(pose(st)).str(weathering(st)).finish())
}

fn layer(st: &BeState) -> LayerDef {
    match pose(st) {
        "sitting" => models::copper_golem_sitting(),
        "running" => models::copper_golem_running(),
        "star" => models::copper_golem_star(),
        _ => copper_golem::layer(),
    }
}

fn texture(st: &BeState) -> String {
    match weathering(st) {
        "" => "entity/copper_golem/copper_golem".to_string(),
        state => format!("entity/copper_golem/copper_golem_{state}"),
    }
}

fn setup(model: &BakedModel, parts: &mut [PartState], _st: &BeState) {
    let root = &mut parts[model.id("root")];
    root.y = 0.0;
    root.z_rot = PI;
}

fn transform(st: &BeState) -> Transform {
    let facing = st.state.facing.unwrap_or(Direction::North).opposite();
    Transform {
        translation: Vec3::new(0.5, 0.0, 0.5),
        rotation: Quat::from_rotation_y(-facing_y_rot(Some(facing)).to_radians()),
        scale: Vec3::ONE,
    }
}

#[cfg(test)]
pub(super) fn test_states(out: &mut Vec<BeState>) {
    for block in [
        "copper_golem_statue",
        "exposed_copper_golem_statue",
        "weathered_copper_golem_statue",
        "oxidized_copper_golem_statue",
        "waxed_copper_golem_statue",
        "waxed_exposed_copper_golem_statue",
        "waxed_weathered_copper_golem_statue",
        "waxed_oxidized_copper_golem_statue",
    ] {
        for pose in POSES {
            for facing in ["north", "south", "east", "west"] {
                out.push(super::test_state(
                    block,
                    &[("facing", facing), ("copper_golem_pose", pose)],
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waxing_keeps_the_weathering_texture() {
        let waxed = super::super::test_state(
            "waxed_weathered_copper_golem_statue",
            &[("facing", "north"), ("copper_golem_pose", "star")],
        );
        assert_eq!(
            texture(&waxed),
            "entity/copper_golem/copper_golem_weathered"
        );
        assert_eq!(pose(&waxed), "star");
    }

    #[test]
    fn every_texture_is_on_disk() {
        let mut states = Vec::new();
        test_states(&mut states);
        for st in &states {
            let file = crate::assets_root().join(format!("textures/{}.png", texture(st)));
            assert!(file.exists(), "missing {}", texture(st));
        }
    }
}
