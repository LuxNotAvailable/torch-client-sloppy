//! The player in the inventory: `InventoryScreen.extractEntityInInventoryFollowsMouse`.
//!
//! The GUI says where the figure goes each frame. A copy of the local
//! player's rig stands on its own layer, posed to face the cursor, and an
//! orthographic camera draws it into a texture the size of the figure's box.
//! The GUI then draws that texture in place, between the panel and the items
//! and tooltips over it.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::camera::{ClearColorConfig, RenderTarget, ScalingMode};
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::TextureFormat;

use super::anim::AnimSample;

pub const FIGURE_LAYER: usize = 4;
const FIGURE_CAMERA_ORDER: isize = -2;
/// Where the figure stands on its layer.
pub const FIGURE_ORIGIN: Vec3 = Vec3::ZERO;
/// How far in front of the figure the camera sits, and the depth it sees.
const CAMERA_DISTANCE: f32 = 50.0;

/// `Lighting.INVENTORY_DIFFUSE_LIGHT_0` and `_1`. Vanilla gives them in the
/// GUI's space (y down, z toward the viewer); here y is up and the camera
/// looks down -z, so y flips.
const INVENTORY_LIGHTS: [Vec3; 2] = [Vec3::new(0.2, 1.0, 1.0), Vec3::new(-0.2, 1.0, 0.0)];

pub fn inventory_lights() -> [Vec3; 2] {
    INVENTORY_LIGHTS.map(Vec3::normalize)
}

/// Where the GUI wants the figure, in GUI pixels: its box, the pixels per
/// block (`size`), how far below the box's centre the figure's middle sits,
/// in blocks (`offsetY`), and the cursor it turns toward.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Figure {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub size: f32,
    pub offset_y: f32,
    pub mouse: Option<Vec2>,
}

impl Figure {
    /// `xAngle` and `yAngle`: how far the cursor is from the box's centre,
    /// across and down, as angles. A cursor outside the window counts as
    /// centred.
    pub fn angles(&self) -> (f32, f32) {
        let centre = Vec2::new((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0);
        let mouse = self.mouse.unwrap_or(centre);
        (
            ((centre.x - mouse.x) / 40.0).atan(),
            ((centre.y - mouse.y) / 40.0).atan(),
        )
    }

    /// The pose the figure stands in: the player's own, turned to the
    /// cursor. The body turns `xAngle * 20` degrees and the head as far again;
    /// the head tilts with `yAngle`, except while gliding.
    pub fn pose(&self, player: &AnimSample) -> AnimSample {
        let (x_angle, y_angle) = self.angles();
        let mut pose = player.clone();
        pose.body_yaw = x_angle * 20.0;
        pose.head_yaw = x_angle * 20.0;
        pose.pitch = if pose.fall_flying {
            0.0
        } else {
            -y_angle * 20.0
        };
        pose
    }

    /// The whole figure leans with `yAngle` about its feet: back, top away,
    /// while the cursor is above the box.
    pub fn lean(&self) -> Quat {
        let (_, y_angle) = self.angles();
        Quat::from_rotation_x(-(y_angle * 20.0).to_radians())
    }
}

/// The player's bounding box height in its current pose, which centres the
/// figure in its box.
pub fn box_height(s: &AnimSample) -> f32 {
    if s.sleeping || s.death_time > 0.0 {
        0.2
    } else if s.fall_flying || s.spin_attack || s.visually_swimming {
        0.6
    } else if s.crouching {
        1.5
    } else {
        1.8
    }
}

/// The figure the GUI drew this frame, if any.
#[derive(Resource, Default)]
pub struct InventoryFigure(pub Option<Figure>);

/// The texture the figure is drawn into, for the GUI to sample.
#[derive(Resource, Default, Clone, ExtractResource)]
pub struct FigureImage(pub Option<Handle<Image>>);

/// Whether the figure's texture holds this frame's figure. The GUI only
/// draws it then, so it never shows a stale one or the atlas standing in.
static READY: AtomicBool = AtomicBool::new(false);

pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

#[derive(Component)]
pub struct FigureCamera;

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        super::systems::scene_camera(
            FIGURE_CAMERA_ORDER,
            ClearColorConfig::Custom(Color::NONE),
            FIGURE_LAYER,
        ),
        Projection::Orthographic(OrthographicProjection {
            near: 0.0,
            far: CAMERA_DISTANCE * 2.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(FIGURE_ORIGIN + Vec3::Z * CAMERA_DISTANCE),
        FigureCamera,
        Name::new("inventory_figure_camera"),
    ));
}

