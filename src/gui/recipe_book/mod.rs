mod page;
mod search;

use std::sync::Arc;

use crate::client::recipes::{
    BookMenu, BookRequest, BookType, CraftableStatus, RecipeBook, Selection, StackedContents, Tab,
    TypeSettings,
};
use crate::gui::ScreenCtx;
use crate::gui::painter::Painter;
use crate::gui::render::GuiInput;
use crate::gui::widgets::TextBox;
use crate::session::SlotStack;
use crate::text::Span;

use page::{ANIMATION_TIME, Button, Overlay, squeeze};

const IMAGE_W: i32 = 147;
const IMAGE_H: i32 = 166;
const OFFSET_X: i32 = 86;
const NARROW_W: i32 = 379;
const TICKS_TO_SWAP_SLOT: f32 = 30.0;
const PER_PAGE: usize = 20;
const SEARCH_H: f32 = 14.0;

#[derive(Clone, Copy, Default)]
struct TabState {
    visible: bool,
    anim: f32,
}

pub struct RecipeBookUi {
    visible: bool,
    init_pending: bool,
    contents_stale: bool,
    narrow: bool,
    tab: Option<Tab>,
    tabs: Vec<TabState>,
    page: usize,
    collections: Vec<usize>,
    buttons: Vec<Button>,
    overlay: Option<Overlay>,
    search: TextBox,
    last_search: String,
    time: f32,
    selection: Selection,
    selection_version: Option<u64>,
    selection_contents: Option<StackedContents>,
    selection_menu: Option<BookMenu>,
    tab_entries: usize,
    search_terms: search::Cache,
    last_placed: Option<u32>,
    last_recipe: Option<u32>,
    took_click: bool,
    selected_tab_hovered: bool,
    tooltip: Option<String>,
    hover: Option<(SlotStack, bool)>,
}

impl Default for RecipeBookUi {
    fn default() -> Self {
        let mut search = TextBox::bordered(50, "");
        search.color = 0xFF_FFFF;
        Self {
            visible: false,
            init_pending: false,
            contents_stale: false,
            narrow: false,
            tab: None,
            tabs: Vec::new(),
            page: 0,
            collections: Vec::new(),
            buttons: Vec::new(),
            overlay: None,
            search,
            last_search: String::new(),
            time: 0.0,
            selection: Selection::default(),
            selection_version: None,
            selection_contents: None,
            selection_menu: None,
            tab_entries: 0,
            search_terms: search::Cache::default(),
            last_placed: None,
            last_recipe: None,
            took_click: false,
            selected_tab_hovered: false,
            tooltip: None,
            hover: None,
        }
    }
}

pub struct BookCtx<'a> {
    pub book: &'a Arc<RecipeBook>,
    pub menu: BookMenu,
    pub slots: &'a [SlotStack],
    pub container_id: i32,
    pub spectator: bool,
    pub ctrl_down: bool,
}

impl BookCtx<'_> {
    fn settings(&self) -> TypeSettings {
        self.book.settings().get(self.menu.book)
    }

    fn set(&self, settings: TypeSettings) -> BookRequest {
        BookRequest::Settings {
            book: self.menu.book,
            settings,
        }
    }
}

#[derive(Clone, Copy)]
struct Layout {
    xo: f32,
    yo: f32,
}

impl Layout {
    fn new(vw: i32, vh: i32, narrow: bool) -> Self {
        let x_offset = if narrow { 0 } else { OFFSET_X };
        Self {
            xo: ((vw - IMAGE_W) / 2 - x_offset) as f32,
            yo: ((vh - IMAGE_H) / 2) as f32,
        }
    }

    fn search(&self) -> (f32, f32, f32, f32) {
        (self.xo + 25.0, self.yo + 13.0, 81.0, SEARCH_H)
    }

    fn magnifier(&self) -> (f32, f32, f32, f32) {
        (self.xo + 8.0, self.yo + 13.0, 25.0, SEARCH_H)
    }

    fn filter(&self) -> (f32, f32, f32, f32) {
        (self.xo + 110.0, self.yo + 12.0, 26.0, 16.0)
    }

    fn tab(&self, n: usize) -> (f32, f32, f32, f32) {
        (self.xo - 30.0, self.yo + 3.0 + 27.0 * n as f32, 35.0, 27.0)
    }

