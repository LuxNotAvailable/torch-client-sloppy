//! Entity sounds vanilla's client plays itself (`playLocalSound` from an
//! entity's client tick or `handleEntityEvent`): the server never sends
//! them.

use std::collections::HashMap;
use std::sync::Mutex;

use azalea_registry::builtin::EntityKind;

use crate::audio::SoundCategory;

/// The entity data the sounds read, from the ECS.
#[derive(Clone, Copy, Default)]
pub(crate) struct Meta {
    pub silent: bool,
    pub baby: bool,
    /// The warden's `ClientAngerLevel`.
    pub anger: i32,
    /// The enderman's `Creepy` and `StaredAt`.
    pub creepy: bool,
    pub stared_at: bool,
    /// The sniffer's state.
    pub searching: bool,
    pub digging: bool,
    /// The bee's anger end time, in game ticks.
    pub anger_end: i64,
    /// The guardian's `AttackTarget` id, 0 for none.
    pub attack_target: i32,
}

/// What one entity's client tick needs.
pub(crate) struct Ticked {
    pub id: i32,
    pub kind: EntityKind,
    pub pos: [f64; 3],
    pub eye_height: f32,
    /// `tickCount` since this client saw it.
    pub ticks: u32,
    pub meta: Meta,
    pub invisible: bool,
    pub hurt_time: u32,
    /// The dragon's `oFlapTime` and `flapTime`.
    pub flap: Option<(f32, f32)>,
}

/// The kinds `tick` does anything for, so the feed reads only their data.
pub(crate) fn wants(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::LightningBolt
            | EntityKind::Blaze
            | EntityKind::Phantom
            | EntityKind::Warden
            | EntityKind::Breeze
            | EntityKind::Sniffer
            | EntityKind::Enderman
            | EntityKind::Bee
            | EntityKind::Guardian
            | EntityKind::ElderGuardian
            | EntityKind::EnderDragon
            | EntityKind::Illusioner
    )
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
    /// A sound instance following it, and whether it's the bee's aggressive
    /// loop.
    voice: Option<u64>,
    aggressive: bool,
    last_pos: Option<[f64; 3]>,
    /// The guardian's `clientSideAttackTime` and the target it counts for.
    attack_time: u32,
    attack_target: i32,
    /// An entity event asked for its sound instance (guardian 21, sniffer 63).
    requested: bool,
    seen: u64,
}

