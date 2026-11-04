//! A snapshot of every entity the game draws, for the Vulkan renderer's
//! viewer (VULKAN_REWRITE_CHECKLIST.md phase 4): with
//! `MC_DUMP_TERRAIN=<folder>`, beside the terrain dump, the game rewrites
//! `entities.bin` every few seconds with what it draws through
//! `EntityMaterial`: mobs and players (GPU-posed rigs), block entities,
//! dropped items, and the like. It holds their meshes, textures, part
//! matrices, materials and transforms, and the world camera. `vkview
//! --terrain <folder>` draws them with the new entity path, so it is
//! tested on exactly what the game draws.
//!
//! Most of these meshes and images are main-world assets only until their
//! first upload. So while dumping, a system copies each one the frame it is
//! added or changed.
//!
//! `entities.bin` (`TVKE`, version 1), all little-endian:
//! - camera: world transform (16 × f32, columns), vertical fov (f32)
//! - textures: count, then id, width, height (3 × u32) and RGBA8 pixels
//! - meshes: count, then id, vertex count, index count, rigged (4 × u32),
//!   the vertices (`EntityVertex`, 52 bytes) and u32 indices
//! - part rows: count, then rows (12 × f32, the top three rows of each part
//!   matrix)
//! - instances: count, then `EntityInstance` (184 bytes)

use std::collections::HashMap;

use bevy::prelude::*;

use crate::renderer::entity_material::EntityMaterial;

pub const MAGIC: &[u8; 4] = b"TVKE";
pub const VERSION: u32 = 2;

/// Seconds between rewrites; the last one stands when the game stops.
const EVERY: f32 = 3.0;

/// One mesh vertex: what `entity.wgsl` reads, and the rig part.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EntityVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    /// The part matrix offset from the instance's tag; 0 for static meshes.
    pub part: u32,
}

const _: () = assert!(std::mem::size_of::<EntityVertex>() == 52);

/// One drawn entity mesh.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EntityInstance {
    pub mesh: u32,
    /// The texture's id, or `u32::MAX` for none.
    pub texture: u32,
    /// The first part matrix of a GPU-posed rig (its `MeshTag`).
    pub tag: u32,
    /// 0 opaque, 1 cutout (alpha mask), 2 blended, 3 additive.
    pub alpha: u32,
    pub depth_bias: i32,
    /// 0 entity, 1 block entity, 2 particle batch, 3 beam, 4 weather.
    pub kind: u32,
    /// `EntityParams` as the shader has it (mode last).
    pub tint: [f32; 4],
    pub light: [f32; 4],
    pub light0: [f32; 4],
    pub light1: [f32; 4],
    pub switches: [f32; 4],
    pub levels: [f32; 2],
    pub mode: u32,
    pub see_through: u32,
    /// The world transform, columns.
    pub transform: [f32; 16],
}

const _: () = assert!(std::mem::size_of::<EntityInstance>() == 184);

/// Mesh and image data copied while they still sit in the main world.
#[derive(Resource, Default)]
struct Cache {
    meshes: HashMap<AssetId<Mesh>, (Vec<EntityVertex>, Vec<u32>, bool)>,
    images: HashMap<AssetId<Image>, (u32, u32, Vec<u8>)>,
}

/// Adds the dump's systems when `MC_DUMP_TERRAIN` names a folder.
pub fn plugin(app: &mut App) {
    if super::terrain_dump::folder().is_none() {
        return;
    }
    app.init_resource::<Cache>()
        .add_systems(PostUpdate, (capture, write).chain());
}

fn mesh_data(mesh: &Mesh) -> Option<(Vec<EntityVertex>, Vec<u32>, bool)> {
    use bevy::mesh::VertexAttributeValues as V;
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
    Some((verts, indices, rigged))
}

fn image_data(image: &Image) -> Option<(u32, u32, Vec<u8>)> {
    use bevy::render::render_resource::TextureFormat as F;
    if !matches!(
        image.texture_descriptor.format,
        F::Rgba8UnormSrgb | F::Rgba8Unorm
    ) {
        return None;
    }
    let size = image.texture_descriptor.size;
    let bytes = (size.width * size.height * 4) as usize;
    let data = image.data.as_ref()?;
    (data.len() >= bytes).then(|| (size.width, size.height, data[..bytes].to_vec()))
}

/// Copies meshes and images as they arrive, before the upload drops them.
fn capture(
    mut cache: ResMut<Cache>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
) {
    for event in mesh_events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                if let Some(data) = meshes.get(id).and_then(mesh_data) {
                    cache.meshes.insert(id, data);
                }
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                cache.meshes.remove(&id);
            }
            _ => {}
        }
    }
    for event in image_events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                if let Some(data) = images.get(id).and_then(image_data) {
                    cache.images.insert(id, data);
                }
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                cache.images.remove(&id);
            }
            _ => {}
        }
    }
}

