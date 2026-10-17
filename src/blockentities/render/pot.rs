use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use crate::blockentities::feed::BlockEntityData;
use crate::blockentities::models;
use crate::blockentities::render::chest::facing_y_rot;
use crate::blockentities::sim::Anim;
use crate::blockentities::sim::pot::SIDES;
use crate::blockentities::{BeRegistry, BeSpec, BeState, GeomKey, no_anim};
use crate::entities::geom::LayerDef;
use crate::entities::rotate_around;

const WOBBLE_AMPLITUDE: f32 = 0.125;
const BASE_TEXTURE: &str = "entity/decorated_pot/decorated_pot_base";
const NAMES: [&str; SIDES] = ["pot_back", "pot_left", "pot_right", "pot_front"];

pub fn register(registry: &mut BeRegistry) {
    let base = BeSpec::model("pot_base", base_key, base_layer, base_texture, no_anim)
        .with_transform(transform);
    registry.add(BlockEntityKind::DecoratedPot, base);
    let sides = [
        side_spec::<0>(),
        side_spec::<1>(),
        side_spec::<2>(),
        side_spec::<3>(),
    ];
    for spec in sides {
        registry.add(BlockEntityKind::DecoratedPot, spec);
    }
}

fn side_spec<const SIDE: usize>() -> BeSpec {
    BeSpec::model(
        NAMES[SIDE],
        side_key::<SIDE>,
        side_layer::<SIDE>,
        side_texture::<SIDE>,
        no_anim,
    )
    .with_transform(transform)
}

fn base_key(_st: &BeState) -> Option<u64> {
    Some(GeomKey::new().finish())
}

fn base_layer(_st: &BeState) -> LayerDef {
    models::decorated_pot_base()
}

fn base_texture(_st: &BeState) -> String {
    BASE_TEXTURE.to_string()
}

fn stem<const SIDE: usize>(st: &BeState) -> &'static str {
    match &*st.data {
        BlockEntityData::Pot(decorations) => decorations.stem(SIDE),
        _ => "",
    }
}

fn side_key<const SIDE: usize>(st: &BeState) -> Option<u64> {
    Some(GeomKey::new().str(stem::<SIDE>(st)).finish())
}

fn side_layer<const SIDE: usize>(_st: &BeState) -> LayerDef {
    models::decorated_pot_side(SIDE)
}

fn side_texture<const SIDE: usize>(st: &BeState) -> String {
    match stem::<SIDE>(st) {
        "" => "entity/decorated_pot/decorated_pot_side".to_string(),
        pattern => format!("entity/decorated_pot/{pattern}_pottery_pattern"),
    }
}

fn wobble_yaw(progress: f32) -> f32 {
    let amplitude = (-progress * 3.0 * std::f32::consts::PI).sin() * WOBBLE_AMPLITUDE;
    amplitude * (1.0 - progress)
}

fn transform(st: &BeState) -> Transform {
    let facing = Quat::from_rotation_y((180.0 - facing_y_rot(st.state.facing)).to_radians());
    let placed = rotate_around(facing, Vec3::splat(0.5));
    let progress = match &st.anim {
        Anim::Pot(wobble) => wobble.progress(st.game_time, st.partial),
        _ => None,
    };
    match progress {
        Some(progress) if (0.0..=1.0).contains(&progress) => placed.mul_transform(rotate_around(
            Quat::from_rotation_y(wobble_yaw(progress)),
            Vec3::new(0.5, 0.0, 0.5),
        )),
        _ => placed,
    }
}

#[cfg(test)]
pub(super) fn test_states(out: &mut Vec<BeState>) {
    use std::sync::Arc;

    use crate::blockentities::sim::pot::decode;
    use crate::blockentities::sim::test_nbt::{root, strings};

    for facing in ["north", "south", "east", "west"] {
        out.push(super::test_state("decorated_pot", &[("facing", facing)]));
    }
    let decorations = decode(&root(vec![(
        "sherds",
        strings(&[
            "minecraft:angler_pottery_sherd",
            "minecraft:brick",
            "minecraft:skull_pottery_sherd",
            "minecraft:heartbreak_pottery_sherd",
        ]),
    )]));
    out.push(BeState {
        data: Arc::new(BlockEntityData::Pot(decorations)),
        ..super::test_state("decorated_pot", &[("facing", "north")])
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wobble_starts_and_ends_at_rest() {
        assert_eq!(wobble_yaw(0.0), 0.0);
        assert!(wobble_yaw(1.0).abs() < 1e-6);
        assert!(wobble_yaw(0.2).abs() > 0.0);
    }

    #[test]
    fn a_resting_pot_only_turns_to_face() {
        let st = super::super::test_state("decorated_pot", &[("facing", "south")]);
        let t = transform(&st);
        let center = t.transform_point(Vec3::splat(0.5));
        assert!((center - Vec3::splat(0.5)).length() < 1e-5);
    }

    #[test]
    fn textures_are_on_disk() {
        let st = super::super::test_state("decorated_pot", &[("facing", "north")]);
        let mut paths = vec![base_texture(&st), side_texture::<0>(&st)];
        paths.extend(
            crate::blockentities::sim::pot::STEMS
                .iter()
                .map(|stem| format!("entity/decorated_pot/{stem}_pottery_pattern")),
        );
        for path in paths {
            let file = crate::assets_root().join(format!("textures/{path}.png"));
            assert!(file.exists(), "missing {path}");
        }
    }
}
