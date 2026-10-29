use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use azalea::entity::particle::Particle as P;

use crate::renderer::AppState;
use crate::renderer::entity_material::{
    EntityMaterial, EntityParams, LightMode, Lit, diffuse_lights,
};
use crate::renderer::systems::{ChunkMaterial, LightmapState, Shared, WorldCamera};
use crate::util::javarandom::JavaRandom;
use crate::util::particle_assets;


// What the rest of the client reaches by `crate::util::particles::...`.
pub(crate) use ambient::{Emitter, block_at, rel};
pub(crate) use draw::{ParticleBatch, particle_uv};
pub(crate) use fireworks::{FireworkExplosion, reset_fireworks, start_firework};

mod recipes;
#[allow(unused_imports)]
use recipes::*;
mod behavior;
#[allow(unused_imports)]
use behavior::*;
mod fireworks;
#[allow(unused_imports)]
use fireworks::*;
mod ambient;
#[allow(unused_imports)]
use ambient::*;
mod draw;
#[allow(unused_imports)]
use draw::*;

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleMaterialCache>()
            .init_resource::<ParticleRng>()
            .init_resource::<LiveParticles>()
            .init_resource::<ParticleAtlas>()
            .init_resource::<ParticleBatches>()
            .init_resource::<AmbientClock>()
            .add_systems(OnEnter(AppState::InGame), load_particle_atlas)
            .add_systems(
                Update,
                (
                    share_item_icons,
                    spawn_particles,
                    spawn_crit_particles,
                    animate_tick_particles,
                    tick_fireworks,
                    tick_particles,
                    draw_particles,
                )
                    .chain()
                    .after(crate::renderer::FrameViewSystems)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ParticleStatus {
    #[default]
    All,
    Decreased,
    Minimal,
}

impl ParticleStatus {
    pub const ALL: [ParticleStatus; 3] = [
        ParticleStatus::All,
        ParticleStatus::Decreased,
        ParticleStatus::Minimal,
    ];

    pub fn caption(self) -> &'static str {
        match self {
            ParticleStatus::All => "All",
            ParticleStatus::Decreased => "Decreased",
            ParticleStatus::Minimal => "Minimal",
        }
    }

    pub fn serialized_name(self) -> &'static str {
        match self {
            ParticleStatus::All => "all",
            ParticleStatus::Decreased => "decreased",
            ParticleStatus::Minimal => "minimal",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<ParticleStatus> {
        ParticleStatus::ALL
            .into_iter()
            .find(|s| s.serialized_name() == name)
    }

    pub fn next(self) -> ParticleStatus {
        let i = ParticleStatus::ALL
            .iter()
            .position(|s| *s == self)
            .unwrap_or(0);
        ParticleStatus::ALL[(i + 1) % ParticleStatus::ALL.len()]
    }
}

fn allow_particle(status: ParticleStatus, always_show: bool, rng: &mut JavaRandom) -> bool {
    let mut level = status;
    if always_show && level == ParticleStatus::Minimal && rng.next_int(10) == 0 {
        level = ParticleStatus::Decreased;
    }
    if level == ParticleStatus::Decreased && rng.next_int(3) == 0 {
        level = ParticleStatus::Minimal;
    }
    level != ParticleStatus::Minimal
}

#[derive(Clone, Copy)]
struct SpawnGate {
    override_limiter: bool,
    always_show: bool,
}

impl SpawnGate {
    fn normal(kind: &P) -> SpawnGate {
        SpawnGate {
            override_limiter: overrides_limiter(kind),
            always_show: false,
        }
    }
    const ALWAYS_SHOW: SpawnGate = SpawnGate {
        override_limiter: false,
        always_show: true,
    };
}

#[derive(Resource)]
struct ParticleRng(JavaRandom);

impl Default for ParticleRng {
    fn default() -> Self {
        let seed = crate::platform::time::SystemTime::now()
            .duration_since(crate::platform::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        ParticleRng(JavaRandom::new(seed))
    }
}

#[derive(Resource, Default)]
struct LiveParticles(Vec<Particle>);

#[derive(Resource, Default)]
struct ParticleAtlas(Option<Atlas>);

pub(crate) struct Atlas {
    image: Handle<Image>,
    rows: u32,
    sheets: Vec<Vec<u32>>,
    by_name: HashMap<String, u16>,
    crit: Option<[f32; 4]>,
}

impl Atlas {
    fn sheet_index(&self, particle_type: &str) -> Option<u16> {
        self.by_name.get(particle_type).copied()
    }

    fn frame_uv(&self, sheet: u16, frame: usize) -> [f32; 4] {
        let tiles = &self.sheets[sheet as usize];
        particle_assets::tile_uv(tiles[frame % tiles.len()], self.rows)
    }
}

fn load_particle_atlas(mut atlas: ResMut<ParticleAtlas>, mut images: ResMut<Assets<Image>>) {
    if atlas.0.is_some() {
        return;
    }
    let built = particle_assets::build_atlas();
    if built.sheets.is_empty() {
        return;
    }
    let mut sheets = Vec::with_capacity(built.sheets.len());
    let mut by_name = HashMap::with_capacity(built.sheets.len());
    for (name, tiles) in built.sheets {
        by_name.insert(name, sheets.len() as u16);
        sheets.push(tiles);
    }
    let crit = by_name
        .get("crit")
        .map(|&i| particle_assets::tile_uv(sheets[i as usize][0], built.rows));
    atlas.0 = Some(Atlas {
        image: images.add(built.image),
        rows: built.rows,
        sheets,
        by_name,
        crit,
    });
}

#[derive(Resource, Default)]
struct ParticleMaterialCache(HashMap<BatchKey, Handle<EntityMaterial>>);

const MAX_CACHED_MATERIALS: usize = 256;

pub(crate) const MAX_LIVE_PARTICLES: usize = 5000;

const MAX_PARTICLE_BATCHES: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BatchKey {
    block_light: u8,
    sky_light: u8,
    dim: u8,
    source: Source,
    translucent: bool,
}

/// The texture a particle's sprite comes from.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    Sheet,
    Terrain,
    /// Item icons in the GUI atlas, for item particles.
    Items,
}

#[derive(Resource, Default)]
struct ParticleBatches {
    pool: Vec<(Entity, Handle<Mesh>)>,
    index: HashMap<BatchKey, usize>,
    keys: Vec<BatchKey>,
    groups: Vec<Vec<u32>>,
}

pub(crate) struct Particle {
    prev: Vec3,
    pos: Vec3,
    velocity: Vec3,
    gravity: f32,
    friction: f32,
    tick_acc: f32,
    age: u32,
    lifetime: u32,
    tint: [f32; 4],
    size: f32,
    grow: Grow,
    sprite: Sprite,
    flags: u8,
    /// Vanilla particle classes with their own `tick`.
    behavior: Behavior,
}

/// What kind of drip, and so its colour, fluid, timing and what follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drip {
    WaterHang,
    LavaHang,
    HoneyHang,
    TearHang,
    DripstoneWaterHang,
    DripstoneLavaHang,
    WaterFall,
    LavaFall,
    HoneyFall,
    TearFall,
    DripstoneWaterFall,
    DripstoneLavaFall,
    NectarFall,
    SporeFall,
    LavaLand,
    HoneyLand,
    TearLand,
}

