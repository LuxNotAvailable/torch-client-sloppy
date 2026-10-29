//! AppleSkin's HUD and tooltip additions: saturation drawn over the hunger
//! bar, what the held food would restore (hunger, saturation, and the health
//! natural regeneration would then give back) flashing on the bars, and food
//! values in item tooltips. Its exhaustion bar needs the server to send the
//! exhaustion, which vanilla servers never do, so it is left out.

use crate::gui::Snapshot;
use crate::gui::painter::Painter;

const ICON: f32 = 9.0;
const STRIDE: f32 = 8.0;
/// The saturation icon: the drumstick's outline in gold, made from vanilla's
/// food icon when the GUI atlas is built.
pub const SATURATION_SPRITE: &str = "appleskin/saturation";
const GOLD: [u8; 4] = [255, 200, 50, 255];

/// The opaque pixels of `icon` that touch a transparent one (or the edge),
/// in gold.
pub fn outline_of(icon: &image::RgbaImage) -> image::RgbaImage {
    let (w, h) = icon.dimensions();
    let solid = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < w as i64
            && y < h as i64
            && icon.get_pixel(x as u32, y as u32)[3] > 0
    };
    image::RgbaImage::from_fn(w, h, |x, y| {
        let (x, y) = (x as i64, y as i64);
        let edge = solid(x, y)
            && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| !solid(x + dx, y + dy));
        image::Rgba(if edge { GOLD } else { [0; 4] })
    })
}

crate::options! {
    /// Which of AppleSkin's additions are on.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct AppleSkinOptions {
        pub saturation_overlay: bool = true => key("appleskinSaturationOverlay"),
        pub food_preview: bool = true => key("appleskinFoodPreview"),
        pub health_preview: bool = true => key("appleskinHealthPreview"),
        pub tooltip: bool = true => key("appleskinTooltip"),
    }
}

impl Default for AppleSkinOptions {
    fn default() -> Self {
        Self::defaults()
    }
}

fn options() -> AppleSkinOptions {
    crate::gui::current_options().appleskin
}

/// A food's nutrition and the saturation it gives (`nutrition * modifier *
/// 2`), from vanilla's `Foods` as `Items` hands them out.
pub fn food_values(item: &str) -> Option<(i32, f32)> {
    let item = item.strip_prefix("minecraft:").unwrap_or(item);
    let (nutrition, modifier): (i32, f32) = match item {
        "apple" => (4, 0.3),
        "baked_potato" => (5, 0.6),
        "beef" => (3, 0.3),
        "beetroot" => (1, 0.6),
        "beetroot_soup" => (6, 0.6),
        "bread" => (5, 0.6),
        "carrot" => (3, 0.6),
        "chicken" => (2, 0.3),
        "chorus_fruit" => (4, 0.3),
        "cod" => (2, 0.1),
        "cooked_beef" => (8, 0.8),
        "cooked_chicken" => (6, 0.6),
        "cooked_cod" => (5, 0.6),
        "cooked_mutton" => (6, 0.8),
        "cooked_porkchop" => (8, 0.8),
        "cooked_rabbit" => (5, 0.6),
        "cooked_salmon" => (6, 0.8),
        "cookie" => (2, 0.1),
        "dried_kelp" => (1, 0.3),
        "enchanted_golden_apple" => (4, 1.2),
        "glow_berries" => (2, 0.1),
        "golden_apple" => (4, 1.2),
        "golden_carrot" => (6, 1.2),
        "honey_bottle" => (6, 0.1),
        "melon_slice" => (2, 0.3),
        "mushroom_stew" => (6, 0.6),
        "mutton" => (2, 0.3),
        "poisonous_potato" => (2, 0.3),
        "porkchop" => (3, 0.3),
        "potato" => (1, 0.3),
        "pufferfish" => (1, 0.1),
        "pumpkin_pie" => (8, 0.3),
        "rabbit" => (3, 0.3),
        "rabbit_stew" => (10, 0.6),
        "rotten_flesh" => (4, 0.1),
        "salmon" => (2, 0.1),
        "spider_eye" => (2, 0.8),
        "suspicious_stew" => (6, 0.6),
        "sweet_berries" => (2, 0.1),
        "tropical_fish" => (1, 0.1),
        _ => return None,
    };
    Some((nutrition, nutrition as f32 * modifier * 2.0))
}

