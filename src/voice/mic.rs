//! The microphone, as Simple Voice Chat's `MicThread` runs it: 20 ms mono
//! frames at 48 kHz, sent while push-to-talk is held or the frame is louder
//! than the activation threshold, for a few frames past either
//! (`ptt_deactivation_delay`, `voice_deactivation_delay`), then one empty
//! packet to end the transmission.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use rodio::cpal;
use rodio::cpal::traits::{DeviceTrait, StreamTrait};

use super::codec::{Encoder, FRAME, SAMPLE_RATE};
use super::wire::{Codec, Packet};

/// `MicrophoneActivationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activation {
    PushToTalk,
    Voice,
}

/// What the game sets and the microphone reads.
pub(crate) struct Controls {
    pub(crate) push_to_talk: AtomicBool,
    pub(crate) whisper: AtomicBool,
    pub(crate) muted: AtomicBool,
    pub(crate) activation: AtomicU8,
    /// `voice_activation_threshold` in dB, as f32 bits.
    pub(crate) threshold: AtomicU32,
    /// The loudest recent frame in dB, as f32 bits, for a level meter.
    pub(crate) level: AtomicU32,
    /// Whether frames are going out now.
    pub(crate) talking: AtomicBool,
}

pub(crate) static CONTROLS: Controls = Controls {
    push_to_talk: AtomicBool::new(false),
    whisper: AtomicBool::new(false),
    muted: AtomicBool::new(false),
    activation: AtomicU8::new(0),
    threshold: AtomicU32::new(DEFAULT_THRESHOLD.to_bits()),
    level: AtomicU32::new(LOWEST_DB.to_bits()),
    talking: AtomicBool::new(false),
};

/// `voice_activation_threshold`'s default.
pub(crate) const DEFAULT_THRESHOLD: f32 = -50.0;
/// `AudioUtils.LOWEST_DB`.
const LOWEST_DB: f32 = -127.0;
/// `ptt_deactivation_delay`.
const PTT_DELAY: u32 = 5;
/// `voice_deactivation_delay`.
const VOICE_DELAY: u32 = 25;

impl Controls {
    pub(crate) fn activation(&self) -> Activation {
        match self.activation.load(Ordering::Relaxed) {
            1 => Activation::Voice,
            _ => Activation::PushToTalk,
        }
    }

    pub(crate) fn set_activation(&self, activation: Activation) {
        self.activation.store(
            match activation {
                Activation::PushToTalk => 0,
                Activation::Voice => 1,
            },
            Ordering::Relaxed,
        );
    }
}

/// `AudioUtils.getHighestAudioLevel`: the peak sample in dB.
pub(crate) fn peak_db(frame: &[i16]) -> f32 {
    let peak = frame.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    if peak == 0 {
        return LOWEST_DB;
    }
    (20.0 * (f32::from(peak) / 32768.0).log10()).clamp(LOWEST_DB, 0.0)
}

/// `MicrophoneProcessor`: whether each frame goes out.
struct Gate {
    /// Frames still to send after the trigger stopped.
    hold: u32,
}

impl Gate {
    fn decide(&mut self, frame: &[i16], controls: &Controls) -> bool {
        let level = peak_db(frame);
        controls.level.store(level.to_bits(), Ordering::Relaxed);
        if controls.muted.load(Ordering::Relaxed) {
            self.hold = 0;
            return false;
        }
        let (triggered, delay) = match controls.activation() {
            Activation::PushToTalk => (controls.push_to_talk.load(Ordering::Relaxed), PTT_DELAY),
            Activation::Voice => {
                let threshold = f32::from_bits(controls.threshold.load(Ordering::Relaxed));
                (level >= threshold, VOICE_DELAY)
            }
        };
        if triggered {
            self.hold = delay;
            return true;
        }
        if self.hold > 0 {
            self.hold -= 1;
            return true;
        }
        false
    }
}

