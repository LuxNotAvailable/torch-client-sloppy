use std::mem::size_of;
use std::num::NonZeroU64;
use std::sync::atomic::Ordering;

use bevy::asset::uuid::Uuid;
use bevy::asset::{AssetId, UntypedAssetId};
use bevy::core_pipeline::core_3d::{
    AlphaMask3d, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey, Transparent3d,
};
use bevy::core_pipeline::prepass::{OpaqueNoLightmap3dBatchSetKey, OpaqueNoLightmap3dBinKey};
use bevy::ecs::query::Has;
use bevy::ecs::system::lifetimeless::{Read, SRes};
use bevy::ecs::system::{SystemChangeTick, SystemParamItem};
use bevy::mesh::{
    MeshVertexBufferLayout, MeshVertexBufferLayoutRef, MeshVertexBufferLayouts, VertexBufferLayout,
};
use bevy::pbr::{
    MeshPipeline, MeshPipelineKey, SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup,
    ViewKeyCache,
};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::mesh::allocator::SlabId;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
    PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass,
    ViewBinnedRenderPhases, ViewSortedRenderPhases,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
    BufferDescriptor, BufferUsages, Face, IndexFormat, PipelineCache, PrimitiveTopology,
    RenderPipelineDescriptor, SamplerBindingType, ShaderStages, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TextureSampleType, TextureViewId, VertexAttribute, VertexFormat,
    VertexStepMode,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, RetainedViewEntity};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::{Shader, ShaderDefVal};

use super::cull::{CullBuffers, DirectLists, TerrainViews};
use super::pools::{SlotTable, TerrainPools};
use super::{
    STREAM_CUTOUT, STREAM_SOLID, STREAM_WATER, STREAMS, SlotMeta, TerrainParams, TerrainShaders,
    TerrainTextures, TerrainTier, TerrainView,
};

pub const TERRAIN_GROUP: usize = 2;

const VERTEX_STRIDE: u64 = 20;

const TERRAIN_ASSET_TAG: u128 = 0x7e_77a1 << 64;

pub(crate) fn packed_vertex_layout() -> VertexBufferLayout {
    VertexBufferLayout {
        array_stride: VERTEX_STRIDE,
        step_mode: VertexStepMode::Vertex,
        attributes: vec![
            VertexAttribute {
                format: VertexFormat::Sint16x4,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: VertexFormat::Unorm16x2,
                offset: 8,
                shader_location: 1,
            },
            VertexAttribute {
                format: VertexFormat::Unorm8x4,
                offset: 12,
                shader_location: 2,
            },
            VertexAttribute {
                format: VertexFormat::Unorm8x4,
                offset: 16,
                shader_location: 3,
            },
        ],
    }
}

fn item_asset_id(pool: u32, stream: u32) -> UntypedAssetId {
    AssetId::<Mesh>::Uuid {
        uuid: Uuid::from_u128(TERRAIN_ASSET_TAG | (pool * STREAMS + stream) as u128),
    }
    .untyped()
}

fn item_entity(pool: u32, stream: u32) -> (Entity, MainEntity) {
    let entity =
        Entity::from_raw_u32(0xFF00_0000 + pool * STREAMS + stream).expect("below u32::MAX");
    (entity, MainEntity::from(entity))
}

fn decode_asset_id(id: UntypedAssetId) -> (u32, u32) {
    match id {
        UntypedAssetId::Uuid { uuid, .. } => {
            let packed = uuid.as_u128() as u32;
            (packed / STREAMS, packed % STREAMS)
        }
        UntypedAssetId::Index { .. } => (0, STREAM_SOLID),
    }
}

pub trait TerrainItem: PhaseItem {
    fn pool_stream(&self) -> (u32, u32);
    /// Whether this is a water run, whose `pool_stream` pool is the run.
    fn water_run(&self) -> bool {
        false
    }
}

impl TerrainItem for Opaque3d {
    fn pool_stream(&self) -> (u32, u32) {
        decode_asset_id(self.bin_key.asset_id)
    }
}

