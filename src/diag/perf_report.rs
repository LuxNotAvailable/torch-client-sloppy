//! Performance reports (`perf_report` builds): the pause menu's Profile
//! button closes the menu, lets the frame rate settle, records ten seconds
//! of play and writes what it saw to `profiles/` in the client's folder, as
//! text to read and JSON to compare across machines.
//!
//! Every frame it records:
//! - the frame time, for frame rate, lows and stutters;
//! - each main schedule, render stage and `prof_span!` region
//!   ([`frame_profile`]), so anything timed anywhere shows up;
//! - each value in Bevy's diagnostics store measured lately (GPU and CPU
//!   time per render pass, among others);
//! - what the frame drew: entities, players, terrain draws and memory.
//!
//! Four times a second it samples the process (CPU and memory), and over the
//! whole recording it takes the change in the pipeline counters (chunks
//! received, sections meshed and so on). Each series is summarised as its
//! average, low, high and percentiles. The report opens with the machine:
//! OS, CPU, memory, GPU and driver, display, the build and the settings.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;
use std::time::Duration;

use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::frame_profile::{self, Group};
use super::stats::{Counts, Stat};
use crate::platform::time::Instant;

/// How long the frame rate settles after the menu closes.
const SETTLE: Duration = Duration::from_secs(2);
/// How long a report records.
pub const RECORD: Duration = Duration::from_secs(10);
/// How often the process is sampled.
const PROCESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Section {
    Main,
    Render,
    Spans,
    Diagnostics,
    World,
    Process,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Main => "Main schedules (ms per frame)",
            Section::Render => "Render stages (ms per frame)",
            Section::Spans => "Timed spans (ms per frame, all threads)",
            Section::Diagnostics => "Bevy diagnostics (render passes in ms)",
            Section::World => "What each frame drew",
            Section::Process => "Process (sampled 4 times a second)",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Section::Main => "main",
            Section::Render => "render",
            Section::Spans => "spans",
            Section::Diagnostics => "diagnostics",
            Section::World => "world",
            Section::Process => "process",
        }
    }
}

enum Phase {
    Settling { until: Instant },
    Recording { started: Instant, until: Instant },
}

struct Capture {
    phase: Phase,
    frame_ms: Vec<f32>,
    series: BTreeMap<(Section, String), Vec<f32>>,
    counts_at_start: Counts,
    traffic_at_start: (u64, u64),
    system: sysinfo::System,
    pid: Option<sysinfo::Pid>,
    last_process: Instant,
}

static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

/// Starts a report unless one is running.
pub fn start() {
    let Ok(mut slot) = CAPTURE.lock() else {
        return;
    };
    if slot.is_some() {
        return;
    }
    *slot = Some(Capture {
        phase: Phase::Settling {
            until: Instant::now() + SETTLE,
        },
        frame_ms: Vec::with_capacity(4096),
        series: BTreeMap::new(),
        counts_at_start: Counts::default(),
        traffic_at_start: (0, 0),
        system: sysinfo::System::new(),
        pid: sysinfo::get_current_pid().ok(),
        last_process: Instant::now(),
    });
}

/// What a running report is doing, for the button's caption.
pub fn status() -> Option<String> {
    let slot = CAPTURE.lock().ok()?;
    let now = Instant::now();
    Some(match &slot.as_ref()?.phase {
        Phase::Settling { .. } => "Profiling: settling...".to_owned(),
        Phase::Recording { until, .. } => format!(
            "Profiling: {}s left",
            until.saturating_duration_since(now).as_secs() + 1
        ),
    })
}

pub struct PerfReportPlugin;

impl Plugin for PerfReportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Last, sample);
    }
}

/// What the report reads about the machine and settings when it finishes.
#[derive(bevy::ecs::system::SystemParam)]
struct Context<'w, 's> {
    window: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    adapter: Option<Res<'w, bevy::render::renderer::RenderAdapterInfo>>,
    state: Res<'w, crate::gui::GuiState>,
    shared: Res<'w, crate::renderer::Shared>,
}