/// Frames the device callback cut, at 48 kHz mono.
struct Framer {
    rate: f64,
    /// Position in the device's samples of the next output sample.
    at: f64,
    previous: f32,
    frame: Vec<i16>,
}

impl Framer {
    fn new(rate: u32) -> Framer {
        Framer {
            rate: f64::from(rate.max(1)),
            at: 0.0,
            previous: 0.0,
            frame: Vec::with_capacity(FRAME),
        }
    }

    /// Mono samples from the device, resampled linearly to 48 kHz and cut
    /// into whole frames.
    fn feed(&mut self, mono: &[f32], out: &Sender<[i16; FRAME]>) {
        let step = self.rate / f64::from(SAMPLE_RATE);
        while self.at < mono.len() as f64 {
            let index = self.at.floor() as usize;
            let t = (self.at - index as f64) as f32;
            let a = if index == 0 {
                self.previous
            } else {
                mono[index - 1]
            };
            let b = mono[index];
            let sample = a + (b - a) * t;
            self.frame.push((sample.clamp(-1.0, 1.0) * 32767.0) as i16);
            if self.frame.len() == FRAME {
                let frame: [i16; FRAME] = self.frame[..].try_into().expect("a whole frame");
                self.frame.clear();
                let _ = out.send(frame);
            }
            self.at += step;
        }
        self.at -= mono.len() as f64;
        if let Some(last) = mono.last() {
            self.previous = *last;
        }
    }
}

/// The microphone thread; stopped when dropped.
pub(crate) struct Mic {
    stop: Arc<AtomicBool>,
    /// `super::input_generation` when it opened: a newer one means another
    /// microphone was chosen, and this one should be replaced.
    pub(crate) generation: u32,
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Mic {
    /// Opens the default input device and sends what it hears to `send`
    /// (the connection's queue), encoded as the server asked.
    pub(crate) fn start(codec: Codec, send: Sender<Packet>) -> Mic {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("voicechat-mic".into())
            .spawn(move || {
                if let Err(e) = run(codec, &send, &thread_stop) {
                    crate::log_warn!("voice", "no microphone: {e}");
                }
                CONTROLS.talking.store(false, Ordering::Relaxed);
            });
        if let Err(e) = spawned {
            crate::log_warn!("voice", "cannot start the microphone thread: {e}");
        }
        Mic {
            stop,
            generation: super::input_generation(),
        }
    }
}

fn open(frames: Sender<[i16; FRAME]>) -> Result<cpal::Stream, String> {
    let wanted = super::input_device();
    let device = crate::audio::devices::input(wanted.as_deref()).ok_or("no input device")?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let channels = usize::from(config.channels().max(1));
    let mut framer = Framer::new(config.sample_rate().0);
    crate::log_info!(
        "voice",
        "microphone: {} ({} Hz, {} channels)",
        device.name().unwrap_or_else(|_| "?".into()),
        config.sample_rate().0,
        channels
    );
    let error = |e| crate::log_warn!("voice", "microphone error: {e}");
    let mut mono = Vec::new();
    let mut take = move |samples: &mut dyn Iterator<Item = f32>| {
        mono.clear();
        let mut sum = 0.0;
        for (i, s) in samples.enumerate() {
            sum += s;
            if i % channels == channels - 1 {
                mono.push(sum / channels as f32);
                sum = 0.0;
            }
        }
        framer.feed(&mono, &frames);
    };
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| take(&mut data.iter().copied()),
            error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| take(&mut data.iter().map(|s| f32::from(*s) / 32768.0)),
            error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _: &_| {
                take(&mut data.iter().map(|s| (f32::from(*s) - 32768.0) / 32768.0))
            },
            error,
            None,
        ),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn run(codec: Codec, send: &Sender<Packet>, stop: &AtomicBool) -> Result<(), String> {
    let (frames_in, frames) = std::sync::mpsc::channel();
    // The stream must stay on this thread (cpal streams are not `Send`).
    let _stream = open(frames_in)?;
    let mut encoder = Encoder::new(codec).ok_or("cannot create an Opus encoder")?;
    let mut gate = Gate { hold: 0 };
    let mut sequence = 0i64;
    let mut sent = false;
    while !stop.load(Ordering::Relaxed) {
        let frame = match next(&frames) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(()) => break,
        };
        if gate.decide(&frame, &CONTROLS) {
            let opus = encoder.encode(&frame);
            let whispering = CONTROLS.whisper.load(Ordering::Relaxed);
            let _ = send.send(Packet::Mic {
                opus,
                sequence,
                whispering,
            });
            sequence += 1;
            sent = true;
        } else if sent {
            // `MicThread.flush`: an empty frame ends the transmission.
            let _ = send.send(Packet::Mic {
                opus: Vec::new(),
                sequence,
                whispering: false,
            });
            sequence += 1;
            sent = false;
            encoder.reset(codec);
        }
        CONTROLS.talking.store(sent, Ordering::Relaxed);
    }
    if sent {
        let _ = send.send(Packet::Mic {
            opus: Vec::new(),
            sequence,
            whispering: false,
        });
    }
    Ok(())
}

