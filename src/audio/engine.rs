//! The mixer: one thread owns the output device and every voice playing.
//! Sounds are decoded once and kept (`Decoded`), and each voice is a source of
//! our own (`Voice`) whose gain, pan and pitch the thread can change while it
//! plays, which a following sound (one bound to a moving entity) and live
//! volume sliders need. Positioned sounds attenuate as vanilla's OpenAL source
//! does (`Channel.linearAttenuation`: linear, clamped, reference 0), and only
//! mono files are positioned, as OpenAL positions only mono buffers.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use super::SoundCategory;
use super::store::Pack;

pub(super) enum Command {
    Play(Play),
    /// A sound fed as it plays (a voice), from `buffer` at `rate` Hz, mono.
    Stream {
        id: u64,
        category: SoundCategory,
        volume: f32,
        place: Place,
        rate: u32,
        buffer: Arc<StreamBuffer>,
    },
    /// Puts a voice somewhere else, or at the listener.
    Relocate { id: u64, place: Place },
    /// A tickable sound's new volume and pitch (`AbstractTickableSoundInstance`).
    Tune { id: u64, volume: f32, pitch: f32 },
    /// Moves a positioned voice (one following an entity).
    Move { id: u64, at: [f32; 3] },
    /// `SoundManager.stop(name, source)`: every voice of `event` in
    /// `category`, either left out meaning any.
    Stop {
        category: Option<SoundCategory>,
        event: Option<Arc<str>>,
    },
    StopVoice { id: u64 },
    /// The speaker changed: everything playing stops, and the mixer opens
    /// the new one (`SoundEngine.reload` on a `soundDevice` change).
    Reopen,
}

