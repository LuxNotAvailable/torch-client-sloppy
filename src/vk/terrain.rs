//! Terrain on the native renderer (VULKAN_REWRITE_CHECKLIST.md phase 3).
//!
//! Sections stream in and out of shared pools (3.1, 3.6): each pool is a
//! vertex buffer and an index buffer with offset allocators, and every
//! section layer has a 128-byte slot. Changes queue up and go through the
//! staging ring under a byte budget per frame. The vertex shader pulls the
//! mesher's 20-byte vertices from the slot's pool by bindless index.
//!
//! One compute pass culls every slot (3.2): cave visibility (the game's
//! flood fill, as bits per slot), the frustum, and last frame's depth
//! pyramid (3.4). Survivors become indirect commands: per pool, a solid
//! list holding only the face buckets that can face the camera (3.3) and a
//! cutout list, each drawn with one `vkCmdDrawIndexedIndirectCount`.
//!
//! The translucent layer (water, glass, ice, slime, honey, the portal; 3.5,
//! 3.7) has its own index buffer, re-sorted back to front per section on
//! the CPU as the eye moves, and its sections ranked far to near: each
//! writes its command at its rank, so one indirect draw blends them in
//! order across sections.
//!
//! For now the sections come from a terrain dump the game wrote
//! (`terrain_dump`), replayed as the stream of loads, re-meshes and unloads
//! it was.

use std::collections::{HashMap, VecDeque};

use ash::vk;
use bevy::math::{IVec3, Mat4, Vec3};
use gpu_allocator::MemoryLocation;
use offset_allocator::{Allocation, Allocator};

use super::bindless::Bindless;
use super::depth_view::{DEPTH_FORMAT, DepthTarget, DepthView, set_viewport};
use super::frame::FRAMES_IN_FLIGHT;
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsSpec, Pipelines, Push, Spec, push};
use super::terrain_dump::{Record, Replay, Section};
use super::test_scene::View;
use super::upload::Uploads;
use super::{Context, SetupError};
use crate::renderer::mesh::PackedVertex;
use crate::renderer::visgraph::VisibilitySet;

const NEAR: f32 = 0.05;

// The game's halved pools (clean-up 12d).
#[cfg(not(target_os = "android"))]
const POOL_VERTICES: u32 = 1_600_000;
#[cfg(target_os = "android")]
const POOL_VERTICES: u32 = 1_200_000;
#[cfg(not(target_os = "android"))]
const POOL_INDICES: u32 = 3_200_000;
#[cfg(target_os = "android")]
const POOL_INDICES: u32 = 2_400_000;
const MAX_POOLS: u32 = 64;

const SLOT_SIZE: u64 = 128;
const COMMAND_SIZE: u64 = 20;
/// The most commands a slot's solid buckets can take: seven buckets with
/// hidden ones between visible ones split into at most four runs (at most
/// one of each axis's pair is hidden, and faces on no axis always draw).
const MAX_RUNS: u32 = 4;
const TRANSLUCENT_INITIAL: u32 = 1 << 20;

/// Sections nearer than this re-sort their translucent quads whenever the
/// eye enters a new block; farther ones when it enters a new section (as
/// the game's water sort).
const SORT_NEAR: f32 = 48.0;
/// Quads re-sorted per frame at most; the rest wait, nearest first.
const SORT_BUDGET: usize = 65_536;
/// Upload bytes per frame for section changes.
pub const UPLOAD_BUDGET: u64 = 4 << 20;

const SLOT_LIVE: u32 = 1;
const SLOT_TRANSLUCENT: u32 = 2;

const CULL_HIZ: u32 = 1;
const CULL_VISIBILITY: u32 = 2;
const CULL_BUCKETS: u32 = 4;
const CULL_FRUSTUM: u32 = 8;
const CULL_NO_WAVE: u32 = 16;

/// The camera moved farther than this (blocks) since the pyramid was
/// built: skip Hi-Z for a frame, as newly uncovered sections would pop in
/// a frame late (the game's camera-motion guard).
const HIZ_GUARD: f32 = 4.0;

/// One section layer's slot, matching `Slot` in `shaders/terrain.slang`.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Slot {
    /// xyz the box's low corner from the origin.
    lo: [f32; 4],
    /// xyz the box's high corner.
    hi: [f32; 4],
    /// xyz the origin in blocks, w the base vertex.
    origin_base: [i32; 4],
    /// The pool, the vertex buffer's bindless index, flags.
    info: [u32; 4],
    /// The first index, the cutout (or translucent) index count.
    ranges: [u32; 4],
    /// Index counts of the +x, -x, +y, -y, +z, -z and no-axis buckets.
    buckets: [u32; 8],
    spare: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<Slot>() == SLOT_SIZE as usize);

/// The frame's cull inputs, matching the `VIEW_*` offsets in
/// `shaders/terrain.slang`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CullView {
    view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    /// The pyramid's bindless index, its top level, the depth's size.
    hiz: [u32; 4],
    flags: [u32; 4],
    pad: [u32; 4],
    /// The fog's colour and its ranges (environmental start and end,
    /// render-distance start and end), and the lightmap's bindless index
    /// (or `u32::MAX`) with fog on or off (`shaders/world.slang`).
    fog_color: [f32; 4],
    fog_ranges: [f32; 4],
    env: [u32; 4],
}

/// Where `vkview --terrain` looks from: a position and a yaw and pitch in
/// degrees, as the game's camera (yaw 0 faces +z).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Vertical field of view, degrees.
    pub fov: f32,
}

impl Camera {
    /// Parses `x,y,z,yaw,pitch`.
    pub fn parse(text: &str) -> Option<Camera> {
        let v: Vec<f32> = text
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect::<Option<_>>()?;
        let [x, y, z, yaw, pitch] = v[..] else {
            return None;
        };
        Some(Camera {
            eye: Vec3::new(x, y, z),
            yaw,
            pitch,
            fov: 70.0,
        })
    }

    fn forward(self) -> Vec3 {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        Vec3::new(
            -yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        )
    }
}

/// Which culling the terrain does, for comparing with each off.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub hiz: bool,
    pub visibility: bool,
    pub buckets: bool,
    /// Reserve commands with one atomic per subgroup (off: per lane).
    pub wave: bool,
    /// Bytes of section changes uploaded per frame.
    pub budget: u64,
    /// The pyramid level `--view hiz` shows.
    pub hiz_level: u32,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            hiz: true,
            visibility: true,
            buckets: true,
            wave: true,
            budget: UPLOAD_BUDGET,
            hiz_level: 3,
        }
    }
}

/// What the terrain holds and did last frame, for the viewer's summary.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub sections: usize,
    pub pools: usize,
    pub vertices: u64,
    pub indices: u64,
    pub uploaded: u64,
    pub records: u64,
    /// Last finished frame's commands: solid, cutout, translucent drawn.
    pub commands: [u32; 3],
    /// Last finished frame's vertex shader invocations and primitives
    /// clipped, when the device counts them.
    pub vertex_invocations: Option<u64>,
    pub primitives: Option<u64>,
    /// Slots the flood fill reached.
    pub reached: usize,
    pub hiz_frames: u64,
    /// The worker's milliseconds for the pipelines, and the frame they were
    /// ready on.
    pub pipelines: Option<(f32, u64)>,
    /// Times the translucent index buffer and the slot buffer grew.
    pub grown: [u32; 2],
}

struct Pool {
    vertices: Buffer,
    indices: Buffer,
    vertex_index: u32,
    valloc: Allocator,
    ialloc: Allocator,
    slots: u32,
}

/// Where a section layer's indices live.
#[derive(Clone, Copy)]
enum Indices {
    Pool(Allocation),
    /// In the translucent buffer, allocated in `generation`.
    Translucent(Allocation, u32),
}

struct Entry {
    slot: u32,
    pool: u32,
    vertices: Allocation,
    indices: Indices,
    vertex_count: u32,
    index_count: u32,
    sort: Option<Sort>,
}

/// A translucent layer's quads, for re-sorting back to front.
struct Sort {
    origin: [i32; 3],
    first: u32,
    /// Quad centres relative to `origin`.
    centres: Vec<Vec3>,
    /// The layer's indices as meshed, six per quad.
    indices: Vec<u32>,
    sorted_for: Option<IVec3>,
}

impl Sort {
    fn new(section: &Section, first: u32) -> Sort {
        let indices: Vec<u32> = section
            .idx
            .iter()
            .chain(&section.cutout_idx)
            .copied()
            .collect();
        // A quad's six indices name its four vertices in any order (fluid
        // quads start at their third), so its first vertex is the lowest.
        let centres = indices
            .chunks_exact(6)
            .map(|quad| {
                let first = quad.iter().copied().min().unwrap_or(0) as usize;
                let corners = section.verts.get(first..first + 4).unwrap_or(&[]);
                let sum = corners.iter().fold(Vec3::ZERO, |acc, v| {
                    acc + Vec3::new(v.pos[0] as f32, v.pos[1] as f32, v.pos[2] as f32)
                });
                sum / (corners.len().max(1) as f32 * 256.0)
            })
            .collect();
        Sort {
            origin: section.origin,
            first,
            centres,
            indices,
            sorted_for: None,
        }
    }

