//! Predicting the minecart this player rides: `OldMinecartBehavior` on rails.
//!
//! Vanilla's client only interpolates a minecart toward the server's
//! positions, so the cart, and its rider, trail the server. Here the ridden
//! cart runs the server's rail physics each tick. Each server position is
//! checked against the recent predictions. A miss rebases on the server and
//! replays the ticks since, and the jump is eased out.

use std::collections::VecDeque;
use std::sync::Mutex;

use azalea::block::BlockState;
use azalea::block::properties::{Powered, RailShape, Shape};
use azalea::ecs::prelude::*;
use azalea::entity::indexing::EntityIdIndex;
use azalea::entity::{EntityKindComponent, LocalEntity, LookDirection, Physics, Position};
use azalea::physics::client_movement::ClientMovementState;
use azalea::physics::collision::BlockWithShape;
use azalea::world::{World, WorldName, Worlds};
use azalea_core::aabb::Aabb;
use azalea_core::entity_id::MinecraftEntityId;
use azalea_core::position::{BlockPos, Vec3};
use azalea_registry::builtin::{BlockKind, EntityKind};

use crate::util::mth::wrap_degrees;

/// `AbstractMinecart.EXITS`'s shapes, normal and straight rails alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rail {
    NorthSouth,
    EastWest,
    AscendingEast,
    AscendingWest,
    AscendingNorth,
    AscendingSouth,
    SouthEast,
    SouthWest,
    NorthWest,
    NorthEast,
}

type Exit = (i32, i32, i32);

impl Rail {
    /// `AbstractMinecart.exits`.
    fn exits(self) -> (Exit, Exit) {
        const W: Exit = (-1, 0, 0);
        const E: Exit = (1, 0, 0);
        const N: Exit = (0, 0, -1);
        const S: Exit = (0, 0, 1);
        const fn below(e: Exit) -> Exit {
            (e.0, e.1 - 1, e.2)
        }
        match self {
            Rail::NorthSouth => (N, S),
            Rail::EastWest => (W, E),
            Rail::AscendingEast => (below(W), E),
            Rail::AscendingWest => (W, below(E)),
            Rail::AscendingNorth => (N, below(S)),
            Rail::AscendingSouth => (below(N), S),
            Rail::SouthEast => (S, E),
            Rail::SouthWest => (S, W),
            Rail::NorthWest => (N, W),
            Rail::NorthEast => (N, E),
        }
    }

    fn is_slope(self) -> bool {
        matches!(
            self,
            Rail::AscendingEast | Rail::AscendingWest | Rail::AscendingNorth | Rail::AscendingSouth
        )
    }
}

/// What the cart needs from the world: rails, conductors, solid blocks.
trait Blocks {
    /// A rail at `pos`: its shape and, for a powered rail, whether it's on.
    fn rail(&self, pos: BlockPos) -> Option<(Rail, Option<bool>)>;
    fn conductor(&self, pos: BlockPos) -> bool;
    /// Collision boxes touching `area`.
    fn boxes(&self, area: &Aabb) -> Vec<Aabb>;
}

fn rail_of(state: BlockState) -> Option<(Rail, Option<bool>)> {
    let kind = BlockKind::from(state);
    if kind == BlockKind::Rail {
        let shape = match state.property::<Shape>()? {
            Shape::NorthSouth => Rail::NorthSouth,
            Shape::EastWest => Rail::EastWest,
            Shape::AscendingEast => Rail::AscendingEast,
            Shape::AscendingWest => Rail::AscendingWest,
            Shape::AscendingNorth => Rail::AscendingNorth,
            Shape::AscendingSouth => Rail::AscendingSouth,
            Shape::SouthEast => Rail::SouthEast,
            Shape::SouthWest => Rail::SouthWest,
            Shape::NorthWest => Rail::NorthWest,
            Shape::NorthEast => Rail::NorthEast,
        };
        return Some((shape, None));
    }
    if !matches!(
        kind,
        BlockKind::PoweredRail | BlockKind::DetectorRail | BlockKind::ActivatorRail
    ) {
        return None;
    }
    let shape = match state.property::<RailShape>()? {
        RailShape::NorthSouth => Rail::NorthSouth,
        RailShape::EastWest => Rail::EastWest,
        RailShape::AscendingEast => Rail::AscendingEast,
        RailShape::AscendingWest => Rail::AscendingWest,
        RailShape::AscendingNorth => Rail::AscendingNorth,
        RailShape::AscendingSouth => Rail::AscendingSouth,
    };
    let powered = (kind == BlockKind::PoweredRail)
        .then(|| state.property::<Powered>().unwrap_or_default());
    Some((shape, powered))
}