/// The food in hand: the main hand's, or else the off hand's.
fn held_food(snap: &Snapshot) -> Option<(i32, f32)> {
    let main = snap
        .hotbar
        .get(snap.selected as usize)
        .and_then(|s| food_values(s.item));
    main.or_else(|| snap.hotbar.get(9).and_then(|s| food_values(s.item)))
}

/// Hunger and saturation after eating `food`: `FoodData.add`.
fn after_eating(snap: &Snapshot, (nutrition, saturation): (i32, f32)) -> (i32, f32) {
    let food = (snap.food as i32 + nutrition).min(20);
    let saturation = (snap.saturation + saturation).min(food as f32);
    (food, saturation)
}

/// `FoodHelper.getEstimatedHealthIncrement`: the health natural regeneration
/// gives back from this hunger and saturation before hunger drops below 18.
pub fn estimated_health(food: i32, saturation: f32, mut exhaustion: f32) -> f32 {
    let (mut food, mut saturation) = (food as f32, saturation);
    let mut health = 0.0;
    for _ in 0..1000 {
        while exhaustion >= 4.0 {
            exhaustion -= 4.0;
            if saturation > 0.0 {
                saturation = (saturation - 1.0).max(0.0);
            } else {
                food = (food - 1.0).max(0.0);
            }
        }
        if food >= 20.0 && saturation > 0.0 {
            // The fast regeneration: up to 6 saturation a heal, a health
            // point for each 6 spent.
            let spent = saturation.min(6.0);
            health += spent / 6.0;
            exhaustion += spent;
        } else if food >= 18.0 {
            health += 1.0;
            exhaustion += 6.0;
        } else {
            break;
        }
    }
    health
}

/// The preview's flash: in and out about once a second.
fn flash() -> f32 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| (d.as_millis() % 100_000) as f32 / 1000.0);
    0.5 + 0.35 * (t * std::f32::consts::TAU * 0.8).sin()
}

fn white(alpha: f32) -> u32 {
    ((alpha.clamp(0.0, 1.0) * 255.0) as u32) << 24 | 0x00FF_FFFF
}

/// Saturation as gold outlines over the hunger bar, a quarter icon at a time
/// (each icon stands for 2 saturation), filling from the right as the bar
/// does.
fn saturation_icons(p: &mut Painter, saturation: f32, right: f32, top: f32, alpha: f32) {
    let quarters = (saturation * 2.0).round() as i32;
    for i in 0..10 {
        let filled = (quarters - i * 4).clamp(0, 4);
        if filled == 0 {
            break;
        }
        let x = right - i as f32 * STRIDE - ICON;
        let w = (ICON * filled as f32 / 4.0).round();
        let clip = p.push_clip(x + ICON - w, top, w, ICON);
        p.sprite_tinted(SATURATION_SPRITE, x, top, ICON, ICON, white(alpha));
        p.pop_clip(clip);
    }
}

/// The hunger bar's additions, drawn after vanilla's icons.
pub fn draw_food_overlay(p: &mut Painter, snap: &Snapshot, right: f32, top: f32) {
    let options = options();
    if options.saturation_overlay {
        saturation_icons(p, snap.saturation, right, top, 1.0);
    }
    let Some(food) = held_food(snap).filter(|_| options.food_preview) else {
        return;
    };
    let (after, after_saturation) = after_eating(snap, food);
    let alpha = flash();
    let now = snap.food as i32;
    for i in 0..10 {
        let halves = i * 2 + 1;
        // Only the icons the food would fill or finish.
        if halves < now || halves > after {
            continue;
        }
        let x = right - i as f32 * STRIDE - ICON;
        let sprite = if halves < after {
            "hud/food_full"
        } else {
            "hud/food_half"
        };
        p.sprite_tinted(sprite, x, top, ICON, ICON, white(alpha));
    }
    if options.saturation_overlay && after_saturation > snap.saturation {
        saturation_icons(p, after_saturation, right, top, alpha);
    }
}

