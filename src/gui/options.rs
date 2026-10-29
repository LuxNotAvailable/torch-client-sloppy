use crate::gui::focus;
use crate::gui::form::Form;
use crate::gui::painter::Painter;
use crate::gui::render::{GuiInput, auto_gui_scale};
use crate::gui::scroll_list::{ListArea, ScrollList};
use crate::gui::widgets::{
    self, Button, HeaderFooter, Slider, WIDGET_HEIGHT, WIDGET_WIDTH_BIG, centered_x,
};
use crate::gui::{GAMMA_MAX, GuiState, Screen, ScreenCtx, keybinds};
use crate::text::LINE_HEIGHT;

const TITLE: &str = "Options";

const VIDEO_TITLE: &str = "Video Settings";
const VIDEO: &str = "Video Settings...";

const CONTROLS_TITLE: &str = "Controls";
const CONTROLS: &str = "Controls...";
const MOUSE_TITLE: &str = "Mouse Settings";
const MOUSE_SETTINGS: &str = "Mouse Settings...";
const SENSITIVITY: &str = "Sensitivity";
const SCROLL_SENSITIVITY: &str = "Scroll Sensitivity";
const INVERT_X: &str = "Invert Mouse X";
const INVERT_Y: &str = "Invert Mouse Y";
const DISCRETE_SCROLLING: &str = "Discrete Scrolling";
const SNEAK: &str = "Sneak";
const SPRINT: &str = "Sprint";
const HOLD: &str = "Hold";
const TOGGLE: &str = "Toggle";
const MOUSE_SENSITIVITY_SLIDER_ID: u32 = 40;
const VIDEO_SLIDER_ID_BASE: u32 = 300;
const ZOOM_DEFAULT_SLIDER_ID: u32 = 50;
const CHAT_SLIDER_ID_BASE: u32 = 60;
const ENTITY_SHADOWS: &str = "Entity Shadows";
const SHADOW_OPACITY: &str = "Shadow Opacity";
const ZOOM_TITLE: &str = "Zoom Settings";
const ZOOM_SETTINGS: &str = "Zoom...";
const FEATURES: &str = "Features...";
const FEATURES_TITLE: &str = "Features";
const INVENTORY_SORT: &str = "Inventory Sorting...";
const INVENTORY_SORT_TITLE: &str = "Inventory Sorting";
const SORT_BUTTONS: &str = "Sort Buttons";
const SORT_HOTBAR: &str = "Include Hotbar";
const RESET_ZOOM: &str = "Reset Zoom Settings";

#[cfg(audio)]
const AUDIO_TITLE: &str = "Music & Sounds";
#[cfg(audio)]
const AUDIO: &str = "Music & Sounds...";

const CHAT_TITLE: &str = "Chat Settings";
const CHAT: &str = "Chat Settings...";

const SKIN_TITLE: &str = "Skin Customisation";
const SKIN: &str = "Skin Customisation...";

#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const STORAGE_HEADER: &str = "Storage";
#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const CLEAR_ASSETS: &str = "Clear Cached Assets";

#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const CLEAR_ASSETS_TITLE: &str = "Clear cached assets?";
#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const CLEAR_ASSETS_MESSAGE: &str = concat!(
    "The stored copy of the game's assets will be deleted.\n",
    "The client downloads about 44 MB from Mojang again\n",
    "the next time this page is opened.",
);
#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const CLEAR_ASSETS_WARNING: &str = "The page reloads immediately.";
#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const CLEAR_ASSETS_YES: &str = "Clear";
#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const CLEAR_ASSETS_NO: &str = "Cancel";

const DISPLAY_HEADER: &str = "Display";

const QUALITY_HEADER: &str = "Quality & Performance";

const DONE: &str = "Done";

const GUI_SCALE: &str = "GUI Scale";

const CLOUDS: &str = "Clouds";

const PARTICLES: &str = "Particles";

#[cfg(feature = "builtin_shaders")]
const SHADER_QUALITY: &str = "Shader Quality";

#[cfg(feature = "builtin_shaders")]
const SHADERS: &str = "Shaders";

#[cfg(feature = "builtin_shaders")]
const PLAYER_SHADOWS: &str = "Player Shadows";

const ANTIALIASING: &str = "Antialiasing";
const AA_LEVEL: &str = "AA Level";

const ON: &str = "ON";
const OFF: &str = "OFF";

const SMOOTH_LIGHTING: &str = "Smooth Lighting";
const LIGHTING_ENGINE: &str = "Lighting Engine";
const MAX_FRAMERATE: &str = "Max Framerate";
const UNLIMITED: &str = "Unlimited";
const VSYNC: &str = "VSync";

const FPS_COUNTER: &str = "FPS Counter";
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
const RENDERER: &str = "Renderer";

#[cfg(feature = "mobile_ui")]
const TOUCH_MOVEMENT: &str = "Movement";

const RESET_VIDEO: &str = "Reset Video Settings";

const RESET_CHAT: &str = "Reset Chat Settings";

const RESET_ALL: &str = "Reset All Options";

const MANAGE_STORAGE: &str = "Manage Storage...";
const MANAGE_STORAGE_TITLE: &str = "Manage Storage";
const OPEN_CLIENT_FOLDER: &str = "Open torch-client Folder";
#[cfg(feature = "asset_download")]
const CHECK_ASSETS: &str = "Install missing assets";
#[cfg(feature = "asset_download")]
const NOTHING_MISSING: &str = "Nothing is missing";

const AUTO_JUMP: &str = "Auto-Jump";
const VIEW_BOBBING: &str = "View Bobbing";

const MAIN_HAND: &str = "Main Hand";
const MAIN_HAND_LEFT: &str = "Left";
const MAIN_HAND_RIGHT: &str = "Right";

#[cfg(feature = "skins")]
const SKIN_PART_LABELS: [&str; 7] = [
    "Cape",
    "Jacket",
    "Left Sleeve",
    "Right Sleeve",
    "Left Pants Leg",
    "Right Pants Leg",
    "Hat",
];

const SIGNED_CHAT: &str = "Allow Signed Chat";

const TOAST_HEADER: &str = "Toasts";

const TOAST_ADVANCEMENT: &str = "Advancements";
const TOAST_RECIPE: &str = "Recipes";
const TOAST_SYSTEM: &str = "System";

const TOAST_TIME: &str = "Toast Time";

const SIGNED_CHAT_UNAVAILABLE: &str = "Needs a Microsoft account";

const FOV: &str = "FOV";

const BRIGHTNESS: &str = "Brightness";

const BRIGHTNESS_MOODY: &str = "Moody";

const BRIGHTNESS_BRIGHT: &str = "Bright";

const RENDER_DISTANCE: &str = "Render Distance";

const CHUNKS: &str = "Chunks";

const FOV_NORMAL: &str = "Normal";

const FOV_QUAKE_PRO: &str = "Quake Pro";

const FOV_MIN: i32 = 30;
const FOV_MAX: i32 = 110;

const FOV_EFFECTS: &str = "FOV Effects";

const FOV_EFFECTS_MAX: i32 = 100;

pub(crate) const RENDER_DISTANCE_MIN: i32 = 2;
pub(crate) const RENDER_DISTANCE_MAX: i32 = 32;

const GUI_SCALE_AUTO: &str = "Auto";

pub(in crate::gui) const LAYOUT: HeaderFooter = HeaderFooter::default_heights();


const FOV_SLIDER_ID: u32 = 2;





#[cfg(audio)]
const VOLUME_SLIDER_ID_BASE: u32 = 7;

const ROW_SPACING: f32 = 4.0;

fn menu_content_h(entries: usize) -> f32 {
    let rows = entries.div_ceil(2) as f32;
    rows * WIDGET_HEIGHT + (rows - 1.0) * ROW_SPACING
}

pub(in crate::gui) const ROW_H: f32 = 25.0;

pub(in crate::gui) const ROW_INSET: f32 = 2.0;

pub(in crate::gui) const GRID_LEFT_OFFSET: f32 = 155.0;

pub(in crate::gui) const COLUMN_STEP: f32 = 160.0;

pub(in crate::gui) const COLUMN_W: f32 = 150.0;

pub(in crate::gui) const BIG_W: f32 = 310.0;

pub(in crate::gui) const HEADER_PAD_TOP: f32 = 2.0 * LINE_HEIGHT;

pub(in crate::gui) const HEADER_H: f32 = LINE_HEIGHT + 4.0;

#[cfg(feature = "builtin_shaders")]
const Y_QUALITY_ROW_4: f32 = Y_QUALITY_ROW_3 + ROW_H;

#[cfg(feature = "builtin_shaders")]
const Y_QUALITY_LAST: f32 = Y_QUALITY_ROW_4;



#[cfg(not(feature = "shader_support"))]
const Y_RESET_ROW: f32 = Y_SHADOW_ROW + ROW_H;


const LIST_PAD_BOTTOM: f32 = 4.0;


pub(in crate::gui) const SCROLLBAR_W: f32 = 6.0;

