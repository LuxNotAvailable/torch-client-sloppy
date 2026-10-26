use azalea_registry::builtin::EntityKind;
use bevy::math::Vec3;
use bevy::prelude::{Mesh, Transform};

use crate::entities::quads::billboard_quad;
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::{Blend, CameraView, RenderSpec, RootPose, TexturePath};

const ORB_ICONS_PER_ROW: u32 = 4;
const ORB_ICON_SIZE: f32 = 16.0 / 64.0;

pub fn register(registry: &mut Registry) {
    registry.add(
        EntityKind::DragonFireball,
        RenderSpec::built(
            "dragon_fireball",
            constant_key,
            fireball_mesh,
            fireball_texture,
        )
        .with_camera_root(fireball_root)
        .ignoring_invisibility(),
    );
    registry.add(
        EntityKind::ExperienceOrb,
        RenderSpec::built("experience_orb", orb_key, orb_mesh, orb_texture)
            .with_camera_root(orb_root)
            .with_blend(Blend::Translucent)
            .with_tint(orb_tint)
            .ignoring_invisibility(),
    );
    registry.add(
        EntityKind::FishingBobber,
        RenderSpec::built("fishing_hook", constant_key, hook_mesh, hook_texture)
            .with_camera_root(hook_root)
            .with_visible(has_owner)
            .ignoring_invisibility(),
    );
}

fn constant_key(_st: &EntityState) -> TexturePath {
    "quad".into()
}

fn facing_camera(translation: Vec3, scale: f32, camera: &CameraView) -> RootPose {
    RootPose {
        hook: Transform {
            translation,
            rotation: camera.orientation,
            scale: Vec3::splat(scale),
        },
        ..RootPose::default()
    }
}

fn fireball_texture(_st: &EntityState) -> Option<TexturePath> {
    Some("entity/enderdragon/dragon_fireball".into())
}

fn fireball_mesh(_st: &EntityState) -> Mesh {
    billboard_quad(0.25, [0.0, 0.0, 1.0, 1.0])
}

fn fireball_root(_st: &EntityState, camera: &CameraView) -> RootPose {
    facing_camera(Vec3::ZERO, 2.0, camera)
}

fn orb_icon(value: i32) -> u32 {
    const THRESHOLDS: [i32; 10] = [3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477];
    THRESHOLDS.iter().filter(|&&t| value >= t).count() as u32
}

fn orb_key(st: &EntityState) -> TexturePath {
    orb_icon(st.extras.xp_value).to_string().into()
}

fn orb_texture(_st: &EntityState) -> Option<TexturePath> {
    Some("entity/experience/experience_orb".into())
}

fn orb_mesh(st: &EntityState) -> Mesh {
    let icon = orb_icon(st.extras.xp_value);
    let u0 = (icon % ORB_ICONS_PER_ROW) as f32 * ORB_ICON_SIZE;
    let v0 = (icon / ORB_ICONS_PER_ROW) as f32 * ORB_ICON_SIZE;
    billboard_quad(0.25, [u0, v0, u0 + ORB_ICON_SIZE, v0 + ORB_ICON_SIZE])
}

fn orb_root(_st: &EntityState, camera: &CameraView) -> RootPose {
    facing_camera(Vec3::new(0.0, 0.1, 0.0), 0.3, camera)
}

fn orb_tint(st: &EntityState) -> [f32; 4] {
    let phase = st.age_ticks / 2.0;
    let red = ((phase.sin() + 1.0) * 0.5 * 255.0) as i32 as f32 / 255.0;
    let blue = (((phase + 4.188_790_3).sin() + 1.0) * 0.1 * 255.0) as i32 as f32 / 255.0;
    [red, 1.0, blue, 128.0 / 255.0]
}

fn hook_texture(_st: &EntityState) -> Option<TexturePath> {
    Some("entity/fishing/fishing_hook".into())
}

fn hook_mesh(_st: &EntityState) -> Mesh {
    billboard_quad(0.5, [0.0, 0.0, 1.0, 1.0])
}

fn hook_root(_st: &EntityState, camera: &CameraView) -> RootPose {
    facing_camera(Vec3::ZERO, 0.5, camera)
}

fn has_owner(st: &EntityState) -> bool {
    st.extras.hook_owner.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_orb_icon_steps_at_the_vanilla_thresholds() {
        assert_eq!(orb_icon(0), 0);
        assert_eq!(orb_icon(2), 0);
        assert_eq!(orb_icon(3), 1);
        assert_eq!(orb_icon(16), 2);
        assert_eq!(orb_icon(17), 3);
        assert_eq!(orb_icon(2476), 9);
        assert_eq!(orb_icon(2477), 10);
    }
}
