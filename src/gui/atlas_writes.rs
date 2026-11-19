use bevy::prelude::*;

use super::atlas::Region;

#[derive(Clone)]
pub struct AtlasWrite {
    pub region: Region,
    pub pixels: std::sync::Arc<Vec<u8>>,
}

#[derive(Resource, Clone, Default)]
pub struct AtlasWrites {
    pub image: Option<Handle<Image>>,
    pub pending: Vec<AtlasWrite>,
}

impl AtlasWrites {
    pub fn push(&mut self, region: Region, pixels: impl Into<std::sync::Arc<Vec<u8>>>) {
        let pixels = pixels.into();
        if pixels.len() as u32 == region.w * region.h * 4 {
            self.pending.push(AtlasWrite { region, pixels });
        }
    }
}
