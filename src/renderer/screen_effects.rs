//! Full-view overlays: underwater tint, inside-block, on-fire, vignette and
//! the carved pumpkin blur, as `draw::Env::screen`. All are quads in the hand
//! camera's view space, laid out as vanilla's and pulled in just past the
//! near plane so they draw over the world and the hand.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::AppState;
use super::hand::HandCamera;
use super::input::{FreecamState, ThirdPersonState};
use super::systems::{LightmapState, Shared};
use crate::draw::{ALPHA_BLEND, EntityVertex, EnvDraw, MeshData, Texture};
use crate::session::Gamemode;
use crate::util::block_model::{EyeFluid, eye_fluid_at, is_solid};

/// View-space distance vanilla draws these at.
const VANILLA_Z: f32 = 0.5;
/// How far in front of the camera the scaled overlays sit, as a multiple of
/// the hand camera's near plane.
const DEPTH: [f32; 3] = [2.5, 1.6, 1.3];

const WATER_TILES: f32 = 4.0;
const WATER_SCROLL: f32 = 64.0;
const WATER_ALPHA: f32 = 0.1;
const WALL_SHADE: f32 = 25.0 / 255.0;
const FIRE_ALPHA: f32 = 0.9;
const FIRE_TILT: f32 = std::f32::consts::PI / 18.0;

/// A full-view overlay quad.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay {
    Water,
    Wall,
    Fire(bool),
    Vignette,
    Pumpkin,
}

impl Overlay {
    const ALL: [Overlay; 6] = [
        Overlay::Water,
        Overlay::Wall,
        Overlay::Fire(false),
        Overlay::Fire(true),
        Overlay::Vignette,
        Overlay::Pumpkin,
    ];
}

pub struct ScreenEffectsPlugin;

impl Plugin for ScreenEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                setup.run_if(not(resource_exists::<ScreenAssets>)),
                update
                    .in_set(crate::draw::DrawSystems)
                    .run_if(in_state(AppState::InGame))
                    .run_if(resource_exists::<ScreenAssets>),
            )
                .chain(),
        );
    }
}

