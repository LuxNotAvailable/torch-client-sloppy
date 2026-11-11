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
//!
//! Beside it, `env.bin` (`TVKS`, version 1) holds what surrounds the world
//! (VULKAN_REWRITE_CHECKLIST.md phase 5):
//! - the clear colour (4 × f32), and the fog: colour (4 × f32), the
//!   environmental start and end and the render-distance start and end
//!   (4 × f32)
//! - the lightmap: 16 × 16 RGBA8, sRGB, block level across, sky down
//! - textures and meshes as in `entities.bin`
//! - parts: count, then `EnvPart` (112 bytes): the sky parts and the cloud
//!   layer
//!
//! And `particles.bin` (`TVKP`, version 1), the live particles as sprites
//! the vertex shader billboards (VULKAN_REWRITE_CHECKLIST.md 4.6); their
//! batch meshes stay out of `entities.bin`:
//! - textures as in `entities.bin` (the particle sheet, the item atlas)
//! - sprites: count, then `ParticleSprite` (80 bytes), oldest first

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

pub const PARTICLE_MAGIC: &[u8; 4] = b"TVKP";
pub const PARTICLE_VERSION: u32 = 1;

/// One live particle, billboarded in the vertex shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ParticleSprite {
    /// The world position of its centre.
    pub centre: [f32; 3],
    /// Half its side, in blocks.
    pub half: f32,
    /// Its sprite's UV rectangle: u0, v0, u1, v1.
    pub uv: [f32; 4],
    pub color: [f32; 4],
    /// Its roll about the view axis (falling leaves), in radians.
    pub roll: f32,
    /// The texture's id, `TEXTURE_ATLAS` for the block atlas.
    pub texture: u32,
    /// Block and sky light levels, times 16 (as entities' `levels`).
    pub levels: [f32; 2],
    /// `PARTICLE_TRANSLUCENT` when it blends.
    pub flags: u32,
    pub pad: [u32; 3],
}

const _: () = assert!(std::mem::size_of::<ParticleSprite>() == 80);

/// The sprite blends (vanilla's translucent particle layer).
pub const PARTICLE_TRANSLUCENT: u32 = 1;

pub const ENV_MAGIC: &[u8; 4] = b"TVKS";
pub const ENV_VERSION: u32 = 2;

/// A screen part's texture that is the terrain atlas (the wall overlay),
/// which vkview has from the terrain dump.
pub const TEXTURE_ATLAS: u32 = u32::MAX - 1;

/// One sky part, the cloud layer, or a screen overlay.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EnvPart {
    pub mesh: u32,
    /// The texture's id, or `u32::MAX` for none.
    pub texture: u32,
    /// 0 opaque, 1 cutout, 2 blended, 3 additive.
    pub alpha: u32,
    /// 0 a sky part (pinned to the far plane), 1 the clouds (at their
    /// depth), 2 a screen overlay (its transform straight to clip space,
    /// drawn over everything).
    pub kind: u32,
    /// Drawing order among the parts, lowest first.
    pub bias: f32,
    pub pad: [f32; 3],
    /// The texture coordinates' scale (xy) and offset (zw).
    pub uv: [f32; 4],
    pub tint: [f32; 4],
    /// The world transform (clip from local for overlays), columns.
    pub transform: [f32; 16],
}

const _: () = assert!(std::mem::size_of::<EnvPart>() == 128);

/// A screen overlay (`renderer::screen_effects`), in the hand camera's view
/// space.
type ScreenDrawn<'a> = (
    &'a Mesh3d,
    &'a MeshMaterial3d<StandardMaterial>,
    &'a Transform,
    &'a InheritedVisibility,
);

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
        .add_systems(PostUpdate, (capture, write, write_particles).chain());
}