    fn button(&self, i: usize) -> (f32, f32) {
        (
            self.xo + 11.0 + 25.0 * (i % 5) as f32,
            self.yo + 31.0 + 25.0 * (i / 5) as f32,
        )
    }

    fn forward(&self) -> (f32, f32, f32, f32) {
        (self.xo + 93.0, self.yo + 137.0, 12.0, 17.0)
    }

    fn back(&self) -> (f32, f32, f32, f32) {
        (self.xo + 38.0, self.yo + 137.0, 12.0, 17.0)
    }
}

fn over(ctx: &ScreenCtx, (x, y, w, h): (f32, f32, f32, f32)) -> bool {
    ctx.hovering(x, y, w, h)
}

fn page_count(collections: usize) -> usize {
    collections.div_ceil(PER_PAGE)
}

fn clamp_page(page: usize, pages: usize, reset: bool) -> usize {
    if pages <= page || reset { 0 } else { page }
}

impl RecipeBookUi {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn crafting_slot_clicked(&mut self) {
        self.last_placed = None;
        self.contents_stale = true;
    }

    pub fn slot_select_index(&self) -> usize {
        (self.time / TICKS_TO_SWAP_SLOT).floor() as usize
    }

    pub fn search_focused(&self) -> bool {
        self.visible && self.search.focused
    }

    fn total_pages(&self) -> usize {
        page_count(self.collections.len())
    }

    fn forward_visible(&self) -> bool {
        let pages = self.total_pages();
        pages > 1 && self.page < pages - 1
    }

    fn back_visible(&self) -> bool {
        self.total_pages() > 1 && self.page > 0
    }

    fn refresh(&mut self, book: &BookCtx, filtering: bool, out: &mut Vec<BookRequest>) {
        let book_changed = self.selection_version != Some(book.book.version())
            || self.selection_menu != Some(book.menu);
        let contents = book.menu.stacked_contents(book.slots);
        let contents_changed = self.selection_contents.as_ref() != Some(&contents);
        if !(book_changed || contents_changed || self.init_pending || self.contents_stale) {
            return;
        }
        self.selection = Selection::compute(book.book, book.menu, book.slots);
        self.selection_version = Some(book.book.version());
        self.selection_contents = Some(contents);
        self.selection_menu = Some(book.menu);
        if book_changed || self.init_pending {
            let animate = self.init_pending || book.book.entries().len() != self.tab_entries;
            self.update_tabs(book, filtering, animate);
        }
        self.init_pending = false;
        self.contents_stale = false;
        self.update_collections(book, false, filtering, out);
    }

    fn update_tabs(&mut self, book: &BookCtx, filtering: bool, animate: bool) {
        let b = book.book;
        let infos = book.menu.book.tabs();
        self.tab_entries = b.entries().len();
        self.tabs.resize(infos.len(), TabState::default());
        if !infos.iter().any(|t| Some(t.tab) == self.tab) {
            self.tab = Some(infos[0].tab);
        }
        let status = if filtering {
            CraftableStatus::Craftable
        } else {
            CraftableStatus::Any
        };
        for (state, info) in self.tabs.iter_mut().zip(infos) {
            if matches!(info.tab, Tab::Search(_)) {
                state.visible = true;
                continue;
            }
            let collections = b.tab(info.tab);
            state.visible = collections
                .iter()
                .any(|&c| self.selection.has_any_selected(b, c));
            if state.visible
                && animate
                && collections.iter().any(|&c| {
                    self.selection
                        .selected_recipes(b, c, status)
                        .any(|e| b.will_highlight(b.entry(e).id))
                })
            {
                state.anim = ANIMATION_TIME;
            }
        }
    }

    fn update_collections(
        &mut self,
        book: &BookCtx,
        reset: bool,
        filtering: bool,
        out: &mut Vec<BookRequest>,
    ) {
        let b = book.book;
        let Some(tab) = self.tab else { return };
        let query = self.search.text.to_lowercase();
        let sel = &self.selection;
        let terms = &mut self.search_terms;
        self.collections = b
            .tab(tab)
            .iter()
            .copied()
            .filter(|&c| sel.has_any_selected(b, c))
            .filter(|&c| query.is_empty() || terms.matches(b, c, &query))
            .filter(|&c| !filtering || sel.has_craftable(b, c))
            .collect();
        self.page = clamp_page(self.page, self.total_pages(), reset);
        self.update_buttons(book, filtering, out);
    }

