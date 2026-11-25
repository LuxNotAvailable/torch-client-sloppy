//! Java's renderer transforms: JOML's `Quaternionf`, `PoseStack`,
//! Flywheel's `TransformStack` methods (`center()`, `rotateYDegrees`, ...)
//! and Catnip's `SuperByteBuffer`/`CachedBuffers`, ending in a [`Part`].
//!
//! Replay the Java calls in order on a [`PoseStack`] (or a
//! [`SuperByteBuffer`]) and take the result with `part`/`render_into`.
//! Units are blocks, as in Java; the [`Transform`] it makes is in
//! sixteenths.

use crate::{Geometry, Part, Scroll, Transform};

use super::direction::{Axis, Direction};
use super::vec::{BlockPos, Vec3};

/// A number Java would widen or narrow to `float` for a transform call.
pub trait Real: Copy {
    fn f32(self) -> f32;
}

impl Real for f32 {
    fn f32(self) -> f32 {
        self
    }
}

impl Real for f64 {
    fn f32(self) -> f32 {
        self as f32
    }
}

impl Real for i32 {
    fn f32(self) -> f32 {
        self as f32
    }
}

/// What a rotation call can turn about: a `Direction` (its normal), a
/// `Direction.Axis` (positive), a [`MathAxis`] or a vector.
pub trait RotAxis {
    fn axis_vec(self) -> [f32; 3];
}

impl RotAxis for Direction {
    fn axis_vec(self) -> [f32; 3] {
        self.step()
    }
}

impl RotAxis for Axis {
    fn axis_vec(self) -> [f32; 3] {
        self.unit()
    }
}

impl RotAxis for MathAxis {
    fn axis_vec(self) -> [f32; 3] {
        self.0
    }
}

impl RotAxis for [f32; 3] {
    fn axis_vec(self) -> [f32; 3] {
        self
    }
}

/// JOML's `Quaternionf`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quaternion {
    fn default() -> Quaternion {
        Quaternion::IDENTITY
    }
}

