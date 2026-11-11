//! The recipe viewer, after Just Enough Items: beside an inventory or a
//! container, every item in a searchable, paged list, and a window showing
//! an item's recipes (R, or a left click) or uses (U, or a right click).
//! Bookmarks (A) sit in a list on the left. Ctrl+O hides it all.
//!
//! Searching takes words that must all match: a plain word matches the
//! item's name or ID, `@word` its namespace, `#word` one of its tags.
//!
//! Recipes come from `items::recipe_index`, the game's own. There is no
//! cheat mode. A recipe's "+" moves its ingredients into the open
//! workstation by the clicks a player would make, so any server takes it.

use bevy::prelude::KeyCode;

use crate::generated_items::{CREATIVE_TABS, ITEMS, item};
use crate::gui::painter::Painter;
use crate::gui::widgets::{Button, TextBox};
use crate::gui::{Screen, ScreenCtx};
use crate::items::recipe_index::{self, Category, Recipe};
use crate::session::{ContainerKind, FurnaceKind, InvAction, SlotStack};

const CELL: f32 = 18.0;
/// The item list's most columns, as JEI's default.
const MAX_COLUMNS: usize = 9;
const MARGIN: f32 = 4.0;
const BAR_H: f32 = 18.0;
const SEARCH_H: f32 = 16.0;
const WINDOW_W: f32 = 176.0;
const TAB_W: f32 = 24.0;
const TAB_H: f32 = 22.0;

const PANEL: u32 = 0xFFC6_C6C6;
const LIGHT: u32 = 0xFFFF_FFFF;
const SHADE: u32 = 0xFF55_5555;
const EDGE: u32 = 0xFF00_0000;
const LABEL: u32 = 0xFF40_4040;

/// What the window shows: `item`'s recipes, or its uses, in one category.
#[derive(Clone, Debug, PartialEq)]
struct View {
    item: &'static str,
    uses: bool,
    category: Category,
    page: usize,
}

pub struct ViewerState {
    /// Ctrl+O: the item list and bookmarks are shown.
    pub shown: bool,
    pub search: TextBox,
    page: usize,
    filtered: Vec<&'static str>,
    filtered_for: Option<String>,
    bookmarks: Vec<&'static str>,
    bookmarks_loaded: bool,
    view: Option<View>,
    history: Vec<View>,
}

impl Default for ViewerState {
    fn default() -> ViewerState {
        ViewerState {
            shown: true,
            search: TextBox::new(64, "Search…"),
            page: 0,
            filtered: Vec::new(),
            filtered_for: None,
            bookmarks: Vec::new(),
            bookmarks_loaded: false,
            view: None,
            history: Vec::new(),
        }
    }
}

impl ViewerState {
    pub fn search_focused(&self) -> bool {
        self.search.focused
    }

    pub fn viewing(&self) -> bool {
        self.view.is_some()
    }

    /// Escape: leaves the search box, or goes back out of the recipe window,
    /// before it would close the screen. Whether it was taken.
    pub fn take_escape(&mut self) -> bool {
        if self.search.focused {
            self.search.focused = false;
            return true;
        }
        if self.view.take().is_some() {
            self.history.clear();
            return true;
        }
        false
    }

    /// The screen left the inventory: nothing stays open or focused.
    pub fn leave(&mut self) {
        self.view = None;
        self.history.clear();
        self.search.focused = false;
    }

    fn open(&mut self, item: &'static str, uses: bool) {
        let index = recipe_index::index();
        let found = if uses {
            index.uses_of(item)
        } else {
            index.recipes_for(item)
        };
        let Some(first) = found.first() else {
            return;
        };
        let view = View {
            item,
            uses,
            category: first.category,
            page: 0,
        };
        if let Some(current) = self.view.replace(view) {
            self.history.push(current);
        }
    }

    fn toggle_bookmark(&mut self, item: &'static str) {
        self.load_bookmarks();
        match self.bookmarks.iter().position(|b| *b == item) {
            Some(at) => {
                self.bookmarks.remove(at);
            }
            None => self.bookmarks.push(item),
        }
        save_bookmarks(&self.bookmarks);
    }

    fn load_bookmarks(&mut self) {
        if std::mem::replace(&mut self.bookmarks_loaded, true) {
            return;
        }
        let text = std::fs::read_to_string(bookmarks_path()).unwrap_or_default();
        self.bookmarks = text
            .lines()
            .filter_map(|line| item(line.trim()).map(|d| d.id))
            .collect();
    }

    /// The items matching the search, rebuilt when it changes.
    fn filtered(&mut self) -> &[&'static str] {
        if self.filtered_for.as_deref() != Some(self.search.text.as_str()) {
            self.filtered = search(&self.search.text);
            self.filtered_for = Some(self.search.text.clone());
            self.page = 0;
        }
        &self.filtered
    }
}

