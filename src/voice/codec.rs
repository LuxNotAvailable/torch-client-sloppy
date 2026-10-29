//! Opus, as Simple Voice Chat uses it: 48 kHz mono, 20 ms frames
//! (`AudioUtils`). The codec is `unsafe-libopus`, a mechanical translation of
//! libopus 1.3.1 to Rust, so no C toolchain is needed to build it.
// The Opus codec's interface is C-shaped (raw encoder and decoder states).
#![allow(unsafe_code)]

use unsafe_libopus as opus;

use super::wire::Codec;

pub(crate) const SAMPLE_RATE: i32 = 48_000;
/// `AudioUtils.FRAME_SIZE`: 20 ms.
pub(crate) const FRAME: usize = 960;
/// `AudioUtils.MAX_OPUS_PAYLOAD_SIZE`.
const MAX_PAYLOAD: usize = 1275;

pub(crate) struct Decoder {
    state: *mut opus::OpusDecoder,
}

// The state is owned and only touched through `&mut self`.
unsafe impl Send for Decoder {}

impl Decoder {
    pub(crate) fn new() -> Option<Decoder> {
        let mut error = 0;
        // SAFETY: plain constructor; checked below.
        let state = unsafe { opus::opus_decoder_create(SAMPLE_RATE, 1, &mut error) };
        (error == opus::OPUS_OK && !state.is_null()).then_some(Decoder { state })
    }

    /// One frame from `packet`, or a concealed frame for a lost one (`None`).
    pub(crate) fn decode(&mut self, packet: Option<&[u8]>) -> Vec<i16> {
        let mut pcm = vec![0i16; FRAME];
        let (data, len) = match packet {
            Some(p) if !p.is_empty() => (p.as_ptr(), p.len() as i32),
            _ => (std::ptr::null(), 0),
        };
        // SAFETY: `pcm` holds FRAME samples, the size passed; `data` is
        // `len` bytes or null for concealment.
        let got =
            unsafe { opus::opus_decode(self.state, data, len, pcm.as_mut_ptr(), FRAME as i32, 0) };
        pcm.truncate(usize::try_from(got).unwrap_or(0));
        pcm
    }

    /// `OpusDecoder.resetState`, after a stream ends: a fresh state.
    pub(crate) fn reset(&mut self) {
        if let Some(fresh) = Decoder::new() {
            *self = fresh;
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by `opus_decoder_create`, destroyed once.
        unsafe { opus::opus_decoder_destroy(self.state) }
    }
}

pub(crate) struct Encoder {
    state: *mut opus::OpusEncoder,
}

unsafe impl Send for Encoder {}

impl Encoder {
    /// An encoder in the application the server asked for (`ServerConfig.Codec`).
    pub(crate) fn new(codec: Codec) -> Option<Encoder> {
        let application = match codec {
            Codec::Voip => opus::OPUS_APPLICATION_VOIP,
            Codec::Audio => opus::OPUS_APPLICATION_AUDIO,
            Codec::RestrictedLowDelay => opus::OPUS_APPLICATION_RESTRICTED_LOWDELAY,
        };
        let mut error = 0;
        // SAFETY: plain constructor; checked below.
        let state = unsafe { opus::opus_encoder_create(SAMPLE_RATE, 1, application, &mut error) };
        (error == opus::OPUS_OK && !state.is_null()).then_some(Encoder { state })
    }

    /// One 20 ms frame, encoded.
    pub(crate) fn encode(&mut self, frame: &[i16; FRAME]) -> Vec<u8> {
        let mut out = vec![0u8; MAX_PAYLOAD];
        // SAFETY: `frame` is FRAME samples; `out` holds MAX_PAYLOAD bytes.
        let len = unsafe {
            opus::opus_encode(
                self.state,
                frame.as_ptr(),
                FRAME as i32,
                out.as_mut_ptr(),
                MAX_PAYLOAD as i32,
            )
        };
        out.truncate(usize::try_from(len).unwrap_or(0));
        out
    }

    /// `OpusEncoder.resetState`, after a stop packet: a fresh state.
    pub(crate) fn reset(&mut self, codec: Codec) {
        if let Some(fresh) = Encoder::new(codec) {
            *self = fresh;
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: created by `opus_encoder_create`, destroyed once.
        unsafe { opus::opus_encoder_destroy(self.state) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_survives_a_round_trip_and_a_loss_is_concealed() {
        let mut encoder = Encoder::new(Codec::Voip).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let tone: [i16; FRAME] = std::array::from_fn(|i| ((i as f32 * 0.06).sin() * 8000.0) as i16);
        let mut heard = Vec::new();
        for _ in 0..5 {
            let packet = encoder.encode(&tone);
            assert!(!packet.is_empty() && packet.len() <= MAX_PAYLOAD);
            heard = decoder.decode(Some(&packet));
            assert_eq!(heard.len(), FRAME);
        }
        let energy: f64 = heard.iter().map(|s| f64::from(*s).powi(2)).sum();
        assert!(energy > 1e6, "decoded silence: {energy}");
        assert_eq!(decoder.decode(None).len(), FRAME);
        decoder.reset();
        encoder.reset(Codec::Voip);
    }
}
