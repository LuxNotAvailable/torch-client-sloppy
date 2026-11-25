//! Vanilla's `FastColor.ARGB32`/`ABGR32` (`Argb32::red(c)`) and Catnip's
//! `Color`. Colours are Java `int`s (`i32`); the API's tints take them as
//! `c as u32`.

use super::mth::Mth;
use super::vec::Vec3;

/// `FastColor.as8BitChannel(f)`.
pub fn as_8bit_channel(f: f32) -> i32 {
    Mth::floor(f * 255.0)
}

/// `FastColor.ARGB32`.
pub struct Argb32;

impl Argb32 {
    pub fn alpha(c: i32) -> i32 {
        ((c as u32) >> 24) as i32
    }

    pub fn red(c: i32) -> i32 {
        (c >> 16) & 255
    }

    pub fn green(c: i32) -> i32 {
        (c >> 8) & 255
    }

    pub fn blue(c: i32) -> i32 {
        c & 255
    }

    pub fn color(a: i32, r: i32, g: i32, b: i32) -> i32 {
        (a << 24) | (r << 16) | (g << 8) | b
    }

    /// `color(r, g, b)`: opaque.
    pub fn color_rgb(r: i32, g: i32, b: i32) -> i32 {
        Argb32::color(255, r, g, b)
    }

    /// `color(alpha, rgb)`.
    pub fn color_alpha(a: i32, rgb: i32) -> i32 {
        (a << 24) | (rgb & 0xFF_FFFF)
    }

    pub fn multiply(a: i32, b: i32) -> i32 {
        Argb32::color(
            Argb32::alpha(a) * Argb32::alpha(b) / 255,
            Argb32::red(a) * Argb32::red(b) / 255,
            Argb32::green(a) * Argb32::green(b) / 255,
            Argb32::blue(a) * Argb32::blue(b) / 255,
        )
    }

    pub fn lerp(delta: f32, a: i32, b: i32) -> i32 {
        Argb32::color(
            Mth::lerp_int(delta, Argb32::alpha(a), Argb32::alpha(b)),
            Mth::lerp_int(delta, Argb32::red(a), Argb32::red(b)),
            Mth::lerp_int(delta, Argb32::green(a), Argb32::green(b)),
            Mth::lerp_int(delta, Argb32::blue(a), Argb32::blue(b)),
        )
    }

    pub fn opaque(c: i32) -> i32 {
        c | 0xFF00_0000u32 as i32
    }

    pub fn color_from_float(a: f32, r: f32, g: f32, b: f32) -> i32 {
        Argb32::color(
            as_8bit_channel(a),
            as_8bit_channel(r),
            as_8bit_channel(g),
            as_8bit_channel(b),
        )
    }

    pub fn average(a: i32, b: i32) -> i32 {
        Argb32::color(
            (Argb32::alpha(a) + Argb32::alpha(b)) / 2,
            (Argb32::red(a) + Argb32::red(b)) / 2,
            (Argb32::green(a) + Argb32::green(b)) / 2,
            (Argb32::blue(a) + Argb32::blue(b)) / 2,
        )
    }
}

/// `FastColor.ABGR32`.
pub struct Abgr32;

impl Abgr32 {
    pub fn alpha(c: i32) -> i32 {
        ((c as u32) >> 24) as i32
    }

    pub fn red(c: i32) -> i32 {
        c & 255
    }

    pub fn green(c: i32) -> i32 {
        (c >> 8) & 255
    }

    pub fn blue(c: i32) -> i32 {
        (c >> 16) & 255
    }

    pub fn color(a: i32, b: i32, g: i32, r: i32) -> i32 {
        (a << 24) | (b << 16) | (g << 8) | r
    }

    pub fn from_argb32(c: i32) -> i32 {
        (c & 0xFF00_FF00u32 as i32) | ((c & 0xFF_0000) >> 16) | ((c & 255) << 16)
    }
}

/// Catnip's `Color` (`net.createmod.catnip.theme.Color`); the mutable
/// setters return the changed copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Color(pub i32);

impl Color {
    pub const TRANSPARENT_BLACK: Color = Color(0);
    pub const BLACK: Color = Color(0xFF00_0000u32 as i32);
    pub const WHITE: Color = Color(0xFFFF_FFFFu32 as i32);
    pub const RED: Color = Color(0xFFFF_0000u32 as i32);
    pub const GREEN: Color = Color(0xFF00_FF00u32 as i32);
    pub const PURPLE: Color = Color(0xFF80_0080u32 as i32);
    pub const SPRING_GREEN: Color = Color(0xFF00_FFBBu32 as i32);

    /// `new Color(r, g, b)`: opaque.
    pub fn rgb(r: i32, g: i32, b: i32) -> Color {
        Color::rgba(r, g, b, 255)
    }

    /// `new Color(r, g, b, a)`.
    pub fn rgba(r: i32, g: i32, b: i32, a: i32) -> Color {
        Color(((a & 255) << 24) | ((r & 255) << 16) | ((g & 255) << 8) | (b & 255))
    }

    /// `new Color(float r, g, b, a)`.
    pub fn rgba_f(r: f32, g: f32, b: f32, a: f32) -> Color {
        let c = |v: f32| (0.5 + 255.0 * Mth::clamp(v, 0.0, 1.0) as f64) as i32;
        Color::rgba(c(r), c(g), c(b), c(a))
    }

    /// `new Color(argb)`.
    pub fn new(argb: i32) -> Color {
        Color(argb)
    }

