//! Applies the Antialiasing option (`gui::antialias`) to the wgpu cameras.
//!
//! MSAA and supersampling stay off while a shader pack draws the world: its
//! passes are single-sampled and size their own targets. FXAA still runs,
//! after the pack, on the last scene camera.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::anti_alias::fxaa::{Fxaa, Sensitivity};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::RenderAdapter;

use super::hand::HandCamera;
use super::panorama::PanoramaCamera;
use super::systems::WorldCamera;
use crate::gui::antialias::{AaMode, MSAA_SAMPLES, set_supported, supports};

/// `gui::antialias::FXAA_LEVELS` as Bevy's sensitivities.
const FXAA_SENSITIVITY: [Sensitivity; 5] = [
    Sensitivity::Low,
    Sensitivity::Medium,
    Sensitivity::High,
    Sensitivity::Ultra,
    Sensitivity::Extreme,
];

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
    set_supported(bits);
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
    let pack = crate::shaderpack::packvertex::active();
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
        let level = FXAA_SENSITIVITY[aa.fxaa.min(FXAA_SENSITIVITY.len() - 1)];
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
