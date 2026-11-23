//! torch-convert: turns NeoForge mod jars into `.torchmod` packages the
//! Torch client loads (`docs/TORCHMOD_FORMAT.md`). The client itself never
//! converts; everything that needs a mod's code runs here.
//!
//! ```text
//! torch-convert setup                      install NeoForge, accept the EULA
//! torch-convert convert JAR|DIR... [--with JAR|DIR]... [--skip NAME]... [--out DIR] [--ports DIR]
//! ```
//!
//! `--ports DIR` adds ported client code: for a mod `create`, the crate in
//! `DIR/create/` (`docs/porting/AGENT_GUIDE.md`) is built for wasm32 and
//! packaged as its `plugin.wasm`.
//!
//! `convert` starts a headless NeoForge server once with every jar given
//! (and `--with` ones, loaded but not packaged) plus the embedded dumper
//! mod, which writes each block's states, shapes and behaviour and stops the
//! server before a world loads. Each jar then becomes one package: its own
//! assets and the dump's entries for its namespaces.

mod dump;
mod jar;
mod models;
mod package;
mod ports;
mod setup;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) const DEFAULT_NEOFORGE: &str = "21.1.264";

/// Options every command shares.
pub(crate) struct Env {
    pub work: PathBuf,
    pub neoforge: String,
    pub java: String,
    pub javac: String,
}

impl Env {
    pub fn server(&self) -> PathBuf {
        self.work.join(format!("server-{}", self.neoforge))
    }
}

fn default_work() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("torch-convert")
}

const USAGE: &str = "usage:
  torch-convert setup [options]
  torch-convert convert JAR|DIR... [--with JAR|DIR]... [--skip NAME]... [--out DIR] [--ports DIR] [options]

options: --work DIR (default: the user data folder's torch-convert)
         --neoforge VERSION (default 21.1.264)  --java PATH  --javac PATH";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("torch-convert: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut env = Env {
        work: default_work(),
        neoforge: DEFAULT_NEOFORGE.to_owned(),
        java: "java".to_owned(),
        javac: "javac".to_owned(),
    };
    let mut command = None;
    let mut targets = Vec::new();
    let mut with = Vec::new();
    let mut skip = Vec::new();
    let mut out = PathBuf::from("converted");
    let mut ports: Option<PathBuf> = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--work" => env.work = PathBuf::from(value("--work")?),
            "--neoforge" => env.neoforge = value("--neoforge")?,
            "--java" => env.java = value("--java")?,
            "--javac" => env.javac = value("--javac")?,
            "--with" => with.push(PathBuf::from(value("--with")?)),
            "--skip" => skip.push(value("--skip")?.to_lowercase()),
            "--out" => out = PathBuf::from(value("--out")?),
            "--ports" => ports = Some(PathBuf::from(value("--ports")?)),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ if command.is_none() => command = Some(arg),
            _ => targets.push(PathBuf::from(arg)),
        }
    }
    // The server runs in its own folder: every path must survive that.
    env.work = std::path::absolute(&env.work).map_err(|e| e.to_string())?;
    let out = std::path::absolute(&out).map_err(|e| e.to_string())?;
    match command.as_deref() {
        Some("setup") => setup::setup(&env),
        Some("convert") => {
            if targets.is_empty() {
                return Err(format!("convert needs at least one jar\n{USAGE}"));
            }
            let targets = jars(&targets, &skip)?;
            let with = jars(&with, &skip)?;
            let ports = match ports {
                Some(p) => Some(std::path::absolute(p).map_err(|e| e.to_string())?),
                None => None,
            };
            convert(&env, &targets, &with, &out, ports.as_deref())
        }
        _ => Err(USAGE.to_owned()),
    }
}

/// The jars in `paths`: files as given, folders' `.jar`s, minus any whose
/// file name contains a `skip` entry.
fn jars(paths: &[PathBuf], skip: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")))
                .collect();
            found.sort();
            out.extend(found);
        } else if path.is_file() {
            out.push(path.clone());
        } else {
            return Err(format!("{} doesn't exist", path.display()));
        }
    }
    out.retain(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase());
        let name = name.unwrap_or_default();
        let skipped = skip.iter().any(|s| name.contains(s.as_str()));
        if skipped {
            println!("skipping {name}");
        }
        !skipped
    });
    Ok(out)
}

fn convert(
    env: &Env,
    targets: &[PathBuf],
    with: &[PathBuf],
    out: &Path,
    ports: Option<&Path>,
) -> Result<(), String> {
    let server = env.server();
    if !setup::installed(env) {
        return Err(format!(
            "no NeoForge {} server at {}; run `setup` first",
            env.neoforge,
            server.display()
        ));
    }
    if !setup::eula_accepted(&server) {
        return Err("the EULA isn't accepted; run `setup` first".into());
    }
    let mut mods = Vec::new();
    for path in targets {
        let mut m = jar::ModJar::read(path)?;
        let (obj, composite) = models::rewrite(&mut m.assets);
        if obj + composite > 0 {
            println!(
                "{}: {obj} OBJ and {composite} composite model(s) made plain",
                m.file
            );
        }
        // The OBJ sources are baked into their models now.
        m.assets
            .retain(|(k, _)| !k.ends_with(".obj") && !k.ends_with(".mtl"));
        mods.push(m);
    }
    let loaded: Vec<&Path> = targets.iter().chain(with).map(PathBuf::as_path).collect();
    let dumped = dump::run(env, &loaded)?;
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut written = 0;
    for m in &mods {
        let port = match (ports, m.mods.first()) {
            (Some(dir), Some(main)) => ports::build(dir, &main.id)?,
            _ => None,
        };
        if let Some(port) = &port {
            println!("{}: ported code, {} bytes", m.file, port.wasm.len());
        }
        match package::write(env, m, &dumped, out, port.as_ref())? {
            Some(path) => {
                written += 1;
                println!("wrote {}", path.display());
            }
            None => println!("{}: nothing the client uses", m.file),
        }
    }
    println!("{written} package(s) in {}", out.display());
    Ok(())
}