impl Quaternion {
    pub const IDENTITY: Quaternion = Quaternion {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Quaternion {
        Quaternion { x, y, z, w }
    }

    /// `setAngleAxis(radians, x, y, z)`: the axis as given (JOML doesn't
    /// normalise it).
    pub fn angle_axis(radians: f32, axis: impl RotAxis) -> Quaternion {
        let [x, y, z] = axis.axis_vec();
        let (s, c) = (radians * 0.5).sin_cos();
        Quaternion::new(x * s, y * s, z * s, c)
    }

    pub fn rotation_x(r: f32) -> Quaternion {
        Quaternion::angle_axis(r, [1.0, 0.0, 0.0])
    }

    pub fn rotation_y(r: f32) -> Quaternion {
        Quaternion::angle_axis(r, [0.0, 1.0, 0.0])
    }

    pub fn rotation_z(r: f32) -> Quaternion {
        Quaternion::angle_axis(r, [0.0, 0.0, 1.0])
    }

    /// `rotationXYZ(x, y, z)`: `rotationX(x).mul(rotationY(y)).mul(rotationZ(z))`.
    pub fn rotation_xyz(x: f32, y: f32, z: f32) -> Quaternion {
        Quaternion::rotation_x(x)
            .mul(Quaternion::rotation_y(y))
            .mul(Quaternion::rotation_z(z))
    }

    /// `rotationYXZ(y, x, z)`.
    pub fn rotation_yxz(y: f32, x: f32, z: f32) -> Quaternion {
        Quaternion::rotation_y(y)
            .mul(Quaternion::rotation_x(x))
            .mul(Quaternion::rotation_z(z))
    }

    /// `rotationZYX(z, y, x)`.
    pub fn rotation_zyx(z: f32, y: f32, x: f32) -> Quaternion {
        Quaternion::rotation_z(z)
            .mul(Quaternion::rotation_y(y))
            .mul(Quaternion::rotation_x(x))
    }

    /// `rotationTo(from, to)`: the shortest turn from one vector to another.
    pub fn rotation_to(from: [f32; 3], to: [f32; 3]) -> Quaternion {
        let n = |v: [f32; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v.map(|c| c / l)
        };
        let (a, b) = (n(from), n(to));
        let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        if dot < -1.0 + 1e-6 {
            // Opposite: half a turn about any perpendicular.
            let p = if a[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let c = [
                a[1] * p[2] - a[2] * p[1],
                a[2] * p[0] - a[0] * p[2],
                a[0] * p[1] - a[1] * p[0],
            ];
            return Quaternion::angle_axis(std::f32::consts::PI, n(c));
        }
        let c = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        Quaternion::new(c[0], c[1], c[2], 1.0 + dot).normalize()
    }

    pub fn normalize(self) -> Quaternion {
        let l = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        Quaternion::new(self.x / l, self.y / l, self.z / l, self.w / l)
    }

    /// `q.mul(r)`: `q * r` (r turns the vertex first).
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, r: Quaternion) -> Quaternion {
        let q = self;
        Quaternion::new(
            q.w * r.x + q.x * r.w + q.y * r.z - q.z * r.y,
            q.w * r.y - q.x * r.z + q.y * r.w + q.z * r.x,
            q.w * r.z + q.x * r.y - q.y * r.x + q.z * r.w,
            q.w * r.w - q.x * r.x - q.y * r.y - q.z * r.z,
        )
    }

    pub fn conjugate(self) -> Quaternion {
        Quaternion::new(-self.x, -self.y, -self.z, self.w)
    }

    /// `q.rotateX(r)` and friends: `q * rotationX(r)`.
    pub fn rotate_x(self, r: f32) -> Quaternion {
        self.mul(Quaternion::rotation_x(r))
    }

    pub fn rotate_y(self, r: f32) -> Quaternion {
        self.mul(Quaternion::rotation_y(r))
    }

    pub fn rotate_z(self, r: f32) -> Quaternion {
        self.mul(Quaternion::rotation_z(r))
    }

    /// As a 3x3 rotation matrix (rows).
    pub fn matrix(self) -> [[f32; 3]; 3] {
        let Quaternion { x, y, z, w } = self;
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
            ],
            [
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
            ],
            [
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }

    /// `transform(vector)`: the vector turned.
    pub fn transform(self, v: [f32; 3]) -> [f32; 3] {
        let m = self.matrix();
        std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
    }
}

/// `com.mojang.math.Axis`: `Axis.YP.rotationDegrees(a)` is
/// `MathAxis::YP.rotation_degrees(a)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MathAxis(pub [f32; 3]);

impl MathAxis {
    pub const XP: MathAxis = MathAxis([1.0, 0.0, 0.0]);
    pub const XN: MathAxis = MathAxis([-1.0, 0.0, 0.0]);
    pub const YP: MathAxis = MathAxis([0.0, 1.0, 0.0]);
    pub const YN: MathAxis = MathAxis([0.0, -1.0, 0.0]);
    pub const ZP: MathAxis = MathAxis([0.0, 0.0, 1.0]);
    pub const ZN: MathAxis = MathAxis([0.0, 0.0, -1.0]);

    /// `Axis.of(vector)`.
    pub fn of(v: [f32; 3]) -> MathAxis {
        MathAxis(v)
    }

    pub fn rotation(self, radians: f32) -> Quaternion {
        Quaternion::angle_axis(radians, self)
    }

    pub fn rotation_degrees(self, degrees: f32) -> Quaternion {
        self.rotation(degrees * (std::f32::consts::PI / 180.0))
    }
}

type M3 = [[f32; 3]; 3];

const I3: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mul3(a: M3, b: M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// `PoseStack.Pose`: an affine transform (a 3x3 part and a translation),
/// in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub m: M3,
    pub t: [f32; 3],
}

impl Default for Pose {
    fn default() -> Pose {
        Pose { m: I3, t: [0.0; 3] }
    }
}

impl Pose {
    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        for i in 0..3 {
            self.t[i] += self.m[i][0] * x + self.m[i][1] * y + self.m[i][2] * z;
        }
    }

    pub fn rotate(&mut self, q: Quaternion) {
        self.m = mul3(self.m, q.matrix());
    }

    pub fn scale(&mut self, x: f32, y: f32, z: f32) {
        for row in &mut self.m {
            row[0] *= x;
            row[1] *= y;
            row[2] *= z;
        }
    }

