//! Video Settings > Tune Settings: tries each performance setting's values
//! in the running world and keeps the ones that make frames cheaper. Each
//! candidate is A/B tested against the current value over six alternating
//! windows. Each window waits for the terrain to finish rebuilding
//! (`JobQueue::busy`) and stay rebuilt, is retaken after a hitch, and counts
//! the trimmed mean frame time (GPU time under vsync). A candidate wins only
//! when every one of its windows beats every base window. Settings that cost nothing to look at
//! switch for a small gain; ones that change the picture need a larger one.
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

/// Seconds after a switch before the rebuild check, then seconds the
/// terrain must stay rebuilt (uploads, caches), then seconds counted.
const SETTLE: f32 = 0.5;
const SETTLE_REMESH: f32 = 1.5;
const QUIET: f32 = 1.0;
const SAMPLE: f32 = 2.5;
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
    /// The picture changes: needs a bigger gain.
    Costly,
}

struct Knob {
    name: &'static str,
    cost: Cost,
    remesh: bool,
    get: fn(&GuiOptions) -> u32,
    set: fn(&mut GuiOptions, u32),
    /// Values to try besides the current one.
    values: fn(&GuiOptions) -> Vec<u32>,
    label: fn(u32) -> String,
}

fn on_off(v: u32) -> String {
    if v != 0 { "ON" } else { "OFF" }.to_owned()
}

fn both(_: &GuiOptions) -> Vec<u32> {
    vec![0, 1]
}