const SCROLLBAR_MIN_H: f32 = 32.0;

const SCROLLBAR_MAX_SLACK: f32 = 8.0;

pub(in crate::gui) const SCROLLBAR_GAP: f32 = SCROLLBAR_W + 2.0;

const SCROLL_RATE: f32 = ROW_H / 2.0;

#[cfg(feature = "mobile_ui")]
const SCROLLBAR_TOUCH_PAD: f32 = 8.0;
#[cfg(not(feature = "mobile_ui"))]
const SCROLLBAR_TOUCH_PAD: f32 = 0.0;

fn scroller_height(list_h: f32, content_h: f32) -> f32 {
    let h = list_h * list_h / content_h.max(1.0);
    h.min(list_h - SCROLLBAR_MAX_SLACK).max(SCROLLBAR_MIN_H)
}

fn scroller_y(scroll: f32, max_scroll: f32, list_y: f32, list_h: f32, scroller_h: f32) -> f32 {
    if max_scroll <= 0.0 {
        return list_y;
    }
    list_y + (scroll * (list_h - scroller_h) / max_scroll).max(0.0)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::gui) fn scroll_input(
    scroll: &mut f32,
    drag: &mut Option<f32>,
    ctx: &ScreenCtx,
    left: f32,
    row_w: f32,
    list_y: f32,
    list_h: f32,
    content_h: f32,
) {
    let max_scroll = (content_h - list_h).max(0.0);
    if max_scroll <= 0.0 {
        *scroll = 0.0;
        *drag = None;
        return;
    }

    if ctx.input.scroll != 0.0 && ctx.hovering(0.0, list_y, ctx.vw, list_h) {
        *scroll -= ctx.input.scroll * SCROLL_RATE;
    }

    let scroller_h = scroller_height(list_h, content_h);
    let bar_x = left + row_w + SCROLLBAR_GAP;
    let over_bar = ctx.hovering(
        bar_x - SCROLLBAR_TOUCH_PAD,
        list_y,
        SCROLLBAR_W + 2.0 * SCROLLBAR_TOUCH_PAD,
        list_h,
    );

    #[cfg(feature = "mobile_ui")]
    content_drag(scroll, ctx, list_y, list_h, over_bar, max_scroll);

    if !ctx.input.left_down || ctx.input.left_release {
        *drag = None;
    } else if ctx.input.left_click && over_bar {
        *drag = ctx.mouse().map(|m| m.y);
    } else if let (Some(previous), Some(m)) = (*drag, ctx.mouse()) {
        if m.y < list_y {
            *scroll = 0.0;
        } else if m.y > list_y + list_h {
            *scroll = max_scroll;
        } else {
            let per_unit = (max_scroll / (list_h - scroller_h)).max(1.0);
            *scroll += (m.y - previous) * per_unit;
        }
        *drag = Some(m.y);
    }

    *scroll = scroll.clamp(0.0, max_scroll);
}

#[cfg_attr(not(any(feature = "shader_support", resource_packs)), allow(dead_code))]
pub(in crate::gui) fn masked_input(
    ctx: &ScreenCtx,
    rect: (f32, f32, f32, f32),
    dragging: bool,
) -> GuiInput {
    let (x, y, w, h) = rect;
    let over = ctx.hovering(x, y, w, h);
    let mut input = ctx.input.clone();
    if dragging || (!over && !widgets::slider_dragging()) {
        input.mouse = None;
    }
    if dragging || !over {
        input.left_click = false;
    }
    input
}

#[cfg(feature = "mobile_ui")]
pub(in crate::gui) fn content_drag(
    scroll: &mut f32,
    ctx: &ScreenCtx,
    list_y: f32,
    list_h: f32,
    over_bar: bool,
    max_scroll: f32,
) {
    content_drag_in(
        scroll, ctx, 0.0, list_y, ctx.vw, list_h, over_bar, max_scroll,
    );
}

#[cfg(feature = "mobile_ui")]
#[allow(clippy::too_many_arguments)]
pub(in crate::gui) fn content_drag_in(
    scroll: &mut f32,
    ctx: &ScreenCtx,
    list_x: f32,
    list_y: f32,
    list_w: f32,
    list_h: f32,
    over_bar: bool,
    max_scroll: f32,
) {
    use crate::gui::widgets::touch_tap;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    const DRAG_THRESHOLD: f32 = 4.0;

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static LAST_Y: AtomicU32 = AtomicU32::new(0);
    static START_Y: AtomicU32 = AtomicU32::new(0);

    let Some(m) = ctx.mouse() else {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    };

    if !ctx.input.left_down || ctx.input.left_release {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }

    if ctx.input.left_click {
        let inside = ctx.hovering(list_x, list_y, list_w, list_h);
        ACTIVE.store(inside && !over_bar, Ordering::Relaxed);
        LAST_Y.store(m.y.to_bits(), Ordering::Relaxed);
        START_Y.store(m.y.to_bits(), Ordering::Relaxed);
        return;
    }

    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let start = f32::from_bits(START_Y.load(Ordering::Relaxed));
    let last = f32::from_bits(LAST_Y.load(Ordering::Relaxed));
    LAST_Y.store(m.y.to_bits(), Ordering::Relaxed);
    if (m.y - start).abs() < DRAG_THRESHOLD && !touch_tap::scrolled() {
        return;
    }
    touch_tap::mark_scrolled();
    *scroll = (*scroll - (m.y - last)).clamp(0.0, max_scroll);
}

pub(in crate::gui) fn draw_scrollbar(
    p: &mut Painter,
    scroll: f32,
    left: f32,
    row_w: f32,
    list_y: f32,
    list_h: f32,
    content_h: f32,
) {
    let max_scroll = content_h - list_h;
    if max_scroll <= 0.0 {
        return;
    }
    let x = left + row_w + SCROLLBAR_GAP;
    let scroller_h = scroller_height(list_h, content_h);
    p.sprite("widget/scroller_background", x, list_y, SCROLLBAR_W, list_h);
    p.sprite(
        "widget/scroller",
        x,
        scroller_y(scroll, max_scroll, list_y, list_h, scroller_h).floor(),
        SCROLLBAR_W,
        scroller_h,
    );
}



#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const Y_STORAGE_HEADER: f32 = Y_GAME_LAST_GROUP_ROW + ROW_H;
#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const Y_STORAGE_ROW_0: f32 = Y_STORAGE_HEADER + HEADER_PAD_TOP + HEADER_H;

#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
const Y_GAME_RESET_ROW: f32 = Y_STORAGE_ROW_0 + ROW_H;



#[cfg(audio)]
const Y_AUDIO_MASTER: f32 = 0.0;
#[cfg(audio)]
const Y_AUDIO_ROW_0: f32 = Y_AUDIO_MASTER + ROW_H;
#[cfg(audio)]
const Y_AUDIO_ROW_1: f32 = Y_AUDIO_ROW_0 + ROW_H;
#[cfg(audio)]
const Y_AUDIO_ROW_2: f32 = Y_AUDIO_ROW_1 + ROW_H;
#[cfg(audio)]
const Y_AUDIO_ROW_3: f32 = Y_AUDIO_ROW_2 + ROW_H;
#[cfg(audio)]
const Y_AUDIO_ROW_4: f32 = Y_AUDIO_ROW_3 + ROW_H;

#[cfg(audio)]
const Y_AUDIO_DEVICES: f32 = Y_AUDIO_ROW_4 + ROW_H;
#[cfg(audio)]
const AUDIO_CONTENT_H: f32 = Y_AUDIO_DEVICES + ROW_H;

#[cfg(voice)]
const VOICE_PLAYER_SLIDER_ID_BASE: u32 = 1000;

fn fov_label(value: i32) -> String {
    match value {
        70 => format!("{FOV}: {FOV_NORMAL}"),
        FOV_MAX => format!("{FOV}: {FOV_QUAKE_PRO}"),
        v => format!("{FOV}: {v}"),
    }
}

fn fov_effects_label(value: i32) -> String {
    if value == 0 {
        format!("{FOV_EFFECTS}: {OFF}")
    } else {
        format!("{FOV_EFFECTS}: {value}%")
    }
}

fn shadow_opacity_label(value: i32) -> String {
    format!("{SHADOW_OPACITY}: {value}%")
}

fn render_distance_label(value: i32) -> String {
    format!("{RENDER_DISTANCE}: {value} {CHUNKS}")
}

fn brightness_label(value: i32) -> String {
    match value {
        0 => format!("{BRIGHTNESS}: {BRIGHTNESS_MOODY}"),
        100 => format!("{BRIGHTNESS}: {BRIGHTNESS_BRIGHT}"),
        v => format!("{BRIGHTNESS}: {v}%"),
    }
}

fn toast_time_label(value: i32) -> String {
    format!("{TOAST_TIME}: {}.{}s", value / 10, value % 10)
}

fn gui_scale_label(value: i32) -> String {
    if value == 0 {
        format!("{GUI_SCALE}: {GUI_SCALE_AUTO}")
    } else {
        format!("{GUI_SCALE}: {value}")
    }
}