impl Blocks for World {
    fn rail(&self, pos: BlockPos) -> Option<(Rail, Option<bool>)> {
        rail_of(self.get_block_state(pos)?)
    }

    fn conductor(&self, pos: BlockPos) -> bool {
        self.get_block_state(pos)
            .is_some_and(azalea::physics::collision::legacy_calculate_solid)
    }

    fn boxes(&self, area: &Aabb) -> Vec<Aabb> {
        let mut out = Vec::new();
        for x in floor(area.min.x) - 1..=floor(area.max.x) {
            for y in floor(area.min.y) - 1..=floor(area.max.y) {
                for z in floor(area.min.z) - 1..=floor(area.max.z) {
                    let pos = BlockPos::new(x, y, z);
                    let Some(state) = self.get_block_state(pos) else {
                        continue;
                    };
                    if state.is_collision_shape_empty() {
                        continue;
                    }
                    let offset = Vec3::new(x as f64, y as f64, z as f64);
                    out.extend(
                        state
                            .collision_shape(pos)
                            .to_aabbs()
                            .iter()
                            .map(|b| b.move_relative(offset))
                            .filter(|b| b.intersects_aabb(area)),
                    );
                }
            }
        }
        out
    }
}

fn floor(v: f64) -> i32 {
    v.floor() as i32
}

const GRAVITY: f64 = 0.04;
const MAX_SPEED: f64 = 0.4;
/// `getSlowdownFactor` with a rider.
const SLOWDOWN: f64 = 0.997;
const HALF_WIDTH: f64 = 0.49;
const HEIGHT: f64 = 0.7;
/// Predictions kept to match server positions against.
const HISTORY: usize = 40;
/// Within this of a prediction, a server position needs no correction.
const TOLERANCE: f64 = 0.05;
/// Within this, drift is nudged toward the server; past it, a rebase.
const SOFT: f64 = 0.75;
/// How much of a drift each server position takes back.
const NUDGE: f64 = 0.5;
/// Per tick, how much of a correction is still shown (about half a second).
const EASE: f64 = 0.88;

#[derive(Clone, Copy, Debug)]
struct Cart {
    pos: Vec3,
    velocity: Vec3,
    yaw: f32,
    flipped: bool,
    on_ground: bool,
}

fn horizontal(v: Vec3) -> f64 {
    (v.x * v.x + v.z * v.z).sqrt()
}

/// `getPos`: the point on the rail under `(x, y, z)`.
fn track_pos(world: &impl Blocks, x: f64, y: f64, z: f64) -> Option<Vec3> {
    let (xt, mut yt, zt) = (floor(x), floor(y), floor(z));
    if world.rail(BlockPos::new(xt, yt - 1, zt)).is_some() {
        yt -= 1;
    }
    let (shape, _) = world.rail(BlockPos::new(xt, yt, zt))?;
    let (e0, e1) = shape.exits();
    let x0 = xt as f64 + 0.5 + e0.0 as f64 * 0.5;
    let y0 = yt as f64 + 0.0625 + e0.1 as f64 * 0.5;
    let z0 = zt as f64 + 0.5 + e0.2 as f64 * 0.5;
    let x1 = xt as f64 + 0.5 + e1.0 as f64 * 0.5;
    let y1 = yt as f64 + 0.0625 + e1.1 as f64 * 0.5;
    let z1 = zt as f64 + 0.5 + e1.2 as f64 * 0.5;
    let (xd, yd, zd) = (x1 - x0, (y1 - y0) * 2.0, z1 - z0);
    let progress = if xd == 0.0 {
        z - zt as f64
    } else if zd == 0.0 {
        x - xt as f64
    } else {
        ((x - x0) * xd + (z - z0) * zd) * 2.0
    };
    let mut out = Vec3::new(x0 + xd * progress, y0 + yd * progress, z0 + zd * progress);
    if yd < 0.0 {
        out.y += 1.0;
    } else if yd > 0.0 {
        out.y += 0.5;
    }
    Some(out)
}

