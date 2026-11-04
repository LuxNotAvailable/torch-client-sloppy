use crate::platform::time::Instant;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bevy::prelude::*;

use super::systems::{ChunkIndex, SectionSlot, WorldCamera};
use super::terrain_pool::{TerrainOp, TerrainOps};
use super::visgraph::{Direction, VisibilitySet};

#[derive(Resource, Default)]
pub struct OcclusionState {
    section: Option<(i32, i32, i32)>,
    generation: u64,
    filled_at: Option<Instant>,
    grid: Grid,
    spare: Grid,
    snapshot: VisSnapshot,
    task: Option<FillTask>,
    /// The last accepted fill's visible slots, waiting to be pushed.
    bits: Vec<u32>,
    reached: usize,
    pub fills: u64,
}

const TERRAIN_REFILL_INTERVAL: Duration = Duration::from_millis(250);

/// Boxes spanning more sections than this on an axis are never culled.
const MAX_CULL_SPAN: i32 = 4;

impl OcclusionState {
    /// Whether the last flood fill reached any section the box overlaps, so
    /// something inside it may be seen. Unknown counts as reached: before the
    /// first fill, with occlusion off, or outside the filled grid.
    pub fn reaches_box(&self, min: Vec3, max: Vec3) -> bool {
        if self.section.is_none() || disabled() {
            return true;
        }
        let column = |v: f32| (v / 16.0).floor() as i32;
        let (x0, x1) = (column(min.x), column(max.x));
        let (y0, y1) = (column(min.y), column(max.y));
        let (z0, z1) = (column(min.z), column(max.z));
        if x1 - x0 >= MAX_CULL_SPAN || y1 - y0 >= MAX_CULL_SPAN || z1 - z0 >= MAX_CULL_SPAN {
            return true;
        }
        let grid = &self.grid;
        (y0..=y1).any(|y| {
            (z0..=z1).any(|z| {
                (x0..=x1).any(|x| grid.index((x, y * 16, z)).is_none_or(|i| grid.reached[i]))
            })
        })
    }
}

#[derive(Default)]
struct Grid {
    origin: (i32, i32, i32),
    dim_x: usize,
    dim_y: usize,
    dim_z: usize,
    sources: Vec<u8>,
    directions: Vec<u8>,
    seen: Vec<bool>,
    reached: Vec<bool>,
    list: Vec<(i32, i32, i32)>,
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

    fn index(&self, pos: (i32, i32, i32)) -> Option<usize> {
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

    fn fill<F>(&mut self, here: (i32, i32, i32), radius: i32, y_lo: i32, y_hi: i32, vis_at: F)
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

    fn visible_sections(&self) -> impl Iterator<Item = (i32, i32, i32)> + '_ {
        self.list.iter().copied()
    }
}

#[derive(Default)]
struct VisSnapshot {
    origin: (i32, i32, i32),
    dim_x: usize,
    dim_y: usize,
    dim_z: usize,
    cells: Vec<VisibilitySet>,
    /// Each cell's terrain slots (opaque, water), so the fill task can mark
    /// the visible ones without the main thread's index.
    slots: Vec<[Option<u32>; 2]>,
    /// The index's slot count when the snapshot was taken.
    slot_count: u32,
}

impl VisSnapshot {
    fn reload<I>(
        &mut self,
        sections: I,
        slot_count: u32,
        here: (i32, i32, i32),
        radius: i32,
        y_lo: i32,
        y_hi: i32,
    ) where
        I: Iterator<Item = ((i32, i32, i32), SectionSlot)>,
    {
        self.slot_count = slot_count;
        self.origin = (here.0 - radius, y_lo, here.2 - radius);
        self.dim_x = (2 * radius + 1) as usize;
        self.dim_z = (2 * radius + 1) as usize;
        self.dim_y = (((y_hi - y_lo) / 16) + 1).max(1) as usize;
        let cells = self.dim_x * self.dim_y * self.dim_z;
        self.cells.clear();
        self.cells.resize(cells, VisibilitySet::all());
        self.slots.clear();
        self.slots.resize(cells, [None, None]);
        for (pos, section) in sections {
            if let Some(i) = self.index(pos) {
                self.cells[i] = section.vis;
                self.slots[i] = section.slots;
            }
        }
    }

