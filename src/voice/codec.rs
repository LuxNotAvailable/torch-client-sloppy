//! Opus, as Simple Voice Chat uses it: 48 kHz mono, 20 ms frames
//! (`AudioUtils`). The codec is `torch-opus`, a safe interface over
//! `unsafe-libopus` (libopus 1.3.1 translated to Rust, so no C toolchain is
//! needed to build it).

use super::wire::Codec;

pub(crate) const SAMPLE_RATE: i32 = 48_000;
/// `AudioUtils.FRAME_SIZE`: 20 ms.
pub(crate) const FRAME: usize = 960;
/// `AudioUtils.MAX_OPUS_PAYLOAD_SIZE`.
const MAX_PAYLOAD: usize = 1275;

pub(crate) struct Decoder(torch_opus::Decoder);

impl Decoder {
    pub(crate) fn new() -> Option<Decoder> {
        torch_opus::Decoder::new(SAMPLE_RATE, 1).ok().map(Decoder)
    }

    /// One frame from `packet`, or a concealed frame for a lost one (`None`).
    pub(crate) fn decode(&mut self, packet: Option<&[u8]>) -> Vec<i16> {
        let mut pcm = vec![0i16; FRAME];
        let packet = packet.filter(|p| !p.is_empty());
        let got = self.0.decode(packet, &mut pcm).unwrap_or(0);
        pcm.truncate(got);
        pcm
    }

    /// `OpusDecoder.resetState`, after a stream ends: a fresh state.
    pub(crate) fn reset(&mut self) {
        if let Some(fresh) = Decoder::new() {
            *self = fresh;
        }
    }
}

pub(crate) struct Encoder(torch_opus::Encoder);

impl Encoder {
    /// An encoder in the application the server asked for (`ServerConfig.Codec`).
    pub(crate) fn new(codec: Codec) -> Option<Encoder> {
        let application = match codec {
            Codec::Voip => torch_opus::Application::Voip,
            Codec::Audio => torch_opus::Application::Audio,
            Codec::RestrictedLowDelay => torch_opus::Application::RestrictedLowDelay,
        };
        torch_opus::Encoder::new(SAMPLE_RATE, 1, application)
            .ok()
            .map(Encoder)
    }

    /// One 20 ms frame, encoded.
    pub(crate) fn encode(&mut self, frame: &[i16; FRAME]) -> Vec<u8> {
        let mut out = vec![0u8; MAX_PAYLOAD];
        let len = self.0.encode(frame, &mut out).unwrap_or(0);
        out.truncate(len);
        out
    }

    /// `OpusEncoder.resetState`, after a stop packet: a fresh state.
    pub(crate) fn reset(&mut self, codec: Codec) {
        if let Some(fresh) = Encoder::new(codec) {
            *self = fresh;
        }
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