    fn eye_from(&self, eye: Vec3) -> Vec3 {
        eye - Vec3::from(self.origin.map(|c| c as f32))
    }

    /// The indices with quads farthest from `eye` first.
    fn back_to_front(&self, eye: Vec3) -> Vec<u32> {
        let mut order: Vec<(f32, u32)> = self
            .centres
            .iter()
            .enumerate()
            .map(|(i, c)| (c.distance_squared(eye), i as u32))
            .collect();
        order.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        let mut out = Vec::with_capacity(self.indices.len());
        for (_, quad) in order {
            let at = quad as usize * 6;
            out.extend_from_slice(&self.indices[at..at + 6]);
        }
        out.extend_from_slice(&self.indices[self.centres.len() * 6..]);
        out
    }
}

/// Sorts a section's solid indices into the seven face buckets by each
/// quad's normal: +x, -x, +y, -y, +z, -z, then faces on no axis.
fn bucket(verts: &[PackedVertex], idx: &[u32]) -> ([u32; 7], Vec<u32>) {
    let mut lists: [Vec<u32>; 7] = Default::default();
    if !idx.len().is_multiple_of(6) {
        lists[6].extend_from_slice(idx);
    } else {
        for quad in idx.chunks_exact(6) {
            let at = |i: usize| {
                verts.get(quad[i] as usize).map_or([0i64; 3], |v| {
                    [v.pos[0] as i64, v.pos[1] as i64, v.pos[2] as i64]
                })
            };
            let (a, b, c) = (at(0), at(1), at(2));
            let (u, v) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let which = match n {
                [x, 0, 0] if x != 0 => usize::from(x < 0),
                [0, y, 0] if y != 0 => 2 + usize::from(y < 0),
                [0, 0, z] if z != 0 => 4 + usize::from(z < 0),
                _ => 6,
            };
            lists[which].extend_from_slice(quad);
        }
    }
    let counts = std::array::from_fn(|i| lists[i].len() as u32);
    (counts, lists.concat())
}

/// The terrain's pipelines, in the order `TerrainScene::new` asks for them.
#[derive(Clone, Copy)]
struct Pipes {
    cull: vk::Pipeline,
    hiz_reduce: vk::Pipeline,
    solid: vk::Pipeline,
    cutout: vk::Pipeline,
    translucent: vk::Pipeline,
}

impl Pipes {
    fn from(built: &[vk::Pipeline]) -> Pipes {
        Pipes {
            cull: built[0],
            hiz_reduce: built[1],
            solid: built[2],
            cutout: built[3],
            translucent: built[4],
        }
    }

    fn all(self) -> [vk::Pipeline; 5] {
        [
            self.cull,
            self.hiz_reduce,
            self.solid,
            self.cutout,
            self.translucent,
        ]
    }
}

/// A buffer the terrain replaces when it outgrows it, with its bindless
/// index.
struct Grown {
    buffer: Buffer,
    index: u32,
}

/// A copy from the staging ring.
struct Copy {
    src: vk::Buffer,
    from: u64,
    to: vk::Buffer,
    at: u64,
    size: u64,
}

/// A grown buffer's old contents, copied into the new one.
struct GrowCopy {
    from: vk::Buffer,
    to: vk::Buffer,
    size: u64,
}

pub struct TerrainScene {
    options: Options,
    module: vk::ShaderModule,
    /// The pipelines, once the worker has built them; until then frames
    /// only stream sections in and clear.
    pipes: Option<Pipes>,
    pending: Option<super::pipeline::Pending>,
    depth_view: DepthView,

    pools: Vec<Pool>,
    translucent: Grown,
    translucent_alloc: Allocator,
    translucent_capacity: u32,
    translucent_generation: u32,
    /// The block at the start of the grown translucent buffer holding the
    /// layers allocated before it grew, and how many still live there.
    translucent_legacy: Option<(Allocation, u32)>,

    slots: Grown,
    slot_capacity: u32,
    slot_cpu: Vec<Slot>,
    slot_high: u32,
    free_slots: Vec<u32>,
    dirty_slots: Vec<u32>,

    entries: HashMap<([i32; 3], bool), Entry>,
    vis: HashMap<(i32, i32, i32), VisibilitySet>,
    queue: VecDeque<Record>,
    replay: Option<Replay>,

    visible: Grown,
    visible_bits: Vec<u32>,
    visible_dirty: bool,
    fill_for: Option<IVec3>,
    sections_changed: bool,
    frames_since_fill: u32,

    ranks: Grown,
    rank_of: Vec<u32>,
    ranks_dirty: bool,
    ranked_for: Option<IVec3>,
    translucent_count: u32,

    commands: Buffer,
    commands_index: u32,
    list_capacity: u32,
    translucent_start: u32,
    counts: Buffer,
    counts_index: u32,
    readback: Vec<Buffer>,
    views: Vec<Buffer>,
    view_indices: Vec<u32>,

    /// The depth pyramid, the depth size it is for, and its level count.
    pyramid: Option<(Grown, (u32, u32), u32)>,
    /// The camera the pyramid's depth was drawn with.
    pyramid_view: Option<(Mat4, Vec3)>,

    stats_pools: Vec<vk::QueryPool>,
    stats_issued: Vec<bool>,

    atlas: Image,
    atlas_index: u32,
    atlas_staging: Option<(Buffer, Option<u64>)>,
    atlas_size: (u32, u32),
    depth: DepthTarget,
    retired: Vec<(Buffer, u64)>,

    lo: Vec3,
    hi: Vec3,
    pub stats: Stats,
    /// The entity snapshot beside the terrain dump, when there is one
    /// (phase 4).
    entities: Option<super::entities::EntityScene>,
    env: Option<super::env::EnvScene>,
}

fn full() -> SetupError {
    SetupError("the bindless set is full".to_owned())
}

