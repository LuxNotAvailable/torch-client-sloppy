//! Voices from the server, played as Simple Voice Chat's `AudioChannel`
//! plays them: one channel per stream, a small reorder buffer
//! (`AudioPacketBuffer`), lost frames concealed by the decoder up to the
//! output buffer's length, and an empty packet ending a transmission. A
//! player's voice is heard at their eyes out to the distance the server sent,
//! fading linearly as the mod's OpenAL speaker does.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::codec::{Decoder, FRAME, SAMPLE_RATE};
use super::wire::{Packet, Sound};
use crate::audio::{SoundCategory, StreamPlace};

/// `audio_packet_threshold`: packets held back for one that arrived late.
const PACKET_THRESHOLD: usize = 3;
/// `output_buffer_size`: the most lost frames concealed before resetting.
const OUTPUT_BUFFER: i64 = 5;
/// `AudioChannel.canKill`: a channel silent this long is closed.
const IDLE: Duration = Duration::from_secs(30);

/// Where voices are heard from, kept by the game tick.
pub(crate) struct Listening<'a> {
    /// This player's id, whose own voice (a plugin playing it back) is
    /// heard unpositioned.
    pub(crate) me: Option<Uuid>,
    pub(crate) listener: [f32; 3],
    /// Players' eye positions, by id.
    pub(crate) eyes: &'a HashMap<Uuid, [f32; 3]>,
    /// `voice_chat_volume`, 0 to 3.
    pub(crate) volume: f32,
    /// `PlayerVolumeConfig`: each speaker's own multiplier, 0 to 4.
    pub(crate) player_volumes: &'a HashMap<Uuid, f32>,
}

impl Listening<'_> {
    /// `AudioChannel.writeToSpeaker`'s volume: voice chat's times the
    /// sender's own.
    fn volume_of(&self, sender: Uuid) -> f32 {
        self.volume * self.player_volumes.get(&sender).copied().unwrap_or(1.0)
    }
}

/// `AudioPacketBuffer`: hands packets on in sequence, holding a few back to
/// wait for one that arrived out of order.
#[derive(Default)]
struct Reorder {
    held: Vec<Sound>,
    last: i64,
    flushing: bool,
}

impl Reorder {
    fn new() -> Reorder {
        Reorder {
            held: Vec::new(),
            last: -1,
            flushing: false,
        }
    }

    /// The packets ready to play after `sound` arrived, in order.
    fn push(&mut self, sound: Sound) -> Vec<Sound> {
        let mut out = Vec::new();
        if sound.sequence == self.last + 1 || self.last < 0 {
            self.last = sound.sequence;
            out.push(sound);
        } else {
            if sound.opus.is_empty() {
                self.flushing = true;
            }
            self.held.push(sound);
            self.held.sort_by_key(|s| s.sequence);
        }
        while let Some(next) = self.next() {
            out.push(next);
        }
        out
    }

    fn next(&mut self) -> Option<Sound> {
        if self.flushing {
            if self.held.is_empty() {
                self.flushing = false;
                return None;
            }
            return Some(self.first());
        }
        if self.held.len() > PACKET_THRESHOLD {
            return Some(self.first());
        }
        let first = self.held.first()?;
        (first.sequence == self.last + 1 || self.last < 0).then(|| self.first())
    }

    fn first(&mut self) -> Sound {
        let sound = self.held.remove(0);
        self.last = sound.sequence;
        sound
    }

    fn clear(&mut self) {
        self.held.clear();
        self.last = -1;
    }
}

/// How a channel's packets are heard.
#[derive(Clone, Copy)]
enum Kind {
    Player { distance: f32 },
    Group,
    Location { at: [f32; 3], distance: f32 },
}

struct Channel {
    decoder: Decoder,
    reorder: Reorder,
    last_sequence: i64,
    stream: Option<crate::audio::Stream>,
    place: Option<StreamPlace>,
    volume: f32,
    heard: Instant,
}

impl Channel {
    fn new() -> Option<Channel> {
        Some(Channel {
            decoder: Decoder::new()?,
            reorder: Reorder::new(),
            last_sequence: -1,
            stream: None,
            place: None,
            volume: 1.0,
            heard: Instant::now(),
        })
    }

    /// `AudioChannel.run` for one packet out of the reorder buffer.
    fn play(&mut self, sound: &Sound, kind: Kind, listening: &Listening) {
        let from_plugin = sound.sequence < 0;
        if !from_plugin && self.last_sequence >= 0 && sound.sequence <= self.last_sequence {
            return;
        }
        if sound.opus.is_empty() {
            self.last_sequence = -1;
            self.reorder.clear();
            self.decoder.reset();
            return;
        }
        if from_plugin {
            let pcm: Vec<i16> = sound
                .opus
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            self.write(&pcm, sound, kind, listening);
            return;
        }
        let mut lost = 0;
        if self.last_sequence >= 0 {
            lost = sound.sequence - (self.last_sequence + 1);
            if lost > OUTPUT_BUFFER {
                lost = 0;
                self.decoder.reset();
            }
        }
        self.last_sequence = sound.sequence;
        for _ in 0..lost.max(0) {
            let concealed = self.decoder.decode(None);
            self.write(&concealed, sound, kind, listening);
        }
        let frame = self.decoder.decode(Some(&sound.opus));
        self.write(&frame, sound, kind, listening);
    }

