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
//! moves quickly, so the test is off for any frame where the camera moved,
//! turned or zoomed (its field of view changed) more than a little since
//! the pyramid was built.

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
/// The field of view changed by more than this share (sprinting, flying
/// and freecam speed animate it): last frame's pyramid no longer lines up
/// with this frame's view, worst at the edges of a wide view.
const MAX_ZOOM: f32 = 0.002;

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
    /// The clip matrix this frame's cull tests through, when it tests, and
    /// whether its y runs down the screen (a pack's), for Culled sections.
    pub tested: Option<(Mat4, bool)>,
    /// `MC_HIZ_DUMP`: a readback of one pyramid level, now and then.
    dump: std::sync::Mutex<Dump>,
}

/// The pyramid dump (`MC_HIZ_DUMP=<folder>`): about once a second, level 1
/// is copied back and saved as a PNG beside the frame's sizes and camera,
/// so its alignment with the picture can be checked by eye.
#[derive(Default)]
struct Dump {
    frame: u64,
    /// A copy waiting to be read: buffer, width, height, padded row bytes,
    /// what to write beside it.
    waiting: Option<(wgpu::Buffer, u32, u32, u32, String)>,
    count: u32,
}

fn dump_folder() -> Option<std::path::PathBuf> {
    static FOLDER: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            crate::platform::env::var("MC_HIZ_DUMP")
                .ok()
                .map(std::path::PathBuf::from)
        })
        .clone()
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
    /// The projection's vertical focal length (1 / tan(fov / 2)).
    focal: f32,
}

struct Pipelines {
    from_depth: ComputePipeline,
    from_depth_layout: BindGroupLayout,
    /// Level 0 from a multisampled depth (MSAA).
    from_depth_ms: ComputePipeline,
    from_depth_ms_layout: BindGroupLayout,
    /// The debug view (`CullView`): a pyramid level drawn over the frame,
    /// by target format.
    view: std::sync::Mutex<Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>>,
    view_layout: wgpu::BindGroupLayout,
    view_module: wgpu::ShaderModule,
    /// Culled sections: the same, drawing a rectangle per hidden section.
    culled: std::sync::Mutex<Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>>,
    culled_layout: wgpu::BindGroupLayout,
    culled_module: wgpu::ShaderModule,
    reduce: ComputePipeline,
    reduce_layout: BindGroupLayout,
    /// The `reverse` parameter for each source: Bevy's (1) and a pack's (0).
    params_reversed: bevy::render::render_resource::Buffer,
    params_standard: bevy::render::render_resource::Buffer,
}

