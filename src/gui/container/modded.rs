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

/// A plugin screen's client-side widgets: its text fields (with the text
/// the plugin last gave each), the scroll bar being dragged, and a close
/// the plugin asked for. Reset when another window opens.
#[derive(Default)]
pub struct PluginUi {
    window: i32,
    fields: std::collections::HashMap<String, (crate::gui::widgets::TextBox, String)>,
    drag: Option<String>,
    /// The plugin answered `close`: the screen closes as Escape would.
    pub close: bool,
}

impl PluginUi {
    /// A text field has focus: keys are typing, not binds.
    pub fn typing(&self) -> bool {
        self.fields.values().any(|(f, _)| f.focused)
    }
}

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

/// The menu's first player slot: `player` (azalea's match), else the last
/// 36.
fn player_start(count: usize, player: Option<usize>) -> usize {
    player
        .filter(|p| p + PLAYER_SLOTS <= count)
        .unwrap_or(count.saturating_sub(PLAYER_SLOTS))
}

/// Slot positions for the plain grid: the menu's own slots nine to a row
/// (those after the player's too, as upgrade slots), then the inventory.
fn plain_layout(count: usize, player: Option<usize>) -> Placed {
    let player = player_start(count, player);
    let rows = rows(count);
    let inv_y = 18.0 + rows as f32 * SLOT_PITCH + 13.0;
    let slots = (0..count)
        .map(|i| {
            Some(if (player..player + PLAYER_SLOTS).contains(&i) {
                standard_inventory(i - player, 8.0, inv_y)
            } else {
                let own = if i < player { i } else { i - PLAYER_SLOTS };
                (
                    8.0 + (own % 9) as f32 * SLOT_PITCH,
                    18.0 + (own / 9) as f32 * SLOT_PITCH,
                )
            })
        })
        .collect();
    Placed {
        size: (CONTAINER_W, 114.0 + rows as f32 * SLOT_PITCH),
        slots,
    }
}

/// Slot positions from a plugin's screen. Player slots it leaves out sit
/// where vanilla menus put them: from x 8, y `height - 82`.
fn screen_layout(screen: &torch_mod_api::Screen, count: usize, found: Option<usize>) -> Placed {
    // azalea's match first: the plugin's start only fills in for none.
    let player = player_start(count, found.or(screen.player_start));
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
    let gui = [ctx.vw as i32, ctx.vh as i32];
    let screen = crate::modapi::screens::current(
        snap.container_id,
        kind,
        &title,
        &data,
        count,
        gui,
        snap.container_player_start,
    );
    *PLACED.write() = match &screen {
        Some(screen) => screen_layout(screen, count, snap.container_player_start),
        None => plain_layout(count, snap.container_player_start),
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
        blit(p, b, (left, top));
    }
    let mut events = Vec::new();
    let (pressed, tooltip) = draw_buttons(p, ctx, screen, (left, top));
    if let Some((button, menu_button)) = pressed {
        if let Some(id) = menu_button.and_then(|id| u8::try_from(id).ok()) {
            out.push(InvAction::ButtonClick(id));
        }
        events.push(torch_mod_api::ScreenInput::Button {
            id: button,
            right: ctx.input.right_click,
            shift: ctx.input.shift,
        });
    }
    let ui = &mut state.container.plugin;
    if ui.window != snap.container_id {
        *ui = PluginUi {
            window: snap.container_id,
            ..PluginUi::default()
        };
    }
    text_fields(p, ctx, ui, screen, (left, top), &mut events);
    scroll_bars(p, ctx, ui, screen, (left, top), &mut events);
    let hovered = slots::panel(
        p,
        Layout::Modded,
        (left, top),
        ctx,
        snap,
        &mut state.slots,
        out,
    );
    // Clicks on the plugin's own slots are its, not the server's (and any
    // pickup the slot panel previewed is dropped).
    if !screen.plugin_slots.is_empty() {
        let before = events.len();
        out.retain(|action| {
            let InvAction::Click(op) = action else {
                return true;
            };
            let Some(slot) = op.slot_num().map(u32::from) else {
                return true;
            };
            if !screen.plugin_slots.contains(&slot) {
                return true;
            }
            use azalea_inventory::operations::{ClickOperation, PickupClick};
            events.push(torch_mod_api::ScreenInput::SlotClick {
                slot,
                right: matches!(op, ClickOperation::Pickup(PickupClick::Right { .. })),
                shift: matches!(op, ClickOperation::QuickMove(_)),
            });
            false
        });
        if events.len() > before {
            state.slots.cursor = None;
        }
    }
    for d in &screen.overlay {
        draw_overlay(p, d, (left, top));
    }
    for input in events {
        let answer = crate::modapi::screens::input(snap.container_id, input);
        state.container.plugin.close |= answer.close;
    }
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
    if let (Some(lines), Some(m)) = (tooltip, ctx.mouse()) {
        let lines: Vec<Vec<crate::text::Span>> = lines
            .iter()
            .map(|l| crate::text::parse_formatted(l))
            .collect();
        crate::gui::tooltip::draw_lines(p, &lines, m.x, m.y, ctx.vw, ctx.vh);
    }
    hovered
}