    /// `AudioChannel.writeToSpeaker`: where this frame is heard, if at all.
    fn write(&mut self, pcm: &[i16], sound: &Sound, kind: Kind, listening: &Listening) {
        let place = match kind {
            Kind::Group => StreamPlace::Listener,
            Kind::Player { .. } if listening.me == Some(sound.sender) => StreamPlace::Listener,
            Kind::Player { distance } => {
                let Some(eye) = listening.eyes.get(&sound.sender).copied() else {
                    return;
                };
                if distance_between(eye, listening.listener) > distance + 1.0 {
                    return;
                }
                StreamPlace::At { at: eye, range: distance }
            }
            Kind::Location { at, distance } => {
                if distance_between(at, listening.listener) > distance + 1.0 {
                    return;
                }
                StreamPlace::At { at, range: distance }
            }
        };
        let volume = listening.volume_of(sound.sender);
        let stream = match &self.stream {
            Some(stream) => stream,
            None => {
                let Some(stream) =
                    crate::audio::open_stream(SoundCategory::Voice, volume, place, SAMPLE_RATE as u32)
                else {
                    return;
                };
                self.place = Some(place);
                self.volume = volume;
                self.stream.insert(stream)
            }
        };
        if self.place != Some(place) {
            stream.place(place);
            self.place = Some(place);
        }
        if self.volume != volume {
            stream.set_volume(volume);
            self.volume = volume;
        }
        stream.push(pcm);
    }
}

fn distance_between(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

/// Every voice channel open.
#[derive(Default)]
pub(crate) struct Playback {
    channels: HashMap<Uuid, Channel>,
}

impl Playback {
    pub(crate) fn receive(&mut self, packet: Packet, listening: &Listening) {
        let (sound, kind) = match packet {
            Packet::PlayerSound {
                sound, distance, ..
            } => (sound, Kind::Player { distance }),
            Packet::GroupSound { sound } => (sound, Kind::Group),
            Packet::LocationSound {
                sound, at, distance,
            } => (
                sound,
                Kind::Location {
                    at: at.map(|v| v as f32),
                    distance,
                },
            ),
            _ => return,
        };
        let channel = match self.channels.entry(sound.channel) {
            std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::hash_map::Entry::Vacant(slot) => match Channel::new() {
                Some(channel) => slot.insert(channel),
                None => return,
            },
        };
        channel.heard = Instant::now();
        for ready in channel.reorder.push(sound) {
            channel.play(&ready, kind, listening);
        }
    }

    /// Closes channels nobody has spoken on for half a minute.
    pub(crate) fn sweep(&mut self) {
        self.channels.retain(|_, channel| channel.heard.elapsed() < IDLE);
    }
}

const _: () = assert!(FRAME == 960);

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(sequence: i64, opus: &[u8]) -> Sound {
        Sound {
            channel: Uuid::nil(),
            sender: Uuid::nil(),
            opus: opus.to_vec(),
            sequence,
            category: None,
        }
    }

    fn order(reorder: &mut Reorder, sequence: i64) -> Vec<i64> {
        reorder
            .push(sound(sequence, &[1]))
            .iter()
            .map(|s| s.sequence)
            .collect()
    }

    #[test]
    fn a_late_packet_is_waited_for_and_played_in_order() {
        let mut r = Reorder::new();
        assert_eq!(order(&mut r, 0), [0]);
        assert_eq!(order(&mut r, 2), Vec::<i64>::new());
        assert_eq!(order(&mut r, 1), [1, 2]);
    }

    #[test]
    fn a_lost_packet_is_given_up_on_past_the_threshold() {
        let mut r = Reorder::new();
        assert_eq!(order(&mut r, 0), [0]);
        for s in 2..=4 {
            assert!(order(&mut r, s).is_empty(), "{s}");
        }
        // A fourth held packet passes the threshold of three: play on.
        assert_eq!(order(&mut r, 5), [2, 3, 4, 5]);
    }

    #[test]
    fn an_end_of_transmission_flushes_what_was_held() {
        let mut r = Reorder::new();
        assert_eq!(order(&mut r, 0), [0]);
        assert!(order(&mut r, 2).is_empty());
        let out: Vec<i64> = r.push(sound(3, &[])).iter().map(|s| s.sequence).collect();
        assert_eq!(out, [2, 3]);
    }
}
