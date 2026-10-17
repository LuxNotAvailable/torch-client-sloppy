use std::f32::consts::{FRAC_PI_2, PI};

use azalea_core::math;
use azalea_registry::builtin::BlockEntityKind;
use bevy::prelude::*;

use crate::blockentities::sim::Anim;
use crate::blockentities::sim::conduit::Conduit;
use crate::blockentities::{BeRegistry, BeSpec, BeState, GeomKey, models, no_anim};
use crate::entities::geom::LayerDef;
use crate::util::mth::DEG_TO_RAD;

const WIND_INNER_SCALE: f32 = 0.875;
const EYE_SCALE: f32 = 0.5 * 1.3333334;

pub fn register(registry: &mut BeRegistry) {
    registry.add(
        BlockEntityKind::Conduit,
        BeSpec::model(
            "conduit_shell",
            shell_key,
            shell_layer,
            shell_texture,
            no_anim,
        )
        .with_transform(shell_transform),
    );
    registry.add(
        BlockEntityKind::Conduit,
        BeSpec::model(
            "conduit_cage",
            active_key,
            cage_layer,
            cage_texture,
            no_anim,
        )
        .with_transform(cage_transform),
    );
    registry.add(
        BlockEntityKind::Conduit,
        BeSpec::model("conduit_wind", wind_key, wind_layer, wind_texture, no_anim)
            .with_transform(wind_transform)
            .with_animated_texture(),
    );
    registry.add(
        BlockEntityKind::Conduit,
        BeSpec::model(
            "conduit_wind_inner",
            wind_key,
            wind_layer,
            wind_texture,
            no_anim,
        )
        .with_transform(wind_inner_transform)
        .with_animated_texture(),
    );
    registry.add(
        BlockEntityKind::Conduit,
        BeSpec::model("conduit_eye", eye_key, eye_layer, eye_texture, no_anim)
            .with_transform(eye_transform),
    );
}

fn conduit(st: &BeState) -> Conduit {
    match st.anim {
        Anim::Conduit(conduit) => conduit,
        _ => Conduit::default(),
    }
}

fn shell_key(st: &BeState) -> Option<u64> {
    (!conduit(st).active).then(|| GeomKey::new().finish())
}

fn active_key(st: &BeState) -> Option<u64> {
    conduit(st).active.then(|| GeomKey::new().finish())
}

fn wind_key(st: &BeState) -> Option<u64> {
    let conduit = conduit(st);
    conduit.active.then(|| {
        GeomKey::new()
            .mix((conduit.animation_phase() == 1) as u64)
            .finish()
    })
}

fn wind_frame(st: &BeState) -> u32 {
    let conduit = conduit(st);
    crate::entities::sheet_animation(wind_sheet(st))
        .map_or(0, |anim| conduit.tick_count / anim.frametime % anim.frames)
}

fn wind_sheet(st: &BeState) -> &'static str {
    if conduit(st).animation_phase() == 1 {
        "entity/conduit/wind_vertical"
    } else {
        "entity/conduit/wind"
    }
}

fn eye_key(st: &BeState) -> Option<u64> {
    let conduit = conduit(st);
    conduit
        .active
        .then(|| GeomKey::new().mix(conduit.hunting as u64).finish())
}

fn shell_layer(_st: &BeState) -> LayerDef {
    models::conduit_shell()
}

fn cage_layer(_st: &BeState) -> LayerDef {
    models::conduit_cage()
}

fn wind_layer(_st: &BeState) -> LayerDef {
    models::conduit_wind()
}

fn eye_layer(_st: &BeState) -> LayerDef {
    models::conduit_eye()
}

fn shell_texture(_st: &BeState) -> String {
    "entity/conduit/base".to_string()
}

fn cage_texture(_st: &BeState) -> String {
    "entity/conduit/cage".to_string()
}

fn wind_texture(st: &BeState) -> String {
    format!("{}#{}", wind_sheet(st), wind_frame(st))
}

fn eye_texture(st: &BeState) -> String {
    let sheet = if conduit(st).hunting {
        "open_eye"
    } else {
        "closed_eye"
    };
    format!("entity/conduit/{sheet}")
}

fn shell_transform(st: &BeState) -> Transform {
    Transform {
        translation: Vec3::splat(0.5),
        rotation: Quat::from_rotation_y(conduit(st).active_rotation(0.0) * DEG_TO_RAD),
        scale: Vec3::ONE,
    }
}

fn bob(st: &BeState) -> f32 {
    let anim_time = conduit(st).tick_count as f32 + st.partial;
    let hh = math::sin(anim_time * 0.1) / 2.0 + 0.5;
    0.3 + (hh + hh * hh) * 0.2
}

fn cage_transform(st: &BeState) -> Transform {
    let axis = Vec3::new(0.5, 1.0, 0.5).normalize();
    Transform {
        translation: Vec3::new(0.5, bob(st), 0.5),
        rotation: Quat::from_axis_angle(axis, conduit(st).active_rotation(st.partial)),
        scale: Vec3::ONE,
    }
}

fn wind_transform(st: &BeState) -> Transform {
    let rotation = match conduit(st).animation_phase() {
        1 => Quat::from_rotation_x(FRAC_PI_2),
        2 => Quat::from_rotation_z(FRAC_PI_2),
        _ => Quat::IDENTITY,
    };
    Transform {
        translation: Vec3::splat(0.5),
        rotation,
        scale: Vec3::ONE,
    }
}

fn wind_inner_transform(_st: &BeState) -> Transform {
    Transform {
        translation: Vec3::splat(0.5),
        rotation: Quat::from_euler(EulerRot::ZYX, PI, 0.0, PI),
        scale: Vec3::splat(WIND_INNER_SCALE),
    }
}

fn eye_transform(st: &BeState) -> Transform {
    Transform {
        translation: Vec3::new(0.5, bob(st), 0.5),
        rotation: st.camera.orientation * Quat::from_rotation_z(PI) * Quat::from_rotation_y(PI),
        scale: Vec3::splat(EYE_SCALE),
    }
}

#[cfg(test)]
pub(super) fn test_states(out: &mut Vec<BeState>) {
    for (active, hunting, tick_count) in [
        (false, false, 0),
        (true, false, 0),
        (true, true, 0),
        (true, false, 66),
        (true, true, 66),
        (true, false, 132),
    ] {
        let mut st = super::test_state("conduit", &[("waterlogged", "true")]);
        st.anim = Anim::Conduit(Conduit {
            tick_count,
            active,
            hunting,
            ..Conduit::default()
        });
        out.push(st);
    }
}
