macro_rules! gpu_label {
    ($s:literal) => {
        concat!("shaderpack ", $s)
    };
}

pub(crate) mod center_depth;
pub(crate) mod compile;
mod depthcopy;
pub(crate) mod endflash;
mod entities;
mod formats;
mod frame;
mod fullscreen;
mod images;
pub(crate) mod inputs;
pub(crate) mod install;
mod mipmaps;
mod node;
mod packatlas;
mod pipelines;
mod plan;
pub(crate) mod sources;
mod targets;
pub(crate) mod uniforms;
pub(crate) mod usage;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use bevy::core_pipeline::core_3d::graph::{Core3d, Node3d};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_graph::{RenderGraphExt, ViewNodeRunner};
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::RenderDevice;
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderSystems};

use crate::renderer::terrain_pool::draw::PackTakeover;

pub(crate) const GPU_LABEL: &str = "shaderpack";

static CURRENT_GENERATION: AtomicU32 = AtomicU32::new(0);

fn set_current_generation(generation: u32) {
    CURRENT_GENERATION.store(generation, Ordering::Release);
}

pub(super) fn pipeline_label(generation: u32, rest: std::fmt::Arguments) -> String {
    format!("{GPU_LABEL}#{generation} {rest}")
}

fn labelled_generation(message: &str) -> Option<u32> {
    let digits = |s: &str| -> Option<u32> {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        s[..end].parse().ok()
    };
    if let Some(at) = message.find(&format!("{GPU_LABEL}#")) {
        return digits(&message[at + GPU_LABEL.len() + 1..]);
    }
    let at = message.find(&format!("{GPU_LABEL}/"))?;
    digits(message[at + GPU_LABEL.len() + 1..].split('/').nth(1)?)
}

static GPU_FAILED: AtomicBool = AtomicBool::new(false);

static GPU_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn install_error_handler(device: &RenderDevice) {
    device
        .wgpu_device()
        .on_uncaptured_error(Arc::new(|error: wgpu::Error| {
            let message = error.to_string();
            if message.contains(GPU_LABEL)
                && let Some(generation) = labelled_generation(&message)
                && generation != CURRENT_GENERATION.load(Ordering::Acquire)
            {
                crate::log_warn!(
                    "shaders",
                    "a superseded shader pack's pipeline failed: {message}"
                );
                return;
            }
            if message.contains(GPU_LABEL) || GPU_FAILED.load(Ordering::Acquire) {
                GPU_FAILED.store(true, Ordering::Release);
                if let Ok(mut slot) = GPU_ERROR.lock() {
                    slot.get_or_insert(message);
                }
                return;
            }
            crate::log_error!("render", "Handling wgpu errors as fatal by default");
            panic!("wgpu error: {message}\n");
        }));
}

fn timing_requested() -> bool {
    crate::platform::env::var_os("MC_SHADERPACK_TIMING").is_some_and(|v| v != "0")
}

const TIMED_STAGES: [&str; 11] = [
    "clears",
    "begin",
    "shadow",
    "prepare",
    "sky",
    "opaque",
    "entities",
    "deferred",
    "translucent",
    "composite",
    "final",
];

fn report_pack_timing(
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    inputs: Option<Res<inputs::Inputs>>,
    mut last: Local<Option<std::time::Instant>>,
) {
    let now = std::time::Instant::now();
    if last.is_some_and(|at| now.duration_since(at) < std::time::Duration::from_secs(1)) {
        return;
    }
    *last = Some(now);
    let read = |stage: &str, field: &str| {
        let tail = format!("shaderpack/{stage}/{field}");
        diagnostics
            .iter()
            .find(|d| d.path().as_str().ends_with(&tail))
            .and_then(|d| d.smoothed())
    };
    let gpu = TIMED_STAGES
        .iter()
        .any(|stage| read(stage, "elapsed_gpu").is_some());
    let field = if gpu { "elapsed_gpu" } else { "elapsed_cpu" };
    let stages: Vec<(&str, f64)> = TIMED_STAGES
        .iter()
        .filter_map(|stage| Some((*stage, read(stage, field)?)))
        .collect();
    if stages.is_empty() {
        return;
    }
    let total: f64 = stages.iter().map(|(_, ms)| ms).sum();
    let line = stages
        .iter()
        .map(|(stage, ms)| format!("{stage} {ms:.2}"))
        .collect::<Vec<_>>()
        .join("  ");
    crate::log_info!(
        "shaders",
        "pack frame {} {total:.2} ms: {line}",
        if gpu {
            "gpu"
        } else {
            "cpu (no timestamp queries)"
        }
    );
    if let Some(inputs) = inputs {
        crate::log_info!(
            "shaders",
            "pack eye: isEyeInWater {}, eyeBrightness {:?}, fog {:?}",
            inputs.eye_in as i32,
            (inputs.eye_light * 16).to_array(),
            inputs.fog_color.to_array()
        );
    }
}

fn drop_failed_pack(
    takeover: Option<ResMut<PackTakeover>>,
    render: Option<ResMut<frame::PackRender>>,
) {
    if !GPU_FAILED.load(Ordering::Acquire) {
        return;
    }
    let (Some(mut takeover), Some(mut render)) = (takeover, render) else {
        return;
    };
    let generation = render.generation();
    if generation != 0 {
        render.reject_install();
        takeover.0 = false;
        let message = GPU_ERROR
            .lock()
            .ok()
            .and_then(|mut e| e.take())
            .unwrap_or_default();
        crate::log_warn!(
            "shaders",
            "the GPU rejected the shader pack, drawing the built-in terrain: {message}"
        );
    }
    GPU_FAILED.store(false, Ordering::Release);
}