pub(super) struct Play {
    pub id: u64,
    pub event: Arc<str>,
    pub key: Arc<str>,
    pub category: SoundCategory,
    /// The instance volume, before the category's: vanilla clamps it to 1
    /// for gain but lets it stretch the range (`Sound.getAttenuationDistance`).
    pub volume: f32,
    pub pitch: f32,
    pub place: Place,
    /// Starts over at its end until stopped.
    pub looping: bool,
    /// Decoded as it plays rather than all at once (music, records).
    pub stream: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Place {
    /// Played at the listener, unattenuated (the interface, music).
    Listener,
    /// At a point in the world, heard out to `range` blocks.
    World { at: [f32; 3], range: f32 },
}

const QUEUE: usize = 256;

/// Vanilla's static pool: 255 OpenAL sources less 8 kept for streaming
/// (`Library`); a sound past it does not start.
const MAX_VOICES: usize = 247;

/// Decoded sounds kept, by total samples (about 128 MB of f32).
const CACHE_SAMPLES: usize = 32 << 20;

/// How often voices are re-panned and re-weighed without a command.
const UPDATE: Duration = Duration::from_millis(20);

static SENDER: OnceLock<SyncSender<Command>> = OnceLock::new();

/// Ids of voices that finished since the last call, for the entity tracker.
static FINISHED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// Ids of the voices playing, for asking whether one still is.
static LIVE: std::sync::LazyLock<Mutex<std::collections::HashSet<u64>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// Whether voice `id` was started and has not ended. A voice just asked for
/// counts as playing until the mixer has tried it.
pub(super) fn live(id: u64) -> bool {
    LIVE.lock().is_ok_and(|live| live.contains(&id))
}

pub(super) fn expect(id: u64) {
    if let Ok(mut live) = LIVE.lock() {
        live.insert(id);
    }
}

pub(super) fn start(index_id: String) {
    let spawned = std::thread::Builder::new()
        .name("audio".to_owned())
        .spawn(move || run(&index_id));
    if let Err(e) = spawned {
        crate::log_warn!("audio", "cannot start the mixer thread: {e}");
    }
}

pub(super) fn send(command: Command) {
    let Some(sender) = SENDER.get() else {
        return;
    };
    match sender.try_send(command) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

pub(super) fn take_finished() -> Vec<u64> {
    FINISHED
        .lock()
        .map(|mut f| std::mem::take(&mut *f))
        .unwrap_or_default()
}

struct Decoded {
    samples: Box<[f32]>,
    channels: u16,
    rate: u32,
}

struct Cache {
    sounds: HashMap<Arc<str>, Arc<Decoded>>,
    order: VecDeque<Arc<str>>,
    held: usize,
}

impl Cache {
    fn get(&mut self, pack: &mut Pack, key: &Arc<str>) -> Option<Arc<Decoded>> {
        if let Some(found) = self.sounds.get(key) {
            return Some(found.clone());
        }
        let bytes = pack.read(key)?;
        let decoder = match rodio::Decoder::new(std::io::Cursor::new(bytes)) {
            Ok(decoder) => decoder,
            Err(e) => {
                crate::log_warn!("audio", "{key} will not decode: {e}");
                return None;
            }
        };
        let channels = rodio::Source::channels(&decoder).max(1);
        let rate = rodio::Source::sample_rate(&decoder).max(1);
        let samples: Box<[f32]> = rodio::Source::convert_samples::<f32>(decoder).collect();
        let decoded = Arc::new(Decoded {
            samples,
            channels,
            rate,
        });
        self.held += decoded.samples.len();
        while self.held > CACHE_SAMPLES
            && let Some(oldest) = self.order.pop_front()
        {
            if let Some(gone) = self.sounds.remove(&oldest) {
                self.held -= gone.samples.len();
            }
        }
        self.sounds.insert(key.clone(), decoded.clone());
        self.order.push_back(key.clone());
        Some(decoded)
    }
}

/// Samples a streamed sound plays, pushed by its owner; the stream ends once
/// `closed` is set and the samples run out.
#[derive(Default)]
pub(crate) struct StreamBuffer {
    pub(crate) samples: Mutex<VecDeque<f32>>,
    pub(crate) closed: AtomicBool,
}

/// Half a second at 48 kHz: more than this queued means the playback has
/// fallen behind, and the oldest is dropped rather than heard late.
const STREAM_BACKLOG: usize = 24_000;

impl StreamBuffer {
    pub(crate) fn push(&self, samples: impl IntoIterator<Item = f32>) {
        if let Ok(mut queue) = self.samples.lock() {
            queue.extend(samples);
            let over = queue.len().saturating_sub(STREAM_BACKLOG);
            queue.drain(..over);
        }
    }
}

/// A streamed sound: mono samples from its buffer to two eased gains, and
/// silence while the buffer is empty.
struct Streamed {
    buffer: Arc<StreamBuffer>,
    control: Arc<Control>,
    rate: u32,
    gain: [f32; 2],
    pending: Option<f32>,
}

impl Iterator for Streamed {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.pending.take() {
            return Some(right);
        }
        if self.control.stop.load(Ordering::Relaxed) {
            self.control.done.store(true, Ordering::Relaxed);
            return None;
        }
        let sample = match self.buffer.samples.lock().ok().and_then(|mut q| q.pop_front()) {
            Some(sample) => sample,
            None if self.buffer.closed.load(Ordering::Relaxed) => {
                self.control.done.store(true, Ordering::Relaxed);
                return None;
            }
            None => 0.0,
        };
        let target = [
            f32::from_bits(self.control.left.load(Ordering::Relaxed)),
            f32::from_bits(self.control.right.load(Ordering::Relaxed)),
        ];
        for (g, t) in self.gain.iter_mut().zip(target) {
            *g += (t - *g) * EASE;
        }
        self.pending = Some(sample * self.gain[1]);
        Some(sample * self.gain[0])
    }
}

impl rodio::Source for Streamed {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// What the mixer thread sets and a playing voice reads.
struct Control {
    left: AtomicU32,
    right: AtomicU32,
    /// Playback rate, as f32 bits, clamped to 0.5 to 2 (`calculatePitch`).
    pitch: AtomicU32,
    stop: AtomicBool,
    done: AtomicBool,
}

impl Control {
    fn new(pitch: f32) -> Arc<Control> {
        Arc::new(Control {
            left: AtomicU32::new(0),
            right: AtomicU32::new(0),
            pitch: AtomicU32::new(pitch.clamp(0.5, 2.0).to_bits()),
            stop: AtomicBool::new(false),
            done: AtomicBool::new(false),
        })
    }

