//! `MusicManager`: which music the situation calls for, how long to wait
//! between tracks, and when a new situation cuts the current track short.
//! Ticked twenty times a second, in menus too.

use super::environment::{self, Music};
use super::{SoundCategory, StreamPlace};
use crate::util::javarandom::JavaRandom;

/// Where the player is, for `Minecraft.getSituationalMusic`.
#[derive(Clone, Debug, PartialEq)]
pub enum Situation {
    /// The title screen and the menus around it (`Musics.MENU`).
    Menu,
    InGame {
        dimension: String,
        biome: Option<String>,
        /// `abilities.instabuild && abilities.mayfly`.
        creative: bool,
        underwater: bool,
        /// In the End with a boss bar asking for music (`Musics.END_BOSS`).
        end_boss: bool,
    },
}

/// `MusicFrequency.DEFAULT`: at most twenty minutes between tracks.
const MAX_FREQUENCY: i32 = 20 * 1200;
/// `MusicManager.STARTING_DELAY`.
const STARTING_DELAY: i32 = 100;

fn menu() -> Music {
    Music {
        event: "music.menu".into(),
        min_delay: 20,
        max_delay: 600,
        replace_current: true,
    }
}

fn end_boss() -> Music {
    Music {
        event: "music.dragon".into(),
        min_delay: 0,
        max_delay: 0,
        replace_current: true,
    }
}

impl Situation {
    pub fn music(&self) -> Option<Music> {
        match self {
            Situation::Menu => Some(menu()),
            Situation::InGame { end_boss: true, dimension, .. } if dimension.ends_with("the_end") => {
                Some(end_boss())
            }
            Situation::InGame {
                dimension,
                biome,
                creative,
                underwater,
                ..
            } => environment::music(dimension, biome.as_deref())
                .select(*creative, *underwater)
                .cloned(),
        }
    }
}

/// What starts and checks the tracks: the mixer, or a test's stand-in.
pub trait Player {
    fn start(&mut self, event: &str) -> Option<u64>;
    fn stop(&mut self, voice: u64);
    fn playing(&self, voice: u64) -> bool;
}

/// The mixer.
pub struct Mixer;

impl Player for Mixer {
    fn start(&mut self, event: &str) -> Option<u64> {
        super::start(event, SoundCategory::Music, StreamPlace::Listener, 1.0, 1.0, false)
    }

    fn stop(&mut self, voice: u64) {
        super::stop_voice(voice);
    }

    fn playing(&self, voice: u64) -> bool {
        super::playing(voice)
    }
}

pub struct MusicManager {
    rng: JavaRandom,
    current: Option<(u64, String)>,
    next_delay: i32,
}

impl Default for MusicManager {
    fn default() -> MusicManager {
        MusicManager {
            rng: JavaRandom::new(super::roll_seed()),
            current: None,
            next_delay: STARTING_DELAY,
        }
    }
}

impl MusicManager {
    /// `Mth.nextInt`: `low..=high`, or `low` if the range is empty.
    fn between(&mut self, low: i32, high: i32) -> i32 {
        if low >= high {
            low
        } else {
            low + self.rng.next_int((high - low + 1) as u32) as i32
        }
    }

    /// `MusicFrequency.getNextSongDelay` for DEFAULT.
    fn song_delay(&mut self, music: &Music) -> i32 {
        let low = music.min_delay.min(MAX_FREQUENCY);
        let high = music.max_delay.min(MAX_FREQUENCY);
        self.between(low, high)
    }

    /// `MusicManager.tick`.
    pub fn tick(&mut self, music: Option<&Music>, player: &mut dyn Player) {
        let Some(music) = music else {
            self.next_delay = self.next_delay.max(STARTING_DELAY);
            return;
        };
        if let Some((voice, event)) = &self.current {
            let voice = *voice;
            if music.replace_current && *event != music.event {
                player.stop(voice);
                self.next_delay = self.between(0, music.min_delay / 2);
            }
            if !player.playing(voice) {
                self.current = None;
                let delay = self.song_delay(music);
                self.next_delay = self.next_delay.min(delay);
            }
        }
        self.next_delay = self.next_delay.min(music.max_delay);
        if self.current.is_none() {
            self.next_delay -= 1;
            if self.next_delay <= 0 {
                self.current = player.start(&music.event).map(|voice| (voice, music.event.clone()));
                self.next_delay = i32::MAX;
            }
        }
    }

    /// `MusicManager.stopPlaying`: the track ends, and the next waits a
    /// song delay for `music` (the most, with none) plus the starting delay.
    pub fn stop(&mut self, music: Option<&Music>, player: &mut dyn Player) {
        if let Some((voice, _)) = self.current.take() {
            player.stop(voice);
        }
        let delay = match music {
            Some(music) => self.song_delay(music),
            None => MAX_FREQUENCY,
        };
        self.next_delay = delay + STARTING_DELAY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        started: Vec<String>,
        live: std::collections::HashSet<u64>,
        next: u64,
    }

    impl Player for Fake {
        fn start(&mut self, event: &str) -> Option<u64> {
            self.next += 1;
            self.started.push(event.to_owned());
            self.live.insert(self.next);
            Some(self.next)
        }

        fn stop(&mut self, voice: u64) {
            self.live.remove(&voice);
        }

        fn playing(&self, voice: u64) -> bool {
            self.live.contains(&voice)
        }
    }

    fn game() -> Music {
        Music {
            event: "music.game".into(),
            min_delay: 12000,
            max_delay: 24000,
            replace_current: false,
        }
    }

    #[test]
    fn the_first_track_waits_out_the_starting_delay() {
        let mut manager = MusicManager::default();
        let mut fake = Fake::default();
        let music = game();
        for _ in 0..99 {
            manager.tick(Some(&music), &mut fake);
        }
        assert!(fake.started.is_empty());
        manager.tick(Some(&music), &mut fake);
        assert_eq!(fake.started, ["music.game"]);
    }

    #[test]
    fn a_finished_track_is_followed_after_the_situations_delay() {
        let mut manager = MusicManager::default();
        let mut fake = Fake::default();
        let music = game();
        for _ in 0..100 {
            manager.tick(Some(&music), &mut fake);
        }
        fake.live.clear();
        manager.tick(Some(&music), &mut fake);
        assert!((12000..=24000).contains(&(manager.next_delay + 1)), "{}", manager.next_delay);
        assert_eq!(fake.started.len(), 1);
    }

    #[test]
    fn menu_music_replaces_game_music_at_once() {
        let mut manager = MusicManager::default();
        let mut fake = Fake::default();
        for _ in 0..100 {
            manager.tick(Some(&game()), &mut fake);
        }
        let menu = menu();
        // Replaced: stopped, then a delay of at most min_delay / 2 = 10.
        for _ in 0..12 {
            manager.tick(Some(&menu), &mut fake);
        }
        assert_eq!(fake.started, ["music.game", "music.menu"]);
    }
}
