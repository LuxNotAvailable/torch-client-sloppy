//! The world's section changes, handed to the renderer a frame's budget at a
//! time. Every listener (block entity caches, the native renderer's terrain
//! feed and dump) hears them here, so no renderer owns the hooks.

use std::collections::HashSet;
use std::sync::OnceLock;

use super::PendingSection;
use crate::session::SessionWorld;

const DEFAULT_VERT_BUDGET: usize = 120_000;
const DEFAULT_SECTION_BUDGET: usize = 128;

/// One frame's section changes, in order: clear or drop columns, then store.
#[derive(Default)]
pub struct Batch {
    pub clear_all: bool,
    pub unloaded: Vec<(i32, i32)>,
    pub sections: Vec<PendingSection>,
}

/// Reused dedup buffers.
#[derive(Default)]
pub struct Scratch {
    seen: HashSet<(i32, i32, i32)>,
    keep: Vec<bool>,
}

/// Takes this frame's changes off `world` within the upload budgets
/// (`MC_UPLOAD_VERTS`, `MC_UPLOAD_SECTIONS`). Runs under the session lock,
/// so it only moves data; `finish` does the rest. `None` when idle.
pub fn take(world: &mut SessionWorld) -> Option<Batch> {
    if let Some(q) = crate::CHUNK_Q.get() {
        q.set_backlog(world.pending_chunks.len() + world.pending_edits.len());
    }
    let unloaded = std::mem::take(&mut world.unloaded_chunks);
    let clear_all = std::mem::take(&mut world.clear_chunks);
    if world.pending_chunks.is_empty()
        && world.pending_edits.is_empty()
        && unloaded.is_empty()
        && !clear_all
    {
        return None;
    }

    static VERT_BUDGET: OnceLock<usize> = OnceLock::new();
    static SECTION_BUDGET: OnceLock<usize> = OnceLock::new();
    let mut verts = env_budget(&VERT_BUDGET, "MC_UPLOAD_VERTS", DEFAULT_VERT_BUDGET);
    let mut sections = env_budget(
        &SECTION_BUDGET,
        "MC_UPLOAD_SECTIONS",
        DEFAULT_SECTION_BUDGET,
    );
    let mut pending = take_budgeted(&mut world.pending_edits, &mut verts, &mut sections);
    pending.extend(take_budgeted(
        &mut world.pending_chunks,
        &mut verts,
        &mut sections,
    ));
    Some(Batch {
        clear_all,
        unloaded,
        sections: pending,
    })
}

/// Outside the lock: drops sections of columns no longer loaded, keeps the
/// last copy of each section, and tells every listener.
pub fn finish(batch: &mut Batch, scratch: &mut Scratch) {
    crate::log_debug!("render", "uploading {} sections", batch.sections.len());
    crate::client::tracking::retain_loaded(&mut batch.sections, |s| (s.chunk_x, s.chunk_z));

    let Scratch { seen, keep } = scratch;
    seen.clear();
    keep.clear();
    keep.extend(
        batch
            .sections
            .iter()
            .rev()
            .map(|c| seen.insert((c.chunk_x, c.chunk_z, c.sec_y))),
    );
    keep.reverse();
    let mut next = 0;
    batch.sections.retain(|_| {
        let kept = keep[next];
        next += 1;
        kept
    });

    if batch.clear_all {
        #[cfg(feature = "vulkan")]
        crate::vk::terrain_dump::cleared();
        crate::blockentities::transient::clear();
        crate::blockentities::bake::clear();
    } else {
        for &(cx, cz) in &batch.unloaded {
            #[cfg(feature = "vulkan")]
            crate::vk::terrain_dump::column_dropped(cx, cz);
            #[cfg(not(feature = "vulkan"))]
            let _ = (cx, cz);
        }
    }
    for sec in &batch.sections {
        let origin = [sec.chunk_x * 16, sec.sec_y, sec.chunk_z * 16];
        #[cfg(feature = "vulkan")]
        crate::vk::terrain_dump::section(origin, sec.vis, [&sec.opaque, &sec.water]);
        crate::blockentities::transient::section_rebuilt(origin);
        crate::blockentities::bake::section_rebuilt(origin);
    }
}

fn take_budgeted(
    queue: &mut Vec<PendingSection>,
    verts: &mut usize,
    sections: &mut usize,
) -> Vec<PendingSection> {
    if queue.is_empty() || *verts == 0 || *sections == 0 {
        return Vec::new();
    }
    let mut room = 0usize;
    for sec in queue.iter() {
        room += 1;
        let n = sec.opaque.verts.len() + sec.water.verts.len();
        let over = n >= *verts;
        *verts = verts.saturating_sub(n);
        if over || room >= *sections {
            break;
        }
    }
    *sections -= room.min(*sections);
    queue.drain(..room).collect()
}

fn env_budget(lock: &'static OnceLock<usize>, name: &str, default: usize) -> usize {
    *lock.get_or_init(|| {
        crate::platform::env::var(name)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(default)
    })
}
