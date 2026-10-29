//! Simple Voice Chat's screens: its settings, and its groups (join one,
//! with its password if it has one; create one; leave the one you are in).

use crate::gui::options::{
    COLUMN_STEP, COLUMN_W, GRID_LEFT_OFFSET, LAYOUT, ROW_H, ROW_INSET, draw_done, draw_title, fit,
};
use crate::gui::painter::Painter;
use crate::gui::widgets::{Button, Slider, TextBox, WIDGET_HEIGHT, WIDGET_WIDTH_BIG, centered_x};
use crate::gui::{GuiState, Screen, ScreenCtx};
use crate::voice::GroupType;

const VOLUME_SLIDER_ID: u32 = 50;
const THRESHOLD_SLIDER_ID: u32 = 51;

/// What the groups screen keeps between frames.
pub struct GroupsState {
    pub name: TextBox,
    pub password: TextBox,
    pub kind: GroupType,
    /// The group whose password is being asked for.
    pub joining: Option<uuid::Uuid>,
    pub join_password: TextBox,
}

impl Default for GroupsState {
    fn default() -> GroupsState {
        GroupsState {
            name: TextBox::bordered(crate::voice::wire::MAX_GROUP_NAME, "Group name"),
            password: TextBox::bordered(crate::voice::wire::MAX_GROUP_NAME, "Password (optional)"),
            kind: GroupType::Normal,
            joining: None,
            join_password: TextBox::bordered(crate::voice::wire::MAX_GROUP_NAME, "Password"),
        }
    }
}

fn kind_caption(kind: GroupType) -> &'static str {
    match kind {
        GroupType::Normal => "Normal",
        GroupType::Open => "Open",
        GroupType::Isolated => "Isolated",
    }
}

fn next_kind(kind: GroupType) -> GroupType {
    match kind {
        GroupType::Normal => GroupType::Open,
        GroupType::Open => GroupType::Isolated,
        GroupType::Isolated => GroupType::Normal,
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "ON" } else { "OFF" }
}

