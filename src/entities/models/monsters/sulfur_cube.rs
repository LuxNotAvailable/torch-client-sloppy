use crate::entities::geom::{BakedModel, CubeList, LayerDef, MeshDef, PartPose, PartState};
use crate::entities::state::EntityState;

fn cube(tex_v: i32, half: f32, texture: u32) -> LayerDef {
    let mut mesh = MeshDef::new();
    let size = half * 2.0;
    mesh.root().child(
        "cube",
        CubeList::new()
            .tex_offs(0, tex_v)
            .add_box(-half, -half, -half, size, size, size),
        PartPose::ZERO,
    );
    LayerDef::create(mesh, texture, texture)
}

pub fn outer_layer() -> LayerDef {
    cube(0, 9.0, 128)
}

pub fn inner_layer() -> LayerDef {
    cube(36, 8.0, 128)
}

pub fn small_outer_layer() -> LayerDef {
    cube(0, 5.0, 64)
}

pub fn small_inner_layer() -> LayerDef {
    cube(20, 4.0, 64)
}

pub fn setup_anim(_model: &BakedModel, _parts: &mut [PartState], _st: &EntityState) {}
