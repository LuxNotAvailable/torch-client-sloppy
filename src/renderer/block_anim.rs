//! Animated block textures.
//!
//! Every frame is its own atlas tile. Meshes only ever name a texture's first
//! tile, and a small `tile -> (frame, next frame, blend)` table, rewritten once
//! a tick, tells the terrain shader which tile to sample instead. Nothing in
//! the atlas moves after it is built.

use std::sync::{Arc, RwLock};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::terrain_pool::TerrainTextures;

/// Table row width; the shader indexes it the same way.
pub(crate) const ROW: u32 = 256;

const TICKS_PER_SECOND: f64 = 20.0;

pub(crate) struct BlockAnimation {
    /// The tile meshes reference.
    pub base: u16,
    /// Atlas tile and how many ticks it shows, in play order.
    pub frames: Box<[(u16, u32)]>,
    pub interpolate: bool,
}

impl BlockAnimation {
    fn period(&self) -> u64 {
        self.frames
            .iter()
            .map(|&(_, t)| u64::from(t))
            .sum::<u64>()
            .max(1)
    }

    /// `(frame tile, next frame tile, blend 0..=u16::MAX)` at `tick`.
    fn at(&self, tick: u64) -> (u16, u16, u16) {
        let mut into = tick % self.period();
        for (i, &(tile, time)) in self.frames.iter().enumerate() {
            let time = u64::from(time.max(1));
            if into < time {
                let next = self.frames[(i + 1) % self.frames.len()].0;
                let blend = if self.interpolate {
                    (into * u64::from(u16::MAX) / time) as u16
                } else {
                    0
                };
                return (tile, next, blend);
            }
            into -= time;
        }
        (self.frames[0].0, self.frames[0].0, 0)
    }
}

pub(crate) struct AnimationSet {
    generation: u64,
    tile_count: u32,
    animations: Vec<BlockAnimation>,
}

static SET: RwLock<Option<Arc<AnimationSet>>> = RwLock::new(None);

/// Called by the atlas builder; the table follows on the next frame.
pub(crate) fn publish(tile_count: u32, animations: Vec<BlockAnimation>) {
    if let Ok(mut slot) = SET.write() {
        let generation = slot.as_ref().map_or(1, |s| s.generation + 1);
        *slot = Some(Arc::new(AnimationSet {
            generation,
            tile_count,
            animations,
        }));
    }
}

fn current() -> Option<Arc<AnimationSet>> {
    SET.read().ok()?.clone()
}

/// `animation` from a texture's `.mcmeta`, resolved against `frame_count`
/// frames in the strip: `(frame index, ticks)` in play order.
pub(crate) struct Meta {
    pub frames: Vec<(u32, u32)>,
    pub interpolate: bool,
}

pub(crate) fn read_meta(png: &std::path::Path, frame_count: u32) -> Option<Meta> {
    let text = crate::platform::assets::read_to_string(png.with_extension("png.mcmeta"))?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let anim = json.get("animation")?;
    let default_time = anim
        .get("frametime")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1)
        .max(1) as u32;
    let interpolate = anim
        .get("interpolate")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let frames: Vec<(u32, u32)> = match anim.get("frames").and_then(serde_json::Value::as_array) {
        Some(list) => list
            .iter()
            .filter_map(|f| match f {
                serde_json::Value::Number(n) => Some((n.as_u64()? as u32, default_time)),
                serde_json::Value::Object(o) => Some((
                    o.get("index")?.as_u64()? as u32,
                    o.get("time")
                        .and_then(serde_json::Value::as_u64)
                        .map_or(default_time, |t| t.max(1) as u32),
                )),
                _ => None,
            })
            .filter(|&(index, _)| index < frame_count)
            .collect(),
        None => (0..frame_count).map(|i| (i, default_time)).collect(),
    };
    (frames.len() > 1).then_some(Meta {
        frames,
        interpolate,
    })
}

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

    fn anim(frames: &[(u16, u32)], interpolate: bool) -> BlockAnimation {
        BlockAnimation {
            base: frames[0].0,
            frames: frames.into(),
            interpolate,
        }
    }

    #[test]
    fn frames_play_for_their_time_and_loop() {
        let a = anim(&[(10, 2), (11, 1), (12, 3)], false);
        let shown: Vec<u16> = (0..7).map(|t| a.at(t).0).collect();
        assert_eq!(shown, [10, 10, 11, 12, 12, 12, 10]);
    }

    #[test]
    fn interpolation_blends_towards_the_next_frame() {
        let a = anim(&[(1, 4), (2, 4)], true);
        assert_eq!(a.at(0), (1, 2, 0));
        assert_eq!(a.at(2), (1, 2, u16::MAX / 2));
        assert_eq!(a.at(4).0, 2);
        assert_eq!(a.at(7).1, 1, "the last frame blends back to the first");
    }

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
        assert!(set.tile_count <= rows * crate::renderer::ATLAS_COLS);
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
