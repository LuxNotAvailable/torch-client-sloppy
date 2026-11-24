//! The pipeline, on its own thread: dump server setup, conversion, the
//! port agent, copying into the client. Every line its processes print
//! goes to the window; `@@{json}` lines are progress events.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::layout::Layout;

pub const PORT_MODEL: &str = "qwen3.8-port";

pub enum Msg {
    Event(Value),
    Command(String),
    Line(String),
    Finished { ok: bool, why: String },
}

/// What a run does.
pub struct Plan {
    pub input: PathBuf,
    pub output: PathBuf,
    pub convert: Vec<PathBuf>,
    pub load_only: Vec<PathBuf>,
    pub skipped: usize,
    pub do_convert: bool,
    pub do_port: bool,
    pub port_mods: Vec<String>,
    pub install: bool,
    pub accept_eula: bool,
}

/// Stops the run: the process running now is killed with its children.
#[derive(Clone, Default)]
pub struct Stop {
    stopped: Arc<AtomicBool>,
    pid: Arc<Mutex<Option<u32>>>,
}

impl Stop {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(pid) = *self.pid.lock().unwrap() {
            kill_tree(pid);
        }
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

fn kill_tree(pid: u32) {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]);
        c
    } else {
        let mut c = Command::new("kill");
        c.args(["-9", &pid.to_string()]);
        c
    };
    no_window(&mut cmd);
    let _ = cmd.output();
}

/// No console window for a child process on Windows.
pub fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

struct Run<'a> {
    tx: Sender<Msg>,
    stop: Stop,
    repaint: Box<dyn Fn() + Send + 'a>,
}

impl Run<'_> {
    fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
        (self.repaint)();
    }

    fn event(&self, value: Value) {
        self.send(Msg::Event(value));
    }

    /// Runs `cmd`, streaming its output; its exit code.
    fn run(&self, mut cmd: Command) -> Result<i32, String> {
        if self.stop.stopped() {
            return Err("stopped".into());
        }
        self.send(Msg::Command(show(&cmd)));
        no_window(&mut cmd);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("{}: {e}", cmd.get_program().to_string_lossy()))?;
        *self.stop.pid.lock().unwrap() = Some(child.id());
        let err = child.stderr.take().map(|e| {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(e).lines().map_while(Result::ok) {
                    let _ = tx.send(Msg::Line(line));
                }
            })
        });
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                match line
                    .strip_prefix("@@")
                    .and_then(|j| serde_json::from_str::<Value>(j).ok())
                {
                    Some(ev) => self.event(ev),
                    None => self.send(Msg::Line(line)),
                }
            }
        }
        if let Some(t) = err {
            let _ = t.join();
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        *self.stop.pid.lock().unwrap() = None;
        if self.stop.stopped() {
            return Err("stopped".into());
        }
        Ok(status.code().unwrap_or(-1))
    }
}

