//! `SocialInteractionsScreen`: the players on the server, each with a
//! button to hide (or show again) their chat messages. Hidden players are
//! kept for the session, as vanilla's `PlayerSocialManager` does.

use crate::gui::scroll_list::ListArea;
use std::collections::HashSet;
use std::sync::Mutex;

use crate::gui::painter::Painter;
use crate::gui::widgets::{self, Button, HeaderFooter, WIDGET_HEIGHT, WIDGET_WIDTH_BIG};
use crate::gui::{Screen, ScreenCtx};

const LAYOUT: HeaderFooter =
    HeaderFooter::new(HeaderFooter::DEFAULT_H + 24.0, HeaderFooter::DEFAULT_H);
const ROW: f32 = 24.0;
const TABS: [&str; 2] = ["All", "Hidden"];

static HIDDEN: Mutex<Option<HashSet<u128>>> = Mutex::new(None);

/// A new session: hidden players are per server, as vanilla's.
pub fn reset() {
    *HIDDEN.lock().unwrap() = None;
}

/// Whether this player's chat is hidden.
pub fn is_hidden(uuid: u128) -> bool {
    HIDDEN
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|h| h.contains(&uuid))
}

fn set_hidden(uuid: u128, hidden: bool) {
    let mut guard = HIDDEN.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    if hidden {
        set.insert(uuid);
    } else {
        set.remove(&uuid);
    }
}

#[derive(Default)]
pub struct SocialState {
    pub tab: usize,
    pub list: crate::gui::scroll_list::ScrollList,
}

pub fn draw(
    p: &mut Painter,
    state: &mut SocialState,
    ctx: &ScreenCtx,
    tab_list: &crate::client::tablist::TabList,
    own: u128,
) -> Option<Screen> {
    crate::gui::menu::background(p, ctx.vw, ctx.vh);
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Social Interactions");
    if let Some(tab) = widgets::tab_bar(p, ctx, &TABS, state.tab) {
        state.tab = tab;
    }
    let area = ListArea::centered(ctx, LAYOUT.content_rect(ctx.vw, ctx.vh), 238.0);
    let (left, width) = (area.left, area.width);
    let rows: Vec<_> = tab_list
        .rows
        .iter()
        .filter(|r| r.uuid != own)
        .filter(|r| state.tab == 0 || is_hidden(r.uuid))
        .collect();
    if rows.is_empty() {
        let text = if state.tab == 0 {
            "No other players online"
        } else {
            "No hidden players"
        };
        let w = p.atlas.font.width_str(text);
        p.text_str(
            text,
            ((ctx.vw - w) / 2.0).floor(),
            area.y + 12.0,
            0xA0A0A0,
            true,
        );
    }
    state.list.rows(p, ctx, &area, rows.len(), ROW, |p, i, y| {
        let row = rows[i];
        p.fill(left, y + 1.0, width, ROW - 2.0, 0x4000_0000);
        p.text(&row.name, left + 6.0, y + 8.0, true);
        let hidden = is_hidden(row.uuid);
        let label = if hidden { "Show" } else { "Hide" };
        if Button::new(left + width - 46.0, y + 2.0, 42.0, 20.0, label).draw(p, ctx) {
            set_hidden(row.uuid, !hidden);
        }
    });
    let x = widgets::centered_x(ctx.vw);
    let pressed = Button::new(
        x,
        LAYOUT.footer_y(ctx.vh),
        WIDGET_WIDTH_BIG,
        WIDGET_HEIGHT,
        "Done",
    )
    .draw(p, ctx);
    (pressed || ctx.input.escape).then_some(Screen::None)
}
