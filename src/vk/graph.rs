//! A small render graph (VULKAN_REWRITE_CHECKLIST.md 2.7): passes run in
//! the order they're added, each declaring how it uses its images and
//! buffers, and the graph inserts the synchronization2 barriers and layout
//! changes between them. No reordering or aliasing yet: an ordered list
//! with derived barriers, so no pass places a barrier by hand.

use std::collections::HashMap;

use ash::vk;

use super::Context;
use super::timestamps::Timestamps;

/// How a pass uses a resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    ColorAttachment,
    DepthAttachment,
    /// Sampled or loaded in a fragment shader.
    SampledFragment,
    /// Read as a storage buffer or sampled image in compute.
    ComputeRead,
    ComputeWrite,
    /// Read as a storage buffer in a vertex shader.
    VertexRead,
    /// Read as indirect draw arguments or counts.
    IndirectRead,
    TransferDst,
    TransferSrc,
    Present,
}

impl Use {
    fn stage(self) -> vk::PipelineStageFlags2 {
        use vk::PipelineStageFlags2 as S;
        match self {
            Use::ColorAttachment => S::COLOR_ATTACHMENT_OUTPUT,
            Use::DepthAttachment => S::EARLY_FRAGMENT_TESTS | S::LATE_FRAGMENT_TESTS,
            Use::SampledFragment => S::FRAGMENT_SHADER,
            Use::ComputeRead | Use::ComputeWrite => S::COMPUTE_SHADER,
            Use::VertexRead => S::VERTEX_SHADER,
            Use::IndirectRead => S::DRAW_INDIRECT,
            // Copies, but also fills and inline updates, which count as
            // clears.
            Use::TransferDst | Use::TransferSrc => S::ALL_TRANSFER,
            Use::Present => S::BOTTOM_OF_PIPE,
        }
    }

    fn access(self) -> vk::AccessFlags2 {
        use vk::AccessFlags2 as A;
        match self {
            Use::ColorAttachment => A::COLOR_ATTACHMENT_WRITE | A::COLOR_ATTACHMENT_READ,
            Use::DepthAttachment => {
                A::DEPTH_STENCIL_ATTACHMENT_WRITE | A::DEPTH_STENCIL_ATTACHMENT_READ
            }
            Use::SampledFragment => A::SHADER_SAMPLED_READ | A::SHADER_STORAGE_READ,
            Use::ComputeRead | Use::VertexRead => A::SHADER_STORAGE_READ | A::SHADER_SAMPLED_READ,
            Use::ComputeWrite => A::SHADER_STORAGE_WRITE | A::SHADER_STORAGE_READ,
            Use::IndirectRead => A::INDIRECT_COMMAND_READ,
            Use::TransferDst => A::TRANSFER_WRITE,
            Use::TransferSrc => A::TRANSFER_READ,
            Use::Present => A::NONE,
        }
    }

    fn writes(self) -> bool {
        matches!(
            self,
            Use::ColorAttachment | Use::DepthAttachment | Use::ComputeWrite | Use::TransferDst
        )
    }

    fn layout(self, depth: bool) -> vk::ImageLayout {
        use vk::ImageLayout as L;
        match self {
            Use::ColorAttachment => L::COLOR_ATTACHMENT_OPTIMAL,
            Use::DepthAttachment => L::DEPTH_ATTACHMENT_OPTIMAL,
            Use::SampledFragment | Use::ComputeRead | Use::VertexRead => {
                if depth {
                    L::DEPTH_READ_ONLY_OPTIMAL
                } else {
                    L::SHADER_READ_ONLY_OPTIMAL
                }
            }
            Use::ComputeWrite => L::GENERAL,
            Use::TransferDst => L::TRANSFER_DST_OPTIMAL,
            Use::TransferSrc => L::TRANSFER_SRC_OPTIMAL,
            Use::Present => L::PRESENT_SRC_KHR,
            Use::IndirectRead => L::GENERAL,
        }
    }
}

/// A resource's state before its first use this frame: whatever earlier
/// frames, possibly still running on the queue, did with it. The first use
/// waits for all of it, which covers hazards across frames in flight (a
/// buffer the previous frame still reads, written again by this one).
const FROM_EARLIER_FRAMES: State = State {
    stage: vk::PipelineStageFlags2::ALL_COMMANDS,
    access: vk::AccessFlags2::MEMORY_WRITE,
    layout: vk::ImageLayout::UNDEFINED,
    written: true,
};

#[allow(non_snake_case, reason = "the image form of FROM_EARLIER_FRAMES")]
const fn FROM_EARLIER_FRAMES_IMAGE(layout: vk::ImageLayout) -> State {
    State {
        layout,
        ..FROM_EARLIER_FRAMES
    }
}

#[derive(Clone, Copy)]
struct State {
    stage: vk::PipelineStageFlags2,
    access: vk::AccessFlags2,
    layout: vk::ImageLayout,
    /// Whether the last use wrote, so a following read must wait for it.
    written: bool,
}

