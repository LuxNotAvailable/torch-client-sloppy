//! Catnip (`net.createmod.catnip`), the library Create and its addons are
//! built on: `LerpedFloat`, `PhysicalFloat`, `AngleHelper`, `VecHelper`,
//! `DirectionHelper`, `Pointing`, `Iterate`, `AnimationFunctions`. Same
//! names as Java, methods in snake case. Its `Color` is
//! [`crate::mc::color::Color`], its `NBTHelper` [`crate::mc::nbt::NBTHelper`],
//! its `CachedBuffers`/`SuperByteBuffer` in [`crate::mc::pose`].

use crate::Value;
use crate::mc::direction::{Axis, AxisDirection, Direction, Mirror};
use crate::mc::java::{JMath, JavaRandom};
use crate::mc::mth::Mth;
use crate::mc::nbt::CompoundTag;
use crate::mc::vec::{BlockPos, Vec3};

/// `LerpedFloat.Chaser`: how `tickChaser` moves the value to its target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Chaser {
    /// `Chaser.IDLE`: stays.
    Idle,
    /// `Chaser.LINEAR`: at most `speed` a tick.
    Linear,
    /// `Chaser.exp(max)`: `speed` of the gap a tick, at most `max`.
    Exp(f64),
}

impl Chaser {
    pub const IDLE: Chaser = Chaser::Idle;
    pub const LINEAR: Chaser = Chaser::Linear;
    /// `Chaser.EXP`: `exp(Double.MAX_VALUE)`.
    pub const EXP: Chaser = Chaser::Exp(f64::MAX);

    pub fn exp(max_effective_speed: f64) -> Chaser {
        Chaser::Exp(max_effective_speed)
    }

