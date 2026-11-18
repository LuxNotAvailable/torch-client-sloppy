//! Terrain draw inputs the renderer reads: the block atlas, the water
//! clarity scale, and the lightmap's pixels.

use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug)]
pub struct TerrainParams {
    /// Scales the water surface's alpha (the water clarity setting).
    pub water_alpha: f32,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self { water_alpha: 1.0 }
    }
}

#[derive(Resource, Clone)]
pub struct TerrainTextures {
    pub atlas: Handle<Image>,
}

/// The 16×16 lightmap (columns block light, rows sky light) as RGBA8,
/// rewritten every frame.
#[derive(Resource, Clone, Default)]
pub struct LightmapPixels {
    pub data: Vec<u8>,
}
