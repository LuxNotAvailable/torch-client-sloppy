//! View bobbing: `GameRenderer.bobView`. Walking sways the view side to side
//! and dips it with each step. The world sees it through the camera, the
//! hand through the node its poses hang from, as vanilla applies the same
//! pose to the projection and to the hand.

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::frame_view::FrameView;
use super::input::FreecamState;
use super::systems::WorldCamera;

/// The node between the hand camera and the hand poses, which carries the
/// bob.
#[derive(Component)]
pub struct HandBob;

/// `bobView`'s pose, in view space, for a backwards walk distance and a bob.
pub fn bob_pose(walk: f32, bob: f32) -> Transform {
    use std::f32::consts::PI;
    let swing = (walk * PI).sin();
    let dip = (walk * PI).cos();
    Transform::from_xyz(swing * bob * 0.5, -(dip * bob).abs(), 0.0).with_rotation(
        Quat::from_rotation_z((swing * bob * 3.0).to_radians())
            * Quat::from_rotation_x((((walk * PI - 0.2).cos() * bob).abs() * 5.0).to_radians()),
    )
}

/// When the local player was last hurt, and the direction the hit came from
/// (`hurtTime`, `hurtDir`).
static HURT: std::sync::Mutex<Option<(crate::platform::time::Instant, f32)>> =
    std::sync::Mutex::new(None);

const HURT_TICKS: f32 = 10.0;

/// The local player was hurt (`animateHurt`); `dir` is the hit's yaw when the
/// server sent one (`ClientboundHurtAnimationPacket`), which a plain damage
/// event leaves as it was.
pub fn note_local_hurt(dir: Option<f32>) {
    let mut hurt = HURT.lock().unwrap();
    let dir = dir.unwrap_or_else(|| hurt.map_or(0.0, |(_, d)| d));
    *hurt = Some((crate::platform::time::Instant::now(), dir));
}

/// A new player entity (joining, respawning) starts with no hurt and a
/// hurt direction of 0, as vanilla's new `LocalPlayer` does; fall, fire and
/// other knockback-free damage reuse the last direction.
pub fn reset_hurt() {
    *HURT.lock().unwrap() = None;
}

/// `bobHurt`: the view rolls toward the hit and back over the hurt time, and
/// leans over as the player dies.
pub fn hurt_pose(hurt_ticks: Option<(f32, f32)>, death_ticks: f32) -> Quat {
    let mut rotation = Quat::IDENTITY;
    if death_ticks > 0.0 {
        let duration = death_ticks.min(20.0);
        rotation *= Quat::from_rotation_z((40.0 - 8000.0 / (duration + 200.0)).to_radians());
    }
    if let Some((left, dir)) = hurt_ticks
        && left >= 0.0
    {
        let t = left / HURT_TICKS;
        let hurt = (t * t * t * t * std::f32::consts::PI).sin();
        let turn = Quat::from_rotation_y((-dir).to_radians());
        // `damageTiltStrength`.
        let strength = crate::gui::more_options::accessibility().damage_tilt as f32 / 100.0;
        rotation *=
            turn * Quat::from_rotation_z((-hurt * 14.0 * strength).to_radians()) * turn.inverse();
    }
    rotation
}

/// Ticks of hurt left, with the hit's direction.
fn hurt_now() -> Option<(f32, f32)> {
    let (at, dir) = (*HURT.lock().unwrap())?;
    Some((HURT_TICKS - at.elapsed().as_secs_f32() * 20.0, dir))
}

fn current_pose(
    view: &FrameView,
    state: &crate::gui::GuiState,
    freecam: &FreecamState,
) -> Transform {
    if freecam.active || !view.in_world {
        return Transform::IDENTITY;
    }
    // `renderLevel`: the hurt roll, then the bob.
    let hurt = Transform::from_rotation(hurt_pose(hurt_now(), view.local.death_time));
    // Zooming holds the view still.
    if !state.options.view_bobbing || state.zooming || view.sleeping {
        return hurt;
    }
    let (walk, bob) = view.view_bob;
    hurt * bob_pose(walk, bob)
}

/// The world: the projection times the bob is the camera moved by the
/// bob's inverse. The camera's transform is set afresh every frame, so this
/// never builds up.
fn bob_world_camera(
    view: Res<FrameView>,
    state: Res<crate::gui::GuiState>,
    freecam: Res<FreecamState>,
    mut camera: Query<&mut Transform, With<WorldCamera>>,
) {
    let pose = current_pose(&view, &state, &freecam);
    if pose == Transform::IDENTITY {
        return;
    }
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let inverse = pose.compute_affine().inverse();
    *transform = Transform::from_matrix((transform.compute_affine() * inverse).into());
}