/// `FallingParticle`'s wind and spin.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Leaf {
    roll: f32,
    old_roll: f32,
    rot_speed: f32,
    spin: f32,
    wind: f32,
    swirl: bool,
    flow: bool,
    x_flow: f32,
    z_flow: f32,
    swirl_period: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Behavior {
    None,
    /// A firework spark: fades to its fade colour, trails, twinkles.
    Spark(Spark),
    Drip(Drip),
    Leaf(Leaf),
    /// `FireflyParticle`: fades in and out, darts about, dies in blocks.
    Firefly,
    /// `SquidInkParticle`: sinks slowly through air.
    SquidInk,
    /// `SnowflakeParticle`: slows down and settles.
    Snowflake,
    /// `FlyStraightTowardsParticle`: from the start toward the spot, the
    /// colour sliding from one to the other.
    FlyStraight { start: [f32; 3], offset: [f32; 3], from: u32, to: u32 },
    /// `SulfurBubbleParticle`: rises through still water, swelling.
    Bubble { y_start: f32, y_end: f32, y_prev: f32, size_start: f32 },
}

/// `FireworkParticles.SparkParticle`'s state beyond a plain particle.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Spark {
    fade: Option<[f32; 3]>,
    trail: bool,
    twinkle: bool,
}

#[derive(Clone, Copy)]
enum Sprite {
    Fixed([f32; 4]),
    Animated(u16),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grow {
    Fixed,
    In,
    Flame,
    Lava,
    Portal,
    /// `FireworkParticles.OverlayParticle`: swells and fades over 4 ticks.
    Flash,
}

const PHYSICS: u8 = 1 << 0;
const TERRAIN: u8 = 1 << 1;
const TRANSLUCENT: u8 = 1 << 2;
const FULLBRIGHT: u8 = 1 << 3;
const CRIT_DECAY: u8 = 1 << 4;
const FADE_OUT: u8 = 1 << 5;
const ITEMS: u8 = 1 << 6;

/// The GUI atlas, whose item icons item particles are cut from.
static ITEM_ICONS: std::sync::RwLock<Option<std::sync::Arc<crate::gui::atlas::GuiAtlas>>> =
    std::sync::RwLock::new(None);

fn share_item_icons(assets: Option<Res<crate::gui::render::GuiAssets>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some(assets) = assets else { return };
    *done = true;
    if let Ok(mut icons) = ITEM_ICONS.write() {
        *icons = Some(assets.atlas.clone());
    }
}

/// An item's icon in the GUI atlas, as UVs.
fn item_icon_uv(id: &str) -> Option<[f32; 4]> {
    let icons = ITEM_ICONS.read().ok()?;
    let atlas = icons.as_ref()?;
    let region = atlas.item_for_size(id, 16.0)?;
    let (w, h) = atlas.size();
    let (w, h) = (w as f32, h as f32);
    Some([
        region.x as f32 / w,
        region.y as f32 / h,
        (region.x + region.w) as f32 / w,
        (region.y + region.h) as f32 / h,
    ])
}

const DESTROY_GRID: i32 = 4;

fn spawn_particles(
    shared: Res<Shared>,
    gui: Res<crate::gui::GuiState>,
    atlas: Res<ParticleAtlas>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut live: ResMut<LiveParticles>,
    mut particle_rng: ResMut<ParticleRng>,
) {
    use crate::session::ParticleEmit;

    let emits = std::mem::take(&mut shared.0.lock().unwrap().session.world.particle_emits);
    if emits.is_empty() {
        return;
    }
    let status = gui.options.particles;
    let eye = camera
        .iter()
        .next()
        .map(GlobalTransform::translation)
        .unwrap_or(Vec3::ZERO);
    let world_arc = particle_world();
    let world = world_arc.as_deref().map(|w| w.read());
    let world = world.as_deref();
    let atlas = atlas.0.as_ref();
    let rng = &mut particle_rng.0;

    let (mut breaks, mut packets, mut events) = (0u32, 0u32, 0u32);
    let before = live.0.len();
    let mut explosions = Vec::new();

    for emit in emits {
        if live.0.len() >= MAX_LIVE_PARTICLES {
            break;
        }
        match emit {
            ParticleEmit::Block(spawn) => {
                breaks += 1;
                spawn_block_break(&spawn, status, world, &mut live.0, rng);
            }
            ParticleEmit::Level(level) => {
                packets += 1;
                if let Some(atlas) = atlas {
                    spawn_level_particle(&level, atlas, world, eye, status, &mut live.0, rng);
                }
            }
            ParticleEmit::Event(event) => {
                let (Some(atlas), Some(world)) = (atlas, world) else {
                    continue;
                };
                events += 1;
                let mut ctx = Emitter {
                    cell: event.pos,
                    world,
                    atlas,
                    status,
                    eye,
                    live: &mut live.0,
                    rng: &mut *rng,
                };
                crate::util::level_events::spawn(&event, &mut ctx);
            }
            ParticleEmit::Explosion(explosion) => {
                packets += 1;
                let Some(atlas) = atlas else { continue };
                let c = explosion.center;
                add_level_particle(
                    &mut live.0,
                    atlas,
                    world,
                    rng,
                    status,
                    &explosion.particle,
                    SpawnGate::normal(&explosion.particle),
                    Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32),
                    Vec3::X,
                    eye,
                );
                if !explosion.block_particles.is_empty() {
                    explosions.push(explosion);
                }
            }
        }
    }

