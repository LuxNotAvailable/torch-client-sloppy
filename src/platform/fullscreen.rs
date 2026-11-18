pub fn toggle() {
    #[cfg(not(target_os = "android"))]
    desktop::TOGGLE.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(target_os = "android")]
pub fn android_init() {
    android::keep_screen_on();
}

/// Keeps the window as the saved `fullscreen` option says: exclusive
/// fullscreen on the monitor the window is on, at its current video mode, as
/// vanilla's does; it lets the driver present without the desktop compositor
/// in between. F11 (`toggle`) flips and saves the option; Video Settings
/// sets it directly.
#[cfg(not(target_os = "android"))]
pub fn apply_desktop(
    mut windows: bevy::prelude::Query<
        &mut bevy::window::Window,
        bevy::prelude::With<bevy::window::PrimaryWindow>,
    >,
    mut state: bevy::prelude::ResMut<crate::gui::GuiState>,
) {
    use bevy::window::{MonitorSelection, VideoModeSelection, WindowMode};
    use std::sync::atomic::Ordering::Relaxed;
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    if desktop::TOGGLE.swap(false, Relaxed) {
        state.options.fullscreen = !state.options.fullscreen;
        crate::gui::save_options(&state.options);
    }
    let wanted = state.options.fullscreen;
    let is = window.mode != WindowMode::Windowed;
    if wanted != is {
        window.mode = if wanted {
            WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current)
        } else {
            WindowMode::Windowed
        };
    }
}

/// Fullscreen there is the system's, not the window's.
#[cfg(target_os = "android")]
pub fn apply_desktop() {}

#[cfg(not(target_os = "android"))]
mod desktop {
    use std::sync::atomic::AtomicBool;

    /// Asked for by F11 or the menu, applied by `apply_desktop`.
    pub(super) static TOGGLE: AtomicBool = AtomicBool::new(false);
}

#[cfg(target_os = "android")]
mod android {
    use bevy::android::android_activity::WindowManagerFlags;

    pub(super) fn keep_screen_on() {
        if let Some(app) = bevy::android::ANDROID_APP.get() {
            app.set_window_flags(
                WindowManagerFlags::KEEP_SCREEN_ON,
                WindowManagerFlags::empty(),
            );
        }
    }
}
