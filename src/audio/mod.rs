pub mod ambience;
pub mod category;
pub(crate) mod defs;
pub mod devices;
mod engine;
pub mod environment;
pub mod music;
pub(crate) mod store;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub use category::SoundCategory;

use engine::{Command, Place, StreamBuffer};

static DEFS: OnceLock<defs::Defs> = OnceLock::new();

static VOLUMES: [AtomicU32; SoundCategory::COUNT] =
    [const { AtomicU32::new(DEFAULT_VOLUME) }; SoundCategory::COUNT];

pub const DEFAULT_VOLUME: u32 = 100;

static LISTENER: Mutex<Listener> = Mutex::new(Listener::ORIGIN);

#[derive(Clone, Copy)]
struct Listener {
    position: [f32; 3],
    /// The ear-to-ear axis, which is all panning needs.
    right: [f32; 3],
}

impl Listener {
    const ORIGIN: Listener = Listener {
        position: [0.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0],
    };
}

fn listener() -> Listener {
    LISTENER.lock().map_or(Listener::ORIGIN, |l| *l)
}

pub fn init(index_id: &str) {
    if DEFS.get().is_some() {
        return;
    }
    if !store::installed(index_id) {
        crate::log_info!(
            "audio",
            "no sound set for index {index_id}; the client will be silent"
        );
        return;
    }
    engine::start(index_id.to_owned());
}

pub(crate) fn install_defs(parsed: defs::Defs) {
    let _ = DEFS.set(parsed);
}

pub fn ready() -> bool {
    DEFS.get().is_some()
}

pub fn set_volumes(volumes: &[u32; SoundCategory::COUNT]) {
    for (slot, value) in volumes.iter().enumerate() {
        VOLUMES[slot].store((*value).min(100), Ordering::Relaxed);
    }
}

pub fn set_listener(position: [f32; 3], right: [f32; 3]) {
    if let Ok(mut listener) = LISTENER.lock() {
        *listener = Listener { position, right };
    }
}

/// The speaker chosen by name (`soundDevice`), `None` for the default.
static OUTPUT_DEVICE: Mutex<Option<String>> = Mutex::new(None);

fn output_device() -> Option<String> {
    OUTPUT_DEVICE.lock().ok().and_then(|d| d.clone())
}

/// Chooses the speaker; the mixer moves to it if it is running.
pub fn set_output_device(name: Option<String>) {
    let changed = OUTPUT_DEVICE
        .lock()
        .is_ok_and(|mut d| std::mem::replace(&mut *d, name.clone()) != name);
    if changed {
        engine::send(Command::Reopen);
    }
}

fn volume_of(category: SoundCategory) -> f32 {
    VOLUMES[category.index()].load(Ordering::Relaxed) as f32 / 100.0
}

/// `Options.getFinalSoundSourceVolume`: master times the category's own.
/// Read by the mixer as sounds play, so a slider moves what is playing.
fn category_gain(category: SoundCategory) -> f32 {
    let master = volume_of(SoundCategory::Master);
    if category == SoundCategory::Master {
        master
    } else {
        master * volume_of(category)
    }
}

