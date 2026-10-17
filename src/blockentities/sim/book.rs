use super::TickCtx;
use crate::util::mth::wrap_rad;

const FLIP_RATE: f32 = 0.2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Book {
    pub time: u32,
    pub flip: f32,
    pub o_flip: f32,
    flip_t: f32,
    flip_a: f32,
    pub open: f32,
    pub o_open: f32,
    pub rot: f32,
    pub o_rot: f32,
    t_rot: f32,
    rng: u64,
}

impl Book {
    fn next_int(&mut self, bound: u64) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.rng >> 33) % bound
    }

    fn seed(&mut self, pos: [i32; 3]) {
        let [x, y, z] = pos.map(|v| v as u32 as u64);
        self.rng = ((x << 40) ^ (y << 20) ^ z).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    }

    pub fn tick(&mut self, ctx: &TickCtx) {
        if self.rng == 0 {
            self.seed(ctx.pos);
        }
        let center = ctx.center();
        self.o_open = self.open;
        self.o_rot = self.rot;
        if let Some(player) = ctx.nearest_player(center, 3.0) {
            self.t_rot = (player[2] - center[2]).atan2(player[0] - center[0]) as f32;
            self.open += 0.1;
            if self.open < 0.5 || self.next_int(40) == 0 {
                let old = self.flip_t;
                while old == self.flip_t {
                    let step = self.next_int(4) as f32 - self.next_int(4) as f32;
                    self.flip_t += step;
                }
            }
        } else {
            self.t_rot += 0.02;
            self.open -= 0.1;
        }

        self.rot = wrap_rad(self.rot);
        self.t_rot = wrap_rad(self.t_rot);
        self.rot += wrap_rad(self.t_rot - self.rot) * 0.4;
        self.open = self.open.clamp(0.0, 1.0);
        self.time += 1;
        self.o_flip = self.flip;
        let diff = ((self.flip_t - self.flip) * 0.4).clamp(-FLIP_RATE, FLIP_RATE);
        self.flip_a += (diff - self.flip_a) * 0.9;
        self.flip += self.flip_a;
    }
}