    /// `new Color(rgb, false)`: made opaque.
    pub fn new_alpha(argb: i32, has_alpha: bool) -> Color {
        Color(if has_alpha {
            argb
        } else {
            argb | 0xFF00_0000u32 as i32
        })
    }

    pub fn get_rgb(self) -> i32 {
        self.0
    }

    pub fn get_red(self) -> i32 {
        (self.0 >> 16) & 255
    }

    pub fn get_green(self) -> i32 {
        (self.0 >> 8) & 255
    }

    pub fn get_blue(self) -> i32 {
        self.0 & 255
    }

    pub fn get_alpha(self) -> i32 {
        (self.0 >> 24) & 255
    }

    pub fn get_red_as_float(self) -> f32 {
        self.get_red() as f32 / 255.0
    }

    pub fn get_green_as_float(self) -> f32 {
        self.get_green() as f32 / 255.0
    }

    pub fn get_blue_as_float(self) -> f32 {
        self.get_blue() as f32 / 255.0
    }

    pub fn get_alpha_as_float(self) -> f32 {
        self.get_alpha() as f32 / 255.0
    }

    pub fn as_vector(self) -> Vec3 {
        Vec3::new(
            self.get_red_as_float() as f64,
            self.get_green_as_float() as f64,
            self.get_blue_as_float() as f64,
        )
    }

    pub fn set_red(self, r: i32) -> Color {
        Color((self.0 & !0xFF_0000) | ((r & 255) << 16))
    }

    pub fn set_green(self, g: i32) -> Color {
        Color((self.0 & !0xFF00) | ((g & 255) << 8))
    }

    pub fn set_blue(self, b: i32) -> Color {
        Color((self.0 & !0xFF) | (b & 255))
    }

    pub fn set_alpha(self, a: i32) -> Color {
        Color((self.0 & 0xFF_FFFF) | ((a & 255) << 24))
    }

    /// `setAlpha(float)`.
    pub fn set_alpha_f(self, a: f32) -> Color {
        self.set_alpha((255.0 * Mth::clamp(a, 0.0, 1.0)) as i32)
    }

    pub fn scale_alpha(self, f: f32) -> Color {
        self.set_alpha((self.get_alpha() as f32 * Mth::clamp(f, 0.0, 1.0)) as i32)
    }

    pub fn mix_with(self, o: Color, w: f32) -> Color {
        let m = |a: i32, b: i32| (a as f32 + (b - a) as f32 * w) as i32;
        Color(self.0)
            .set_red(m(self.get_red(), o.get_red()))
            .set_green(m(self.get_green(), o.get_green()))
            .set_blue(m(self.get_blue(), o.get_blue()))
            .set_alpha(m(self.get_alpha(), o.get_alpha()))
    }

    pub fn darker(self) -> Color {
        self.mix_with(Color::BLACK, 0.25)
            .set_alpha(self.get_alpha())
    }

    pub fn brighter(self) -> Color {
        self.mix_with(Color::WHITE, 0.25)
            .set_alpha(self.get_alpha())
    }

    /// `Color.mixColors(c1, c2, w)`.
    pub fn mix_colors(a: Color, b: Color, w: f32) -> Color {
        let m = |x: i32, y: i32| (x as f32 + (y - x) as f32 * w) as i32;
        Color::rgba(
            m(a.get_red(), b.get_red()),
            m(a.get_green(), b.get_green()),
            m(a.get_blue(), b.get_blue()),
            m(a.get_alpha(), b.get_alpha()),
        )
    }

    /// `Color.mixColors(int, int, w)`: alpha sign-extended, as Java's.
    pub fn mix_colors_int(c1: i32, c2: i32, w: f32) -> i32 {
        let (a1, r1, g1, b1) = (c1 >> 24, (c1 >> 16) & 255, (c1 >> 8) & 255, c1 & 255);
        let (a2, r2, g2, b2) = (c2 >> 24, (c2 >> 16) & 255, (c2 >> 8) & 255, c2 & 255);
        let m = |x: i32, y: i32| (x as f32 + (y - x) as f32 * w) as i32;
        (m(a1, a2) << 24)
            .wrapping_add(m(r1, r2) << 16)
            .wrapping_add(m(g1, g2) << 8)
            .wrapping_add(m(b1, b2))
    }

    /// `Color.rainbowColor(timeStep)`.
    pub fn rainbow_color(time_step: i32) -> Color {
        let local = time_step.wrapping_abs() % 1536;
        let in_phase = local % 256;
        let phase_blue = local / 256;
        let phase = |p: i32| {
            let p = p % 6;
            if p <= 1 {
                0
            } else if p == 2 {
                in_phase
            } else if p <= 4 {
                255
            } else {
                255 - in_phase
            }
        };
        Color::rgb(
            phase(phase_blue + 4),
            phase(phase_blue + 2),
            phase(phase_blue),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_and_mixes_match_java() {
        let c = Argb32::color(0x80, 0x10, 0x20, 0x30);
        assert_eq!(
            (Argb32::alpha(c), Argb32::red(c), Argb32::blue(c)),
            (0x80, 0x10, 0x30)
        );
        assert_eq!(Argb32::multiply(-1, c), c);
        assert_eq!(
            Abgr32::from_argb32(0xFF11_2233u32 as i32),
            0xFF33_2211u32 as i32
        );
        assert_eq!(Color::rgb(255, 0, 0), Color::RED);
        assert_eq!(Color::WHITE.darker(), Color::rgb(191, 191, 191));
        assert_eq!(Color::rainbow_color(0), Color::rgb(255, 0, 0));
    }
}
