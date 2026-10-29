use std::sync::Arc;

use bevy::prelude::*;

use crate::direction::Direction;
use crate::entities::feed::EntityAnim;
use crate::renderer::anim::AnimSample;
use crate::renderer::environment::BiomeLayer;
use crate::platform::time::Instant;
use crate::session::{BreakingBlock, Gamemode, SharedState, TargetedBlock};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameViewSystems;

#[derive(Resource, Default)]
pub struct FrameView {
    pub partial: f32,
    pub player_pos: [f32; 3],
    pub player_lerp: [f32; 3],
    pub eye_height: f32,
    pub sleeping: bool,
    pub bed_orientation: Option<Direction>,
    pub day_ticks: f64,
    pub day_rate: f32,
    pub biome: Option<BiomeLayer>,
    pub fov_modifier: f32,
    /// The backwards walk distance and the bob, for view bobbing.
    pub view_bob: (f32, f32),
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    pub in_world: bool,
    pub gamemode: Gamemode,
    pub flying: bool,
    pub hotbar_selected: u8,
    pub item_used: [u32; 2],
    pub attack_strength: f32,
    pub crosshair_entity: Option<i32>,
    pub main_hand_left: bool,
    pub entities: Arc<Vec<EntityAnim>>,
    /// Other players, from the same tick as `entities` and `partial`.
    pub players: Arc<Vec<crate::session::OtherPlayerInfo>>,
    pub block_entities:
        Arc<std::collections::HashMap<[i32; 3], crate::blockentities::feed::BlockEntityInfo>>,
    pub block_entities_version: u64,
    pub game_time: u64,
    pub op_level: u8,
    pub local: AnimSample,
    pub breaking: Option<BreakingBlock>,
    pub targeted_block: Option<TargetedBlock>,
    #[cfg(feature = "skins")]
    pub local_skin: crate::client::skins::SkinState,
    #[cfg(feature = "click_gui")]
    pub esp: Arc<crate::modules::esp::EspFrame>,
    pacer: TickPacer,
    shown: Option<TickInputs>,
    pending: Option<TickInputs>,
}

const TICK_SECS: f64 = 0.05;

/// The tick-interpolated session state as one tick left it.
#[derive(Clone)]
struct TickInputs {
    seq: u64,
    player_prev: [f32; 3],
    player_pos: [f32; 3],
    day_clock: crate::session::WorldClock,
    env_prev: Option<BiomeLayer>,
    env: Option<BiomeLayer>,
    fov: crate::session::FovState,
    view_bob: crate::session::ViewBob,
    entities: Arc<Vec<EntityAnim>>,
    players: Arc<Vec<crate::session::OtherPlayerInfo>>,
    local_anim: crate::renderer::anim::HumanoidAnim,
}

impl TickInputs {
    fn of(session: &crate::session::SessionState) -> TickInputs {
        TickInputs {
            seq: session.tick_seq,
            player_prev: session.player_pos_prev,
            player_pos: session.player_pos,
            day_clock: session.day_clock,
            env_prev: session.env_prev,
            env: session.env,
            fov: session.fov,
            view_bob: session.view_bob,
            entities: session.entities.clone(),
            players: session.other_players.clone(),
            local_anim: session.local_anim.clone(),
        }
    }
}

/// A steady 20 Hz clock for interpolation, locked to when ticks land.
///
/// Client ticks run on the network thread, which wakes at its own update rate
/// on a coarse OS timer, so they land 50 ms apart only on average. Timing
/// `partial` from each landing stalls the camera after a late tick and jumps
/// it after an early one. This clock instead runs at exactly one tick per
/// 50 ms, trailing the on-time schedule by the lateness seen recently, so
/// each tick has landed by the time the clock reaches it.
#[derive(Default)]
struct TickPacer {
    origin: Option<Instant>,
    /// When tick 0 would have landed had every tick been on time, in seconds
    /// after `origin`.
    base: f64,
    /// Recent worst lateness past `base`, in seconds.
    lateness: f64,
    /// How far the clock trails `origin`, slewed toward `base + lateness` so
    /// a change in either only speeds or slows it a little.
    lag: f64,
    last_now: Option<f64>,
    seq: Option<u64>,
}

