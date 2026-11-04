//! Frames in flight: each has its own command buffer, a fence the CPU waits
//! on before reusing it, and a semaphore for its swapchain image.

use ash::vk;

use super::{Context, SetupError, Swapchain};

/// How many frames the CPU may record ahead of the GPU.
pub const FRAMES_IN_FLIGHT: usize = 2;

pub struct Frame {
    pub pool: vk::CommandPool,
    pub commands: vk::CommandBuffer,
    pub done: vk::Fence,
    pub image_ready: vk::Semaphore,
}

pub struct Frames {
    pub frames: Vec<Frame>,
    pub index: usize,
}

/// A frame being recorded: its command buffer and the swapchain image it
/// draws to.
pub struct Recording {
    pub commands: vk::CommandBuffer,
    pub image_index: u32,
}

/// Why a frame couldn't start or finish.
pub enum FrameError {
    /// The swapchain no longer matches the window; remake it and retry.
    OutOfDate,
    Failed(vk::Result),
}

impl Frames {
    pub fn new(ctx: &Context) -> Result<Frames, SetupError> {
        let frames = (0..FRAMES_IN_FLIGHT)
            .map(|_| {
                // SAFETY: plain object creation on a live device.
                unsafe {
                    let pool = ctx.device.create_command_pool(
                        &vk::CommandPoolCreateInfo::default()
                            .queue_family_index(ctx.queue_family)
                            .flags(vk::CommandPoolCreateFlags::TRANSIENT),
                        None,
                    )?;
                    let commands = ctx.device.allocate_command_buffers(
                        &vk::CommandBufferAllocateInfo::default()
                            .command_pool(pool)
                            .level(vk::CommandBufferLevel::PRIMARY)
                            .command_buffer_count(1),
                    )?[0];
                    let done = ctx.device.create_fence(
                        &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                        None,
                    )?;
                    let image_ready = ctx
                        .device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?;
                    Ok(Frame {
                        pool,
                        commands,
                        done,
                        image_ready,
                    })
                }
            })
            .collect::<Result<Vec<_>, vk::Result>>()?;
        Ok(Frames { frames, index: 0 })
    }

    /// Waits for this slot's last frame, acquires a swapchain image and
    /// starts recording.
    pub fn begin(&mut self, ctx: &Context, swapchain: &Swapchain) -> Result<Recording, FrameError> {
        let frame = &self.frames[self.index];
        // SAFETY: the fence and pool belong to this frame slot, which the
        // CPU only touches after the fence says the GPU is done with it.
        unsafe {
            ctx.device
                .wait_for_fences(&[frame.done], true, u64::MAX)
                .map_err(FrameError::Failed)?;
            let image_index = match ctx.swapchain_fn.acquire_next_image(
                swapchain.handle,
                u64::MAX,
                frame.image_ready,
                vk::Fence::null(),
            ) {
                Ok((index, false)) => index,
                Ok((_, true)) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    return Err(FrameError::OutOfDate);
                }
                Err(e) => return Err(FrameError::Failed(e)),
            };
            ctx.device
                .reset_fences(&[frame.done])
                .map_err(FrameError::Failed)?;
            ctx.device
                .reset_command_pool(frame.pool, vk::CommandPoolResetFlags::empty())
                .map_err(FrameError::Failed)?;
            ctx.device
                .begin_command_buffer(
                    frame.commands,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(FrameError::Failed)?;
            Ok(Recording {
                commands: frame.commands,
                image_index,
            })
        }
    }

    /// Ends recording, submits, and presents the image.
    pub fn end(
        &mut self,
        ctx: &Context,
        swapchain: &Swapchain,
        recording: Recording,
    ) -> Result<(), FrameError> {
        let frame = &self.frames[self.index];
        let finished = swapchain.render_finished[recording.image_index as usize];
        // SAFETY: the command buffer was begun in `begin` on this slot; the
        // semaphores and fence are this slot's and this image's.
        unsafe {
            ctx.device
                .end_command_buffer(recording.commands)
                .map_err(FrameError::Failed)?;
            let waits = [vk::SemaphoreSubmitInfo::default()
                .semaphore(frame.image_ready)
                .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)];
            let signals = [vk::SemaphoreSubmitInfo::default()
                .semaphore(finished)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
            let commands =
                [vk::CommandBufferSubmitInfo::default().command_buffer(recording.commands)];
            let submit = vk::SubmitInfo2::default()
                .wait_semaphore_infos(&waits)
                .command_buffer_infos(&commands)
                .signal_semaphore_infos(&signals);
            ctx.device
                .queue_submit2(ctx.queue, &[submit], frame.done)
                .map_err(FrameError::Failed)?;
            let swapchains = [swapchain.handle];
            let indices = [recording.image_index];
            let wait = [finished];
            let present = vk::PresentInfoKHR::default()
                .wait_semaphores(&wait)
                .swapchains(&swapchains)
                .image_indices(&indices);
            self.index = (self.index + 1) % FRAMES_IN_FLIGHT;
            match ctx.swapchain_fn.queue_present(ctx.queue, &present) {
                Ok(false) => Ok(()),
                Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(FrameError::OutOfDate),
                Err(e) => Err(FrameError::Failed(e)),
            }
        }
    }

    /// Destroys the frames. The device must be idle.
    pub fn destroy(self, ctx: &Context) {
        // SAFETY: the caller idles the device first.
        unsafe {
            for frame in self.frames {
                ctx.device.destroy_fence(frame.done, None);
                ctx.device.destroy_semaphore(frame.image_ready, None);
                ctx.device.destroy_command_pool(frame.pool, None);
            }
        }
    }
}

/// Moves `image` between layouts with a synchronization2 barrier.
pub fn transition(
    device: &ash::Device,
    commands: vk::CommandBuffer,
    image: vk::Image,
    (from, from_stage, from_access): (vk::ImageLayout, vk::PipelineStageFlags2, vk::AccessFlags2),
    (to, to_stage, to_access): (vk::ImageLayout, vk::PipelineStageFlags2, vk::AccessFlags2),
) {
    let barrier = [vk::ImageMemoryBarrier2::default()
        .src_stage_mask(from_stage)
        .src_access_mask(from_access)
        .dst_stage_mask(to_stage)
        .dst_access_mask(to_access)
        .old_layout(from)
        .new_layout(to)
        .image(image)
        .subresource_range(super::swapchain::color_range())];
    // SAFETY: recorded into a command buffer the caller is recording.
    unsafe {
        device.cmd_pipeline_barrier2(
            commands,
            &vk::DependencyInfo::default().image_memory_barriers(&barrier),
        );
    }
}
