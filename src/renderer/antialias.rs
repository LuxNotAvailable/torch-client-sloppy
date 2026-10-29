//! Antialiasing: FXAA (a post pass, at a chosen sensitivity), MSAA (2, 4 or
//! 8 samples, as many as the GPU supports) or supersampling (`ssaa`, the
//! scene drawn larger and scaled down).
//!
//! MSAA and supersampling stay off while a shader pack draws the world: its
//! passes are single-sampled and size their own targets. FXAA still runs,
//! after the pack, on the last scene camera.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use bevy::anti_alias::fxaa::{Fxaa, Sensitivity};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::RenderAdapter;

use super::hand::HandCamera;
use super::panorama::PanoramaCamera;
use super::systems::WorldCamera;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AaMode {
    #[default]
    Off,
    Fxaa,
    Msaa,
    Supersample,
}

impl AaMode {
    pub fn caption(self) -> &'static str {
        match self {
            AaMode::Off => "OFF",
            AaMode::Fxaa => "FXAA",
            AaMode::Msaa => "MSAA",
            AaMode::Supersample => "Supersampling",
        }
    }

    pub fn next(self) -> AaMode {
        match self {
            AaMode::Off => AaMode::Fxaa,
            AaMode::Fxaa => AaMode::Msaa,
            AaMode::Msaa => AaMode::Supersample,
            AaMode::Supersample => AaMode::Off,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AaMode::Off => "off",
            AaMode::Fxaa => "fxaa",
            AaMode::Msaa => "msaa",
            AaMode::Supersample => "ssaa",
        }
    }

    pub fn from_key(key: &str) -> Option<AaMode> {
        match key {
            "off" => Some(AaMode::Off),
            "fxaa" => Some(AaMode::Fxaa),
            "msaa" => Some(AaMode::Msaa),
            "ssaa" => Some(AaMode::Supersample),
            _ => None,
        }
    }
}

/// FXAA's sensitivities, sharpest first.
pub const FXAA_LEVELS: [(&str, Sensitivity); 5] = [
    ("Low", Sensitivity::Low),
    ("Medium", Sensitivity::Medium),
    ("High", Sensitivity::High),
    ("Ultra", Sensitivity::Ultra),
    ("Extreme", Sensitivity::Extreme),
];
pub const MSAA_SAMPLES: [u8; 3] = [2, 4, 8];
/// Supersampling's scale along each axis.
pub const SSAA_SCALES: [f32; 2] = [1.5, 2.0];

/// The Antialiasing settings: the mode, and a level kept for each mode so
/// switching back and forth keeps what was picked.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Antialiasing {
    pub mode: AaMode,
    /// An index into `FXAA_LEVELS`.
    pub fxaa: usize,
    pub msaa: u8,
    /// An index into `SSAA_SCALES`.
    pub ssaa: usize,
}

impl Default for Antialiasing {
    fn default() -> Antialiasing {
        Antialiasing {
            mode: AaMode::Off,
            fxaa: 2,
            msaa: 4,
            ssaa: 1,
        }
    }
}

impl Antialiasing {
    /// The level's caption for the current mode; `None` when it has none.
    pub fn level_caption(&self) -> Option<String> {
        match self.mode {
            AaMode::Off => None,
            AaMode::Fxaa => Some(
                FXAA_LEVELS[self.fxaa.min(FXAA_LEVELS.len() - 1)]
                    .0
                    .to_owned(),
            ),
            AaMode::Msaa => Some(format!("{}x", self.msaa_samples())),
            AaMode::Supersample => Some(format!(
                "{}x",
                SSAA_SCALES[self.ssaa.min(SSAA_SCALES.len() - 1)]
            )),
        }
    }

    /// Steps the current mode's level, wrapping, skipping sample counts the
    /// GPU cannot do.
    pub fn next_level(&mut self) {
        match self.mode {
            AaMode::Off => {}
            AaMode::Fxaa => self.fxaa = (self.fxaa + 1) % FXAA_LEVELS.len(),
            AaMode::Msaa => {
                let supported: Vec<u8> =
                    MSAA_SAMPLES.into_iter().filter(|&n| supports(n)).collect();
                if supported.is_empty() {
                    return;
                }
                let at = supported.iter().position(|&n| n == self.msaa);
                self.msaa = supported[at.map_or(0, |i| (i + 1) % supported.len())];
            }
            AaMode::Supersample => self.ssaa = (self.ssaa + 1) % SSAA_SCALES.len(),
        }
    }

    /// The MSAA samples to draw with: the chosen count, lowered to one the
    /// GPU supports.
    pub fn msaa_samples(&self) -> u8 {
        MSAA_SAMPLES
            .into_iter()
            .rev()
            .filter(|&n| n <= self.msaa && supports(n))
            .next()
            .unwrap_or(1)
    }

    /// Supersampling's per-axis scale, when it is the mode.
    pub fn supersample(&self) -> Option<f32> {
        (self.mode == AaMode::Supersample)
            .then(|| SSAA_SCALES[self.ssaa.min(SSAA_SCALES.len() - 1)])
    }

    /// Reads the saved settings. Before modes there was a single
    /// `antialiasing` switch, which was 2x supersampling.
    pub fn read(value: impl Fn(&str) -> Option<serde_json::Value>) -> Antialiasing {
        let mut aa = Antialiasing::default();
        match value("antialiasing") {
            Some(serde_json::Value::Bool(true)) => aa.mode = AaMode::Supersample,
            Some(serde_json::Value::String(key)) => {
                aa.mode = AaMode::from_key(&key).unwrap_or_default()
            }
            _ => {}
        }
        if let Some(i) = value("fxaaLevel").and_then(|v| v.as_u64()) {
            aa.fxaa = (i as usize).min(FXAA_LEVELS.len() - 1);
        }
        if let Some(n) = value("msaaSamples").and_then(|v| v.as_u64())
            && MSAA_SAMPLES.contains(&(n as u8))
        {
            aa.msaa = n as u8;
        }
        if let Some(i) = value("ssaaLevel").and_then(|v| v.as_u64()) {
            aa.ssaa = (i as usize).min(SSAA_SCALES.len() - 1);
        }
        aa
    }