impl TickPacer {
    const MARGIN: f64 = 0.001;
    const MAX_LATENESS: f64 = 0.035;
    /// Lateness past this is a stall or a new session, not jitter.
    const RESYNC: f64 = 0.25;
    const DECAY: f64 = 0.99;
    /// How fast `base` follows ticks that keep landing later, per tick.
    const CREEP: f64 = 0.01;
    /// The most the clock's speed bends while `lag` catches up, as a fraction
    /// of real time.
    const SLEW: f64 = 0.05;

    fn secs(&mut self, at: Instant) -> f64 {
        let origin = *self.origin.get_or_insert(at);
        match at.checked_duration_since(origin) {
            Some(d) => d.as_secs_f64(),
            None => -origin.duration_since(at).as_secs_f64(),
        }
    }

    /// Notes tick `seq` landing at `at`; true when the clock restarted on it.
    fn landed(&mut self, seq: u64, at: Instant) -> bool {
        let offset = self.secs(at) - seq as f64 * TICK_SECS;
        let late = offset - self.base;
        let restart =
            self.seq.is_none_or(|last| seq <= last) || !(-Self::RESYNC..=Self::RESYNC).contains(&late);
        self.seq = Some(seq);
        if restart {
            self.base = offset;
            self.lateness = 0.0;
            self.lag = offset + Self::MARGIN;
        } else if late < 0.0 {
            self.base = offset;
            self.lateness = (self.lateness - late).min(Self::MAX_LATENESS);
        } else {
            self.lateness = late.max(self.lateness * Self::DECAY).min(Self::MAX_LATENESS);
            self.base += late * Self::CREEP;
        }
        restart
    }

    /// The tick being displayed at `now`: inputs of tick `n` cover `n - 1`
    /// to `n`.
    fn clock(&mut self, now: Instant) -> f64 {
        let now = self.secs(now);
        let target = self.base + self.lateness + Self::MARGIN;
        let room = self.last_now.map_or(0.0, |last| (now - last).max(0.0) * Self::SLEW);
        self.lag += (target - self.lag).clamp(-room, room);
        self.last_now = Some(now);
        (now - self.lag) / TICK_SECS - 1.0
    }
}

impl FrameView {
    /// Picks the tick inputs to draw now and how far through them.
    fn pace(&mut self, session: &crate::session::SessionState, now: Instant) -> (TickInputs, f32) {
        let Some(at) = session.last_tick_time else {
            self.pacer = TickPacer::default();
            self.pending = None;
            self.shown = None;
            return (TickInputs::of(session), 0.0);
        };
        if self.pacer.seq != Some(session.tick_seq) {
            let fresh = TickInputs::of(session);
            if self.pacer.landed(session.tick_seq, at) || self.shown.is_none() {
                self.pending = None;
                self.shown = Some(fresh);
            } else {
                if let Some(skipped) = self.pending.replace(fresh) {
                    self.shown = Some(skipped);
                }
            }
        }
        let clock = self.pacer.clock(now);
        if self
            .pending
            .as_ref()
            .is_some_and(|p| clock >= p.seq as f64 - 1.0)
        {
            self.shown = self.pending.take();
        }
        let shown = self.shown.clone().expect("a landed tick is always shown");
        let partial = (clock - (shown.seq as f64 - 1.0)).clamp(0.0, 1.0) as f32;
        (shown, partial)
    }

    pub(crate) fn fill(&mut self, s: &SharedState, in_game: bool) {
        let session = &s.session;
        let (tick, partial) = self.pace(session, Instant::now());
        self.partial = partial;
        let (prev, cur) = (tick.player_prev, tick.player_pos);
        self.player_pos = cur;
        self.player_lerp = std::array::from_fn(|i| prev[i] + (cur[i] - prev[i]) * partial);
        self.eye_height = super::systems::session_eye_height(session);
        self.sleeping = session.sleeping;
        self.bed_orientation = session.bed_orientation;
        self.day_ticks = tick.day_clock.at(partial);
        self.day_rate = tick.day_clock.rate;
        self.biome = match (tick.env_prev, tick.env) {
            (Some(from), Some(now)) => Some(BiomeLayer::lerp(partial, &from, &now)),
            (_, now) => now,
        };
        self.fov_modifier = tick.fov.sample(partial);
        self.view_bob = tick.view_bob.sample(partial);
        self.camera_yaw = s.camera_yaw;
        self.camera_pitch = s.camera_pitch;
        self.in_world = s.in_world;
        self.gamemode = session.gamemode;
        self.flying = session.flying;
        self.hotbar_selected = session.hotbar_selected;
        self.item_used = session.item_used;
        self.attack_strength = session.attack_strength;
        self.crosshair_entity = session.crosshair_entity;
        self.main_hand_left = s.skin_prefs.main_hand_left;
        self.entities = tick.entities;
        self.players = tick.players;
        self.game_time = session.game_time;
        self.op_level = session.op_level;
        #[cfg(feature = "skins")]
        {
            self.local_skin = session.local_skin.clone();
        }
        #[cfg(feature = "click_gui")]
        {
            self.esp = session.esp.clone();
        }
        if in_game {
            if self.block_entities_version != session.block_entities_version {
                self.block_entities_version = session.block_entities_version;
                self.block_entities = session.block_entities.clone();
            }
            self.local = tick.local_anim.sample(partial);
            if self.breaking != session.breaking {
                self.breaking = session.breaking.clone();
            }
            if self.targeted_block != session.targeted_block {
                self.targeted_block = session.targeted_block.clone();
            }
        }
    }

