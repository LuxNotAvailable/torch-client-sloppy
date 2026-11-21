//! Video Settings > Tune Settings: tries each performance setting's values
//! in the running world and keeps the ones that make frames cheaper. Each
//! candidate is A/B tested against the current value over six alternating
//! windows. Each window waits until terrain work (sections meshed,
//! uploaded, chunks received) drops back to the background rate measured
//! at the start and stays there a second, is retaken after a hitch, and counts
//! the trimmed mean frame time (GPU time under vsync). A candidate wins only
//! when every one of its windows beats every base window.
//!
//! A setup page picks the goal first ([`Goal`]). Gate 1 always tries
//! settings that keep the picture (culling, meshing): free ones switch for
//! a small gain, barely visible ones for a larger one. Then a stable-fps
//! check (90th percentile frame) decides Gate 2:
//! - Hit Target FPS: under the target, every setting that costs frames,
//!   least destructive first, the mildest value that helps, stopping once
//!   the target is reached.
//! - Max FPS: all of those, each at its fastest value that helps.
//! - Best Quality: under the target, as Hit Target FPS; over it, a Quality
//!   Boost that raises settings, most visible first, keeping each at its
//!   highest value whose stable fps stays 5% over the target.
//!
//! While it runs, input is swallowed ([`swallow_input`]), the frame cap is
//! lifted (`app::systems::limit_framerate`) and nothing is saved. The
//! options from before the run are written to `tune_backup.json` first; a
//! close or crash mid-run leaves that file, and the next start restores it
//! (`gui::load_options`). At the end each change can be kept or dropped,
//! then Apply saves the kept ones and Cancel puts everything back.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;

use crate::gui::antialias::AaMode;
use crate::gui::options::{BIG_W, LAYOUT};
use crate::gui::painter::Painter;
use crate::gui::widgets::{Button, WIDGET_HEIGHT, WIDGET_WIDTH_BIG};
use crate::gui::{GuiOptions, GuiState, Screen, ScreenCtx};
use crate::platform::time::Instant;
use crate::scene::clouds::CloudStatus;
use crate::text::{Span, Style};
use crate::util::particles::ParticleStatus;

const TITLE: &str = "Tune Settings";
const GREEN: u32 = 0x55FF55;
const RED: u32 = 0xFF5555;
const YELLOW: u32 = 0xFFFF55;
const GRAY: u32 = 0x808080;
const WHITE: u32 = 0xFFFFFF;
const AQUA: u32 = 0x55FFFF;
const GOLD: u32 = 0xFFAA00;
const DARK_RED: u32 = 0xAA0000;
const PURPLE: u32 = 0xFF55FF;

/// Seconds after a switch before the rebuild check, then seconds the
/// terrain must stay rebuilt (uploads, caches), then seconds counted.
const SETTLE: f32 = 0.5;
const SETTLE_REMESH: f32 = 1.5;
/// Render distance: the server resends chunks before the queue fills.
const SETTLE_RELOAD: f32 = 5.0;
const QUIET: f32 = 1.0;
const SAMPLE: f32 = 2.5;
/// Terrain work is judged per bucket of this many seconds; the background
/// rate is measured over `CALIBRATE` seconds first. A bucket over
/// `BURST` times the background, plus `BURST_MIN`, is a rebuild.
const BUCKET: f32 = 0.5;
const CALIBRATE: f32 = 2.0;
const BURST: f32 = 2.0;
const BURST_MIN: f32 = 8.0;
/// A rebuild that never ends (chunks streaming in) is measured anyway.
const MAX_WAIT: f32 = 30.0;
/// A window whose 90th percentile frame is this many times its 10th is
/// a hitch: measured again, up to `RETRIES` times.
const SPREAD_MAX: f32 = 1.6;
const RETRIES: u32 = 2;
/// Base and candidate alternating: drift cancels out.
const ORDER: [bool; 6] = [false, true, true, false, false, true];
/// The gain a setting needs to switch: free ones change nothing on screen.
const GAIN_FREE: f32 = 0.02;
const GAIN_COSTLY: f32 = 0.06;
/// Gate 2 trades looks for frames: any clear gain counts.
const GAIN_LAST: f32 = 0.03;
/// The targets offered, and the default.
const TARGETS: [u32; 7] = [30, 60, 90, 120, 144, 165, 240];
const DEFAULT_TARGET: u32 = 60;
/// Quality Boost keeps a value only this far under the target's frame time.
const BOOST_MARGIN: f32 = 0.95;
/// A Quality Boost trial measures the candidate alone, twice.
const ORDER_UP: [bool; 2] = [true, true];
/// The FPS sidebar's width, and how often its number updates.
const SIDEBAR_W: f32 = 72.0;
const FPS_EVERY: f32 = 0.25;

static RUNNING: AtomicBool = AtomicBool::new(false);
/// GPU ms per frame summed, and frames (`note_gpu`).
static GPU: Mutex<(f64, u32)> = Mutex::new((0.0, 0));

/// Whether a tune is measuring: input is off and the frame cap lifted.
pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// A frame's GPU time, from the native renderer's timestamps.
pub fn note_gpu(ms: f32) {
    let mut g = GPU.lock().unwrap_or_else(|e| e.into_inner());
    g.0 += f64::from(ms);
    g.1 += 1;
}

fn take_gpu() -> Option<f32> {
    let mut g = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let (sum, n) = std::mem::take(&mut *g);
    (n > 0).then(|| (sum / f64::from(n)) as f32)
}