    fn step(&self) -> f64 {
        f64::from(f32::from_bits(self.pitch.load(Ordering::Relaxed)))
    }
}

impl Control {
    fn set(&self, [left, right]: [f32; 2]) {
        self.left.store(left.to_bits(), Ordering::Relaxed);
        self.right.store(right.to_bits(), Ordering::Relaxed);
    }
}

/// A decoded sound played once at `pitch`, to two channels whose gains follow
/// `Control`, eased so a step in gain does not click.
struct Voice {
    sound: Arc<Decoded>,
    control: Arc<Control>,
    /// Position in source frames.
    at: f64,
    looping: bool,
    gain: [f32; 2],
    pending: Option<f32>,
}

const EASE: f32 = 0.002;

impl Iterator for Voice {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.pending.take() {
            return Some(right);
        }
        let frames = self.sound.samples.len() / self.sound.channels as usize;
        let end = (frames.max(1) - 1) as f64;
        if self.looping && self.at >= end && end > 0.0 {
            self.at %= end;
        }
        if self.control.stop.load(Ordering::Relaxed) || self.at >= end {
            self.control.done.store(true, Ordering::Relaxed);
            return None;
        }
        let target = [
            f32::from_bits(self.control.left.load(Ordering::Relaxed)),
            f32::from_bits(self.control.right.load(Ordering::Relaxed)),
        ];
        for (g, t) in self.gain.iter_mut().zip(target) {
            *g += (t - *g) * EASE;
        }
        let i = self.at as usize;
        let f = (self.at - i as f64) as f32;
        let channels = self.sound.channels as usize;
        let sample = |frame: usize, channel: usize| {
            let frame = frame.min(frames - 1);
            self.sound.samples[frame * channels + channel.min(channels - 1)]
        };
        let lerp = |channel: usize| sample(i, channel) * (1.0 - f) + sample(i + 1, channel) * f;
        let (left, right) = if channels == 1 {
            let s = lerp(0);
            (s, s)
        } else {
            (lerp(0), lerp(1))
        };
        self.at += self.control.step();
        self.pending = Some(right * self.gain[1]);
        Some(left * self.gain[0])
    }
}

/// A long file (music, a record) decoded as it plays, at its own rate; the
/// pitch these play at is 1.
struct FileVoice {
    decoder: rodio::source::SamplesConverter<rodio::Decoder<std::io::Cursor<Arc<[u8]>>>, f32>,
    channels: u16,
    rate: u32,
    control: Arc<Control>,
    gain: [f32; 2],
    pending: Option<f32>,
}

impl Iterator for FileVoice {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.pending.take() {
            return Some(right);
        }
        let end = |control: &Control| {
            control.done.store(true, Ordering::Relaxed);
            None
        };
        if self.control.stop.load(Ordering::Relaxed) {
            return end(&self.control);
        }
        let Some(left) = self.decoder.next() else {
            return end(&self.control);
        };
        let right = if self.channels >= 2 {
            let right = self.decoder.next().unwrap_or(left);
            for _ in 2..self.channels {
                self.decoder.next();
            }
            right
        } else {
            left
        };
        let target = [
            f32::from_bits(self.control.left.load(Ordering::Relaxed)),
            f32::from_bits(self.control.right.load(Ordering::Relaxed)),
        ];
        for (g, t) in self.gain.iter_mut().zip(target) {
            *g += (t - *g) * EASE;
        }
        self.pending = Some(right * self.gain[1]);
        Some(left * self.gain[0])
    }
}

