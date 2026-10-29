//! Camera-facing sprites: the experience orb (`ExperienceOrbRenderer`), the
//! ender dragon's fireball (`DragonFireballRenderer`) and the fishing bobber
//! (`FishingHookRenderer`; its line is drawn with the other tethers).

use azalea_registry::builtin::EntityKind;
use bevy::math::{Quat, Vec3};
use bevy::prelude::{Mesh, Transform};

use crate::entities::quads::billboard_quad;
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::{Blend, CameraView, RenderSpec, RootPose, TexturePath};

pub fn register(registry: &mut Registry) {
    registry.add(
        EntityKind::ExperienceOrb,
        RenderSpec::built("experience_orb", orb_key, orb_mesh, orb_texture)
            .with_camera_root(orb_root)
            .with_tint(orb_tint)
            .with_blend(Blend::Translucent),
    );
    registry.add(
        EntityKind::DragonFireball,
        RenderSpec::built("dragon_fireball", fireball_key, fireball_mesh, fireball_texture)
            .with_camera_root(fireball_root),
    );
    registry.add(
        EntityKind::FishingBobber,
        RenderSpec::built("fishing_bobber", bobber_key, bobber_mesh, bobber_texture)
            .with_camera_root(bobber_root),
    );
}

/// A sprite turned to face the camera, `lift` above the entity, `scale`
/// blocks across.
fn facing_camera(camera: &CameraView, lift: f32, scale: f32) -> RootPose {
    RootPose {
        scale: 1.0,
        hook: Transform {
            translation: Vec3::new(0.0, lift, 0.0),
            rotation: camera.orientation,
            scale: Vec3::splat(scale),
        },
        rotation: Quat::IDENTITY,
        ..RootPose::default()
    }
}

/// `ExperienceOrb.getIcon`: the sprite for an orb's value, 0 to 10.
pub fn orb_icon(value: i32) -> u32 {
    const STEPS: [i32; 10] = [3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477];
    STEPS.iter().filter(|&&step| value >= step).count() as u32
}

fn orb_key(st: &EntityState) -> TexturePath {
    format!("experience_orb:{}", orb_icon(st.extras.xp_value)).into()
}

fn orb_mesh(st: &EntityState) -> Mesh {
    let icon = orb_icon(st.extras.xp_value);
    let (u, v) = ((icon % 4 * 16) as f32 / 64.0, (icon / 4 * 16) as f32 / 64.0);
    billboard_quad(0.25, [u, v, u + 0.25, v + 0.25])
}

fn orb_texture(_st: &EntityState) -> Option<TexturePath> {
    Some("entity/experience/experience_orb".into())
}

fn orb_root(_st: &EntityState, camera: &CameraView) -> RootPose {
    facing_camera(camera, 0.1, 0.3)
}

/// The orb's pulse between green and yellow.
fn orb_tint(st: &EntityState) -> [f32; 4] {
    let t = st.age_ticks / 2.0;
    let red = (t.sin() + 1.0) * 0.5;
    let blue = ((t + std::f32::consts::PI * 4.0 / 3.0).sin() + 1.0) * 0.1;
    [red, 1.0, blue, 1.0]
}

fn fireball_key(_st: &EntityState) -> TexturePath {
    "dragon_fireball".into()
}

fn fireball_mesh(_st: &EntityState) -> Mesh {
    billboard_quad(0.25, [0.0, 0.0, 1.0, 1.0])
}

fn fireball_texture(_st: &EntityState) -> Option<TexturePath> {
    Some("entity/enderdragon/dragon_fireball".into())
}

fn fireball_root(_st: &EntityState, camera: &CameraView) -> RootPose {
    facing_camera(camera, 0.0, 2.0)
}

fn bobber_key(_st: &EntityState) -> TexturePath {
    "fishing_bobber".into()
}

fn bobber_mesh(_st: &EntityState) -> Mesh {
    billboard_quad(0.5, [0.0, 0.0, 1.0, 1.0])
}

fn bobber_texture(_st: &EntityState) -> Option<TexturePath> {
    Some("entity/fishing/fishing_hook".into())
}

fn bobber_root(_st: &EntityState, camera: &CameraView) -> RootPose {
    facing_camera(camera, 0.0, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orb_icons_follow_the_value_thresholds() {
        assert_eq!(orb_icon(0), 0);
        assert_eq!(orb_icon(3), 1);
        assert_eq!(orb_icon(16), 2);
        assert_eq!(orb_icon(17), 3);
        assert_eq!(orb_icon(2477), 10);
        assert_eq!(orb_icon(99999), 10);
    }
}
