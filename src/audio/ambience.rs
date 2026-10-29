//! The sounds the client makes for itself each game tick, as vanilla's
//! `AmbientSoundHandler`s and tickable sound instances do: the biome's loop,
//! additions and mood (`BiomeAmbientSoundsHandler`), the underwater loop and
//! its additions (`UnderwaterAmbientSoundHandler`,
//! `UnderLiquidAmbientSoundInstance`), rain (`ClientLevel.tickRainParticles`),
//! the wind of elytra flight (`ElytraOnPlayerSoundInstance`), and rolling
//! minecarts (`MinecartSoundInstance`).

use std::collections::HashMap;

use super::environment::{self, AmbientSounds};
use super::{SoundCategory, StreamPlace};
use crate::util::javarandom::JavaRandom;

/// What the game tick knows that these sounds need.
pub struct Surroundings<'a> {
    /// The dimension type's id (`overworld`).
    pub dimension: &'a str,
    pub biome: Option<&'a str>,
    pub feet: [f64; 3],
    pub eye: [f64; 3],
    /// `LocalPlayer.isUnderWater`: the eye in water.
    pub underwater: bool,
    pub fall_flying: bool,
    /// This tick's movement.
    pub velocity: [f64; 3],
    /// Block and sky light at a block.
    pub light: &'a dyn Fn([i32; 3]) -> (u8, u8),
    /// Where rain fell this tick near the camera, if anywhere, and whether
    /// that is above a roof over the camera (`weather.rain.above`).
    pub rain: Option<([f32; 3], bool)>,
    /// Every minecart seen: its id and where it is.
    pub minecarts: &'a [(i32, [f64; 3])],
}

/// `LoopSoundInstance` and `UnderLiquidAmbientSoundInstance`: a loop that
/// fades over 40 ticks.
struct Fading {
    voice: u64,
    fade: i32,
    direction: i32,
}

const FADE: i32 = 40;

struct Elytra {
    voice: u64,
    time: i32,
}

struct Minecart {
    voice: u64,
    last: [f64; 3],
    pitch: f32,
}

pub struct Ambience {
    rng: JavaRandom,
    loops: HashMap<String, Fading>,
    previous_loop: Option<String>,
    moodiness: f32,
    was_underwater: bool,
    underwater_loop: Option<Fading>,
    underwater_delay: i32,
    rain_time: i32,
    was_flying: bool,
    elytra: Option<Elytra>,
    minecarts: HashMap<i32, Minecart>,
}

impl Default for Ambience {
    fn default() -> Ambience {
        Ambience {
            rng: JavaRandom::new(super::roll_seed()),
            loops: HashMap::new(),
            previous_loop: None,
            moodiness: 0.0,
            was_underwater: false,
            underwater_loop: None,
            underwater_delay: 0,
            rain_time: 0,
            was_flying: false,
            elytra: None,
            minecarts: HashMap::new(),
        }
    }
}

fn at(p: [f64; 3]) -> [f32; 3] {
    p.map(|v| v as f32)
}

impl Ambience {
    pub fn tick(&mut self, s: &Surroundings) {
        let ambient = environment::ambient(s.dimension, s.biome);
        self.biome(s, &ambient);
        self.underwater(s);
        self.rain(s);
        self.elytra(s);
        self.minecarts(s);
    }

    /// Stops everything this keeps playing (leaving the world).
    pub fn stop(&mut self) {
        for fading in self
            .loops
            .drain()
            .map(|(_, f)| f)
            .chain(self.underwater_loop.take())
        {
            super::stop_voice(fading.voice);
        }
        if let Some(elytra) = self.elytra.take() {
            super::stop_voice(elytra.voice);
        }
        for (_, cart) in self.minecarts.drain() {
            super::stop_voice(cart.voice);
        }
        self.previous_loop = None;
    }

