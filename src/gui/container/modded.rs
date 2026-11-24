//! A mod's menu screen. With a plugin's `Screen` (`modapi::screens`) it
//! draws as Java's `AbstractContainerScreen` does: `renderBg`'s blits, the
//! slots where the menu puts them, then the labels. Without one, the
//! menu's slots sit in a chest-style grid above the inventory.

use std::sync::Arc;

use parking_lot::RwLock;

use super::{
    CONTAINER_W, GuiState, InvAction, LABEL_COLOR, Layout, Painter, ScreenCtx, SlotStack, Snapshot,
    dim_background, draw_title, inventory_label, slots, window_origin,
};
use crate::gui::slots::{PLAYER_SLOTS, SLOT_PITCH, standard_inventory};

/// Where a hidden slot goes: off every screen.
const HIDDEN: (f32, f32) = (-10_000.0, -10_000.0);

/// The open modded screen's size and slot positions, for `Layout::Modded`.
#[derive(Default)]
struct Placed {
    size: (f32, f32),
    slots: Vec<Option<(f32, f32)>>,
}

static PLACED: RwLock<Placed> = RwLock::new(Placed {
    size: (0.0, 0.0),
    slots: Vec::new(),
});

pub(crate) fn size() -> (f32, f32) {
    PLACED.read().size
}

pub(crate) fn slot_count() -> usize {
    PLACED.read().slots.len()
}

pub(crate) fn slot_pos(i: usize) -> (f32, f32) {
    PLACED
        .read()
        .slots
        .get(i)
        .copied()
        .flatten()
        .unwrap_or(HIDDEN)
}

/// The plain grid's rows for a menu of `count` slots, the player's 36
/// among them.
fn rows(count: usize) -> usize {
    count.saturating_sub(PLAYER_SLOTS).div_ceil(9).max(1)
}

/// Slot positions for the plain grid: the menu's own nine to a row, then
/// the inventory.
fn plain_layout(count: usize) -> Placed {
    let own = count.saturating_sub(PLAYER_SLOTS);
    let rows = rows(count);
    let inv_y = 18.0 + rows as f32 * SLOT_PITCH + 13.0;
    let slots = (0..count)
        .map(|i| {
            Some(if i < own {
                (
                    8.0 + (i % 9) as f32 * SLOT_PITCH,
                    18.0 + (i / 9) as f32 * SLOT_PITCH,
                )
            } else {
                standard_inventory(i - own, 8.0, inv_y)
            })
        })
        .collect();
    Placed {
        size: (CONTAINER_W, 114.0 + rows as f32 * SLOT_PITCH),
        slots,
    }
}

/// Slot positions from a plugin's screen. Player slots it leaves out sit
/// where vanilla menus put them: 8 across, `height - 82` down.
fn screen_layout(screen: &torch_mod_api::Screen, count: usize) -> Placed {
    let player = screen
        .player_start
        .unwrap_or(count.saturating_sub(PLAYER_SLOTS));
    let inv = (8.0, screen.height as f32 - 82.0);
    let slots = (0..count)
        .map(|i| match screen.slots.get(i) {
            Some(Some([x, y])) => Some((*x as f32, *y as f32)),
            Some(None) => None,
            None if (player..player + PLAYER_SLOTS).contains(&i) => {
                Some(standard_inventory(i - player, inv.0, inv.1))
            }
            None => None,
        })
        .collect();
    Placed {
        size: (screen.width as f32, screen.height as f32),
        slots,
    }
}

pub fn draw(
    p: &mut Painter,
    state: &mut GuiState,
    ctx: &ScreenCtx,
    snap: &Snapshot,
    kind: u32,
    out: &mut Vec<InvAction>,
) -> Option<SlotStack> {
    let title: String = snap.container_title.iter().map(|s| &*s.text).collect();
    let data: Vec<i32> = snap.container_data.iter().map(|v| i32::from(*v)).collect();
    let count = snap.menu_slots.len();
    let screen = crate::modapi::screens::current(snap.container_id, kind, &title, &data, count);
    *PLACED.write() = match &screen {
        Some(screen) => screen_layout(screen, count),
        None => plain_layout(count),
    };
    let (left, top) = window_origin(Layout::Modded, ctx);
    dim_background(p, ctx.vw, ctx.vh);
    match screen {
        Some(screen) => draw_screen(p, state, ctx, snap, &screen, (left, top), out),
        None => draw_plain(p, state, ctx, snap, count, (left, top), out),
    }
}