fn sample(
    time: Res<Time<Real>>,
    diagnostics: Option<Res<DiagnosticsStore>>,
    view: Res<crate::renderer::frame_view::FrameView>,
    context: Context,
) {
    let Ok(mut slot) = CAPTURE.lock() else {
        return;
    };
    let Some(capture) = slot.as_mut() else {
        return;
    };
    let now = Instant::now();
    match capture.phase {
        Phase::Settling { until } => {
            if now >= until {
                frame_profile::set_capturing(true);
                capture.counts_at_start = super::counts();
                capture.traffic_at_start = traffic();
                capture.phase = Phase::Recording {
                    started: now,
                    until: now + RECORD,
                };
                toast(
                    &context.shared,
                    "Profiling",
                    "Recording 10 seconds: keep playing",
                );
            }
        }
        Phase::Recording { started, until } => {
            capture.frame_ms.push(time.delta_secs() * 1000.0);
            capture.record_frame(diagnostics.as_deref(), &view);
            if now.duration_since(capture.last_process) >= PROCESS_EVERY {
                capture.last_process = now;
                capture.record_process();
            }
            if now >= until {
                frame_profile::set_capturing(false);
                let capture = slot.take().expect("checked above");
                let secs = now.duration_since(started).as_secs_f32();
                finish(capture, secs, &context);
            }
        }
    }
}

impl Capture {
    fn push(&mut self, section: Section, name: impl Into<String>, value: f32) {
        self.series
            .entry((section, name.into()))
            .or_default()
            .push(value);
    }

    fn record_frame(
        &mut self,
        diagnostics: Option<&DiagnosticsStore>,
        view: &crate::renderer::frame_view::FrameView,
    ) {
        for (group, name, ns) in frame_profile::last_frame() {
            let section = match group {
                Group::Main => Section::Main,
                Group::Render => Section::Render,
                Group::Span => Section::Spans,
            };
            self.push(section, name, ns as f32 / 1e6);
        }
        if let Some(store) = diagnostics {
            for diagnostic in store.iter() {
                // A value nothing measured lately (a pass that stopped
                // running) would only repeat its last reading.
                let Some(m) = diagnostic
                    .measurement()
                    .filter(|m| m.time.elapsed() < Duration::from_secs(1))
                else {
                    continue;
                };
                let name = match diagnostic.suffix.trim() {
                    "" => diagnostic.path().as_str().to_owned(),
                    unit => format!("{} ({unit})", diagnostic.path().as_str()),
                };
                self.push(Section::Diagnostics, name, m.value as f32);
            }
        }
        self.push(Section::World, "entities", view.entities.len() as f32);
        self.push(Section::World, "other players", view.players.len() as f32);
        if let Some(terrain) = crate::renderer::terrain_pool::stats() {
            use std::sync::atomic::Ordering::Relaxed;
            let t = &terrain.0;
            self.push(
                Section::World,
                "terrain sections resident",
                t.live_slots.load(Relaxed) as f32,
            );
            self.push(
                Section::World,
                "terrain sections drawn",
                t.drawn.load(Relaxed) as f32,
            );
            self.push(
                Section::World,
                "terrain draw calls",
                t.draws.load(Relaxed) as f32,
            );
            self.push(
                Section::World,
                "terrain buffer used (MiB)",
                t.used_bytes.load(Relaxed) as f32 / 1048576.0,
            );
        }
    }

    fn record_process(&mut self) {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
        self.system.refresh_cpu_usage();
        self.push(
            Section::Process,
            "machine CPU (%)",
            self.system.global_cpu_usage(),
        );
        let Some(pid) = self.pid else {
            return;
        };
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        if let Some((cpu, memory)) = self
            .system
            .process(pid)
            .map(|p| (p.cpu_usage(), p.memory()))
        {
            // One core flat out is 100%.
            self.push(Section::Process, "client CPU (% of one core)", cpu);
            self.push(
                Section::Process,
                "client memory (MiB)",
                memory as f32 / 1048576.0,
            );
        }
    }
}

