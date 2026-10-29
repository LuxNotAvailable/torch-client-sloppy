//! The block information panel (as the Jade mod draws it) and its settings
//! screen: what the crosshair is on, an icon, extra lines, and the source,
//! in a box whose size, colours and contents are the player's to choose.

use crate::gui::painter::{ITEM_SIZE, Painter};
use crate::gui::widgets::{Button, Slider, WIDGET_HEIGHT};
use crate::gui::{GuiState, Screen, ScreenCtx};
use crate::play::looking::Looking;
use crate::text::{LINE_HEIGHT, Span, Style};

/// When a block's state properties show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Details {
    Never,
    /// While sneaking, as Jade's extended view.
    Sneaking,
    Always,
}

impl Details {
    fn caption(self) -> &'static str {
        match self {
            Details::Never => "Never",
            Details::Sneaking => "While Sneaking",
            Details::Always => "Always",
        }
    }

    fn next(self) -> Details {
        match self {
            Details::Never => Details::Sneaking,
            Details::Sneaking => Details::Always,
            Details::Always => Details::Never,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Details::Never => "never",
            Details::Sneaking => "sneaking",
            Details::Always => "always",
        }
    }

    fn from_key(key: &str) -> Option<Details> {
        match key {
            "never" => Some(Details::Never),
            "sneaking" => Some(Details::Sneaking),
            "always" => Some(Details::Always),
            _ => None,
        }
    }
}

/// Colours a click on a colour button steps through: name, then RGB.
const COLORS: &[(&str, u32)] = &[
    ("Jade", 0x100010),
    ("Black", 0x000000),
    ("Charcoal", 0x202020),
    ("Slate", 0x1E2A3A),
    ("Navy", 0x0A1A40),
    ("Forest", 0x0F2A14),
    ("Wine", 0x3A0A16),
    ("White", 0xFFFFFF),
];

const BORDERS: &[(&str, u32)] = &[
    ("Purple", 0x5000FF),
    ("Gold", 0xFFAA00),
    ("Aqua", 0x55FFFF),
    ("Green", 0x55FF55),
    ("Red", 0xFF5555),
    ("White", 0xFFFFFF),
    ("Grey", 0x555555),
    ("None", 0x000000),
];

const SOURCES: &[(&str, u32)] = &[
    ("Blue", 0x5555FF),
    ("Aqua", 0x55FFFF),
    ("Gold", 0xFFAA00),
    ("Grey", 0xAAAAAA),
    ("White", 0xFFFFFF),
];

#[derive(Clone, Debug, PartialEq)]
pub struct BlockInfoOptions {
    pub enabled: bool,
    /// Size, percent of the HUD's (50 to 300).
    pub scale: u32,
    /// The background's opacity, percent.
    pub opacity: u32,
    /// Index into `COLORS`.
    pub background: usize,
    /// Index into `BORDERS`; the last is no border.
    pub border: usize,
    /// Index into `SOURCES`.
    pub source_color: usize,
    pub icon: bool,
    pub source: bool,
    pub harvestable: bool,
    pub progress: bool,
    pub details: Details,
    /// The block's position, as a line.
    pub coordinates: bool,
    pub entity_health: bool,
}

impl Default for BlockInfoOptions {
    fn default() -> BlockInfoOptions {
        BlockInfoOptions {
            enabled: true,
            scale: 100,
            opacity: 85,
            background: 0,
            border: 0,
            source_color: 0,
            icon: true,
            source: true,
            harvestable: true,
            progress: true,
            details: Details::Sneaking,
            coordinates: false,
            entity_health: true,
        }
    }
}

impl BlockInfoOptions {
    pub fn load(json: Option<&serde_json::Value>) -> BlockInfoOptions {
        let d = BlockInfoOptions::default();
        let Some(json) = json else { return d };
        let flag = |key: &str, default: bool| json.get(key).and_then(|v| v.as_bool()).unwrap_or(default);
        let number = |key: &str, default: u32, max: u32| {
            json.get(key)
                .and_then(|v| v.as_u64())
                .map_or(default, |v| v.min(u64::from(max)) as u32)
        };
        let index = |key: &str, default: usize, len: usize| {
            json.get(key)
                .and_then(|v| v.as_u64())
                .map_or(default, |v| (v as usize).min(len - 1))
        };
        BlockInfoOptions {
            enabled: flag("enabled", d.enabled),
            scale: number("scale", d.scale, 300).max(50),
            opacity: number("opacity", d.opacity, 100),
            background: index("background", d.background, COLORS.len()),
            border: index("border", d.border, BORDERS.len()),
            source_color: index("sourceColor", d.source_color, SOURCES.len()),
            icon: flag("icon", d.icon),
            source: flag("source", d.source),
            harvestable: flag("harvestable", d.harvestable),
            progress: flag("progress", d.progress),
            details: json
                .get("details")
                .and_then(|v| v.as_str())
                .and_then(Details::from_key)
                .unwrap_or(d.details),
            coordinates: flag("coordinates", d.coordinates),
            entity_health: flag("entityHealth", d.entity_health),
        }
    }