/// A command line as a shell would show it.
fn show(cmd: &Command) -> String {
    std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|a| {
            let a = a.to_string_lossy();
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn start(
    layout: Arc<Layout>,
    plan: Plan,
    tx: Sender<Msg>,
    stop: Stop,
    repaint: impl Fn() + Send + 'static,
) {
    std::thread::spawn(move || {
        let run = Run {
            tx,
            stop,
            repaint: Box::new(repaint),
        };
        let result = pipeline(&layout, &plan, &run);
        let (ok, why) = match result {
            Ok(()) => (true, String::new()),
            Err(e) => (false, e),
        };
        run.send(Msg::Finished { ok, why });
    });
}

fn pipeline(l: &Layout, p: &Plan, run: &Run) -> Result<(), String> {
    std::fs::create_dir_all(&p.output).map_err(|e| format!("{}: {e}", p.output.display()))?;
    run.event(json!({"ev": "stage", "id": "input", "label": format!("Mods in {}", p.input.display()), "total": 0}));
    run.event(json!({"ev": "done", "stage": "input", "summary": format!(
        "{} to convert, {} load only, {} skipped", p.convert.len(), p.load_only.len(), p.skipped)}));

    if p.do_convert {
        let work = |c: &mut Command| {
            c.arg("--work").arg(&l.convert_work).arg("--progress");
        };
        let mut setup = Command::new(&l.convert);
        setup.arg("setup");
        work(&mut setup);
        if p.accept_eula {
            setup.arg("--accept-eula");
        }
        if run.run(setup)? != 0 {
            return Err("setup failed (see the output)".into());
        }
        let mut convert = Command::new(&l.convert);
        convert
            .arg("convert")
            .args(&p.convert)
            .arg("--out")
            .arg(&p.output);
        work(&mut convert);
        for j in &p.load_only {
            convert.arg("--with").arg(j);
        }
        let ports = l.workspace.join("ports");
        if ports.is_dir() {
            convert.arg("--ports").arg(&ports);
        }
        if run.run(convert)? != 0 {
            return Err("conversion failed (see the output)".into());
        }
    }

    if p.do_port {
        port(l, p, run)?;
    }

    if p.install {
        let Some(dest) = &l.client_mods else {
            return Err("no Torch client mods folder on this system".into());
        };
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        let pkgs: Vec<PathBuf> = std::fs::read_dir(&p.output)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|f| f.extension().is_some_and(|e| e == "torchmod"))
            .collect();
        run.event(json!({"ev": "stage", "id": "install", "label": format!("Copying into {}", dest.display()), "total": pkgs.len()}));
        for (i, f) in pkgs.iter().enumerate() {
            let name = f.file_name().unwrap_or_default();
            std::fs::copy(f, dest.join(name)).map_err(|e| e.to_string())?;
            run.event(json!({"ev": "step", "stage": "install", "label": name.to_string_lossy(), "detail": "copied", "done": i + 1, "total": pkgs.len()}));
        }
        run.event(json!({"ev": "done", "stage": "install", "summary": format!("{} package(s) copied", pkgs.len())}));
    }
    Ok(())
}

/// Python for the port agent: `python`, else the `py` launcher.
fn python() -> Command {
    if crate::tools::on_path("python").is_some() {
        Command::new("python")
    } else {
        Command::new("py")
    }
}

fn port(l: &Layout, p: &Plan, run: &Run) -> Result<(), String> {
    ensure_workspace(&l.workspace, run)?;
    let mut has_model = Command::new("ollama");
    has_model.args(["show", PORT_MODEL]);
    no_window(&mut has_model);
    if !has_model.output().is_ok_and(|o| o.status.success()) {
        let mut setup = python();
        setup.arg(&l.agent_model);
        run.run(setup)?;
    }
    let ids: Vec<String> = if p.port_mods.is_empty() {
        p.convert
            .iter()
            .filter_map(|j| crate::jars::mod_ids(j).into_iter().next())
            .collect()
    } else {
        p.port_mods.clone()
    };
    for id in ids {
        let mut cmd = python();
        cmd.arg(&l.agent)
            .arg(&id)
            .args(["--events", "--apply", "--install", "--all"])
            .current_dir(&l.workspace)
            .env("TORCH_PORT_ROOT", &l.workspace)
            .env("TORCH_PORT_WORK", &l.port_work)
            .env("TORCH_PORT_MODS", &p.input)
            .env("TORCH_PORT_PACKAGES", &p.output)
            .env("TORCH_PORT_CLIENT_MODS", "")
            .env("PYTHONUNBUFFERED", "1");
        if l.checker.is_file() {
            cmd.env("TORCH_PORT_CHECK", &l.checker);
        }
        if l.vanilla.exists() {
            cmd.env("TORCH_PORT_VANILLA", &l.vanilla);
        }
        // A mod without renderers stops after its inventory; that's fine.
        run.run(cmd)?;
    }
    Ok(())
}

/// The agent works in a git checkout: a standalone workspace becomes one
/// on first use.
fn ensure_workspace(ws: &Path, run: &Run) -> Result<(), String> {
    if ws.join(".git").exists() {
        return Ok(());
    }
    run.event(json!({"ev": "stage", "id": "workspace", "label": "Making the port workspace a git repository", "total": 0}));
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &["commit", "-q", "-m", "workspace"],
    ] {
        let mut git = Command::new("git");
        git.args([
            "-c",
            "user.name=torch-converter",
            "-c",
            "user.email=torch-converter@localhost",
        ])
        .args(args)
        .current_dir(ws);
        run.run(git)?;
    }
    run.event(json!({"ev": "done", "stage": "workspace", "summary": "ready"}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real pipeline on one jar from the repo's mods folder, through
    /// the repo's dump server (EULA already accepted there). Slow: run with
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn converts_a_jar_end_to_end() {
        let layout = Arc::new(Layout::find());
        let jar = layout
            .default_in
            .join("carryon-neoforge-1.21.1-2.2.6.13.jar");
        assert!(jar.is_file(), "needs {}", jar.display());
        let out = std::env::temp_dir().join("torch-converter-test");
        let _ = std::fs::remove_dir_all(&out);
        let plan = Plan {
            input: layout.default_in.clone(),
            output: out.clone(),
            convert: vec![jar],
            load_only: Vec::new(),
            skipped: 0,
            do_convert: true,
            do_port: false,
            port_mods: Vec::new(),
            install: false,
            accept_eula: false,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        start(layout, plan, tx, Stop::default(), || {});
        let mut stages = Vec::new();
        let mut commands = 0;
        let finished = loop {
            match rx.recv().expect("the runner hung up") {
                Msg::Event(e) if e["ev"] == "stage" => {
                    stages.push(e["id"].as_str().unwrap_or("").to_owned())
                }
                // The runner's own, and torch-convert's (javac, the server).
                Msg::Event(e) if e["ev"] == "command" => commands += 1,
                Msg::Command(_) => commands += 1,
                Msg::Finished { ok, why } => break (ok, why),
                _ => {}
            }
        };
        assert!(finished.0, "{}", finished.1);
        for s in ["input", "setup", "read", "dump", "package"] {
            assert!(stages.iter().any(|x| x == s), "no {s} stage in {stages:?}");
        }
        assert!(commands >= 4, "{commands} commands");
        assert!(out.join("carryon-2.2.6.torchmod").is_file());
        let _ = std::fs::remove_dir_all(&out);
    }
}
