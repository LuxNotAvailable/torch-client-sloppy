//! Vanilla's `Vec3`, `BlockPos`/`Vec3i` and `AABB`, with Java's method
//! names in snake case (`vec.distanceToSqr(o)` is `vec.distance_to_sqr(o)`).
//! Overloads taking loose numbers end in `_xyz` (`add_xyz(x, y, z)`).

use std::ops::{Add, Mul, Neg, Sub};

use super::direction::{Axis, Direction};
use super::java::JavaRandom;
use super::mth::Mth;

/// `net.minecraft.world.phys.Vec3`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    pub fn from_array(v: [f64; 3]) -> Vec3 {
        Vec3::new(v[0], v[1], v[2])
    }

    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// `toVector3f()`.
    pub fn to_vector3f(self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }

    pub fn from_rgb24(rgb: i32) -> Vec3 {
        Vec3::new(
            ((rgb >> 16) & 255) as f64 / 255.0,
            ((rgb >> 8) & 255) as f64 / 255.0,
            (rgb & 255) as f64 / 255.0,
        )
    }

    pub fn at_lower_corner_of(p: BlockPos) -> Vec3 {
        Vec3::new(p.x as f64, p.y as f64, p.z as f64)
    }

    pub fn at_lower_corner_with_offset(p: BlockPos, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(p.x as f64 + x, p.y as f64 + y, p.z as f64 + z)
    }

    pub fn at_center_of(p: BlockPos) -> Vec3 {
        Vec3::at_lower_corner_with_offset(p, 0.5, 0.5, 0.5)
    }

    pub fn at_bottom_center_of(p: BlockPos) -> Vec3 {
        Vec3::at_lower_corner_with_offset(p, 0.5, 0.0, 0.5)
    }

    pub fn up_from_bottom_center_of(p: BlockPos, y: f64) -> Vec3 {
        Vec3::at_lower_corner_with_offset(p, 0.5, y, 0.5)
    }

    pub fn vector_to(self, o: Vec3) -> Vec3 {
        o - self
    }

    /// Zero when shorter than 1e-4, as Java's.
    pub fn normalize(self) -> Vec3 {
        let d = self.length();
        if d < 1.0e-4 {
            Vec3::ZERO
        } else {
            Vec3::new(self.x / d, self.y / d, self.z / d)
        }
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn subtract(self, o: Vec3) -> Vec3 {
        self - o
    }

    pub fn subtract_xyz(self, x: f64, y: f64, z: f64) -> Vec3 {
        self.add_xyz(-x, -y, -z)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add(self, o: Vec3) -> Vec3 {
        self + o
    }

    pub fn add_xyz(self, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(self.x + x, self.y + y, self.z + z)
    }

    pub fn closer_than(self, o: Vec3, distance: f64) -> bool {
        self.distance_to_sqr(o) < distance * distance
    }

    /// `closerThan(other, horizontal, vertical)`.
    pub fn closer_than_hv(self, o: Vec3, horizontal: f64, vertical: f64) -> bool {
        let d = o - self;
        d.x * d.x + d.z * d.z < horizontal * horizontal && d.y.abs() < vertical
    }

    pub fn distance_to(self, o: Vec3) -> f64 {
        self.distance_to_sqr(o).sqrt()
    }

    pub fn distance_to_sqr(self, o: Vec3) -> f64 {
        (o - self).length_sqr()
    }

    pub fn scale(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn reverse(self) -> Vec3 {
        self.scale(-1.0)
    }

    pub fn multiply(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    pub fn multiply_xyz(self, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(self.x * x, self.y * y, self.z * z)
    }

    pub fn offset_random(self, r: &mut JavaRandom, f: f32) -> Vec3 {
        let x = ((r.next_float() - 0.5) * f) as f64;
        let y = ((r.next_float() - 0.5) * f) as f64;
        let z = ((r.next_float() - 0.5) * f) as f64;
        self.add_xyz(x, y, z)
    }

    pub fn length(self) -> f64 {
        self.length_sqr().sqrt()
    }

    pub fn length_sqr(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn horizontal_distance(self) -> f64 {
        self.horizontal_distance_sqr().sqrt()
    }

    pub fn horizontal_distance_sqr(self) -> f64 {
        self.x * self.x + self.z * self.z
    }

    /// `a.lerp(b, delta)`.
    pub fn lerp(self, o: Vec3, delta: f64) -> Vec3 {
        Vec3::new(
            Mth::lerp(delta, self.x, o.x),
            Mth::lerp(delta, self.y, o.y),
            Mth::lerp(delta, self.z, o.z),
        )
    }

    /// `xRot(radians)`, with `Mth`'s table.
    pub fn x_rot(self, r: f32) -> Vec3 {
        let (c, s) = (Mth::cos(r) as f64, Mth::sin(r) as f64);
        Vec3::new(self.x, self.y * c + self.z * s, self.z * c - self.y * s)
    }

    pub fn y_rot(self, r: f32) -> Vec3 {
        let (c, s) = (Mth::cos(r) as f64, Mth::sin(r) as f64);
        Vec3::new(self.x * c + self.z * s, self.y, self.z * c - self.x * s)
    }

    pub fn z_rot(self, r: f32) -> Vec3 {
        let (c, s) = (Mth::cos(r) as f64, Mth::sin(r) as f64);
        Vec3::new(self.x * c + self.y * s, self.y * c - self.x * s, self.z)
    }

    /// `Vec3.directionFromRotation(xRot, yRot)`: a look vector.
    pub fn direction_from_rotation(x_rot: f32, y_rot: f32) -> Vec3 {
        let r = std::f32::consts::PI / 180.0;
        let c = Mth::cos(-y_rot * r - std::f32::consts::PI);
        let s = Mth::sin(-y_rot * r - std::f32::consts::PI);
        let pc = -Mth::cos(-x_rot * r);
        let ps = Mth::sin(-x_rot * r);
        Vec3::new((s * pc) as f64, ps as f64, (c * pc) as f64)
    }

    /// `align(axes)`: floors the given axes.
    pub fn align(self, axes: &[Axis]) -> Vec3 {
        let f = |a: Axis, v: f64| {
            if axes.contains(&a) {
                Mth::floor(v) as f64
            } else {
                v
            }
        };
        Vec3::new(f(Axis::X, self.x), f(Axis::Y, self.y), f(Axis::Z, self.z))
    }

    pub fn get(self, axis: Axis) -> f64 {
        axis.choose(self.x, self.y, self.z)
    }

    pub fn with(self, axis: Axis, v: f64) -> Vec3 {
        Vec3::new(
            if axis == Axis::X { v } else { self.x },
            if axis == Axis::Y { v } else { self.y },
            if axis == Axis::Z { v } else { self.z },
        )
    }

    pub fn relative(self, d: Direction, distance: f64) -> Vec3 {
        let n = d.get_normal();
        self.add_xyz(
            distance * n.x as f64,
            distance * n.y as f64,
            distance * n.z as f64,
        )
    }

    /// `BlockPos.containing(vec)`.
    pub fn block_pos(self) -> BlockPos {
        BlockPos::containing(self.x, self.y, self.z)
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        self.scale(s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        self.reverse()
    }
}

impl From<[f64; 3]> for Vec3 {
    fn from(v: [f64; 3]) -> Vec3 {
        Vec3::from_array(v)
    }
}

impl From<Vec3> for [f64; 3] {
    fn from(v: Vec3) -> [f64; 3] {
        v.to_array()
    }
}

/// `BlockPos` (and `Vec3i`, its parent).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const ZERO: BlockPos = BlockPos::new(0, 0, 0);

    pub const fn new(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos { x, y, z }
    }

    pub fn from_array(p: [i32; 3]) -> BlockPos {
        BlockPos::new(p[0], p[1], p[2])
    }

    pub fn to_array(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }

    /// `BlockPos.containing(x, y, z)`: the block a point is in.
    pub fn containing(x: f64, y: f64, z: f64) -> BlockPos {
        BlockPos::new(Mth::floor(x), Mth::floor(y), Mth::floor(z))
    }

    pub fn get_x(self) -> i32 {
        self.x
    }

    pub fn get_y(self) -> i32 {
        self.y
    }

    pub fn get_z(self) -> i32 {
        self.z
    }

    pub fn get(self, axis: Axis) -> i32 {
        axis.choose(self.x, self.y, self.z)
    }

    pub fn offset(self, x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos::new(self.x + x, self.y + y, self.z + z)
    }

    /// `offset(Vec3i)`.
    pub fn offset_by(self, o: BlockPos) -> BlockPos {
        self + o
    }

    pub fn subtract(self, o: BlockPos) -> BlockPos {
        self - o
    }

    pub fn multiply(self, s: i32) -> BlockPos {
        BlockPos::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn relative(self, d: Direction) -> BlockPos {
        self + d.get_normal()
    }

    /// `relative(direction, distance)`.
    pub fn relative_by(self, d: Direction, n: i32) -> BlockPos {
        self + d.get_normal().multiply(n)
    }

    /// `relative(axis, distance)`.
    pub fn relative_axis(self, axis: Axis, n: i32) -> BlockPos {
        match axis {
            Axis::X => self.offset(n, 0, 0),
            Axis::Y => self.offset(0, n, 0),
            Axis::Z => self.offset(0, 0, n),
        }
    }

    pub fn above(self) -> BlockPos {
        self.offset(0, 1, 0)
    }

    pub fn above_by(self, n: i32) -> BlockPos {
        self.offset(0, n, 0)
    }

    pub fn below(self) -> BlockPos {
        self.offset(0, -1, 0)
    }

    pub fn below_by(self, n: i32) -> BlockPos {
        self.offset(0, -n, 0)
    }

    pub fn north(self) -> BlockPos {
        self.offset(0, 0, -1)
    }

    pub fn south(self) -> BlockPos {
        self.offset(0, 0, 1)
    }

    pub fn west(self) -> BlockPos {
        self.offset(-1, 0, 0)
    }

    pub fn east(self) -> BlockPos {
        self.offset(1, 0, 0)
    }

    pub fn cross(self, o: BlockPos) -> BlockPos {
        BlockPos::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// `distSqr(other)`, between corners.
    pub fn dist_sqr(self, o: BlockPos) -> f64 {
        let d = self - o;
        (d.x as f64).powi(2) + (d.y as f64).powi(2) + (d.z as f64).powi(2)
    }

    /// `distToCenterSqr(x, y, z)`: from this block's centre.
    pub fn dist_to_center_sqr(self, x: f64, y: f64, z: f64) -> f64 {
        let (dx, dy, dz) = (
            self.x as f64 + 0.5 - x,
            self.y as f64 + 0.5 - y,
            self.z as f64 + 0.5 - z,
        );
        dx * dx + dy * dy + dz * dz
    }

    pub fn dist_manhattan(self, o: BlockPos) -> i32 {
        (self.x - o.x).abs() + (self.y - o.y).abs() + (self.z - o.z).abs()
    }

    pub fn closer_than(self, o: BlockPos, distance: f64) -> bool {
        self.dist_sqr(o) < distance * distance
    }

    /// `getCenter()`.
    pub fn get_center(self) -> Vec3 {
        Vec3::at_center_of(self)
    }

    /// `asLong()`: the packed form (`BlockPos.of(long)` reverses it).
    pub fn as_long(self) -> i64 {
        ((self.x as i64 & 0x3FF_FFFF) << 38)
            | ((self.z as i64 & 0x3FF_FFFF) << 12)
            | (self.y as i64 & 0xFFF)
    }

    pub fn of(packed: i64) -> BlockPos {
        BlockPos::new(
            (packed >> 38) as i32,
            ((packed << 52) >> 52) as i32,
            ((packed << 26) >> 38) as i32,
        )
    }
}

impl Add for BlockPos {
    type Output = BlockPos;
    fn add(self, o: BlockPos) -> BlockPos {
        BlockPos::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for BlockPos {
    type Output = BlockPos;
    fn sub(self, o: BlockPos) -> BlockPos {
        BlockPos::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl From<[i32; 3]> for BlockPos {
    fn from(p: [i32; 3]) -> BlockPos {
        BlockPos::from_array(p)
    }
}

impl From<BlockPos> for [i32; 3] {
    fn from(p: BlockPos) -> [i32; 3] {
        p.to_array()
    }
}

/// `AABB`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AABB {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl AABB {
    /// `new AABB(x1, y1, z1, x2, y2, z2)`: corners in any order.
    pub fn new(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> AABB {
        AABB {
            min_x: x1.min(x2),
            min_y: y1.min(y2),
            min_z: z1.min(z2),
            max_x: x1.max(x2),
            max_y: y1.max(y2),
            max_z: z1.max(z2),
        }
    }

    /// `new AABB(vec1, vec2)`.
    pub fn of_corners(a: Vec3, b: Vec3) -> AABB {
        AABB::new(a.x, a.y, a.z, b.x, b.y, b.z)
    }

    /// `new AABB(blockPos)`: the block's unit cube.
    pub fn of_block(p: BlockPos) -> AABB {
        let (x, y, z) = (p.x as f64, p.y as f64, p.z as f64);
        AABB::new(x, y, z, x + 1.0, y + 1.0, z + 1.0)
    }

    pub fn unit_cube_from_lower_corner(v: Vec3) -> AABB {
        AABB::new(v.x, v.y, v.z, v.x + 1.0, v.y + 1.0, v.z + 1.0)
    }

    pub fn of_size(center: Vec3, x: f64, y: f64, z: f64) -> AABB {
        AABB::new(
            center.x - x / 2.0,
            center.y - y / 2.0,
            center.z - z / 2.0,
            center.x + x / 2.0,
            center.y + y / 2.0,
            center.z + z / 2.0,
        )
    }

    /// An entity's box: `EntityDimensions.makeBoundingBox(pos)`.
    pub fn entity(feet: Vec3, width: f64, height: f64) -> AABB {
        let w = width / 2.0;
        AABB::new(
            feet.x - w,
            feet.y,
            feet.z - w,
            feet.x + w,
            feet.y + height,
            feet.z + w,
        )
    }

    pub fn min(self, axis: Axis) -> f64 {
        axis.choose(self.min_x, self.min_y, self.min_z)
    }

    pub fn max(self, axis: Axis) -> f64 {
        axis.choose(self.max_x, self.max_y, self.max_z)
    }

    pub fn contract(self, x: f64, y: f64, z: f64) -> AABB {
        let mut b = self;
        if x < 0.0 {
            b.min_x -= x
        } else if x > 0.0 {
            b.max_x -= x
        }
        if y < 0.0 {
            b.min_y -= y
        } else if y > 0.0 {
            b.max_y -= y
        }
        if z < 0.0 {
            b.min_z -= z
        } else if z > 0.0 {
            b.max_z -= z
        }
        AABB::new(b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z)
    }

    pub fn expand_towards(self, x: f64, y: f64, z: f64) -> AABB {
        let mut b = self;
        if x < 0.0 {
            b.min_x += x
        } else if x > 0.0 {
            b.max_x += x
        }
        if y < 0.0 {
            b.min_y += y
        } else if y > 0.0 {
            b.max_y += y
        }
        if z < 0.0 {
            b.min_z += z
        } else if z > 0.0 {
            b.max_z += z
        }
        AABB::new(b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z)
    }

    pub fn inflate_xyz(self, x: f64, y: f64, z: f64) -> AABB {
        AABB::new(
            self.min_x - x,
            self.min_y - y,
            self.min_z - z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }

    pub fn inflate(self, v: f64) -> AABB {
        self.inflate_xyz(v, v, v)
    }

    pub fn deflate_xyz(self, x: f64, y: f64, z: f64) -> AABB {
        self.inflate_xyz(-x, -y, -z)
    }

    pub fn deflate(self, v: f64) -> AABB {
        self.inflate(-v)
    }

    pub fn intersect(self, o: AABB) -> AABB {
        AABB::new(
            self.min_x.max(o.min_x),
            self.min_y.max(o.min_y),
            self.min_z.max(o.min_z),
            self.max_x.min(o.max_x),
            self.max_y.min(o.max_y),
            self.max_z.min(o.max_z),
        )
    }

    pub fn minmax(self, o: AABB) -> AABB {
        AABB::new(
            self.min_x.min(o.min_x),
            self.min_y.min(o.min_y),
            self.min_z.min(o.min_z),
            self.max_x.max(o.max_x),
            self.max_y.max(o.max_y),
            self.max_z.max(o.max_z),
        )
    }

    /// `move(x, y, z)` (`move` is a Rust keyword).
    pub fn move_xyz(self, x: f64, y: f64, z: f64) -> AABB {
        AABB {
            min_x: self.min_x + x,
            min_y: self.min_y + y,
            min_z: self.min_z + z,
            max_x: self.max_x + x,
            max_y: self.max_y + y,
            max_z: self.max_z + z,
        }
    }

    /// `move(vec)`.
    pub fn move_by(self, v: Vec3) -> AABB {
        self.move_xyz(v.x, v.y, v.z)
    }

    pub fn intersects(self, o: AABB) -> bool {
        self.min_x < o.max_x
            && self.max_x > o.min_x
            && self.min_y < o.max_y
            && self.max_y > o.min_y
            && self.min_z < o.max_z
            && self.max_z > o.min_z
    }

    /// `contains(vec)`: min inclusive, max exclusive.
    pub fn contains(self, v: Vec3) -> bool {
        v.x >= self.min_x
            && v.x < self.max_x
            && v.y >= self.min_y
            && v.y < self.max_y
            && v.z >= self.min_z
            && v.z < self.max_z
    }

    pub fn get_xsize(self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn get_ysize(self) -> f64 {
        self.max_y - self.min_y
    }

    pub fn get_zsize(self) -> f64 {
        self.max_z - self.min_z
    }

    /// `getSize()`: the mean edge.
    pub fn get_size(self) -> f64 {
        (self.get_xsize() + self.get_ysize() + self.get_zsize()) / 3.0
    }

    pub fn get_center(self) -> Vec3 {
        Vec3::new(
            Mth::lerp(0.5, self.min_x, self.max_x),
            Mth::lerp(0.5, self.min_y, self.max_y),
            Mth::lerp(0.5, self.min_z, self.max_z),
        )
    }

    pub fn get_bottom_center(self) -> Vec3 {
        Vec3::new(
            Mth::lerp(0.5, self.min_x, self.max_x),
            self.min_y,
            Mth::lerp(0.5, self.min_z, self.max_z),
        )
    }

    pub fn get_min_position(self) -> Vec3 {
        Vec3::new(self.min_x, self.min_y, self.min_z)
    }

    pub fn get_max_position(self) -> Vec3 {
        Vec3::new(self.max_x, self.max_y, self.max_z)
    }

    pub fn distance_to_sqr(self, v: Vec3) -> f64 {
        let dx = (self.min_x - v.x).max(v.x - self.max_x).max(0.0);
        let dy = (self.min_y - v.y).max(v.y - self.max_y).max(0.0);
        let dz = (self.min_z - v.z).max(v.z - self.max_z).max(0.0);
        dx * dx + dy * dy + dz * dz
    }

    /// `clip(from, to)`: where a segment first enters the box.
    pub fn clip(self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let d = to - from;
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for (o, dv, lo, hi) in [
            (from.x, d.x, self.min_x, self.max_x),
            (from.y, d.y, self.min_y, self.max_y),
            (from.z, d.z, self.min_z, self.max_z),
        ] {
            if dv.abs() < 1.0e-7 {
                if o < lo || o > hi {
                    return None;
                }
            } else {
                let (a, b) = ((lo - o) / dv, (hi - o) / dv);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(from + d * t0)
    }

    pub fn has_nan(self) -> bool {
        [
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        ]
        .iter()
        .any(|v| v.is_nan())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_behave_as_vanillas() {
        let v = Vec3::new(1.0, 0.0, 0.0);
        let r = v.y_rot(std::f32::consts::FRAC_PI_2);
        assert!(r.x.abs() < 1e-6 && (r.z + 1.0).abs() < 1e-6, "{r:?}");
        assert_eq!(Vec3::new(1e-5, 0.0, 0.0).normalize(), Vec3::ZERO);
        assert_eq!(
            BlockPos::containing(-0.5, 1.2, 3.0),
            BlockPos::new(-1, 1, 3)
        );
        let p = BlockPos::new(-5, 70, 1234);
        assert_eq!(BlockPos::of(p.as_long()), p);
        let b = AABB::entity(Vec3::new(0.5, 1.0, 0.5), 0.6, 1.8);
        assert!(b.contains(Vec3::new(0.5, 2.0, 0.5)));
        assert!(!b.contains(Vec3::new(0.5, 2.8, 0.5)));
        let hit = AABB::of_block(BlockPos::ZERO)
            .clip(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(2.0, 0.5, 0.5));
        assert_eq!(hit, Some(Vec3::new(0.0, 0.5, 0.5)));
    }
}