    /// `pose.mul(other)`: `other` acts on the vertex first.
    pub fn mul(&self, o: &Pose) -> Pose {
        let mut t = self.t;
        for (i, ti) in t.iter_mut().enumerate() {
            *ti += (0..3).map(|k| self.m[i][k] * o.t[k]).sum::<f32>();
        }
        Pose {
            m: mul3(self.m, o.m),
            t,
        }
    }

    /// Where it puts a point, in blocks.
    pub fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|i| {
            self.m[i][0] * p[0] + self.m[i][1] * p[1] + self.m[i][2] * p[2] + self.t[i]
        })
    }

    /// As the API's [`Transform`], about the block's corner (pivot 0).
    /// Exact when the 3x3 part is a rotation times a per-axis scale (scales
    /// called after rotations, or uniform ones); a skew otherwise comes out
    /// as its nearest rotation.
    pub fn transform(&self) -> Transform {
        let mut s: [f32; 3] = std::array::from_fn(|j| {
            (0..3)
                .map(|i| self.m[i][j] * self.m[i][j])
                .sum::<f32>()
                .sqrt()
        });
        let det = self.m[0][0] * (self.m[1][1] * self.m[2][2] - self.m[1][2] * self.m[2][1])
            - self.m[0][1] * (self.m[1][0] * self.m[2][2] - self.m[1][2] * self.m[2][0])
            + self.m[0][2] * (self.m[1][0] * self.m[2][1] - self.m[1][1] * self.m[2][0]);
        if det < 0.0 {
            s[0] = -s[0];
        }
        let r: M3 = std::array::from_fn(|i| {
            std::array::from_fn(|j| self.m[i][j] / if s[j].abs() < 1e-9 { 1.0 } else { s[j] })
        });
        Transform {
            pivot: [0.0; 3],
            rotate: euler(r),
            translate: self.t.map(|v| v * 16.0),
            scale: s,
        }
    }
}