/// Clears every key, button and touch while a tune runs.
pub fn swallow_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut touches: ResMut<bevy::input::touch::Touches>,
) {
    if running() {
        keys.reset_all();
        buttons.reset_all();
        touches.clear();
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Cost {
    /// Nothing changes on screen.
    Free,
    /// Barely visible: needs a bigger gain.
    Costly,
    /// Gate 2: trades looks for frames.
    Last,
}

struct Knob {
    name: &'static str,
    cost: Cost,
    /// Seconds after a switch before the rebuild check.
    settle: f32,
    get: fn(&GuiOptions) -> u32,
    set: fn(&mut GuiOptions, u32),
    /// Values to try besides the current one, mildest first.
    values: fn(&GuiOptions) -> Vec<u32>,
    label: fn(u32) -> String,
}

impl Knob {
    fn gate(&self) -> u8 {
        if self.cost == Cost::Last { 2 } else { 1 }
    }
}

fn on_off(v: u32) -> String {
    if v != 0 { "ON" } else { "OFF" }.to_owned()
}

fn both(_: &GuiOptions) -> Vec<u32> {
    vec![0, 1]
}

fn off(_: &GuiOptions) -> Vec<u32> {
    vec![0]
}

/// `values` at or below `current`, mildest (highest) first.
fn at_most(current: u32, values: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = values.iter().copied().filter(|&x| x <= current).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v
}

macro_rules! toggle {
    ($name:literal, $field:ident, $cost:ident, $settle:expr, $values:expr) => {
        Knob {
            name: $name,
            cost: Cost::$cost,
            settle: $settle,
            get: |o| u32::from(o.$field),
            set: |o, v| o.$field = v != 0,
            values: $values,
            label: on_off,
        }
    };
}

fn cloud(v: u32) -> CloudStatus {
    [CloudStatus::Off, CloudStatus::Fast, CloudStatus::Fancy][v.min(2) as usize]
}

fn particle(v: u32) -> ParticleStatus {
    [
        ParticleStatus::All,
        ParticleStatus::Decreased,
        ParticleStatus::Minimal,
    ][v.min(2) as usize]
}

const AA_MODES: [AaMode; 4] = [AaMode::Off, AaMode::Fxaa, AaMode::Msaa, AaMode::Supersample];

fn aa_index(o: &GuiOptions) -> u32 {
    AA_MODES
        .iter()
        .position(|&m| m == o.antialiasing.mode)
        .unwrap_or(0) as u32
}

const KNOBS: &[Knob] = &[
    // Gate 1: the picture stays.
    toggle!("Entity Culling", entity_culling, Free, SETTLE, both),
    toggle!(
        "Cutout Face Culling",
        cutout_face_culling,
        Free,
        SETTLE,
        both
    ),
    toggle!(
        "Angled Face Culling",
        angled_face_culling,
        Free,
        SETTLE,
        both
    ),
    toggle!("Exact Face Culling", exact_face_culling, Free, SETTLE, both),
    toggle!("Per-Face Culling", per_face_culling, Free, SETTLE, both),
    toggle!("Near-First", near_first, Free, SETTLE, both),
    toggle!("Half-Rate Occlusion", hiz_every_other, Free, SETTLE, both),
    toggle!(
        "Entity Away-Face Culling",
        entity_away_faces,
        Free,
        SETTLE,
        both
    ),
    Knob {
        name: "Leaf Culling",
        cost: Cost::Free,
        settle: SETTLE_REMESH,
        get: |o| o.leaf_culling,
        set: |o, v| o.leaf_culling = v,
        values: |_| vec![1, 2, 0],
        label: |v| ["Off", "Smart", "Full"][v.min(2) as usize].to_owned(),
    },
    toggle!("Quarter-Res Occlusion", hiz_quarter, Costly, SETTLE, both),
    toggle!(
        "Entity Back Faces Hidden",
        entity_backface_culling,
        Costly,
        SETTLE,
        both
    ),
    toggle!("Leaf Inner Faces", leaf_backs, Costly, SETTLE_REMESH, both),
    Knob {
        name: "Greedy Meshing",
        cost: Cost::Costly,
        settle: SETTLE_REMESH,
        get: |o| o.greedy_meshing,
        set: |o, v| o.greedy_meshing = v,
        // Aggressive is experimental: never picked for the user.
        values: |_| vec![0, 1],
        label: |v| ["OFF", "Standard", "Aggressive"][v.min(2) as usize].to_owned(),
    },
    // Gate 2: least destructive first.
    toggle!("Smooth Player Model", smooth_gui_model, Last, SETTLE, off),
    toggle!(
        "Underwater Light Rays",
        underwater_light_rays,
        Last,
        SETTLE,
        off
    ),
    Knob {
        name: "Light Ray Frequency",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| o.light_ray_frequency,
        set: |o, v| o.light_ray_frequency = v,
        values: |o| at_most(o.light_ray_frequency, &[25, 10]),
        label: |v| format!("{v}%"),
    },
    Knob {
        name: "Sign Distance",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| o.sign_distance,
        set: |o, v| o.sign_distance = v,
        values: |o| at_most(o.sign_distance, &[2, 1]),
        label: |v| format!("{} blocks", v * 16),
    },
    Knob {
        name: "Torch Distance",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| o.torch_distance,
        set: |o, v| o.torch_distance = v,
        values: |o| at_most(o.torch_distance, &[3, 2]),
        label: |v| {
            if v >= 5 {
                "No Limit".to_owned()
            } else {
                format!("{} blocks", v * 16)
            }
        },
    },
    Knob {
        name: "Shader Shadow Updates",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| o.shadow_update,
        set: |o, v| o.shadow_update = v,
        values: |o| {
            [2, 3]
                .into_iter()
                .filter(|&v| v > o.shadow_update)
                .collect()
        },
        label: |v| format!("every {v} frames"),
    },
    toggle!("Entity Shadows", entity_shadows, Last, SETTLE, off),
    Knob {
        name: "Biome Blend",
        cost: Cost::Last,
        settle: SETTLE_REMESH,
        get: |o| o.biome_blend,
        set: |o, v| o.biome_blend = v,
        values: |o| at_most(o.biome_blend, &[o.biome_blend / 2, 0]),
        label: |v| format!("{v} blocks"),
    },
    Knob {
        name: "Particles",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| match o.particles {
            ParticleStatus::All => 0,
            ParticleStatus::Decreased => 1,
            ParticleStatus::Minimal => 2,
        },
        set: |o, v| o.particles = particle(v),
        values: |o| {
            let now = match o.particles {
                ParticleStatus::All => 0,
                ParticleStatus::Decreased => 1,
                ParticleStatus::Minimal => 2,
            };
            [1, 2].into_iter().filter(|&v| v > now).collect()
        },
        label: |v| ["All", "Decreased", "Minimal"][v.min(2) as usize].to_owned(),
    },
    Knob {
        name: "Clouds",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| match o.clouds {
            CloudStatus::Off => 0,
            CloudStatus::Fast => 1,
            CloudStatus::Fancy => 2,
        },
        set: |o, v| o.clouds = cloud(v),
        values: |o| {
            let now = match o.clouds {
                CloudStatus::Off => 0,
                CloudStatus::Fast => 1,
                CloudStatus::Fancy => 2,
            };
            at_most(now, &[1, 0])
        },
        label: |v| cloud(v).caption().to_owned(),
    },
    Knob {
        name: "Entity Distance",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| o.entity_distance,
        set: |o, v| o.entity_distance = v,
        values: |o| {
            let d = o.entity_distance;
            at_most(d, &[d.saturating_sub(1).max(2), d.saturating_sub(2).max(2)])
        },
        label: |v| format!("{}%", v * 25),
    },
    Knob {
        name: "Leaves",
        cost: Cost::Last,
        settle: SETTLE_REMESH,
        get: |o| o.leaves,
        set: |o, v| o.leaves = v,
        values: |_| vec![1],
        label: |v| ["Fancy", "Fast", "Hybrid"][v.min(2) as usize].to_owned(),
    },
    toggle!("Light Rays", light_rays, Last, SETTLE, off),
    Knob {
        name: "Antialiasing",
        cost: Cost::Last,
        settle: SETTLE,
        get: aa_index,
        set: |o, v| o.antialiasing.mode = AA_MODES[v.min(3) as usize],
        values: |o| at_most(aa_index(o), &[1, 0]),
        label: |v| AA_MODES[v.min(3) as usize].caption().to_owned(),
    },
    toggle!("Smooth Lighting", smooth_lighting, Last, SETTLE_REMESH, off),
    Knob {
        name: "Render Scale",
        cost: Cost::Last,
        settle: SETTLE,
        get: |o| o.pack_render_scale,
        set: |o, v| o.pack_render_scale = v,
        values: |o| {
            at_most(o.pack_render_scale, &[85, 70, 50])
                .into_iter()
                .filter(|&v| v >= crate::gui::PACK_RENDER_SCALE_MIN)
                .collect()
        },
        label: |v| format!("{v}%"),
    },
    Knob {
        name: "Render Distance",
        cost: Cost::Last,
        settle: SETTLE_RELOAD,
        get: |o| o.render_distance,
        set: |o, v| o.render_distance = v,
        values: |o| {
            let d = o.render_distance;
            let min = crate::gui::options::RENDER_DISTANCE_MIN as u32;
            at_most(d, &[(d * 3 / 4).max(min), (d / 2).max(min)])
        },
        label: |v| format!("{v} chunks"),
    },
    toggle!(
        "Lighting Engine",
        lighting_enabled,
        Last,
        SETTLE_REMESH,
        off
    ),
];

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Pending,
    Testing,
    /// Measured: the value before and the one chosen, and the frame time
    /// change (negative is faster).
    Done {
        from: u32,
        to: u32,
        change: f32,
    },
    /// Gate 2 wasn't needed.
    Skipped,
}

#[derive(Clone, Copy)]
struct Row {
    status: Status,
    /// Review: the change is kept.
    keep: bool,
}

/// One measuring window: waits out the switch and any terrain rebuild,
/// then counts frames, retaking itself after a hitch.
struct Window {
    switched: Instant,
    /// When the terrain was last seen rebuilt, for `QUIET`.
    quiet_since: Option<Instant>,
    /// Settling or rebuilding, for the checklist.
    waiting: bool,
    /// Terrain work seen during the wait, for the checklist.
    rebuilt: bool,
    /// The current bucket's start and `terrain_work` then.
    bucket: (Instant, u64),
    /// The last full bucket was a rebuild; true until one is judged.
    active: bool,
    retries: u32,
    frames: Vec<f32>,
}

impl Window {
    fn new(now: Instant) -> Self {
        Window {
            switched: now,
            quiet_since: None,
            waiting: true,
            rebuilt: false,
            bucket: (now, terrain_work()),
            active: true,
            retries: 0,
            frames: Vec::new(),
        }
    }

    /// The window's score and stable time (ms per frame: trimmed mean and
    /// 90th percentile, or GPU time under vsync) once it's full.
    fn tick(
        &mut self,
        now: Instant,
        dt: Option<f32>,
        settle: f32,
        vsync: bool,
        background: f32,
    ) -> Option<(f32, f32)> {
        let since = now.duration_since(self.switched).as_secs_f32();
        // Frames count only once the terrain has rebuilt and stayed so.
        // Work well over the background in a bucket restarts the quiet
        // time; a switch that rebuilds nothing goes on after the settle.
        let (at, work0) = self.bucket;
        if now.duration_since(at).as_secs_f32() >= BUCKET {
            let work = terrain_work();
            let done = work.saturating_sub(work0) as f32;
            self.active = done > background * BURST + BURST_MIN;
            self.rebuilt |= self.active;
            self.bucket = (now, work);
        }
        let rebuilding = since < settle || (self.active && since < MAX_WAIT);
        if rebuilding {
            self.quiet_since = None;
        }
        let quiet = *self.quiet_since.get_or_insert(now);
        let start = now.duration_since(quiet).as_secs_f32();
        self.waiting = rebuilding || start < QUIET;
        if self.waiting {
            self.frames.clear();
            take_gpu();
            return None;
        }
        if let Some(ms) = dt {
            self.frames.push(ms);
        }
        if start < QUIET + SAMPLE {
            return None;
        }
        // A hitch in the window (a stray rebuild, a GC): measure again.
        if spread(&mut self.frames) > SPREAD_MAX && self.retries < RETRIES {
            self.retries += 1;
            self.quiet_since = Some(now);
            self.frames.clear();
            take_gpu();
            return None;
        }
        // Under vsync frames pace to the display: the GPU's time tells more.
        let p90 = percentile(&mut self.frames, 0.9);
        match take_gpu() {
            Some(g) if vsync => Some((g, g)),
            _ => Some((trimmed_mean(&mut self.frames), p90)),
        }
    }
}

/// One candidate's A/B.
struct Trial {
    candidate: u32,
    /// Which side each window measures (`ORDER`, or `ORDER_UP`).
    order: &'static [bool],
    step: usize,
    window: Window,
    base: Vec<f32>,
    cand: Vec<f32>,
    /// The candidate's stable times.
    cand_stable: Vec<f32>,
    base_stable: Vec<f32>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Gate2 {
    #[default]
    NotReached,
    /// Measuring the frame rate Gate 1 left.
    Checking,
    /// Gate 1 left this many fps: at the target, so skipped.
    Skipped(f32),
    /// Running from this many fps.
    Running(f32),
    /// Reached the target at this many fps.
    Reached(f32),
    /// Best Quality over the target: raising settings from this many fps.
    Boosting(f32),
}

/// What the tune chases, picked on the setup page.
#[derive(Default, Clone, Copy, PartialEq)]
pub enum Goal {
    /// Reach the target, giving up as little as possible.
    #[default]
    Target,
    /// As many frames as the settings allow.
    MaxFps,
    /// The best looks that still hold the target.
    Quality,
}

impl Goal {
    fn name(self) -> &'static str {
        match self {
            Goal::Target => "Hit Target FPS",
            Goal::MaxFps => "Max FPS",
            Goal::Quality => "Best Quality",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Goal::Target => "Cuts looks only as far as the target needs.",
            Goal::MaxFps => "Takes every setting that makes frames cheaper.",
            Goal::Quality => "Raises looks while the target holds steady.",
        }
    }

    fn next(self) -> Goal {
        match self {
            Goal::Target => Goal::MaxFps,
            Goal::MaxFps => Goal::Quality,
            Goal::Quality => Goal::Target,
        }
    }
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    /// The goal and target are being picked.
    Setup,
    Running,
    Review,
}

#[derive(Default)]
pub struct TuneState {
    phase: Phase,
    goal: Goal,
    /// The target fps (`TARGETS`); 0 before setup.
    target: u32,
    original: Option<GuiOptions>,
    rows: Vec<Row>,
    /// Knob indices in the order they're tried, and the place in it.
    order: Vec<usize>,
    pos: usize,
    knob: usize,
    base: u32,
    /// Candidates left for the current knob, mildest last (popped first),
    /// and the best so far (value, frame time change, ms).
    queue: Vec<u32>,
    best: Option<(u32, f32, f32)>,
    worst_change: f32,
    /// The current value's ms and stable ms, from the last measuring.
    base_ms: f32,
    base_stable: f32,
    trial: Option<Trial>,
    gate2: Gate2,
    check: Option<Window>,
    last_frame: Option<Instant>,
    /// Background terrain work per `BUCKET`, once measured, and the
    /// measuring's start.
    background: Option<f32>,
    calibrate: Option<(Instant, u64)>,
    /// The sidebar: frames since its last update (ms summed, count), and
    /// the number shown.
    fps_sum: (f32, u32),
    fps_at: Option<Instant>,
    fps: f32,
}

impl TuneState {
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    fn target_ms(&self) -> f32 {
        1000.0 / self.target.max(1) as f32
    }

    fn boosting(&self) -> bool {
        matches!(self.gate2, Gate2::Boosting(_))
    }
}

/// Opens the setup page: the goal and target, then Start.
pub fn open(state: &mut GuiState) {
    state.tune = TuneState {
        phase: Phase::Setup,
        target: DEFAULT_TARGET,
        ..Default::default()
    };
    state.nav = Some(Screen::TuneSettings);
}

/// Starts a tune: backs the options up and runs the checklist.
fn start(state: &mut GuiState) {
    let original = state.options.clone();
    crate::platform::storage::TUNE_BACKUP.store(&crate::gui::options_json(&original));
    state.tune = TuneState {
        phase: Phase::Running,
        goal: state.tune.goal,
        target: state.tune.target,
        order: (0..KNOBS.len()).collect(),
        original: Some(original),
        rows: KNOBS
            .iter()
            .map(|_| Row {
                status: Status::Pending,
                keep: true,
            })
            .collect(),
        ..Default::default()
    };
    RUNNING.store(true, Ordering::Relaxed);
    take_gpu();
    advance(state);
}

fn apply_live(state: &GuiState) {
    crate::gui::publish_unsaved(&state.options);
}

/// From `knob` on: starts the next knob with something to try, the gate
/// check, or the review.
fn advance(state: &mut GuiState) {
    let t = &mut state.tune;
    while t.pos < t.order.len() {
        t.knob = t.order[t.pos];
        let k = &KNOBS[t.knob];
        if k.gate() == 2 {
            match t.gate2 {
                Gate2::NotReached => {
                    t.gate2 = Gate2::Checking;
                    t.check = Some(Window::new(Instant::now()));
                    take_gpu();
                    return;
                }
                Gate2::Checking | Gate2::Running(_) | Gate2::Boosting(_) => {}
                Gate2::Skipped(_) | Gate2::Reached(_) => {
                    for &i in &t.order[t.pos..] {
                        t.rows[i].status = Status::Skipped;
                    }
                    break;
                }
            }
        }
        let base = (k.get)(&state.options);
        let values = if k.gate() == 2 && t.boosting() {
            up(k, &state.options)
        } else {
            (k.values)(&state.options)
        };
        let mut queue: Vec<u32> = values.into_iter().filter(|&v| v != base).collect();
        if !queue.is_empty() {
            queue.reverse();
            t.base = base;
            t.queue = queue;
            t.best = None;
            t.worst_change = 0.0;
            t.rows[t.knob].status = Status::Testing;
            next_trial(state);
            return;
        }
        t.rows[t.knob].status = Status::Done {
            from: base,
            to: base,
            change: 0.0,
        };
        t.pos += 1;
    }
    t.phase = Phase::Review;
    t.trial = None;
    RUNNING.store(false, Ordering::Relaxed);
}

fn next_trial(state: &mut GuiState) {
    let t = &mut state.tune;
    let Some(candidate) = t.queue.pop() else {
        finish_knob(state);
        return;
    };
    let order: &'static [bool] = if KNOBS[t.knob].gate() == 2 && t.boosting() {
        &ORDER_UP
    } else {
        &ORDER
    };
    t.trial = Some(Trial {
        candidate,
        order,
        step: 0,
        window: Window::new(Instant::now()),
        base: Vec::new(),
        cand: Vec::new(),
        cand_stable: Vec::new(),
        base_stable: Vec::new(),
    });
    set_side(state);
}

