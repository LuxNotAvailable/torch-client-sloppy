//! `StatsScreen`: the player's statistics, asked of the server when opened
//! (`ServerboundClientCommand` `REQUEST_STATS`), in three tabs: General
//! (custom stats, formatted as vanilla's `StatFormatter`s), Items (per item:
//! broken, crafted, used, mined, picked up, dropped) and Mobs (killed and
//! killed by).

use azalea_protocol::packets::game::c_award_stats::Stat;

use crate::gui::painter::Painter;
use crate::gui::widgets::{self, Button, HeaderFooter, WIDGET_HEIGHT, WIDGET_WIDTH_BIG};
use crate::gui::{Screen, ScreenCtx};

const LAYOUT: HeaderFooter = HeaderFooter::new(HeaderFooter::DEFAULT_H + 24.0, HeaderFooter::DEFAULT_H);
const ROW: f32 = 12.0;
const ITEM_ROW: f32 = 20.0;
const TABS: [&str; 3] = ["General", "Items", "Mobs"];

#[derive(Default)]
pub struct StatsState {
    pub tab: usize,
    pub scroll: f32,
    pub drag: Option<f32>,
    /// The statistics the server sent; `None` until they come.
    pub stats: Option<std::collections::HashMap<Stat, i32>>,
}

/// `StatFormatter.TIME`: ticks as seconds, minutes, hours, days or years.
pub fn format_time(ticks: i32) -> String {
    let seconds = ticks as f64 / 20.0;
    let minutes = seconds / 60.0;
    let hours = minutes / 60.0;
    let days = hours / 24.0;
    let years = days / 365.0;
    if years > 0.5 {
        format!("{years:.2} y")
    } else if days > 0.5 {
        format!("{days:.2} d")
    } else if hours > 0.5 {
        format!("{hours:.2} h")
    } else if minutes > 0.5 {
        format!("{minutes:.2} min")
    } else {
        format!("{seconds:.2} s")
    }
}

/// `StatFormatter.DISTANCE`: centimetres as cm, m or km.
pub fn format_distance(cm: i32) -> String {
    let meters = cm as f64 / 100.0;
    let km = meters / 1000.0;
    if km > 0.5 {
        format!("{km:.2} km")
    } else if meters > 0.5 {
        format!("{meters:.2} m")
    } else {
        format!("{cm} cm")
    }
}

/// `StatFormatter.DEFAULT`: thousands grouped.
pub fn format_count(value: i32) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if value < 0 { format!("-{out}") } else { out }
}

/// A custom stat's value as vanilla shows it.
pub fn format_custom(name: &str, value: i32) -> String {
    match name {
        "play_time" | "total_world_time" | "time_since_death" | "time_since_rest" | "sneak_time" => format_time(value),
        n if n.ends_with("_one_cm") => format_distance(value),
        n if n.starts_with("damage_") => format!("{:.1}", value as f64 * 0.1),
        _ => format_count(value),
    }
}

fn translate(key: &str) -> String {
    crate::gui::tooltip::translate(key, &[])
}

fn item_name(item: &str) -> String {
    let name = translate(&format!("item.minecraft.{item}"));
    if name.starts_with("item.minecraft.") {
        translate(&format!("block.minecraft.{item}"))
    } else {
        name
    }
}

