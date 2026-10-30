//! Driving a boat: `AbstractBoat.tick` for the boat this player steers.
//!
//! Vanilla moves a boat on the client of the player in its front seat and
//! tells the server where it went (`ServerboundMoveVehiclePacket`), with the
//! paddles (`ServerboundPaddleBoatPacket`). The boat floats, slides and turns
//! here; the server only corrects it.

use std::sync::Mutex;

use azalea::app::{App, Plugin};
use azalea::block::BlockState;
use azalea::ecs::prelude::*;
use azalea::entity::dimensions::EntityDimensions;
use azalea::entity::indexing::EntityIdIndex;
use azalea::entity::metadata::Sprinting;
use azalea::entity::{
    Attributes, EntityKindComponent, Jumping, LocalEntity, LookDirection, OnClimbable, Physics,
    Position,
};
use azalea::packet::game::SendGamePacketEvent;
use azalea::physics::PhysicsSystems;
use azalea::physics::client_movement::ClientMovementState;
use azalea::physics::collision::entity_collisions::{AabbQuery, CollidableEntityQuery};
use azalea::physics::collision::{BlockWithShape, MoveCtx, MoverType, move_colliding};
use azalea::physics::travel::travel;
use azalea::world::{World, WorldName, Worlds};
use azalea_core::aabb::Aabb;
use azalea_core::entity_id::MinecraftEntityId;
use azalea_core::position::{BlockPos, Vec3};
use azalea_core::tick::GameTick;
use azalea_protocol::packets::game::s_move_vehicle::ServerboundMoveVehicle;
use azalea_protocol::packets::game::s_paddle_boat::ServerboundPaddleBoat;
use azalea_registry::builtin::{BlockKind, EntityKind};

use crate::util::mth::wrap_degrees;

/// `AbstractBoat.Status`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    InWater,
    UnderWater,
    UnderFlowingWater,
    OnLand,
    InAir,
}

/// The driven boat's own state, kept here so the server's position updates
/// for it (which describe where it was, not where it is going) do not pull it
/// back between corrections.
struct Boat {
    id: i32,
    pos: Vec3,
    velocity: Vec3,
    yaw: f32,
    delta_rotation: f32,
    status: Status,
    old_status: Status,
    water_level: f64,
    land_friction: f32,
    last_yd: f64,
}

static DRIVEN: Mutex<Option<Boat>> = Mutex::new(None);

const GRAVITY: f64 = 0.04;
/// `clampRotation`: how far the driver may look away from the bow.
const MAX_HEAD_ROTATION: f32 = 105.0;

pub(crate) fn is_boat(kind: EntityKind) -> bool {
    use crate::entities::render::objects::boat;
    boat::BOATS.contains(&kind)
        || boat::CHEST_BOATS.contains(&kind)
        || matches!(kind, EntityKind::BambooRaft | EntityKind::BambooChestRaft)
}

/// The server moved the boat this player drives (`ClientboundMoveVehicle`).
pub(crate) fn correct(pos: Vec3, yaw: f32) {
    if let Some(boat) = DRIVEN.lock().unwrap().as_mut() {
        boat.pos = pos;
        boat.yaw = yaw;
        boat.velocity = Vec3::ZERO;
    }
}

pub(crate) fn reset() {
    if let Ok(mut driven) = DRIVEN.try_lock() {
        *driven = None;
    }
    set_steered(None);
}

/// Where the boat this player steers is, if `id` is that boat.
pub(crate) fn driven_pose(id: i32) -> Option<(Vec3, f32)> {
    DRIVEN
        .lock()
        .unwrap()
        .as_ref()
        .filter(|boat| boat.id == id)
        .map(|boat| (boat.pos, boat.yaw))
}

/// Tells the camera the steered boat's yaw, which bounds the driver's look.
fn set_steered(yaw: Option<f32>) {
    if let Some(shared) = crate::SHARED.get()
        && let Ok(mut s) = shared.try_lock()
    {
        s.session.steered_boat_yaw = yaw;
    }
}

