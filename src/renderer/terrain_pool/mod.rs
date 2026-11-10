pub mod cull;
pub mod draw;
pub mod hiz;
pub mod pools;

use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, OnceLock};

use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::extract_resource::ExtractResource;
use bevy::shader::Shader;
use bytemuck::{Pod, Zeroable};
use parking_lot::Mutex;

use super::mesh::MeshBuf;

pub const STREAM_SOLID: u32 = 0;
pub const STREAM_CUTOUT: u32 = 1;
pub const STREAM_WATER: u32 = 2;
pub const STREAMS: u32 = 3;

#[cfg(feature = "shader_support")]
pub fn stream(geometry: crate::shaderpack::programs::Geometry) -> u32 {
    use crate::shaderpack::programs::Geometry;
    match geometry {
        Geometry::TerrainSolid => STREAM_SOLID,
        Geometry::TerrainCutout => STREAM_CUTOUT,
        Geometry::Water => STREAM_WATER,
    }
}

pub const SLOTS_INITIAL: u32 = 8192;

pub const ORIGIN_ROW: u32 = 256;

pub const MAX_VIEWS: u32 = 5;

pub const META_LIVE: u32 = 1;
pub const META_WATER: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SlotMeta {
    pub min: [f32; 3],
    pub first_index: u32,
    pub max: [f32; 3],
    pub index_count: u32,
    pub origin: [i32; 3],
    pub base_vertex: u32,
    pub solid_count: u32,
    pub flags: u32,
    pub pool: u32,
    pub cutout_first: u32,
    /// The solid stream's index counts by the way its quads face, in
    /// `FACE_GROUP` order and laid end to end from `first_index`, so the
    /// cull can leave out the groups facing away from the camera.
    pub solid_groups: [u32; 8],
}

/// The solid stream's facing groups: +Y, -Y, -Z, +Z, -X, +X, then quads
/// that face no axis (or are drawn from both sides), which always draw.
pub const FACE_GROUPS: u32 = 7;

impl SlotMeta {
    pub fn stream_range(&self, stream: u32) -> (u32, u32) {
        match stream {
            STREAM_SOLID => (self.first_index, self.solid_count),
            STREAM_CUTOUT => (self.cutout_first, self.index_count - self.solid_count),
            _ => (self.first_index, self.index_count),
        }
    }

    pub fn world_box(&self) -> ([f32; 3], [f32; 3]) {
        let at = |local: [f32; 3]| [0, 1, 2].map(|a| self.origin[a] as f32 + local[a]);
        (at(self.min), at(self.max))
    }

    /// Indices by kind, for a report: solid, the solid the facing test in
    /// `cull.wgsl` keeps for an eye there (all of it with no eye), cutout,
    /// and water.
    pub fn index_split(&self, eye: Option<[f32; 3]>) -> [u64; 4] {
        if self.flags & META_WATER != 0 {
            return [0, 0, 0, u64::from(self.index_count)];
        }
        let solid = u64::from(self.solid_count);
        let facing = match eye {
            None => solid,
            Some(e) => {
                let (lo, hi) = self.world_box();
                let shows = [
                    e[1] > lo[1],
                    e[1] < hi[1],
                    e[2] < hi[2],
                    e[2] > lo[2],
                    e[0] < hi[0],
                    e[0] > lo[0],
                    true,
                ];
                (0..FACE_GROUPS as usize)
                    .filter(|&g| shows[g])
                    .map(|g| u64::from(self.solid_groups[g]))
                    .sum()
            }
        };
        let cutout = u64::from(self.index_count - self.solid_count);
        [solid, facing, cutout, 0]
    }
}

pub enum TerrainOp {
    Upload {
        slot: u32,
        origin: [i32; 3],
        water: bool,
        mesh: MeshBuf,
    },
    Free {
        slot: u32,
    },
    ClearAll,
    Visibility(Arc<[u32]>),
}

#[derive(Resource, Default)]
pub struct TerrainOps(pub Mutex<Vec<TerrainOp>>);

impl TerrainOps {
    pub fn push(&self, op: TerrainOp) {
        self.0.lock().push(op);
    }
}

#[derive(Resource, Default)]
pub struct TerrainOpQueue(pub Vec<TerrainOp>);

#[derive(Resource, Clone, Copy, Debug, ExtractResource)]
pub struct TerrainParams {
    pub cutoff: f32,
    pub mip_bias: f32,
    pub shadow_mip: f32,
    /// Scales the water surface's alpha (the water clarity setting).
    pub water_alpha: f32,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            cutoff: 0.1,
            mip_bias: 0.0,
            shadow_mip: super::terrain::SHADOW_ALPHA_MIP,
            water_alpha: 1.0,
        }
    }
}

#[derive(Resource, Clone, ExtractResource)]
pub struct TerrainTextures {
    pub atlas: Handle<Image>,
    pub lightmap: Handle<Image>,
    /// Which tile each atlas tile shows this tick; see `block_anim`.
    pub anim: Handle<Image>,
}

#[derive(Component, Clone, Copy, Default, ExtractComponent)]
pub struct TerrainView;

