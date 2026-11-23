//! Ported client code (`docs/MOD_API.md`): a mod's port is a Rust crate in
//! `<ports>/<modid>/`, built here for wasm32 into its package's
//! `plugin.wasm`.

use std::path::Path;
use std::process::Command;

pub struct Port {
    pub wasm: Vec<u8>,
    /// The mod API version it targets (`torch-mod-api`'s, at build time).
    pub api: u32,
    /// Its `PORTED.md`: what's ported, skipped and why.
    pub notes: Option<String>,
}

/// The port for `mod_id` under `dir`, built; `None` when there's none.
pub fn build(dir: &Path, mod_id: &str) -> Result<Option<Port>, String> {
    let crate_dir = dir.join(mod_id);
    let manifest = crate_dir.join("Cargo.toml");
    if !manifest.is_file() {
        return Ok(None);
    }
    println!("building the {mod_id} port");
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
        ])
        .arg(&manifest)
        .status()
        .map_err(|e| format!("can't run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("the {mod_id} port didn't build ({status})"));
    }
    let toml = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    let name = toml
        .lines()
        .find_map(|l| l.trim().strip_prefix("name = "))
        .map(|v| v.trim().trim_matches('"').replace('-', "_"))
        .ok_or("its Cargo.toml has no package name")?;
    let wasm_path = crate_dir
        .join("target/wasm32-unknown-unknown/release")
        .join(format!("{name}.wasm"));
    let wasm = std::fs::read(&wasm_path).map_err(|e| format!("{}: {e}", wasm_path.display()))?;
    Ok(Some(Port {
        wasm,
        api: 1,
        notes: std::fs::read_to_string(crate_dir.join("PORTED.md")).ok(),
    }))
}