fn bob_hand(
    view: Res<FrameView>,
    state: Res<crate::gui::GuiState>,
    freecam: Res<FreecamState>,
    mut node: Query<&mut Transform, With<HandBob>>,
) {
    let pose = current_pose(&view, &state, &freecam);
    for mut transform in &mut node {
        if *transform != pose {
            *transform = pose;
        }
    }
}

/// This frame's portal and nausea warp, in view space: `renderLevel` puts it
/// on the world's projection.
pub(crate) fn current_warp(view: &FrameView) -> Mat4 {
    view.in_world
        .then(|| {
            crate::play::screen_warp::view_warp(crate::play::screen_warp::sample(view.partial))
        })
        .flatten()
        .unwrap_or(Mat4::IDENTITY)
}

/// The warp, snapshotted for the render world.
#[derive(Resource, Clone, Copy, Default)]
struct ViewWarp(Mat4);

fn snapshot_warp(view: Res<FrameView>, mut warp: ResMut<ViewWarp>) {
    warp.0 = current_warp(&view);
}

/// Puts the warp on the world camera's extracted projection, so the camera
/// itself stays rigid for everything that reads its transform.
fn warp_projection(
    warp: bevy::render::Extract<Res<ViewWarp>>,
    camera: bevy::render::Extract<
        Query<&bevy::render::sync_world::RenderEntity, With<WorldCamera>>,
    >,
    mut views: Query<&mut bevy::render::view::ExtractedView>,
) {
    if warp.0 == Mat4::IDENTITY {
        return;
    }
    for render in &camera {
        if let Ok(mut view) = views.get_mut(render.id()) {
            view.clip_from_view *= warp.0;
        }
    }
}

pub struct ViewBobPlugin;

impl Plugin for ViewBobPlugin {
    fn build(&self, app: &mut App) {
        use crate::renderer::AppState;
        app.add_systems(
            PostUpdate,
            (bob_world_camera, bob_hand)
                .before(super::hand::follow_world_camera)
                .before(TransformSystems::Propagate)
                .run_if(in_state(AppState::InGame)),
        );
        app.init_resource::<ViewWarp>()
            .add_systems(PostUpdate, snapshot_warp);
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(
                bevy::render::ExtractSchedule,
                warp_projection.after(bevy::render::camera::extract_cameras),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hurt_rolls_the_view_and_settles() {
        assert_eq!(hurt_pose(None, 0.0), Quat::IDENTITY);
        // The roll peaks part way through and is gone at the end.
        let mid = hurt_pose(Some((8.5, 0.0)), 0.0);
        assert!(mid.angle_between(Quat::IDENTITY) > 0.1, "{mid:?}");
        let done = hurt_pose(Some((0.0, 0.0)), 0.0);
        assert!(done.angle_between(Quat::IDENTITY) < 1e-4);
        assert_eq!(hurt_pose(Some((-1.0, 0.0)), 0.0), Quat::IDENTITY);
    }

    #[test]
    fn standing_still_does_not_bob() {
        assert_eq!(bob_pose(3.7, 0.0), Transform::IDENTITY);
    }

    #[test]
    fn a_step_dips_the_view_and_sways_it() {
        let pose = bob_pose(0.5, 0.1);
        assert!((pose.translation.x - 0.05).abs() < 1e-6, "{pose:?}");
        assert!(pose.translation.y <= 0.0);
        let pose = bob_pose(0.0, 0.1);
        assert!((pose.translation.y + 0.1).abs() < 1e-6, "{pose:?}");
    }

    #[test]
    fn the_bob_eases_toward_the_walking_speed() {
        let mut bob = crate::session::ViewBob::default();
        bob.tick(0.2, 0.2, true, false);
        assert!((bob.bob - 0.04).abs() < 1e-6);
        assert!((bob.walk - 0.12).abs() < 1e-6);
        bob.tick(0.0, 0.0, false, true);
        assert_eq!(bob.bob, 0.0);
        let (walk, eased) = bob.sample(0.5);
        assert!((walk + 0.12).abs() < 1e-6);
        assert!((eased - 0.0).abs() < 1e-6 || eased <= 0.04);
    }
}
