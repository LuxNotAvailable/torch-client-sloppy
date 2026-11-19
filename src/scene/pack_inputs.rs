//! Gathers the frame's pack `Inputs` from the session, cameras and options,
//! for the native renderer's shader packs.

use bevy::math::{IVec2, Vec3};
use bevy::prelude::*;

use crate::app::frame_view::FrameView;
use crate::app::systems::{BlockAtlasHandle, Shared, WorldCamera};
use crate::scene::environment::Environment;
use crate::shaderpack::inputs::{EyeIn, Inputs, Vehicle, Vitals, view_vector};

fn in_shallow_water(feet: Vec3, eye_height: f32) -> bool {
    use crate::util::block_model::{EyeFluid, eye_fluid_at};
    let water = |at: Vec3| {
        eye_fluid_at(f64::from(at.x), f64::from(at.y), f64::from(at.z)) == Some(EyeFluid::Water)
    };
    water(feet) && !water(feet + Vec3::Y * eye_height)
}

fn fading(duration: i32) -> f32 {
    if duration < 0 {
        1.0
    } else {
        (duration as f32 / 20.0).min(1.0)
    }
}

fn night_vision_scale(duration: i32, partial: f32) -> f32 {
    if duration < 0 || duration > 200 {
        1.0
    } else {
        0.7 + ((duration as f32 - partial) * std::f32::consts::PI * 0.2).sin() * 0.3
    }
}

fn held_block_light(item: &str) -> i32 {
    use std::str::FromStr;
    azalea_registry::builtin::BlockKind::from_str(item).map_or(0, |kind| {
        i32::from(crate::lighting::props::of(azalea::block::BlockState::from(kind)).emission)
    })
}