/// `clampRotation` on a look `yaw`, both in Minecraft degrees: within 105°
/// of the bow.
pub(crate) fn clamp_driver_yaw(yaw: f32, boat_yaw: f32) -> f32 {
    let delta = wrap_degrees(yaw - boat_yaw);
    yaw + delta.clamp(-MAX_HEAD_ROTATION, MAX_HEAD_ROTATION) - delta
}

pub struct BoatPlugin;

impl Plugin for BoatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            GameTick,
            drive_boat
                .in_set(PhysicsSystems)
                .after(azalea::physics::ai_step)
                .before(travel),
        );
    }
}

/// Water at `pos`, as `FluidState.getHeight` and `isSource` see it: the
/// surface height within the block (full when water continues above), and
/// whether it is a source.
fn water(world: &World, pos: BlockPos) -> Option<(f32, bool)> {
    let (own, source) = water_level(world.get_block_state(pos)?)?;
    let above = world
        .get_block_state(pos.up(1))
        .and_then(water_level)
        .is_some();
    Some((if above { 1.0 } else { own }, source))
}

pub(crate) fn water_level(state: BlockState) -> Option<(f32, bool)> {
    const SOURCE: f32 = 8.0 / 9.0;
    if state
        .property::<azalea::block::properties::Waterlogged>()
        .unwrap_or_default()
    {
        return Some((SOURCE, true));
    }
    match BlockKind::from(state) {
        BlockKind::Water => {
            let level = state
                .property::<azalea::block::properties::WaterLevel>()
                .map_or(0, |l| l as u8);
            Some(match level {
                0 => (SOURCE, true),
                1..=7 => ((8 - level) as f32 / 9.0, false),
                // Falling water is a full flowing column.
                _ => (SOURCE, false),
            })
        }
        BlockKind::BubbleColumn
        | BlockKind::Seagrass
        | BlockKind::TallSeagrass
        | BlockKind::Kelp
        | BlockKind::KelpPlant => Some((SOURCE, true)),
        _ => None,
    }
}

fn floor(v: f64) -> i32 {
    v.floor() as i32
}

fn ceil(v: f64) -> i32 {
    v.ceil() as i32
}

/// `isUnderwater`: water over the boat's top.
fn under_water(world: &World, bb: &Aabb) -> Option<Status> {
    let max_y = bb.max.y + 0.001;
    let mut under = false;
    for x in floor(bb.min.x)..ceil(bb.max.x) {
        for y in floor(bb.max.y)..ceil(max_y) {
            for z in floor(bb.min.z)..ceil(bb.max.z) {
                let Some((height, source)) = water(world, BlockPos::new(x, y, z)) else {
                    continue;
                };
                if max_y < y as f64 + height as f64 {
                    if !source {
                        return Some(Status::UnderFlowingWater);
                    }
                    under = true;
                }
            }
        }
    }
    under.then_some(Status::UnderWater)
}

/// `checkInWater`: whether the hull sits in water, and the highest surface
/// under it.
fn in_water(world: &World, bb: &Aabb) -> (bool, f64) {
    let mut inside = false;
    let mut level = -f64::MAX;
    for x in floor(bb.min.x)..ceil(bb.max.x) {
        for y in floor(bb.min.y)..ceil(bb.min.y + 0.001) {
            for z in floor(bb.min.z)..ceil(bb.max.z) {
                let Some((height, _)) = water(world, BlockPos::new(x, y, z)) else {
                    continue;
                };
                let surface = y as f64 + height as f64;
                level = level.max(surface);
                inside |= bb.min.y < surface;
            }
        }
    }
    (inside, level)
}