fn mesh_data(mesh: &Mesh) -> Option<(Vec<EntityVertex>, Vec<u32>, bool)> {
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
    // Meshes made before the dump's first frame (the sky's stars and
    // moon at the loading screen) sent their events unseen too.
    let missed: Vec<_> = meshes
        .iter()
        .filter(|(id, _)| !cache.meshes.contains_key(id))
        .filter_map(|(id, mesh)| Some((id, mesh_data(mesh)?)))
        .collect();
    cache.meshes.extend(missed);
    // Images made before the dump's first frame (weather at the loading
    // screen) sent their events unseen: pick up any still readable.
    let missed: Vec<_> = images
        .iter()
        .filter(|(id, _)| !cache.images.contains_key(id))
        .filter_map(|(id, image)| Some((id, image_data(image)?)))
        .collect();
    cache.images.extend(missed);
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
type SkyDrawn<'a> = (
    &'a Mesh3d,
    &'a MeshMaterial3d<crate::renderer::sky::SkyPartMaterial>,
    &'a GlobalTransform,
    &'a InheritedVisibility,
);

type CloudDrawn<'a> = (
    &'a Mesh3d,
    &'a MeshMaterial3d<StandardMaterial>,
    &'a GlobalTransform,
    &'a InheritedVisibility,
);

fn alpha_code(mode: AlphaMode) -> u32 {
    match mode {
        AlphaMode::Opaque => 0,
        AlphaMode::Mask(_) => 1,
        AlphaMode::Add => 3,
        _ => 2,
    }
}