impl TerrainItem for AlphaMask3d {
    fn pool_stream(&self) -> (u32, u32) {
        decode_asset_id(self.bin_key.asset_id)
    }
}

impl TerrainItem for Transparent3d {
    fn water_run(&self) -> bool {
        matches!(self.extra_index, PhaseItemExtraIndex::DynamicOffset(p) if p & WATER_RUN_TAG != 0)
    }

    fn pool_stream(&self) -> (u32, u32) {
        match self.extra_index {
            PhaseItemExtraIndex::DynamicOffset(packed) if packed & WATER_RUN_TAG != 0 => {
                (packed & !WATER_RUN_TAG, STREAM_WATER)
            }
            PhaseItemExtraIndex::DynamicOffset(packed) => (packed / STREAMS, packed % STREAMS),
            _ => (0, STREAM_WATER),
        }
    }
}

/// Marks a translucent item as one water run (`WaterRuns`) rather than a
/// whole pool; the low bits are the run's index.
pub const WATER_RUN_TAG: u32 = 1 << 30;

/// One stretch of water sections from the same pool, in the view's global
/// back-to-front order. Each run is its own translucent item at its own
/// depth, so water from different pools draws far to near across pools and
/// sorts among the other translucent things (sky, entities, clouds) as
/// vanilla's does. Within a pool's command region the runs sit back to
/// back, so a whole-pool draw (shader packs) still covers them all.
pub struct WaterRun {
    pub pool: u32,
    /// The view depth its sort uses (view-space z of its farthest section).
    pub distance: f32,
    pub slots: Vec<u32>,
}

#[derive(Resource, Default)]
pub struct WaterRuns(pub HashMap<RetainedViewEntity, Vec<WaterRun>>);

/// Water nearer than this (blocks, to the section's centre) draws in the
/// view's global back-to-front order. Farther water, which seldom overlaps
/// other water, draws a pool at a time before it: ordering the whole sea
/// split it into thousands of runs and halved the frame rate.
const WATER_ORDER_NEAR: f32 = 64.0;

fn water_runs(pools: &TerrainPools, view: &ExtractedView, runs: &mut Vec<WaterRun>) {
    let rangefinder = view.rangefinder3d();
    let eye = view.world_from_view.translation();
    let mut far: Vec<Option<WaterRun>> = Vec::new();
    let mut near: Vec<(f32, u32, u32)> = Vec::new();
    for &slot in &pools.water_slots {
        let meta = &pools.meta_cpu[slot as usize];
        let (lo, hi) = meta.world_box();
        let centre = (Vec3::from(lo) + Vec3::from(hi)) * 0.5;
        let depth = rangefinder.distance(&centre);
        if centre.distance(eye) < WATER_ORDER_NEAR {
            near.push((depth, meta.pool, slot));
            continue;
        }
        let pool = meta.pool as usize;
        if far.len() <= pool {
            far.resize_with(pool + 1, || None);
        }
        let run = far[pool].get_or_insert_with(|| WaterRun {
            pool: meta.pool,
            distance: depth,
            slots: Vec::new(),
        });
        run.distance = run.distance.min(depth);
        run.slots.push(slot);
    }
    runs.clear();
    runs.extend(far.into_iter().flatten());
    // The far pools first, each at its farthest section, then the near
    // runs; view z grows towards the camera, so ascending is far to near.
    runs.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    let floor = runs.last().map_or(f32::MIN, |r| r.distance);
    near.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
    for run in near.chunk_by(|a, b| a.1 == b.1) {
        runs.push(WaterRun {
            pool: run[0].1,
            // Never ahead of a far pool in Bevy's sort.
            distance: run[0].0.max(floor),
            slots: run.iter().map(|&(_, _, slot)| slot).collect(),
        });
    }
}

#[derive(Resource)]
pub struct TerrainParamsBuffer(pub Buffer);

#[derive(Resource, Default)]
pub struct TerrainBindGroup {
    pub group: Option<BindGroup>,
    key: Option<(TextureViewId, TextureViewId, TextureViewId, u64)>,
}

