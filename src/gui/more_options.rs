//! Vanilla's remaining option screens: Language (with Font settings),
//! Accessibility, Online and Debug Options.

use crate::gui::painter::Painter;
use crate::gui::widgets::{self, Button, HeaderFooter, Slider, WIDGET_HEIGHT, WIDGET_WIDTH_BIG};
use crate::gui::{GuiState, Screen, ScreenCtx};

const LAYOUT: HeaderFooter = HeaderFooter::default_heights();
const COLUMN_W: f32 = 150.0;
const COLUMN_STEP: f32 = 160.0;
const ROW: f32 = 24.0;

/// Accessibility settings the renderer reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AccessibilityOptions {
    /// `screenEffectScale`, percent: nausea's and the portal's warp.
    pub distortion: u32,
    /// `damageTiltStrength`, percent.
    pub damage_tilt: u32,
    pub hide_lightning: bool,
    pub hide_splash: bool,
    /// `panoramaScrollSpeed`, percent.
    pub panorama_speed: u32,
}

impl AccessibilityOptions {
    pub const DEFAULT: AccessibilityOptions = AccessibilityOptions {
        distortion: 100,
        damage_tilt: 100,
        hide_lightning: false,
        hide_splash: false,
        panorama_speed: 100,
    };
}

impl Default for AccessibilityOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

static ACCESSIBILITY: std::sync::Mutex<AccessibilityOptions> = std::sync::Mutex::new(AccessibilityOptions::DEFAULT);

pub fn accessibility() -> AccessibilityOptions {
    *ACCESSIBILITY.lock().unwrap()
}

pub fn apply(options: &crate::gui::GuiOptions) {
    *ACCESSIBILITY.lock().unwrap() = options.accessibility;
    // The saved language, once at start (the screen applies later picks).
    static APPLIED: std::sync::Once = std::sync::Once::new();
    APPLIED.call_once(|| crate::client::languages::apply(&options.language));
    crate::text::FORCE_UNICODE.store(options.force_unicode, std::sync::atomic::Ordering::Relaxed);
}

fn grid_left(ctx: &ScreenCtx) -> f32 {
    (ctx.vw / 2.0 - 155.0).floor()
}

fn toggle(p: &mut Painter, ctx: &ScreenCtx, at: (f32, f32), label: &str, value: &mut bool) -> bool {
    let caption = format!("{label}: {}", if *value { "ON" } else { "OFF" });
    if Button::new(at.0, at.1, COLUMN_W, WIDGET_HEIGHT, &caption).draw(p, ctx) {
        *value = !*value;
        return true;
    }
    false
}

fn percent_slider(p: &mut Painter, ctx: &ScreenCtx, id: u32, at: (f32, f32), value: &mut u32, label: fn(i32) -> String) -> bool {
    let new = Slider {
        id,
        x: at.0,
        y: at.1,
        w: COLUMN_W,
        h: WIDGET_HEIGHT,
        min: 0,
        max: 100,
        value: *value as i32,
        label,
    }
    .draw(p, ctx)
    .clamp(0, 100) as u32;
    let changed = new != *value;
    *value = new;
    changed
}

fn done(p: &mut Painter, ctx: &ScreenCtx) -> bool {
    let x = widgets::centered_x(ctx.vw);
    Button::new(x, LAYOUT.footer_y(ctx.vh), WIDGET_WIDTH_BIG, WIDGET_HEIGHT, "Done").draw(p, ctx) || ctx.input.escape
}