    pub fn save(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled": self.enabled,
            "scale": self.scale,
            "opacity": self.opacity,
            "background": self.background,
            "border": self.border,
            "sourceColor": self.source_color,
            "icon": self.icon,
            "source": self.source,
            "harvestable": self.harvestable,
            "progress": self.progress,
            "details": self.details.key(),
            "coordinates": self.coordinates,
            "entityHealth": self.entity_health,
        })
    }
}

/// The item a block shows as its icon where the two ids differ.
fn block_icon(id: &str) -> std::borrow::Cow<'_, str> {
    let item = match id {
        "redstone_wire" => "redstone",
        "tripwire" => "string",
        "potatoes" => "potato",
        "carrots" => "carrot",
        "beetroots" => "beetroot",
        "cocoa" => "cocoa_beans",
        "sweet_berry_bush" => "sweet_berries",
        "melon_stem" | "attached_melon_stem" => "melon_seeds",
        "pumpkin_stem" | "attached_pumpkin_stem" => "pumpkin_seeds",
        "cave_vines" | "cave_vines_plant" => "glow_berries",
        "bamboo_sapling" => "bamboo",
        "kelp_plant" => "kelp",
        "tall_seagrass" => "seagrass",
        "big_dripleaf_stem" => "big_dripleaf",
        "twisting_vines_plant" => "twisting_vines",
        "weeping_vines_plant" => "weeping_vines",
        "water" | "bubble_column" => "water_bucket",
        "lava" => "lava_bucket",
        "powder_snow" => "powder_snow_bucket",
        "fire" | "soul_fire" => "flint_and_steel",
        "nether_portal" | "end_portal" | "end_gateway" => "obsidian",
        "piston_head" | "moving_piston" => "piston",
        "frosted_ice" => "ice",
        other => {
            if let Some(rest) = other.strip_prefix("potted_") {
                return std::borrow::Cow::Owned(rest.to_owned());
            }
            if other.contains("wall_") && !other.ends_with("_wall") {
                return std::borrow::Cow::Owned(other.replacen("wall_", "", 1));
            }
            other
        }
    };
    std::borrow::Cow::Borrowed(item)
}

/// A block's name. Wall banners have none of their own: vanilla names them
/// after the standing banner they drop, so a block without a name takes its
/// item's.
fn block_name(id: &str) -> String {
    use crate::gui::tooltip::{has_translation, translate};
    let own = format!("block.minecraft.{id}");
    if has_translation(&own) {
        return translate(&own, &[]);
    }
    let item = block_icon(id);
    for key in [format!("block.minecraft.{item}"), format!("item.minecraft.{item}")] {
        if has_translation(&key) {
            return translate(&key, &[]);
        }
    }
    translate(&own, &[])
}

/// A state property as Jade shows it, names with spaces for underscores.
fn property_line(name: &str, value: &str) -> String {
    format!("{}: {}", name.replace('_', " "), value.replace('_', " "))
}