    /// `chase(current, speed, target)`.
    pub fn chase(self, c: f64, s: f64, t: f64) -> f32 {
        match self {
            Chaser::Idle => c as f32,
            Chaser::Linear => (c + Mth::clamp(t - c, -s, s)) as f32,
            Chaser::Exp(max) => (c + Mth::clamp((t - c) * s, -max, max)) as f32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Interpolator {
    Linear,
    Angular,
}

/// Catnip's `LerpedFloat`: a value chasing a target a tick at a time,
/// read between ticks with `get_value(partial)`. Setters change it in place
/// and return a copy, so Java's builder chains (`LerpedFloat.linear()
/// .startWithValue(0).chase(0, .1, Chaser.EXP)`) read the same.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LerpedFloat {
    interpolator: Interpolator,
    pub previous_value: f32,
    pub value: f32,
    pub chase_function: Option<Chaser>,
    pub chase_target: f32,
    pub chase_speed: f32,
    pub angular_chase: bool,
    pub forced_sync: bool,
}

impl LerpedFloat {
    fn with(interpolator: Interpolator) -> LerpedFloat {
        LerpedFloat {
            interpolator,
            previous_value: 0.0,
            value: 0.0,
            chase_function: None,
            chase_target: 0.0,
            chase_speed: 0.0,
            angular_chase: false,
            forced_sync: true,
        }
    }

    pub fn linear() -> LerpedFloat {
        LerpedFloat::with(Interpolator::Linear)
    }

    /// `angular()`: lerps and chases the short way round.
    pub fn angular() -> LerpedFloat {
        let mut f = LerpedFloat::with(Interpolator::Angular);
        f.angular_chase = true;
        f
    }

    pub fn start_with_value(&mut self, value: f64) -> LerpedFloat {
        let f = value as f32;
        self.previous_value = f;
        self.chase_target = f;
        self.value = f;
        *self
    }

    pub fn chase(&mut self, value: f64, speed: f64, chaser: Chaser) -> LerpedFloat {
        self.update_chase_target(value as f32);
        self.chase_speed = speed as f32;
        self.chase_function = Some(chaser);
        *self
    }

    pub fn chase_timed(&mut self, value: f64, ticks: i32) -> LerpedFloat {
        let diff = value - self.value as f64;
        self.chase(value, (diff / ticks as f64).abs(), Chaser::LINEAR)
    }

    pub fn disable_smart_angle_chasing(&mut self) -> LerpedFloat {
        self.angular_chase = false;
        *self
    }

    pub fn update_chase_target(&mut self, mut target: f32) {
        if self.angular_chase {
            target =
                self.value + AngleHelper::get_shortest_angle_diff(self.value as f64, target as f64);
        }
        self.chase_target = target;
    }

    /// Whether the speed changed.
    pub fn update_chase_speed(&mut self, speed: f64) -> bool {
        let prev = self.chase_speed;
        self.chase_speed = speed as f32;
        !Mth::equal(prev as f64, speed)
    }

    pub fn tick_chaser(&mut self) {
        self.previous_value = self.value;
        if let Some(chaser) = self.chase_function {
            if Mth::equal(self.value, self.chase_target) {
                self.value = self.chase_target;
            } else {
                self.value = chaser.chase(
                    self.value as f64,
                    self.chase_speed as f64,
                    self.chase_target as f64,
                );
            }
        }
    }

    pub fn set_value_no_update(&mut self, value: f64) {
        self.value = value as f32;
    }

    pub fn set_value(&mut self, value: f64) {
        self.previous_value = self.value;
        self.value = value as f32;
    }

    /// `getValue(partialTicks)`; `get_value(1.0)` for Java's `getValue()`.
    pub fn get_value(&self, partial_ticks: f32) -> f32 {
        let (p, c, t) = (
            partial_ticks as f64,
            self.previous_value as f64,
            self.value as f64,
        );
        match self.interpolator {
            Interpolator::Linear => Mth::lerp(p, c, t) as f32,
            Interpolator::Angular => AngleHelper::angle_lerp(p, c, t),
        }
    }

    pub fn settled(&self) -> bool {
        Mth::equal(self.previous_value, self.value)
            && (self.chase_function.is_none() || Mth::equal(self.value, self.chase_target))
    }

    pub fn get_chase_target(&self) -> f32 {
        self.chase_target
    }

    pub fn force_next_sync(&mut self) {
        self.forced_sync = true;
    }

    /// `readNBT(tag, clientPacket)`: the value only on a full read or a
    /// forced sync; target and speed always.
    pub fn read_nbt(&mut self, nbt: &Value, client_packet: bool) {
        if !client_packet || nbt.contains("Force") {
            self.start_with_value(nbt.get_float("Value") as f64);
        }
        self.chase_speed = nbt.get_float("Speed");
        self.chase_target = nbt.get_float("Target");
    }
}

/// A force on a [`PhysicalFloat`] (`Force.Drag`, `Impulse`, ...).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Force {
    Drag(f32),
    /// Once.
    Impulse(f32),
    /// `OverTime(time, total)`: `total / time` a tick for `time` ticks.
    OverTime {
        time_remaining: i32,
        f: f32,
    },
    Static(f32),
    /// `Zeroing(g)`: pulls toward 0 at `g / 20`.
    Zeroing(f32),
}

impl Force {
    pub fn over_time(time: i32, total: f32) -> Force {
        Force::OverTime {
            time_remaining: time,
            f: total / time as f32,
        }
    }

    pub fn zeroing(g: f32) -> Force {
        Force::Zeroing(g / 20.0)
    }

    fn get(&mut self, mass: f32, value: f32, speed: f32) -> f32 {
        match self {
            Force::Drag(d) => -speed * *d,
            Force::Impulse(f) | Force::Static(f) => *f,
            Force::OverTime { time_remaining, f } => {
                *time_remaining -= 1;
                *f
            }
            Force::Zeroing(g) => -JMath::signum(value) * *g * mass,
        }
    }