    fn index(&self, pos: (i32, i32, i32)) -> Option<usize> {
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

    /// One bit per terrain slot, set for each slot in a section the fill
    /// reached, and how many sections it reached.
    fn visible_bits(&self, grid: &Grid) -> (Vec<u32>, usize) {
        let words = (self.slot_count as usize).div_ceil(32).max(1);
        let mut bits = vec![0u32; words];
        let mut reached = 0;
        for pos in grid.visible_sections() {
            reached += 1;
            let Some(i) = self.index(pos) else {
                continue;
            };
            for slot in self.slots[i].into_iter().flatten() {
                if let Some(word) = bits.get_mut(slot as usize / 32) {
                    *word |= 1 << (slot % 32);
                }
            }
        }
        (bits, reached)
    }

    fn at(&self, pos: (i32, i32, i32)) -> VisibilitySet {
        match self.index(pos) {
            Some(i) => self.cells[i],
            None => VisibilitySet::all(),
        }
    }
}

struct FillResult {
    grid: Grid,
    snapshot: VisSnapshot,
    bits: Vec<u32>,
    reached: usize,
    section: (i32, i32, i32),
    generation: u64,
}

#[cfg(not(target_arch = "wasm32"))]
type FillTask = bevy::tasks::Task<FillResult>;
#[cfg(target_arch = "wasm32")]
type FillTask = FillResult;

fn run_fill(
    mut grid: Grid,
    snapshot: VisSnapshot,
    here: (i32, i32, i32),
    radius: i32,
    y_lo: i32,
    y_hi: i32,
    generation: u64,
) -> FillResult {
    grid.fill(here, radius, y_lo, y_hi, |pos| snapshot.at(pos));
    // Built here rather than on the main thread: one index lookup per reached
    // section took about 3 ms at RD 32 on a fast desktop, 9-11 on a laptop.
    let (bits, reached) = snapshot.visible_bits(&grid);
    FillResult {
        grid,
        snapshot,
        bits,
        reached,
        section: here,
        generation,
    }
}

fn spawn_fill(
    grid: Grid,
    snapshot: VisSnapshot,
    here: (i32, i32, i32),
    radius: i32,
    y_lo: i32,
    y_hi: i32,
    generation: u64,
) -> FillTask {
    #[cfg(not(target_arch = "wasm32"))]
    {
        bevy::tasks::AsyncComputeTaskPool::get()
            .spawn(async move { run_fill(grid, snapshot, here, radius, y_lo, y_hi, generation) })
    }
    #[cfg(target_arch = "wasm32")]
    {
        run_fill(grid, snapshot, here, radius, y_lo, y_hi, generation)
    }
}

fn install(
    state: &mut OcclusionState,
    result: FillResult,
    here: (i32, i32, i32),
    now: Instant,
) -> bool {
    state.snapshot = result.snapshot;
    if result.section != here {
        state.spare = result.grid;
        return false;
    }
    state.spare = std::mem::replace(&mut state.grid, result.grid);
    state.bits = result.bits;
    state.reached = result.reached;
    state.section = Some(result.section);
    state.generation = result.generation;
    state.filled_at = Some(now);
    true
}

fn poll_fill(task: &mut Option<FillTask>) -> Option<FillResult> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let pending = task.as_mut()?;
        let done = bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(pending));
        if done.is_some() {
            *task = None;
        }
        done
    }
    #[cfg(target_arch = "wasm32")]
    {
        task.take()
    }
}

fn section_of(y: f32) -> i32 {
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

fn disabled() -> bool {
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| {
        matches!(
            crate::platform::env::var("MC_NO_OCCLUSION").as_deref(),
            Ok("1") | Ok("true")
        )
    })
}

