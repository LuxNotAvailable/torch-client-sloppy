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

use crate::draw::*;
use crate::renderer::entity_material::EntityMaterial;

pub const MAGIC: &[u8; 4] = b"TVKE";
pub const VERSION: u32 = 2;

/// Seconds between rewrites; the last one stands when the game stops.
const EVERY: f32 = 3.0;

pub const PARTICLE_MAGIC: &[u8; 4] = b"TVKP";
pub const PARTICLE_VERSION: u32 = 1;

pub const ENV_MAGIC: &[u8; 4] = b"TVKS";
pub const ENV_VERSION: u32 = 2;

/// Snapshots for the live renderer (`vk::live`), by file name, as the dump
/// would write them; each taken once.
static LIVE: std::sync::Mutex<Vec<(&'static str, Vec<u8>)>> = std::sync::Mutex::new(Vec::new());

/// The latest snapshot named `name` (`entities.bin`, `env.bin`,
/// `particles.bin`, `inputs.json`), if a new one came since the last take.
pub fn take_live(name: &str) -> Option<Vec<u8>> {
    let mut live = LIVE.lock().ok()?;
    let at = live.iter().position(|(n, _)| *n == name)?;
    Some(live.swap_remove(at).1)
}

/// Hands a snapshot to the live renderer, or writes it beside the dump
/// (aside then renamed, so a reader never sees half of one).
fn deliver(dir: Option<&std::path::Path>, name: &'static str, bytes: Vec<u8>) {
    if super::live::wanted() {
        if let Ok(mut live) = LIVE.lock() {
            live.retain(|(n, _)| *n != name);
            live.push((name, bytes));
        }
        return;
    }
    let Some(dir) = dir else { return };
    let _ = std::fs::create_dir_all(dir);
    let partial = dir.join(format!("{name}.part"));
    if std::fs::write(&partial, &bytes).is_ok() {
        let _ = std::fs::rename(&partial, dir.join(name));
    }
}

/// Whether a writer runs this frame: every frame live, else every
/// `EVERY` seconds.
fn due(since: &mut f32, dt: f32) -> bool {
    if super::live::wanted() {
        return true;
    }
    *since += dt;
    if *since < EVERY {
        return false;
    }
    *since = 0.0;
    true
}

/// What a layer draws: instances naming meshes and textures by place in
/// the two lists that follow.
type Collected = (Vec<EntityInstance>, Vec<AssetId<Mesh>>, Vec<AssetId<Image>>);

/// A frame's entities for the live renderer (`vk::live`): meshes and
/// textures new since the last frame taken, by stable index, then this
/// frame's instances (naming those indices) and pose rows.
#[derive(Default)]
pub struct LiveEntities {
    pub meshes: Vec<(u32, Vec<EntityVertex>, Vec<u32>, bool)>,
    pub textures: Vec<(u32, u32, u32, Vec<u8>)>,
    pub parts: Vec<[f32; 12]>,
    pub instances: Vec<EntityInstance>,
}

/// Stable indices for what a live channel has sent: by asset, with the
/// address of the cached data so a modified asset is sent anew.
#[derive(Default)]
struct Registry {
    /// Index, the cached data's address, and a hash of its contents.
    meshes: HashMap<AssetId<Mesh>, (u32, usize, u64)>,
    images: HashMap<AssetId<Image>, (u32, usize)>,
    next_mesh: u32,
    next_image: u32,
}

/// The live channels: the world, and the inventory figure.
pub const LIVE_WORLD: usize = 0;
pub const LIVE_FIGURE: usize = 1;
static LIVE_ENTITIES: std::sync::Mutex<Vec<(Registry, Option<LiveEntities>)>> =
    std::sync::Mutex::new(Vec::new());

/// The channel's entities since the last take, if a frame came.
pub fn take_live_entities(channel: usize) -> Option<LiveEntities> {
    LIVE_ENTITIES.lock().ok()?.get_mut(channel)?.1.take()
}

/// Forgets what a channel sent (its renderer was remade).
pub fn reset_live_entities(channel: usize) {
    if let Ok(mut live) = LIVE_ENTITIES.lock()
        && let Some(slot) = live.get_mut(channel)
    {
        *slot = Default::default();
    }
}

/// A mesh's contents, hashed: a rebuilt mesh with the same data isn't
/// sent again.
fn content_hash(verts: &[EntityVertex], indices: &[u32], rigged: bool) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write(bytemuck::cast_slice(verts));
    h.write(bytemuck::cast_slice(indices));
    h.write_u8(u8::from(rigged));
    h.finish()
}