#[derive(Resource)]
pub struct TerrainPipeline {
    mesh_pipeline: MeshPipeline,
    shader: Handle<Shader>,
    vertex_layout_ref: MeshVertexBufferLayoutRef,
    layout_indirect: BindGroupLayoutDescriptor,
    layout_direct: BindGroupLayoutDescriptor,
}

#[derive(Resource, Default)]
pub struct PackTakeover(pub bool);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerrainPipelineKey {
    pub view: MeshPipelineKey,
    pub stream: u32,
    pub direct: bool,
    pub shadow: bool,
}

#[cfg(feature = "shader_support")]
pub const SLOT_TABLE_BINDING: u32 = 5;

fn terrain_layouts() -> (BindGroupLayoutDescriptor, BindGroupLayoutDescriptor) {
    let meta_size = NonZeroU64::new(size_of::<SlotMeta>() as u64);
    let common = (
        texture_2d(TextureSampleType::Float { filterable: true }),
        sampler(SamplerBindingType::Filtering),
        texture_2d(TextureSampleType::Float { filterable: true }),
        sampler(SamplerBindingType::Filtering),
        uniform_buffer::<Vec4>(false),
    );
    let indirect = BindGroupLayoutEntries::sequential(
        ShaderStages::VERTEX_FRAGMENT,
        (
            common.0,
            common.1,
            common.2,
            common.3,
            common.4,
            bevy::render::render_resource::binding_types::storage_buffer_read_only_sized(
                false, meta_size,
            ),
            texture_2d(TextureSampleType::Uint),
        ),
    );
    let direct = BindGroupLayoutEntries::sequential(
        ShaderStages::VERTEX_FRAGMENT,
        (
            common.0,
            common.1,
            common.2,
            common.3,
            common.4,
            texture_2d(TextureSampleType::Sint),
            texture_2d(TextureSampleType::Uint),
        ),
    );
    (
        BindGroupLayoutDescriptor::new("terrain_indirect", &indirect),
        BindGroupLayoutDescriptor::new("terrain_direct", &direct),
    )
}

impl FromWorld for TerrainPipeline {
    fn from_world(world: &mut World) -> Self {
        let shaders = world.resource::<TerrainShaders>().clone();
        let (layout_indirect, layout_direct) = terrain_layouts();
        let mut layouts = MeshVertexBufferLayouts::default();
        let vertex_layout_ref = layouts.insert(MeshVertexBufferLayout::new(
            vec![Mesh::ATTRIBUTE_POSITION.id],
            VertexBufferLayout {
                array_stride: 12,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            },
        ));
        Self {
            mesh_pipeline: world.resource::<MeshPipeline>().clone(),
            shader: shaders.terrain,
            vertex_layout_ref,
            layout_indirect,
            layout_direct,
        }
    }
}

impl TerrainPipeline {
    pub fn terrain_layout(&self, direct: bool) -> BindGroupLayoutDescriptor {
        if direct {
            self.layout_direct.clone()
        } else {
            self.layout_indirect.clone()
        }
    }

    fn terrain_defs(key: TerrainPipelineKey) -> Vec<ShaderDefVal> {
        let mut defs = Vec::new();
        if key.stream == STREAM_CUTOUT {
            defs.push("ALPHA_CUTOUT".into());
        }
        if key.stream == STREAM_WATER {
            defs.push("WATER".into());
        }
        if key.direct {
            defs.push("META_TEXTURE".into());
        }
        defs
    }

    fn specialize_main(&self, key: TerrainPipelineKey) -> RenderPipelineDescriptor {
        let mut view_key =
            key.view | MeshPipelineKey::from_primitive_topology(PrimitiveTopology::TriangleList);
        if key.stream == STREAM_WATER {
            view_key |= MeshPipelineKey::BLEND_ALPHA;
        }
        let mut descriptor =
            <MeshPipeline as bevy::render::render_resource::SpecializedMeshPipeline>::specialize(
                &self.mesh_pipeline,
                view_key,
                &self.vertex_layout_ref,
            )
            .expect("terrain vertex layout is missing ATTRIBUTE_POSITION");

        descriptor.label = Some(
            match key.stream {
                STREAM_SOLID => "terrain_solid",
                STREAM_CUTOUT => "terrain_cutout",
                _ => "terrain_water",
            }
            .into(),
        );
        descriptor.vertex.shader = self.shader.clone();
        descriptor.vertex.buffers = vec![packed_vertex_layout()];
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = self.shader.clone();
        }