/// Sets the knob to the side the trial's step calls for.
fn set_side(state: &mut GuiState) {
    let t = &state.tune;
    let Some(trial) = &t.trial else { return };
    let value = if trial.order[trial.step] {
        trial.candidate
    } else {
        t.base
    };
    (KNOBS[t.knob].set)(&mut state.options, value);
    apply_live(state);
    take_gpu();
}

fn finish_knob(state: &mut GuiState) {
    let t = &mut state.tune;
    let k = &KNOBS[t.knob];
    let chosen = t.best.map_or(t.base, |b| b.0);
    let change = t.best.map_or(t.worst_change, |b| b.1);
    let ms = t.best.map_or(t.base_stable, |b| b.2);
    t.rows[t.knob].status = Status::Done {
        from: t.base,
        to: chosen,
        change,
    };
    if k.gate() == 2 && t.goal != Goal::MaxFps && !t.boosting() && ms > 0.0 && ms <= t.target_ms() {
        t.gate2 = Gate2::Reached(1000.0 / ms);
    }
    t.pos += 1;
    (k.set)(&mut state.options, chosen);
    apply_live(state);
    advance(state);
}

/// Sections meshed, uploaded and chunks received so far.
fn terrain_work() -> u64 {
    use crate::diag::{Stat, get};
    get(Stat::SectionsMeshed) + get(Stat::SectionsUploaded) + get(Stat::ChunksReceived)
}