pub fn draw_accessibility(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Accessibility Settings");
    let left = grid_left(ctx);
    let top = LAYOUT.content_y(ctx.vh, 3.0 * ROW);
    let cell = |c: f32, r: f32| (left + c * COLUMN_STEP, top + r * ROW);
    let before = state.options.accessibility;
    let a = &mut state.options.accessibility;
    percent_slider(p, ctx, 70, cell(0.0, 0.0), &mut a.distortion, |v| format!("Distortion Effects: {v}%"));
    percent_slider(p, ctx, 71, cell(1.0, 0.0), &mut a.damage_tilt, |v| format!("Damage Tilt: {v}%"));
    toggle(p, ctx, cell(0.0, 1.0), "Hide Lightning Flashes", &mut a.hide_lightning);
    toggle(p, ctx, cell(1.0, 1.0), "Hide Splash Texts", &mut a.hide_splash);
    percent_slider(p, ctx, 72, cell(0.0, 2.0), &mut a.panorama_speed, |v| format!("Panorama Speed: {v}%"));
    if state.options.accessibility != before {
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

pub fn draw_online(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Online Options");
    let left = grid_left(ctx);
    let top = LAYOUT.content_y(ctx.vh, ROW);
    let mut changed = toggle(p, ctx, (left, top), "Allow Server Listings", &mut state.options.allow_listing);
    changed |= toggle(p, ctx, (left + COLUMN_STEP, top), "Only Show Secure Chat", &mut state.options.chat.only_secure);
    if changed {
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

pub fn draw_debug(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Debug Options");
    let left = grid_left(ctx);
    let top = LAYOUT.content_y(ctx.vh, 3.0 * ROW);
    let cell = |c: f32, r: f32| (left + c * COLUMN_STEP, top + r * ROW);
    let mut changed = toggle(p, ctx, cell(0.0, 0.0), "Pause on Lost Focus", &mut state.options.pause_on_lost_focus);
    changed |= toggle(p, ctx, cell(1.0, 0.0), "FPS Counter", &mut state.options.fps_counter);
    toggle(p, ctx, cell(0.0, 1.0), "Chunk Borders", &mut state.chunk_borders);
    toggle(p, ctx, cell(1.0, 1.0), "Hitboxes", &mut state.hitboxes);
    toggle(p, ctx, cell(0.0, 2.0), "Advanced Tooltips", &mut state.advanced_tooltips);
    if changed {
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

pub fn draw_font(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Font Settings");
    let left = grid_left(ctx);
    let top = LAYOUT.content_y(ctx.vh, ROW);
    if toggle(p, ctx, (left, top), "Force Unicode Font", &mut state.options.force_unicode) {
        apply(&state.options);
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Language);
    }
}

/// The language list's scroll.
#[derive(Default)]
pub struct LanguageState {
    pub scroll: f32,
    pub drag: Option<f32>,
}

pub fn draw_language(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    use crate::client::languages::{self, ListState};
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Language...");
    languages::refresh();
    let (list_x, list_y, list_w, list_h) = LAYOUT.content_rect(ctx.vw, ctx.vh);
    let list_h = list_h - ROW;
    let width = 220.0f32.min(list_w - 20.0);
    let left = ((ctx.vw - width) / 2.0).floor();
    let mut entries = vec![("en_us".to_owned(), "English (US)".to_owned())];
    let status = match languages::list() {
        ListState::Ready(list) => {
            entries = list
                .into_iter()
                .map(|l| {
                    let label = if l.region.is_empty() { l.name.clone() } else { format!("{} ({})", l.name, l.region) };
                    (l.code, label)
                })
                .collect();
            None
        }
        ListState::Failed(e) => Some(format!("Could not list languages: {e}")),
        _ => Some("Fetching the language list...".to_owned()),
    };
    let row_h = 18.0;
    let content_h = entries.len() as f32 * row_h;
    let lang = &mut state.language_list;
    crate::gui::options::scroll_input(&mut lang.scroll, &mut lang.drag, ctx, left, width, list_y, list_h, content_h);
    let clip = p.push_clip(list_x, list_y, list_w, list_h);
    let downloading = languages::downloading();
    for (i, (code, label)) in entries.iter().enumerate() {
        let y = list_y - lang.scroll + i as f32 * row_h;
        if y + row_h < list_y || y > list_y + list_h {
            continue;
        }
        let selected = *code == state.options.language;
        if selected {
            p.outline(left, y, width, row_h, 0xFFFF_FFFF);
        }
        let hovered = ctx.mouse().is_some_and(|m| m.x >= left && m.x < left + width && m.y >= y && m.y < y + row_h && m.y >= list_y && m.y < list_y + list_h);
        let mut text = label.clone();
        if downloading.as_deref() == Some(code.as_str()) {
            text.push_str(" ...");
        }
        let w = p.atlas.font.width_str(&text);
        p.text_str(&text, ((ctx.vw - w) / 2.0).floor(), y + 5.0, if hovered { 0xFFFF55 } else { 0xFFFFFF }, true);
        if hovered && ctx.input.left_click && !selected {
            state.options.language = code.clone();
            crate::gui::save_options(&state.options);
            languages::choose(code);
        }
    }
    p.pop_clip(clip);
    crate::gui::options::draw_scrollbar(p, lang.scroll, left, width, list_y, list_h, content_h);
    if let Some(status) = status {
        let w = p.atlas.font.width_str(&status);
        p.text_str(&status, ((ctx.vw - w) / 2.0).floor(), list_y + list_h + 6.0, 0xA0A0A0, true);
    }
    let footer = LAYOUT.footer_y(ctx.vh);
    let cx = (ctx.vw / 2.0).floor();
    if Button::new(cx - 155.0, footer, COLUMN_W, WIDGET_HEIGHT, "Font Settings...").draw(p, ctx) {
        state.nav = Some(Screen::FontSettings);
    }
    if Button::new(cx + 5.0, footer, COLUMN_W, WIDGET_HEIGHT, "Done").draw(p, ctx) || ctx.input.escape {
        state.nav = Some(Screen::Options);
    }
}