fn next(frames: &Receiver<[i16; FRAME]>) -> Result<Option<[i16; FRAME]>, ()> {
    match frames.recv_timeout(Duration::from_millis(100)) {
        Ok(frame) => Ok(Some(frame)),
        Err(RecvTimeoutError::Timeout) => Ok(None),
        Err(RecvTimeoutError::Disconnected) => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controls(activation: Activation) -> Controls {
        let c = Controls {
            push_to_talk: AtomicBool::new(false),
            whisper: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            activation: AtomicU8::new(0),
            threshold: AtomicU32::new(DEFAULT_THRESHOLD.to_bits()),
            level: AtomicU32::new(0),
            talking: AtomicBool::new(false),
        };
        c.set_activation(activation);
        c
    }

    #[test]
    fn levels_are_the_peak_in_decibels() {
        assert_eq!(peak_db(&[0; 4]), LOWEST_DB);
        assert!((peak_db(&[16384, -3]) - (-6.0206)).abs() < 1e-3);
        assert_eq!(peak_db(&[i16::MIN]), 0.0);
    }

    #[test]
    fn push_to_talk_holds_five_frames_past_release() {
        let c = controls(Activation::PushToTalk);
        let mut gate = Gate { hold: 0 };
        let quiet = [0i16; 8];
        assert!(!gate.decide(&quiet, &c));
        c.push_to_talk.store(true, Ordering::Relaxed);
        assert!(gate.decide(&quiet, &c));
        c.push_to_talk.store(false, Ordering::Relaxed);
        let sent = (0..10).filter(|_| gate.decide(&quiet, &c)).count();
        assert_eq!(sent, PTT_DELAY as usize);
    }

    #[test]
    fn voice_activation_opens_above_the_threshold_and_mute_shuts_it() {
        let c = controls(Activation::Voice);
        let mut gate = Gate { hold: 0 };
        let quiet = [10i16; 8];
        let loud = [12000i16; 8];
        assert!(!gate.decide(&quiet, &c));
        assert!(gate.decide(&loud, &c));
        let held = (0..40).filter(|_| gate.decide(&quiet, &c)).count();
        assert_eq!(held, VOICE_DELAY as usize);
        c.muted.store(true, Ordering::Relaxed);
        assert!(!gate.decide(&loud, &c));
    }

    #[test]
    fn the_framer_resamples_to_whole_48k_frames() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut framer = Framer::new(44_100);
        // One second at 44.1 kHz is fifty 20 ms frames at 48 kHz.
        let second: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        for chunk in second.chunks(441) {
            framer.feed(chunk, &tx);
        }
        let frames = rx.try_iter().count();
        assert!((49..=50).contains(&frames), "{frames}");
    }
}
