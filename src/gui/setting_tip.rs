//! Settings tooltips: what a setting does, then its cost to frame rate or
//! its anticheat risk, coloured by level.

use crate::gui::painter::Painter;
use crate::text::{Span, Style};

/// How much a setting costs at its heavier values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Impact {
    /// Depends on the value picked (a preset, a frame cap).
    Varies,
    None,
    Low,
    Medium,
    High,
}

impl Impact {
    fn caption(self) -> (&'static str, u32) {
        match self {
            Impact::Varies => ("Varies", 0x55FFFF),
            Impact::None => ("None", 0xAAAAAA),
            Impact::Low => ("Low", 0x55FF55),
            Impact::Medium => ("Medium", 0xFFFF55),
            Impact::High => ("High", 0xFF5555),
        }
    }
}

/// How likely a feature is to get a player flagged by an anticheat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    /// Sends nothing a vanilla client wouldn't.
    Safe,
    /// Sends what a player could, but faster or in patterns a check may flag.
    Risky,
    /// Sends what no fair player could; anticheats look for it.
    #[cfg_attr(
        not(feature = "click_gui"),
        allow(dead_code, reason = "only click_gui modules are dangerous")
    )]
    Dangerous,
}

impl Risk {
    pub fn caption(self) -> (&'static str, u32) {
        match self {
            Risk::Safe => ("Safe", 0x55FF55),
            Risk::Risky => ("Risky", 0xFFFF55),
            Risk::Dangerous => ("Dangerous", 0xFF5555),
        }
    }
}

/// What a tip's last line rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rating {
    Impact(Impact),
    Risk(Risk),
}

/// A setting's tooltip.
#[derive(Debug, PartialEq, Eq)]
pub struct Tip {
    pub text: &'static str,
    pub rating: Rating,
}

impl Tip {
    pub const fn new(text: &'static str, impact: Impact) -> Self {
        Self {
            text,
            rating: Rating::Impact(impact),
        }
    }

    pub const fn risk(text: &'static str, risk: Risk) -> Self {
        Self {
            text,
            rating: Rating::Risk(risk),
        }
    }
}

const WRAP: f32 = 200.0;
const TEXT: u32 = 0xE0E0E0;
const NOTE: u32 = 0xFFAA00;

fn span(text: impl Into<String>, rgb: u32) -> Span {
    Span {
        text: text.into(),
        style: Style::colored(rgb),
    }
}

/// The tooltip's lines: the description, any `note` (such as a pending
/// restart) and the rating.
fn lines(p: &Painter, tip: &Tip, note: Option<&str>) -> Vec<Vec<Span>> {
    let mut out = Vec::new();
    for para in tip.text.split('\n') {
        out.extend(p.atlas.font.wrap(&[span(para, TEXT)], WRAP));
    }
    if let Some(note) = note {
        out.extend(p.atlas.font.wrap(&[span(note, NOTE)], WRAP));
    }
    let (label, (level, rgb)) = match tip.rating {
        Rating::Impact(i) => ("Performance Impact: ", i.caption()),
        Rating::Risk(r) => ("Ban Risk: ", r.caption()),
    };
    out.push(vec![span(label, 0xAAAAAA), span(level, rgb)]);
    out
}

pub fn draw(p: &mut Painter, tip: &Tip, note: Option<&str>, mx: f32, my: f32, vw: f32, vh: f32) {
    let lines = lines(p, tip, note);
    crate::gui::tooltip::draw_lines(p, &lines, mx, my, vw, vh);
}
