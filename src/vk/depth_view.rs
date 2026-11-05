//! The depth debug views every scene shares (`vkview --view depth|hiz`,
//! `shaders/debug_view.slang`): the depth buffer, or a level of the
//! terrain's occlusion pyramid, drawn over the frame as distance fog, so
//! depth passes can be checked by eye.

use ash::vk;

use super::bindless::Bindless;
use super::graph::{Graph, Use};
use super::memory::{Image, Memory};
use super::pipeline::{GraphicsDesc, Pipelines, Push, push};
use super::{Context, SetupError};

pub struct DepthView {
    module: vk::ShaderModule,
    depth: vk::Pipeline,
    hiz: vk::Pipeline,
}

impl DepthView {
    pub fn new(
        ctx: &Context,
        pipelines: &Pipelines,
        color_format: vk::Format,
    ) -> Result<DepthView, SetupError> {
        let module = pipelines.module(ctx, super::shaders::DEBUG_VIEW)?;
        let pipeline = |fragment| {
            pipelines.graphics(
                ctx,
                &GraphicsDesc {
                    module,
                    vertex: c"fullscreen_vertex",
                    fragment,
                    color: &[color_format],
                    depth: None,
                    depth_test: None,
                    depth_write: false,
                    cull: vk::CullModeFlags::NONE,
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    blend: false,
                additive: false,
                },
            )
        };
        Ok(DepthView {
            module,
            depth: pipeline(c"depth_fragment")?,
            hiz: pipeline(c"hiz_fragment")?,
        })
    }

    /// Draws `depth` (its image and bindless index, registered in its
    /// read-only layout) over `color`: `near` is the projection's near
    /// plane, `fog` the distance shown as mid-grey.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    pub fn record(
        &self,
        ctx: &Context,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        depth: (vk::Image, u32),
        color: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        near: f32,
        fog: f32,
    ) {
        let push = Push {
            ids: [0; 4],
            more: [depth.1, 0, 0, 0],
            params: [near, fog, 0.0, 0.0],
            view_proj: [[0.0; 4]; 4],
        };
        let images = [
            (depth.0, Use::SampledFragment),
            (color.0, Use::ColorAttachment),
        ];
        self.draw(
            ctx,
            bindless,
            pipelines,
            graph,
            self.depth,
            &images,
            &[],
            color,
            extent,
            &push,
        );
    }

    /// Draws level `level` of an occlusion pyramid (its buffer and bindless
    /// index; `size0` the depth's size it was built from) over `color`.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    pub fn record_pyramid(
        &self,
        ctx: &Context,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        pyramid: (vk::Buffer, u32),
        size0: (u32, u32),
        level: u32,
        color: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        near: f32,
        fog: f32,
    ) {
        let push = Push {
            ids: [0; 4],
            more: [pyramid.1, size0.0, size0.1, level],
            params: [near, fog, 0.0, 0.0],
            view_proj: [[0.0; 4]; 4],
        };
        let images = [(color.0, Use::ColorAttachment)];
        let buffers = [(pyramid.0, Use::SampledFragment)];
        self.draw(
            ctx, bindless, pipelines, graph, self.hiz, &images, &buffers, color, extent, &push,
        );
    }

    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn draw(
        &self,
        ctx: &Context,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        pipeline: vk::Pipeline,
        images: &[(vk::Image, Use)],
        buffers: &[(vk::Buffer, Use)],
        color: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        values: &Push,
    ) {
        let device = &ctx.device;
        graph.pass("debug view", images, buffers, |cmd| unsafe {
            // SAFETY: the graph moved the images to the layouts their uses
            // name, the colour image to an attachment.
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
            set_viewport(ctx, cmd, extent);
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipelines.layout,
                0,
                &[bindless.set],
                &[],
            );
            push(ctx, cmd, pipelines.layout, values);
            device.cmd_draw(cmd, 3, 1, 0, 0);
            device.cmd_end_rendering(cmd);
        });
    }

    pub fn destroy(self, ctx: &Context) {
        // SAFETY: the caller idles the device first.
        unsafe {
            ctx.device.destroy_pipeline(self.depth, None);
            ctx.device.destroy_pipeline(self.hiz, None);
            ctx.device.destroy_shader_module(self.module, None);
        }
    }
}

/// A scene's depth buffer, remade when the frame size changes, with a
/// bindless index so the depth view can read it.
#[derive(Default)]
pub struct DepthTarget(Option<(Image, u32)>);

impl DepthTarget {
    /// The depth image for `extent`: its image, view and bindless index,
    /// declared to `graph` with its contents discarded.
    pub fn get(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        graph: &mut Graph,
        extent: vk::Extent2D,
        frame: u64,
    ) -> Result<(vk::Image, vk::ImageView, u32), SetupError> {
        if !self.0.as_ref().is_some_and(|(d, _)| d.extent == extent) {
            if let Some((old, index)) = self.0.take() {
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
            self.0 = Some((image, index));
        }
        let (image, index) = self.0.as_ref().expect("made above");
        graph.image(
            image.raw,
            vk::ImageAspectFlags::DEPTH,
            vk::ImageLayout::UNDEFINED,
        );
        Ok((image.raw, image.view, *index))
    }

    pub fn destroy(self, ctx: &Context, memory: &Memory) {
        if let Some((image, _)) = self.0 {
            memory.free_image(ctx, image);
        }
    }
}

pub const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

/// Sets the dynamic viewport and scissor to the whole of `extent`; a
/// negative height turns Vulkan's downward y into the usual upward one.
pub fn set_viewport(ctx: &Context, cmd: vk::CommandBuffer, extent: vk::Extent2D) {
    // SAFETY: dynamic state on a command buffer that is recording.
    unsafe {
        ctx.device.cmd_set_viewport(
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
        ctx.device.cmd_set_scissor(
            cmd,
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent,
            }],
        );
    }
}
