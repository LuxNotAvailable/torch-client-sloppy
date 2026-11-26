//! Vanilla's `Direction`, `Direction.Axis`, `Direction.AxisDirection`,
//! `Direction.Plane`, and the block-state `Rotation` and `Mirror`.

use super::mth::Mth;
use super::vec::{BlockPos, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Axis {
    X,
    #[default]
    Y,
    Z,
}

impl Axis {
    /// `Axis.values()`.
    pub const VALUES: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// `Axis.byName`: `"x"`, `"y"`, `"z"` (a block state's `axis`).
    pub fn by_name(s: &str) -> Option<Axis> {
        match s {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "z" => Some(Axis::Z),
            _ => None,
        }
    }

    pub fn get_name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }

    pub fn is_vertical(self) -> bool {
        self == Axis::Y
    }

    pub fn is_horizontal(self) -> bool {
        self != Axis::Y
    }

    /// `axis.choose(x, y, z)` (the int and double overloads).
    pub fn choose<T>(self, x: T, y: T, z: T) -> T {
        match self {
            Axis::X => x,
            Axis::Y => y,
            Axis::Z => z,
        }
    }

    /// `axis.test(direction)`: whether it lies along this axis.
    pub fn test(self, d: Direction) -> bool {
        d.get_axis() == self
    }

    pub fn get_plane(self) -> Plane {
        if self == Axis::Y {
            Plane::Vertical
        } else {
            Plane::Horizontal
        }
    }

    /// Its positive unit vector.
    pub fn unit(self) -> [f32; 3] {
        Direction::from_axis_and_direction(self, AxisDirection::Positive).step()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AxisDirection {
    #[default]
    Positive,
    Negative,
}

impl AxisDirection {
    pub fn get_step(self) -> i32 {
        match self {
            AxisDirection::Positive => 1,
            AxisDirection::Negative => -1,
        }
    }

    pub fn opposite(self) -> AxisDirection {
        match self {
            AxisDirection::Positive => AxisDirection::Negative,
            AxisDirection::Negative => AxisDirection::Positive,
        }
    }
}

/// `Direction.Plane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Plane {
    #[default]
    Horizontal,
    Vertical,
}

impl Plane {
    /// Its directions in Java's order: north, east, south, west; up, down.
    pub fn directions(self) -> &'static [Direction] {
        match self {
            Plane::Horizontal => &[
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ],
            Plane::Vertical => &[Direction::Up, Direction::Down],
        }
    }

    pub fn test(self, d: Direction) -> bool {
        d.get_axis().get_plane() == self
    }
}

/// `Direction`, in declaration order (`values()`, ties in `getNearest`).
/// Serialized by name (`"north"`).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Down,
    Up,
    #[default]
    North,
    South,
    West,
    East,
}

impl Direction {
    /// `Direction.values()`.
    pub const VALUES: [Direction; 6] = [
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ];

    /// `Direction.byName` (a block state's `facing`).
    pub fn by_name(s: &str) -> Option<Direction> {
        Direction::VALUES.into_iter().find(|d| d.get_name() == s)
    }