    fn finished(&self) -> bool {
        match self {
            Force::Impulse(_) => true,
            Force::OverTime { time_remaining, .. } => *time_remaining <= 0,
            _ => false,
        }
    }
}

/// Catnip's `PhysicalFloat`: a value pushed by forces each tick.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicalFloat {
    pub previous_value: f32,
    pub value: f32,
    pub previous_speed: f32,
    pub speed: f32,
    pub limit: f32,
    pub mass: f32,
    pub forces: Vec<Force>,
}

impl PhysicalFloat {
    pub fn create() -> PhysicalFloat {
        PhysicalFloat::with_mass(1.0)
    }

    pub fn with_mass(mass: f32) -> PhysicalFloat {
        PhysicalFloat {
            previous_value: 0.0,
            value: 0.0,
            previous_speed: 0.0,
            speed: 0.0,
            limit: f32::NAN,
            mass,
            forces: Vec::new(),
        }
    }

    pub fn start_at(mut self, value: f64) -> PhysicalFloat {
        self.value = value as f32;
        self.previous_value = self.value;
        self
    }

    pub fn with_drag(self, drag: f64) -> PhysicalFloat {
        self.add_force(Force::Drag(drag as f32))
    }

    pub fn zeroing(self, g: f64) -> PhysicalFloat {
        self.add_force(Force::zeroing(g as f32))
    }

    pub fn with_limit(mut self, limit: f32) -> PhysicalFloat {
        self.limit = limit;
        self
    }

    pub fn add_force(mut self, f: Force) -> PhysicalFloat {
        self.forces.push(f);
        self
    }

    /// `bump(force)`: an impulse next tick.
    pub fn bump(&mut self, force: f64) {
        self.forces.push(Force::Impulse(force as f32));
    }

    /// `bump(time, force)`.
    pub fn bump_over(&mut self, time: i32, force: f64) {
        self.forces.push(Force::over_time(time, force as f32));
    }

    pub fn tick(&mut self) {
        self.previous_speed = self.speed;
        self.previous_value = self.value;
        let (mass, value, speed) = (self.mass, self.value, self.speed);
        let mut impulse = 0.0;
        for f in &mut self.forces {
            impulse += f.get(mass, value, speed) / mass;
        }
        self.speed += impulse;
        self.forces.retain(|f| !f.finished());
        if self.limit.is_finite() {
            self.speed = Mth::clamp(self.speed, -self.limit, self.limit);
        }
        self.value += self.speed;
    }

    pub fn get_value(&self, partial_ticks: f32) -> f32 {
        Mth::lerp(partial_ticks, self.previous_value, self.value)
    }
}

/// Catnip's `AngleHelper`.
pub struct AngleHelper;

impl AngleHelper {
    pub fn horizontal_angle(facing: Direction) -> f32 {
        if facing.get_axis().is_vertical() {
            return 0.0;
        }
        let angle = facing.to_y_rot();
        if facing.get_axis() == Axis::X {
            -angle
        } else {
            angle
        }
    }

    pub fn vertical_angle(facing: Direction) -> f32 {
        match facing {
            Direction::Up => -90.0,
            Direction::Down => 90.0,
            _ => 0.0,
        }
    }

    pub fn rad(angle: f64) -> f32 {
        if angle == 0.0 {
            0.0
        } else {
            (angle / 180.0 * std::f64::consts::PI) as f32
        }
    }

    pub fn deg(angle: f64) -> f32 {
        if angle == 0.0 {
            0.0
        } else {
            (angle * 180.0 / std::f64::consts::PI) as f32
        }
    }

    pub fn angle_lerp(pct: f64, current: f64, target: f64) -> f32 {
        (current + AngleHelper::get_shortest_angle_diff(current, target) as f64 * pct) as f32
    }

    pub fn get_shortest_angle_diff(current: f64, target: f64) -> f32 {
        let (c, t) = (current % 360.0, target % 360.0);
        (((t - c) % 360.0 + 540.0) % 360.0 - 180.0) as f32
    }

