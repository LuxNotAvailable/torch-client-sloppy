//! Bevy `Mesh` assets as `MeshData`, for producers that still build Bevy
//! meshes (the hand, entities, items). Each mesh is copied the frame it is
//! added or changed, before its upload can drop the main-world copy.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;

use super::{EntityVertex, MeshData};

/// Converted meshes by asset. A changed mesh gets a new `MeshData` (new id).
#[derive(Resource, Default)]
pub struct Meshes(HashMap<AssetId<Mesh>, Arc<MeshData>>);

impl Meshes {
    pub fn get(&self, id: AssetId<Mesh>) -> Option<&Arc<MeshData>> {
        self.0.get(&id)
    }
}

/// Runs before `DrawSystems`, so producers find this frame's meshes.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BridgeSystems;

pub fn plugin(app: &mut App) {
    app.init_resource::<Meshes>()
        .configure_sets(PostUpdate, BridgeSystems.before(super::DrawSystems))
        .add_systems(PostUpdate, capture.in_set(BridgeSystems));
}

fn capture(
    mut cache: ResMut<Meshes>,
    mut events: MessageReader<AssetEvent<Mesh>>,
    meshes: Res<Assets<Mesh>>,
) {
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                if let Some(data) = meshes.get(id).and_then(mesh_data) {
                    cache.0.insert(id, data);
                }
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                cache.0.remove(&id);
            }
            _ => {}
        }
    }
    // Meshes made before the first frame (the loading screen's) sent their
    // events unseen.
    if cache.0.is_empty() {
        let missed: Vec<_> = meshes
            .iter()
            .filter_map(|(id, mesh)| Some((id, mesh_data(mesh)?)))
            .collect();
        cache.0.extend(missed);
    }
}

/// A mesh's position, normal, uv, colour and rig part attributes.
pub fn mesh_data(mesh: &Mesh) -> Option<Arc<MeshData>> {
    use bevy::mesh::VertexAttributeValues as V;
    // A mesh kept only in the render world panics on attribute access once
    // extracted; until then (its first frame) it can still be read.
    mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok()?;
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
        V::Float32x3(p) => p,
        _ => return None,
    };
    let mut verts: Vec<EntityVertex> = positions
        .iter()
        .map(|&pos| EntityVertex {
            pos,
            color: [1.0; 4],
            ..Default::default()
        })
        .collect();
    if let Some(V::Float32x3(n)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        verts.iter_mut().zip(n).for_each(|(v, n)| v.normal = *n);
    }
    if let Some(V::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        verts.iter_mut().zip(uv).for_each(|(v, uv)| v.uv = *uv);
    }
    if let Some(V::Float32x4(c)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        verts.iter_mut().zip(c).for_each(|(v, c)| v.color = *c);
    }
    let rigged = match mesh.attribute(crate::renderer::rig_parts::ATTRIBUTE_PART) {
        Some(V::Uint32(p)) => {
            verts.iter_mut().zip(p).for_each(|(v, p)| v.part = *p);
            true
        }
        _ => false,
    };
    let indices = match mesh.indices() {
        Some(i) => i.iter().map(|i| i as u32).collect(),
        None => (0..verts.len() as u32).collect(),
    };
    Some(MeshData::new(verts, indices, rigged))
}