    pub fn get_name(self) -> &'static str {
        match self {
            Direction::Down => "down",
            Direction::Up => "up",
            Direction::North => "north",
            Direction::South => "south",
            Direction::West => "west",
            Direction::East => "east",
        }
    }

    pub fn get_axis(self) -> Axis {
        match self {
            Direction::Down | Direction::Up => Axis::Y,
            Direction::North | Direction::South => Axis::Z,
            Direction::West | Direction::East => Axis::X,
        }
    }

    pub fn get_axis_direction(self) -> AxisDirection {
        match self {
            Direction::Up | Direction::South | Direction::East => AxisDirection::Positive,
            _ => AxisDirection::Negative,
        }
    }

    pub fn get_3d_data_value(self) -> i32 {
        self as i32
    }

    /// South 0, west 1, north 2, east 3; up and down -1.
    pub fn get_2d_data_value(self) -> i32 {
        match self {
            Direction::South => 0,
            Direction::West => 1,
            Direction::North => 2,
            Direction::East => 3,
            Direction::Down | Direction::Up => -1,
        }
    }

    pub fn from_3d_data_value(i: i32) -> Direction {
        Direction::VALUES[(i % 6).unsigned_abs() as usize]
    }

    pub fn from_2d_data_value(i: i32) -> Direction {
        [
            Direction::South,
            Direction::West,
            Direction::North,
            Direction::East,
        ][(i % 4).unsigned_abs() as usize]
    }

    /// `Direction.fromYRot(yaw)`.
    pub fn from_y_rot(yaw: f64) -> Direction {
        Direction::from_2d_data_value(Mth::floor(yaw / 90.0 + 0.5) & 3)
    }

    /// `toYRot`: south 0, west 90, north 180, east 270; up and down 270
    /// (Java's `-1 & 3`).
    pub fn to_y_rot(self) -> f32 {
        ((self.get_2d_data_value() & 3) * 90) as f32
    }

    pub fn from_axis_and_direction(axis: Axis, dir: AxisDirection) -> Direction {
        let positive = dir == AxisDirection::Positive;
        match axis {
            Axis::X if positive => Direction::East,
            Axis::X => Direction::West,
            Axis::Y if positive => Direction::Up,
            Axis::Y => Direction::Down,
            Axis::Z if positive => Direction::South,
            Axis::Z => Direction::North,
        }
    }

    /// `Direction.get(axisDirection, axis)`.
    pub fn get(dir: AxisDirection, axis: Axis) -> Direction {
        Direction::from_axis_and_direction(axis, dir)
    }

    /// `Direction.fromDelta(x, y, z)`: a unit step's direction.
    pub fn from_delta(x: i32, y: i32, z: i32) -> Option<Direction> {
        Direction::VALUES.into_iter().find(|d| {
            let n = d.get_normal();
            (n.x, n.y, n.z) == (x.signum(), y.signum(), z.signum())
                && [x, y, z].iter().filter(|v| **v != 0).count() == 1
        })
    }

    pub fn get_step_x(self) -> i32 {
        self.get_normal().x
    }

    pub fn get_step_y(self) -> i32 {
        self.get_normal().y
    }

    pub fn get_step_z(self) -> i32 {
        self.get_normal().z
    }

    pub fn get_normal(self) -> BlockPos {
        match self {
            Direction::Down => BlockPos::new(0, -1, 0),
            Direction::Up => BlockPos::new(0, 1, 0),
            Direction::North => BlockPos::new(0, 0, -1),
            Direction::South => BlockPos::new(0, 0, 1),
            Direction::West => BlockPos::new(-1, 0, 0),
            Direction::East => BlockPos::new(1, 0, 0),
        }
    }

    /// `step()`: the normal as floats.
    pub fn step(self) -> [f32; 3] {
        let n = self.get_normal();
        [n.x as f32, n.y as f32, n.z as f32]
    }

    pub fn get_opposite(self) -> Direction {
        match self {
            Direction::Down => Direction::Up,
            Direction::Up => Direction::Down,
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::East => Direction::West,
        }
    }

    /// `getClockWise()`, seen from above. Java throws for up and down;
    /// they stay here.
    pub fn get_clock_wise(self) -> Direction {
        match self {
            Direction::North => Direction::East,
            Direction::East => Direction::South,
            Direction::South => Direction::West,
            Direction::West => Direction::North,
            d => d,
        }
    }

    pub fn get_counter_clock_wise(self) -> Direction {
        match self {
            Direction::North => Direction::West,
            Direction::West => Direction::South,
            Direction::South => Direction::East,
            Direction::East => Direction::North,
            d => d,
        }
    }

    /// `getClockWise(axis)`; directions along the axis stay.
    pub fn get_clock_wise_about(self, axis: Axis) -> Direction {
        use Direction::*;
        match axis {
            Axis::X => match self {
                Down => South,
                Up => North,
                North => Down,
                South => Up,
                d => d,
            },
            Axis::Y => self.get_clock_wise(),
            Axis::Z => match self {
                Down => West,
                Up => East,
                West => Up,
                East => Down,
                d => d,
            },
        }
    }

    /// `getCounterClockWise(axis)`.
    pub fn get_counter_clock_wise_about(self, axis: Axis) -> Direction {
        use Direction::*;
        match axis {
            Axis::X => match self {
                Down => North,
                Up => South,
                North => Up,
                South => Down,
                d => d,
            },
            Axis::Y => self.get_counter_clock_wise(),
            Axis::Z => match self {
                Down => East,
                Up => West,
                West => Down,
                East => Up,
                d => d,
            },
        }
    }

    /// `Direction.getNearest(x, y, z)`: the largest dot product; the first
    /// in declaration order on a tie; north when none is positive.
    pub fn get_nearest(x: f32, y: f32, z: f32) -> Direction {
        let mut best = Direction::North;
        // Java's `Float.MIN_VALUE`: the smallest denormal.
        let mut max = f32::from_bits(1);
        for d in Direction::VALUES {
            let n = d.get_normal();
            let dot = x * n.x as f32 + y * n.y as f32 + z * n.z as f32;
            if dot > max {
                max = dot;
                best = d;
            }
        }
        best
    }

    pub fn get_nearest_vec(v: Vec3) -> Direction {
        Direction::get_nearest(v.x as f32, v.y as f32, v.z as f32)
    }

    /// `isFacingAngle(yaw)`.
    pub fn is_facing_angle(self, yaw: f32) -> bool {
        let r = yaw * (std::f32::consts::PI / 180.0);
        let n = self.get_normal();
        n.x as f32 * -Mth::sin(r) + n.z as f32 * Mth::cos(r) > 0.0
    }

    /// `getRotation()`: the quaternion that turns up onto this direction.
    pub fn get_rotation(self) -> super::pose::Quaternion {
        use super::pose::Quaternion;
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            Direction::Down => Quaternion::rotation_x(PI),
            Direction::Up => Quaternion::IDENTITY,
            Direction::North => Quaternion::rotation_xyz(FRAC_PI_2, 0.0, PI),
            Direction::South => Quaternion::rotation_x(FRAC_PI_2),
            Direction::West => Quaternion::rotation_xyz(FRAC_PI_2, 0.0, FRAC_PI_2),
            Direction::East => Quaternion::rotation_xyz(FRAC_PI_2, 0.0, -FRAC_PI_2),
        }
    }

    /// `Iterate.horizontalDirections`'s order (south, west, north, east).
    pub const HORIZONTALS_2D: [Direction; 4] = [
        Direction::South,
        Direction::West,
        Direction::North,
        Direction::East,
    ];
}