fn strip(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

pub fn draw(p: &mut Painter, state: &mut StatsState, ctx: &ScreenCtx) -> Option<Screen> {
    crate::gui::menu::background(p, ctx.vw, ctx.vh);
    widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), "Statistics");
    if let Some(tab) = widgets::tab_bar(p, ctx, &TABS, state.tab)
        && tab != state.tab
    {
        state.tab = tab;
        state.scroll = 0.0;
    }
    let (list_x, list_y, list_w, list_h) = LAYOUT.content_rect(ctx.vw, ctx.vh);
    let Some(stats) = &state.stats else {
        let text = "Downloading statistics...";
        let w = p.atlas.font.width_str(text);
        p.text_str(text, ((ctx.vw - w) / 2.0).floor(), list_y + list_h / 2.0 - 4.0, 0xFFFFFF, true);
        return done(p, ctx);
    };

    // The rows of the tab: text to draw, row height, and an icon.
    let width = 280.0f32.min(list_w - 20.0);
    let left = ((ctx.vw - width) / 2.0).floor();
    let mut rows: Vec<(Option<&'static str>, String, String, Vec<String>)> = Vec::new();
    let row_h;
    match state.tab {
        0 => {
            row_h = ROW;
            let mut custom: Vec<(String, i32)> = stats
                .iter()
                .filter_map(|(stat, v)| match stat {
                    Stat::Custom(c) => Some((strip(c.to_str()).to_owned(), *v)),
                    _ => None,
                })
                .collect();
            custom.sort_by_key(|(name, _)| translate(&format!("stat.minecraft.{name}")));
            for (name, value) in custom {
                rows.push((None, translate(&format!("stat.minecraft.{name}")), format_custom(&name, value), Vec::new()));
            }
        }
        1 => {
            row_h = ITEM_ROW;
            // Per item: broken, crafted, used, mined, picked up, dropped.
            let mut per: std::collections::BTreeMap<String, [i32; 6]> = Default::default();
            for (stat, v) in stats {
                let (item, column) = match stat {
                    Stat::Broken(i) => (strip(i.to_str()), 0),
                    Stat::Crafted(i) => (strip(i.to_str()), 1),
                    Stat::Used(i) => (strip(i.to_str()), 2),
                    Stat::Mined(b) => (strip(b.to_str()), 3),
                    Stat::PickedUp(i) => (strip(i.to_str()), 4),
                    Stat::Dropped(i) => (strip(i.to_str()), 5),
                    _ => continue,
                };
                per.entry(item.to_owned()).or_default()[column] += v;
            }
            for (item, values) in per {
                let icon: &'static str = Box::leak(item.clone().into_boxed_str());
                rows.push((
                    Some(icon),
                    item_name(&item),
                    String::new(),
                    values.iter().map(|v| if *v == 0 { "-".to_owned() } else { format_count(*v) }).collect(),
                ));
            }
        }
        _ => {
            row_h = ROW * 3.0;
            let mut per: std::collections::BTreeMap<String, (i32, i32)> = Default::default();
            for (stat, v) in stats {
                match stat {
                    Stat::Killed(e) => per.entry(strip(e.to_str()).to_owned()).or_default().0 += v,
                    Stat::KilledBy(e) => per.entry(strip(e.to_str()).to_owned()).or_default().1 += v,
                    _ => {}
                }
            }
            for (mob, (killed, by)) in per {
                let name = translate(&format!("entity.minecraft.{mob}"));
                let first = if killed == 0 {
                    format!("You have never killed {name}")
                } else {
                    format!("You killed {killed} {name}")
                };
                let second = if by == 0 {
                    format!("{name} has never killed you")
                } else {
                    format!("{name} killed you {by} time(s)")
                };
                rows.push((None, name, String::new(), vec![first, second]));
            }
        }
    }

    let header_h = if state.tab == 1 { ROW + 4.0 } else { 0.0 };
    let content_h = header_h + rows.len() as f32 * row_h;
    crate::gui::options::scroll_input(&mut state.scroll, &mut state.drag, ctx, left, width, list_y, list_h, content_h);
    let clip = p.push_clip(list_x, list_y, list_w, list_h);
    let top = list_y - state.scroll;
    if rows.is_empty() {
        let text = "Nothing yet";
        let w = p.atlas.font.width_str(text);
        p.text_str(text, ((ctx.vw - w) / 2.0).floor(), list_y + 8.0, 0xA0A0A0, true);
    }
    // Items: column headers.
    const COLUMNS: [&str; 6] = ["Broken", "Crafted", "Used", "Mined", "Picked", "Dropped"];
    let column_x = |i: usize| left + 120.0 + i as f32 * 27.0;
    if state.tab == 1 {
        for (i, label) in COLUMNS.iter().enumerate() {
            let w = p.atlas.font.width_str(label) * 0.5;
            p.scaled(0.5, column_x(i) + 24.0 - w, top, |p| {
                p.text_str(label, 0.0, 0.0, 0xFFFF55, true);
            });
        }
    }
    for (i, (icon, name, value, cells)) in rows.iter().enumerate() {
        let y = top + header_h + i as f32 * row_h;
        if y + row_h < list_y || y > list_y + list_h {
            continue;
        }
        if i % 2 == 0 {
            p.fill(left - 2.0, y, width + 4.0, row_h, 0x1AFF_FFFF);
        }
        match state.tab {
            0 => {
                p.text_str(name, left + 2.0, y + 2.0, 0xFFFFFF, true);
                let w = p.atlas.font.width_str(value);
                p.text_str(value, left + width - 2.0 - w, y + 2.0, 0xFFFFFF, true);
            }
            1 => {
                if let Some(icon) = icon {
                    p.item_icon(icon, left + 1.0, y + 2.0);
                }
                let shown: String = name.chars().take(14).collect();
                p.text_str(&shown, left + 20.0, y + 6.0, 0xFFFFFF, true);
                for (c, cell) in cells.iter().enumerate() {
                    let w = p.atlas.font.width_str(cell);
                    p.text_str(cell, column_x(c) + 24.0 - w, y + 6.0, 0xFFFFFF, true);
                }
            }
            _ => {
                p.text_str(name, left + 2.0, y + 1.0, 0xFFFFFF, true);
                for (l, line) in cells.iter().enumerate() {
                    p.text_str(line, left + 10.0, y + 1.0 + (l + 1) as f32 * ROW, 0xA0A0A0, true);
                }
            }
        }
    }
    p.pop_clip(clip);
    crate::gui::options::draw_scrollbar(p, state.scroll, left, width, list_y, list_h, content_h);
    done(p, ctx)
}

fn done(p: &mut Painter, ctx: &ScreenCtx) -> Option<Screen> {
    let x = widgets::centered_x(ctx.vw);
    let pressed = Button::new(x, LAYOUT.footer_y(ctx.vh), WIDGET_WIDTH_BIG, WIDGET_HEIGHT, "Done").draw(p, ctx);
    (pressed || ctx.input.escape).then_some(Screen::Pause)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_format_as_vanilla() {
        assert_eq!(format_time(20 * 30), "30.00 s");
        assert_eq!(format_time(20 * 60 * 90), "1.50 h");
        assert_eq!(format_distance(40), "40 cm");
        assert_eq!(format_distance(12_345), "123.45 m");
        assert_eq!(format_count(1_234_567), "1,234,567");
        assert_eq!(format_custom("damage_dealt", 125), "12.5");
    }
}