    pub fn tick_eye(&self) -> [f64; 3] {
        [
            self.player_pos[0] as f64,
            self.player_pos[1] as f64 + self.eye_height as f64,
            self.player_pos[2] as f64,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_tick_yet_reads_the_previous_position() {
        let mut s = SharedState::default();
        s.session.player_pos_prev = [1.0, 2.0, 3.0];
        s.session.player_pos = [5.0, 6.0, 7.0];
        let mut view = FrameView::default();
        view.fill(&s, true);
        assert_eq!(view.partial, 0.0);
        assert_eq!(view.player_lerp, [1.0, 2.0, 3.0]);
        assert_eq!(view.player_pos, [5.0, 6.0, 7.0]);
        assert_eq!(view.day_ticks, s.session.day_clock.ticks);
        let eye = view.tick_eye();
        assert_eq!(eye[1], 6.0 + view.eye_height as f64);
    }

    #[test]
    fn a_stale_tick_reads_the_current_position() {
        let Some(then) = crate::platform::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(200))
        else {
            return;
        };
        let mut s = SharedState::default();
        s.session.player_pos_prev = [0.0, 0.0, 0.0];
        s.session.player_pos = [4.0, 8.0, 12.0];
        s.session.last_tick_time = Some(then);
        let mut view = FrameView::default();
        view.fill(&s, true);
        assert_eq!(view.partial, 1.0);
        assert_eq!(view.player_lerp, [4.0, 8.0, 12.0]);
    }

    #[test]
    fn jittery_ticks_draw_at_a_steady_pace() {
        use std::time::Duration;
        let start = Instant::now();
        let ms = |t: u64| start + Duration::from_millis(t);
        // Ticks on a 50 ms schedule, each landing late by up to a 15.6 ms
        // timer slice; the player moves one block per tick.
        let late = [0, 12, 3, 15, 0, 9, 15, 1, 14, 6];
        let mut s = SharedState::default();
        let mut view = FrameView::default();
        let mut next = 0u64;
        let mut last: Option<f32> = None;
        for t in (0..3000).step_by(2) {
            while next < 60 && 50 * next + late[next as usize % late.len()] <= t {
                s.session.tick_seq = next;
                s.session.player_pos_prev = [next as f32 - 1.0, 0.0, 0.0];
                s.session.player_pos = [next as f32, 0.0, 0.0];
                s.session.last_tick_time = Some(ms(50 * next + late[next as usize % late.len()]));
                next += 1;
            }
            let (tick, partial) = view.pace(&s.session, ms(t));
            let x = tick.player_prev[0] + (tick.player_pos[0] - tick.player_prev[0]) * partial;
            if t > 500 && next < 60 {
                let step = x - last.unwrap();
                assert!(
                    (step - 0.04).abs() <= 0.04 * TickPacer::SLEW as f32 + 1e-4,
                    "at {t} ms the player moved {step} blocks in a 2 ms frame"
                );
            }
            last = Some(x);
        }
    }

    #[test]
    fn a_menu_frame_leaves_the_world_only_fields() {
        let s = SharedState::default();
        let mut view = FrameView {
            hotbar_selected: 3,
            ..FrameView::default()
        };
        view.local.walk_pos = 9.0;
        view.fill(&s, false);
        assert_eq!(view.local.walk_pos, 9.0);
        assert_eq!(view.hotbar_selected, 0);
    }
}
