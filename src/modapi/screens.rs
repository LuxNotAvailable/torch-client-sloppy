//! Mod menu screens (`Plugin::screen`): the extra data NeoForge opens a menu
//! with, kept until the GUI draws it, and the open screen's layout, asked
//! for again when the menu's data or slot count changes.

use std::sync::Arc;

use parking_lot::Mutex;

/// NeoForge open data by window id, until the GUI takes it.
static OPEN_DATA: Mutex<Vec<(i32, Vec<u8>)>> = Mutex::new(Vec::new());

/// The open modded screen and what its layout was asked with.
static OPEN: Mutex<Option<Open>> = Mutex::new(None);

struct Open {
    window: i32,
    kind: u32,
    extra: Vec<u8>,
    asked: Option<(Vec<i32>, usize)>,
    screen: Option<Arc<torch_mod_api::Screen>>,
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
/// again only when `data` or `slots` change.
pub(crate) fn current(
    window: i32,
    kind: u32,
    title: &str,
    data: &[i32],
    slots: usize,
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
            asked: None,
            screen: None,
        });
    }
    let open = guard.as_mut()?;
    let inputs = (data.to_vec(), slots);
    if open.asked.as_ref() == Some(&inputs) {
        return open.screen.clone();
    }
    let menu = crate::protocol::modded_menu(kind)?;
    let view = torch_mod_api::ScreenView {
        menu: menu.to_owned(),
        title: title.to_owned(),
        extra: open.extra.clone(),
        data: inputs.0.clone(),
        slots,
    };
    open.asked = Some(inputs);
    open.screen = super::screen(&view).map(Arc::new);
    open.screen.clone()
}

/// A button on the open plugin screen pressed: the plugin hears it, and
/// the screen it answers is shown from then on.
pub(crate) fn press(window: i32, button: &str, right: bool, shift: bool) {
    let mut guard = OPEN.lock();
    let Some(open) = guard.as_mut().filter(|o| o.window == window) else {
        return;
    };
    let Some(menu) = crate::protocol::modded_menu(open.kind) else {
        return;
    };
    let (data, slots) = open.asked.clone().unwrap_or_default();
    let view = torch_mod_api::ButtonView {
        button: button.to_owned(),
        right,
        shift,
        screen: torch_mod_api::ScreenView {
            menu: menu.to_owned(),
            title: String::new(),
            extra: open.extra.clone(),
            data,
            slots,
        },
    };
    // The plugin may call back into the host; not under this lock.
    drop(guard);
    let answer = super::button(&view);
    if let Some(screen) = answer
        && let Some(open) = OPEN.lock().as_mut().filter(|o| o.window == window)
    {
        open.screen = Some(Arc::new(screen));
    }
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