#[derive(Resource, Clone, Default)]
pub struct TerrainStats(pub Arc<TerrainStatsInner>);

#[derive(Default)]
pub struct TerrainStatsInner {
    pub live_slots: AtomicUsize,
    pub used_bytes: AtomicU64,
    /// The vertex and index buffers allocated, against `used_bytes`.
    pub capacity_bytes: AtomicU64,
    /// The shader pack's per-pool quad textures, apart from the buffers.
    pub pack_bytes: AtomicU64,
    pub pools: AtomicUsize,
    pub drawn: AtomicUsize,
    /// Triangles in the sections the camera draws, counted where `drawn` is.
    pub triangles: AtomicU64,
    /// `triangles` by kind (solid, solid after the facing test, cutout,
    /// water), while a report records.
    pub triangle_split: [AtomicU64; 4],
    pub draws: AtomicUsize,
    draws_pending: AtomicUsize,
    /// 1 while the GPU culls, so `drawn` is only known when counted.
    pub indirect: AtomicUsize,
    /// Section bytes written to the GPU last frame (vertices, indices and
    /// water re-sorts), each a staging copy the queue submit carries.
    pub uploaded: AtomicU64,
    pub(crate) uploaded_pending: AtomicU64,
    /// Asks the GPU-culled path to count `drawn` on the CPU too, and to
    /// read back the Hi-Z counters.
    pub count_sections: std::sync::atomic::AtomicBool,
    /// Sections in the camera's frustum the GPU tested against the
    /// occlusion pyramid, and those it hid, a frame or two ago (while a
    /// report records).
    pub hiz_tested: AtomicUsize,
    pub hiz_culled: AtomicUsize,
}

static STATS: OnceLock<TerrainStats> = OnceLock::new();

pub fn stats() -> Option<&'static TerrainStats> {
    STATS.get()
}

#[derive(Resource, Clone)]
pub struct TerrainShaders {
    pub terrain: Handle<Shader>,
    pub cull: Handle<Shader>,
}

fn embedded_shader(assets: &AssetServer, file: &str) -> Handle<Shader> {
    assets.load(super::embedded_path(&format!("renderer/{file}")))
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct TerrainTier {
    pub indirect: bool,
    pub count: bool,
    pub base_vertex: bool,
}

pub struct TerrainPoolPlugin;

impl Plugin for TerrainPoolPlugin {
    fn build(&self, app: &mut App) {
        let stats = TerrainStats::default();
        let _ = STATS.set(stats.clone());
        app.init_resource::<TerrainOps>()
            .init_resource::<TerrainParams>()
            .insert_resource(stats.clone())
            .add_plugins((
                bevy::render::extract_resource::ExtractResourcePlugin::<TerrainParams>::default(),
                bevy::render::extract_resource::ExtractResourcePlugin::<TerrainTextures>::default(),
                bevy::render::extract_component::ExtractComponentPlugin::<TerrainView>::default(),
            ));
        pools::build(app, stats);
        cull::build(app);
        draw::build(app);
    }

    fn finish(&self, app: &mut App) {
        let shaders = {
            let assets = app.world().resource::<AssetServer>();
            TerrainShaders {
                terrain: embedded_shader(assets, "terrain.wgsl"),
                cull: embedded_shader(assets, "terrain_pool/cull.wgsl"),
            }
        };
        let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
            return;
        };
        render_app.insert_resource(shaders);
        let tier = decide_tier(render_app.world());
        crate::log_info!(
            "render",
            "terrain tier: indirect={} count={} base_vertex={}",
            tier.indirect,
            tier.count,
            tier.base_vertex
        );
        render_app.insert_resource(tier);
        app.insert_resource(tier);
        pools::finish(app);
        cull::finish(app);
        draw::finish(app);
    }
}

fn decide_tier(world: &World) -> TerrainTier {
    use bevy::render::renderer::RenderDevice;
    let device = world.resource::<RenderDevice>();
    let features = device.features();
    let forced_direct = matches!(
        crate::platform::env::var("MC_TERRAIN_DIRECT").as_deref(),
        Ok("1") | Ok("true")
    );
    let storage_ok = device.limits().max_storage_buffers_per_shader_stage >= 4;
    let indirect_ok = world
        .resource::<bevy::render::renderer::RenderAdapter>()
        .get_downlevel_capabilities()
        .flags
        .contains(bevy::render::render_resource::DownlevelFlags::INDIRECT_EXECUTION);
    let indirect = !forced_direct
        && storage_ok
        && indirect_ok
        && features.contains(bevy::render::render_resource::WgpuFeatures::INDIRECT_FIRST_INSTANCE);
    let count_ok =
        features.contains(bevy::render::render_resource::WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT);
    let indirect =
        indirect && (count_ok || !(cfg!(target_os = "android") || cfg!(target_arch = "wasm32")));
    let count = indirect && count_ok;
    let base_vertex = world
        .resource::<bevy::render::renderer::RenderAdapter>()
        .get_downlevel_capabilities()
        .flags
        .contains(bevy::render::render_resource::DownlevelFlags::BASE_VERTEX);
    TerrainTier {
        indirect,
        count,
        base_vertex,
    }
}