struct Pyramid {
    /// The size of the depth it was made for.
    source: UVec2,
    texture: bevy::render::render_resource::Texture,
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
            tested: None,
            dump: Default::default(),
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
        let focal = match source {
            Some(Source::Pack { projection, .. }) => projection.y_axis.y,
            _ => view.clip_from_view.y_axis.y,
        };
        let shot = Shot {
            clip,
            eye: view.world_from_view.translation(),
            forward: view.world_from_view.forward().as_vec3(),
            focal,
        };
        self.tested = None;
        let previous = std::mem::replace(&mut self.pending, source.map(|_| shot));
        let previous_source = std::mem::replace(&mut self.built_source, self.build);
        self.built = previous;
        self.build = source;
        let source = source?;
        let generation = self.generation;
        self.ensure(device, size.max(UVec2::ONE));
        // A pyramid made just now is all zeros (the nearest depth): tested
        // against it, nearly every section would count as hidden for a
        // frame (a window resize, a pack's render scale).
        if self.generation != generation {
            return None;
        }
        let built = self.built?;
        // A pyramid from the other source (a pack turned on or off) is
        // in the wrong depth convention and maybe the wrong size.
        if previous_source.map(Source::kind) != Some(source.kind()) {
            return None;
        }
        let still = built.eye.distance(shot.eye) <= MAX_MOVE
            && built.forward.dot(shot.forward) >= MIN_TURN_COS
            && (built.focal - shot.focal).abs() <= MAX_ZOOM * shot.focal.abs();
        let levels = self.pyramid.as_ref()?.levels;
        if still {
            self.tested = Some((built.clip, matches!(source, Source::Pack { .. })));
        }
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
            // Copied from by the dump (MC_HIZ_DUMP).
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
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
            texture: texture.clone(),
            levels,
            all: TextureView::from(texture.create_view(&wgpu::TextureViewDescriptor::default())),
            reduce_groups,
            level0: mip(0),
        });
        LEVELS.store(levels, std::sync::atomic::Ordering::Relaxed);
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
        let multisampled = depth.texture().sample_count() > 1;
        let (first_layout, first_pipeline, depth_binding) = if multisampled {
            (&pipelines.from_depth_ms_layout, &pipelines.from_depth_ms, 5)
        } else {
            (&pipelines.from_depth_layout, &pipelines.from_depth, 0)
        };
        let first = device.create_bind_group(
            "terrain_hiz_from_depth",
            first_layout,
            &[
                bevy::render::render_resource::BindGroupEntry {
                    binding: depth_binding,
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
        pass.set_pipeline(first_pipeline);
        pass.set_bind_group(0, &*first, &[]);
        pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
        pass.set_pipeline(&pipelines.reduce);
        for group in &pyramid.reduce_groups {
            size = (size / 2).max(UVec2::ONE);
            pass.set_bind_group(0, &**group, &[]);
            pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
        }
        drop(pass);
        if let Some(dir) = dump_folder() {
            self.dump_level(device, encoder, pyramid, &dir, pack);
        }
    }

    /// Saves last time's copy, and every 60 frames copies level 1 back.
    fn dump_level(
        &self,
        device: &RenderDevice,
        encoder: &mut wgpu::CommandEncoder,
        pyramid: &Pyramid,
        dir: &std::path::Path,
        pack: bool,
    ) {
        let Ok(mut dump) = self.dump.lock() else {
            return;
        };
        dump.frame += 1;
        if let Some((buffer, w, h, row, info)) = dump.waiting.take() {
            let slice = buffer.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            let _ = device
                .wgpu_device()
                .poll(wgpu::PollType::wait_indefinitely());
            let data = slice.get_mapped_range();
            let mut pixels = vec![0u8; (w * h) as usize];
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            let read = |x: u32, y: u32| {
                let at = (y * row + x * 4) as usize;
                f32::from_le_bytes(data[at..at + 4].try_into().unwrap())
            };
            for y in 0..h {
                for x in 0..w {
                    let d = read(x, y);
                    if d < 1.0 {
                        lo = lo.min(d);
                        hi = hi.max(d);
                    }
                }
            }
            for y in 0..h {
                for x in 0..w {
                    let d = read(x, y);
                    // Near bright, far dark, sky black; stretched over the
                    // range present.
                    let t = if d >= 1.0 {
                        0.0
                    } else {
                        1.0 - (d - lo) / (hi - lo).max(1e-6)
                    };
                    pixels[(y * w + x) as usize] = (40.0 + t * 215.0) as u8;
                }
            }
            drop(data);
            buffer.unmap();
            let _ = std::fs::create_dir_all(dir);
            let name = format!("hiz-{:03}", dump.count);
            let _ = image::save_buffer(
                dir.join(format!("{name}.png")),
                &pixels,
                w,
                h,
                image::ColorType::L8,
            );
            let _ = std::fs::write(
                dir.join(format!("{name}.txt")),
                format!("{info}\ndepth range {lo} .. {hi}\n"),
            );
            dump.count += 1;
        }
        if dump.frame % 60 != 0 {
            return;
        }
        let level = 1.min(pyramid.levels - 1);
        let mut w = (pyramid.source.x / 2).max(1);
        let mut h = (pyramid.source.y / 2).max(1);
        for _ in 0..level {
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        let row = (w * 4).next_multiple_of(256);
        let buffer = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain_hiz_dump"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &pyramid.texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let info = format!(
            "frame {} pack {pack} source {}x{} level {level} ({w}x{h})\npending clip {:?}\nbuilt clip {:?}",
            dump.frame,
            pyramid.source.x,
            pyramid.source.y,
            self.pending.as_ref().map(|s| s.clip.to_cols_array()),
            self.built.as_ref().map(|s| s.clip.to_cols_array()),
        );
        dump.waiting = Some((buffer, w, h, row, info));
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
        let from_depth_ms_layout = device.create_bind_group_layout(
            "terrain_hiz_from_depth_ms",
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
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
        let view_module = device
            .wgpu_device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("terrain_hiz_view"),
                source: wgpu::ShaderSource::Wgsl(include_str!("hiz_view.wgsl").into()),
            });
        let view_layout =
            device
                .wgpu_device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("terrain_hiz_view"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
        let culled_module =
            device
                .wgpu_device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("terrain_hiz_culled"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("hiz_culled.wgsl").into()),
                });
        let storage_read = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let culled_layout =
            device
                .wgpu_device()
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("terrain_hiz_culled"),
                    entries: &[
                        storage_read(0),
                        storage_read(1),
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::VERTEX,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
        Pipelines {
            culled: std::sync::Mutex::new(Vec::new()),
            culled_layout,
            culled_module,
            from_depth_ms: pipeline(&from_depth_ms_layout, "from_depth_ms"),
            from_depth_ms_layout,
            view: std::sync::Mutex::new(Vec::new()),
            view_layout,
            view_module,
            from_depth: pipeline(&from_depth_layout, "from_depth"),
            reduce: pipeline(&reduce_layout, "reduce"),
            from_depth_layout,
            reduce_layout,
            params_reversed: params(1),
            params_standard: params(0),
        }
    }
}

