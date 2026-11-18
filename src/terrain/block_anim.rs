//! Animated block textures.
//!
//! Every frame is its own atlas tile. Meshes only ever name a texture's first
//! tile, and a small `tile -> (frame, next frame, blend)` table, rewritten once
//! a tick, tells the terrain shader which tile to sample instead. Nothing in
//! the atlas moves after it is built.

use std::sync::{Arc, RwLock};

pub(crate) const TICKS_PER_SECOND: f64 = 20.0;

pub(crate) struct BlockAnimation {
    /// The tile meshes reference.
    pub base: u16,
    /// Atlas tile and how many ticks it shows, in play order.
    pub frames: Box<[(u16, u32)]>,
    pub interpolate: bool,
    /// The native renderer's blend tile, for interpolated strips.
    pub spare: Option<u16>,
}

impl BlockAnimation {
    fn period(&self) -> u64 {
        self.frames
            .iter()
            .map(|&(_, t)| u64::from(t))
            .sum::<u64>()
            .max(1)
    }

    /// `(frame tile, next frame tile, blend 0..=u16::MAX)` at `tick`.
    pub(crate) fn at(&self, tick: u64) -> (u16, u16, u16) {
        let mut into = tick % self.period();
        for (i, &(tile, time)) in self.frames.iter().enumerate() {
            let time = u64::from(time.max(1));
            if into < time {
                let next = self.frames[(i + 1) % self.frames.len()].0;
                let blend = if self.interpolate {
                    (into * u64::from(u16::MAX) / time) as u16
                } else {
                    0
                };
                return (tile, next, blend);
            }
            into -= time;
        }
        (self.frames[0].0, self.frames[0].0, 0)
    }
}

pub(crate) struct AnimationSet {
    pub(crate) generation: u64,
    pub(crate) tile_count: u32,
    pub(crate) animations: Vec<BlockAnimation>,
}

static SET: RwLock<Option<Arc<AnimationSet>>> = RwLock::new(None);

/// Called by the atlas builder; the table follows on the next frame.
pub(crate) fn publish(tile_count: u32, animations: Vec<BlockAnimation>) {
    if let Ok(mut slot) = SET.write() {
        let generation = slot.as_ref().map_or(1, |s| s.generation + 1);
        *slot = Some(Arc::new(AnimationSet {
            generation,
            tile_count,
            animations,
        }));
    }
}

/// The game tick now, on the clock the block animations run by.
impl AnimationSet {
    pub(crate) fn tile_count(&self) -> u32 {
        self.tile_count
    }

    /// The table for `tick`, as the native renderer reads it: per tile, the
    /// frame tile and the next (16 bits each), then the blend.
    /// Words 0 and 1: the atlas's rows of tiles (`rows`), the tile count;
    /// then per tile its entry; then the blend list: a count, and per
    /// interpolated strip (frame, next frame, blend, spare tile). Those
    /// strips name their spare tile, which the GPU fills with the blend.
    pub(crate) fn table(&self, tick: u64, rows: u32) -> Vec<u32> {
        let mut out: Vec<u32> = vec![rows, self.tile_count];
        out.extend((0..self.tile_count).flat_map(|t| [t | (t << 16), 0]));
        let mut blends = Vec::new();
        for animation in &self.animations {
            let (a, b, blend) = animation.at(tick);
            let at = 2 + usize::from(animation.base) * 2;
            if at + 1 >= out.len() {
                continue;
            }
            if let Some(spare) = animation.spare {
                out[at] = u32::from(spare) | (u32::from(spare) << 16);
                out[at + 1] = 0;
                blends.extend([
                    u32::from(a),
                    u32::from(b),
                    u32::from(blend),
                    u32::from(spare),
                ]);
            } else {
                out[at] = u32::from(a) | (u32::from(b) << 16);
                out[at + 1] = u32::from(blend);
            }
        }
        out.push((blends.len() / 4) as u32);
        out.extend(blends);
        out
    }
}

pub(crate) fn tick_now() -> u64 {
    static START: std::sync::OnceLock<crate::platform::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(crate::platform::time::Instant::now);
    (start.elapsed().as_secs_f64() * TICKS_PER_SECOND) as u64
}

pub(crate) fn current() -> Option<Arc<AnimationSet>> {
    SET.read().ok()?.clone()
}

/// `animation` from a texture's `.mcmeta`, resolved against `frame_count`
/// frames in the strip: `(frame index, ticks)` in play order.
pub(crate) struct Meta {
    pub frames: Vec<(u32, u32)>,
    pub interpolate: bool,
}

pub(crate) fn read_meta(png: &std::path::Path, frame_count: u32) -> Option<Meta> {
    let text = crate::platform::assets::read_to_string(png.with_extension("png.mcmeta"))?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let anim = json.get("animation")?;
    let default_time = anim
        .get("frametime")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1)
        .max(1) as u32;
    let interpolate = anim
        .get("interpolate")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let frames: Vec<(u32, u32)> = match anim.get("frames").and_then(serde_json::Value::as_array) {
        Some(list) => list
            .iter()
            .filter_map(|f| match f {
                serde_json::Value::Number(n) => Some((n.as_u64()? as u32, default_time)),
                serde_json::Value::Object(o) => Some((
                    o.get("index")?.as_u64()? as u32,
                    o.get("time")
                        .and_then(serde_json::Value::as_u64)
                        .map_or(default_time, |t| t.max(1) as u32),
                )),
                _ => None,
            })
            .filter(|&(index, _)| index < frame_count)
            .collect(),
        None => (0..frame_count).map(|i| (i, default_time)).collect(),
    };
    (frames.len() > 1).then_some(Meta {
        frames,
        interpolate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(frames: &[(u16, u32)], interpolate: bool) -> BlockAnimation {
        BlockAnimation {
            base: frames[0].0,
            frames: frames.into(),
            interpolate,
            spare: None,
        }
    }

    #[test]
    fn frames_play_for_their_time_and_loop() {
        let a = anim(&[(10, 2), (11, 1), (12, 3)], false);
        let shown: Vec<u16> = (0..7).map(|t| a.at(t).0).collect();
        assert_eq!(shown, [10, 10, 11, 12, 12, 12, 10]);
    }

    #[test]
    fn interpolation_blends_towards_the_next_frame() {
        let a = anim(&[(1, 4), (2, 4)], true);
        assert_eq!(a.at(0), (1, 2, 0));
        assert_eq!(a.at(2), (1, 2, u16::MAX / 2));
        assert_eq!(a.at(4).0, 2);
        assert_eq!(a.at(7).1, 1, "the last frame blends back to the first");
    }
}
