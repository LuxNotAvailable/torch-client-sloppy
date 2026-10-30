use bevy::asset::embedded_asset;
use bevy::image::ImageSampler;
use bevy::prelude::*;

pub const SHADOW_ALPHA_MIP: f32 = 1.0;

const UNUSED_SHADOW_MAP_SIZE: usize = 1;

pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "terrain.wgsl");

        app.insert_resource(bevy::light::PointLightShadowMap {
            size: UNUSED_SHADOW_MAP_SIZE,
        });
        app.insert_resource(bevy::light::DirectionalLightShadowMap {
            size: UNUSED_SHADOW_MAP_SIZE,
        });

        app.add_systems(Update, super::block_anim::animate);
        app.add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
            LightmapPixels,
        >::default());
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(
                bevy::render::Render,
                upload_lightmap.in_set(bevy::render::RenderSystems::PrepareResources),
            );
        }
    }
}

#[derive(Resource, Clone, bevy::render::extract_resource::ExtractResource)]
pub struct LightmapPixels {
    pub image: Handle<Image>,
    pub data: Vec<u8>,
}

const LIGHTMAP_SIZE: u32 = 16;
const LIGHTMAP_BYTES: usize = (LIGHTMAP_SIZE * LIGHTMAP_SIZE * 4) as usize;

fn upload_lightmap(
    pixels: Option<Res<LightmapPixels>>,
    images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    queue: Res<bevy::render::renderer::RenderQueue>,
) {
    use bevy::render::render_resource::{
        Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    };
    let Some(pixels) = pixels else { return };
    if pixels.data.len() != LIGHTMAP_BYTES {
        return;
    }
    let Some(gpu) = images.get(&pixels.image) else {
        return;
    };
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &gpu.texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &pixels.data,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(LIGHTMAP_SIZE * 4),
            rows_per_image: Some(LIGHTMAP_SIZE),
        },
        Extent3d {
            width: LIGHTMAP_SIZE,
            height: LIGHTMAP_SIZE,
            depth_or_array_layers: 1,
        },
    );
}

pub fn new_lightmap_image() -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut image = Image::new_fill(
        Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::linear();
    #[cfg(feature = "shader_support")]
    super::packdraw::allow_display_view(&mut image);
    image
}