/// The mean without the fastest and slowest tenth.
fn trimmed_mean(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    let cut = v.len() / 10;
    let kept = &v[cut..v.len() - cut];
    kept.iter().sum::<f32>() / kept.len() as f32
}

/// The 90th percentile frame over the 10th.
fn spread(v: &mut [f32]) -> f32 {
    if v.len() < 10 {
        return 1.0;
    }
    v.sort_by(f32::total_cmp);
    let low = v[v.len() / 10].max(0.01);
    v[v.len() * 9 / 10] / low
}

fn percentile(v: &mut [f32], at: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[((v.len() as f32 * at) as usize).min(v.len() - 1)]
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// One frame of measuring: counts the frame, and moves on when a window
/// is full.
fn step(state: &mut GuiState, now: Instant, dt: Option<f32>) {
    let vsync = state.options.vsync;
    let t = &mut state.tune;
    // First the background terrain work, with nothing switched.
    let background = match t.background {
        Some(b) => b,
        None => {
            let (at, work0) = *t.calibrate.get_or_insert((now, terrain_work()));
            let secs = now.duration_since(at).as_secs_f32();
            if secs < CALIBRATE {
                return;
            }
            let b = terrain_work().saturating_sub(work0) as f32 * BUCKET / secs;
            t.background = Some(b);
            info!("tune: background terrain work {b:.1} per {BUCKET} s");
            b
        }
    };
    if let Some(check) = &mut t.check {
        if let Some((ms, stable)) = check.tick(now, dt, SETTLE, vsync, background) {
            t.check = None;
            t.base_ms = ms;
            t.base_stable = stable;
            let fps = if stable > 0.0 { 1000.0 / stable } else { 0.0 };
            let under = fps < t.target as f32;
            t.gate2 = match t.goal {
                Goal::MaxFps => Gate2::Running(fps),
                _ if under => Gate2::Running(fps),
                Goal::Target => Gate2::Skipped(fps),
                Goal::Quality => {
                    // Most visible first: Gate 2's list backwards.
                    t.order[t.pos..].reverse();
                    Gate2::Boosting(fps)
                }
            };
            advance(state);
        }
        return;
    }
    let k = &KNOBS[t.knob];
    let boosting = k.gate() == 2 && t.boosting();
    let target_ms = t.target_ms();
    let Some(trial) = &mut t.trial else { return };
    let Some((score, stable)) = trial.window.tick(now, dt, k.settle, vsync, background) else {
        return;
    };
    if trial.order[trial.step] {
        trial.cand.push(score);
        trial.cand_stable.push(stable);
    } else {
        trial.base.push(score);
        trial.base_stable.push(stable);
    }
    trial.step += 1;
    trial.window = Window::new(now);
    if trial.step < trial.order.len() {
        set_side(state);
        return;
    }
    let fmax = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max);
    // Quality Boost: the highest value whose every window holds the target.
    if boosting {
        let cand = median(&mut trial.cand);
        let change = if t.base_ms > 0.0 {
            (cand - t.base_ms) / t.base_ms
        } else {
            0.0
        };
        let stable = fmax(&trial.cand_stable);
        if stable <= target_ms * BOOST_MARGIN {
            t.best = Some((trial.candidate, change, stable));
            t.base_ms = cand;
            t.base_stable = stable;
            t.queue.clear();
        } else if change.abs() > t.worst_change.abs() {
            t.worst_change = change;
        }
        (k.set)(&mut state.options, t.base);
        next_trial(state);
        return;
    }
    // Every candidate window must beat every base one, or it was noise.
    let fmin = |v: &[f32]| v.iter().copied().fold(f32::MAX, f32::min);
    let consistent = fmax(&trial.cand) < fmin(&trial.base);
    let base = median(&mut trial.base);
    let cand = median(&mut trial.cand);
    let change = if base > 0.0 {
        (cand - base) / base
    } else {
        0.0
    };
    t.base_ms = base;
    t.base_stable = median(&mut trial.base_stable);
    let cand_stable = median(&mut trial.cand_stable);
    let need = match k.cost {
        Cost::Free => GAIN_FREE,
        Cost::Costly => GAIN_COSTLY,
        Cost::Last => GAIN_LAST,
    };
    if -change >= need && consistent && t.best.is_none_or(|b| change < b.1) {
        t.best = Some((trial.candidate, change, cand_stable));
        // Gate 2 takes the mildest value that reaches the target.
        if k.cost == Cost::Last && t.goal != Goal::MaxFps && cand_stable <= target_ms {
            t.queue.clear();
        }
    }
    if change.abs() > t.worst_change.abs() {
        t.worst_change = change;
    }
    (k.set)(&mut state.options, t.base);
    next_trial(state);
}