    /// `getShortestAngleDiff(current, target, hint)`: a half turn goes the
    /// hint's way.
    pub fn get_shortest_angle_diff_hint(current: f64, target: f64, hint: f32) -> f32 {
        let diff = AngleHelper::get_shortest_angle_diff(current, target);
        if Mth::equal(diff.abs(), 180.0) && JMath::signum(diff) != JMath::signum(hint) {
            diff + 360.0 * JMath::signum(hint)
        } else {
            diff
        }
    }

    pub fn wrap_angle180(angle: f32) -> f32 {
        (angle + 180.0) % 360.0 - 180.0
    }
}

/// Catnip's `VecHelper`.
pub struct VecHelper;

impl VecHelper {
    pub const CENTER_OF_ORIGIN: Vec3 = Vec3::new(0.5, 0.5, 0.5);

    /// `rotate(vec, degrees, axis)`, with `Mth`'s sine table.
    pub fn rotate(v: Vec3, deg: f64, axis: Axis) -> Vec3 {
        if deg == 0.0 || v == Vec3::ZERO {
            return v;
        }
        let a = (deg / 180.0 * std::f64::consts::PI) as f32;
        let (s, c) = (Mth::sin(a) as f64, Mth::cos(a) as f64);
        let (x, y, z) = (v.x, v.y, v.z);
        match axis {
            Axis::X => Vec3::new(x, y * c - z * s, z * c + y * s),
            Axis::Y => Vec3::new(x * c + z * s, y, z * c - x * s),
            Axis::Z => Vec3::new(x * c - y * s, y * c + x * s, z),
        }
    }

    /// `rotate(vec, xRot, yRot, zRot)`: X, then Y, then Z.
    pub fn rotate_xyz(v: Vec3, x: f64, y: f64, z: f64) -> Vec3 {
        VecHelper::rotate(
            VecHelper::rotate(VecHelper::rotate(v, x, Axis::X), y, Axis::Y),
            z,
            Axis::Z,
        )
    }

    /// `rotate(vec, rotationVec)`.
    pub fn rotate_vec(v: Vec3, r: Vec3) -> Vec3 {
        VecHelper::rotate_xyz(v, r.x, r.y, r.z)
    }

    pub fn rotate_centered(v: Vec3, deg: f64, axis: Axis) -> Vec3 {
        let shift = VecHelper::CENTER_OF_ORIGIN;
        VecHelper::rotate(v - shift, deg, axis) + shift
    }

    pub fn mirror(v: Vec3, mirror: Mirror) -> Vec3 {
        match mirror {
            Mirror::None => v,
            Mirror::LeftRight => Vec3::new(v.x, v.y, -v.z),
            Mirror::FrontBack => Vec3::new(-v.x, v.y, v.z),
        }
    }

    pub fn mirror_centered(v: Vec3, mirror: Mirror) -> Vec3 {
        let shift = VecHelper::CENTER_OF_ORIGIN;
        VecHelper::mirror(v - shift, mirror) + shift
    }

    pub fn look_at(v: Vec3, fwd: Vec3) -> Vec3 {
        let fwd = fwd.normalize();
        let mut up = Vec3::new(0.0, 1.0, 0.0);
        let dot = fwd.dot(up);
        if dot.abs() > 0.999 {
            up = Vec3::new(0.0, 0.0, if dot > 0.0 { 1.0 } else { -1.0 });
        }
        let right = fwd.cross(up).normalize();
        let up = right.cross(fwd).normalize();
        Vec3::new(
            v.x * right.x + v.y * up.x + v.z * fwd.x,
            v.x * right.y + v.y * up.y + v.z * fwd.y,
            v.x * right.z + v.y * up.z + v.z * fwd.z,
        )
    }

    pub fn is_vec_pointing_towards(v: Vec3, d: Direction) -> bool {
        Vec3::at_lower_corner_of(d.get_normal()).dot(v.normalize()) > 0.125
    }

