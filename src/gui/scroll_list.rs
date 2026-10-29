//! A scrolling list: wheel, scrollbar drag and touch drag, the clip, the
//! culling of rows outside the view, and the scrollbar, which every list
//! screen otherwise repeats around its own rows.

use super::options::{draw_scrollbar, scroll_input};
use super::ScreenCtx;
use super::painter::{ClipGuard, Painter};

/// Where a list sits: the clip rectangle, and the rows' column inside it
/// (the scrollbar hangs off the column's right edge).
#[derive(Clone, Copy, Debug)]
pub struct ListArea {
    pub clip: (f32, f32, f32, f32),
    pub left: f32,
    pub width: f32,
    pub y: f32,
    pub h: f32,
}

impl ListArea {
    /// A column at most `max_w` wide, centred on the screen, in `rect`
    /// (`LAYOUT.content_rect`) and leaving room for the scrollbar.
    pub fn centered(ctx: &ScreenCtx, rect: (f32, f32, f32, f32), max_w: f32) -> Self {
        let (x, y, w, h) = rect;
        let width = max_w.min(w - 20.0);
        ListArea {
            clip: (x, y, w, h),
            left: ((ctx.vw - width) / 2.0).floor(),
            width,
            y,
            h,
        }
    }

    /// Whether a row at `y`, `row_h` tall, shows at all.
    pub fn shows(&self, y: f32, row_h: f32) -> bool {
        y + row_h >= self.y && y <= self.y + self.h
    }

    /// Whether the pointer is over the row at `y`, inside the view.
    pub fn hovered(&self, ctx: &ScreenCtx, y: f32, row_h: f32) -> bool {
        ctx.mouse().is_some_and(|m| {
            m.x >= self.left
                && m.x < self.left + self.width
                && m.y >= y.max(self.y)
                && m.y < (y + row_h).min(self.y + self.h)
        })
    }
}

/// One list's scroll position and scrollbar drag.
#[derive(Clone, Debug, Default)]
pub struct ScrollList {
    pub scroll: f32,
    pub drag: Option<f32>,
}

/// An open list frame: where its content starts, and the clip to pop.
pub struct ListFrame {
    pub top: f32,
    clip: ClipGuard,
}

impl ScrollList {
    /// Back to the top, dropping any drag.
    pub fn reset(&mut self) {
        *self = ScrollList::default();
    }

    /// Takes this frame's wheel, scrollbar and touch input.
    pub fn input(&mut self, ctx: &ScreenCtx, area: &ListArea, content_h: f32) {
        scroll_input(
            &mut self.scroll,
            &mut self.drag,
            ctx,
            area.left,
            area.width,
            area.y,
            area.h,
            content_h,
        );
    }

    /// Draws the scrollbar, when the content overflows.
    pub fn draw_bar(&self, p: &mut Painter, area: &ListArea, content_h: f32) {
        draw_scrollbar(p, self.scroll, area.left, area.width, area.y, area.h, content_h);
    }

    /// Where the content's top is drawn this frame.
    pub fn top(&self, area: &ListArea) -> f32 {
        area.y - self.scroll
    }

    /// Takes input and opens the clip; draw the content from `top`, then
    /// call [`ScrollList::end`].
    pub fn begin(
        &mut self,
        p: &mut Painter,
        ctx: &ScreenCtx,
        area: &ListArea,
        content_h: f32,
    ) -> ListFrame {
        self.input(ctx, area, content_h);
        let (x, y, w, h) = area.clip;
        ListFrame {
            top: self.top(area),
            clip: p.push_clip(x, y, w, h),
        }
    }

    /// Closes the clip and draws the scrollbar.
    pub fn end(&self, p: &mut Painter, frame: ListFrame, area: &ListArea, content_h: f32) {
        p.pop_clip(frame.clip);
        self.draw_bar(p, area, content_h);
    }

    /// `count` rows of `row_h`: `draw(p, index, y)` runs for each one in view.
    pub fn rows(
        &mut self,
        p: &mut Painter,
        ctx: &ScreenCtx,
        area: &ListArea,
        count: usize,
        row_h: f32,
        mut draw: impl FnMut(&mut Painter, usize, f32),
    ) {
        let content_h = count as f32 * row_h;
        let frame = self.begin(p, ctx, area, content_h);
        for i in 0..count {
            let y = frame.top + i as f32 * row_h;
            if area.shows(y, row_h) {
                draw(p, i, y);
            }
        }
        self.end(p, frame, area, content_h);
    }
}