/// `minecraft:entity.pig.ambient` as sounds.json names it.
pub fn event_name(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// A sound at the listener: the interface, toasts.
pub fn play(event: &str, category: SoundCategory, volume: f32, pitch: f32) {
    submit(event, category, volume, pitch, None, roll_seed(), true);
}

/// Whether a sound at `at` could be heard at the default range, to skip
/// working out one that could not.
pub fn audible_at(at: [f32; 3]) -> bool {
    if DEFS.get().is_none() {
        return false;
    }
    let listener = listener();
    let d: f32 = (0..3).map(|i| (at[i] - listener.position[i]).powi(2)).sum();
    d <= defs::DEFAULT_ATTENUATION * defs::DEFAULT_ATTENUATION
}

/// A sound the client makes itself, at a point.
pub fn play_at(event: &str, category: SoundCategory, at: [f32; 3], volume: f32, pitch: f32) {
    submit(event, category, volume, pitch, Some(at), roll_seed(), true);
}

/// A sound the server sent (`ClientboundSoundPacket`): its seed chooses
/// the variant, as `RandomSource.create(seed)` does, so every client hears
/// the same one.
pub fn play_seeded_at(
    event: &str,
    category: SoundCategory,
    at: [f32; 3],
    volume: f32,
    pitch: f32,
    seed: u64,
) {
    submit(event, category, volume, pitch, Some(at), seed as i64, true);
}

/// `SoundManager.stop(name, source)`: either left out means every one.
pub fn stop(category: Option<SoundCategory>, event: Option<&str>) {
    engine::send(Command::Stop {
        category,
        event: event.map(Arc::from),
    });
}

/// The voice id, so a sound can be moved or stopped later. `cull` skips a
/// positioned sound already out of its range; one following an entity starts
/// anyway, since the entity may come closer.
fn submit(
    event: &str,
    category: SoundCategory,
    volume: f32,
    pitch: f32,
    at: Option<[f32; 3]>,
    seed: i64,
    cull: bool,
) -> Option<u64> {
    submit_looping(event, category, volume, pitch, at, seed, cull, false)
}

#[allow(clippy::too_many_arguments)]
fn submit_looping(
    event: &str,
    category: SoundCategory,
    volume: f32,
    pitch: f32,
    at: Option<[f32; 3]>,
    seed: i64,
    cull: bool,
    looping: bool,
) -> Option<u64> {
    let defs = DEFS.get()?;
    debug_assert_ne!(category, SoundCategory::Master, "{event} played on Master");
    let mut random = crate::util::javarandom::JavaRandom::new(seed);
    let picked = defs.pick(event, &mut |bound| random.next_int(bound))?;
    // `AbstractSoundInstance.getVolume`/`getPitch`: the instance's times the
    // chosen sound's.
    let volume = volume * picked.volume;
    let pitch = pitch * picked.pitch;
    let place = match at {
        None => Place::Listener,
        Some(at) => {
            let range = volume.max(1.0) * picked.attenuation;
            let listener = listener();
            let d: f32 = (0..3).map(|i| (at[i] - listener.position[i]).powi(2)).sum();
            if cull && d > range * range {
                return None;
            }
            Place::World { at, range }
        }
    };
    let id = NEXT_VOICE.fetch_add(1, Ordering::Relaxed);
    engine::expect(id);
    engine::send(Command::Play(engine::Play {
        id,
        event: Arc::from(event),
        key: picked.object_key(),
        category,
        volume,
        pitch,
        place,
        looping,
        stream: picked.stream,
    }));
    Some(id)
}

/// A sound the client keeps a hold of (`AbstractTickableSoundInstance`):
/// it can loop, be moved, retuned and stopped, and asked whether it still
/// plays. Never culled by distance, since what it follows may come closer.
pub fn start(
    event: &str,
    category: SoundCategory,
    place: StreamPlace,
    volume: f32,
    pitch: f32,
    looping: bool,
) -> Option<u64> {
    let at = match place {
        StreamPlace::Listener => None,
        StreamPlace::At { at, .. } => Some(at),
    };
    submit_looping(event, category, volume, pitch, at, roll_seed(), false, looping)
}

/// Stops one sound `start` returned.
pub fn stop_voice(id: u64) {
    engine::send(Command::StopVoice { id });
}

pub fn move_voice(id: u64, at: [f32; 3]) {
    engine::send(Command::Move { id, at });
}

pub fn tune(id: u64, volume: f32, pitch: f32) {
    engine::send(Command::Tune { id, volume, pitch });
}

/// Whether a sound `start` returned is still playing.
pub fn playing(id: u64) -> bool {
    engine::live(id)
}

static NEXT_VOICE: AtomicU64 = AtomicU64::new(1);

/// A sound fed as it plays: voice chat. Mono samples at `rate`; dropped, it
/// plays out what it holds and ends.
pub struct Stream {
    id: u64,
    buffer: Arc<StreamBuffer>,
}

/// Where a stream is heard: at the listener, or at a point out to `range`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StreamPlace {
    Listener,
    At { at: [f32; 3], range: f32 },
}

impl StreamPlace {
    fn engine(self) -> Place {
        match self {
            StreamPlace::Listener => Place::Listener,
            StreamPlace::At { at, range } => Place::World { at, range },
        }
    }
}

/// A new stream in `category`, or `None` with no mixer.
pub fn open_stream(category: SoundCategory, volume: f32, place: StreamPlace, rate: u32) -> Option<Stream> {
    DEFS.get()?;
    let id = NEXT_VOICE.fetch_add(1, Ordering::Relaxed);
    let buffer = Arc::new(StreamBuffer::default());
    engine::send(Command::Stream {
        id,
        category,
        volume,
        place: place.engine(),
        rate,
        buffer: buffer.clone(),
    });
    Some(Stream { id, buffer })
}

