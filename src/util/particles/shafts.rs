//! Light shafts: soft beams of sun or moonlight through gaps overhead, as
//! particles. While the light may show (the gates in `tick_shafts`),
//! probes drop from 40 blocks up along the light until a block stops them;
//! a second ray a block aside that goes deeper has found a gap, and its
//! deeper stretch becomes a beam that fades in, lingers and fades out.

use azalea::block::fluid_state::FluidState;
use azalea_core::position::{BlockPos, Vec3 as AzVec3};
use azalea_physics::collision::BlockWithShape;
use bevy::math::DVec3;

use super::shaft_profile::{self, Weather};
use super::*;

/// Probes a second (at All particles).
const PROBES_PER_SECOND: f32 = 400.0;
/// How far around the camera probes fall, in blocks.
const RADIUS: f32 = 30.0;
/// How high above the camera, along the light, a probe starts.
const START: f32 = 40.0;
/// How far a probe walks before giving up, in blocks.
const MAX_WALK: i32 = 60;
/// Beams shorter than this (in height) aren't kept.
const MIN_HEIGHT: f32 = 3.5;
const MAX_BEAMS: usize = 4000;
/// Half the beam's width, in blocks.
const HALF_WIDTH: f32 = 1.0;
/// How far the player moves before the biome is read again.
const ANCHOR_RADIUS: f32 = 3.0;
/// The light's height above the horizon (degrees) that starts the shafts,
/// and the lower one that keeps them going.
const START_ELEVATION: f32 = 17.5;
const KEEP_ELEVATION: f32 = 15.0;

struct Beam {
    from: Vec3,
    to: Vec3,
    age: f32,
    life: f32,
    moon: bool,
    tint: [f32; 3],
}

