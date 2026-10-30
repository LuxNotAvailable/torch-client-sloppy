//! A settings screen as a list of controls: toggles, percent and range
//! sliders, cycle buttons and plain buttons, laid out two to a row the way
//! vanilla's option screens are, centred, with a title and Done. Each
//! control reports whether it changed something, and the form gathers that
//! so a screen saves once. Headers split a form into groups, and a form
//! can sit inside a scrolling list ([`Form::embedded`]), where it measures
//! its own height ([`Form::height`]).
//!
//! ```ignore
//! let mut form = Form::new(p, ctx, "Accessibility Settings", 3, 70);
//! form.slider_percent("Distortion Effects", &mut a.distortion);
//! form.toggle("Hide Lightning Flashes", &mut a.hide_lightning);
//! if form.finish() { save_options(..) }
//! ```

use crate::gui::ScreenCtx;
use crate::gui::options::{
    BIG_W, COLUMN_STEP, COLUMN_W, GRID_LEFT_OFFSET, HEADER_H, HEADER_PAD_TOP, LAYOUT, ROW_H,
    ROW_INSET,
};
use crate::gui::painter::Painter;
use crate::gui::widgets::{self, Button, Slider, WIDGET_HEIGHT};

const ON: &str = "ON";
const OFF: &str = "OFF";

pub struct Form<'p, 'a, 'c> {
    p: &'p mut Painter<'a>,
    ctx: &'c ScreenCtx<'c>,
    left: f32,
    top: f32,
    /// The current row's offset from `top`.
    y: f32,
    /// The next column in the current row; 2 when the row is full.
    column: usize,
    /// Slider ids: the form's base plus each slider's order.
    next_id: u32,
    changed: bool,
}

impl<'p, 'a, 'c> Form<'p, 'a, 'c> {
    /// A form of `rows` rows, centred; its sliders take ids from `id_base`.
    pub fn new(
        p: &'p mut Painter<'a>,
        ctx: &'c ScreenCtx<'c>,
        title: &str,
        rows: usize,
        id_base: u32,
    ) -> Self {
        widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), title);
        let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
        let top = LAYOUT.content_y(ctx.vh, rows as f32 * ROW_H);
        Self::embedded(p, ctx, left, top, id_base)
    }

    /// A form with no title whose first row is at `top`, its left column
    /// at `left`: the content of a scrolling list.
    pub fn embedded(
        p: &'p mut Painter<'a>,
        ctx: &'c ScreenCtx<'c>,
        left: f32,
        top: f32,
        id_base: u32,
    ) -> Self {
        Form {
            p,
            ctx,
            left,
            top,
            y: 0.0,
            column: 0,
            next_id: id_base,
            changed: false,
        }
    }

    /// The next cell's corner, taken: for a control the form doesn't have.
    pub fn cell(&mut self) -> (f32, f32) {
        if self.column == 2 {
            self.new_row();
        }
        let at = (
            self.left + self.column as f32 * COLUMN_STEP,
            self.top + self.y + ROW_INSET,
        );
        self.column += 1;
        at
    }

    /// Leaves the next cell empty.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "the browser build has no VSync cell")
    )]
    pub fn skip(&mut self) {
        self.cell();
    }

    /// The next control starts a new row.
    pub fn new_row(&mut self) {
        if self.column > 0 {
            self.y += ROW_H;
            self.column = 0;
        }
    }

    /// A group header on its own line, spaced from any rows above it.
    pub fn header(&mut self, text: &str) {
        self.new_row();
        let pad = if self.y > 0.0 { HEADER_PAD_TOP } else { 0.0 };
        self.p.text_str(
            text,
            self.left,
            self.top + self.y + ROW_INSET + pad,
            0xFFFFFF,
            true,
        );
        self.y += pad + HEADER_H;
    }

    /// A row the width of both columns, taken: for a control the form
    /// doesn't have.
    pub fn wide_cell(&mut self) -> (f32, f32) {
        self.new_row();
        let at = self.cell();
        self.column = 2;
        at
    }

    /// How tall the rows so far are.
    pub fn height(&self) -> f32 {
        self.y + if self.column > 0 { ROW_H } else { 0.0 }
    }

    /// The input the controls see.
    pub fn ctx(&self) -> &'c ScreenCtx<'c> {
        self.ctx
    }

    /// The painter, for anything drawn beside the controls.
    pub fn painter(&mut self) -> &mut Painter<'a> {
        self.p
    }

    pub fn toggle(&mut self, label: &str, value: &mut bool) -> bool {
        let (x, y) = self.cell();
        let caption = format!("{label}: {}", if *value { ON } else { OFF });
        let pressed = Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &caption).draw(self.p, self.ctx);
        if pressed {
            *value = !*value;
            self.changed = true;
        }
        pressed
    }

    /// A slider over `min..=max`, captioned by `label`.
    pub fn slider(
        &mut self,
        value: &mut u32,
        min: u32,
        max: u32,
        label: fn(i32) -> String,
    ) -> bool {
        let (x, y) = self.cell();
        let id = self.next_id;
        self.next_id += 1;
        let new = Slider {
            id,
            x,
            y,
            w: COLUMN_W,
            h: WIDGET_HEIGHT,
            min: min as i32,
            max: max as i32,
            value: *value as i32,
            label,
        }
        .draw(self.p, self.ctx)
        .clamp(min as i32, max as i32) as u32;
        let changed = new != *value;
        if changed {
            *value = new;
            self.changed = true;
        }
        changed
    }

    /// A plain button in the next cell.
    pub fn button(&mut self, label: &str) -> bool {
        self.button_active(label, true)
    }

    /// A button in the next cell, greyed out and dead unless `active`.
    pub fn button_active(&mut self, label: &str, active: bool) -> bool {
        let (x, y) = self.cell();
        Button {
            active,
            ..Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, label)
        }
        .draw(self.p, self.ctx)
    }

    /// Draws `button`, placed by the caller in a taken cell.
    pub fn draw(&mut self, button: Button) -> bool {
        button.draw(self.p, self.ctx)
    }

    /// A button the width of both columns, on its own row.
    pub fn wide_button(&mut self, label: &str) -> bool {
        let (x, y) = self.wide_cell();
        Button::new(x, y, BIG_W, WIDGET_HEIGHT, label).draw(self.p, self.ctx)
    }

    /// Notes a change made outside the controls (a reset button).
    pub fn mark_changed(&mut self) {
        self.changed = true;
    }

    /// Whether anything changed.
    pub fn finish(self) -> bool {
        self.changed
    }
}

/// A slider caption: `label: N%`.
#[macro_export]
macro_rules! percent_label {
    ($label:literal) => {
        |v: i32| format!(concat!($label, ": {}%"), v)
    };
}
