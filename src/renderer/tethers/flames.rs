//! The fire on burning entities (`FlameFeatureRenderer`): you seen from
//! outside, other players, and any entity showing fire.

use super::*;

pub(super) fn draw_flames(
    view: Res<FrameView>,
    camera: CameraQuery,
    third_person: Res<crate::renderer::input::ThirdPersonState>,
    time: Res<Time>,
    sampled: Res<crate::renderer::frame_view::SampledEntities>,
    mut out: LayerOut,
) {
    let (mut fire0, mut fire1) = (Geo::default(), Geo::default());
    if view.in_world {
        let (_, cam_rot, _) = camera_pose(&camera);
        let frame = fire_frame(time.elapsed_secs());
        let partial = view.partial;
        let mut burn = |pos: Vec3, width: f32, height: f32| {
            flames(&mut fire0, &mut fire1, pos, width, height, cam_rot, frame);
        };
        if third_person.active && view.local.on_fire {
            let height = if view.local.crouching { 1.5 } else { 1.8 };
            burn(Vec3::from(view.player_lerp), 0.6, height);
        }
        for (player, sample) in view.players.iter().zip(&sampled.players) {
            if sample.on_fire {
                let height = if sample.crouching { 1.5 } else { 1.8 };
                burn(Vec3::from(player.anim.position(partial)), 0.6, height);
            }
        }
        for (anim, st) in view.entities.iter().zip(&sampled.states) {
            if st.display_fire && !st.is_invisible {
                burn(
                    Vec3::from(anim.position(partial)),
                    st.bounding_box_width,
                    st.bounding_box_height,
                );
            }
        }
    }
    out.put(Kind::Fire0, fire0);
    out.put(Kind::Fire1, fire1);
}

/// The fire textures' frame this instant: 32 frames a tick each, starting
/// on frame 16 (their `.mcmeta`).
fn fire_frame(seconds: f32) -> u32 {
    ((seconds * 20.0) as u32 + 16) % 32
}

/// `FlameFeatureRenderer`: planes 1.4 tall stacked up the body, alternating
/// the two fire textures, each 10% narrower and further back, turned to the
/// camera about the vertical.
fn flames(
    fire0: &mut Geo,
    fire1: &mut Geo,
    pos: Vec3,
    width: f32,
    height: f32,
    cam: Quat,
    frame: u32,
) {
    let s = width * 1.4;
    let (yaw, _, _) = cam.to_euler(EulerRot::YXZ);
    let turn = Quat::from_rotation_y(yaw);
    let mut h = height / s;
    let mut r = 0.5;
    let (mut yo, mut zo) = (0.0f32, 0.0f32);
    let z0 = 0.3 - (h as i32) as f32 * 0.02;
    let (v0, v1) = (frame as f32 / 32.0, (frame + 1) as f32 / 32.0);
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut layer = 0;
    while h > 0.0 {
        let (mut u0, mut u1) = (0.0f32, 1.0f32);
        if layer / 2 % 2 == 0 {
            std::mem::swap(&mut u0, &mut u1);
        }
        let p = |x: f32, y: f32| pos + turn * (Vec3::new(x, y - yo, z0 + zo) * s);
        let geo = if layer % 2 == 0 {
            &mut *fire0
        } else {
            &mut *fire1
        };
        geo.quad([
            (p(-r, 0.0), [u1, v1], white),
            (p(r, 0.0), [u0, v1], white),
            (p(r, 1.4), [u0, v0], white),
            (p(-r, 1.4), [u1, v0], white),
        ]);
        h -= 0.45;
        yo -= 0.45;
        r *= 0.9;
        zo -= 0.03;
        layer += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burning_player_gets_five_alternating_flame_planes() {
        let (mut a, mut b) = (Geo::default(), Geo::default());
        flames(&mut a, &mut b, Vec3::ZERO, 0.6, 1.8, Quat::IDENTITY, 16);
        // 1.8 / (0.6 * 1.4) = 2.14 units, a plane every 0.45: five planes.
        assert_eq!((a.pos.len() / 4, b.pos.len() / 4), (3, 2));
        assert_eq!(fire_frame(0.0), 16);
        assert_eq!(fire_frame(0.8), 0);
    }
}