    pub fn get_center_of(p: BlockPos) -> Vec3 {
        Vec3::at_lower_corner_of(p).add_xyz(0.5, 0.5, 0.5)
    }

    pub fn offset_randomly(v: Vec3, r: &mut JavaRandom, radius: f32) -> Vec3 {
        let mut n = || ((r.next_float() - 0.5) * 2.0 * radius) as f64;
        let (x, y, z) = (n(), n(), n());
        Vec3::new(v.x + x, v.y + y, v.z + z)
    }

    /// `axisAlingedPlaneOf(vec)` (Java's spelling).
    pub fn axis_alinged_plane_of(v: Vec3) -> Vec3 {
        let v = v.normalize();
        Vec3::new(1.0 - v.x.abs(), 1.0 - v.y.abs(), 1.0 - v.z.abs())
    }

    pub fn axis_alinged_plane_of_face(face: Direction) -> Vec3 {
        VecHelper::axis_alinged_plane_of(Vec3::at_lower_corner_of(face.get_normal()))
    }

    pub fn read_nbt(list: &[Value]) -> Vec3 {
        crate::mc::nbt::read_vec3(list)
    }

    /// `readNBTCompound(nbt)`: its `V` list.
    pub fn read_nbt_compound(nbt: &Value) -> Vec3 {
        crate::mc::nbt::read_vec3(nbt.get_list("V"))
    }

