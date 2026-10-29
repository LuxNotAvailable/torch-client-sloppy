//! `WinScreen`: the end poem and the credits, rolled after the player leaves
//! the End (game event 4 with a value of 1). The poem scrolls at half a
//! pixel a frame (vanilla's per-partial-tick rate at 60 fps is near it),
//! held keys speed it up, and finishing or Escape respawns the player.

use crate::gui::painter::Painter;
use crate::gui::{Screen, ScreenCtx};
use crate::text::{Span, Style};

const LINE: f32 = 12.0;
const WIDTH: f32 = 256.0;

/// The rolled lines; `true` marks a centred line.
#[derive(Default)]
pub struct WinState {
    lines: Vec<(Vec<Span>, bool)>,
    scroll: f32,
    last: Option<std::time::Instant>,
    /// Asked to roll; the lines are read on the first frame, with the font.
    pub pending: bool,
}

impl WinState {
    pub fn open(&mut self, font: &crate::text::Font, player: &str) {
        self.lines.clear();
        self.scroll = 0.0;
        self.last = None;
        let root = crate::assets_root().join("texts");
        let read = |name: &str| crate::platform::assets::read_to_string(root.join(name));
        if let Some(poem) = read("end.txt") {
            self.add_poem(font, &poem, player);
        }
        if let Some(credits) = read("credits.json") {
            self.add_credits(&credits);
        }
        if let Some(post) = read("postcredits.txt") {
            self.add_poem(font, &post, player);
        }
    }

    fn empty(&mut self) {
        self.lines.push((Vec::new(), false));
    }

    /// `addPoemFile`: the player's name in, the obfuscated token as 3 to 6
    /// scrambled letters, each line wrapped to 256 and followed by a blank.
    fn add_poem(&mut self, font: &crate::text::Font, text: &str, player: &str) {
        const TOKEN: &str = "\u{a7}f\u{a7}k\u{a7}a\u{a7}b";
        let mut random = crate::util::javarandom::JavaRandom::new(8124371);
        for raw in text.lines() {
            let mut line = raw.replace("PLAYERNAME", player);
            while let Some(at) = line.find(TOKEN) {
                let n = 3 + random.next_int(4) as usize;
                line = format!("{}\u{a7}f\u{a7}k{}{}", &line[..at], &"XXXXXXXX"[..n], &line[at + TOKEN.len()..]);
            }
            let spans = crate::text::parse_formatted(&line);
            for wrapped in font.wrap(&spans, WIDTH) {
                self.lines.push((wrapped, false));
            }
            self.empty();
        }
        for _ in 0..8 {
            self.empty();
        }
    }

    /// `addCreditsFile`: sections, disciplines, titles and names.
    fn add_credits(&mut self, json: &str) {
        let Ok(serde_json::Value::Array(sections)) = serde_json::from_str::<serde_json::Value>(json) else {
            return;
        };
        let plain = |text: &str, color: u32| vec![Span { text: text.to_owned(), style: Style::colored(color) }];
        let heading = "============";
        for section in &sections {
            let name = section["section"].as_str().unwrap_or("");
            self.lines.push((plain(heading, 0xFFFFFF), true));
            self.lines.push((plain(name, 0xFFFF55), true));
            self.lines.push((plain(heading, 0xFFFFFF), true));
            self.empty();
            self.empty();
            for discipline in section["disciplines"].as_array().into_iter().flatten() {
                let name = discipline["discipline"].as_str().unwrap_or("");
                if !name.is_empty() {
                    self.lines.push((plain(name, 0xFFFF55), true));
                    self.empty();
                    self.empty();
                }
                for title in discipline["titles"].as_array().into_iter().flatten() {
                    self.lines.push((plain(title["title"].as_str().unwrap_or(""), 0xAAAAAA), false));
                    for name in title["names"].as_array().into_iter().flatten() {
                        let name = format!("           {}", name.as_str().unwrap_or(""));
                        self.lines.push((plain(&name, 0xFFFFFF), false));
                    }
                    self.empty();
                    self.empty();
                }
            }
        }
    }
}

/// Draws the roll; `Some` when it is over (rolled out, or Escape).
pub fn draw(p: &mut Painter, state: &mut WinState, ctx: &ScreenCtx) -> Option<Screen> {
    if std::mem::take(&mut state.pending) {
        let player = crate::client::bot::username();
        state.open(&p.atlas.font, &player);
    }
    let now = std::time::Instant::now();
    let dt = state.last.map_or(0.0, |t| (now - t).as_secs_f32());
    state.last = Some(now);
    // 0.5 a partial tick: 10 pixels a second.
    state.scroll += dt * 0.5 * 20.0;

    p.fill(0.0, 0.0, ctx.vw, ctx.vh, 0xFF00_0000);
    let logo_x = (ctx.vw / 2.0 - 128.0).floor();
    let logo_y = ctx.vh + 50.0 - state.scroll;
    let logo_h = p
        .atlas
        .sprite(crate::gui::atlas::TORCH_LOGO)
        .map_or(44.0, |s| (256.0 * s.region.h as f32 / s.region.w.max(1) as f32).round());
    p.sprite(crate::gui::atlas::TORCH_LOGO, logo_x, logo_y, 256.0, logo_h);
    let mut y = ctx.vh + 150.0 - state.scroll;
    let count = state.lines.len();
    for (i, (line, centred)) in state.lines.iter().enumerate() {
        // The last line holds at the middle of the screen.
        if i + 1 == count && y < ctx.vh / 2.0 - 6.0 {
            y = ctx.vh / 2.0 - 6.0;
        }
        if y + LINE + 8.0 > 0.0 && y < ctx.vh && !line.is_empty() {
            let x = if *centred {
                logo_x + 128.0 - (p.atlas.font.width(line) / 2.0).floor()
            } else {
                logo_x
            };
            p.text(line, x, y, true);
        }
        y += LINE;
    }
    let rolled_out = ctx.vh + 150.0 + count as f32 * LINE - state.scroll < ctx.vh / 2.0 - 6.0 - 200.0;
    (rolled_out || ctx.input.escape).then_some(Screen::None)
}