fn bookmarks_path() -> std::path::PathBuf {
    crate::platform::storage::dir().join("recipe_bookmarks.txt")
}

fn save_bookmarks(bookmarks: &[&str]) {
    let mut text = bookmarks.join("\n");
    text.push('\n');
    if let Err(e) = std::fs::write(bookmarks_path(), text) {
        crate::log_warn!("gui", "could not save recipe bookmarks: {e}");
    }
}

/// The width of the screen the item list sits beside, for the screens that
/// show it (the inventory and containers with slots).
pub fn container_width(screen: Screen) -> Option<f32> {
    match screen {
        Screen::Inventory => Some(176.0),
        Screen::Container(kind) => match kind {
            ContainerKind::None | ContainerKind::Lectern => None,
            ContainerKind::Merchant => Some(276.0),
            ContainerKind::Beacon => Some(230.0),
            _ => Some(176.0),
        },
        _ => None,
    }
}

/// Every item, in creative tab order, then any no tab lists.
fn all_items() -> &'static [&'static str] {
    static ALL: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for tab in CREATIVE_TABS {
            for &id in tab.items {
                if seen.insert(id) {
                    out.push(id);
                }
            }
        }
        for def in ITEMS {
            if def.id != "air" && seen.insert(def.id) {
                out.push(def.id);
            }
        }
        out
    })
}

/// The items `query` matches: every word must (`@namespace`, `#tag`, or a
/// piece of the name or ID).
fn search(query: &str) -> Vec<&'static str> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return all_items().to_vec();
    }
    let index = recipe_index::index();
    all_items()
        .iter()
        .copied()
        .filter(|id| {
            let name = item(id).map(|d| d.name.to_lowercase()).unwrap_or_default();
            words.iter().all(|word| {
                if let Some(namespace) = word.strip_prefix('@') {
                    "minecraft".contains(namespace)
                } else if let Some(tag) = word.strip_prefix('#') {
                    index.tags_of(id).iter().any(|t| t.contains(tag))
                } else {
                    name.contains(word.as_str()) || id.contains(word.as_str())
                }
            })
        })
        .collect()
}

fn stack(id: &'static str, count: u8) -> SlotStack {
    SlotStack {
        item: id,
        count,
        max_damage: item(id).map_or(0, |d| d.max_damage),
        ..Default::default()
    }
}

/// A raised grey panel, as vanilla's container backgrounds.
fn panel(p: &mut Painter, x: f32, y: f32, w: f32, h: f32) {
    panel_filled(p, x, y, w, h, PANEL);
}

/// `panel` with its face in `face` (a darker one for tabs not chosen).
fn panel_filled(p: &mut Painter, x: f32, y: f32, w: f32, h: f32, face: u32) {
    p.fill(x + 1.0, y, w - 2.0, 1.0, EDGE);
    p.fill(x + 1.0, y + h - 1.0, w - 2.0, 1.0, EDGE);
    p.fill(x, y + 1.0, 1.0, h - 2.0, EDGE);
    p.fill(x + w - 1.0, y + 1.0, 1.0, h - 2.0, EDGE);
    p.fill(x + 1.0, y + 1.0, w - 2.0, h - 2.0, face);
    p.fill(x + 1.0, y + 1.0, w - 3.0, 2.0, LIGHT);
    p.fill(x + 1.0, y + 1.0, 2.0, h - 3.0, LIGHT);
    p.fill(x + 2.0, y + h - 3.0, w - 3.0, 2.0, SHADE);
    p.fill(x + w - 3.0, y + 2.0, 2.0, h - 3.0, SHADE);
}

/// A sunken slot behind an item drawn at (`x`, `y`).
fn slot(p: &mut Painter, x: f32, y: f32) {
    p.fill(x - 1.0, y - 1.0, 18.0, 18.0, 0xFF8B_8B8B);
    p.fill(x - 1.0, y - 1.0, 17.0, 1.0, 0xFF37_3737);
    p.fill(x - 1.0, y - 1.0, 1.0, 17.0, 0xFF37_3737);
    p.fill(x, y + 16.0, 17.0, 1.0, LIGHT);
    p.fill(x + 16.0, y, 1.0, 17.0, LIGHT);
}

/// A right-pointing arrow, 22 by 15, as between a recipe's inputs and output.
fn arrow(p: &mut Painter, x: f32, y: f32) {
    const COLOR: u32 = 0xFF8B_8B8B;
    p.fill(x, y + 6.0, 15.0, 3.0, COLOR);
    for i in 0..8 {
        let i = i as f32;
        p.fill(x + 14.0 + i, y + i, 1.0, 15.0 - 2.0 * i, COLOR);
    }
}

