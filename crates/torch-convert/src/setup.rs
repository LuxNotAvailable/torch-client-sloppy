//! Installs the official NeoForge server the dumps run on, and asks for
//! the Minecraft EULA, which that server needs.

use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::Env;

const MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

pub fn installed(env: &Env) -> bool {
    env.server()
        .join("libraries/net/neoforged/neoforge")
        .join(&env.neoforge)
        .is_dir()
}

pub fn eula_accepted(server: &Path) -> bool {
    std::fs::read_to_string(server.join("eula.txt")).is_ok_and(|text| {
        text.lines()
            .any(|line| line.replace(' ', "").eq_ignore_ascii_case("eula=true"))
    })
}

pub fn setup(env: &Env) -> Result<(), String> {
    let server = env.server();
    if installed(env) {
        println!(
            "NeoForge {} is installed at {}",
            env.neoforge,
            server.display()
        );
    } else {
        std::fs::create_dir_all(&server).map_err(|e| format!("{}: {e}", server.display()))?;
        let jar = installer(env)?;
        println!("installing NeoForge {} server", env.neoforge);
        let status = Command::new(&env.java)
            .arg("-jar")
            .arg(&jar)
            .arg("--installServer")
            .arg(&server)
            .current_dir(&server)
            .status()
            .map_err(|e| format!("can't run {}: {e} (a JDK 21 or newer is needed)", env.java))?;
        if !status.success() || !installed(env) {
            return Err(format!("the NeoForge installer failed ({status})"));
        }
    }
    if eula_accepted(&server) {
        println!("the EULA is accepted");
        return Ok(());
    }
    print!("The dump server needs Minecraft's EULA ({EULA_URL}). Do you accept it? [y/N] ");
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).ok();
    let accepted = matches!(answer.trim().to_lowercase().as_str(), "y" | "yes");
    let text = format!("# {EULA_URL}\neula={accepted}\n");
    std::fs::write(server.join("eula.txt"), text).map_err(|e| e.to_string())?;
    if accepted {
        println!("ready: run `torch-convert convert`");
        Ok(())
    } else {
        Err("the EULA wasn't accepted; conversions can't run".into())
    }
}

/// The installer jar, from NeoForge's maven, checked against the SHA-256
/// published beside it.
fn installer(env: &Env) -> Result<std::path::PathBuf, String> {
    let v = &env.neoforge;
    let url = format!("{MAVEN}/{v}/neoforge-{v}-installer.jar");
    let path = env.work.join(format!("neoforge-{v}-installer.jar"));
    let expected = fetch(&format!("{url}.sha256"))?;
    let expected = String::from_utf8_lossy(&expected)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if std::fs::read(&path).is_ok_and(|bytes| sha256(&bytes) == expected) {
        return Ok(path);
    }
    println!("downloading {url}");
    let bytes = fetch(&url)?;
    if sha256(&bytes) != expected {
        return Err(format!("{url} doesn't match its published SHA-256"));
    }
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{url}: {e}"))?;
    Ok(bytes)
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
