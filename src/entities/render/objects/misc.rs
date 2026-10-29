use azalea_registry::builtin::EntityKind;
use bevy::math::{Mat4, Quat, Vec3};

use crate::entities::TexturePath;
use crate::entities::models::objects::misc;
use crate::entities::registry::Registry;
use crate::entities::render::objects::{hook_for, mirror};
use crate::entities::state::EntityState;
use crate::entities::{MODEL_Y_OFFSET, RenderSpec, RootPose};

pub fn register(registry: &mut Registry) {
    registry.add(
        EntityKind::Cushion,
        RenderSpec::new(
            "cushion",
            misc::cushion_layer,
            cushion_texture,
            misc::no_anim,
        )
        .with_root(cushion_root),
    );
    registry.add(
        EntityKind::EndCrystal,
        RenderSpec::new(
            "end_crystal",
            misc::end_crystal_layer,
            end_crystal_texture,
            misc::end_crystal_setup_anim,
        )
        .with_root(end_crystal_root),
    );
    registry.add(
        EntityKind::EvokerFangs,
        RenderSpec::new(
            "evoker_fangs",
            misc::evoker_fangs_layer,
            evoker_fangs_texture,
            misc::evoker_fangs_setup_anim,
        )
        .with_root(evoker_fangs_root)
        .with_visible(evoker_fangs_visible),
    );
    registry.add(
        EntityKind::LeashKnot,
        RenderSpec::new(
            "leash_knot",
            misc::leash_knot_layer,
            leash_knot_texture,
            misc::no_anim,
        )
        .with_root(leash_knot_root),
    );
}

fn end_crystal_texture(_st: &EntityState) -> TexturePath {
    "entity/end_crystal/end_crystal".into()
}

fn evoker_fangs_texture(_st: &EntityState) -> TexturePath {
    "entity/illager/evoker_fangs".into()
}

fn leash_knot_texture(_st: &EntityState) -> TexturePath {
    "entity/lead_knot/lead_knot".into()
}

fn end_crystal_root(_st: &EntityState) -> RootPose {
    RootPose {
        scale: 1.0,
        hook: hook_for(
            Mat4::from_scale(Vec3::splat(2.0)) * Mat4::from_translation(Vec3::new(0.0, -0.5, 0.0)),
        ),
        ..RootPose::default()
    }
}

fn evoker_fangs_root(st: &EntityState) -> RootPose {
    RootPose {
        rotation: Quat::from_rotation_y((90.0 - st.body_rot).to_radians()),
        scale: 1.0,
        hook: hook_for(mirror() * Mat4::from_translation(Vec3::new(0.0, MODEL_Y_OFFSET, 0.0))),
        ..RootPose::default()
    }
}

fn evoker_fangs_visible(st: &EntityState) -> bool {
    st.extras.bite_progress != 0.0
}

fn leash_knot_root(_st: &EntityState) -> RootPose {
    RootPose {
        scale: 1.0,
        hook: hook_for(mirror()),
        ..RootPose::default()
    }
}

fn cushion_texture(st: &EntityState) -> TexturePath {
    let color = crate::util::dye::DYES
        .get(st.extras.look.color as usize)
        .map_or("white", |dye| dye.name);
    format!("entity/cushion/{color}_cushion").into()
}

/// A cushion faces one of the four horizontal directions its yaw is nearest.
fn cushion_root(st: &EntityState) -> RootPose {
    let facing = (st.body_rot / 90.0).round().rem_euclid(4.0) * 90.0;
    RootPose {
        rotation: Quat::from_rotation_y((180.0 - facing).to_radians()),
        scale: 1.0,
        hook: hook_for(
            Mat4::from_rotation_x(std::f32::consts::PI)
                * Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0)),
        ),
        ..RootPose::default()
    }
}