/// `getPosOffs`: the point on the rail `offs` along it from `(x, y, z)`.
fn track_pos_offs(world: &impl Blocks, x: f64, y: f64, z: f64, offs: f64) -> Option<Vec3> {
    let (xt, mut yt, zt) = (floor(x), floor(y), floor(z));
    if world.rail(BlockPos::new(xt, yt - 1, zt)).is_some() {
        yt -= 1;
    }
    let (shape, _) = world.rail(BlockPos::new(xt, yt, zt))?;
    let mut y = yt as f64 + if shape.is_slope() { 1.0 } else { 0.0 };
    let (e0, e1) = shape.exits();
    let (xd, zd) = ((e1.0 - e0.0) as f64, (e1.2 - e0.2) as f64);
    let len = (xd * xd + zd * zd).sqrt();
    let (x, z) = (x + xd / len * offs, z + zd / len * offs);
    let (cx, cz) = (floor(x) - xt, floor(z) - zt);
    if e0.1 != 0 && cx == e0.0 && cz == e0.2 {
        y += e0.1 as f64;
    } else if e1.1 != 0 && cx == e1.0 && cz == e1.2 {
        y += e1.1 as f64;
    }
    track_pos(world, x, y, z)
}

/// `AbstractMinecartRenderer.oldRender`'s pose for a cart at `(x, y, z)`:
/// its offset onto the rail, yaw and pitch (degrees), or `None` off rails.
pub(crate) fn rail_render_pose(world: &World, x: f64, y: f64, z: f64) -> Option<(Vec3, f32, f32)> {
    const HALF_LENGTH: f64 = 0.3;
    let on_rail = track_pos(world, x, y, z)?;
    let front = track_pos_offs(world, x, y, z, HALF_LENGTH).unwrap_or(on_rail);
    let back = track_pos_offs(world, x, y, z, -HALF_LENGTH).unwrap_or(on_rail);
    let offset = Vec3::new(on_rail.x - x, (front.y + back.y) / 2.0 - y, on_rail.z - z);
    let dir = back - front;
    let len = dir.length();
    if len == 0.0 {
        return Some((offset, f32::NAN, f32::NAN));
    }
    let dir = dir / len;
    let yaw = dir.z.atan2(dir.x).to_degrees() as f32;
    let pitch = (dir.y.atan() * 73.0) as f32;
    Some((offset, yaw, pitch))
}

fn bounding_box(pos: Vec3) -> Aabb {
    Aabb {
        min: Vec3::new(pos.x - HALF_WIDTH, pos.y, pos.z - HALF_WIDTH),
        max: Vec3::new(pos.x + HALF_WIDTH, pos.y + HEIGHT, pos.z + HALF_WIDTH),
    }
}

/// How far `bb` may go `d` along `axis` (0 x, 1 y, 2 z) before `boxes` stop it.
fn clip(boxes: &[Aabb], bb: &Aabb, axis: usize, mut d: f64) -> f64 {
    let get = |v: Vec3, a: usize| [v.x, v.y, v.z][a];
    const EPS: f64 = 1.0e-7;
    for b in boxes {
        let overlaps = (0..3).filter(|&a| a != axis).all(|a| {
            get(b.max, a) - EPS > get(bb.min, a) && get(b.min, a) + EPS < get(bb.max, a)
        });
        if !overlaps {
            continue;
        }
        if d > 0.0 && get(b.min, axis) >= get(bb.max, axis) - EPS {
            d = d.min(get(b.min, axis) - get(bb.max, axis));
        } else if d < 0.0 && get(b.max, axis) <= get(bb.min, axis) + EPS {
            d = d.max(get(b.max, axis) - get(bb.min, axis));
        }
    }
    d
}

/// `Entity.collide` against blocks: y, then the larger of x and z first.
fn sweep(world: &impl Blocks, bb: Aabb, d: Vec3) -> Vec3 {
    let area = Aabb {
        min: Vec3::new(
            bb.min.x + d.x.min(0.0),
            bb.min.y + d.y.min(0.0),
            bb.min.z + d.z.min(0.0),
        ),
        max: Vec3::new(
            bb.max.x + d.x.max(0.0),
            bb.max.y + d.y.max(0.0),
            bb.max.z + d.z.max(0.0),
        ),
    };
    let boxes = world.boxes(&area);
    let mut bb = bb;
    let mut out = Vec3::ZERO;
    let order = if d.x.abs() < d.z.abs() { [1, 2, 0] } else { [1, 0, 2] };
    for axis in order {
        let want = [d.x, d.y, d.z][axis];
        if want == 0.0 {
            continue;
        }
        let got = clip(&boxes, &bb, axis, want);
        let shift = match axis {
            0 => Vec3::new(got, 0.0, 0.0),
            1 => Vec3::new(0.0, got, 0.0),
            _ => Vec3::new(0.0, 0.0, got),
        };
        bb = bb.move_relative(shift);
        out = out + shift;
    }
    out
}

