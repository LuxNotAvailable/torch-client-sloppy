//! Torch Mod Converter: a window over the conversion pipeline (egui on
//! wgpu). Pick a folder of NeoForge mod jars and an output folder:
//!
//! - the Mods tab lists every jar with what happens to it: Convert, Load
//!   only (a dependency the dump server needs, not packaged) or Skip;
//!   client-side and performance mods start as Skip;
//! - Start runs torch-convert (dump server setup, conversion), optionally
//!   the port agent per mod, and copies packages into the client;
//! - the Activity tab shows every stage, step and command live, the raw
//!   output below.
//!
//! Runs from the repo (`cargo run --release` here) or from the standalone
//! folder `tools/converter_app/build_dist.py` makes.

#![windows_subsystem = "windows"]

mod jars;
mod layout;
mod runner;
mod tools;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use eframe::egui::{self, Color32, RichText};
use serde_json::Value;

use jars::{Action, Jar};
use layout::Layout;
use runner::{Msg, Plan, Stop};

const EULA_URL: &str = "https://aka.ms/MinecraftEULA";
const LOG_LINES: usize = 5000;
const GREEN: Color32 = Color32::from_rgb(40, 150, 70);
const RED: Color32 = Color32::from_rgb(200, 40, 50);
const BLUE: Color32 = Color32::from_rgb(40, 100, 200);

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Settings {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    #[serde(default = "yes")]
    convert: bool,
    #[serde(default)]
    port: bool,
    #[serde(default)]
    install: bool,
    #[serde(default)]
    port_mods: String,
    /// Chosen actions by jar file name.
    #[serde(default)]
    actions: std::collections::BTreeMap<String, Action>,
}

fn yes() -> bool {
    true
}

#[derive(PartialEq)]
enum Tab {
    Mods,
    Activity,
}

#[derive(Default)]
struct Step {
    label: String,
    detail: String,
    done: bool,
    failed: bool,
    commands: Vec<String>,
}

#[derive(Default)]
struct Stage {
    id: String,
    label: String,
    summary: String,
    finished: bool,
    steps: Vec<Step>,
    commands: Vec<String>,
}

impl Stage {
    fn failed(&self) -> bool {
        self.steps.iter().any(|s| s.failed)
    }
}

