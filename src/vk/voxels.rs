//! The voxel window traced reflections read (`terrain::voxels`): the
//! sections around the camera, `side` x `HEIGHT` x `side`, in one storage
//! buffer the pack's water reads by bindless index
//! (`PackGeometry::voxels`). Its reach is the pack's `torchVoxelDistance`.
//!
//! Layout, in u32 words: `MAGIC`, side, height and a pad; then per slot
//! its section (x, y, z in sections) and state (`UNKNOWN`, `EMPTY`, or
//! `FILLED | page << 2`); then the pages, 4096 two-word records each. Only
//! filled sections take a page, so air and buried rock cost a header.
//! Section `s` lives in slot `m(s.x) + m(s.z) * side + m(s.y, HEIGHT) *
//! side^2` (`m`: the non-negative remainder); a slot whose header names
//! another section is unknown. `lib/water.slang` reads the same.

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

pub const MAGIC: u32 = 0x5658_4c32;
/// Sections tall: three below the eye's, four above.
const HEIGHT: i32 = 8;
const PAGE_BYTES: u64 = 4096 * 8;
/// At most 64 MB of pages; past that, the farthest filled sections read
/// unknown (sky).
const MAX_PAGES: u32 = 2048;
const UNKNOWN: u32 = 0;
const EMPTY: u32 = 1;
const FILLED: u32 = 2;
/// Bytes of records staged a frame; slots past it wait (and read unknown).
const BUDGET: u64 = 2 << 20;

/// A copy from the staging ring into the window.
pub struct Upload {
    pub src: vk::Buffer,
    pub from: u64,
    pub at: u64,
    pub size: u64,
}

struct Window {
    buffer: Buffer,
    index: u32,
    side: i32,
    pages: u32,
    /// What each slot holds on the GPU: section and generation, and its
    /// page.
    slots: Vec<(Option<([i32; 3], u64)>, Option<u32>)>,
    free: Vec<u32>,
    magic: bool,
    /// The window's corner section and the store's generation when every
    /// slot last matched them; nothing to do until either moves.
    settled: Option<([i32; 3], u64)>,
}

#[derive(Default)]
pub struct VoxelWindow {
    window: Option<Window>,
    /// Windows replaced by a bigger reach, freed with the scene (a frame in
    /// flight may still read them).
    retired: Vec<Buffer>,
    /// Every stored section's records, by section, with a generation.
    store: HashMap<[i32; 3], (Arc<SectionVoxels>, u64)>,
    generation: u64,
}

/// Pages a window `side` sections across keeps: one a slot, up to
/// `MAX_PAGES` (past it, near sections take far ones' pages).
fn pages_for(side: i32) -> u32 {
    ((side * side * HEIGHT) as u32).min(MAX_PAGES)
}

fn header_bytes(side: i32) -> u64 {
    16 + (side * side * HEIGHT) as u64 * 16
}

