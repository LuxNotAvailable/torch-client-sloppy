//! Which light the sky gives the shafts: the biome and weather "profile"
//! around the player, picked as the first that fits from a fixed list
//! (caves before weather, weather before most biomes). Some profiles
//! stop light shafts; each picks the beams' tint.

const T_THE_END: u64 = 1 << 0;
const T_NETHER: u64 = 1 << 1;
const T_BASALT_DELTAS: u64 = 1 << 2;
const T_CRIMSON_FOREST: u64 = 1 << 3;
const T_NETHER_WASTES: u64 = 1 << 4;
const T_SOULSAND_VALLEY: u64 = 1 << 5;
const T_WARPED_FOREST: u64 = 1 << 6;
const T_DEEP_DARK: u64 = 1 << 7;
const T_LUSH_CAVES: u64 = 1 << 8;
const T_DRIPSTONE_CAVES: u64 = 1 << 9;
const T_SULFUR_CAVES: u64 = 1 << 10;
const T_COLD: u64 = 1 << 11;
const T_GROVE: u64 = 1 << 12;
const T_OCEAN: u64 = 1 << 13;
const T_ICE_PLAINS: u64 = 1 << 14;
const T_MUTATED: u64 = 1 << 15;
const T_FROZEN: u64 = 1 << 16;
const T_RIVER: u64 = 1 << 17;
const T_OVERWORLD: u64 = 1 << 18;
const T_DESERT: u64 = 1 << 19;
const T_JUNGLE: u64 = 1 << 20;
const T_MESA: u64 = 1 << 21;
const T_SAVANNA: u64 = 1 << 22;
const T_WARM: u64 = 1 << 23;
const T_BEACH: u64 = 1 << 24;
const T_PALE_GARDEN: u64 = 1 << 25;
const T_BIRCH: u64 = 1 << 26;
const T_CHERRY_GROVE: u64 = 1 << 27;
const T_ROOFED: u64 = 1 << 28;
const T_TAIGA: u64 = 1 << 29;
const T_FOREST: u64 = 1 << 30;
const T_MANGROVE_SWAMP: u64 = 1 << 31;
const T_SWAMP: u64 = 1 << 32;
const T_MOOSHROOM_ISLAND: u64 = 1 << 33;