#[cfg(audio)]
fn volume_label(name: &str, value: i32) -> String {
    if value == 0 {
        format!("{name}: {OFF}")
    } else {
        format!("{name}: {value}%")
    }
}

fn toggle(p: &mut Painter, ctx: &ScreenCtx, at: (f32, f32), label: &str, value: &mut bool) -> bool {
    let caption = format!("{label}: {}", if *value { ON } else { OFF });
    let (x, y) = at;

    if !Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &caption).draw(p, ctx) {
        return false;
    }

    *value = !*value;
    true
}

pub(in crate::gui) fn draw_title(p: &mut Painter, ctx: &ScreenCtx, title: &str) {
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), title);
}

pub(in crate::gui) fn draw_done(p: &mut Painter, ctx: &ScreenCtx) -> bool {
    Button::new(
        centered_x(ctx.vw),
        LAYOUT.footer_y(ctx.vh),
        WIDGET_WIDTH_BIG,
        WIDGET_HEIGHT,
        DONE,
    )
    .draw(p, ctx)
}

fn reset_video_options(o: &mut crate::gui::GuiOptions) {
    o.gui_scale = 0;
    o.fov_effects = crate::gui::FOV_EFFECTS_DEFAULT;
    o.view_bobbing = true;
    o.entity_shadows = true;
    o.shadow_opacity = 50;
    o.clouds = if cfg!(feature = "mobile_ui") {
        crate::renderer::clouds::CloudStatus::Fast
    } else {
        Default::default()
    };
    o.particles = if cfg!(feature = "mobile_ui") {
        crate::util::particles::ParticleStatus::Decreased
    } else {
        Default::default()
    };
    o.render_distance = crate::gui::RENDER_DISTANCE_DEFAULT;
    o.gamma = crate::gui::GAMMA_DEFAULT;
    o.antialiasing = crate::renderer::antialias::Antialiasing::default();
    o.smooth_lighting = crate::gui::SMOOTH_LIGHTING_DEFAULT;
    o.lighting_enabled = crate::gui::LIGHTING_ENABLED_DEFAULT;
    o.max_fps = crate::gui::MAX_FPS_DEFAULT;
    o.vsync = crate::gui::VSYNC_DEFAULT;
    o.fps_counter = crate::gui::FPS_COUNTER_DEFAULT;
    o.shaders_enabled = false;
    o.shader_quality = Default::default();
    o.player_shadows = false;
    o.touch_movement = Default::default();
}

fn reset_chat_options(o: &mut crate::gui::GuiOptions) {
    o.chat = crate::gui::ChatOptions::default();
    o.allow_signed_chat = false;
    o.toast_advancement = true;
    o.toast_recipe = true;
    o.toast_system = true;
    o.toast_time = crate::gui::TOAST_TIME_DEFAULT;
}

#[cfg(audio)]
fn reset_audio_options(o: &mut crate::gui::GuiOptions) {
    o.volumes = [crate::gui::VOLUME_DEFAULT; crate::audio::category::SoundCategory::COUNT];
}

/// Vanilla's `OptionsScreen` header: 61 pixels holding the title and, 8
/// below it, a row of FOV beside one more button.
const MENU_LAYOUT: HeaderFooter = HeaderFooter::new(61.0, HeaderFooter::DEFAULT_H);
const MENU_HEADER_ROW_Y: f32 = 12.0 + LINE_HEIGHT + 8.0;
/// Its grid: 150 wide buttons padded 4 each side, 4 below.
const MENU_COLUMN_STEP: f32 = COLUMN_W + 8.0;
const MENU_ROW_STEP: f32 = WIDGET_HEIGHT + 4.0;

pub fn draw(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, TITLE);

    enum Entry {
        Screen(Screen),
        #[cfg(resource_packs)]
        Packs,
    }

    let left = (ctx.vw / 2.0 - MENU_COLUMN_STEP + 4.0).floor();

    // The header row: FOV, and voice chat where vanilla has Online.
    let fov = Slider {
        id: FOV_SLIDER_ID,
        x: left,
        y: MENU_HEADER_ROW_Y,
        w: COLUMN_W,
        h: WIDGET_HEIGHT,
        min: FOV_MIN,
        max: FOV_MAX,
        value: state.options.fov as i32,
        label: fov_label,
    };
    let new_fov = fov.draw(p, ctx).clamp(FOV_MIN, FOV_MAX) as u32;
    if new_fov != state.options.fov {
        state.options.fov = new_fov;
        crate::gui::save_options(&state.options);
    }
    #[cfg(voice)]
    let header_entry = Some(("Voice Chat...", Entry::Screen(Screen::VoiceChat)));
    #[cfg(not(voice))]
    let header_entry: Option<(&str, Entry)> = None;

    // Vanilla's grid order. Torch's own screens take the places of
    // Language, Accessibility, Telemetry and Credits, so Chat Settings and
    // Resource Packs sit where vanilla players look for them.
    let mut entries = vec![(SKIN, Entry::Screen(Screen::SkinCustomisation))];
    #[cfg(audio)]
    entries.push((AUDIO, Entry::Screen(Screen::AudioSettings)));
    entries.push((VIDEO, Entry::Screen(Screen::VideoSettings)));
    entries.push((CONTROLS, Entry::Screen(Screen::Controls)));
    entries.push((FEATURES, Entry::Screen(Screen::Features)));
    entries.push((CHAT, Entry::Screen(Screen::ChatSettings)));
    #[cfg(resource_packs)]
    entries.push((crate::gui::resourcepacks::RESOURCE_PACKS, Entry::Packs));
    entries.push(("Language...", Entry::Screen(Screen::Language)));
    entries.push((
        "Accessibility Settings...",
        Entry::Screen(Screen::AccessibilitySettings),
    ));
    entries.push(("Online...", Entry::Screen(Screen::OnlineSettings)));
    entries.push(("Debug Options...", Entry::Screen(Screen::DebugOptions)));
    entries.push((MANAGE_STORAGE, Entry::Screen(Screen::ManageStorage)));

    let rows = entries.len().div_ceil(2) as f32;
    let top = MENU_LAYOUT.content_y(ctx.vh, rows * MENU_ROW_STEP);
    let mut buttons: Vec<(f32, f32, &str, &Entry)> = entries
        .iter()
        .enumerate()
        .map(|(i, (label, entry))| {
            let x = left + (i % 2) as f32 * MENU_COLUMN_STEP;
            (x, top + (i / 2) as f32 * MENU_ROW_STEP, *label, entry)
        })
        .collect();
    if let Some((label, entry)) = &header_entry {
        buttons.insert(
            0,
            (left + MENU_COLUMN_STEP, MENU_HEADER_ROW_Y, *label, entry),
        );
    }

    for (x, y, label, entry) in buttons {
        let button = Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, label);
        if !button.draw(p, ctx) {
            continue;
        }
        match entry {
            Entry::Screen(screen) => {
                state.video_list.reset();
                state.game_list.reset();
                #[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
                {
                    state.confirm_clear_assets = false;
                }
                #[cfg(feature = "asset_download")]
                {
                    state.storage_nothing_missing = false;
                }
                state.controls_list.reset();
                state.keybinds.capturing = None;
                state.nav = Some(*screen);
            }
            #[cfg(resource_packs)]
            Entry::Packs => {
                state.resourcepacks.open();
                state.nav = Some(Screen::ResourcePacks);
            }
        }
    }

    if draw_done(p, ctx) {
        state.nav = Some(state.options_parent);
    }
}

pub fn draw_manage_storage(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, MANAGE_STORAGE_TITLE);

    let mut count = 1;
    if cfg!(not(any(target_arch = "wasm32", target_os = "android"))) {
        count += 1;
    }
    if cfg!(feature = "asset_download") {
        count += 1;
    }
    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let top = LAYOUT.content_y(ctx.vh, menu_content_h(count));
    #[allow(unused)]
    let cell = |i: usize| {
        (
            left + (i % 2) as f32 * COLUMN_STEP,
            top + (i / 2) as f32 * (WIDGET_HEIGHT + ROW_SPACING),
        )
    };
    #[allow(unused)]
    let mut next = 1;

    if Button::new(left, top, COLUMN_W, WIDGET_HEIGHT, RESET_ALL).draw(p, ctx) {
        reset_video_options(&mut state.options);
        reset_chat_options(&mut state.options);
        let o = &mut state.options;
        o.fov = crate::renderer::systems::VANILLA_FOV_DEGREES as u32;
        o.auto_jump = crate::gui::AUTO_JUMP_DEFAULT;
        o.main_hand_left = false;
        #[cfg(feature = "skins")]
        {
            o.skin_parts = crate::client::skins::ALL_PARTS;
        }
        #[cfg(audio)]
        {
            reset_audio_options(&mut state.options);
            crate::audio::set_volumes(&state.options.volumes);
        }
        state.keybinds.reset_all();
        crate::gui::save_options(&state.options);
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    {
        let (x, y) = cell(next);
        next += 1;
        if Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, OPEN_CLIENT_FOLDER).draw(p, ctx) {
            crate::platform::url::open_folder(&crate::platform::storage::dir());
        }
    }

    #[cfg(feature = "asset_download")]
    {
        let (x, y) = cell(next);
        let label = if state.storage_nothing_missing {
            NOTHING_MISSING
        } else {
            CHECK_ASSETS
        };
        if Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, label).draw(p, ctx) {
            state.storage_nothing_missing = !crate::gui::assetdownload::check_missing(state);
        }
    }

    if draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

