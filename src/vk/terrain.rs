//! Terrain on the native renderer (VULKAN_REWRITE_CHECKLIST.md phase 3):
//! every section's vertices and indices in one shared pool each, a slot per
//! section, and a compute pass that culls the slots against the frustum
//! and writes the surviving draws into three indirect streams (solid,
//! cutout, water), drawn with `vkCmdDrawIndexedIndirectCount`. The vertex
//! shader pulls the mesher's 20-byte vertices from the pool itself
//! (`shaders/terrain.slang`).
//!
//! For now the sections come from a terrain dump (`terrain_dump`), loaded
//! once; streaming sections in and out of the pools comes with the game.

use ash::vk;
use bevy::math::{Mat4, Vec3};
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::depth_view::{DEPTH_FORMAT, DepthTarget, DepthView, set_viewport};
use super::frame::FRAMES_IN_FLIGHT;
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsDesc, Pipelines, Push, push};
use super::terrain_dump;
use super::test_scene::View;
use super::{Context, SetupError};

const NEAR: f32 = 0.05;
/// The draw streams the cull writes, in `shaders/terrain.slang`'s order.
const STREAMS: u64 = 3;
const COMMAND_SIZE: u64 = 20;

const SLOT_LIVE: u32 = 1;
const SLOT_WATER: u32 = 2;

/// One section's slot, matching `Slot` in `shaders/terrain.slang`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Slot {
    /// xyz the box's low corner from the origin.
    lo: [f32; 4],
    /// xyz the box's high corner.
    hi: [f32; 4],
    /// xyz the origin in blocks, w the base vertex.
    origin_base: [i32; 4],
    /// The first index, the index count, the solid (not cutout) index
    /// count, and flags.
    counts: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<Slot>() == 64);

/// Where `vkview --terrain` looks from: a position and a yaw and pitch in
/// degrees, as the game's camera (yaw 0 faces +z).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
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

/// What a loaded dump holds, for the viewer's summary.
pub struct Stats {
    pub sections: usize,
    pub vertices: u64,
    pub indices: u64,
}

pub struct TerrainScene {
    module: vk::ShaderModule,
    cull: vk::Pipeline,
    solid: vk::Pipeline,
    cutout: vk::Pipeline,
    water: vk::Pipeline,
    depth_view: DepthView,
    vertices: Buffer,
    indices: Buffer,
    slots: Buffer,
    commands: Buffer,
    counts: Buffer,
    atlas: Image,
    vertices_index: u32,
    slots_index: u32,
    commands_index: u32,
    counts_index: u32,
    atlas_index: u32,
    slot_count: u32,
    /// Everything to copy in, staged at load and freed once the frame that
    /// copied it is done; the frame it was copied in.
    staging: Option<(Buffer, Option<u64>)>,
    /// Byte offsets in `staging` of the indices, slots and atlas.
    staged_at: [u64; 3],
    atlas_size: (u32, u32),
    depth: DepthTarget,
    /// The middle of the loaded sections and the highest top, for the
    /// default camera.
    centre: Vec3,
    pub stats: Stats,
}

