//! The tools a run needs, and whether this machine has them.

use std::path::PathBuf;
use std::process::Command;

use crate::layout::Layout;

pub struct Req {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    /// "convert" or "port": what needs it.
    pub for_: &'static str,
}

/// `name` on PATH (with Windows' executable extensions).
pub fn on_path(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(str::to_owned)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter()
            .map(|e| dir.join(format!("{name}{e}")))
            .chain(std::iter::once(dir.join(name)))
            .find(|p| p.is_file())
    })
}

/// The first line a command prints, if it runs and succeeds.
fn probe(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    crate::runner::no_window(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    Some(text.lines().next().unwrap_or("").trim().to_owned())
}

pub fn check(l: &Layout) -> Vec<Req> {
    let req = |name, found: Option<String>, missing: &str, for_| Req {
        name,
        ok: found.is_some(),
        detail: found.unwrap_or_else(|| missing.to_owned()),
        for_,
    };
    let wasm = probe("rustup", &["target", "list", "--installed"]).is_some() && {
        let mut c = Command::new("rustup");
        c.args(["target", "list", "--installed"]);
        crate::runner::no_window(&mut c);
        c.output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-unknown-unknown"))
    };
    vec![
        req(
            "Java 21+",
            probe("java", &["-version"]),
            "install a JDK 21 (e.g. Temurin)",
            "convert",
        ),
        req(
            "javac",
            probe("javac", &["-version"]),
            "the JDK's compiler, not just a JRE",
            "convert",
        ),
        req(
            "torch-convert",
            l.convert.is_file().then(|| l.convert.display().to_string()),
            "not built: cargo build --release in crates/torch-convert",
            "convert",
        ),
        req(
            "Python 3.10+",
            probe("python", &["--version"]).or_else(|| probe("py", &["--version"])),
            "install Python (runs the port agent)",
            "port",
        ),
        req("git", probe("git", &["--version"]), "install git", "port"),
        req(
            "ripgrep (rg)",
            on_path("rg").map(|p| p.display().to_string()),
            "install ripgrep: the agent searches with it",
            "port",
        ),
        req(
            "cargo",
            probe("cargo", &["--version"]),
            "install Rust (rustup.rs)",
            "port",
        ),
        req(
            "wasm32 target",
            wasm.then(|| "installed".to_owned()),
            "rustup target add wasm32-unknown-unknown",
            "port",
        ),
        req(
            "Ollama",
            probe("ollama", &["--version"]),
            "install Ollama (ollama.com)",
            "port",
        ),
        req(
            "Qwen Code",
            on_path("qwen").map(|p| p.display().to_string()),
            "npm install -g @qwen-code/qwen-code",
            "port",
        ),
    ]
}
