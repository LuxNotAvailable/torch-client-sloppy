//! Uploads through a staging ring (VULKAN_REWRITE_CHECKLIST.md 2.2): each
//! frame in flight owns a host-visible slice, data is copied in on the CPU
//! and copy commands go into that frame's command buffer, ahead of the
//! passes that read it. A per-frame byte budget spreads bursts (terrain's
//! `take_budgeted` behaviour) instead of one submit carrying them all.

use ash::vk;
use gpu_allocator::MemoryLocation;

use super::frame::FRAMES_IN_FLIGHT;
use super::memory::{Buffer, Memory};
use super::{Context, SetupError};

pub struct Uploads {
    ring: Buffer,
    slice: u64,
    frame: usize,
    used: u64,
    /// Bytes staged per frame, for the report.
    pub last_bytes: u64,
}

/// A region of the ring holding staged bytes, to copy from.
#[derive(Clone, Copy)]
pub struct Staged {
    pub buffer: vk::Buffer,
    pub offset: u64,
    pub size: u64,
}

impl Uploads {
    /// A ring of `per_frame` bytes for each frame in flight.
    pub fn new(ctx: &Context, memory: &Memory, per_frame: u64) -> Result<Uploads, SetupError> {
        let ring = memory.buffer(
            ctx,
            "staging ring",
            per_frame * FRAMES_IN_FLIGHT as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        Ok(Uploads {
            ring,
            slice: per_frame,
            frame: 0,
            used: 0,
            last_bytes: 0,
        })
    }

    /// Starts frame slot `frame`; its slice is free once the slot's fence
    /// has signalled.
    pub fn begin(&mut self, frame: usize) {
        self.last_bytes = self.used;
        self.frame = frame;
        self.used = 0;
    }

    /// Copies `bytes` into this frame's slice, or `None` when the slice (the
    /// frame's budget) is full: the caller tries again next frame.
    pub fn stage(&mut self, bytes: &[u8]) -> Option<Staged> {
        let size = bytes.len() as u64;
        let start = self.used.next_multiple_of(16);
        if start + size > self.slice {
            return None;
        }
        let offset = self.frame as u64 * self.slice + start;
        self.ring.write(offset as usize, bytes);
        self.used = start + size;
        Some(Staged {
            buffer: self.ring.raw,
            offset,
            size,
        })
    }

    pub fn destroy(self, ctx: &Context, memory: &Memory) {
        memory.free_buffer(ctx, self.ring);
    }
}