impl TerrainScene {
    /// Loads the dump in `dir` (`sections.bin` and `atlas.png`) and stages
    /// it for the first frame.
    pub fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        color_format: vk::Format,
        dir: &std::path::Path,
    ) -> Result<TerrainScene, SetupError> {
        let failed = |what: &str, e: &dyn std::fmt::Display| SetupError(format!("{what}: {e}"));
        let bytes = std::fs::read(dir.join("sections.bin"))
            .map_err(|e| failed("can't read sections.bin", &e))?;
        let sections = terrain_dump::read(&bytes).map_err(|e| failed("bad sections.bin", &e))?;
        drop(bytes);
        let atlas = image::open(dir.join("atlas.png"))
            .map_err(|e| failed("can't read atlas.png", &e))?
            .into_rgba8();
        let atlas_size = atlas.dimensions();

        // Lay the sections out: each one's vertices after the last, and its
        // indices with the cutout ones straight after the solid ones.
        let mut slots = Vec::with_capacity(sections.len());
        let (mut vertex_count, mut index_count) = (0u64, 0u64);
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for section in &sections {
            let (mut min, mut max) = ([i16::MAX; 3], [i16::MIN; 3]);
            for v in &section.verts {
                for axis in 0..3 {
                    min[axis] = min[axis].min(v.pos[axis]);
                    max[axis] = max[axis].max(v.pos[axis]);
                }
            }
            let to_blocks = |v: [i16; 3]| v.map(|c| c as f32 / 256.0);
            let (min, max) = (to_blocks(min), to_blocks(max));
            let solid = section.idx.len() as u32;
            let total = solid + section.cutout_idx.len() as u32;
            let first = index_count as u32;
            slots.push(Slot {
                lo: [min[0], min[1], min[2], 0.0],
                hi: [max[0], max[1], max[2], 0.0],
                origin_base: [
                    section.origin[0],
                    section.origin[1],
                    section.origin[2],
                    vertex_count as i32,
                ],
                counts: [
                    first,
                    total,
                    solid,
                    SLOT_LIVE | if section.water { SLOT_WATER } else { 0 },
                ],
            });
            let origin = Vec3::from(section.origin.map(|c| c as f32));
            lo = lo.min(origin);
            hi = hi.max(origin + Vec3::from(max));
            vertex_count += section.verts.len() as u64;
            index_count += total as u64;
        }
        let slot_count = slots.len() as u32;
        let centre = Vec3::new((lo.x + hi.x) / 2.0, hi.y, (lo.z + hi.z) / 2.0);

        let vertex_bytes = vertex_count * 20;
        let index_bytes = index_count * 4;
        let slot_bytes = slots.len() as u64 * 64;
        let atlas_bytes = atlas.as_raw().len() as u64;
        let staged_at = [
            vertex_bytes,
            vertex_bytes + index_bytes,
            vertex_bytes + index_bytes + slot_bytes,
        ];
        let mut staging = memory.buffer(
            ctx,
            "terrain staging",
            (staged_at[2] + atlas_bytes).max(4),
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        let (mut at_vertex, mut at_index) = (0usize, staged_at[0] as usize);
        for section in &sections {
            let verts: &[u8] = bytemuck::cast_slice(&section.verts);
            staging.write(at_vertex, verts);
            at_vertex += verts.len();
            for list in [&section.idx, &section.cutout_idx] {
                let list: &[u8] = bytemuck::cast_slice(list);
                staging.write(at_index, list);
                at_index += list.len();
            }
        }
        staging.write(staged_at[1] as usize, bytemuck::cast_slice(&slots));
        staging.write(staged_at[2] as usize, atlas.as_raw());
        let stats = Stats {
            sections: sections.len(),
            vertices: vertex_count,
            indices: index_count,
        };
        drop(sections);

        let module = pipelines.module(ctx, super::shaders::TERRAIN)?;
        let cull = pipelines.compute(ctx, module, c"cull")?;
        let stream = |fragment, cull, depth_write, blend| {
            pipelines.graphics(
                ctx,
                &GraphicsDesc {
                    module,
                    vertex: c"terrain_vertex",
                    fragment,
                    color: &[color_format],
                    depth: Some(DEPTH_FORMAT),
                    depth_test: Some(vk::CompareOp::GREATER),
                    depth_write,
                    cull,
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend,
                },
            )
        };
        // Faces aren't culled yet: the per-direction draws (3.3) do it for
        // whole groups instead.
        let none = vk::CullModeFlags::NONE;
        let solid = stream(c"terrain_solid", none, true, false)?;
        let cutout = stream(c"terrain_cutout", none, true, false)?;
        let water = stream(c"terrain_water", none, false, true)?;
        let depth_view = DepthView::new(ctx, pipelines, color_format)?;

        let storage = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST;
        let gpu = MemoryLocation::GpuOnly;
        let vertices = memory.buffer(ctx, "terrain vertices", vertex_bytes.max(4), storage, gpu)?;
        let indices = memory.buffer(
            ctx,
            "terrain indices",
            index_bytes.max(4),
            vk::BufferUsageFlags::INDEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            gpu,
        )?;
        let slots_buffer = memory.buffer(ctx, "terrain slots", slot_bytes.max(64), storage, gpu)?;
        let indirect = storage | vk::BufferUsageFlags::INDIRECT_BUFFER;
        let commands = memory.buffer(
            ctx,
            "terrain commands",
            (STREAMS * slot_count as u64 * COMMAND_SIZE).max(4),
            indirect,
            gpu,
        )?;
        let counts = memory.buffer(ctx, "terrain counts", 16, indirect, gpu)?;
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
        let full = || SetupError("the bindless set is full".to_owned());
        let vertices_index = bindless.add_buffer(ctx, vertices.raw).ok_or_else(full)?;
        let slots_index = bindless
            .add_buffer(ctx, slots_buffer.raw)
            .ok_or_else(full)?;
        let commands_index = bindless.add_buffer(ctx, commands.raw).ok_or_else(full)?;
        let counts_index = bindless.add_buffer(ctx, counts.raw).ok_or_else(full)?;
        let atlas_index = bindless
            .add_texture(
                ctx,
                atlas_image.view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            )
            .ok_or_else(full)?;
        Ok(TerrainScene {
            module,
            cull,
            solid,
            cutout,
            water,
            depth_view,
            vertices,
            indices,
            slots: slots_buffer,
            commands,
            counts,
            atlas: atlas_image,
            vertices_index,
            slots_index,
            commands_index,
            counts_index,
            atlas_index,
            slot_count,
            staging: Some((staging, None)),
            staged_at,
            atlas_size,
            depth: DepthTarget::default(),
            centre,
            stats,
        })
    }

    /// The camera when none is given: above the middle of the loaded
    /// terrain, looking down at it, turning slowly.
    pub fn default_camera(&self, seconds: f32) -> Camera {
        Camera {
            eye: self.centre + Vec3::new(0.0, 40.0, 0.0),
            yaw: seconds * 10.0,
            pitch: -30.0,
        }
    }

    /// Records the terrain into `graph`, drawing to `color`.
    #[allow(clippy::too_many_arguments, reason = "one frame's whole context")]
    pub fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        color: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        frame: u64,
        camera: Camera,
        view: View,
    ) -> Result<(), SetupError> {
        let (depth, depth_view, depth_index) = self
            .depth
            .get(ctx, memory, bindless, graph, extent, frame)?;
        let uploaded = match &self.staging {
            None => true,
            Some((_, copied)) => copied.is_some(),
        };
        graph.image(
            self.atlas.raw,
            vk::ImageAspectFlags::COLOR,
            if uploaded {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            } else {
                vk::ImageLayout::UNDEFINED
            },
        );
        // The staging buffer is free once the frame that copied from it has
        // come round again, past its fence.
        if let Some((_, Some(copied))) = &self.staging
            && frame >= copied + FRAMES_IN_FLIGHT as u64
            && let Some((staging, _)) = self.staging.take()
        {
            memory.free_buffer(ctx, staging);
        }
        if !uploaded {
            self.upload(ctx, graph, frame);
        }
        let device = &ctx.device;

        graph.pass(
            "reset counts",
            &[],
            &[(self.counts.raw, Use::TransferDst)],
            |cmd| unsafe {
                // SAFETY: a fill of a buffer this pass owns.
                device.cmd_fill_buffer(cmd, self.counts.raw, 0, 16, 0);
            },
        );

        let aspect = extent.width as f32 / extent.height.max(1) as f32;
        let view_proj = Mat4::perspective_infinite_reverse_rh(70f32.to_radians(), aspect, NEAR)
            * Mat4::look_to_rh(camera.eye, camera.forward(), Vec3::Y);
        let view_proj = view_proj.to_cols_array_2d();
        let capacity = self.slot_count;

        graph.pass(
            "terrain cull",
            &[],
            &[
                (self.slots.raw, Use::ComputeRead),
                (self.commands.raw, Use::ComputeWrite),
                (self.counts.raw, Use::ComputeWrite),
            ],
            |cmd| unsafe {
                // SAFETY: recording with the shared layout and the bindless
                // set; the push names live buffers.
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.cull);
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
                        ids: [self.slots_index, 0, self.commands_index, self.counts_index],
                        more: [self.slot_count, capacity, 0, 0],
                        params: [0.0; 4],
                        view_proj,
                    },
                );
                device.cmd_dispatch(cmd, self.slot_count.div_ceil(64).max(1), 1, 1);
            },
        );

        graph.pass(
            "terrain",
            &[
                (color.0, Use::ColorAttachment),
                (depth, Use::DepthAttachment),
                (self.atlas.raw, Use::SampledFragment),
            ],
            &[
                (self.vertices.raw, Use::VertexRead),
                (self.slots.raw, Use::VertexRead),
                (self.indices.raw, Use::IndexRead),
                (self.commands.raw, Use::IndirectRead),
                (self.counts.raw, Use::IndirectRead),
            ],
            |cmd| unsafe {
                // SAFETY: as above; the attachments are in the layouts the
                // graph just moved them to.
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
                device.cmd_bind_index_buffer(cmd, self.indices.raw, 0, vk::IndexType::UINT32);
                push(
                    ctx,
                    cmd,
                    pipelines.layout,
                    &Push {
                        ids: [self.slots_index, 0, self.vertices_index, 0],
                        more: [self.atlas_index, view.mode(), 0, 0],
                        params: [0.0; 4],
                        view_proj,
                    },
                );
                // Solid, then cutout, then water over both.
                for (stream, pipeline) in [self.solid, self.cutout, self.water]
                    .into_iter()
                    .enumerate()
                {
                    let stream = stream as u64;
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                    device.cmd_draw_indexed_indirect_count(
                        cmd,
                        self.commands.raw,
                        stream * capacity as u64 * COMMAND_SIZE,
                        self.counts.raw,
                        stream * 4,
                        capacity,
                        COMMAND_SIZE as u32,
                    );
                }
                device.cmd_end_rendering(cmd);
            },
        );

        if view == View::Depth {
            self.depth_view.record(
                ctx,
                bindless,
                pipelines,
                graph,
                (depth, depth_index),
                color,
                extent,
                NEAR,
                64.0,
            );
        }
        Ok(())
    }

    /// Copies the staged pools, slots and atlas in.
    fn upload(&mut self, ctx: &Context, graph: &mut Graph, frame: u64) {
        let Some((staging, copied)) = &mut self.staging else {
            return;
        };
        *copied = Some(frame);
        let source = staging.raw;
        let [indices_at, slots_at, atlas_at] = self.staged_at;
        let device = &ctx.device;
        let copy = |cmd, to: &Buffer, from: u64, size: u64| unsafe {
            // SAFETY: a copy from the staging buffer into a buffer in the
            // transfer state.
            if size > 0 {
                device.cmd_copy_buffer(
                    cmd,
                    source,
                    to.raw,
                    &[vk::BufferCopy {
                        src_offset: from,
                        dst_offset: 0,
                        size,
                    }],
                );
            }
        };
        graph.pass(
            "terrain upload",
            &[(self.atlas.raw, Use::TransferDst)],
            &[
                (self.vertices.raw, Use::TransferDst),
                (self.indices.raw, Use::TransferDst),
                (self.slots.raw, Use::TransferDst),
            ],
            |cmd| {
                copy(cmd, &self.vertices, 0, indices_at);
                copy(cmd, &self.indices, indices_at, slots_at - indices_at);
                copy(cmd, &self.slots, slots_at, atlas_at - slots_at);
                // SAFETY: as above, into the atlas in TRANSFER_DST.
                unsafe {
                    device.cmd_copy_buffer_to_image(
                        cmd,
                        source,
                        self.atlas.raw,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::BufferImageCopy {
                            buffer_offset: atlas_at,
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
                                width: self.atlas_size.0,
                                height: self.atlas_size.1,
                                depth: 1,
                            },
                        }],
                    );
                }
            },
        );
    }

    pub fn destroy(self, ctx: &Context, memory: &Memory) {
        // SAFETY: the caller idles the device first.
        unsafe {
            for pipeline in [self.cull, self.solid, self.cutout, self.water] {
                ctx.device.destroy_pipeline(pipeline, None);
            }
            ctx.device.destroy_shader_module(self.module, None);
        }
        self.depth_view.destroy(ctx);
        for buffer in [
            self.vertices,
            self.indices,
            self.slots,
            self.commands,
            self.counts,
        ] {
            memory.free_buffer(ctx, buffer);
        }
        if let Some((staging, _)) = self.staging {
            memory.free_buffer(ctx, staging);
        }
        memory.free_image(ctx, self.atlas);
        self.depth.destroy(ctx, memory);
    }
}