    if let (Some(atlas), Some(world)) = (atlas, world) {
        let mut ctx = Emitter {
            cell: [0; 3],
            world,
            atlas,
            status,
            eye,
            live: &mut live.0,
            rng,
        };
        scatter_explosions(&explosions, &mut ctx);
    }

    if breaks + packets + events > 0 {
        crate::log_debug!(
            "particles",
            "drained {breaks} break, {packets} packet, {events} event: {} live, was {before}",
            live.0.len()
        );
    }
}

const MAX_EXPLOSION_PARTICLES: i32 = 512;

fn scatter_explosions(explosions: &[crate::session::ExplosionEmit], ctx: &mut Emitter) {
    if ctx.status != ParticleStatus::All {
        return;
    }
    let total = explosions
        .iter()
        .fold(0i32, |sum, e| sum.saturating_add(e.block_count.max(0)));
    for _ in 0..total.min(MAX_EXPLOSION_PARTICLES) {
        if let Some(explosion) = weighted_pick(explosions, |e| e.block_count, ctx.rng) {
            scatter_one(explosion, ctx);
        }
    }
}

fn scatter_one(explosion: &crate::session::ExplosionEmit, ctx: &mut Emitter) {
    let dir = Vec3::new(
        ctx.f32() * 2.0 - 1.0,
        ctx.f32() * 2.0 - 1.0,
        ctx.f32() * 2.0 - 1.0,
    )
    .normalize_or_zero();
    let radius = ctx.f32().cbrt() * explosion.radius;
    let local = dir * radius;
    let c = explosion.center;
    let center = Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32);
    let cell = (center + local).floor();
    if ctx
        .block([cell.x as i32, cell.y as i32, cell.z as i32])
        .is_some_and(|s| !s.is_air())
    {
        return;
    }
    let speed = 0.5 / (radius / explosion.radius + 0.1) * ctx.f32() * ctx.f32() + 0.3;
    let Some(info) = weighted_pick(&explosion.block_particles, |w| w.weight, ctx.rng) else {
        return;
    };
    let info = &info.value;
    ctx.emit(
        &info.particle,
        center + local * info.scaling,
        dir * (speed * info.speed),
        false,
    );
}