/// A settings form inside a scrolling list filling the content area: `draw`
/// lays out the controls, and the list learns its height from them. Whether
/// anything changed.
fn scrolling_form(
    p: &mut Painter,
    ctx: &ScreenCtx,
    list: &mut ScrollList,
    id_base: u32,
    draw: impl FnOnce(&mut Form<'_, '_, '_>),
) -> bool {
    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let clip = LAYOUT.content_rect(ctx.vw, ctx.vh);
    let area = ListArea {
        clip,
        left,
        width: BIG_W,
        y: clip.1,
        h: clip.3,
    };
    let frame = list.begin(p, ctx, &area, list.content_h);
    let input = list.masked_input(ctx, &area);
    let list_ctx = ScreenCtx {
        input: &input,
        vw: ctx.vw,
        vh: ctx.vh,
    };
    let mut form = Form::embedded(p, &list_ctx, left, frame.top.floor(), id_base);
    draw(&mut form);
    list.content_h = form.height() + LIST_PAD_BOTTOM;
    let changed = form.finish();
    list.end(p, frame, &area, list.content_h);
    changed
}

pub fn draw_video(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, VIDEO_TITLE);
    let max_gui_scale = auto_gui_scale(ctx.vw * ctx.input.scale, ctx.vh * ctx.input.scale);
    let o = &mut state.options;
    #[cfg_attr(any(target_arch = "wasm32", target_os = "android"), allow(unused_mut))]
    let mut renderer_tip: Option<String> = None;

    let changed = scrolling_form(p, ctx, &mut state.video_list, VIDEO_SLIDER_ID_BASE, |f| {
        f.header(DISPLAY_HEADER);
        let max_fps_label = format!(
            "{MAX_FRAMERATE}: {}",
            if o.max_fps >= crate::gui::MAX_FPS_UNLIMITED {
                UNLIMITED.to_string()
            } else {
                format!("{} fps", o.max_fps)
            }
        );
        if f.button(&max_fps_label) {
            const STEPS: [u32; 6] = [30, 60, 75, 120, 144, crate::gui::MAX_FPS_UNLIMITED];
            o.max_fps = STEPS
                .iter()
                .find(|s| **s > o.max_fps)
                .copied()
                .unwrap_or(crate::gui::MAX_FPS_MIN);
            f.mark_changed();
        }
        f.slider(&mut o.gui_scale, 0, max_gui_scale as u32, gui_scale_label);
        f.slider(&mut o.gamma, 0, GAMMA_MAX, brightness_label);
        f.toggle(VIEW_BOBBING, &mut o.view_bobbing);
        f.toggle(FPS_COUNTER, &mut o.fps_counter);
        #[cfg(not(target_arch = "wasm32"))]
        f.toggle(VSYNC, &mut o.vsync);
        #[cfg(target_arch = "wasm32")]
        f.skip();
        f.slider(
            &mut o.fov_effects,
            0,
            FOV_EFFECTS_MAX as u32,
            fov_effects_label,
        );
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            let backend = o.render_backend;
            let pending = backend != crate::renderer::systems::started_backend();
            let label = format!(
                "{RENDERER}: {}{}",
                backend.caption(),
                if pending { "*" } else { "" }
            );
            let (x, y) = f.cell();
            let hovered = f.ctx().hovering(x, y, COLUMN_W, WIDGET_HEIGHT);
            if f.draw(Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &label)) {
                o.render_backend = backend.next();
                f.mark_changed();
            }
            if pending && hovered {
                renderer_tip = Some(format!(
                    "Restart to start on {}",
                    o.render_backend.caption()
                ));
            }
        }

        f.header(QUALITY_HEADER);
        f.slider(
            &mut o.render_distance,
            RENDER_DISTANCE_MIN as u32,
            RENDER_DISTANCE_MAX as u32,
            render_distance_label,
        );
        if f.button(&format!("{CLOUDS}: {}", o.clouds.caption())) {
            o.clouds = o.clouds.next();
            f.mark_changed();
        }
        f.toggle(SMOOTH_LIGHTING, &mut o.smooth_lighting);
        // Antialiasing: the mode here, and its level in row 3's free right cell.
        let (x, y) = f.cell();
        let mode_hovered = f.ctx().hovering(x, y, COLUMN_W, WIDGET_HEIGHT);
        let label = format!("{ANTIALIASING}: {}", o.antialiasing.mode.caption());
        if f.draw(Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &label)) {
            o.antialiasing.mode = o.antialiasing.mode.next();
            f.mark_changed();
        }
        f.toggle(LIGHTING_ENGINE, &mut o.lighting_enabled);
        #[cfg(feature = "builtin_shaders")]
        f.toggle(SHADERS, &mut o.shaders_enabled);
        #[cfg(not(feature = "builtin_shaders"))]
        f.skip();
        if f.button(&format!("{PARTICLES}: {}", o.particles.caption())) {
            o.particles = o.particles.next();
            f.mark_changed();
        }
        let (x, y) = f.cell();
        let level_hovered = f.ctx().hovering(x, y, COLUMN_W, WIDGET_HEIGHT);
        let level = o.antialiasing.level_caption();
        let label = format!("{AA_LEVEL}: {}", level.as_deref().unwrap_or("-"));
        let level_button = Button {
            active: level.is_some(),
            ..Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &label)
        };
        if f.draw(level_button) {
            o.antialiasing.next_level();
            f.mark_changed();
        }
        #[cfg(feature = "shader_support")]
        if (mode_hovered || level_hovered)
            && crate::renderer::packvertex::active()
            && matches!(
                o.antialiasing.mode,
                crate::renderer::antialias::AaMode::Msaa
                    | crate::renderer::antialias::AaMode::Supersample
            )
        {
            renderer_tip = Some("Off while a shader pack is on; FXAA still works".to_owned());
        }
        #[cfg(not(feature = "shader_support"))]
        let _ = (mode_hovered, level_hovered);

        #[cfg(feature = "builtin_shaders")]
        {
            let enabled = o.shaders_enabled;
            let label = format!("{SHADER_QUALITY}: {}", o.shader_quality.caption());
            if f.button_active(&label, enabled) {
                o.shader_quality = o.shader_quality.next();
                f.mark_changed();
            }
            let label = format!(
                "{PLAYER_SHADOWS}: {}",
                if o.player_shadows { ON } else { OFF }
            );
            if f.button_active(&label, enabled) {
                o.player_shadows = !o.player_shadows;
                f.mark_changed();
            }
        }

        f.toggle(ENTITY_SHADOWS, &mut o.entity_shadows);
        f.slider(&mut o.shadow_opacity, 0, 100, shadow_opacity_label);
        #[cfg(feature = "shader_support")]
        if f.wide_button(crate::gui::shaderpacks::SHADER_PACKS) {
            state.shaderpacks.refresh();
            state.nav = Some(Screen::ShaderPacks);
        }
        if f.wide_button(RESET_VIDEO) {
            reset_video_options(o);
            f.mark_changed();
        }
    });

    if changed {
        crate::gui::save_options(&state.options);
    }
    if let (Some(tip), Some(m)) = (renderer_tip, ctx.mouse()) {
        draw_bind_tooltip(p, &[tip], m.x, m.y, ctx.vw);
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

enum BindRow {
    Category(&'static str),
    Bind(usize),
}

fn bind_rows() -> Vec<BindRow> {
    let mut rows = Vec::with_capacity(keybinds::BINDS.len() + 8);
    let mut category = "";
    for (i, def) in keybinds::BINDS.iter().enumerate() {
        if def.category != category {
            category = def.category;
            rows.push(BindRow::Category(category));
        }
        rows.push(BindRow::Bind(i));
    }
    rows
}

const BIND_ROW_H: f32 = 20.0;

const BIND_ROW_W: f32 = 340.0;

const CHANGE_W: f32 = 75.0;
const RESET_W: f32 = 50.0;

const BIND_PADDING: f32 = 10.0;

const BIND_BUTTON_GAP: f32 = 5.0;

const COLLISION_COLOR: u32 = 0xFFFF_FF00;
const COLLISION_W: f32 = 3.0;

const RESET: &str = "Reset";
const RESET_ALL_KEYS: &str = "Reset Keys";

const DUPLICATE: &str = "This key is also used for:";

const FOOTER_GAP: f32 = 8.0;
const FOOTER_BUTTON_W: f32 = 150.0;

fn zoom_level_label(name: &str, tenths: i32) -> String {
    format!("{name}: {}x", tenths as f32 / 10.0)
}

fn zoom_default_label(tenths: i32) -> String {
    zoom_level_label("Default Zoom", tenths)
}

fn zoom_max_label(tenths: i32) -> String {
    zoom_level_label("Max Zoom", tenths)
}

fn zoom_smoothing_label(ms: i32) -> String {
    if ms == 0 {
        format!("Zoom Smoothing: {OFF}")
    } else {
        format!("Zoom Smoothing: {ms} ms")
    }
}

fn zoom_sensitivity_label(percent: i32) -> String {
    format!("Sensitivity Scaling: {percent}%")
}

/// The zoom key's settings: where a press starts, how far the wheel goes,
/// how the view eases, and how much the mouse slows with it.
pub fn draw_zoom(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    use crate::gui::{ZOOM_SMOOTHING_MAX_MS, ZOOM_TENTHS_MAX, ZOOM_TENTHS_MIN, ZoomOptions};
    let mut form = crate::gui::form::Form::new(p, ctx, ZOOM_TITLE, 3, ZOOM_DEFAULT_SLIDER_ID);
    let z = &mut state.options.zoom;
    let max = z.max_tenths;
    form.slider(
        &mut z.default_tenths,
        ZOOM_TENTHS_MIN,
        max,
        zoom_default_label,
    );
    form.slider(
        &mut z.max_tenths,
        ZOOM_TENTHS_MIN,
        ZOOM_TENTHS_MAX,
        zoom_max_label,
    );
    // The default can't sit past the maximum.
    z.default_tenths = z.default_tenths.min(z.max_tenths);
    form.slider(
        &mut z.smoothing_ms,
        0,
        ZOOM_SMOOTHING_MAX_MS,
        zoom_smoothing_label,
    );
    form.slider(&mut z.sensitivity, 0, 100, zoom_sensitivity_label);
    if form.wide_button(RESET_ZOOM) {
        *z = ZoomOptions::default();
        form.mark_changed();
    }
    if form.finish() {
        crate::gui::save_options(&state.options);
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Features);
    }
}

/// Torch's own features, each with its settings.
pub fn draw_features(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, FEATURES_TITLE);
    #[allow(unused_mut)]
    let mut entries = vec![
        (ZOOM_SETTINGS, Screen::ZoomSettings),
        ("Block Info...", Screen::BlockInfoSettings),
        (INVENTORY_SORT, Screen::InventorySortSettings),
        ("AppleSkin...", Screen::AppleSkinSettings),
    ];
    #[cfg(feature = "hud_editor")]
    entries.push(("Edit HUD...", Screen::HudEditor));
    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let top = LAYOUT.content_y(ctx.vh, menu_content_h(entries.len()));
    for (i, (label, screen)) in entries.into_iter().enumerate() {
        let x = left + (i % 2) as f32 * COLUMN_STEP;
        let y = top + (i / 2) as f32 * (WIDGET_HEIGHT + ROW_SPACING);
        if Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, label).draw(p, ctx) {
            #[cfg(feature = "hud_editor")]
            if screen == Screen::HudEditor {
                // The editor returns here when done.
                state.hud_state.parent = Screen::Features;
                state.hud_state.over_menu = state.in_menu();
            }
            state.nav = Some(screen);
        }
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

/// Inventory sorting: whether the buttons show, and whether the hotbar
/// sorts and stores along with the rest of the inventory.
pub fn draw_inventory_sort(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let mut form = crate::gui::form::Form::new(p, ctx, INVENTORY_SORT_TITLE, 1, 0);
    form.toggle(SORT_BUTTONS, &mut state.options.sort_buttons);
    form.toggle(SORT_HOTBAR, &mut state.options.sort_hotbar);
    if form.finish() {
        crate::gui::save_options(&state.options);
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Features);
    }
}