    pub fn voxel_space(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z).scale(0.0625)
    }

    pub fn get_coordinate(p: BlockPos, axis: Axis) -> i32 {
        p.get(axis)
    }

    pub fn get_coordinate_vec(v: Vec3, axis: Axis) -> f32 {
        v.get(axis) as f32
    }

    pub fn on_same_axis(a: BlockPos, b: BlockPos, axis: Axis) -> bool {
        a == b
            || Axis::VALUES
                .iter()
                .all(|&o| o == axis || a.get(o) == b.get(o))
    }

    pub fn clamp(v: Vec3, max_length: f32) -> Vec3 {
        let m = max_length as f64;
        if v.length_sqr() > m * m {
            v.normalize().scale(m)
        } else {
            v
        }
    }

    pub fn lerp(p: f32, from: Vec3, to: Vec3) -> Vec3 {
        from + (to - from).scale(p as f64)
    }

    /// `slerp(p, from, to)`, as Catnip writes it.
    pub fn slerp(p: f32, from: Vec3, to: Vec3) -> Vec3 {
        let theta = from.dot(to).acos();
        from.scale(Mth::sin(1.0 - p) as f64 * theta)
            .add(to.scale(Mth::sin((theta * p as f64) as f32) as f64))
            .scale((1.0 / Mth::sin(theta as f32)) as f64)
    }

    pub fn clamp_component_wise(v: Vec3, max_length: f32) -> Vec3 {
        let m = max_length as f64;
        Vec3::new(
            Mth::clamp(v.x, -m, m),
            Mth::clamp(v.y, -m, m),
            Mth::clamp(v.z, -m, m),
        )
    }

    pub fn component_min(a: Vec3, b: Vec3) -> Vec3 {
        Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
    }

    pub fn component_max(a: Vec3, b: Vec3) -> Vec3 {
        Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
    }

    pub fn project(v: Vec3, onto: Vec3) -> Vec3 {
        if onto == Vec3::ZERO {
            Vec3::ZERO
        } else {
            onto.scale(v.dot(onto) / onto.length_sqr())
        }
    }

    pub fn intersect_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f64) -> Option<Vec3> {
        if dir == Vec3::ZERO {
            return None;
        }
        let dir = if dir.length_sqr() != 1.0 {
            dir.normalize()
        } else {
            dir
        };
        let diff = origin - center;
        let ld = dir.dot(diff);
        let delta = ld * ld - (diff.length_sqr() - radius * radius);
        if delta < 0.0 {
            return None;
        }
        Some(origin + dir.scale(-ld + delta.sqrt()))
    }

    pub fn bezier(p1: Vec3, p2: Vec3, q1: Vec3, q2: Vec3, t: f32) -> Vec3 {
        let v1 = VecHelper::lerp(t, p1, q1);
        let v2 = VecHelper::lerp(t, q1, q2);
        let v3 = VecHelper::lerp(t, q2, p2);
        let i1 = VecHelper::lerp(t, v1, v2);
        let i2 = VecHelper::lerp(t, v2, v3);
        VecHelper::lerp(t, i1, i2)
    }

    pub fn bezier_derivative(p1: Vec3, p2: Vec3, q1: Vec3, q2: Vec3, t: f32) -> Vec3 {
        let s = |f: f32| f as f64;
        p1.scale(s(-3.0 * t * t + 6.0 * t - 3.0))
            .add(q1.scale(s(9.0 * t * t - 12.0 * t + 3.0)))
            .add(q2.scale(s(-9.0 * t * t + 6.0 * t)))
            .add(p2.scale(s(3.0 * t * t)))
    }

    pub fn intersect_ranged(
        p1: Vec3,
        q1: Vec3,
        p2: Vec3,
        q2: Vec3,
        plane: Axis,
    ) -> Option<[f64; 2]> {
        let pd = p2 - p1;
        let qd = q2 - q1;
        let i = VecHelper::intersect(p1, q1, pd.normalize(), qd.normalize(), plane)?;
        if i[0] < 0.0
            || i[1] < 0.0
            || i[0] * i[0] > pd.length_sqr()
            || i[1] * i[1] > qd.length_sqr()
        {
            None
        } else {
            Some(i)
        }
    }

    pub fn intersect(p1: Vec3, p2: Vec3, r: Vec3, s: Vec3, plane: Axis) -> Option<[f64; 2]> {
        let flat = |v: Vec3| match plane {
            Axis::X => Vec3::new(v.y, 0.0, v.z),
            Axis::Z => Vec3::new(v.x, 0.0, v.y),
            Axis::Y => v,
        };
        let (p1, p2, r, s) = (flat(p1), flat(p2), flat(r), flat(s));
        let qp = p2 - p1;
        let rcs = r.x * s.z - r.z * s.x;
        if Mth::equal(rcs, 0.0) {
            return None;
        }
        let rd = r.scale(1.0 / rcs);
        let sd = s.scale(1.0 / rcs);
        Some([qp.x * sd.z - qp.z * sd.x, qp.x * rd.z - qp.z * rd.x])
    }

    pub fn aligned_distance_to_face(pos: Vec3, block: BlockPos, face: Direction) -> f64 {
        let axis = face.get_axis();
        let edge = block.get(axis)
            + if face.get_axis_direction() == AxisDirection::Positive {
                1
            } else {
                0
            };
        (VecHelper::get_coordinate_vec(pos, axis) as f64 - edge as f64).abs()
    }
}

/// Catnip's `DirectionHelper`.
pub struct DirectionHelper;

impl DirectionHelper {
    /// `rotateAround(dir, axis)`: directions along the axis stay.
    pub fn rotate_around(dir: Direction, axis: Axis) -> Direction {
        match axis {
            Axis::X if dir.get_axis() != Axis::X => DirectionHelper::rotate_x(dir),
            Axis::Y if dir.get_axis() != Axis::Y => dir.get_clock_wise(),
            Axis::Z if dir.get_axis() != Axis::Z => DirectionHelper::rotate_z(dir),
            _ => dir,
        }
    }

    /// `rotateX(dir)`. Panics for east and west, as Java throws.
    pub fn rotate_x(dir: Direction) -> Direction {
        match dir {
            Direction::North => Direction::Down,
            Direction::South => Direction::Up,
            Direction::Up => Direction::North,
            Direction::Down => Direction::South,
            d => panic!("Unable to get X-rotated facing of {d:?}"),
        }
    }

