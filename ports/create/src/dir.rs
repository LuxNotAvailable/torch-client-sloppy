//! Minecraft's `Direction` and `Direction.Axis`, with the angle helpers
//! Create's rendering uses (Catnip's `AngleHelper`, `CachedBuffers`).

use torch_mod_api::Transform;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn parse(s: &str) -> Option<Axis> {
        match s {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "z" => Some(Axis::Z),
            _ => None,
        }
    }

    /// The axis's positive unit vector.
    pub fn vector(self) -> [f32; 3] {
        match self {
            Axis::X => [1.0, 0.0, 0.0],
            Axis::Y => [0.0, 1.0, 0.0],
            Axis::Z => [0.0, 0.0, 1.0],
        }
    }

    /// `Axis.choose(x, y, z)`: the coordinate along this axis.
    pub fn choose(self, pos: [i32; 3]) -> i32 {
        match self {
            Axis::X => pos[0],
            Axis::Y => pos[1],
            Axis::Z => pos[2],
        }
    }
}

/// `Direction`, in its declaration order (down, up, north, south, west,
/// east): `getNearest` breaks ties by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

pub const ALL: [Dir; 6] = [
    Dir::Down,
    Dir::Up,
    Dir::North,
    Dir::South,
    Dir::West,
    Dir::East,
];

impl Dir {
    pub fn parse(s: &str) -> Option<Dir> {
        ALL.into_iter().find(|d| d.name() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Dir::Down => "down",
            Dir::Up => "up",
            Dir::North => "north",
            Dir::South => "south",
            Dir::West => "west",
            Dir::East => "east",
        }
    }

    pub fn axis(self) -> Axis {
        match self {
            Dir::Down | Dir::Up => Axis::Y,
            Dir::North | Dir::South => Axis::Z,
            Dir::West | Dir::East => Axis::X,
        }
    }

    /// Whether it points along its axis's positive direction.
    pub fn positive(self) -> bool {
        matches!(self, Dir::Up | Dir::South | Dir::East)
    }

    /// `Direction.fromAxisAndDirection`.
    pub fn from_axis(axis: Axis, positive: bool) -> Dir {
        match (axis, positive) {
            (Axis::Y, false) => Dir::Down,
            (Axis::Y, true) => Dir::Up,
            (Axis::Z, false) => Dir::North,
            (Axis::Z, true) => Dir::South,
            (Axis::X, false) => Dir::West,
            (Axis::X, true) => Dir::East,
        }
    }

    /// The unit step toward it.
    pub fn normal(self) -> [i32; 3] {
        match self {
            Dir::Down => [0, -1, 0],
            Dir::Up => [0, 1, 0],
            Dir::North => [0, 0, -1],
            Dir::South => [0, 0, 1],
            Dir::West => [-1, 0, 0],
            Dir::East => [1, 0, 0],
        }
    }

    /// `Direction.getNearest(x, y, z)`: the largest dot product, the first
    /// in declaration order on a tie, north when all are zero.
    pub fn nearest(v: [i32; 3]) -> Dir {
        let mut best = Dir::North;
        let mut max = f32::MIN_POSITIVE;
        for d in ALL {
            let n = d.normal();
            let dot = (v[0] * n[0] + v[1] * n[1] + v[2] * n[2]) as f32;
            if dot > max {
                max = dot;
                best = d;
            }
        }
        best
    }

    /// `Direction.toYRot`: south 0, west 90, north 180, east 270.
    fn y_rot(self) -> f32 {
        match self {
            Dir::South => 0.0,
            Dir::West => 90.0,
            Dir::North => 180.0,
            Dir::East => 270.0,
            Dir::Down | Dir::Up => 0.0,
        }
    }

    /// `AngleHelper.horizontalAngle`.
    pub fn horizontal_angle(self) -> f32 {
        if self.axis() == Axis::Y {
            return 0.0;
        }
        let angle = self.y_rot();
        if self.axis() == Axis::X {
            -angle
        } else {
            angle
        }
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::West => Dir::East,
            Dir::East => Dir::West,
        }
    }

    /// `Direction.getClockWise`, seen from above; up and down stay.
    pub fn clockwise(self) -> Dir {
        match self {
            Dir::North => Dir::East,
            Dir::East => Dir::South,
            Dir::South => Dir::West,
            Dir::West => Dir::North,
            other => other,
        }
    }

    /// `AngleHelper.verticalAngle`.
    pub fn vertical_angle(self) -> f32 {
        match self {
            Dir::Up => -90.0,
            Dir::Down => 90.0,
            _ => 0.0,
        }
    }
}