        descriptor.layout.truncate(2);
        descriptor.layout.push(self.terrain_layout(key.direct));

        descriptor.primitive.cull_mode = Some(Face::Back);
        // Translucent terrain writes depth, as vanilla's does, so the clouds
        // and every translucent thing drawn after it are hidden behind it.
        // It draws in runs in the view's global back-to-front order
        // (`WaterRun`), so a near section never comes before a far one.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = true;
        }

        for def in Self::terrain_defs(key) {
            descriptor.vertex.shader_defs.push(def.clone());
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push(def);
            }
        }

        descriptor
    }
}

impl SpecializedRenderPipeline for TerrainPipeline {
    type Key = TerrainPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        self.specialize_main(key)
    }
}

pub struct SetTerrainBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetTerrainBindGroup<I> {
    type Param = SRes<TerrainBindGroup>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: (),
        _entity: Option<()>,
        bind_group: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(group) = bind_group.into_inner().group.as_ref() else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, group, &[]);
        RenderCommandResult::Success
    }
}

pub struct DrawTerrain;

impl<P: TerrainItem> RenderCommand<P> for DrawTerrain {
    type Param = (
        SRes<TerrainTier>,
        SRes<TerrainPools>,
        SRes<CullBuffers>,
        SRes<TerrainViews>,
        SRes<DirectLists>,
        SRes<WaterRuns>,
    );
    type ViewQuery = Read<ExtractedView>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view: &'w ExtractedView,
        _entity: Option<()>,
        (tier, pools, cull, views, lists, runs): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (pool_index, stream) = item.pool_stream();
        let sources = StreamSources {
            tier: tier.into_inner(),
            pools: pools.into_inner(),
            cull: cull.into_inner(),
            views: views.into_inner(),
            lists: lists.into_inner(),
        };
        let drawn = if item.water_run() {
            draw_water_run(
                pass,
                &sources,
                runs.into_inner(),
                view.retained_view_entity,
                pool_index,
            )
        } else {
            draw_region(
                pass,
                &sources,
                view.retained_view_entity,
                pool_index,
                stream,
            )
        };
        match drawn {
            Ok(()) => RenderCommandResult::Success,
            Err(DrawSkipped::NoPool) => RenderCommandResult::Skip,
            Err(DrawSkipped::NoCullSlot) => {
                RenderCommandResult::Failure("terrain view has no cull slot")
            }
        }
    }
}

pub struct StreamSources<'w> {
    pub tier: &'w TerrainTier,
    pub pools: &'w TerrainPools,
    pub cull: &'w CullBuffers,
    pub views: &'w TerrainViews,
    pub lists: &'w DirectLists,
}

