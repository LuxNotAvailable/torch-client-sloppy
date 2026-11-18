//! The game state a shader pack's uniforms read each frame (`Inputs`). The
//! renderer gathers it; the native renderer gets it as `inputs.json`.

use bevy::math::{IVec2, Mat4, Vec3};
use bevy::prelude::Resource;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum EyeIn {
    #[default]
    Air = 0,
    Water = 1,
    Lava = 2,
    PowderSnow = 3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vitals {
    pub(crate) health: f32,
    pub(crate) max_health: f32,
    pub(crate) hunger: f32,
    pub(crate) air: f32,
    pub(crate) max_air: f32,
    pub(crate) armor: f32,
}

pub(crate) const MAX_HUNGER: f32 = 20.0;
pub(crate) const MAX_ARMOR: f32 = 50.0;

impl Vitals {
    pub(crate) fn of(
        survival: bool,
        health: f32,
        max_health: f32,
        food: u32,
        air: i32,
        max_air: i32,
        armor: u8,
    ) -> Vitals {
        if !survival {
            return Vitals {
                health: -1.0,
                max_health: -1.0,
                hunger: -1.0,
                air: -1.0,
                max_air: -1.0,
                armor: -1.0,
            };
        }
        Vitals {
            health: health / max_health,
            max_health,
            hunger: food as f32 / MAX_HUNGER,
            air: air as f32 / max_air as f32,
            max_air: max_air as f32,
            armor: f32::from(armor) / MAX_ARMOR,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vehicle {
    pub(crate) id: i32,
    pub(crate) in_water: bool,
    pub(crate) look: Vec3,
    pub(crate) relative: Vec3,
}

pub(crate) fn view_vector(x_rot: f32, y_rot: f32) -> Vec3 {
    let (x, y) = (x_rot.to_radians(), (-y_rot).to_radians());
    Vec3::new(y.sin() * x.cos(), -x.sin(), y.cos() * x.cos())
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub(crate) struct Inputs {
    pub(crate) day_ticks: f64,
    pub(crate) sun_angle: f32,
    pub(crate) moon_angle: f32,
    pub(crate) moon_phase: i32,
    pub(crate) sky_color: Vec3,
    pub(crate) fog_color: Vec3,
    pub(crate) fog_range: bevy::math::Vec2,
    pub(crate) screen_brightness: f32,
    pub(crate) body_vector: Vec3,
    pub(crate) rainfall: f32,
    pub(crate) temperature: f32,
    pub(crate) precipitation: i32,
    pub(crate) rain: f32,
    /// The portal and nausea warp on the projection.
    pub(crate) warp: Mat4,
    pub(crate) thunder: f32,
    pub(crate) eye_in: EyeIn,
    pub(crate) eye_light: IVec2,
    pub(crate) blindness: f32,
    pub(crate) night_vision: f32,
    pub(crate) darkness: f32,
    pub(crate) biome: Option<u8>,
    pub(crate) atlas_size: IVec2,
    pub(crate) relative_eye: Vec3,
    pub(crate) hide_gui: bool,
    pub(crate) held_items: [i32; 2],
    pub(crate) held_light: [i32; 2],
    pub(crate) lightning: bevy::math::Vec4,
    pub(crate) bedrock_level: i32,
    pub(crate) height_limit: i32,
    pub(crate) logical_height: i32,
    pub(crate) sea_level: i32,
    pub(crate) has_ceiling: bool,
    pub(crate) has_skylight: bool,
    pub(crate) vitals: Vitals,
    pub(crate) spectator: bool,
    pub(crate) first_person: bool,
    pub(crate) riding: bool,
    pub(crate) elytra_flying: bool,
    pub(crate) swimming: bool,
    pub(crate) feet_in_water: bool,
    pub(crate) eye_position: Vec3,
    pub(crate) vehicle: Vehicle,
}

impl Inputs {
    /// The fields packs read, for the native renderer's snapshot
    /// (`inputs.json`).
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let v3 = |v: Vec3| [v.x, v.y, v.z];
        serde_json::json!({
            "day_ticks": self.day_ticks,
            "sun_angle": self.sun_angle,
            "moon_angle": self.moon_angle,
            "moon_phase": self.moon_phase,
            "sky_color": v3(self.sky_color),
            "fog_color": v3(self.fog_color),
            "fog_range": [self.fog_range.x, self.fog_range.y],
            "screen_brightness": self.screen_brightness,
            "rainfall": self.rainfall,
            "temperature": self.temperature,
            "precipitation": self.precipitation,
            "rain": self.rain,
            "thunder": self.thunder,
            "eye_in": self.eye_in as i32,
            "eye_light": [self.eye_light.x, self.eye_light.y],
            "biome": self.biome,
            "atlas_size": [self.atlas_size.x, self.atlas_size.y],
            "relative_eye": v3(self.relative_eye),
            "eye_position": v3(self.eye_position),
            "bedrock_level": self.bedrock_level,
            "height_limit": self.height_limit,
            "logical_height": self.logical_height,
            "sea_level": self.sea_level,
            "has_ceiling": self.has_ceiling,
            "has_skylight": self.has_skylight,
            "first_person": self.first_person,
        })
    }

    /// `to_json` read back; missing fields keep their defaults.
    pub(crate) fn from_json(value: &serde_json::Value) -> Inputs {
        let f = |k: &str| value[k].as_f64().unwrap_or(0.0) as f32;
        let i = |k: &str| value[k].as_i64().unwrap_or(0) as i32;
        let b = |k: &str| value[k].as_bool().unwrap_or(false);
        let a = |k: &str, n: usize| {
            value[k]
                .get(n)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0) as f32
        };
        let v3 = |k: &str| Vec3::new(a(k, 0), a(k, 1), a(k, 2));
        Inputs {
            day_ticks: value["day_ticks"].as_f64().unwrap_or(0.0),
            sun_angle: f("sun_angle"),
            moon_angle: f("moon_angle"),
            moon_phase: i("moon_phase"),
            sky_color: v3("sky_color"),
            fog_color: v3("fog_color"),
            fog_range: bevy::math::Vec2::new(a("fog_range", 0), a("fog_range", 1)),
            screen_brightness: f("screen_brightness"),
            rainfall: f("rainfall"),
            temperature: f("temperature"),
            precipitation: i("precipitation"),
            rain: f("rain"),
            thunder: f("thunder"),
            eye_in: match i("eye_in") {
                1 => EyeIn::Water,
                2 => EyeIn::Lava,
                3 => EyeIn::PowderSnow,
                _ => EyeIn::Air,
            },
            eye_light: IVec2::new(a("eye_light", 0) as i32, a("eye_light", 1) as i32),
            biome: value["biome"].as_u64().map(|b| b as u8),
            atlas_size: IVec2::new(a("atlas_size", 0) as i32, a("atlas_size", 1) as i32),
            relative_eye: v3("relative_eye"),
            eye_position: v3("eye_position"),
            bedrock_level: i("bedrock_level"),
            height_limit: i("height_limit"),
            logical_height: i("logical_height"),
            sea_level: i("sea_level"),
            has_ceiling: b("has_ceiling"),
            has_skylight: b("has_skylight"),
            first_person: b("first_person"),
            warp: Mat4::IDENTITY,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vitals_read_minus_one_outside_survival() {
        let creative = Vitals::of(false, 20.0, 20.0, 20, 300, 300, 10);
        assert_eq!(
            creative,
            Vitals {
                health: -1.0,
                max_health: -1.0,
                hunger: -1.0,
                air: -1.0,
                max_air: -1.0,
                armor: -1.0
            }
        );
        assert!(crate::session::Gamemode::Adventure.is_survival());
        assert!(!crate::session::Gamemode::Spectator.is_survival());
        let survival = Vitals::of(true, 10.0, 20.0, 10, 150, 300, 25);
        assert_eq!(
            survival,
            Vitals {
                health: 0.5,
                max_health: 20.0,
                hunger: 0.5,
                air: 0.5,
                max_air: 300.0,
                armor: 0.5
            }
        );
    }

    #[test]
    fn view_vectors_follow_vanilla() {
        assert!(view_vector(0.0, 0.0).abs_diff_eq(Vec3::Z, 1e-6));
        assert!(view_vector(0.0, 90.0).abs_diff_eq(-Vec3::X, 1e-6));
        assert!(view_vector(-90.0, 0.0).abs_diff_eq(Vec3::Y, 1e-6));
    }
}
