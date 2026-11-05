//! The local player's footstep, swim and splash sounds: vanilla's client
//! runs `Entity.move` for its own player and plays them itself
//! (`applyMovementEmissionAndPlaySound`, `Player.playStepSound`,
//! `doWaterSplashEffect`); the server plays them to everyone else.

use azalea::block::BlockState;
use azalea_core::position::BlockPos;

use crate::audio::SoundCategory;

/// What the tick knows about the player's movement.
pub(crate) struct Moved<'a> {
    pub pos: [f64; 3],
    /// Where the player was at the start of the tick.
    pub from: [f64; 3],
    /// `getDeltaMovement`, for swim and splash volumes.
    pub velocity: [f64; 3],
    pub on_ground: bool,
    pub in_water: bool,
    /// The swimming pose (`isSwimming`).
    pub swimming: bool,
    pub crouching: bool,
    pub flying: bool,
    pub passenger: bool,
    pub climbing: bool,
    pub block_at: &'a dyn Fn(BlockPos) -> Option<BlockState>,
}

/// The emission state vanilla keeps on the entity.
#[derive(Default)]
pub(crate) struct Steps {
    move_dist: f32,
    next_step: f32,
    was_in_water: Option<bool>,
    tick: u32,
    crystal_intensity: f32,
    last_crystal_tick: u32,
}

fn id(state: BlockState) -> &'static str {
    crate::util::block_model::block_id(state)
}

fn random_spread() -> f32 {
    crate::audio::random_unit() - crate::audio::random_unit()
}

impl Steps {
    pub(crate) fn tick(&mut self, m: &Moved) {
        self.tick += 1;
        let at = [m.pos[0] as f32, m.pos[1] as f32, m.pos[2] as f32];
        let play = |event: &str, volume: f32, pitch: f32| {
            crate::audio::play_at(event, SoundCategory::Players, at, volume, pitch);
        };
        let [vx, vy, vz] = m.velocity;
        // `waterSwimSound` and `doWaterSplashEffect`'s speed.
        let speed =
            |scale: f32| ((vx * vx * 0.2 + vy * vy + vz * vz * 0.2).sqrt() as f32 * scale).min(1.0);

        // `updateFluidInteraction`: entering water splashes (not on the
        // first tick).
        if m.in_water && self.was_in_water == Some(false) {
            let s = speed(0.2);
            let event = if s < 0.25 {
                "entity.player.splash"
            } else {
                "entity.player.splash.high_speed"
            };
            play(event, s, 1.0 + random_spread() * 0.4);
        }
        self.was_in_water = Some(m.in_water);

        // `Player.getMovementEmission`.
        if m.flying || (m.on_ground && m.crouching) || m.passenger {
            return;
        }
        let d = [
            m.pos[0] - m.from[0],
            m.pos[1] - m.from[1],
            m.pos[2] - m.from[2],
        ];
        let moved = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * 0.6) as f32;
        let horizontal = ((d[0] * d[0] + d[2] * d[2]).sqrt() * 0.6) as f32;
        let cell = |y: f64| {
            BlockPos::new(
                m.pos[0].floor() as i32,
                y.floor() as i32,
                m.pos[2].floor() as i32,
            )
        };
        // `getOnPos` and `getOnPosLegacy`.
        let supporting = cell(m.pos[1] - 1.0e-5);
        let effect = cell(m.pos[1] - 0.2);
        let block = |p: BlockPos| (m.block_at)(p).unwrap_or(BlockState::AIR);
        let supporting_state = block(supporting);
        self.move_dist += if m.climbing { moved } else { horizontal };
        if self.move_dist <= self.next_step || supporting_state.is_air() {
            return;
        }
        // `vibrationAndSoundEffectsFromBlock` on the effect block.
        let effect_state = block(effect);
        let stepping = (m.on_ground || m.climbing || (m.crouching && d[1] == 0.0)) && !m.swimming;
        if !effect_state.is_air() && stepping {
            self.step(effect, effect_state, m, &play);
            self.next_step = self.move_dist.trunc() + 1.0;
        } else if supporting != effect && !supporting_state.is_air() && stepping {
            // The supporting block emits events but no sound.
            self.next_step = self.move_dist.trunc() + 1.0;
        } else if m.in_water {
            self.next_step = self.move_dist.trunc() + 1.0;
            play(
                "entity.player.swim",
                speed(0.35),
                1.0 + random_spread() * 0.4,
            );
        }
    }

    /// `walkingStepSound` with `Player.playStepSound`.
    fn step(&mut self, pos: BlockPos, state: BlockState, m: &Moved, play: &dyn Fn(&str, f32, f32)) {
        let group = |s: BlockState| crate::util::block_model::sound_group(id(s));
        let normal = |s: BlockState| {
            let g = group(s);
            play(g.step_event, g.volume * 0.15, g.pitch);
        };
        let muffled = |s: BlockState| {
            let g = group(s);
            play(g.step_event, g.volume * 0.05, g.pitch * 0.8);
        };
        if m.in_water {
            let [vx, vy, vz] = m.velocity;
            let s = ((vx * vx * 0.2 + vy * vy + vz * vz * 0.2).sqrt() as f32 * 0.35).min(1.0);
            play("entity.player.swim", s, 1.0 + random_spread() * 0.4);
            muffled(state);
        } else {
            let above = pos + BlockPos::new(0, 1, 0);
            let above_state = (m.block_at)(above).unwrap_or(BlockState::AIR);
            let tagged = |tag| crate::blocks::tags::has(above_state, tag);
            if tagged("combination_step_sound_blocks") {
                normal(above_state);
                muffled(state);
            } else if tagged("inside_step_sound_blocks") {
                normal(above_state);
            } else {
                normal(state);
            }
        }
        // `shouldPlayAmethystStepSound`.
        if crate::blocks::tags::has(state, "crystal_sound_blocks")
            && self.tick >= self.last_crystal_tick + 20
        {
            let since = (self.tick - self.last_crystal_tick) as f32;
            self.crystal_intensity *= 0.997f32.powf(since);
            self.crystal_intensity = (self.crystal_intensity + 0.07).min(1.0);
            let pitch = 0.5 + self.crystal_intensity * crate::audio::random_unit() * 1.2;
            let volume = 0.1 + self.crystal_intensity * 1.2;
            play("block.amethyst_block.chime", volume, pitch);
            self.last_crystal_tick = self.tick;
        }
    }
}