/// `getGroundFriction`: the mean friction of the blocks touching the hull's
/// underside, lily pads aside; 0 with nothing under it.
fn ground_friction(world: &World, bb: &Aabb) -> f32 {
    let probe = Aabb {
        min: Vec3::new(bb.min.x, bb.min.y - 0.001, bb.min.z),
        max: Vec3::new(bb.max.x, bb.min.y, bb.max.z),
    };
    let (x0, x1) = (floor(probe.min.x) - 1, ceil(probe.max.x) + 1);
    let (y0, y1) = (floor(probe.min.y) - 1, ceil(probe.max.y) + 1);
    let (z0, z1) = (floor(probe.min.z) - 1, ceil(probe.max.z) + 1);
    let (mut friction, mut count) = (0.0f32, 0u32);
    for x in x0..x1 {
        for z in z0..z1 {
            let edges = (x == x0 || x == x1 - 1) as u8 + (z == z0 || z == z1 - 1) as u8;
            if edges == 2 {
                continue;
            }
            for y in y0..y1 {
                if edges > 0 && (y == y0 || y == y1 - 1) {
                    continue;
                }
                let pos = BlockPos::new(x, y, z);
                let Some(state) = world.get_block_state(pos) else {
                    continue;
                };
                if BlockKind::from(state) == BlockKind::LilyPad || state.is_collision_shape_empty()
                {
                    continue;
                }
                let offset = Vec3::new(x as f64, y as f64, z as f64);
                let touches = state
                    .collision_shape(pos)
                    .to_aabbs()
                    .iter()
                    .any(|b| b.move_relative(offset).intersects_aabb(&probe));
                if touches {
                    friction += crate::blocks::info::info(state).friction;
                    count += 1;
                }
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        friction / count as f32
    }
}

/// `getWaterLevelAbove`: the surface above a boat that fell into water.
fn water_level_above(world: &World, bb: &Aabb, last_yd: f64) -> f64 {
    let max_y = ceil(bb.max.y - last_yd);
    'rows: for y in floor(bb.max.y)..max_y {
        let mut height = 0.0f32;
        for x in floor(bb.min.x)..ceil(bb.max.x) {
            for z in floor(bb.min.z)..ceil(bb.max.z) {
                if let Some((h, _)) = water(world, BlockPos::new(x, y, z)) {
                    height = height.max(h);
                }
                if height >= 1.0 {
                    continue 'rows;
                }
            }
        }
        return y as f64 + height as f64;
    }
    (max_y + 1) as f64
}

impl Boat {
    /// `getStatus`.
    fn status(&mut self, world: &World, bb: &Aabb) -> Status {
        if let Some(status) = under_water(world, bb) {
            self.water_level = bb.max.y;
            return status;
        }
        let (inside, level) = in_water(world, bb);
        self.water_level = level;
        if inside {
            return Status::InWater;
        }
        let friction = ground_friction(world, bb);
        if friction > 0.0 {
            self.land_friction = friction;
            Status::OnLand
        } else {
            Status::InAir
        }
    }

    /// `floatBoat`, but for the drop onto water, which needs the world and
    /// is handled by the caller (`surface`).
    fn float(&mut self, height: f64, surface: Option<f64>) {
        if self.old_status == Status::InAir
            && self.status != Status::InAir
            && self.status != Status::OnLand
        {
            if let Some(target_y) = surface {
                self.pos.y = target_y;
                self.velocity.y = 0.0;
                self.last_yd = 0.0;
            }
            self.status = Status::InWater;
            return;
        }
        let (vspeed, buoyancy, inv_friction) = match self.status {
            Status::InWater => (-GRAVITY, (self.water_level - self.pos.y) / height, 0.9f32),
            Status::UnderFlowingWater => (-7.0e-4, 0.0, 0.9),
            Status::UnderWater => (-GRAVITY, 0.01, 0.45),
            Status::InAir => (-GRAVITY, 0.0, 0.9),
            Status::OnLand => {
                let friction = self.land_friction;
                // The driver is a player, so the land friction halves.
                self.land_friction /= 2.0;
                (-GRAVITY, 0.0, friction)
            }
        };
        let f = inv_friction as f64;
        self.velocity = Vec3::new(
            self.velocity.x * f,
            self.velocity.y + vspeed,
            self.velocity.z * f,
        );
        self.delta_rotation *= inv_friction;
        if buoyancy > 0.0 {
            self.velocity.y = (self.velocity.y + buoyancy * (GRAVITY / 0.65)) * 0.75;
        }
    }

    /// `controlBoat`: turns and pushes the boat; the paddles that move.
    fn control(&mut self, left: bool, right: bool, up: bool, down: bool) -> (bool, bool) {
        let mut acceleration = 0.0f32;
        if left {
            self.delta_rotation -= 1.0;
        }
        if right {
            self.delta_rotation += 1.0;
        }
        if right != left && !up && !down {
            acceleration += 0.005;
        }
        self.yaw += self.delta_rotation;
        if up {
            acceleration += 0.04;
        }
        if down {
            acceleration -= 0.005;
        }
        let rad = self.yaw.to_radians();
        self.velocity.x += ((-rad).sin() * acceleration) as f64;
        self.velocity.z += (rad.cos() * acceleration) as f64;
        (right && !left || up, left && !right || up)
    }
}

#[allow(clippy::type_complexity)]
fn drive_boat(
    local: Query<(Entity, &EntityIdIndex, &ClientMovementState), With<LocalEntity>>,
    mut boats: Query<
        (
            Entity,
            &EntityKindComponent,
            &mut Position,
            &mut Physics,
            &mut LookDirection,
            &EntityDimensions,
            &WorldName,
            &OnClimbable,
        ),
        Without<LocalEntity>,
    >,
    worlds: Res<Worlds>,
    aabb_query: AabbQuery,
    collidable_entity_query: CollidableEntityQuery,
    mut commands: Commands,
) {
    let mut driven = DRIVEN.lock().unwrap();
    let Some(vehicle_id) = crate::client::tracking::local_vehicle_id() else {
        if driven.take().is_some() {
            set_steered(None);
        }
        return;
    };
    let Ok((rider, id_index, movement)) = local.single() else {
        return;
    };
    let Some(entity) = id_index.get_by_minecraft_entity(MinecraftEntityId(vehicle_id)) else {
        return;
    };
    let Ok((
        entity,
        kind,
        mut position,
        mut physics,
        mut direction,
        dimensions,
        world_name,
        on_climbable,
    )) = boats.get_mut(entity)
    else {
        return;
    };
    if !crate::entities::facts::facts(**kind).boat || !crate::play::riding::drives(vehicle_id) {
        if driven.take().is_some() {
            set_steered(None);
        }
        return;
    }
    let Some(world_lock) = worlds.get(world_name) else {
        return;
    };
    let world = world_lock.read();

    let boat = match driven.as_mut() {
        Some(boat) if boat.id == vehicle_id => boat,
        _ => driven.insert(Boat {
            id: vehicle_id,
            pos: **position,
            velocity: Vec3::ZERO,
            yaw: direction.y_rot(),
            delta_rotation: 0.0,
            status: Status::InAir,
            old_status: Status::InAir,
            water_level: 0.0,
            land_friction: 0.0,
            last_yd: 0.0,
        }),
    };

    let bb = dimensions.make_bounding_box(boat.pos);
    boat.old_status = boat.status;
    boat.status = boat.status(&world, &bb);

    // `floatBoat`'s drop onto water: settle on the surface when it is free.
    let surface = if boat.old_status == Status::InAir
        && boat.status != Status::InAir
        && boat.status != Status::OnLand
    {
        boat.water_level = boat.pos.y + dimensions.height as f64;
        let target_y =
            water_level_above(&world, &bb, boat.last_yd) - dimensions.height as f64 + 0.101;
        let moved = bb.move_relative(Vec3::new(0.0, target_y - boat.pos.y, 0.0));
        azalea::physics::travel::no_collision(
            &world,
            Some(entity),
            &aabb_query,
            &collidable_entity_query,
            &physics,
            &moved,
            false,
            Default::default(),
        )
        .then_some(target_y)
    } else {
        None
    };
    boat.float(dimensions.height as f64, surface);

    let x = movement.move_vector.x;
    let z = movement.move_vector.y;
    let (paddle_left, paddle_right) = boat.control(x > 0.0, x < 0.0, z > 0.0, z < 0.0);
    commands.trigger(SendGamePacketEvent::new(
        rider,
        ServerboundPaddleBoat {
            left: paddle_left,
            right: paddle_right,
        },
    ));

    **position = boat.pos;
    physics.bounding_box = dimensions.make_bounding_box(boat.pos);
    physics.velocity = boat.velocity;
    *direction = LookDirection::new(boat.yaw, 0.0);
    // `checkFallDamage` keeps the vertical speed before collisions settle it.
    boat.last_yd = boat.velocity.y;

    let mut attributes = Attributes::new(**kind);
    attributes.step_height.base = 0.0;
    let movement_now = physics.velocity;
    let mut ctx = MoveCtx {
        mover_type: MoverType::Own,
        world: &world,
        position: position.reborrow(),
        physics: &mut physics,
        source_entity: entity,
        aabb_query: &aabb_query,
        collidable_entity_query: &collidable_entity_query,
        physics_state: None,
        attributes: &attributes,
        abilities: None,
        active_effects: None,
        powder_snow_walkable: false,
        fall_flying: false,
        direction: *direction,
        sprinting: Sprinting(false),
        on_climbable: *on_climbable,
        pose: None,
        jumping: Jumping(false),
        no_physics: false,
    };
    move_colliding(&mut ctx, movement_now);
    drop(ctx);
    physics.bounding_box = dimensions.make_bounding_box(**position);

    boat.pos = **position;
    boat.velocity = physics.velocity;

    // `positionRider`: the driver's view turns with the boat. The 105° clamp
    // happens on every mouse movement (`onPassengerTurned`), in the camera.
    if let Some(shared) = crate::SHARED.get() {
        let mut s = shared.lock().unwrap();
        s.session.vehicle_yaw_nudge += boat.delta_rotation;
        s.session.steered_boat_yaw = Some(boat.yaw);
    }

    commands.trigger(SendGamePacketEvent::new(
        rider,
        ServerboundMoveVehicle {
            pos: boat.pos,
            look_direction: *direction,
            on_ground: physics.on_ground(),
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boat() -> Boat {
        Boat {
            id: 1,
            pos: Vec3::ZERO,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            delta_rotation: 0.0,
            status: Status::InWater,
            old_status: Status::InWater,
            water_level: 0.0,
            land_friction: 0.0,
            last_yd: 0.0,
        }
    }

    #[test]
    fn rowing_forward_heads_where_the_boat_faces() {
        let mut b = boat();
        let paddles = b.control(false, false, true, false);
        assert_eq!(paddles, (true, true));
        // Yaw 0 faces south, +z.
        assert!((b.velocity.z - 0.04).abs() < 1e-6 && b.velocity.x.abs() < 1e-9);
        b.yaw = 90.0;
        b.velocity = Vec3::ZERO;
        b.control(false, false, true, false);
        assert!((b.velocity.x + 0.04).abs() < 1e-6, "{:?}", b.velocity);
    }

    #[test]
    fn steering_alone_turns_and_creeps() {
        let mut b = boat();
        assert_eq!(b.control(true, false, false, false), (false, true));
        assert_eq!(b.delta_rotation, -1.0);
        assert_eq!(b.yaw, -1.0);
        assert!((b.velocity.z - 0.005).abs() < 1e-6);
    }

    #[test]
    fn water_holds_the_boat_up_and_slows_it() {
        let mut b = boat();
        b.velocity = Vec3::new(1.0, 0.0, 0.0);
        // Sunk most of the way: buoyancy beats gravity.
        b.water_level = 0.5;
        b.float(0.5625, None);
        assert!((b.velocity.x - 0.9).abs() < 1e-6);
        assert!(b.velocity.y > 0.0, "{:?}", b.velocity);
    }

    #[test]
    fn the_driver_looks_no_more_than_105_degrees_off_the_bow() {
        assert_eq!(clamp_driver_yaw(30.0, 0.0), 30.0);
        assert!((clamp_driver_yaw(120.0, 0.0) - 105.0).abs() < 1e-4);
        assert!((clamp_driver_yaw(-170.0, 0.0) + 105.0).abs() < 1e-4);
        // Across the wrap: the bow at 170, a look at -100 is 90 to the right.
        assert_eq!(clamp_driver_yaw(-100.0, 170.0), -100.0);
    }
}
