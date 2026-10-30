use azalea::block::fluid_state::{FluidKind, FluidState};
use simdnbt::owned::NbtCompound;

use super::TickCtx;

const VALID_BLOCKS: [&str; 4] = [
    "prismarine",
    "prismarine_bricks",
    "sea_lantern",
    "dark_prismarine",
];
const MIN_ACTIVE_SIZE: usize = 16;
const MIN_KILL_SIZE: usize = 42;
const ROTATION_SPEED: f32 = -0.0375;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Data {}

pub fn decode(_tag: &NbtCompound) -> Data {
    Data::default()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Conduit {
    pub tick_count: u32,
    pub active_rotation: u32,
    pub active: bool,
    pub hunting: bool,
}

impl Conduit {
    pub fn tick(&mut self, ctx: &TickCtx) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if ctx.game_time % 40 == 0 {
            let frame = frame_size(ctx);
            self.active = frame.is_some_and(|size| size >= MIN_ACTIVE_SIZE);
            self.hunting = frame.is_some_and(|size| size >= MIN_KILL_SIZE);
        }
        if self.active {
            self.active_rotation = self.active_rotation.wrapping_add(1);
        }
    }

    pub fn active_rotation(&self, partial: f32) -> f32 {
        (self.active_rotation as f32 + partial) * ROTATION_SPEED
    }

    pub fn animation_phase(&self) -> u32 {
        self.tick_count / 66 % 3
    }
}

fn frame_size(ctx: &TickCtx) -> Option<usize> {
    let [x, y, z] = ctx.pos;
    for (ox, oy, oz) in cube(1) {
        let water = ctx
            .block([x + ox, y + oy, z + oz])
            .is_some_and(|state| FluidState::from(state).kind == FluidKind::Water);
        if !water {
            return None;
        }
    }
    let count = cube(2)
        .filter(|&(ox, oy, oz)| {
            let (ax, ay, az) = (ox.abs(), oy.abs(), oz.abs());
            (ax > 1 || ay > 1 || az > 1)
                && (ox == 0 && (ay == 2 || az == 2)
                    || oy == 0 && (ax == 2 || az == 2)
                    || oz == 0 && (ax == 2 || ay == 2))
        })
        .filter(|&(ox, oy, oz)| {
            ctx.block([x + ox, y + oy, z + oz]).is_some_and(|state| {
                let block = crate::blocks::info::info(state);
                VALID_BLOCKS.contains(&block.id)
            })
        })
        .count();
    Some(count)
}

fn cube(radius: i32) -> impl Iterator<Item = (i32, i32, i32)> {
    (-radius..=radius).flat_map(move |x| {
        (-radius..=radius).flat_map(move |y| (-radius..=radius).map(move |z| (x, y, z)))
    })
}