impl rodio::Source for FileVoice {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl rodio::Source for Voice {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        self.sound.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Playing {
    id: u64,
    event: Arc<str>,
    category: SoundCategory,
    volume: f32,
    place: Place,
    mono: bool,
    control: Arc<Control>,
}

impl Playing {
    /// The two channels' gains now: the category's and master's volume times
    /// the instance volume (clamped to 1), then for a positioned mono sound
    /// the linear fall-off to its range and a constant-power pan.
    fn gains(&self, listener: &super::Listener) -> [f32; 2] {
        let gain = self.volume.clamp(0.0, 1.0) * super::category_gain(self.category);
        let Place::World { at, range } = self.place else {
            return [gain; 2];
        };
        if !self.mono {
            return [gain; 2];
        }
        let offset = [
            at[0] - listener.position[0],
            at[1] - listener.position[1],
            at[2] - listener.position[2],
        ];
        let distance = (offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2]).sqrt();
        let falloff = (1.0 - distance / range.max(1e-3)).clamp(0.0, 1.0);
        let side = if distance > 1e-4 {
            let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            (dot(offset, listener.right) / distance).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let angle = (side + 1.0) * std::f32::consts::FRAC_PI_4;
        // Centred, each ear hears what an unpositioned sound gives both.
        let level = gain * falloff * std::f32::consts::SQRT_2;
        [(level * angle.cos()).min(1.0), (level * angle.sin()).min(1.0)]
    }
}

fn run(index_id: &str) {
    let mut pack = match Pack::open(index_id) {
        Ok(pack) => pack,
        Err(e) => {
            crate::log_warn!("audio", "the sound pack will not open: {e}");
            return;
        }
    };
    let Some(text) = super::store::load_defs(index_id) else {
        crate::log_warn!("audio", "the sound definitions will not read");
        return;
    };
    let defs = match super::defs::Defs::parse(&text) {
        Ok(defs) => defs,
        Err(e) => {
            crate::log_warn!("audio", "sounds.json will not parse: {e}");
            return;
        }
    };
    drop(text);

    let Some(output) = open_output() else {
        return;
    };

    let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE);
    if SENDER.set(tx).is_err() {
        return;
    }
    crate::log_info!(
        "audio",
        "mixer running: {} events over {} files",
        defs.len(),
        pack.len()
    );
    super::install_defs(defs);
    mix(&rx, &mut pack, output);
}

/// The chosen speaker opened, or the default; `None` with no speaker at all.
fn open_output() -> Option<(rodio::OutputStream, rodio::OutputStreamHandle)> {
    let wanted = super::output_device();
    let opened = super::devices::output(wanted.as_deref())
        .ok_or_else(|| "no output device".to_owned())
        .and_then(|device| {
            use rodio::cpal::traits::DeviceTrait;
            let name = device.name().unwrap_or_else(|_| "?".into());
            let pair = rodio::OutputStream::try_from_device(&device).map_err(|e| e.to_string())?;
            crate::log_info!("audio", "speaker: {name}");
            Ok(pair)
        });
    match opened {
        Ok(pair) => Some(pair),
        Err(e) => {
            crate::log_warn!("audio", "{e}");
            rodio::OutputStream::try_default().ok()
        }
    }
}

fn mix(
    rx: &Receiver<Command>,
    pack: &mut Pack,
    mut output: (rodio::OutputStream, rodio::OutputStreamHandle),
) {
    let mut cache = Cache {
        sounds: HashMap::new(),
        order: VecDeque::new(),
        held: 0,
    };
    let mut playing: Vec<Playing> = Vec::new();
    loop {
        let mut commands = Vec::new();
        match rx.recv_timeout(UPDATE) {
            Ok(command) => {
                commands.push(command);
                commands.extend(rx.try_iter());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        for command in commands {
            if matches!(command, Command::Reopen) {
                for voice in &playing {
                    voice.control.stop.store(true, Ordering::Relaxed);
                }
                // The old stream goes first, so the device is free to reopen.
                if let Some(opened) = open_output() {
                    output = opened;
                }
                continue;
            }
            apply(command, &mut playing, &mut cache, pack, &output.1);
        }
        let listener = super::listener();
        let mut finished = Vec::new();
        playing.retain(|voice| {
            if voice.control.done.load(Ordering::Relaxed) {
                finished.push(voice.id);
                if let Ok(mut live) = LIVE.lock() {
                    live.remove(&voice.id);
                }
                return false;
            }
            voice.control.set(voice.gains(&listener));
            true
        });
        if !finished.is_empty()
            && let Ok(mut list) = FINISHED.lock()
        {
            list.extend(finished);
        }
    }
}

fn apply(
    command: Command,
    playing: &mut Vec<Playing>,
    cache: &mut Cache,
    pack: &mut Pack,
    handle: &rodio::OutputStreamHandle,
) {
    match command {
        Command::Play(play) => {
            let id = play.id;
            if !start_voice(play, playing, cache, pack, handle)
                && let Ok(mut live) = LIVE.lock()
            {
                live.remove(&id);
            }
        }
        Command::Tune { id, volume, pitch } => {
            for voice in playing.iter_mut().filter(|v| v.id == id) {
                voice.volume = volume;
                voice
                    .control
                    .pitch
                    .store(pitch.clamp(0.5, 2.0).to_bits(), Ordering::Relaxed);
            }
        }
        Command::Stream {
            id,
            category,
            volume,
            place,
            rate,
            buffer,
        } => {
            let control = Control::new(1.0);
            let voice = Playing {
                id,
                event: Arc::from("voice"),
                category,
                volume,
                place,
                mono: true,
                control: control.clone(),
            };
            let gains = voice.gains(&super::listener());
            control.set(gains);
            let source = Streamed {
                buffer,
                control,
                rate,
                gain: gains,
                pending: None,
            };
            if handle.play_raw(source).is_ok() {
                playing.push(voice);
            }
        }
        Command::Relocate { id, place } => {
            for voice in playing.iter_mut().filter(|v| v.id == id) {
                voice.place = place;
            }
        }
        Command::Move { id, at } => {
            for voice in playing.iter_mut().filter(|v| v.id == id) {
                if let Place::World { range, .. } = voice.place {
                    voice.place = Place::World { at, range };
                }
            }
        }
        Command::Stop { category, event } => {
            for voice in playing.iter() {
                if category.is_none_or(|c| c == voice.category)
                    && event.as_ref().is_none_or(|e| *e == voice.event)
                {
                    voice.control.stop.store(true, Ordering::Relaxed);
                }
            }
        }
        Command::Reopen => {}
        Command::StopVoice { id } => {
            for voice in playing.iter().filter(|v| v.id == id) {
                voice.control.stop.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// Starts `play`; false if it could not.
fn start_voice(
    play: Play,
    playing: &mut Vec<Playing>,
    cache: &mut Cache,
    pack: &mut Pack,
    handle: &rodio::OutputStreamHandle,
) -> bool {
    playing.retain(|voice| !voice.control.done.load(Ordering::Relaxed));
    if playing.len() >= MAX_VOICES {
        return false;
    }
    let control = Control::new(play.pitch);
    let mut voice = Playing {
        id: play.id,
        event: play.event,
        category: play.category,
        volume: play.volume,
        place: play.place,
        mono: true,
        control: control.clone(),
    };
    let started = if play.stream {
        let Some(bytes) = pack.read(&play.key) else {
            return false;
        };
        let decoder = match rodio::Decoder::new(std::io::Cursor::new(bytes)) {
            Ok(decoder) => decoder,
            Err(e) => {
                crate::log_warn!("audio", "{} will not decode: {e}", play.key);
                return false;
            }
        };
        let channels = rodio::Source::channels(&decoder).max(1);
        let rate = rodio::Source::sample_rate(&decoder).max(1);
        voice.mono = channels == 1;
        let gains = voice.gains(&super::listener());
        control.set(gains);
        handle
            .play_raw(FileVoice {
                decoder: rodio::Source::convert_samples(decoder),
                channels,
                rate,
                control,
                gain: gains,
                pending: None,
            })
            .is_ok()
    } else {
        let Some(sound) = cache.get(pack, &play.key) else {
            return false;
        };
        voice.mono = sound.channels == 1;
        let gains = voice.gains(&super::listener());
        control.set(gains);
        handle
            .play_raw(Voice {
                sound,
                control,
                at: 0.0,
                looping: play.looping,
                // Start at the gain, not eased up from silence.
                gain: gains,
                pending: None,
            })
            .is_ok()
    };
    if started {
        if let Ok(mut live) = LIVE.lock() {
            live.insert(voice.id);
        }
        playing.push(voice);
    }
    started
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(place: Place, mono: bool) -> Playing {
        Playing {
            id: 0,
            event: Arc::from("e"),
            category: SoundCategory::Blocks,
            volume: 1.0,
            place,
            mono,
            control: Control::new(1.0),
        }
    }

    const LISTENER: super::super::Listener = super::super::Listener::ORIGIN;

    #[test]
    fn a_positioned_sound_falls_off_linearly_to_its_range() {
        let near = voice(Place::World { at: [0.0, 0.0, -4.0], range: 16.0 }, true).gains(&LISTENER);
        let edge = voice(Place::World { at: [0.0, 0.0, -16.0], range: 16.0 }, true).gains(&LISTENER);
        // Ahead: centred, so each ear gets the same; 1 - 4/16 of full power.
        assert!((near[0] - near[1]).abs() < 1e-5);
        assert!((near[0] - 0.75).abs() < 1e-5, "{near:?}");
        assert_eq!(edge, [0.0, 0.0]);
    }

    #[test]
    fn a_sound_to_the_right_is_heard_on_the_right() {
        let g = voice(Place::World { at: [4.0, 0.0, 0.0], range: 16.0 }, true).gains(&LISTENER);
        assert!(g[1] > 0.7 && g[0] < 1e-5, "{g:?}");
    }

    #[test]
    fn stereo_files_and_the_interface_are_not_positioned() {
        let _serial = super::super::VOLUME_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let stereo = voice(Place::World { at: [40.0, 0.0, 0.0], range: 16.0 }, false);
        let ui = voice(Place::Listener, true);
        let full = super::super::category_gain(SoundCategory::Blocks);
        assert_eq!(stereo.gains(&LISTENER), [full; 2]);
        assert_eq!(ui.gains(&LISTENER), [full; 2]);
    }
}
