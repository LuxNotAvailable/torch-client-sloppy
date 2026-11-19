//! The sky: its parts as `draw::Env::sky`, and the world's fog and clear
//! colour on the world camera.

use std::sync::Arc;

use crate::util::javarandom::JavaRandom;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{Camera, ClearColorConfig};
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::AppState;
use super::systems::WorldCamera;
use crate::client::dimension::{self, Skybox};
use crate::client::timeline::{self, Argb, SkyState};
use crate::draw::{ALPHA_ADD, ALPHA_BLEND, EntityVertex, EnvDraw, MeshData, SKY_SCALE, Texture};

const SKY_DISC_RADIUS: f32 = 512.0;
const SKY_DISC_HEIGHT: f32 = 16.0;
const DARK_DISC_OFFSET: f32 = 12.0;
const SUN_SIZE: f32 = 30.0;
const SUN_HEIGHT: f32 = 100.0;
const MOON_SIZE: f32 = 20.0;
const MOON_HEIGHT: f32 = 100.0;
const STAR_COUNT: usize = 1500;
const STAR_DISTANCE: f32 = 100.0;
const SUNRISE_STEPS: usize = 16;
const STAR_SEED: i64 = 10842;

const DISC_SIDES: usize = 8;

const DISC_RINGS: usize = 16;

/// Drawing order among the sky parts (`EnvPart::bias`).
const BIAS_SKY_DISC: f32 = 0.0;
const BIAS_END_SKY: f32 = 0.0;
const BIAS_SUNRISE: f32 = 1000.0 * SKY_SCALE;
const BIAS_SUN: f32 = 2000.0 * SKY_SCALE;
const BIAS_MOON: f32 = 3000.0 * SKY_SCALE;
const BIAS_STARS: f32 = 4000.0 * SKY_SCALE;
const BIAS_DARK_DISC: f32 = 5000.0 * SKY_SCALE;
// Clouds must out-bias every sky part.
pub(super) const BIAS_CLOUDS: f32 = 6000.0 * SKY_SCALE;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::draw::Env>()
            .add_systems(
                Update,
                setup.run_if(resource_added::<crate::renderer::systems::AssetsReady>),
            )
            .add_systems(
                PostUpdate,
                (sync_water_clarity, update_sky)
                    .chain()
                    .in_set(crate::draw::DrawSystems)
                    .before(TransformSystems::Propagate)
                    .run_if(in_state(AppState::InGame))
                    .run_if(resource_exists::<SkyAssets>),
            );
    }
}

