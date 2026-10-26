use std::sync::atomic::{AtomicBool, Ordering};

use super::registry::{Id, zoom as setting};
use super::store;

static TOUCH: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "mobile_ui")]
pub fn set_touch(on: bool) {
    TOUCH.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub on: bool,
    pub fov: f32,
    pub duration: f32,
}

pub fn settings() -> Settings {
    let s = store();
    Settings {
        on: s.enabled(Id::Zoom) || TOUCH.load(Ordering::Relaxed),
        fov: s.num(setting::FOV),
        duration: s.num(setting::DURATION).max(0.001),
    }
}
