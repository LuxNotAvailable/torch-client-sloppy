use std::sync::OnceLock;

use crate::gui::menu;
use crate::gui::painter::Painter;
use crate::gui::widgets::{Button, WIDGET_HEIGHT, WIDGET_WIDTH_BIG};
use crate::gui::{GuiState, Screen, ScreenCtx};
use crate::platform::time::{SystemTime, UNIX_EPOCH};
use crate::text::{Span, Style, texts};

const MULTIPLAYER: &str = "Multiplayer";
const OPTIONS: &str = "Options...";
const QUIT: &str = "Quit Game";
const EDIT_PROFILE: &str = "Manage Profiles";
const PROFILE_LABEL_PAD: f32 = 8.0;

/// The logo is drawn as wide as vanilla's, its height kept to `logo.png`'s
/// own proportions.
const LOGO_W: f32 = 256.0;
const LOGO_Y: f32 = 30.0;

const BOTTOM_ROW_STEP: f32 = 36.0;

const HALF_WIDTH: f32 = 98.0;
#[allow(dead_code, reason = "asserted on by this module's layout test")]
const BUTTON_GAP: f32 = 4.0;

use crate::ASSET_VERSION;

fn first_row_y(vh: f32) -> f32 {
    (vh / 4.0).floor() + 48.0
}

pub fn draw(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    if crate::renderer::panorama::absent() {
        menu::background(p, ctx.vw, ctx.vh);
    }

    let logo_x = (ctx.vw / 2.0 - LOGO_W / 2.0).floor();
    let logo_h = p
        .atlas
        .sprite(crate::gui::atlas::TORCH_LOGO)
        .map_or(44.0, |s| (LOGO_W * s.region.h as f32 / s.region.w.max(1) as f32).round());
    p.sprite(crate::gui::atlas::TORCH_LOGO, logo_x, LOGO_Y, LOGO_W, logo_h);

    if let Some(splash) = session_splash() {
        draw_splash(p, ctx, splash);
    }

    let x = ((ctx.vw - WIDGET_WIDTH_BIG) / 2.0).floor();
    let top = first_row_y(ctx.vh);

    if Button::new(x, top, WIDGET_WIDTH_BIG, WIDGET_HEIGHT, MULTIPLAYER).draw(p, ctx) {
        state.nav = Some(Screen::Multiplayer);
    }

    let bottom = top + BOTTOM_ROW_STEP;
    if Button::new(x, bottom, HALF_WIDTH, WIDGET_HEIGHT, OPTIONS).draw(p, ctx) {
        state.options_parent = Screen::Title;
        state.nav = Some(Screen::Options);
    }
    if Button::new(
        (ctx.vw / 2.0).floor() + 2.0,
        bottom,
        HALF_WIDTH,
        WIDGET_HEIGHT,
        QUIT,
    )
    .draw(p, ctx)
    {
        state.quit = true;
    }

    let version = format!(
        "Minecraft {ASSET_VERSION} (protocol {})",
        azalea_protocol::packets::PROTOCOL_VERSION
    );
    p.text_str(&version, 2.0, ctx.vh - 10.0, 0xFFFFFF, true);

    let disclaimer_y = ctx.vh - 20.0;
    let credits_x =
        p.text_str(DISCLAIMER, 2.0, disclaimer_y, 0xA0A0A0, true) + p.atlas.font.width_str(" ");
    if credits_link(p, ctx, credits_x, disclaimer_y) {
        state.nav = Some(Screen::Credits);
    }

    #[cfg(all(feature = "mobile_ui", target_arch = "wasm32"))]
    {
        const W: f32 = 90.0;
        let label = if crate::platform::fullscreen::is_fullscreen() {
            "Exit Fullscreen"
        } else {
            "Fullscreen"
        };
        if Button::new(ctx.vw - W - 4.0, 4.0, W, WIDGET_HEIGHT, label).draw(p, ctx) {
            crate::platform::fullscreen::toggle();
        }
    }

    {
        let w = p.atlas.font.width_str(EDIT_PROFILE) + 2.0 * PROFILE_LABEL_PAD;
        if Button::new(
            (ctx.vw - w - 4.0).floor(),
            ctx.vh - WIDGET_HEIGHT - 14.0,
            w,
            WIDGET_HEIGHT,
            EDIT_PROFILE,
        )
        .draw(p, ctx)
        {
            state.profile.open();
            state.nav = Some(Screen::Accounts);
        }
    }

    if crate::platform::address::EAGLER {
        let w = p.atlas.font.width_str(EAGLER_MODE);
        p.text_str(
            EAGLER_MODE,
            (ctx.vw - w - 2.0).floor(),
            ctx.vh - 10.0,
            0xFFAA00,
            true,
        );
    }
}