/// Draws water run `run` of `view` (a `WATER_RUN_TAG` item).
fn draw_water_run<'w>(
    pass: &mut TrackedRenderPass<'w>,
    sources: &StreamSources<'w>,
    runs: &'w WaterRuns,
    view: RetainedViewEntity,
    run: u32,
) -> Result<(), DrawSkipped> {
    let Some(water) = runs.0.get(&view).and_then(|r| r.get(run as usize)) else {
        return Ok(());
    };
    let StreamSources {
        tier,
        pools,
        cull,
        views,
        ..
    } = *sources;
    let Some(pool) = pools.pools.get(water.pool as usize) else {
        return Err(DrawSkipped::NoPool);
    };
    pass.set_vertex_buffer(0, pool.vertices.slice(..));
    pass.set_index_buffer(pool.indices.slice(..), IndexFormat::Uint32);
    let mut draws = 0usize;
    if tier.indirect {
        let Some(&view_index) = views.index.get(&view) else {
            return Err(DrawSkipped::NoCullSlot);
        };
        let (start, count) = cull.water_run(view_index, run);
        if count > 0 {
            let region = ((view_index * cull.pool_count) + water.pool) * STREAMS + STREAM_WATER;
            let offset = (region as u64 * cull.region_cap() + start as u64) * INDIRECT_ARGS_SIZE;
            pass.multi_draw_indexed_indirect(&cull.commands, offset, count);
            draws = 1;
        }
    } else {
        for &slot in &water.slots {
            if !pools.is_visible(slot) {
                continue;
            }
            let (first, count) = pools.meta_cpu[slot as usize].stream_range(STREAM_WATER);
            if count > 0 {
                pass.draw_indexed(first..first + count, 0, 0..1);
                draws += 1;
            }
        }
    }
    if draws > 0 {
        pools
            .stats
            .0
            .draws_pending
            .fetch_add(draws, Ordering::Relaxed);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawSkipped {
    NoPool,
    NoCullSlot,
}

pub fn draw_region<'w>(
    pass: &mut TrackedRenderPass<'w>,
    sources: &StreamSources<'w>,
    view: RetainedViewEntity,
    pool_index: u32,
    stream: u32,
) -> Result<(), DrawSkipped> {
    let StreamSources {
        tier,
        pools,
        cull,
        views,
        lists,
    } = *sources;
    let Some(pool) = pools.pools.get(pool_index as usize) else {
        return Err(DrawSkipped::NoPool);
    };
    pass.set_vertex_buffer(0, pool.vertices.slice(..));
    pass.set_index_buffer(pool.indices.slice(..), IndexFormat::Uint32);

    if tier.indirect {
        let Some(&view_index) = views.index.get(&view) else {
            return Err(DrawSkipped::NoCullSlot);
        };
        let region = ((view_index * cull.pool_count) + pool_index) * STREAMS + stream;
        let commands_offset = region as u64 * cull.region_cap() * INDIRECT_ARGS_SIZE;
        // Each multi-draw counts as one call, however many sections it draws.
        let mut issued = false;
        if stream == STREAM_WATER {
            let count = cull.water_count(view_index, pool_index);
            if count > 0 {
                pass.multi_draw_indexed_indirect(&cull.commands, commands_offset, count);
                issued = true;
            }
        } else if tier.count {
            pass.multi_draw_indexed_indirect_count(
                &cull.commands,
                commands_offset,
                &cull.counts,
                region as u64 * 4,
                cull.region_cap() as u32,
            );
            issued = true;
        } else {
            // Each slot has `FACE_GROUPS` commands, unused ones zeroed.
            let draw_end = pools.draw_end.min(cull.slot_cap) * super::FACE_GROUPS;
            if draw_end > 0 {
                pass.multi_draw_indexed_indirect(&cull.commands, commands_offset, draw_end);
                issued = true;
            }
        }
        if issued {
            pools.stats.0.draws_pending.fetch_add(1, Ordering::Relaxed);
        }
        return Ok(());
    }

    let Some(slots) = lists
        .0
        .get(&view)
        .and_then(|per_view| per_view.get(stream as usize))
    else {
        return Ok(());
    };

    let mut run: Option<(u32, u32)> = None;
    let mut draws = 0usize;

    let pool_of = |slot: u32| pools.meta_cpu[slot as usize].pool;
    let start = slots.partition_point(|&slot| pool_of(slot) < pool_index);
    let end = start + slots[start..].partition_point(|&slot| pool_of(slot) == pool_index);

    for &slot in &slots[start..end] {
        let (first, count) = pools.meta_cpu[slot as usize].stream_range(stream);
        if count == 0 {
            continue;
        }
        match run {
            Some((start, end)) if end == first => {
                run = Some((start, first + count));
            }
            Some((start, end)) => {
                pass.draw_indexed(start..end, 0, 0..1);
                draws += 1;
                run = Some((first, first + count));
            }
            None => {
                run = Some((first, first + count));
            }
        }
    }

    if let Some((start, end)) = run {
        pass.draw_indexed(start..end, 0, 0..1);
        draws += 1;
    }
    if draws > 0 {
        pools
            .stats
            .0
            .draws_pending
            .fetch_add(draws, Ordering::Relaxed);
    }
    Ok(())
}

const INDIRECT_ARGS_SIZE: u64 = 20;

pub type DrawTerrainMain = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    SetTerrainBindGroup<TERRAIN_GROUP>,
    DrawTerrain,
);