impl TerrainScene {
    /// Opens the dump in `dir` (`sections.bin` and `atlas.png`); its
    /// records stream in over the following frames.
    pub fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        color_format: vk::Format,
        dir: &std::path::Path,
        options: Options,
    ) -> Result<TerrainScene, SetupError> {
        let failed = |what: &str, e: &dyn std::fmt::Display| SetupError(format!("{what}: {e}"));
        let bytes = std::fs::read(dir.join("sections.bin"))
            .map_err(|e| failed("can't read sections.bin", &e))?;
        let replay = Replay::new(bytes).map_err(|e| failed("bad sections.bin", &e))?;
        let atlas = image::open(dir.join("atlas.png"))
            .map_err(|e| failed("can't read atlas.png", &e))?
            .into_rgba8();
        let atlas_size = atlas.dimensions();

        let module = pipelines.module(ctx, super::shaders::TERRAIN)?;
        // Built on a worker: the dump starts streaming in meanwhile (2.5).
        let stream = |fragment: &std::ffi::CStr, cull, depth_write, blend| {
            Spec::Graphics(GraphicsSpec {
                module,
                vertex: c"terrain_vertex".to_owned(),
                fragment: fragment.to_owned(),
                color: vec![color_format],
                depth: Some(DEPTH_FORMAT),
                depth_test: Some(vk::CompareOp::GREATER),
                depth_write,
                cull,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend,
                additive: false,
            })
        };
        let pending = pipelines.build_on_worker(
            ctx,
            vec![
                Spec::Compute {
                    module,
                    entry: c"cull".to_owned(),
                },
                Spec::Compute {
                    module,
                    entry: c"hiz_reduce".to_owned(),
                },
                // Solid faces are one-sided, as vanilla's; cutout (plants,
                // leaves) and translucent faces draw both sides.
                stream(c"terrain_solid", vk::CullModeFlags::BACK, true, false),
                stream(c"terrain_cutout", vk::CullModeFlags::NONE, true, false),
                stream(c"terrain_translucent", vk::CullModeFlags::NONE, false, true),
            ],
        );
        let depth_view = DepthView::new(ctx, pipelines, color_format)?;

        let storage = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST;
        let grown = |bindless: &mut Bindless, name, size, usage| -> Result<Grown, SetupError> {
            let buffer = memory.buffer(ctx, name, size, usage, MemoryLocation::GpuOnly)?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            Ok(Grown { buffer, index })
        };
        let slot_capacity = 4096;
        let translucent = grown(
            bindless,
            "terrain translucent indices",
            TRANSLUCENT_INITIAL as u64 * 4,
            vk::BufferUsageFlags::INDEX_BUFFER
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC,
        )?;
        let slots = grown(
            bindless,
            "terrain slots",
            slot_capacity as u64 * SLOT_SIZE,
            storage | vk::BufferUsageFlags::TRANSFER_SRC,
        )?;
        let visible = grown(
            bindless,
            "terrain visible slots",
            slot_capacity as u64 / 8,
            storage,
        )?;
        let ranks = grown(
            bindless,
            "terrain translucent ranks",
            slot_capacity as u64 * 4,
            storage,
        )?;
        let indirect = storage | vk::BufferUsageFlags::INDIRECT_BUFFER;
        let commands = memory.buffer(
            ctx,
            "terrain commands",
            COMMAND_SIZE * 1024,
            indirect,
            MemoryLocation::GpuOnly,
        )?;
        let commands_index = bindless.add_buffer(ctx, commands.raw).ok_or_else(full)?;
        let counts = memory.buffer(
            ctx,
            "terrain counts",
            MAX_POOLS as u64 * 2 * 4,
            indirect | vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::GpuOnly,
        )?;
        let counts_index = bindless.add_buffer(ctx, counts.raw).ok_or_else(full)?;
        let mut readback = Vec::new();
        let mut views = Vec::new();
        let mut view_indices = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT {
            readback.push(memory.buffer(
                ctx,
                "terrain counts readback",
                MAX_POOLS as u64 * 2 * 4,
                vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuToCpu,
            )?);
            let view = memory.buffer(
                ctx,
                "terrain cull view",
                std::mem::size_of::<CullView>() as u64,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            view_indices.push(bindless.add_buffer(ctx, view.raw).ok_or_else(full)?);
            views.push(view);
        }

        let mut stats_pools = Vec::new();
        if ctx.info.pipeline_statistics {
            for _ in 0..FRAMES_IN_FLIGHT {
                // SAFETY: creation and a host reset on a live device with
                // pipeline statistics and host query reset on.
                let pool = unsafe {
                    let pool = ctx.device.create_query_pool(
                        &vk::QueryPoolCreateInfo::default()
                            .query_type(vk::QueryType::PIPELINE_STATISTICS)
                            .query_count(1)
                            .pipeline_statistics(
                                vk::QueryPipelineStatisticFlags::VERTEX_SHADER_INVOCATIONS
                                    | vk::QueryPipelineStatisticFlags::CLIPPING_INVOCATIONS,
                            ),
                        None,
                    )?;
                    ctx.device.reset_query_pool(pool, 0, 1);
                    pool
                };
                stats_pools.push(pool);
            }
        }

        let atlas_bytes = atlas.as_raw();
        let mut atlas_staging = memory.buffer(
            ctx,
            "atlas staging",
            atlas_bytes.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        atlas_staging.write(0, atlas_bytes);
        let atlas_image = memory.image(
            ctx,
            "block atlas",
            vk::Extent2D {
                width: atlas_size.0,
                height: atlas_size.1,
            },
            vk::Format::R8G8B8A8_SRGB,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let atlas_index = bindless
            .add_texture(
                ctx,
                atlas_image.view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            )
            .ok_or_else(full)?;

        // The entity snapshot, when the game wrote one beside the terrain.
        let entities = match std::fs::read(dir.join("entities.bin")) {
            Ok(bytes) => {
                let snapshot =
                    super::entity_dump::read(&bytes).map_err(|e| failed("bad entities.bin", &e))?;
                Some(super::entities::EntityScene::new(
                    ctx,
                    memory,
                    bindless,
                    pipelines,
                    color_format,
                    snapshot,
                )?)
            }
            Err(_) => None,
        };
        // And what surrounds the world: sky, clouds, fog, lightmap.
        let env = match std::fs::read(dir.join("env.bin")) {
            Ok(bytes) => {
                let snapshot =
                    super::entity_dump::read_env(&bytes).map_err(|e| failed("bad env.bin", &e))?;
                Some(super::env::EnvScene::new(
                    ctx,
                    memory,
                    bindless,
                    pipelines,
                    color_format,
                    snapshot,
                    entities
                        .as_ref()
                        .map_or(Vec3::ZERO, |e| e.camera.0.w_axis.truncate()),
                )?)
            }
            Err(_) => None,
        };

        Ok(TerrainScene {
            options,
            module,
            pipes: None,
            pending: Some(pending),
            depth_view,
            pools: Vec::new(),
            translucent,
            translucent_alloc: Allocator::new(TRANSLUCENT_INITIAL),
            translucent_capacity: TRANSLUCENT_INITIAL,
            translucent_generation: 0,
            translucent_legacy: None,
            slots,
            slot_capacity,
            slot_cpu: vec![Slot::default(); slot_capacity as usize],
            slot_high: 0,
            free_slots: Vec::new(),
            dirty_slots: Vec::new(),
            entries: HashMap::new(),
            vis: HashMap::new(),
            queue: VecDeque::new(),
            replay: Some(replay),
            visible,
            visible_bits: vec![0; slot_capacity as usize / 32],
            visible_dirty: true,
            fill_for: None,
            sections_changed: false,
            frames_since_fill: 0,
            ranks,
            rank_of: vec![0; slot_capacity as usize],
            ranks_dirty: true,
            ranked_for: None,
            translucent_count: 0,
            commands,
            commands_index,
            list_capacity: 0,
            translucent_start: 0,
            counts,
            counts_index,
            readback,
            views,
            view_indices,
            pyramid: None,
            pyramid_view: None,
            stats_issued: vec![false; stats_pools.len()],
            stats_pools,
            atlas: atlas_image,
            atlas_index,
            atlas_staging: Some((atlas_staging, None)),
            atlas_size,
            depth: DepthTarget::default(),
            retired: Vec::new(),
            lo: Vec3::splat(f32::MAX),
            hi: Vec3::splat(f32::MIN),
            stats: Stats::default(),
            entities,
            env,
        })
    }

    /// Whether the dump has fully streamed in.
    pub fn loaded(&self) -> bool {
        self.replay.is_none() && self.queue.is_empty()
    }

    /// How far the replay has got, from 0 to 1.
    pub fn progress(&self) -> f32 {
        self.replay.as_ref().map_or(1.0, Replay::progress)
    }

    /// The camera when none is given: above the middle of the loaded
    /// terrain, looking down at it, circling and turning slowly.
    pub fn default_camera(&self, seconds: f32) -> Camera {
        let centre = if self.lo.x <= self.hi.x {
            Vec3::new(
                (self.lo.x + self.hi.x) / 2.0,
                self.hi.y,
                (self.lo.z + self.hi.z) / 2.0,
            )
        } else {
            Vec3::new(0.0, 100.0, 0.0)
        };
        // Circling as it turns, so culling sees both moves.
        let angle = (seconds * 10.0).to_radians();
        Camera {
            eye: centre + Vec3::new(angle.sin() * 24.0, 40.0, angle.cos() * 24.0),
            yaw: seconds * 10.0,
            pitch: -30.0,
            fov: 70.0,
        }
    }

    /// The camera the game drew from when it wrote the entity snapshot.
    pub fn game_camera(&self) -> Option<Camera> {
        let (world, fov) = self.entities.as_ref()?.camera;
        let eye = world.w_axis.truncate();
        // The camera looks down its -z.
        let forward = -world.z_axis.truncate().normalize_or_zero();
        Some(Camera {
            eye,
            yaw: (-forward.x).atan2(forward.z).to_degrees(),
            pitch: forward.y.clamp(-1.0, 1.0).asin().to_degrees(),
            fov: fov.to_degrees(),
        })
    }

    /// The entity scene's numbers, when the dump has one.
    pub fn entity_stats(&self) -> Option<super::entities::EntityStats> {
        self.entities.as_ref().map(|e| e.stats)
    }

    fn new_slot(&mut self) -> u32 {
        if let Some(slot) = self.free_slots.pop() {
            return slot;
        }
        self.slot_high += 1;
        self.slot_high - 1
    }

    fn release_slot(&mut self, slot: u32) {
        self.slot_cpu[slot as usize] = Slot::default();
        self.dirty_slots.push(slot);
        self.free_slots.push(slot);
        self.set_visible(slot, false);
    }

    fn set_visible(&mut self, slot: u32, on: bool) {
        let (word, bit) = (slot as usize / 32, slot % 32);
        if let Some(w) = self.visible_bits.get_mut(word) {
            if on {
                *w |= 1 << bit;
            } else {
                *w &= !(1 << bit);
            }
            self.visible_dirty = true;
        }
    }

    /// Removes a section layer, freeing its ranges and slot.
    fn remove(&mut self, key: ([i32; 3], bool)) {
        let Some(entry) = self.entries.remove(&key) else {
            return;
        };
        match entry.indices {
            Indices::Pool(a) => self.pools[entry.pool as usize].ialloc.free(a),
            Indices::Translucent(a, generation) if generation == self.translucent_generation => {
                self.translucent_alloc.free(a);
            }
            Indices::Translucent(..) => {
                if let Some((block, live)) = &mut self.translucent_legacy {
                    *live -= 1;
                    if *live == 0 {
                        self.translucent_alloc.free(*block);
                        self.translucent_legacy = None;
                    }
                }
            }
        }
        if entry.sort.is_some() {
            self.translucent_count -= 1;
            self.ranks_dirty = true;
        }
        let pool = &mut self.pools[entry.pool as usize];
        pool.valloc.free(entry.vertices);
        pool.slots -= 1;
        self.stats.vertices -= entry.vertex_count as u64;
        self.stats.indices -= entry.index_count as u64;
        self.release_slot(entry.slot);
        self.sections_changed = true;
    }

    /// A new pool, when every pool is too full for a section.
    fn add_pool(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
    ) -> Result<u32, SetupError> {
        if self.pools.len() as u32 >= MAX_POOLS {
            return Err(SetupError("the terrain pools are full".to_owned()));
        }
        let vertices = memory.buffer(
            ctx,
            "terrain vertices",
            POOL_VERTICES as u64 * 20,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
        )?;
        let indices = memory.buffer(
            ctx,
            "terrain indices",
            POOL_INDICES as u64 * 4,
            vk::BufferUsageFlags::INDEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
        )?;
        let vertex_index = bindless.add_buffer(ctx, vertices.raw).ok_or_else(full)?;
        self.pools.push(Pool {
            vertices,
            indices,
            vertex_index,
            valloc: Allocator::new(POOL_VERTICES),
            ialloc: Allocator::new(POOL_INDICES),
            slots: 0,
        });
        Ok(self.pools.len() as u32 - 1)
    }

    /// Takes queued changes until the byte budget or the staging ring runs
    /// out, adding their copies to `copies`.
    #[allow(clippy::too_many_arguments, reason = "one frame's upload context")]
    fn apply_changes(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        uploads: &mut Uploads,
        copies: &mut Vec<Copy>,
        grow: &mut Vec<GrowCopy>,
        frame: u64,
        eye: Vec3,
    ) -> Result<(), SetupError> {
        let mut spent = 0u64;
        while spent < self.options.budget {
            if self.queue.len() < 64
                && let Some(replay) = &mut self.replay
            {
                match replay.next() {
                    Some(record) => self.queue.push_back(record),
                    None => self.replay = None,
                }
            }
            let Some(record) = self.queue.pop_front() else {
                break;
            };
            self.stats.records += 1;
            let section = match record {
                Record::Dropped { cx, cz } => {
                    let keys: Vec<_> = self
                        .entries
                        .keys()
                        .filter(|(o, _)| o[0] >> 4 == cx && o[2] >> 4 == cz)
                        .copied()
                        .collect();
                    for key in keys {
                        self.remove(key);
                    }
                    self.vis.retain(|&(x, _, z), _| x != cx || z != cz);
                    continue;
                }
                Record::Layer(section) => section,
            };
            let key = (section.origin, section.water);
            if !section.water {
                self.vis.insert(
                    (
                        section.origin[0] >> 4,
                        section.origin[1],
                        section.origin[2] >> 4,
                    ),
                    section.vis,
                );
            }
            self.remove(key);
            if section.verts.is_empty() {
                continue;
            }

            let (counts, indices) = if section.water {
                ([0; 7], Vec::new())
            } else {
                let (counts, mut solid) = bucket(&section.verts, &section.idx);
                solid.extend_from_slice(&section.cutout_idx);
                (counts, solid)
            };
            let index_count = (section.idx.len() + section.cutout_idx.len()) as u32;
            let vertex_count = section.verts.len() as u32;

            // Room for the vertices (and opaque indices) in some pool.
            let mut placed = None;
            for (p, pool) in self.pools.iter_mut().enumerate() {
                let Some(v) = pool.valloc.allocate(vertex_count) else {
                    continue;
                };
                if section.water {
                    placed = Some((p as u32, v, None));
                    break;
                }
                match pool.ialloc.allocate(index_count) {
                    Some(i) => {
                        placed = Some((p as u32, v, Some(i)));
                        break;
                    }
                    None => pool.valloc.free(v),
                }
            }
            let (pool, vertices, pool_indices) = match placed {
                Some(p) => p,
                None => {
                    let p = self.add_pool(ctx, memory, bindless)?;
                    let pool = &mut self.pools[p as usize];
                    let v = pool.valloc.allocate(vertex_count).ok_or_else(|| {
                        SetupError(format!("a section of {vertex_count} vertices fits no pool"))
                    })?;
                    let i = if section.water {
                        None
                    } else {
                        Some(pool.ialloc.allocate(index_count).ok_or_else(|| {
                            SetupError(format!("a section of {index_count} indices fits no pool"))
                        })?)
                    };
                    (p, v, i)
                }
            };
            let index_alloc = match pool_indices {
                Some(i) => Indices::Pool(i),
                None => {
                    let a =
                        self.translucent_range(ctx, memory, bindless, index_count, grow, frame)?;
                    Indices::Translucent(a, self.translucent_generation)
                }
            };
            let first = match index_alloc {
                Indices::Pool(a) | Indices::Translucent(a, _) => a.offset,
            };

            // A translucent layer goes up already sorted for the eye.
            let mut sort = section.water.then(|| Sort::new(&section, first));
            let sorted = sort.as_mut().map(|sort| {
                sort.sorted_for = Some(eye.floor().as_ivec3());
                sort.back_to_front(sort.eye_from(eye))
            });
            let index_bytes: &[u8] = match &sorted {
                Some(sorted) => bytemuck::cast_slice(sorted),
                None => bytemuck::cast_slice(&indices),
            };
            let staged_v = uploads.stage(bytemuck::cast_slice(&section.verts));
            let staged_i = staged_v.as_ref().and_then(|_| uploads.stage(index_bytes));
            let (Some(staged_v), Some(staged_i)) = (staged_v, staged_i) else {
                // The ring is full this frame: undo and try again next one.
                let pool_ref = &mut self.pools[pool as usize];
                pool_ref.valloc.free(vertices);
                match index_alloc {
                    Indices::Pool(a) => pool_ref.ialloc.free(a),
                    Indices::Translucent(a, _) => self.translucent_alloc.free(a),
                }
                self.queue.push_front(Record::Layer(section));
                self.stats.records -= 1;
                break;
            };
            let pool_ref = &self.pools[pool as usize];
            copies.push(Copy {
                src: staged_v.buffer,
                from: staged_v.offset,
                to: pool_ref.vertices.raw,
                at: vertices.offset as u64 * 20,
                size: staged_v.size,
            });
            copies.push(Copy {
                src: staged_i.buffer,
                from: staged_i.offset,
                to: match index_alloc {
                    Indices::Pool(_) => pool_ref.indices.raw,
                    Indices::Translucent(..) => self.translucent.buffer.raw,
                },
                at: first as u64 * 4,
                size: staged_i.size,
            });
            spent += staged_v.size + staged_i.size;
            self.stats.uploaded += staged_v.size + staged_i.size;

            let (mut min, mut max) = ([i16::MAX; 3], [i16::MIN; 3]);
            for v in &section.verts {
                for axis in 0..3 {
                    min[axis] = min[axis].min(v.pos[axis]);
                    max[axis] = max[axis].max(v.pos[axis]);
                }
            }
            let to_blocks = |v: [i16; 3]| v.map(|c| c as f32 / 256.0);
            let (min, max) = (to_blocks(min), to_blocks(max));
            let origin = Vec3::from(section.origin.map(|c| c as f32));
            self.lo = self.lo.min(origin);
            self.hi = self.hi.max(origin + Vec3::from(max));

            let slot = self.new_slot();
            self.ensure_slots(ctx, memory, bindless, slot + 1, grow, frame)?;
            let solid: u32 = counts.iter().sum();
            let mut buckets = [0u32; 8];
            buckets[..7].copy_from_slice(&counts);
            self.slot_cpu[slot as usize] = Slot {
                lo: [min[0], min[1], min[2], 0.0],
                hi: [max[0], max[1], max[2], 0.0],
                origin_base: [
                    section.origin[0],
                    section.origin[1],
                    section.origin[2],
                    vertices.offset as i32,
                ],
                info: [
                    pool,
                    self.pools[pool as usize].vertex_index,
                    SLOT_LIVE | if section.water { SLOT_TRANSLUCENT } else { 0 },
                    0,
                ],
                ranges: [
                    first,
                    if section.water {
                        index_count
                    } else {
                        index_count - solid
                    },
                    0,
                    0,
                ],
                buckets,
                spare: [0; 4],
            };
            self.dirty_slots.push(slot);
            self.set_visible(slot, true);
            if sort.is_some() {
                self.translucent_count += 1;
                self.ranks_dirty = true;
            }
            self.pools[pool as usize].slots += 1;
            self.stats.vertices += vertex_count as u64;
            self.stats.indices += index_count as u64;
            self.entries.insert(
                key,
                Entry {
                    slot,
                    pool,
                    vertices,
                    indices: index_alloc,
                    vertex_count,
                    index_count,
                    sort,
                },
            );
            self.sections_changed = true;
        }
        self.stats.sections = self.entries.len();
        self.stats.pools = self.pools.len();
        Ok(())
    }

    /// A range in the translucent index buffer, growing the buffer (with a
    /// copy of what it held) when it is full.
    #[allow(clippy::too_many_arguments, reason = "the upload context")]
    fn translucent_range(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        count: u32,
        grow: &mut Vec<GrowCopy>,
        frame: u64,
    ) -> Result<Allocation, SetupError> {
        if let Some(a) = self.translucent_alloc.allocate(count) {
            return Ok(a);
        }
        let old_capacity = self.translucent_capacity;
        let capacity = (old_capacity * 2).max(count * 2);
        let buffer = memory.buffer(
            ctx,
            "terrain translucent indices",
            capacity as u64 * 4,
            vk::BufferUsageFlags::INDEX_BUFFER
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::GpuOnly,
        )?;
        let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
        let old = std::mem::replace(&mut self.translucent, Grown { buffer, index });
        grow.push(GrowCopy {
            from: old.buffer.raw,
            to: self.translucent.buffer.raw,
            size: old_capacity as u64 * 4,
        });
        bindless.remove_buffer(old.index, frame);
        self.retired.push((old.buffer, frame));
        // Everything allocated so far keeps its place, in one block at the
        // start of the new buffer.
        let mut alloc = Allocator::new(capacity);
        let block = alloc
            .allocate(old_capacity)
            .ok_or_else(|| SetupError("the translucent buffer can't grow".to_owned()))?;
        let live = self
            .entries
            .values()
            .filter(|e| matches!(e.indices, Indices::Translucent(..)))
            .count() as u32;
        if live == 0 {
            alloc.free(block);
        }
        self.translucent_alloc = alloc;
        self.translucent_capacity = capacity;
        self.translucent_generation += 1;
        self.stats.grown[0] += 1;
        self.translucent_legacy = (live > 0).then_some((block, live));
        self.translucent_alloc
            .allocate(count)
            .ok_or_else(|| SetupError("the translucent buffer can't grow".to_owned()))
    }

    /// Grows the slot buffer (and the per-slot visibility and rank buffers)
    /// to hold `needed` slots.
    #[allow(clippy::too_many_arguments, reason = "the upload context")]
    fn ensure_slots(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        needed: u32,
        grow: &mut Vec<GrowCopy>,
        frame: u64,
    ) -> Result<(), SetupError> {
        if needed <= self.slot_capacity {
            return Ok(());
        }
        let capacity = (self.slot_capacity * 2).max(needed.next_multiple_of(32));
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST;
        let retired = &mut self.retired;
        let mut replace = |grown: &mut Grown, name, size, usage, copy: Option<u64>| {
            let buffer = memory.buffer(ctx, name, size, usage, MemoryLocation::GpuOnly)?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            let old = std::mem::replace(grown, Grown { buffer, index });
            if let Some(size) = copy {
                grow.push(GrowCopy {
                    from: old.buffer.raw,
                    to: grown.buffer.raw,
                    size,
                });
            }
            bindless.remove_buffer(old.index, frame);
            retired.push((old.buffer, frame));
            Ok::<_, SetupError>(())
        };
        replace(
            &mut self.slots,
            "terrain slots",
            capacity as u64 * SLOT_SIZE,
            storage | vk::BufferUsageFlags::TRANSFER_SRC,
            Some(self.slot_capacity as u64 * SLOT_SIZE),
        )?;
        replace(
            &mut self.visible,
            "terrain visible slots",
            capacity as u64 / 8,
            storage,
            None,
        )?;
        replace(
            &mut self.ranks,
            "terrain translucent ranks",
            capacity as u64 * 4,
            storage,
            None,
        )?;
        self.slot_capacity = capacity;
        self.stats.grown[1] += 1;
        self.slot_cpu.resize(capacity as usize, Slot::default());
        self.visible_bits.resize(capacity as usize / 32, 0);
        self.rank_of.resize(capacity as usize, 0);
        self.visible_dirty = true;
        self.ranks_dirty = true;
        Ok(())
    }

    /// Re-sorts translucent layers whose order the eye's move made stale,
    /// nearest first, within the quad budget.
    fn sort_translucent(&mut self, uploads: &mut Uploads, eye: Vec3, copies: &mut Vec<Copy>) {
        let eye_block = eye.floor().as_ivec3();
        let mut due: Vec<(f32, ([i32; 3], bool))> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                let sort = entry.sort.as_ref()?;
                let distance = (sort.eye_from(eye) - Vec3::splat(8.0)).length();
                let stale = match sort.sorted_for {
                    None => true,
                    Some(at) if distance < SORT_NEAR => at != eye_block,
                    Some(at) => at >> 4 != eye_block >> 4,
                };
                stale.then_some((distance, *key))
            })
            .collect();
        due.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut budget = SORT_BUDGET;
        let target = self.translucent.buffer.raw;
        for (_, key) in due {
            let Some(sort) = self.entries.get_mut(&key).and_then(|e| e.sort.as_mut()) else {
                continue;
            };
            if sort.centres.len() > budget && budget < SORT_BUDGET {
                break;
            }
            budget = budget.saturating_sub(sort.centres.len());
            let order = sort.back_to_front(sort.eye_from(eye));
            let Some(staged) = uploads.stage(bytemuck::cast_slice(&order)) else {
                break;
            };
            copies.push(Copy {
                src: staged.buffer,
                from: staged.offset,
                to: target,
                at: sort.first as u64 * 4,
                size: staged.size,
            });
            sort.sorted_for = Some(eye_block);
        }
    }

    /// Ranks the translucent layers far to near, when the eye changes
    /// section or the set changes. True when the ranks changed.
    fn rank_translucent(&mut self, eye: Vec3) -> bool {
        let section = eye.floor().as_ivec3() >> 4;
        if !self.ranks_dirty && self.ranked_for == Some(section) {
            return false;
        }
        let mut order: Vec<(f32, u32)> = self
            .entries
            .values()
            .filter(|e| e.sort.is_some())
            .map(|e| {
                let s = &self.slot_cpu[e.slot as usize];
                let origin = Vec3::new(
                    s.origin_base[0] as f32,
                    s.origin_base[1] as f32,
                    s.origin_base[2] as f32,
                );
                let centre =
                    origin + (Vec3::from_slice(&s.lo[..3]) + Vec3::from_slice(&s.hi[..3])) / 2.0;
                (centre.distance_squared(eye), e.slot)
            })
            .collect();
        order.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        for (rank, (_, slot)) in order.iter().enumerate() {
            self.rank_of[*slot as usize] = rank as u32;
        }
        self.ranks_dirty = false;
        self.ranked_for = Some(section);
        true
    }

    /// Runs the game's cave-visibility flood fill from the eye's section
    /// and marks the slots it reaches.
    fn fill_visibility(&mut self, eye: Vec3) {
        let here = IVec3::new(
            (eye.x / 16.0).floor() as i32,
            crate::renderer::occlusion::section_y(eye.y),
            (eye.z / 16.0).floor() as i32,
        );
        self.frames_since_fill += 1;
        let stale =
            self.fill_for != Some(here) || (self.sections_changed && self.frames_since_fill >= 10);
        if !stale || self.vis.is_empty() {
            return;
        }
        self.frames_since_fill = 0;
        self.sections_changed = false;
        self.fill_for = Some(here);
        let (mut radius, mut y_lo, mut y_hi) = (0, i32::MAX, i32::MIN);
        for &(x, y, z) in self.vis.keys() {
            radius = radius.max((x - here.x).abs()).max((z - here.z).abs());
            y_lo = y_lo.min(y);
            y_hi = y_hi.max(y);
        }
        let vis = &self.vis;
        let reached = crate::renderer::occlusion::flood_fill(
            (here.x, here.y, here.z),
            radius.min(64),
            y_lo,
            y_hi,
            |pos| vis.get(&pos).copied().unwrap_or(VisibilitySet::all()),
        );
        self.visible_bits.iter_mut().for_each(|w| *w = 0);
        let mut count = 0;
        for (x, y, z) in reached {
            for water in [false, true] {
                if let Some(entry) = self.entries.get(&([x * 16, y, z * 16], water)) {
                    let slot = entry.slot;
                    self.visible_bits[slot as usize / 32] |= 1 << (slot % 32);
                    count += 1;
                }
            }
        }
        self.stats.reached = count;
        self.visible_dirty = true;
    }

    /// Records the terrain into `graph`, drawing to `color`. `slot` is the
    /// frame-in-flight slot, whose fence has signalled.
    #[allow(clippy::too_many_arguments, reason = "one frame's whole context")]
    pub fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        uploads: &mut Uploads,
        graph: &mut Graph,
        color: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        rotation: super::swapchain::PreRotation,
        frame: u64,
        slot: usize,
        camera: Camera,
        view: View,
    ) -> Result<(), SetupError> {
        self.read_back(ctx, slot);
        // Buffers replaced at least a round of frames ago are unused now.
        let mut i = 0;
        while i < self.retired.len() {
            if frame >= self.retired[i].1 + FRAMES_IN_FLIGHT as u64 {
                let (buffer, _) = self.retired.swap_remove(i);
                memory.free_buffer(ctx, buffer);
            } else {
                i += 1;
            }
        }

        let (depth, depth_view, depth_index) = self
            .depth
            .get(ctx, memory, bindless, graph, extent, frame)?;
        if let Some(entities) = &mut self.entities {
            entities.prepare(ctx, memory, graph, frame, slot);
        }
        if let Some(env) = &mut self.env {
            env.prepare(ctx, memory, graph, frame);
        }

        // Section changes, re-sorts and per-slot data, all copied in one
        // pass.
        let mut copies = Vec::new();
        let mut grow = Vec::new();
        self.apply_changes(
            ctx,
            memory,
            bindless,
            uploads,
            &mut copies,
            &mut grow,
            frame,
            camera.eye,
        )?;
        self.sort_translucent(uploads, camera.eye, &mut copies);
        if self.options.visibility {
            self.fill_visibility(camera.eye);
        }
        if self.rank_translucent(camera.eye) {
            match uploads.stage(bytemuck::cast_slice(
                &self.rank_of[..self.slot_high.max(1) as usize],
            )) {
                Some(staged) => copies.push(Copy {
                    src: staged.buffer,
                    from: staged.offset,
                    to: self.ranks.buffer.raw,
                    at: 0,
                    size: staged.size,
                }),
                // Try again next frame.
                None => self.ranks_dirty = true,
            }
        }
        if self.visible_dirty
            && let Some(staged) = uploads.stage(bytemuck::cast_slice(&self.visible_bits))
        {
            copies.push(Copy {
                src: staged.buffer,
                from: staged.offset,
                to: self.visible.buffer.raw,
                at: 0,
                size: staged.size,
            });
            self.visible_dirty = false;
        }
        self.dirty_slots.sort_unstable();
        self.dirty_slots.dedup();
        let mut kept = Vec::new();
        for &s in &self.dirty_slots {
            match uploads.stage(bytemuck::bytes_of(&self.slot_cpu[s as usize])) {
                Some(staged) => copies.push(Copy {
                    src: staged.buffer,
                    from: staged.offset,
                    to: self.slots.buffer.raw,
                    at: s as u64 * SLOT_SIZE,
                    size: staged.size,
                }),
                None => kept.push(s),
            }
        }
        self.dirty_slots = kept;
        self.ensure_commands(ctx, memory, bindless, frame)?;

        let atlas_ready = self
            .atlas_staging
            .as_ref()
            .is_none_or(|(_, copied)| copied.is_some());
        graph.image(
            self.atlas.raw,
            vk::ImageAspectFlags::COLOR,
            if atlas_ready {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            } else {
                vk::ImageLayout::UNDEFINED
            },
        );
        if let Some((_, Some(copied))) = &self.atlas_staging
            && frame >= copied + FRAMES_IN_FLIGHT as u64
            && let Some((staging, _)) = self.atlas_staging.take()
        {
            memory.free_buffer(ctx, staging);
        }
        let atlas_copy = match &mut self.atlas_staging {
            Some((staging, copied @ None)) => {
                *copied = Some(frame);
                Some(staging.raw)
            }
            _ => None,
        };
        self.record_copies(ctx, graph, &copies, &grow, atlas_copy);
        let device = &ctx.device;

        if self.pipes.is_none()
            && let Some(result) = self.pending.as_mut().and_then(|p| p.take())
        {
            let (built, ms) = result?;
            self.pipes = Some(Pipes::from(&built));
            self.pending = None;
            self.stats.pipelines = Some((ms, frame));
        }
        let Some(pipes) = self.pipes else {
            // Still compiling: clear the frame; the uploads above went on.
            graph.pass(
                "terrain waiting",
                &[(color.0, Use::ColorAttachment)],
                &[],
                |cmd| unsafe {
                    // SAFETY: the colour image is an attachment, as the graph set.
                    let colors = [vk::RenderingAttachmentInfo::default()
                        .image_view(color.1)
                        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                        .load_op(vk::AttachmentLoadOp::CLEAR)
                        .store_op(vk::AttachmentStoreOp::STORE)
                        .clear_value(vk::ClearValue {
                            color: vk::ClearColorValue {
                                float32: [0.47, 0.65, 1.0, 1.0],
                            },
                        })];
                    device.cmd_begin_rendering(
                        cmd,
                        &vk::RenderingInfo::default()
                            .render_area(vk::Rect2D {
                                offset: vk::Offset2D::default(),
                                extent,
                            })
                            .layer_count(1)
                            .color_attachments(&colors),
                    );
                    device.cmd_end_rendering(cmd);
                },
            );
            let _ = (depth, depth_view, depth_index);
            return Ok(());
        };

        let aspect = rotation.aspect;
        let view_proj = rotation.clip
            * Mat4::perspective_infinite_reverse_rh(camera.fov.to_radians(), aspect, NEAR)
            * Mat4::look_to_rh(camera.eye, camera.forward(), Vec3::Y);

        // Hi-Z only with a pyramid of this size, from a camera close by.
        let hiz = match (&self.pyramid, self.pyramid_view) {
            (Some((grown, size, levels)), Some((prev, eye)))
                if self.options.hiz
                    && *size == (extent.width, extent.height)
                    && eye.distance(camera.eye) < HIZ_GUARD =>
            {
                Some((grown.index, *size, *levels, prev))
            }
            _ => None,
        };
        let mut flags = CULL_FRUSTUM;
        if hiz.is_some() {
            flags |= CULL_HIZ;
            self.stats.hiz_frames += 1;
        }
        if self.options.visibility {
            flags |= CULL_VISIBILITY;
        }
        if self.options.buckets {
            flags |= CULL_BUCKETS;
        }
        if !self.options.wave {
            flags |= CULL_NO_WAVE;
        }
        let cull_view = CullView {
            view_proj: view_proj.to_cols_array_2d(),
            prev_view_proj: hiz.map_or(view_proj, |h| h.3).to_cols_array_2d(),
            camera: [camera.eye.x, camera.eye.y, camera.eye.z, 0.0],
            hiz: hiz.map_or([0; 4], |(index, size, levels, _)| {
                [index, levels, size.0, size.1]
            }),
            flags: [flags, 0, 0, 0],
            pad: [0; 4],
            fog_color: self.env.as_ref().map_or([0.0; 4], |e| e.fog_color),
            fog_ranges: self.env.as_ref().map_or([0.0; 4], |e| e.fog_ranges),
            env: self
                .env
                .as_ref()
                .map_or([u32::MAX, 0, 0, 0], |e| [e.lightmap_index(), 1, 0, 0]),
        };
        self.views[slot].write(0, bytemuck::bytes_of(&cull_view));

        graph.pass(
            "reset counts",
            &[],
            &[(self.counts.raw, Use::TransferDst)],
            |cmd| unsafe {
                // SAFETY: a fill of a buffer this pass owns.
                device.cmd_fill_buffer(cmd, self.counts.raw, 0, vk::WHOLE_SIZE, 0);
            },
        );

        let slot_count = self.slot_high;
        let mut cull_buffers = vec![
            (self.slots.buffer.raw, Use::ComputeRead),
            (self.visible.buffer.raw, Use::ComputeRead),
            (self.ranks.buffer.raw, Use::ComputeRead),
            (self.commands.raw, Use::ComputeWrite),
            (self.counts.raw, Use::ComputeWrite),
        ];
        if let Some((grown, ..)) = &self.pyramid {
            cull_buffers.push((grown.buffer.raw, Use::ComputeRead));
        }
        let pyramid_buffer = self.pyramid.as_ref().map(|(g, ..)| g.buffer.raw);
        if let Some(entities) = &mut self.entities {
            entities.record_cull(
                ctx,
                bindless,
                pipelines,
                uploads,
                graph,
                self.view_indices[slot],
                pyramid_buffer,
                camera.eye,
                slot,
            )?;
        }
        graph.pass("terrain cull", &[], &cull_buffers, |cmd| unsafe {
            // SAFETY: recording with the shared layout and the bindless set;
            // the push names live buffers.
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipes.cull);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                pipelines.layout,
                0,
                &[bindless.set],
                &[],
            );
            push(
                ctx,
                cmd,
                pipelines.layout,
                &Push {
                    ids: [
                        self.slots.index,
                        self.visible.index,
                        self.commands_index,
                        self.counts_index,
                    ],
                    more: [
                        slot_count,
                        self.list_capacity,
                        self.view_indices[slot],
                        self.translucent_start,
                    ],
                    params: [f32::from_bits(self.ranks.index), 0.0, 0.0, 0.0],
                    view_proj: [[0.0; 4]; 4],
                },
            );
            device.cmd_dispatch(cmd, slot_count.div_ceil(64).max(1), 1, 1);
        });

        let readback = self.readback[slot].raw;
        let lists = self.pools.len() as u64 * 2;
        if lists > 0 {
            graph.pass(
                "terrain counts readback",
                &[],
                &[(self.counts.raw, Use::TransferSrc)],
                |cmd| unsafe {
                    // SAFETY: a copy from the counts into this slot's
                    // readback buffer, read after the slot's fence.
                    device.cmd_copy_buffer(
                        cmd,
                        self.counts.raw,
                        readback,
                        &[vk::BufferCopy {
                            src_offset: 0,
                            dst_offset: 0,
                            size: lists * 4,
                        }],
                    );
                },
            );
        }

        let mut buffers = vec![
            (self.slots.buffer.raw, Use::VertexRead),
            (self.commands.raw, Use::IndirectRead),
            (self.counts.raw, Use::IndirectRead),
            (self.translucent.buffer.raw, Use::IndexRead),
        ];
        for pool in &self.pools {
            buffers.push((pool.vertices.raw, Use::VertexRead));
            buffers.push((pool.indices.raw, Use::IndexRead));
        }
        let stats_pool = self.stats_pools.get(slot).copied();
        if let Some(issued) = self.stats_issued.get_mut(slot) {
            *issued = true;
        }
        let view_proj_cols = view_proj.to_cols_array_2d();
        let mut images = vec![
            (color.0, Use::ColorAttachment),
            (depth, Use::DepthAttachment),
            (self.atlas.raw, Use::SampledFragment),
        ];
        if let Some(entities) = &self.entities {
            entities.uses(&mut images, &mut buffers);
        }
        if let Some(env) = &self.env {
            env.uses(&mut images, &mut buffers);
        }
        let view_index = self.view_indices[slot];
        let clear = self
            .env
            .as_ref()
            .map_or([0.47, 0.65, 1.0, 1.0], |e| e.clear);
        let seconds = frame as f32 / 60.0;
        graph.pass("terrain", &images, &buffers, |cmd| unsafe {
            // SAFETY: as above; the attachments are in the layouts the
            // graph just moved them to, and the query was reset.
            let colors = [vk::RenderingAttachmentInfo::default()
                .image_view(color.1)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue { float32: clear },
                })];
            let depth_attachment = vk::RenderingAttachmentInfo::default()
                .image_view(depth_view)
                .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 0.0,
                        stencil: 0,
                    },
                });
            if let Some(pool) = stats_pool {
                device.cmd_begin_query(cmd, pool, 0, vk::QueryControlFlags::empty());
            }
            device.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent,
                    })
                    .layer_count(1)
                    .color_attachments(&colors)
                    .depth_attachment(&depth_attachment),
            );
            set_viewport(ctx, cmd, extent);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipelines.layout,
                0,
                &[bindless.set],
                &[],
            );
            if let Some(env) = &self.env {
                env.draw_sky(ctx, cmd, pipelines, view_index, view_proj_cols);
            }
            push(
                ctx,
                cmd,
                pipelines.layout,
                &Push {
                    ids: [self.slots.index, view_index, 0, 0],
                    more: [self.atlas_index, view.mode(), 0, 0],
                    params: [0.0; 4],
                    view_proj: view_proj_cols,
                },
            );
            let capacity = self.list_capacity;
            for (kind, pipeline) in [pipes.solid, pipes.cutout].into_iter().enumerate() {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                for (p, pool) in self.pools.iter().enumerate() {
                    let list = p as u64 * 2 + kind as u64;
                    device.cmd_bind_index_buffer(cmd, pool.indices.raw, 0, vk::IndexType::UINT32);
                    device.cmd_draw_indexed_indirect_count(
                        cmd,
                        self.commands.raw,
                        list * capacity as u64 * COMMAND_SIZE,
                        self.counts.raw,
                        list * 4,
                        capacity,
                        COMMAND_SIZE as u32,
                    );
                }
            }
            if let Some(entities) = &self.entities {
                entities.draw_solid(
                    ctx,
                    cmd,
                    pipelines,
                    view_proj_cols,
                    view.mode(),
                    seconds,
                    view_index,
                );
                // The entity draw pushed its own constants; the terrain's
                // translucent draw needs its own back (its slot buffer
                // index, or it reads past the entity buffer and the
                // device is lost).
                push(
                    ctx,
                    cmd,
                    pipelines.layout,
                    &Push {
                        ids: [self.slots.index, view_index, 0, 0],
                        more: [self.atlas_index, view.mode(), 0, 0],
                        params: [0.0; 4],
                        view_proj: view_proj_cols,
                    },
                );
            }
            // Translucent last, far to near over everything opaque.
            if self.translucent_count > 0 {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipes.translucent);
                device.cmd_bind_index_buffer(
                    cmd,
                    self.translucent.buffer.raw,
                    0,
                    vk::IndexType::UINT32,
                );
                device.cmd_draw_indexed_indirect(
                    cmd,
                    self.commands.raw,
                    self.translucent_start as u64 * COMMAND_SIZE,
                    self.translucent_count,
                    COMMAND_SIZE as u32,
                );
            }
            if let Some(entities) = &self.entities {
                entities.draw_blend(
                    ctx,
                    cmd,
                    pipelines,
                    view_proj_cols,
                    view.mode(),
                    seconds,
                    view_index,
                );
            }
            if let Some(env) = &self.env {
                env.draw_clouds(ctx, cmd, pipelines, view_index, view_proj_cols);
            }
            device.cmd_end_rendering(cmd);
            if let Some(pool) = stats_pool {
                device.cmd_end_query(cmd, pool, 0);
            }
        });

        self.build_pyramid(
            ctx,
            memory,
            bindless,
            pipelines,
            pipes.hiz_reduce,
            graph,
            (depth, depth_index),
            extent,
            frame,
        )?;
        self.pyramid_view = Some((view_proj, camera.eye));

        match view {
            View::Depth => self.depth_view.record(
                ctx,
                bindless,
                pipelines,
                graph,
                (depth, depth_index),
                color,
                extent,
                NEAR,
                64.0,
            ),
            View::Pyramid => {
                if let Some((grown, size, levels)) = &self.pyramid {
                    self.depth_view.record_pyramid(
                        ctx,
                        bindless,
                        pipelines,
                        graph,
                        (grown.buffer.raw, grown.index),
                        *size,
                        self.options.hiz_level.clamp(1, *levels),
                        color,
                        extent,
                        NEAR,
                        64.0,
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Makes room for this frame's commands: per pool a solid and a cutout
    /// list sized for the fullest pool, then the translucent commands.
    fn ensure_commands(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        frame: u64,
    ) -> Result<(), SetupError> {
        let fullest = self.pools.iter().map(|p| p.slots).max().unwrap_or(0);
        let capacity = (fullest * MAX_RUNS).max(64);
        let lists = self.pools.len() as u32 * 2;
        let translucent = self.translucent_count.next_multiple_of(256);
        let needed = (lists * capacity + translucent) as u64 * COMMAND_SIZE;
        if needed > self.commands.size {
            let size = needed + needed / 2;
            let buffer = memory.buffer(
                ctx,
                "terrain commands",
                size,
                vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::TRANSFER_DST
                    | vk::BufferUsageFlags::INDIRECT_BUFFER,
                MemoryLocation::GpuOnly,
            )?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            bindless.remove_buffer(self.commands_index, frame);
            let old = std::mem::replace(&mut self.commands, buffer);
            self.retired.push((old, frame));
            self.commands_index = index;
        }
        self.list_capacity = capacity;
        self.translucent_start = lists * capacity;
        Ok(())
    }

    /// Builds the occlusion pyramid from this frame's depth, for next
    /// frame's cull.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn build_pyramid(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        hiz_reduce: vk::Pipeline,
        graph: &mut Graph,
        depth: (vk::Image, u32),
        extent: vk::Extent2D,
        frame: u64,
    ) -> Result<(), SetupError> {
        let size = (extent.width, extent.height);
        if self.pyramid.as_ref().is_none_or(|(_, s, _)| *s != size) {
            if let Some((old, ..)) = self.pyramid.take() {
                bindless.remove_buffer(old.index, frame);
                self.retired.push((old.buffer, frame));
            }
            let (mut w, mut h, mut levels, mut texels) = (size.0, size.1, 0u32, 0u64);
            while w > 1 || h > 1 {
                w = w.div_ceil(2);
                h = h.div_ceil(2);
                levels += 1;
                texels += w as u64 * h as u64;
            }
            let buffer = memory.buffer(
                ctx,
                "terrain depth pyramid",
                texels.max(1) * 4,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                MemoryLocation::GpuOnly,
            )?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            self.pyramid = Some((Grown { buffer, index }, size, levels.max(1)));
            self.pyramid_view = None;
        }
        let Some((grown, size, levels)) = &self.pyramid else {
            return Ok(());
        };
        let device = &ctx.device;
        for base in [0u32, 6] {
            if base >= *levels {
                break;
            }
            let (mut w, mut h) = *size;
            for _ in 0..base {
                w = w.div_ceil(2);
                h = h.div_ceil(2);
            }
            let depth_use = [(depth.0, Use::ComputeRead)];
            let images: &[(vk::Image, Use)] = if base == 0 { &depth_use } else { &[] };
            graph.pass(
                if base == 0 {
                    "hiz levels 1-6"
                } else {
                    "hiz levels 7+"
                },
                images,
                &[(grown.buffer.raw, Use::ComputeWrite)],
                |cmd| unsafe {
                    // SAFETY: the depth is in its read-only layout and the
                    // pyramid is this pass's to write.
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, hiz_reduce);
                    device.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::COMPUTE,
                        pipelines.layout,
                        0,
                        &[bindless.set],
                        &[],
                    );
                    push(
                        ctx,
                        cmd,
                        pipelines.layout,
                        &Push {
                            ids: [grown.index, depth.1, 0, 0],
                            more: [base, size.0, size.1, *levels],
                            params: [0.0; 4],
                            view_proj: [[0.0; 4]; 4],
                        },
                    );
                    device.cmd_dispatch(cmd, w.div_ceil(64), h.div_ceil(64), 1);
                },
            );
        }
        Ok(())
    }

    /// Records this frame's copies: buffers grown, then section data,
    /// re-sorts and per-slot data from the staging ring, and the atlas.
    fn record_copies(
        &self,
        ctx: &Context,
        graph: &mut Graph,
        copies: &[Copy],
        grow: &[GrowCopy],
        atlas: Option<vk::Buffer>,
    ) {
        if copies.is_empty() && grow.is_empty() && atlas.is_none() {
            return;
        }
        let mut buffers: Vec<(vk::Buffer, Use)> = Vec::new();
        for g in grow {
            buffers.push((g.from, Use::TransferSrc));
            buffers.push((g.to, Use::TransferDst));
        }
        for c in copies {
            if !buffers.iter().any(|(b, _)| *b == c.to) {
                buffers.push((c.to, Use::TransferDst));
            }
        }
        let images: Vec<(vk::Image, Use)> = atlas
            .iter()
            .map(|_| (self.atlas.raw, Use::TransferDst))
            .collect();
        let device = &ctx.device;
        let (width, height) = self.atlas_size;
        let atlas_raw = self.atlas.raw;
        graph.pass("terrain upload", &images, &buffers, |cmd| unsafe {
            // SAFETY: copies between live buffers in the transfer states the
            // graph set; grown buffers are copied before anything is written
            // into them.
            for g in grow {
                device.cmd_copy_buffer(
                    cmd,
                    g.from,
                    g.to,
                    &[vk::BufferCopy {
                        src_offset: 0,
                        dst_offset: 0,
                        size: g.size,
                    }],
                );
            }
            if !grow.is_empty() {
                // The grown copies and the writes after them share
                // destinations: order them.
                let barrier = [vk::MemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                    .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)];
                device.cmd_pipeline_barrier2(
                    cmd,
                    &vk::DependencyInfo::default().memory_barriers(&barrier),
                );
            }
            // Copies within one pass run in no order: one overlapping an
            // earlier copy's target (a section re-meshed twice in a frame
            // reusing its range) waits for it.
            let mut written: Vec<(vk::Buffer, u64, u64)> = Vec::new();
            for c in copies {
                let overlaps = written
                    .iter()
                    .any(|&(b, at, end)| b == c.to && c.at < end && at < c.at + c.size);
                if overlaps {
                    let barrier = [vk::MemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)];
                    device.cmd_pipeline_barrier2(
                        cmd,
                        &vk::DependencyInfo::default().memory_barriers(&barrier),
                    );
                    written.clear();
                }
                written.push((c.to, c.at, c.at + c.size));
                device.cmd_copy_buffer(
                    cmd,
                    c.src,
                    c.to,
                    &[vk::BufferCopy {
                        src_offset: c.from,
                        dst_offset: c.at,
                        size: c.size,
                    }],
                );
            }
            if let Some(staging) = atlas {
                device.cmd_copy_buffer_to_image(
                    cmd,
                    staging,
                    atlas_raw,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy {
                        buffer_offset: 0,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: 0,
                            layer_count: 1,
                        },
                        image_offset: vk::Offset3D::default(),
                        image_extent: vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        },
                    }],
                );
            }
        });
    }

    /// Reads what frame slot `slot` counted last time round (its fence has
    /// signalled): commands drawn and vertex invocations.
    fn read_back(&mut self, ctx: &Context, slot: usize) {
        let lists = self.pools.len() * 2;
        let capacity = self.list_capacity;
        if let Some(bytes) = self.readback[slot].mapped() {
            let counts: &[u32] = bytemuck::cast_slice(&bytes[..lists * 4]);
            let mut sums = [0u32; 3];
            for (i, &n) in counts.iter().enumerate() {
                sums[i % 2] += n.min(capacity);
            }
            sums[2] = self.translucent_count;
            self.stats.commands = sums;
        }
        let Some(&pool) = self.stats_pools.get(slot) else {
            return;
        };
        if self.stats_issued[slot] {
            // One query of two statistics: ash counts queries by the
            // slice's length, so one element of both.
            let mut values = [[0u64; 2]];
            // SAFETY: the slot's fence has signalled, so its query is done.
            let read = unsafe {
                ctx.device.get_query_pool_results(
                    pool,
                    0,
                    &mut values,
                    vk::QueryResultFlags::TYPE_64,
                )
            };
            if read.is_ok() {
                self.stats.vertex_invocations = Some(values[0][0]);
                self.stats.primitives = Some(values[0][1]);
            }
        }
        // SAFETY: host reset of a query the GPU is done with.
        unsafe { ctx.device.reset_query_pool(pool, 0, 1) };
        self.stats_issued[slot] = false;
    }

    pub fn destroy(mut self, ctx: &Context, memory: &Memory) {
        // A batch still compiling finishes first, so its pipelines go too.
        let mut pipelines: Vec<vk::Pipeline> =
            self.pipes.map(Pipes::all).into_iter().flatten().collect();
        if let Some(Ok((built, _))) = self.pending.as_mut().and_then(|p| p.wait()) {
            pipelines.extend(built);
        }
        // SAFETY: the caller idles the device first.
        unsafe {
            for pipeline in pipelines {
                ctx.device.destroy_pipeline(pipeline, None);
            }
            ctx.device.destroy_shader_module(self.module, None);
            for pool in self.stats_pools {
                ctx.device.destroy_query_pool(pool, None);
            }
        }
        self.depth_view.destroy(ctx);
        let mut buffers = vec![
            self.translucent.buffer,
            self.slots.buffer,
            self.visible.buffer,
            self.ranks.buffer,
            self.commands,
            self.counts,
        ];
        buffers.extend(self.readback);
        buffers.extend(self.views);
        buffers.extend(self.retired.into_iter().map(|(b, _)| b));
        for pool in self.pools {
            buffers.push(pool.vertices);
            buffers.push(pool.indices);
        }
        if let Some((grown, ..)) = self.pyramid {
            buffers.push(grown.buffer);
        }
        if let Some((staging, _)) = self.atlas_staging {
            buffers.push(staging);
        }
        for buffer in buffers {
            memory.free_buffer(ctx, buffer);
        }
        if let Some(entities) = self.entities.take() {
            entities.destroy(ctx, memory);
        }
        if let Some(env) = self.env.take() {
            env.destroy(ctx, memory);
        }
        memory.free_image(ctx, self.atlas);
        self.depth.destroy(ctx, memory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: i16, y: i16, z: i16) -> PackedVertex {
        PackedVertex {
            pos: [x * 256, y * 256, z * 256, 0],
            ..Default::default()
        }
    }

    #[test]
    fn quads_go_to_the_bucket_their_normal_names() {
        // A +y face (counter-clockwise seen from above) and a -x face
        // (counter-clockwise seen from -x).
        let verts = [
            vertex(0, 1, 0),
            vertex(0, 1, 1),
            vertex(1, 1, 1),
            vertex(1, 1, 0),
            vertex(0, 0, 0),
            vertex(0, 0, 1),
            vertex(0, 1, 1),
            vertex(0, 1, 0),
        ];
        let idx = [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let (counts, sorted) = bucket(&verts, &idx);
        assert_eq!(counts, [0, 6, 6, 0, 0, 0, 0]);
        assert_eq!(&sorted[..6], &[4, 5, 6, 4, 6, 7]);
        assert_eq!(sorted.len(), 12);
    }

    #[test]
    fn translucent_quads_sort_far_to_near() {
        let mut verts = Vec::new();
        let mut idx = Vec::new();
        for z in 0..3i16 {
            let base = verts.len() as u32;
            verts.extend([
                vertex(0, 0, z),
                vertex(1, 0, z),
                vertex(1, 1, z),
                vertex(0, 1, z),
            ]);
            idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let section = Section {
            origin: [0, 0, 0],
            water: true,
            vis: VisibilitySet::all(),
            verts,
            idx,
            cutout_idx: Vec::new(),
        };
        let sort = Sort::new(&section, 0);
        let order = sort.back_to_front(Vec3::new(0.5, 0.5, -5.0));
        assert_eq!(order[0], 8, "the farthest quad (z = 2) first");
        assert_eq!(order[12], 0, "the nearest quad (z = 0) last");
    }
}
