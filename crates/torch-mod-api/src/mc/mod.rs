//! Vanilla Minecraft's client-side helpers as Rust, with Java's behaviour
//! (its sine table, float rounding, `Direction` tie-breaks): port a call
//! by renaming it, not by rewriting it. `use torch_mod_api::prelude::*`
//! brings them all in.
//!
//! Naming: Java `Class.methodName(..)` is `Class::method_name(..)`; an
//! instance call `v.methodName(..)` is `v.method_name(..)`. Where Java
//! overloads by argument type, Rust has one name per form, listed in the
//! doc of each type (`translate`/`translate_vec`, `add`/`add_xyz`).
//! `docs/MOD_API.md` has the table.

pub mod color;
pub mod direction;
pub mod java;
pub mod mth;
pub mod nbt;
pub mod pose;
pub mod vec;

use crate::{BlockAt, BlockEntityView, BlockStateView, Motion, MovingEntity, Step};
use direction::{Axis, Direction};
use vec::{AABB, BlockPos, Vec3};

/// Reading a block state's properties as Java's `state.getValue(PROP)`.
pub trait StateProps {
    fn props_map(&self) -> &std::collections::BTreeMap<String, String>;

    /// `getValue(FACING)` and the like: a `Direction` property.
    fn direction(&self, prop: &str) -> Option<Direction> {
        Direction::by_name(self.props_map().get(prop)?)
    }

    /// `getValue(AXIS)`: an `Axis` property.
    fn axis(&self, prop: &str) -> Option<Axis> {
        Axis::by_name(self.props_map().get(prop)?)
    }

    /// A boolean property (`POWERED`, `WATERLOGGED`); false when missing.
    fn flag(&self, prop: &str) -> bool {
        self.props_map().get(prop).is_some_and(|v| v == "true")
    }

    /// An integer property (`AGE`, `POWER`).
    fn int(&self, prop: &str) -> Option<i32> {
        self.props_map().get(prop)?.parse().ok()
    }

    /// `hasProperty(PROP)`.
    fn has_property(&self, prop: &str) -> bool {
        self.props_map().contains_key(prop)
    }
}

impl StateProps for BlockEntityView {
    fn props_map(&self) -> &std::collections::BTreeMap<String, String> {
        &self.props
    }
}

impl StateProps for BlockStateView {
    fn props_map(&self) -> &std::collections::BTreeMap<String, String> {
        &self.props
    }
}

impl StateProps for BlockAt {
    fn props_map(&self) -> &std::collections::BTreeMap<String, String> {
        &self.props
    }
}

impl BlockEntityView {
    /// `getBlockPos()`.
    pub fn block_pos(&self) -> BlockPos {
        BlockPos::from_array(self.pos)
    }

    /// `level.getBlockState(pos)`, through the host (connection logic).
    /// `None` off the wasm host or where the world isn't loaded.
    pub fn block_at(&self, p: BlockPos) -> Option<BlockAt> {
        crate::host::block(p.x, p.y, p.z)
    }

    /// The block next to it: `level.getBlockState(pos.relative(d))`.
    pub fn neighbour(&self, d: Direction) -> Option<BlockAt> {
        self.block_at(self.block_pos().relative(d))
    }
}

impl MovingEntity {
    /// `position()`: its feet.
    pub fn position(&self) -> Vec3 {
        Vec3::from_array(self.pos)
    }

    pub fn get_x(&self) -> f64 {
        self.pos[0]
    }

    pub fn get_y(&self) -> f64 {
        self.pos[1]
    }

    pub fn get_z(&self) -> f64 {
        self.pos[2]
    }

    /// `getDeltaMovement()`.
    pub fn get_delta_movement(&self) -> Vec3 {
        Vec3::from_array(self.velocity)
    }

    /// `getBoundingBox()`.
    pub fn get_bounding_box(&self) -> AABB {
        AABB::entity(self.position(), self.width, self.height)
    }

    /// `blockPosition()`: the block its feet are in.
    pub fn block_position(&self) -> BlockPos {
        self.position().block_pos()
    }

    /// `isShiftKeyDown()`.
    pub fn is_shift_key_down(&self) -> bool {
        self.sneaking
    }

    /// `onGround()`.
    pub fn on_ground(&self) -> bool {
        self.on_ground
    }
}

/// Builds a [`Motion`] from Java's entity calls, in the order Java makes
/// them: `entity.move(SELF, v)` is `m.move_entity(v)`,
/// `entity.setDeltaMovement(v)` is `m.set_delta_movement(v)`, and so on.
impl Motion {
    /// `entity.move(MoverType.SELF, v)`: with collision.
    pub fn move_entity(&mut self, v: Vec3) -> &mut Motion {
        self.steps.push(Step::Move(v.to_array()));
        self
    }

    /// `entity.setDeltaMovement(v)`.
    pub fn set_delta_movement(&mut self, v: Vec3) -> &mut Motion {
        self.steps.push(Step::Velocity(v.to_array()));
        self
    }

    /// `entity.setPos(getX() + x, getY() + y, getZ() + z)`: no collision.
    pub fn offset_pos(&mut self, v: Vec3) -> &mut Motion {
        self.steps.push(Step::Offset(v.to_array()));
        self
    }

    /// `entity.setOnGround(b)`.
    pub fn set_on_ground(&mut self, b: bool) -> &mut Motion {
        self.steps.push(Step::OnGround(b));
        self
    }

    /// `entity.fallDistance = 0`.
    pub fn reset_fall_distance(&mut self) -> &mut Motion {
        self.steps.push(Step::ResetFall);
        self
    }

    /// Whether it does anything.
    pub fn is_empty(&self) -> bool {
        self.move_by.is_none() && self.steps.is_empty() && self.velocity.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_read_like_java() {
        let v = BlockEntityView {
            pos: [1, 2, 3],
            props: [("facing", "east"), ("powered", "true")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .into(),
            ..BlockEntityView::default()
        };
        assert_eq!(v.block_pos().above(), BlockPos::new(1, 3, 3));
        assert_eq!(v.direction("facing"), Some(Direction::East));
        assert!(v.flag("powered") && !v.flag("waterlogged"));
        let e = MovingEntity {
            pos: [0.5, 1.0, 0.5],
            width: 0.6,
            height: 1.8,
            ..MovingEntity::default()
        };
        assert_eq!(e.get_bounding_box().max_y, 2.8);
        let mut m = Motion::default();
        m.move_entity(Vec3::new(0.1, 0.0, 0.0)).set_on_ground(true);
        assert_eq!(
            m.steps,
            vec![Step::Move([0.1, 0.0, 0.0]), Step::OnGround(true)]
        );
    }
}
