pub(crate) use bevy::platform::time::Instant;

pub(crate) use std::time::{SystemTime, UNIX_EPOCH};

#[allow(unsafe_code, reason = "winmm's timer resolution is a foreign call")]
/// Asks Windows for 1 ms timer resolution for the life of the process.
///
/// The default 15.6 ms resolution rounds every tokio sleep up to a whole
/// slice, so the network thread wakes for game ticks up to a slice late and
/// the renderer has to trail them by that much. Java clients get 1 ms from
/// the JVM and GLFW.
#[cfg(target_os = "windows")]
pub(crate) fn fine_timer() {
    #[link(name = "winmm")]
    unsafe extern "system" {
        fn timeBeginPeriod(period: u32) -> u32;
    }
    // SAFETY: a plain winmm call with no pointers; a refusal leaves the
    // default resolution in place.
    unsafe {
        timeBeginPeriod(1);
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn fine_timer() {}

pub(crate) fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

pub(crate) async fn sleep(duration: std::time::Duration) {
    tokio::time::sleep(duration).await;
}

pub(crate) async fn timeout<F: Future>(
    duration: std::time::Duration,
    future: F,
) -> Result<F::Output, ()> {
    tokio::time::timeout(duration, future).await.map_err(|_| ())
}