    /// `rotateZ(dir)`. Panics for north and south.
    pub fn rotate_z(dir: Direction) -> Direction {
        match dir {
            Direction::East => Direction::Down,
            Direction::West => Direction::Up,
            Direction::Up => Direction::East,
            Direction::Down => Direction::West,
            d => panic!("Unable to get Z-rotated facing of {d:?}"),
        }
    }

    pub fn get_positive_perpendicular(horizontal: Axis) -> Direction {
        if horizontal == Axis::X {
            Direction::South
        } else {
            Direction::East
        }
    }
}

/// Catnip's `Pointing`, in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pointing {
    Up,
    Left,
    Down,
    Right,
}

impl Pointing {
    pub const VALUES: [Pointing; 4] = [
        Pointing::Up,
        Pointing::Left,
        Pointing::Down,
        Pointing::Right,
    ];

    pub fn by_name(s: &str) -> Option<Pointing> {
        Pointing::VALUES
            .into_iter()
            .find(|p| p.get_serialized_name() == s)
    }

    pub fn get_serialized_name(self) -> &'static str {
        match self {
            Pointing::Up => "up",
            Pointing::Left => "left",
            Pointing::Down => "down",
            Pointing::Right => "right",
        }
    }

    pub fn get_x_rotation(self) -> i32 {
        match self {
            Pointing::Up => 0,
            Pointing::Left => 270,
            Pointing::Down => 180,
            Pointing::Right => 90,
        }
    }

    pub fn get_combined_direction(self, d: Direction) -> Direction {
        let axis = d.get_axis();
        let mut top = if axis == Axis::Y {
            Direction::South
        } else {
            Direction::Up
        };
        let ordinal = self as i32;
        let n = if d.get_axis_direction() == AxisDirection::Negative {
            4 - ordinal
        } else {
            ordinal
        };
        for _ in 0..n {
            top = top.get_clock_wise_about(axis);
        }
        top
    }
}

/// Catnip's `Iterate`.
pub struct Iterate;

impl Iterate {
    pub const TRUE_AND_FALSE: [bool; 2] = [true, false];
    pub const FALSE_AND_TRUE: [bool; 2] = [false, true];
    pub const ZERO_AND_ONE: [i32; 2] = [0, 1];
    pub const POSITIVE_AND_NEGATIVE: [i32; 2] = [1, -1];
    pub const DIRECTIONS: [Direction; 6] = Direction::VALUES;
    /// `from2DDataValue` order: south, west, north, east.
    pub const HORIZONTAL_DIRECTIONS: [Direction; 4] = Direction::HORIZONTALS_2D;
    pub const AXES: [Axis; 3] = Axis::VALUES;

    pub fn directions_in_axis(axis: Axis) -> [Direction; 2] {
        match axis {
            Axis::X => [Direction::East, Direction::West],
            Axis::Y => [Direction::Up, Direction::Down],
            Axis::Z => [Direction::South, Direction::North],
        }
    }

    pub fn here_and_below(p: BlockPos) -> [BlockPos; 2] {
        [p, p.below()]
    }

    pub fn here_below_and_above(p: BlockPos) -> [BlockPos; 3] {
        [p, p.below(), p.above()]
    }

    /// `cycleValue(list, current)`: the next one, wrapping. Panics when
    /// `current` isn't in it, as Java throws.
    pub fn cycle_value<T: PartialEq + Clone>(list: &[T], current: &T) -> T {
        let i = list
            .iter()
            .position(|v| v == current)
            .expect("Current value not found in list");
        list[(i + 1) % list.len()].clone()
    }
}

/// Catnip's `AnimationFunctions`.
pub struct AnimationFunctions;

impl AnimationFunctions {
    pub fn ease_out(t: f32) -> f32 {
        Mth::sin(std::f32::consts::FRAC_PI_2 * t)
    }