impl Drop for Kept {
    fn drop(&mut self) {
        if let Some(voice) = self.voice.take() {
            crate::audio::stop_voice(voice);
        }
    }
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
    let m = &e.meta;
    let at = [x as f32, y as f32, z as f32];
    let moved = k.last_pos.map_or(0.0, |p| {
        (((x - p[0]).powi(2) + (z - p[2]).powi(2)).sqrt()) as f32
    });
    k.last_pos = Some(e.pos);
    match e.kind {
        // `BeeFlyingSoundInstance` and `BeeAggressiveSoundInstance`, swapped
        // as the bee's anger comes and goes.
        EntityKind::Bee => {
            let angry = m.anger_end > 0 && m.anger_end > crate::client::tracking::game_time();
            if k.voice.is_none() || angry != k.aggressive {
                if let Some(voice) = k.voice.take() {
                    crate::audio::stop_voice(voice);
                }
                let event = if angry {
                    "entity.bee.loop_aggressive"
                } else {
                    "entity.bee.loop"
                };
                k.voice = crate::audio::start(
                    event,
                    SoundCategory::Neutral,
                    crate::audio::StreamPlace::At { at, range: 16.0 },
                    0.0,
                    1.0,
                    true,
                );
                k.aggressive = angry;
            }
            // `BeeSoundInstance.tick`.
            let (min, max) = if m.baby { (1.1, 1.5) } else { (0.7, 1.1) };
            let (volume, pitch) = if moved >= 0.01 && !m.silent {
                (
                    moved.clamp(0.0, 0.5) * 1.2,
                    min + moved.clamp(min, max) * (max - min),
                )
            } else {
                (0.0, min)
            };
            if let Some(voice) = k.voice {
                crate::audio::move_voice(voice, at);
                crate::audio::tune(voice, volume, pitch);
            }
        }
        // `GuardianAttackSoundInstance`, started by entity event 21: louder
        // and higher as the beam charges over 80 ticks.
        EntityKind::Guardian | EntityKind::ElderGuardian => {
            if m.attack_target != k.attack_target {
                k.attack_target = m.attack_target;
                k.attack_time = 0;
            }
            if m.attack_target != 0 && k.attack_time < 80 {
                k.attack_time += 1;
            }
            if std::mem::take(&mut k.requested) && k.voice.is_none() && !m.silent {
                k.voice = crate::audio::start(
                    "entity.guardian.attack",
                    SoundCategory::Hostile,
                    crate::audio::StreamPlace::Listener,
                    0.0,
                    0.7,
                    true,
                );
            }
            if let Some(voice) = k.voice {
                let scale = k.attack_time as f32 / 80.0;
                crate::audio::tune(voice, scale * scale, 0.7 + 0.5 * scale);
            }
        }
        // `LightningBolt.tick` at life 2: its first tick.
        EntityKind::LightningBolt if !k.struck => {
            k.struck = true;
            if !m.silent {
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
        EntityKind::Blaze if !m.silent && crate::audio::random_below(24) == 0 => {
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
            let anger = (m.anger as f32 / 80.0).clamp(0.0, 1.0);
            let delay = 40 - (anger * 30.0).floor() as u32;
            if e.ticks.is_multiple_of(delay) && !m.silent {
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
        // `EnderDragon.isFlapping` and `onFlap`: the wing passing -0.3.
        EntityKind::EnderDragon => {
            if let Some((old, new)) = e.flap {
                let tau = std::f32::consts::TAU;
                if (old * tau).cos() <= -0.3 && (new * tau).cos() >= -0.3 && !m.silent {
                    let pitch = 0.8 + unit() * 0.3;
                    play(
                        "entity.ender_dragon.flap",
                        SoundCategory::Hostile,
                        e.pos,
                        5.0,
                        pitch,
                    );
                }
            }
        }
        // `Illusioner.aiStep`: the illusions reshuffle when hurt and every
        // 1200 ticks while it's invisible.
        EntityKind::Illusioner if e.invisible && (e.hurt_time == 1 || e.ticks % 1200 == 0) => {
            play(
                "entity.illusioner.mirror_move",
                SoundCategory::Hostile,
                e.pos,
                1.0,
                1.0,
            );
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
        // `Sniffer.playSearchingSound`, and `SnifferSoundInstance` from
        // entity event 63 while it digs.
        EntityKind::Sniffer => {
            if m.searching && e.ticks.is_multiple_of(20) {
                play(
                    "entity.sniffer.searching",
                    SoundCategory::Neutral,
                    e.pos,
                    1.0,
                    1.0,
                );
            }
            if std::mem::take(&mut k.requested) && k.voice.is_none() && !m.silent {
                k.voice = crate::audio::start(
                    "entity.sniffer.digging",
                    SoundCategory::Neutral,
                    crate::audio::StreamPlace::At { at, range: 16.0 },
                    1.0,
                    1.0,
                    false,
                );
            }
            if let Some(voice) = k.voice {
                if m.digging && crate::audio::playing(voice) {
                    crate::audio::move_voice(voice, at);
                } else {
                    crate::audio::stop_voice(voice);
                    k.voice = None;
                }
            }
        }
        // `Enderman.onSyncedDataUpdated`: `Creepy` set while stared at,
        // at most every 400 ticks.
        EntityKind::Enderman => {
            let changed = k.creepy.is_some_and(|c| c != m.creepy);
            k.creepy = Some(m.creepy);
            let rested = k.last_stare.is_none_or(|t| e.ticks >= t + 400);
            if changed && m.stared_at && rested {
                k.last_stare = Some(e.ticks);
                if !m.silent {
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
pub(crate) fn entity_event(id: i32, kind: EntityKind, pos: [f64; 3], eye_height: f32, event: u8) {
    let [x, y, z] = pos;
    match (kind, event) {
        // `ClientPacketListener.handleEntityEvent`: the sound instances.
        (EntityKind::Guardian | EntityKind::ElderGuardian, 21) | (EntityKind::Sniffer, 63) => {
            if let Some(kept) = KEPT.lock().unwrap().as_mut() {
                kept.entry(id).or_default().requested = true;
            }
        }
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

/// The local player's riding sounds (`LocalPlayer.startRiding`'s
/// `RidingMinecartSoundInstance` and `RidingEntitySoundInstance`).
#[derive(Default)]
pub(crate) struct Riding {
    vehicle: Option<i32>,
    /// The vehicle's loops, with whether each is the underwater one.
    voices: Vec<(u64, bool)>,
    last_pos: Option<[f64; 3]>,
}

impl Riding {
    /// `vehicle` is what the player rides, if anything: its id, kind and
    /// position. `under_water` is the player's eye in water, `silent` the
    /// vehicle's.
    pub(crate) fn tick(
        &mut self,
        vehicle: Option<(i32, EntityKind, [f64; 3])>,
        under_water: bool,
        silent: bool,
    ) {
        if vehicle.map(|v| v.0) != self.vehicle {
            self.stop();
            self.vehicle = vehicle.map(|v| v.0);
            if let Some((_, kind, pos)) = vehicle {
                self.start(kind, pos);
            }
        }
        let Some((_, kind, pos)) = vehicle else {
            return;
        };
        let speed3 = self.last_pos.map_or(0.0, |p| {
            ((pos[0] - p[0]).powi(2) + (pos[1] - p[1]).powi(2) + (pos[2] - p[2]).powi(2)).sqrt()
        }) as f32;
        let horizontal = self.last_pos.map_or(0.0, |p| {
            ((pos[0] - p[0]).powi(2) + (pos[2] - p[2]).powi(2)).sqrt()
        }) as f32;
        self.last_pos = Some(pos);
        let minecart = is_minecart(kind);
        let (speed, max, amplifier) = if minecart {
            (horizontal, 0.75, 1.0)
        } else {
            (speed3, 1.0, 5.0)
        };
        for &(voice, underwater_sound) in &self.voices {
            let volume = if silent || underwater_sound != under_water || speed < 0.01 {
                0.0
            } else {
                amplifier * speed.clamp(0.0, max)
            };
            crate::audio::tune(voice, volume, 1.0);
        }
    }

    fn start(&mut self, kind: EntityKind, pos: [f64; 3]) {
        self.last_pos = Some(pos);
        let loops: &[(&str, SoundCategory, bool)] = if is_minecart(kind) {
            &[
                (
                    "entity.minecart.inside.underwater",
                    SoundCategory::Neutral,
                    true,
                ),
                ("entity.minecart.inside", SoundCategory::Neutral, false),
            ]
        } else if kind == EntityKind::HappyGhast {
            &[("entity.happy_ghast.riding", SoundCategory::Neutral, false)]
        } else if kind == EntityKind::Nautilus || kind == EntityKind::ZombieNautilus {
            &[("entity.nautilus.riding", SoundCategory::Neutral, true)]
        } else {
            &[]
        };
        for &(event, category, underwater) in loops {
            if let Some(voice) = crate::audio::start(
                event,
                category,
                crate::audio::StreamPlace::Listener,
                0.0,
                1.0,
                true,
            ) {
                self.voices.push((voice, underwater));
            }
        }
    }

    fn stop(&mut self) {
        for (voice, _) in self.voices.drain(..) {
            crate::audio::stop_voice(voice);
        }
        self.last_pos = None;
    }
}

fn is_minecart(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Minecart
            | EntityKind::ChestMinecart
            | EntityKind::FurnaceMinecart
            | EntityKind::HopperMinecart
            | EntityKind::TntMinecart
            | EntityKind::SpawnerMinecart
            | EntityKind::CommandBlockMinecart
    )
}

/// A block entity's client tick sounds (`VaultBlockEntity.Client`,
/// `TrialSpawner.tickClient`): a 2% chance each tick while a vault shows
/// its item or a spawner can spawn.
pub(crate) fn block_entity_tick(state: azalea::block::BlockState, pos: [i32; 3]) {
    let id = crate::util::block_model::block_id(state);
    let property = |name| crate::blocks::info::property(state, name);
    let event = match id {
        "vault" if property("vault_state") != Some("inactive") => "block.vault.ambient",
        "trial_spawner"
            if matches!(
                property("trial_spawner_state"),
                Some("waiting_for_players" | "active")
            ) =>
        {
            if property("ominous") == Some("true") {
                "block.trial_spawner.ambient_ominous"
            } else {
                "block.trial_spawner.ambient"
            }
        }
        _ => return,
    };
    if unit() <= 0.02 {
        let volume = unit() * 0.25 + 0.75;
        let pitch = unit() + 0.5;
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 0.5,
            f64::from(pos[2]) + 0.5,
        ];
        play(event, SoundCategory::Blocks, at, volume, pitch);
    }
}

/// The End's flash thunder: `ClientLevel.tick` plays a
/// `DirectionalSoundInstance` 30 ticks after a flash starts, 10 blocks from
/// the camera towards the flash, without falloff.
#[derive(Default)]
pub(crate) struct EndFlashes {
    /// Ticks left and the flash's angles, per flash waiting to be heard.
    pub(crate) pending: Vec<(u32, f32, f32)>,
}

impl EndFlashes {
    pub(crate) fn tick(&mut self, in_end: bool, clock_tick: i64, camera: [f32; 3]) {
        if !in_end {
            self.pending.clear();
            return;
        }
        if let Some((x, y)) = crate::renderer::packdraw::endflash::started(clock_tick) {
            self.pending.push((30, x, y));
        }
        self.pending.retain_mut(|(left, x_angle, y_angle)| {
            *left -= 1;
            if *left > 0 {
                return true;
            }
            // `Vec3.directionFromRotation`.
            let (x, y) = (x_angle.to_radians(), y_angle.to_radians());
            let (f, g) = (
                (-y - std::f32::consts::PI).cos(),
                (-y - std::f32::consts::PI).sin(),
            );
            let (h, i) = (-(-x).cos(), (-x).sin());
            let dir = [g * h, i, f * h];
            let at = std::array::from_fn(|k| camera[k] + dir[k] * 10.0);
            crate::audio::start(
                "weather.end_flash",
                SoundCategory::Weather,
                crate::audio::StreamPlace::At { at, range: 1.0e6 },
                1.0,
                1.0,
                false,
            );
            false
        });
    }
}