    fn update_buttons(&mut self, book: &BookCtx, filtering: bool, out: &mut Vec<BookRequest>) {
        let old = std::mem::take(&mut self.buttons);
        self.buttons = self
            .collections
            .iter()
            .skip(PER_PAGE * self.page)
            .take(PER_PAGE)
            .enumerate()
            .map(|(i, &c)| {
                let mut b = Button::new(book.book, &self.selection, c, filtering, out);
                b.anim = b.anim.max(old.get(i).map_or(0.0, |o| o.anim));
                b
            })
            .collect();
    }

    fn search_changed(&mut self, book: &BookCtx, out: &mut Vec<BookRequest>) {
        let text = self.search.text.to_lowercase();
        if text != self.last_search {
            self.update_collections(book, false, book.settings().filtering, out);
            self.last_search = text;
        }
    }

    fn try_place(
        &mut self,
        book: &BookCtx,
        recipe: u32,
        use_max_items: bool,
        out: &mut Vec<BookRequest>,
    ) -> bool {
        let craftable = book
            .book
            .index_of(recipe)
            .is_some_and(|e| self.selection.is_craftable(e));
        if !craftable && self.last_placed == Some(recipe) {
            return false;
        }
        self.last_placed = Some(recipe);
        out.push(BookRequest::Place {
            container_id: book.container_id,
            recipe,
            use_max_items,
        });
        true
    }

    fn click(
        &mut self,
        ctx: &ScreenCtx,
        p: &Painter,
        book: &BookCtx,
        l: Layout,
        out: &mut Vec<BookRequest>,
    ) {
        let input = ctx.input;
        if book.spectator {
            return;
        }
        if !(input.left_click || input.right_click) {
            let (x, y, w, h) = l.search();
            self.search.handle_mouse(input, &p.atlas.font, x, y, w, h);
            return;
        }
        let left = input.left_click;
        let settings = book.settings();
        let index = self.slot_select_index();

        let mut clicked = None;
        let mut on_page = true;
        if let Some(overlay) = &self.overlay {
            match overlay.hit(ctx).filter(|_| left) {
                Some(id) => clicked = Some(id),
                None => self.overlay = None,
            }
        } else if left && self.forward_visible() && over(ctx, l.forward()) {
            self.page += 1;
            self.update_buttons(book, settings.filtering, out);
        } else if left && self.back_visible() && over(ctx, l.back()) {
            self.page -= 1;
            self.update_buttons(book, settings.filtering, out);
        } else if let Some(i) = (0..self.buttons.len()).find(|&i| {
            let (x, y) = l.button(i);
            ctx.hovering(x, y, 25.0, 25.0)
        }) {
            let b = &self.buttons[i];
            if left {
                clicked = b.current_recipe(index);
            } else if !b.only_option() {
                let (x, y) = l.button(i);
                self.overlay = Some(Overlay::new(
                    book.book,
                    &self.selection,
                    b.collection,
                    settings.filtering,
                    book.menu.book != BookType::Crafting,
                    (x as i32, y as i32),
                    (l.xo as i32 + IMAGE_W / 2, l.yo as i32 + 13 + IMAGE_H / 2),
                ));
            }
        } else {
            on_page = false;
        }
        if on_page {
            self.took_click = true;
            if let Some(recipe) = clicked
                && self.try_place(book, recipe, input.shift, out)
            {
                self.last_recipe = Some(recipe);
                if self.narrow {
                    out.push(book.set(TypeSettings {
                        open: false,
                        ..settings
                    }));
                }
            }
            return;
        }

        let (x, y, w, h) = l.search();
        self.search.handle_mouse(input, &p.atlas.font, x, y, w, h);
        if left && (over(ctx, l.magnifier()) || over(ctx, l.search())) {
            self.search.focused = true;
            self.took_click = true;
            return;
        }
        self.search.focused = false;
        if !left {
            return;
        }
        if over(ctx, l.filter()) {
            out.push(book.set(TypeSettings {
                filtering: !settings.filtering,
                ..settings
            }));
            self.took_click = true;
            return;
        }
        let infos = book.menu.book.tabs();
        let hit = (self.tabs.iter().zip(infos))
            .filter(|(s, _)| s.visible)
            .enumerate()
            .find(|(n, _)| over(ctx, l.tab(*n)))
            .map(|(_, (_, info))| info);
        if let Some(info) = hit {
            self.took_click = true;
            if self.tab != Some(info.tab) {
                self.tab = Some(info.tab);
                self.update_collections(book, true, settings.filtering, out);
            }
        }
    }