/// `Entity.getInputVector` at speed 1.
fn input_vector(left: f64, forward: f64, yaw: f32) -> Vec3 {
    let len = left * left + forward * forward;
    if len < 1.0e-7 {
        return Vec3::ZERO;
    }
    let scale = if len > 1.0 { 1.0 / len.sqrt() } else { 1.0 };
    let (x, z) = (left * scale, forward * scale);
    let (sin, cos) = (yaw.to_radians().sin() as f64, yaw.to_radians().cos() as f64);
    Vec3::new(x * cos - z * sin, 0.0, z * cos + x * sin)
}

impl Cart {
    /// The block the cart rides in: `getCurrentBlockPosOrRailBelow`.
    fn rail_pos(&self, world: &impl Blocks) -> BlockPos {
        let (x, mut y, z) = (floor(self.pos.x), floor(self.pos.y), floor(self.pos.z));
        if world.rail(BlockPos::new(x, y - 1, z)).is_some() {
            y -= 1;
        }
        BlockPos::new(x, y, z)
    }

    /// `move`: the cart's box swept through the blocks; a stopped axis
    /// loses its speed.
    fn move_colliding(&mut self, world: &impl Blocks, d: Vec3) {
        let got = sweep(world, bounding_box(self.pos), d);
        self.pos = self.pos + got;
        if got.x != d.x {
            self.velocity.x = 0.0;
        }
        if got.z != d.z {
            self.velocity.z = 0.0;
        }
        if got.y != d.y {
            self.on_ground = d.y < 0.0;
            self.velocity.y = 0.0;
        } else {
            self.on_ground = false;
        }
    }

    /// `AbstractMinecart.comeOffTrack`.
    fn come_off_track(&mut self, world: &impl Blocks) {
        self.velocity.x = self.velocity.x.clamp(-MAX_SPEED, MAX_SPEED);
        self.velocity.z = self.velocity.z.clamp(-MAX_SPEED, MAX_SPEED);
        if self.on_ground {
            self.velocity = self.velocity * 0.5;
        }
        self.move_colliding(world, self.velocity);
        if !self.on_ground {
            self.velocity = self.velocity * 0.95;
        }
    }

    /// One server tick; false when the cart is off the rails.
    fn tick(&mut self, world: &impl Blocks, intent: Vec3) -> bool {
        let start = self.pos;
        self.velocity.y -= GRAVITY;
        let pos = self.rail_pos(world);
        let on_rails = match world.rail(pos) {
            Some((shape, powered)) => {
                self.move_along_track(world, pos, shape, powered, intent);
                true
            }
            None => {
                self.come_off_track(world);
                false
            }
        };
        // The yaw follows the motion, flipped to stay within 170 of the last.
        let (dx, dz) = (start.x - self.pos.x, start.z - self.pos.z);
        let old = self.yaw;
        if dx * dx + dz * dz > 0.001 {
            self.yaw = dz.atan2(dx).to_degrees() as f32;
            if self.flipped {
                self.yaw += 180.0;
            }
        }
        let diff = wrap_degrees(self.yaw - old);
        if !(-170.0..170.0).contains(&diff) {
            self.yaw += 180.0;
            self.flipped = !self.flipped;
        }
        self.yaw %= 360.0;
        on_rails
    }