/// Cancels a tune whose screen closed under it (a disconnect or kick), so
/// input comes back with the options from before.
pub fn guard(mut state: ResMut<GuiState>) {
    if state.tune.active()
        && state.screen != Screen::TuneSettings
        && state.nav != Some(Screen::TuneSettings)
    {
        end(&mut state, false, false);
    }
}

/// Leaves the tune: `keep` saves the kept changes, else the options from
/// before come back; `back` returns to Video Settings.
fn end(state: &mut GuiState, keep: bool, back: bool) {
    let t = std::mem::take(&mut state.tune);
    RUNNING.store(false, Ordering::Relaxed);
    if back {
        state.nav = Some(Screen::VideoSettings);
    }
    let Some(mut options) = t.original else {
        return;
    };
    if keep {
        for (k, row) in KNOBS.iter().zip(&t.rows) {
            if let Status::Done { from, to, .. } = row.status
                && from != to
                && row.keep
            {
                (k.set)(&mut options, to);
            }
        }
    }
    state.options = options;
    crate::gui::save_options(&state.options);
    crate::platform::storage::TUNE_BACKUP.remove();
}

fn span(text: String, color: u32) -> Span {
    Span {
        text,
        style: Style::colored(color),
    }
}

fn change_text(change: f32) -> String {
    format!("{:+.1}%", change * 100.0)
}

