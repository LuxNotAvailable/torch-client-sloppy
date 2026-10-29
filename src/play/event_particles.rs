//! Particles an entity event sets off on the client: death smoke (60,
//! `makePoofParticles`), a teleport's trail (46), taming (6 and 7,
//! `spawnTamingParticles`), love (18), and villagers' and dolphins' moods
//! (12, 13, 14, 42 and 38, `addParticlesAroundSelf`).

use std::sync::{Arc, Mutex};

use azalea::entity::particle::Particle;
use azalea_registry::builtin::EntityKind;

use crate::session::{LevelParticle, ParticleEmit, SharedMutex};
use crate::util::javarandom::JavaRandom;

/// Where an entity is and how big: its position now and a tick ago, width
/// and height.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Body {
    pub pos: [f64; 3],
    pub old: [f64; 3],
    pub width: f64,
    pub height: f64,
    pub kind: EntityKind,
}

/// A particle to spawn: kind, position, velocity.
type Spawn = (Particle, [f64; 3], [f32; 3]);

fn random() -> std::sync::MutexGuard<'static, Option<JavaRandom>> {
    static RNG: Mutex<Option<JavaRandom>> = Mutex::new(None);
    let mut guard = RNG.lock().unwrap();
    guard.get_or_insert_with(|| JavaRandom::new(0x5EED_0E7E));
    guard
}

/// `getRandomX(1)`, `getRandomY() + lift`, `getRandomZ(1)`.
fn in_box(rng: &mut JavaRandom, b: &Body, lift: f64) -> [f64; 3] {
    let x = b.pos[0] + b.width * (2.0 * rng.next_f64() - 1.0);
    let y = b.pos[1] + b.height * rng.next_f64() + lift;
    let z = b.pos[2] + b.width * (2.0 * rng.next_f64() - 1.0);
    [x, y, z]
}

fn drift(rng: &mut JavaRandom) -> [f32; 3] {
    [
        (rng.next_gaussian() * 0.02) as f32,
        (rng.next_gaussian() * 0.02) as f32,
        (rng.next_gaussian() * 0.02) as f32,
    ]
}

/// `count` particles drifting about the body, `lift` up from a random
/// height.
fn around(rng: &mut JavaRandom, b: &Body, particle: Particle, count: u32, lift: f64) -> Vec<Spawn> {
    (0..count)
        .map(|_| {
            let v = drift(rng);
            (particle.clone(), in_box(rng, b, lift), v)
        })
        .collect()
}

fn is_villager(kind: EntityKind) -> bool {
    kind == EntityKind::Villager
}

fn is_tamable(kind: EntityKind) -> bool {
    use EntityKind as K;
    matches!(kind, K::Wolf | K::Cat | K::Parrot | K::Nautilus | K::ZombieNautilus)
}

/// The particles `event` sets off on `body`, if any.
pub(crate) fn particles(event: u8, b: &Body, rng: &mut JavaRandom) -> Vec<Spawn> {
    match event {
        60 => (0..20)
            .map(|_| {
                let v = drift(rng);
                let [x, y, z] = in_box(rng, b, 0.0);
                // Spawned upstream of their drift by ten ticks of it.
                let at = [x - v[0] as f64 * 10.0, y - v[1] as f64 * 10.0, z - v[2] as f64 * 10.0];
                (Particle::Poof, at, v)
            })
            .collect(),
        46 => (0..128)
            .map(|i| {
                let d = i as f64 / 127.0;
                let v = [
                    (rng.next_f32() - 0.5) * 0.2,
                    (rng.next_f32() - 0.5) * 0.2,
                    (rng.next_f32() - 0.5) * 0.2,
                ];
                let lerp = |axis: usize| b.old[axis] + (b.pos[axis] - b.old[axis]) * d;
                let x = lerp(0) + (rng.next_f64() - 0.5) * b.width * 2.0;
                let y = lerp(1) + rng.next_f64() * b.height;
                let z = lerp(2) + (rng.next_f64() - 0.5) * b.width * 2.0;
                (Particle::Portal, [x, y, z], v)
            })
            .collect(),
        7 if is_tamable(b.kind) => around(rng, b, Particle::Heart, 7, 0.5),
        6 if is_tamable(b.kind) => around(rng, b, Particle::Smoke, 7, 0.5),
        18 => around(rng, b, Particle::Heart, 7, 0.5),
        12 if is_villager(b.kind) => around(rng, b, Particle::Heart, 5, 1.0),
        13 if is_villager(b.kind) => around(rng, b, Particle::AngryVillager, 5, 1.0),
        14 if is_villager(b.kind) => around(rng, b, Particle::HappyVillager, 5, 1.0),
        42 if is_villager(b.kind) => around(rng, b, Particle::Splash, 5, 1.0),
        38 if b.kind == EntityKind::Dolphin => around(rng, b, Particle::HappyVillager, 5, 1.0),
        _ => Vec::new(),
    }
}

