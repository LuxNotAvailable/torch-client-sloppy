//! The enchantment shimmer, drawn additively over an item mesh and masked by
//! that item's own texture.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct GlintMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub mask: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    pub glint: Handle<Image>,
}

impl Material for GlintMaterial {
    fn fragment_shader() -> ShaderRef {
        super::embedded_shader("renderer/glint.wgsl")
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn depth_bias(&self) -> f32 {
        1.0
    }
}

pub struct GlintPlugin;

impl Plugin for GlintPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "glint.wgsl");
        app.add_plugins(MaterialPlugin::<GlintMaterial>::default())
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
                    glint: super::entity_material::GLINT_ITEM,
                })
            })
            .clone()
    }
}
