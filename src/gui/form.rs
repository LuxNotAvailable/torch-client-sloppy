//! A settings screen as a list of controls: toggles, percent and range
//! sliders, cycle buttons and plain buttons, laid out two to a row the way
//! vanilla's option screens are, centred, with a title and Done. Each
//! control reports whether it changed something, and the form gathers that
//! so a screen saves once.
//!
//! ```ignore
//! let mut form = Form::new(p, ctx, "Accessibility Settings", 3, 70);
//! form.slider_percent("Distortion Effects", &mut a.distortion);
//! form.toggle("Hide Lightning Flashes", &mut a.hide_lightning);
//! if form.finish() { save_options(..) }
//! ```

use crate::gui::options::{COLUMN_STEP, COLUMN_W, GRID_LEFT_OFFSET, LAYOUT, ROW_H, ROW_INSET};
use crate::gui::painter::Painter;
use crate::gui::widgets::{self, Button, Slider, WIDGET_HEIGHT};
use crate::gui::ScreenCtx;

const ON: &str = "ON";
const OFF: &str = "OFF";

pub struct Form<'p, 'a, 'c> {
    p: &'p mut Painter<'a>,
    ctx: &'c ScreenCtx<'c>,
    left: f32,
    top: f32,
    /// The next cell, counting across then down.
    cell: usize,
    /// Slider ids: the form's base plus each slider's order.
    next_id: u32,
    changed: bool,
}

impl<'p, 'a, 'c> Form<'p, 'a, 'c> {
    /// A form of `rows` rows, centred; its sliders take ids from `id_base`.
    pub fn new(p: &'p mut Painter<'a>, ctx: &'c ScreenCtx<'c>, title: &str, rows: usize, id_base: u32) -> Self {
        widgets::draw_title(p, ctx.vw, LAYOUT.title_y(), title);
        let left = (ctx.vw / 2.0 - GRID_LEFT_OFFSET).floor();
        let top = LAYOUT.content_y(ctx.vh, rows as f32 * ROW_H);
        Form { p, ctx, left, top, cell: 0, next_id: id_base, changed: false }
    }

    fn at(&self, cell: usize) -> (f32, f32) {
        (
            self.left + (cell % 2) as f32 * COLUMN_STEP,
            self.top + (cell / 2) as f32 * ROW_H + ROW_INSET,
        )
    }

    fn take_cell(&mut self) -> (f32, f32) {
        let at = self.at(self.cell);
        self.cell += 1;
        at
    }

    /// The next control starts a new row.
    pub fn new_row(&mut self) {
        self.cell = self.cell.div_ceil(2) * 2;
    }

    /// The painter, for anything drawn beside the controls.
    pub fn painter(&mut self) -> &mut Painter<'a> {
        self.p
    }

    pub fn toggle(&mut self, label: &str, value: &mut bool) -> bool {
        let (x, y) = self.take_cell();
        let caption = format!("{label}: {}", if *value { ON } else { OFF });
        let pressed = Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &caption).draw(self.p, self.ctx);
        if pressed {
            *value = !*value;
            self.changed = true;
        }
        pressed
    }

    /// A slider over `min..=max`, captioned by `label`.
    pub fn slider(&mut self, value: &mut u32, min: u32, max: u32, label: fn(i32) -> String) -> bool {
        let (x, y) = self.take_cell();
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

    /// A button cycling through `options`, captioned `label: option`.
    pub fn cycle<T: Copy + PartialEq>(&mut self, label: &str, value: &mut T, options: &[(T, &str)]) -> bool {
        let (x, y) = self.take_cell();
        let at = options.iter().position(|(v, _)| v == value).unwrap_or(0);
        let caption = format!("{label}: {}", options[at].1);
        let pressed = Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, &caption).draw(self.p, self.ctx);
        if pressed {
            *value = options[(at + 1) % options.len()].0;
            self.changed = true;
        }
        pressed
    }

    /// A plain button in the next cell.
    pub fn button(&mut self, label: &str) -> bool {
        let (x, y) = self.take_cell();
        Button::new(x, y, COLUMN_W, WIDGET_HEIGHT, label).draw(self.p, self.ctx)
    }

    /// A button the width of both columns, on its own row.
    pub fn wide_button(&mut self, label: &str) -> bool {
        self.new_row();
        let (x, y) = self.take_cell();
        self.cell += 1;
        Button::new(x, y, COLUMN_STEP + COLUMN_W, WIDGET_HEIGHT, label).draw(self.p, self.ctx)
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
