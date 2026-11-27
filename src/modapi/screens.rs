//! Mod menu screens (`Plugin::screen`): the extra data NeoForge opens a menu
//! with, kept until the GUI draws it, and the open screen's layout, asked
//! for again when the menu's data or slot count changes.

use std::sync::Arc;

use parking_lot::Mutex;

/// NeoForge open data by window id, until the GUI takes it.
static OPEN_DATA: Mutex<Vec<(i32, Vec<u8>)>> = Mutex::new(Vec::new());

/// The open modded screen and what its layout was asked with.
static OPEN: Mutex<Option<Open>> = Mutex::new(None);

/// A plugin asked for its open screen again (`Redraw::screen`).
static REFRESH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct Open {
    window: i32,
    kind: u32,
    extra: Vec<u8>,
    title: String,
    /// The menu's data, slot count and GUI size the screen was asked with.
    asked: Option<(Vec<i32>, usize, [i32; 2])>,
    screen: Option<Arc<torch_mod_api::Screen>>,
}

impl Open {
    fn view(&self) -> Option<torch_mod_api::ScreenView> {
        let (data, slots, window) = self.asked.clone().unwrap_or_default();
        Some(torch_mod_api::ScreenView {
            menu: crate::protocol::modded_menu(self.kind)?.to_owned(),
            title: self.title.clone(),
            extra: self.extra.clone(),
            data,
            slots,
            window,
        })
    }
}

/// `neoforge:advanced_open_screen`'s data for `window` (the hop reads it).
pub(crate) fn on_open_data(window: i32, data: Vec<u8>) {
    let mut open = OPEN_DATA.lock();
    open.retain(|(w, _)| *w != window);
    open.push((window, data));
    // Only the latest few opens matter.
    if open.len() > 8 {
        open.remove(0);
    }
}

pub(crate) fn take_open_data(window: i32) -> Option<Vec<u8>> {
    let mut open = OPEN_DATA.lock();
    let at = open.iter().position(|(w, _)| *w == window)?;
    Some(open.remove(at).1)
}

/// The plugin's screen for the open modded menu (`kind`, the server's
/// menu type id), or `None` for the client's plain grid. Asks the plugin
/// again when the menu's data, its slot count or the GUI's size change,
/// or the plugin asked to be ([`refresh`]).
pub(crate) fn current(
    window: i32,
    kind: u32,
    title: &str,
    data: &[i32],
    slots: usize,
    gui: [i32; 2],
) -> Option<Arc<torch_mod_api::Screen>> {
    let mut guard = OPEN.lock();
    // New open data means a new open, even of a reused window id.
    let fresh = take_open_data(window);
    let same = guard
        .as_ref()
        .is_some_and(|o| o.window == window && o.kind == kind);
    if fresh.is_some() || !same {
        *guard = Some(Open {
            window,
            kind,
            extra: fresh.unwrap_or_default(),
            title: title.to_owned(),
            asked: None,
            screen: None,
        });
    }
    let open = guard.as_mut()?;
    let inputs = (data.to_vec(), slots, gui);
    let refresh = REFRESH.swap(false, std::sync::atomic::Ordering::Relaxed);
    if open.asked.as_ref() == Some(&inputs) && !refresh {
        return open.screen.clone();
    }
    open.title = title.to_owned();
    open.asked = Some(inputs);
    let view = open.view()?;
    // The plugin may call back into the host; not under this lock.
    drop(guard);
    let screen = super::screen(&view).map(Arc::new);
    let mut guard = OPEN.lock();
    let open = guard.as_mut().filter(|o| o.window == window)?;
    open.screen = screen;
    open.screen.clone()
}

/// The open screen asked for again on its next draw.
pub(crate) fn refresh() {
    REFRESH.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether `window` shows a plugin's screen.
pub(crate) fn is_open(window: i32) -> bool {
    OPEN.lock()
        .as_ref()
        .is_some_and(|o| o.window == window && o.screen.is_some())
}

/// Input on the open plugin screen: the plugin hears it, the screen it
/// answers is shown from then on, and its answer comes back (consumed,
/// close).
pub(crate) fn input(window: i32, input: torch_mod_api::ScreenInput) -> torch_mod_api::ScreenAnswer {
    let view = {
        let guard = OPEN.lock();
        let Some(view) = guard
            .as_ref()
            .filter(|o| o.window == window && o.screen.is_some())
            .and_then(Open::view)
        else {
            return Default::default();
        };
        view
    };
    // The plugin may call back into the host; not under this lock.
    let mut answer = super::screen_input(&torch_mod_api::ScreenEvent {
        screen: view,
        input,
    });
    if let Some(screen) = answer.screen.take()
        && let Some(open) = OPEN.lock().as_mut().filter(|o| o.window == window)
    {
        open.screen = Some(Arc::new(screen));
    }
    answer
}

/// The open screen's window and where its player slots start, when its
/// plugin says (`Screen::player_start`).
pub(crate) fn player_start() -> Option<(i32, usize)> {
    let open = OPEN.lock();
    let open = open.as_ref()?;
    Some((open.window, open.screen.as_ref()?.player_start?))
}

/// Forgets everything (disconnect).
pub(crate) fn reset() {
    OPEN_DATA.lock().clear();
    *OPEN.lock() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_data_waits_for_its_window() {
        on_open_data(91, vec![1]);
        on_open_data(92, vec![2]);
        on_open_data(91, vec![3]);
        assert_eq!(take_open_data(91), Some(vec![3]));
        assert_eq!(take_open_data(91), None);
        assert_eq!(take_open_data(92), Some(vec![2]));
    }
}