type Drawn<'a> = (
    &'a Mesh3d,
    &'a MeshMaterial3d<EntityMaterial>,
    &'a GlobalTransform,
    &'a InheritedVisibility,
    Option<&'a bevy::camera::visibility::RenderLayers>,
    Option<&'a bevy::mesh::MeshTag>,
    (
        Has<crate::blockentities::BeRigNode>,
        Has<crate::util::particles::draw::ParticleBatch>,
        Has<crate::blockentities::BeamNode>,
        Has<crate::renderer::tethers::WeatherLayer>,
    ),
);

/// Rewrites `entities.bin` every few seconds.
#[allow(clippy::too_many_arguments, reason = "a system's queries")]
fn write(
    time: Res<Time>,
    mut since: Local<f32>,
    cache: Res<Cache>,
    drawn: Query<Drawn>,
    materials: Res<Assets<EntityMaterial>>,
    parts: Res<crate::renderer::rig_parts::RigParts>,
    camera: Query<(&GlobalTransform, &Projection), With<crate::renderer::systems::WorldCamera>>,
) {
    *since += time.delta_secs();
    if *since < EVERY {
        return;
    }
    *since = 0.0;
    let Some(dir) = super::terrain_dump::folder() else {
        return;
    };
    let world = bevy::camera::visibility::RenderLayers::default();
    let mut instances = Vec::new();
    let mut used_meshes = Vec::new();
    let mut used_images = Vec::new();
    let (mut mesh_ids, mut image_ids) = (HashMap::new(), HashMap::new());
    for (mesh, material, transform, visible, layers, tag, (block, particle, beam, weather)) in
        &drawn
    {
        if !visible.get() || layers.is_some_and(|l| !l.intersects(&world)) {
            continue;
        }
        let Some(material) = materials.get(&material.0) else {
            continue;
        };
        if !cache.meshes.contains_key(&mesh.0.id()) {
            continue;
        }
        let next = mesh_ids.len() as u32;
        let mesh_id = *mesh_ids.entry(mesh.0.id()).or_insert_with(|| {
            used_meshes.push(mesh.0.id());
            next
        });
        let texture = material
            .texture
            .as_ref()
            .filter(|t| cache.images.contains_key(&t.id()))
            .map_or(u32::MAX, |t| {
                let next = image_ids.len() as u32;
                *image_ids.entry(t.id()).or_insert_with(|| {
                    used_images.push(t.id());
                    next
                })
            });
        let p = &material.params;
        instances.push(EntityInstance {
            mesh: mesh_id,
            texture,
            tag: tag.map_or(0, |t| t.0),
            alpha: match material.alpha_mode {
                AlphaMode::Opaque => 0,
                AlphaMode::Mask(_) => 1,
                AlphaMode::Add => 3,
                _ => 2,
            },
            depth_bias: material.depth_bias,
            kind: match (block, particle, beam, weather) {
                (true, ..) => 1,
                (_, true, ..) => 2,
                (_, _, true, _) => 3,
                (.., true) => 4,
                _ => 0,
            },
            tint: p.tint.to_array(),
            light: p.light.to_array(),
            light0: p.light0.to_array(),
            light1: p.light1.to_array(),
            switches: p.switches.to_array(),
            levels: [p.light_block.w, p.light_sky.w],
            mode: p.mode,
            see_through: u32::from(material.see_through),
            transform: transform.to_matrix().to_cols_array(),
        });
    }

    let mut out = Vec::new();
    let mut put = |bytes: &[u8]| out.extend_from_slice(bytes);
    put(MAGIC);
    put(&VERSION.to_le_bytes());
    let (cam, fov) = camera
        .iter()
        .next()
        .map(|(t, p)| {
            let fov = match p {
                Projection::Perspective(p) => p.fov,
                _ => 70f32.to_radians(),
            };
            (t.to_matrix(), fov)
        })
        .unwrap_or((Mat4::IDENTITY, 70f32.to_radians()));
    put(bytemuck::cast_slice(&cam.to_cols_array()));
    put(&fov.to_le_bytes());
    put(&(used_images.len() as u32).to_le_bytes());
    for (i, id) in used_images.iter().enumerate() {
        let (w, h, rgba) = &cache.images[id];
        for n in [i as u32, *w, *h] {
            put(&n.to_le_bytes());
        }
        put(rgba);
    }
    put(&(used_meshes.len() as u32).to_le_bytes());
    for (i, id) in used_meshes.iter().enumerate() {
        let (verts, indices, rigged) = &cache.meshes[id];
        for n in [
            i as u32,
            verts.len() as u32,
            indices.len() as u32,
            u32::from(*rigged),
        ] {
            put(&n.to_le_bytes());
        }
        put(bytemuck::cast_slice(verts));
        put(bytemuck::cast_slice(indices));
    }
    let rows = parts.rows();
    put(&(rows.len() as u32).to_le_bytes());
    put(bytemuck::cast_slice(rows));
    put(&(instances.len() as u32).to_le_bytes());
    put(bytemuck::cast_slice(&instances));

    // Written aside then renamed, so a reader never sees half a snapshot.
    let _ = std::fs::create_dir_all(&dir);
    let partial = dir.join("entities.bin.part");
    if std::fs::write(&partial, &out).is_ok() {
        let _ = std::fs::rename(&partial, dir.join("entities.bin"));
    }
}

