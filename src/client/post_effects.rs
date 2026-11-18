//! Server-requested post effects (`ClientboundPostEffects`): vanilla's
//! `creeper`, `spider`, `invert` and `blur` chains, as flags the renderers
//! read.

use std::sync::atomic::{AtomicU32, Ordering};

pub const CREEPER: u32 = 1;
pub const SPIDER: u32 = 2;
pub const INVERT: u32 = 4;
pub const BLUR: u32 = 8;

static ACTIVE: AtomicU32 = AtomicU32::new(0);

/// The active effects' flags.
pub fn active() -> u32 {
    ACTIVE.load(Ordering::Relaxed)
}

/// Replaces the active set; unknown ids are ignored.
pub fn set<'a>(ids: impl IntoIterator<Item = &'a str>) {
    let flags = ids.into_iter().fold(0, |flags, id| {
        flags
            | match id.rsplit(':').next().unwrap_or(id) {
                "creeper" => CREEPER,
                "spider" => SPIDER,
                "invert" => INVERT,
                "blur" => BLUR,
                _ => 0,
            }
    });
    ACTIVE.store(flags, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_effects_map_to_flags_and_others_drop_out() {
        set(["minecraft:creeper", "invert", "minecraft:entity_outline"]);
        assert_eq!(ACTIVE.load(Ordering::Relaxed), CREEPER | INVERT);
        set([]);
        assert_eq!(ACTIVE.load(Ordering::Relaxed), 0);
    }
}