fn changed(row: &Row) -> bool {
    matches!(row.status, Status::Done { from, to, .. } if from != to)
}

/// A row's line: its name, what was found, in the status's colour.
fn row_line(i: usize, t: &TuneState) -> (String, u32) {
    let k = &KNOBS[i];
    match t.rows[i].status {
        Status::Pending => (format!("  {}", k.name), GRAY),
        Status::Skipped => (format!("- {} (not needed)", k.name), GRAY),
        Status::Testing => {
            let detail = t.trial.as_ref().map_or(String::new(), |tr| {
                let side = if tr.order[tr.step.min(tr.order.len() - 1)] {
                    (k.label)(tr.candidate)
                } else {
                    (k.label)(t.base)
                };
                let wait = match (tr.window.waiting, tr.window.rebuilt) {
                    (false, _) => "",
                    (true, true) => ", rebuilding",
                    (true, false) => ", settling",
                };
                let again = if tr.window.retries > 0 {
                    ", hitch, again"
                } else {
                    ""
                };
                format!(": {side} ({}/{}{wait}{again})", tr.step + 1, tr.order.len())
            });
            (format!("> {}{detail}", k.name), YELLOW)
        }
        Status::Done { from, to, change } if from != to => (
            format!(
                "{}: {} -> {} ({})",
                k.name,
                (k.label)(from),
                (k.label)(to),
                change_text(change)
            ),
            GREEN,
        ),
        Status::Done { from, change, .. } => (
            format!(
                "= {}: {} kept ({})",
                k.name,
                (k.label)(from),
                change_text(change)
            ),
            WHITE,
        ),
    }
}

fn gate_header(gate: u8, t: &TuneState) -> (String, u32) {
    if gate == 1 {
        return ("Gate 1: Quality-Preserving".to_owned(), AQUA);
    }
    let target = t.target;
    let name = match (t.goal, t.gate2) {
        (_, Gate2::Boosting(_)) => "Gate 2: Quality Boost",
        (Goal::MaxFps, _) => "Gate 2: Max FPS",
        _ => "Gate 2: Last Resort",
    };
    match t.gate2 {
        Gate2::NotReached => match t.goal {
            Goal::Target => (format!("{name} (only below {target} fps)"), GOLD),
            Goal::MaxFps => (format!("{name} (after Gate 1)"), GOLD),
            Goal::Quality => (
                format!("Gate 2: Last Resort or Quality Boost ({target} fps)"),
                GOLD,
            ),
        },
        Gate2::Checking => (format!("{name}: checking stable fps..."), YELLOW),
        Gate2::Skipped(fps) => (format!("{name}: skipped, {fps:.0} fps"), GRAY),
        Gate2::Running(fps) => (format!("{name}: from {fps:.0} fps"), GOLD),
        Gate2::Reached(fps) => (format!("{name}: reached {fps:.0} fps"), GREEN),
        Gate2::Boosting(fps) => (format!("{name}: from {fps:.0} fps"), PURPLE),
    }
}

/// A checklist entry.
enum Item {
    Header(String, u32),
    Text(String, u32),
    /// A change with its Keep/Discard toggle (review).
    Toggle(usize),
}

