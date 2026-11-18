//! Cave visibility: the flood fill over section visibility sets that decides
//! which sections the camera can see into.

use std::collections::VecDeque;

use super::visgraph::{Direction, VisibilitySet};

#[derive(Default)]
pub(crate) struct Grid {
    origin: (i32, i32, i32),
    dim_x: usize,
    dim_y: usize,
    dim_z: usize,
    sources: Vec<u8>,
    directions: Vec<u8>,
    seen: Vec<bool>,
    pub(crate) reached: Vec<bool>,
    pub(crate) list: Vec<(i32, i32, i32)>,
}

impl Grid {
    fn resize(&mut self, here: (i32, i32, i32), radius: i32, y_lo: i32, y_hi: i32) {
        self.origin = (here.0 - radius, y_lo, here.2 - radius);
        self.dim_x = (2 * radius + 1) as usize;
        self.dim_z = (2 * radius + 1) as usize;
        self.dim_y = (((y_hi - y_lo) / 16) + 1).max(1) as usize;
        let cells = self.dim_x * self.dim_y * self.dim_z;
        self.sources.clear();
        self.sources.resize(cells, 0);
        self.directions.clear();
        self.directions.resize(cells, 0);
        self.seen.clear();
        self.seen.resize(cells, false);
        self.reached.clear();
        self.reached.resize(cells, false);
        self.list.clear();
    }

    pub(crate) fn index(&self, pos: (i32, i32, i32)) -> Option<usize> {
        let x = pos.0 - self.origin.0;
        let z = pos.2 - self.origin.2;
        let y = (pos.1 - self.origin.1).div_euclid(16);
        if x < 0 || z < 0 || y < 0 {
            return None;
        }
        let (x, y, z) = (x as usize, y as usize, z as usize);
        if x >= self.dim_x || y >= self.dim_y || z >= self.dim_z {
            return None;
        }
        Some((y * self.dim_z + z) * self.dim_x + x)
    }

    pub(crate) fn fill<F>(&mut self, here: (i32, i32, i32), radius: i32, y_lo: i32, y_hi: i32, vis_at: F)
    where
        F: Fn((i32, i32, i32)) -> VisibilitySet,
    {
        self.resize(here, radius, y_lo, y_hi);
        let mut queue: VecDeque<(i32, i32, i32)> = VecDeque::with_capacity(1024);

        match self.index(here) {
            Some(i) => {
                self.seen[i] = true;
                queue.push_back(here);
            }
            None => {
                let above = here.1 > y_hi;
                let plane = if above { y_hi } else { y_lo };
                let source = 1u8
                    << if above {
                        Direction::Down.index()
                    } else {
                        Direction::Up.index()
                    };
                let mut seeds: Vec<(i32, i32, i32)> = Vec::new();
                for dx in -radius..=radius {
                    for dz in -radius..=radius {
                        let mut directions = source;
                        if dx > 0 {
                            directions |= 1 << Direction::East.index();
                        } else if dx < 0 {
                            directions |= 1 << Direction::West.index();
                        }
                        if dz > 0 {
                            directions |= 1 << Direction::South.index();
                        } else if dz < 0 {
                            directions |= 1 << Direction::North.index();
                        }
                        let pos = (here.0 + dx, plane, here.2 + dz);
                        let Some(i) = self.index(pos) else { continue };
                        self.seen[i] = true;
                        self.sources[i] = source;
                        self.directions[i] = directions;
                        seeds.push(pos);
                    }
                }
                seeds.sort_by_key(|p| {
                    let dx = (p.0 - here.0) as i64;
                    let dz = (p.2 - here.2) as i64;
                    dx * dx + dz * dz
                });
                queue.extend(seeds);
            }
        }

        while let Some(pos) = queue.pop_front() {
            let Some(i) = self.index(pos) else { continue };
            let (sources, directions) = (self.sources[i], self.directions[i]);
            self.reached[i] = true;
            self.list.push(pos);
            let vis = vis_at(pos);
            for face in Direction::ALL {
                if directions & (1 << face.opposite().index()) != 0 {
                    continue;
                }
                if sources != 0 {
                    let connected = Direction::ALL.iter().any(|src| {
                        sources & (1 << src.index()) != 0
                            && vis.visible_between(src.opposite(), face)
                    });
                    if !connected {
                        continue;
                    }
                }
                let next = step(pos, face);
                let Some(j) = self.index(next) else { continue };
                if self.seen[j] {
                    self.sources[j] |= 1 << face.index();
                    continue;
                }
                self.seen[j] = true;
                self.sources[j] = 1 << face.index();
                self.directions[j] = directions | (1 << face.index());
                queue.push_back(next);
            }
        }
    }

    pub(crate) fn visible_sections(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        self.list.iter().copied()
    }
}

/// The cave-visibility flood fill on its own, for the Vulkan renderer
/// (VULKAN_REWRITE_CHECKLIST.md 3.2): the sections (chunk x, block y of the
/// section, chunk z) reachable from `here` within `radius` columns, given
/// each section's visibility set.
pub fn flood_fill<F>(
    here: (i32, i32, i32),
    radius: i32,
    y_lo: i32,
    y_hi: i32,
    vis_at: F,
) -> Vec<(i32, i32, i32)>
where
    F: Fn((i32, i32, i32)) -> VisibilitySet,
{
    let mut grid = Grid::default();
    grid.fill(here, radius, y_lo, y_hi, vis_at);
    grid.list
}

/// The block y of the section holding block height `y`.
pub fn section_y(y: f32) -> i32 {
    section_of(y)
}

pub(crate) fn section_of(y: f32) -> i32 {
    (y / 16.0).floor() as i32 * 16
}

fn step(pos: (i32, i32, i32), face: Direction) -> (i32, i32, i32) {
    let (x, y, z) = pos;
    match face {
        Direction::Down => (x, y - 16, z),
        Direction::Up => (x, y + 16, z),
        Direction::North => (x, y, z - 1),
        Direction::South => (x, y, z + 1),
        Direction::West => (x - 1, y, z),
        Direction::East => (x + 1, y, z),
    }
}