/// `TrackingEmitter`: an entity's particles for a while, 16 a tick, from
/// spots about it (the totem's 30 ticks of green).
struct Emitter {
    entity: i32,
    particle: Particle,
    ticks_left: u32,
}

static EMITTERS: Mutex<Vec<Emitter>> = Mutex::new(Vec::new());

/// `ItemActivation`: the item flying at the screen, when it began and its
/// sideways drift.
static ACTIVATION: Mutex<Option<(std::time::Instant, f32, f32, &'static str)>> = Mutex::new(None);

/// `MobAppearanceParticle`: when the elder guardian's face appeared.
static ELDER: Mutex<Option<std::time::Instant>> = Mutex::new(None);

pub(crate) fn elder_guardian() {
    *ELDER.lock().unwrap() = Some(std::time::Instant::now());
}

/// How far through the elder guardian's appearance (0 to 1), if showing.
pub(crate) fn elder_guardian_progress() -> Option<f32> {
    let mut guard = ELDER.lock().unwrap();
    let t = guard.as_ref()?.elapsed().as_secs_f32() / 1.5;
    if t >= 1.0 {
        *guard = None;
        return None;
    }
    Some(t)
}

/// The item activation now: how far through (0 to 1), its drift, and the
/// item.
pub(crate) fn activation() -> Option<(f32, f32, f32, &'static str)> {
    let mut guard = ACTIVATION.lock().unwrap();
    let (start, x, y, item) = (*guard)?;
    let t = start.elapsed().as_secs_f32() / 2.0;
    if t >= 1.0 {
        *guard = None;
        return None;
    }
    Some((t, x, y, item))
}

/// One game tick of the tracking emitters.
pub(crate) fn tick(shared: &Arc<SharedMutex>) {
    let mut emitters = EMITTERS.lock().unwrap();
    if emitters.is_empty() {
        return;
    }
    let bodies: Vec<Option<Body>> = {
        let anims = crate::client::tracking::entity_anims().lock().unwrap();
        emitters.iter().map(|e| body_of(&anims, e.entity)).collect()
    };
    let mut spawns = Vec::new();
    {
        let mut rng = random();
        let rng = rng.as_mut().unwrap();
        for (emitter, body) in emitters.iter_mut().zip(bodies) {
            let Some(b) = body else {
                emitter.ticks_left = 0;
                continue;
            };
            for _ in 0..16 {
                let (x, y, z) = (
                    rng.next_f32() * 2.0 - 1.0,
                    rng.next_f32() * 2.0 - 1.0,
                    rng.next_f32() * 2.0 - 1.0,
                );
                if x * x + y * y + z * z > 1.0 {
                    continue;
                }
                let at = [
                    b.pos[0] + b.width * (x as f64 / 4.0),
                    b.pos[1] + b.height * (0.5 + y as f64 / 4.0),
                    b.pos[2] + b.width * (z as f64 / 4.0),
                ];
                spawns.push((emitter.particle.clone(), at, [x, y + 0.2, z]));
            }
            emitter.ticks_left = emitter.ticks_left.saturating_sub(1);
        }
    }
    emitters.retain(|e| e.ticks_left > 0);
    drop(emitters);
    push(shared, spawns);
}

fn body_of(anims: &bevy::platform::collections::HashMap<i32, crate::entities::feed::EntityAnim>, id: i32) -> Option<Body> {
    if id == crate::client::tracking::local_entity_id() {
        let pos = crate::items::dispatch::context().pos;
        return Some(Body { pos, old: pos, width: 0.6, height: 1.8, kind: EntityKind::Player });
    }
    let anim = anims.get(&id)?;
    let st = anim.sample(1.0);
    Some(Body {
        pos: anim.position(1.0).map(f64::from),
        old: anim.position(0.0).map(f64::from),
        width: st.bounding_box_width as f64,
        height: st.bounding_box_height as f64,
        kind: anim.kind,
    })
}

fn push(shared: &Arc<SharedMutex>, spawns: Vec<Spawn>) {
    if spawns.is_empty() {
        return;
    }
    let mut s = shared.lock().unwrap();
    for (particle, pos, velocity) in spawns {
        crate::session::push_particle_emit(
            &mut s.session.particle_emits,
            ParticleEmit::Level(LevelParticle {
                particle,
                pos,
                dist: velocity,
                max_speed: [1.0; 3],
                count: 0,
                randomization: Default::default(),
                override_limiter: false,
                always_show: false,
            }),
        );
    }
}

/// `LivingEntity.handleEntityEvent(35)`: a Totem of Undying saves the
/// entity: its green burst, its sound, and for this player the totem
/// flying at the screen.
fn totem(entity_id: i32) {
    EMITTERS.lock().unwrap().push(Emitter {
        entity: entity_id,
        particle: Particle::TotemOfUndying,
        ticks_left: 30,
    });
    let local = entity_id == crate::client::tracking::local_entity_id();
    #[cfg(audio)]
    {
        let anims = crate::client::tracking::entity_anims().lock().unwrap();
        if let Some(b) = body_of(&anims, entity_id) {
            let at = [b.pos[0] as f32, b.pos[1] as f32, b.pos[2] as f32];
            crate::audio::play_at("item.totem.use", crate::audio::category::SoundCategory::Players, at, 1.0, 1.0);
        }
    }
    if local {
        let mut rng = random();
        let rng = rng.as_mut().unwrap();
        let (x, y) = (rng.next_f32() * 2.0 - 1.0, rng.next_f32() * 2.0 - 1.0);
        *ACTIVATION.lock().unwrap() = Some((std::time::Instant::now(), x, y, "totem_of_undying"));
    }
}

/// Spawns what `event` sets off on the entity, if the client knows where it
/// is.
pub(crate) fn emit(shared: &Arc<SharedMutex>, entity_id: i32, event: u8) {
    if event == 35 {
        totem(entity_id);
        return;
    }
    let body = {
        let anims = crate::client::tracking::entity_anims().lock().unwrap();
        let Some(anim) = anims.get(&entity_id) else { return };
        // `FireworkRocketEntity.handleEntityEvent`: the burst.
        if event == 17 && anim.kind == EntityKind::FireworkRocket {
            let pos = anim.position(1.0).map(f64::from);
            let old = anim.position(0.0).map(f64::from);
            let velocity = [pos[0] - old[0], pos[1] - old[1], pos[2] - old[2]];
            let explosions = anim.shared().fireworks.as_deref().cloned().unwrap_or_default();
            crate::util::particles::start_firework(pos, velocity, explosions);
            return;
        }
        let st = anim.sample(1.0);
        let pos = anim.position(1.0);
        let old = anim.position(0.0);
        Body {
            pos: pos.map(f64::from),
            old: old.map(f64::from),
            width: st.bounding_box_width as f64,
            height: st.bounding_box_height as f64,
            kind: anim.kind,
        }
    };
    let spawns = {
        let mut rng = random();
        particles(event, &body, rng.as_mut().unwrap())
    };
    if spawns.is_empty() {
        return;
    }
    let mut s = shared.lock().unwrap();
    for (particle, pos, velocity) in spawns {
        crate::session::push_particle_emit(
            &mut s.session.particle_emits,
            ParticleEmit::Level(LevelParticle {
                particle,
                pos,
                dist: velocity,
                max_speed: [1.0; 3],
                count: 0,
                randomization: Default::default(),
                override_limiter: false,
                always_show: false,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(kind: EntityKind) -> Body {
        Body {
            pos: [10.0, 64.0, 10.0],
            old: [0.0, 64.0, 0.0],
            width: 0.6,
            height: 1.8,
            kind,
        }
    }

    #[test]
    fn each_event_sets_off_vanillas_count() {
        let mut rng = JavaRandom::new(1);
        assert_eq!(particles(60, &body(EntityKind::Zombie), &mut rng).len(), 20);
        assert_eq!(particles(46, &body(EntityKind::Enderman), &mut rng).len(), 128);
        assert_eq!(particles(7, &body(EntityKind::Wolf), &mut rng).len(), 7);
        assert_eq!(particles(18, &body(EntityKind::Cow), &mut rng).len(), 7);
        assert_eq!(particles(13, &body(EntityKind::Villager), &mut rng).len(), 5);
        // Ids mean other things on other entities.
        assert!(particles(13, &body(EntityKind::Cow), &mut rng).is_empty());
        assert!(particles(7, &body(EntityKind::Cow), &mut rng).is_empty());
    }

    #[test]
    fn a_teleport_trails_from_the_old_spot_to_the_new() {
        let mut rng = JavaRandom::new(2);
        let trail = particles(46, &body(EntityKind::Enderman), &mut rng);
        assert!(trail.first().unwrap().1[0] < 2.0);
        assert!(trail.last().unwrap().1[0] > 8.0);
    }
}