fn traffic() -> (u64, u64) {
    (
        azalea_protocol::traffic::rx(),
        azalea_protocol::traffic::tx(),
    )
}

fn toast(shared: &crate::renderer::Shared, title: &str, message: &str) {
    use crate::gui::toast::{SystemId, ToastEvent};
    let plain = |s: &str| crate::text::styled(s, crate::text::Style::default());
    crate::session::queue_toasts(
        &shared.0,
        [ToastEvent::System {
            id: SystemId::PeriodicNotification,
            title: plain(title),
            message: plain(message),
            multiline: true,
        }],
    );
}

/// Average, extremes, spread and percentiles of one series.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Summary {
    n: usize,
    avg: f32,
    min: f32,
    max: f32,
    p1: f32,
    p5: f32,
    p50: f32,
    p95: f32,
    p99: f32,
    std: f32,
}

fn summarise(values: &[f32]) -> Option<Summary> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f32::total_cmp);
    let n = sorted.len();
    let at = |q: f32| sorted[((q * (n - 1) as f32).round() as usize).min(n - 1)];
    let avg = values.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let var = values
        .iter()
        .map(|&v| (v as f64 - avg).powi(2))
        .sum::<f64>()
        / n as f64;
    Some(Summary {
        n,
        avg: avg as f32,
        min: sorted[0],
        max: sorted[n - 1],
        p1: at(0.01),
        p5: at(0.05),
        p50: at(0.5),
        p95: at(0.95),
        p99: at(0.99),
        std: var.sqrt() as f32,
    })
}

/// Frame rate as players quote it, from per-frame times in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FrameRate {
    avg_fps: f32,
    /// The average frame rate over the slowest 1% and 0.1% of frames.
    low_1: f32,
    low_01: f32,
    min_fps: f32,
    max_fps: f32,
    /// Frames over twice the median, and over 50 ms.
    stutters: usize,
    hitches: usize,
}

fn frame_rate(frame_ms: &[f32], secs: f32) -> Option<FrameRate> {
    let s = summarise(frame_ms)?;
    let mut slow = frame_ms.to_vec();
    slow.sort_unstable_by(|a, b| b.total_cmp(a));
    let low = |share: f32| {
        let take = ((slow.len() as f32 * share).ceil() as usize).clamp(1, slow.len());
        1000.0 / (slow[..take].iter().sum::<f32>() / take as f32)
    };
    Some(FrameRate {
        avg_fps: frame_ms.len() as f32 / secs.max(1e-3),
        low_1: low(0.01),
        low_01: low(0.001),
        min_fps: 1000.0 / s.max.max(1e-3),
        max_fps: 1000.0 / s.min.max(1e-3),
        stutters: frame_ms.iter().filter(|&&ms| ms > 2.0 * s.p50).count(),
        hitches: frame_ms.iter().filter(|&&ms| ms > 50.0).count(),
    })
}

