//! The viewer's test scene (VULKAN_REWRITE_CHECKLIST.md 2.9): a field of
//! textured tiles, culled and compacted on the GPU into an indirect draw,
//! with debug views. One scene exercises uploads, bindless textures and
//! buffers, compute, indirect count, depth, the graph's barriers and the
//! timestamps (`shaders/test_scene.slang`).

use ash::vk;
use bevy::math::{Mat4, Vec3};
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsDesc, Pipelines, Push, push};
use super::upload::Uploads;
use super::{Context, SetupError};

const GRID: u32 = 64;
const TILES: u32 = GRID * GRID;
const TEXTURE_SIZE: u32 = 64;
pub const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
const NEAR: f32 = 0.1;

/// What `vkview --view` shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Shaded,
    Normals,
    TextureIndex,
    Depth,
}

impl View {
    pub fn parse(name: &str) -> Option<View> {
        Some(match name {
            "shaded" => View::Shaded,
            "normals" => View::Normals,
            "texture" => View::TextureIndex,
            "depth" => View::Depth,
            _ => return None,
        })
    }

    fn mode(self) -> u32 {
        match self {
            View::Shaded => 0,
            View::Normals => 1,
            View::TextureIndex => 2,
            View::Depth => 3,
        }
    }
}

pub struct TestScene {
    module: vk::ShaderModule,
    cull: vk::Pipeline,
    draw: vk::Pipeline,
    depth_view: vk::Pipeline,
    tiles: Buffer,
    kept: Buffer,
    command: Buffer,
    count: Buffer,
    texture: Image,
    tiles_index: u32,
    kept_index: u32,
    command_index: u32,
    texture_index: u32,
    depth: Option<(Image, u32)>,
    uploaded: bool,
    color_format: vk::Format,
}