impl Stream {
    /// The stream's volume from now on (a player's voice volume moved).
    pub fn set_volume(&self, volume: f32) {
        engine::send(Command::Tune {
            id: self.id,
            volume,
            pitch: 1.0,
        });
    }

    /// Queues 16-bit samples to play next.
    pub fn push(&self, pcm: &[i16]) {
        self.buffer.push(pcm.iter().map(|s| f32::from(*s) / 32768.0));
    }

    pub fn place(&self, place: StreamPlace) {
        engine::send(Command::Relocate {
            id: self.id,
            place: place.engine(),
        });
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.buffer.closed.store(true, Ordering::Relaxed);
    }
}

fn roll_seed() -> i64 {
    use crate::util::javarandom::JavaRandom;
    static RNG: Mutex<Option<JavaRandom>> = Mutex::new(None);
    let Ok(mut guard) = RNG.lock() else {
        return 0;
    };
    let rng = guard.get_or_insert_with(|| {
        let elapsed = crate::platform::time::epoch().elapsed().as_nanos() as i64;
        JavaRandom::new(elapsed ^ (&raw const RNG as usize as i64))
    });
    rng.next_long()
}

/// A sound waiting for the next tick to find its entity
/// (`ClientboundSoundEntityPacket`).
struct PendingBound {
    entity: i32,
    event: String,
    category: SoundCategory,
    volume: f32,
    pitch: f32,
    seed: u64,
}

static PENDING: Mutex<Vec<PendingBound>> = Mutex::new(Vec::new());

/// Items and orbs picked up, waiting for the tick to find them.
static PICKUPS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// Where each item and orb was last tick: the server removes a picked-up
/// item in the same batch it announces the pickup in, so by the tick it may
/// be gone.
static LAST_ITEMS: std::sync::LazyLock<Mutex<HashMap<i32, EntityAt>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Each tick, every item and orb the client sees.
pub fn remember_items(items: impl IntoIterator<Item = (i32, EntityAt)>) {
    if let Ok(mut last) = LAST_ITEMS.lock() {
        last.clear();
        last.extend(items);
    }
}

/// `handleTakeItemEntity`: someone picked up entity `item` (its network
/// id); the pickup sound plays where it lay, on the next tick.
pub fn on_take_item(item: i32) {
    if DEFS.get().is_some()
        && let Ok(mut pickups) = PICKUPS.lock()
    {
        pickups.push(item);
    }
}

/// What the tick knows of an entity: where its feet are, and whether it is
/// an experience orb.
pub struct EntityAt {
    pub at: [f32; 3],
    pub orb: bool,
}

/// A random number below `bound`, for the client's own sounds.
pub fn random_below(bound: u32) -> u32 {
    (roll_seed() as u64 % u64::from(bound.max(1))) as u32
}

fn uniform() -> f32 {
    (roll_seed() as u64 >> 40) as f32 / (1u64 << 24) as f32
}

/// Voices following an entity (`EntityBoundSoundInstance`), by voice id.
static BOUND: Mutex<Option<HashMap<u64, i32>>> = Mutex::new(None);

/// A sound attached to entity `entity` (its network id), started on the next
/// game tick at the entity's position and moved with it until it ends or the
/// entity goes. An entity this client does not know plays nothing, as in
/// `handleSoundEntityEvent`.
pub fn play_on_entity(
    event: &str,
    category: SoundCategory,
    entity: i32,
    volume: f32,
    pitch: f32,
    seed: u64,
) {
    if DEFS.get().is_none() {
        return;
    }
    if let Ok(mut pending) = PENDING.lock() {
        pending.push(PendingBound {
            entity,
            event: event.to_owned(),
            category,
            volume,
            pitch,
            seed,
        });
    }
}

