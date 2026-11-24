//! Runs the embedded dumper mod (`dumper/TorchDump.java`) in the installed
//! server with the given jars, and returns its JSON.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::Env;

const DUMPER_SOURCE: &str = include_str!("../dumper/TorchDump.java");
const DUMPER_TOML: &str = include_str!("../dumper/neoforge.mods.toml");

pub fn run(env: &Env, jars: &[&Path]) -> Result<Value, String> {
    let server = env.server();
    crate::progress::stage(
        "dump",
        "Dumping blocks, items and entities (NeoForge server)",
        3,
    );
    let build = server.join("torchdump-build");
    let dumper = build_dumper(env, &server, &build)?;
    stage(&server, jars, &dumper)?;
    let out = build.join("torch-dump.json");
    let _ = std::fs::remove_file(&out);
    let args_file = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    let args = server
        .join("libraries/net/neoforged/neoforge")
        .join(&env.neoforge)
        .join(args_file);
    println!(
        "starting the dump server with {} jar(s); it stops itself",
        jars.len()
    );
    crate::progress::step(
        "dump",
        "Dump server",
        &format!(
            "NeoForge with {} jar(s): block, item and entity data",
            jars.len()
        ),
        2,
        3,
    );
    let mut cmd = Command::new(&env.java);
    cmd.arg(format!("-Dtorchdump.out={}", out.display()))
        .arg(format!("@{}", args.display()))
        .arg("nogui")
        .current_dir(&server);
    crate::progress::command(&cmd);
    let status = cmd
        .status()
        .map_err(|e| format!("can't run {}: {e}", env.java))?;
    let text = std::fs::read_to_string(&out).map_err(|_| {
        format!(
            "the server exited ({status}) without a dump; see {}",
            server.join("logs/latest.log").display()
        )
    })?;
    serde_json::from_str(&text).map_err(|e| format!("the dump doesn't read: {e}"))
}

/// Compiles the dumper against the server's libraries: NeoForge's patched
/// game classes first, the unpatched jar's rest after them.
fn build_dumper(env: &Env, server: &Path, build: &Path) -> Result<PathBuf, String> {
    let classes = build.join("classes");
    let _ = std::fs::remove_dir_all(&classes);
    std::fs::create_dir_all(classes.join("src")).map_err(|e| e.to_string())?;
    let source = classes.join("src/TorchDump.java");
    std::fs::write(&source, DUMPER_SOURCE).map_err(|e| e.to_string())?;
    let mut libs = Vec::new();
    walk(&server.join("libraries"), &mut libs);
    libs.retain(|p| p.extension().is_some_and(|e| e == "jar"));
    libs.sort();
    let (mut patched, rest): (Vec<PathBuf>, Vec<PathBuf>) = libs.into_iter().partition(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
        name.is_some_and(|n| n.starts_with("neoforge-") && n.ends_with("-server.jar"))
    });
    patched.extend(rest);
    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath: Vec<String> = patched
        .iter()
        .map(|p| p.display().to_string().replace('\\', "/"))
        .collect();
    let argfile = build.join("classpath.txt");
    std::fs::write(&argfile, format!("-cp\n\"{}\"\n", classpath.join(sep)))
        .map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&env.javac);
    cmd.args(["--release", "21", "-nowarn"])
        .arg(format!("@{}", argfile.display()))
        .arg("-d")
        .arg(&classes)
        .arg(&source);
    crate::progress::step("dump", "Dumper", "compiling the dumper mod", 1, 3);
    crate::progress::command(&cmd);
    let status = cmd
        .status()
        .map_err(|e| format!("can't run {}: {e} (a JDK 21 or newer is needed)", env.javac))?;
    if !status.success() {
        return Err(format!("the dumper didn't compile ({status})"));
    }
    let jar = build.join("torchdump.jar");
    let file = std::fs::File::create(&jar).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        z.start_file(name, options).map_err(|e| e.to_string())?;
        z.write_all(bytes).map_err(|e| e.to_string())
    };
    add("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n")?;
    add("META-INF/neoforge.mods.toml", DUMPER_TOML.as_bytes())?;
    let mut class_files = Vec::new();
    walk(&classes.join("torchdump"), &mut class_files);
    for f in class_files {
        let name = f
            .strip_prefix(&classes)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        add(&name, &std::fs::read(&f).map_err(|e| e.to_string())?)?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(jar)
}

/// The server's mods folder holds exactly `jars` and the dumper.
fn stage(server: &Path, jars: &[&Path], dumper: &Path) -> Result<(), String> {
    let mods = server.join("mods");
    let _ = std::fs::remove_dir_all(&mods);
    std::fs::create_dir_all(&mods).map_err(|e| e.to_string())?;
    for jar in jars.iter().copied().chain([dumper]) {
        let name = jar.file_name().ok_or("a jar has no name")?;
        std::fs::copy(jar, mods.join(name)).map_err(|e| format!("{}: {e}", jar.display()))?;
    }
    std::fs::write(
        server.join("server.properties"),
        "online-mode=false\nserver-port=0\nlevel-name=torchdump-world\nmax-players=1\n",
    )
    .map_err(|e| e.to_string())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}
