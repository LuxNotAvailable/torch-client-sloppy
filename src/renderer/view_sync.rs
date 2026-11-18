//! Copies the Bevy cameras into `client::view::View` each frame.

use bevy::camera::{ClearColorConfig, ScalingMode};
use bevy::prelude::*;

use super::frame_view::FrameView;
use super::hand::HandCamera;
use super::inventory_figure::{FigureCamera, FigureImage};
use super::panorama::PanoramaCamera;
use super::systems::WorldCamera;
use crate::client::view::{Eye, Figure, Fog, View, ViewSystems};

pub struct ViewSyncPlugin;

impl Plugin for ViewSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<View>().add_systems(
            PostUpdate,
            fill.in_set(ViewSystems)
                .after(bevy::transform::TransformSystems::Propagate),
        );
    }
}

#[allow(clippy::too_many_arguments, reason = "a system's queries")]
fn fill(
    mut view: ResMut<View>,
    frame_view: Res<FrameView>,
    panorama: Query<&Camera, With<PanoramaCamera>>,
    world: Query<(&GlobalTransform, &Projection, &Camera, Option<&DistanceFog>), With<WorldCamera>>,
    hand: Query<(&GlobalTransform, &Projection), With<HandCamera>>,
    figure: Query<(&GlobalTransform, &Projection), With<FigureCamera>>,
    figure_image: Option<Res<FigureImage>>,
    images: Res<Assets<Image>>,
) {
    let view = &mut *view;
    view.menu = panorama.iter().next().is_some_and(|c| c.is_active);
    let world = world.iter().next();
    view.world = world.and_then(|(t, p, ..)| match p {
        Projection::Perspective(p) => Some(Eye {
            world_from_view: t.to_matrix(),
            fov: p.fov,
        }),
        _ => None,
    });
    view.fog = world.and_then(|(_, _, camera, fog)| fog.map(|fog| fog_of(fog, camera)));
    let hand = hand.iter().next();
    view.hand_clip_from_view = hand.map(|(_, p)| p.get_clip_from_view());
    view.hand_view_from_world = hand.map(|(t, _)| t.to_matrix().inverse());
    view.figure_ready = super::inventory_figure::ready();
    view.figure = view
        .figure_ready
        .then(|| figure_of(&figure, figure_image.as_deref()?, &images))
        .flatten();
    view.warp = super::view_bob::current_warp(&frame_view);
}

fn fog_of(fog: &DistanceFog, camera: &Camera) -> Fog {
    let clear = match camera.clear_color {
        ClearColorConfig::Custom(c) => c.to_linear().to_f32_array(),
        _ => fog.color.to_linear().to_f32_array(),
    };
    let (start, end) = match fog.falloff {
        bevy::pbr::FogFalloff::Linear { start, end } => (start, end),
        _ => (0.0, f32::MAX),
    };
    let env = fog.directional_light_color.to_linear();
    Fog {
        clear,
        color: fog.color.to_linear().to_f32_array(),
        env: [env.red, env.green],
        start,
        end,
    }
}

fn figure_of(
    camera: &Query<(&GlobalTransform, &Projection), With<FigureCamera>>,
    image: &FigureImage,
    images: &Assets<Image>,
) -> Option<Figure> {
    let size = images.get(image.0.as_ref()?)?.texture_descriptor.size;
    let (transform, projection) = camera.iter().next()?;
    let Projection::Orthographic(o) = projection else {
        return None;
    };
    let ScalingMode::Fixed {
        width: w,
        height: h,
    } = o.scaling_mode
    else {
        return None;
    };
    // Bevy's orthographic projection, depth reversed as the world's.
    let proj = Mat4::orthographic_rh(-w / 2.0, w / 2.0, -h / 2.0, h / 2.0, o.far, o.near);
    Some(Figure {
        clip_from_world: proj * transform.to_matrix().inverse(),
        eye: transform.translation(),
        size: [size.width, size.height],
    })
}