/// A vanilla biome's tags (Bedrock's biome tags, which the order below
/// reads). Biomes not listed have none.
fn tags(biome: &str) -> u64 {
    match biome {
        "badlands" => T_MESA | T_OVERWORLD,
        "bamboo_jungle" => T_JUNGLE | T_OVERWORLD,
        "basalt_deltas" => T_BASALT_DELTAS | T_NETHER,
        "beach" => T_BEACH | T_OVERWORLD | T_WARM,
        "birch_forest" => T_BIRCH | T_FOREST | T_OVERWORLD,
        "cherry_grove" => T_CHERRY_GROVE | T_OVERWORLD,
        "cold_ocean" => T_COLD | T_OCEAN | T_OVERWORLD,
        "crimson_forest" => T_CRIMSON_FOREST | T_NETHER,
        "dappled_forest" => T_FOREST | T_OVERWORLD,
        "dark_forest" => T_FOREST | T_OVERWORLD | T_ROOFED,
        "deep_cold_ocean" => T_COLD | T_OCEAN | T_OVERWORLD,
        "deep_dark" => T_DEEP_DARK | T_OVERWORLD,
        "deep_frozen_ocean" => T_FROZEN | T_OCEAN | T_OVERWORLD,
        "deep_lukewarm_ocean" => T_OCEAN | T_OVERWORLD,
        "deep_ocean" => T_OCEAN | T_OVERWORLD,
        "desert" => T_DESERT | T_OVERWORLD,
        "dripstone_caves" => T_DRIPSTONE_CAVES | T_OVERWORLD,
        "end_barrens" => T_THE_END,
        "end_highlands" => T_THE_END,
        "end_midlands" => T_THE_END,
        "eroded_badlands" => T_MESA | T_MUTATED | T_OVERWORLD,
        "flower_forest" => T_MUTATED | T_OVERWORLD,
        "forest" => T_FOREST | T_OVERWORLD,
        "frozen_ocean" => T_FROZEN | T_OCEAN | T_OVERWORLD,
        "frozen_peaks" => T_FROZEN | T_OVERWORLD,
        "frozen_river" => T_FROZEN | T_OVERWORLD | T_RIVER,
        "grove" => T_COLD | T_GROVE | T_OVERWORLD,
        "ice_spikes" => T_FROZEN | T_ICE_PLAINS | T_MUTATED | T_OVERWORLD,
        "jagged_peaks" => T_FROZEN | T_OVERWORLD,
        "jungle" => T_JUNGLE | T_OVERWORLD,
        "lukewarm_ocean" => T_OCEAN | T_OVERWORLD,
        "lush_caves" => T_LUSH_CAVES | T_OVERWORLD,
        "mangrove_swamp" => T_MANGROVE_SWAMP | T_OVERWORLD,
        "meadow" => T_OVERWORLD,
        "mushroom_fields" => T_MOOSHROOM_ISLAND | T_OVERWORLD,
        "nether_wastes" => T_NETHER | T_NETHER_WASTES,
        "ocean" => T_OCEAN | T_OVERWORLD,
        "old_growth_birch_forest" => T_BIRCH | T_FOREST | T_MUTATED,
        "old_growth_pine_taiga" => T_FOREST | T_OVERWORLD | T_TAIGA,
        "old_growth_spruce_taiga" => T_FOREST | T_MUTATED | T_OVERWORLD | T_TAIGA,
        "pale_garden" => T_OVERWORLD | T_PALE_GARDEN,
        "plains" => T_OVERWORLD,
        "river" => T_OVERWORLD | T_RIVER,
        "savanna" => T_OVERWORLD | T_SAVANNA,
        "savanna_plateau" => T_OVERWORLD | T_SAVANNA,
        "small_end_islands" => T_THE_END,
        "snowy_beach" => T_BEACH | T_COLD | T_OVERWORLD,
        "snowy_plains" => T_FROZEN | T_ICE_PLAINS | T_OVERWORLD,
        "snowy_slopes" => T_FROZEN | T_OVERWORLD,
        "snowy_taiga" => T_COLD | T_FOREST | T_OVERWORLD | T_TAIGA,
        "soul_sand_valley" => T_NETHER | T_SOULSAND_VALLEY,
        "sparse_jungle" => T_JUNGLE | T_OVERWORLD,
        "stony_peaks" => T_OVERWORLD,
        "stony_shore" => T_BEACH | T_OVERWORLD,
        "sulfur_caves" => T_OVERWORLD | T_SULFUR_CAVES,
        "sunflower_plains" => T_MUTATED | T_OVERWORLD,
        "swamp" => T_OVERWORLD | T_SWAMP,
        "taiga" => T_FOREST | T_OVERWORLD | T_TAIGA,
        "the_end" => T_THE_END,
        "warm_ocean" => T_OCEAN | T_OVERWORLD | T_WARM,
        "warped_forest" => T_NETHER | T_WARPED_FOREST,
        "windswept_forest" => T_FOREST | T_OVERWORLD,
        "windswept_gravelly_hills" => T_MUTATED | T_OVERWORLD,
        "windswept_hills" => T_OVERWORLD,
        "windswept_savanna" => T_MUTATED | T_OVERWORLD | T_SAVANNA,
        "wooded_badlands" => T_MESA | T_OVERWORLD,
        _ => 0,
    }
}

fn has(tags: u64, t: u64) -> bool {
    tags & t != 0
}