/// One overlay drawing.
fn draw_overlay(p: &mut Painter, d: &torch_mod_api::Draw, (left, top): (f32, f32)) {
    match d {
        torch_mod_api::Draw::Blit(b) => blit(p, b, (left, top)),
        torch_mod_api::Draw::Fill {
            x,
            y,
            width,
            height,
            color,
        } => p.fill(
            left + *x as f32,
            top + *y as f32,
            *width as f32,
            *height as f32,
            *color,
        ),
        torch_mod_api::Draw::Item { item, count, x, y } => {
            let (x, y) = (left + *x as f32, top + *y as f32);
            p.item_icon(item.strip_prefix("minecraft:").unwrap_or(item), x, y);
            let count = u8::try_from(*count).unwrap_or(u8::MAX);
            p.item_decorations(x, y, count, 0, 0);
        }
    }
}

/// The screen's text fields: focused by a click, edited by the client,
/// each change and Enter told to the plugin.
fn text_fields(
    p: &mut Painter,
    ctx: &ScreenCtx,
    ui: &mut PluginUi,
    screen: &torch_mod_api::Screen,
    (left, top): (f32, f32),
    events: &mut Vec<torch_mod_api::ScreenInput>,
) {
    let frame = p.frame;
    ui.fields
        .retain(|id, _| screen.text_fields.iter().any(|f| &f.id == id));
    for f in &screen.text_fields {
        let (x, y, w, h) = (
            left + f.x as f32,
            top + f.y as f32,
            f.width as f32,
            f.height as f32,
        );
        let max = if f.max_length == 0 { 32 } else { f.max_length };
        let (field, given) = ui.fields.entry(f.id.clone()).or_insert_with(|| {
            let mut field = crate::gui::widgets::TextBox::new(max, &f.hint);
            field.set_text(&f.text);
            (field, f.text.clone())
        });
        // The plugin set new text (a cleared search).
        if *given != f.text {
            field.set_text(&f.text);
            *given = f.text.clone();
        }
        field.max_len = max;
        field.bordered = f.bordered;
        field.placeholder.clone_from(&f.hint);
        if ctx.input.left_click {
            field.focused = ctx.hovering(x, y, w, h);
        }
        field.handle_mouse(ctx.input, &p.atlas.font, x, y, w, h);
        if field.focused {
            if field.handle_input(ctx.input) {
                given.clone_from(&field.text);
                events.push(torch_mod_api::ScreenInput::Text {
                    id: f.id.clone(),
                    text: field.text.clone(),
                });
            }
            if ctx.input.enter {
                events.push(torch_mod_api::ScreenInput::Submit {
                    id: f.id.clone(),
                    text: field.text.clone(),
                });
            }
        }
        field.draw(p, x, y, w, h, frame);
    }
}

/// The screen's scroll bars: the wheel over a bar's area steps it, a drag
/// of its track moves it; a new value is told to the plugin, whose answer
/// shows it.
fn scroll_bars(
    p: &mut Painter,
    ctx: &ScreenCtx,
    ui: &mut PluginUi,
    screen: &torch_mod_api::Screen,
    (left, top): (f32, f32),
    events: &mut Vec<torch_mod_api::ScreenInput>,
) {
    if !ctx.input.left_down {
        ui.drag = None;
    }
    for sb in &screen.scrolls {
        let (x, y, w, h) = (
            left + sb.x as f32,
            top + sb.y as f32,
            sb.width as f32,
            sb.height as f32,
        );
        let max = sb.max.max(0);
        let thumb = if sb.thumb_height > 0 {
            sb.thumb_height as f32
        } else {
            15.0
        };
        let [ax, ay, aw, ah] = sb.area.unwrap_or([sb.x, sb.y, sb.width, sb.height]);
        let over_area = ctx.hovering(left + ax as f32, top + ay as f32, aw as f32, ah as f32);
        if ctx.input.left_click && ctx.hovering(x, y, w, h) && max > 0 {
            ui.drag = Some(sb.id.clone());
        }
        let mut value = sb.value.clamp(0, max);
        if ui.drag.as_deref() == Some(sb.id.as_str())
            && let Some(m) = ctx.mouse()
        {
            let t = ((m.y - y - thumb / 2.0) / (h - thumb).max(1.0)).clamp(0.0, 1.0);
            value = (t * max as f32).round() as i32;
        } else if ctx.input.scroll != 0.0 && over_area {
            value = (value - ctx.input.scroll.signum() as i32).clamp(0, max);
        }
        if value != sb.value.clamp(0, max) {
            events.push(torch_mod_api::ScreenInput::Scroll {
                id: sb.id.clone(),
                value,
            });
        }
        let at = if max > 0 {
            (h - thumb) * sb.value.clamp(0, max) as f32 / max as f32
        } else {
            0.0
        };
        if sb.thumb.is_empty() {
            let sprite = if max > 0 {
                "container/creative_inventory/scroller"
            } else {
                "container/creative_inventory/scroller_disabled"
            };
            p.sprite(sprite, x, y + at, 12.0, thumb);
        } else {
            for b in &sb.thumb {
                blit(p, b, (x, y + at));
            }
        }
    }
}