pub fn update_occlusion(
    mut state: ResMut<OcclusionState>,
    index: Res<ChunkIndex>,
    gui: Res<crate::gui::GuiState>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    ops: Res<TerrainOps>,
    mut hiding_everything: Local<bool>,
    mut disabled_pushed: Local<bool>,
) {
    crate::prof_span!("render:update_occlusion");
    #[cfg(feature = "budget")]
    let _t = crate::diag::budget::timed(crate::diag::budget::Slot::Occlusion);
    if disabled() {
        if !*disabled_pushed && index.next_slot() > 0 {
            let words = (index.next_slot() as usize).div_ceil(32).max(1);
            ops.push(TerrainOp::Visibility(Arc::from(vec![u32::MAX; words])));
            *disabled_pushed = true;
        }
        return;
    }
    let Ok(transform) = camera.single() else {
        return;
    };

    let eye = transform.translation();
    let here = (
        (eye.x / 16.0).floor() as i32,
        section_of(eye.y),
        (eye.z / 16.0).floor() as i32,
    );

    let now = Instant::now();

    let mut refill = false;
    if let Some(result) = poll_fill(&mut state.task) {
        refill = install(&mut state, result, here, now);
    }

    let terrain_changed = state.generation != index.generation();
    let terrain_due = state
        .filled_at
        .is_none_or(|at| now.duration_since(at) >= TERRAIN_REFILL_INTERVAL);

    if state.task.is_none() && (state.section != Some(here) || (terrain_changed && terrain_due)) {
        let radius = gui.options.render_distance as i32 + 1;
        let (y_lo, y_hi) = index.y_range();
        let generation = index.generation();
        let grid = std::mem::take(&mut state.spare);
        let mut snapshot = std::mem::take(&mut state.snapshot);
        snapshot.reload(
            index.sections(),
            index.next_slot(),
            here,
            radius,
            y_lo,
            y_hi,
        );
        state.fills += 1;
        state.task = Some(spawn_fill(
            grid, snapshot, here, radius, y_lo, y_hi, generation,
        ));
    }

    #[cfg(target_arch = "wasm32")]
    if let Some(result) = poll_fill(&mut state.task) {
        refill |= install(&mut state, result, here, now);
    }

    if state.section.is_none() || !refill {
        return;
    }

    // Slots made after the snapshot lie past the bits' end, which the cull
    // treats as visible until the next fill.
    let reached_sections = state.reached;
    ops.push(TerrainOp::Visibility(Arc::from(std::mem::take(
        &mut state.bits,
    ))));

    crate::log_debug!(
        "render",
        "occlusion: {} indexed, {reached_sections} reached, refill={refill}, camera {:?}",
        index.len(),
        here
    );

    let indexed = index.len();
    let anomalous = indexed >= 16 && reached_sections * 4 < indexed;
    if anomalous != *hiding_everything {
        *hiding_everything = anomalous;
        if anomalous {
            crate::log_warn!(
                "render",
                "occlusion is hiding {} of {indexed} loaded sections from section {here:?}; the \
                 flood fill reached {reached_sections} sections. If the terrain is visibly \
                 disappearing rather than being correctly culled behind rock, MC_NO_OCCLUSION=1 \
                 draws every loaded section and says whether this module is the cause.",
                indexed - reached_sections,
            );
        } else {
            crate::log_info!(
                "render",
                "occlusion is back to normal: {reached_sections} of {indexed} sections visible",
            );
        }
    }

    #[cfg(feature = "budget")]
    crate::diag::budget::note_visible_columns(reached_sections);
}

pub struct OcclusionPlugin;