fn draw_screen(
    p: &mut Painter,
    state: &mut GuiState,
    ctx: &ScreenCtx,
    snap: &Snapshot,
    screen: &Arc<torch_mod_api::Screen>,
    (left, top): (f32, f32),
    out: &mut Vec<InvAction>,
) -> Option<SlotStack> {
    for b in &screen.background {
        p.texture_part(
            &b.texture,
            (b.u, b.v, b.width as f32, b.height as f32),
            (b.texture_width as f32, b.texture_height as f32),
            left + b.x as f32,
            top + b.y as f32,
        );
    }
    let hovered = slots::panel(
        p,
        Layout::Modded,
        (left, top),
        ctx,
        snap,
        &mut state.slots,
        out,
    );
    if !screen.hide_title {
        let [x, y] = screen.title_at.unwrap_or([8, 6]);
        draw_title(p, &snap.container_title, left + x as f32, top + y as f32);
    }
    if !screen.hide_inventory_label {
        let [x, y] = screen.inventory_label_at.unwrap_or([8, screen.height - 94]);
        p.text_plain(
            inventory_label(),
            left + x as f32,
            top + y as f32,
            LABEL_COLOR,
            false,
        );
    }
    for t in &screen.texts {
        p.text_plain(
            &t.text,
            left + t.x as f32,
            top + t.y as f32,
            t.color,
            t.shadow,
        );
    }
    hovered
}

fn draw_plain(
    p: &mut Painter,
    state: &mut GuiState,
    ctx: &ScreenCtx,
    snap: &Snapshot,
    count: usize,
    (left, top): (f32, f32),
    out: &mut Vec<InvAction>,
) -> Option<SlotStack> {
    let rows = rows(count);
    // The chest sheet: its top edge, a slot row per row, then the inventory.
    let sheet = "container/generic_54";
    p.blit_sheet(sheet, 0.0, 0.0, CONTAINER_W, 17.0, left, top);
    for r in 0..rows {
        let y = top + 17.0 + r as f32 * SLOT_PITCH;
        p.blit_sheet(sheet, 0.0, 17.0, CONTAINER_W, SLOT_PITCH, left, y);
    }
    let bottom = top + 17.0 + rows as f32 * SLOT_PITCH;
    p.blit_sheet(sheet, 0.0, 126.0, CONTAINER_W, 96.0, left, bottom);
    let hovered = slots::panel(
        p,
        Layout::Modded,
        (left, top),
        ctx,
        snap,
        &mut state.slots,
        out,
    );
    let img_h = 114.0 + rows as f32 * SLOT_PITCH;
    super::draw_labels(p, snap, left, top, img_h, 8.0);
    hovered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_grid_puts_the_inventory_under_the_menu_s_rows() {
        // 12 slots of the mod's: two rows.
        let placed = plain_layout(12 + PLAYER_SLOTS);
        assert_eq!(placed.size, (176.0, 150.0));
        assert_eq!(placed.slots[9], Some((8.0, 36.0)));
        // The inventory's first slot, then the hotbar's.
        assert_eq!(placed.slots[12], Some((8.0, 67.0)));
        assert_eq!(placed.slots[12 + 27], Some((8.0, 125.0)));
    }

    #[test]
    fn a_screen_s_missing_player_slots_sit_where_vanilla_s_do() {
        let screen = torch_mod_api::Screen {
            width: 176,
            height: 166,
            slots: vec![Some([56, 17]), None],
            ..Default::default()
        };
        let placed = screen_layout(&screen, 2 + PLAYER_SLOTS);
        assert_eq!(placed.slots[0], Some((56.0, 17.0)));
        assert_eq!(placed.slots[1], None);
        assert_eq!(placed.slots[2], Some((8.0, 84.0)));
        assert_eq!(placed.slots[2 + 27], Some((8.0, 142.0)));
        assert_eq!(slot_pos(usize::MAX), HIDDEN);
    }
}