/// A block state's `Rotation` (`rotate(state, rotation)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    None,
    Clockwise90,
    Clockwise180,
    Counterclockwise90,
}

impl Rotation {
    pub fn rotate(self, d: Direction) -> Direction {
        if d.get_axis() == Axis::Y {
            return d;
        }
        match self {
            Rotation::None => d,
            Rotation::Clockwise90 => d.get_clock_wise(),
            Rotation::Clockwise180 => d.get_opposite(),
            Rotation::Counterclockwise90 => d.get_counter_clock_wise(),
        }
    }

    /// `rotate(rotation, positionCount)`: a 16-step rotation (signs,
    /// skulls).
    pub fn rotate_count(self, rotation: i32, count: i32) -> i32 {
        let q = match self {
            Rotation::None => 0,
            Rotation::Clockwise90 => count / 4,
            Rotation::Clockwise180 => count / 2,
            Rotation::Counterclockwise90 => count * 3 / 4,
        };
        (rotation + q) % count
    }
}

/// A block state's `Mirror`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Mirror {
    #[default]
    None,
    LeftRight,
    FrontBack,
}

impl Mirror {
    pub fn mirror(self, d: Direction) -> Direction {
        match (self, d.get_axis()) {
            (Mirror::FrontBack, Axis::X) | (Mirror::LeftRight, Axis::Z) => d.get_opposite(),
            _ => d,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_follows_vanilla() {
        assert_eq!(Direction::get_nearest(0.0, 0.0, 0.0), Direction::North);
        assert_eq!(Direction::get_nearest(3.0, 0.0, 0.0), Direction::East);
        assert_eq!(Direction::get_nearest(0.0, -2.0, 0.0), Direction::Down);
        assert_eq!(Direction::get_nearest(1.0, 1.0, 0.0), Direction::Up);
        // Java's MIN_VALUE is a denormal: a tiny push still counts.
        assert_eq!(Direction::get_nearest(1e-40, 0.0, 0.0), Direction::East);
    }

    #[test]
    fn rotations_and_data_values_match_vanilla() {
        assert_eq!(Direction::North.get_clock_wise(), Direction::East);
        assert_eq!(
            Direction::North.get_clock_wise_about(Axis::X),
            Direction::Down
        );
        assert_eq!(Direction::Up.get_clock_wise_about(Axis::Z), Direction::East);
        assert_eq!(Direction::West.to_y_rot(), 90.0);
        assert_eq!(Direction::from_y_rot(-90.0), Direction::East);
        assert_eq!(Direction::from_y_rot(179.0), Direction::North);
        assert_eq!(Direction::from_3d_data_value(-7), Direction::Up);
        assert_eq!(Direction::from_delta(0, 0, 4), Some(Direction::South));
        assert_eq!(Direction::from_delta(0, 1, 4), None);
        assert_eq!(Direction::from_delta(0, 0, 1), Some(Direction::South));
        assert!(Direction::South.is_facing_angle(0.0));
        assert_eq!(
            Rotation::Clockwise90.rotate(Direction::West),
            Direction::North
        );
        assert_eq!(Mirror::FrontBack.mirror(Direction::East), Direction::West);
    }
}
