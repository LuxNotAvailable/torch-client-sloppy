#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "only Android shows a soft keyboard")
)]
pub(crate) fn show(text: &str, anchor: usize, cursor: usize) {
    #[cfg(target_os = "android")]
    android::want(true);

    let _ = (text, anchor, cursor);
}

pub(crate) fn hide() {
    #[cfg(target_os = "android")]
    {
        android::set_dismissed(false);
        android::want(false);
    }
}

pub(crate) fn dismiss() {
    #[cfg(target_os = "android")]
    {
        if android::pinned() {
            return;
        }
        android::set_dismissed(true);
        android::want(false);
    }
}

#[cfg_attr(
    not(feature = "mobile_ui"),
    allow(dead_code, reason = "used by the touch controls only")
)]
pub(crate) fn pin(on: bool) {
    #[cfg(target_os = "android")]
    {
        android::set_pinned(on);
        if on {
            android::set_dismissed(false);
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = on;
}

#[cfg_attr(
    not(feature = "mobile_ui"),
    allow(dead_code, reason = "used by the touch controls only")
)]
pub(crate) fn reassert() {
    #[cfg(target_os = "android")]
    if android::wanted() && !android::dismissed() {
        android::want(true);
    }
}

#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "read only by the Android arm of TextBox::sync_keyboard"
    )
)]
pub(crate) fn dismissed() -> bool {
    #[cfg(target_os = "android")]
    return android::dismissed();
    #[cfg(not(target_os = "android"))]
    false
}

pub(crate) fn note_tap(inside: bool) {
    #[cfg(target_os = "android")]
    android::note_tap(inside);
    #[cfg(not(target_os = "android"))]
    let _ = inside;
}

pub(crate) fn resolve_taps() {
    #[cfg(target_os = "android")]
    if android::take_outside_tap() {
        crate::log_info!(
            "input",
            "tap outside the text field: dismissing the keyboard"
        );
        dismiss();
    }
}

#[cfg_attr(
    not(target_os = "android"),
    allow(dead_code, reason = "only Android shows a soft keyboard")
)]
pub(crate) fn has_focus() -> bool {
    #[cfg(target_os = "android")]
    return android::wanted();
    #[cfg(not(target_os = "android"))]
    false
}

pub(crate) fn inset_px() -> f32 {
    #[cfg(target_os = "android")]
    let px = android::inset_px();
    #[cfg(not(target_os = "android"))]
    let px = 0.0;
    #[cfg(target_os = "android")]
    log_inset(px);
    px
}

#[cfg(target_os = "android")]
fn log_inset(px: f32) {
    use std::sync::atomic::{AtomicI32, Ordering};
    static LAST: AtomicI32 = AtomicI32::new(-1);
    let now = px as i32;
    if LAST.swap(now, Ordering::Relaxed) != now {
        crate::log_info!("input", "soft keyboard: {now}px of window covered");
    }
}

#[derive(Default)]
pub(crate) struct Nav {
    pub up: bool,
    pub down: bool,
    pub page_up: bool,
    pub page_down: bool,
}

pub(crate) fn take_nav() -> Nav {
    Nav::default()
}

#[cfg(target_os = "android")]
pub(crate) mod android {
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

    use bevy::android::android_activity::AndroidApp;
    use jni::{Env, JValue, JavaVM, errors::Result, jni_sig, jni_str, objects::JObject};

    static WANTED: AtomicBool = AtomicBool::new(false);

    pub(super) fn want(on: bool) {
        WANTED.store(on, Ordering::Relaxed);
        crate::log_info!(
            "input",
            "soft keyboard: {}",
            if on { "show" } else { "hide" }
        );
        let Some(app) = bevy::android::ANDROID_APP.get() else {
            crate::log_warn!("input", "soft keyboard: no AndroidApp yet");
            return;
        };
        if on {
            app.show_soft_input(false);
        } else {
            app.hide_soft_input(false);
        }
    }

    pub(super) fn wanted() -> bool {
        WANTED.load(Ordering::Relaxed)
    }

    static DISMISSED: AtomicBool = AtomicBool::new(false);
    static PINNED: AtomicBool = AtomicBool::new(false);

    pub(super) fn set_pinned(on: bool) {
        PINNED.store(on, Ordering::Relaxed);
    }

    pub(super) fn pinned() -> bool {
        PINNED.load(Ordering::Relaxed)
    }
    static TAP_OUTSIDE: AtomicBool = AtomicBool::new(false);
    static TAP_INSIDE: AtomicBool = AtomicBool::new(false);

    pub(super) fn set_dismissed(on: bool) {
        DISMISSED.store(on, Ordering::Relaxed);
    }

    pub(super) fn dismissed() -> bool {
        DISMISSED.load(Ordering::Relaxed)
    }

    pub(super) fn note_tap(inside: bool) {
        if inside {
            TAP_INSIDE.store(true, Ordering::Relaxed);
            DISMISSED.store(false, Ordering::Relaxed);
        } else {
            TAP_OUTSIDE.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn take_outside_tap() -> bool {
        let outside = TAP_OUTSIDE.swap(false, Ordering::Relaxed);
        let inside = TAP_INSIDE.swap(false, Ordering::Relaxed);
        outside && !inside
    }

    pub(super) fn inset_px() -> f32 {
        const POLL_MS: u64 = 50;
        static CACHED: AtomicI32 = AtomicI32::new(0);
        static NEXT_READ_MS: AtomicU64 = AtomicU64::new(0);

        let now = crate::platform::time::epoch().elapsed().as_millis() as u64;
        if now >= NEXT_READ_MS.load(Ordering::Relaxed) {
            NEXT_READ_MS.store(now + POLL_MS, Ordering::Relaxed);
            CACHED.store(read_ime_inset().unwrap_or(0), Ordering::Relaxed);
        }
        CACHED.load(Ordering::Relaxed) as f32
    }

    #[allow(
        unsafe_code,
        reason = "the JNI handles Android hands over are raw pointers"
    )]
    fn read_ime_inset() -> Option<i32> {
        let app = bevy::android::ANDROID_APP.get()?;
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        vm.attach_current_thread(|env| {
            let bottom = ime_bottom(env, app);
            if bottom.is_err() {
                env.exception_clear();
            }
            bottom
        })
        .ok()
    }

    #[allow(
        unsafe_code,
        reason = "the JNI handles Android hands over are raw pointers"
    )]
    fn ime_bottom(env: &mut Env<'_>, app: &AndroidApp) -> Result<i32> {
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
        let window = env
            .call_method(
                &activity,
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        let decor = env
            .call_method(
                &window,
                jni_str!("getDecorView"),
                jni_sig!("()Landroid/view/View;"),
                &[],
            )?
            .l()?;
        let insets = env
            .call_method(
                &decor,
                jni_str!("getRootWindowInsets"),
                jni_sig!("()Landroid/view/WindowInsets;"),
                &[],
            )?
            .l()?;
        let kind = env
            .call_static_method(
                jni_str!("android/view/WindowInsets$Type"),
                jni_str!("ime"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        let ime = env
            .call_method(
                &insets,
                jni_str!("getInsets"),
                jni_sig!("(I)Landroid/graphics/Insets;"),
                &[JValue::Int(kind)],
            )?
            .l()?;
        env.get_field(&ime, jni_str!("bottom"), jni_sig!("I"))?.i()
    }
}
