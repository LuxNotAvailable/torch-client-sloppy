//! Aggressive entity culling: entities and block entities hidden behind
//! solid blocks are not drawn, as the EntityCulling mod does.
//!
//! The section flood fill (`OcclusionState::reaches_box`) already skips
//! whatever sits in a section the camera cannot see into, but anything in a
//! visible section still draws, though a wall stands between it and the eye.
//! Here rays from the eye to the centre and corners of each one's box are
//! walked block by block on the client thread, which holds the world; when
//! every ray meets a solid block, it is hidden.
//!
//! The render thread asks each frame (`request_entities`, `request_blocks`), the client thread answers
//! each tick (`run`), and the render thread reads the answer (`entity_hidden`,
//! `block_hidden`). An answer is a tick late, so a box is grown before it is
//! tested, and something is only hidden once two answers in a row agree; it
//! shows again on the first answer that sees it.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use azalea::BlockPos;

/// The Video Settings toggle.
static ENABLED: AtomicBool = AtomicBool::new(true);

static REQUEST: Mutex<Option<Request>> = Mutex::new(None);
static ANSWER: std::sync::LazyLock<Mutex<Answer>> =
    std::sync::LazyLock::new(|| Mutex::new(Answer::default()));

/// Nearer than this, everything draws: a ray this short is cheap to be wrong
/// about, and a popped-in mob at arm's length would be noticed.
const ALWAYS_DRAWN: f64 = 4.0;
/// Farther than this, rays are not walked and everything draws.
const MAX_TESTED: f64 = 128.0;
/// How far a box is grown before it is tested, for the tick it is late.
const GROW: f64 = 0.5;
/// Answers in a row that must find something hidden before it is hidden.
const HIDE_AFTER: u8 = 2;

#[derive(Default)]
struct Request {
    eye: [f64; 3],
    entities: Vec<(i32, [f64; 3], [f64; 3])>,
    blocks: Vec<[i32; 3]>,
}

#[derive(Default)]
struct Answer {
    entities: HashSet<i32>,
    blocks: HashSet<[i32; 3]>,
    entity_streak: HashMap<i32, u8>,
    block_streak: HashMap<[i32; 3], u8>,
}

pub fn set_enabled(on: bool) {
    if ENABLED.swap(on, Relaxed) && !on {
        let mut answer = ANSWER.lock().unwrap_or_else(|e| e.into_inner());
        *answer = Answer::default();
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Relaxed)
}

/// The entities the render thread draws this frame, by box (feet to head),
/// seen from `eye`. Replaces the last frame's.
pub fn request_entities(eye: [f64; 3], entities: Vec<(i32, [f64; 3], [f64; 3])>) {
    if !enabled() {
        return;
    }
    let mut request = REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    let request = request.get_or_insert_with(Request::default);
    request.eye = eye;
    request.entities = entities;
}

/// The block entities the render thread draws this frame, by block.
/// Replaces the last frame's.
pub fn request_blocks(blocks: Vec<[i32; 3]>) {
    if !enabled() {
        return;
    }
    let mut request = REQUEST.lock().unwrap_or_else(|e| e.into_inner());
    request.get_or_insert_with(Request::default).blocks = blocks;
}

pub fn entity_hidden(id: i32) -> bool {
    enabled()
        && ANSWER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entities
            .contains(&id)
}

pub fn block_hidden(pos: [i32; 3]) -> bool {
    enabled()
        && ANSWER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .blocks
            .contains(&pos)
}

/// Answers the latest request against `world`, on the client thread.
pub fn run(world: &azalea_world::World) {
    if !enabled() {
        return;
    }
    // The request stays for the next tick (the render thread replaces each
    // half every frame it draws), so a tick with no new frame still answers.
    let request = {
        let guard = REQUEST.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(r) => Request {
                eye: r.eye,
                entities: r.entities.clone(),
                blocks: r.blocks.clone(),
            },
            None => return,
        }
    };
    let solid = |x: i32, y: i32, z: i32| {
        world
            .get_block_state(BlockPos::new(x, y, z))
            .is_some_and(crate::util::block_model::is_solid)
    };
    let eye = request.eye;
    let entities: Vec<(i32, bool)> = request
        .entities
        .iter()
        .map(|&(id, min, max)| (id, box_hidden(eye, min, max, &solid)))
        .collect();
    let blocks: Vec<([i32; 3], bool)> = request
        .blocks
        .iter()
        .map(|&pos| {
            let min = pos.map(f64::from);
            let max = min.map(|v| v + 1.0);
            (pos, box_hidden(eye, min, max, &solid))
        })
        .collect();

    let mut answer = ANSWER.lock().unwrap_or_else(|e| e.into_inner());
    let answer = &mut *answer;
    answer.entities = settle(&mut answer.entity_streak, entities);
    answer.blocks = settle(&mut answer.block_streak, blocks);
}

