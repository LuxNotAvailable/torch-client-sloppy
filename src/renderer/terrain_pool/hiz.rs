//! Occlusion culling for terrain: a depth pyramid ("Hi-Z").
//!
//! After the main pass, the camera's depth is reduced into a mip chain where
//! each texel holds the farthest depth of the area it covers (`hiz.wgsl`).
//! Next frame the GPU cull tests each section's box against it, seen from
//! the camera of the frame the pyramid came from (`hiz_occluded` in
//! `cull.wgsl`): a box whose nearest point lies behind everything over its
//! area is skipped, so the terrain behind a hill isn't drawn.
//!
//! With a shader pack the pyramid comes from the pack's own depth, built in
//! the pack's frame after its opaque terrain, and seen through the pack's
//! projection: GL depth (remapped to 0..1 here) drawn upside down, both
//! folded into the matrix the cull uses.
//!
//! The pyramid is a frame old. A section that comes into view from behind
//! an occluder is drawn a frame late, which shows when the camera turns or
//! moves quickly, so the test is off for any frame where the camera moved
//! more than a little since the pyramid was built.

use bevy::ecs::query::{Has, QueryItem};
use bevy::math::{Mat4, UVec2, Vec3};
use bevy::prelude::*;
use bevy::render::render_graph::{NodeRunError, RenderGraphContext, RenderLabel, ViewNode};
use bevy::render::render_resource::{BindGroup, BindGroupLayout, ComputePipeline, TextureView};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue};
use bevy::render::view::{ExtractedView, ViewDepthTexture};

use super::TerrainView;

/// How far the camera may move, in blocks, or turn, as the cosine of the
/// angle between its old and new forward directions, between the pyramid's
/// frame and the frame that uses it.
const MAX_MOVE: f32 = 1.0;
const MIN_TURN_COS: f32 = 0.9986; // about 3 degrees

/// The pyramid and what the cull needs to use it.
#[derive(Resource)]
pub struct HiZ {
    pipelines: Option<Pipelines>,
    pyramid: Option<Pyramid>,
    dummy: TextureView,
    /// Bumped when the pyramid's texture is remade, to rebuild bind groups.
    pub generation: u64,
    /// The camera of the frame the current pyramid holds (clip matrix, eye,
    /// forward), once one has been built.
    built: Option<Shot>,
    /// This frame's camera, which the pyramid built this frame will hold.
    pending: Option<Shot>,
    /// Which depth this frame's pyramid is built from, if any.
    pub build: Option<Source>,
    /// The source the current pyramid was built from.
    built_source: Option<Source>,
}

/// Where the pyramid's depth comes from.
#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    /// Bevy's main pass depth: reversed, 1 near, 0 far.
    Bevy,
    /// A shader pack's depth, through its projection (GL depth, drawn
    /// upside down), at its own size.
    Pack { projection: Mat4, size: UVec2 },
}

impl Source {
    fn kind(self) -> u8 {
        match self {
            Source::Bevy => 0,
            Source::Pack { .. } => 1,
        }
    }
}

#[derive(Clone, Copy)]
struct Shot {
    clip: Mat4,
    eye: Vec3,
    forward: Vec3,
}

struct Pipelines {
    from_depth: ComputePipeline,
    from_depth_layout: BindGroupLayout,
    reduce: ComputePipeline,
    reduce_layout: BindGroupLayout,
    /// The `reverse` parameter for each source: Bevy's (1) and a pack's (0).
    params_reversed: bevy::render::render_resource::Buffer,
    params_standard: bevy::render::render_resource::Buffer,
}

struct Pyramid {
    /// The size of the depth it was made for.
    source: UVec2,
    levels: u32,
    all: TextureView,
    reduce_groups: Vec<BindGroup>,
    level0: TextureView,
}

/// What the cull uses this frame: the clip matrix and level count, when the
/// test is on.
pub struct Use {
    pub clip: Mat4,
    pub levels: u32,
    /// Whether depth through `clip` is reversed (Bevy's).
    pub reverse: bool,
}

