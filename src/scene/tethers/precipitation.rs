//! Falling rain and snow (`WeatherEffectRenderer`): a sheet on every open
//! column about the camera.

use super::*;

pub(super) fn draw_precipitation(
    view: Res<FrameView>,
    camera: CameraQuery,
    mut out: LayerOut,
    mut env_out: ResMut<crate::draw::Env>,
) {
    let (mut rain, mut snow) = (Geo::default(), Geo::default());
    // The native renderer builds the sheets on the GPU (no pack).
    let native = crate::scene::weather::NATIVE.load(std::sync::atomic::Ordering::Relaxed);
    let mut sheets = Vec::new();
    let level = crate::client::tracking::current_world()
        .lock()
        .unwrap()
        .clone();
    let intensity = crate::scene::environment::rain_level();
    let env = crate::client::envprobe::camera_biome().map(crate::util::biome_color::env);
    if view.in_world
        && intensity > 0.0
        && let Some(level) = level
        && let Some(env) = env.filter(|e| e.has_precipitation)
    {
        use crate::scene::weather::Fall;
        use azalea_core::heightmap_kind::HeightmapKind;
        use azalea_core::position::ChunkPos;
        let level = crate::client::tracking::read_world(&level);
        let (cam_pos, _, _) = camera_pose(&camera);
        let camera = [
            cam_pos.x.floor() as i32,
            cam_pos.y.floor() as i32,
            cam_pos.z.floor() as i32,
        ];
        let min_y = level.chunks.min_y();
        let terrain = |x: i32, z: i32| {
            let chunk = level
                .chunks
                .get(&ChunkPos::new(x.div_euclid(16), z.div_euclid(16)));
            chunk
                .and_then(|c| {
                    c.read()
                        .heightmaps
                        .get(&HeightmapKind::MotionBlocking)
                        .map(|m| {
                            m.get_first_available(x.rem_euclid(16) as u8, z.rem_euclid(16) as u8)
                        })
                })
                .unwrap_or(min_y)
        };
        // `coldEnoughToSnow` at the camera's height.
        let temperature = env.temperature - (camera[1] - 80).max(0) as f32 * 0.00125;
        let kind = if env.frozen || temperature < 0.15 {
            Fall::Snow
        } else {
            Fall::Rain
        };
        let columns = crate::scene::weather::columns(
            camera,
            crate::client::tracking::game_time(),
            view.partial,
            terrain,
            |_, _| Some(kind),
        );
        for column in &columns {
            let sample_y = camera[1].max(terrain(column.x, column.z));
            let (block, sky) = crate::lighting::lightmap::levels_at([
                column.x as f32,
                sample_y as f32,
                column.z as f32,
            ]);
            // Snow is lit a quarter of the way to full.
            let (block, sky) = match column.fall {
                Fall::Rain => (block, sky),
                Fall::Snow => ((block * 3 + 15) / 4, (sky * 3 + 15) / 4),
            };
            let shade = crate::scene::shadows::brightness(block.max(sky));
            let target = match column.fall {
                Fall::Rain => &mut rain,
                Fall::Snow => &mut snow,
            };
            if native {
                sheets.push(crate::scene::weather::native_sheet(
                    column, cam_pos, intensity, shade,
                ));
                continue;
            }
            for corners in crate::scene::weather::sheet_pieces(column, cam_pos, intensity) {
                target.quad(corners.map(|(p, uv, a)| (cam_pos + p, uv, [shade, shade, shade, a])));
            }
        }
    }
    // Sheets as env parts (kind 6): a unit quad stretched by the
    // transform; uv scale and offset scroll the texture.
    env_out.weather.clear();
    if let Some((quad, rain, snow)) = crate::scene::weather::ASSETS.get() {
        env_out.weather.extend(sheets.into_iter().map(|s| {
            let texture = if s.snow { snow } else { rain };
            crate::draw::EnvDraw {
                texture: texture
                    .clone()
                    .map_or(crate::draw::Texture::None, crate::draw::Texture::Image),
                alpha: crate::draw::ALPHA_BLEND,
                uv: s.uv,
                tint: s.tint,
                ..crate::draw::EnvDraw::new(
                    quad.clone(),
                    6,
                    Mat4::from_cols(
                        s.across.extend(0.0),
                        s.down.extend(0.0),
                        Vec4::ZERO,
                        s.origin.extend(1.0),
                    ),
                )
            }
        }));
    }
    out.put(Kind::Rain, rain);
    out.put(Kind::Snow, snow);
}