#[derive(Resource, Default)]
pub(super) struct Shafts {
    beams: Vec<Beam>,
    /// Probes owed to the next tick.
    owed: f32,
    /// Whether shafts are spawning (the gates have a lower bar to stay).
    active: bool,
    /// The profile's tint id while active.
    profile_id: i32,
    /// Opacity by day and night (percent: beams' alpha scales by it, to
    /// at most 1), and the share of the biome tint over the sun's colour.
    day_opacity: f32,
    night_opacity: f32,
    profile_tint: f32,
    /// The sun's height (sine), for the beams' colour.
    sun_y: f32,
    metrics: Metrics,
    /// Where the biome was last read, and the biome.
    anchor: Option<(Vec3, &'static str)>,
    tick: f32,
    /// Seconds since the beam count was logged, and probes since.
    logged: f32,
    probed: u32,
}

/// The light's direction (toward the sun, or the moon at night), and
/// whether it is the moon. Vanilla's sun turns about the z axis.
fn light(sun_angle: f32) -> (Vec3, bool) {
    let sun = Vec3::new(-sun_angle.sin(), sun_angle.cos(), 0.0);
    if sun.y >= 0.0 {
        (sun, false)
    } else {
        (-sun, true)
    }
}

fn az(v: DVec3) -> AzVec3 {
    AzVec3 {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

fn block_id(state: azalea::block::BlockState) -> &'static str {
    crate::blocks::info::info(state).id
}

/// The first block along `from` to `from + dir * len` that `hit` stops at:
/// vanilla's block traversal, which sees every block the segment crosses.
fn traverse<T>(
    world: &azalea_world::World,
    from: DVec3,
    dir: DVec3,
    len: f64,
    hit: impl Fn(azalea::block::BlockState, BlockPos, AzVec3, AzVec3) -> Option<T>,
) -> Option<T> {
    let (a, b) = (az(from), az(from + dir * len));
    azalea_physics::clip::traverse_blocks(
        a,
        b,
        (),
        |_, pos| hit(world.get_block_state(pos)?, pos, a, b).map(Some),
        |_| None,
    )
}

/// Whether the sky toward the light is blocked: anything but air and
/// glass stops it, fluids and shapeless blocks whole, the rest by their
/// outline.
fn sky_blocked(world: &azalea_world::World, from: DVec3, dir: DVec3, len: f64) -> bool {
    traverse(world, from, dir, len, |state, pos, a, b| {
        if state.is_air() || matches!(block_id(state), "glass" | "glass_pane") {
            return None;
        }
        if !FluidState::from(state).is_empty() {
            return Some(());
        }
        let shape = state.outline_shape(pos);
        if shape.is_empty() {
            return Some(());
        }
        shape.clip(a, b, pos).map(|_| ())
    })
    .is_some()
}

/// The first solid block along the ray: leaves, bamboo and anything
/// without collision let it through.
fn solid(
    world: &azalea_world::World,
    from: DVec3,
    dir: DVec3,
    len: f64,
) -> Option<azalea_core::hit_result::BlockHitResult> {
    traverse(world, from, dir, len, |state, pos, a, b| {
        if state.is_air()
            || crate::blocks::tags::has(state, "leaves")
            || block_id(state) == "bamboo"
        {
            return None;
        }
        let shape = state.collision_shape(pos);
        if shape.is_empty() {
            return None;
        }
        shape.clip(a, b, pos)
    })
}

/// Stone, cobblestone and the deep stones.
fn stone(state: azalea::block::BlockState) -> bool {
    use crate::blocks::tags::has;
    if has(state, "base_stone_overworld") || has(state, "c:stones") || has(state, "c:cobblestones")
    {
        return true;
    }
    let id = block_id(state);
    id.contains("deepslate") || id.contains("sculk") || id.contains("tuff")
}

const RAYS: usize = 40;
const RAYS_PER_UPDATE: usize = 2;
const ESCAPE_RAYS: u32 = 3;
const MAX_DISTANCE: f64 = 100.0;

/// What the space around the player looks like, from 40 rays spread over
/// a sphere, two cast a tick: how much of it leads outside, how much ends
/// on stone, and how big it is. Enclosed stone space is a cave.
struct Metrics {
    rng: JavaRandom,
    exposure: [f32; RAYS],
    stone: [bool; RAYS],
    distance: [f32; RAYS],
    order: [usize; RAYS],
    cursor: usize,
    outside_exposure: f32,
    stone_factor: f32,
    enclosure_volume: f32,
}

impl Default for Metrics {
    fn default() -> Metrics {
        let mut m = Metrics {
            rng: JavaRandom::new(0x5EED),
            exposure: [0.0; RAYS],
            stone: [false; RAYS],
            distance: [1.0; RAYS],
            order: std::array::from_fn(|i| i),
            cursor: 0,
            outside_exposure: 0.0,
            stone_factor: 0.0,
            enclosure_volume: 0.0,
        };
        m.shuffle();
        m.summarize();
        m
    }
}

impl Metrics {
    fn shuffle(&mut self) {
        for i in (1..RAYS).rev() {
            let j = self.rng.next_int(i as u32 + 1) as usize;
            self.order.swap(i, j);
        }
    }

    fn fibonacci(i: usize) -> DVec3 {
        let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
        let y = 1.0 - 2.0 * i as f64 / RAYS as f64;
        let a = i as f64 * golden;
        let r = (1.0 - y * y).sqrt();
        DVec3::new(r * a.cos(), y, r * a.sin())
    }

    fn cosine_hemisphere(&mut self) -> DVec3 {
        let a = self.rng.next_f64() * 2.0 * std::f64::consts::PI;
        let z = self.rng.next_f64().sqrt();
        let r = (1.0 - z * z).sqrt();
        DVec3::new(r * a.cos(), r * a.sin(), z)
    }

    fn update(&mut self, world: &azalea_world::World, eye: DVec3, render_distance: u32) {
        let len = (16.0 * f64::from(render_distance + 1)).min(MAX_DISTANCE);
        let from = eye + DVec3::new(0.0, 0.1, 0.0);
        for _ in 0..RAYS_PER_UPDATE {
            let i = self.order[self.cursor];
            let dir = Self::fibonacci(i);
            let mut exposure = 1.0;
            let mut stone_hit = false;
            let mut dist = 0.0;
            if let Some(hit) = solid(world, from, dir, len) {
                let at = DVec3::new(hit.location.x, hit.location.y, hit.location.z);
                dist = at.distance(from) as f32;
                stone_hit = world.get_block_state(hit.block_pos).is_some_and(stone);
                let n = hit.direction.normal_vec3();
                let n = DVec3::new(n.x, n.y, n.z);
                let up = if n.y.abs() > 0.9 { DVec3::Z } else { DVec3::Y };
                let t = n.cross(up).normalize();
                let b = n.cross(t).normalize();
                let back = at + dir * -0.01;
                let mut escaped = 0;
                for _ in 0..ESCAPE_RAYS {
                    let h = self.cosine_hemisphere();
                    let d = t * h.x + b * h.y + n * h.z;
                    if solid(world, back, d, len).is_none() {
                        escaped += 1;
                    }
                }
                exposure = escaped as f32 / ESCAPE_RAYS as f32;
            }
            self.distance[i] = dist;
            self.stone[i] = stone_hit;
            self.exposure[i] = exposure;
            self.cursor = (self.cursor + 1) % RAYS;
            if self.cursor == 0 {
                self.shuffle();
            }
        }
        self.summarize();
    }

    fn summarize(&mut self) {
        let n = RAYS as f32;
        self.outside_exposure = self.exposure.iter().sum::<f32>() / n;
        self.stone_factor = self.stone.iter().filter(|s| **s).count() as f32 / n;
        self.enclosure_volume = self
            .distance
            .iter()
            .map(|d| (4.0 * std::f64::consts::PI * f64::from(*d).powi(3) / 3.0) as f32)
            .sum::<f32>()
            / n;
    }

    /// Enclosed, cut off from outside, and mostly stone.
    fn cave(&self) -> bool {
        self.enclosure_volume > 30.0 && self.outside_exposure < 0.1 && self.stone_factor > 0.7
    }

    /// Enclosed and cut off (sulfur caves' underground test).
    fn underground(&self) -> bool {
        self.enclosure_volume > 30.0 && self.outside_exposure < 0.2
    }
}

/// What a probe walks through: air and glass, as the probe particles
/// expire in anything else.
fn open(state: Option<azalea::block::BlockState>) -> bool {
    let Some(s) = state else { return true };
    s.is_air() || matches!(block_id(s), "glass" | "glass_pane")
}

fn cell(p: Vec3) -> [i32; 3] {
    [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32]
}

/// Steps from `start` along `down`, a block at a time, to the first block
/// that isn't open: the steps taken, past `MAX_WALK` if none.
fn walk(world: &azalea_world::World, start: Vec3, down: Vec3) -> f32 {
    for k in 1..=MAX_WALK {
        let p = start + down * k as f32;
        if !open(block_at(world, cell(p))) {
            return k as f32;
        }
    }
    (MAX_WALK + 1) as f32
}

#[allow(clippy::too_many_arguments, reason = "a system's resources")]
pub(super) fn tick_shafts(
    time: Res<Time>,
    gui: Res<crate::gui::GuiState>,
    env: Res<crate::scene::environment::Environment>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut rng: ResMut<ParticleRng>,
    mut shafts: ResMut<Shafts>,
) {
    let dt = time.delta_secs();
    for b in &mut shafts.beams {
        b.age += dt;
    }
    shafts.beams.retain(|b| b.age < b.life);
    if !gui.options.light_rays {
        shafts.beams.clear();
    }
    shafts.logged += dt;
    if shafts.logged > 5.0 {
        shafts.logged = 0.0;
        bevy::log::info!(
            "light shafts: {} beams, {} probes in 5 s, active {}",
            shafts.beams.len(),
            shafts.probed,
            shafts.active
        );
        shafts.probed = 0;
    }

    let Some(eye) = camera.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    // Before the world's read lock: `eye_fluid_at` takes it too, and a
    // second read behind a waiting writer deadlocks.
    let in_water = crate::util::block_model::eye_fluid_at(
        f64::from(eye.x),
        f64::from(eye.y),
        f64::from(eye.z),
    ) == Some(crate::util::block_model::EyeFluid::Water);
    let Some(world_arc) = particle_world() else {
        return;
    };
    let world = crate::client::tracking::read_world(&world_arc);
    let (toward, moon) = light(env.sky.sun_angle);
    let render_distance = gui.options.render_distance;
    shafts.day_opacity = gui.options.light_ray_day_opacity as f32;
    shafts.night_opacity = gui.options.light_ray_night_opacity as f32;
    shafts.profile_tint = gui.options.light_ray_tint as f32 / 100.0;
    shafts.sun_y = env.sky.sun_angle.cos();

    // The gates are read once a tick, as the mod's are.
    shafts.tick += dt;
    if shafts.tick >= TICK {
        shafts.tick = (shafts.tick - TICK).min(TICK);
        let feet = eye - Vec3::new(0.0, 1.62, 0.0);
        if shafts
            .anchor
            .is_none_or(|(at, _)| at.distance(feet) > ANCHOR_RADIUS)
        {
            let biome = crate::client::envprobe::camera_biome()
                .and_then(crate::util::biome_color::biome_name)
                .map_or("plains", |n| n.strip_prefix("minecraft:").unwrap_or(n));
            shafts.anchor = Some((feet, biome));
        }
        shafts
            .metrics
            .update(&world, eye.as_dvec3(), render_distance);

        // The sky toward the light, from 40 blocks out: what's near
        // (trees overhead) doesn't count, distant hills do.
        let len = (16.0 * f64::from(render_distance + 1)).min(1000.0);
        let dir = toward.as_dvec3();
        let sky_clear = !sky_blocked(
            &world,
            eye.as_dvec3() + DVec3::new(0.0, 0.1, 0.0) + dir * 40.0,
            dir,
            len,
        );
        let biome = shafts.anchor.map_or("plains", |(_, b)| b);
        let m = &shafts.metrics;
        let cave = m.cave() || (biome == "sulfur_caves" && m.underground());
        let profile = shaft_profile::profile(biome, Weather::now(), cave);
        let elevation = toward.y.clamp(-1.0, 1.0).asin().to_degrees();
        let bar = if shafts.active {
            KEEP_ELEVATION
        } else {
            START_ELEVATION
        };
        shafts.active = !in_water
            && gui.options.particles != ParticleStatus::Minimal
            && gui.options.light_rays
            && (if moon {
                gui.options.light_ray_night_opacity
            } else {
                gui.options.light_ray_day_opacity
            }) > 0
            && sky_clear
            && elevation > bar
            && profile.is_some();
        if let Some(id) = profile {
            shafts.profile_id = id;
        }
    }
    if !shafts.active {
        shafts.owed = 0.0;
        return;
    }

    let density = match gui.options.particles {
        ParticleStatus::All => 1.0,
        ParticleStatus::Decreased => 0.5,
        ParticleStatus::Minimal => 0.0,
    };
    let frequency = gui.options.light_ray_frequency as f32 / 100.0;
    shafts.owed += PROBES_PER_SECOND * density * frequency * dt;
    let probes = shafts.owed.floor() as u32;
    shafts.owed -= probes as f32;
    if probes == 0 {
        return;
    }
    shafts.probed += probes;
    let tint = shaft_profile::tint(shafts.profile_id);

    // Two axes across the light, to scatter probes over a disc.
    let side = toward.cross(Vec3::Z).normalize_or(Vec3::X);
    let rng = &mut rng.0;
    for _ in 0..probes {
        if shafts.beams.len() >= MAX_BEAMS {
            break;
        }
        let angle = rng.next_f32() * std::f32::consts::TAU;
        let r = RADIUS * rng.next_f32().powf(0.75);
        let offset = side * (r * angle.cos()) + Vec3::Z * (r * angle.sin());
        let start = eye + offset + toward * START;
        // The canopy: where this ray stops. A ray a block aside that goes
        // deeper has passed a gap; the beam is that ray's deeper stretch.
        let canopy = walk(&world, start, -toward);
        if canopy > MAX_WALK as f32 {
            continue;
        }
        let aside = rng.next_f32() * std::f32::consts::TAU;
        let beside = start + side * aside.cos() + Vec3::Z * aside.sin();
        let floor = walk(&world, beside, -toward);
        if (floor - canopy) * toward.y.abs() <= MIN_HEIGHT {
            continue;
        }
        let from = beside - toward * canopy;
        let to = beside - toward * floor;
        let life = 5.0 + 5.0 * rng.next_f32();
        shafts.beams.push(Beam {
            from,
            to,
            age: 0.0,
            life,
            moon,
            tint,
        });
    }
}

/// Fades in over 0.6 s to full, then out over the rest of its life: a
/// Bézier bump through 0, 2.25, 0, 0, peaking at a third of its span.
fn fade(age: f32, life: f32) -> f32 {
    let x = if age < 0.6 {
        age / 1.8
    } else {
        1.0 / 3.0 + (age - 0.6) * 2.0 / (3.0 * (life - 0.6).max(0.01))
    }
    .clamp(0.0, 1.0);
    3.0 * (1.0 - x) * (1.0 - x) * x * 2.25
}

/// The sun's colour at height `sun_y` (sine): warm near the horizon,
/// near white at noon, as the built-in pack's direct light.
fn sun_color(sun_y: f32) -> [f32; 3] {
    let t = sun_y.clamp(0.0, 0.5) / 0.5;
    let height = t * t * (3.0 - 2.0 * t);
    let warm = [1.0, 0.52, 0.26];
    let noon = [1.0, 0.95, 0.88];
    std::array::from_fn(|i| warm[i] + (noon[i] - warm[i]) * height)
}

/// The live beams as glowing sprites along their light.
pub(super) fn beam_sprites(shafts: &Shafts, eye: Vec3, out: &mut Vec<crate::draw::ParticleSprite>) {
    for b in &shafts.beams {
        let centre = (b.from + b.to) * 0.5;
        let axis = (b.from - b.to).normalize_or_zero();
        let d = (centre - eye).length();
        let near = if d < 8.0 { d } else { 8.0 + (d - 8.0) / 3.0 };
        let strength = if b.moon { 0.3 } else { 0.6 };
        let alpha = fade(b.age, b.life) * (strength / near.max(0.01)).clamp(0.0, 1.0);
        let opacity = if b.moon {
            shafts.night_opacity
        } else {
            shafts.day_opacity
        };
        let alpha = (alpha * opacity / 100.0).min(1.0);
        if alpha <= 0.002 {
            continue;
        }
        let color = if b.moon {
            shaft_profile::MOON_TINT
        } else {
            let sun = sun_color(shafts.sun_y);
            std::array::from_fn(|i| sun[i] + (b.tint[i] - sun[i]) * shafts.profile_tint)
        };
        out.push(crate::draw::ParticleSprite {
            centre: centre.to_array(),
            half: (b.from - b.to).length() * 0.5,
            uv: [0.0, 0.0, 1.0, 1.0],
            color: [color[0], color[1], color[2], alpha],
            roll: HALF_WIDTH,
            texture: crate::draw::TEXTURE_ATLAS,
            levels: [240.0, 240.0],
            flags: crate::draw::PARTICLE_GLOW,
            pad: [axis.x.to_bits(), axis.y.to_bits(), axis.z.to_bits()],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beam_fades_in_peaks_and_fades_out() {
        assert!(fade(0.0, 8.0) < 1e-6);
        assert!((fade(0.6, 8.0) - 1.0).abs() < 1e-3);
        assert!(fade(8.0, 8.0) < 1e-6);
    }

    #[test]
    fn the_light_is_overhead_at_noon_and_the_moon_at_midnight() {
        let (noon, moon) = light(0.0);
        assert!(noon.y > 0.99 && !moon);
        let (night, moon) = light(std::f32::consts::PI);
        assert!(night.y > 0.99 && moon);
    }

    #[test]
    fn the_sphere_rays_run_pole_to_pole() {
        assert!((Metrics::fibonacci(0).y - 1.0).abs() < 1e-9);
        assert!(Metrics::fibonacci(RAYS - 1).y < -0.9);
    }
}
