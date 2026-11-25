//! Java's arithmetic where it differs from Rust's, and `java.util.Random`
//! (vanilla's `LegacyRandomSource`, `RandomSource.create(seed)`).
//!
//! Same as Rust, no helper needed: `(int) f` is `f as i32` (saturates,
//! NaN to 0), `%` on ints and floats, `Math.min`/`max` on ints.

/// `java.lang.Math`, where Rust's std doesn't match it.
pub struct JMath;

impl JMath {
    /// `Math.round(float)`: `floor(f + 0.5)` (Rust rounds halves away
    /// from zero: -2.5 is -3 there, -2 here).
    pub fn round(f: f32) -> i32 {
        (f + 0.5).floor() as i32
    }

    /// `Math.round(double)`.
    pub fn round_d(d: f64) -> i64 {
        (d + 0.5).floor() as i64
    }

    /// `Math.signum`: 0 stays 0 (Rust's `signum` gives 1 for +0).
    pub fn signum(f: f32) -> f32 {
        if f > 0.0 {
            1.0
        } else if f < 0.0 {
            -1.0
        } else {
            f
        }
    }

    /// `Math.signum(double)`.
    pub fn signum_d(d: f64) -> f64 {
        if d > 0.0 {
            1.0
        } else if d < 0.0 {
            -1.0
        } else {
            d
        }
    }

    /// `Math.floorMod(int, int)`: the sign of `y`.
    pub fn floor_mod(x: i32, y: i32) -> i32 {
        let r = x % y;
        if r != 0 && ((r < 0) != (y < 0)) {
            r + y
        } else {
            r
        }
    }

    /// `Math.floorDiv(int, int)`.
    pub fn floor_div(x: i32, y: i32) -> i32 {
        let q = x.wrapping_div(y);
        if (x % y != 0) && ((x < 0) != (y < 0)) {
            q - 1
        } else {
            q
        }
    }

    /// `Math.floorMod(long, long)`.
    pub fn floor_mod_l(x: i64, y: i64) -> i64 {
        let r = x % y;
        if r != 0 && ((r < 0) != (y < 0)) {
            r + y
        } else {
            r
        }
    }

    /// `Math.toRadians`.
    pub fn to_radians(deg: f64) -> f64 {
        deg * 0.017453292519943295
    }

    /// `Math.toDegrees`.
    pub fn to_degrees(rad: f64) -> f64 {
        rad * 57.29577951308232
    }
}

/// `String.hashCode()`, over UTF-16 code units as Java does.
pub fn string_hash(s: &str) -> i32 {
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// `java.util.Random` and vanilla's `LegacyRandomSource` (the same
/// generator): `new Random(seed)`, `RandomSource.create(seed)`.
#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: u64,
    next_gaussian: Option<f64>,
}

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const MASK: u64 = (1 << 48) - 1;

impl JavaRandom {
    pub fn new(seed: i64) -> JavaRandom {
        JavaRandom {
            seed: (seed as u64 ^ MULTIPLIER) & MASK,
            next_gaussian: None,
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        *self = JavaRandom::new(seed);
    }

    /// `next(bits)`.
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(0xB) & MASK;
        (self.seed >> (48 - bits)) as i64 as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextInt(bound)`: in `0..bound`. Panics if `bound <= 0`, as Java
    /// throws.
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    /// `RandomSource.nextIntBetweenInclusive(min, max)`.
    pub fn next_int_between_inclusive(&mut self, min: i32, max: i32) -> i32 {
        self.next_int_bounded(max - min + 1) + min
    }

    pub fn next_long(&mut self) -> i64 {
        let hi = self.next(32) as i64;
        let lo = self.next(32) as i64;
        (hi << 32).wrapping_add(lo)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * 5.960_464_5e-8
    }

    pub fn next_double(&mut self) -> f64 {
        let hi = self.next(26) as i64;
        let lo = self.next(27) as i64;
        ((hi << 27) + lo) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// `nextGaussian`: Marsaglia's polar method, a spare kept for the next
    /// call.
    pub fn next_gaussian(&mut self) -> f64 {
        if let Some(g) = self.next_gaussian.take() {
            return g;
        }
        loop {
            let a = 2.0 * self.next_double() - 1.0;
            let b = 2.0 * self.next_double() - 1.0;
            let s = a * a + b * b;
            if s < 1.0 && s != 0.0 {
                let m = (-2.0 * s.ln() / s).sqrt();
                self.next_gaussian = Some(b * m);
                return a * m;
            }
        }
    }

    /// `RandomSource.triangle(mode, deviation)`.
    pub fn triangle(&mut self, mode: f64, deviation: f64) -> f64 {
        mode + deviation * (self.next_double() - self.next_double())
    }

    /// `RandomSource.fork()`.
    pub fn fork(&mut self) -> JavaRandom {
        JavaRandom::new(self.next_long())
    }

    /// `consumeCount(n)`: skips `n` `nextInt()` calls.
    pub fn consume_count(&mut self, n: i32) {
        for _ in 0..n {
            self.next_int();
        }
    }
}

/// Vanilla's name for it.
pub type RandomSource = JavaRandom;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_matches_java() {
        // Values printed by java.util.Random.
        assert_eq!(JavaRandom::new(0).next_float(), 0.730_967_76);
        let mut r = JavaRandom::new(0);
        let ints: Vec<i32> = (0..3).map(|_| r.next_int_bounded(10)).collect();
        assert_eq!(ints, [0, 8, 9]);
        let mut r = JavaRandom::new(0);
        assert_eq!((r.next_int_bounded(16), r.next_int_bounded(16)), (11, 13));
        assert_eq!(JavaRandom::new(42).next_int(), -1_170_105_035);
        assert_eq!(JavaRandom::new(42).next_double(), 0.727_563_680_032_868_1);
        assert_eq!(JavaRandom::new(42).next_long(), -5_025_562_857_975_149_833);
        // Java's StrictMath.log can differ from Rust's ln in the last bit.
        let mut r = JavaRandom::new(42);
        assert!((r.next_gaussian() - 1.141_905_315_473_054_7).abs() < 1e-14);
        assert!((r.next_gaussian() - 0.919_407_948_982_787_9).abs() < 1e-14);
        let mut r = JavaRandom::new(-7);
        assert!(!r.next_boolean());
        assert_eq!(r.next_float(), 0.901_497_54);
    }

    #[test]
    fn math_matches_java() {
        assert_eq!(JMath::round(-2.5), -2);
        assert_eq!(JMath::round(2.5), 3);
        assert_eq!(JMath::floor_mod(-1, 4), 3);
        assert_eq!(JMath::floor_mod(1, -4), -3);
        assert_eq!(JMath::floor_mod(-4, 4), 0);
        assert_eq!(JMath::floor_div(-1, 4), -1);
        assert_eq!(JMath::floor_div(7, 2), 3);
        assert_eq!(JMath::signum(0.0), 0.0);
        assert_eq!(string_hash("hello"), 99_162_322);
        assert_eq!(string_hash("create:belt"), -528_284_471);
    }
}