struct Player {
    riding: bool,
    vehicle_id: i32,
    vehicle_at: Option<(Vec3, Vec3, f32)>,
    vitals: Vitals,
    spectator: bool,
    elytra_flying: bool,
    swimming: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn gather_inputs(
    shared: Res<Shared>,
    view: Res<FrameView>,
    environment: Res<Environment>,
    camera: Query<(&Transform, Option<&crate::scene::sky::WorldFog>), With<WorldCamera>>,
    atlas: Option<Res<BlockAtlasHandle>>,
    images: Res<Assets<Image>>,
    gui: Res<crate::gui::GuiState>,
    third_person: Res<crate::app::input::ThirdPersonState>,
    freecam: Res<crate::app::input::FreecamState>,
    mut inputs: ResMut<Inputs>,
) {
    // The native renderer fills pack ids from its own install.
    let item_ids: Option<&std::collections::HashMap<String, i32>> = None;
    let entity_ids: Option<&std::collections::HashMap<String, i32>> = None;
    let old_hand_light = true;
    let partial = view.partial;
    let day_ticks = view.day_ticks;
    let pos = Vec3::from(view.player_lerp);
    let eye = pos + Vec3::Y * view.eye_height;
    let body_vector = view_vector(-view.camera_pitch, -view.camera_yaw - 180.0);
    let (effects, held_items, held_light, vitals, elytra_flying, swimming) = {
        let Ok(state) = shared.0.lock();
        let session = &state.session;
        let effect = |name: &str| {
            session
                .vitals
                .active_effects
                .iter()
                .find(|e| e.id == name)
                .map(|e| e.duration)
        };
        let held = |slot: usize| {
            let item = session.inventory.hotbar.get(slot).map_or("", |s| s.item);
            item_ids
                .and_then(|ids| ids.get(item))
                .copied()
                .unwrap_or(-1)
        };
        let light = |slot: usize| {
            session
                .inventory
                .hotbar
                .get(slot)
                .map_or(0, |s| held_block_light(s.item))
        };
        let (main, off) = (
            light(usize::from(session.inventory.hotbar_selected)),
            light(9),
        );
        (
            (
                effect("blindness"),
                effect("night_vision"),
                effect("darkness"),
            ),
            [
                held(usize::from(session.inventory.hotbar_selected)),
                held(9),
            ],
            [if old_hand_light { main.max(off) } else { main }, off],
            Vitals::of(
                session.gamemode.is_survival(),
                session.vitals.health,
                if session.vitals.health_display.max_health > 0.0 {
                    session.vitals.health_display.max_health
                } else {
                    crate::session::VANILLA_MAX_HEALTH
                },
                session.vitals.food,
                session.vitals.air_supply,
                crate::session::MAX_AIR_SUPPLY,
                session.vitals.health_display.armor,
            ),
            session.fall_flying,
            session.swimming,
        )
    };
    let vehicle_entity = crate::client::tracking::local_vehicle_id();
    let vehicle = vehicle_entity.and_then(|id| view.entities.iter().find(|e| e.id == id));
    let player = Player {
        riding: vehicle_entity.is_some(),
        vehicle_id: vehicle.zip(entity_ids).map_or(0, |(e, ids)| {
            let name = e.kind.to_str();
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            if ids.is_empty() {
                0
            } else {
                ids.get(name).copied().unwrap_or(-1)
            }
        }),
        vehicle_at: vehicle.map(|e| {
            let (y_rot, x_rot) = e.look(1.0);
            (
                Vec3::from(e.position(partial)),
                view_vector(x_rot, y_rot),
                azalea::entity::dimensions::EntityDimensions::from(e.kind).eye_height,
            )
        }),
        vitals,
        spectator: view.gamemode == crate::session::Gamemode::Spectator,
        elytra_flying,
        swimming,
    };
    let bolt = view
        .entities
        .iter()
        .find(|e| e.kind == azalea_registry::builtin::EntityKind::LightningBolt)
        .map(|e| Vec3::from(e.position(partial)));

    let (camera_at, fog) = match camera.single() {
        Ok((transform, fog)) => (transform.translation, fog),
        Err(_) => (eye, None),
    };
    let fog_color = fog.map_or(Vec3::ZERO, |f| {
        let c = f.color.to_srgba();
        Vec3::new(c.red, c.green, c.blue)
    });
    let fog_range = fog.map_or(bevy::math::Vec2::ZERO, |f| f.env);
    let [r, g, b, _] = environment.sky.sky_color.to_rgba_f32();
    let (block, sky) = crate::lighting::lightmap::levels_at(eye.to_array());
    let eye_in =
        match crate::util::block_model::eye_fluid_at(eye.x as f64, eye.y as f64, eye.z as f64) {
            Some(crate::util::block_model::EyeFluid::Water) => EyeIn::Water,
            Some(crate::util::block_model::EyeFluid::Lava) => EyeIn::Lava,
            None if crate::util::block_model::powder_snow_at(
                camera_at.x as f64,
                camera_at.y as f64,
                camera_at.z as f64,
            ) =>
            {
                EyeIn::PowderSnow
            }
            None => EyeIn::Air,
        };
    let atlas_size = atlas
        .and_then(|handle| images.get(&handle.0))
        .map_or(IVec2::ZERO, |image| image.size().as_ivec2());

    let row = crate::client::envprobe::camera_biome();
    let climate = row.map(|row| {
        (
            crate::util::biome_color::env(row),
            crate::util::biome_color::downfall(row),
        )
    });
    let dimension = crate::client::dimension::current();
    let sea_level = crate::client::dimension::sea_level();
    let precipitation = climate.map_or(0, |(env, _)| {
        use crate::scene::environment::{Precipitation, precipitation_at};
        match precipitation_at(env, pos.y.floor() as i32, sea_level) {
            Precipitation::None => 0,
            Precipitation::Rain => 1,
            Precipitation::Snow => 2,
        }
    });

    let (min_y, max_y) = dimension.y_range();
    let vehicle = player
        .vehicle_at
        .map_or_else(Vehicle::default, |(at, look, eye_height)| Vehicle {
            id: player.vehicle_id,
            in_water: in_shallow_water(at, eye_height),
            look,
            relative: camera_at - at,
        });

    *inputs = Inputs {
        day_ticks,
        sun_angle: environment.sky.sun_angle,
        moon_angle: environment.sky.moon_angle,
        moon_phase: environment.sky.moon_phase as i32,
        sky_color: Vec3::new(r, g, b),
        fog_color,
        fog_range,
        screen_brightness: gui.options.gamma as f32 / 100.0,
        body_vector,
        rainfall: climate.map_or(0.0, |(_, downfall)| downfall),
        temperature: climate.map_or(0.0, |(env, _)| env.temperature),
        precipitation,
        rain: environment.rain,
        warp: crate::scene::view_bob::current_warp(&view),
        thunder: environment.thunder,
        eye_in,
        eye_light: IVec2::new(i32::from(block), i32::from(sky)),
        blindness: effects.0.map_or(0.0, fading),
        night_vision: effects.1.map_or(0.0, |d| night_vision_scale(d, partial)),
        darkness: effects.2.map_or(0.0, fading),
        biome: row,
        atlas_size,
        relative_eye: camera_at - eye,
        hide_gui: gui.gui_hidden(),
        held_items,
        held_light,
        lightning: bolt.map_or(bevy::math::Vec4::ZERO, |at| (at - camera_at).extend(1.0)),
        bedrock_level: min_y,
        height_limit: max_y - min_y,
        logical_height: dimension.logical_height(),
        sea_level,
        has_ceiling: dimension.has_ceiling(),
        has_skylight: dimension.has_skylight(),
        vitals: player.vitals,
        spectator: player.spectator,
        first_person: !third_person.active && !freecam.active,
        riding: player.riding,
        elytra_flying: player.elytra_flying,
        swimming: player.swimming,
        feet_in_water: in_shallow_water(pos, eye.y - pos.y),
        eye_position: eye,
        vehicle,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_fade_in_their_last_second() {
        assert_eq!(fading(-1), 1.0);
        assert_eq!(fading(400), 1.0);
        assert_eq!(fading(10), 0.5);
        assert_eq!(night_vision_scale(400, 0.0), 1.0);
        assert!(night_vision_scale(100, 0.0) <= 1.0);
    }
}

pub(crate) struct PackInputsPlugin;

impl Plugin for PackInputsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inputs>().add_systems(
            Update,
            (
                gather_inputs.after(crate::app::frame_view::FrameViewSystems),
                sync_render_scale,
            ),
        );
    }
}

fn sync_render_scale(gui: Res<crate::gui::GuiState>) {
    crate::shaderpack::frame_state::set_options(
        gui.options.pack_render_scale,
        gui.options.shadow_update,
    );
}
