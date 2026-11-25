//! Vanilla's `net.minecraft.util.Mth`, as `Mth::name` (Java `Mth.name`).
//! Float functions take `f32` or `f64`, as Java's overloads do; mixed
//! arguments need a cast, as they get one in Java.

use super::java::JavaRandom;

/// `f32` or `f64`, for `Mth`'s overloads.
pub trait Float:
    Copy
    + PartialOrd
    + std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Mul<Output = Self>
    + std::ops::Div<Output = Self>
    + std::ops::Rem<Output = Self>
    + std::ops::Neg<Output = Self>
{
    fn of(v: f64) -> Self;
    fn f64(self) -> f64;
}

impl Float for f32 {
    fn of(v: f64) -> f32 {
        v as f32
    }
    fn f64(self) -> f64 {
        self as f64
    }
}

impl Float for f64 {
    fn of(v: f64) -> f64 {
        v
    }
    fn f64(self) -> f64 {
        self
    }
}

/// `Mth.clamp`'s overloads: floats as Java's `v < min ? min : min(v, max)`,
/// ints as `min(max(v, min), max)`.
pub trait Clamp: Copy {
    fn clamp_java(self, min: Self, max: Self) -> Self;
}

macro_rules! clamp_float {
    ($t:ty) => {
        impl Clamp for $t {
            fn clamp_java(self, min: $t, max: $t) -> $t {
                if self < min {
                    min
                } else if self.is_nan() {
                    self
                } else {
                    self.min(max)
                }
            }
        }
    };
}
clamp_float!(f32);
clamp_float!(f64);

macro_rules! clamp_int {
    ($t:ty) => {
        impl Clamp for $t {
            fn clamp_java(self, min: $t, max: $t) -> $t {
                Ord::min(Ord::max(self, min), max)
            }
        }
    };
}
clamp_int!(i32);
clamp_int!(i64);

/// `Mth`.
pub struct Mth;

impl Mth {
    pub const PI: f32 = std::f32::consts::PI;
    pub const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;
    pub const TWO_PI: f32 = std::f32::consts::TAU;
    pub const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;
    pub const RAD_TO_DEG: f32 = 180.0 / std::f32::consts::PI;
    pub const EPSILON: f32 = 1.0e-5;
    pub const SQRT_OF_TWO: f32 = std::f32::consts::SQRT_2;

