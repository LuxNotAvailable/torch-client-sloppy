//! Face occlusion by shape, as vanilla's `Block.shouldRenderFace`: a face
//! on a block's side is hidden when the neighbour's opposite side covers
//! it. Each baked block state gets a coverage mask per side, 16 by 16 (one
//! bit per sixteenth of a block), from its opaque faces lying on that side.
//! A stair's back and bottom, a slab's bottom, a path's bottom are full;
//! two slabs or two stairs side by side cover each other's halves exactly.
//!
//! Masks are interned in one table shared by every mesher: a block state
//! keeps its table index (`RenderedBlock::shape`, 0 for none), and the
//! occupancy grid holds that index per cell.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::bake::BakedBlock;

/// One side's coverage: bit `v * 16 + u` for the sixteenth at (u, v).
pub type Mask = [u64; 4];
/// A block's six sides, by `Direction` order (down, up, north, south,
/// west, east).
pub type Sides = [Mask; 6];

/// For a side: the axis it faces along, where on that axis it lies (0 or
/// 1), and the two axes it spans (u, v). Both blocks of a pair use the
/// same u and v for the plane between them, so masks compare directly.
const PLANES: [(usize, f32, usize, usize); 6] = [
    (1, 0.0, 0, 2),
    (1, 1.0, 0, 2),
    (2, 0.0, 0, 1),
    (2, 1.0, 0, 1),
    (0, 0.0, 2, 1),
    (0, 1.0, 2, 1),
];

/// What a quad covers on side `side`, if it lies flat on that side.
pub fn footprint(pos: &[[f32; 3]; 4], side: usize) -> Option<Mask> {
    let (axis, at, u, v) = *PLANES.get(side)?;
    if pos.iter().any(|p| (p[axis] - at).abs() > 1e-4) {
        return None;
    }
    let sixteenth = |x: f32| (x * 16.0).round().clamp(0.0, 16.0) as u32;
    let (mut u0, mut u1, mut v0, mut v1) = (16, 0, 16, 0);
    for p in pos {
        u0 = u0.min(sixteenth(p[u]));
        u1 = u1.max(sixteenth(p[u]));
        v0 = v0.min(sixteenth(p[v]));
        v1 = v1.max(sixteenth(p[v]));
    }
    if u0 >= u1 || v0 >= v1 {
        return None;
    }
    let row = (((1u32 << (u1 - u0)) - 1) << u0) as u64;
    let mut mask = [0u64; 4];
    for v in v0..v1 {
        let bit = v * 16;
        mask[(bit / 64) as usize] |= row << (bit % 64);
    }
    Some(mask)
}

/// Whether `outer` covers every bit of `inner`.
#[inline]
pub fn covers(outer: &Mask, inner: &Mask) -> bool {
    (0..4).all(|i| inner[i] & !outer[i] == 0)
}

/// A block's coverage per side, from the opaque faces of its first model
/// choice per part (random variants share their shape).
pub fn sides_of(baked: &BakedBlock) -> Sides {
    let mut sides = [[0u64; 4]; 6];
    for part in &baked.parts {
        let Some(model) = part.choices.first() else {
            continue;
        };
        for quad in &model.quads {
            if !quad.opaque {
                continue;
            }
            let side = quad.face as usize;
            if let Some(mask) = footprint(&quad.pos, side) {
                for (out, bits) in sides[side].iter_mut().zip(mask) {
                    *out |= bits;
                }
            }
        }
    }
    sides
}

struct Table {
    index: HashMap<Sides, u32>,
    shapes: Arc<Vec<Sides>>,
}

fn table() -> &'static Mutex<Table> {
    static TABLE: OnceLock<Mutex<Table>> = OnceLock::new();
    TABLE.get_or_init(|| {
        Mutex::new(Table {
            index: HashMap::new(),
            // Index 0: covers nothing.
            shapes: Arc::new(vec![[[0; 4]; 6]]),
        })
    })
}

/// The table index of a block's sides; 0 when they cover nothing.
pub fn intern(sides: Sides) -> u32 {
    if sides.iter().all(|m| m.iter().all(|b| *b == 0)) {
        return 0;
    }
    let mut table = table().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&at) = table.index.get(&sides) {
        return at;
    }
    let at = table.shapes.len() as u32;
    Arc::make_mut(&mut table.shapes).push(sides);
    table.index.insert(sides, at);
    at
}

/// Every shape interned so far, for a mesher to read without locking.
pub fn shapes() -> Arc<Vec<Sides>> {
    table()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .shapes
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(points: [[f32; 3]; 4]) -> [[f32; 3]; 4] {
        points
    }

    #[test]
    fn a_full_side_covers_a_half_and_a_half_does_not_cover_a_full() {
        let full = footprint(
            &quad([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
            0,
        )
        .unwrap();
        let half = footprint(
            &quad([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.5], [0.0, 0.0, 0.5]]),
            0,
        )
        .unwrap();
        assert_eq!(full, [u64::MAX; 4]);
        assert!(covers(&full, &half));
        assert!(!covers(&half, &full));
        assert!(covers(&half, &half));
    }

    #[test]
    fn a_face_off_the_side_has_no_footprint() {
        let inset = quad([[0.0, 0.5, 0.0], [1.0, 0.5, 0.0], [1.0, 0.5, 1.0], [0.0, 0.5, 1.0]]);
        assert!(footprint(&inset, 0).is_none());
        assert!(footprint(&inset, 1).is_none());
    }

    #[test]
    fn interning_shares_an_index_and_empty_is_zero() {
        assert_eq!(intern([[0; 4]; 6]), 0);
        let mut sides = [[0; 4]; 6];
        sides[3] = [1, 2, 3, 4];
        let a = intern(sides);
        assert_ne!(a, 0);
        assert_eq!(intern(sides), a);
        assert_eq!(shapes()[a as usize], sides);
    }
}
