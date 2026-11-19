//! Pack state shared between the renderers and the uniforms, set from the
//! options and the GPU: render scale, shadow update rate, and the centre
//! depth read back.

use std::sync::atomic::{AtomicU32, Ordering};

use bevy::math::UVec2;

/// The shader pack's render scale in percent, from the options; the render
/// world reads it when it sizes the pack's targets.
static RENDER_SCALE: AtomicU32 = AtomicU32::new(crate::gui::PACK_RENDER_SCALE_MAX);

/// Shadow Updates (Video Settings): the shadow map is redrawn every this
/// many frames.
static SHADOW_EVERY: AtomicU32 = AtomicU32::new(1);

/// Stores the Render Scale and Shadow Updates options.
pub(crate) fn set_options(render_scale: u32, shadow_every: u32) {
    RENDER_SCALE.store(render_scale, Ordering::Relaxed);
    SHADOW_EVERY.store(shadow_every.clamp(1, 3), Ordering::Relaxed);
}

pub(crate) fn shadow_every() -> u32 {
    SHADOW_EVERY.load(Ordering::Relaxed)
}

/// The size a shader pack draws at for a view of `size`.
pub(crate) fn scaled(size: UVec2) -> UVec2 {
    let percent = RENDER_SCALE.load(Ordering::Relaxed).clamp(
        crate::gui::PACK_RENDER_SCALE_MIN,
        crate::gui::PACK_RENDER_SCALE_MAX,
    );
    scaled_by(size, percent)
}

fn scaled_by(size: UVec2, percent: u32) -> UVec2 {
    ((size.max(UVec2::ONE) * percent + 50) / 100).max(UVec2::ONE)
}

/// The latest read-back centre depth, as `f32` bits; the far plane until
/// one lands.
static CENTER_DEPTH: AtomicU32 = AtomicU32::new(0x3F80_0000);

pub(crate) fn center_depth() -> f32 {
    f32::from_bits(CENTER_DEPTH.load(Ordering::Relaxed))
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
}