    /// The table entry Java's `SIN[i]` holds.
    fn sin_table(i: i32) -> f32 {
        ((i & 65535) as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
    }

    /// `Mth.sin`: Java's 65536-entry table, not `f32::sin`.
    pub fn sin(f: f32) -> f32 {
        Mth::sin_table((f * 10430.378) as i32)
    }

    /// `Mth.cos`: the same table, a quarter turn on.
    pub fn cos(f: f32) -> f32 {
        Mth::sin_table((f * 10430.378 + 16384.0) as i32)
    }

    pub fn sqrt(f: f32) -> f32 {
        f.sqrt()
    }

    /// `Mth.floor(float|double)`.
    pub fn floor<T: Float>(v: T) -> i32 {
        let i = v.f64() as i32;
        if v.f64() < i as f64 { i - 1 } else { i }
    }

    pub fn lfloor(v: f64) -> i64 {
        let i = v as i64;
        if v < i as f64 { i - 1 } else { i }
    }

    pub fn ceil<T: Float>(v: T) -> i32 {
        let i = v.f64() as i32;
        if v.f64() > i as f64 { i + 1 } else { i }
    }

    pub fn abs<T: Float>(v: T) -> T {
        if v < T::of(0.0) { -v } else { v }
    }

    /// `Mth.clamp`, for floats and ints.
    pub fn clamp<T: Clamp>(v: T, min: T, max: T) -> T {
        v.clamp_java(min, max)
    }

    pub fn clamped_lerp<T: Float>(start: T, end: T, delta: T) -> T {
        if delta < T::of(0.0) {
            start
        } else if delta > T::of(1.0) {
            end
        } else {
            Mth::lerp(delta, start, end)
        }
    }

    pub fn abs_max(a: f64, b: f64) -> f64 {
        a.abs().max(b.abs())
    }

    pub fn floor_div(a: i32, b: i32) -> i32 {
        super::java::JMath::floor_div(a, b)
    }

    /// `Mth.nextInt(random, min, max)`: inclusive.
    pub fn next_int(r: &mut JavaRandom, min: i32, max: i32) -> i32 {
        if min >= max {
            min
        } else {
            r.next_int_bounded(max - min + 1) + min
        }
    }

    pub fn next_float(r: &mut JavaRandom, min: f32, max: f32) -> f32 {
        if min >= max {
            min
        } else {
            r.next_float() * (max - min) + min
        }
    }

    pub fn next_double(r: &mut JavaRandom, min: f64, max: f64) -> f64 {
        if min >= max {
            min
        } else {
            r.next_double() * (max - min) + min
        }
    }

    /// `Mth.equal`: closer than 1e-5.
    pub fn equal<T: Float>(a: T, b: T) -> bool {
        Mth::abs(b - a).f64() < 1.0e-5f32 as f64
    }

    /// `Mth.positiveModulo(int, int)` (`Math.floorMod`).
    pub fn positive_modulo(a: i32, b: i32) -> i32 {
        super::java::JMath::floor_mod(a, b)
    }

    /// `Mth.positiveModulo(float|double, ...)`.
    pub fn positive_modulo_f<T: Float>(a: T, b: T) -> T {
        (a % b + b) % b
    }

    pub fn is_multiple_of(a: i32, b: i32) -> bool {
        a % b == 0
    }

    /// `Mth.wrapDegrees(float|double)`: into `[-180, 180)`.
    pub fn wrap_degrees<T: Float>(v: T) -> T {
        let mut r = v % T::of(360.0);
        if r >= T::of(180.0) {
            r = r - T::of(360.0);
        }
        if r < T::of(-180.0) {
            r = r + T::of(360.0);
        }
        r
    }

    /// `Mth.wrapDegrees(int)`.
    pub fn wrap_degrees_i(v: i32) -> i32 {
        let mut r = v % 360;
        if r >= 180 {
            r -= 360;
        }
        if r < -180 {
            r += 360;
        }
        r
    }

    pub fn degrees_difference(a: f32, b: f32) -> f32 {
        Mth::wrap_degrees(b - a)
    }

    pub fn degrees_difference_abs(a: f32, b: f32) -> f32 {
        Mth::degrees_difference(a, b).abs()
    }

    pub fn rotate_if_necessary(angle: f32, target: f32, max: f32) -> f32 {
        let d = Mth::degrees_difference(angle, target);
        target - Mth::clamp(d, -max, max)
    }

    pub fn approach(from: f32, to: f32, step: f32) -> f32 {
        let step = step.abs();
        if from < to {
            Mth::clamp(from + step, from, to)
        } else {
            Mth::clamp(from - step, to, from)
        }
    }

    pub fn approach_degrees(from: f32, to: f32, step: f32) -> f32 {
        let d = Mth::degrees_difference(from, to);
        Mth::approach(from, from + d, step)
    }

    /// `Mth.getInt(string, default)`.
    pub fn get_int(s: &str, default: i32) -> i32 {
        s.trim().parse().unwrap_or(default)
    }

    pub fn smallest_encompassing_power_of_two(v: i32) -> i32 {
        let mut i = v.wrapping_sub(1);
        i |= i >> 1;
        i |= i >> 2;
        i |= i >> 4;
        i |= i >> 8;
        i |= i >> 16;
        i.wrapping_add(1)
    }

    pub fn is_power_of_two(v: i32) -> bool {
        v != 0 && (v & v.wrapping_sub(1)) == 0
    }

    pub fn ceillog2(v: i32) -> i32 {
        const DE_BRUIJN: [i32; 32] = [
            0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19, 16,
            7, 26, 12, 18, 6, 11, 5, 10, 9,
        ];
        let v = if Mth::is_power_of_two(v) {
            v
        } else {
            Mth::smallest_encompassing_power_of_two(v)
        };
        DE_BRUIJN[(((v as i64).wrapping_mul(125_613_361) >> 27) as i32 & 31) as usize]
    }

    pub fn log2(v: i32) -> i32 {
        Mth::ceillog2(v) - if Mth::is_power_of_two(v) { 0 } else { 1 }
    }

    /// `Mth.color(r, g, b)`: RGB from 0..1 floats, alpha 0.
    pub fn color(r: f32, g: f32, b: f32) -> i32 {
        super::color::Argb32::color(
            0,
            Mth::floor(r * 255.0),
            Mth::floor(g * 255.0),
            Mth::floor(b * 255.0),
        )
    }

    pub fn frac(v: f32) -> f32 {
        v - Mth::floor(v) as f32
    }

    pub fn frac_d(v: f64) -> f64 {
        v - Mth::lfloor(v) as f64
    }

    /// `Mth.getSeed(x, y, z)`: the per-position seed (random block model
    /// offsets, positional randoms).
    pub fn get_seed(x: i32, y: i32, z: i32) -> i64 {
        let mut l =
            (x.wrapping_mul(3_129_871) as i64) ^ (z as i64).wrapping_mul(116_129_781) ^ y as i64;
        l = l
            .wrapping_mul(l)
            .wrapping_mul(42_317_861)
            .wrapping_add(l.wrapping_mul(11));
        l >> 16
    }

    pub fn inverse_lerp<T: Float>(v: T, start: T, end: T) -> T {
        (v - start) / (end - start)
    }

    /// `Mth.atan2(y, x)`: Java's own approximation, not `f64::atan2`.
    pub fn atan2(mut y: f64, mut x: f64) -> f64 {
        let d = x * x + y * y;
        if d.is_nan() {
            return f64::NAN;
        }
        let neg_y = y < 0.0;
        if neg_y {
            y = -y;
        }
        let neg_x = x < 0.0;
        if neg_x {
            x = -x;
        }
        let swap = y > x;
        if swap {
            std::mem::swap(&mut x, &mut y);
        }
        let inv = Mth::fast_inv_sqrt(d);
        x *= inv;
        y *= inv;
        let frac_bias = f64::from_bits(4_805_340_802_404_319_232);
        let e = frac_bias + y;
        let idx = e.to_bits() as i32 as usize;
        let asin = (idx as f64 / 256.0).asin();
        let cos = asin.cos();
        let f = e - frac_bias;
        let g = y * cos - x * f;
        let h = (6.0 + g * g) * g * 0.16666666666666666;
        let mut r = asin + h;
        if swap {
            r = std::f64::consts::FRAC_PI_2 - r;
        }
        if neg_x {
            r = std::f64::consts::PI - r;
        }
        if neg_y {
            r = -r;
        }
        r
    }

    pub fn inv_sqrt<T: Float>(v: T) -> T {
        T::of(1.0 / v.f64().sqrt())
    }

    pub fn fast_inv_sqrt(v: f64) -> f64 {
        let half = 0.5 * v;
        let i = 6_910_469_410_427_058_090i64 - ((v.to_bits() as i64) >> 1);
        let v = f64::from_bits(i as u64);
        v * (1.5 - half * v * v)
    }

    pub fn fast_inv_cube_root(v: f32) -> f32 {
        let i = 1_419_967_116 - (v.to_bits() as i32) / 3;
        let mut f = f32::from_bits(i as u32);
        f = 0.666_666_7 * f + 1.0 / (3.0 * f * f * v);
        0.666_666_7 * f + 1.0 / (3.0 * f * f * v)
    }

    pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> i32 {
        Mth::hsv_to_argb(h, s, v, 0)
    }

    pub fn hsv_to_argb(h: f32, s: f32, v: f32, alpha: i32) -> i32 {
        let i = (h * 6.0) as i32 % 6;
        let f = h * 6.0 - i as f32;
        let p = v * (1.0 - s);
        let q = v * (1.0 - f * s);
        let t = v * (1.0 - (1.0 - f) * s);
        let (r, g, b) = match i {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            5 => (v, p, q),
            _ => panic!(
                "Something went wrong when converting from HSV to RGB. Input was {h}, {s}, {v}"
            ),
        };
        let c = |x: f32| Mth::clamp((x * 255.0) as i32, 0, 255);
        super::color::Argb32::color(alpha, c(r), c(g), c(b))
    }

    pub fn murmur_hash3_mixer(mut v: i32) -> i32 {
        v ^= ((v as u32) >> 16) as i32;
        v = v.wrapping_mul(-2_048_144_789);
        v ^= ((v as u32) >> 13) as i32;
        v = v.wrapping_mul(-1_028_477_387);
        v ^ ((v as u32) >> 16) as i32
    }

    /// `Mth.binarySearch(from, to, predicate)`.
    pub fn binary_search(mut from: i32, to: i32, test: impl Fn(i32) -> bool) -> i32 {
        let mut len = to - from;
        while len > 0 {
            let half = len / 2;
            let mid = from + half;
            if test(mid) {
                len = half;
            } else {
                from = mid + 1;
                len -= half + 1;
            }
        }
        from
    }

    pub fn lerp_int(delta: f32, start: i32, end: i32) -> i32 {
        start + Mth::floor(delta * (end - start) as f32)
    }

    pub fn lerp_discrete(delta: f32, start: i32, end: i32) -> i32 {
        let d = end - start;
        start + Mth::floor(delta * (d - 1) as f32) + if delta > 0.0 { 1 } else { 0 }
    }

    /// `Mth.lerp(delta, start, end)`.
    pub fn lerp<T: Float>(delta: T, start: T, end: T) -> T {
        start + delta * (end - start)
    }

    pub fn lerp2(dx: f64, dy: f64, x0y0: f64, x1y0: f64, x0y1: f64, x1y1: f64) -> f64 {
        Mth::lerp(dy, Mth::lerp(dx, x0y0, x1y0), Mth::lerp(dx, x0y1, x1y1))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn lerp3(
        dx: f64,
        dy: f64,
        dz: f64,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
    ) -> f64 {
        Mth::lerp(
            dz,
            Mth::lerp2(dx, dy, a, b, c, d),
            Mth::lerp2(dx, dy, e, f, g, h),
        )
    }

    pub fn catmullrom(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
        0.5 * (2.0 * p1
            + (p2 - p0) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
            + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
    }

    pub fn smoothstep(t: f64) -> f64 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    pub fn smoothstep_derivative(t: f64) -> f64 {
        30.0 * t * t * (t - 1.0) * (t - 1.0)
    }

    pub fn sign(v: f64) -> i32 {
        if v == 0.0 {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    }

    /// `Mth.rotLerp`: the short way round.
    pub fn rot_lerp<T: Float>(delta: T, start: T, end: T) -> T {
        start + delta * Mth::wrap_degrees(end - start)
    }

    pub fn triangle_wave(v: f32, period: f32) -> f32 {
        ((v % period - period * 0.5).abs() - period * 0.25) / (period * 0.25)
    }

    pub fn square<T: std::ops::Mul<Output = T> + Copy>(v: T) -> T {
        v * v
    }

    pub fn clamped_map<T: Float>(v: T, from0: T, from1: T, to0: T, to1: T) -> T {
        Mth::clamped_lerp(to0, to1, Mth::inverse_lerp(v, from0, from1))
    }

    pub fn map<T: Float>(v: T, from0: T, from1: T, to0: T, to1: T) -> T {
        Mth::lerp(Mth::inverse_lerp(v, from0, from1), to0, to1)
    }

    pub fn wobble(v: f64) -> f64 {
        v + (2.0 * JavaRandom::new(Mth::floor(v * 3000.0) as i64).next_double() - 1.0) * 1.0e-7
            / 2.0
    }

    pub fn round_toward(v: i32, multiple: i32) -> i32 {
        Mth::positive_ceil_div(v, multiple) * multiple
    }

    pub fn positive_ceil_div(a: i32, b: i32) -> i32 {
        -Mth::floor_div(-a, b)
    }

    pub fn random_between_inclusive(r: &mut JavaRandom, min: i32, max: i32) -> i32 {
        r.next_int_bounded(max - min + 1) + min
    }

    pub fn random_between(r: &mut JavaRandom, min: f32, max: f32) -> f32 {
        r.next_float() * (max - min) + min
    }

    pub fn normal(r: &mut JavaRandom, mean: f32, deviation: f32) -> f32 {
        mean + r.next_gaussian() as f32 * deviation
    }

    pub fn length_squared(x: f64, y: f64, z: f64) -> f64 {
        x * x + y * y + z * z
    }

    pub fn length(x: f64, y: f64, z: f64) -> f64 {
        Mth::length_squared(x, y, z).sqrt()
    }

    pub fn length2(x: f64, y: f64) -> f64 {
        (x * x + y * y).sqrt()
    }

    pub fn quantize(v: f64, step: i32) -> i32 {
        Mth::floor(v / step as f64) * step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_uses_javas_table() {
        // A table step is ~9.6e-5 rad: f32::sin differs, the table doesn't.
        assert_eq!(Mth::sin(0.0), 0.0);
        assert_eq!(Mth::sin(Mth::HALF_PI), 1.0);
        assert!((Mth::cos(0.0) - 1.0).abs() < 1e-7);
        assert_eq!(Mth::sin(1.0), Mth::sin_table((1.0f32 * 10430.378) as i32));
        assert_eq!(Mth::sin(-1.0), -Mth::sin_table(10430));
    }

    #[test]
    fn floors_wraps_and_clamps_follow_java() {
        assert_eq!(Mth::floor(-0.5f32), -1);
        assert_eq!(Mth::floor(2.0f64), 2);
        assert_eq!(Mth::ceil(-0.5f32), 0);
        assert_eq!(Mth::wrap_degrees(190.0f32), -170.0);
        assert_eq!(Mth::wrap_degrees(-180.0f32), -180.0);
        assert_eq!(Mth::wrap_degrees(180.0f64), -180.0);
        assert_eq!(Mth::wrap_degrees_i(540), -180);
        assert_eq!(Mth::clamp(5, 0, 3), 3);
        assert_eq!(Mth::clamp(-1.0f32, 0.0, 1.0), 0.0);
        assert!(Mth::clamp(f32::NAN, 0.0, 1.0).is_nan());
        assert_eq!(Mth::rot_lerp(0.5f32, 350.0, 10.0), 360.0);
        assert_eq!(Mth::approach(0.0, 1.0, 0.3), 0.3);
        assert!(Mth::equal(1.0f32, 1.000_001));
        assert_eq!(Mth::positive_modulo_f(-1.0f32, 4.0), 3.0);
        assert_eq!(Mth::frac(-0.25), 0.75);
    }

    #[test]
    fn hashes_and_seeds_match_java() {
        // Values from vanilla's code run in Java.
        assert_eq!(Mth::get_seed(1, 2, 3), -33_674_130_277_896);
        assert_eq!(Mth::get_seed(-100, 64, 12345), -134_443_635_571_458);
        assert_eq!(Mth::murmur_hash3_mixer(1), 1_364_076_727);
        assert_eq!(Mth::murmur_hash3_mixer(-123_456), 2_066_350_575);
        assert_eq!((Mth::ceillog2(17), Mth::ceillog2(16)), (5, 4));
        assert_eq!(Mth::log2(17), 4);
        assert_eq!(Mth::hsv_to_rgb(0.5, 1.0, 1.0), 65535);
        assert_eq!(Mth::hsv_to_rgb(0.1, 0.5, 0.8), 13_411_174);
        let bits = |f: f32| f.to_bits() as i32;
        assert_eq!(bits(Mth::sin(1.0)), 1_062_693_212);
        assert_eq!(bits(Mth::cos(1.0)), 1_057_641_281);
        assert_eq!(bits(Mth::sin(-1.0)), -1_084_790_436);
        assert_eq!(bits(Mth::sin(100.5)), -1_124_227_936);
    }

    #[test]
    fn atan2_is_close_to_the_real_one() {
        for (y, x) in [(1.0, 1.0), (-1.0, 0.5), (0.3, -2.0), (-4.0, -4.0)] {
            let a: f64 = Mth::atan2(y, x);
            assert!((a - f64::atan2(y, x)).abs() < 1e-6, "{y} {x} {a}");
        }
    }
}