/// Fits the camera and its texture to the box the GUI drew: one texel per
/// physical pixel, `size` GUI pixels per block, the figure's middle at the
/// box's centre plus `offsetY`.
#[allow(clippy::type_complexity)]
fn aim_camera(
    mut commands: Commands,
    figure: Res<InventoryFigure>,
    input: Res<crate::gui::GuiInput>,
    view: Res<super::frame_view::FrameView>,
    mut images: ResMut<Assets<Image>>,
    mut image: ResMut<FigureImage>,
    mut size: Local<UVec2>,
    mut camera: Query<(Entity, &mut Camera, &mut Projection, &mut Transform), With<FigureCamera>>,
) {
    let Ok((entity, mut camera, mut projection, mut transform)) = camera.single_mut() else {
        return;
    };
    let Some(figure) = figure.0.filter(|_| view.in_world) else {
        if camera.is_active {
            camera.is_active = false;
        }
        READY.store(false, Ordering::Relaxed);
        return;
    };

    let (w, h) = (figure.x1 - figure.x0, figure.y1 - figure.y0);
    let wanted = UVec2::new(
        (w * input.scale).round().max(1.0) as u32,
        (h * input.scale).round().max(1.0) as u32,
    );
    if image.0.is_none() || *size != wanted {
        let mut texture =
            Image::new_target_texture(wanted.x, wanted.y, TextureFormat::Rgba8UnormSrgb, None);
        texture.sampler = ImageSampler::nearest();
        let handle = images.add(texture);
        commands
            .entity(entity)
            .insert(RenderTarget::Image(handle.clone().into()));
        image.0 = Some(handle);
        *size = wanted;
    }

    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scaling_mode = ScalingMode::Fixed {
            width: w / figure.size,
            height: h / figure.size,
        };
    }
    let middle = box_height(&view.local) / 2.0 + figure.offset_y;
    let at = FIGURE_ORIGIN + Vec3::new(0.0, middle, CAMERA_DISTANCE);
    if transform.translation != at {
        transform.translation = at;
    }
    if !camera.is_active {
        camera.is_active = true;
    }
    READY.store(true, Ordering::Relaxed);
}

pub struct InventoryFigurePlugin;

impl Plugin for InventoryFigurePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryFigure>()
            .init_resource::<FigureImage>()
            .add_plugins(ExtractResourcePlugin::<FigureImage>::default())
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, aim_camera);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn figure(mouse: Option<Vec2>) -> Figure {
        Figure {
            x0: 26.0,
            y0: 8.0,
            x1: 75.0,
            y1: 78.0,
            size: 30.0,
            offset_y: 0.0625,
            mouse,
        }
    }

    #[test]
    fn a_centred_cursor_faces_the_figure_forward() {
        let pose = figure(Some(Vec2::new(50.5, 43.0))).pose(&AnimSample::default());
        assert_eq!(pose.body_yaw, 0.0);
        assert_eq!(pose.head_yaw, 0.0);
        assert_eq!(pose.pitch, 0.0);
        assert_eq!(figure(None).angles(), (0.0, 0.0));
    }

    #[test]
    fn the_figure_turns_and_looks_toward_the_cursor() {
        // Up and to the right: the player faces east (negative yaw) and
        // looks up (negative pitch), and the head turns past the body.
        let fig = figure(Some(Vec2::new(90.5, 3.0)));
        let pose = fig.pose(&AnimSample::default());
        let expected = (-1.0f32).atan() * 20.0;
        assert!((pose.body_yaw - expected).abs() < 1e-5);
        assert_eq!(pose.head_yaw, pose.body_yaw);
        assert!((pose.pitch - expected).abs() < 1e-5);
        // Leaning back takes the head away from the camera at +z.
        assert!((fig.lean() * Vec3::Y).z < 0.0);
    }

    #[test]
    fn gliding_keeps_the_head_level() {
        let gliding = AnimSample {
            fall_flying: true,
            ..AnimSample::default()
        };
        let pose = figure(Some(Vec2::new(50.5, 0.0))).pose(&gliding);
        assert_eq!(pose.pitch, 0.0);
    }
}
