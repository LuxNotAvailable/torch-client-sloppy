//! This frame's views, as plain data: the world camera, fog, the hand and
//! inventory figure cameras, and the screen warp. The game's camera systems
//! fill it (in `ViewSystems`, after transforms propagate); renderers read
//! it instead of the cameras.

use bevy::math::{Mat4, Vec3};
use bevy::prelude::{Resource, SystemSet};

/// The render layer the first-person hand draws on.
pub const HAND_LAYER: usize = 2;
/// The render layer the inventory's player figure draws on.
pub const FIGURE_LAYER: usize = 4;

/// Fills `View`; readers run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSystems;

#[derive(Resource, Clone, Default)]
pub struct View {
    /// A menu shows: the title panorama draws instead of the world.
    pub menu: bool,
    pub world: Option<Eye>,
    pub fog: Option<Fog>,
    /// The hand camera's projection.
    pub hand_clip_from_view: Option<Mat4>,
    /// The hand camera's view matrix.
    pub hand_view_from_world: Option<Mat4>,
    /// The figure's texture holds this frame's figure.
    pub figure_ready: bool,
    /// Set while the figure is ready.
    pub figure: Option<Figure>,
    /// The portal and nausea warp on the world's projection.
    pub warp: Mat4,
}

/// The world camera. It looks down its -z.
#[derive(Clone, Copy, Debug)]
pub struct Eye {
    pub world_from_view: Mat4,
    /// Vertical, in radians.
    pub fov: f32,
}

impl Eye {
    pub fn position(&self) -> Vec3 {
        self.world_from_view.w_axis.truncate()
    }

    pub fn forward(&self) -> Vec3 {
        -self.world_from_view.z_axis.truncate().normalize_or_zero()
    }
}

/// The world's distance fog, linear.
#[derive(Clone, Copy, Debug)]
pub struct Fog {
    /// Linear RGBA the frame clears to.
    pub clear: [f32; 4],
    /// Linear RGBA.
    pub color: [f32; 4],
    /// Red and green of the directional light colour (pack env channels).
    pub env: [f32; 2],
    pub start: f32,
    pub end: f32,
}

/// The inventory figure's orthographic camera.
#[derive(Clone, Copy, Debug)]
pub struct Figure {
    /// Depth reversed, as the world's.
    pub clip_from_world: Mat4,
    pub eye: Vec3,
    /// The texture size the GUI wants, in pixels.
    pub size: [u32; 2],
}