impl HiZ {
    pub fn new(device: &RenderDevice, queue: &RenderQueue) -> HiZ {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_hiz_dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            bytemuck::bytes_of(&1.0f32),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        HiZ {
            pipelines: None,
            pyramid: None,
            dummy: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            generation: 0,
            built: None,
            pending: None,
            build: None,
            built_source: None,
        }
    }

    /// The pyramid's whole mip chain, or a far 1×1 stand-in.
    pub fn view(&self) -> &TextureView {
        self.pyramid.as_ref().map_or(&self.dummy, |p| &p.all)
    }

    /// Readies this frame: returns what the cull may use (last frame's
    /// pyramid, if the camera has barely moved since), and sizes the
    /// pyramid for this frame's camera, which the node then fills.
    pub fn begin(
        &mut self,
        device: &RenderDevice,
        view: &ExtractedView,
        source: Option<Source>,
    ) -> Option<Use> {
        let view_from_world = view.world_from_view.to_matrix().inverse();
        let (clip, size) = match source {
            Some(Source::Pack { projection, size }) => {
                // GL depth to 0..1, and the pack's upside-down rows.
                let remap = Mat4::from_cols_array(&[
                    1.0, 0.0, 0.0, 0.0, //
                    0.0, -1.0, 0.0, 0.0, //
                    0.0, 0.0, 0.5, 0.0, //
                    0.0, 0.0, 0.5, 1.0,
                ]);
                (remap * projection * view_from_world, size)
            }
            _ => (
                view.clip_from_world
                    .unwrap_or_else(|| view.clip_from_view * view_from_world),
                UVec2::new(view.viewport.z, view.viewport.w),
            ),
        };
        let shot = Shot {
            clip,
            eye: view.world_from_view.translation(),
            forward: view.world_from_view.forward().as_vec3(),
        };
        let previous = std::mem::replace(&mut self.pending, source.map(|_| shot));
        let previous_source = std::mem::replace(&mut self.built_source, self.build);
        self.built = previous;
        self.build = source;
        let source = source?;
        self.ensure(device, size.max(UVec2::ONE));
        let built = self.built?;
        // A pyramid from the other source (a pack turned on or off) is
        // in the wrong depth convention and maybe the wrong size.
        if previous_source.map(Source::kind) != Some(source.kind()) {
            return None;
        }
        let still = built.eye.distance(shot.eye) <= MAX_MOVE
            && built.forward.dot(shot.forward) >= MIN_TURN_COS;
        let levels = self.pyramid.as_ref()?.levels;
        still.then_some(Use {
            clip: built.clip,
            levels,
            reverse: source == Source::Bevy,
        })
    }

