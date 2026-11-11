//! The native renderer in the game (VULKAN_REWRITE_CHECKLIST.md phase 8),
//! on with `MC_RENDERER=native`: Bevy's renderer starts with no backend,
//! and this plugin takes the primary window, draws each frame and presents.
//!
//! Step 1: the GUI (`gui::pool`'s painter output) over a clear colour. The
//! world follows, fed live the way `vkview` reads its snapshots.

use std::mem::ManuallyDrop;
use std::sync::OnceLock;

use ash::vk;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RawHandleWrapper};
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::frame::{FRAMES_IN_FLIGHT, FrameError, Frames};
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsDesc, Pipelines, Push, push};
use super::{Context, SetupError, Swapchain};

/// Whether this run draws with the native renderer (`MC_RENDERER=native`).
pub fn wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| std::env::var("MC_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("native")))
}

pub struct LivePlugin;

impl Plugin for LivePlugin {
    fn build(&self, app: &mut App) {
        if !wanted() {
            return;
        }
        app.add_systems(Last, draw_frame);
    }
}

/// Everything the renderer owns; main-thread only (the window's surface).
/// Device children are `ManuallyDrop`, freed in order in `Drop` before
/// `ctx` goes.
struct Live {
    ctx: Context,
    swapchain: Option<Swapchain>,
    frames: ManuallyDrop<Frames>,
    memory: ManuallyDrop<Memory>,
    bindless: ManuallyDrop<Bindless>,
    pipelines: ManuallyDrop<Pipelines>,
    gui: ManuallyDrop<GuiPass>,
    frame: u64,
    size: (u32, u32),
    stale: bool,
}

fn setup(world: &mut World) -> Result<Live, SetupError> {
    let mut windows = world.query_filtered::<(&RawHandleWrapper, &Window), With<PrimaryWindow>>();
    let (handles, window) = windows
        .iter(world)
        .next()
        .ok_or_else(|| SetupError("no window yet".into()))?;
    let size = (window.physical_width(), window.physical_height());
    let ctx = Context::new(Some((handles.get_display_handle(), handles.get_window_handle())), None)?;
    info!(
        "native renderer: {} ({:?}), Vulkan {}.{}.{}",
        ctx.info.name, ctx.info.kind, ctx.info.api.0, ctx.info.api.1, ctx.info.api.2
    );
    let swapchain = Swapchain::new(&ctx, size, true, None)?;
    let frames = Frames::new(&ctx)?;
    let memory = Memory::new(&ctx)?;
    let mut bindless = Bindless::new(&ctx)?;
    let cache = std::env::temp_dir().join("torch-native-pipelines.bin");
    let pipelines = Pipelines::new(&ctx, bindless.layout, Some(cache))?;
    let format = swapchain
        .as_ref()
        .map_or(vk::Format::B8G8R8A8_SRGB, |s| s.format.format);
    let gui = GuiPass::new(&ctx, &memory, &mut bindless, &pipelines, format)?;
    Ok(Live {
        ctx,
        swapchain,
        frames: ManuallyDrop::new(frames),
        memory: ManuallyDrop::new(memory),
        bindless: ManuallyDrop::new(bindless),
        pipelines: ManuallyDrop::new(pipelines),
        gui: ManuallyDrop::new(gui),
        frame: 0,
        size,
        stale: false,
    })
}

fn draw_frame(world: &mut World) {
    if world.get_non_send_resource::<Live>().is_none() {
        match setup(world) {
            Ok(live) => world.insert_non_send_resource(live),
            Err(e) => {
                // No window yet is normal on the first frames; the rest is fatal.
                if e.0 != "no window yet" {
                    error!("native renderer: {}", e.0);
                    std::process::exit(3);
                }
                return;
            }
        }
    }
    let size = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map(|w| (w.physical_width(), w.physical_height()));
    let gui_frame = world
        .get_resource::<crate::gui::pool::GuiFrames>()
        .map(|f| std::mem::take(&mut *f.0.lock()));
    let textures = gui_textures(world);
    let Some(mut live) = world.remove_non_send_resource::<Live>() else {
        return;
    };
    if let Some(size) = size
        && size != live.size
    {
        live.size = size;
        live.stale = true;
    }
    live.draw(gui_frame.unwrap_or_default(), textures);
    world.insert_non_send_resource(live);
}

