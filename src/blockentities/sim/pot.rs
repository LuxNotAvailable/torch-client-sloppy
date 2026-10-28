use simdnbt::owned::NbtCompound;

pub const SIDES: usize = 4;

const SHERD_SUFFIX: &str = "_pottery_sherd";

pub const STEMS: [&str; 23] = [
    "angler",
    "archer",
    "arms_up",
    "blade",
    "brewer",
    "burn",
    "danger",
    "explorer",
    "flow",
    "friend",
    "guster",
    "heart",
    "heartbreak",
    "howl",
    "miner",
    "mourner",
    "plenty",
    "prize",
    "scrape",
    "sheaf",
    "shelter",
    "skull",
    "snort",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decorations {
    stems: [&'static str; SIDES],
}

impl Decorations {
    pub fn stem(&self, side: usize) -> &'static str {
        self.stems[side]
    }
}

fn stem_of(item: &str) -> &'static str {
    let name = item.strip_prefix("minecraft:").unwrap_or(item);
    name.strip_suffix(SHERD_SUFFIX)
        .and_then(|stem| STEMS.iter().find(|known| **known == stem))
        .copied()
        .unwrap_or("")
}

pub fn decode(tag: &NbtCompound) -> Decorations {
    let mut decorations = Decorations::default();
    if let Some(items) = tag.list("sherds").and_then(|list| list.strings()) {
        for (stem, item) in decorations.stems.iter_mut().zip(items) {
            *stem = stem_of(&item.to_str());
        }
    }
    decorations
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WobbleStyle {
    Positive,
    Negative,
}

impl WobbleStyle {
    const fn duration(self) -> u64 {
        match self {
            WobbleStyle::Positive => 7,
            WobbleStyle::Negative => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wobble {
    started_at: u64,
    style: Option<WobbleStyle>,
}

impl Wobble {
    pub fn event(&mut self, action: u8, param: u8, game_time: u64) {
        if action != 1 {
            return;
        }
        self.style = match param {
            0 => Some(WobbleStyle::Positive),
            1 => Some(WobbleStyle::Negative),
            _ => return,
        };
        self.started_at = game_time;
    }

    pub fn progress(&self, game_time: u64, partial: f32) -> Option<f32> {
        let style = self.style?;
        let elapsed = game_time.saturating_sub(self.started_at);
        Some((elapsed as f32 + partial) / style.duration() as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sherds_map_to_their_pattern_stem() {
        assert_eq!(stem_of("minecraft:angler_pottery_sherd"), "angler");
        assert_eq!(stem_of("minecraft:heartbreak_pottery_sherd"), "heartbreak");
        assert_eq!(stem_of("minecraft:brick"), "");
        assert_eq!(stem_of("minecraft:bogus_pottery_sherd"), "");
    }

    #[test]
    fn wobble_progress_uses_the_event_duration() {
        let mut wobble = Wobble::default();
        assert_eq!(wobble.progress(100, 0.0), None);
        wobble.event(1, 1, 100);
        assert_eq!(wobble.progress(105, 0.0), Some(0.5));
        wobble.event(1, 0, 200);
        assert_eq!(wobble.progress(203, 0.5), Some(0.5));
        wobble.event(1, 9, 300);
        assert_eq!(wobble.progress(203, 0.5), Some(0.5));
    }
}