    fn ensure(&mut self, device: &RenderDevice, source: UVec2) {
        if self.pipelines.is_none() {
            self.pipelines = Some(Pipelines::new(device));
        }
        if self.pyramid.as_ref().is_some_and(|p| p.source == source) {
            return;
        }
        let pipelines = self.pipelines.as_ref().expect("made above");
        let level0 = (source / 2).max(UVec2::ONE);
        let levels = 32 - level0.x.max(level0.y).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain_hiz"),
            size: wgpu::Extent3d {
                width: level0.x,
                height: level0.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let mip = |level: u32| {
            TextureView::from(texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            }))
        };
        let reduce_groups = (1..levels)
            .map(|level| {
                device.create_bind_group(
                    "terrain_hiz_reduce",
                    &pipelines.reduce_layout,
                    &[
                        bevy::render::render_resource::BindGroupEntry {
                            binding: 3,
                            resource: bevy::render::render_resource::BindingResource::TextureView(
                                &mip(level - 1),
                            ),
                        },
                        bevy::render::render_resource::BindGroupEntry {
                            binding: 4,
                            resource: bevy::render::render_resource::BindingResource::TextureView(
                                &mip(level),
                            ),
                        },
                    ],
                )
            })
            .collect();
        self.pyramid = Some(Pyramid {
            source,
            levels,
            all: TextureView::from(texture.create_view(&wgpu::TextureViewDescriptor::default())),
            reduce_groups,
            level0: mip(0),
        });
        self.generation += 1;
    }

    /// Builds the pyramid from `depth`, if this frame's source is `kind`.
    pub fn record(
        &self,
        device: &RenderDevice,
        encoder: &mut wgpu::CommandEncoder,
        depth: &TextureView,
        pack: bool,
    ) {
        let (Some(pipelines), Some(pyramid), Some(source)) =
            (&self.pipelines, &self.pyramid, self.build)
        else {
            return;
        };
        if matches!(source, Source::Pack { .. }) != pack {
            return;
        }
        let params = if pack {
            &pipelines.params_standard
        } else {
            &pipelines.params_reversed
        };
        let first = device.create_bind_group(
            "terrain_hiz_from_depth",
            &pipelines.from_depth_layout,
            &[
                bevy::render::render_resource::BindGroupEntry {
                    binding: 0,
                    resource: bevy::render::render_resource::BindingResource::TextureView(depth),
                },
                bevy::render::render_resource::BindGroupEntry {
                    binding: 1,
                    resource: bevy::render::render_resource::BindingResource::TextureView(
                        &pyramid.level0,
                    ),
                },
                bevy::render::render_resource::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
            ],
        );
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("terrain_hiz"),
            timestamp_writes: None,
        });
        let mut size = (pyramid.source / 2).max(UVec2::ONE);
        pass.set_pipeline(&pipelines.from_depth);
        pass.set_bind_group(0, &*first, &[]);
        pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
        pass.set_pipeline(&pipelines.reduce);
        for group in &pyramid.reduce_groups {
            size = (size / 2).max(UVec2::ONE);
            pass.set_bind_group(0, &**group, &[]);
            pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
        }
    }
}

impl Pipelines {
    fn new(device: &RenderDevice) -> Pipelines {
        let module = device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain_hiz"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hiz.wgsl").into()),
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::R32Float,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let from_depth_layout = device.create_bind_group_layout(
            "terrain_hiz_from_depth",
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                storage(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        );
        let reduce_layout = device.create_bind_group_layout(
            "terrain_hiz_reduce",
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                storage(4),
            ],
        );
        let pipeline = |layout: &BindGroupLayout, entry: &str| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("terrain_hiz"),
                bind_group_layouts: &[layout],
                push_constant_ranges: &[],
            });
            ComputePipeline::from(device.wgpu_device().create_compute_pipeline(
                &wgpu::ComputePipelineDescriptor {
                    label: Some("terrain_hiz"),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                },
            ))
        };
        let params = |reverse: u32| {
            device.create_buffer_with_data(&wgpu::util::BufferInitDescriptor {
                label: Some("terrain_hiz_params"),
                contents: bytemuck::cast_slice(&[reverse, 0, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };
        Pipelines {
            from_depth: pipeline(&from_depth_layout, "from_depth"),
            reduce: pipeline(&reduce_layout, "reduce"),
            from_depth_layout,
            reduce_layout,
            params_reversed: params(1),
            params_standard: params(0),
        }
    }
}

/// `MC_NO_HIZ=1` turns occlusion culling off, to compare.
pub fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| crate::platform::env::var("MC_NO_HIZ").is_ok_and(|v| v == "1"))
}

#[derive(RenderLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct HiZLabel;

/// Builds this frame's pyramid from the camera's depth, after the main pass.
#[derive(Default)]
pub struct HiZNode;

impl ViewNode for HiZNode {
    type ViewQuery = (&'static ViewDepthTexture, Has<TerrainView>);

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (depth, is_terrain): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        if !is_terrain {
            return Ok(());
        }
        let Some(hiz) = world.get_resource::<HiZ>() else {
            return Ok(());
        };
        if hiz.build != Some(Source::Bevy) || depth.texture.sample_count() > 1 {
            return Ok(());
        }
        crate::prof_span!("render:terrain_hiz");
        let device = render_context.render_device().clone();
        hiz.record(
            &device,
            render_context.command_encoder(),
            depth.view(),
            false,
        );
        Ok(())
    }
}

#[cfg(all(test, feature = "shader_support"))]
mod tests {
    #[test]
    fn the_pyramid_shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("hiz.wgsl")).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }
}
