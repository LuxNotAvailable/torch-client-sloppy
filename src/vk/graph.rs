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
    /// Read as an index buffer.
    IndexRead,
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
            Use::IndexRead => S::INDEX_INPUT,
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
            Use::IndexRead => A::INDEX_READ,
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
            Use::IndirectRead | Use::IndexRead => L::GENERAL,
        }
    }
}

/// A resource's state before its first use this frame: whatever earlier
/// frames, possibly still running on the queue, did with it. The first use
/// waits for all of it, which covers hazards across frames in flight (a
/// buffer the previous frame still reads, written again by this one).
const FROM_EARLIER_FRAMES: State = State {
    readers: vk::PipelineStageFlags2::NONE,
    write_stage: vk::PipelineStageFlags2::ALL_COMMANDS,
    write_access: vk::AccessFlags2::MEMORY_WRITE,
    seen: vk::PipelineStageFlags2::NONE,
    layout: vk::ImageLayout::UNDEFINED,
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
    /// Stages that read since the last write, which a write must wait for.
    readers: vk::PipelineStageFlags2,
    /// The last write (or layout change), which reads must wait for.
    write_stage: vk::PipelineStageFlags2,
    write_access: vk::AccessFlags2,
    /// The stages the last write has been made visible to: a read in any
    /// other stage needs its own barrier, even after other reads.
    seen: vk::PipelineStageFlags2,
    layout: vk::ImageLayout,
}

impl State {
    /// The barrier's source scope, and the state after, for `usage` in
    /// `layout`; `None` when no barrier is needed.
    fn next(
        &mut self,
        usage: Use,
        layout: vk::ImageLayout,
    ) -> Option<(vk::PipelineStageFlags2, vk::AccessFlags2)> {
        let stage = usage.stage();
        if usage.writes() || layout != self.layout {
            let src = (self.readers | self.write_stage, self.write_access);
            *self = State {
                readers: vk::PipelineStageFlags2::NONE,
                write_stage: stage,
                write_access: if usage.writes() {
                    usage.access() & WRITE_ACCESS
                } else {
                    vk::AccessFlags2::NONE
                },
                seen: stage,
                layout,
            };
            return Some(src);
        }
        self.readers |= stage;
        if self.seen.contains(stage) || self.write_stage == vk::PipelineStageFlags2::NONE {
            return None;
        }
        self.seen |= stage;
        Some((self.write_stage, self.write_access))
    }
}

/// The write accesses, so a write's state keeps only those to flush.
const WRITE_ACCESS: vk::AccessFlags2 = vk::AccessFlags2::from_raw(
    vk::AccessFlags2::COLOR_ATTACHMENT_WRITE.as_raw()
        | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE.as_raw()
        | vk::AccessFlags2::SHADER_STORAGE_WRITE.as_raw()
        | vk::AccessFlags2::TRANSFER_WRITE.as_raw(),
);

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
            let old_layout = entry.state.layout;
            if let Some((stage, access)) = entry.state.next(usage, layout) {
                image_barriers.push(
                    vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(stage)
                        .src_access_mask(access)
                        .dst_stage_mask(usage.stage())
                        .dst_access_mask(usage.access())
                        .old_layout(old_layout)
                        .new_layout(layout)
                        .image(image)
                        .subresource_range(super::memory::range(entry.aspect)),
                );
            }
        }
        let mut buffer_barriers = Vec::new();
        for &(buffer, usage) in buffers {
            let state = self.buffers.entry(buffer).or_insert(FROM_EARLIER_FRAMES);
            if let Some((stage, access)) = state.next(usage, vk::ImageLayout::UNDEFINED) {
                buffer_barriers.push(
                    vk::BufferMemoryBarrier2::default()
                        .src_stage_mask(stage)
                        .src_access_mask(access)
                        .dst_stage_mask(usage.stage())
                        .dst_access_mask(usage.access())
                        .buffer(buffer)
                        .offset(0)
                        .size(vk::WHOLE_SIZE),
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_in_a_new_stage_waits_for_the_write_even_after_another_read() {
        let mut state = FROM_EARLIER_FRAMES;
        let none = vk::ImageLayout::UNDEFINED;
        assert!(state.next(Use::ComputeWrite, none).is_some());
        let (stage, access) = state.next(Use::TransferSrc, none).unwrap();
        assert_eq!(stage, vk::PipelineStageFlags2::COMPUTE_SHADER);
        assert_eq!(access, vk::AccessFlags2::SHADER_STORAGE_WRITE);
        // The indirect read needs the compute write made visible too.
        let (stage, _) = state.next(Use::IndirectRead, none).unwrap();
        assert_eq!(stage, vk::PipelineStageFlags2::COMPUTE_SHADER);
        // A second read in a stage already covered needs nothing.
        assert!(state.next(Use::IndirectRead, none).is_none());
        // A write waits for both readers.
        let (stage, _) = state.next(Use::TransferDst, none).unwrap();
        assert!(stage.contains(
            vk::PipelineStageFlags2::ALL_TRANSFER | vk::PipelineStageFlags2::DRAW_INDIRECT
        ));
    }
}