fn weighted_pick<'a, T>(
    items: &'a [T],
    weight: impl Fn(&T) -> i32,
    rng: &mut JavaRandom,
) -> Option<&'a T> {
    let total = items
        .iter()
        .fold(0i32, |sum, i| sum.saturating_add(weight(i).max(0)));
    if total <= 0 {
        return None;
    }
    let mut pick = rng.next_int(total as u32) as i32;
    items.iter().find(|i| {
        pick -= weight(i).max(0);
        pick < 0
    })
}

pub(crate) fn spawn_block_break(
    spawn: &crate::session::ParticleSpawn,
    status: ParticleStatus,
    world: Option<&azalea_world::World>,
    live: &mut Vec<Particle>,
    particle_rng: &mut JavaRandom,
) {
    let Some(tile) = particle_uv(spawn.state) else {
        return;
    };
    let block_min = Vec3::new(
        spawn.pos[0] as f32,
        spawn.pos[1] as f32,
        spawn.pos[2] as f32,
    );
    let seed = ((particle_rng.next_int(i32::MAX as u32) as i64) << 31)
        | particle_rng.next_int(i32::MAX as u32) as i64;
    let mut rng = JavaRandom::new(seed);

    let points: Vec<(Vec3, Vec3)> = if spawn.mining {
        let mut pos = block_min
            + Vec3::new(
                0.1 + rng.next_f32() * 0.8,
                0.1 + rng.next_f32() * 0.8,
                0.1 + rng.next_f32() * 0.8,
            );
        let face = Vec3::from(spawn.face);
        if face.x.abs() > 0.5 {
            pos.x = block_min.x + if face.x > 0.0 { 1.1 } else { -0.1 };
        }
        if face.y.abs() > 0.5 {
            pos.y = block_min.y + if face.y > 0.0 { 1.1 } else { -0.1 };
        }
        if face.z.abs() > 0.5 {
            pos.z = block_min.z + if face.z > 0.0 { 1.1 } else { -0.1 };
        }
        vec![(pos, Vec3::ZERO)]
    } else {
        let mut out = Vec::with_capacity((DESTROY_GRID * DESTROY_GRID * DESTROY_GRID) as usize);
        for xx in 0..DESTROY_GRID {
            for yy in 0..DESTROY_GRID {
                for zz in 0..DESTROY_GRID {
                    let rel = Vec3::new(
                        (xx as f32 + 0.5) / DESTROY_GRID as f32,
                        (yy as f32 + 0.5) / DESTROY_GRID as f32,
                        (zz as f32 + 0.5) / DESTROY_GRID as f32,
                    );
                    out.push((block_min + rel, rel - Vec3::splat(0.5)));
                }
            }
        }
        out
    };

    let ratio = tint_ratio(spawn.state, spawn.pos, world);
    let tint = [0.6 * ratio[0], 0.6 * ratio[1], 0.6 * ratio[2], 1.0];

    for (pos, seed) in points {
        if live.len() >= MAX_LIVE_PARTICLES {
            break;
        }
        if !allow_particle(status, false, particle_rng) {
            continue;
        }
        let velocity = base_velocity(seed, &mut rng);
        let velocity = if spawn.mining {
            set_power(velocity, 0.2)
        } else {
            velocity
        };
        let size =
            0.1 * (rng.next_f32() * 0.5 + 0.5) * 2.0 / 2.0 * if spawn.mining { 0.6 } else { 1.0 };
        let lifetime = (4.0 / (rng.next_f32() * 0.9 + 0.1)) as u32;

        let mut particle = Particle {
            prev: pos,
            pos,
            velocity,
            gravity: 1.0,
            friction: 0.98,
            tick_acc: rng.next_f32() / 20.0,
            age: 0,
            lifetime,
            tint,
            size,
            grow: Grow::Fixed,
            sprite: Sprite::Fixed(random_sub_uv(tile, &mut rng)),
            flags: PHYSICS | TERRAIN,
            behavior: Behavior::None,
        };
        if particle.lifetime > 0 {
            particle.age = 1;
            advance_one_tick(&mut particle, world);
        }
        live.push(particle);
    }
}

