//! Server-sent transient blocks (`ClientboundAddTransientBlock`): a block drawn
//! as a moving block for up to a second, bridging the gap until its section's
//! terrain mesh catches up.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

const TIME_TO_LIVE: Duration = Duration::from_secs(1);

struct Transient {
    pos: [i32; 3],
    state: u32,
    created: Instant,
}

static LIVE: Mutex<Vec<Transient>> = Mutex::new(Vec::new());

pub fn add(pos: [i32; 3], state: u32) {
    let mut live = LIVE.lock();
    live.retain(|t| t.pos != pos);
    live.push(Transient {
        pos,
        state,
        created: Instant::now(),
    });
}

/// A fresh mesh for the section at `origin` has landed: anything queued
/// before it is now part of the terrain.
pub fn section_rebuilt(origin: [i32; 3]) {
    let mut live = LIVE.lock();
    if live.is_empty() {
        return;
    }
    let now = Instant::now();
    live.retain(|t| {
        t.created >= now
            || t.pos[0].div_euclid(16) * 16 != origin[0]
            || t.pos[1].div_euclid(16) * 16 != origin[1]
            || t.pos[2].div_euclid(16) * 16 != origin[2]
    });
}

pub fn clear() {
    LIVE.lock().clear();
}

/// Drops expired entries and calls `draw` for the rest.
pub fn each(mut draw: impl FnMut([i32; 3], u32)) {
    let mut live = LIVE.lock();
    live.retain(|t| t.created.elapsed() < TIME_TO_LIVE);
    for t in live.iter() {
        draw(t.pos, t.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rebuild_of_its_own_section_retires_a_transient_block() {
        clear();
        add([17, -3, 5], 1);
        add([40, 70, 40], 2);
        section_rebuilt([16, -16, 0]);
        let mut left = Vec::new();
        each(|pos, state| left.push((pos, state)));
        assert_eq!(left, vec![([40, 70, 40], 2)]);
        clear();
    }
}
