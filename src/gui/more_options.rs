//! Vanilla's remaining option screens: Language (with Font settings),
//! Accessibility, Online and Debug Options.

use crate::gui::form::Form;
use crate::gui::painter::Painter;
use crate::gui::widgets::{self, Button, HeaderFooter, WIDGET_HEIGHT, WIDGET_WIDTH_BIG};
use crate::gui::{GuiState, Screen, ScreenCtx};

const LAYOUT: HeaderFooter = HeaderFooter::default_heights();
const COLUMN_W: f32 = 150.0;
const COLUMN_STEP: f32 = 160.0;
const ROW: f32 = 24.0;

crate::options! {
    /// Accessibility settings the renderer reads.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct AccessibilityOptions {
        /// `screenEffectScale`, percent: nausea's and the portal's warp.
        pub distortion: u32 = 100 => key("screenEffectScale").clamp(0, 100),
        /// `damageTiltStrength`, percent.
        pub damage_tilt: u32 = 100 => key("damageTiltStrength").clamp(0, 100),
        pub hide_lightning: bool = false => key("hideLightningFlashes"),
        pub hide_splash: bool = false => key("hideSplashTexts"),
        /// `panoramaScrollSpeed`, percent.
        pub panorama_speed: u32 = 100 => key("panoramaScrollSpeed").clamp(0, 100),
    }
}

impl Default for AccessibilityOptions {
    fn default() -> Self {
        Self::defaults()
    }
}

pub fn accessibility() -> AccessibilityOptions {
    crate::gui::current_options().accessibility
}

pub fn apply(options: &crate::gui::GuiOptions) {
    // The saved language, once at start (the screen applies later picks).
    static APPLIED: std::sync::Once = std::sync::Once::new();
    APPLIED.call_once(|| crate::client::languages::apply(&options.language));
    crate::text::FORCE_UNICODE.store(options.force_unicode, std::sync::atomic::Ordering::Relaxed);
}

fn done(p: &mut Painter, ctx: &ScreenCtx) -> bool {
    let x = widgets::centered_x(ctx.vw);
    Button::new(
        x,
        LAYOUT.footer_y(ctx.vh),
        WIDGET_WIDTH_BIG,
        WIDGET_HEIGHT,
        "Done",
    )
    .draw(p, ctx)
        || ctx.input.escape
}

pub fn draw_accessibility(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let mut form = Form::new(p, ctx, "Accessibility Settings", 3, 70);
    let a = &mut state.options.accessibility;
    form.slider(
        &mut a.distortion,
        0,
        100,
        crate::percent_label!("Distortion Effects"),
    );
    form.slider(
        &mut a.damage_tilt,
        0,
        100,
        crate::percent_label!("Damage Tilt"),
    );
    form.toggle("Hide Lightning Flashes", &mut a.hide_lightning);
    form.toggle("Hide Splash Texts", &mut a.hide_splash);
    form.slider(
        &mut a.panorama_speed,
        0,
        100,
        crate::percent_label!("Panorama Speed"),
    );
    if form.finish() {
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

pub fn draw_online(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let mut form = Form::new(p, ctx, "Online Options", 1, 0);
    form.toggle("Allow Server Listings", &mut state.options.allow_listing);
    form.toggle("Only Show Secure Chat", &mut state.options.chat.only_secure);
    if form.finish() {
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

pub fn draw_debug(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let mut form = Form::new(p, ctx, "Debug Options", 3, 0);
    let saved_before = (state.options.pause_on_lost_focus, state.options.fps_counter);
    form.toggle(
        "Pause on Lost Focus",
        &mut state.options.pause_on_lost_focus,
    );
    form.toggle("FPS Counter", &mut state.options.fps_counter);
    form.toggle("Chunk Borders", &mut state.chunk_borders);
    form.toggle("Hitboxes", &mut state.hitboxes);
    form.toggle("Advanced Tooltips", &mut state.advanced_tooltips);
    form.finish();
    if (state.options.pause_on_lost_focus, state.options.fps_counter) != saved_before {
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

pub fn draw_font(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let mut form = Form::new(p, ctx, "Font Settings", 1, 0);
    form.toggle("Force Unicode Font", &mut state.options.force_unicode);
    if form.finish() {
        apply(&state.options);
        crate::gui::save_options(&state.options);
    }
    if done(p, ctx) {
        state.nav = Some(Screen::Language);
    }
}

pub fn draw_language(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    use crate::client::languages::{self, ListState};
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Language...");
    languages::refresh();
    let (list_x, list_y, list_w, list_h) = LAYOUT.content_rect(ctx.vw, ctx.vh);
    let area = crate::gui::scroll_list::ListArea::centered(
        ctx,
        (list_x, list_y, list_w, list_h - ROW),
        220.0,
    );
    let (left, width) = (area.left, area.width);
    let mut entries = vec![("en_us".to_owned(), "English (US)".to_owned())];
    let status = match languages::list() {
        ListState::Ready(list) => {
            entries = list
                .into_iter()
                .map(|l| {
                    let label = if l.region.is_empty() {
                        l.name.clone()
                    } else {
                        format!("{} ({})", l.name, l.region)
                    };
                    (l.code, label)
                })
                .collect();
            None
        }
        ListState::Failed(e) => Some(format!("Could not list languages: {e}")),
        _ => Some("Fetching the language list...".to_owned()),
    };
    let row_h = 18.0;
    let downloading = languages::downloading();
    let options = &mut state.options;
    state.language_list.rows(p, ctx, &area, entries.len(), row_h, |p, i, y| {
        let (code, label) = &entries[i];
        let selected = *code == options.language;
        if selected {
            p.outline(left, y, width, row_h, 0xFFFF_FFFF);
        }
        let hovered = area.hovered(ctx, y, row_h);
        let mut text = label.clone();
        if downloading.as_deref() == Some(code.as_str()) {
            text.push_str(" ...");
        }
        let w = p.atlas.font.width_str(&text);
        p.text_str(
            &text,
            ((ctx.vw - w) / 2.0).floor(),
            y + 5.0,
            if hovered { 0xFFFF55 } else { 0xFFFFFF },
            true,
        );
        if hovered && ctx.input.left_click && !selected {
            options.language = code.clone();
            crate::gui::save_options(options);
            languages::choose(code);
        }
    });
    if let Some(status) = status {
        let w = p.atlas.font.width_str(&status);
        p.text_str(
            &status,
            ((ctx.vw - w) / 2.0).floor(),
            area.y + area.h + 6.0,
            0xA0A0A0,
            true,
        );
    }
    let footer = LAYOUT.footer_y(ctx.vh);
    let cx = (ctx.vw / 2.0).floor();
    if Button::new(
        cx - 155.0,
        footer,
        COLUMN_W,
        WIDGET_HEIGHT,
        "Font Settings...",
    )
    .draw(p, ctx)
    {
        state.nav = Some(Screen::FontSettings);
    }
    if Button::new(cx + 5.0, footer, COLUMN_W, WIDGET_HEIGHT, "Done").draw(p, ctx)
        || ctx.input.escape
    {
        state.nav = Some(Screen::Options);
    }
}
