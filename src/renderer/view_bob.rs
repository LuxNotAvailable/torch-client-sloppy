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

fn current_pose(view: &FrameView, state: &crate::gui::GuiState, freecam: &FreecamState) -> Transform {
    // Zooming holds the view still.
    if !state.options.view_bobbing
        || state.zooming
        || freecam.active
        || view.sleeping
        || !view.in_world
    {
        return Transform::IDENTITY;
    }
    let (walk, bob) = view.view_bob;
    bob_pose(walk, bob)
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