/// The input that picks something: left for its recipes, right for uses.
fn pick(ctx: &ScreenCtx) -> Option<bool> {
    if ctx.input.left_click {
        Some(false)
    } else if ctx.input.right_click {
        Some(true)
    } else {
        None
    }
}

/// The item list beside the screen, and the bookmarks on its other side.
/// `under` is what the screen itself has under the pointer, for R, U and A.
/// What this draws under the pointer, for the tooltip.
pub fn draw_overlay(
    p: &mut Painter,
    st: &mut ViewerState,
    ctx: &ScreenCtx,
    screen_w: f32,
    under: Option<&SlotStack>,
    carrying: bool,
) -> Option<SlotStack> {
    if ctx.input.ctrl && ctx.input.pressed_key == Some(KeyCode::KeyO) {
        st.shown = !st.shown;
        st.search.focused = false;
    }
    if !st.shown {
        return None;
    }
    st.load_bookmarks();
    let mut hovered: Option<&'static str> = None;

    // The list, against the right edge, up to nine columns wide.
    let room_left = ((ctx.vw + screen_w) / 2.0).floor() + MARGIN;
    let right = ctx.vw - MARGIN;
    let cols = (((right - room_left) / CELL).floor().max(0.0) as usize).min(MAX_COLUMNS);
    let left = right - cols as f32 * CELL;
    let top = MARGIN + BAR_H + 2.0;
    let bottom = ctx.vh - MARGIN - SEARCH_H - 2.0;
    let rows = ((bottom - top) / CELL).floor().max(0.0) as usize;
    if cols >= 2 && rows >= 1 {
        let per_page = cols * rows;
        let width = cols as f32 * CELL;
        let count = st.filtered().len();
        let pages = count.div_ceil(per_page).max(1);
        st.page = st.page.min(pages - 1);
        if ctx.hovering(left, top, width, rows as f32 * CELL) && ctx.input.scroll != 0.0 {
            st.page = if ctx.input.scroll > 0.0 {
                st.page.checked_sub(1).unwrap_or(pages - 1)
            } else {
                (st.page + 1) % pages
            };
        }
        if Button::new(left, MARGIN, BAR_H, BAR_H, "<").draw(p, ctx) {
            st.page = st.page.checked_sub(1).unwrap_or(pages - 1);
        }
        if Button::new(left + width - BAR_H, MARGIN, BAR_H, BAR_H, ">").draw(p, ctx) {
            st.page = (st.page + 1) % pages;
        }
        let label = format!("{}/{}", st.page + 1, pages);
        let label_w = p.atlas.font.width_str(&label);
        p.text_str(
            &label,
            left + (width - label_w) / 2.0,
            MARGIN + (BAR_H - 8.0) / 2.0,
            0xFFFF_FFFF,
            true,
        );

        let start = st.page * per_page;
        let shown: Vec<&'static str> = st.filtered[start.min(count)..(start + per_page).min(count)].to_vec();
        for (i, id) in shown.into_iter().enumerate() {
            let x = left + (i % cols) as f32 * CELL + 1.0;
            let y = top + (i / cols) as f32 * CELL + 1.0;
            p.item_icon(id, x, y);
            if ctx.hovering(x - 1.0, y - 1.0, CELL, CELL) {
                p.slot_highlight(x, y);
                hovered = Some(id);
            }
        }

        // Search, under the list.
        let search_y = ctx.vh - MARGIN - SEARCH_H;
        if ctx.input.left_click {
            st.search.focused = ctx.hovering(left, search_y, width, SEARCH_H);
        }
        if st.search.focused {
            st.search.handle_input(ctx.input);
        }
        let frame = p.frame;
        st.search.draw(p, left, search_y, width, SEARCH_H, frame);
    }

    // Bookmarks, left of the screen.
    let screen_left = ((ctx.vw - screen_w) / 2.0).floor();
    let b_cols = ((screen_left - 2.0 * MARGIN) / CELL).floor().max(0.0) as usize;
    if b_cols >= 1 && !st.bookmarks.is_empty() {
        let b_left = screen_left - MARGIN - b_cols as f32 * CELL;
        p.text_str("Bookmarks", b_left, MARGIN + 2.0, 0xFFFF_FFFF, true);
        let b_rows = ((bottom - top) / CELL).floor().max(0.0) as usize;
        let bookmarks: Vec<&'static str> = st.bookmarks.iter().take(b_cols * b_rows).copied().collect();
        for (i, id) in bookmarks.into_iter().enumerate() {
            let x = b_left + (i % b_cols) as f32 * CELL + 1.0;
            let y = top + (i / b_cols) as f32 * CELL + 1.0;
            p.item_icon(id, x, y);
            if ctx.hovering(x - 1.0, y - 1.0, CELL, CELL) {
                p.slot_highlight(x, y);
                hovered = Some(id);
            }
        }
    }

    // Clicks on the lists, and the keys over anything with an item.
    if let Some(id) = hovered
        && !carrying
        && let Some(uses) = pick(ctx)
    {
        st.open(id, uses);
    }
    let target = hovered.or_else(|| under.filter(|s| !s.is_empty()).map(|s| s.item));
    if let Some(id) = target
        && !st.search.focused
    {
        match ctx.input.pressed_key {
            Some(KeyCode::KeyR) => st.open(id, false),
            Some(KeyCode::KeyU) => st.open(id, true),
            Some(KeyCode::KeyA) => st.toggle_bookmark(id),
            _ => {}
        }
    }
    hovered.map(|id| stack(id, 1))
}

