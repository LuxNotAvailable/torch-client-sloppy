//! `GameModeSwitcherScreen`: F3+F4 brings up the four game modes, F4 steps
//! through them, and letting go of F3 switches to the one picked with
//! `/gamemode`.

use crate::gui::painter::Painter;
use crate::gui::ScreenCtx;
use crate::session::Gamemode;

const SLOT: f32 = 26.0;
const STRIDE: f32 = 31.0;

/// The modes in the switcher's order, with their names, commands and icons.
const MODES: [(Gamemode, &str, &str, &str); 4] = [
    (Gamemode::Creative, "Creative Mode", "creative", "grass_block"),
    (Gamemode::Survival, "Survival Mode", "survival", "iron_sword"),
    (Gamemode::Adventure, "Adventure Mode", "adventure", "map"),
    (Gamemode::Spectator, "Spectator Mode", "spectator", "ender_eye"),
];

#[derive(Default)]
pub struct SwitcherState {
    /// The picked mode, as an index into `MODES`.
    pub hovered: usize,
    /// The mode before the current one, which the switcher opens on.
    pub previous: Option<Gamemode>,
    last_seen: Option<Gamemode>,
    first_mouse: Option<(f32, f32)>,
}

fn index_of(mode: Gamemode) -> usize {
    MODES.iter().position(|m| m.0 == mode).unwrap_or(0)
}

impl SwitcherState {
    /// Tracks the mode, to know the previous one.
    pub fn note_mode(&mut self, mode: Gamemode) {
        if self.last_seen != Some(mode) {
            if let Some(old) = self.last_seen {
                self.previous = Some(old);
            }
            self.last_seen = Some(mode);
        }
    }

    /// Opens on the previous mode, or else creative (survival from
    /// creative).
    pub fn open(&mut self, current: Gamemode) {
        let default = self.previous.unwrap_or(if current == Gamemode::Creative {
            Gamemode::Survival
        } else {
            Gamemode::Creative
        });
        self.hovered = index_of(default);
        self.first_mouse = None;
    }

    /// F4: the next mode.
    pub fn next(&mut self) {
        self.hovered = (self.hovered + 1) % MODES.len();
    }

    /// The command for the picked mode, if it changes anything.
    pub fn command(&self, current: Gamemode) -> Option<String> {
        let (mode, _, name, _) = MODES[self.hovered];
        (mode != current).then(|| format!("gamemode {name}"))
    }
}

pub fn draw(p: &mut Painter, state: &mut SwitcherState, ctx: &ScreenCtx, current: Gamemode) {
    let (cx, cy) = ((ctx.vw / 2.0).floor(), (ctx.vh / 2.0).floor());
    p.sprite_part("gamemode_switcher_bg", 0.0, 0.0, 125.0, 75.0, cx - 62.0, cy - 31.0 - 27.0);
    let total = MODES.len() as f32 * STRIDE - 5.0;
    let x0 = cx - (total / 2.0).floor();
    let y = cy - 31.0;
    // The mouse picks a slot once it moves.
    if let Some(m) = ctx.mouse() {
        let first = *state.first_mouse.get_or_insert((m.x, m.y));
        if first != (m.x, m.y) {
            for i in 0..MODES.len() {
                let x = x0 + i as f32 * STRIDE;
                if m.x >= x && m.x < x + SLOT && m.y >= y && m.y < y + SLOT {
                    state.hovered = i;
                }
            }
        }
    }
    for (i, (mode, _, _, icon)) in MODES.iter().enumerate() {
        let x = x0 + i as f32 * STRIDE;
        p.sprite("gamemode_switcher/slot", x, y, SLOT, SLOT);
        p.item_icon(icon, x + 5.0, y + 5.0);
        if i == state.hovered {
            p.sprite("gamemode_switcher/selection", x, y, SLOT, SLOT);
        }
        let _ = (mode, current);
    }
    let name = MODES[state.hovered].1;
    let w = p.atlas.font.width_str(name);
    p.text_str(name, cx - (w / 2.0).floor(), cy - 31.0 - 20.0, 0xFFFFFF, true);
    let hint = "[ F4 ] Next";
    let w = p.atlas.font.width_str(hint);
    p.text_str(hint, cx - (w / 2.0).floor(), cy + 5.0, 0xAAAAAA, true);
}
