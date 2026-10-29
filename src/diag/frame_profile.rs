//! Live per-frame timings for the F3 profiler page (F3+7).
//!
//! Three sources, averaged per frame over half-second windows:
//! - the main world's schedules, timed by probe schedules slotted between
//!   them in [`MainScheduleOrder`];
//! - the render world's stages, timed by probe systems on the boundaries of
//!   Bevy's chained [`RenderSystems`] sets;
//! - named CPU spans, from every `prof_span!` (including worker threads).
//!
//! GPU and render-graph pass times come from Bevy's render diagnostics and are
//! read straight from the [`DiagnosticsStore`] when the page is drawn.
//!
//! Everything is a single atomic load while the page is closed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, LazyLock, Mutex};

use bevy::app::MainScheduleOrder;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::platform::time::{Instant, epoch};

const WINDOW_NS: u64 = 500_000_000;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    ENABLED.load(Relaxed)
}

/// Turned on while the profiler page shows; turning it on starts a fresh
/// window so stale probe stamps never count.
pub fn set_enabled(on: bool) {
    if ENABLED.swap(on, Relaxed) != on {
        MAIN_AT.store(0, Relaxed);
        RENDER_AT.store(0, Relaxed);
        if let Ok(mut window) = WINDOW.lock() {
            *window = Window::default();
        }
    }
}