/// Biomes cold enough to snow in.
fn cold_biome(t: u64) -> bool {
    (has(t, T_COLD) && !has(t, T_GROVE) && !has(t, T_OCEAN))
        || (has(t, T_ICE_PLAINS) && has(t, T_MUTATED))
        || has(t, T_FROZEN)
        || has(t, T_GROVE)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Weather {
    Clear,
    Rain,
    Thunder,
}

impl Weather {
    /// Vanilla's `isThundering` and `isRaining`.
    pub(super) fn now() -> Weather {
        if crate::scene::environment::thunder_level() > 0.9 {
            Weather::Thunder
        } else if crate::scene::environment::rain_level() > 0.2 {
            Weather::Rain
        } else {
            Weather::Clear
        }
    }
}

/// The profile: its tint id, or `None` where shafts don't show (the End,
/// the Nether, the deep dark, rain, thunder, snow, blizzards and sand or
/// savanna wind). `cave` is whether the player stands enclosed in stone.
pub(super) fn profile(biome: &str, weather: Weather, cave: bool) -> Option<i32> {
    let t = tags(biome);
    let wet = weather != Weather::Clear;
    if has(t, T_THE_END) || has(t, T_NETHER) || has(t, T_DEEP_DARK) {
        return None;
    }
    if has(t, T_LUSH_CAVES) {
        return Some(7);
    }
    if has(t, T_DRIPSTONE_CAVES) {
        return Some(5);
    }
    if has(t, T_SULFUR_CAVES) && cave {
        return Some(29);
    }
    if cave {
        return Some(21);
    }
    if wet && cold_biome(t) {
        return None;
    }
    if cold_biome(t) {
        return Some(17);
    }
    for (tag, id) in [(T_DESERT, 4), (T_MESA, 9), (T_SAVANNA, 14)] {
        if has(t, tag) {
            return (!wet).then_some(id);
        }
    }
    if wet {
        return None;
    }
    Some(if has(t, T_SULFUR_CAVES) {
        28
    } else if has(t, T_BEACH) {
        11
    } else if has(t, T_PALE_GARDEN) {
        27
    } else if has(t, T_BIRCH) {
        0
    } else if has(t, T_CHERRY_GROVE) {
        1
    } else if has(t, T_ROOFED) {
        22
    } else if has(t, T_TAIGA) {
        19
    } else if has(t, T_FOREST) {
        11
    } else if has(t, T_JUNGLE) {
        6
    } else if has(t, T_MANGROVE_SWAMP) || has(t, T_SWAMP) {
        18
    } else if has(t, T_MOOSHROOM_ISLAND) {
        10
    } else {
        11
    })
}

/// A profile's beam tint (sRGB, 0 to 1).
pub(super) fn tint(id: i32) -> [f32; 3] {
    let c: [u8; 3] = match id {
        0 => [255, 232, 166],
        1 => [255, 171, 248],
        2 => [150, 169, 243],
        3 => [255, 213, 122],
        4 => [255, 199, 96],
        5 => [184, 135, 96],
        6 => [118, 218, 88],
        7 => [136, 238, 132],
        8 => [255, 143, 82],
        9 => [218, 104, 56],
        10 => [205, 142, 255],
        12 => [142, 179, 220],
        13 => [255, 188, 92],
        14 => [255, 181, 76],
        15 => [170, 221, 255],
        16 => [235, 246, 255],
        17 => [220, 238, 255],
        18 => [154, 191, 126],
        19 => [184, 218, 176],
        20 => [116, 130, 165],
        21 => [174, 170, 160],
        22 => [130, 173, 103],
        23 => [230, 151, 73],
        24 => [200, 104, 62],
        25 => [218, 139, 64],
        26 => [205, 230, 255],
        27 => [181, 181, 181],
        28 => [255, 222, 66],
        29 => [210, 255, 74],
        _ => [255, 199, 96],
    };
    c.map(|v| f32::from(v) / 255.0)
}

/// The moon's beams, whatever the profile.
pub(super) const MOON_TINT: [f32; 3] = [188.0 / 255.0, 239.0 / 255.0, 1.0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caves_come_before_the_weather() {
        assert_eq!(profile("lush_caves", Weather::Rain, false), Some(7));
        assert_eq!(profile("plains", Weather::Rain, false), None);
        assert_eq!(profile("plains", Weather::Rain, true), Some(21));
    }

    #[test]
    fn the_end_nether_and_deep_dark_have_no_shafts() {
        for b in ["the_end", "nether_wastes", "deep_dark"] {
            assert_eq!(profile(b, Weather::Clear, false), None, "{b}");
        }
    }

    #[test]
    fn clear_biomes_pick_their_tint() {
        assert_eq!(profile("birch_forest", Weather::Clear, false), Some(0));
        assert_eq!(profile("snowy_plains", Weather::Clear, false), Some(17));
        assert_eq!(profile("desert", Weather::Clear, false), Some(4));
        assert_eq!(profile("desert", Weather::Rain, false), None);
        assert_eq!(profile("plains", Weather::Clear, false), Some(11));
    }
}