/// The settings: microphone, how it opens, volumes, the HUD's status, and
/// the way to the player volumes and the groups.
pub fn draw_settings(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, "Voice Chat");
    let rows = 5.0;
    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let top = LAYOUT.content_y(ctx.vh, rows * ROW_H);
    let cell = |column: f32, row: f32| (left + column * COLUMN_STEP, top + row * ROW_H + ROW_INSET);
    let button = |p: &mut Painter, (x, y): (f32, f32), caption: &str| {
        Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, caption).draw(p, ctx)
    };

    let before = state.options.voice.clone();
    let mut navigate = None;
    {
        let voice = &mut state.options.voice;

        if button(p, cell(0.0, 0.0), &format!("Voice Chat: {}", on_off(!voice.disabled))) {
            voice.disabled = !voice.disabled;
        }
        let device = voice.input_device.clone();
        let caption = fit(p, "Microphone", device.as_deref().unwrap_or("Default"), COLUMN_W - 8.0);
        if button(p, cell(1.0, 0.0), &caption) {
            voice.input_device =
                crate::audio::devices::next(device.as_deref(), &crate::audio::devices::inputs(true));
        }

        let mode = if voice.voice_activation {
            "Voice Activation"
        } else {
            "Push to Talk"
        };
        if button(p, cell(0.0, 1.0), &format!("Activation: {mode}")) {
            voice.voice_activation = !voice.voice_activation;
        }
        if button(p, cell(1.0, 1.0), &format!("Microphone Muted: {}", on_off(voice.muted))) {
            voice.muted = !voice.muted;
        }

        let (x, y) = cell(0.0, 2.0);
        voice.volume = Slider {
            id: VOLUME_SLIDER_ID,
            x,
            y,
            w: COLUMN_W,
            h: WIDGET_HEIGHT,
            min: 0,
            max: 300,
            value: voice.volume as i32,
            label: |v| format!("Voice Chat Volume: {v}%"),
        }
        .draw(p, ctx)
        .clamp(0, 300) as u32;
        let (x, y) = cell(1.0, 2.0);
        voice.threshold = Slider {
            id: THRESHOLD_SLIDER_ID,
            x,
            y,
            w: COLUMN_W,
            h: WIDGET_HEIGHT,
            min: -127,
            max: 0,
            value: voice.threshold,
            label: |v| format!("Activation Level: {v} dB"),
        }
        .draw(p, ctx)
        .clamp(-127, 0);

        if button(p, cell(0.0, 3.0), &format!("Status on HUD: {}", voice.status.caption())) {
            voice.status = voice.status.next();
        }
        if button(p, cell(1.0, 3.0), "Player Volumes...") {
            navigate = Some(Screen::VoicePlayers);
        }
        if button(p, cell(0.0, 4.0), "Groups...") {
            navigate = Some(Screen::VoiceGroups);
        }
    }

    if state.options.voice != before {
        state.options.voice.apply();
        crate::gui::save_options(&state.options);
    }
    if let Some(screen) = navigate {
        state.voice_scroll = 0.0;
        state.nav = Some(screen);
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

/// The groups: the one you are in with its members, the ones to join, and
/// a form to create one, as the mod's group screen offers.
pub fn draw_groups(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, "Voice Chat Groups");
    let groups = crate::voice::groups();
    let x = centered_x(ctx.vw);
    let (_, list_y, _, list_h) = LAYOUT.content_rect(ctx.vw, ctx.vh);
    let mut y = list_y + 4.0;
    let line = |p: &mut Painter, text: &str, y: f32, color: u32| {
        let w = p.atlas.font.width_str(text);
        p.text_str(text, ((ctx.vw - w) / 2.0).floor(), y, color, true);
    };

    if crate::voice::status() != crate::voice::Status::Connected {
        line(p, "Not connected to a voice chat server", y + 6.0, 0xAAAAAA);
        if draw_done(p, ctx) {
            state.nav = Some(Screen::VoiceChat);
        }
        return;
    }
    if !groups.enabled {
        line(p, "This server has groups turned off", y + 6.0, 0xAAAAAA);
        if draw_done(p, ctx) {
            state.nav = Some(Screen::VoiceChat);
        }
        return;
    }

    let gs = &mut state.voice_groups;

    // The group you are in.
    if let Some(current) = groups.current {
        let name = groups
            .list
            .iter()
            .find(|g| g.id == current)
            .map_or("a group", |g| g.name.as_str());
        line(p, &format!("In group: {name}"), y, 0x55FF55);
        y += 12.0;
        let members = crate::voice::group_members(current).join(", ");
        if !members.is_empty() {
            line(p, &members, y, 0xAAAAAA);
            y += 12.0;
        }
        if Button::new(x, y, WIDGET_WIDTH_BIG, WIDGET_HEIGHT, "Leave Group").draw(p, ctx) {
            crate::voice::leave_group();
        }
        y += ROW_H + 4.0;
    }

    // A password being asked for.
    if let Some(joining) = gs.joining {
        let name = groups
            .list
            .iter()
            .find(|g| g.id == joining)
            .map_or(String::new(), |g| g.name.clone());
        line(p, &format!("Password for {name}"), y, 0xFFFFFF);
        y += 12.0;
        gs.join_password.update(p, ctx, x, y, WIDGET_WIDTH_BIG, WIDGET_HEIGHT);
        y += ROW_H;
        let half = (WIDGET_WIDTH_BIG - 4.0) / 2.0;
        if Button::new(x, y, half, WIDGET_HEIGHT, "Join").draw(p, ctx) || ctx.input.enter {
            let password = gs.join_password.text.clone();
            crate::voice::join_group(joining, Some(&password));
            gs.joining = None;
            gs.join_password.clear();
        }
        if Button::new(x + half + 4.0, y, half, WIDGET_HEIGHT, "Cancel").draw(p, ctx) {
            gs.joining = None;
            gs.join_password.clear();
        }
        if draw_done(p, ctx) {
            state.nav = Some(Screen::VoiceChat);
        }
        return;
    }
    if groups.wrong_password {
        line(p, "Wrong password", y, 0xFF5555);
        y += 12.0;
    }

    // The groups to join.
    let bottom = list_y + list_h;
    let joinable: Vec<_> = groups.list.iter().filter(|g| Some(g.id) != groups.current).collect();
    if joinable.is_empty() {
        line(p, "No other groups", y + 2.0, 0xAAAAAA);
        y += 14.0;
    }
    for group in joinable {
        if y + WIDGET_HEIGHT > bottom - 3.0 * ROW_H {
            break;
        }
        let lock = if group.has_password { " (password)" } else { "" };
        let caption = format!("Join {}{lock} - {}", group.name, kind_caption(group.kind));
        if Button::new(x, y, WIDGET_WIDTH_BIG, WIDGET_HEIGHT, &caption).draw(p, ctx) {
            if group.has_password {
                gs.joining = Some(group.id);
                gs.join_password.clear();
                gs.join_password.focus();
            } else {
                crate::voice::join_group(group.id, None);
            }
        }
        y += ROW_H;
    }

    // Creating one.
    y += 4.0;
    line(p, "Create a group", y, 0xFFFFFF);
    y += 12.0;
    let half = (WIDGET_WIDTH_BIG - 4.0) / 2.0;
    gs.name.update(p, ctx, x, y, half, WIDGET_HEIGHT);
    gs.password.update(p, ctx, x + half + 4.0, y, half, WIDGET_HEIGHT);
    y += ROW_H;
    if Button::new(x, y, half, WIDGET_HEIGHT, &format!("Type: {}", kind_caption(gs.kind))).draw(p, ctx) {
        gs.kind = next_kind(gs.kind);
    }
    let name = gs.name.text.trim().to_owned();
    let create = Button {
        active: !name.is_empty(),
        ..Button::new(x + half + 4.0, y, half, WIDGET_HEIGHT, "Create")
    };
    if create.draw(p, ctx) && !name.is_empty() {
        let password = gs.password.text.trim().to_owned();
        crate::voice::create_group(&name, (!password.is_empty()).then_some(password.as_str()), gs.kind);
        gs.name.clear();
        gs.password.clear();
    }

    if draw_done(p, ctx) {
        state.nav = Some(Screen::VoiceChat);
    }
}