/// Writes `env.bin`: fog, clear colour, lightmap, sky parts and clouds.
#[allow(clippy::too_many_arguments, reason = "a system's queries")]
fn write_env(
    dir: &std::path::Path,
    cache: &Cache,
    sky: &Query<SkyDrawn>,
    clouds: &Query<CloudDrawn, With<crate::renderer::clouds::CloudLayer>>,
    sky_materials: &Assets<crate::renderer::sky::SkyPartMaterial>,
    standard: &Assets<StandardMaterial>,
    lightmap: Option<&crate::renderer::terrain::LightmapPixels>,
    fog: Option<(&DistanceFog, &Camera)>,
    (overlays, hand, atlas, warp): (
        &Query<ScreenDrawn, With<crate::renderer::screen_effects::Overlay>>,
        Option<Mat4>,
        Option<AssetId<Image>>,
        Mat4,
    ),
) {
    let mut parts = Vec::new();
    let (mut mesh_ids, mut image_ids) = (HashMap::new(), HashMap::new());
    let (mut used_meshes, mut used_images) = (Vec::new(), Vec::new());
    let mut add = |mesh: AssetId<Mesh>,
                   texture: Option<AssetId<Image>>,
                   tint: Vec4,
                   alpha: AlphaMode,
                   bias: f32,
                   kind: u32,
                   uv: [f32; 4],
                   transform: Mat4| {
        if !cache.meshes.contains_key(&mesh) {
            return;
        }
        let next = mesh_ids.len() as u32;
        let mesh_id = *mesh_ids.entry(mesh).or_insert_with(|| {
            used_meshes.push(mesh);
            next
        });
        // The atlas goes by name: vkview has its own copy.
        let texture = texture
            .filter(|t| Some(*t) == atlas || cache.images.contains_key(t))
            .map_or(u32::MAX, |t| {
                if Some(t) == atlas {
                    return TEXTURE_ATLAS;
                }
                let next = image_ids.len() as u32;
                *image_ids.entry(t).or_insert_with(|| {
                    used_images.push(t);
                    next
                })
            });
        parts.push(EnvPart {
            mesh: mesh_id,
            texture,
            alpha: alpha_code(alpha),
            kind,
            bias,
            pad: [0.0; 3],
            uv,
            tint: tint.to_array(),
            transform: transform.to_cols_array(),
        });
    };
    for (mesh, material, transform, visible) in sky {
        if !visible.get() {
            continue;
        }
        if let Some(m) = sky_materials.get(&material.0) {
            let (tint, texture, alpha, bias) = m.parts();
            add(
                mesh.0.id(),
                texture.map(Handle::id),
                tint,
                alpha,
                bias,
                0,
                [1.0, 1.0, 0.0, 0.0],
                transform.to_matrix(),
            );
        }
    }
    for (mesh, material, transform, visible) in clouds {
        if !visible.get() {
            continue;
        }
        if let Some(m) = standard.get(&material.0) {
            add(
                mesh.0.id(),
                m.base_color_texture.as_ref().map(Handle::id),
                m.base_color.to_linear().to_vec4(),
                m.alpha_mode,
                m.depth_bias,
                1,
                [1.0, 1.0, 0.0, 0.0],
                transform.to_matrix(),
            );
        }
    }
    // Screen overlays as the hand camera draws them: their transforms are
    // in its view space.
    if let Some(clip_from_view) = hand {
        for (i, (mesh, material, transform, visible)) in overlays.iter().enumerate() {
            if !visible.get() {
                continue;
            }
            if let Some(m) = standard.get(&material.0) {
                let uv = m.uv_transform;
                add(
                    mesh.0.id(),
                    m.base_color_texture.as_ref().map(Handle::id),
                    m.base_color.to_linear().to_vec4(),
                    m.alpha_mode,
                    i as f32,
                    2,
                    [
                        uv.matrix2.x_axis.x,
                        uv.matrix2.y_axis.y,
                        uv.translation.x,
                        uv.translation.y,
                    ],
                    clip_from_view * transform.to_matrix(),
                );
            }
        }
    }

    let mut out = Vec::new();
    let mut put = |bytes: &[u8]| out.extend_from_slice(bytes);
    put(ENV_MAGIC);
    put(&ENV_VERSION.to_le_bytes());
    let (clear, fog_color, ranges) = match fog {
        Some((fog, camera)) => {
            let clear = match camera.clear_color {
                ClearColorConfig::Custom(c) => c.to_linear().to_f32_array(),
                _ => fog.color.to_linear().to_f32_array(),
            };
            let (start, end) = match fog.falloff {
                bevy::pbr::FogFalloff::Linear { start, end } => (start, end),
                _ => (0.0, f32::MAX),
            };
            let env = fog.directional_light_color.to_linear();
            (
                clear,
                fog.color.to_linear().to_f32_array(),
                [env.red, env.green, start, end],
            )
        }
        None => ([0.0; 4], [0.0; 4], [0.0, f32::MAX, 0.0, f32::MAX]),
    };
    put(bytemuck::cast_slice(&clear));
    put(bytemuck::cast_slice(&fog_color));
    put(bytemuck::cast_slice(&ranges));
    // The lightmap is rewritten each frame from these pixels; the image
    // asset itself keeps its first fill.
    let lightmap = lightmap
        .map(|l| l.data.clone())
        .filter(|d| d.len() >= 1024)
        .unwrap_or_else(|| vec![255; 1024]);
    put(&lightmap[..1024]);
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
    put(&(parts.len() as u32).to_le_bytes());
    put(bytemuck::cast_slice(&parts));
    // The portal and nausea warp on the world's projection, and the
    // server's post effects (`renderer::post_effects` flags).
    put(bytemuck::cast_slice(&warp.to_cols_array()));
    put(bytemuck::cast_slice(&[
        crate::renderer::post_effects::active(),
        0,
        0,
        0,
    ]));
    let partial = dir.join("env.bin.part");
    if std::fs::write(&partial, &out).is_ok() {
        let _ = std::fs::rename(&partial, dir.join("env.bin"));
    }
}

