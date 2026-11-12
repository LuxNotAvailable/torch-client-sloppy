//! `ClientBundleTooltip`: a bundle's contents grid and fill bar, drawn
//! under the tooltip's title.

use crate::gui::painter::Painter;
use crate::gui::tooltip::tr;
use crate::session::BundleView;
use crate::text::{Span, Style};

const SLOT: f32 = 24.0;
pub(crate) const WIDTH: f32 = 96.0;
const BAR_H: f32 = 13.0;
const BAR_FILL_MAX: f64 = 94.0;
const MAX_SLOTS: usize = 12;
const DESCRIPTION_COLOR: u32 = 0xAAAAAA;

fn description(p: &Painter) -> Vec<Vec<Span>> {
    let text = tr("item.minecraft.bundle.empty.description");
    let span = Span {
        text: text.into(),
        style: Style::colored(DESCRIPTION_COLOR),
    };
    p.atlas.font.wrap(&[span], WIDTH)
}

fn rows(b: &BundleView) -> usize {
    b.items.len().min(MAX_SLOTS).div_ceil(4)
}

/// `getNumberOfItemsToShow`: fills whole rows, one slot kept for "+N".
fn shown(b: &BundleView) -> usize {
    let n = b.items.len();
    let available = if n > MAX_SLOTS { 11 } else { 12 };
    let partial = n % 4;
    let gap = if partial == 0 { 0 } else { 4 - partial };
    n.min(available - gap)
}

pub(crate) fn height(p: &Painter, b: &BundleView) -> f32 {
    let body = if b.items.is_empty() {
        description(p).len() as f32 * 9.0
    } else {
        rows(b) as f32 * SLOT
    };
    body + BAR_H + 8.0
}

/// Draws at the tooltip's content box (`x`, `y`, width `w`).
pub(crate) fn draw(p: &mut Painter, b: &BundleView, x: f32, y: f32, w: f32) {
    let left = x + ((w - WIDTH) / 2.0).floor();
    if b.items.is_empty() {
        let lines = description(p);
        for (i, line) in lines.iter().enumerate() {
            p.text(line, left, y + i as f32 * 9.0, true);
        }
        bar(p, b, left, y + lines.len() as f32 * 9.0 + 4.0);
        return;
    }

    let rows = rows(b);
    let shown = shown(b);
    let overflowing = b.items.len() > MAX_SLOTS;
    let (x_start, y_start) = (left + WIDTH, y + rows as f32 * SLOT);
    let mut slot = 1;
    for row in 1..=rows {
        for col in 1..=4 {
            let dx = x_start - col as f32 * SLOT;
            let dy = y_start - row as f32 * SLOT;
            if overflowing && col * row == 1 {
                let hidden: u32 = b.items[shown..].iter().map(|s| s.count as u32).sum();
                let text = format!("+{hidden}");
                let tw = (p.atlas.font.width_str(&text) / 2.0).floor();
                p.text_str(&text, dx + 12.0 - tw, dy + 10.0, 0xFFFFFF, true);
            } else if shown >= slot {
                // Slot 1 sits bottom right and holds the oldest shown item.
                let item = &b.items[shown - slot];
                p.sprite("container/bundle/slot_background", dx, dy, SLOT, SLOT);
                crate::gui::slots::draw_stack(p, item, dx + 4.0, dy + 4.0);
                slot += 1;
            }
        }
    }
    bar(p, b, left, y + rows as f32 * SLOT + 4.0);
}

fn bar(p: &mut Painter, b: &BundleView, x: f32, y: f32) {
    let fill = b.scaled(BAR_FILL_MAX).clamp(0, BAR_FILL_MAX as i32) as f32;
    let sprite = if b.is_full() {
        "container/bundle/bundle_progressbar_full"
    } else {
        "container/bundle/bundle_progressbar_fill"
    };
    if fill > 0.0 {
        p.sprite(sprite, x + 1.0, y, fill, BAR_H);
    }
    p.sprite(
        "container/bundle/bundle_progressbar_border",
        x,
        y,
        WIDTH,
        BAR_H,
    );
    let label = if b.weight <= 0.0 {
        Some(tr("item.minecraft.bundle.empty"))
    } else if b.is_full() {
        Some(tr("item.minecraft.bundle.full"))
    } else {
        None
    };
    if let Some(label) = label {
        let tw = (p.atlas.font.width_str(label) / 2.0).floor();
        p.text_str(label, x + 48.0 - tw, y + 3.0, 0xFFFFFF, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SlotStack;

    fn bundle(n: usize) -> BundleView {
        BundleView {
            items: vec![SlotStack::default(); n].into(),
            weight: 0.5,
        }
    }

    #[test]
    fn shown_items_fill_whole_rows() {
        assert_eq!(shown(&bundle(3)), 3);
        assert_eq!(shown(&bundle(5)), 5);
        assert_eq!(shown(&bundle(12)), 12);
        // Over 12: 11 slots, "+N" in the twelfth.
        assert_eq!(shown(&bundle(13)), 8);
        assert_eq!(shown(&bundle(16)), 11);
        assert_eq!(rows(&bundle(13)), 3);
    }
}
