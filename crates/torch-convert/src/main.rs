//! torch-convert: turns NeoForge mod jars into `.torchmod` packages the
//! Torch client loads (`docs/TORCHMOD_FORMAT.md`). The client itself never
//! converts; everything that needs a mod's code runs here.
//!
//! ```text
//! torch-convert setup [--accept-eula]     install NeoForge, accept the EULA
//! torch-convert convert JAR|DIR... [--with JAR|DIR]... [--skip NAME]... [--out DIR] [--ports DIR]
//! ```
//!
//! `--progress` adds machine-readable progress lines for front ends
//! (`progress.rs`).
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
mod progress;
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
         --neoforge VERSION (default 21.1.264)  --java PATH  --javac PATH
         --progress (JSON progress lines, prefixed @@, for front ends)
         --accept-eula (setup: the user accepted Minecraft's EULA elsewhere)";

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
    let mut accept_eula = false;
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
            "--progress" => progress::enable(),
            "--accept-eula" => accept_eula = true,
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
        Some("setup") => setup::setup(&env, accept_eula),
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
    progress::stage("read", "Reading jars and extracting assets", targets.len());
    for (i, path) in targets.iter().enumerate() {
        let mut m = jar::ModJar::read(path)?;
        progress::step("read", &m.file, &asset_summary(&m), i + 1, targets.len());
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
    progress::done("read", &format!("{} jar(s) read", mods.len()));
    let loaded: Vec<&Path> = targets.iter().chain(with).map(PathBuf::as_path).collect();
    let dumped = dump::run(env, &loaded)?;
    let count = |key: &str| dumped[key].as_array().map_or(0, Vec::len);
    let states: usize = dumped["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| b["states"].as_array().map_or(1, Vec::len))
        .sum();
    progress::done(
        "dump",
        &format!(
            "{} blocks ({states} states), {} items, {} entities dumped (vanilla's included)",
            count("blocks"),
            count("items"),
            count("entities")
        ),
    );
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut written = 0;
    progress::stage("package", "Writing .torchmod packages", mods.len());
    for (i, m) in mods.iter().enumerate() {
        let port = match (ports, m.mods.first()) {
            (Some(dir), Some(main)) => ports::build(dir, &main.id)?,
            _ => None,
        };
        if let Some(port) = &port {
            println!("{}: ported code, {} bytes", m.file, port.wasm.len());
        }
        let detail = match package::write(env, m, &dumped, out, port.as_ref())? {
            Some(path) => {
                written += 1;
                println!("wrote {}", path.display());
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                let plugin = port
                    .as_ref()
                    .map(|p| format!(", plugin {} KB", p.wasm.len() / 1024))
                    .unwrap_or_default();
                format!("{}{plugin}", name.unwrap_or_default())
            }
            None => {
                println!("{}: nothing the client uses", m.file);
                "nothing the client uses: no package".to_owned()
            }
        };
        progress::step("package", &m.file, &detail, i + 1, mods.len());
    }
    println!("{written} package(s) in {}", out.display());
    progress::done(
        "package",
        &format!("{written} package(s) in {}", out.display()),
    );
    Ok(())
}

/// What a jar carries, for progress: its mods and its assets by kind.
fn asset_summary(m: &jar::ModJar) -> String {
    let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
    for (key, _) in &m.assets {
        let kind = key.split('/').nth(2).unwrap_or("other");
        *kinds.entry(kind).or_default() += 1;
    }
    let ids: Vec<&str> = m.mods.iter().map(|x| x.id.as_str()).collect();
    let parts: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
    format!(
        "mods {}; {}",
        if ids.is_empty() {
            "none".to_owned()
        } else {
            ids.join(", ")
        },
        if parts.is_empty() {
            "no assets".to_owned()
        } else {
            parts.join(", ")
        }
    )
}