/// `CachedBuffers.rotateToFace(facing)`: about the centre, Y by the
/// horizontal angle after X by the vertical one.
pub fn to_face(facing: Dir) -> Transform {
    Transform {
        rotate: [facing.vertical_angle(), facing.horizontal_angle(), 0.0],
        ..Transform::default()
    }
}

/// `CachedBuffers.rotateToFaceVertical(facing)`: as `to_face`, with X
/// turned a further 90 degrees.
pub fn to_face_vertical(facing: Dir) -> Transform {
    Transform {
        rotate: [
            facing.vertical_angle() + 90.0,
            facing.horizontal_angle(),
            0.0,
        ],
        ..Transform::default()
    }
}

/// Java's `rotateXDegrees`/`Y`/`Z` calls on a pose, in call order, as a
/// [`Transform::rotate`] (X, then Y, then Z, on the vertex). Java applies
/// the last call to the vertex first.
pub fn java_rotations(calls: &[(Axis, f32)]) -> [f32; 3] {
    type M = [[f32; 3]; 3];
    let mul = |a: M, b: M| -> M {
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
    };
    let mut m: M = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for &(axis, degrees) in calls {
        let (s, c) = degrees.to_radians().sin_cos();
        let r = match axis {
            Axis::X => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
            Axis::Y => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
            Axis::Z => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
        };
        m = mul(m, r);
    }
    // m = Rz(z) Ry(y) Rx(x).
    let y = (-m[2][0]).clamp(-1.0, 1.0).asin();
    let (x, z) = if y.cos() > 1e-4 {
        (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0]))
    } else {
        ((-m[1][2]).atan2(m[1][1]), 0.0)
    };
    let tidy = |r: f32| {
        let d = r.to_degrees();
        if d.abs() < 1e-3 { 0.0 } else { d }
    };
    [tidy(x), tidy(y), tidy(z)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point turned by Java-ordered calls, the way `java_rotations`'s
    /// result turns it (X, then Y, then Z).
    fn turn(rotate: [f32; 3], p: [f32; 3]) -> [f32; 3] {
        let one = |axis: Axis, d: f32, p: [f32; 3]| {
            let (s, c) = d.to_radians().sin_cos();
            match axis {
                Axis::X => [p[0], c * p[1] - s * p[2], s * p[1] + c * p[2]],
                Axis::Y => [c * p[0] + s * p[2], p[1], -s * p[0] + c * p[2]],
                Axis::Z => [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]],
            }
        };
        one(
            Axis::Z,
            rotate[2],
            one(Axis::Y, rotate[1], one(Axis::X, rotate[0], p)),
        )
    }

    #[test]
    fn java_order_rotations_become_xyz_ones() {
        // rotateY(90) then rotateZ(90): Z reaches the vertex first.
        let r = java_rotations(&[(Axis::Y, 90.0), (Axis::Z, 90.0)]);
        let p = turn(r, [1.0, 0.0, 0.0]);
        // Z: x -> y; then Y leaves y.
        assert!(
            (p[1] - 1.0).abs() < 1e-4 && p[0].abs() < 1e-4,
            "{r:?} {p:?}"
        );
        let q = turn(r, [0.0, 0.0, 1.0]);
        // Z leaves z; Y turns z -> x.
        assert!((q[0] - 1.0).abs() < 1e-4, "{r:?} {q:?}");
    }

    #[test]
    fn nearest_follows_minecraft() {
        assert_eq!(Dir::nearest([0, 0, 0]), Dir::North);
        assert_eq!(Dir::nearest([3, 0, 0]), Dir::East);
        assert_eq!(Dir::nearest([0, -2, 0]), Dir::Down);
        // A tie goes to the first in declaration order.
        assert_eq!(Dir::nearest([1, 1, 0]), Dir::Up);
    }

    #[test]
    fn angles_match_angle_helper() {
        assert_eq!(Dir::South.horizontal_angle(), 0.0);
        assert_eq!(Dir::North.horizontal_angle(), 180.0);
        assert_eq!(Dir::West.horizontal_angle(), -90.0);
        assert_eq!(Dir::East.horizontal_angle(), -270.0);
        assert_eq!(Dir::Up.vertical_angle(), -90.0);
        assert_eq!(to_face_vertical(Dir::Up).rotate, [0.0, 0.0, 0.0]);
    }
}
