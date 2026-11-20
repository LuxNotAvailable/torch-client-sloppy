//! The voxel window traced reflections read (`terrain::voxels`): the
//! sections around the camera, 8 x 4 x 8, in one storage buffer the pack's
//! water reads by bindless index (`PackGeometry::voxels`).
//!
//! Layout, in u32 words: `MAGIC` and three pads, then per slot its section
//! (x, y, z in sections) and state (`UNKNOWN`, `EMPTY`, `FILLED`), then per
//! slot 4096 two-word records. Section `s` lives in slot
//! `(s.x & 7) | (s.z & 7) << 3 | (s.y & 3) << 6`; a slot whose header names
//! another section is unknown. `gbuffers_water.slang` reads the same.

use std::collections::HashMap;
use std::sync::Arc;

use ash::vk;
use bevy::math::Vec3;
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::memory::{Buffer, Memory};
use super::upload::Uploads;
use super::{Context, SetupError};
use crate::terrain::voxels::SectionVoxels;

pub const MAGIC: u32 = 0x5658_4c31;
const SLOTS: usize = 8 * 4 * 8;
const HEADER_BYTES: u64 = 16 + SLOTS as u64 * 16;
const SLOT_BYTES: u64 = 4096 * 8;
const UNKNOWN: u32 = 0;
const EMPTY: u32 = 1;
const FILLED: u32 = 2;
/// Bytes of records staged a frame; slots past it wait (and read unknown).
const BUDGET: u64 = 1 << 20;

/// A copy from the staging ring into the window.
pub struct Upload {
    pub src: vk::Buffer,
    pub from: u64,
    pub at: u64,
    pub size: u64,
}

#[derive(Default)]
pub struct VoxelWindow {
    buffer: Option<(Buffer, u32)>,
    /// Every stored section's records, by section, with a generation.
    store: HashMap<[i32; 3], (Arc<SectionVoxels>, u64)>,
    generation: u64,
    /// What each slot holds on the GPU: section and generation (0 unknown).
    slots: Vec<Option<([i32; 3], u64)>>,
    magic: bool,
}

fn slot_of(s: [i32; 3]) -> usize {
    ((s[0] & 7) | (s[2] & 7) << 3 | (s[1] & 3) << 6) as usize
}

impl VoxelWindow {
    /// A section's records (origin in blocks); `None` forgets them.
    pub fn store(&mut self, origin: [i32; 3], voxels: Option<Arc<SectionVoxels>>) {
        let key = [origin[0] >> 4, origin[1] >> 4, origin[2] >> 4];
        match voxels {
            Some(v) => {
                self.generation += 1;
                self.store.insert(key, (v, self.generation));
            }
            None => {
                self.store.remove(&key);
            }
        }
    }

    pub fn drop_column(&mut self, cx: i32, cz: i32) {
        self.store.retain(|k, _| k[0] != cx || k[2] != cz);
    }

    pub fn clear(&mut self) {
        self.store.clear();
    }

    /// The window's bindless index and buffer, once made.
    pub fn bound(&self) -> Option<(vk::Buffer, u32)> {
        self.buffer.as_ref().map(|(b, i)| (b.raw, *i))
    }

    /// Stages what changed in the window around `eye` (blocks), making the
    /// buffer the first time. Does nothing unless a pack wants voxels.
    pub fn update(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        uploads: &mut Uploads,
        eye: Vec3,
        out: &mut Vec<Upload>,
    ) -> Result<(), SetupError> {
        if !crate::terrain::voxels::wanted() {
            return Ok(());
        }
        if self.buffer.is_none() {
            let buffer = memory.buffer(
                ctx,
                "voxel window",
                HEADER_BYTES + SLOTS as u64 * SLOT_BYTES,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuOnly,
            )?;
            let index = bindless
                .add_buffer(ctx, buffer.raw)
                .ok_or_else(|| SetupError("the bindless set is full".into()))?;
            self.buffer = Some((buffer, index));
            self.slots = vec![None; SLOTS];
            self.magic = false;
        }
        let mut stage = |bytes: &[u8], at: u64, out: &mut Vec<Upload>| -> bool {
            match uploads.stage(bytes) {
                Some(s) => {
                    out.push(Upload {
                        src: s.buffer,
                        from: s.offset,
                        at,
                        size: s.size,
                    });
                    true
                }
                None => false,
            }
        };
        if !self.magic {
            self.magic = stage(bytemuck::cast_slice(&[MAGIC, 0, 0, 0]), 0, out);
        }
        // Sections around the eye: four either side across, two up and down.
        let base = [
            (eye.x / 16.0 + 0.5).floor() as i32 - 4,
            (eye.y / 16.0 + 0.5).floor() as i32 - 2,
            (eye.z / 16.0 + 0.5).floor() as i32 - 4,
        ];
        let mut spent = 0u64;
        for dy in 0..4 {
            for dz in 0..8 {
                for dx in 0..8 {
                    let s = [base[0] + dx, base[1] + dy, base[2] + dz];
                    let slot = slot_of(s);
                    let entry = self.store.get(&s);
                    let want = (s, entry.map_or(0, |e| e.1));
                    if self.slots[slot] == Some(want) {
                        continue;
                    }
                    let state = match entry {
                        None => UNKNOWN,
                        Some((v, _)) if v.cells.is_empty() => EMPTY,
                        Some(_) => FILLED,
                    };
                    if state == FILLED {
                        if spent + SLOT_BYTES > BUDGET {
                            continue;
                        }
                        let dense = entry.map(|(v, _)| v.dense()).unwrap_or_default();
                        let at = HEADER_BYTES + slot as u64 * SLOT_BYTES;
                        if !stage(bytemuck::cast_slice(&dense), at, out) {
                            return Ok(());
                        }
                        spent += SLOT_BYTES;
                    }
                    let header = [s[0] as u32, s[1] as u32, s[2] as u32, state];
                    if !stage(bytemuck::cast_slice(&header), 16 + slot as u64 * 16, out) {
                        return Ok(());
                    }
                    self.slots[slot] = Some(want);
                }
            }
        }
        Ok(())
    }

    pub fn destroy(self, ctx: &Context, memory: &Memory) {
        if let Some((buffer, _)) = self.buffer {
            memory.free_buffer(ctx, buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_sections_take_different_slots() {
        let mut seen = std::collections::HashSet::new();
        for y in -2..2 {
            for z in -4..4 {
                for x in -4..4 {
                    assert!(seen.insert(slot_of([x, y, z])));
                }
            }
        }
    }
}