    fn draw_tabs(&mut self, p: &mut Painter, ctx: &ScreenCtx, book: &BookCtx, l: Layout, dt: f32) {
        let infos = book.menu.book.tabs();
        let mut n = 0;
        for (state, info) in self.tabs.iter_mut().zip(infos) {
            if !state.visible {
                continue;
            }
            let (x, y, w, h) = l.tab(n);
            n += 1;
            let selected = self.tab == Some(info.tab);
            if selected && ctx.hovering(x, y, w, h) {
                self.selected_tab_hovered = true;
            }
            let s = if state.anim > 0.0 {
                squeeze(state.anim)
            } else {
                1.0
            };
            let cy = y + 12.0;
            let sy = |v: f32| cy + (v - cy) * s;
            let shift = if selected { -2.0 } else { 0.0 };
            let sprite = if selected {
                "recipe_book/tab_selected"
            } else {
                "recipe_book/tab"
            };
            p.sprite(sprite, x + shift, sy(y), w, h * s);
            let iy = sy(y + 5.0);
            match info.secondary {
                Some(second) => {
                    p.item_icon(info.primary, x + 3.0 + shift, iy);
                    p.item_icon(second, x + 14.0 + shift, iy);
                }
                None => p.item_icon(info.primary, x + 9.0 + shift, iy),
            }
            if state.anim > 0.0 {
                state.anim -= dt;
            }
        }
    }
}

pub fn left_pos(open: bool, vw: i32, image_w: i32) -> i32 {
    if open && vw >= NARROW_W {
        177 + (vw - image_w - 200) / 2
    } else {
        (vw - image_w) / 2
    }
}

