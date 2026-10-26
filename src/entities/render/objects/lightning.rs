use azalea_registry::builtin::EntityKind;
use bevy::prelude::Mesh;

use crate::entities::quads::QuadMesh;
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::{Blend, RenderSpec, RootPose, TexturePath};
use crate::util::javarandom::JavaRandom;

const SEEDS: i32 = 64;
const COLOR: [f32; 4] = [0.45, 0.45, 0.5, 0.3];
const STEP_HEIGHT: f32 = 16.0;
const STEPS: usize = 8;

pub fn register(registry: &mut Registry) {
    registry.add(
        EntityKind::LightningBolt,
        RenderSpec::built("lightning_bolt", seed_key, bolt_mesh, untextured)
            .with_root(bolt_root)
            .with_blend(Blend::Additive)
            .with_tint(|_| COLOR)
            .ignoring_invisibility(),
    );
}

fn seed(st: &EntityState) -> i64 {
    st.id.rem_euclid(SEEDS) as i64
}

fn seed_key(st: &EntityState) -> TexturePath {
    seed(st).to_string().into()
}

fn untextured(_st: &EntityState) -> Option<TexturePath> {
    None
}

fn bolt_root(_st: &EntityState) -> RootPose {
    RootPose::default()
}

fn jitter(random: &mut JavaRandom, range: u32) -> f32 {
    random.next_int(range * 2 + 1) as f32 - range as f32
}

fn bolt_mesh(st: &EntityState) -> Mesh {
    let seed = seed(st);
    let mut x_offs = [0.0f32; STEPS];
    let mut z_offs = [0.0f32; STEPS];
    let (mut x_off, mut z_off) = (0.0f32, 0.0f32);
    let mut random = JavaRandom::new(seed);
    for h in (0..STEPS).rev() {
        x_offs[h] = x_off;
        z_offs[h] = z_off;
        x_off += jitter(&mut random, 5);
        z_off += jitter(&mut random, 5);
    }

    let mut out = QuadMesh::new();
    for r in 0..4 {
        let mut random = JavaRandom::new(seed);
        for p in 0..3 {
            let hs = if p > 0 { 7 - p } else { 7 };
            let ht = if p > 0 { hs - 2 } else { 0 };
            let mut xo0 = x_offs[hs] - x_off;
            let mut zo0 = z_offs[hs] - z_off;
            for h in (ht..=hs).rev() {
                let (xo1, zo1) = (xo0, zo0);
                let range = if p == 0 { 5 } else { 15 };
                xo0 += jitter(&mut random, range);
                zo0 += jitter(&mut random, range);
                let mut rr1 = 0.1 + r as f32 * 0.2;
                let mut rr2 = 0.1 + r as f32 * 0.2;
                if p == 0 {
                    rr1 *= h as f32 * 0.1 + 1.0;
                    rr2 *= (h as f32 - 1.0) * 0.1 + 1.0;
                }
                let strip = Strip {
                    xo0,
                    zo0,
                    xo1,
                    zo1,
                    h: h as f32,
                    rr1,
                    rr2,
                };
                for corners in [
                    [false, false, true, false],
                    [true, false, true, true],
                    [true, true, false, true],
                    [false, true, false, false],
                ] {
                    strip.quad(&mut out, corners);
                }
            }
        }
    }
    out.finish()
}

struct Strip {
    xo0: f32,
    zo0: f32,
    xo1: f32,
    zo1: f32,
    h: f32,
    rr1: f32,
    rr2: f32,
}

impl Strip {
    fn quad(&self, out: &mut QuadMesh, [px1, pz1, px2, pz2]: [bool; 4]) {
        let sign = |positive: bool, r: f32| if positive { r } else { -r };
        let (bottom, top) = (self.h * STEP_HEIGHT, (self.h + 1.0) * STEP_HEIGHT);
        out.untextured(
            [
                [
                    self.xo0 + sign(px1, self.rr2),
                    bottom,
                    self.zo0 + sign(pz1, self.rr2),
                ],
                [
                    self.xo1 + sign(px1, self.rr1),
                    top,
                    self.zo1 + sign(pz1, self.rr1),
                ],
                [
                    self.xo1 + sign(px2, self.rr1),
                    top,
                    self.zo1 + sign(pz2, self.rr1),
                ],
                [
                    self.xo0 + sign(px2, self.rr2),
                    bottom,
                    self.zo0 + sign(pz2, self.rr2),
                ],
            ],
            [0.0, 1.0, 0.0],
        );
    }
}
