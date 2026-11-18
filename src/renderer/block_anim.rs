//! The wgpu side of animated block textures: the tile table image, rewritten
//! once a tick. The animation data lives in `terrain::block_anim`.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::terrain_pool::TerrainTextures;
use crate::terrain::block_anim::{ROW, TICKS_PER_SECOND, current};

pub(crate) fn table_image() -> Image {
    identity_table(1)
}

fn identity_table(tile_count: u32) -> Image {
    let rows = tile_count.div_ceil(ROW).max(1);
    let mut data = Vec::with_capacity((ROW * rows * 8) as usize);
    for tile in 0..ROW * rows {
        let tile = tile.min(u32::from(u16::MAX)) as u16;
        for value in [tile, tile, 0, 0] {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    Image::new(
        Extent3d {
            width: ROW,
            height: rows,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Uint,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

#[derive(Default)]
pub(crate) struct Clock {
    generation: u64,
    tick: u64,
}

pub(crate) fn animate(
    time: Res<Time>,
    textures: Option<Res<TerrainTextures>>,
    mut images: ResMut<Assets<Image>>,
    mut clock: Local<Clock>,
) {
    let (Some(textures), Some(set)) = (textures, current()) else {
        return;
    };
    let tick = (time.elapsed_secs_f64() * TICKS_PER_SECOND) as u64;
    let rebuilt = clock.generation != set.generation;
    if !rebuilt && (tick == clock.tick || set.animations.is_empty()) {
        return;
    }
    clock.tick = tick;
    let Some(image) = images.get_mut(&textures.anim) else {
        return;
    };
    if rebuilt {
        *image = identity_table(set.tile_count);
        clock.generation = set.generation;
    }
    let Some(data) = image.data.as_mut() else {
        return;
    };
    for animation in &set.animations {
        let (a, b, blend) = animation.at(tick);
        let at = usize::from(animation.base) * 8;
        let Some(entry) = data.get_mut(at..at + 8) else {
            continue;
        };
        entry[0..2].copy_from_slice(&a.to_le_bytes());
        entry[2..4].copy_from_slice(&b.to_le_bytes());
        entry[4..6].copy_from_slice(&blend.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[cfg(feature = "shader_support")]
    #[test]
    fn the_shader_lookup_validates() {
        let shader = include_str!("terrain.wgsl");
        let start = shader
            .find("@group(2) @binding(6)")
            .expect("the table binding");
        let end = shader[start..]
            .find("fn section_origin")
            .expect("the lookup ends")
            + start;
        let wgsl = format!(
            "@group(2) @binding(0) var atlas_texture: texture_2d<f32>;
             @group(2) @binding(1) var atlas_sampler: sampler;
{}
             @fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{
             return sample_atlas(uv, 0.0);
}}
",
            &shader[start..end]
        );
        let module = naga::front::wgsl::parse_str(&wgsl).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }

    #[test]
    fn the_vanilla_atlas_animates_water_lava_and_fire() {
        let dir = crate::textures_dir().to_string_lossy().into_owned();
        let textures = crate::renderer::Textures::blocks(&dir);
        let (image, map) = crate::renderer::build_block_atlas(&textures, &dir);
        let set = current().expect("the atlas published its animations");
        assert!(
            map.keys().all(|k| !k.contains('#')),
            "frame tiles stay out of the texture map"
        );
        for stem in ["water_still", "lava_flow", "fire_0", "sea_lantern"] {
            let base = map[stem] as u16;
            let anim = set
                .animations
                .iter()
                .find(|a| a.base == base)
                .unwrap_or_else(|| panic!("{stem} is not animated"));
            assert!(anim.frames.len() > 1);
            assert!(
                anim.frames
                    .iter()
                    .all(|&(t, _)| u32::from(t) < set.tile_count)
            );
        }
        let rows = crate::renderer::atlas_rows(&image);
        assert!(set.tile_count <= rows * crate::terrain::ATLAS_COLS);
    }

    #[test]
    fn the_table_starts_as_identity() {
        let image = identity_table(300);
        assert_eq!(image.texture_descriptor.size.height, 2);
        let data = image.data.unwrap();
        let entry = |t: usize| u16::from_le_bytes([data[t * 8], data[t * 8 + 1]]);
        assert_eq!(entry(0), 0);
        assert_eq!(entry(299), 299);
    }
}
