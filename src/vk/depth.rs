//! The depth buffer scenes share (`DepthTarget`), its format, and the
//! flipped full-frame viewport.

use ash::vk;

use super::bindless::Bindless;
use super::graph::Graph;
use super::memory::{Image, Memory};
use super::{Context, SetupError};

/// A scene's depth buffer, remade when the frame size changes, with a
/// bindless index so shaders can sample it.
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