/// A frame's particles for the live renderer: textures new since the last
/// frame taken (stable index, size, texels), then this frame's sprites
/// naming those indices (or `TEXTURE_ATLAS`).
#[derive(Default)]
pub struct LiveParticles {
    pub textures: Vec<(u32, u32, u32, Vec<u8>)>,
    pub sprites: Vec<ParticleSprite>,
}

static LIVE_PARTICLES: std::sync::Mutex<Option<(Registry, Option<LiveParticles>)>> =
    std::sync::Mutex::new(None);

/// The particles since the last take, if a frame came.
pub fn take_live_particles() -> Option<LiveParticles> {
    LIVE_PARTICLES.lock().ok()?.as_mut()?.1.take()
}

fn particles_to_live(
    cache: &Cache,
    mut sprites: Vec<ParticleSprite>,
    used_images: &[AssetId<Image>],
) {
    let Ok(mut live) = LIVE_PARTICLES.lock() else {
        return;
    };
    let (registry, pending) = live.get_or_insert_with(Default::default);
    let mut frame = pending.take().unwrap_or_default();
    let index: Vec<u32> = used_images
        .iter()
        .map(|id| {
            let (w, h, rgba) = &cache.images[id];
            let key = rgba.as_ptr() as usize;
            match registry.images.get(id) {
                Some(&(index, at)) if at == key => index,
                _ => {
                    let index = registry.images.get(id).map_or_else(
                        || {
                            registry.next_image += 1;
                            registry.next_image - 1
                        },
                        |&(index, _)| index,
                    );
                    registry.images.insert(*id, (index, key));
                    frame.textures.push((index, *w, *h, rgba.clone()));
                    index
                }
            }
        })
        .collect();
    for s in &mut sprites {
        if s.texture != TEXTURE_ATLAS {
            s.texture = index[s.texture as usize];
        }
    }
    frame.sprites = sprites;
    *pending = Some(frame);
}

fn to_live(
    channel: usize,
    cache: &Cache,
    (mut instances, used_meshes, used_images): Collected,
    rows: &[[f32; 12]],
) {
    let Ok(mut live) = LIVE_ENTITIES.lock() else {
        return;
    };
    while live.len() <= channel {
        live.push(Default::default());
    }
    let (registry, pending) = &mut live[channel];
    // Frames not yet taken keep their new meshes and textures.
    let mut frame = pending.take().unwrap_or_default();
    let mesh_index: Vec<u32> = used_meshes
        .iter()
        .map(|id| {
            let (verts, indices, rigged) = &cache.meshes[id];
            let key = verts.as_ptr() as usize;
            match registry.meshes.get_mut(id) {
                Some((index, at, _)) if *at == key => *index,
                // Rebuilt (the same asset, new data): sent again only when
                // its contents changed, under the same index.
                Some((index, at, hash)) => {
                    *at = key;
                    let new = content_hash(verts, indices, *rigged);
                    if new != *hash {
                        *hash = new;
                        frame
                            .meshes
                            .push((*index, verts.clone(), indices.clone(), *rigged));
                        super::live::count_new(0, verts.len());
                    }
                    *index
                }
                None => {
                    let index = registry.next_mesh;
                    registry.next_mesh += 1;
                    registry
                        .meshes
                        .insert(*id, (index, key, content_hash(verts, indices, *rigged)));
                    frame
                        .meshes
                        .push((index, verts.clone(), indices.clone(), *rigged));
                    super::live::count_new(0, verts.len());
                    index
                }
            }
        })
        .collect();
    let image_index: Vec<u32> = used_images
        .iter()
        .map(|id| {
            let (w, h, rgba) = &cache.images[id];
            let key = rgba.as_ptr() as usize;
            match registry.images.get(id) {
                Some(&(index, at)) if at == key => index,
                _ => {
                    let index = registry.next_image;
                    registry.next_image += 1;
                    registry.images.insert(*id, (index, key));
                    frame.textures.push((index, *w, *h, rgba.clone()));
                    super::live::count_new(1, rgba.len());
                    index
                }
            }
        })
        .collect();
    for i in &mut instances {
        i.mesh = mesh_index[i.mesh as usize];
        if i.texture != u32::MAX {
            i.texture = image_index[i.texture as usize];
        }
    }
    frame.instances = instances;
    frame.parts = rows.to_vec();
    *pending = Some(frame);
}

/// Mesh and image data copied while they still sit in the main world.
#[derive(Resource, Default)]
struct Cache {
    meshes: HashMap<AssetId<Mesh>, (Vec<EntityVertex>, Vec<u32>, bool)>,
    images: HashMap<AssetId<Image>, (u32, u32, Vec<u8>)>,
    /// Images sampled with repeat (rain, beams); the rest clamp.
    repeat: std::collections::HashSet<AssetId<Image>>,
}