pub fn draw(
    p: &mut Painter,
    ui: &mut RecipeBookUi,
    ctx: &ScreenCtx,
    book: &BookCtx,
    dt_ticks: f32,
    out: &mut Vec<BookRequest>,
) {
    ui.took_click = false;
    ui.selected_tab_hovered = false;
    ui.tooltip = None;
    ui.hover = None;

    let settings = book.settings();
    if settings.open != ui.visible {
        ui.visible = settings.open;
        if ui.visible {
            ui.init_pending = true;
        } else {
            ui.overlay = None;
        }
    }
    if !ui.visible {
        return;
    }
    if ui.search.placeholder.is_empty() {
        ui.search.placeholder =
            crate::gui::tooltip::label!("gui.recipebook.search_hint").to_string();
    }

    let (vw, vh) = (ctx.vw as i32, ctx.vh as i32);
    ui.narrow = vw < NARROW_W;
    ui.refresh(book, settings.filtering, out);
    if !book.ctrl_down {
        ui.time += dt_ticks;
    }
    let l = Layout::new(vw, vh, ui.narrow);
    ui.click(ctx, p, book, l, out);
    let index = ui.slot_select_index();

    p.blit_sheet(
        "recipe_book",
        1.0,
        1.0,
        IMAGE_W as f32,
        IMAGE_H as f32,
        l.xo,
        l.yo,
    );
    let (sx, sy, sw, sh) = l.search();
    ui.search.draw(p, sx, sy, sw, sh);

    ui.draw_tabs(p, ctx, book, l, dt_ticks);

    let filter_hovered = over(ctx, l.filter());
    let crafting = book.menu.book == BookType::Crafting;
    let filter = match (crafting, settings.filtering, filter_hovered) {
        (true, true, false) => "recipe_book/filter_enabled",
        (true, true, true) => "recipe_book/filter_enabled_highlighted",
        (true, false, false) => "recipe_book/filter_disabled",
        (true, false, true) => "recipe_book/filter_disabled_highlighted",
        (false, true, false) => "recipe_book/furnace_filter_enabled",
        (false, true, true) => "recipe_book/furnace_filter_enabled_highlighted",
        (false, false, false) => "recipe_book/furnace_filter_disabled",
        (false, false, true) => "recipe_book/furnace_filter_disabled_highlighted",
    };
    let (fx, fy, fw, fh) = l.filter();
    p.sprite(filter, fx, fy, fw, fh);
    if filter_hovered {
        let key = match (settings.filtering, book.menu.book) {
            (false, _) => "gui.recipebook.toggleRecipes.all",
            (true, BookType::Crafting) => "gui.recipebook.toggleRecipes.craftable",
            (true, BookType::Furnace) => "gui.recipebook.toggleRecipes.smeltable",
            (true, BookType::BlastFurnace) => "gui.recipebook.toggleRecipes.blastable",
            (true, BookType::Smoker) => "gui.recipebook.toggleRecipes.smokable",
        };
        ui.tooltip = Some(crate::gui::tooltip::translate(key, &[]));
    }

    let pages = ui.total_pages();
    if pages > 1 {
        let text = crate::gui::tooltip::translate(
            "gui.recipebook.page",
            &[(ui.page + 1).to_string(), pages.to_string()],
        );
        let w = p.atlas.font.width_str(&text) as i32;
        p.text_plain(
            &text,
            l.xo - (w / 2) as f32 + 73.0,
            l.yo + 141.0,
            0xFF_FFFF,
            true,
        );
    }
    let mut hovered = None;
    for (i, b) in ui.buttons.iter_mut().enumerate() {
        let (x, y) = l.button(i);
        if ctx.hovering(x, y, 25.0, 25.0) {
            hovered = Some(i);
        }
        if b.anim > 0.0 {
            let s = squeeze(b.anim);
            let (cx, cy) = (x + 8.0, y + 12.0);
            p.scaled(s, cx * (1.0 - s), cy * (1.0 - s), |p| {
                b.draw(p, x, y, index)
            });
            b.anim -= dt_ticks;
        } else {
            b.draw(p, x, y, index);
        }
    }
    for (visible, rect, sprite, key) in [
        (
            ui.forward_visible(),
            l.forward(),
            "recipe_book/page_forward",
            "gui.recipebook.next_page",
        ),
        (
            ui.back_visible(),
            l.back(),
            "recipe_book/page_backward",
            "gui.recipebook.previous_page",
        ),
    ] {
        if !visible {
            continue;
        }
        let (x, y, w, h) = rect;
        if over(ctx, rect) {
            p.sprite(&format!("{sprite}_highlighted"), x, y, w, h);
            ui.tooltip = Some(crate::gui::tooltip::translate(key, &[]));
        } else {
            p.sprite(sprite, x, y, w, h);
        }
    }

    match &ui.overlay {
        Some(overlay) => overlay.draw(p, ctx, index),
        None => {
            ui.hover = hovered.and_then(|i| {
                let b = &ui.buttons[i];
                b.display_stack(index).map(|s| (s.clone(), b.many()))
            });
        }
    }
}

pub fn draw_tooltip(p: &mut Painter, ui: &RecipeBookUi, ctx: &ScreenCtx, advanced: bool) {
    let Some(m) = ctx.mouse() else { return };
    if let Some((stack, many)) = &ui.hover {
        let more =
            many.then(|| crate::gui::tooltip::label!("gui.recipebook.moreRecipes").to_string());
        crate::gui::tooltip::draw_with_line(p, stack, more, m.x, m.y, ctx.vw, ctx.vh, advanced);
    } else if let Some(text) = &ui.tooltip {
        let line = vec![Span::new(text.clone())];
        crate::gui::tooltip::draw_lines(p, &[line], m.x, m.y, ctx.vw, ctx.vh);
    }
}

pub fn toggle_button(
    p: &mut Painter,
    ctx: &ScreenCtx,
    book: &BookCtx,
    x: f32,
    y: f32,
    out: &mut Vec<BookRequest>,
) -> bool {
    let hovered = ctx.hovering(x, y, 20.0, 18.0);
    let sprite = if hovered {
        "recipe_book/button_highlighted"
    } else {
        "recipe_book/button"
    };
    p.sprite(sprite, x, y, 20.0, 18.0);
    if hovered && ctx.input.left_click {
        let settings = book.settings();
        out.push(book.set(TypeSettings {
            open: !settings.open,
            ..settings
        }));
        return true;
    }
    false
}

