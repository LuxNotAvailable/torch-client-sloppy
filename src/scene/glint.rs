//! The enchantment shimmer, drawn additively over an item mesh and masked by
//! that item's own texture.

use bevy::prelude::*;

#[derive(Asset, TypePath, Clone, Debug)]
pub struct GlintMaterial {
    /// The item's texture, whose alpha masks the shimmer.
    pub mask: Handle<Image>,
}

pub struct GlintPlugin;

impl Plugin for GlintPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<GlintMaterial>()
            .init_resource::<GlintMaterials>();
    }
}

/// One glint material per item texture, built the first time that item glints.
#[derive(Resource, Default)]
pub struct GlintMaterials(std::collections::HashMap<AssetId<Image>, Handle<GlintMaterial>>);

impl GlintMaterials {
    pub fn for_texture(
        &mut self,
        texture: &Handle<Image>,
        materials: &mut Assets<GlintMaterial>,
    ) -> Handle<GlintMaterial> {
        self.0
            .entry(texture.id())
            .or_insert_with(|| {
                materials.add(GlintMaterial {
                    mask: texture.clone(),
                })
            })
            .clone()
    }
}