/// The checklist: live while measuring, then the changes to keep or drop
/// under Apply and Cancel, with the frame rate beside it.
pub fn draw(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let now = Instant::now();
    let dt = state
        .tune
        .last_frame
        .replace(now)
        .map(|at| now.duration_since(at).as_secs_f32() * 1000.0);
    count_fps(&mut state.tune, now, dt);
    crate::gui::options::draw_title(p, ctx, TITLE);
    match state.tune.phase {
        Phase::Idle => {
            state.nav = Some(Screen::VideoSettings);
            return;
        }
        Phase::Setup => {
            draw_setup(p, state, ctx);
            return;
        }
        Phase::Running => {
            step(state, now, dt);
            draw_running(p, state, ctx);
        }
        Phase::Review => draw_review(p, state, ctx),
    }
    draw_fps(p, &state.tune, ctx);
}

fn count_fps(t: &mut TuneState, now: Instant, dt: Option<f32>) {
    if let Some(ms) = dt {
        t.fps_sum.0 += ms;
        t.fps_sum.1 += 1;
    }
    let at = *t.fps_at.get_or_insert(now);
    if now.duration_since(at).as_secs_f32() >= FPS_EVERY && t.fps_sum.1 > 0 {
        let (sum, n) = std::mem::take(&mut t.fps_sum);
        t.fps = 1000.0 * n as f32 / sum.max(0.001);
        t.fps_at = Some(now);
    }
}

/// The frame rate's word and colour.
fn fps_rating(fps: f32) -> (&'static str, u32) {
    match fps {
        f if f < 30.0 => ("Very Poor", DARK_RED),
        f if f < 45.0 => ("Poor", RED),
        f if f < 60.0 => ("Healthy", YELLOW),
        f if f < 90.0 => ("Good", GREEN),
        f if f < 144.0 => ("High", AQUA),
        _ => ("Very High", PURPLE),
    }
}

/// The sidebar: frames per second now, and what that rates as.
fn draw_fps(p: &mut Painter, t: &TuneState, ctx: &ScreenCtx) {
    let x = ctx.vw - SIDEBAR_W - 4.0;
    let y = LAYOUT.title_y() + 14.0;
    p.fill(x, y - 3.0, SIDEBAR_W, 3.0 * line_h() + 4.0, 0x80000000);
    let (word, color) = fps_rating(t.fps);
    let lines = [
        ("FPS".to_owned(), GRAY),
        (format!("{:.0}", t.fps), WHITE),
        (word.to_owned(), color),
    ];
    for (i, (text, c)) in lines.iter().enumerate() {
        let w = p.atlas.font.width_str(text);
        let cx = (x + (SIDEBAR_W - w) / 2.0).floor();
        p.text_str(text, cx, y + i as f32 * line_h(), *c, true);
    }
}

fn line_h() -> f32 {
    crate::text::LINE_HEIGHT + 3.0
}

/// Each gate's header and rows; in review, a gate's changes come first.
fn gate_items(t: &TuneState, review: bool) -> Vec<Item> {
    let mut items = Vec::new();
    for gate in [1, 2] {
        let (s, c) = gate_header(gate, t);
        items.push(Item::Header(s, c));
        let rows: Vec<usize> = (0..KNOBS.len())
            .filter(|&i| KNOBS[i].gate() == gate)
            .collect();
        if review {
            items.extend(
                rows.iter()
                    .filter(|&&i| changed(&t.rows[i]))
                    .map(|&i| Item::Toggle(i)),
            );
        }
        for &i in &rows {
            if review && changed(&t.rows[i]) {
                continue;
            }
            let (s, c) = row_line(i, t);
            items.push(Item::Text(s, if review { GRAY } else { c }));
        }
    }
    items
}

fn draw_running(p: &mut Painter, state: &GuiState, ctx: &ScreenCtx) {
    let t = &state.tune;
    let mut y = LAYOUT.title_y() + 14.0;
    let done = t
        .rows
        .iter()
        .filter(|r| !matches!(r.status, Status::Pending))
        .count();
    let head = if t.background.is_none() {
        "Measuring background terrain work. Input is off until it finishes.".to_owned()
    } else {
        let goal = match t.goal {
            Goal::MaxFps => t.goal.name().to_owned(),
            g => format!("{} ({} fps)", g.name(), t.target),
        };
        format!(
            "{goal}: setting {} of {}. Input is off until it finishes.",
            done.clamp(1, KNOBS.len()),
            KNOBS.len()
        )
    };
    p.text_str(&head, 8.0, y, AQUA, true);
    y += line_h() + 3.0;
    draw_items(p, ctx, y, &gate_items(t, false), &t.rows);
}

/// The items in columns left of the sidebar; the toggle clicked, if any.
fn draw_items(
    p: &mut Painter,
    ctx: &ScreenCtx,
    top: f32,
    items: &[Item],
    rows: &[Row],
) -> Option<usize> {
    let lh = line_h();
    let room = (ctx.vh - 6.0 - top).max(lh);
    let per = ((room / lh).floor() as usize).max(1);
    let columns = items.len().div_ceil(per).max(1);
    let area = ctx.vw - SIDEBAR_W - 12.0;
    let col_w = (area / columns as f32).min(BIG_W).floor();
    let start = ((area - col_w * columns as f32) / 2.0).floor().max(4.0);
    let mut clicked = None;
    for (i, item) in items.iter().enumerate() {
        let x = start + (i / per) as f32 * col_w;
        let y = top + (i % per) as f32 * lh;
        let clip = p.push_clip(x, y - 1.0, col_w - 4.0, lh);
        match item {
            Item::Header(text, color) | Item::Text(text, color) => {
                p.text_str(text, x, y, *color, true);
            }
            Item::Toggle(k) => {
                let keep = rows[*k].keep;
                let button_w = 44.0;
                let text = changed_line(*k, rows);
                p.text_str(&text, x, y, if keep { GREEN } else { GRAY }, true);
                let (label, color) = if keep {
                    ("Keep", GREEN)
                } else {
                    ("Discard", RED)
                };
                let bx = x + col_w - 4.0 - button_w;
                let spans = [span(label.to_owned(), color)];
                if (Button {
                    spans: Some(&spans),
                    ..Button::new(bx, y - 1.0, button_w, lh - 1.0, "")
                })
                .draw(p, ctx)
                {
                    clicked = Some(*k);
                }
            }
        }
        p.pop_clip(clip);
    }
    clicked
}