/// Occlusion culling switched off: `MC_NO_HIZ=1`, or Debug Options >
/// Occlusion Culling.
pub fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| crate::platform::env::var("MC_NO_HIZ").is_ok_and(|v| v == "1"))
        || !(ENABLED.load(std::sync::atomic::Ordering::Relaxed)
            || *ON.get_or_init(|| crate::platform::env::var("MC_HIZ").is_ok_and(|v| v == "1")))
}

/// Debug Options > Occlusion Culling, copied from the GUI; on by default.
pub static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Culling View's Culled sections mode (in `VIEW_LEVEL`).
pub const VIEW_CULLED: u32 = u32::MAX;

/// Debug Options > Culling View: 0 off, `VIEW_CULLED`, else 1 + the
/// pyramid level shown.
pub static VIEW_LEVEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The pyramid's level count, for the GUI's cycle (0 before one is built).
pub static LEVELS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[derive(RenderLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct HiZViewLabel;

/// Debug Options > Culling View: draws a level of the pyramid over the
/// frame, near light and far dark; texels the pyramid has nothing behind
/// (the sky) show dark blue. A section is culled when its nearest depth is
/// behind the texel covering it.
#[derive(Default)]
pub struct HiZViewNode;

impl ViewNode for HiZViewNode {
    type ViewQuery = (&'static bevy::render::view::ViewTarget, Has<TerrainView>);

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (target, is_terrain): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        // `MC_CULLING_VIEW=N` starts with level N - 1 shown (test runs).
        static FROM_ENV: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        let from_env = *FROM_ENV.get_or_init(|| {
            crate::platform::env::var("MC_CULLING_VIEW")
                .ok()
                .and_then(|v| match v.as_str() {
                    "culled" => Some(VIEW_CULLED),
                    v => v.parse().ok(),
                })
                .unwrap_or(0)
        });
        let shown = match VIEW_LEVEL.load(std::sync::atomic::Ordering::Relaxed) {
            0 => from_env,
            level => level,
        };
        if !is_terrain || shown == 0 {
            return Ok(());
        }
        let Some(hiz) = world.get_resource::<HiZ>() else {
            return Ok(());
        };
        let (Some(pipelines), Some(pyramid)) = (&hiz.pipelines, &hiz.pyramid) else {
            return Ok(());
        };
        if shown == VIEW_CULLED {
            return draw_culled(render_context, world, hiz, pipelines, target);
        }
        let level = (shown - 1).min(pyramid.levels - 1);
        let format = target.main_texture_format();
        let device = render_context.render_device().clone();
        let pipeline = {
            let mut cache = pipelines.view.lock().unwrap();
            match cache.iter().find(|(f, _)| *f == format) {
                Some((_, p)) => p.clone(),
                None => {
                    let p = pipelines.view_pipeline(&device, format);
                    cache.push((format, p.clone()));
                    p
                }
            }
        };
        let level_view = pyramid.texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("terrain_hiz_view_level"),
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let params = device.create_buffer_with_data(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain_hiz_view_params"),
            contents: bytemuck::cast_slice(&[
                target.main_texture().width() as f32,
                target.main_texture().height() as f32,
                // A shader pack's depth (and so its pyramid) is upside down.
                if matches!(hiz.built_source, Some(Source::Pack { .. })) {
                    1.0
                } else {
                    0.0
                },
                0.0,
            ]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device
            .wgpu_device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("terrain_hiz_view"),
                layout: &pipelines.view_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&level_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: params.as_entire_binding(),
                    },
                ],
            });
        let mut pass =
            render_context
                .command_encoder()
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("terrain_hiz_view"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target.main_texture_view(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

impl Pipelines {
    fn view_pipeline(
        &self,
        device: &RenderDevice,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let layout = device
            .wgpu_device()
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("terrain_hiz_view"),
                bind_group_layouts: &[&self.view_layout],
                push_constant_ranges: &[],
            });
        device
            .wgpu_device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("terrain_hiz_view"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &self.view_module,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.view_module,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
    }
}

#[derive(RenderLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct HiZLabel;

/// Builds this frame's pyramid from the camera's depth, after the opaque pass
/// (before translucent water, which writes depth but hides nothing).
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
        if hiz.build != Some(Source::Bevy) {
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

/// Culled sections: a rectangle per section this frame's test hid, over the
/// finished frame.
fn draw_culled<'w>(
    render_context: &mut RenderContext<'w>,
    world: &'w World,
    hiz: &HiZ,
    pipelines: &Pipelines,
    target: &bevy::render::view::ViewTarget,
) -> Result<(), NodeRunError> {
    let Some((clip, flip)) = hiz.tested else {
        return Ok(());
    };
    let pools = world.resource::<super::pools::TerrainPools>();
    let super::pools::SlotTable::Indirect { meta, .. } = &pools.table else {
        return Ok(());
    };
    let cull = world.resource::<super::cull::CullBuffers>();
    let (counts, bits, slots) = cull.culled_bits();
    let sections = pools.draw_end.min(slots);
    if sections == 0 {
        return Ok(());
    }
    let format = target.main_texture_format();
    let device = render_context.render_device().clone();
    let pipeline = {
        let mut cache = pipelines.culled.lock().unwrap();
        match cache.iter().find(|(f, _)| *f == format) {
            Some((_, p)) => p.clone(),
            None => {
                let p = pipelines.culled_pipeline(&device, format);
                cache.push((format, p.clone()));
                p
            }
        }
    };
    let mut values = clip.to_cols_array().to_vec();
    values.extend([
        f32::from_bits(bits),
        f32::from_bits(u32::from(flip)),
        f32::from_bits(u32::from(!flip)),
        0.0,
    ]);
    let Some(pyramid) = &hiz.pyramid else {
        return Ok(());
    };
    let params = device.create_buffer_with_data(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain_hiz_culled_params"),
        contents: bytemuck::cast_slice(&values),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let group = device
        .wgpu_device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain_hiz_culled"),
            layout: &pipelines.culled_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: meta.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: counts.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&pyramid.level0),
                },
            ],
        });
    let mut pass =
        render_context
            .command_encoder()
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain_hiz_culled"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.main_texture_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..6, 0..sections);
    Ok(())
}

impl Pipelines {
    fn culled_pipeline(
        &self,
        device: &RenderDevice,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let layout = device
            .wgpu_device()
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("terrain_hiz_culled"),
                bind_group_layouts: &[&self.culled_layout],
                push_constant_ranges: &[],
            });
        device
            .wgpu_device()
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("terrain_hiz_culled"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &self.culled_module,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.culled_module,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
    }
}
