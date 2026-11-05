//! The frame's last full-screen pass (VULKAN_REWRITE_CHECKLIST.md 5.3,
//! `shaders/post.slang`): the scene is drawn into an image of its own,
//! larger under SSAA, and this pass reads it into the output with the
//! server's post effects (`renderer::post_effects` flags) and FXAA.

use ash::vk;

use super::bindless::Bindless;
use super::depth_view::set_viewport;
use super::graph::{Graph, Use};
use super::memory::{Image, Memory};
use super::pipeline::{GraphicsDesc, Pipelines, Push, push};
use super::{Context, SetupError};

/// What the pass does; with none of it the scene draws straight into the
/// output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub fxaa: bool,
    /// The scene's size over the output's (1, 1.5 or 2 in the game).
    pub ssaa: f32,
    /// Post effect flags in place of the snapshot's.
    pub effects: Option<u32>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            fxaa: false,
            ssaa: 1.0,
            effects: None,
        }
    }
}

impl Settings {
    /// The scene's extent for an output of `extent`.
    pub fn scene_extent(&self, extent: vk::Extent2D) -> vk::Extent2D {
        let scale = |v: u32| ((v as f32 * self.ssaa).round() as u32).max(1);
        vk::Extent2D {
            width: scale(extent.width),
            height: scale(extent.height),
        }
    }
}

pub struct PostPass {
    module: vk::ShaderModule,
    pipeline: vk::Pipeline,
    format: vk::Format,
    scene: Option<(Image, u32)>,
}

impl PostPass {
    pub fn new(
        ctx: &Context,
        pipelines: &Pipelines,
        color_format: vk::Format,
    ) -> Result<PostPass, SetupError> {
        let module = pipelines.module(ctx, super::shaders::POST)?;
        let pipeline = pipelines.graphics(
            ctx,
            &GraphicsDesc {
                module,
                vertex: c"post_vertex",
                fragment: c"post_fragment",
                color: &[color_format],
                depth: None,
                depth_test: None,
                depth_write: false,
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: false,
                additive: false,
            },
        )?;
        Ok(PostPass {
            module,
            pipeline,
            format: color_format,
            scene: None,
        })
    }

    /// The scene image for `extent` (its image and view), declared to
    /// `graph` with its contents discarded.
    pub fn scene(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        graph: &mut Graph,
        extent: vk::Extent2D,
        frame: u64,
    ) -> Result<(vk::Image, vk::ImageView), SetupError> {
        if !self.scene.as_ref().is_some_and(|(s, _)| s.extent == extent) {
            if let Some((old, index)) = self.scene.take() {
                // Resizes idle the device first, so the old image is unused.
                bindless.remove_texture(index, frame);
                memory.free_image(ctx, old);
            }
            let image = memory.image(
                ctx,
                "post scene",
                extent,
                self.format,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                vk::ImageAspectFlags::COLOR,
            )?;
            let index = bindless
                .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(|| SetupError("the bindless set is full".to_owned()))?;
            self.scene = Some((image, index));
        }
        let (image, _) = self.scene.as_ref().expect("made above");
        graph.image(
            image.raw,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::UNDEFINED,
        );
        Ok((image.raw, image.view))
    }

    /// Reads the scene into `output` with `flags` effects and FXAA if asked.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    pub fn record(
        &self,
        ctx: &Context,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        output: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        flags: u32,
        fxaa: bool,
    ) {
        let Some((scene, index)) = &self.scene else {
            return;
        };
        let values = Push {
            ids: [0; 4],
            more: [*index, flags, u32::from(fxaa), 0],
            params: [
                1.0 / scene.extent.width as f32,
                1.0 / scene.extent.height as f32,
                extent.width as f32,
                extent.height as f32,
            ],
            view_proj: [[0.0; 4]; 4],
        };
        let device = &ctx.device;
        let pipeline = self.pipeline;
        graph.pass(
            "post",
            &[
                (scene.raw, Use::SampledFragment),
                (output.0, Use::ColorAttachment),
            ],
            &[],
            |cmd| unsafe {
                // SAFETY: the graph moved the scene to a sampled layout and
                // the output to an attachment.
                let colors = [vk::RenderingAttachmentInfo::default()
                    .image_view(output.1)
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
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.layout,
                    0,
                    &[bindless.set],
                    &[],
                );
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                push(ctx, cmd, pipelines.layout, &values);
                device.cmd_draw(cmd, 3, 1, 0, 0);
                device.cmd_end_rendering(cmd);
            },
        );
    }

    pub fn destroy(self, ctx: &Context, memory: &Memory) {
        // SAFETY: the caller idles the device first.
        unsafe {
            ctx.device.destroy_pipeline(self.pipeline, None);
            ctx.device.destroy_shader_module(self.module, None);
        }
        if let Some((image, _)) = self.scene {
            memory.free_image(ctx, image);
        }
    }
}
