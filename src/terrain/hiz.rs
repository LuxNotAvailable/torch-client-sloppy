//! The terrain occlusion pyramid's switch (Debug Options > Occlusion Culling).

/// Copied from the GUI; on by default.
pub static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Occlusion culling switched off: `MC_NO_HIZ=1`, or the debug option
/// (`MC_HIZ=1` forces it on).
pub fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| crate::platform::env::var("MC_NO_HIZ").is_ok_and(|v| v == "1"))
        || !(ENABLED.load(std::sync::atomic::Ordering::Relaxed)
            || *ON.get_or_init(|| crate::platform::env::var("MC_HIZ").is_ok_and(|v| v == "1")))
}