fn blit(p: &mut Painter, b: &torch_mod_api::Blit, (left, top): (f32, f32)) {
    p.texture_part(
        &b.texture,
        (b.u, b.v, b.width as f32, b.height as f32),
        (b.texture_width as f32, b.texture_height as f32),
        left + b.x as f32,
        top + b.y as f32,
    );
}

/// The screen's buttons' looks (vanilla's, labelled, when they give none).
/// The one clicked this frame, with its `menu_button`, and the tooltip of
/// the one under the cursor (drawn last, over the slots).
fn draw_buttons<'s>(
    p: &mut Painter,
    ctx: &ScreenCtx,
    screen: &'s torch_mod_api::Screen,
    (left, top): (f32, f32),
) -> (Option<(String, Option<i32>)>, Option<&'s Vec<String>>) {
    let mut pressed = None;
    let mut tooltip = None;
    for b in &screen.buttons {
        let (x, y, w, h) = (
            left + b.x as f32,
            top + b.y as f32,
            b.width as f32,
            b.height as f32,
        );
        let hot = ctx.hovering(x, y, w, h);
        if b.look.is_empty() {
            crate::gui::widgets::Button::new(x, y, w, h, &b.label).draw(p, ctx);
        } else {
            let look = if hot && !b.hover.is_empty() {
                &b.hover
            } else {
                &b.look
            };
            for blit_ in look {
                blit(p, blit_, (left, top));
            }
            if !b.label.is_empty() {
                let tw = p.atlas.font.width_str(&b.label);
                p.text_plain(
                    &b.label,
                    x + ((w - tw) / 2.0).floor(),
                    y + ((h - 8.0) / 2.0).floor(),
                    0xFFFFFF,
                    true,
                );
            }
        }
        if hot {
            if ctx.input.left_click || ctx.input.right_click {
                pressed = Some((b.id.clone(), b.menu_button));
            }
            if !b.tooltip.is_empty() {
                tooltip = Some(&b.tooltip);
            }
        }
    }
    (pressed, tooltip)
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
        let placed = plain_layout(12 + PLAYER_SLOTS, None);
        assert_eq!(placed.size, (176.0, 150.0));
        assert_eq!(placed.slots[9], Some((8.0, 36.0)));
        // The inventory's first slot, then the hotbar's.
        assert_eq!(placed.slots[12], Some((8.0, 67.0)));
        assert_eq!(placed.slots[12 + 27], Some((8.0, 125.0)));
    }

    #[test]
    fn slots_after_the_player_s_join_the_menu_s_grid() {
        // 4 storage slots, the player's 36, then 3 upgrade slots: one row.
        let placed = plain_layout(4 + PLAYER_SLOTS + 3, Some(4));
        assert_eq!(placed.slots[3], Some((62.0, 18.0)));
        assert_eq!(placed.slots[4], Some((8.0, 49.0)));
        assert_eq!(placed.slots[4 + PLAYER_SLOTS], Some((80.0, 18.0)));
        assert_eq!(placed.slots[4 + PLAYER_SLOTS + 2], Some((116.0, 18.0)));
    }

    #[test]
    fn a_screen_s_missing_player_slots_sit_where_vanilla_s_do() {
        let screen = torch_mod_api::Screen {
            width: 176,
            height: 166,
            slots: vec![Some([56, 17]), None],
            ..Default::default()
        };
        let placed = screen_layout(&screen, 2 + PLAYER_SLOTS, None);
        assert_eq!(placed.slots[0], Some((56.0, 17.0)));
        assert_eq!(placed.slots[1], None);
        assert_eq!(placed.slots[2], Some((8.0, 84.0)));
        assert_eq!(placed.slots[2 + 27], Some((8.0, 142.0)));
        assert_eq!(slot_pos(usize::MAX), HIDDEN);
    }
}