#[allow(clippy::too_many_arguments, reason = "a system's queries")]
fn write(
    mut commands: Commands,
    time: Res<Time>,
    mut since: Local<f32>,
    cache: Res<Cache>,
    drawn: Query<Drawn>,
    materials: Res<Assets<EntityMaterial>>,
    parts: Res<crate::renderer::rig_parts::RigParts>,
    camera: Query<(&GlobalTransform, &Projection), With<crate::renderer::systems::WorldCamera>>,
    (sky, clouds, sky_materials, standard, images, fog): (
        Query<SkyDrawn>,
        Query<CloudDrawn, With<crate::renderer::clouds::CloudLayer>>,
        Res<Assets<crate::renderer::sky::SkyPartMaterial>>,
        Res<Assets<StandardMaterial>>,
        Option<Res<crate::renderer::terrain::LightmapPixels>>,
        Query<(&DistanceFog, &Camera), With<crate::renderer::systems::WorldCamera>>,
    ),
    (overlays, hand, terrain_textures, frame_view): (
        Query<ScreenDrawn, With<crate::renderer::screen_effects::Overlay>>,
        Query<&Projection, With<crate::renderer::hand::HandCamera>>,
        Option<Res<crate::renderer::terrain_pool::TerrainTextures>>,
        Res<crate::renderer::frame_view::FrameView>,
    ),
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
        // Particles go to `particles.bin`, as sprites.
        if particle {
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

    // Out of a world (the title screen after leaving a server) the camera
    // sits at the origin with nothing to draw: keep the last snapshot taken
    // in the world rather than overwrite it.
    if camera
        .iter()
        .next()
        .is_none_or(|(t, _)| t.translation() == Vec3::ZERO)
    {
        return;
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
    // The game's own frame beside it, the ground truth for vkview
    // (`frames/0000.png` is the latest; one frame late, which a still
    // camera doesn't mind).
    crate::renderer::screenshot::request_numbered(&mut commands, dir.join("frames"), 0);
    write_env(
        &dir,
        &cache,
        &sky,
        &clouds,
        &sky_materials,
        &standard,
        images.as_deref(),
        fog.iter().next(),
        (
            &overlays,
            hand.iter().next().map(|p| p.get_clip_from_view()),
            terrain_textures.as_ref().map(|t| t.atlas.id()),
            crate::renderer::view_bob::current_warp(&frame_view),
        ),
    );
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

/// `env.bin` read back.
pub struct Environment {
    pub clear: [f32; 4],
    pub fog_color: [f32; 4],
    /// Environmental start and end, render-distance start and end.
    pub fog_ranges: [f32; 4],
    /// 16 × 16 RGBA8 (sRGB): block level across, sky level down.
    pub lightmap: Vec<u8>,
    pub textures: Vec<(u32, u32, Vec<u8>)>,
    pub meshes: Vec<(Vec<EntityVertex>, Vec<u32>, bool)>,
    pub parts: Vec<EnvPart>,
    /// The portal and nausea warp, applied after the projection's view.
    pub warp: [f32; 16],
    /// The server's post effects (`renderer::post_effects` flags).
    pub post: u32,
}

/// Reads `env.bin`.
pub fn read_env(bytes: &[u8]) -> Result<Environment, String> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let slice = bytes.get(at..at + n).ok_or("env.bin ends early")?;
        at += n;
        Ok(slice)
    };
    if take(4)? != ENV_MAGIC {
        return Err("not an environment dump".to_owned());
    }
    let u32_at = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let version = u32_at(take(4)?);
    if version != ENV_VERSION {
        return Err(format!(
            "environment dump version {version}, expected {ENV_VERSION}"
        ));
    }
    let clear = bytemuck::pod_read_unaligned::<[f32; 4]>(take(16)?);
    let fog_color = bytemuck::pod_read_unaligned::<[f32; 4]>(take(16)?);
    let fog_ranges = bytemuck::pod_read_unaligned::<[f32; 4]>(take(16)?);
    let lightmap = take(1024)?.to_vec();
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
    let parts = bytemuck::pod_collect_to_vec(take(count * std::mem::size_of::<EnvPart>())?);
    let warp = bytemuck::pod_read_unaligned::<[f32; 16]>(take(64)?);
    let post = u32_at(&take(16)?[..4]);
    Ok(Environment {
        clear,
        fog_color,
        fog_ranges,
        lightmap,
        textures,
        meshes,
        parts,
        warp,
        post,
    })
}

/// Rewrites `particles.bin` every few seconds, with `entities.bin`.
fn write_particles(
    time: Res<Time>,
    mut since: Local<f32>,
    cache: Res<Cache>,
    live: Option<Res<crate::util::particles::LiveParticles>>,
    atlas: Option<Res<crate::util::particles::ParticleAtlas>>,
    gui: Option<Res<crate::gui::render::GuiAssets>>,
    camera: Query<&GlobalTransform, With<crate::renderer::systems::WorldCamera>>,
) {
    use crate::util::particles::draw::{SpriteTexture, live_sprites};
    *since += time.delta_secs();
    if *since < EVERY {
        return;
    }
    *since = 0.0;
    let Some(dir) = super::terrain_dump::folder() else {
        return;
    };
    // Out of a world: keep the last snapshot taken in one (see `write`).
    if camera
        .iter()
        .next()
        .is_none_or(|t| t.translation() == Vec3::ZERO)
    {
        return;
    }
    let (Some(live), Some(atlas)) = (live, atlas) else {
        return;
    };
    let sheet = atlas.image().map(|h| h.id());
    let items = gui.as_ref().map(|g| g.image.id());
    let mut used_images: Vec<AssetId<Image>> = Vec::new();
    let mut sprites = Vec::new();
    for sprite in live_sprites(&live, &atlas) {
        let texture = match sprite.texture {
            SpriteTexture::Terrain => TEXTURE_ATLAS,
            SpriteTexture::Sheet | SpriteTexture::Items => {
                let id = if sprite.texture == SpriteTexture::Sheet {
                    sheet
                } else {
                    items
                };
                let Some(id) = id.filter(|id| cache.images.contains_key(id)) else {
                    continue;
                };
                match used_images.iter().position(|u| *u == id) {
                    Some(at) => at as u32,
                    None => {
                        used_images.push(id);
                        used_images.len() as u32 - 1
                    }
                }
            }
        };
        let shape = &sprite.shape;
        sprites.push(ParticleSprite {
            centre: shape.centre.to_array(),
            half: shape.half,
            uv: shape.uv,
            color: shape.color,
            roll: shape.roll,
            texture,
            levels: [
                f32::from(sprite.levels.0) * 16.0,
                f32::from(sprite.levels.1) * 16.0,
            ],
            flags: if sprite.translucent {
                PARTICLE_TRANSLUCENT
            } else {
                0
            },
            pad: [0; 3],
        });
    }
    let mut out = Vec::new();
    out.extend_from_slice(PARTICLE_MAGIC);
    out.extend_from_slice(&PARTICLE_VERSION.to_le_bytes());
    out.extend_from_slice(&(used_images.len() as u32).to_le_bytes());
    for (i, id) in used_images.iter().enumerate() {
        let (w, h, rgba) = &cache.images[id];
        for n in [i as u32, *w, *h] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(rgba);
    }
    out.extend_from_slice(&(sprites.len() as u32).to_le_bytes());
    out.extend_from_slice(bytemuck::cast_slice(&sprites));
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join("particles.bin.tmp");
    if std::fs::write(&tmp, &out).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join("particles.bin"));
    }
}

