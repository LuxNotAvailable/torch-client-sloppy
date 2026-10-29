use bevy::camera::primitives::Aabb;
use bevy::math::{Mat3, Vec3A};
use bevy::mesh::{Indices, Mesh, VertexAttributeValues};
use bevy::prelude::*;

use super::geom::{BakedModel, PartState, PosedPart};

pub fn layer(
    mesh: &mut Mesh,
    model: &BakedModel,
    posed: &[PosedPart],
    states: &[PartState],
) -> Aabb {
    let mut positions = f32x3(mesh.remove_attribute(Mesh::ATTRIBUTE_POSITION));
    let mut normals = f32x3(mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL));
    let mut uvs = f32x2(mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0));
    let mut colors = f32x4(mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR));
    let mut indices = match mesh.remove_indices() {
        Some(Indices::U32(v)) => v,
        _ => Vec::new(),
    };
    positions.clear();
    normals.clear();
    uvs.clear();
    indices.clear();

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);

    for ((part, pose), state) in model.parts.iter().zip(posed).zip(states) {
        if !part_drawn(pose, state) {
            continue;
        }
        let Some(arrays) = part.arrays() else {
            continue;
        };

        let linear = Mat3::from_mat4(pose.mat);
        let normal_mat = if linear.determinant().abs() > 1e-9 {
            linear.inverse().transpose()
        } else {
            linear
        };

        let base = positions.len() as u32;
        for p in arrays.positions {
            let v = pose.mat.transform_point3(Vec3::from(*p));
            min = min.min(v);
            max = max.max(v);
            positions.push(v.to_array());
        }
        for n in arrays.normals {
            let v = (normal_mat * Vec3::from(*n)).normalize_or_zero();
            normals.push(v.to_array());
        }
        uvs.extend_from_slice(arrays.uvs);
        indices.extend(arrays.indices.iter().map(|i| i + base));
    }

    colors.clear();
    colors.resize(positions.len(), [1.0; 4]);

    if positions.is_empty() {
        min = Vec3::ZERO;
        max = Vec3::ZERO;
    }

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));

    Aabb::from_min_max(min, max)
}

pub fn empty_aabb() -> Aabb {
    Aabb {
        center: Vec3A::ZERO,
        half_extents: Vec3A::ZERO,
    }
}

/// The whole model in part-local space, each vertex tagged with its part, for
/// the vertex shader to pose from the rig's part matrices.
pub fn rigged_mesh(model: &BakedModel) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut parts: Vec<u32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (id, part) in model.parts.iter().enumerate() {
        let Some(arrays) = part.arrays() else {
            continue;
        };
        let base = positions.len() as u32;
        positions.extend_from_slice(arrays.positions);
        normals.extend_from_slice(arrays.normals);
        uvs.extend_from_slice(arrays.uvs);
        parts.resize(positions.len(), id as u32);
        indices.extend(arrays.indices.iter().map(|i| i + base));
    }
    let colors = vec![[1.0f32; 4]; positions.len()];
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(crate::renderer::rig_parts::ATTRIBUTE_PART, parts);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Each part's own-space bounds, `None` for a part with no cubes.
pub fn part_bounds(model: &BakedModel) -> Vec<Option<(Vec3, Vec3)>> {
    model
        .parts
        .iter()
        .map(|part| {
            let arrays = part.arrays()?;
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for p in arrays.positions {
                min = min.min(Vec3::from(*p));
                max = max.max(Vec3::from(*p));
            }
            (min.x <= max.x).then_some((min, max))
        })
        .collect()
}

/// Whether a posed part draws, the same test [`layer`] bakes by.
pub fn part_drawn(pose: &PosedPart, state: &PartState) -> bool {
    pose.visible && !state.skip_draw
}

/// The bounds [`layer`] would compute, from each drawn part's box corners
/// instead of every vertex.
pub fn posed_aabb(
    bounds: &[Option<(Vec3, Vec3)>],
    posed: &[PosedPart],
    states: &[PartState],
) -> Aabb {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for ((bounds, pose), state) in bounds.iter().zip(posed).zip(states) {
        let Some((lo, hi)) = bounds else {
            continue;
        };
        if !part_drawn(pose, state) {
            continue;
        }
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let v = pose.mat.transform_point3(corner);
            min = min.min(v);
            max = max.max(v);
        }
    }
    if min.x > max.x {
        return empty_aabb();
    }
    Aabb::from_min_max(min, max)
}

fn f32x3(values: Option<VertexAttributeValues>) -> Vec<[f32; 3]> {
    match values {
        Some(VertexAttributeValues::Float32x3(v)) => v,
        _ => Vec::new(),
    }
}