type ViewLights = ();

#[allow(clippy::too_many_arguments)]
pub fn queue_terrain(
    tier: Res<TerrainTier>,
    pools: Res<TerrainPools>,
    terrain_pipeline: Res<TerrainPipeline>,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<TerrainPipeline>>,
    view_key_cache: Res<ViewKeyCache>,
    opaque_draws: Res<DrawFunctions<Opaque3d>>,
    alpha_draws: Res<DrawFunctions<AlphaMask3d>>,
    transparent_draws: Res<DrawFunctions<Transparent3d>>,
    mut opaque_phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    mut alpha_phases: ResMut<ViewBinnedRenderPhases<AlphaMask3d>>,
    mut transparent_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(&ExtractedView, Has<TerrainView>, ViewLights)>,
    mut water_runs_res: ResMut<WaterRuns>,
    ticks: SystemChangeTick,
    #[cfg(feature = "shader_support")] takeover: Option<Res<PackTakeover>>,
) {
    crate::prof_span!("render:terrain_queue_terrain");
    // Rebuilt for the views queued below; a view left out (a shader pack's)
    // writes its water a pool at a time.
    water_runs_res.0.clear();
    if pools.pools.is_empty() {
        return;
    }
    let direct = !tier.indirect;
    let change_tick = ticks.this_run();
    let transparent_draw = transparent_draws.read().id::<DrawTerrainMain>();

    let opaque_draw = opaque_draws.read().id::<DrawTerrainMain>();
    let alpha_draw = alpha_draws.read().id::<DrawTerrainMain>();

    #[cfg(feature = "shader_support")]
    if takeover.is_some_and(|t| t.0) {
        return;
    }

    for (view, is_terrain, lights) in views.iter() {
        if !is_terrain {
            continue;
        }
        let Some(&view_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };

        for pool in 0..pools.pools.len() as u32 {
            let mut specialize = |stream: u32, shadow: bool, key: MeshPipelineKey| {
                pipelines.specialize(
                    &pipeline_cache,
                    &terrain_pipeline,
                    TerrainPipelineKey {
                        view: key,
                        stream,
                        direct,
                        shadow,
                    },
                )
            };

            if let Some(phase) = opaque_phases.get_mut(&view.retained_view_entity) {
                phase.add(
                    Opaque3dBatchSetKey {
                        pipeline: specialize(STREAM_SOLID, false, view_key),
                        draw_function: opaque_draw,
                        material_bind_group_index: None,
                        vertex_slab: SlabId::default(),
                        index_slab: None,
                        lightmap_slab: None,
                    },
                    Opaque3dBinKey {
                        asset_id: item_asset_id(pool, STREAM_SOLID),
                    },
                    item_entity(pool, STREAM_SOLID),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                    change_tick,
                );
            }

            if let Some(phase) = alpha_phases.get_mut(&view.retained_view_entity) {
                phase.add(
                    OpaqueNoLightmap3dBatchSetKey {
                        pipeline: specialize(STREAM_CUTOUT, false, view_key),
                        draw_function: alpha_draw,
                        material_bind_group_index: None,
                        vertex_slab: SlabId::default(),
                        index_slab: None,
                    },
                    OpaqueNoLightmap3dBinKey {
                        asset_id: item_asset_id(pool, STREAM_CUTOUT),
                    },
                    item_entity(pool, STREAM_CUTOUT),
                    InputUniformIndex::default(),
                    BinnedRenderPhaseType::NonMesh,
                    change_tick,
                );
            }
        }

        let runs = water_runs_res
            .0
            .entry(view.retained_view_entity)
            .or_default();
        water_runs(&pools, view, runs);
        if let Some(phase) = transparent_phases.get_mut(&view.retained_view_entity) {
            let pipeline = pipelines.specialize(
                &pipeline_cache,
                &terrain_pipeline,
                TerrainPipelineKey {
                    view: view_key,
                    stream: STREAM_WATER,
                    direct,
                    shadow: false,
                },
            );
            for (index, run) in runs.iter().enumerate() {
                phase.add(Transparent3d {
                    distance: run.distance,
                    pipeline,
                    entity: item_entity(run.pool, STREAM_WATER),
                    draw_function: transparent_draw,
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::DynamicOffset(WATER_RUN_TAG | index as u32),
                    indexed: true,
                });
            }
        }

        let _ = lights;
    }
}