/// Counts how many answers in a row found each one hidden, and returns
/// those hidden for long enough. Ones no longer asked about are forgotten.
fn settle<K: std::hash::Hash + Eq + Copy>(
    streak: &mut HashMap<K, u8>,
    found: Vec<(K, bool)>,
) -> HashSet<K> {
    let mut next = HashMap::with_capacity(found.len());
    let mut hidden = HashSet::new();
    for (key, is_hidden) in found {
        if !is_hidden {
            continue;
        }
        let count = streak.get(&key).copied().unwrap_or(0).saturating_add(1);
        next.insert(key, count);
        if count >= HIDE_AFTER {
            hidden.insert(key);
        }
    }
    *streak = next;
    hidden
}

/// Whether solid blocks stand between `eye` and every one of the grown box's
/// centre and eight corners.
fn box_hidden(
    eye: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
    solid: &impl Fn(i32, i32, i32) -> bool,
) -> bool {
    let min = min.map(|v| v - GROW);
    let max = max.map(|v| v + GROW);
    let centre = [0, 1, 2].map(|a| (min[a] + max[a]) * 0.5);
    let nearest = [0, 1, 2].map(|a| eye[a].clamp(min[a], max[a]));
    let distance = distance(eye, nearest);
    if distance < ALWAYS_DRAWN || distance > MAX_TESTED {
        return false;
    }
    if (0..3).all(|a| eye[a] >= min[a] && eye[a] <= max[a]) {
        return false;
    }
    let corners = (0..8).map(|i| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ]
    });
    std::iter::once(centre)
        .chain(corners)
        .all(|target| ray_blocked(eye, target, solid))
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Walks the blocks a ray from `from` to `to` passes through (Amanatides and
/// Woo), leaving out the block it starts in and the one it ends in, and
/// whether one of them is solid.
fn ray_blocked(from: [f64; 3], to: [f64; 3], solid: &impl Fn(i32, i32, i32) -> bool) -> bool {
    let dir = [0, 1, 2].map(|a| to[a] - from[a]);
    let mut cell = from.map(|v| v.floor() as i32);
    let end = to.map(|v| v.floor() as i32);
    let step = dir.map(|d| if d > 0.0 { 1 } else { -1 });
    let delta = dir.map(|d| if d == 0.0 { f64::INFINITY } else { 1.0 / d.abs() });
    let mut next = [0, 1, 2].map(|a| {
        if dir[a] == 0.0 {
            f64::INFINITY
        } else if dir[a] > 0.0 {
            (f64::from(cell[a]) + 1.0 - from[a]) * delta[a]
        } else {
            (from[a] - f64::from(cell[a])) * delta[a]
        }
    });
    for _ in 0..(MAX_TESTED as usize * 3) {
        let axis = if next[0] < next[1] {
            if next[0] < next[2] { 0 } else { 2 }
        } else if next[1] < next[2] {
            1
        } else {
            2
        };
        if next[axis] > 1.0 {
            return false;
        }
        cell[axis] += step[axis];
        next[axis] += delta[axis];
        if cell == end {
            return false;
        }
        if solid(cell[0], cell[1], cell[2]) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall at x = 10, from y -8 to 8 and z -8 to 8.
    fn wall(x: i32, y: i32, z: i32) -> bool {
        x == 10 && (-8..=8).contains(&y) && (-8..=8).contains(&z)
    }

    #[test]
    fn a_box_behind_a_wall_is_hidden_and_one_in_front_is_not() {
        let eye = [0.5, 0.5, 0.5];
        assert!(box_hidden(eye, [14.0, 0.0, 0.0], [14.6, 1.8, 0.6], &wall));
        assert!(!box_hidden(eye, [7.0, 0.0, 0.0], [7.6, 1.8, 0.6], &wall));
    }

    #[test]
    fn a_box_peeking_past_the_wall_edge_is_drawn() {
        let eye = [0.5, 0.5, 0.5];
        assert!(!box_hidden(eye, [14.0, 0.0, 13.0], [14.6, 1.8, 13.6], &wall));
    }

    #[test]
    fn near_and_far_boxes_always_draw() {
        let solid_everywhere = |_: i32, _: i32, _: i32| true;
        let eye = [0.5, 0.5, 0.5];
        assert!(!box_hidden(eye, [2.0, 0.0, 0.0], [2.6, 1.8, 0.6], &solid_everywhere));
        assert!(!box_hidden(eye, [300.0, 0.0, 0.0], [300.6, 1.8, 0.6], &solid_everywhere));
    }

    #[test]
    fn hiding_waits_for_two_answers_and_showing_does_not() {
        let mut streak = HashMap::new();
        assert!(settle(&mut streak, vec![(1, true)]).is_empty());
        assert!(settle(&mut streak, vec![(1, true)]).contains(&1));
        assert!(settle(&mut streak, vec![(1, false)]).is_empty());
        assert!(settle(&mut streak, vec![(1, true)]).is_empty());
    }
}
