//! Bevy/wgpu renderer (`MC_RENDERER=wgpu`): terrain, entities, sky, weather,
//! post, shader-pack draw (`packdraw`).

pub mod anim;
pub mod antialias;
pub(crate) mod atlas;
pub(crate) mod block_anim;
pub mod clouds;
mod entity_draw;
pub mod entity_material;
pub mod environment;
pub mod esp_material;
pub mod frame_view;
pub mod glint;
pub mod hand;
pub mod input;
pub mod intent;
pub mod inventory_figure;
pub mod item_assets;
pub mod maps;
pub mod occlusion;
pub mod overlays;
#[cfg(feature = "shader_support")]
pub mod packdraw;
pub mod panorama;
pub mod player_model;
pub mod post;
pub mod post_effects;
pub mod rig_parts;
pub mod screen_effects;
pub(crate) mod screenshot;
pub mod shadows;
pub mod skin;
pub mod sky;
pub mod ssaa;
pub mod systems;
pub mod terrain;
pub mod terrain_pool;
pub mod tethers;
pub mod view_bob;
mod view_sync;
pub mod weather;
pub mod world_text;

pub use anim::{AnimInput, Armor, HeldItem, HumanoidAnim};
pub use atlas::{Textures, build_block_atlas, placeholder_atlas, texture_is_opaque};
pub use frame_view::{FrameView, FrameViewSystems};

use crate::terrain::ATLAS_COLS;
pub use systems::{Shared, run};

pub fn embedded_path(path: &str) -> String {
    let crate_name = module_path!().split(':').next().unwrap_or("torch_client");
    format!("embedded://{crate_name}/{path}")
}

pub fn embedded_shader(path: &str) -> bevy::shader::ShaderRef {
    bevy::shader::ShaderRef::Path(embedded_path(path).into())
}

pub fn atlas_rows(atlas: &bevy::image::Image) -> u32 {
    (atlas.height() * ATLAS_COLS / atlas.width().max(1)).max(1)
}

pub(crate) fn rgba_image(
    width: u32,
    height: u32,
    data: Vec<u8>,
    usage: bevy::asset::RenderAssetUsages,
) -> bevy::image::Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let mut image = bevy::image::Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        usage,
    );
    image.sampler = bevy::image::ImageSampler::nearest();
    image
}

#[derive(bevy::prelude::States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Menu,
    InGame,
}