/// The panel, drawn with its top centred at `centre_x`, `top`.
/// Draws the panel with its top at `top`, and returns its drawn height.
pub fn draw(p: &mut Painter, centre_x: f32, top: f32, looking: &Looking, sneaking: bool, o: &BlockInfoOptions) -> f32 {
    const PAD: f32 = 5.0;
    const ICON_GAP: f32 = 5.0;
    const ROW: f32 = LINE_HEIGHT + 1.0;

    let mut icon: Option<String> = None;
    let mut lines: Vec<(String, u32)> = Vec::new();
    let mut progress = None;
    let title = match looking {
        Looking::Block {
            id,
            pos,
            properties,
            progress: mining,
            harvestable,
        } => {
            let shown = block_icon(id).into_owned();
            if o.icon && p.atlas.has_item(&shown) {
                icon = Some(shown);
            }
            if o.harvestable
                && let Some(ok) = harvestable
            {
                lines.push(if *ok {
                    ("\u{2714} Harvestable".to_owned(), 0x55FF55)
                } else {
                    ("\u{2718} Not harvestable".to_owned(), 0xFF5555)
                });
            }
            let details = match o.details {
                Details::Never => false,
                Details::Sneaking => sneaking,
                Details::Always => true,
            };
            if details {
                lines.extend(
                    properties
                        .iter()
                        .map(|(name, value)| (property_line(name, value), 0xAAAAAA)),
                );
            }
            if o.coordinates {
                lines.push((format!("{}, {}, {}", pos[0], pos[1], pos[2]), 0xAAAAAA));
            }
            if o.progress {
                progress = *mining;
            }
            block_name(id)
        }
        Looking::Entity { id, name, health } => {
            let egg = format!("{id}_spawn_egg");
            if o.icon && p.atlas.has_item(&egg) {
                icon = Some(egg);
            }
            if o.entity_health
                && let Some((health, max)) = health
            {
                lines.push((format!("\u{2764} {health:.1} / {max:.1}"), 0xFF5555));
            }
            name.clone()
                .unwrap_or_else(|| crate::gui::tooltip::translate(&format!("entity.minecraft.{id}"), &[]))
        }
    };
    let source = o.source.then_some("Minecraft");

    let text_w = std::iter::once(title.as_str())
        .chain(lines.iter().map(|(l, _)| l.as_str()))
        .chain(source)
        .map(|t| p.atlas.font.width_str(t))
        .fold(0.0f32, f32::max)
        .ceil();
    let rows = 1 + lines.len() + usize::from(source.is_some());
    let text_h = rows as f32 * ROW - 1.0;
    let icon_w = if icon.is_some() { ITEM_SIZE + ICON_GAP } else { 0.0 };
    let inner_h = text_h.max(if icon.is_some() { ITEM_SIZE } else { 0.0 });
    let (w, h) = (icon_w + text_w + 2.0 * PAD, inner_h + 2.0 * PAD);

    let scale = o.scale as f32 / 100.0;
    let x = (centre_x - w * scale / 2.0).floor();
    let device = p.scale;
    p.scale = device * scale;
    p.scaled(scale, x, top, |p| {
        let alpha = (o.opacity.min(100) * 255 / 100) << 24;
        let background = COLORS[o.background.min(COLORS.len() - 1)].1;
        p.fill(1.0, 0.0, w - 2.0, h, alpha | background);
        p.fill(0.0, 1.0, 1.0, h - 2.0, alpha | background);
        p.fill(w - 1.0, 1.0, 1.0, h - 2.0, alpha | background);
        let border = BORDERS[o.border.min(BORDERS.len() - 1)];
        if border.0 != "None" {
            let edge = 0xFF00_0000 | border.1;
            let dim = 0xFF00_0000 | ((border.1 >> 1) & 0x7F7F7F);
            p.fill(1.0, 1.0, w - 2.0, 1.0, edge);
            p.fill(1.0, h - 2.0, w - 2.0, 1.0, dim);
            p.fill(1.0, 2.0, 1.0, h - 4.0, edge);
            p.fill(w - 2.0, 2.0, 1.0, h - 4.0, edge);
        }
        if let Some(icon) = &icon {
            p.item_icon(icon, PAD, ((h - ITEM_SIZE) / 2.0).floor());
        }
        let tx = PAD + icon_w;
        let mut ty = PAD + ((inner_h - text_h) / 2.0).floor();
        p.text_str(&title, tx, ty, 0xFFFFFF, true);
        ty += ROW;
        for (line, color) in &lines {
            p.text_str(line, tx, ty, *color, true);
            ty += ROW;
        }
        if let Some(source) = source {
            let spans = [Span {
                text: source.to_owned(),
                style: Style {
                    color: SOURCES[o.source_color.min(SOURCES.len() - 1)].1,
                    italic: true,
                    ..Default::default()
                },
            }];
            p.text(&spans, tx, ty, true);
        }
        if let Some(progress) = progress.filter(|v| *v > 0.0) {
            p.fill(2.0, h - 3.0, (w - 4.0) * progress, 1.0, 0xFFFF_FFFF);
        }
    });
    p.scale = device;
    h * scale
}

const SCALE_SLIDER_ID: u32 = 1100;
const OPACITY_SLIDER_ID: u32 = 1101;

fn on_off(on: bool) -> &'static str {
    if on { "ON" } else { "OFF" }
}