/// AppleSkin's additions, each on or off.
pub fn draw_appleskin(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    let mut form = crate::gui::form::Form::new(p, ctx, "AppleSkin", 3, 0);
    let o = &mut state.options.appleskin;
    form.toggle("Saturation Overlay", &mut o.saturation_overlay);
    form.toggle("Held Food Preview", &mut o.food_preview);
    form.toggle("Health Preview", &mut o.health_preview);
    form.toggle("Food Tooltips", &mut o.tooltip);
    if form.wide_button("Reset AppleSkin Settings") {
        *o = crate::gui::appleskin::AppleSkinOptions::default();
        form.mark_changed();
    }
    if form.finish() {
        crate::gui::save_options(&state.options);
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Features);
    }
}

/// `0%` and `200%` read as vanilla's jokes.
fn sensitivity_label(value: i32) -> String {
    match value {
        0 => format!("{SENSITIVITY}: *yawn*"),
        200 => format!("{SENSITIVITY}: HYPERSPEED!!!"),
        v => format!("{SENSITIVITY}: {v}%"),
    }
}

fn scroll_label(value: i32) -> String {
    format!("{SCROLL_SENSITIVITY}: {:.2}", value as f32 / 100.0)
}

pub fn draw_mouse(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    use crate::gui::{MOUSE_SENSITIVITY_MAX, SCROLL_SENSITIVITY_MAX, SCROLL_SENSITIVITY_MIN};
    let mut form = Form::new(p, ctx, MOUSE_TITLE, 3, MOUSE_SENSITIVITY_SLIDER_ID);
    let o = &mut state.options;
    form.slider(
        &mut o.mouse_sensitivity,
        0,
        MOUSE_SENSITIVITY_MAX,
        sensitivity_label,
    );
    form.slider(
        &mut o.scroll_sensitivity,
        SCROLL_SENSITIVITY_MIN,
        SCROLL_SENSITIVITY_MAX,
        scroll_label,
    );
    form.toggle(INVERT_X, &mut o.invert_x);
    form.toggle(INVERT_Y, &mut o.invert_y);
    form.toggle(DISCRETE_SCROLLING, &mut o.discrete_scrolling);
    if form.finish() {
        crate::gui::save_options(&state.options);
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Controls);
    }
}

fn hold_or_toggle(
    p: &mut Painter,
    ctx: &ScreenCtx,
    (x, y, w): (f32, f32, f32),
    label: &str,
    value: &mut bool,
) -> bool {
    let caption = format!("{label}: {}", if *value { TOGGLE } else { HOLD });
    if !Button::new(x, y, w, WIDGET_HEIGHT, &caption).draw(p, ctx) {
        return false;
    }
    *value = !*value;
    true
}