/// A snapshot read back.
pub struct Snapshot {
    pub camera: Mat4,
    pub fov: f32,
    /// Width, height and RGBA8 pixels, by id.
    pub textures: Vec<(u32, u32, Vec<u8>)>,
    /// Vertices, indices, and whether the mesh is a GPU-posed rig.
    pub meshes: Vec<(Vec<EntityVertex>, Vec<u32>, bool)>,
    pub parts: Vec<[f32; 12]>,
    pub instances: Vec<EntityInstance>,
}

/// Reads `entities.bin`.
pub fn read(bytes: &[u8]) -> Result<Snapshot, String> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let slice = bytes.get(at..at + n).ok_or("entities.bin ends early")?;
        at += n;
        Ok(slice)
    };
    if take(4)? != MAGIC {
        return Err("not an entity dump".to_owned());
    }
    let u32_at = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let version = u32_at(take(4)?);
    if version != VERSION {
        return Err(format!("entity dump version {version}, expected {VERSION}"));
    }
    let camera = Mat4::from_cols_array(&bytemuck::pod_read_unaligned::<[f32; 16]>(take(64)?));
    let fov = f32::from_le_bytes(take(4)?.try_into().unwrap());
    let mut textures = Vec::new();
    for _ in 0..u32_at(take(4)?) {
        let head = take(12)?;
        let (w, h) = (u32_at(&head[4..8]), u32_at(&head[8..12]));
        textures.push((w, h, take((w * h * 4) as usize)?.to_vec()));
    }
    let mut meshes = Vec::new();
    for _ in 0..u32_at(take(4)?) {
        let head = take(16)?;
        let (v, i) = (u32_at(&head[4..8]) as usize, u32_at(&head[8..12]) as usize);
        let rigged = u32_at(&head[12..16]) != 0;
        let verts = bytemuck::pod_collect_to_vec(take(v * 52)?);
        let indices = bytemuck::pod_collect_to_vec(take(i * 4)?);
        meshes.push((verts, indices, rigged));
    }
    let count = u32_at(take(4)?) as usize;
    let parts = bytemuck::pod_collect_to_vec(take(count * 48)?);
    let count = u32_at(take(4)?) as usize;
    let instances = bytemuck::pod_collect_to_vec(take(count * 184)?);
    Ok(Snapshot {
        camera,
        fov,
        textures,
        meshes,
        parts,
        instances,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_reads_back() {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()));
        out.extend_from_slice(&1.5f32.to_le_bytes());
        // One 1×1 texture.
        for n in [1u32, 0, 1, 1] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(&[9, 8, 7, 255]);
        // One rigged mesh: three vertices, three indices.
        for n in [1u32, 0, 3, 3, 1] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        let verts = [EntityVertex {
            part: 2,
            ..Default::default()
        }; 3];
        out.extend_from_slice(bytemuck::cast_slice(&verts));
        out.extend_from_slice(bytemuck::cast_slice(&[0u32, 1, 2]));
        // Two part rows, one instance.
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(bytemuck::cast_slice(&[[1.0f32; 12]; 2]));
        out.extend_from_slice(&1u32.to_le_bytes());
        let instance = EntityInstance {
            tag: 5,
            alpha: 2,
            ..Default::default()
        };
        out.extend_from_slice(bytemuck::bytes_of(&instance));

        let s = read(&out).unwrap();
        assert_eq!(s.fov, 1.5);
        assert_eq!(s.textures, vec![(1, 1, vec![9, 8, 7, 255])]);
        assert_eq!(s.meshes.len(), 1);
        assert!(s.meshes[0].2, "the rigged flag survives");
        assert_eq!(s.meshes[0].0[1].part, 2);
        assert_eq!(s.parts.len(), 2);
        assert_eq!((s.instances[0].tag, s.instances[0].alpha), (5, 2));
        assert!(
            read(&out[..out.len() - 1]).is_err(),
            "a cut file is refused"
        );
    }
}