pub fn key_pressed(
    ui: &mut RecipeBookUi,
    book: &BookCtx,
    input: &GuiInput,
    chat_key: bool,
    out: &mut Vec<BookRequest>,
) -> bool {
    if !ui.visible || book.spectator {
        return false;
    }
    if input.escape && ui.narrow {
        out.push(book.set(TypeSettings {
            open: false,
            ..book.settings()
        }));
        return true;
    }
    if ui.search.focused {
        if input.escape {
            return false;
        }
        if ui.search.handle_input(input) {
            ui.search_changed(book, out);
        }
        return true;
    }
    if chat_key {
        ui.search.focused = true;
        return true;
    }
    if input.select
        && let Some(recipe) = ui.last_recipe
    {
        return ui.try_place(book, recipe, input.shift, out);
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub fn blocks_container_click(
    ui: &RecipeBookUi,
    book: &BookCtx,
    x: f32,
    y: f32,
    left: f32,
    top: f32,
    w: f32,
    h: f32,
) -> bool {
    let _ = book;
    if !ui.visible {
        return false;
    }
    if ui.narrow || ui.took_click {
        return true;
    }
    let outside = x < left || y < top || x >= left + w || y >= top + h;
    let on_book = left - (IMAGE_W as f32) < x && x < left && top < y && y < top + h;
    outside && (on_book || ui.selected_tab_hovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_pos_shifts_only_when_open_and_wide() {
        assert_eq!(left_pos(false, 427, 176), 125);
        assert_eq!(left_pos(true, 427, 176), 177 + 25);
        assert_eq!(left_pos(true, 378, 176), 101);
    }

    #[test]
    fn layout_follows_the_offset() {
        let wide = Layout::new(427, 240, false);
        assert_eq!((wide.xo, wide.yo), (54.0, 37.0));
        let narrow = Layout::new(320, 240, true);
        assert_eq!(narrow.xo, 86.0);
        assert_eq!(wide.button(7), (54.0 + 11.0 + 50.0, 37.0 + 31.0 + 25.0));
        assert_eq!(wide.tab(2).1, 37.0 + 3.0 + 54.0);
        assert_eq!(wide.xo as i32 + IMAGE_W + 1, left_pos(true, 427, 176));
    }

    #[test]
    fn pages_count_and_clamp() {
        assert_eq!(page_count(0), 0);
        assert_eq!(page_count(20), 1);
        assert_eq!(page_count(21), 2);
        assert_eq!(clamp_page(1, 2, false), 1);
        assert_eq!(clamp_page(2, 2, false), 0);
        assert_eq!(clamp_page(1, 2, true), 0);
        assert_eq!(clamp_page(0, 0, false), 0);
    }

    #[test]
    fn slot_index_steps_every_30_ticks() {
        let mut ui = RecipeBookUi::default();
        ui.time = 59.9;
        assert_eq!(ui.slot_select_index(), 1);
        ui.time = 60.0;
        assert_eq!(ui.slot_select_index(), 2);
    }

    #[test]
    fn clicks_beside_the_container_stay_with_the_book() {
        let book = Arc::new(RecipeBook::default());
        let ctx = BookCtx {
            book: &book,
            menu: BookMenu::INVENTORY,
            slots: &[],
            container_id: 0,
            spectator: false,
            ctrl_down: false,
        };
        let mut ui = RecipeBookUi::default();
        let (l, t, w, h) = (202.0, 37.0, 176.0, 166.0);
        assert!(!blocks_container_click(&ui, &ctx, 100.0, 100.0, l, t, w, h));
        ui.visible = true;
        assert!(blocks_container_click(&ui, &ctx, 100.0, 100.0, l, t, w, h));
        assert!(!blocks_container_click(&ui, &ctx, 20.0, 100.0, l, t, w, h));
        assert!(!blocks_container_click(&ui, &ctx, 250.0, 100.0, l, t, w, h));
        ui.narrow = true;
        assert!(blocks_container_click(&ui, &ctx, 250.0, 100.0, l, t, w, h));
    }
}