pub fn draw_controls(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, CONTROLS_TITLE);

    let captured = state
        .keybinds
        .resolve_capture(ctx.input.pressed_key, ctx.input.pressed_mouse);
    let mut screen_input = ctx.input.clone();
    if captured {
        screen_input.left_click = false;
    }
    let screen_ctx = &ScreenCtx {
        input: &screen_input,
        vw: ctx.vw,
        vh: ctx.vh,
    };

    let rows = bind_rows();
    let content_h = rows.len() as f32 * BIND_ROW_H;
    let list_h_total = content_h + LIST_PAD_BOTTOM;

    let left = (ctx.vw / 2.0 - BIND_ROW_W / 2.0).floor();
    let (list_x, list_y, list_w, list_h) = LAYOUT.content_rect(ctx.vw, ctx.vh);

    #[cfg(feature = "mobile_ui")]
    let (list_y, list_h) = {
        let x = centered_x(ctx.vw) + (WIDGET_WIDTH_BIG - COLUMN_W) / 2.0;
        let label = format!(
            "{TOUCH_MOVEMENT}: {}",
            state.options.touch_movement.caption()
        );
        if Button::new(x, list_y, COLUMN_W, WIDGET_HEIGHT, &label).draw(p, ctx) {
            state.options.touch_movement = state.options.touch_movement.next();
            crate::gui::save_options(&state.options);
        }
        let advance = WIDGET_HEIGHT + ROW_SPACING;
        (list_y + advance, (list_h - advance).max(0.0))
    };

    // Vanilla's controls header: mouse settings beside auto-jump (where the
    // key binds button sits; the binds are listed below), then the
    // hold/toggle pair.
    let (list_y, list_h) = {
        let x = (ctx.vw / 2.0 - BIG_W / 2.0).floor();
        let right = x + COLUMN_STEP;
        let second = list_y + WIDGET_HEIGHT + ROW_SPACING;
        if Button::new(x, list_y, COLUMN_W, WIDGET_HEIGHT, MOUSE_SETTINGS).draw(p, screen_ctx) {
            state.nav = Some(Screen::MouseSettings);
        }
        let o = &mut state.options;
        let mut changed = toggle(p, screen_ctx, (right, list_y), AUTO_JUMP, &mut o.auto_jump);
        changed |= hold_or_toggle(
            p,
            screen_ctx,
            (x, second, COLUMN_W),
            SNEAK,
            &mut o.toggle_crouch,
        );
        changed |= hold_or_toggle(
            p,
            screen_ctx,
            (right, second, COLUMN_W),
            SPRINT,
            &mut o.toggle_sprint,
        );
        if changed {
            crate::gui::save_options(&state.options);
        }
        let advance = 2.0 * (WIDGET_HEIGHT + ROW_SPACING);
        (list_y + advance, (list_h - advance).max(0.0))
    };

    let area = ListArea {
        clip: (list_x, list_y, list_w, list_h),
        left,
        width: BIND_ROW_W,
        y: list_y,
        h: list_h,
    };
    state.controls_list.input(screen_ctx, &area, list_h_total);
    let top = state.controls_list.top(&area).floor();

    let over_list = screen_ctx.hovering(list_x, list_y, list_w, list_h);
    let dragging_bar = state.controls_list.drag.is_some();
    let mut list_input = screen_input.clone();
    if dragging_bar || !over_list {
        list_input.mouse = None;
        list_input.left_click = false;
    }
    let list_ctx = &ScreenCtx {
        input: &list_input,
        vw: ctx.vw,
        vh: ctx.vh,
    };

    let bar_x = left + BIND_ROW_W + SCROLLBAR_GAP;
    let reset_x = bar_x - RESET_W - BIND_PADDING;
    let change_x = reset_x - BIND_BUTTON_GAP - CHANGE_W;

    focus::suspend();
    let clip = p.push_clip(list_x, list_y, list_w, list_h);
    let mut tooltip: Option<Vec<String>> = None;

    for (i, row) in rows.iter().enumerate() {
        let row_y = top + i as f32 * BIND_ROW_H;
        if row_y + BIND_ROW_H < list_y || row_y > list_y + list_h {
            continue;
        }
        match row {
            BindRow::Category(name) => {
                let w = p.atlas.font.width_str(name);
                p.text_str(
                    name,
                    ((ctx.vw - w) / 2.0).floor(),
                    row_y + BIND_ROW_H - LINE_HEIGHT - ROW_INSET,
                    0xFFFFFF,
                    true,
                );
            }
            BindRow::Bind(bind) => {
                let def = &keybinds::BINDS[*bind];
                let collisions = state.keybinds.collisions(*bind);
                let selected = state.keybinds.capturing == Some(*bind);

                p.text_str(def.name, left, row_y + 6.0, 0xFFFFFF, true);

                if !collisions.is_empty() {
                    p.fill(
                        change_x - 6.0,
                        row_y + ROW_INSET - 1.0,
                        COLLISION_W,
                        BIND_ROW_H - 2.0 * ROW_INSET + 1.0,
                        COLLISION_COLOR,
                    );
                }

                let key_name = state.keybinds.row(*bind).display_name();
                let (label, color) = if selected {
                    (format!("> {key_name} <"), 0xFFFF55)
                } else if collisions.is_empty() {
                    (key_name.to_string(), 0xFFFFFF)
                } else {
                    (format!("[ {key_name} ]"), 0xFFFF55)
                };
                let change = Button::new(change_x, row_y, CHANGE_W, BIND_ROW_H, "");
                if change.draw(p, list_ctx) {
                    state.keybinds.capturing = Some(*bind);
                }
                let lw = p.atlas.font.width_str(&label);
                p.text_str(
                    &label,
                    (change_x + (CHANGE_W - lw) / 2.0).floor(),
                    row_y + 6.0,
                    color,
                    true,
                );

                if !collisions.is_empty()
                    && list_ctx.hovering(change_x, row_y, CHANGE_W, BIND_ROW_H)
                {
                    tooltip = Some(vec![DUPLICATE.to_string(), collisions.join(", ")]);
                }

                let reset = Button {
                    x: reset_x,
                    y: row_y,
                    w: RESET_W,
                    h: BIND_ROW_H,
                    label: RESET,
                    active: !state.keybinds.is_default(*bind),
                };
                if reset.draw(p, list_ctx) {
                    state.keybinds.reset(*bind);
                }
            }
        }
    }

    p.pop_clip(clip);
    focus::resume();
    state.controls_list.draw_bar(p, &area, list_h_total);

    if let (Some(lines), Some(m)) = (tooltip, list_ctx.mouse()) {
        draw_bind_tooltip(p, &lines, m.x, m.y, ctx.vw);
    }

    let footer_left = ((ctx.vw - 2.0 * FOOTER_BUTTON_W - FOOTER_GAP) / 2.0).floor();
    let footer_y = LAYOUT.footer_y(ctx.vh);
    let reset_all = Button {
        x: footer_left,
        y: footer_y,
        w: FOOTER_BUTTON_W,
        h: WIDGET_HEIGHT,
        label: RESET_ALL_KEYS,
        active: state.keybinds.any_rebound(),
    };
    if reset_all.draw(p, screen_ctx) {
        state.keybinds.reset_all();
    }
    let done = Button::new(
        footer_left + FOOTER_BUTTON_W + FOOTER_GAP,
        footer_y,
        FOOTER_BUTTON_W,
        WIDGET_HEIGHT,
        DONE,
    );
    if done.draw(p, screen_ctx) {
        state.keybinds.capturing = None;
        state.nav = Some(Screen::Options);
    }
}

pub(in crate::gui) fn draw_bind_tooltip(
    p: &mut Painter,
    lines: &[String],
    mx: f32,
    my: f32,
    vw: f32,
) {
    let w = lines
        .iter()
        .map(|l| p.atlas.font.width_str(l))
        .fold(0.0f32, f32::max);
    let h = lines.len() as f32 * LINE_HEIGHT + (lines.len() as f32 - 1.0);
    let (x, y) = crate::gui::tooltip::position_flat(mx, my, vw, w);
    p.fill(x - 3.0, y - 3.0, w + 6.0, h + 6.0, 0xF010_0010);
    for (i, line) in lines.iter().enumerate() {
        p.text_str(line, x, y + i as f32 * (LINE_HEIGHT + 1.0), 0xFFFFFF, true);
    }
}

#[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
fn draw_clear_assets(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    crate::gui::screens::dim_background(p, ctx.vw, ctx.vh);
    let buttons = [CLEAR_ASSETS_NO.to_owned(), CLEAR_ASSETS_YES.to_owned()];
    let chosen = crate::gui::confirmlink::draw_box(
        p,
        ctx,
        CLEAR_ASSETS_TITLE,
        CLEAR_ASSETS_MESSAGE,
        Some(CLEAR_ASSETS_WARNING),
        &buttons,
        COLUMN_W,
    );
    match chosen {
        Some(1) => {
            if let Err(e) = crate::client::assets::clear_web_cache() {
                crate::log_warn!("assets", "could not clear the stored archive: {e}");
            }
            state.confirm_clear_assets = false;
        }
        Some(_) => state.confirm_clear_assets = false,
        None => {}
    }
}

/// Vanilla's `SkinCustomizationScreen`: a toggle for each layer of the skin,
/// in `PlayerModelPart` order, then the main hand.
pub fn draw_skin(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, SKIN_TITLE);

    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let (_, list_y, _, _) = LAYOUT.content_rect(ctx.vw, ctx.vh);
    let cell = |i: usize| {
        (
            left + (i % 2) as f32 * COLUMN_STEP,
            list_y + (i / 2) as f32 * ROW_H + ROW_INSET,
        )
    };

    #[cfg_attr(not(feature = "skins"), allow(unused_mut))]
    let mut next = 0;
    #[cfg(feature = "skins")]
    for (label, bit) in SKIN_PART_LABELS
        .iter()
        .zip(crate::gui::MODEL_PART_KEYS.map(|(_, bit)| bit))
    {
        let mut on = state.options.skin_parts & (1 << bit) != 0;
        if toggle(p, ctx, cell(next), label, &mut on) {
            state.options.skin_parts ^= 1 << bit;
            crate::gui::save_options(&state.options);
        }
        next += 1;
    }

    let (x, y) = cell(next);
    let caption = format!(
        "{MAIN_HAND}: {}",
        if state.options.main_hand_left {
            MAIN_HAND_LEFT
        } else {
            MAIN_HAND_RIGHT
        }
    );
    if Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &caption).draw(p, ctx) {
        state.options.main_hand_left = !state.options.main_hand_left;
        crate::gui::save_options(&state.options);
    }

    if draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