/// The settings screen.
pub fn draw_settings(p: &mut Painter, state: &mut GuiState, ctx: &ScreenCtx) {
    use crate::gui::options::{COLUMN_STEP, COLUMN_W, GRID_LEFT_OFFSET, ROW_H, ROW_INSET};
    crate::gui::options::draw_title(p, ctx, "Block Info");
    let before = state.options.block_info.clone();
    let o = &mut state.options.block_info;

    // A preview of the panel, so the settings show as they change. It is
    // drawn at the origin first, then moved below the buttons once its
    // height is known, so buttons and preview sit together above Done.
    let sample = Looking::Block {
        id: "oak_log",
        pos: [0, 64, 0],
        properties: vec![("axis", "y")],
        progress: Some(0.4),
        harvestable: Some(true),
    };
    let first_vertex = p.positions.len();
    let preview_h = draw(p, (ctx.vw / 2.0).floor(), 0.0, &sample, true, o);

    const PREVIEW_GAP: f32 = 4.0;
    let rows = 7.0;
    let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
    let top = crate::gui::options::LAYOUT.content_y(ctx.vh, rows * ROW_H + PREVIEW_GAP + preview_h);
    let preview_y = (top + rows * ROW_H + PREVIEW_GAP).floor();
    for v in &mut p.positions[first_vertex..] {
        v[1] += preview_y;
    }
    let cell = |column: f32, row: f32| (left + column * COLUMN_STEP, top + row * ROW_H + ROW_INSET);
    let button = |p: &mut Painter, (x, y): (f32, f32), caption: String| {
        Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &caption).draw(p, ctx)
    };

    if button(p, cell(0.0, 0.0), format!("Block Info: {}", on_off(o.enabled))) {
        o.enabled = !o.enabled;
    }
    if button(p, cell(1.0, 0.0), format!("Show Tags: {}", o.details.caption())) {
        o.details = o.details.next();
    }

    let (x, y) = cell(0.0, 1.0);
    o.scale = Slider {
        id: SCALE_SLIDER_ID,
        x,
        y,
        w: COLUMN_W,
        h: WIDGET_HEIGHT,
        min: 50,
        max: 300,
        value: o.scale as i32,
        label: |v| format!("Scale: {v}%"),
    }
    .draw(p, ctx)
    .clamp(50, 300) as u32;
    let (x, y) = cell(1.0, 1.0);
    o.opacity = Slider {
        id: OPACITY_SLIDER_ID,
        x,
        y,
        w: COLUMN_W,
        h: WIDGET_HEIGHT,
        min: 0,
        max: 100,
        value: o.opacity as i32,
        label: |v| format!("Background Opacity: {v}%"),
    }
    .draw(p, ctx)
    .clamp(0, 100) as u32;

    if button(p, cell(0.0, 2.0), format!("Background: {}", COLORS[o.background].0)) {
        o.background = (o.background + 1) % COLORS.len();
    }
    if button(p, cell(1.0, 2.0), format!("Border: {}", BORDERS[o.border].0)) {
        o.border = (o.border + 1) % BORDERS.len();
    }
    if button(p, cell(0.0, 3.0), format!("Source Colour: {}", SOURCES[o.source_color].0)) {
        o.source_color = (o.source_color + 1) % SOURCES.len();
    }
    if button(p, cell(1.0, 3.0), format!("Show Source: {}", on_off(o.source))) {
        o.source = !o.source;
    }
    if button(p, cell(0.0, 4.0), format!("Show Icon: {}", on_off(o.icon))) {
        o.icon = !o.icon;
    }
    if button(p, cell(1.0, 4.0), format!("Show Harvestable: {}", on_off(o.harvestable))) {
        o.harvestable = !o.harvestable;
    }
    if button(p, cell(0.0, 5.0), format!("Show Progress: {}", on_off(o.progress))) {
        o.progress = !o.progress;
    }
    if button(p, cell(1.0, 5.0), format!("Show Coordinates: {}", on_off(o.coordinates))) {
        o.coordinates = !o.coordinates;
    }
    if button(p, cell(0.0, 6.0), format!("Entity Health: {}", on_off(o.entity_health))) {
        o.entity_health = !o.entity_health;
    }
    if button(p, cell(1.0, 6.0), "Reset to Defaults".to_owned()) {
        *o = BlockInfoOptions::default();
    }

    if state.options.block_info != before {
        crate::gui::save_options(&state.options);
    }
    if crate::gui::options::draw_done(p, ctx) {
        state.nav = Some(Screen::Options);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_banner_is_named_after_its_banner() {
        assert_eq!(block_name("red_wall_banner"), "Red Banner");
        assert_eq!(block_name("oak_wall_sign"), "Oak Wall Sign");
        assert_eq!(block_name("stone"), "Stone");
    }

    #[test]
    fn settings_round_trip_through_the_options_file() {
        let o = BlockInfoOptions {
            scale: 150,
            opacity: 40,
            border: 3,
            details: Details::Always,
            coordinates: true,
            ..BlockInfoOptions::default()
        };
        assert_eq!(BlockInfoOptions::load(Some(&o.save())), o);
        assert_eq!(BlockInfoOptions::load(None), BlockInfoOptions::default());
    }

    #[test]
    fn icons_map_blocks_whose_item_differs() {
        assert_eq!(block_icon("wall_torch"), "torch");
        assert_eq!(block_icon("oak_wall_sign"), "oak_sign");
        assert_eq!(block_icon("cobblestone_wall"), "cobblestone_wall");
        assert_eq!(block_icon("potted_poppy"), "poppy");
        assert_eq!(block_icon("carrots"), "carrot");
    }
}
