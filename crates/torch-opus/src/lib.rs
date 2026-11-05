//! A safe Opus encoder and decoder.
//!
//! The codec is `unsafe-libopus`, a mechanical translation of libopus 1.3.1
//! to Rust, which keeps libopus's C interface: raw state pointers, and
//! buffers passed as pointer and length. This crate is the one place those
//! are touched. Every call checks the lengths it hands over against the
//! buffers it was given, each state is created once and destroyed once, and
//! a state is only reached through `&mut self`, so the types are `Send` but
//! not `Sync`.

#![deny(unsafe_op_in_unsafe_fn)]

use std::fmt;

use unsafe_libopus as opus;

/// What the encoder tunes for (`OPUS_APPLICATION_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Application {
    Voip,
    Audio,
    RestrictedLowDelay,
}

impl Application {
    fn code(self) -> i32 {
        match self {
            Application::Voip => opus::OPUS_APPLICATION_VOIP,
            Application::Audio => opus::OPUS_APPLICATION_AUDIO,
            Application::RestrictedLowDelay => opus::OPUS_APPLICATION_RESTRICTED_LOWDELAY,
        }
    }
}

/// A libopus error code (`OPUS_BAD_ARG`, `OPUS_BUFFER_TOO_SMALL`, ...), or
/// a size this crate refused before calling in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Opus(i32),
    /// A frame that isn't 2.5, 5, 10, 20, 40 or 60 ms at the sample rate,
    /// or a buffer that doesn't hold whole frames for the channels.
    BadFrame,
    /// Sample rate or channel count libopus doesn't take.
    BadConfig,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Opus(code) => write!(f, "libopus error {code}"),
            Error::BadFrame => f.write_str("not a whole Opus frame"),
            Error::BadConfig => f.write_str("unsupported sample rate or channel count"),
        }
    }
}

impl std::error::Error for Error {}

/// libopus's rates and channel counts.
fn check_config(sample_rate: i32, channels: usize) -> Result<(), Error> {
    let rate_ok = matches!(sample_rate, 8_000 | 12_000 | 16_000 | 24_000 | 48_000);
    if rate_ok && (channels == 1 || channels == 2) {
        Ok(())
    } else {
        Err(Error::BadConfig)
    }
}

/// Whether `samples_per_channel` is one of Opus's frame lengths.
fn is_frame(sample_rate: i32, samples_per_channel: usize) -> bool {
    // 2.5 ms is rate / 400; the others are its doubles up to 60 ms.
    let unit = sample_rate as usize / 400;
    [1, 2, 4, 8, 16, 24].iter().any(|n| samples_per_channel == unit * n)
}

/// Packets are at most 1275 bytes per frame and 6 frames (`opus_encode`).
pub const MAX_PACKET: usize = 1275 * 6;

/// The largest frame, 120 ms, in samples per channel at 48 kHz.
const MAX_FRAME_48K: usize = 5760;

pub struct Decoder {
    state: *mut opus::OpusDecoder,
    sample_rate: i32,
    channels: usize,
}

// SAFETY: the state is owned by this value alone and only reached through
// `&mut self`, so moving it to another thread moves sole access with it.
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new(sample_rate: i32, channels: usize) -> Result<Decoder, Error> {
        check_config(sample_rate, channels)?;
        let mut error = 0;
        // SAFETY: the rate and channel count are ones libopus takes, and
        // `error` is a valid place for it to write the result code.
        let state = unsafe { opus::opus_decoder_create(sample_rate, channels as i32, &mut error) };
        if error != opus::OPUS_OK || state.is_null() {
            return Err(Error::Opus(error));
        }
        Ok(Decoder {
            state,
            sample_rate,
            channels,
        })
    }

    /// Decodes `packet` into `out` (interleaved, whole frames), or for
    /// `None` conceals a lost packet across `out`. Returns the samples per
    /// channel written.
    pub fn decode(&mut self, packet: Option<&[u8]>, out: &mut [i16]) -> Result<usize, Error> {
        if !out.len().is_multiple_of(self.channels) {
            return Err(Error::BadFrame);
        }
        let frame = out.len() / self.channels;
        // Concealment has to cover a whole frame; a packet may be shorter
        // than `out`, which only bounds it.
        if packet.is_none() && !is_frame(self.sample_rate, frame) {
            return Err(Error::BadFrame);
        }
        let frame = frame.min(MAX_FRAME_48K * self.sample_rate as usize / 48_000) as i32;
        let (data, len) = match packet {
            Some(p) if !p.is_empty() => (p.as_ptr(), i32::try_from(p.len()).map_err(|_| Error::BadFrame)?),
            _ => (std::ptr::null(), 0),
        };
        // SAFETY: `self.state` came from `opus_decoder_create` and is live.
        // `data` points at `len` readable bytes, or is null with `len` 0,
        // which asks for concealment. `out` has room for `frame` samples per
        // channel, as `frame` is at most `out.len() / channels`.
        let got = unsafe { opus::opus_decode(self.state, data, len, out.as_mut_ptr(), frame, 0) };
        usize::try_from(got).map_err(|_| Error::Opus(got))
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by `opus_decoder_create` and destroyed only here.
        unsafe { opus::opus_decoder_destroy(self.state) }
    }
}

