pub fn toggle() {
    #[cfg(target_arch = "wasm32")]
    web::toggle();
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    desktop::TOGGLE.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(target_os = "android")]
pub fn android_init() {
    android::keep_screen_on();
}

pub fn capture_escape(take: bool) {
    #[cfg(target_arch = "wasm32")]
    web::capture_escape(take);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = take;
}

pub fn is_fullscreen() -> bool {
    #[cfg(target_arch = "wasm32")]
    return web::is_fullscreen();
    #[cfg(target_os = "android")]
    return true;
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    desktop::FULLSCREEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Keeps the window as the saved `fullscreen` option says: exclusive
/// fullscreen on the monitor the window is on, at its current video mode, as
/// vanilla's does; it lets the driver present without the desktop compositor
/// in between. F11 (`toggle`) flips and saves the option; Video Settings
/// sets it directly.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
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
    desktop::FULLSCREEN.store(wanted, Relaxed);
}

/// Fullscreen there is the browser's or the system's, not the window's.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn apply_desktop() {}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod desktop {
    use std::sync::atomic::AtomicBool;

    /// Asked for by F11 or the menu, applied by `apply_desktop`.
    pub(super) static TOGGLE: AtomicBool = AtomicBool::new(false);
    /// Whether the window is fullscreen, as `apply_desktop` last saw it.
    pub(super) static FULLSCREEN: AtomicBool = AtomicBool::new(false);
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

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::{JsCast, JsValue};

    fn document() -> Option<web_sys::Document> {
        web_sys::window()?.document()
    }

    pub(super) fn toggle() {
        let Some(doc) = document() else { return };
        if doc.fullscreen_element().is_some() {
            let _ = doc.exit_fullscreen();
            return;
        }
        let Some(root) = doc.document_element() else {
            return;
        };
        let _ = root.request_fullscreen();
    }

    pub(super) fn capture_escape(take: bool) {
        let Some(nav) = web_sys::window().map(|w| w.navigator()) else {
            return;
        };
        let Ok(keyboard) = js_sys::Reflect::get(&nav, &JsValue::from_str("keyboard")) else {
            return;
        };
        if !keyboard.is_object() {
            return;
        }
        let name = if take { "lock" } else { "unlock" };
        let Ok(method) = js_sys::Reflect::get(&keyboard, &JsValue::from_str(name)) else {
            return;
        };
        let Ok(method) = method.dyn_into::<js_sys::Function>() else {
            return;
        };
        let result = if take {
            method.call1(&keyboard, &js_sys::Array::of1(&JsValue::from_str("Escape")))
        } else {
            method.call0(&keyboard)
        };
        let _ = result;
    }

    pub(super) fn is_fullscreen() -> bool {
        document().is_some_and(|d| d.fullscreen_element().is_some())
    }
}