/// The GUI's textures as the main world holds them: atlas, unihex font.
fn gui_textures(world: &World) -> Option<[(u32, u32, vk::Format, Vec<u8>); 2]> {
    let handles = world.get_resource::<crate::gui::pool::GuiTextures>()?;
    let images = world.get_resource::<Assets<bevy::image::Image>>()?;
    let pick = |h: &Handle<bevy::image::Image>| -> Option<(u32, u32, vk::Format, Vec<u8>)> {
        use bevy::render::render_resource::TextureFormat as F;
        let image = images.get(h)?;
        let format = match image.texture_descriptor.format {
            F::Rgba8UnormSrgb => vk::Format::R8G8B8A8_SRGB,
            F::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
            F::R8Unorm => vk::Format::R8_UNORM,
            _ => return None,
        };
        let size = image.texture_descriptor.size;
        Some((size.width, size.height, format, image.data.clone()?))
    };
    Some([pick(&handles.atlas)?, pick(&handles.unihex)?])
}

impl Drop for Live {
    fn drop(&mut self) {
        self.ctx.idle();
        // SAFETY: each field is taken once, here, and not used after.
        unsafe {
            let memory = ManuallyDrop::take(&mut self.memory);
            ManuallyDrop::take(&mut self.gui).destroy(&self.ctx, &memory);
            ManuallyDrop::take(&mut self.pipelines).destroy(&self.ctx);
            ManuallyDrop::take(&mut self.bindless).destroy(&self.ctx);
            if let Some(s) = self.swapchain.take() {
                s.destroy(&self.ctx);
            }
            ManuallyDrop::take(&mut self.frames).destroy(&self.ctx);
            // The allocator's memory goes before the device.
            drop(memory);
        }
    }
}

impl Live {
    fn recreate(&mut self) {
        self.ctx.idle();
        match Swapchain::new(&self.ctx, self.size, true, self.swapchain.as_ref()) {
            Ok(Some(new)) => {
                if let Some(old) = self.swapchain.replace(new) {
                    old.destroy(&self.ctx);
                }
                self.stale = false;
            }
            // No area (minimised): try again next frame.
            Ok(None) => {}
            Err(e) => error!("native renderer: swapchain: {}", e.0),
        }
    }

