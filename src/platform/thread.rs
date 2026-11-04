//! Thread scheduling hints.

/// Whether background workers drop their priority; `MC_WORKER_PRIORITY=normal`
/// keeps it, to compare the two.
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "android"))]
fn lowering() -> bool {
    !crate::platform::env::var("MC_WORKER_PRIORITY").is_ok_and(|v| v == "normal")
}

/// Asks the OS to run the calling thread below normal priority.
///
/// For the background workers (meshing, lighting). On a 4-core CPU they
/// compete with the main and render threads, and when the GPU stops
/// limiting the frame those threads lose whole slices together: the
/// frame's p99 spikes. Below-normal priority lets a frame's own work run
/// first, while the workers still use every idle core.
#[allow(unsafe_code, reason = "thread priority is a foreign call")]
#[cfg(target_os = "windows")]
pub(crate) fn lower_priority() {
    if !lowering() {
        return;
    }
    const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut core::ffi::c_void;
        fn SetThreadPriority(thread: *mut core::ffi::c_void, priority: i32) -> i32;
    }
    // SAFETY: `GetCurrentThread` returns a pseudo-handle for the calling
    // thread that needs no closing; a refusal leaves the priority as it was.
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[allow(unsafe_code, reason = "thread priority is a foreign call")]
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn lower_priority() {
    if !lowering() {
        return;
    }
    const PRIO_PROCESS: i32 = 0;
    /// Nice values run -20 to 19; 5 yields to normal threads under load
    /// without starving the worker.
    const NICENESS: i32 = 5;
    unsafe extern "C" {
        fn setpriority(which: i32, who: u32, prio: i32) -> i32;
    }
    // SAFETY: no pointers. On Linux, `who` 0 with PRIO_PROCESS names the
    // calling thread, not the whole process; a refusal changes nothing.
    unsafe {
        setpriority(PRIO_PROCESS, 0, NICENESS);
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
pub(crate) fn lower_priority() {}