/// `(label, value)` rows about the machine, the build and the settings.
fn describe(
    context: &Context,
    system: &mut sysinfo::System,
) -> Vec<(&'static str, Vec<(String, String)>)> {
    system.refresh_memory();
    system.refresh_cpu_all();
    let cpu = system.cpus().first();
    let gib = |bytes: u64| format!("{:.1} GiB", bytes as f64 / 1073741824.0);
    let mut device = vec![
        (
            "os".to_owned(),
            sysinfo::System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_owned()),
        ),
        (
            "kernel".to_owned(),
            sysinfo::System::kernel_version().unwrap_or_default(),
        ),
        ("arch".to_owned(), std::env::consts::ARCH.to_owned()),
        (
            "cpu".to_owned(),
            cpu.map(|c| c.brand().trim().to_owned()).unwrap_or_default(),
        ),
        (
            "cpu vendor".to_owned(),
            cpu.map(|c| c.vendor_id().to_owned()).unwrap_or_default(),
        ),
        (
            "cpu cores".to_owned(),
            format!(
                "{} physical, {} logical",
                system.physical_core_count().unwrap_or(0),
                system.cpus().len()
            ),
        ),
        (
            "cpu clock".to_owned(),
            cpu.map(|c| format!("{} MHz", c.frequency()))
                .unwrap_or_default(),
        ),
        ("memory total".to_owned(), gib(system.total_memory())),
        (
            "memory available".to_owned(),
            gib(system.available_memory()),
        ),
    ];
    if let Some(adapter) = &context.adapter {
        device.extend([
            ("gpu".to_owned(), adapter.name.clone()),
            ("gpu type".to_owned(), format!("{:?}", adapter.device_type)),
            (
                "gpu vendor id".to_owned(),
                format!("{:#06x}", adapter.vendor),
            ),
            (
                "gpu device id".to_owned(),
                format!("{:#06x}", adapter.device),
            ),
            ("graphics api".to_owned(), format!("{:?}", adapter.backend)),
            (
                "driver".to_owned(),
                format!("{} {}", adapter.driver, adapter.driver_info)
                    .trim()
                    .to_owned(),
            ),
        ]);
    }
    if let Ok(window) = context.window.single() {
        device.extend([
            (
                "window".to_owned(),
                format!(
                    "{}x{} physical, scale {}",
                    window.physical_width(),
                    window.physical_height(),
                    window.scale_factor()
                ),
            ),
            (
                "present mode".to_owned(),
                format!("{:?}", window.present_mode),
            ),
            ("window mode".to_owned(), format!("{:?}", window.mode)),
        ]);
    }

    let features: Vec<&str> = [
        ("shader_support", cfg!(feature = "shader_support")),
        ("audio", cfg!(feature = "audio")),
        ("voice", cfg!(feature = "voice")),
        ("skins", cfg!(feature = "skins")),
        ("resourcepacks", cfg!(feature = "resourcepacks")),
        ("hud_editor", cfg!(feature = "hud_editor")),
        ("click_gui", cfg!(feature = "click_gui")),
        ("mobile_ui", cfg!(feature = "mobile_ui")),
        ("profiling", cfg!(feature = "profiling")),
        ("alloc_diag", cfg!(feature = "alloc_diag")),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect();
    let build = vec![
        ("version".to_owned(), env!("CARGO_PKG_VERSION").to_owned()),
        (
            "profile".to_owned(),
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .to_owned(),
        ),
        ("features".to_owned(), features.join(", ")),
    ];

    let o = &context.state.options;
    let fps_cap = if o.max_fps >= crate::gui::MAX_FPS_UNLIMITED {
        "unlimited".to_owned()
    } else {
        o.max_fps.to_string()
    };
    #[cfg(feature = "shader_support")]
    let pack = crate::platform::storage::SHADER_PACK
        .load()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "none".to_owned());
    #[cfg(not(feature = "shader_support"))]
    let pack = "not built".to_owned();
    let settings = vec![
        ("render distance".to_owned(), o.render_distance.to_string()),
        ("fps cap".to_owned(), fps_cap),
        ("vsync".to_owned(), o.vsync.to_string()),
        ("renderer".to_owned(), o.render_backend.caption().to_owned()),
        (
            "antialiasing".to_owned(),
            o.antialiasing.mode.caption().to_owned(),
        ),
        ("clouds".to_owned(), o.clouds.caption().to_owned()),
        ("particles".to_owned(), o.particles.caption().to_owned()),
        ("smooth lighting".to_owned(), o.smooth_lighting.to_string()),
        ("entity shadows".to_owned(), o.entity_shadows.to_string()),
        ("fov".to_owned(), o.fov.to_string()),
        ("gui scale".to_owned(), o.gui_scale.to_string()),
        ("shader pack".to_owned(), pack),
        (
            "dimension".to_owned(),
            crate::renderer::dimension::current()
                .registry_id()
                .to_owned(),
        ),
    ];
    vec![("Device", device), ("Build", build), ("Settings", settings)]
}