/// How tall one recipe's panel is, by category.
fn recipe_h(category: Category) -> f32 {
    match category {
        Category::Crafting => 62.0,
        Category::Smelting | Category::Blasting | Category::Smoking | Category::Campfire => 44.0,
        Category::Stonecutting | Category::Smithing => 34.0,
    }
}

/// A sunken panel, lighter than the window, around each recipe.
fn inset(p: &mut Painter, x: f32, y: f32, w: f32, h: f32) {
    p.fill(x, y, w, h, 0xFFB4_B4B4);
    p.fill(x, y, w - 1.0, 1.0, 0xFF7A_7A7A);
    p.fill(x, y, 1.0, h - 1.0, 0xFF7A_7A7A);
    p.fill(x + 1.0, y + h - 1.0, w - 1.0, 1.0, LIGHT);
    p.fill(x + w - 1.0, y + 1.0, 1.0, h - 1.0, LIGHT);
}

/// A bar with arrows either side of a centred label (a category, a page).
/// -1 or 1 when an arrow was pressed.
/// The arrows are greyed out, and do nothing, unless `active` (there is
/// something to step to).
fn arrow_bar(
    p: &mut Painter,
    ctx: &ScreenCtx,
    x: f32,
    y: f32,
    w: f32,
    label: &str,
    active: bool,
) -> i32 {
    const H: f32 = 14.0;
    p.fill(x, y, w, H, 0xFF8B_8B8B);
    let mut step = 0;
    let arrow = |x: f32, text| Button {
        active,
        ..Button::new(x, y, H, H, text)
    };
    if arrow(x, "<").draw(p, ctx) && active {
        step = -1;
    }
    if arrow(x + w - H, ">").draw(p, ctx) && active {
        step = 1;
    }
    let room = w - 2.0 * H - 4.0;
    let label = fit(p, label, room);
    let label_w = p.atlas.font.width_str(&label);
    p.text_str(&label, x + (w - label_w) / 2.0, y + 3.0, 0xFFFF_FFFF, true);
    step
}

/// The little crossed arrows marking a shapeless recipe.
fn shapeless_mark(p: &mut Painter, x: f32, y: f32) {
    const C: u32 = 0xFF55_5555;
    for i in 0..7 {
        let i = i as f32;
        p.fill(x + i, y + i, 1.0, 1.0, C);
        p.fill(x + i, y + 6.0 - i, 1.0, 1.0, C);
    }
    p.fill(x + 4.0, y, 3.0, 1.0, C);
    p.fill(x + 6.0, y, 1.0, 3.0, C);
    p.fill(x + 4.0, y + 6.0, 3.0, 1.0, C);
    p.fill(x + 6.0, y + 4.0, 1.0, 3.0, C);
}