/// The world camera's distance fog, set by the sky each frame.
#[derive(Component, Clone, Copy, Debug)]
pub struct WorldFog {
    pub color: Color,
    pub start: f32,
    pub end: f32,
    /// The environmental fog range (pack env channels).
    pub env: Vec2,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkyPart {
    SkyDisc,
    DarkDisc,
    Sun,
    Moon,
    Stars,
    SunriseFan,
    EndSky,
}

impl SkyPart {
    /// Its place in `draw::SKY_PIECES`.
    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

#[derive(Resource)]
struct SkyAssets {
    sky_disc: Arc<MeshData>,
    dark_disc: Arc<MeshData>,
    sun_quad: Arc<MeshData>,
    moon_quad: Arc<MeshData>,
    stars: Arc<MeshData>,
    sunrise: Arc<MeshData>,
    end_sky: Arc<MeshData>,
    sun: Handle<Image>,
    moons: Vec<Handle<Image>>,
    end: Handle<Image>,
    discs_built_for: Option<(Argb, Argb, f32)>,
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let celestial = crate::textures_dir().join("environment/celestial");
    let white = Argb(0xFFFF_FFFF);
    let black = Argb(0xFF00_0000);
    commands.insert_resource(SkyAssets {
        sky_disc: disc_mesh(SKY_DISC_HEIGHT, white, white, SKY_DISC_RADIUS),
        dark_disc: disc_mesh(-SKY_DISC_HEIGHT, black, white, SKY_DISC_RADIUS),
        sun_quad: celestial_quad(false),
        moon_quad: celestial_quad(true),
        stars: star_mesh(),
        sunrise: sunrise_mesh(),
        end_sky: end_sky_mesh(),
        sun: images.add(load_celestial(&celestial.join("sun.png"))),
        moons: timeline::MOON_PHASE_NAMES
            .iter()
            .map(|name| images.add(load_celestial(&celestial.join(format!("moon/{name}.png")))))
            .collect(),
        end: images.add(load_tiled(
            &crate::textures_dir().join("environment/end_sky.png"),
        )),
        discs_built_for: None,
    });
}

fn load_celestial(path: &std::path::Path) -> Image {
    use bevy::image::ImageSampler;
    use wgpu_types::{Extent3d, TextureDimension, TextureFormat};

    let (w, h, data) = match crate::platform::assets::open_image(path) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            (rgba.width(), rgba.height(), rgba.into_raw())
        }
        Err(e) => {
            eprintln!("sky: {} could not be read ({e})", path.display());
            (1, 1, vec![0u8; 4])
        }
    };
    let mut image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

fn load_tiled(path: &std::path::Path) -> Image {
    let mut image = load_celestial(path);
    image.sampler = crate::entities::repeating_nearest();
    image
}

/// A sky mesh: positions, colours and uvs, normals up.
fn sky_mesh(
    positions: Vec<[f32; 3]>,
    colors: Option<Vec<[f32; 4]>>,
    uvs: Option<Vec<[f32; 2]>>,
    indices: Vec<u32>,
) -> Arc<MeshData> {
    let verts = positions
        .iter()
        .enumerate()
        .map(|(i, &pos)| EntityVertex {
            pos,
            normal: [0.0, 1.0, 0.0],
            uv: uvs.as_ref().map_or([0.0; 2], |u| u[i]),
            color: colors.as_ref().map_or([1.0; 4], |c| c[i]),
            part: 0,
        })
        .collect();
    MeshData::new(verts, indices, false)
}

fn disc_mesh(yy: f32, sky: Argb, fog: Argb, sky_end: f32) -> Arc<MeshData> {
    use crate::util::mth::srgb_to_linear;

    let sign = yy.signum();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let sky = sky.to_rgba_f32();
    let fog = fog.to_rgba_f32();
    let shade = |dist: f32| -> [f32; 4] {
        let f = if sky_end <= 0.0 {
            1.0
        } else {
            (dist / sky_end).clamp(0.0, 1.0)
        };
        let mix = |a: f32, b: f32| srgb_to_linear(a + (b - a) * f);
        [
            mix(sky[0], fog[0]),
            mix(sky[1], fog[1]),
            mix(sky[2], fog[2]),
            sky[3],
        ]
    };

    let height = yy.abs();
    positions.push([0.0, yy, 0.0]);
    colors.push(shade(height));

    let mut radii: Vec<f32> = Vec::with_capacity(DISC_RINGS + 1);
    for k in 1..=DISC_RINGS {
        let dist = height + (sky_end.max(height) - height) * (k as f32 / DISC_RINGS as f32);
        let r = (dist * dist - height * height).max(0.0).sqrt();
        radii.push(r.min(SKY_DISC_RADIUS));
    }
    radii.push(SKY_DISC_RADIUS);

    for &r in &radii {
        for i in 0..DISC_SIDES {
            let angle = (-180.0 + 45.0 * i as f32).to_radians();
            positions.push([sign * r * angle.cos(), yy, r * angle.sin()]);
            colors.push(shade((r * r + height * height).sqrt()));
        }
    }

    for i in 0..DISC_SIDES {
        let a = 1 + i as u32;
        let b = 1 + ((i + 1) % DISC_SIDES) as u32;
        indices.extend_from_slice(&[0, a, b]);
    }
    for ring in 0..radii.len() - 1 {
        let inner = 1 + (ring * DISC_SIDES) as u32;
        let outer = inner + DISC_SIDES as u32;
        for i in 0..DISC_SIDES as u32 {
            let j = (i + 1) % DISC_SIDES as u32;
            indices.extend_from_slice(&[inner + i, outer + i, outer + j]);
            indices.extend_from_slice(&[inner + i, outer + j, inner + j]);
        }
    }
    sky_mesh(positions, Some(colors), None, indices)
}

fn celestial_quad(mirrored: bool) -> Arc<MeshData> {
    let positions = vec![
        [-1.0, 0.0, -1.0],
        [1.0, 0.0, -1.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
    ];
    let uvs: [[f32; 2]; 4] = if mirrored {
        [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]]
    } else {
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
    };
    sky_mesh(positions, None, Some(uvs.to_vec()), vec![0, 1, 2, 0, 2, 3])
}

fn end_sky_mesh() -> Arc<MeshData> {
    use crate::util::mth::srgb_to_linear;

    const CORNERS: [[f32; 3]; 4] = [
        [-100.0, -100.0, -100.0],
        [-100.0, -100.0, 100.0],
        [100.0, -100.0, 100.0],
        [100.0, -100.0, -100.0],
    ];
    const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [0.0, 16.0], [16.0, 16.0], [16.0, 0.0]];