    /// `BiomeAmbientSoundsHandler.tick`.
    fn biome(&mut self, s: &Surroundings, ambient: &AmbientSounds) {
        self.loops.retain(|_, fading| {
            if fading.fade < 0 {
                super::stop_voice(fading.voice);
                return false;
            }
            fading.fade += fading.direction;
            let volume = (fading.fade as f32 / FADE as f32).clamp(0.0, 1.0);
            super::tune(fading.voice, volume, 1.0);
            true
        });
        if ambient.looping != self.previous_loop {
            self.previous_loop = ambient.looping.clone();
            for fading in self.loops.values_mut() {
                fading.fade = fading.fade.min(FADE);
                fading.direction = -1;
            }
            if let Some(event) = &ambient.looping {
                if !self.loops.contains_key(event)
                    && let Some(voice) = super::start(
                        event,
                        SoundCategory::Ambient,
                        StreamPlace::Listener,
                        0.0,
                        1.0,
                        true,
                    )
                {
                    self.loops.insert(
                        event.clone(),
                        Fading {
                            voice,
                            fade: 0,
                            direction: 1,
                        },
                    );
                }
                if let Some(fading) = self.loops.get_mut(event) {
                    fading.fade = fading.fade.max(0);
                    fading.direction = 1;
                }
            }
        }
        for addition in &ambient.additions {
            if self.rng.next_f64() < addition.tick_chance {
                super::play(&addition.event, SoundCategory::Ambient, 1.0, 1.0);
            }
        }
        let Some(mood) = &ambient.mood else { return };
        let extent = mood.block_search_extent;
        let span = (extent * 2 + 1).max(1) as u32;
        let mut offset = || self.rng.next_int(span) as i32 - extent;
        let sample = [
            (s.feet[0] + f64::from(offset())).floor() as i32,
            (s.eye[1] + f64::from(offset())).floor() as i32,
            (s.feet[2] + f64::from(offset())).floor() as i32,
        ];
        let (block, sky) = (s.light)(sample);
        if sky > 0 {
            self.moodiness -= f32::from(sky) / 15.0 * 0.001;
        } else {
            self.moodiness -= (f32::from(block) - 1.0) / mood.tick_delay.max(1) as f32;
        }
        if self.moodiness >= 1.0 {
            let toward = [
                f64::from(sample[0]) + 0.5 - s.feet[0],
                f64::from(sample[1]) + 0.5 - s.eye[1],
                f64::from(sample[2]) + 0.5 - s.feet[2],
            ];
            let distance = toward.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-6);
            let reach = distance + mood.offset;
            let point = [
                s.feet[0] + toward[0] / distance * reach,
                s.eye[1] + toward[1] / distance * reach,
                s.feet[2] + toward[2] / distance * reach,
            ];
            super::play_at(&mood.event, SoundCategory::Ambient, at(point), 1.0, 1.0);
            self.moodiness = 0.0;
        } else {
            self.moodiness = self.moodiness.max(0.0);
        }
    }

    /// `LocalPlayer.updateIsUnderwater`, `UnderwaterAmbientSoundHandler`
    /// and `UnderLiquidAmbientSoundInstance`.
    fn underwater(&mut self, s: &Surroundings) {
        if s.underwater && !self.was_underwater {
            super::play_at(
                "ambient.underwater.enter",
                SoundCategory::Ambient,
                at(s.feet),
                1.0,
                1.0,
            );
            if self.underwater_loop.is_none()
                && let Some(voice) = super::start(
                    "ambient.underwater.loop",
                    SoundCategory::Ambient,
                    StreamPlace::Listener,
                    0.0,
                    1.0,
                    true,
                )
            {
                self.underwater_loop = Some(Fading {
                    voice,
                    fade: 0,
                    direction: 1,
                });
            }
        }
        if !s.underwater && self.was_underwater {
            super::play_at(
                "ambient.underwater.exit",
                SoundCategory::Ambient,
                at(s.feet),
                1.0,
                1.0,
            );
        }
        self.was_underwater = s.underwater;
        if let Some(fading) = &mut self.underwater_loop {
            if fading.fade < 0 {
                super::stop_voice(fading.voice);
                self.underwater_loop = None;
            } else {
                fading.fade = if s.underwater {
                    fading.fade + 1
                } else {
                    fading.fade - 2
                }
                .min(FADE);
                let volume = (fading.fade as f32 / FADE as f32).clamp(0.0, 1.0);
                super::tune(fading.voice, volume, 1.0);
            }
        }
        self.underwater_delay -= 1;
        if self.underwater_delay <= 0 && s.underwater {
            let roll = self.rng.next_f32();
            let event = if roll < 1.0e-4 {
                Some("ambient.underwater.loop.additions.ultra_rare")
            } else if roll < 1.0e-3 {
                Some("ambient.underwater.loop.additions.rare")
            } else if roll < 1.0e-2 {
                Some("ambient.underwater.loop.additions")
            } else {
                None
            };
            if let Some(event) = event {
                self.underwater_delay = 0;
                super::play(event, SoundCategory::Ambient, 1.0, 1.0);
            }
        }
    }

    /// The rain sound in `ClientLevel.tickRainParticles`.
    fn rain(&mut self, s: &Surroundings) {
        let Some((spot, above)) = s.rain else { return };
        let roll = self.rng.next_int(3) as i32;
        let due = roll < self.rain_time;
        self.rain_time += 1;
        if due {
            self.rain_time = 0;
            if above {
                super::play_at("weather.rain.above", SoundCategory::Weather, spot, 0.1, 0.5);
            } else {
                super::play_at("weather.rain", SoundCategory::Weather, spot, 0.2, 1.0);
            }
        }
    }

    /// `ElytraOnPlayerSoundInstance`, started when flight starts.
    fn elytra(&mut self, s: &Surroundings) {
        if s.fall_flying && !self.was_flying && self.elytra.is_none() {
            self.elytra = super::start(
                "item.elytra.flying",
                SoundCategory::Players,
                StreamPlace::At {
                    at: at(s.feet),
                    range: super::defs::DEFAULT_ATTENUATION,
                },
                0.1,
                1.0,
                true,
            )
            .map(|voice| Elytra { voice, time: 0 });
        }
        self.was_flying = s.fall_flying;
        let Some(elytra) = &mut self.elytra else {
            return;
        };
        elytra.time += 1;
        if elytra.time <= 20 || s.fall_flying {
            super::move_voice(elytra.voice, at(s.feet));
            let speed = s.velocity.iter().map(|v| v * v).sum::<f64>() as f32;
            let mut volume = if speed >= 1.0e-7 {
                (speed / 4.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if elytra.time < 20 {
                volume = 0.0;
            } else if elytra.time < 40 {
                volume *= (elytra.time - 20) as f32 / 20.0;
            }
            let pitch = if volume > 0.8 {
                1.0 + (volume - 0.8)
            } else {
                1.0
            };
            super::tune(elytra.voice, volume, pitch);
        } else {
            super::stop_voice(elytra.voice);
            self.elytra = None;
        }
    }

    /// `MinecartSoundInstance`, one per minecart, from when it is seen until
    /// it goes.
    fn minecarts(&mut self, s: &Surroundings) {
        let seen: HashMap<i32, [f64; 3]> = s.minecarts.iter().copied().collect();
        self.minecarts.retain(|id, cart| {
            let keep = seen.contains_key(id);
            if !keep {
                super::stop_voice(cart.voice);
            }
            keep
        });
        for (&id, &pos) in &seen {
            let cart = match self.minecarts.entry(id) {
                std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    let Some(voice) = super::start(
                        "entity.minecart.riding",
                        SoundCategory::Neutral,
                        StreamPlace::At {
                            at: at(pos),
                            range: super::defs::DEFAULT_ATTENUATION,
                        },
                        0.0,
                        0.0,
                        true,
                    ) else {
                        continue;
                    };
                    slot.insert(Minecart {
                        voice,
                        last: pos,
                        pitch: 0.0,
                    })
                }
            };
            let speed =
                ((pos[0] - cart.last[0]).powi(2) + (pos[2] - cart.last[2]).powi(2)).sqrt() as f32;
            cart.last = pos;
            let volume = if speed >= 0.01 {
                cart.pitch = (cart.pitch + 0.0025).clamp(0.0, 1.0);
                0.7 * speed.clamp(0.0, 0.5)
            } else {
                cart.pitch = 0.0;
                0.0
            };
            super::move_voice(cart.voice, at(pos));
            super::tune(cart.voice, volume, cart.pitch);
        }
    }
}
