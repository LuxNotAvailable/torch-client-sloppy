//! The game's scene as Bevy entities: cameras, entity and hand models, sky,
//! clouds, weather, overlays, world text. It fills the `draw` lists and
//! `client::view::View` that `vk` draws from.

pub mod anim;
pub(crate) mod atlas;
pub mod clouds;
mod entity_draw;
pub mod entity_material;
pub mod environment;
pub mod glint;
pub mod hand;
pub mod inventory_figure;
pub mod item_assets;
pub mod maps;
pub mod occlusion;
pub mod overlays;
#[cfg(feature = "shader_support")]
pub(crate) mod pack_inputs;
pub mod panorama;
pub mod player_model;
pub mod rig_parts;
pub mod screen_effects;
pub mod shadows;
pub mod skin;
pub mod sky;
pub mod tethers;
pub mod view_bob;
pub(crate) mod view_sync;
pub mod weather;
pub mod world_text;

pub use anim::{AnimInput, Armor, HeldItem, HumanoidAnim};
pub use atlas::{Textures, build_block_atlas, placeholder_atlas, texture_is_opaque};

use crate::terrain::ATLAS_COLS;

pub fn atlas_rows(atlas: &bevy::image::Image) -> u32 {
    (atlas.height() * ATLAS_COLS / atlas.width().max(1)).max(1)
}

pub(crate) fn rgba_image(
    width: u32,
    height: u32,
    data: Vec<u8>,
    usage: bevy::asset::RenderAssetUsages,
) -> bevy::image::Image {
    use wgpu_types::{Extent3d, TextureDimension, TextureFormat};

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
