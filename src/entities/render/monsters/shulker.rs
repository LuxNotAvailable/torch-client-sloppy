use azalea_registry::builtin::EntityKind;
use bevy::prelude::Vec3;

use crate::entities::TexturePath;
use crate::entities::models::monsters::shulker;
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::{RenderSpec, RootPose, rotate_around, setup_rotations};

pub fn register(registry: &mut Registry) {
    registry.add(
        EntityKind::Shulker,
        RenderSpec::new("shulker", shulker::layer, texture, shulker::setup_anim).with_root(root),
    );
}

fn texture(st: &EntityState) -> TexturePath {
    match crate::util::dye::DYES.get(st.extras.color as usize) {
        Some(dye) => format!("entity/shulker/shulker_{}", dye.name).into(),
        None => "entity/shulker/shulker".into(),
    }
}

fn root(st: &EntityState) -> RootPose {
    let flipped = EntityState {
        body_rot: st.body_rot + 180.0,
        ..st.clone()
    };
    let mut pose = setup_rotations(&flipped, 90.0);
    let about = rotate_around(
        st.extras.attach_face.opposite().rotation(),
        Vec3::new(0.0, 0.5, 0.0),
    );
    pose.extra_offset += pose.extra_rotation * about.translation;
    pose.extra_rotation *= about.rotation;
    pose
}