fn load(path: &str, repeat: bool, f: impl FnOnce(&mut image::RgbaImage)) -> Image {
    let file = crate::textures_dir().join(path);
    let mut rgba = match crate::platform::assets::open_image(&file) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            crate::log_warn!(
                "screen_effects",
                "{} could not be read ({e})",
                file.display()
            );
            image::RgbaImage::new(1, 1)
        }
    };
    f(&mut rgba);
    let mut image = Image::new(
        Extent3d {
            width: rgba.width(),
            height: rgba.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        // Kept in the main world too, for the Vulkan dump (a few small
        // images).
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    let mode = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

/// The vignette darkens by multiplying with `1 - c * texture`; as black with
/// the texture in alpha, plain alpha blending does exactly that.
/// The vignette's opacity this frame, eased toward the light at the eye.
static VIGNETTE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The vignette's opacity for the GUI to draw (0 while the HUD is hidden).
pub(crate) fn vignette_strength() -> f32 {
    f32::from_bits(VIGNETTE.load(std::sync::atomic::Ordering::Relaxed))
}

fn luma_to_alpha(img: &mut image::RgbaImage) {
    for p in img.pixels_mut() {
        *p = image::Rgba([0, 0, 0, p[0]]);
    }
}

fn quad(mirror_u: bool) -> Arc<MeshData> {
    let (u0, u1) = if mirror_u { (1.0, 0.0) } else { (0.0, 1.0) };
    let corners = [
        ([-1.0, -1.0, 0.0], [u0, 1.0]),
        ([1.0, -1.0, 0.0], [u1, 1.0]),
        ([1.0, 1.0, 0.0], [u1, 0.0]),
        ([-1.0, 1.0, 0.0], [u0, 0.0]),
    ];
    let verts = corners
        .iter()
        .map(|&(pos, uv)| EntityVertex {
            pos,
            normal: [0.0, 0.0, 1.0],
            uv,
            color: [1.0; 4],
            part: 0,
        })
        .collect();
    MeshData::new(verts, vec![0, 1, 2, 0, 2, 3], false)
}

#[derive(Resource)]
struct ScreenAssets {
    plain: Arc<MeshData>,
    mirrored: Arc<MeshData>,
    /// Underwater, fire, vignette, pumpkin blur.
    textures: [Handle<Image>; 4],
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let textures = [
        ("misc/underwater.png", true, false),
        ("block/fire_1.png", false, false),
        ("misc/vignette.png", false, true),
        ("misc/pumpkinblur.png", false, false),
    ]
    .map(|(path, repeat, luma)| {
        images.add(load(path, repeat, |img| {
            if luma {
                luma_to_alpha(img);
            }
        }))
    });
    commands.insert_resource(ScreenAssets {
        plain: quad(false),
        mirrored: quad(true),
        textures,
    });
}

/// Places a vanilla view-space quad (centre `at`, half size `half`, turned
/// `yaw` about Y) `depth` near-planes in front of the camera, keeping its
/// on-screen footprint.
fn place(at: Vec3, half: Vec2, yaw: f32, depth: f32, near: f32) -> Mat4 {
    let k = depth * near / VANILLA_Z;
    let rotation = Quat::from_rotation_y(yaw);
    Mat4::from_scale_rotation_translation(
        (half * k).extend(1.0),
        rotation,
        (at + rotation * Vec3::new(0.0, 0.0, -VANILLA_Z)) * k,
    )
}

/// Lightmap brightness of a raw light level, vanilla's `getBrightness`.
fn brightness(level: f32, ambient: f32) -> f32 {
    let f = level / 15.0;
    ambient + (f / (4.0 - 3.0 * f)) * (1.0 - ambient)
}

/// The first view-blocking block among the eight corners of a small box around
/// the eye, as vanilla's `getViewBlockingState`.
fn view_blocker(eye: [f64; 3]) -> Option<azalea::block::BlockState> {
    let world = crate::client::tracking::current_world()
        .lock()
        .unwrap()
        .clone()?;
    let world = crate::client::tracking::read_world(&world);
    const WIDTH: f64 = 0.6 * 0.8;
    (0..8).find_map(|i| {
        let corner = |bit: i32, span: f64| (f64::from((i >> bit) & 1) - 0.5) * span;
        let pos = azalea_core::position::BlockPos::new(
            (eye[0] + corner(0, WIDTH)).floor() as i32,
            (eye[1] + corner(1, 0.1)).floor() as i32,
            (eye[2] + corner(2, WIDTH)).floor() as i32,
        );
        world.get_block_state(pos).filter(|&s| is_solid(s))
    })
}

#[allow(clippy::too_many_arguments)]
fn update(
    shared: Res<Shared>,
    gui: Res<crate::gui::GuiState>,
    time: Res<Time>,
    lightmap: Res<LightmapState>,
    third_person: Res<ThirdPersonState>,
    freecam: Res<FreecamState>,
    assets: Res<ScreenAssets>,
    camera: Query<&Projection, With<HandCamera>>,
    mut out: ResMut<crate::draw::Env>,
    mut vignette: Local<Option<f32>>,
) {
    crate::prof_span!("render:screen_effects");
    out.screen.clear();
    let Ok(Projection::Perspective(projection)) = camera.single() else {
        return;
    };
    let (near, aspect, half_h) = (
        projection.near,
        projection.aspect_ratio,
        (projection.fov * 0.5).tan() * VANILLA_Z,
    );
    let Ok(state) = shared.0.lock();
    let session = &state.session;
    let spectator = session.gamemode == Gamemode::Spectator;
    let first_person = !third_person.active && !freecam.active;
    let eye_height = super::systems::session_eye_height(session) as f64;
    let eye = [
        session.player_pos[0] as f64,
        session.player_pos[1] as f64 + eye_height,
        session.player_pos[2] as f64,
    ];
    let (yaw, pitch) = (state.camera_yaw, state.camera_pitch);
    let on_fire = session.vitals.health_display.on_fire;
    let pumpkin = session.inventory.armor[0] == Some("carved_pumpkin");
    let sleeping = session.sleeping;
    let hud = !gui.in_menu() && !gui.gui_hidden();
    drop(state);

    let eye_f = eye.map(|v| v as f32);
    let (block, sky) = crate::lighting::lightmap::levels_at(eye_f);
    let lm = &lightmap.current;
    let level = f32::from(block).max(f32::from(sky) * lm.sky_factor);
    // Dimension `ambient_light`: only the nether's is non-zero.
    let ambient = match crate::client::dimension::current() {
        crate::client::dimension::Dimension::Nether => 0.1,
        _ => 0.0,
    };
    let target = (1.0 - brightness(level, ambient)).clamp(0.0, 1.0);
    let rate = 1.0 - 0.99f32.powf(time.delta_secs() * 20.0);
    let v = vignette.get_or_insert(target);
    *v += (target - *v) * rate;
    VIGNETTE.store(
        if hud { v.clamp(0.0, 1.0).to_bits() } else { 0 },
        std::sync::atomic::Ordering::Relaxed,
    );

    let water =
        first_person && !spectator && eye_fluid_at(eye[0], eye[1], eye[2]) == Some(EyeFluid::Water);
    let wall = (!spectator && !sleeping)
        .then(|| view_blocker(eye))
        .flatten()
        .and_then(crate::util::particles::particle_uv);
    let fire_frames = 32.0;
    let fire_frame = (time.elapsed_secs() * 20.0).floor() % fire_frames;
    let water_light = brightness(level, ambient);

    for (order, overlay) in Overlay::ALL.into_iter().enumerate() {
        let show = match overlay {
            Overlay::Water => water,
            Overlay::Wall => wall.is_some() && first_person,
            Overlay::Fire(_) => on_fire && first_person && !spectator && !sleeping,
            // Drawn by the GUI (`vignette_strength`), where it costs a
            // textured quad rather than a full-screen material pass.
            Overlay::Vignette => false,
            Overlay::Pumpkin => hud && pumpkin && first_person,
        };
        if !show {
            continue;
        }
        let full = Vec2::new(half_h * aspect, half_h);
        let transform = match overlay {
            Overlay::Water | Overlay::Wall => place(Vec3::ZERO, Vec2::ONE, 0.0, DEPTH[0], near),
            Overlay::Fire(left) => {
                let side = if left { -1.0 } else { 1.0 };
                place(
                    Vec3::new(0.24 * side, -0.3, 0.0),
                    Vec2::splat(0.5),
                    FIRE_TILT * side,
                    DEPTH[0],
                    near,
                )
            }
            Overlay::Vignette => place(Vec3::ZERO, full, 0.0, DEPTH[1], near),
            Overlay::Pumpkin => place(Vec3::ZERO, full, 0.0, DEPTH[2], near),
        };
        let mesh = match overlay {
            Overlay::Vignette | Overlay::Pumpkin => &assets.plain,
            _ => &assets.mirrored,
        };
        let [water_tex, fire_tex, vignette_tex, pumpkin_tex] = &assets.textures;
        // uv: scale xy, offset zw; tint sRGB.
        let (texture, uv, tint) = match overlay {
            Overlay::Water => (
                Texture::Image(water_tex.clone()),
                {
                    // Vanilla's (-yRot, xRot) / 64; ours are yaw = -yRot - 180,
                    // pitch = -xRot.
                    let offset = Vec2::new(yaw, -pitch) / WATER_SCROLL;
                    [WATER_TILES, WATER_TILES, offset.x, offset.y]
                },
                Color::srgba(water_light, water_light, water_light, WATER_ALPHA),
            ),
            Overlay::Wall => {
                let Some([u0, v0, u1, v1]) = wall else {
                    continue;
                };
                (
                    Texture::Atlas,
                    [u1 - u0, v1 - v0, u0, v0],
                    Color::srgb(WALL_SHADE, WALL_SHADE, WALL_SHADE),
                )
            }
            Overlay::Fire(_) => (
                Texture::Image(fire_tex.clone()),
                [1.0, 1.0 / fire_frames, 0.0, fire_frame / fire_frames],
                Color::srgba(1.0, 1.0, 1.0, FIRE_ALPHA),
            ),
            Overlay::Vignette => (
                Texture::Image(vignette_tex.clone()),
                [1.0, 1.0, 0.0, 0.0],
                Color::srgba(1.0, 1.0, 1.0, v.clamp(0.0, 1.0)),
            ),
            Overlay::Pumpkin => (
                Texture::Image(pumpkin_tex.clone()),
                [1.0, 1.0, 0.0, 0.0],
                Color::WHITE,
            ),
        };
        out.screen.push(EnvDraw {
            texture,
            alpha: ALPHA_BLEND,
            bias: order as f32,
            uv,
            tint: tint.to_linear().to_f32_array(),
            ..EnvDraw::new(mesh.clone(), 2, transform)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_follows_the_vanilla_curve() {
        assert_eq!(brightness(15.0, 0.0), 1.0);
        assert_eq!(brightness(0.0, 0.0), 0.0);
        assert!((brightness(7.5, 0.0) - 0.2).abs() < 1e-6);
        assert!((brightness(0.0, 0.1) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn a_placed_quad_keeps_its_screen_footprint() {
        let near = 0.05;
        let t = place(Vec3::new(0.24, -0.3, 0.0), Vec2::splat(0.5), 0.0, 2.5, near);
        let corner = t.transform_point3(Vec3::new(1.0, 1.0, 0.0));
        let vanilla = Vec3::new(0.74, 0.2, -0.5);
        assert!((corner.x / -corner.z - vanilla.x / -vanilla.z).abs() < 1e-5);
        assert!((corner.y / -corner.z - vanilla.y / -vanilla.z).abs() < 1e-5);
        assert!(-corner.z > near);
    }
}