/// The shader pack's render scale in percent, from the options; the render
/// world reads it when it sizes the pack's targets.
static RENDER_SCALE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(crate::gui::PACK_RENDER_SCALE_MAX);

fn sync_render_scale(gui: Res<crate::gui::GuiState>) {
    RENDER_SCALE.store(gui.options.pack_render_scale, Ordering::Relaxed);
    SHADOW_EVERY.store(gui.options.shadow_update.clamp(1, 3), Ordering::Relaxed);
}

/// Shadow Updates (Video Settings): the shadow map is redrawn every this
/// many frames.
static SHADOW_EVERY: AtomicU32 = AtomicU32::new(1);

pub(crate) fn shadow_every() -> u32 {
    SHADOW_EVERY.load(Ordering::Relaxed)
}

/// The pack's terrain projection and drawing size this frame, for the
/// terrain cull's occlusion pyramid built from the pack's depth.
static HIZ_VIEW: std::sync::Mutex<Option<(Mat4, UVec2)>> = std::sync::Mutex::new(None);

pub(super) fn publish_hiz_view(projection: Mat4, size: UVec2) {
    if let Ok(mut view) = HIZ_VIEW.lock() {
        *view = Some((projection, size));
    }
}

pub(crate) fn hiz_view() -> Option<(Mat4, UVec2)> {
    HIZ_VIEW.lock().ok().and_then(|view| *view)
}

/// The size a shader pack draws at for a view of `size`.
pub(super) fn scaled(size: UVec2) -> UVec2 {
    let percent = RENDER_SCALE.load(Ordering::Relaxed).clamp(
        crate::gui::PACK_RENDER_SCALE_MIN,
        crate::gui::PACK_RENDER_SCALE_MAX,
    );
    scaled_by(size, percent)
}

fn scaled_by(size: UVec2, percent: u32) -> UVec2 {
    ((size.max(UVec2::ONE) * percent + 50) / 100).max(UVec2::ONE)
}

pub(crate) fn allow_display_view(image: &mut Image) {
    if image.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb {
        image.texture_descriptor.view_formats = &[TextureFormat::Rgba8Unorm];
    }
}

pub(crate) struct ShaderPackPlugin;

impl Plugin for ShaderPackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<inputs::Inputs>().add_systems(
            Update,
            (
                (install::sync_compiled_pack, install::sync_pack_vertices).chain(),
                inputs::gather_inputs
                    .after(crate::renderer::frame_view::FrameViewSystems)
                    .run_if(resource_exists::<install::CompiledPack>),
                sync_render_scale,
            ),
        );
        if timing_requested() {
            // The frame profiler already records the render diagnostics.
            app.add_systems(Update, report_pack_timing);
        }
        app.add_plugins(bevy::render::extract_component::ExtractComponentPlugin::<
            crate::renderer::hand::HandCamera,
        >::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(
                ExtractSchedule,
                (
                    frame::extract_pack,
                    entities::extract_entity_draws,
                    entities::extract_standard_draws,
                )
                    .chain(),
            )
            .add_systems(
                Render,
                (
                    (
                        drop_failed_pack,
                        pipelines::prepare_pipelines,
                        pipelines::prepare_blit,
                        frame::take_over_view,
                    )
                        .chain()
                        .in_set(RenderSystems::PrepareAssets),
                    frame::prepare_uniforms
                        .in_set(RenderSystems::PrepareResources)
                        .before(crate::renderer::terrain_pool::cull::prepare_cull),
                    plan::prepare_plan.in_set(RenderSystems::PrepareBindGroups),
                    entities::prepare_entity_draws
                        .in_set(RenderSystems::PrepareBindGroups)
                        .after(plan::prepare_plan),
                    center_depth::read_back.in_set(RenderSystems::Cleanup),
                ),
            )
            .add_render_graph_node::<ViewNodeRunner<node::PackFrameNode>>(
                Core3d,
                node::PackFrameLabel,
            )
            .add_render_graph_edges(
                Core3d,
                (
                    Node3d::EndMainPass,
                    node::PackFrameLabel,
                    Node3d::StartMainPassPostProcessing,
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        install_error_handler(render_app.world().resource::<RenderDevice>());
        // Debug Options > Culling View draws over the pack's finished frame
        // (both nodes exist by now, whichever plugin added them first).
        render_app.add_render_graph_edge(
            Core3d,
            node::PackFrameLabel,
            crate::renderer::terrain_pool::hiz::HiZViewLabel,
        );
        render_app.init_resource::<center_depth::CenterDepth>();
        formats::init_formats(
            render_app
                .world()
                .resource::<bevy::render::renderer::RenderAdapter>(),
        );
        render_app.insert_resource(frame::PackRender::new());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_render_scale_rounds_and_never_reaches_zero() {
        use bevy::math::UVec2;
        let window = UVec2::new(1920, 974);
        assert_eq!(super::scaled_by(window, 100), window);
        assert_eq!(super::scaled_by(window, 50), UVec2::new(960, 487));
        assert_eq!(super::scaled_by(window, 75), UVec2::new(1440, 731));
        assert_eq!(super::scaled_by(UVec2::new(1, 1), 50), UVec2::ONE);
        assert_eq!(super::scaled_by(UVec2::ZERO, 50), UVec2::ONE);
    }

    use super::*;

    #[test]
    fn errors_name_their_install() {
        assert_eq!(
            labelled_generation("In create_render_pipeline, label = 'shaderpack#12 composite'"),
            Some(12)
        );
        assert_eq!(
            labelled_generation("ShaderModule 'shaderpack/Solas V3/7/final.fsh.wgsl' invalid"),
            Some(7)
        );
        assert_eq!(
            labelled_generation("bind group 'shaderpack composite' invalid"),
            None
        );
    }
}