impl TestScene {
    pub fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        color_format: vk::Format,
    ) -> Result<TestScene, SetupError> {
        let module = pipelines.module(ctx, super::shaders::TEST_SCENE)?;
        let cull = pipelines.compute(ctx, module, c"cull")?;
        let draw = pipelines.graphics(
            ctx,
            &GraphicsDesc {
                module,
                vertex: c"tile_vertex",
                fragment: c"tile_fragment",
                color: &[color_format],
                depth: Some(DEPTH_FORMAT),
                depth_test: Some(vk::CompareOp::GREATER),
                depth_write: true,
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            },
        )?;
        let depth_view = pipelines.graphics(
            ctx,
            &GraphicsDesc {
                module,
                vertex: c"fullscreen_vertex",
                fragment: c"depth_fragment",
                color: &[color_format],
                depth: None,
                depth_test: None,
                depth_write: false,
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            },
        )?;
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST;
        let tiles = memory.buffer(
            ctx,
            "tiles",
            TILES as u64 * 16,
            storage,
            MemoryLocation::GpuOnly,
        )?;
        let kept = memory.buffer(
            ctx,
            "tiles kept",
            TILES as u64 * 16,
            storage,
            MemoryLocation::GpuOnly,
        )?;
        let indirect = storage | vk::BufferUsageFlags::INDIRECT_BUFFER;
        let command = memory.buffer(ctx, "tile command", 16, indirect, MemoryLocation::GpuOnly)?;
        let count = memory.buffer(ctx, "tile count", 4, indirect, MemoryLocation::GpuOnly)?;
        let texture = memory.image(
            ctx,
            "checker",
            vk::Extent2D {
                width: TEXTURE_SIZE,
                height: TEXTURE_SIZE,
            },
            vk::Format::R8G8B8A8_SRGB,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let full = || SetupError("the bindless set is full".to_owned());
        let tiles_index = bindless.add_buffer(ctx, tiles.raw).ok_or_else(full)?;
        let kept_index = bindless.add_buffer(ctx, kept.raw).ok_or_else(full)?;
        let command_index = bindless.add_buffer(ctx, command.raw).ok_or_else(full)?;
        let texture_index = bindless
            .add_texture(ctx, texture.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .ok_or_else(full)?;
        Ok(TestScene {
            module,
            cull,
            draw,
            depth_view,
            tiles,
            kept,
            command,
            count,
            texture,
            tiles_index,
            kept_index,
            command_index,
            texture_index,
            depth: None,
            uploaded: false,
            color_format,
        })
    }

    /// The depth image for this size, remade when the size changes.
    fn depth_for(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        extent: vk::Extent2D,
        frame: u64,
    ) -> Result<(), SetupError> {
        if self.depth.as_ref().is_some_and(|(d, _)| d.extent == extent) {
            return Ok(());
        }
        if let Some((old, index)) = self.depth.take() {
            // Resizes idle the device first, so the old image is unused.
            bindless.remove_texture(index, frame);
            memory.free_image(ctx, old);
        }
        let image = memory.image(
            ctx,
            "depth",
            extent,
            DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let index = bindless
            .add_texture(ctx, image.view, vk::ImageLayout::DEPTH_READ_ONLY_OPTIMAL)
            .ok_or_else(|| SetupError("the bindless set is full".to_owned()))?;
        self.depth = Some((image, index));
        Ok(())
    }

    /// Records the scene into `graph`, drawing to `color`.
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
        frame: u64,
        seconds: f32,
        view: View,
    ) -> Result<(), SetupError> {
        self.depth_for(ctx, memory, bindless, extent, frame)?;
        let (depth, depth_index) = self
            .depth
            .as_ref()
            .map(|(d, i)| ((d.raw, d.view), *i))
            .expect("made above");
        graph.image(
            depth.0,
            vk::ImageAspectFlags::DEPTH,
            vk::ImageLayout::UNDEFINED,
        );
        graph.image(
            self.texture.raw,
            vk::ImageAspectFlags::COLOR,
            if self.uploaded {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            } else {
                vk::ImageLayout::UNDEFINED
            },
        );
        let device = &ctx.device;

        if !self.uploaded {
            self.upload(ctx, uploads, graph)?;
        }

        graph.pass(
            "reset command",
            &[],
            &[
                (self.command.raw, Use::TransferDst),
                (self.count.raw, Use::TransferDst),
            ],
            |cmd| unsafe {
                // SAFETY: small inline updates into buffers this pass owns.
                device.cmd_update_buffer(
                    cmd,
                    self.command.raw,
                    0,
                    bytemuck::cast_slice(&[6u32, 0, 0, 0]),
                );
                device.cmd_update_buffer(cmd, self.count.raw, 0, bytemuck::cast_slice(&[1u32]));
            },
        );

        let eye = Vec3::new(
            (seconds * 0.3).sin() * 40.0,
            25.0,
            (seconds * 0.3).cos() * 40.0,
        );
        let aspect = extent.width as f32 / extent.height.max(1) as f32;
        let view_proj = Mat4::perspective_infinite_reverse_rh(70f32.to_radians(), aspect, NEAR)
            * Mat4::look_at_rh(eye, Vec3::new(0.0, 0.0, 0.0), Vec3::Y);

        graph.pass(
            "cull",
            &[],
            &[
                (self.tiles.raw, Use::ComputeRead),
                (self.kept.raw, Use::ComputeWrite),
                (self.command.raw, Use::ComputeWrite),
            ],
            |cmd| unsafe {
                // SAFETY: recording into the frame's command buffer with the
                // shared layout and the bindless set.
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
                        ids: [self.tiles_index, self.kept_index, self.command_index, 0],
                        more: [TILES, 0, 0, 0],
                        params: [eye.x, 0.0, eye.z, 45.0],
                        view_proj: [[0.0; 4]; 4],
                    },
                );
                device.cmd_dispatch(cmd, TILES.div_ceil(64), 1, 1);
            },
        );

        let viewport = |cmd| unsafe {
            // SAFETY: dynamic viewport and scissor; a negative height turns
            // Vulkan's downward y into the usual upward one.
            device.cmd_set_viewport(
                cmd,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: extent.height as f32,
                    width: extent.width as f32,
                    height: -(extent.height as f32),
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            device.cmd_set_scissor(
                cmd,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                }],
            );
        };

        graph.pass(
            "tiles",
            &[
                (color.0, Use::ColorAttachment),
                (depth.0, Use::DepthAttachment),
            ],
            &[
                (self.kept.raw, Use::VertexRead),
                (self.command.raw, Use::IndirectRead),
                (self.count.raw, Use::IndirectRead),
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
                    .image_view(depth.1)
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
                viewport(cmd);
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.draw);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
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
                        ids: [0, self.kept_index, 0, 0],
                        more: [self.texture_index, view.mode(), 0, 0],
                        params: [0.0; 4],
                        view_proj: view_proj.to_cols_array_2d(),
                    },
                );
                device.cmd_draw_indirect_count(cmd, self.command.raw, 0, self.count.raw, 0, 1, 16);
                device.cmd_end_rendering(cmd);
            },
        );

        if view == View::Depth {
            graph.pass(
                "depth view",
                &[
                    (depth.0, Use::SampledFragment),
                    (color.0, Use::ColorAttachment),
                ],
                &[],
                |cmd| unsafe {
                    // SAFETY: as above; the depth image is in its read-only
                    // layout, as registered.
                    let colors = [vk::RenderingAttachmentInfo::default()
                        .image_view(color.1)
                        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                        .load_op(vk::AttachmentLoadOp::DONT_CARE)
                        .store_op(vk::AttachmentStoreOp::STORE)];
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
                    viewport(cmd);
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.depth_view);
                    device.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
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
                            ids: [0; 4],
                            more: [depth_index, 0, 0, 0],
                            params: [NEAR, 30.0, 0.0, 0.0],
                            view_proj: [[0.0; 4]; 4],
                        },
                    );
                    device.cmd_draw(cmd, 3, 1, 0, 0);
                    device.cmd_end_rendering(cmd);
                },
            );
        }
        Ok(())
    }

    /// Stages the tiles and the checker texture and copies them in.
    fn upload(
        &mut self,
        ctx: &Context,
        uploads: &mut Uploads,
        graph: &mut Graph,
    ) -> Result<(), SetupError> {
        let mut tiles = Vec::with_capacity(TILES as usize * 4);
        for z in 0..GRID {
            for x in 0..GRID {
                let (fx, fz) = (x as f32 - GRID as f32 / 2.0, z as f32 - GRID as f32 / 2.0);
                let height = (fx * 0.25).sin() * (fz * 0.25).cos() * 4.0;
                tiles.extend_from_slice(&[fx * 2.0, height, fz * 2.0, 0.95]);
            }
        }
        let mut texels = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
        for y in 0..TEXTURE_SIZE {
            for x in 0..TEXTURE_SIZE {
                let light = ((x / 8) + (y / 8)) % 2 == 0;
                texels.extend_from_slice(if light {
                    &[230, 200, 120, 255]
                } else {
                    &[90, 140, 70, 255]
                });
            }
        }
        let busy = || SetupError("the staging ring is too small for the test scene".to_owned());
        let staged_tiles = uploads
            .stage(bytemuck::cast_slice(&tiles))
            .ok_or_else(busy)?;
        let staged_texels = uploads.stage(&texels).ok_or_else(busy)?;
        let device = &ctx.device;
        graph.pass(
            "upload",
            &[(self.texture.raw, Use::TransferDst)],
            &[(self.tiles.raw, Use::TransferDst)],
            |cmd| unsafe {
                // SAFETY: copies from this frame's staging slice into
                // resources in transfer layouts.
                device.cmd_copy_buffer(
                    cmd,
                    staged_tiles.buffer,
                    self.tiles.raw,
                    &[vk::BufferCopy {
                        src_offset: staged_tiles.offset,
                        dst_offset: 0,
                        size: staged_tiles.size,
                    }],
                );
                device.cmd_copy_buffer_to_image(
                    cmd,
                    staged_texels.buffer,
                    self.texture.raw,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy {
                        buffer_offset: staged_texels.offset,
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
                            width: TEXTURE_SIZE,
                            height: TEXTURE_SIZE,
                            depth: 1,
                        },
                    }],
                );
            },
        );
        graph.pass(
            "texture ready",
            &[(self.texture.raw, Use::SampledFragment)],
            &[],
            |_| {},
        );
        self.uploaded = true;
        Ok(())
    }

    pub fn destroy(self, ctx: &Context, memory: &Memory) {
        // SAFETY: the caller idles the device first.
        unsafe {
            ctx.device.destroy_pipeline(self.cull, None);
            ctx.device.destroy_pipeline(self.draw, None);
            ctx.device.destroy_pipeline(self.depth_view, None);
            ctx.device.destroy_shader_module(self.module, None);
        }
        memory.free_buffer(ctx, self.tiles);
        memory.free_buffer(ctx, self.kept);
        memory.free_buffer(ctx, self.command);
        memory.free_buffer(ctx, self.count);
        memory.free_image(ctx, self.texture);
        if let Some((depth, _)) = self.depth {
            memory.free_image(ctx, depth);
        }
        let _ = self.color_format;
    }
}
