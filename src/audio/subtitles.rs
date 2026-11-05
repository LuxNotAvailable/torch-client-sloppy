//! Vanilla's `SubtitleOverlay`: each sound with a subtitle notes where and
//! when it played, and the HUD lists the ones still heard, newest brightest,
//! with an arrow towards a sound out of view.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::platform::time::Instant;

/// `showSubtitles`: sounds are noted only while subtitles show, as vanilla's
/// overlay only listens then.
static SHOWN: AtomicBool = AtomicBool::new(false);

/// `SubtitleOverlay.DISPLAY_TIME`.
const DISPLAY_MS: f32 = 3000.0;
/// A looping sound stays at full brightness this long after each refresh.
const LOOPING_FULL_MS: f32 = 500.0;
/// `SoundEngine.tick` re-notifies a looping sound every 10 ticks.
const LOOP_REFRESH_MS: u128 = 500;

struct PlayedAt {
    at: Option<[f32; 3]>,
    time: Instant,
}

struct Subtitle {
    key: Arc<str>,
    /// How far the sound carries; infinite at the listener.
    range: f32,
    played: Vec<PlayedAt>,
    looping: bool,
}

/// A looping sound still playing, re-noted while it lasts.
struct Loop {
    voice: u64,
    key: Arc<str>,
    at: Option<[f32; 3]>,
    range: f32,
    noted: Instant,
}

static SUBTITLES: Mutex<Vec<Subtitle>> = Mutex::new(Vec::new());
static LOOPS: Mutex<Vec<Loop>> = Mutex::new(Vec::new());

pub fn set_shown(shown: bool) {
    if SHOWN.swap(shown, Ordering::Relaxed) && !shown {
        SUBTITLES.lock().unwrap().clear();
        LOOPS.lock().unwrap().clear();
    }
}

pub(super) fn shown() -> bool {
    SHOWN.load(Ordering::Relaxed)
}

/// `SubtitleOverlay.onPlaySound`: a subtitle already listed is refreshed at
/// the new place.
pub(super) fn notify(key: &Arc<str>, at: Option<[f32; 3]>, range: f32, looping: bool) {
    let now = Instant::now();
    let mut subtitles = SUBTITLES.lock().unwrap();
    if let Some(subtitle) = subtitles.iter_mut().find(|s| s.key == *key) {
        subtitle.played.retain(|p| p.at != at);
        subtitle.played.push(PlayedAt { at, time: now });
        return;
    }
    subtitles.push(Subtitle {
        key: key.clone(),
        range,
        played: vec![PlayedAt { at, time: now }],
        looping,
    });
}

pub(super) fn track_loop(voice: u64, key: Arc<str>, at: Option<[f32; 3]>, range: f32) {
    LOOPS.lock().unwrap().push(Loop {
        voice,
        key,
        at,
        range,
        noted: Instant::now(),
    });
}

pub(super) fn move_loop(voice: u64, at: [f32; 3]) {
    if let Some(l) = LOOPS.lock().unwrap().iter_mut().find(|l| l.voice == voice) {
        l.at = Some(at);
    }
}

/// Which side a sound out of view is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    None,
    Left,
    Right,
}

pub struct Line {
    pub key: Arc<str>,
    /// Text brightness, 255 fresh fading to 75.
    pub brightness: u8,
    pub side: Side,
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

impl Subtitle {
    /// `getBestSubtitleCandidate`: the newest play within range, the nearest
    /// among equals; a lone play regardless.
    fn best(&self, camera: [f32; 3]) -> Option<&PlayedAt> {
        if self.played.len() == 1 {
            return self.played.first();
        }
        let within = |p: &&PlayedAt| p.at.is_none_or(|at| distance(at, camera) < self.range);
        self.played.iter().filter(within).max_by(|a, b| {
            a.time.cmp(&b.time).then_with(|| {
                let d = |p: &PlayedAt| p.at.map_or(0.0, |at| distance(at, camera));
                d(a).total_cmp(&d(b))
            })
        })
    }

    fn brightness(&self, played: &PlayedAt, now: Instant) -> u8 {
        let full = if self.looping { LOOPING_FULL_MS } else { 0.0 };
        let age = now.duration_since(played.time).as_secs_f32() * 1000.0;
        let t = ((age - full).max(0.0) / (DISPLAY_MS - full)).clamp(0.0, 1.0);
        (255.0 + (75.0 - 255.0) * t).floor() as u8
    }
}

/// Which side of the view a sound at `at` is on: none in view (within 60°
/// of `forward`) or at the listener.
fn side(at: Option<[f32; 3]>, camera: [f32; 3], forward: [f32; 3], right: [f32; 3]) -> Side {
    let Some(at) = at else {
        return Side::None;
    };
    let d = distance(at, camera).max(1e-6);
    let delta: [f32; 3] = std::array::from_fn(|i| (at[i] - camera[i]) / d);
    let dot = |v: [f32; 3]| (0..3).map(|i| v[i] * delta[i]).sum::<f32>();
    let rightness = dot(right);
    if dot(forward) > 0.5 || rightness == 0.0 {
        Side::None
    } else if rightness > 0.0 {
        Side::Right
    } else {
        Side::Left
    }
}

/// The subtitles to show, oldest first as vanilla lists them from the bottom
/// row up, heard from `camera` looking along `forward`.
pub fn lines(camera: [f32; 3], forward: [f32; 3], right: [f32; 3]) -> Vec<Line> {
    if !shown() {
        return Vec::new();
    }
    let now = Instant::now();
    LOOPS.lock().unwrap().retain_mut(|l| {
        if !super::playing(l.voice) {
            return false;
        }
        if now.duration_since(l.noted).as_millis() >= LOOP_REFRESH_MS {
            l.noted = now;
            notify(&l.key, l.at, l.range, true);
        }
        true
    });
    let mut subtitles = SUBTITLES.lock().unwrap();
    subtitles.retain_mut(|s| {
        s.played
            .retain(|p| now.duration_since(p.time).as_secs_f32() * 1000.0 <= DISPLAY_MS);
        !s.played.is_empty()
    });
    subtitles
        .iter()
        .filter_map(|s| {
            let played = s.best(camera)?;
            if let Some(at) = played.at
                && distance(at, camera) >= s.range
            {
                return None;
            }
            Some(Line {
                key: s.key.clone(),
                brightness: s.brightness(played, now),
                side: side(played.at, camera, forward, right),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subtitle_fades_from_full_to_dim_over_the_display_time() {
        let s = Subtitle {
            key: Arc::from("subtitles.test"),
            range: 16.0,
            played: vec![PlayedAt {
                at: Some([3.0, 0.0, 0.0]),
                time: Instant::now(),
            }],
            looping: false,
        };
        let best = s.best([0.0; 3]).unwrap();
        assert_eq!(s.brightness(best, best.time), 255);
        let later = best.time + std::time::Duration::from_secs(3);
        assert_eq!(s.brightness(best, later), 75);
    }

    #[test]
    fn a_sound_out_of_view_points_to_its_side() {
        let (camera, forward, right) = ([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
        let at = |p| side(Some(p), camera, forward, right);
        assert_eq!(at([0.0, 0.0, -5.0]), Side::None, "ahead");
        assert_eq!(at([5.0, 0.0, 1.0]), Side::Right);
        assert_eq!(at([-5.0, 0.0, 1.0]), Side::Left);
        assert_eq!(side(None, camera, forward, right), Side::None);
    }
}
