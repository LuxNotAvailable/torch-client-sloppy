//! Vanilla particle classes with their own `tick`: drips, leaves, fireflies, ink, sparks.

#![allow(unused_imports)]

use super::*;

impl Drip {
    pub(super) fn of(p: &P) -> Option<Drip> {
        Some(match p {
            P::DrippingWater => Drip::WaterHang,
            P::DrippingLava => Drip::LavaHang,
            P::DrippingHoney => Drip::HoneyHang,
            P::DrippingObsidianTear => Drip::TearHang,
            P::DrippingDripstoneWater => Drip::DripstoneWaterHang,
            P::DrippingDripstoneLava => Drip::DripstoneLavaHang,
            P::FallingWater => Drip::WaterFall,
            P::FallingLava => Drip::LavaFall,
            P::FallingHoney => Drip::HoneyFall,
            P::FallingObsidianTear => Drip::TearFall,
            P::FallingDripstoneWater => Drip::DripstoneWaterFall,
            P::FallingDripstoneLava => Drip::DripstoneLavaFall,
            P::FallingNectar => Drip::NectarFall,
            P::FallingSporeBlossom => Drip::SporeFall,
            P::LandingLava => Drip::LavaLand,
            P::LandingHoney => Drip::HoneyLand,
            P::LandingObsidianTear => Drip::TearLand,
            _ => return None,
        })
    }

    pub(super) fn hangs(self) -> bool {
        matches!(
            self,
            Drip::WaterHang | Drip::LavaHang | Drip::HoneyHang | Drip::TearHang | Drip::DripstoneWaterHang | Drip::DripstoneLavaHang
        )
    }

    pub(super) fn falls(self) -> bool {
        matches!(
            self,
            Drip::WaterFall
                | Drip::LavaFall
                | Drip::HoneyFall
                | Drip::TearFall
                | Drip::DripstoneWaterFall
                | Drip::DripstoneLavaFall
                | Drip::NectarFall
                | Drip::SporeFall
        )
    }

    pub(super) fn cools(self) -> bool {
        matches!(self, Drip::LavaHang | Drip::DripstoneLavaHang)
    }

    pub(super) fn glows(self) -> bool {
        matches!(self, Drip::TearHang | Drip::TearFall | Drip::TearLand)
    }

    /// The fluid it vanishes into: 1 water, 2 lava, 0 none.
    pub(super) fn fluid(self) -> u8 {
        match self {
            Drip::WaterHang | Drip::DripstoneWaterHang | Drip::WaterFall | Drip::DripstoneWaterFall => 1,
            Drip::LavaHang | Drip::DripstoneLavaHang | Drip::LavaFall | Drip::DripstoneLavaFall | Drip::LavaLand => 2,
            _ => 0,
        }
    }

    /// What a hanging drip turns into, or a falling one lands as.
    pub(super) fn next(self) -> Option<P> {
        Some(match self {
            Drip::WaterHang => P::FallingWater,
            Drip::LavaHang => P::FallingLava,
            Drip::HoneyHang => P::FallingHoney,
            Drip::TearHang => P::FallingObsidianTear,
            Drip::DripstoneWaterHang => P::FallingDripstoneWater,
            Drip::DripstoneLavaHang => P::FallingDripstoneLava,
            Drip::WaterFall | Drip::DripstoneWaterFall => P::Splash,
            Drip::LavaFall | Drip::DripstoneLavaFall => P::LandingLava,
            Drip::HoneyFall => P::LandingHoney,
            Drip::TearFall => P::LandingObsidianTear,
            _ => return None,
        })
    }

    /// The sound a landing makes.
    pub(super) fn land_sound(self) -> Option<&'static str> {
        match self {
            Drip::HoneyFall => Some("block.beehive.drip"),
            Drip::DripstoneWaterFall => Some("block.pointed_dripstone.drip_water"),
            Drip::DripstoneLavaFall => Some("block.pointed_dripstone.drip_lava"),
            _ => None,
        }
    }

    pub(super) const WATER: [f32; 3] = [0.2, 0.3, 1.0];
    pub(super) const LAVA: [f32; 3] = [1.0, 0.285_714_3, 0.083_333_336];
    pub(super) const TEAR: [f32; 3] = [0.511_718_75, 0.031_25, 0.890_625];
}

