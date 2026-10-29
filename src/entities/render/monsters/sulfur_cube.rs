use azalea_registry::builtin::EntityKind;
use bevy::prelude::{Mat4, Transform, Vec3};

use crate::entities::TexturePath;
use crate::entities::models::monsters::sulfur_cube;
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::{Blend, BlockRef, RenderSpec, RootPose, default_root};

pub fn register(registry: &mut Registry) {
    let layers: [(
        &'static str,
        fn() -> _,
        fn(&EntityState) -> TexturePath,
        bool,
    ); 4] = [
        (
            "sulfur_cube_inner",
            sulfur_cube::inner_layer,
            inner_texture,
            false,
        ),
        (
            "sulfur_cube",
            sulfur_cube::outer_layer,
            outer_texture,
            false,
        ),
        (
            "sulfur_cube_small_inner",
            sulfur_cube::small_inner_layer,
            inner_texture,
            true,
        ),
        (
            "sulfur_cube_small",
            sulfur_cube::small_outer_layer,
            outer_texture,
            true,
        ),
    ];
    for (name, layer, texture, baby) in layers {
        // A swallowed block takes the inner body's place.
        let visible: fn(&EntityState) -> bool = match (baby, name.ends_with("_inner")) {
            (true, true) => |st| st.extras.is_baby && st.extras.body_block.is_none(),
            (false, true) => |st| !st.extras.is_baby && st.extras.body_block.is_none(),
            (true, false) => |st| st.extras.is_baby,
            (false, false) => |st| !st.extras.is_baby,
        };
        registry.add(
            EntityKind::SulfurCube,
            RenderSpec::new(name, layer, texture, sulfur_cube::setup_anim)
                .with_root(root)
                .with_visible(visible)
                .with_blend(Blend::Translucent),
        );
    }
    registry.add(
        EntityKind::SulfurCube,
        RenderSpec::block("sulfur_cube_block", swallowed).with_root(swallowed_root),
    );
}

fn swallowed(st: &EntityState) -> Option<BlockRef> {
    st.extras.body_block.map(BlockRef::State)
}

/// `TntRenderer.getSwellAmount`: the last ten fuse ticks puff the cube up.
fn swell(st: &EntityState) -> f32 {
    let fuse = st.extras.fuse_remaining;
    if fuse <= 0.0 || fuse >= 10.0 {
        return 1.0;
    }
    let g = (1.0 - fuse / 10.0).clamp(0.0, 1.0).powi(4);
    1.0 + g * 0.3
}

/// Vanilla walks the block through the model flip, the cube's scale chain and
/// the inner layer's offsets; what comes out is a block half the cube's size,
/// centred a fixed fraction of the way up, turned to face backwards.
fn swallowed_root(st: &EntityState) -> RootPose {
    let mut pose = default_root(st);
    let grow = 0.999 * st.extras.size as f32 * swell(st);
    let pixel = if st.is_invisible { 0.0 } else { 1.0 / 16.0 };
    let centre = if st.extras.is_baby {
        grow * (1.501 - 1.24 + pixel - 0.009)
    } else {
        grow * 0.5 * (1.501 - 0.98 + pixel - 0.018)
    };
    let edge = grow * 0.5;
    pose.hook = Transform::from_matrix(
        Mat4::from_translation(Vec3::new(0.0, centre, 0.0))
            * Mat4::from_rotation_y(std::f32::consts::PI)
            * Mat4::from_scale(Vec3::splat(edge))
            * Mat4::from_translation(Vec3::splat(-0.5)),
    );
    pose
}

fn outer_texture(st: &EntityState) -> TexturePath {
    if st.extras.is_baby {
        "entity/sulfur_cube/sulfur_cube_outer_small".into()
    } else {
        "entity/sulfur_cube/sulfur_cube_outer".into()
    }
}

fn inner_texture(st: &EntityState) -> TexturePath {
    if st.extras.is_baby {
        "entity/sulfur_cube/sulfur_cube_inner_small".into()
    } else {
        "entity/sulfur_cube/sulfur_cube_inner".into()
    }
}

/// `SulfurCubeRenderer.scale`: the slime's slight downscale and squash, then
/// a fixed shrink for the full-size model and a lift onto the ground.
fn root(st: &EntityState) -> RootPose {
    let mut pose = default_root(st);
    let size = st.extras.size as f32;
    // A cube holding a block keeps its shape.
    let ss = if st.extras.body_block.is_some() {
        0.0
    } else {
        st.extras.squish / (size * 0.5 + 1.0)
    };
    let w = 1.0 / (ss + 1.0);
    let (lift, shrink) = if st.extras.is_baby {
        (1.24, 1.0)
    } else {
        (0.98, 0.5)
    };
    let pixel = if st.is_invisible { 0.0 } else { 1.0 / 16.0 };
    let m = Mat4::from_scale(Vec3::splat(0.999))
        * Mat4::from_translation(Vec3::new(0.0, 0.001, 0.0))
        * Mat4::from_scale(Vec3::new(w * size, size / w, w * size))
        * Mat4::from_scale(Vec3::splat(swell(st)))
        * Mat4::from_scale(Vec3::splat(shrink))
        * Mat4::from_translation(Vec3::new(0.0, lift - pixel, 0.0));
    pose.hook = Transform::from_matrix(m);
    pose
}