/// The health the held food would give back, flashing on the hearts.
pub fn draw_health_preview(
    p: &mut Painter,
    snap: &Snapshot,
    rows: &crate::gui::health::Rows,
    left: f32,
    top: f32,
) {
    if !options().health_preview {
        return;
    }
    let Some(food) = held_food(snap) else { return };
    let (after, saturation) = after_eating(snap, food);
    let gain = estimated_health(after, saturation, 0.0)
        - estimated_health(snap.food as i32, snap.saturation, 0.0);
    if gain <= 0.0 {
        return;
    }
    let now = snap.health.max(0.0);
    let target = (now + gain).min(rows.max_health).ceil() as i32;
    let current = now.ceil() as i32;
    let alpha = white(flash());
    let containers = (rows.max_health / 2.0).ceil() as i32;
    for index in 0..containers {
        let halves = index * 2;
        if halves + 2 <= current || halves >= target {
            continue;
        }
        let x = left + (index % 10) as f32 * STRIDE;
        let y = top - (index / 10) as f32 * rows.row_height;
        let sprite = if halves + 1 < target {
            "hud/heart/full"
        } else {
            "hud/heart/half"
        };
        p.sprite_tinted(sprite, x, y, ICON, ICON, alpha);
    }
}

/// The tooltip's food values, when the tooltip row is on and the item is a
/// food.
pub fn tooltip_food(item: &str) -> Option<(i32, f32)> {
    options().tooltip.then(|| food_values(item)).flatten()
}

fn icon_counts((nutrition, saturation): (i32, f32)) -> (i32, i32) {
    ((nutrition + 1) / 2, (saturation / 2.0).ceil() as i32)
}

/// How wide the tooltip's food row is.
pub fn tooltip_row_width(food: (i32, f32)) -> f32 {
    let (hunger, sat) = icon_counts(food);
    (hunger + sat) as f32 * STRIDE + 4.0 + 1.0
}

/// Draws the food row, hunger then saturation, its left edge at `x` and top
/// at `y`.
pub fn draw_tooltip_row(p: &mut Painter, food: (i32, f32), x: f32, y: f32) {
    let (nutrition, saturation) = food;
    let (hunger, sat) = icon_counts(food);
    for i in 0..hunger {
        let at = x + i as f32 * STRIDE;
        p.sprite("hud/food_empty", at, y, ICON, ICON);
        let half = i * 2 + 1 == nutrition;
        p.sprite(
            if half {
                "hud/food_half"
            } else {
                "hud/food_full"
            },
            at,
            y,
            ICON,
            ICON,
        );
    }
    let start = x + hunger as f32 * STRIDE + 4.0;
    for i in 0..sat {
        let at = start + i as f32 * STRIDE;
        let part = (saturation - i as f32 * 2.0).clamp(0.0, 2.0) / 2.0;
        let w = (ICON * part).round().max(1.0);
        let clip = p.push_clip(at, y, w, ICON);
        p.sprite(SATURATION_SPRITE, at, y, ICON, ICON);
        p.pop_clip(clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foods_match_vanilla() {
        assert_eq!(food_values("cooked_beef").map(|f| f.0), Some(8));
        assert!((food_values("minecraft:golden_carrot").unwrap().1 - 14.4).abs() < 1e-4);
        assert_eq!(food_values("stone"), None);
    }

    #[test]
    fn full_hunger_and_saturation_heal_and_low_hunger_does_not() {
        assert_eq!(estimated_health(17, 5.0, 0.0), 0.0);
        assert!(estimated_health(20, 20.0, 0.0) > 3.0);
        assert!(estimated_health(20, 20.0, 0.0) > estimated_health(20, 0.0, 0.0));
    }
}