/// Whether `pos` sits in `fluid` (1 water, 2 lava) below its surface.
pub(super) fn in_fluid(world: Option<&azalea_world::World>, pos: Vec3, fluid: u8) -> bool {
    use azalea::block::fluid_state::{FluidKind, FluidState};
    let Some(world) = world else { return false };
    let cell = azalea_core::position::BlockPos::new(pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
    let Some(state) = world.get_block_state(cell) else { return false };
    let f = FluidState::from(state);
    let kind = match fluid {
        1 => FluidKind::Water,
        2 => FluidKind::Lava,
        _ => return false,
    };
    f.kind == kind && pos.y < cell.y as f32 + f.height()
}

pub(super) fn is_air_at(world: Option<&azalea_world::World>, pos: Vec3) -> bool {
    let Some(world) = world else { return true };
    let cell = azalea_core::position::BlockPos::new(pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
    world.get_block_state(cell).is_none_or(|s| s.is_air())
}

/// What a behaviour asks for after a tick: to die, and what to set off.
pub(super) struct After {
    pub(super) remove: bool,
    pub(super) spawn: Option<(P, Vec3)>,
    pub(super) sound: Option<(&'static str, Vec3)>,
}

/// A behaviour's tick, after the plain motion; `on_ground` is that
/// motion's landing.
pub(super) fn tick_behavior(p: &mut Particle, world: Option<&azalea_world::World>, on_ground: bool, rng: &mut JavaRandom) -> After {
    let mut after = After { remove: false, spawn: None, sound: None };
    match &mut p.behavior {
        Behavior::None | Behavior::Spark(_) => {}
        Behavior::Drip(drip) => {
            let drip = *drip;
            if drip.hangs() {
                p.velocity *= 0.02;
                if drip.cools() {
                    let left = p.lifetime.saturating_sub(p.age) as f32;
                    p.tint = [1.0, 16.0 / (40.0 - left + 16.0), 4.0 / (40.0 - left + 8.0), 1.0];
                }
                if p.age >= p.lifetime {
                    after.spawn = drip.next().map(|n| (n, p.pos));
                }
            } else if drip.falls() && on_ground {
                after.remove = true;
                after.spawn = drip.next().map(|n| (n, p.pos));
                after.sound = drip.land_sound().map(|s| (s, p.pos));
            }
            if drip.fluid() != 0 && in_fluid(world, p.pos, drip.fluid()) {
                after.remove = true;
                after.spawn = None;
            }
        }
        Behavior::Leaf(leaf) => {
            let alive = p.age as f32;
            let relative = (alive / 300.0).min(1.0);
            let (mut xa, mut za) = (0.0f32, 0.0f32);
            if leaf.flow {
                xa += leaf.x_flow * relative.powf(1.25);
                za += leaf.z_flow * relative.powf(1.25);
            }
            if leaf.swirl {
                xa += relative * (relative * leaf.swirl_period).cos() * leaf.wind;
                za += relative * (relative * leaf.swirl_period).sin() * leaf.wind;
            }
            p.velocity.x += xa * 0.0025;
            p.velocity.z += za * 0.0025;
            leaf.rot_speed += leaf.spin / 20.0;
            leaf.old_roll = leaf.roll;
            leaf.roll += leaf.rot_speed / 20.0;
            if on_ground || (p.age > 1 && (p.velocity.x == 0.0 || p.velocity.z == 0.0)) {
                after.remove = true;
            }
        }
        Behavior::Firefly => {
            if !is_air_at(world, p.pos) {
                after.remove = true;
            }
            let progress = (p.age as f32 / p.lifetime as f32).clamp(0.0, 1.0);
            let fade = |in_time: f32, out_time: f32| {
                if progress >= 1.0 - in_time {
                    (1.0 - progress) / in_time
                } else if progress <= out_time {
                    progress / out_time
                } else {
                    1.0
                }
            };
            p.tint[3] = fade(0.3, 0.5);
            if rng.next_f32() > 0.95 || p.age == 1 {
                p.velocity = Vec3::new(
                    -0.05 + 0.1 * rng.next_f32(),
                    -0.05 + 0.1 * rng.next_f32(),
                    -0.05 + 0.1 * rng.next_f32(),
                );
            }
        }
        Behavior::SquidInk => {
            if is_air_at(world, p.pos) {
                p.velocity.y -= 0.0074;
            }
        }
        Behavior::Snowflake => {
            p.velocity.x *= 0.95;
            p.velocity.y *= 0.9;
            p.velocity.z *= 0.95;
        }
        Behavior::FlyStraight { start, offset, from, to } => {
            let t = p.age as f32 / p.lifetime.max(1) as f32;
            let left = 1.0 - t;
            p.pos = Vec3::from(*start) + Vec3::from(*offset) * left;
            let lerp = |shift: u32| {
                let a = ((*from >> shift) & 0xFF) as f32 / 255.0;
                let b = ((*to >> shift) & 0xFF) as f32 / 255.0;
                a + (b - a) * t
            };
            p.tint = [lerp(16), lerp(8), lerp(0), lerp(24)];
            p.velocity = Vec3::ZERO;
        }
        Behavior::Bubble { y_start, y_end, y_prev, size_start } => {
            if *y_prev == f32::MIN {
                *y_start = p.prev.y;
                *y_end = p.prev.y + 3.0;
                *y_prev = p.prev.y;
            }
            if !in_fluid(world, p.pos, 1) || p.pos.y >= *y_end || p.pos.y <= *y_prev {
                after.remove = true;
            }
            let wiggle = |rng: &mut JavaRandom| rng.next_f32() * 0.003 * if rng.next_int(2) == 0 { 1.0 } else { -1.0 } * 0.5;
            p.velocity.x += wiggle(rng);
            p.velocity.z += wiggle(rng);
            let progress = (p.pos.y - *y_start) / (*y_end - *y_start).max(1e-3);
            p.size = *size_start + progress * (0.15 - *size_start);
            *y_prev = p.pos.y;
        }
    }
    after
}

/// `SparkParticle.tick` and `SimpleAnimatedParticle.tick`: past half its
/// life the colour eases to the fade colour; through its first half a
/// trailing spark leaves a still copy every other tick.
pub(super) fn tick_spark(p: &mut Particle, spark: Spark, trails: &mut Vec<Particle>) {
    if p.age > p.lifetime / 2
        && let Some(fade) = spark.fade
    {
        for (c, f) in p.tint.iter_mut().zip(fade) {
            *c += (f - *c) * 0.2;
        }
    }
    if spark.trail && p.age < p.lifetime / 2 && (p.age + p.lifetime) % 2 == 0 {
        let mut copy = Particle {
            prev: p.pos,
            pos: p.pos,
            velocity: Vec3::ZERO,
            gravity: p.gravity,
            friction: p.friction,
            tick_acc: 0.0,
            age: 0,
            lifetime: p.lifetime,
            tint: [p.tint[0], p.tint[1], p.tint[2], 0.99],
            size: p.size,
            grow: p.grow,
            sprite: p.sprite,
            flags: p.flags,
            behavior: Behavior::Spark(Spark { trail: false, ..spark }),
        };
        copy.age = copy.lifetime / 2;
        trails.push(copy);
    }
}