    /// `OldMinecartBehavior.moveAlongTrack`.
    fn move_along_track(
        &mut self,
        world: &impl Blocks,
        pos: BlockPos,
        shape: Rail,
        powered: Option<bool>,
        intent: Vec3,
    ) {
        let (x, z) = (self.pos.x, self.pos.z);
        let old_pos = track_pos(world, x, self.pos.y, z);
        let mut y = pos.y as f64;
        let power_track = powered == Some(true);
        let mut halt_track = powered == Some(false);
        const SLIDE: f64 = 0.0078125;
        match shape {
            Rail::AscendingEast => self.velocity.x -= SLIDE,
            Rail::AscendingWest => self.velocity.x += SLIDE,
            Rail::AscendingNorth => self.velocity.z += SLIDE,
            Rail::AscendingSouth => self.velocity.z -= SLIDE,
            _ => {}
        }
        if shape.is_slope() {
            y += 1.0;
        }

        let (e0, e1) = shape.exits();
        let (mut xd, mut zd) = ((e1.0 - e0.0) as f64, (e1.2 - e0.2) as f64);
        let length = (xd * xd + zd * zd).sqrt();
        if self.velocity.x * xd + self.velocity.z * zd < 0.0 {
            xd = -xd;
            zd = -zd;
        }
        let pow = horizontal(self.velocity).min(2.0);
        self.velocity = Vec3::new(pow * xd / length, self.velocity.y, pow * zd / length);

        // The rider's keys nudge a cart that has all but stopped.
        if intent.length_squared() > 0.0 && horizontal(self.velocity).powi(2) < 0.01 {
            self.velocity.x += intent.x * 0.001;
            self.velocity.z += intent.z * 0.001;
            halt_track = false;
        }
        if halt_track {
            if horizontal(self.velocity) < 0.03 {
                self.velocity = Vec3::ZERO;
            } else {
                self.velocity.x *= 0.5;
                self.velocity.z *= 0.5;
            }
        }

        let x0 = pos.x as f64 + 0.5 + e0.0 as f64 * 0.5;
        let z0 = pos.z as f64 + 0.5 + e0.2 as f64 * 0.5;
        let x1 = pos.x as f64 + 0.5 + e1.0 as f64 * 0.5;
        let z1 = pos.z as f64 + 0.5 + e1.2 as f64 * 0.5;
        let (xd, zd) = (x1 - x0, z1 - z0);
        let progress = if xd == 0.0 {
            z - pos.z as f64
        } else if zd == 0.0 {
            x - pos.x as f64
        } else {
            ((x - x0) * xd + (z - z0) * zd) * 2.0
        };
        self.pos = Vec3::new(x0 + xd * progress, y, z0 + zd * progress);

        // `move`, with a rider's 0.75 and the speed cap.
        let step = Vec3::new(
            (0.75 * self.velocity.x).clamp(-MAX_SPEED, MAX_SPEED),
            0.0,
            (0.75 * self.velocity.z).clamp(-MAX_SPEED, MAX_SPEED),
        );
        self.move_colliding(world, step);
        let (cx, cz) = (floor(self.pos.x) - pos.x, floor(self.pos.z) - pos.z);
        if e0.1 != 0 && cx == e0.0 && cz == e0.2 {
            self.pos.y += e0.1 as f64;
        } else if e1.1 != 0 && cx == e1.0 && cz == e1.2 {
            self.pos.y += e1.1 as f64;
        }

        // `applyNaturalSlowdown`.
        self.velocity = Vec3::new(self.velocity.x * SLOWDOWN, 0.0, self.velocity.z * SLOWDOWN);
        if let (Some(new_pos), Some(old_pos)) = (
            track_pos(world, self.pos.x, self.pos.y, self.pos.z),
            old_pos,
        ) {
            let speed = (old_pos.y - new_pos.y) * 0.05;
            let pow = horizontal(self.velocity);
            if pow > 0.0 {
                let k = (pow + speed) / pow;
                self.velocity.x *= k;
                self.velocity.z *= k;
            }
            self.pos.y = new_pos.y;
        }

        let (xn, zn) = (floor(self.pos.x), floor(self.pos.z));
        if xn != pos.x || zn != pos.z {
            let pow = horizontal(self.velocity);
            self.velocity.x = pow * (xn - pos.x) as f64;
            self.velocity.z = pow * (zn - pos.z) as f64;
        }

        if power_track {
            let speed = horizontal(self.velocity);
            if speed > 0.01 {
                self.velocity.x += self.velocity.x / speed * 0.06;
                self.velocity.z += self.velocity.z / speed * 0.06;
            } else if shape == Rail::EastWest {
                if world.conductor(pos.west(1)) {
                    self.velocity.x = 0.02;
                } else if world.conductor(pos.east(1)) {
                    self.velocity.x = -0.02;
                }
            } else if shape == Rail::NorthSouth {
                if world.conductor(pos.north(1)) {
                    self.velocity.z = 0.02;
                } else if world.conductor(pos.south(1)) {
                    self.velocity.z = -0.02;
                }
            }
        }
    }
}

struct Ridden {
    id: i32,
    cart: Cart,
    /// The server's last position (`vec_delta_codec`'s base).
    server: Vec3,
    /// Newest first: each tick's predicted position and the rider's intent.
    history: VecDeque<(Vec3, Vec3)>,
    /// Ticks between a prediction and the server position matching it.
    lag: usize,
    /// What's left of a rebase's jump, added to the shown position.
    ease: Vec3,
}

static RIDDEN: Mutex<Option<Ridden>> = Mutex::new(None);

pub(crate) fn is_minecart(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Minecart
            | EntityKind::ChestMinecart
            | EntityKind::FurnaceMinecart
            | EntityKind::TntMinecart
            | EntityKind::HopperMinecart
            | EntityKind::SpawnerMinecart
            | EntityKind::CommandBlockMinecart
    )
}