/// Each game tick, with where each entity is now (feet, in blocks): starts
/// the waiting entity sounds and moves the playing ones, stopping those
/// whose entity is gone (`EntityBoundSoundInstance.tick`).
pub fn tick_entities(entity: impl Fn(i32) -> Option<EntityAt>) {
    if DEFS.get().is_none() {
        return;
    }
    let position = |id: i32| entity(id).map(|e| e.at);
    let pickups = PICKUPS
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default();
    let last = LAST_ITEMS.lock().ok();
    for item in pickups {
        let Some(found) = entity(item).or_else(|| {
            last.as_ref()?.get(&item).map(|e| EntityAt { at: e.at, orb: e.orb })
        }) else {
            continue;
        };
        // `handleTakeItemEntity`'s volumes and pitches.
        let spread = uniform() - uniform();
        if found.orb {
            play_at(
                "entity.experience_orb.pickup",
                SoundCategory::Players,
                found.at,
                0.1,
                spread * 0.35 + 0.9,
            );
        } else {
            play_at(
                "entity.item.pickup",
                SoundCategory::Players,
                found.at,
                0.2,
                spread * 1.4 + 2.0,
            );
        }
    }
    let pending = PENDING
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default();
    let Ok(mut bound) = BOUND.lock() else { return };
    let bound = bound.get_or_insert_with(HashMap::new);
    for id in engine::take_finished() {
        bound.remove(&id);
    }
    bound.retain(|voice, entity| match position(*entity) {
        Some(at) => {
            engine::send(Command::Move { id: *voice, at });
            true
        }
        None => {
            engine::send(Command::StopVoice { id: *voice });
            false
        }
    });
    for sound in pending {
        let Some(at) = position(sound.entity) else {
            continue;
        };
        let voice = submit(
            &sound.event,
            sound.category,
            sound.volume,
            sound.pitch,
            Some(at),
            sound.seed as i64,
            false,
        );
        if let Some(voice) = voice {
            bound.insert(voice, sound.entity);
        }
    }
}

static AMBIENCE: std::sync::LazyLock<Mutex<ambience::Ambience>> =
    std::sync::LazyLock::new(|| Mutex::new(ambience::Ambience::default()));

static MUSIC: std::sync::LazyLock<Mutex<(music::MusicManager, music::Situation)>> =
    std::sync::LazyLock::new(|| Mutex::new((music::MusicManager::default(), music::Situation::Menu)));

/// Each game tick: the ambience, and where the music should follow.
pub fn tick_world(surroundings: &ambience::Surroundings, situation: music::Situation) {
    if DEFS.get().is_none() {
        return;
    }
    if let Ok(mut ambience) = AMBIENCE.lock() {
        ambience.tick(surroundings);
    }
    if let Ok(mut music) = MUSIC.lock() {
        music.1 = situation;
    }
}

/// Leaving a world: the ambience stops, and the menus' music takes over.
pub fn leave_world() {
    if let Ok(mut ambience) = AMBIENCE.lock() {
        ambience.stop();
    }
    if let Ok(mut guard) = MUSIC.lock() {
        // `Minecraft.disconnect` stops the world's music.
        let (manager, situation) = &mut *guard;
        *situation = music::Situation::Menu;
        let menu = situation.music();
        manager.stop(menu.as_ref(), &mut music::Mixer);
    }
}

/// Twenty times a second, in game or not.
pub fn tick_music(in_game: bool) {
    if DEFS.get().is_none() {
        return;
    }
    let Ok(mut guard) = MUSIC.lock() else { return };
    let (manager, situation) = &mut *guard;
    if !in_game {
        *situation = music::Situation::Menu;
    }
    let wanted = situation.music();
    manager.tick(wanted.as_ref(), &mut music::Mixer);
}

/// Tests that read or set the shared volumes take this first.
#[cfg(test)]
static VOLUME_TESTS: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_scales_every_category_but_itself() {
        let _serial = VOLUME_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let saved: [u32; SoundCategory::COUNT] =
            std::array::from_fn(|i| VOLUMES[i].load(Ordering::Relaxed));
        let mut volumes = [100; SoundCategory::COUNT];
        volumes[SoundCategory::Master.index()] = 50;
        volumes[SoundCategory::Blocks.index()] = 50;
        set_volumes(&volumes);
        assert_eq!(category_gain(SoundCategory::Master), 0.5);
        assert_eq!(category_gain(SoundCategory::Blocks), 0.25);
        assert_eq!(category_gain(SoundCategory::Hostile), 0.5);
        set_volumes(&saved);
    }

    #[test]
    fn a_registry_name_loses_its_namespace() {
        assert_eq!(event_name("minecraft:entity.pig.ambient"), "entity.pig.ambient");
        assert_eq!(event_name("mymod:thing"), "mymod:thing");
    }
}
