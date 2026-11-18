//! What a frame draws besides terrain, as plain data: entity meshes and
//! their instances, particle sprites, and sky, cloud, overlay and hand
//! parts. The game builds these; the native renderer uploads them.

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

/// Set in an instance's `kind` when its texture repeats.
pub const KIND_REPEAT: u32 = 1 << 31;
/// Set in an instance's `kind` when only a pack's shadow pass draws it (the
/// local player in first person).
pub const KIND_SHADOW_ONLY: u32 = 1 << 30;

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
    /// drawn over everything), 3 a first-person hand part (its transform
    /// in the hand camera's view space), 4 world lines (block outline,
    /// hitboxes, chunk borders: a line list), 5 the block-breaking cracks
    /// (multiplied over the block).
    pub kind: u32,
    /// Drawing order among the parts, lowest first.
    pub bias: f32,
    /// pad[0]: the sky piece (`SKY_PIECES` index + 1), 0 for none.
    pub pad: [f32; 3],
    /// The texture coordinates' scale (xy) and offset (zw).
    pub uv: [f32; 4],
    pub tint: [f32; 4],
    /// The world transform (clip from local for overlays), columns.
    pub transform: [f32; 16],
}

const _: () = assert!(std::mem::size_of::<EnvPart>() == 128);

/// Sky pieces as `EnvPart::pad[0]` - 1 names them.
pub const SKY_PIECES: [&str; 7] = ["SKY", "VOID", "SUN", "MOON", "STARS", "SUNSET", "END_SKY"];

/// `renderer::sky::SKY_SCALE`: the sky root's scale.
pub const SKY_SCALE: f32 = 16.0;

/// Fills the frame's draw lists; readers run after it.
#[derive(bevy::prelude::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DrawSystems;

/// This frame's live particles, oldest first. A sprite's `texture` indexes
/// `textures`, or is `TEXTURE_ATLAS`.
#[derive(bevy::prelude::Resource, Default)]
pub struct Particles {
    pub sprites: Vec<ParticleSprite>,
    pub textures: Vec<bevy::prelude::Handle<bevy::image::Image>>,
}

/// `EntityInstance::alpha` and `EnvPart::alpha` values.
pub const ALPHA_OPAQUE: u32 = 0;
pub const ALPHA_BLEND: u32 = 2;
pub const ALPHA_ADD: u32 = 3;

/// A mesh the game built. `id` is new for every build, so a renderer can
/// tell a rebuilt mesh from the one it holds.
#[derive(Debug)]
pub struct MeshData {
    pub id: u64,
    pub verts: Vec<EntityVertex>,
    pub indices: Vec<u32>,
    /// Vertices name rig parts (`EntityVertex::part`).
    pub rigged: bool,
}

impl MeshData {
    pub fn new(
        verts: Vec<EntityVertex>,
        indices: Vec<u32>,
        rigged: bool,
    ) -> std::sync::Arc<MeshData> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        std::sync::Arc::new(MeshData {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            verts,
            indices,
            rigged,
        })
    }
}

/// What a part samples.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Texture {
    #[default]
    None,
    /// The terrain atlas.
    Atlas,
    Image(bevy::prelude::Handle<bevy::image::Image>),
}

/// One env part (sky, clouds, weather, ...): `EnvPart` with its mesh and
/// texture by value.
#[derive(Clone, Debug)]
pub struct EnvDraw {
    pub mesh: std::sync::Arc<MeshData>,
    pub texture: Texture,
    /// `ALPHA_*`.
    pub alpha: u32,
    /// As `EnvPart::kind`.
    pub kind: u32,
    pub bias: f32,
    pub pad: [f32; 3],
    pub uv: [f32; 4],
    /// Linear RGBA.
    pub tint: [f32; 4],
    pub transform: bevy::math::Mat4,
}

impl EnvDraw {
    /// A part with no uv transform, untinted and unpadded.
    pub fn new(mesh: std::sync::Arc<MeshData>, kind: u32, transform: bevy::math::Mat4) -> EnvDraw {
        EnvDraw {
            mesh,
            texture: Texture::None,
            alpha: ALPHA_OPAQUE,
            kind,
            bias: 0.0,
            pad: [0.0; 3],
            uv: [1.0, 1.0, 0.0, 0.0],
            tint: [1.0; 4],
            transform,
        }
    }
}

/// This frame's env parts, by producer, each list in drawing order.
#[derive(bevy::prelude::Resource, Default)]
pub struct Env {
    pub sky: Vec<EnvDraw>,
    pub clouds: Vec<EnvDraw>,
    pub weather: Vec<EnvDraw>,
    /// The block outline, cracks, chunk borders and hitboxes.
    pub overlays: Vec<EnvDraw>,
    /// Full-view overlays (underwater, inside a wall, fire, pumpkin), their
    /// transforms in the hand camera's view space.
    pub screen: Vec<EnvDraw>,
}
