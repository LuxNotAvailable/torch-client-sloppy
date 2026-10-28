use bevy::math::{IVec2, IVec3, Mat4, Vec3, Vec4};

use super::values::FrameValues;
use crate::shaderpack::expressions::{MATRIX_ELEMENT, Resolved};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Value {
    Float(f32),
    Int(i32),
    Vec3(Vec3),
    Vec4(Vec4),
    IVec2(IVec2),
    IVec3(IVec3),
    Mat4(Mat4),
}

impl Value {
    pub(super) fn write(self, out: &mut [u8]) {
        let mut put = |bytes: &[u8]| out[..bytes.len()].copy_from_slice(bytes);
        match self {
            Value::Float(v) => put(bytemuck::bytes_of(&v)),
            Value::Int(v) => put(bytemuck::bytes_of(&v)),
            Value::Vec3(v) => put(bytemuck::bytes_of(&v)),
            Value::Vec4(v) => put(bytemuck::bytes_of(&v)),
            Value::IVec2(v) => put(bytemuck::bytes_of(&v)),
            Value::IVec3(v) => put(bytemuck::bytes_of(&v)),
            Value::Mat4(v) => put(bytemuck::bytes_of(&v)),
        }
    }

    fn components(self) -> Option<usize> {
        match self {
            Value::Float(_) | Value::Int(_) => None,
            Value::IVec2(_) => Some(2),
            Value::Vec3(_) | Value::IVec3(_) => Some(3),
            Value::Vec4(_) => Some(4),
            Value::Mat4(_) => Some(0),
        }
    }

    pub(super) fn has(self, index: Option<usize>) -> bool {
        if let Value::Mat4(_) = self {
            return index.is_some_and(|i| (MATRIX_ELEMENT..MATRIX_ELEMENT + 16).contains(&i));
        }
        match (self.components(), index) {
            (None, None) => true,
            (Some(n), Some(i)) => i < n,
            _ => false,
        }
    }