/// `particles.bin` as written: textures (id, width, height, RGBA8) and the
/// sprites.
pub struct ParticleSnapshot {
    pub textures: Vec<(u32, u32, Vec<u8>)>,
    pub sprites: Vec<ParticleSprite>,
}

pub fn read_particles(bytes: &[u8]) -> Result<ParticleSnapshot, String> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let slice = bytes
            .get(at..at + n)
            .ok_or_else(|| "particles.bin ends early".to_owned())?;
        at += n;
        Ok(slice)
    };
    if take(4)? != PARTICLE_MAGIC {
        return Err("not a particle snapshot".to_owned());
    }
    let u32_at = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap_or([0; 4]));
    let version = u32_at(take(4)?);
    if version != PARTICLE_VERSION {
        return Err(format!("particle snapshot version {version}"));
    }
    let count = u32_at(take(4)?) as usize;
    let mut textures = Vec::with_capacity(count);
    for _ in 0..count {
        let _id = u32_at(take(4)?);
        let w = u32_at(take(4)?);
        let h = u32_at(take(4)?);
        let rgba = take((w * h * 4) as usize)?.to_vec();
        textures.push((w, h, rgba));
    }
    let count = u32_at(take(4)?) as usize;
    let size = std::mem::size_of::<ParticleSprite>();
    let raw = take(count * size)?;
    let sprites = raw
        .chunks_exact(size)
        .map(bytemuck::pod_read_unaligned::<ParticleSprite>)
        .collect();
    Ok(ParticleSnapshot { textures, sprites })
}