fn finish(mut capture: Capture, secs: f32, context: &Context) {
    let about = describe(context, &mut capture.system);
    let counts_end = super::counts();
    let (rx, tx) = traffic();
    let counters: Vec<(String, u64)> = Stat::ALL
        .iter()
        .map(|&stat| {
            (
                stat.name().to_owned(),
                counts_end[stat].saturating_sub(capture.counts_at_start[stat]),
            )
        })
        .chain([
            (
                "bytes received".to_owned(),
                rx.saturating_sub(capture.traffic_at_start.0),
            ),
            (
                "bytes sent".to_owned(),
                tx.saturating_sub(capture.traffic_at_start.1),
            ),
        ])
        .collect();
    let rate = frame_rate(&capture.frame_ms, secs);
    let frame = summarise(&capture.frame_ms);
    let stamp = timestamp();

    let text = render_text(&stamp, secs, &capture, &about, rate, frame, &counters);
    let json = render_json(&stamp, secs, &capture, &about, rate, frame, &counters);

    let dir = crate::platform::storage::dir().join("profiles");
    let file = format!("profile-{}", stamp.replace([':', ' '], "-"));
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join(format!("{file}.txt")), &text))
        .and_then(|()| std::fs::write(dir.join(format!("{file}.json")), json.to_string()));
    match written {
        Ok(()) => {
            crate::log_info!("perf", "report written to {}", dir.join(&file).display());
            let headline = rate
                .map(|r| format!("{:.0} fps avg, {:.0} 1% low", r.avg_fps, r.low_1))
                .unwrap_or_default();
            toast(
                &context.shared,
                "Profile saved",
                &format!("{headline}\nprofiles/{file}.txt"),
            );
        }
        Err(e) => {
            crate::log_warn!("perf", "could not write the report: {e}");
            toast(&context.shared, "Profile not saved", &e.to_string());
        }
    }
}