struct ImageEntry {
    aspect: vk::ImageAspectFlags,
    state: State,
}

/// One frame's passes, recorded into its command buffer.
pub struct Graph<'a> {
    ctx: &'a Context,
    commands: vk::CommandBuffer,
    images: HashMap<vk::Image, ImageEntry>,
    buffers: HashMap<vk::Buffer, State>,
    timestamps: Option<&'a mut Timestamps>,
    /// Barriers placed this frame, for the debug overlay and tests.
    pub barriers: u32,
}

impl<'a> Graph<'a> {
    pub fn new(
        ctx: &'a Context,
        commands: vk::CommandBuffer,
        timestamps: Option<&'a mut Timestamps>,
    ) -> Graph<'a> {
        Graph {
            ctx,
            commands,
            images: HashMap::new(),
            buffers: HashMap::new(),
            timestamps,
            barriers: 0,
        }
    }

    /// Declares an image the graph tracks, in the layout it holds now
    /// (UNDEFINED discards its contents on first use).
    pub fn image(
        &mut self,
        image: vk::Image,
        aspect: vk::ImageAspectFlags,
        layout: vk::ImageLayout,
    ) {
        self.images.insert(
            image,
            ImageEntry {
                aspect,
                state: FROM_EARLIER_FRAMES_IMAGE(layout),
            },
        );
    }

    /// Runs a pass: places the barriers its uses need, then records it,
    /// timed as `name`.
    pub fn pass(
        &mut self,
        name: &'static str,
        images: &[(vk::Image, Use)],
        buffers: &[(vk::Buffer, Use)],
        record: impl FnOnce(vk::CommandBuffer),
    ) {
        let mut image_barriers = Vec::new();
        for &(image, usage) in images {
            let Some(entry) = self.images.get_mut(&image) else {
                debug_assert!(false, "pass {name} uses an image the graph doesn't track");
                continue;
            };
            let depth = entry.aspect.contains(vk::ImageAspectFlags::DEPTH);
            let layout = usage.layout(depth);
            let before = entry.state;
            let needs = before.written || usage.writes() || before.layout != layout;
            if needs {
                image_barriers.push(
                    vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(before.stage)
                        .src_access_mask(if before.written {
                            before.access
                        } else {
                            vk::AccessFlags2::NONE
                        })
                        .dst_stage_mask(usage.stage())
                        .dst_access_mask(usage.access())
                        .old_layout(before.layout)
                        .new_layout(layout)
                        .image(image)
                        .subresource_range(super::memory::range(entry.aspect)),
                );
                entry.state = State {
                    stage: usage.stage(),
                    access: usage.access(),
                    layout,
                    written: usage.writes(),
                };
            } else {
                // A read after a read in the same layout: no barrier, but a
                // later write must wait for both readers.
                entry.state.stage |= usage.stage();
                entry.state.access |= usage.access();
            }
        }
        let mut buffer_barriers = Vec::new();
        for &(buffer, usage) in buffers {
            let before = self
                .buffers
                .get(&buffer)
                .copied()
                .unwrap_or(FROM_EARLIER_FRAMES);
            if before.written || usage.writes() {
                buffer_barriers.push(
                    vk::BufferMemoryBarrier2::default()
                        .src_stage_mask(before.stage)
                        .src_access_mask(if before.written {
                            before.access
                        } else {
                            vk::AccessFlags2::NONE
                        })
                        .dst_stage_mask(usage.stage())
                        .dst_access_mask(usage.access())
                        .buffer(buffer)
                        .offset(0)
                        .size(vk::WHOLE_SIZE),
                );
                self.buffers.insert(
                    buffer,
                    State {
                        stage: usage.stage(),
                        access: usage.access(),
                        layout: vk::ImageLayout::UNDEFINED,
                        written: usage.writes(),
                    },
                );
            } else {
                let state = self.buffers.entry(buffer).or_insert(before);
                state.stage |= usage.stage();
                state.access |= usage.access();
            }
        }
        if !image_barriers.is_empty() || !buffer_barriers.is_empty() {
            self.barriers += (image_barriers.len() + buffer_barriers.len()) as u32;
            // SAFETY: recorded into the frame's command buffer.
            unsafe {
                self.ctx.device.cmd_pipeline_barrier2(
                    self.commands,
                    &vk::DependencyInfo::default()
                        .image_memory_barriers(&image_barriers)
                        .buffer_memory_barriers(&buffer_barriers),
                );
            }
        }
        if let Some(t) = self.timestamps.as_deref_mut() {
            t.begin(self.ctx, self.commands, name);
        }
        record(self.commands);
        if let Some(t) = self.timestamps.as_deref_mut() {
            t.end(self.ctx, self.commands);
        }
    }

    pub fn commands(&self) -> vk::CommandBuffer {
        self.commands
    }
}