impl Plugin for OcclusionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OcclusionState>()
            .add_systems(PostUpdate, update_occlusion);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::time::Instant;

    fn surface_world(ground: i32) -> impl Fn((i32, i32, i32)) -> VisibilitySet {
        move |pos: (i32, i32, i32)| {
            if pos.1 < ground {
                VisibilitySet::none()
            } else {
                VisibilitySet::all()
            }
        }
    }

    fn time_fill<F>(here: (i32, i32, i32), world: &F, runs: u32) -> (usize, f64)
    where
        F: Fn((i32, i32, i32)) -> VisibilitySet,
    {
        let (radius, y_lo, y_hi) = (33, -64, 320);
        let mut grid = Grid::default();
        grid.fill(here, radius, y_lo, y_hi, world);
        let start = Instant::now();
        for _ in 0..runs {
            grid.fill(here, radius, y_lo, y_hi, world);
        }
        (
            grid.list.len(),
            start.elapsed().as_secs_f64() * 1000.0 / runs as f64,
        )
    }

    #[test]
    fn fill_cost_at_render_distance_32() {
        let world = surface_world(64);
        let (surface, surface_ms) = time_fill((0, 80, 0), &world, 20);
        println!("surface:     {surface} reached, {surface_ms:.2} ms per fill");

        let (buried, buried_ms) = time_fill((0, 0, 0), &world, 20);
        println!("underground: {buried} reached, {buried_ms:.2} ms per fill");

        let open = |_: (i32, i32, i32)| VisibilitySet::all();
        let (air, air_ms) = time_fill((0, 80, 0), &open, 20);
        println!("open air:    {air} reached, {air_ms:.2} ms per fill");

        assert!(
            buried < surface,
            "a camera buried in solid rock reached {buried} sections, no fewer than \
             the {surface} reached in open air: the visibility sets are not \
             stopping the fill"
        );
    }

    #[test]
    fn reached_and_grid_agree() {
        let world = surface_world(64);
        let mut grid = Grid::default();
        grid.fill((0, 80, 0), 4, -64, 320, &world);
        for pos in grid.list.clone() {
            assert!(
                grid.index(pos).is_some(),
                "{pos:?} reached but outside the grid"
            );
            assert!(
                grid.reached[grid.index(pos).unwrap()],
                "{pos:?} listed but not marked"
            );
        }
        assert!(!grid.reached[grid.index((0, -32, 0)).unwrap()]);
    }

    #[test]
    fn a_box_buried_past_the_fill_is_culled_and_one_in_the_open_is_not() {
        let mut state = OcclusionState::default();
        let open = Vec3::new(4.0, 82.0, 4.0);
        assert!(
            state.reaches_box(open, open),
            "no fill yet: draw everything"
        );

        state.grid.fill((0, 80, 0), 4, -64, 320, &surface_world(64));
        state.section = Some((0, 80, 0));
        assert!(state.reaches_box(open, open + 1.0));
        let buried = Vec3::new(4.0, -30.0, 4.0);
        assert!(!state.reaches_box(buried, buried + 1.0));
        let far = Vec3::new(4000.0, -30.0, 4.0);
        assert!(
            state.reaches_box(far, far + 1.0),
            "outside the grid is unknown"
        );
    }

    #[test]
    fn main_thread_cost_at_render_distance_32() {
        let radius = 33;
        let mut index = ChunkIndex::default();
        let mut slot = 0;
        for cx in -radius..=radius {
            for cz in -radius..=radius {
                for sec_y in (-64..320).step_by(16) {
                    index.index_for_test(cx, cz, sec_y, [Some(slot), None]);
                    slot += 1;
                }
            }
        }
        let (y_lo, y_hi) = index.y_range();
        let here = (0, 80, 0);
        let mut snapshot = VisSnapshot::default();
        let runs = 10;
        let start = Instant::now();
        for _ in 0..runs {
            snapshot.reload(
                index.sections(),
                index.next_slot(),
                here,
                radius,
                y_lo,
                y_hi,
            );
        }
        let reload_ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;

        let mut grid = Grid::default();
        grid.fill(here, radius, y_lo, y_hi, &|pos| snapshot.at(pos));
        let start = Instant::now();
        let mut reached = 0;
        let mut bits = Vec::new();
        for _ in 0..runs {
            (bits, reached) = snapshot.visible_bits(&grid);
        }
        let bits_ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        println!(
            "{} indexed: snapshot reload {reload_ms:.2} ms on the main thread, visibility bits {bits_ms:.2} ms in the fill task for {reached} reached",
            index.len()
        );
        // The bits match the index's own lookups.
        for (cx, sec_y, cz) in grid.visible_sections() {
            if let (Some(slot), _) = index.slots_of(cx, cz, sec_y) {
                assert_ne!(bits[slot as usize / 32] & (1 << (slot % 32)), 0);
            }
        }
        let marked: u32 = bits.iter().map(|w| w.count_ones()).sum();
        let expected = grid
            .visible_sections()
            .filter(|&(cx, sec_y, cz)| index.slots_of(cx, cz, sec_y).0.is_some())
            .count();
        assert_eq!(marked as usize, expected);
    }
}
