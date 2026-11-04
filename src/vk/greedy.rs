//! How much greedy meshing would save on real terrain
//! (VULKAN_REWRITE_CHECKLIST.md 3.9), measured before anyone builds it:
//! `vkview --terrain DIR --greedy` counts the opaque quads of a dump that
//! could merge with their neighbours.
//!
//! A quad can merge when it is a whole block face on the block grid, its
//! four corners share one light and one colour (so a merged quad shades the
//! same), and its neighbour in the plane has the same texture, uv layout,
//! light and colour. Merging such runs into rectangles (the usual greedy
//! sweep, per section, face direction and plane) gives the quad count a
//! greedy mesher would emit.

use std::collections::HashMap;

use super::terrain_dump::Section;
use crate::renderer::mesh::PackedVertex;

/// Quads before and after merging.
#[derive(Clone, Copy, Debug, Default)]
pub struct Estimate {
    pub quads: u64,
    /// Quads that are whole, evenly lit block faces.
    pub candidates: u64,
    pub after: u64,
}

impl Estimate {
    /// The share of quads merging would remove.
    pub fn saving(&self) -> f64 {
        1.0 - self.after as f64 / self.quads.max(1) as f64
    }
}

/// A face that may merge: what must match its neighbour's.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Look {
    uv: [[u16; 2]; 4],
    light: [u8; 4],
    color: [u8; 4],
}

pub fn estimate(sections: &[Section]) -> Estimate {
    let mut total = Estimate::default();
    for section in sections.iter().filter(|s| !s.water) {
        // (direction, plane) -> cell -> look id
        let mut planes: HashMap<(usize, i32), HashMap<(i32, i32), usize>> = HashMap::new();
        let mut looks: HashMap<Look, usize> = HashMap::new();
        for quad in section.idx.chunks_exact(6) {
            total.quads += 1;
            let first = quad.iter().copied().min().unwrap_or(0) as usize;
            let Some(corners) = section.verts.get(first..first + 4) else {
                total.after += 1;
                continue;
            };
            match face(corners) {
                Some((dir, plane, cell, look)) => {
                    total.candidates += 1;
                    let next = looks.len();
                    let id = *looks.entry(look).or_insert(next);
                    planes.entry((dir, plane)).or_default().insert(cell, id);
                }
                None => total.after += 1,
            }
        }
        for cells in planes.values() {
            total.after += rectangles(cells);
        }
    }
    total
}

/// A whole, evenly lit block face: its direction, plane, cell in the plane
/// and look; `None` for anything else.
fn face(corners: &[PackedVertex]) -> Option<(usize, i32, (i32, i32), Look)> {
    let light = corners[0].light;
    let color = corners[0].color;
    if corners.iter().any(|v| v.light != light || v.color != color) {
        return None;
    }
    let pos: Vec<[i32; 3]> = corners
        .iter()
        .map(|v| [v.pos[0] as i32, v.pos[1] as i32, v.pos[2] as i32])
        .collect();
    let lo: [i32; 3] = std::array::from_fn(|a| pos.iter().map(|p| p[a]).min().unwrap());
    let hi: [i32; 3] = std::array::from_fn(|a| pos.iter().map(|p| p[a]).max().unwrap());
    let flat: Vec<usize> = (0..3).filter(|&a| lo[a] == hi[a]).collect();
    let [axis] = flat[..] else { return None };
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    if hi[u] - lo[u] != 256 || hi[v] - lo[v] != 256 || pos.iter().flatten().any(|c| c % 256 != 0) {
        return None;
    }
    // The uv at each corner, in corner order, so the texture's layout
    // (rotation, flips) must match too.
    let mut uv = [[0u16; 2]; 4];
    for (p, vertex) in pos.iter().zip(corners) {
        let corner = usize::from(p[u] > lo[u]) + 2 * usize::from(p[v] > lo[v]);
        uv[corner] = vertex.uv;
    }
    // Which side the face looks to, from its winding.
    let (a, b, c) = (pos[0], pos[1], pos[2]);
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = e1[(axis + 1) % 3] as i64 * e2[(axis + 2) % 3] as i64
        - e1[(axis + 2) % 3] as i64 * e2[(axis + 1) % 3] as i64;
    let dir = axis * 2 + usize::from(n < 0);
    Some((
        dir,
        lo[axis],
        (lo[u] / 256, lo[v] / 256),
        Look { uv, light, color },
    ))
}

/// The rectangles a greedy sweep covers `cells` with, matching looks only.
fn rectangles(cells: &HashMap<(i32, i32), usize>) -> u64 {
    let mut left: HashMap<(i32, i32), usize> = cells.clone();
    let mut keys: Vec<(i32, i32)> = cells.keys().copied().collect();
    keys.sort_unstable_by_key(|&(a, b)| (b, a));
    let mut count = 0;
    for start in keys {
        let Some(&look) = left.get(&start) else {
            continue;
        };
        let mut width = 1;
        while left.get(&(start.0 + width, start.1)) == Some(&look) {
            width += 1;
        }
        let mut height = 1;
        'grow: loop {
            for x in 0..width {
                if left.get(&(start.0 + x, start.1 + height)) != Some(&look) {
                    break 'grow;
                }
            }
            height += 1;
        }
        for y in 0..height {
            for x in 0..width {
                left.remove(&(start.0 + x, start.1 + y));
            }
        }
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[((i32, i32), usize)]) -> u64 {
        rectangles(&cells.iter().copied().collect())
    }

    #[test]
    fn a_uniform_square_is_one_rectangle_and_a_checkerboard_none() {
        let square: Vec<_> = (0..4)
            .flat_map(|x| (0..4).map(move |y| ((x, y), 0)))
            .collect();
        assert_eq!(rows(&square), 1);
        let checker: Vec<_> = (0..4)
            .flat_map(|x| (0..4).map(move |y| ((x, y), ((x + y) % 2) as usize)))
            .collect();
        assert_eq!(rows(&checker), 16);
        // An L: two rectangles.
        assert_eq!(rows(&[((0, 0), 0), ((1, 0), 0), ((0, 1), 0)]), 2);
    }
}