fn spawn_crit_particles(
    view: Res<crate::renderer::FrameView>,
    gui: Res<crate::gui::GuiState>,
    atlas: Res<ParticleAtlas>,
    mut live: ResMut<LiveParticles>,
    mut particle_rng: ResMut<ParticleRng>,
) {
    let hits = std::mem::take(&mut *crate::client::tracking::crit_hits().lock().unwrap());
    if hits.is_empty() {
        return;
    }
    let Some(uv) = atlas.0.as_ref().and_then(|a| a.crit) else {
        return;
    };
    let status = gui.options.particles;

    let partial = view.partial;
    let targets = hits.into_iter().filter_map(|(id, magic)| {
        let anim = view.entities.iter().find(|e| e.id == id)?;
        Some((
            Vec3::from(anim.position(partial)),
            anim.bounding_box_height(),
            magic,
        ))
    });

    for (base, height, magic) in targets {
        for _round in 0..3 {
            for _ in 0..16 {
                if live.0.len() >= MAX_LIVE_PARTICLES {
                    break;
                }
                let xa = particle_rng.0.next_f32() * 2.0 - 1.0;
                let ya = particle_rng.0.next_f32() * 2.0 - 1.0;
                let za = particle_rng.0.next_f32() * 2.0 - 1.0;
                if xa * xa + ya * ya + za * za > 1.0 {
                    continue;
                }
                if !allow_particle(status, false, &mut particle_rng.0) {
                    continue;
                }

                let pos = base + Vec3::new(xa / 4.0, height * (0.5 + ya / 4.0), za / 4.0);
                let velocity = Vec3::new(xa, ya + 0.2, za) * 0.4;

                let grey = particle_rng.0.next_f32() * 0.3 + 0.6;
                let mut tint = [grey, grey, grey, 1.0];
                if magic {
                    tint[0] *= 0.3;
                    tint[1] *= 0.8;
                }
                let size = 0.1 * (particle_rng.0.next_f32() * 0.5 + 0.5) * 2.0 * 0.75;
                let lifetime = (6.0 / (particle_rng.0.next_f32() * 0.8 + 0.6)).max(1.0) as u32;

                let mut particle = Particle {
                    prev: pos,
                    pos,
                    velocity,
                    gravity: 0.5,
                    friction: 0.7,
                    tick_acc: particle_rng.0.next_f32() / 20.0,
                    age: 0,
                    lifetime,
                    tint,
                    size,
                    grow: Grow::In,
                    sprite: Sprite::Fixed(uv),
                    flags: CRIT_DECAY,
                    behavior: Behavior::None,
                };
                if particle.lifetime > 0 {
                    particle.age = 1;
                    advance_one_tick(&mut particle, None);
                }
                live.0.push(particle);
            }
        }
    }
}