    pub(super) fn component(self, index: Option<usize>) -> f32 {
        if !self.has(index) {
            return 0.0;
        }
        let i = index.unwrap_or(0);
        match self {
            Value::Float(v) => v,
            Value::Int(v) => v as f32,
            Value::Vec3(v) => v[i],
            Value::Vec4(v) => v[i],
            Value::IVec2(v) => v[i] as f32,
            Value::IVec3(v) => v[i] as f32,
            Value::Mat4(m) => {
                let element = i - MATRIX_ELEMENT;
                m.col(element / 4)[element % 4]
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    ModelView,
    ModelViewInverse,
    Projection,
    ProjectionInverse,
    PreviousModelView,
    PreviousProjection,
    CameraPosition,
    PreviousCameraPosition,
    CameraPositionInt,
    CameraPositionFract,
    PreviousCameraPositionInt,
    PreviousCameraPositionFract,
    EyeAltitude,
    RelativeEyePosition,
    ViewWidth,
    ViewHeight,
    AspectRatio,
    Near,
    Far,
    FrameTimeCounter,
    FrameTime,
    FrameCounter,
    WorldTime,
    WorldDay,
    MoonPhase,
    SunAngle,
    ShadowAngle,
    SunPosition,
    MoonPosition,
    ShadowLightPosition,
    UpPosition,
    SkyColor,
    FogColor,
    RainStrength,
    Wetness,
    ThunderStrength,
    EyeBrightness,
    EyeBrightnessSmooth,
    IsEyeInWater,
    Blindness,
    NightVision,
    DarknessFactor,
    HideGui,
    EndFlashIntensity,
    PreviousEndFlashIntensity,
    EndFlashPosition,
    HeldItemId,
    HeldItemId2,
    HeldBlockLightValue,
    HeldBlockLightValue2,
    HeldBlockLightColor,
    FogStart,
    FogEnd,
    ScreenBrightness,
    PlayerBodyVector,
    Rainfall,
    Temperature,
    BiomePrecipitation,
    RenderStage,
    LightningBoltPosition,
    NoId,
    NoEntityColor,
    AtlasSize,
    Biome,
    ShadowModelView,
    ShadowModelViewInverse,
    ShadowProjection,
    ShadowProjectionInverse,
    BedrockLevel,
    HeightLimit,
    LogicalHeightLimit,
    SeaLevel,
    HasCeiling,
    HasSkylight,
    CurrentPlayerHealth,
    MaxPlayerHealth,
    CurrentPlayerHunger,
    MaxPlayerHunger,
    CurrentPlayerAir,
    MaxPlayerAir,
    CurrentPlayerArmor,
    MaxPlayerArmor,
    IsSpectator,
    FirstPersonCamera,
    IsRiding,
    IsElytraFlying,
    InSwimmingAnimation,
    FeetInWater,
    EyePosition,
    PlayerLookVector,
    VehicleId,
    VehicleInWater,
    VehicleLookVector,
    RelativeVehiclePosition,
    FogShape,
    DarknessLightFactor,
    BiomeCategory,
    FogMode,
    CloudHeight,
    CenterDepthSmooth,
}

/// Iris's `biome_category` values: the 1.18 `Biome.BiomeCategory` order, which
/// packs reach through the `CAT_*` names.
pub(crate) const BIOME_CATEGORIES: [&str; 19] = [
    "NONE",
    "TAIGA",
    "EXTREME_HILLS",
    "JUNGLE",
    "MESA",
    "PLAINS",
    "SAVANNA",
    "ICY",
    "THE_END",
    "BEACH",
    "FOREST",
    "OCEAN",
    "DESERT",
    "RIVER",
    "SWAMP",
    "MUSHROOM",
    "NETHER",
    "UNDERGROUND",
    "MOUNTAIN",
];

/// A biome's category by its name, as 1.18 filed the biomes that existed then
/// and the nearest category for the ones added since.
pub(crate) fn biome_category(name: &str) -> i32 {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let has = |parts: &[&str]| parts.iter().any(|p| name.contains(p));
    let category = if name == "the_void" {
        "NONE"
    } else if name == "the_end" || has(&["end_"]) || name == "small_end_islands" {
        "THE_END"
    } else if has(&["nether", "soul_sand", "crimson", "warped", "basalt"]) {
        "NETHER"
    } else if has(&["mushroom"]) {
        "MUSHROOM"
    } else if has(&["ocean"]) {
        "OCEAN"
    } else if has(&["river"]) {
        "RIVER"
    } else if has(&["beach", "shore"]) {
        "BEACH"
    } else if has(&["swamp"]) {
        "SWAMP"
    } else if has(&["desert"]) {
        "DESERT"
    } else if has(&["badlands"]) {
        "MESA"
    } else if has(&["savanna"]) {
        "SAVANNA"
    } else if has(&["jungle"]) {
        "JUNGLE"
    } else if has(&["taiga"]) {
        "TAIGA"
    } else if has(&["caves", "deep_dark"]) {
        "UNDERGROUND"
    } else if has(&["windswept"]) {
        "EXTREME_HILLS"
    } else if has(&["peaks", "slopes", "meadow", "grove"]) {
        "MOUNTAIN"
    } else if has(&["snowy", "ice", "frozen"]) {
        "ICY"
    } else if has(&["forest", "garden"]) {
        "FOREST"
    } else if has(&["plains"]) {
        "PLAINS"
    } else {
        "NONE"
    };
    BIOME_CATEGORIES
        .iter()
        .position(|c| *c == category)
        .map_or(0, |i| i as i32)
}

/// The GLSL type of the builtin uniform `name`, as packs declare it.
pub(crate) fn builtin_type(name: &str) -> Option<&'static str> {
    BUILTINS.iter().find(|(n, _, _)| *n == name).map(|(_, t, _)| *t)
}

pub(crate) const BUILTINS: &[(&str, &str, Builtin)] = &[
    ("gbufferModelView", "mat4", Builtin::ModelView),
    ("gbufferModelViewInverse", "mat4", Builtin::ModelViewInverse),
    ("gbufferProjection", "mat4", Builtin::Projection),
    (
        "gbufferProjectionInverse",
        "mat4",
        Builtin::ProjectionInverse,
    ),
    (
        "gbufferPreviousModelView",
        "mat4",
        Builtin::PreviousModelView,
    ),
    (
        "gbufferPreviousProjection",
        "mat4",
        Builtin::PreviousProjection,
    ),
    ("cameraPosition", "vec3", Builtin::CameraPosition),
    (
        "previousCameraPosition",
        "vec3",
        Builtin::PreviousCameraPosition,
    ),
    ("cameraPositionInt", "ivec3", Builtin::CameraPositionInt),
    ("cameraPositionFract", "vec3", Builtin::CameraPositionFract),
    (
        "previousCameraPositionInt",
        "ivec3",
        Builtin::PreviousCameraPositionInt,
    ),
    (
        "previousCameraPositionFract",
        "vec3",
        Builtin::PreviousCameraPositionFract,
    ),
    ("eyeAltitude", "float", Builtin::EyeAltitude),
    ("relativeEyePosition", "vec3", Builtin::RelativeEyePosition),
    ("viewWidth", "float", Builtin::ViewWidth),
    ("viewHeight", "float", Builtin::ViewHeight),
    ("aspectRatio", "float", Builtin::AspectRatio),
    ("near", "float", Builtin::Near),
    ("far", "float", Builtin::Far),
    ("frameTimeCounter", "float", Builtin::FrameTimeCounter),
    ("frameTime", "float", Builtin::FrameTime),
    ("frameCounter", "int", Builtin::FrameCounter),
    ("worldTime", "int", Builtin::WorldTime),
    ("worldDay", "int", Builtin::WorldDay),
    ("moonPhase", "int", Builtin::MoonPhase),
    ("sunAngle", "float", Builtin::SunAngle),
    ("shadowAngle", "float", Builtin::ShadowAngle),
    ("sunPosition", "vec3", Builtin::SunPosition),
    ("moonPosition", "vec3", Builtin::MoonPosition),
    ("shadowLightPosition", "vec3", Builtin::ShadowLightPosition),
    ("upPosition", "vec3", Builtin::UpPosition),
    ("skyColor", "vec3", Builtin::SkyColor),
    ("fogColor", "vec3", Builtin::FogColor),
    ("rainStrength", "float", Builtin::RainStrength),
    ("wetness", "float", Builtin::Wetness),
    ("thunderStrength", "float", Builtin::ThunderStrength),
    ("eyeBrightness", "ivec2", Builtin::EyeBrightness),
    ("eyeBrightnessSmooth", "ivec2", Builtin::EyeBrightnessSmooth),
    ("isEyeInWater", "int", Builtin::IsEyeInWater),
    ("blindness", "float", Builtin::Blindness),
    ("nightVision", "float", Builtin::NightVision),
    ("darknessFactor", "float", Builtin::DarknessFactor),
    ("hideGUI", "int", Builtin::HideGui),
    ("endFlashIntensity", "float", Builtin::EndFlashIntensity),
    (
        "previousEndFlashIntensity",
        "float",
        Builtin::PreviousEndFlashIntensity,
    ),
    ("endFlashPosition", "vec3", Builtin::EndFlashPosition),
    ("heldItemId", "int", Builtin::HeldItemId),
    ("heldItemId2", "int", Builtin::HeldItemId2),
    ("heldBlockLightValue", "int", Builtin::HeldBlockLightValue),
    ("heldBlockLightValue2", "int", Builtin::HeldBlockLightValue2),
    ("heldBlockLightColor", "vec3", Builtin::HeldBlockLightColor),
    ("heldBlockLightColor2", "vec3", Builtin::HeldBlockLightColor),
    ("fogStart", "float", Builtin::FogStart),
    ("fogEnd", "float", Builtin::FogEnd),
    ("screenBrightness", "float", Builtin::ScreenBrightness),
    ("playerBodyVector", "vec3", Builtin::PlayerBodyVector),
    ("rainfall", "float", Builtin::Rainfall),
    ("temperature", "float", Builtin::Temperature),
    ("biome_precipitation", "int", Builtin::BiomePrecipitation),
    ("renderStage", "int", Builtin::RenderStage),
    (
        "lightningBoltPosition",
        "vec4",
        Builtin::LightningBoltPosition,
    ),
    ("entityId", "int", Builtin::NoId),
    ("blockEntityId", "int", Builtin::NoId),
    ("currentRenderedItemId", "int", Builtin::NoId),
    ("entityColor", "vec4", Builtin::NoEntityColor),
    ("atlasSize", "ivec2", Builtin::AtlasSize),
    ("biome", "int", Builtin::Biome),
    ("shadowModelView", "mat4", Builtin::ShadowModelView),
    (
        "shadowModelViewInverse",
        "mat4",
        Builtin::ShadowModelViewInverse,
    ),
    ("shadowProjection", "mat4", Builtin::ShadowProjection),
    (
        "shadowProjectionInverse",
        "mat4",
        Builtin::ShadowProjectionInverse,
    ),
    ("bedrockLevel", "int", Builtin::BedrockLevel),
    ("heightLimit", "int", Builtin::HeightLimit),
    ("logicalHeightLimit", "int", Builtin::LogicalHeightLimit),
    ("seaLevel", "int", Builtin::SeaLevel),
    ("hasCeiling", "bool", Builtin::HasCeiling),
    ("hasSkylight", "bool", Builtin::HasSkylight),
    ("currentPlayerHealth", "float", Builtin::CurrentPlayerHealth),
    ("maxPlayerHealth", "float", Builtin::MaxPlayerHealth),
    ("currentPlayerHunger", "float", Builtin::CurrentPlayerHunger),
    ("maxPlayerHunger", "float", Builtin::MaxPlayerHunger),
    ("currentPlayerAir", "float", Builtin::CurrentPlayerAir),
    ("maxPlayerAir", "float", Builtin::MaxPlayerAir),
    ("currentPlayerArmor", "float", Builtin::CurrentPlayerArmor),
    ("maxPlayerArmor", "float", Builtin::MaxPlayerArmor),
    ("isSpectator", "bool", Builtin::IsSpectator),
    ("firstPersonCamera", "bool", Builtin::FirstPersonCamera),
    ("isRiding", "bool", Builtin::IsRiding),
    ("isElytraFlying", "bool", Builtin::IsElytraFlying),
    ("inSwimmingAnimation", "bool", Builtin::InSwimmingAnimation),
    ("feetInWater", "bool", Builtin::FeetInWater),
    ("eyePosition", "vec3", Builtin::EyePosition),
    ("playerLookVector", "vec3", Builtin::PlayerLookVector),
    ("vehicleId", "int", Builtin::VehicleId),
    ("vehicleInWater", "bool", Builtin::VehicleInWater),
    ("vehicleLookVector", "vec3", Builtin::VehicleLookVector),
    ("fogShape", "int", Builtin::FogShape),
    ("darknessLightFactor", "float", Builtin::DarknessLightFactor),
    ("biome_category", "int", Builtin::BiomeCategory),
    ("fogMode", "int", Builtin::FogMode),
    ("cloudHeight", "float", Builtin::CloudHeight),
    ("centerDepthSmooth", "float", Builtin::CenterDepthSmooth),
    (
        "relativeVehiclePosition",
        "vec3",
        Builtin::RelativeVehiclePosition,
    ),
];

pub(super) fn builtin(name: &str, ty: &str) -> Option<Builtin> {
    BUILTINS
        .iter()
        .find(|(n, t, _)| *n == name && (*t == ty || interchangeable(t, ty)))
        .map(|(_, _, b)| *b)
}

/// GL sets a `bool` uniform with `glUniform1i`, so Iris's int uniforms reach a
/// pack that declares them `bool`, and its bool ones a pack that declares them
/// `int`; both upload here as a 32-bit integer.
fn interchangeable(supplied: &str, declared: &str) -> bool {
    matches!((supplied, declared), ("int", "bool") | ("bool", "int"))
}

pub(crate) const IDS_PER_BUILTIN: usize = MATRIX_ELEMENT + 17;

fn input_id(index: usize, member: Option<usize>) -> u32 {
    (index * IDS_PER_BUILTIN + member.map_or(0, |m| m + 1)) as u32
}

pub(crate) fn resolve_input(name: &str, member: Option<usize>) -> Option<Resolved> {
    if member.is_none()
        && let Some(biome) = name.strip_prefix("BIOME_")
    {
        let row = crate::util::biome_color::biome_index(&biome.to_ascii_lowercase());
        return Some(Resolved::Constant(row.map_or(-1.0, f32::from)));
    }
    if member.is_none()
        && let Some(category) = name.strip_prefix("CAT_")
        && let Some(i) = BIOME_CATEGORIES.iter().position(|c| *c == category)
    {
        return Some(Resolved::Constant(i as f32));
    }
    let index = BUILTINS.iter().position(|(n, _, _)| *n == name)?;
    FrameValues::default()
        .get(BUILTINS[index].2)
        .has(member)
        .then(|| Resolved::Input(input_id(index, member)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biomes_file_under_iris_categories() {
        let cat = |c: &str| BIOME_CATEGORIES.iter().position(|n| *n == c).unwrap() as i32;
        for (biome, category) in [
            ("minecraft:plains", "PLAINS"),
            ("swamp", "SWAMP"),
            ("mangrove_swamp", "SWAMP"),
            ("eroded_badlands", "MESA"),
            ("old_growth_spruce_taiga", "TAIGA"),
            ("snowy_plains", "ICY"),
            ("frozen_peaks", "MOUNTAIN"),
            ("frozen_ocean", "OCEAN"),
            ("crimson_forest", "NETHER"),
            ("end_highlands", "THE_END"),
            ("deep_dark", "UNDERGROUND"),
            ("windswept_forest", "EXTREME_HILLS"),
            ("birch_forest", "FOREST"),
        ] {
            assert_eq!(biome_category(biome), cat(category), "{biome}");
        }
        assert_eq!(
            resolve_input("CAT_SWAMP", None),
            Some(Resolved::Constant(cat("SWAMP") as f32))
        );
    }

    #[test]
    fn an_int_uniform_reaches_a_bool_declaration_and_back() {
        assert_eq!(builtin("hideGUI", "bool"), Some(Builtin::HideGui));
        assert_eq!(builtin("isSpectator", "int"), Some(Builtin::IsSpectator));
        assert_eq!(builtin("hideGUI", "float"), None);
    }

    #[test]
    fn custom_inputs_resolve_by_name_and_member() {
        assert!(matches!(
            resolve_input("sunAngle", None),
            Some(Resolved::Input(_))
        ));
        assert!(matches!(
            resolve_input("cameraPosition", Some(1)),
            Some(Resolved::Input(_))
        ));
        assert!(matches!(
            resolve_input("gbufferModelViewInverse", Some(MATRIX_ELEMENT + 2 * 4 + 1)),
            Some(Resolved::Input(_))
        ));
        assert!(resolve_input("gbufferModelViewInverse", None).is_none());
        assert!(resolve_input("gbufferModelViewInverse", Some(1)).is_none());
        assert_eq!(
            resolve_input("cameraPosition", None),
            None,
            "a vector is read through a member"
        );
        assert_eq!(
            resolve_input("sunAngle", Some(0)),
            None,
            "a scalar has no members"
        );
        assert!(builtin("hasCeiling", "bool").is_some());
        assert_eq!(
            builtin("hasCeiling", "float"),
            None,
            "the declared type has to match, bool and int aside"
        );
        assert!(matches!(
            resolve_input("isRiding", None),
            Some(Resolved::Input(_))
        ));
        assert_eq!(FrameValues::default().get(Builtin::IsRiding), Value::Int(0));
        assert!(
            matches!(resolve_input("BIOME_NOT_A_BIOME", None), Some(Resolved::Constant(v)) if v == -1.0)
        );
    }
}