/// Euler angles of a rotation as [`Transform::rotate`] takes them
/// (m = Rz Ry Rx), in degrees; ones under 1e-3 tidied to 0.
pub fn euler(m: M3) -> [f32; 3] {
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

/// Flywheel's `TransformStack` (`Translate`, `Rotate`, `Scale`, `Affine`)
/// on anything holding a [`Pose`]. Java's overloads by argument type are
/// one method each here (`rotate_by(radians, axis)` takes a `Direction`,
/// an `Axis`, a [`MathAxis`] or a vector).
pub trait TransformStack {
    fn pose_mut(&mut self) -> &mut Pose;

    /// `translate(x, y, z)` (float, double or int).
    fn translate(&mut self, x: impl Real, y: impl Real, z: impl Real) -> &mut Self {
        self.pose_mut().translate(x.f32(), y.f32(), z.f32());
        self
    }

    /// `translate(Vec3)`.
    fn translate_vec(&mut self, v: Vec3) -> &mut Self {
        self.translate(v.x, v.y, v.z)
    }

    /// `translate(Vec3i)`.
    fn translate_pos(&mut self, p: BlockPos) -> &mut Self {
        self.translate(p.x, p.y, p.z)
    }

    /// `translate(float v)`: the same on all three.
    fn translate_all(&mut self, v: impl Real) -> &mut Self {
        self.translate(v, v, v)
    }

    fn translate_x(&mut self, v: impl Real) -> &mut Self {
        self.translate(v, 0.0, 0.0)
    }

    fn translate_y(&mut self, v: impl Real) -> &mut Self {
        self.translate(0.0, v, 0.0)
    }

    fn translate_z(&mut self, v: impl Real) -> &mut Self {
        self.translate(0.0, 0.0, v)
    }

    fn translate_back(&mut self, x: impl Real, y: impl Real, z: impl Real) -> &mut Self {
        self.translate(-x.f32(), -y.f32(), -z.f32())
    }

    fn translate_back_vec(&mut self, v: Vec3) -> &mut Self {
        self.translate(-v.x, -v.y, -v.z)
    }

    /// `center()`: `translate(0.5)`.
    fn center(&mut self) -> &mut Self {
        self.translate(0.5, 0.5, 0.5)
    }

    /// `uncenter()`: `translate(-0.5)`.
    fn uncenter(&mut self) -> &mut Self {
        self.translate(-0.5, -0.5, -0.5)
    }

    /// `nudge(seed)`: Flywheel's tiny per-seed offset against z-fighting.
    fn nudge(&mut self, seed: i32) -> &mut Self {
        let mut bits = (seed as i64).wrapping_mul(31).wrapping_mul(493_286_711);
        bits = bits
            .wrapping_mul(bits)
            .wrapping_mul(4_392_167_121)
            .wrapping_add(bits.wrapping_mul(98_761));
        let n = |shift: u32| ((((bits >> shift) & 7) as f32 + 0.5) / 8.0 - 0.5) * 0.004;
        self.translate(n(16), n(20), n(24))
    }

    /// `rotate(Quaternionf)`, and `PoseStack.mulPose`.
    fn rotate(&mut self, q: Quaternion) -> &mut Self {
        self.pose_mut().rotate(q);
        self
    }

    fn mul_pose(&mut self, q: Quaternion) -> &mut Self {
        self.rotate(q)
    }

    /// `rotate(radians, axis)`: no-op for 0, as Java's.
    fn rotate_by(&mut self, radians: f32, axis: impl RotAxis) -> &mut Self {
        if radians == 0.0 {
            return self;
        }
        self.rotate(Quaternion::angle_axis(radians, axis))
    }

    /// `rotateDegrees(degrees, axis)`.
    fn rotate_degrees(&mut self, degrees: f32, axis: impl RotAxis) -> &mut Self {
        self.rotate_by((std::f32::consts::PI / 180.0) * degrees, axis)
    }

    fn rotate_x(&mut self, radians: f32) -> &mut Self {
        self.rotate_by(radians, MathAxis::XP)
    }

    fn rotate_y(&mut self, radians: f32) -> &mut Self {
        self.rotate_by(radians, MathAxis::YP)
    }

    fn rotate_z(&mut self, radians: f32) -> &mut Self {
        self.rotate_by(radians, MathAxis::ZP)
    }

    fn rotate_x_degrees(&mut self, degrees: f32) -> &mut Self {
        self.rotate_degrees(degrees, MathAxis::XP)
    }

    fn rotate_y_degrees(&mut self, degrees: f32) -> &mut Self {
        self.rotate_degrees(degrees, MathAxis::YP)
    }

    fn rotate_z_degrees(&mut self, degrees: f32) -> &mut Self {
        self.rotate_degrees(degrees, MathAxis::ZP)
    }

    /// Flywheel's `rotateToFace(facing)` (north stays; not
    /// `CachedBuffers.rotateToFace`, which is [`CachedBuffers::rotate_to_face`]).
    fn rotate_to_face(&mut self, facing: Direction) -> &mut Self {
        match facing {
            Direction::Down => self.rotate_x_degrees(-90.0),
            Direction::Up => self.rotate_x_degrees(90.0),
            Direction::North => self,
            Direction::South => self.rotate_y_degrees(180.0),
            Direction::West => self.rotate_y_degrees(90.0),
            Direction::East => self.rotate_y_degrees(270.0),
        }
    }

    /// `rotateTo(from, to)`.
    fn rotate_to(&mut self, from: impl RotAxis, to: impl RotAxis) -> &mut Self {
        self.rotate(Quaternion::rotation_to(from.axis_vec(), to.axis_vec()))
    }

    /// `rotateAround(q, x, y, z)`.
    fn rotate_around(&mut self, q: Quaternion, x: f32, y: f32, z: f32) -> &mut Self {
        self.translate(x, y, z).rotate(q).translate_back(x, y, z)
    }

    /// `rotateCentered(Quaternionf)`.
    fn rotate_centered(&mut self, q: Quaternion) -> &mut Self {
        self.rotate_around(q, 0.5, 0.5, 0.5)
    }

    /// `rotateCentered(radians, axis)`.
    fn rotate_centered_by(&mut self, radians: f32, axis: impl RotAxis) -> &mut Self {
        if radians == 0.0 {
            return self;
        }
        self.rotate_centered(Quaternion::angle_axis(radians, axis))
    }

    fn rotate_centered_degrees(&mut self, degrees: f32, axis: impl RotAxis) -> &mut Self {
        self.rotate_centered_by((std::f32::consts::PI / 180.0) * degrees, axis)
    }

    fn rotate_x_centered(&mut self, radians: f32) -> &mut Self {
        self.rotate_centered_by(radians, MathAxis::XP)
    }

    fn rotate_y_centered(&mut self, radians: f32) -> &mut Self {
        self.rotate_centered_by(radians, MathAxis::YP)
    }

    fn rotate_z_centered(&mut self, radians: f32) -> &mut Self {
        self.rotate_centered_by(radians, MathAxis::ZP)
    }

    fn rotate_x_centered_degrees(&mut self, degrees: f32) -> &mut Self {
        self.rotate_centered_degrees(degrees, MathAxis::XP)
    }

    fn rotate_y_centered_degrees(&mut self, degrees: f32) -> &mut Self {
        self.rotate_centered_degrees(degrees, MathAxis::YP)
    }

    fn rotate_z_centered_degrees(&mut self, degrees: f32) -> &mut Self {
        self.rotate_centered_degrees(degrees, MathAxis::ZP)
    }

    /// `scale(factor)`.
    fn scale(&mut self, f: impl Real) -> &mut Self {
        let f = f.f32();
        self.pose_mut().scale(f, f, f);
        self
    }

    /// `scale(x, y, z)` (`PoseStack.scale`).
    fn scale_xyz(&mut self, x: impl Real, y: impl Real, z: impl Real) -> &mut Self {
        self.pose_mut().scale(x.f32(), y.f32(), z.f32());
        self
    }

    /// `transform(pose)` / `mulPose(matrix)`: `p` acts on the vertex first.
    fn transform_by(&mut self, p: &Pose) -> &mut Self {
        let next = self.pose_mut().mul(p);
        *self.pose_mut() = next;
        self
    }
}

/// `PoseStack`, starting at the block's corner (what a block entity
/// renderer's `ms` holds).
#[derive(Clone, Debug)]
pub struct PoseStack {
    stack: Vec<Pose>,
}

impl Default for PoseStack {
    fn default() -> PoseStack {
        PoseStack {
            stack: vec![Pose::default()],
        }
    }
}

impl PoseStack {
    pub fn new() -> PoseStack {
        PoseStack::default()
    }

    pub fn push_pose(&mut self) {
        let top = *self.last();
        self.stack.push(top);
    }

    /// `popPose()`; the bottom pose stays.
    pub fn pop_pose(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }

    pub fn last(&self) -> &Pose {
        self.stack.last().expect("never empty")
    }

    /// `ms.setIdentity()`.
    pub fn set_identity(&mut self) {
        *self.pose_mut() = Pose::default();
    }

    /// A model part drawn with the current pose (`renderSingleBlock`,
    /// `renderModel` of a partial).
    pub fn model(&self, model: impl Into<String>) -> Part {
        Part::model(model).with_transform(self.last().transform())
    }

    /// An item drawn with the current pose, as
    /// `itemRenderer.renderStatic(stack, ItemDisplayContext.FIXED, ...)`.
    pub fn item(&self, item: impl Into<String>) -> Part {
        Part::item(item).with_transform(self.last().transform())
    }
}

impl TransformStack for PoseStack {
    fn pose_mut(&mut self) -> &mut Pose {
        self.stack.last_mut().expect("never empty")
    }
}

/// Catnip's `SuperByteBuffer`: a model with its own transforms, drawn into
/// a renderer's pose with [`render_into`](SuperByteBuffer::render_into).
/// Light, overlay and colour calls are accepted and dropped: the client
/// lights rigs itself.
#[derive(Clone, Debug)]
pub struct SuperByteBuffer {
    geometry: Geometry,
    /// The model transform baked into the buffer (`partialFacing`'s turn).
    base: Pose,
    pose: Pose,
    scroll: Option<Scroll>,
}

impl TransformStack for SuperByteBuffer {
    fn pose_mut(&mut self) -> &mut Pose {
        &mut self.pose
    }
}

impl SuperByteBuffer {
    pub fn new(geometry: Geometry) -> SuperByteBuffer {
        SuperByteBuffer {
            geometry,
            base: Pose::default(),
            pose: Pose::default(),
            scroll: None,
        }
    }

    /// `rotate(Axis, radians)`: the buffer's own overload, axis first.
    pub fn rotate_axis(&mut self, axis: Axis, radians: f32) -> &mut Self {
        self.rotate_by(radians, axis)
    }

    pub fn light(&mut self, _light: i32) -> &mut Self {
        self
    }

    pub fn overlay(&mut self, _overlay: i32) -> &mut Self {
        self
    }

    pub fn color(&mut self, _argb: i32) -> &mut Self {
        self
    }

    pub fn disable_diffuse(&mut self) -> &mut Self {
        self
    }

    /// `shiftUVScrolling(entry, ...)` as a [`Scroll`] (the host moves it).
    pub fn scroll(&mut self, scroll: Scroll) -> &mut Self {
        self.scroll = Some(scroll);
        self
    }

    /// `renderInto(ms, buffer)`: the part, placed by `ms`'s pose. Then the
    /// buffer's own transforms and scroll reset, as Java's `reset()` does,
    /// so a buffer drawn again (once per face) starts from its model.
    pub fn render_into(&mut self, ms: &PoseStack) -> Part {
        let part = self.part_at(ms.last());
        self.pose = Pose::default();
        self.scroll = None;
        part
    }

    /// As drawn with the block's own pose (`renderInto` with a fresh `ms`).
    pub fn part(&self) -> Part {
        self.part_at(&Pose::default())
    }

    fn part_at(&self, at: &Pose) -> Part {
        let pose = at.mul(&self.pose).mul(&self.base);
        Part {
            geometry: self.geometry.clone(),
            transform: pose.transform(),
            animation: crate::Animation::None,
            scroll: self.scroll.clone(),
        }
    }
}

/// An empty model: what a buffer field holds before it's set.
impl Default for SuperByteBuffer {
    fn default() -> SuperByteBuffer {
        CachedBuffers::partial("")
    }
}

/// Catnip's `CachedBuffers`.
pub struct CachedBuffers;

impl CachedBuffers {
    /// `partial(partialModel, state)`: a model by id
    /// (`create:block/shaft`).
    pub fn partial(model: impl Into<String>) -> SuperByteBuffer {
        SuperByteBuffer::new(Geometry::Model {
            model: model.into(),
            x: 0,
            y: 0,
            uvlock: false,
        })
    }

    /// `partialFacing(partial, state, facing)`.
    pub fn partial_facing(model: impl Into<String>, facing: Direction) -> SuperByteBuffer {
        let mut b = CachedBuffers::partial(model);
        b.base = CachedBuffers::rotate_to_face(facing);
        b
    }

    /// `partialFacingVertical(partial, state, facing)`.
    pub fn partial_facing_vertical(model: impl Into<String>, facing: Direction) -> SuperByteBuffer {
        let mut b = CachedBuffers::partial(model);
        b.base = CachedBuffers::rotate_to_face_vertical(facing);
        b
    }

    /// `partialDirectional(partial, state, dir, modelTransform)`.
    pub fn partial_directional(model: impl Into<String>, transform: Pose) -> SuperByteBuffer {
        let mut b = CachedBuffers::partial(model);
        b.base = transform;
        b
    }

    /// `block(state)`: a whole block state's model.
    pub fn block(
        block: impl Into<String>,
        props: std::collections::BTreeMap<String, String>,
    ) -> SuperByteBuffer {
        SuperByteBuffer::new(Geometry::Block {
            block: block.into(),
            props,
        })
    }

    /// `CachedBuffers.rotateToFace(facing)`: centre, Y by
    /// `horizontalAngle`, X by `verticalAngle`, uncentre.
    pub fn rotate_to_face(facing: Direction) -> Pose {
        use crate::catnip::AngleHelper;
        let mut ms = PoseStack::new();
        ms.center()
            .rotate_y_degrees(AngleHelper::horizontal_angle(facing))
            .rotate_x_degrees(AngleHelper::vertical_angle(facing))
            .uncenter();
        *ms.last()
    }

    /// `CachedBuffers.rotateToFaceVertical(facing)`.
    pub fn rotate_to_face_vertical(facing: Direction) -> Pose {
        use crate::catnip::AngleHelper;
        let mut ms = PoseStack::new();
        ms.center()
            .rotate_y_degrees(AngleHelper::horizontal_angle(facing))
            .rotate_x_degrees(AngleHelper::vertical_angle(facing) + 90.0)
            .uncenter();
        *ms.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the API puts a point (sixteenths) with a part's transform.
    fn api(t: &Transform, p: [f32; 3]) -> [f32; 3] {
        let r = Quaternion::rotation_z(t.rotate[2].to_radians())
            .mul(Quaternion::rotation_y(t.rotate[1].to_radians()))
            .mul(Quaternion::rotation_x(t.rotate[0].to_radians()));
        let s = [
            (p[0] - t.pivot[0]) * t.scale[0],
            (p[1] - t.pivot[1]) * t.scale[1],
            (p[2] - t.pivot[2]) * t.scale[2],
        ];
        let q = r.transform(s);
        std::array::from_fn(|i| q[i] + t.pivot[i] + t.translate[i])
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3)
    }

    #[test]
    fn a_pose_stack_becomes_a_transform_that_moves_points_alike() {
        let mut ms = PoseStack::new();
        ms.translate(0.5, 0.9375, 0.5)
            .rotate_y_degrees(37.0)
            .translate(0.0, -0.09375, 0.0)
            .mul_pose(MathAxis::XP.rotation_degrees(90.0))
            .scale_xyz(0.5, 0.25, 0.5);
        let t = ms.last().transform();
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.3, -0.2, 0.7]] {
            let a = ms.last().apply(p).map(|v| v * 16.0);
            let b = api(&t, p.map(|v| v * 16.0));
            assert!(close(a, b), "{a:?} {b:?}");
        }
    }

    #[test]
    fn push_and_pop_restore_the_pose() {
        let mut ms = PoseStack::new();
        ms.translate(1, 0, 0);
        ms.push_pose();
        ms.rotate_x_degrees(45.0).scale(2.0);
        ms.pop_pose();
        assert_eq!(ms.last().apply([0.0; 3]), [1.0, 0.0, 0.0]);
        assert_eq!(ms.last().m, I3);
    }

    #[test]
    fn a_facing_partial_turns_about_the_centre() {
        // horizontalAngle(east) is -270 (+90 about Y): south (+Z) turns
        // to east (+X).
        let b = CachedBuffers::partial_facing("m", Direction::East);
        let p = b.part();
        let tip = api(&p.transform, [8.0, 8.0, 16.0]);
        assert!(close(tip, [16.0, 8.0, 8.0]), "{tip:?}");
        let centre = api(&p.transform, [8.0, 8.0, 8.0]);
        assert!(close(centre, [8.0, 8.0, 8.0]), "{centre:?}");
    }

    #[test]
    fn a_buffer_resets_after_it_draws() {
        let ms = PoseStack::new();
        let mut b = CachedBuffers::partial_facing("m", Direction::East);
        let turned = b
            .rotate_centered_degrees(90.0, Direction::Up)
            .render_into(&ms);
        let again = b.render_into(&ms);
        assert_ne!(turned.transform, again.transform);
        // The facing turn is the model's and stays.
        assert_eq!(
            again.transform,
            CachedBuffers::partial_facing("m", Direction::East)
                .part()
                .transform
        );
    }

    #[test]
    fn rotate_centered_keeps_the_centre() {
        let mut b = CachedBuffers::partial("m");
        b.rotate_centered_degrees(90.0, Direction::Up);
        let t = b.part().transform;
        assert!(close(api(&t, [8.0; 3]), [8.0; 3]));
        // Y +90: +X turns to -Z.
        assert!(close(api(&t, [16.0, 8.0, 8.0]), [8.0, 8.0, 0.0]));
    }

    #[test]
    fn direction_rotations_turn_up_onto_the_direction() {
        for d in Direction::VALUES {
            let v = d.get_rotation().transform([0.0, 1.0, 0.0]);
            assert!(close(v, d.step()), "{d:?} {v:?}");
        }
    }
}