const MAX_EMIT_COUNT: u32 = 512;

const PARTICLE_CULL_DISTANCE_SQ: f32 = 1024.0;

#[allow(clippy::too_many_arguments)]
fn spawn_level_particle(
    emit: &crate::session::LevelParticle,
    atlas: &Atlas,
    world: Option<&azalea_world::World>,
    eye: Vec3,
    status: ParticleStatus,
    live: &mut Vec<Particle>,
    rng: &mut JavaRandom,
) {
    let base = Vec3::new(emit.pos[0] as f32, emit.pos[1] as f32, emit.pos[2] as f32);
    let gate = SpawnGate {
        override_limiter: emit.override_limiter || overrides_limiter(&emit.particle),
        always_show: emit.always_show,
    };
    let dist = Vec3::from_array(emit.dist);
    let speed = Vec3::from_array(emit.max_speed);
    if emit.count == 0 {
        let arg = dist * speed;
        add_level_particle(
            live,
            atlas,
            world,
            rng,
            status,
            &emit.particle,
            gate,
            base,
            arg,
            eye,
        );
        return;
    }
    use azalea_protocol::packets::game::c_level_particles::ParticleRandomization;
    fn gaussian(rng: &mut JavaRandom) -> Vec3 {
        Vec3::new(
            rng.next_gaussian() as f32,
            rng.next_gaussian() as f32,
            rng.next_gaussian() as f32,
        )
    }
    fn uniform(rng: &mut JavaRandom) -> Vec3 {
        Vec3::new(
            rng.next_f64() as f32,
            rng.next_f64() as f32,
            rng.next_f64() as f32,
        )
    }
    for _ in 0..emit.count.min(MAX_EMIT_COUNT) {
        if live.len() >= MAX_LIVE_PARTICLES {
            break;
        }
        let (pos, arg) = match emit.randomization {
            ParticleRandomization::Default => {
                let pos = base + gaussian(rng) * dist;
                (pos, gaussian(rng) * speed)
            }
            ParticleRandomization::Alternative => (base + uniform(rng) * dist, speed),
            ParticleRandomization::AlternativeWithSpeed => {
                let pos = base + uniform(rng) * dist;
                (pos, uniform(rng) * speed)
            }
        };
        add_level_particle(
            live,
            atlas,
            world,
            rng,
            status,
            &emit.particle,
            gate,
            pos,
            arg,
            eye,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn add_level_particle(
    live: &mut Vec<Particle>,
    atlas: &Atlas,
    world: Option<&azalea_world::World>,
    rng: &mut JavaRandom,
    status: ParticleStatus,
    particle_kind: &azalea::entity::particle::Particle,
    gate: SpawnGate,
    pos: Vec3,
    arg: Vec3,
    eye: Vec3,
) {
    if !gate.override_limiter {
        if eye.distance_squared(pos) > PARTICLE_CULL_DISTANCE_SQ {
            return;
        }
        if !allow_particle(status, gate.always_show, rng) {
            return;
        }
    }
    if matches!(particle_kind, P::ExplosionEmitter) {
        spawn_explosion_seed(live, atlas, world, rng, status, pos, eye);
        return;
    }
    let Some(recipe) = recipe(particle_kind, atlas, arg, rng) else {
        return;
    };
    let mut particle = Particle::from_recipe(recipe, pos, rng.next_f32() / 20.0);
    particle.age = 1;
    advance_one_tick(&mut particle, world);
    live.push(particle);
}

impl Particle {
    /// A particle made from `recipe`, starting at `pos`.
    fn from_recipe(recipe: Recipe, pos: Vec3, tick_acc: f32) -> Particle {
        Particle {
            prev: pos,
            pos,
            velocity: recipe.velocity,
            gravity: recipe.gravity,
            friction: recipe.friction,
            tick_acc,
            age: 0,
            lifetime: recipe.lifetime.max(1),
            tint: recipe.tint,
            size: recipe.size,
            grow: recipe.grow,
            sprite: recipe.sprite,
            flags: recipe.flags,
            behavior: match recipe.behavior {
                // `FlyStraightTowardsParticle` starts its offset away and
                // comes in.
                Behavior::FlyStraight { offset, from, to, .. } => Behavior::FlyStraight {
                    start: pos.to_array(),
                    offset,
                    from,
                    to,
                },
                other => other,
            },
        }
    }
}

fn overrides_limiter(kind: &P) -> bool {
    matches!(
        kind,
        P::BlockMarker(_)
            | P::DamageIndicator
            | P::ElderGuardian
            | P::ExplosionEmitter
            | P::Explosion
            | P::Gust
            | P::GustEmitterLarge
            | P::GustEmitterSmall
            | P::SonicBoom
            | P::SculkCharge(_)
            | P::SculkChargePop
            | P::Vibration(_)
            | P::Poof
            | P::Spit
            | P::SquidInk
            | P::SweepAttack
            | P::CampfireCosySmoke
            | P::CampfireSignalSmoke
            | P::GlowSquidInk
            | P::Glow
            | P::WaxOn
            | P::WaxOff
            | P::ElectricSpark
            | P::Scrape
            | P::TrialSpawnerDetection
            | P::TrialSpawnerDetectionOminous
            | P::VaultConnection
            | P::OminousSpawning
    )
}

const SEED_TICKS: u32 = 8;

fn spawn_explosion_seed(
    live: &mut Vec<Particle>,
    atlas: &Atlas,
    world: Option<&azalea_world::World>,
    rng: &mut JavaRandom,
    status: ParticleStatus,
    pos: Vec3,
    eye: Vec3,
) {
    fn spread(rng: &mut JavaRandom) -> f32 {
        ((rng.next_f64() - rng.next_f64()) * 4.0) as f32
    }
    for tick in 0..SEED_TICKS {
        for _ in 0..6 {
            if live.len() >= MAX_LIVE_PARTICLES {
                return;
            }
            let offset = Vec3::new(spread(rng), spread(rng), spread(rng));
            let len = live.len();
            add_level_particle(
                live,
                atlas,
                world,
                rng,
                status,
                &P::Explosion,
                SpawnGate::normal(&P::Explosion),
                pos + offset,
                Vec3::new(tick as f32 / SEED_TICKS as f32, 0.0, 0.0),
                eye,
            );
            if let Some(puff) = live.get_mut(len) {
                puff.tick_acc -= tick as f32 * TICK;
            }
        }
    }
}

fn is_solid_at(world: Option<&azalea_world::World>, pos: Vec3) -> bool {
    let Some(world) = world else { return false };
    let block_pos = azalea_core::position::BlockPos {
        x: pos.x.floor() as i32,
        y: pos.y.floor() as i32,
        z: pos.z.floor() as i32,
    };
    world
        .get_block_state(block_pos)
        .is_some_and(crate::util::block_model::is_solid)
}

fn advance_one_tick(p: &mut Particle, world: Option<&azalea_world::World>) -> bool {
    p.prev = p.pos;
    p.velocity.y -= 0.04 * p.gravity;

    let mut next = p.pos + p.velocity;
    let mut on_ground = false;
    if p.flags & PHYSICS != 0 {
        if p.velocity.y != 0.0 && is_solid_at(world, Vec3::new(p.pos.x, next.y, p.pos.z)) {
            if p.velocity.y < 0.0 {
                next.y = next.y.floor() + 1.0;
                on_ground = true;
            } else {
                next.y = next.y.floor();
            }
            p.velocity.y = 0.0;
        }
        if p.velocity.x != 0.0 && is_solid_at(world, Vec3::new(next.x, p.pos.y, p.pos.z)) {
            next.x = p.pos.x;
            p.velocity.x = 0.0;
        }
        if p.velocity.z != 0.0 && is_solid_at(world, Vec3::new(p.pos.x, p.pos.y, next.z)) {
            next.z = p.pos.z;
            p.velocity.z = 0.0;
        }
    }
    p.pos = next;

    p.velocity *= p.friction;
    if on_ground {
        p.velocity.x *= 0.7;
        p.velocity.z *= 0.7;
    }

    if p.flags & CRIT_DECAY != 0 {
        p.tint[1] *= 0.96;
        p.tint[2] *= 0.9;
    }
    if p.flags & FADE_OUT != 0 && p.age * 2 > p.lifetime {
        let half = p.lifetime / 2;
        p.tint[3] = 1.0 - (p.age.saturating_sub(half) as f32 / p.lifetime as f32);
    }
    on_ground
}

fn particle_world() -> Option<std::sync::Arc<parking_lot::RwLock<azalea_world::World>>> {
    crate::client::tracking::current_world()
        .lock()
        .unwrap()
        .clone()
}

const TICK: f32 = 1.0 / 20.0;

fn tick_particles(
    time: Res<Time>,
    atlas: Res<ParticleAtlas>,
    mut particle_rng: ResMut<ParticleRng>,
    mut live: ResMut<LiveParticles>,
) {
    if live.0.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let world_arc = particle_world();
    let world = world_arc.as_deref().map(|w| w.read());
    let world = world.as_deref();

    let mut trails: Vec<Particle> = Vec::new();
    let mut chained: Vec<(P, Vec3)> = Vec::new();
    let rng = &mut particle_rng.0;
    live.0.retain_mut(|particle| {
        particle.tick_acc += dt;
        while particle.tick_acc >= TICK {
            particle.tick_acc -= TICK;
            if particle.age >= particle.lifetime {
                break;
            }
            particle.age += 1;
            let on_ground = advance_one_tick(particle, world);
            if let Behavior::Spark(spark) = particle.behavior {
                tick_spark(particle, spark, &mut trails);
            } else if particle.behavior != Behavior::None {
                let after = tick_behavior(particle, world, on_ground, rng);
                if let Some(next) = after.spawn {
                    chained.push(next);
                }
                #[cfg(audio)]
                if let Some((sound, at)) = after.sound {
                    let volume = 0.3 + rng.next_f32() * 0.7;
                    crate::audio::play_at(sound, crate::audio::category::SoundCategory::Blocks, at.to_array(), volume, 1.0);
                }
                #[cfg(not(audio))]
                let _ = after.sound;
                if after.remove {
                    particle.age = particle.lifetime;
                    break;
                }
            }
        }
        particle.age < particle.lifetime
    });
    live.0.extend(trails);
    if let Some(atlas) = atlas.0.as_ref() {
        for (kind, pos) in chained {
            let Some(recipe) = recipe(&kind, atlas, Vec3::ZERO, rng) else { continue };
            live.0.push(Particle::from_recipe(recipe, pos, 0.0));
        }
    }
}