/// Section `s`'s slot in a window `side` sections across.
fn slot_of(side: i32, s: [i32; 3]) -> usize {
    (s[0].rem_euclid(side) + s[2].rem_euclid(side) * side + s[1].rem_euclid(HEIGHT) * side * side)
        as usize
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
        self.window.as_ref().map(|w| (w.buffer.raw, w.index))
    }

    /// Stages what changed in the window around `eye` (blocks), making the
    /// buffer the first time and again when the reach grows. Does nothing
    /// unless a pack wants voxels.
    pub fn update(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        uploads: &mut Uploads,
        eye: Vec3,
        frame: u64,
        out: &mut Vec<Upload>,
    ) -> Result<(), SetupError> {
        if !crate::terrain::voxels::wanted() {
            return Ok(());
        }
        let side = (crate::terrain::voxels::distance() as i32 / 16).clamp(2, 64) * 2;
        if self.window.as_ref().is_none_or(|w| w.side != side) {
            let pages = pages_for(side);
            let buffer = memory.buffer(
                ctx,
                "voxel window",
                header_bytes(side) + u64::from(pages) * PAGE_BYTES,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuOnly,
            )?;
            let index = bindless
                .add_buffer(ctx, buffer.raw)
                .ok_or_else(|| SetupError("the bindless set is full".into()))?;
            if let Some(old) = self.window.take() {
                // Its index stays bound until the frames reading it end.
                bindless.remove_buffer(old.index, frame);
                self.retired.push(old.buffer);
            }
            self.window = Some(Window {
                buffer,
                index,
                side,
                pages,
                slots: vec![(None, None); (side * side * HEIGHT) as usize],
                free: (0..pages).rev().collect(),
                magic: false,
                settled: None,
            });
        }
        let Some(w) = self.window.as_mut() else {
            return Ok(());
        };
        let header = header_bytes(w.side);
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
        if !w.magic {
            let words = [MAGIC, w.side as u32, HEIGHT as u32, w.pages];
            w.magic = stage(bytemuck::cast_slice(&words), 0, out);
        }
        let half = w.side / 2;
        let base = [
            (eye.x / 16.0 + 0.5).floor() as i32 - half,
            (eye.y / 16.0).floor() as i32 - 3,
            (eye.z / 16.0 + 0.5).floor() as i32 - half,
        ];
        if w.settled == Some((base, self.generation)) {
            return Ok(());
        }
        let mut complete = true;
        // Nearest first, so the budget goes where reflections are seen.
        let mut order: Vec<[i32; 3]> = Vec::with_capacity(w.slots.len());
        for dy in 0..HEIGHT {
            for dz in 0..w.side {
                for dx in 0..w.side {
                    order.push([base[0] + dx, base[1] + dy, base[2] + dz]);
                }
            }
        }
        let centre = [base[0] + half, base[1] + 3, base[2] + half];
        order.sort_by_key(|s| {
            let d = [s[0] - centre[0], s[1] - centre[1], s[2] - centre[2]];
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        });
        let mut spent = 0u64;
        for s in order {
            let slot = slot_of(w.side, s);
            let entry = self.store.get(&s);
            let want = (s, entry.map_or(0, |e| e.1));
            if w.slots[slot].0 == Some(want) {
                continue;
            }
            let filled = entry.filter(|(v, _)| !v.cells.is_empty());
            let state = match (entry, filled) {
                (None, _) => UNKNOWN,
                (Some(_), None) => EMPTY,
                (Some(_), Some(_)) => FILLED,
            };
            let mut word = state;
            if let Some((v, _)) = filled {
                if spent + PAGE_BYTES > BUDGET {
                    complete = false;
                    continue;
                }
                let dist = |q: [i32; 3]| {
                    let d = [q[0] - centre[0], q[1] - centre[1], q[2] - centre[2]];
                    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
                };
                let page = match w.slots[slot].1.or_else(|| w.free.pop()) {
                    Some(p) => p,
                    // Out of pages: take the farthest filled section's, if
                    // it is farther than this one (it reads unknown).
                    None => {
                        let mine = dist(s);
                        let far = w
                            .slots
                            .iter()
                            .enumerate()
                            .filter_map(|(i, (held, page))| {
                                let (q, _) = (*held)?;
                                page.map(|_| (i, dist(q)))
                            })
                            .filter(|&(_, d)| d > mine)
                            .max_by_key(|&(_, d)| d);
                        let Some((victim, _)) = far else {
                            complete = false;
                            continue;
                        };
                        let Some(q) = w.slots[victim].0.map(|(q, _)| q) else {
                            continue;
                        };
                        let words = [q[0] as u32, q[1] as u32, q[2] as u32, UNKNOWN];
                        if !stage(bytemuck::cast_slice(&words), 16 + victim as u64 * 16, out) {
                            return Ok(());
                        }
                        // Unknown now: it asks again once a page is free.
                        w.slots[victim].0 = Some((q, u64::MAX));
                        match w.slots[victim].1.take() {
                            Some(p) => p,
                            None => continue,
                        }
                    }
                };
                let at = header + u64::from(page) * PAGE_BYTES;
                if !stage(bytemuck::cast_slice(&v.dense()), at, out) {
                    return Ok(());
                }
                spent += PAGE_BYTES;
                w.slots[slot].1 = Some(page);
                word = FILLED | page << 2;
            } else if let Some(page) = w.slots[slot].1.take() {
                w.free.push(page);
            }
            let words = [s[0] as u32, s[1] as u32, s[2] as u32, word];
            if !stage(bytemuck::cast_slice(&words), 16 + slot as u64 * 16, out) {
                return Ok(());
            }
            w.slots[slot].0 = Some(want);
        }
        if complete {
            w.settled = Some((base, self.generation));
        }
        Ok(())
    }

    pub fn destroy(self, ctx: &Context, memory: &Memory) {
        if let Some(w) = self.window {
            memory.free_buffer(ctx, w.buffer);
        }
        for b in self.retired {
            memory.free_buffer(ctx, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_gives_every_section_its_own_slot() {
        let side = 12;
        let mut seen = std::collections::HashSet::new();
        for y in -3..5 {
            for z in -6..6 {
                for x in -6..6 {
                    assert!(seen.insert(slot_of(side, [x + 100, y - 7, z - 31])));
                }
            }
        }
    }
}