/// The recipe window, drawn in place of the screen while it is open. What it
/// has under the pointer, for the tooltip.
pub fn draw_view(
    p: &mut Painter,
    st: &mut ViewerState,
    ctx: &ScreenCtx,
    screen: Screen,
    slots: &[SlotStack],
    actions: &mut Vec<InvAction>,
) -> Option<SlotStack> {
    let Some(view) = st.view.clone() else {
        return None;
    };
    let index = recipe_index::index();
    let all = if view.uses {
        index.uses_of(view.item)
    } else {
        index.recipes_for(view.item)
    };
    let mut categories: Vec<Category> = all.iter().map(|r| r.category).collect();
    categories.dedup();
    let recipes: Vec<&Recipe> = all.iter().copied().filter(|r| r.category == view.category).collect();
    if recipes.is_empty() {
        st.view = st.history.pop();
        return None;
    }

    crate::gui::screens::dim_background(p, ctx.vw, ctx.vh);

    // As many recipes a page as the screen has room for.
    const HEAD: f32 = 8.0 + 14.0 + 3.0 + 14.0 + 6.0;
    let each = recipe_h(view.category) + 4.0;
    let room = ctx.vh - 2.0 * MARGIN - TAB_H - HEAD - 6.0;
    let per = ((room / each).floor() as usize).clamp(1, 4).min(recipes.len());
    let height = HEAD + per as f32 * each + 4.0;
    let left = ((ctx.vw - WINDOW_W) / 2.0).floor();
    let top = (((ctx.vh - height) / 2.0).floor()).max(MARGIN + TAB_H + 1.0);
    let mut hovered: Option<SlotStack> = None;
    let mut next: Option<View> = None;
    let switch_to = |category: Category| View {
        category,
        page: 0,
        ..view.clone()
    };

    // A tab per category with something to show, all in one row on the
    // window's top edge: the others darker, the chosen one the window's own
    // grey and joined to it (drawn after the window, over its edge).
    let tab_y = top - TAB_H - 1.0;
    let mut chosen_tab: Option<f32> = None;
    for (i, category) in categories.iter().enumerate() {
        let x = left + 4.0 + i as f32 * (TAB_W + 1.0);
        let selected = *category == view.category;
        if selected {
            chosen_tab = Some(x);
        } else {
            // Lower than the chosen tab, its foot under the window's edge,
            // the icon raised to sit centred in what shows.
            let low = tab_y + 3.0;
            panel_filled(p, x, low, TAB_W, TAB_H, 0xFF9C_9C9C);
            p.item_icon(category.icon(), x + 4.0, low + 2.0);
        }
        if ctx.hovering(x, tab_y, TAB_W, TAB_H) {
            hovered = Some(SlotStack {
                custom_name: Some(vec![crate::text::Span {
                    text: category.title().to_owned(),
                    style: crate::text::Style::colored(0xFFFFFF),
                }]),
                ..stack(category.icon(), 1)
            });
            if ctx.input.left_click && !selected {
                next = Some(switch_to(*category));
            }
        }
    }

    panel(p, left, top, WINDOW_W, height);
    if let (Some(x), Some(category)) = (chosen_tab, categories.iter().find(|c| **c == view.category)) {
        panel(p, x, tab_y, TAB_W, TAB_H + 3.0);
        // Open into the window: its top edge and highlight run under the tab.
        p.fill(x + 1.0, top, TAB_W - 2.0, 3.0, PANEL);
        p.fill(x + 1.0, top - 1.0, 2.0, 4.0, LIGHT);
        p.fill(x + TAB_W - 3.0, top - 1.0, 2.0, 3.0, PANEL);
        p.item_icon(category.icon(), x + 4.0, tab_y + 4.0);
    }

    // The workstation, in its own panel left of the window: an item like
    // any other, to look up how to make it (left) or what it makes (right).
    panel(p, left - 26.0, top + 4.0, 28.0, 28.0);
    let (wx, wy) = (left - 20.0, top + 10.0);
    slot(p, wx, wy);
    let station = view.category.icon();
    p.item_icon(station, wx, wy);
    if ctx.hovering(wx - 1.0, wy - 1.0, CELL, CELL) {
        p.slot_highlight(wx, wy);
        hovered = Some(stack(station, 1));
        if let Some(uses) = pick(ctx) {
            next = Some(View {
                item: station,
                uses,
                category: view.category,
                page: 0,
            });
        }
    }

    // Category, then page, each with arrows either side.
    let bar_x = left + 6.0;
    let bar_w = WINDOW_W - 12.0;
    let at = categories.iter().position(|c| *c == view.category).unwrap_or(0);
    let step = arrow_bar(
        p,
        ctx,
        bar_x,
        top + 8.0,
        bar_w,
        view.category.title(),
        categories.len() > 1,
    );
    if step != 0 {
        let n = categories.len() as i32;
        next = Some(switch_to(categories[(at as i32 + step).rem_euclid(n) as usize]));
    }
    let pages = recipes.len().div_ceil(per);
    let page = view.page.min(pages - 1);
    let page_label = format!("{}/{}", page + 1, pages);
    let mut set_page = match arrow_bar(p, ctx, bar_x, top + 25.0, bar_w, &page_label, pages > 1) {
        0 => None,
        step => Some((page as i32 + step).rem_euclid(pages as i32) as usize),
    };
    if ctx.hovering(left, top, WINDOW_W, height) && ctx.input.scroll != 0.0 {
        let step = if ctx.input.scroll > 0.0 { -1 } else { 1 };
        set_page = Some((page as i32 + step).rem_euclid(pages as i32) as usize);
    }

    let mut close = false;
    let mut plus_tip: Option<SlotStack> = None;
    // This page's recipes, each in its own panel.
    let mut drew = |p: &mut Painter, slot_items: &[&'static str], count: u8, x: f32, y: f32| {
        if slot_items.is_empty() {
            return;
        }
        let cycle = (ctx.input.time as usize) % slot_items.len();
        let id = slot_items[cycle];
        crate::gui::slots::draw_stack(p, &stack(id, count), x, y);
        if ctx.hovering(x - 1.0, y - 1.0, CELL, CELL) {
            p.slot_highlight(x, y);
            hovered = Some(stack(id, count));
            if let Some(uses) = pick(ctx) {
                next = Some(View {
                    item: id,
                    uses,
                    category: view.category,
                    page: 0,
                });
            }
        }
    };
    for (i, recipe) in recipes.iter().skip(page * per).take(per).enumerate() {
        let y = top + HEAD + i as f32 * each;
        let h = recipe_h(view.category);
        // The recipe's panel, and the "+" outside it at its bottom right,
        // styled as the page arrows.
        let panel_w = WINDOW_W - 16.0 - PLUS - 2.0;
        inset(p, left + 8.0, y, panel_w, h);
        draw_recipe(p, recipe, left + 8.0, y, panel_w, h, &mut drew);
        let (bx, by) = (left + 8.0 + panel_w + 2.0, y + h - PLUS);
        let fill = plan_fill(recipe, screen, slots);
        let over = ctx.hovering(bx, by, PLUS, PLUS);
        let clicked = Button {
            active: fill.is_ok(),
            ..Button::new(bx, by, PLUS, PLUS, "+")
        }
        .draw(p, ctx);
        if over {
            let mut tip = SlotStack {
                custom_name: Some(vec![crate::text::Span {
                    text: "Move Items".to_owned(),
                    style: crate::text::Style::colored(0xFFFFFF),
                }]),
                ..stack(recipe.output, 1)
            };
            match &fill {
                Err(why) => {
                    tip.lore = vec![vec![crate::text::Span {
                        text: why.clone(),
                        style: crate::text::Style::colored(0xFF5555),
                    }]]
                }
                Ok(fill) if clicked => {
                    actions.push(InvAction::FillRecipe {
                        clear: fill.clear.clone(),
                        targets: fill.targets.clone(),
                        sets: if ctx.input.shift { fill.most } else { 1 },
                    });
                    close = true;
                }
                Ok(fill) => {
                    tip.lore = vec![vec![crate::text::Span {
                        text: format!("Shift-click to fill {} sets", fill.most),
                        style: crate::text::Style::colored(0xAAAAAA),
                    }]]
                }
            }
            plus_tip = Some(tip);
        }
    }
    if plus_tip.is_some() {
        hovered = plus_tip;
    }
    if close {
        st.view = None;
        st.history.clear();
        return hovered;
    }

    if let Some(view) = next {
        if view.item != st.view.as_ref().map_or("", |v| v.item) || view.uses != st.view.as_ref().is_some_and(|v| v.uses) {
            st.open(view.item, view.uses);
        } else {
            st.view = Some(view);
        }
    } else if let Some(page) = set_page {
        if let Some(current) = st.view.as_mut() {
            current.page = page;
        }
    } else if ctx.input.backspace {
        st.view = st.history.pop();
    }
    hovered
}