pub fn prepare_params(
    queue: Res<RenderQueue>,
    buffer: Res<TerrainParamsBuffer>,
    params: Option<Res<TerrainParams>>,
) {
    let params = params.as_deref().copied().unwrap_or_default();
    let values = [
        params.cutoff,
        params.mip_bias,
        params.shadow_mip,
        params.water_alpha,
    ];
    queue.write_buffer(&buffer.0, 0, bytemuck::bytes_of(&values));
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_terrain_bind_group(
    device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<TerrainPipeline>,
    pools: Res<TerrainPools>,
    textures: Option<Res<TerrainTextures>>,
    images: Res<RenderAssets<GpuImage>>,
    params: Res<TerrainParamsBuffer>,
    mut bind_group: ResMut<TerrainBindGroup>,
) {
    crate::prof_span!("render:terrain_prepare_terrain_bind_group");
    let Some(textures) = textures else { return };
    let (Some(atlas), Some(lightmap), Some(anim)) = (
        images.get(&textures.atlas),
        images.get(&textures.lightmap),
        images.get(&textures.anim),
    ) else {
        return;
    };

    let key = (
        atlas.texture_view.id(),
        lightmap.texture_view.id(),
        anim.texture_view.id(),
        pools.tables_generation,
    );
    if bind_group.key == Some(key) && bind_group.group.is_some() {
        return;
    }

    let group = match &pools.table {
        SlotTable::Indirect { meta, .. } => device.create_bind_group(
            "terrain",
            &pipeline_cache.get_bind_group_layout(&pipeline.layout_indirect),
            &BindGroupEntries::sequential((
                &atlas.texture_view,
                &atlas.sampler,
                &lightmap.texture_view,
                &lightmap.sampler,
                params.0.as_entire_binding(),
                meta.as_entire_binding(),
                &anim.texture_view,
            )),
        ),
        SlotTable::Direct { origins_view, .. } => device.create_bind_group(
            "terrain",
            &pipeline_cache.get_bind_group_layout(&pipeline.layout_direct),
            &BindGroupEntries::sequential((
                &atlas.texture_view,
                &atlas.sampler,
                &lightmap.texture_view,
                &lightmap.sampler,
                params.0.as_entire_binding(),
                origins_view,
                &anim.texture_view,
            )),
        ),
    };
    bind_group.group = Some(group);
    bind_group.key = Some(key);
}

fn init_terrain_pipeline(world: &mut World) {
    let pipeline = TerrainPipeline::from_world(world);
    world.insert_resource(pipeline);
}

pub fn build(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<SpecializedRenderPipelines<TerrainPipeline>>()
        .init_resource::<TerrainBindGroup>()
        .init_resource::<PackTakeover>()
        .add_render_command::<Opaque3d, DrawTerrainMain>()
        .add_render_command::<AlphaMask3d, DrawTerrainMain>()
        .add_render_command::<Transparent3d, DrawTerrainMain>();
    render_app.add_systems(
        Render,
        (
            prepare_params.in_set(RenderSystems::PrepareResources),
            prepare_terrain_bind_group.in_set(RenderSystems::PrepareBindGroups),
            queue_terrain.in_set(RenderSystems::Queue),
        ),
    );

    render_app.add_systems(RenderStartup, init_terrain_pipeline);
}

pub fn finish(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    let device = render_app.world().resource::<RenderDevice>().clone();
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("terrain_params"),
        size: 16,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    render_app.insert_resource(TerrainParamsBuffer(buffer));
}