    let quarter = std::f32::consts::FRAC_PI_2;
    let rotations = [
        Quat::IDENTITY,
        Quat::from_rotation_x(quarter),
        Quat::from_rotation_x(-quarter),
        Quat::from_rotation_x(std::f32::consts::PI),
        Quat::from_rotation_z(quarter),
        Quat::from_rotation_z(-quarter),
    ];

    let grey = srgb_to_linear(40.0 / 255.0);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(24);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);
    for (face, rotation) in rotations.iter().enumerate() {
        let base = face as u32 * 4;
        for corner in 0..4 {
            positions.push((*rotation * Vec3::from(CORNERS[corner])).to_array());
            uvs.push(UVS[corner]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let n = positions.len();
    sky_mesh(
        positions,
        Some(vec![[grey, grey, grey, 1.0]; n]),
        Some(uvs),
        indices,
    )
}

fn sunrise_mesh() -> Arc<MeshData> {
    let mut positions = vec![[0.0f32, 100.0, 0.0]];
    let mut colors = vec![[1.0f32, 1.0, 1.0, 1.0]];
    for i in 0..=SUNRISE_STEPS {
        let angle = i as f32 * std::f32::consts::TAU / SUNRISE_STEPS as f32;
        let (sin, cos) = (angle.sin(), angle.cos());
        positions.push([sin * 120.0, cos * 120.0, -cos * 40.0]);
        colors.push([1.0, 1.0, 1.0, 0.0]);
    }
    let mut indices = Vec::new();
    for i in 1..SUNRISE_STEPS as u32 + 1 {
        indices.extend_from_slice(&[0, i, i + 1]);
    }
    sky_mesh(positions, Some(colors), None, indices)
}

fn star_mesh() -> Arc<MeshData> {
    let mut random = JavaRandom::new(STAR_SEED);
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for _ in 0..STAR_COUNT {
        let x = random.next_f32() * 2.0 - 1.0;
        let y = random.next_f32() * 2.0 - 1.0;
        let z = random.next_f32() * 2.0 - 1.0;
        let size = 0.15 + random.next_f32() * 0.1;
        let length_sq = x * x + y * y + z * z;
        if length_sq <= 0.010000001 || length_sq >= 1.0 {
            continue;
        }
        let center = Vec3::new(x, y, z).normalize() * STAR_DISTANCE;
        let z_rot = (random.next_f64() * std::f64::consts::PI * 2.0) as f32;

        let dir = (-center).normalize();
        let left = Vec3::Y.cross(dir);
        if left.length_squared() < 1e-12 {
            continue;
        }
        let left = left.normalize();
        let up = dir.cross(left);
        let rotation = Mat3::from_cols(left, up, dir) * Mat3::from_rotation_z(-z_rot);

        let base = positions.len() as u32;
        for corner in [
            Vec3::new(size, -size, 0.0),
            Vec3::new(size, size, 0.0),
            Vec3::new(-size, size, 0.0),
            Vec3::new(-size, -size, 0.0),
        ] {
            positions.push((rotation * corner + center).to_array());
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    sky_mesh(positions, None, None, indices)
}

use crate::util::block_model::{EyeFluid, eye_fluid_at};

const LAVA_FOG_COLOR: Argb = Argb(0xFF99_1900);

const LAVA_FOG_RANGE: (f32, f32) = (0.25, 1.0);

fn sync_water_clarity(
    gui: Res<crate::gui::GuiState>,
    mut params: ResMut<crate::terrain::textures::TerrainParams>,
) {
    let alpha = super::environment::water_opacity(gui.options.water_clarity);
    if params.water_alpha != alpha {
        params.water_alpha = alpha;
    }
}

#[allow(clippy::too_many_arguments)]
fn update_sky(
    time: Res<Time>,
    gui: Res<crate::gui::GuiState>,
    // Ticks the eye has spent underwater, as vanilla's `waterVisionTime`:
    // up one a tick under water, down ten a tick out of it. (Vanilla's
    // spectators gain ten a tick; not told apart here.)
    mut water_vision_ticks: Local<f32>,
    view: Res<super::frame_view::FrameView>,
    env: Res<super::environment::Environment>,
    mut assets: ResMut<SkyAssets>,
    mut out: ResMut<crate::draw::Env>,
    mut world_view: Query<(&Transform, &mut WorldFog, &mut Camera), With<WorldCamera>>,
) {
    crate::prof_span!("render:update_sky");
    out.sky.clear();
    let [eye_x, eye_y, eye_z] = view.tick_eye();
    let dim = dimension::current();
    let skybox = dim.skybox();
    let mut sky = env.sky;

    let Ok((world_transform, mut fog, mut camera)) = world_view.single_mut() else {
        return;
    };

    sky.fog_color = super::environment::fog_base_color(
        &sky,
        env.sky_fog_end,
        world_transform.forward().x,
        env.render_distance,
        env.rain,
        env.thunder,
    );

    let eye_fluid = eye_fluid_at(eye_x, eye_y, eye_z);
    let ticks = time.delta_secs() * 20.0;
    *water_vision_ticks = if eye_fluid == Some(EyeFluid::Water) {
        (*water_vision_ticks + ticks).min(600.0)
    } else {
        (*water_vision_ticks - ticks * 10.0).max(0.0)
    };
    let environmental = match eye_fluid {
        Some(EyeFluid::Water) => {
            // Vanilla's WaterFogEnvironment and FogRenderer: the fog reaches
            // further and its colour brightens as the eyes adjust.
            let vision = super::environment::water_vision(*water_vision_ticks);
            let color = env.water_fog_color;
            let top = color
                .channel(16)
                .max(color.channel(8))
                .max(color.channel(0));
            sky.fog_color =
                if top > 0 && color.channel(16) * color.channel(8) * color.channel(0) > 0 {
                    let scale = 1.0 + vision * (255.0 / top as f32 - 1.0);
                    color.scale_rgb(scale, scale, scale)
                } else {
                    color
                };
            let opacity = super::environment::water_opacity(gui.options.water_clarity);
            let (start, end) = env.water_fog_distance;
            (start, end * vision.max(0.25) / opacity)
        }
        Some(EyeFluid::Lava) => {
            sky.fog_color = LAVA_FOG_COLOR;
            LAVA_FOG_RANGE
        }
        None => env.fog_distance,
    };
    let fog_color = sky.fog_color.to_color();

    fog.color = fog_color;
    (fog.start, fog.end) = env.render_fog_distance;
    fog.env = Vec2::new(environmental.0, environmental.1);
    camera.clear_color = ClearColorConfig::Custom(fog_color);

    if gui.in_menu() {
        return;
    }
    rebuild_discs(&mut assets, &sky, env.sky_fog_end);

    let root = Mat4::from_scale_rotation_translation(
        Vec3::splat(SKY_SCALE),
        Quat::IDENTITY,
        world_transform.translation,
    );
    let mut push = |part: SkyPart,
                    mesh: &Arc<MeshData>,
                    texture: Option<&Handle<Image>>,
                    tint: Vec4,
                    alpha: u32,
                    bias: f32,
                    local: Mat4| {
        out.sky.push(EnvDraw {
            texture: texture.map_or(Texture::None, |t| Texture::Image(t.clone())),
            alpha,
            bias,
            pad: [part.index() as f32 + 1.0, 0.0, 0.0],
            tint: tint.to_array(),
            ..EnvDraw::new(mesh.clone(), 0, root * local)
        });
    };

    if skybox == Skybox::End {
        push(
            SkyPart::EndSky,
            &assets.end_sky,
            Some(&assets.end),
            Vec4::ONE,
            ALPHA_BLEND,
            BIAS_END_SKY,
            Mat4::IDENTITY,
        );
        return;
    }
    // Under water vanilla fogs the sky out at the water fog's end
    // (`WaterFogEnvironment` sets `skyEnd`), well short of the sky, so
    // only the clear colour (the water fog's) shows.
    if skybox != Skybox::Overworld || eye_fluid == Some(EyeFluid::Water) {
        return;
    }

    let yaw = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    let place = |angle: f32, height: f32, size: f32| {
        let rotation = yaw * Quat::from_rotation_x(angle);
        Mat4::from_scale_rotation_translation(
            Vec3::new(size, 1.0, size),
            rotation,
            rotation * Vec3::new(0.0, height, 0.0),
        )
    };

    push(
        SkyPart::SkyDisc,
        &assets.sky_disc,
        None,
        Vec4::ONE,
        ALPHA_BLEND,
        BIAS_SKY_DISC,
        Mat4::IDENTITY,
    );
    let sunrise_alpha = sky.sunrise_color.alpha() as f32 / 255.0;
    if sunrise_alpha > 0.001 {
        let flip = if sky.sun_angle.sin() < 0.0 {
            180.0f32
        } else {
            0.0
        };
        let rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
            * Quat::from_rotation_z((flip + 90.0).to_radians());
        push(
            SkyPart::SunriseFan,
            &assets.sunrise,
            None,
            sky.sunrise_color.to_color().to_linear().to_vec4(),
            ALPHA_BLEND,
            BIAS_SUNRISE,
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.0, 1.0, sunrise_alpha),
                rotation,
                Vec3::ZERO,
            ),
        );
    }
    push(
        SkyPart::Sun,
        &assets.sun_quad,
        Some(&assets.sun),
        Vec4::ONE,
        ALPHA_ADD,
        BIAS_SUN,
        place(sky.sun_angle, SUN_HEIGHT, SUN_SIZE),
    );
    push(
        SkyPart::Moon,
        &assets.moon_quad,
        assets.moons.get(sky.moon_phase),
        Vec4::ONE,
        ALPHA_ADD,
        BIAS_MOON,
        place(sky.moon_angle, MOON_HEIGHT, MOON_SIZE),
    );
    let brightness = sky.star_brightness;
    if brightness > 0.0 {
        push(
            SkyPart::Stars,
            &assets.stars,
            None,
            Color::srgba(brightness, brightness, brightness, brightness)
                .to_linear()
                .to_vec4(),
            ALPHA_ADD,
            BIAS_STARS,
            Mat4::from_quat(yaw * Quat::from_rotation_x(sky.star_angle)),
        );
    }
    if eye_y < dimension::horizon() {
        push(
            SkyPart::DarkDisc,
            &assets.dark_disc,
            None,
            Vec4::ONE,
            ALPHA_BLEND,
            BIAS_DARK_DISC,
            Mat4::from_translation(Vec3::new(0.0, DARK_DISC_OFFSET, 0.0)),
        );
    }
}

fn rebuild_discs(assets: &mut SkyAssets, sky: &SkyState, sky_end: f32) {
    let key = (sky.sky_color, sky.fog_color, sky_end);
    if assets.discs_built_for == Some(key) {
        return;
    }
    assets.discs_built_for = Some(key);
    assets.sky_disc = disc_mesh(SKY_DISC_HEIGHT, sky.sky_color, sky.fog_color, sky_end);
    assets.dark_disc = disc_mesh(-SKY_DISC_HEIGHT, Argb(0xFF00_0000), sky.fog_color, sky_end);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_legacy_generator_matches_java() {
        let mut r = JavaRandom::new(0);
        assert!((r.next_f32() - 0.730_967_8).abs() < 1e-6);
        assert!((r.next_f32() - 0.831_441_0).abs() < 1e-6);
    }

    #[test]
    fn the_star_field_is_built() {
        let count = star_mesh().verts.len();
        assert!(count % 4 == 0);
        assert!(count / 4 > 600 && count / 4 < STAR_COUNT, "{count}");
    }

    #[test]
    fn the_disc_rim_is_fog_coloured() {
        let mesh = disc_mesh(16.0, Argb(0xFF00_00FF), Argb(0xFFFF_0000), 128.0);
        assert_eq!(mesh.verts.last().unwrap().color, [1.0, 0.0, 0.0, 1.0]);
        let expected = crate::util::mth::srgb_to_linear(0.875);
        let centre = mesh.verts[0].color;
        assert!((centre[2] - expected).abs() < 1e-5, "{centre:?}");
    }
}