/// The "+" button's side.
const PLUS: f32 = 12.0;

/// Where a recipe's ingredients go in the open menu: the input slots to
/// empty first, and each slot's item (any of its names).
#[derive(Debug)]
struct Fill {
    clear: Vec<u16>,
    targets: Vec<(u16, Vec<&'static str>)>,
    /// The most sets the player has the ingredients for, up to a stack.
    most: u8,
}

/// How `recipe` would go into the menu `screen` shows, given its `slots`;
/// or why it can't: the wrong workstation, or ingredients the player lacks
/// (counting what already sits in the input slots, which come back first).
fn plan_fill(recipe: &Recipe, screen: Screen, slots: &[SlotStack]) -> Result<Fill, String> {
    let station = match recipe.category {
        Category::Crafting => "a crafting table",
        Category::Smelting => "a furnace",
        Category::Blasting => "a blast furnace",
        Category::Smoking => "a smoker",
        Category::Campfire => return Err("Cooked on a campfire in the world".to_owned()),
        Category::Stonecutting => "a stonecutter",
        Category::Smithing => "a smithing table",
    };
    // The input slots (a grid, row by row, `side` wide) and the player's.
    let (inputs, side, player): (Vec<u16>, usize, std::ops::Range<usize>) =
        match (recipe.category, screen) {
            (Category::Crafting, Screen::Inventory) => ((1..=4).collect(), 2, 9..45),
            (Category::Crafting, Screen::Container(ContainerKind::Crafting)) => {
                ((1..=9).collect(), 3, 10..46)
            }
            (Category::Smelting, Screen::Container(ContainerKind::Furnace(FurnaceKind::Furnace)))
            | (
                Category::Blasting,
                Screen::Container(ContainerKind::Furnace(FurnaceKind::BlastFurnace)),
            )
            | (Category::Smoking, Screen::Container(ContainerKind::Furnace(FurnaceKind::Smoker))) => {
                (vec![0], 1, 3..39)
            }
            (Category::Stonecutting, Screen::Container(ContainerKind::Stonecutter)) => {
                (vec![0], 1, 2..38)
            }
            (Category::Smithing, Screen::Container(ContainerKind::Smithing)) => {
                (vec![0, 1, 2], 3, 4..40)
            }
            _ => return Err(format!("Needs {station}")),
        };
    // Each ingredient's slot.
    let mut targets: Vec<(u16, Vec<&'static str>)> = Vec::new();
    if recipe.category == Category::Crafting && !recipe.shapeless() {
        let width = recipe.width as usize;
        let height = recipe.inputs.len().div_ceil(width);
        if width > side || height > side {
            return Err(format!("Needs {station}"));
        }
        for (i, names) in recipe.inputs.iter().enumerate() {
            if !names.is_empty() {
                targets.push((inputs[(i / width) * side + i % width], names.clone()));
            }
        }
    } else {
        let names: Vec<&Vec<&'static str>> =
            recipe.inputs.iter().filter(|n| !n.is_empty()).collect();
        if names.len() > inputs.len() {
            return Err(format!("Needs {station}"));
        }
        for (i, names) in names.into_iter().enumerate() {
            targets.push((inputs[i], names.clone()));
        }
    }
    // What the player has to give, the input slots' contents included.
    let mut pool: Vec<(&str, u32)> = Vec::new();
    for i in player.chain(inputs.iter().map(|&s| s as usize)) {
        let Some(s) = slots.get(i).filter(|s| !s.is_empty()) else {
            continue;
        };
        match pool.iter_mut().find(|(n, _)| *n == s.item) {
            Some((_, c)) => *c += s.count as u32,
            None => pool.push((s.item, s.count as u32)),
        }
    }
    // One set from the pool, naming what runs out.
    let take = |pool: &mut Vec<(&str, u32)>| -> Vec<&'static str> {
        let mut missing: Vec<&'static str> = Vec::new();
        for (_, names) in &targets {
            match pool.iter_mut().find(|(n, c)| *c > 0 && names.contains(n)) {
                Some((_, c)) => *c -= 1,
                None => {
                    if let Some(&name) = names.first()
                        && !missing.contains(&name)
                    {
                        missing.push(name);
                    }
                }
            }
        }
        missing
    };
    let missing = take(&mut pool);
    // More sets while the ingredients last, up to the smallest stack.
    let cap = targets
        .iter()
        .filter_map(|(_, names)| names.first())
        .map(|id| item(id).map_or(64, |d| d.max_stack.max(1)))
        .min()
        .unwrap_or(1);
    let mut most = 1u8;
    while most < cap {
        let mut next = pool.clone();
        if !take(&mut next).is_empty() {
            break;
        }
        pool = next;
        most += 1;
    }
    if !missing.is_empty() {
        let names: Vec<String> = missing
            .iter()
            .map(|id| item(id).map_or_else(|| id.to_string(), |d| d.name.to_string()))
            .collect();
        return Err(format!("Missing: {}", names.join(", ")));
    }
    let clear = inputs
        .iter()
        .copied()
        .filter(|&s| slots.get(s as usize).is_some_and(|s| !s.is_empty()))
        .collect();
    Ok(Fill {
        clear,
        targets,
        most,
    })
}

/// `text`, cut to `room` pixels with an ellipsis if it is wider.
fn fit(p: &Painter, text: &str, room: f32) -> String {
    if p.atlas.font.width_str(text) <= room {
        return text.to_owned();
    }
    let mut out: String = text.to_owned();
    while !out.is_empty() && p.atlas.font.width_str(&format!("{out}…")) > room {
        out.pop();
    }
    format!("{}…", out.trim_end())
}

/// Lays out one recipe in its panel at (`x`, `y`), `w` by `h`, drawing the
/// slots' frames and each slot's item through `item` (its items, count, x, y).
fn draw_recipe(
    p: &mut Painter,
    recipe: &Recipe,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    item: &mut impl FnMut(&mut Painter, &[&'static str], u8, f32, f32),
) {
    let mut small = |p: &mut Painter, items: &[&'static str], sx: f32, sy: f32| {
        slot(p, sx, sy);
        item(p, items, 1, sx, sy);
    };
    // The output, in a big slot at the right.
    let out_x = x + w - 34.0;
    let out_y = y + ((h - 26.0) / 2.0).floor();
    let arrow_y = y + ((h - 15.0) / 2.0).floor();
    let mid = y + ((h - 16.0) / 2.0).floor();
    match recipe.category {
        Category::Crafting => {
            let (gx, gy) = (x + 8.0, y + 4.0);
            let width = if recipe.shapeless() { 3 } else { recipe.width as usize };
            for row in 0..3 {
                for col in 0..3 {
                    let at = (col < width).then(|| row * width + col);
                    let items = at.and_then(|i| recipe.inputs.get(i)).map_or(&[][..], Vec::as_slice);
                    small(p, items, gx + 1.0 + col as f32 * CELL, gy + 1.0 + row as f32 * CELL);
                }
            }
            arrow(p, x + 74.0, arrow_y);
            if recipe.shapeless() {
                shapeless_mark(p, x + w - 10.0, y + 3.0);
            }
        }
        Category::Smelting | Category::Blasting | Category::Smoking | Category::Campfire => {
            let input = recipe.inputs.first().map_or(&[][..], Vec::as_slice);
            small(p, input, x + 40.0, mid - 4.0);
            arrow(p, x + 74.0, arrow_y - 4.0);
            if let Some((ticks, xp)) = recipe.cooking {
                let note = format!("{:.1}s, {xp} XP", ticks as f32 / 20.0);
                p.text_plain(&note, x + 8.0, y + h - 11.0, LABEL, false);
            }
        }
        Category::Stonecutting => {
            let input = recipe.inputs.first().map_or(&[][..], Vec::as_slice);
            small(p, input, x + 40.0, mid);
            arrow(p, x + 74.0, arrow_y);
        }
        Category::Smithing => {
            for (i, items) in recipe.inputs.iter().enumerate().take(3) {
                small(p, items, x + 8.0 + i as f32 * CELL, mid);
            }
            arrow(p, x + 74.0, arrow_y);
        }
    }
    let out_y = if matches!(
        recipe.category,
        Category::Smelting | Category::Blasting | Category::Smoking | Category::Campfire
    ) {
        out_y - 4.0
    } else {
        out_y
    };
    big_slot(p, out_x, out_y);
    item(p, &[recipe.output], recipe.count, out_x + 5.0, out_y + 5.0);
}

/// A 26 by 26 sunken slot, for a recipe's output.
fn big_slot(p: &mut Painter, x: f32, y: f32) {
    p.fill(x, y, 26.0, 26.0, 0xFF8B_8B8B);
    p.fill(x, y, 25.0, 1.0, 0xFF37_3737);
    p.fill(x, y, 1.0, 25.0, 0xFF37_3737);
    p.fill(x + 1.0, y + 25.0, 25.0, 1.0, LIGHT);
    p.fill(x + 25.0, y + 1.0, 1.0, 25.0, LIGHT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_checks_the_workstation_and_the_ingredients() {
        let sticks = Recipe {
            id: "stick".into(),
            category: Category::Crafting,
            width: 1,
            inputs: vec![vec!["oak_planks"], vec!["oak_planks"]],
            output: "stick",
            count: 4,
            cooking: None,
        };
        let mut slots = vec![SlotStack::default(); 46];
        assert!(plan_fill(&sticks, Screen::Inventory, &slots).is_err());
        slots[20] = stack("oak_planks", 2);
        let fill = plan_fill(&sticks, Screen::Inventory, &slots).unwrap();
        assert_eq!(fill.most, 1);
        assert_eq!(fill.targets.iter().map(|t| t.0).collect::<Vec<_>>(), [1, 3]);
        let fill = plan_fill(&sticks, Screen::Container(ContainerKind::Crafting), &slots).unwrap();
        assert_eq!(fill.targets.iter().map(|t| t.0).collect::<Vec<_>>(), [1, 4]);
        assert!(plan_fill(&sticks, Screen::Container(ContainerKind::Stonecutter), &slots).is_err());
    }

    #[test]
    fn search_takes_names_namespaces_and_tags() {
        let names = search("oak plank");
        assert!(names.contains(&"oak_planks"));
        assert!(!names.contains(&"stone"));
        assert!(search("@minecraft").len() > 1000);
        assert!(search("@create").is_empty());
        let logs = search("#logs");
        assert!(logs.contains(&"oak_log") && !logs.contains(&"stone"));
    }

    #[test]
    fn opening_follows_and_escape_steps_back() {
        let mut st = ViewerState::default();
        st.open("oak_planks", false);
        assert!(st.viewing());
        st.open("oak_log", true);
        assert_eq!(st.history.len(), 1);
        assert!(st.take_escape());
        assert!(!st.viewing() && st.history.is_empty());
        assert!(!st.take_escape());
        // Nothing to show: nothing opens.
        st.open("bedrock", false);
        assert!(!st.viewing());
    }
}