    pub fn write(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        map.insert("antialiasing".into(), self.mode.key().into());
        map.insert("fxaaLevel".into(), self.fxaa.into());
        map.insert("msaaSamples".into(), self.msaa.into());
        map.insert("ssaaLevel".into(), self.ssaa.into());
    }
}

/// The MSAA sample counts this GPU can draw the scene with, as bits
/// (`1 << n`); all of them until the renderer has checked.
static SUPPORTED: AtomicU8 = AtomicU8::new(u8::MAX);

fn supports(samples: u8) -> bool {
    samples <= 1 || SUPPORTED.load(Ordering::Relaxed) & (1 << samples.trailing_zeros()) != 0
}

/// The formats a scene camera's targets can have: its colour (plain or
/// HDR) and its depth.
const SCENE_FORMATS: [TextureFormat; 3] = [
    TextureFormat::Rgba8UnormSrgb,
    TextureFormat::Rgba16Float,
    bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
];

/// Whether `SUPPORTED` has been filled in, once per process.
static CHECKED: AtomicBool = AtomicBool::new(false);

fn check_supported(adapter: Option<Res<RenderAdapter>>) {
    if CHECKED.load(Ordering::Relaxed) {
        return;
    }
    let Some(adapter) = adapter else { return };
    if CHECKED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut bits = 1u8 << 0;
    for samples in MSAA_SAMPLES {
        let all = SCENE_FORMATS.iter().all(|&format| {
            adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(samples as u32)
        });
        if all {
            bits |= 1 << samples.trailing_zeros();
        }
    }
    SUPPORTED.store(bits, Ordering::Relaxed);
    crate::log_info!(
        "render",
        "MSAA sample counts supported: {}",
        MSAA_SAMPLES
            .into_iter()
            .filter(|&n| supports(n))
            .map(|n| format!("{n}x"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Puts the chosen MSAA on every scene camera (they share one target, so
/// they must agree), and FXAA on the hand camera, the last of them, so it
/// smooths the world and the hand once.
fn apply_antialiasing(
    mut commands: Commands,
    state: Res<crate::gui::GuiState>,
    scene: Query<
        (Entity, &Msaa, Has<HandCamera>, Option<&Fxaa>),
        Or<(With<WorldCamera>, With<HandCamera>, With<PanoramaCamera>)>,
    >,
) {
    let aa = state.options.antialiasing;
    #[cfg(feature = "shader_support")]
    let pack = crate::renderer::packvertex::active();
    #[cfg(not(feature = "shader_support"))]
    let pack = false;

    let msaa = match (aa.mode, pack) {
        (AaMode::Msaa, false) => match aa.msaa_samples() {
            2 => Msaa::Sample2,
            4 => Msaa::Sample4,
            8 => Msaa::Sample8,
            _ => Msaa::Off,
        },
        _ => Msaa::Off,
    };
    let fxaa = (aa.mode == AaMode::Fxaa).then(|| {
        let level = FXAA_LEVELS[aa.fxaa.min(FXAA_LEVELS.len() - 1)].1;
        Fxaa {
            enabled: true,
            edge_threshold: level,
            edge_threshold_min: level,
        }
    });

    for (entity, current, is_hand, current_fxaa) in &scene {
        if *current != msaa {
            commands.entity(entity).insert(msaa);
        }
        if !is_hand {
            continue;
        }
        let same = match (current_fxaa, &fxaa) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.enabled == b.enabled
                    && a.edge_threshold == b.edge_threshold
                    && a.edge_threshold_min == b.edge_threshold_min
            }
            _ => false,
        };
        if !same {
            match fxaa.clone() {
                Some(fxaa) => commands.entity(entity).insert(fxaa),
                None => commands.entity(entity).remove::<Fxaa>(),
            };
        }
    }
}

pub struct AntialiasPlugin;

impl Plugin for AntialiasPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (check_supported, apply_antialiasing).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_switch_reads_as_supersampling() {
        let aa = Antialiasing::read(|k| (k == "antialiasing").then(|| serde_json::json!(true)));
        assert_eq!(aa.mode, AaMode::Supersample);
        assert_eq!(aa.supersample(), Some(2.0));
        let aa = Antialiasing::read(|k| (k == "antialiasing").then(|| serde_json::json!(false)));
        assert_eq!(aa.mode, AaMode::Off);
    }

    #[test]
    fn settings_round_trip() {
        let aa = Antialiasing {
            mode: AaMode::Msaa,
            fxaa: 4,
            msaa: 8,
            ssaa: 0,
        };
        let mut map = serde_json::Map::new();
        aa.write(&mut map);
        assert_eq!(Antialiasing::read(|k| map.get(k).cloned()), aa);
    }

    #[test]
    fn levels_wrap() {
        let mut aa = Antialiasing {
            mode: AaMode::Fxaa,
            fxaa: FXAA_LEVELS.len() - 1,
            ..Antialiasing::default()
        };
        aa.next_level();
        assert_eq!(aa.fxaa, 0);
        aa.mode = AaMode::Msaa;
        aa.msaa = 8;
        aa.next_level();
        assert_eq!(aa.msaa, 2);
    }
}