fn repeats(image: &Image) -> bool {
    match &image.sampler {
        bevy::image::ImageSampler::Descriptor(d) => {
            d.address_mode_u == bevy::image::ImageAddressMode::Repeat
        }
        _ => false,
    }
}

/// Adds the dump's systems when `MC_DUMP_TERRAIN` names a folder.
pub fn plugin(app: &mut App) {
    if super::terrain_dump::folder().is_none() && !super::live::wanted() {
        return;
    }
    app.init_resource::<Cache>().add_systems(
        PostUpdate,
        (capture, write, write_particles)
            .chain()
            .after(crate::client::view::ViewSystems)
            .after(crate::draw::DrawSystems),
    );
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
                if let Some(image) = images.get(id) {
                    if repeats(image) {
                        cache.repeat.insert(id);
                    } else {
                        cache.repeat.remove(&id);
                    }
                }
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
    Entity,
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

fn alpha_mode(code: u32) -> AlphaMode {
    match code {
        0 => AlphaMode::Opaque,
        1 => AlphaMode::Mask(0.5),
        3 => AlphaMode::Add,
        _ => AlphaMode::Blend,
    }
}

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
    dir: Option<&std::path::Path>,
    cache: &Cache,
    env: &crate::draw::Env,
    lightmap: Option<&crate::renderer::terrain::LightmapPixels>,
    fog: Option<crate::client::view::Fog>,
    (hand, atlas, warp, water_alpha): (Option<Mat4>, Option<AssetId<Image>>, Mat4, f32),
    hand_view: Option<Mat4>,
) {
    let mut parts = Vec::new();
    let (mut mesh_ids, mut image_ids) = (HashMap::new(), HashMap::new());
    let (mut used_meshes, mut used_images) = (Vec::new(), Vec::new());
    let mut add = |mesh: std::sync::Arc<crate::draw::MeshData>,
                   texture: Option<AssetId<Image>>,
                   tint: Vec4,
                   alpha: AlphaMode,
                   bias: f32,
                   kind: u32,
                   uv: [f32; 4],
                   transform: Mat4,
                   piece: f32|
     -> bool {
        let next = mesh_ids.len() as u32;
        let mesh_id = *mesh_ids.entry(mesh.id).or_insert_with(|| {
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
            pad: [piece, 0.0, 0.0],
            uv,
            tint: tint.to_array(),
            transform: transform.to_cols_array(),
        });
        true
    };
    let add_draw = |add: &mut dyn FnMut(
        std::sync::Arc<crate::draw::MeshData>,
        Option<AssetId<Image>>,
        Vec4,
        AlphaMode,
        f32,
        u32,
        [f32; 4],
        Mat4,
        f32,
    ) -> bool,
                    part: &crate::draw::EnvDraw|
     -> bool {
        add(
            part.mesh.clone(),
            match &part.texture {
                crate::draw::Texture::None => None,
                crate::draw::Texture::Atlas => atlas,
                crate::draw::Texture::Image(h) => Some(h.id()),
            },
            Vec4::from_array(part.tint),
            alpha_mode(part.alpha),
            part.bias,
            part.kind,
            part.uv,
            part.transform,
            part.pad[0],
        )
    };
    for part in env.sky.iter().chain(&env.clouds).chain(&env.weather) {
        add_draw(&mut add, part);
    }
    // Screen overlays as the hand camera draws them: their transforms are
    // in its view space.
    if let Some(clip_from_view) = hand {
        for part in &env.screen {
            let part = EnvDraw {
                transform: clip_from_view * part.transform,
                ..part.clone()
            };
            add_draw(&mut add, &part);
        }
    }

    // The first-person hand, in the hand camera's view space.
    if let Some(view) = hand_view {
        for part in &env.hand {
            let part = EnvDraw {
                transform: view * part.transform,
                ..part.clone()
            };
            add_draw(&mut add, &part);
        }
    }
    // The world's overlays: outline, hitboxes and borders as lines, the
    // breaking cracks as faces.
    for part in &env.overlays {
        add_draw(&mut add, part);
    }
    // The held items' glint: their own texture as the mask, added over
    // them (`renderer::glint`); the glint texture's place in pad[2].
    let mut glints = 0;
    if let Some(view) = hand_view {
        for part in &env.hand_glints {
            let part = EnvDraw {
                transform: view * part.transform,
                ..part.clone()
            };
            if add_draw(&mut add, &part) {
                glints += 1;
            }
        }
    }
    let glint_id = crate::renderer::entity_material::GLINT_ITEM.id();
    if glints > 0 && cache.images.contains_key(&glint_id) {
        let next = image_ids.len() as u32;
        let index = *image_ids.entry(glint_id).or_insert_with(|| {
            used_images.push(glint_id);
            next
        });
        let start = parts.len() - glints;
        for part in &mut parts[start..] {
            part.pad[1] = 1.0;
            part.pad[2] = index as f32;
        }
    }

    let mut out = Vec::new();
    let mut put = |bytes: &[u8]| out.extend_from_slice(bytes);
    put(ENV_MAGIC);
    put(&ENV_VERSION.to_le_bytes());
    let (clear, fog_color, ranges) = match fog {
        Some(fog) => (
            fog.clear,
            fog.color,
            [fog.env[0], fog.env[1], fog.start, fog.end],
        ),
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
    for (i, mesh) in used_meshes.iter().enumerate() {
        let (verts, indices, rigged) = (&mesh.verts, &mesh.indices, mesh.rigged);
        for n in [
            i as u32,
            verts.len() as u32,
            indices.len() as u32,
            u32::from(rigged),
        ] {
            put(&n.to_le_bytes());
        }
        put(bytemuck::cast_slice(verts));
        put(bytemuck::cast_slice(indices));
    }
    put(&(parts.len() as u32).to_le_bytes());
    put(bytemuck::cast_slice(&parts));
    // The portal and nausea warp on the world's projection, the server's
    // post effects (`client::post_effects` flags), and the Water Clarity
    // setting's scale on the translucent layer's alpha (in what was
    // padding; older snapshots read it as 0, meaning unscaled).
    put(bytemuck::cast_slice(&warp.to_cols_array()));
    put(bytemuck::cast_slice(&[
        crate::client::post_effects::active(),
        water_alpha.to_bits(),
        0,
        0,
    ]));
    deliver(dir, "env.bin", out);
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
    view: Res<crate::client::view::View>,
    (env, images): (
        Res<crate::draw::Env>,
        Option<Res<crate::renderer::terrain::LightmapPixels>>,
    ),
    (terrain_textures, terrain_params): (
        Option<Res<crate::renderer::terrain_pool::TerrainTextures>>,
        Option<Res<crate::renderer::terrain_pool::TerrainParams>>,
    ),
    (ancestors, local_roots): (
        Query<&ChildOf>,
        Query<(), With<crate::renderer::player_model::LocalRigRoot>>,
    ),
) {
    if !due(&mut since, time.delta_secs()) {
        return;
    }
    let _timed = super::live::Timed::new(0);
    let dir = super::terrain_dump::folder();
    // Out of a world (the title screen after leaving a server) the camera
    // sits at the origin with nothing to draw: keep the last snapshot taken
    // in the world rather than overwrite it.
    let Some(eye) = view.world.filter(|e| e.position() != Vec3::ZERO) else {
        return;
    };
    let world_camera = (eye.world_from_view, eye.fov);
    let player_shadow = crate::shaderpack::packvertex::player_shadow();
    // Whether a mesh sits in the local player's rig.
    let under_local_rig = |mut at: Entity| {
        for _ in 0..8 {
            if local_roots.contains(at) {
                return true;
            }
            match ancestors.get(at) {
                Ok(parent) => at = parent.parent(),
                Err(_) => return false,
            }
        }
        false
    };
    // One snapshot of what a layer draws, seen from `(cam, fov)`.
    let collect = |world: bevy::camera::visibility::RenderLayers| -> Collected {
        let mut instances = Vec::new();
        let mut used_meshes = Vec::new();
        let mut used_images = Vec::new();
        let (mut mesh_ids, mut image_ids) = (HashMap::new(), HashMap::new());
        for (
            entity,
            mesh,
            material,
            transform,
            visible,
            layers,
            tag,
            (block, particle, beam, weather),
        ) in &drawn
        {
            if layers.is_some_and(|l| !l.intersects(&world)) {
                continue;
            }
            // The hidden local player still casts a pack's shadow.
            let shadow_only = !visible.get();
            if shadow_only && !(player_shadow && under_local_rig(entity)) {
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
                } | if material
                    .texture
                    .as_ref()
                    .is_some_and(|t| cache.repeat.contains(&t.id()))
                {
                    KIND_REPEAT
                } else {
                    0
                } | if shadow_only { KIND_SHADOW_ONLY } else { 0 },
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

        (instances, used_meshes, used_images)
    };
    let snapshot =
        |(instances, used_meshes, used_images): Collected, (cam, fov): (Mat4, f32)| -> Vec<u8> {
            let mut out = Vec::new();
            let mut put = |bytes: &[u8]| out.extend_from_slice(bytes);
            put(MAGIC);
            put(&VERSION.to_le_bytes());
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
            out
        };
    // Live, entities go straight across (`LiveEntities`): new meshes and
    // textures once, then each frame's instances and pose rows.
    let live = super::live::wanted();
    let world_layer = bevy::camera::visibility::RenderLayers::default();
    let out = if live {
        to_live(
            LIVE_WORLD,
            &cache,
            collect(world_layer),
            bytemuck::cast_slice(parts.rows()),
        );
        Vec::new()
    } else {
        snapshot(collect(world_layer), world_camera)
    };
    // The inventory's player figure, on its own layer, for the native GUI.
    if live && view.figure_ready {
        let figure =
            bevy::camera::visibility::RenderLayers::layer(crate::client::view::FIGURE_LAYER);
        to_live(
            LIVE_FIGURE,
            &cache,
            collect(figure),
            bytemuck::cast_slice(parts.rows()),
        );
    }

    // The game's own frame beside it, the ground truth for vkview
    // (`frames/0000.png` is the latest; one frame late, which a still
    // camera doesn't mind).
    if let Some(dir) = &dir
        && !super::live::wanted()
    {
        let _ = std::fs::create_dir_all(dir);
        crate::renderer::screenshot::request_numbered(&mut commands, dir.join("frames"), 0);
    }
    write_env(
        dir.as_deref(),
        &cache,
        &env,
        images.as_deref(),
        view.fog,
        (
            view.hand_clip_from_view,
            terrain_textures.as_ref().map(|t| t.atlas.id()),
            view.warp,
            terrain_params.as_ref().map_or(1.0, |p| p.water_alpha),
        ),
        view.hand_view_from_world,
    );
    if !live {
        deliver(dir.as_deref(), "entities.bin", out);
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

impl Snapshot {
    /// Nothing to draw: a live scene's start, before its first update.
    pub fn empty() -> Snapshot {
        Snapshot {
            camera: Mat4::IDENTITY,
            fov: 1.0,
            textures: Vec::new(),
            meshes: Vec::new(),
            parts: Vec::new(),
            instances: Vec::new(),
        }
    }
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
    /// The server's post effects (`client::post_effects` flags).
    pub post: u32,
    /// The Water Clarity scale on the translucent layer's alpha.
    pub water_alpha: f32,
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
    let tail = take(16)?;
    let post = u32_at(&tail[..4]);
    let water_alpha = match f32::from_bits(u32_at(&tail[4..8])) {
        a if a > 0.0 => a,
        _ => 1.0,
    };
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
        water_alpha,
    })
}

/// Rewrites `particles.bin` every few seconds, with `entities.bin`.
fn write_particles(
    time: Res<Time>,
    mut since: Local<f32>,
    cache: Res<Cache>,
    particles: Res<crate::draw::Particles>,
    view: Res<crate::client::view::View>,
    inputs: Option<Res<crate::shaderpack::inputs::Inputs>>,
) {
    if !due(&mut since, time.delta_secs()) {
        return;
    }
    let _timed = super::live::Timed::new(1);
    let dir = super::terrain_dump::folder();
    // Out of a world: keep the last snapshot taken in one (see `write`).
    if view.world.is_none_or(|e| e.position() == Vec3::ZERO) {
        return;
    }
    // The frame's pack inputs (time of day, sky, weather), for packs.
    if let Some(inputs) = inputs {
        deliver(
            dir.as_deref(),
            "inputs.json",
            inputs.to_json().to_string().into_bytes(),
        );
    }
    // Textures not readable yet drop their sprites; the rest renumber.
    let mut used_images: Vec<AssetId<Image>> = Vec::new();
    let remap: Vec<Option<u32>> = particles
        .textures
        .iter()
        .map(|h| {
            cache.images.contains_key(&h.id()).then(|| {
                used_images.push(h.id());
                used_images.len() as u32 - 1
            })
        })
        .collect();
    let sprites: Vec<ParticleSprite> = particles
        .sprites
        .iter()
        .filter_map(|s| {
            if s.texture == TEXTURE_ATLAS {
                return Some(*s);
            }
            let texture = remap[s.texture as usize]?;
            Some(ParticleSprite { texture, ..*s })
        })
        .collect();
    if super::live::wanted() {
        particles_to_live(&cache, sprites, &used_images);
        return;
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
    deliver(dir.as_deref(), "particles.bin", out);
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