pub fn draw_chat(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    #[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
    if state.confirm_clear_assets {
        draw_clear_assets(p, state, ctx);
        return;
    }

    draw_title(p, ctx, CHAT_TITLE);
    let can_sign = signing_available(state);
    let o = &mut state.options;
    let changed = scrolling_form(p, ctx, &mut state.game_list, CHAT_SLIDER_ID_BASE, |f| {
        chat_options(f, &mut o.chat);

        if can_sign {
            f.toggle(SIGNED_CHAT, &mut o.allow_signed_chat);
        } else {
            f.button_active(&format!("{SIGNED_CHAT}: {OFF}"), false);
            let (x, y) = f.cell();
            f.painter().text_str(
                SIGNED_CHAT_UNAVAILABLE,
                x,
                y + (WIDGET_HEIGHT - LINE_HEIGHT) / 2.0,
                0xA0A0A0,
                true,
            );
        }

        f.header(TOAST_HEADER);
        f.toggle(TOAST_ADVANCEMENT, &mut o.toast_advancement);
        f.toggle(TOAST_RECIPE, &mut o.toast_recipe);
        f.toggle(TOAST_SYSTEM, &mut o.toast_system);
        f.slider(
            &mut o.toast_time,
            crate::gui::TOAST_TIME_MIN as u32,
            crate::gui::TOAST_TIME_MAX as u32,
            toast_time_label,
        );

        #[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
        {
            f.header(STORAGE_HEADER);
            if f.wide_button(CLEAR_ASSETS) {
                state.confirm_clear_assets = true;
            }
        }

        if f.wide_button(RESET_CHAT) {
            reset_chat_options(o);
            f.mark_changed();
        }
    });

    if changed {
        crate::gui::save_options(&state.options);
    }
    if draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

/// `ChatOptionsScreen`'s options, seven rows of two.
fn chat_options(f: &mut Form, c: &mut crate::gui::ChatOptions) {
    use crate::gui::CHAT_DELAY_MAX_TENTHS;
    use crate::percent_label;
    if f.button(&format!("Chat: {}", c.shown.caption())) {
        c.shown = c.shown.next();
        f.mark_changed();
    }
    f.toggle("Colors", &mut c.colors);
    f.toggle("Web Links", &mut c.links);
    f.toggle("Prompt on Links", &mut c.links_prompt);
    f.slider(&mut c.opacity, 0, 100, percent_label!("Chat Text Opacity"));
    f.slider(
        &mut c.background,
        0,
        100,
        percent_label!("Text Background Opacity"),
    );
    f.slider(&mut c.scale, 0, 100, percent_label!("Chat Text Size"));
    f.slider(&mut c.line_spacing, 0, 100, percent_label!("Line Spacing"));
    f.slider(&mut c.delay_tenths, 0, CHAT_DELAY_MAX_TENTHS, |v| {
        if v == 0 {
            "Chat Delay: None".to_owned()
        } else {
            format!("Chat Delay: {:.1} Seconds", v as f32 / 10.0)
        }
    });
    f.slider(&mut c.width, 40, 320, |v| format!("Width: {v}px"));
    f.slider(&mut c.focused_height, 20, 180, |v| {
        format!("Focused Height: {v}px")
    });
    f.slider(&mut c.unfocused_height, 20, 180, |v| {
        format!("Unfocused Height: {v}px")
    });
    f.toggle("Command Suggestions", &mut c.auto_suggestions);
    f.toggle("Only Show Secure Chat", &mut c.only_secure);
}

#[cfg(audio)]
pub fn draw_audio(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    use crate::audio::category::SoundCategory;

    draw_title(p, ctx, AUDIO_TITLE);

    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let top = LAYOUT.content_y(ctx.vh, AUDIO_CONTENT_H);
    let cell = |column: f32, row_y: f32| (left + column * COLUMN_STEP, top + row_y + ROW_INSET);

    let master = SoundCategory::Master;
    let (x, y) = (left, top + Y_AUDIO_MASTER + ROW_INSET);
    let master_slider = Slider {
        id: VOLUME_SLIDER_ID_BASE + master.index() as u32,
        x,
        y,
        w: BIG_W,
        h: WIDGET_HEIGHT,
        min: 0,
        max: 100,
        value: state.options.volumes[master.index()] as i32,
        label: |v| volume_label(master.label(), v),
    };
    let new_master = master_slider.draw(p, ctx).clamp(0, 100) as u32;
    if new_master != state.options.volumes[master.index()] {
        state.options.volumes[master.index()] = new_master;
        crate::gui::save_options(&state.options);
        crate::audio::set_volumes(&state.options.volumes);
    }

    let rows = [
        Y_AUDIO_ROW_0,
        Y_AUDIO_ROW_1,
        Y_AUDIO_ROW_2,
        Y_AUDIO_ROW_3,
        Y_AUDIO_ROW_4,
    ];
    for (i, category) in SoundCategory::ALL
        .into_iter()
        .filter(|c| *c != SoundCategory::Master)
        .enumerate()
    {
        let row_y = rows[i / 2];
        let column = (i % 2) as f32;
        let (x, y) = cell(column, row_y);
        let slider = Slider {
            id: VOLUME_SLIDER_ID_BASE + category.index() as u32,
            x,
            y,
            w: COLUMN_W,
            h: WIDGET_HEIGHT,
            min: 0,
            max: 100,
            value: state.options.volumes[category.index()] as i32,
            label: |v| volume_label(category.label(), v),
        };
        let new_value = slider.draw(p, ctx).clamp(0, 100) as u32;
        if new_value != state.options.volumes[category.index()] {
            state.options.volumes[category.index()] = new_value;
            crate::gui::save_options(&state.options);
            crate::audio::set_volumes(&state.options.volumes);
        }
    }

    draw_devices(p, state, ctx, &cell);

    if draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

/// `name` cut to what fits `width` pixels, with an ellipsis where cut.
#[cfg(audio)]
pub(in crate::gui) fn fit(p: &Painter, label: &str, name: &str, width: f32) -> String {
    let whole = format!("{label}: {name}");
    if p.atlas.font.width_str(&whole) <= width {
        return whole;
    }
    let mut cut: String = name.to_owned();
    while !cut.is_empty() && p.atlas.font.width_str(&format!("{label}: {cut}...")) > width {
        cut.pop();
    }
    format!("{label}: {cut}...")
}

/// The speaker and (with voice chat) the microphone. A click moves to the
/// next device, after the last back to the system default; the list is
/// read again on each click, so a device plugged in shows up.
#[cfg(audio)]
fn draw_devices(
    p: &mut Painter,
    state: &mut GuiState,
    ctx: &ScreenCtx,
    cell: &dyn Fn(f32, f32) -> (f32, f32),
) {
    use crate::audio::devices;
    const DEFAULT: &str = "Default";
    let width = COLUMN_W - 8.0;

    let (x, y) = cell(0.0, Y_AUDIO_DEVICES);
    let name = state.options.sound_device.clone();
    let caption = fit(p, "Speaker", name.as_deref().unwrap_or(DEFAULT), width);
    if Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &caption).draw(p, ctx) {
        let next = devices::next(name.as_deref(), &devices::outputs(true));
        state.options.sound_device = next.clone();
        crate::audio::set_output_device(next);
        crate::gui::save_options(&state.options);
    }
}

/// `AdjustVolumeSlider`'s caption: the change from normal, or muted at 0.
#[cfg(voice)]
fn player_volume_label(name: &str, percent: i32) -> String {
    if percent <= 0 {
        return format!("{name}: Muted");
    }
    let change = percent - 100;
    format!("{name}: {}{change}%", if change > 0 { "+" } else { "" })
}

/// Every player with voice chat on this server, and every player given a
/// volume before (offline ones greyed by name only), each with a slider from
/// muted to four times as loud, as Simple Voice Chat's volume screen has.
#[cfg(voice)]
pub fn draw_voice_players(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    draw_title(p, ctx, "Player Volumes");

    let online = crate::voice::voice_players();
    let mut rows: Vec<(uuid::Uuid, String, bool)> = online
        .iter()
        .map(|(id, name)| (*id, name.clone(), true))
        .collect();
    for (id, (name, _)) in &state.options.voice.player_volumes {
        if !online.iter().any(|(o, _)| o == id) {
            rows.push((*id, name.clone(), false));
        }
    }

    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let (list_x, list_y, list_w, list_h) = LAYOUT.content_rect(ctx.vw, ctx.vh);
    let content_h = rows.len() as f32 * ROW_H;
    let area = ListArea {
        clip: (list_x, list_y, list_w, list_h),
        left: left,
        width: BIG_W,
        y: list_y,
        h: list_h,
    };
    state.voice_list.input(ctx, &area, content_h);
    let top = state.voice_list.top(&area).floor();

    if rows.is_empty() {
        let text = "No other players have voice chat here";
        let w = p.atlas.font.width_str(text);
        p.text_str(
            text,
            ((ctx.vw - w) / 2.0).floor(),
            list_y + ROW_INSET + 6.0,
            0xAAAAAA,
            true,
        );
    }

    let over_list = ctx.hovering(list_x, list_y, list_w, list_h);
    let dragging_bar = state.voice_list.drag.is_some();
    let mut list_input = ctx.input.clone();
    if dragging_bar || (!over_list && !widgets::slider_dragging()) {
        list_input.mouse = None;
    }
    if dragging_bar || !over_list {
        list_input.left_click = false;
    }
    let list_ctx = &ScreenCtx {
        input: &list_input,
        vw: ctx.vw,
        vh: ctx.vh,
    };
    let clip = p.push_clip(list_x, list_y, list_w, list_h);
    let mut changed = false;
    for (i, (id, name, online)) in rows.iter().enumerate() {
        let current = state
            .options
            .voice
            .player_volumes
            .get(id)
            .map_or(100, |(_, v)| *v);
        let shown = if *online {
            name.clone()
        } else {
            format!("{name} (offline)")
        };
        let value = Slider {
            id: VOICE_PLAYER_SLIDER_ID_BASE + i as u32,
            x: left,
            y: top + i as f32 * ROW_H + ROW_INSET,
            w: BIG_W,
            h: WIDGET_HEIGHT,
            min: 0,
            max: 400,
            value: current as i32,
            label: |v| player_volume_label(&shown, v),
        }
        .draw(p, list_ctx)
        .clamp(0, 400) as u32;
        if value != current {
            state
                .options
                .voice
                .player_volumes
                .insert(*id, (name.clone(), value));
            changed = true;
        }
    }
    p.pop_clip(clip);
    state.voice_list.draw_bar(p, &area, content_h);

    if changed {
        state.options.voice.apply();
        crate::gui::save_options(&state.options);
    }

    if draw_done(p, ctx) {
        state.nav = Some(Screen::VoiceChat);
    }
}

fn signing_available(state: &mut GuiState) -> bool {
    #[cfg(feature = "online_mode")]
    {
        use crate::gui::accountlist::AccountKind;
        state.profile.ensure_loaded();
        state
            .profile
            .accounts
            .active()
            .is_some_and(|a| a.kind == AccountKind::Microsoft)
    }
    #[cfg(not(feature = "online_mode"))]
    {
        let _ = state;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::GuiInput;
    use crate::gui::render::effective_gui_scale;
    use crate::gui::widgets::{slider_track_cursor, slider_value_from_mouse};

    const WIN_W: f32 = 1280.0;
    const WIN_H: f32 = 720.0;

    const CURSOR_PX: f32 = 520.0;

    fn feedback_step(option: u32) -> (f32, f32, i32) {
        let scale = effective_gui_scale(WIN_W, WIN_H, option);
        let x = centered_x(WIN_W / scale);
        let mouse_x = CURSOR_PX / scale;
        let max = auto_gui_scale(WIN_W, WIN_H) as i32;
        (
            scale,
            mouse_x,
            slider_value_from_mouse(mouse_x, x, WIDGET_WIDTH_BIG, 0, max),
        )
    }

    #[test]
    fn the_scale_feedback_loop_oscillates() {
        let mut option = 0u32;
        let mut seen = Vec::new();
        for _ in 0..8 {
            option = feedback_step(option).2.max(0) as u32;
            seen.push(option);
        }
        assert_eq!(seen, vec![1, 0, 1, 0, 1, 0, 1, 0]);
    }

    #[test]
    #[cfg(not(feature = "mobile_ui"))]
    fn a_scale_change_ends_the_drag() {
        const ID: u32 = VIDEO_SLIDER_ID_BASE;
        let mut option = 0u32;
        let mut values = Vec::new();
        let mut scales = Vec::new();
        for frame in 0..8 {
            let (scale, mouse_x, from_mouse) = feedback_step(option);
            let x = centered_x(WIN_W / scale);
            let hovered = mouse_x >= x && mouse_x < x + WIDGET_WIDTH_BIG;
            let input = GuiInput {
                scale,
                left_click: frame == 0,
                left_down: true,
                ..Default::default()
            };
            if slider_track_cursor(ID, scale, hovered, &input) {
                option = from_mouse.max(0) as u32;
            }
            values.push(option);
            scales.push(scale);
        }
        slider_track_cursor(
            ID,
            1.0,
            false,
            &GuiInput {
                left_release: true,
                ..Default::default()
            },
        );

        assert_eq!(values, vec![1; 8]);
        assert_eq!(scales, vec![3.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn layout_follows_header_and_footer_layout() {
        assert_eq!(LAYOUT.title_y(), 12.0);
        assert_eq!(LAYOUT.content_y(480.0, WIDGET_HEIGHT), 63.0);
        assert_eq!(LAYOUT.footer_y(480.0), 454.0);
        assert_eq!(centered_x(854.0), 327.0);
    }

    #[test]
    fn short_window_pulls_the_content_up() {
        assert_eq!(LAYOUT.content_y(100.0, WIDGET_HEIGHT), 47.0);
    }

    #[test]
    fn video_grid_stacks_like_the_options_list() {
        // Display: header and four rows; Quality: header (spaced) and four
        // rows, the shader row when built in; then entity shadows, the shader
        // packs button when built in, and reset.
        let atlas = crate::gui::atlas::build_gui_atlas(&crate::assets_root());
        let mut p = Painter::new(&atlas, 0);
        let mut state = GuiState::default();
        let input = GuiInput::default();
        let ctx = ScreenCtx {
            input: &input,
            vw: 427.0,
            vh: 240.0,
        };
        draw_video(&mut p, &mut state, &ctx);
        const EXTRA_ROWS: f32 = cfg!(feature = "builtin_shaders") as u32 as f32
            + cfg!(feature = "shader_support") as u32 as f32;
        let reset_row = 244.0 + (EXTRA_ROWS + 1.0) * ROW_H;
        assert_eq!(state.video_list.content_h, reset_row + ROW_H + LIST_PAD_BOTTOM);

        // A tall window has nothing to scroll.
        let (_, _, _, list_h) = LAYOUT.content_rect(854.0, 480.0);
        assert_eq!(list_h, 414.0);
        assert!(state.video_list.content_h < list_h);
    }

    #[test]
    fn scroller_height_follows_the_scroll_area() {
        assert_eq!(scroller_height(200.0, 400.0), 100.0);
        assert_eq!(scroller_height(200.0, 4000.0), SCROLLBAR_MIN_H);
        assert_eq!(scroller_height(200.0, 205.0), 192.0);
        assert_eq!(scroller_height(20.0, 400.0), SCROLLBAR_MIN_H);
    }

    #[test]
    fn scroller_travels_the_whole_track() {
        let (list_y, list_h, content_h) = (33.0, 200.0, 400.0);
        let max_scroll = content_h - list_h;
        let h = scroller_height(list_h, content_h);
        assert_eq!(scroller_y(0.0, max_scroll, list_y, list_h, h), list_y);
        assert_eq!(
            scroller_y(max_scroll, max_scroll, list_y, list_h, h),
            list_y + list_h - h
        );
    }

    #[test]
    fn video_columns_span_the_big_row() {
        let left = (854.0f32 / 2.0 - GRID_LEFT_OFFSET).floor();
        assert_eq!(left, 272.0);
        assert_eq!(left + COLUMN_STEP + COLUMN_W, left + BIG_W);
    }

    #[test]
    fn fov_label_matches_the_option_stringifier() {
        assert_eq!(fov_label(70), "FOV: Normal");
        assert_eq!(fov_label(110), "FOV: Quake Pro");
        assert_eq!(fov_label(90), "FOV: 90");
        assert_eq!(fov_label(30), "FOV: 30");
    }

    #[test]
    fn label_matches_the_option_stringifier() {
        assert_eq!(gui_scale_label(0), "GUI Scale: Auto");
        assert_eq!(gui_scale_label(3), "GUI Scale: 3");
    }

    #[test]
    fn render_distance_label_matches_the_option_stringifier() {
        assert_eq!(
            render_distance_label(RENDER_DISTANCE_MIN),
            "Render Distance: 2 Chunks"
        );
        assert_eq!(render_distance_label(12), "Render Distance: 12 Chunks");
        assert_eq!(
            render_distance_label(RENDER_DISTANCE_MAX),
            "Render Distance: 32 Chunks"
        );
    }

    #[cfg(audio)]
    #[test]
    fn a_zero_volume_reads_as_off() {
        assert_eq!(volume_label("Master Volume", 0), "Master Volume: OFF");
        assert_eq!(volume_label("Music", 55), "Music: 55%");
        assert_eq!(volume_label("Voice", 100), "Voice: 100%");
    }

    #[cfg(audio)]
    #[test]
    fn saved_volume_keys_match_vanillas_sound_category_names() {
        use crate::audio::category::SoundCategory;
        assert_eq!(
            format!("soundCategory_{}", SoundCategory::Master.key()),
            "soundCategory_master"
        );
        assert_eq!(
            format!("soundCategory_{}", SoundCategory::Records.key()),
            "soundCategory_record"
        );
        assert_eq!(
            format!("soundCategory_{}", SoundCategory::Voice.key()),
            "soundCategory_voice"
        );
    }

    #[cfg(audio)]
    #[test]
    fn a_saved_volume_percentage_round_trips_through_json() {
        use crate::audio::category::SoundCategory;
        use crate::gui::VOLUME_DEFAULT;

        let key = format!("soundCategory_{}", SoundCategory::Music.key());
        let json = serde_json::json!({ key.clone(): 63u32 });
        let value = |k: &str| json.get(k).cloned();

        let read_back = value(&key)
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(VOLUME_DEFAULT)
            .min(100);
        assert_eq!(read_back, 63);

        let missing = value("soundCategory_weather")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(VOLUME_DEFAULT)
            .min(100);
        assert_eq!(missing, VOLUME_DEFAULT);
    }
}
