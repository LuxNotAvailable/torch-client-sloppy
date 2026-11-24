//! Java's `PoseStack`, enough to replay a renderer's `translate`/`rotate`/
//! `scale` calls and hand the result to the API as one [`Transform`].

use torch_mod_api::Transform;

use crate::dir::Axis;

/// An affine pose: a 3x3 linear part and a translation, in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    m: [[f32; 3]; 3],
    t: [f32; 3],
}

impl Default for Pose {
    fn default() -> Pose {
        Pose {
            m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            t: [0.0; 3],
        }
    }
}

fn mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// A right-handed rotation about `axis` (`Axis.XP.rotationDegrees`).
pub fn rotation(axis: Axis, degrees: f32) -> [[f32; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    match axis {
        Axis::X => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        Axis::Y => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        Axis::Z => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    }
}

/// Euler angles of a rotation as [`Transform::rotate`] takes them
/// (m = Rz Ry Rx), in degrees; ones under 1e-3 tidied to 0.
pub fn euler(m: [[f32; 3]; 3]) -> [f32; 3] {
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

impl Pose {
    pub fn translate(mut self, x: f32, y: f32, z: f32) -> Pose {
        for (i, row) in self.m.iter().enumerate() {
            self.t[i] += row[0] * x + row[1] * y + row[2] * z;
        }
        self
    }

    pub fn rotate(mut self, axis: Axis, degrees: f32) -> Pose {
        if degrees != 0.0 {
            self.m = mul(self.m, rotation(axis, degrees));
        }
        self
    }

    pub fn scale(mut self, s: f32) -> Pose {
        for row in &mut self.m {
            for v in row {
                *v *= s;
            }
        }
        self
    }

    /// Where the pose puts a point, in blocks.
    #[cfg(test)]
    pub fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|i| {
            self.m[i][0] * p[0] + self.m[i][1] * p[1] + self.m[i][2] * p[2] + self.t[i]
        })
    }

    /// As a [`Transform`] about the block's corner. Scales must have been
    /// uniform, as Create's are.
    pub fn transform(&self) -> Transform {
        let s = (0..3)
            .map(|i| self.m[i][0] * self.m[i][0])
            .sum::<f32>()
            .sqrt();
        let r = self.m.map(|row| row.map(|v| v / s.max(1e-6)));
        Transform {
            pivot: [0.0; 3],
            rotate: euler(r),
            translate: self.t.map(|v| v * 16.0),
            scale: [s; 3],
        }
    }
}

/// `java.util.Random`, for the scatter Create seeds by an item's angle.
pub struct JavaRandom(u64);

impl JavaRandom {
    pub fn new(seed: i64) -> JavaRandom {
        JavaRandom((seed as u64 ^ 0x5DEE_CE66D) & ((1 << 48) - 1))
    }

    fn next(&mut self, bits: u32) -> u32 {
        self.0 = (self.0.wrapping_mul(0x5DEE_CE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as u32
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns `p` the way the API turns a part (X, then Y, then Z), then
    /// scales and moves it.
    fn api(t: &Transform, p: [f32; 3]) -> [f32; 3] {
        let r = mul(
            mul(
                rotation(Axis::Z, t.rotate[2]),
                rotation(Axis::Y, t.rotate[1]),
            ),
            rotation(Axis::X, t.rotate[0]),
        );
        std::array::from_fn(|i| {
            (0..3).map(|k| r[i][k] * p[k] * t.scale[k]).sum::<f32>() + t.translate[i] / 16.0
        })
    }

    #[test]
    fn a_pose_becomes_a_transform_that_moves_points_alike() {
        let pose = Pose::default()
            .translate(0.5, 0.9375, 0.5)
            .rotate(Axis::Y, 37.0)
            .translate(0.0, -0.09375, 0.0)
            .rotate(Axis::X, 90.0)
            .scale(0.5);
        let t = pose.transform();
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.3, -0.2, 0.7]] {
            let (a, b) = (pose.apply(p), api(&t, p));
            assert!((0..3).all(|i| (a[i] - b[i]).abs() < 1e-4), "{a:?} {b:?}");
        }
    }

    #[test]
    fn java_random_matches_java() {
        // new Random(0).nextFloat() in Java.
        assert!((JavaRandom::new(0).next_float() - 0.730_967_8).abs() < 1e-6);
        let mut r = JavaRandom::new(42);
        assert!((r.next_float() - 0.727_563_7).abs() < 1e-6);
    }
}