    fn draw(&mut self, gui: crate::gui::pool::GuiFrame, textures: Option<[(u32, u32, vk::Format, Vec<u8>); 2]>) {
        if self.stale {
            self.recreate();
            if self.stale {
                return;
            }
        }
        let Some(swapchain) = &self.swapchain else {
            return;
        };
        let recording = match self.frames.begin(&self.ctx, swapchain) {
            Ok(r) => r,
            Err(FrameError::OutOfDate) => {
                self.stale = true;
                return;
            }
            Err(FrameError::Failed(e)) => {
                error!("native renderer: frame failed: {e:?}");
                if e == vk::Result::ERROR_DEVICE_LOST {
                    std::process::exit(3);
                }
                return;
            }
        };
        let slot = self.frames.index;
        self.bindless.recycle(self.frame);
        let image = swapchain.images[recording.image_index as usize];
        let view = swapchain.views[recording.image_index as usize];
        let extent = swapchain.extent;
        let mut graph = Graph::new(&self.ctx, recording.commands, None);
        graph.image(image, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
        if let Err(e) = self.gui.record(
            &self.ctx,
            &self.memory,
            &mut self.bindless,
            &self.pipelines,
            &mut graph,
            slot,
            (image, view, extent),
            gui,
            textures,
        ) {
            error!("native renderer: gui: {}", e.0);
        }
        graph.pass("present", &[(image, Use::Present)], &[], |_| {});
        drop(graph);
        match self.frames.end(&self.ctx, swapchain, recording) {
            Ok(()) => {}
            Err(FrameError::OutOfDate) => self.stale = true,
            Err(FrameError::Failed(e)) => error!("native renderer: present failed: {e:?}"),
        }
        self.frame += 1;
    }
}

/// The GUI painter's output, drawn over the frame (blended, no depth).
struct GuiPass {
    module: vk::ShaderModule,
    pipeline: vk::Pipeline,
    /// Per frame in flight: vertices and indices (host-visible, grown).
    buffers: Vec<Option<((Buffer, u32), Buffer)>>,
    /// Atlas, unihex font, white (glint and figure stand-ins).
    textures: Option<[(Image, u32); 2]>,
    white: (Image, u32),
    /// The white texel's upload, until it's done.
    white_pending: Option<Buffer>,
    /// Staging buffers of past uploads, freed with the pass.
    spent: Vec<Buffer>,
}

impl GuiPass {
    fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        format: vk::Format,
    ) -> Result<GuiPass, SetupError> {
        let module = pipelines.module(ctx, super::shaders::GUI)?;
        let pipeline = pipelines.graphics(
            ctx,
            &GraphicsDesc {
                module,
                vertex: c"gui_vertex",
                fragment: c"gui_fragment",
                color: &[format],
                depth: None,
                depth_test: None,
                depth_write: false,
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: true,
                additive: false,
                fragment_module: None,
            },
        )?;
        let one = vk::Extent2D { width: 1, height: 1 };
        let image = memory.image(
            ctx,
            "gui white",
            one,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let index = bindless
            .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .ok_or_else(|| SetupError("the bindless set is full".into()))?;
        let mut staging = memory.buffer(ctx, "gui white staging", 4, vk::BufferUsageFlags::TRANSFER_SRC, MemoryLocation::CpuToGpu)?;
        staging.write(0, &[255; 4]);
        Ok(GuiPass {
            module,
            pipeline,
            buffers: (0..FRAMES_IN_FLIGHT).map(|_| None).collect(),
            textures: None,
            white: (image, index),
            white_pending: Some(staging),
            spent: Vec::new(),
        })
    }

    fn destroy(self, ctx: &Context, memory: &Memory) {
        // SAFETY: the device is idle.
        unsafe {
            ctx.device.destroy_pipeline(self.pipeline, None);
            ctx.device.destroy_shader_module(self.module, None);
        }
        for ((v, _), i) in self.buffers.into_iter().flatten() {
            memory.free_buffer(ctx, v);
            memory.free_buffer(ctx, i);
        }
        for b in self.spent.into_iter().chain(self.white_pending) {
            memory.free_buffer(ctx, b);
        }
        for (image, _) in self.textures.into_iter().flatten() {
            memory.free_image(ctx, image);
        }
        memory.free_image(ctx, self.white.0);
    }

    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        slot: usize,
        output: (vk::Image, vk::ImageView, vk::Extent2D),
        frame: crate::gui::pool::GuiFrame,
        textures: Option<[(u32, u32, vk::Format, Vec<u8>); 2]>,
    ) -> Result<(), SetupError> {
        let device = &ctx.device;
        // Uploads: the white texel once, the GUI textures once they exist.
        let mut uploads: Vec<(Buffer, vk::Image, vk::Extent2D)> = Vec::new();
        // Textures already uploaded sit sampled since their first frame.
        let sampled = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        if self.white_pending.is_none() {
            graph.image(self.white.0.raw, vk::ImageAspectFlags::COLOR, sampled);
        }
        for (image, _) in self.textures.iter().flatten() {
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, sampled);
        }
        if let Some(staging) = self.white_pending.take() {
            graph.image(self.white.0.raw, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
            uploads.push((staging, self.white.0.raw, vk::Extent2D { width: 1, height: 1 }));
        }
        if self.textures.is_none()
            && let Some(data) = textures
        {
            let mut made = Vec::new();
            for (name, (w, h, format, bytes)) in ["gui atlas", "gui unihex"].into_iter().zip(data) {
                let extent = vk::Extent2D { width: w, height: h };
                let image = memory.image(
                    ctx,
                    name,
                    extent,
                    format,
                    vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                    vk::ImageAspectFlags::COLOR,
                )?;
                let index = bindless
                    .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .ok_or_else(|| SetupError("the bindless set is full".into()))?;
                let mut staging = memory.buffer(
                    ctx,
                    "gui texture staging",
                    bytes.len() as u64,
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    MemoryLocation::CpuToGpu,
                )?;
                staging.write(0, &bytes);
                graph.image(image.raw, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
                uploads.push((staging, image.raw, extent));
                made.push((image, index));
            }
            let mut made = made.into_iter();
            if let (Some(a), Some(b)) = (made.next(), made.next()) {
                self.textures = Some([a, b]);
            }
        }
        if !uploads.is_empty() {
            let uses: Vec<_> = uploads.iter().map(|(_, i, _)| (*i, Use::TransferDst)).collect();
            graph.pass("gui uploads", &uses, &[], |cmd| unsafe {
                // SAFETY: copies into images in TRANSFER_DST, each staging
                // buffer sized for its image.
                for (staging, image, extent) in &uploads {
                    device.cmd_copy_buffer_to_image(
                        cmd,
                        staging.raw,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::BufferImageCopy {
                            image_subresource: vk::ImageSubresourceLayers {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                mip_level: 0,
                                base_array_layer: 0,
                                layer_count: 1,
                            },
                            image_extent: vk::Extent3D {
                                width: extent.width,
                                height: extent.height,
                                depth: 1,
                            },
                            ..Default::default()
                        }],
                    );
                }
            });
            // Kept until the pass goes: a handful of one-off uploads.
            self.spent.extend(uploads.into_iter().map(|(staging, ..)| staging));
        }

        // This frame's vertices and indices, into the slot's buffers.
        let (vbytes, ibytes): (&[u8], &[u8]) = (bytemuck::cast_slice(&frame.vertices), bytemuck::cast_slice(&frame.indices));
        let fits = self.buffers[slot]
            .as_ref()
            .is_some_and(|((v, _), i)| v.size >= vbytes.len() as u64 && i.size >= ibytes.len() as u64);
        if !fits {
            if let Some(((v, vi), i)) = self.buffers[slot].take() {
                // Frames in flight are done with this slot (its fence).
                bindless.remove_buffer(vi, 0);
                memory.free_buffer(ctx, v);
                memory.free_buffer(ctx, i);
            }
            let size = |n: usize| (n.max(1) as u64).next_power_of_two().max(1 << 16);
            let v = memory.buffer(ctx, "gui vertices", size(vbytes.len()), vk::BufferUsageFlags::STORAGE_BUFFER, MemoryLocation::CpuToGpu)?;
            let vi = bindless
                .add_buffer(ctx, v.raw)
                .ok_or_else(|| SetupError("the bindless set is full".into()))?;
            let i = memory.buffer(ctx, "gui indices", size(ibytes.len()), vk::BufferUsageFlags::INDEX_BUFFER, MemoryLocation::CpuToGpu)?;
            self.buffers[slot] = Some(((v, vi), i));
        }
        let Some(((v, vi), i)) = &mut self.buffers[slot] else {
            return Ok(());
        };
        v.write(0, vbytes);
        i.write(0, ibytes);
        let (vertex_index, index_buffer) = (*vi, i.raw);

        let (image, view, extent) = output;
        let tex = self.textures.as_ref().map(|t| [t[0].1, t[1].1]);
        let white = self.white.1;
        let mut uses = vec![(image, Use::ColorAttachment)];
        uses.push((self.white.0.raw, Use::SampledFragment));
        if let Some(t) = &self.textures {
            uses.extend(t.iter().map(|(i, _)| (i.raw, Use::SampledFragment)));
        }
        let ranges = frame.ranges;
        let pipeline = self.pipeline;
        graph.pass("gui", &uses, &[], |cmd| unsafe {
            // SAFETY: the output in COLOR_ATTACHMENT, textures sampled; the
            // shared layout and set.
            let color = [vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] },
                })
                .store_op(vk::AttachmentStoreOp::STORE)];
            let info = vk::RenderingInfo::default()
                .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent })
                .layer_count(1)
                .color_attachments(&color);
            device.cmd_begin_rendering(cmd, &info);
            if let Some([atlas, unihex]) = tex
                && !ranges.is_empty()
            {
                device.cmd_set_viewport(
                    cmd,
                    0,
                    &[vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: extent.width as f32,
                        height: extent.height as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                );
                device.cmd_set_scissor(cmd, 0, &[vk::Rect2D { offset: vk::Offset2D::default(), extent }]);
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, pipelines.layout, 0, &[bindless.set], &[]);
                device.cmd_bind_index_buffer(cmd, index_buffer, 0, vk::IndexType::UINT32);
                push(
                    ctx,
                    cmd,
                    pipelines.layout,
                    &Push {
                        ids: [vertex_index, 0, 0, 0],
                        more: [atlas, unihex, white, white],
                        params: [0.0; 4],
                        view_proj: [[0.0; 4]; 4],
                    },
                );
                for r in &ranges {
                    device.cmd_draw_indexed(cmd, r.index_count, 1, r.first_index, r.base_vertex, 0);
                }
            }
            device.cmd_end_rendering(cmd);
        });
        Ok(())
    }
}