struct App {
    layout: Arc<Layout>,
    settings: Settings,
    input: String,
    output: String,
    scanned: String,
    jars: Vec<Jar>,
    tab: Tab,
    reqs: Option<Vec<tools::Req>>,
    reqs_rx: Receiver<Vec<tools::Req>>,
    rx: Option<Receiver<Msg>>,
    stop: Stop,
    started: Option<Instant>,
    finished: Option<(bool, String)>,
    task: String,
    progress: Option<(usize, usize)>,
    stages: Vec<Stage>,
    log: VecDeque<String>,
    ask_eula: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        let layout = Arc::new(Layout::find());
        let settings: Settings = std::fs::read_to_string(&layout.settings)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Settings {
                convert: true,
                ..Default::default()
            });
        let input = settings
            .input
            .clone()
            .unwrap_or_else(|| layout.default_in.clone());
        let output = settings
            .output
            .clone()
            .unwrap_or_else(|| layout.default_out.clone());
        let (reqs_tx, reqs_rx) = channel();
        let (l, ctx) = (layout.clone(), cc.egui_ctx.clone());
        std::thread::spawn(move || {
            let _ = reqs_tx.send(tools::check(&l));
            ctx.request_repaint();
        });
        let mut app = App {
            layout,
            settings,
            input: input.display().to_string(),
            output: output.display().to_string(),
            scanned: String::new(),
            jars: Vec::new(),
            tab: Tab::Mods,
            reqs: None,
            reqs_rx,
            rx: None,
            stop: Stop::default(),
            started: None,
            finished: None,
            task: "Idle".into(),
            progress: None,
            stages: Vec::new(),
            log: VecDeque::new(),
            ask_eula: false,
        };
        app.scan(false);
        app
    }

    fn running(&self) -> bool {
        self.rx.is_some()
    }

    /// Lists the input folder's jars; saved choices win over suggestions.
    fn scan(&mut self, reset: bool) {
        if reset {
            self.settings.actions.clear();
        }
        self.jars = jars::scan(std::path::Path::new(&self.input));
        for j in &mut self.jars {
            if let Some(a) = self.settings.actions.get(&j.name) {
                j.action = *a;
            }
        }
        self.scanned = self.input.clone();
    }

    fn save(&mut self) {
        self.settings.input = Some(PathBuf::from(&self.input));
        self.settings.output = Some(PathBuf::from(&self.output));
        self.settings.actions = self
            .jars
            .iter()
            .map(|j| (j.name.clone(), j.action))
            .collect();
        if let Some(dir) = self.layout.settings.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.settings) {
            let _ = std::fs::write(&self.layout.settings, text);
        }
    }

    fn counts(&self) -> (usize, usize, usize) {
        let n = |a| self.jars.iter().filter(|j| j.action == a).count();
        (n(Action::Convert), n(Action::LoadOnly), n(Action::Skip))
    }

    fn start(&mut self, ctx: &egui::Context, accept_eula: bool) {
        let pick = |a| {
            self.jars
                .iter()
                .filter(|j| j.action == a)
                .map(|j| j.path.clone())
                .collect::<Vec<_>>()
        };
        let plan = Plan {
            input: PathBuf::from(&self.input),
            output: PathBuf::from(&self.output),
            convert: pick(Action::Convert),
            load_only: pick(Action::LoadOnly),
            skipped: self.counts().2,
            do_convert: self.settings.convert,
            do_port: self.settings.port,
            port_mods: self
                .settings
                .port_mods
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            install: self.settings.install,
            accept_eula,
        };
        self.save();
        self.stages.clear();
        self.log.clear();
        self.finished = None;
        self.progress = None;
        self.task = "Starting...".into();
        self.started = Some(Instant::now());
        self.tab = Tab::Activity;
        self.stop = Stop::default();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        runner::start(
            self.layout.clone(),
            plan,
            tx,
            self.stop.clone(),
            move || ctx.request_repaint(),
        );
    }

    fn try_start(&mut self, ctx: &egui::Context) {
        if self.counts().0 == 0 {
            self.task = "No jar is set to Convert: see the Mods tab.".into();
            self.tab = Tab::Mods;
            return;
        }
        if self.settings.convert && !self.layout.eula_accepted() {
            self.ask_eula = true;
            return;
        }
        self.start(ctx, false);
    }

    // Events ---------------------------------------------------------------

    fn drain(&mut self) {
        let Some(rx) = &self.rx else { return };
        let msgs: Vec<Msg> = rx.try_iter().collect();
        for msg in msgs {
            match msg {
                Msg::Event(e) => self.on_event(&e),
                Msg::Command(c) => self.on_command(c),
                Msg::Line(l) => self.push_log(l),
                Msg::Finished { ok, why } => {
                    for s in &mut self.stages {
                        s.finished = true;
                    }
                    self.task = if ok {
                        "Done".into()
                    } else {
                        format!("Stopped: {why}")
                    };
                    self.progress = Some((1, 1));
                    self.finished = Some((ok, why));
                    self.rx = None;
                    return;
                }
            }
        }
    }

    fn push_log(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    fn on_event(&mut self, e: &Value) {
        let s = |k: &str| e[k].as_str().unwrap_or("").to_owned();
        let n = |k: &str| e[k].as_u64().unwrap_or(0) as usize;
        match e["ev"].as_str() {
            Some("stage") => {
                // A stage id seen again (a second mod's port) starts afresh.
                if let Some(old) = self
                    .stages
                    .iter_mut()
                    .find(|st| st.id == s("id") && !st.finished)
                {
                    old.finished = true;
                }
                self.task = s("label");
                self.progress = (n("total") > 0).then_some((0, n("total")));
                self.stages.push(Stage {
                    id: s("id"),
                    label: s("label"),
                    ..Default::default()
                });
            }
            Some("step") => {
                let id = s("stage");
                let stage = self.stage(&id);
                let label = s("label");
                let detail = s("detail");
                let failed = detail.starts_with("FAILED");
                let done = n("done") >= 1;
                match stage.steps.iter_mut().find(|st| st.label == label) {
                    Some(step) => {
                        step.detail = detail;
                        step.failed = failed;
                        step.done = done;
                    }
                    None => stage.steps.push(Step {
                        label: label.clone(),
                        detail,
                        done,
                        failed,
                        commands: Vec::new(),
                    }),
                }
                let stage_label = stage.label.clone();
                self.task = format!("{stage_label}: {label}");
                if n("total") > 0 {
                    self.progress = Some((n("done"), n("total")));
                }
            }
            Some("command") => self.on_command(s("cmd")),
            Some("done") => {
                let id = s("stage");
                let stage = self.stage(&id);
                stage.summary = s("summary");
                stage.finished = true;
            }
            _ => {}
        }
    }

    /// The newest stage with `id`, made if there's none.
    fn stage(&mut self, id: &str) -> &mut Stage {
        let at = self
            .stages
            .iter()
            .rposition(|s| s.id == id)
            .unwrap_or_else(|| {
                self.stages.push(Stage {
                    id: id.to_owned(),
                    label: id.to_owned(),
                    ..Default::default()
                });
                self.stages.len() - 1
            });
        &mut self.stages[at]
    }

    fn on_command(&mut self, cmd: String) {
        self.push_log(format!("$ {cmd}"));
        if self.stages.is_empty() {
            self.stages.push(Stage {
                id: "commands".into(),
                label: "Commands".into(),
                ..Default::default()
            });
        }
        let stage = self.stages.last_mut().unwrap();
        match stage.steps.last_mut() {
            Some(step) if !stage.finished => step.commands.push(cmd),
            _ => stage.commands.push(cmd),
        }
    }

    // Views ------------------------------------------------------------------

    fn folders(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("folders")
            .num_columns(3)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                for (label, value) in [
                    ("Mods to convert (jars)", &mut self.input),
                    ("Converted packages", &mut self.output),
                ] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
                    if ui.button("Browse...").clicked() {
                        if let Some(dir) =
                            rfd::FileDialog::new().set_directory(&*value).pick_folder()
                        {
                            *value = dir.display().to_string();
                        }
                    }
                    ui.end_row();
                }
            });
        if self.input != self.scanned && !self.running() {
            self.scan(false);
        }
    }

    fn options(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.settings.convert, "Convert jars to .torchmod");
            ui.checkbox(
                &mut self.settings.port,
                "Port renderers with the local model (slow)",
            );
            ui.label("mods (blank: all):");
            ui.add(egui::TextEdit::singleline(&mut self.settings.port_mods).desired_width(160.0));
            ui.checkbox(&mut self.settings.install, "Copy into the Torch client");
        });
    }

    fn buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let running = self.running();
            if ui
                .add_enabled(
                    !running,
                    egui::Button::new(RichText::new("  Start  ").strong()),
                )
                .clicked()
            {
                self.try_start(ctx);
            }
            if ui.add_enabled(running, egui::Button::new("Stop")).clicked() {
                self.task = "Stopping...".into();
                let stop = self.stop.clone();
                std::thread::spawn(move || stop.stop());
            }
            if ui.button("Open output").clicked() {
                open(&PathBuf::from(&self.output));
            }
            if ui.button("Open logs").clicked() {
                open(&self.layout.port_work.join("logs"));
            }
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match &self.reqs {
                    None => {
                        ui.weak("Checking tools...");
                    }
                    Some(reqs) => {
                        let missing: Vec<String> = reqs
                            .iter()
                            .filter(|r| !r.ok)
                            .map(|r| format!("{} ({})", r.name, r.for_))
                            .collect();
                        let text = if missing.is_empty() {
                            RichText::new("All tools found").color(GREEN)
                        } else {
                            RichText::new(format!("Missing: {}", missing.join(", "))).color(RED)
                        };
                        ui.label(text).on_hover_ui(|ui| {
                            for r in reqs {
                                let mark = if r.ok {
                                    RichText::new("✔").color(GREEN)
                                } else {
                                    RichText::new("✖").color(RED)
                                };
                                ui.horizontal(|ui| {
                                    ui.label(mark);
                                    ui.label(RichText::new(r.name).strong());
                                    ui.weak(format!("{} [{}]", r.detail, r.for_));
                                });
                            }
                        });
                    }
                },
            );
        });
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(&self.task).size(15.0).strong());
        ui.horizontal(|ui| {
            let elapsed = self.started.map(|s| s.elapsed().as_secs());
            let bar = match (self.progress, self.running()) {
                (Some((done, total)), _) if total > 0 => {
                    egui::ProgressBar::new(done as f32 / total as f32)
                        .text(format!("{done}/{total}"))
                }
                (_, true) => egui::ProgressBar::new(0.0).animate(true),
                _ => egui::ProgressBar::new(if self.finished.as_ref().is_some_and(|f| f.0) {
                    1.0
                } else {
                    0.0
                }),
            };
            ui.add(bar.desired_width(ui.available_width() - 80.0));
            if let Some(t) = elapsed.filter(|_| self.running()) {
                ui.monospace(format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60));
            }
        });
    }

    fn mods_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Click an action to change it: Convert, Load only (a dependency, not packaged), Skip.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Reset to suggestions").clicked() {
                    self.scan(true);
                }
                if ui.button("Rescan").clicked() {
                    self.scan(false);
                }
            });
        });
        let (c, l, s) = self.counts();
        ui.label(if self.jars.is_empty() {
            "No jars in the input folder.".to_owned()
        } else {
            format!("{c} to convert, {l} load only, {s} skipped")
        });
        ui.separator();
        let running = self.running();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("mods")
                    .striped(true)
                    .num_columns(4)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Jar");
                        ui.strong("Action");
                        ui.strong("Mods");
                        ui.strong("Note");
                        ui.end_row();
                        for j in &mut self.jars {
                            let color = match j.action {
                                Action::Convert => ui.visuals().text_color(),
                                Action::LoadOnly => BLUE,
                                Action::Skip => Color32::GRAY,
                            };
                            ui.label(RichText::new(&j.name).color(color));
                            let button =
                                egui::Button::new(RichText::new(j.action.label()).color(color))
                                    .min_size([80.0, 0.0].into());
                            if ui.add_enabled(!running, button).clicked() {
                                j.action = j.action.next();
                            }
                            ui.label(if j.mods.is_empty() {
                                "-".to_owned()
                            } else {
                                j.mods.join(", ")
                            });
                            if j.action != j.suggested && j.suggested == Action::Skip {
                                ui.weak(format!("suggested Skip: {}", j.why));
                            } else {
                                ui.weak(&j.why);
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn activity_tab(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("activity")
            .max_height(height * 0.62)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (i, stage) in self.stages.iter().enumerate() {
                    let (icon, color) = if !stage.finished {
                        ("▶", BLUE)
                    } else if stage.failed() {
                        ("✖", RED)
                    } else {
                        ("✔", GREEN)
                    };
                    let title = RichText::new(format!("{icon} {}", stage.label))
                        .color(color)
                        .strong();
                    egui::CollapsingHeader::new(title)
                        .id_salt(("stage", i))
                        .default_open(true)
                        .show(ui, |ui| {
                            if !stage.summary.is_empty() {
                                ui.weak(&stage.summary);
                            }
                            for (k, step) in stage.steps.iter().enumerate() {
                                let (icon, color) = if step.failed {
                                    ("✖", RED)
                                } else if step.done {
                                    ("✔", GREEN)
                                } else {
                                    ("▶", BLUE)
                                };
                                let head =
                                    RichText::new(format!("{icon} {}", step.label)).color(color);
                                if step.commands.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(head);
                                        ui.weak(&step.detail);
                                    });
                                } else {
                                    egui::CollapsingHeader::new(head)
                                        .id_salt(("step", i, k))
                                        .default_open(!stage.finished)
                                        .show(ui, |ui| {
                                            ui.weak(&step.detail);
                                            for c in &step.commands {
                                                ui.label(
                                                    RichText::new(format!("$ {c}"))
                                                        .monospace()
                                                        .small()
                                                        .color(Color32::GRAY),
                                                );
                                            }
                                        });
                                }
                            }
                            for c in &stage.commands {
                                ui.label(
                                    RichText::new(format!("$ {c}"))
                                        .monospace()
                                        .small()
                                        .color(Color32::GRAY),
                                );
                            }
                        });
                }
                if let Some((ok, why)) = &self.finished {
                    let text = if *ok {
                        RichText::new("✔ Finished").color(GREEN)
                    } else {
                        RichText::new(format!("✖ Stopped: {why}")).color(RED)
                    };
                    ui.label(text.strong());
                }
            });
        ui.separator();
        ui.weak("Output");
        egui::ScrollArea::both()
            .id_salt("log")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.log {
                    ui.label(RichText::new(line).monospace().small());
                }
            });
    }

    fn eula(&mut self, ctx: &egui::Context) {
        if !self.ask_eula {
            return;
        }
        egui::Window::new("Minecraft EULA")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Converting runs a NeoForge server, which needs Minecraft's EULA:");
                ui.hyperlink(EULA_URL);
                ui.add_space(8.0);
                ui.label("Do you accept it?");
                ui.horizontal(|ui| {
                    if ui.button("I accept").clicked() {
                        self.ask_eula = false;
                        self.start(ctx, true);
                    }
                    if ui.button("Cancel").clicked() {
                        self.ask_eula = false;
                    }
                });
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        if let Ok(reqs) = self.reqs_rx.try_recv() {
            self.reqs = Some(reqs);
        }
        self.drain();
        if self.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(6.0);
            self.folders(ui);
            ui.add_space(4.0);
            self.options(ui);
            ui.add_space(4.0);
            self.buttons(ui, ctx);
            ui.add_space(6.0);
            self.status(ui);
            ui.add_space(6.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Mods, "Mods");
                ui.selectable_value(&mut self.tab, Tab::Activity, "Activity");
            });
            ui.separator();
            match self.tab {
                Tab::Mods => self.mods_tab(ui),
                Tab::Activity => self.activity_tab(ui),
            }
        });
        self.eula(ctx);
    }

    fn on_exit(&mut self) {
        self.save();
        self.stop.stop();
    }
}

fn open(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("explorer");
        c.arg(path);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    let _ = cmd.spawn();
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Torch Mod Converter")
            .with_inner_size([1100.0, 780.0])
            .with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Torch Mod Converter",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