fn now_ns() -> u64 {
    Instant::now().duration_since(epoch()).as_nanos() as u64
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Group {
    /// Main-world schedules, per frame.
    Main,
    /// Render-world stages, per rendered frame.
    Render,
    /// Named `prof_span!` regions, summed over every thread, per frame.
    Span,
}

#[derive(Default, Clone, Copy)]
struct Accum {
    total_ns: u64,
    max_ns: u64,
    /// Order of first appearance, to keep pipeline stages in pipeline order.
    first_seen: u32,
}

#[derive(Default)]
struct Window {
    started_ns: u64,
    frames: u32,
    render_frames: u32,
    seen: u32,
    sums: HashMap<(Group, &'static str), Accum>,
}

/// One row of the page: average and worst time per frame in the last window.
#[derive(Clone, Debug)]
pub struct Line {
    pub group: Group,
    pub name: &'static str,
    pub avg_ms: f32,
    pub max_ms: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub frames: u32,
    pub render_frames: u32,
    pub window_ms: f32,
    pub lines: Vec<Line>,
}

static WINDOW: LazyLock<Mutex<Window>> = LazyLock::new(Mutex::default);
static REPORT: Mutex<Option<Arc<Report>>> = Mutex::new(None);

pub fn report() -> Option<Arc<Report>> {
    REPORT.lock().ok()?.clone()
}

pub fn record(group: Group, name: &'static str, ns: u64) {
    if !enabled() {
        return;
    }
    let Ok(mut window) = WINDOW.lock() else {
        return;
    };
    let window = &mut *window;
    let accum = window.sums.entry((group, name)).or_insert_with(|| {
        window.seen += 1;
        Accum {
            first_seen: window.seen,
            ..Accum::default()
        }
    });
    accum.total_ns += ns;
    accum.max_ns = accum.max_ns.max(ns);
}

/// Closes a frame; publishes and restarts the window once it is long enough.
fn frame_done(now: u64) {
    let Ok(mut window) = WINDOW.lock() else {
        return;
    };
    if window.started_ns == 0 {
        window.started_ns = now;
        return;
    }
    window.frames += 1;
    if now - window.started_ns < WINDOW_NS {
        return;
    }
    let frames = window.frames.max(1);
    let render_frames = window.render_frames.max(1);
    let mut lines: Vec<(u32, Line)> = window
        .sums
        .iter()
        .map(|(&(group, name), accum)| {
            let per = match group {
                Group::Render => render_frames,
                Group::Main | Group::Span => frames,
            };
            (
                accum.first_seen,
                Line {
                    group,
                    name,
                    avg_ms: accum.total_ns as f32 / per as f32 / 1e6,
                    max_ms: accum.max_ns as f32 / 1e6,
                },
            )
        })
        .collect();
    let rank = |g: Group| match g {
        Group::Main => 0,
        Group::Render => 1,
        Group::Span => 2,
    };
    lines.sort_by(|a, b| {
        rank(a.1.group)
            .cmp(&rank(b.1.group))
            .then_with(|| match a.1.group {
                // Spans by cost; stages in the order the pipeline runs them, which
                // first appearance is not when a window opens mid-frame.
                Group::Span => b.1.avg_ms.total_cmp(&a.1.avg_ms),
                Group::Main | Group::Render => stage_order(a.1.name)
                    .cmp(&stage_order(b.1.name))
                    .then(a.0.cmp(&b.0)),
            })
    });
    let report = Report {
        frames: window.frames,
        render_frames: window.render_frames,
        window_ms: (now - window.started_ns) as f32 / 1e6,
        lines: lines.into_iter().map(|(_, line)| line).collect(),
    };
    *window = Window {
        started_ns: now,
        ..Window::default()
    };
    drop(window);
    if let Ok(mut slot) = REPORT.lock() {
        *slot = Some(Arc::new(report));
    }
}

const MAIN_STAGES: [&str; 6] = [
    "First",
    "PreUpdate",
    "Update",
    "PostUpdate",
    "Last",
    "Between frames",
];

const RENDER_STAGES: [&str; 9] = [
    "Extract commands",
    "Prepare assets",
    "Prepare meshes",
    "Manage views",
    "Queue",
    "Phase sort",
    "Prepare (buffers, bind groups)",
    "Render graph (encode, submit)",
    "Cleanup",
];

fn stage_order(name: &str) -> usize {
    MAIN_STAGES
        .iter()
        .chain(&RENDER_STAGES)
        .position(|s| *s == name)
        .unwrap_or(usize::MAX)
}

/// A `prof_span!` region's timer; records on drop while the page is open.
pub struct Span {
    name: &'static str,
    start: Instant,
}

pub fn span(name: &'static str) -> Option<Span> {
    enabled().then(|| Span {
        name,
        start: Instant::now(),
    })
}

impl Drop for Span {
    fn drop(&mut self) {
        record(
            Group::Span,
            self.name,
            self.start.elapsed().as_nanos() as u64,
        );
    }
}

static MAIN_AT: AtomicU64 = AtomicU64::new(0);
static RENDER_AT: AtomicU64 = AtomicU64::new(0);

/// Charges the time since the previous stamp to `stage`.
fn stamp(at: &AtomicU64, group: Group, stage: Option<&'static str>) -> u64 {
    let now = now_ns();
    let prev = at.swap(now, Relaxed);
    if prev != 0
        && let Some(stage) = stage
    {
        record(group, stage, now - prev);
    }
    now
}

#[derive(ScheduleLabel, Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum MainProbe {
    Start,
    AfterFirst,
    AfterPreUpdate,
    AfterUpdate,
    AfterPostUpdate,
    AfterLast,
}

fn main_probe(probe: MainProbe) {
    if !enabled() {
        return;
    }
    let stage = match probe {
        // From the end of `Last` to the next `First`: winit events, render
        // extraction and frame pacing all happen out here.
        MainProbe::Start => Some("Between frames"),
        MainProbe::AfterFirst => Some("First"),
        MainProbe::AfterPreUpdate => Some("PreUpdate"),
        MainProbe::AfterUpdate => Some("Update"),
        MainProbe::AfterPostUpdate => Some("PostUpdate"),
        MainProbe::AfterLast => Some("Last"),
    };
    let now = stamp(&MAIN_AT, Group::Main, stage);
    if probe == MainProbe::Start {
        frame_done(now);
    }
}

fn render_probe(stage: Option<&'static str>) {
    if !enabled() {
        return;
    }
    if stage.is_none() {
        // The render frame's first probe: the gap before it is the render
        // thread waiting for the next frame, not work.
        RENDER_AT.store(now_ns(), Relaxed);
        if let Ok(mut window) = WINDOW.lock() {
            window.render_frames += 1;
        }
        return;
    }
    stamp(&RENDER_AT, Group::Render, stage);
}

/// Wraps a custom render-graph node in a diagnostics span, so its CPU and GPU
/// time show next to Bevy's own passes.
pub fn timed<'w, R>(
    render_context: &mut bevy::render::renderer::RenderContext<'w>,
    name: &'static str,
    body: impl FnOnce(&mut bevy::render::renderer::RenderContext<'w>) -> R,
) -> R {
    use bevy::render::diagnostic::RecordDiagnostics;
    let diagnostics = render_context.diagnostic_recorder();
    let span = diagnostics.time_span(render_context.command_encoder(), name);
    let out = body(render_context);
    span.end(render_context.command_encoder());
    out
}

pub struct FrameProfilePlugin;

impl Plugin for FrameProfilePlugin {
    fn build(&self, app: &mut App) {
        use MainProbe::*;
        for probe in [
            Start,
            AfterFirst,
            AfterPreUpdate,
            AfterUpdate,
            AfterPostUpdate,
            AfterLast,
        ] {
            app.add_systems(probe, move || main_probe(probe));
        }
        {
            let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
            order.insert_before(First, Start);
            order.insert_after(First, AfterFirst);
            order.insert_after(PreUpdate, AfterPreUpdate);
            order.insert_after(Update, AfterUpdate);
            order.insert_after(PostUpdate, AfterPostUpdate);
            order.insert_after(Last, AfterLast);
        }

        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        use RenderSystems as S;
        render_app.add_systems(
            Render,
            (
                (|| render_probe(None)).before(S::ExtractCommands),
                (|| render_probe(Some("Extract commands")))
                    .after(S::ExtractCommands)
                    .before(S::PrepareAssets),
                (|| render_probe(Some("Prepare assets")))
                    .after(S::PrepareAssets)
                    .before(S::PrepareMeshes),
                (|| render_probe(Some("Prepare meshes")))
                    .after(S::PrepareMeshes)
                    .before(S::ManageViews),
                (|| render_probe(Some("Manage views")))
                    .after(S::ManageViews)
                    .before(S::Queue),
                (|| render_probe(Some("Queue")))
                    .after(S::Queue)
                    .before(S::PhaseSort),
                (|| render_probe(Some("Phase sort")))
                    .after(S::PhaseSort)
                    .before(S::Prepare),
                (|| render_probe(Some("Prepare (buffers, bind groups)")))
                    .after(S::Prepare)
                    .before(S::Render),
                (|| render_probe(Some("Render graph (encode, submit)")))
                    .after(S::Render)
                    .before(S::Cleanup),
                (|| render_probe(Some("Cleanup")))
                    .after(S::Cleanup)
                    .before(S::PostCleanup),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The profiler is process-global; its tests take turns.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn a_window_averages_per_frame_and_keeps_pipeline_order() {
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        set_enabled(false);
        set_enabled(true);
        let start = 1_000;
        frame_done(start);
        for _ in 0..4 {
            record(Group::Main, "PreUpdate", 1_000_000);
            record(Group::Main, "Update", 3_000_000);
            record(Group::Span, "render:a", 500_000);
            record(Group::Span, "render:b", 2_000_000);
        }
        for i in 1..=3 {
            frame_done(start + i * 100_000_000);
        }
        frame_done(start + WINDOW_NS + 1);
        let report = report().expect("a full window publishes");
        assert_eq!(report.frames, 4);
        let names: Vec<_> = report.lines.iter().map(|l| l.name).collect();
        assert_eq!(names, ["PreUpdate", "Update", "render:b", "render:a"]);
        let update = &report.lines[1];
        assert!((update.avg_ms - 3.0).abs() < 1e-4, "{}", update.avg_ms);
        assert!((update.max_ms - 3.0).abs() < 1e-4);
        set_enabled(false);
    }

    #[test]
    fn stages_keep_pipeline_order_whatever_came_first() {
        let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        set_enabled(false);
        set_enabled(true);
        frame_done(1_000);
        record(Group::Render, "Queue", 1);
        record(Group::Render, "Prepare assets", 1);
        record(Group::Main, "Last", 1);
        record(Group::Main, "First", 1);
        frame_done(1_000 + WINDOW_NS + 1);
        let report = report().expect("a full window publishes");
        let names: Vec<_> = report.lines.iter().map(|l| l.name).collect();
        assert_eq!(names, ["First", "Last", "Prepare assets", "Queue"]);
        set_enabled(false);
    }
}