macro_rules! toggle {
    ($name:literal, $field:ident, $cost:ident) => {
        Knob {
            name: $name,
            cost: Cost::$cost,
            remesh: false,
            get: |o| u32::from(o.$field),
            set: |o, v| o.$field = v != 0,
            values: both,
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

const KNOBS: &[Knob] = &[
    toggle!("Entity Culling", entity_culling, Free),
    toggle!("Cutout Face Culling", cutout_face_culling, Free),
    toggle!("Angled Face Culling", angled_face_culling, Free),
    toggle!("Exact Face Culling", exact_face_culling, Free),
    toggle!("Per-Face Culling", per_face_culling, Free),
    toggle!("Near-First", near_first, Free),
    toggle!("Half-Rate Occlusion", hiz_every_other, Free),
    toggle!("Quarter-Res Occlusion", hiz_quarter, Costly),
    toggle!("Entity Away-Face Culling", entity_away_faces, Free),
    toggle!("Entity Back Faces Hidden", entity_backface_culling, Costly),
    toggle!("Entity Shadows", entity_shadows, Costly),
    Knob {
        name: "Leaf Culling",
        cost: Cost::Free,
        remesh: true,
        get: |o| o.leaf_culling,
        set: |o, v| o.leaf_culling = v,
        values: |_| vec![0, 1, 2],
        label: |v| ["Off", "Smart", "Full"][v.min(2) as usize].to_owned(),
    },
    Knob {
        name: "Leaves",
        cost: Cost::Costly,
        remesh: true,
        get: |o| o.leaves,
        set: |o, v| o.leaves = v,
        values: |_| vec![0, 1],
        label: |v| ["Fancy", "Fast", "Hybrid"][v.min(2) as usize].to_owned(),
    },
    Knob {
        name: "Smooth Lighting",
        cost: Cost::Costly,
        remesh: true,
        get: |o| u32::from(o.smooth_lighting),
        set: |o, v| o.smooth_lighting = v != 0,
        values: both,
        label: on_off,
    },
    Knob {
        name: "Greedy Meshing",
        cost: Cost::Costly,
        remesh: true,
        get: |o| o.greedy_meshing,
        set: |o, v| o.greedy_meshing = v,
        // Aggressive is experimental: never picked for the user.
        values: |_| vec![0, 1],
        label: |v| ["OFF", "Standard", "Aggressive"][v.min(2) as usize].to_owned(),
    },
    Knob {
        name: "Render Scale",
        cost: Cost::Costly,
        remesh: false,
        get: |o| o.pack_render_scale,
        set: |o, v| o.pack_render_scale = v,
        values: |o| {
            [100, 85, 70]
                .into_iter()
                .filter(|&v| v <= o.pack_render_scale && v >= crate::gui::PACK_RENDER_SCALE_MIN)
                .collect()
        },
        label: |v| format!("{v}%"),
    },
    Knob {
        name: "Entity Distance",
        cost: Cost::Costly,
        remesh: false,
        get: |o| o.entity_distance,
        set: |o, v| o.entity_distance = v,
        values: |o| (o.entity_distance.saturating_sub(2).max(2)..=o.entity_distance).collect(),
        label: |v| format!("{}%", v * 25),
    },
    Knob {
        name: "Clouds",
        cost: Cost::Costly,
        remesh: false,
        get: |o| match o.clouds {
            CloudStatus::Off => 0,
            CloudStatus::Fast => 1,
            CloudStatus::Fancy => 2,
        },
        set: |o, v| o.clouds = cloud(v),
        values: |_| vec![0, 1, 2],
        label: |v| cloud(v).caption().to_owned(),
    },
    Knob {
        name: "Particles",
        cost: Cost::Costly,
        remesh: false,
        get: |o| match o.particles {
            ParticleStatus::All => 0,
            ParticleStatus::Decreased => 1,
            ParticleStatus::Minimal => 2,
        },
        set: |o, v| o.particles = particle(v),
        values: |_| vec![0, 1, 2],
        label: |v| ["All", "Decreased", "Minimal"][v.min(2) as usize].to_owned(),
    },
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
}

#[derive(Clone, Copy)]
struct Row {
    status: Status,
    /// Review: the change is kept.
    keep: bool,
}

/// One candidate's A/B.
struct Trial {
    candidate: u32,
    step: usize,
    switched: Instant,
    /// When the terrain was last seen rebuilt, for `QUIET`.
    quiet_since: Option<Instant>,
    /// Waiting on a rebuild, for the checklist.
    waiting: bool,
    retries: u32,
    frames: Vec<f32>,
    base: Vec<f32>,
    cand: Vec<f32>,
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Running,
    Review,
}

#[derive(Default)]
pub struct TuneState {
    phase: Phase,
    original: Option<GuiOptions>,
    rows: Vec<Row>,
    knob: usize,
    base: u32,
    /// Candidates left for the current knob, and the best so far
    /// (value, frame time change).
    queue: Vec<u32>,
    best: Option<(u32, f32)>,
    worst_change: f32,
    trial: Option<Trial>,
    last_frame: Option<Instant>,
}

impl TuneState {
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
}

/// Starts a tune: backs the options up and opens the checklist.
pub fn start(state: &mut GuiState) {
    let original = state.options.clone();
    crate::platform::storage::TUNE_BACKUP.store(&crate::gui::options_json(&original));
    state.tune = TuneState {
        phase: Phase::Running,
        original: Some(original),
        rows: KNOBS
            .iter()
            .map(|_| Row {
                status: Status::Pending,
                keep: true,
            })
            .collect(),
        knob: usize::MAX,
        ..Default::default()
    };
    RUNNING.store(true, Ordering::Relaxed);
    take_gpu();
    next_knob(state);
    state.nav = Some(Screen::TuneSettings);
}

fn apply_live(state: &GuiState) {
    crate::gui::publish_unsaved(&state.options);
}

/// Moves to the next knob with something to try, or to the review.
fn next_knob(state: &mut GuiState) {
    let t = &mut state.tune;
    t.knob = t.knob.wrapping_add(1);
    while t.knob < KNOBS.len() {
        let k = &KNOBS[t.knob];
        let base = (k.get)(&state.options);
        let queue: Vec<u32> = (k.values)(&state.options)
            .into_iter()
            .filter(|&v| v != base)
            .collect();
        if !queue.is_empty() {
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
        t.knob += 1;
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
    t.trial = Some(Trial {
        candidate,
        step: 0,
        switched: Instant::now(),
        quiet_since: None,
        waiting: true,
        retries: 0,
        frames: Vec::new(),
        base: Vec::new(),
        cand: Vec::new(),
    });
    set_side(state);
}

/// Sets the knob to the side the trial's step calls for.
fn set_side(state: &mut GuiState) {
    let t = &state.tune;
    let Some(trial) = &t.trial else { return };
    let value = if ORDER[trial.step] {
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
    t.rows[t.knob].status = Status::Done {
        from: t.base,
        to: chosen,
        change,
    };
    (k.set)(&mut state.options, chosen);
    apply_live(state);
    next_knob(state);
}

fn terrain_busy() -> bool {
    crate::CHUNK_Q.get().is_some_and(|q| q.busy())
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

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// One frame of measuring: counts the frame, and moves on when a step's
/// window is full.
fn step(state: &mut GuiState) {
    let now = Instant::now();
    let dt = state
        .tune
        .last_frame
        .replace(now)
        .map(|at| now.duration_since(at).as_secs_f32() * 1000.0);
    let vsync = state.options.vsync;
    let t = &mut state.tune;
    let k = &KNOBS[t.knob];
    let Some(trial) = &mut t.trial else { return };
    let since = now.duration_since(trial.switched).as_secs_f32();
    let settle = if k.remesh { SETTLE_REMESH } else { SETTLE };
    // Frames count only once the terrain has rebuilt and stayed so.
    let rebuilding = since < settle || (terrain_busy() && since < MAX_WAIT);
    if rebuilding {
        trial.quiet_since = None;
    }
    let quiet = *trial.quiet_since.get_or_insert(now);
    let start = now.duration_since(quiet).as_secs_f32();
    trial.waiting = rebuilding || start < QUIET;
    if trial.waiting {
        trial.frames.clear();
        take_gpu();
        return;
    }
    if let Some(ms) = dt {
        trial.frames.push(ms);
    }
    if start < QUIET + SAMPLE {
        return;
    }
    // A hitch in the window (a stray rebuild, a GC): measure again.
    let spread = spread(&mut trial.frames);
    if spread > SPREAD_MAX && trial.retries < RETRIES {
        trial.retries += 1;
        trial.quiet_since = Some(now);
        trial.frames.clear();
        take_gpu();
        return;
    }
    trial.retries = 0;
    // Under vsync frames pace to the display: the GPU's time tells more.
    let gpu = take_gpu();
    let score = match gpu {
        Some(g) if vsync => g,
        _ => trimmed_mean(&mut trial.frames),
    };
    trial.frames.clear();
    if ORDER[trial.step] {
        trial.cand.push(score);
    } else {
        trial.base.push(score);
    }
    trial.step += 1;
    trial.switched = now;
    trial.quiet_since = None;
    if trial.step < ORDER.len() {
        set_side(state);
        return;
    }
    let base = median(&mut trial.base);
    let cand = median(&mut trial.cand);
    let change = if base > 0.0 {
        (cand - base) / base
    } else {
        0.0
    };
    let need = match k.cost {
        Cost::Free => GAIN_FREE,
        Cost::Costly => GAIN_COSTLY,
    };
    // Every candidate window must beat every base one, or it was noise.
    let fmax = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max);
    let fmin = |v: &[f32]| v.iter().copied().fold(f32::MAX, f32::min);
    let consistent = fmax(&trial.cand) < fmin(&trial.base);
    if -change >= need && consistent && t.best.is_none_or(|b| change < b.1) {
        t.best = Some((trial.candidate, change));
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
    if back {
        state.nav = Some(Screen::VideoSettings);
    }
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

/// A row's line: its name, what was found, in the status's colour.
fn row_line(i: usize, row: &Row, t: &TuneState) -> (String, u32) {
    let k = &KNOBS[i];
    match row.status {
        Status::Pending => (format!("  {}", k.name), GRAY),
        Status::Testing => {
            let detail = t.trial.as_ref().map_or(String::new(), |tr| {
                let side = if ORDER[tr.step.min(ORDER.len() - 1)] {
                    (k.label)(tr.candidate)
                } else {
                    (k.label)(t.base)
                };
                let wait = if tr.waiting {
                    ", waiting for rebuild"
                } else {
                    ""
                };
                let again = if tr.retries > 0 { ", hitch, again" } else { "" };
                format!(": {side} ({}/{}{wait}{again})", tr.step + 1, ORDER.len())
            });
            (format!("> {}{detail}", k.name), YELLOW)
        }
        Status::Done { from, to, change } if from != to => (
            format!(
                "+ {}: {} -> {} ({})",
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

/// The checklist: live while measuring, then the changes to keep or drop
/// under Apply and Cancel.
pub fn draw(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    crate::gui::options::draw_title(p, ctx, TITLE);
    match state.tune.phase {
        Phase::Idle => {
            state.nav = Some(Screen::VideoSettings);
        }
        Phase::Running => {
            step(state);
            draw_running(p, state, ctx);
        }
        Phase::Review => draw_review(p, state, ctx),
    }
}

fn line_h(_: &Painter) -> f32 {
    crate::text::LINE_HEIGHT + 2.0
}

fn draw_running(p: &mut Painter, state: &GuiState, ctx: &ScreenCtx) {
    let t = &state.tune;
    let left = ((ctx.vw - BIG_W) / 2.0).floor();
    let mut y = LAYOUT.title_y() + 14.0;
    let done = t
        .rows
        .iter()
        .filter(|r| matches!(r.status, Status::Done { .. }))
        .count();
    let head = format!(
        "Testing {} of {}. Input is off until it finishes.",
        (done + 1).min(KNOBS.len()),
        KNOBS.len()
    );
    p.text_str(&head, left, y, AQUA, true);
    y += line_h(p) + 4.0;
    let lines: Vec<(String, u32)> = t
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| row_line(i, r, t))
        .collect();
    draw_columns(p, ctx, y, &lines);
}

/// Lines top to bottom, in two columns when one doesn't fit.
fn draw_columns(p: &mut Painter, ctx: &ScreenCtx, top: f32, lines: &[(String, u32)]) {
    let lh = line_h(p);
    let room = (LAYOUT.footer_y(ctx.vh) - top).max(lh);
    let per = ((room / lh).floor() as usize).max(1);
    let columns = lines.len().div_ceil(per).max(1);
    let col_w = if columns == 1 { BIG_W } else { 210.0 };
    let start = ((ctx.vw - col_w * columns as f32) / 2.0).floor().max(4.0);
    for (i, (text, color)) in lines.iter().enumerate() {
        let x = start + (i / per) as f32 * col_w;
        let y = top + (i % per) as f32 * lh;
        p.text_str(text, x, y, *color, true);
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

fn draw_review(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let half = (WIDGET_WIDTH_BIG / 2.0 - 2.0).floor();
    let left = ((ctx.vw - WIDGET_WIDTH_BIG) / 2.0).floor();
    let top = LAYOUT.title_y() + 12.0;
    if colored_button(p, ctx, left, top, half, "Apply", GREEN) {
        end(state, true, true);
        return;
    }
    if colored_button(p, ctx, left + half + 4.0, top, half, "Cancel", RED) {
        end(state, false, true);
        return;
    }
    let mut y = top + WIDGET_HEIGHT + 6.0;
    let changed: Vec<usize> = (0..KNOBS.len())
        .filter(
            |&i| matches!(state.tune.rows[i].status, Status::Done { from, to, .. } if from != to),
        )
        .collect();
    if changed.is_empty() {
        p.text_str(
            "Nothing made frames cheaper: your settings stay.",
            left,
            y,
            WHITE,
            true,
        );
        y += line_h(p) + 4.0;
    }
    let button_w = 60.0;
    let row_w = BIG_W.max(WIDGET_WIDTH_BIG);
    let row_left = ((ctx.vw - row_w) / 2.0).floor();
    for &i in &changed {
        let (text, _) = row_line(i, &state.tune.rows[i], &state.tune);
        let keep = state.tune.rows[i].keep;
        p.text_str(
            text.trim_start_matches("+ "),
            row_left,
            y + 6.0,
            if keep { GREEN } else { GRAY },
            true,
        );
        let label = if keep { "Keep" } else { "Discard" };
        let color = if keep { GREEN } else { RED };
        if colored_button(
            p,
            ctx,
            row_left + row_w - button_w,
            y,
            button_w,
            label,
            color,
        ) {
            state.tune.rows[i].keep = !keep;
        }
        y += WIDGET_HEIGHT + 2.0;
    }
    y += 4.0;
    let kept: Vec<(String, u32)> = (0..KNOBS.len())
        .filter(|i| !changed.contains(i))
        .map(|i| row_line(i, &state.tune.rows[i], &state.tune))
        .map(|(s, _)| (s, GRAY))
        .collect();
    draw_columns(p, ctx, y, &kept);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_knob_sets_what_it_reads() {
        let mut o = GuiOptions::default();
        for k in KNOBS {
            for v in (k.values)(&o) {
                (k.set)(&mut o, v);
                assert_eq!((k.get)(&o), v, "{}", k.name);
            }
        }
    }
}