fn render_text(
    stamp: &str,
    secs: f32,
    capture: &Capture,
    about: &[(&'static str, Vec<(String, String)>)],
    rate: Option<FrameRate>,
    frame: Option<Summary>,
    counters: &[(String, u64)],
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Torch client performance report\nRecorded {stamp} UTC: {secs:.1} s, {} frames\n",
        capture.frame_ms.len()
    );
    for (title, rows) in about {
        let _ = writeln!(out, "== {title} ==");
        let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (k, v) in rows {
            let _ = writeln!(out, "  {k:<width$}  {v}");
        }
        out.push('\n');
    }
    if let (Some(r), Some(f)) = (rate, frame) {
        let _ = writeln!(out, "== Frame rate ==");
        let _ = writeln!(
            out,
            "  average {:.1} fps   1% low {:.1}   0.1% low {:.1}   min {:.1}   max {:.1}",
            r.avg_fps, r.low_1, r.low_01, r.min_fps, r.max_fps
        );
        let _ = writeln!(
            out,
            "  frame time ms: avg {:.2}  p50 {:.2}  p95 {:.2}  p99 {:.2}  max {:.2}  std {:.2}",
            f.avg, f.p50, f.p95, f.p99, f.max, f.std
        );
        let _ = writeln!(
            out,
            "  stutters (over twice the median): {}   hitches (over 50 ms): {}\n",
            r.stutters, r.hitches
        );
    }
    let mut section = None;
    let width = capture
        .series
        .keys()
        .map(|(_, name)| name.len())
        .max()
        .unwrap_or(0)
        .clamp(24, 60);
    for ((sec, name), values) in &capture.series {
        if section != Some(*sec) {
            section = Some(*sec);
            let _ = writeln!(out, "\n== {} ==", sec.title());
            let _ = writeln!(
                out,
                "  {:<width$} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
                "", "avg", "low", "p50", "p95", "p99", "high"
            );
        }
        if let Some(s) = summarise(values) {
            let shown: String = name.chars().take(width).collect();
            let _ = writeln!(
                out,
                "  {shown:<width$} {:>9.3} {:>9.3} {:>9.3} {:>9.3} {:>9.3} {:>9.3}",
                s.avg, s.min, s.p50, s.p95, s.p99, s.max
            );
        }
    }
    let _ = writeln!(
        out,
        "\n== Counters (change over the recording, per second) =="
    );
    for (name, delta) in counters {
        let _ = writeln!(
            out,
            "  {name:<28} {delta:>12} {:>12.1}/s",
            *delta as f32 / secs.max(1e-3)
        );
    }
    out
}

fn render_json(
    stamp: &str,
    secs: f32,
    capture: &Capture,
    about: &[(&'static str, Vec<(String, String)>)],
    rate: Option<FrameRate>,
    frame: Option<Summary>,
    counters: &[(String, u64)],
) -> serde_json::Value {
    use serde_json::{Map, Value, json};
    let summary = |s: Summary| {
        json!({
            "n": s.n, "avg": s.avg, "min": s.min, "max": s.max,
            "p1": s.p1, "p5": s.p5, "p50": s.p50, "p95": s.p95, "p99": s.p99, "std": s.std,
        })
    };
    let mut root = Map::new();
    root.insert("recorded_utc".into(), json!(stamp));
    root.insert("seconds".into(), json!(secs));
    root.insert("frames".into(), json!(capture.frame_ms.len()));
    for (title, rows) in about {
        let rows: Map<String, Value> = rows.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        root.insert(title.to_lowercase(), Value::Object(rows));
    }
    if let (Some(r), Some(f)) = (rate, frame) {
        root.insert(
            "frame_rate".into(),
            json!({
                "avg_fps": r.avg_fps, "low_1_percent": r.low_1, "low_0_1_percent": r.low_01,
                "min_fps": r.min_fps, "max_fps": r.max_fps,
                "stutters": r.stutters, "hitches": r.hitches,
                "frame_ms": summary(f),
            }),
        );
    }
    let mut sections: BTreeMap<&str, Map<String, Value>> = BTreeMap::new();
    for ((sec, name), values) in &capture.series {
        if let Some(s) = summarise(values) {
            sections
                .entry(sec.key())
                .or_default()
                .insert(name.clone(), summary(s));
        }
    }
    for (key, map) in sections {
        root.insert(key.into(), Value::Object(map));
    }
    let counters: Map<String, Value> = counters
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    root.insert("counters".into(), Value::Object(counters));
    Value::Object(root)
}

/// Now as `YYYY-MM-DD HH:MM:SS`, UTC.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's days-to-civil.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_has_the_right_extremes_and_middle() {
        let values: Vec<f32> = (1..=100).map(|v| v as f32).collect();
        let s = summarise(&values).unwrap();
        assert_eq!((s.min, s.max, s.n), (1.0, 100.0, 100));
        assert!((s.avg - 50.5).abs() < 1e-4);
        assert_eq!(s.p50, 51.0);
        assert_eq!(s.p99, 99.0);
        assert!(summarise(&[]).is_none());
    }

    #[test]
    fn lows_come_from_the_slowest_frames() {
        // 990 frames at 10 ms and 10 at 100 ms, over 10.9 s.
        let mut frames = vec![10.0; 990];
        frames.extend([100.0; 10]);
        let r = frame_rate(&frames, 10.9).unwrap();
        assert!((r.low_1 - 10.0).abs() < 1e-3, "{}", r.low_1);
        assert!((r.min_fps - 10.0).abs() < 1e-3);
        assert!((r.max_fps - 100.0).abs() < 1e-3);
        assert_eq!((r.stutters, r.hitches), (10, 10));
    }

    #[test]
    fn timestamps_are_civil_dates() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), 19);
        assert!(stamp.starts_with("20"));
        assert_eq!(&stamp[4..5], "-");
    }
}