/// Where the minecart this player rides is shown, if `id` is that cart.
pub(crate) fn predicted_pose(id: i32) -> Option<(Vec3, f32)> {
    RIDDEN
        .lock()
        .unwrap()
        .as_ref()
        .filter(|r| r.id == id)
        .map(|r| (r.cart.pos + r.ease, r.cart.yaw))
}

pub(crate) fn reset() {
    if let Ok(mut ridden) = RIDDEN.try_lock() {
        *ridden = None;
    }
}

impl Ridden {
    /// A server position arrived: keep predicting if it matches a recent
    /// prediction, else rebase on it and replay the ticks since.
    fn reconcile(&mut self, world: &impl Blocks, server: Vec3, velocity: Vec3, yaw: f32) {
        self.server = server;
        let distances: Vec<f64> = self
            .history
            .iter()
            .map(|(p, _)| p.distance_squared_to(server))
            .collect();
        let best = distances.iter().copied().fold(f64::MAX, f64::min);
        if best < TOLERANCE * TOLERANCE {
            // Only close matches set the lag; ties (a cart at rest) keep the
            // one nearest the last.
            let lag = self.lag;
            self.lag = (0..distances.len())
                .filter(|&i| distances[i] <= best + 1.0e-6)
                .min_by_key(|&i| i.abs_diff(lag))
                .unwrap_or(lag);
            return;
        }
        // Drift, measured at the known lag so a cart that runs ahead (slopes)
        // is pulled back rather than taken for a longer lag.
        let at = self.lag.min(self.history.len().saturating_sub(1));
        let drift = self
            .history
            .get(at)
            .map_or(f64::MAX, |(p, _)| p.distance_squared_to(server));
        if drift < SOFT * SOFT {
            // Pull part way toward the server, eased so nothing jumps.
            let shift = (server - self.history[at].0) * NUDGE;
            self.cart.pos = self.cart.pos + shift;
            for (p, _) in self.history.iter_mut() {
                *p = *p + shift;
            }
            self.ease = self.ease - shift;
            return;
        }
        // Rebase `lag` ticks back and replay since, rewriting the history so
        // the next server position still has predictions to match.
        let shown = self.cart.pos + self.ease;
        self.cart.pos = server;
        self.cart.velocity = velocity;
        let mut on_rails = true;
        for k in (0..self.lag.min(self.history.len())).rev() {
            on_rails &= self.cart.tick(world, self.history[k].1);
            self.history[k].0 = self.cart.pos;
        }
        if !on_rails {
            self.cart.yaw = yaw;
        }
        self.ease = shown - self.cart.pos;
        if self.ease.length_squared() > 4.0 {
            self.ease = Vec3::ZERO;
        }
    }

    fn step(&mut self, world: &impl Blocks, intent: Vec3) {
        self.ease = self.ease * EASE;
        if self.ease.length_squared() < 1.0e-8 {
            self.ease = Vec3::ZERO;
        }
        self.cart.tick(world, intent);
        self.history.push_front((self.cart.pos, intent));
        self.history.truncate(HISTORY);
    }
}