fn f32x2(values: Option<VertexAttributeValues>) -> Vec<[f32; 2]> {
    match values {
        Some(VertexAttributeValues::Float32x2(v)) => v,
        _ => Vec::new(),
    }
}

fn f32x4(values: Option<VertexAttributeValues>) -> Vec<[f32; 4]> {
    match values {
        Some(VertexAttributeValues::Float32x4(v)) => v,
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::geom::{CubeList, LayerDef, MeshDef, PartPose, bake};
    use crate::renderer::rig_parts::{ATTRIBUTE_PART, part_rows};

    fn model() -> BakedModel {
        let mut mesh = MeshDef::new();
        {
            let root = mesh.root();
            let body = root.child(
                "body",
                CubeList::new()
                    .tex_offs(0, 16)
                    .add_box(-4.0, 0.0, -2.0, 8.0, 12.0, 4.0),
                PartPose::offset(0.0, 2.0, 0.0),
            );
            body.child(
                "arm",
                CubeList::new()
                    .mirror()
                    .add_box(-1.0, -2.0, -2.0, 4.0, 12.0, 4.0),
                PartPose::offset_rotation(5.0, 2.0, 0.0, 0.3, 0.0, 0.1),
            );
            root.child(
                "hat",
                CubeList::new().add_box(-4.0, -8.0, -4.0, 8.0, 8.0, 8.0),
                PartPose::offset(0.0, 0.0, 0.0),
            );
        }
        bake(&LayerDef::create(mesh, 64, 32))
    }

    /// The vertex shader's posing, on the CPU.
    fn shade(rows: [[f32; 4]; 3], p: Vec3, n: Vec3) -> (Vec3, Vec3) {
        let [r0, r1, r2] = rows.map(Vec4::from);
        let p4 = p.extend(1.0);
        let pos = Vec3::new(r0.dot(p4), r1.dot(p4), r2.dot(p4));
        let (c0, c1, c2) = (
            Vec3::new(r0.x, r1.x, r2.x),
            Vec3::new(r0.y, r1.y, r2.y),
            Vec3::new(r0.z, r1.z, r2.z),
        );
        let cof = Mat3::from_cols(c1.cross(c2), c2.cross(c0), c0.cross(c1));
        let normal = (cof * n).normalize() * c0.dot(c1.cross(c2)).signum();
        (pos, normal)
    }

    #[test]
    fn gpu_posing_matches_the_cpu_bake() {
        let model = model();
        let mut states = model.rest_pose();
        let arm = model.id("arm");
        states[arm].x_rot = -0.8;
        states[arm].x_scale = 1.5;
        states[model.id("hat")].visible = false;
        let base = Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
            * Mat4::from_rotation_y(0.6)
            * Mat4::from_translation(Vec3::new(0.0, -1.5, 0.0));
        let mut posed = Vec::new();
        model.pose_parts(&states, base, &mut posed);

        let mut cpu = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::MAIN_WORLD,
        );
        let cpu_aabb = layer(&mut cpu, &model, &posed, &states);
        let cpu_pos = cpu.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap();
        let cpu_nrm = cpu.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().as_float3().unwrap();

        let rigged = rigged_mesh(&model);
        let pos = rigged.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap();
        let nrm = rigged.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap().as_float3().unwrap();
        let Some(VertexAttributeValues::Uint32(parts)) = rigged.attribute(ATTRIBUTE_PART) else {
            panic!("rigged mesh has no part attribute");
        };
        let rows: Vec<_> = posed
            .iter()
            .zip(&states)
            .map(|(p, s)| part_rows(part_drawn(p, s).then_some(p.mat)))
            .collect();

        // The CPU bake drops hidden parts; the GPU collapses them to a point.
        let mut drawn = 0;
        for i in 0..pos.len() {
            let rows = rows[parts[i] as usize];
            if rows == [[0.0; 4]; 3] {
                continue;
            }
            let (p, n) = shade(rows, Vec3::from(pos[i]), Vec3::from(nrm[i]));
            assert!((p - Vec3::from(cpu_pos[drawn])).length() < 1e-5, "vertex {i}");
            assert!((n - Vec3::from(cpu_nrm[drawn])).length() < 1e-5, "normal {i}");
            drawn += 1;
        }
        assert_eq!(drawn, cpu_pos.len());

        let aabb = posed_aabb(&part_bounds(&model), &posed, &states);
        assert!((aabb.center - cpu_aabb.center).length() < 1e-5);
        assert!((aabb.half_extents - cpu_aabb.half_extents).length() < 1e-5);
    }
}
