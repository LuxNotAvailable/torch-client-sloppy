//! Materials as plain data: what a model draws with, read by the draw-list
//! collectors. No GPU bindings; the native renderer builds its own.

use bevy::asset::{Asset, Handle};
use bevy::color::Color;
use bevy::image::Image;
use bevy::prelude::{Component, TypePath};

/// How a surface's alpha applies.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Alpha {
    #[default]
    Opaque,
    /// Cut out below the threshold.
    Mask(f32),
    Blend,
    Add,
}

impl Alpha {
    /// As `ALPHA_*`.
    pub fn code(self) -> u32 {
        match self {
            Alpha::Opaque => super::ALPHA_OPAQUE,
            Alpha::Mask(_) => super::ALPHA_CUTOUT,
            Alpha::Blend => super::ALPHA_BLEND,
            Alpha::Add => super::ALPHA_ADD,
        }
    }
}

/// The material an entity's mesh draws with.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Material3d<M: Asset>(pub Handle<M>);

impl<M: Asset> Default for Material3d<M> {
    fn default() -> Self {
        Material3d(Handle::default())
    }
}

/// An unlit textured material: the hand, held items, overlays, particles.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct SimpleMaterial {
    /// sRGB.
    pub base_color: Color,
    pub base_color_texture: Option<Handle<Image>>,
    pub alpha_mode: Alpha,
}

impl Default for SimpleMaterial {
    fn default() -> Self {
        SimpleMaterial {
            base_color: Color::WHITE,
            base_color_texture: None,
            alpha_mode: Alpha::Opaque,
        }
    }
}