/// A changed row's text (review).
fn changed_line(i: usize, rows: &[Row]) -> String {
    let k = &KNOBS[i];
    match rows[i].status {
        Status::Done { from, to, change } => format!(
            "{}: {} -> {} ({})",
            k.name,
            (k.label)(from),
            (k.label)(to),
            change_text(change)
        ),
        _ => k.name.to_owned(),
    }
}

fn colored_button(
    p: &mut Painter,
    ctx: &ScreenCtx,
    x: f32,
    y: f32,
    w: f32,
    text: &str,
    color: u32,
) -> bool {
    let spans = [span(text.to_owned(), color)];
    Button {
        spans: Some(&spans),
        ..Button::new(x, y, w, WIDGET_HEIGHT, "")
    }
    .draw(p, ctx)
}

/// The setup page: the goal, the target, Start and Cancel.
fn draw_setup(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let x = ((ctx.vw - WIDGET_WIDTH_BIG) / 2.0).floor();
    let mut y = LAYOUT.title_y() + 16.0;
    let t = &mut state.tune;
    let goal = format!("Goal: {}", t.goal.name());
    if Button::new(x, y, WIDGET_WIDTH_BIG, WIDGET_HEIGHT, &goal).draw(p, ctx) {
        t.goal = t.goal.next();
    }
    y += WIDGET_HEIGHT + 4.0;
    if t.goal != Goal::MaxFps {
        let label = format!("Target: {} fps", t.target);
        if Button::new(x, y, WIDGET_WIDTH_BIG, WIDGET_HEIGHT, &label).draw(p, ctx) {
            let at = TARGETS.iter().position(|&v| v == t.target).unwrap_or(0);
            t.target = TARGETS[(at + 1) % TARGETS.len()];
        }
    }
    y += WIDGET_HEIGHT + 6.0;
    let about = t.goal.about();
    let w = p.atlas.font.width_str(about);
    p.text_str(about, ((ctx.vw - w) / 2.0).floor(), y, GRAY, true);
    y += line_h() + 2.0;
    let note = "Takes several minutes; input is off while it runs.";
    let w = p.atlas.font.width_str(note);
    p.text_str(note, ((ctx.vw - w) / 2.0).floor(), y, GRAY, true);
    y += line_h() + 8.0;
    let half = (WIDGET_WIDTH_BIG / 2.0 - 2.0).floor();
    if colored_button(p, ctx, x, y, half, "Start", GREEN) {
        start(state);
        return;
    }
    if colored_button(p, ctx, x + half + 4.0, y, half, "Cancel", RED) {
        end(state, false, true);
    }
}

/// Values above `current`, highest first.
fn above(current: u32, values: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = values.iter().copied().filter(|&x| x > current).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v
}

/// Values below `current`, lowest first: for settings where lower looks
/// better (shadow updates, particle cuts).
fn below(current: u32, values: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = values.iter().copied().filter(|&x| x < current).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Quality Boost's values for a Gate 2 knob: better looking than now,
/// best first.
fn up(k: &Knob, o: &GuiOptions) -> Vec<u32> {
    let now = (k.get)(o);
    match k.name {
        "Smooth Player Model"
        | "Underwater Light Rays"
        | "Entity Shadows"
        | "Light Rays"
        | "Smooth Lighting"
        | "Lighting Engine" => above(now, &[1]),
        "Light Ray Frequency" => above(now, &[100, 75, 50]),
        "Sign Distance" => above(now, &[4, 3]),
        "Torch Distance" => above(now, &[5, 4]),
        "Shader Shadow Updates" => below(now, &[1, 2]),
        "Particles" => below(now, &[0, 1]),
        "Biome Blend" => above(now, &[5, 3, 2]),
        "Clouds" => above(now, &[2, 1]),
        "Entity Distance" => above(now, &[8, 6, 5]),
        "Leaves" if now != 0 => vec![0],
        // Supersampling is never suggested.
        "Antialiasing" => above(now, &[2, 1]),
        "Render Scale" => above(now, &[100, 85]),
        "Render Distance" => {
            let max = crate::gui::options::RENDER_DISTANCE_MAX as u32;
            above(now, &[(now * 3 / 2).min(max), (now * 5 / 4).min(max)])
        }
        _ => Vec::new(),
    }
}

fn draw_review(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let half = (WIDGET_WIDTH_BIG / 2.0 - 2.0).floor();
    let left = ((ctx.vw - SIDEBAR_W - WIDGET_WIDTH_BIG) / 2.0)
        .floor()
        .max(4.0);
    let top = LAYOUT.title_y() + 12.0;
    if colored_button(p, ctx, left, top, half, "Apply", GREEN) {
        end(state, true, true);
        return;
    }
    if colored_button(p, ctx, left + half + 4.0, top, half, "Cancel", RED) {
        end(state, false, true);
        return;
    }
    let mut y = top + WIDGET_HEIGHT + 4.0;
    if !state.tune.rows.iter().any(changed) {
        p.text_str(
            "Nothing made frames cheaper: your settings stay.",
            left,
            y,
            WHITE,
            true,
        );
        y += line_h();
    }
    let items = gate_items(&state.tune, true);
    if let Some(i) = draw_items(p, ctx, y, &items, &state.tune.rows) {
        state.tune.rows[i].keep = !state.tune.rows[i].keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_one_comes_first() {
        let gates: Vec<u8> = KNOBS.iter().map(Knob::gate).collect();
        assert!(gates.is_sorted(), "{gates:?}");
    }

    #[test]
    fn every_knob_sets_what_it_reads() {
        let mut o = GuiOptions::default();
        for k in KNOBS {
            for v in (k.values)(&o).into_iter().chain(up(k, &o)) {
                (k.set)(&mut o, v);
                assert_eq!((k.get)(&o), v, "{}", k.name);
            }
        }
    }
}
