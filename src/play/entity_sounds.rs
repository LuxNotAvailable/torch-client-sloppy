//! Entity sounds vanilla's client plays itself (`playLocalSound` from an
//! entity's client tick or `handleEntityEvent`): the server never sends
//! them.

use std::collections::HashMap;
use std::sync::Mutex;

use azalea_registry::builtin::EntityKind;

use crate::audio::SoundCategory;

/// What one entity's client tick needs.
pub(crate) struct Ticked {
    pub id: i32,
    pub kind: EntityKind,
    pub pos: [f64; 3],
    pub eye_height: f32,
    /// `tickCount` since this client saw it.
    pub ticks: u32,
    pub silent: bool,
    /// The warden's `ClientAngerLevel`.
    pub anger: i32,
    /// The enderman's `Creepy` and `StaredAt`.
    pub creepy: bool,
    pub stared_at: bool,
    /// A sniffer in its searching state.
    pub searching: bool,
}

/// Per-entity state vanilla keeps on the entity.
#[derive(Default)]
struct Kept {
    /// The breeze's `soundTick`.
    sound_tick: i32,
    /// The enderman's last `Creepy` and `lastStareSound`.
    creepy: Option<bool>,
    last_stare: Option<u32>,
    /// A lightning bolt's thunder has played.
    struck: bool,
    seen: u64,
}

static KEPT: Mutex<Option<HashMap<i32, Kept>>> = Mutex::new(None);
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn unit() -> f32 {
    crate::audio::random_unit()
}

fn play(event: &str, category: SoundCategory, at: [f64; 3], volume: f32, pitch: f32) {
    let at = [at[0] as f32, at[1] as f32, at[2] as f32];
    crate::audio::play_at(event, category, at, volume, pitch);
}

/// One client tick of the entities this tick moved.
pub(crate) fn tick(entities: &[Ticked]) {
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let mut guard = KEPT.lock().unwrap();
    let kept = guard.get_or_insert_with(HashMap::new);
    for e in entities {
        let k = kept.entry(e.id).or_default();
        k.seen = generation;
        tick_one(e, k);
    }
    kept.retain(|_, k| k.seen == generation);
}

fn tick_one(e: &Ticked, k: &mut Kept) {
    let [x, y, z] = e.pos;
    match e.kind {
        // `LightningBolt.tick` at life 2: its first tick.
        EntityKind::LightningBolt if !k.struck => {
            k.struck = true;
            if !e.silent {
                let thunder = 0.8 + unit() * 0.2;
                play(
                    "entity.lightning_bolt.thunder",
                    SoundCategory::Weather,
                    e.pos,
                    10000.0,
                    thunder,
                );
                let impact = 0.5 + unit() * 0.2;
                play(
                    "entity.lightning_bolt.impact",
                    SoundCategory::Weather,
                    e.pos,
                    2.0,
                    impact,
                );
            }
        }
        // `Blaze.aiStep`.
        EntityKind::Blaze if !e.silent && crate::audio::random_below(24) == 0 => {
            let volume = 1.0 + unit();
            let pitch = unit() * 0.7 + 0.3;
            play(
                "entity.blaze.burn",
                SoundCategory::Hostile,
                [x + 0.5, y + 0.5, z + 0.5],
                volume,
                pitch,
            );
        }
        // `Phantom.tick`: a wing beat as the flap cosine crosses zero.
        EntityKind::Phantom => {
            let phase = |t: u32| {
                let t = (e.id.wrapping_mul(3) as f32) + t as f32;
                (t * 7.448451f32.to_radians() + std::f32::consts::PI).cos()
            };
            if phase(e.ticks) > 0.0 && phase(e.ticks + 1) <= 0.0 {
                let volume = 0.95 + unit() * 0.05;
                let pitch = 0.95 + unit() * 0.05;
                play(
                    "entity.phantom.flap",
                    SoundCategory::Hostile,
                    e.pos,
                    volume,
                    pitch,
                );
            }
        }
        // `Warden.tick`: the heartbeat quickens with anger.
        EntityKind::Warden => {
            let anger = (e.anger as f32 / 80.0).clamp(0.0, 1.0);
            let delay = 40 - (anger * 30.0).floor() as u32;
            if e.ticks.is_multiple_of(delay) && !e.silent {
                let pitch = (unit() - unit()) * 0.2 + 1.0;
                play(
                    "entity.warden.heartbeat",
                    SoundCategory::Hostile,
                    e.pos,
                    5.0,
                    pitch,
                );
            }
        }
        // `Breeze.tick`: a whirl every 1 to 80 ticks.
        EntityKind::Breeze => {
            k.sound_tick = if k.sound_tick == 0 {
                1 + crate::audio::random_below(80) as i32
            } else {
                k.sound_tick - 1
            };
            if k.sound_tick == 0 {
                let pitch = 0.7 + 0.4 * unit();
                let volume = 0.8 + 0.2 * unit();
                play(
                    "entity.breeze.whirl",
                    SoundCategory::Hostile,
                    e.pos,
                    volume,
                    pitch,
                );
            }
        }
        // `Sniffer.playSearchingSound`.
        EntityKind::Sniffer if e.searching && e.ticks.is_multiple_of(20) => {
            play(
                "entity.sniffer.searching",
                SoundCategory::Neutral,
                e.pos,
                1.0,
                1.0,
            );
        }
        // `Enderman.onSyncedDataUpdated`: `Creepy` set while stared at,
        // at most every 400 ticks.
        EntityKind::Enderman => {
            let changed = k.creepy.is_some_and(|c| c != e.creepy);
            k.creepy = Some(e.creepy);
            let rested = k.last_stare.is_none_or(|t| e.ticks >= t + 400);
            if changed && e.stared_at && rested {
                k.last_stare = Some(e.ticks);
                if !e.silent {
                    let eye = [x, y + f64::from(e.eye_height), z];
                    play(
                        "entity.enderman.stare",
                        SoundCategory::Hostile,
                        eye,
                        2.5,
                        1.0,
                    );
                }
            }
        }
        _ => {}
    }
}

/// `handleEntityEvent` sounds: `eye_height` is the entity's.
pub(crate) fn entity_event(kind: EntityKind, pos: [f64; 3], eye_height: f32, event: u8) {
    let [x, y, z] = pos;
    match (kind, event) {
        (EntityKind::ArmorStand, 32) => {
            play(
                "entity.armor_stand.hit",
                SoundCategory::Neutral,
                pos,
                0.3,
                1.0,
            );
        }
        (EntityKind::ZombieVillager, 16) => {
            let volume = 1.0 + unit();
            let pitch = unit() * 0.7 + 0.3;
            let eye = [x, y + f64::from(eye_height), z];
            play(
                "entity.zombie_villager.cure",
                SoundCategory::Hostile,
                eye,
                volume,
                pitch,
            );
        }
        (EntityKind::EvokerFangs, 4) => {
            let pitch = unit() * 0.2 + 0.85;
            play(
                "entity.evoker_fangs.attack",
                SoundCategory::Neutral,
                pos,
                1.0,
                pitch,
            );
        }
        (EntityKind::Armadillo, 64) => {
            play(
                "entity.armadillo.peek",
                SoundCategory::Neutral,
                pos,
                1.0,
                1.0,
            );
        }
        _ => {}
    }
}