const DISCLAIMER: &str = "Not affiliated with Mojang or Microsoft.";
const CREDITS: &str = "(Credits)";

fn credits_link(p: &mut Painter, ctx: &ScreenCtx, x: f32, y: f32) -> bool {
    let w = p.atlas.font.width_str(CREDITS);
    let (hx, hy, hw, hh) = (x, y - 1.0, w, crate::text::LINE_HEIGHT + 1.0);
    let hovered = ctx.hovering(hx, hy, hw, hh);
    let span = Span {
        text: CREDITS.to_owned(),
        style: Style {
            color: 0xFFFFFF,
            underline: hovered,
            ..Style::default()
        },
    };
    p.text(std::slice::from_ref(&span), x, y, true);
    #[cfg(not(feature = "mobile_ui"))]
    let clicked = hovered && ctx.input.left_click;
    #[cfg(feature = "mobile_ui")]
    let clicked = hovered
        && ctx.input.left_release
        && crate::gui::widgets::touch_tap::pressed_in(hx, hy, hw, hh)
        && !crate::gui::widgets::touch_tap::scrolled();
    #[cfg(audio)]
    if clicked {
        crate::gui::widgets::play_click();
    }
    clicked
}

const EAGLER_MODE: &str = "Eagler mode (wss:// enabled)";

/// `SplashRenderer`: yellow, turned 20 degrees up, at the logo's right end,
/// pulsing in size once a second and shrunk to fit long lines.
fn draw_splash(p: &mut Painter, ctx: &ScreenCtx, splash: &str) {
    let width = p.atlas.font.width_str(splash);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() % 1000) as f32;
    let phase = 1.8 - (millis / 1000.0 * std::f32::consts::TAU).sin().abs() * 0.1;
    let scale = phase * 100.0 / (width + 32.0);
    let radians = -std::f32::consts::PI / 9.0;
    p.rotated(scale, radians, ctx.vw / 2.0 + 123.0, 69.0, |p| {
        p.text_str(splash, (-width / 2.0).trunc(), -8.0, 0xFFFF00, true);
    });
}

fn session_splash() -> Option<&'static str> {
    static SPLASH: OnceLock<Option<&'static str>> = OnceLock::new();
    *SPLASH.get_or_init(|| {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        texts::splash_for(seed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_row_matches_the_java_layout() {
        assert_eq!(first_row_y(240.0), 108.0);
        assert_eq!(first_row_y(480.0), 168.0);
    }

    #[test]
    fn the_bottom_row_lines_up_with_the_column() {
        let vw = 854.0;
        let left = ((vw - WIDGET_WIDTH_BIG) / 2.0).floor();
        let right = (vw / 2.0).floor() + 2.0;
        assert_eq!(left + HALF_WIDTH + BUTTON_GAP, right);
        assert_eq!(right + HALF_WIDTH, left + WIDGET_WIDTH_BIG);
    }

    #[test]
    fn the_bottom_row_sits_36_below_the_last_menu_row() {
        let top = first_row_y(240.0);
        assert_eq!(top + BOTTOM_ROW_STEP - (top + WIDGET_HEIGHT), 16.0);
    }
}