/// Runs the ridden cart one tick, before the rider is seated on it.
#[allow(clippy::type_complexity)]
pub(crate) fn predict(
    local: Query<(&EntityIdIndex, &LookDirection, &ClientMovementState), With<LocalEntity>>,
    carts: Query<
        (&EntityKindComponent, &Position, &Physics, &LookDirection, &WorldName),
        Without<LocalEntity>,
    >,
    worlds: Res<Worlds>,
) {
    let mut ridden = RIDDEN.lock().unwrap();
    let Some(vehicle_id) = crate::client::tracking::local_vehicle_id() else {
        *ridden = None;
        return;
    };
    let Ok((id_index, look, movement)) = local.single() else {
        return;
    };
    let Some(entity) = id_index.get_by_minecraft_entity(MinecraftEntityId(vehicle_id)) else {
        return;
    };
    let Ok((kind, position, physics, direction, world_name)) = carts.get(entity) else {
        return;
    };
    if !is_minecart(**kind) {
        *ridden = None;
        return;
    }
    let Some(world_lock) = worlds.get(world_name) else {
        return;
    };
    let world = world_lock.read();
    let server = physics.vec_delta_codec.base();
    let server = if server == Vec3::ZERO { **position } else { server };

    let r = match ridden.as_mut() {
        Some(r) if r.id == vehicle_id => r,
        _ => ridden.insert(Ridden {
            id: vehicle_id,
            cart: Cart {
                pos: server,
                velocity: physics.velocity,
                yaw: direction.y_rot(),
                flipped: false,
                on_ground: false,
            },
            server,
            history: VecDeque::new(),
            lag: 0,
            ease: Vec3::ZERO,
        }),
    };
    let fresh = server != r.server;
    if fresh {
        r.reconcile(&*world, server, physics.velocity, direction.y_rot());
    }
    // `getLastClientMoveIntent`: only the front seat steers.
    let intent = if crate::play::riding::drives(vehicle_id) {
        let sign = |v: f32| f64::from(v.total_cmp(&0.0) as i8) * f64::from(v != 0.0);
        let v = movement.move_vector;
        input_vector(sign(v.x), sign(v.y), look.y_rot())
    } else {
        Vec3::ZERO
    };
    r.step(&*world, intent);
    crate::log_debug!(
        "minecart",
        "pred={:.3?} ease={:.3?} vel={:.3?} server={:.3?}{} lag={}",
        r.cart.pos,
        r.ease,
        r.cart.velocity,
        server,
        if fresh { " (new)" } else { "" },
        r.lag
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Track {
        rails: HashMap<(i32, i32, i32), (Rail, Option<bool>)>,
        solid: Vec<(i32, i32, i32)>,
    }

    impl Blocks for Track {
        fn rail(&self, p: BlockPos) -> Option<(Rail, Option<bool>)> {
            self.rails.get(&(p.x, p.y, p.z)).copied()
        }
        fn conductor(&self, p: BlockPos) -> bool {
            self.solid.contains(&(p.x, p.y, p.z))
        }
        fn boxes(&self, area: &Aabb) -> Vec<Aabb> {
            self.solid
                .iter()
                .map(|&(x, y, z)| Aabb {
                    min: Vec3::new(x as f64, y as f64, z as f64),
                    max: Vec3::new(x as f64 + 1.0, y as f64 + 1.0, z as f64 + 1.0),
                })
                .filter(|b| b.intersects_aabb(area))
                .collect()
        }
    }

    fn straight(len: i32) -> Track {
        let mut t = Track::default();
        for x in 0..len {
            t.rails.insert((x, 0, 0), (Rail::EastWest, None));
        }
        t
    }

    fn cart(x: f64, vx: f64) -> Cart {
        Cart {
            pos: Vec3::new(x, 0.0625, 0.5),
            velocity: Vec3::new(vx, 0.0, 0.0),
            yaw: 0.0,
            flipped: false,
            on_ground: false,
        }
    }

    #[test]
    fn a_ridden_cart_coasts_along_a_straight_rail() {
        let track = straight(20);
        let mut c = cart(2.5, 0.4);
        assert!(c.tick(&track, Vec3::ZERO));
        // 0.75 of the speed, kept to the rail's line and height.
        assert!((c.pos.x - 2.8).abs() < 1e-9, "{:?}", c.pos);
        assert!((c.pos.z - 0.5).abs() < 1e-9 && (c.pos.y - 0.0625).abs() < 1e-9);
        assert!((c.velocity.x - 0.4 * SLOWDOWN).abs() < 1e-9);
        assert_eq!(c.velocity.y, 0.0);
    }

    #[test]
    fn the_rider_starts_a_still_cart() {
        let track = straight(20);
        let mut c = cart(2.5, 0.0);
        // Facing east (yaw -90) and holding forward.
        let intent = input_vector(0.0, 1.0, -90.0);
        assert!(intent.x > 0.99, "{intent:?}");
        c.tick(&track, intent);
        assert!(c.velocity.x > 0.0, "{:?}", c.velocity);
    }

    #[test]
    fn an_unpowered_rail_brakes_and_a_powered_one_boosts() {
        let mut track = straight(20);
        track.rails.insert((5, 0, 0), (Rail::EastWest, Some(false)));
        let mut c = cart(5.2, 0.02);
        c.tick(&track, Vec3::ZERO);
        assert_eq!(c.velocity.x, 0.0);
        track.rails.insert((5, 0, 0), (Rail::EastWest, Some(true)));
        let mut c = cart(5.2, 0.2);
        c.tick(&track, Vec3::ZERO);
        assert!(c.velocity.x > 0.2, "{:?}", c.velocity);
    }

    #[test]
    fn a_wall_stops_the_cart() {
        let mut track = straight(5);
        track.solid.push((5, 0, 0));
        let mut c = cart(4.4, 0.4);
        c.tick(&track, Vec3::ZERO);
        assert!(c.pos.x <= 4.51, "{:?}", c.pos);
        assert_eq!(c.velocity.x, 0.0);
    }

    #[test]
    fn a_matching_server_position_keeps_the_prediction() {
        let track = straight(40);
        let mut r = Ridden {
            id: 1,
            cart: cart(2.5, 0.3),
            server: Vec3::ZERO,
            history: VecDeque::new(),
            lag: 0,
            ease: Vec3::ZERO,
        };
        for _ in 0..6 {
            r.step(&track, Vec3::ZERO);
        }
        let ahead = r.cart.pos;
        // The server reports where the prediction was 3 ticks ago.
        let seen = r.history[3].0;
        r.reconcile(&track, seen, Vec3::ZERO, 0.0);
        assert_eq!(r.lag, 3);
        assert_eq!(r.cart.pos, ahead);
        assert_eq!(r.ease, Vec3::ZERO);
    }

    #[test]
    fn a_rebase_leaves_predictions_for_the_next_server_position() {
        let track = straight(80);
        let mut r = Ridden {
            id: 1,
            cart: cart(2.5, 0.3),
            server: Vec3::ZERO,
            history: VecDeque::new(),
            lag: 5,
            ease: Vec3::ZERO,
        };
        for _ in 0..8 {
            r.step(&track, Vec3::ZERO);
        }
        // The server is a little behind the prediction 5 ticks ago.
        let seen = r.history[5].0 - Vec3::new(0.4, 0.0, 0.0);
        r.reconcile(&track, seen, Vec3::new(0.3, 0.0, 0.0), 0.0);
        let after = r.cart.pos;
        for _ in 0..2 {
            r.step(&track, Vec3::ZERO);
        }
        // Two ticks on, the server reports the replayed path 5 ticks back:
        // a match, not another rebase.
        let next = r.history[5].0;
        let before = r.cart.pos;
        r.reconcile(&track, next, Vec3::new(0.3, 0.0, 0.0), 0.0);
        assert_eq!(r.lag, 5);
        assert_eq!(r.cart.pos, before);
        assert!(after.x < before.x);
    }

    #[test]
    fn off_the_rails_the_cart_lands_and_slows() {
        let mut track = Track::default();
        for x in 0..10 {
            track.solid.push((x, -1, 0));
        }
        let mut c = cart(2.5, 2.0);
        c.pos.y = 0.3;
        assert!(!c.tick(&track, Vec3::ZERO));
        // Capped to 0.4, and it fell onto the floor.
        assert!(c.pos.x - 2.5 <= 0.4 + 1e-9, "{:?}", c.pos);
        for _ in 0..10 {
            c.tick(&track, Vec3::ZERO);
        }
        assert!(c.on_ground && c.pos.y.abs() < 1e-9, "{:?}", c.pos);
        // Halved each grounded tick: all but stopped.
        assert!(c.velocity.x < 0.01, "{:?}", c.velocity);
    }

    #[test]
    fn drift_is_nudged_not_rebased() {
        let track = straight(40);
        let mut r = Ridden {
            id: 1,
            cart: cart(2.5, 0.3),
            server: Vec3::ZERO,
            history: VecDeque::new(),
            lag: 2,
            ease: Vec3::ZERO,
        };
        for _ in 0..4 {
            r.step(&track, Vec3::ZERO);
        }
        let (shown, ahead) = (r.cart.pos + r.ease, r.cart.pos);
        // 0.1 behind the prediction 2 ticks ago.
        let seen = r.history[2].0 - Vec3::new(0.1, 0.0, 0.0);
        r.reconcile(&track, seen, Vec3::ZERO, 0.0);
        assert_eq!(r.lag, 2);
        assert!((r.cart.pos.x - (ahead.x - 0.1 * NUDGE)).abs() < 1e-9);
        // Shown where it was; the pull eases in.
        assert!((r.cart.pos + r.ease).distance_squared_to(shown) < 1e-12);
    }

    #[test]
    fn a_missed_prediction_rebases_and_eases_the_jump() {
        let track = straight(40);
        let mut r = Ridden {
            id: 1,
            cart: cart(2.5, 0.3),
            server: Vec3::ZERO,
            history: VecDeque::new(),
            lag: 2,
            ease: Vec3::ZERO,
        };
        for _ in 0..4 {
            r.step(&track, Vec3::ZERO);
        }
        let shown = r.cart.pos;
        // The server says the cart stopped well back, at x = 1.5.
        r.reconcile(&track, Vec3::new(1.5, 0.0625, 0.5), Vec3::ZERO, 0.0);
        assert!((r.cart.pos.x - 1.5).abs() < 1e-9, "{:?}", r.cart.pos);
        assert!((r.cart.pos + r.ease).distance_squared_to(shown) < 1e-12);
    }
}