pub struct Encoder {
    state: *mut opus::OpusEncoder,
    sample_rate: i32,
    channels: usize,
}

// SAFETY: as for `Decoder`: sole ownership, reached through `&mut self`.
unsafe impl Send for Encoder {}

impl Encoder {
    pub fn new(sample_rate: i32, channels: usize, application: Application) -> Result<Encoder, Error> {
        check_config(sample_rate, channels)?;
        let mut error = 0;
        // SAFETY: the rate, channels and application are values libopus
        // takes, and `error` is a valid place for the result code.
        let state = unsafe {
            opus::opus_encoder_create(sample_rate, channels as i32, application.code(), &mut error)
        };
        if error != opus::OPUS_OK || state.is_null() {
            return Err(Error::Opus(error));
        }
        Ok(Encoder {
            state,
            sample_rate,
            channels,
        })
    }

    /// Encodes one frame of interleaved `pcm` into `out`, returning the
    /// packet's length.
    pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Result<usize, Error> {
        if !pcm.len().is_multiple_of(self.channels) {
            return Err(Error::BadFrame);
        }
        let frame = pcm.len() / self.channels;
        if !is_frame(self.sample_rate, frame) {
            return Err(Error::BadFrame);
        }
        let room = i32::try_from(out.len().min(MAX_PACKET)).map_err(|_| Error::BadFrame)?;
        // SAFETY: `self.state` came from `opus_encoder_create` and is live.
        // `pcm` holds exactly `frame` samples per channel, a frame length
        // libopus takes, and `out` has `room` writable bytes.
        let len = unsafe {
            opus::opus_encode(self.state, pcm.as_ptr(), frame as i32, out.as_mut_ptr(), room)
        };
        usize::try_from(len).map_err(|_| Error::Opus(len))
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: created by `opus_encoder_create` and destroyed only here.
        unsafe { opus::opus_encoder_destroy(self.state) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: i32 = 48_000;
    const FRAME: usize = 960;

    #[test]
    fn a_tone_survives_encoding_and_a_loss_is_concealed() {
        let mut encoder = Encoder::new(RATE, 1, Application::Voip).unwrap();
        let mut decoder = Decoder::new(RATE, 1).unwrap();
        let tone: Vec<i16> = (0..FRAME).map(|i| ((i as f32 * 0.06).sin() * 8000.0) as i16).collect();
        let mut packet = [0u8; 1275];
        let mut heard = [0i16; FRAME];
        for _ in 0..5 {
            let len = encoder.encode(&tone, &mut packet).unwrap();
            assert!(len > 0);
            assert_eq!(decoder.decode(Some(&packet[..len]), &mut heard).unwrap(), FRAME);
        }
        let energy: f64 = heard.iter().map(|s| f64::from(*s).powi(2)).sum();
        assert!(energy > 1e6, "decoded silence: {energy}");
        assert_eq!(decoder.decode(None, &mut heard).unwrap(), FRAME);
    }

    #[test]
    fn sizes_libopus_would_misread_are_refused_before_calling_it() {
        let mut encoder = Encoder::new(RATE, 1, Application::Audio).unwrap();
        let mut decoder = Decoder::new(RATE, 2).unwrap();
        let mut out = [0u8; 64];
        assert_eq!(encoder.encode(&[0; 900], &mut out), Err(Error::BadFrame));
        assert_eq!(decoder.decode(None, &mut [0; 961]), Err(Error::BadFrame));
        assert_eq!(decoder.decode(None, &mut [0; 1000]), Err(Error::BadFrame));
        assert!(matches!(Decoder::new(44_100, 1), Err(Error::BadConfig)));
        assert!(matches!(Encoder::new(RATE, 3, Application::Voip), Err(Error::BadConfig)));
    }

    #[test]
    fn a_short_output_buffer_is_reported_not_overrun() {
        let mut encoder = Encoder::new(RATE, 1, Application::Audio).unwrap();
        let noise: Vec<i16> = (0..FRAME).map(|i| ((i * 7919) % 65536) as i16).collect();
        let mut tiny = [0u8; 1];
        assert!(matches!(encoder.encode(&noise, &mut tiny), Err(Error::Opus(_)) | Ok(1)));
    }
}