    pub fn ease_in_out(t: f32) -> f32 {
        (Mth::sin(std::f32::consts::FRAC_PI_2 * t) as f64).powf(2.0) as f32
    }

    pub fn ease_in(t: f32) -> f32 {
        (t as f64).powf(1.7) as f32
    }
}

/// Catnip's `AnimationTickHolder`, from a view's game time. Java counts
/// client ticks since joining; game time differs only by a phase.
pub struct AnimationTickHolder;

impl AnimationTickHolder {
    /// `getTicks()`: wraps at 1,728,000 as Java's.
    pub fn get_ticks(time: i64) -> i32 {
        (time % 1_728_000) as i32
    }

    /// `getRenderTime()`: ticks plus the partial tick.
    pub fn get_render_time(time: i64, partial_ticks: f32) -> f32 {
        AnimationTickHolder::get_ticks(time) as f32 + partial_ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lerped_float_chases_like_catnips() {
        let mut f = LerpedFloat::linear()
            .start_with_value(0.0)
            .chase(10.0, 0.5, Chaser::EXP);
        f.tick_chaser();
        assert_eq!(f.value, 5.0);
        assert_eq!(f.get_value(0.5), 2.5);
        f.tick_chaser();
        assert_eq!(f.value, 7.5);
        let mut l = LerpedFloat::linear()
            .start_with_value(0.0)
            .chase(1.0, 0.4, Chaser::LINEAR);
        for _ in 0..3 {
            l.tick_chaser();
        }
        assert_eq!(l.value, 1.0);
        assert!(!l.settled());
        l.tick_chaser();
        assert!(l.settled());
    }

    #[test]
    fn an_angular_float_goes_the_short_way() {
        let mut f = LerpedFloat::angular().start_with_value(350.0);
        f.chase(10.0, 1.0, Chaser::EXP);
        assert_eq!(f.chase_target, 370.0);
        f.tick_chaser();
        assert_eq!(f.value, 370.0);
        assert_eq!(AngleHelper::get_shortest_angle_diff(350.0, 10.0), 20.0);
        assert_eq!(AngleHelper::angle_lerp(0.5, 350.0, 10.0), 360.0);
    }

    #[test]
    fn read_nbt_keeps_the_value_unless_forced() {
        let mut f = LerpedFloat::linear().start_with_value(3.0);
        f.read_nbt(
            &serde_json::json!({"Value": 9.0, "Target": 5.0, "Speed": 0.1}),
            true,
        );
        assert_eq!((f.value, f.chase_target), (3.0, 5.0));
        f.read_nbt(
            &serde_json::json!({"Value": 9.0, "Target": 5.0, "Force": 1}),
            true,
        );
        assert_eq!(f.value, 9.0);
    }

    #[test]
    fn a_physical_float_follows_its_forces() {
        let mut f = PhysicalFloat::create().with_drag(0.5);
        f.bump(1.0);
        f.tick();
        assert_eq!((f.speed, f.value), (1.0, 1.0));
        f.tick();
        assert_eq!((f.speed, f.value), (0.5, 1.5));
    }

    #[test]
    fn helpers_match_catnip() {
        assert_eq!(AngleHelper::horizontal_angle(Direction::East), -270.0);
        assert_eq!(AngleHelper::horizontal_angle(Direction::North), 180.0);
        // Right on a north face: one turn of up about Z (4 - 3).
        assert_eq!(
            Pointing::Right.get_combined_direction(Direction::North),
            Direction::East
        );
        assert_eq!(
            DirectionHelper::rotate_around(Direction::North, Axis::X),
            Direction::Down
        );
        let v = VecHelper::rotate_centered(Vec3::new(1.0, 0.5, 0.5), 90.0, Axis::Y);
        assert!(
            (v.x - 0.5).abs() < 1e-6 && (v.z - 0.0).abs() < 1e-6,
            "{v:?}"
        );
        assert_eq!(Iterate::cycle_value(&[1, 2, 3], &3), 1);
    }
}
