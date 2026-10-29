// A window app on Windows, not a console one: no terminal opens beside the
// game. Debug builds keep the console for their output; everything also goes
// to `logs/latest.log`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn main() -> eyre::Result<()> {
    torch_client::start_from_cli()
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}
