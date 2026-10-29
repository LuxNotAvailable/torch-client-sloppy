//! The audio environment attributes (`minecraft:audio/background_music`,
//! `minecraft:audio/ambient_sounds`) from the datapack's dimension types and
//! biomes. A biome's value replaces its dimension's, as vanilla's attribute
//! layers do.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

/// `Music`: one track list and how long to wait between them, in ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct Music {
    pub event: String,
    pub min_delay: i32,
    pub max_delay: i32,
    pub replace_current: bool,
}

/// `BackgroundMusic`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BackgroundMusic {
    pub default: Option<Music>,
    pub creative: Option<Music>,
    pub underwater: Option<Music>,
}

impl BackgroundMusic {
    /// `BackgroundMusic.select`.
    pub fn select(&self, creative: bool, underwater: bool) -> Option<&Music> {
        if underwater && self.underwater.is_some() {
            return self.underwater.as_ref();
        }
        if creative && self.creative.is_some() {
            return self.creative.as_ref();
        }
        self.default.as_ref()
    }
}

/// `AmbientMoodSettings`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mood {
    pub event: String,
    pub tick_delay: i32,
    pub block_search_extent: i32,
    pub offset: f64,
}

/// `AmbientAdditionsSettings`.
#[derive(Clone, Debug, PartialEq)]
pub struct Addition {
    pub event: String,
    pub tick_chance: f64,
}

/// `AmbientSounds`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AmbientSounds {
    pub looping: Option<String>,
    pub mood: Option<Mood>,
    pub additions: Vec<Addition>,
}

#[derive(Default)]
struct Layer {
    music: Option<BackgroundMusic>,
    ambient: Option<AmbientSounds>,
}

struct Table {
    dimensions: HashMap<String, Layer>,
    biomes: HashMap<String, Layer>,
}

fn event(value: &Value) -> Option<String> {
    let id = value.as_str().or_else(|| value.get("sound_id")?.as_str())?;
    Some(crate::audio::event_name(id).to_owned())
}

fn parse_music(value: &Value) -> Option<Music> {
    Some(Music {
        event: event(value.get("sound")?)?,
        min_delay: value.get("min_delay")?.as_i64()? as i32,
        max_delay: value.get("max_delay")?.as_i64()? as i32,
        replace_current: value
            .get("replace_current_music")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn layer(json: &Value) -> Layer {
    let attributes = json.get("attributes");
    let attribute = |key: &str| attributes.and_then(|a| a.get(key));
    Layer {
        music: attribute("minecraft:audio/background_music").map(|m| BackgroundMusic {
            default: m.get("default").and_then(parse_music),
            creative: m.get("creative").and_then(parse_music),
            underwater: m.get("underwater").and_then(parse_music),
        }),
        ambient: attribute("minecraft:audio/ambient_sounds").map(|a| AmbientSounds {
            looping: a.get("loop").and_then(event),
            mood: a.get("mood").and_then(|m| {
                Some(Mood {
                    event: event(m.get("sound")?)?,
                    tick_delay: m.get("tick_delay").and_then(Value::as_i64).unwrap_or(6000) as i32,
                    block_search_extent: m
                        .get("block_search_extent")
                        .and_then(Value::as_i64)
                        .unwrap_or(8) as i32,
                    offset: m.get("offset").and_then(Value::as_f64).unwrap_or(2.0),
                })
            }),
            // One addition, or a list of them.
            additions: {
                let addition = |entry: &Value| {
                    Some(Addition {
                        event: event(entry.get("sound")?)?,
                        tick_chance: entry.get("tick_chance")?.as_f64()?,
                    })
                };
                match a.get("additions") {
                    Some(Value::Array(list)) => list.iter().filter_map(addition).collect(),
                    Some(one) => addition(one).into_iter().collect(),
                    None => Vec::new(),
                }
            },
        }),
    }
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let load = |registry: &str| -> HashMap<String, Layer> {
            crate::util::datapack::entries(registry)
                .into_iter()
                .map(|(name, json)| (name, layer(&json)))
                .collect()
        };
        Table {
            dimensions: load("dimension_type"),
            biomes: load("worldgen/biome"),
        }
    })
}

fn bare(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// The music where the camera is: the biome's, else the dimension's.
pub fn music(dimension: &str, biome: Option<&str>) -> BackgroundMusic {
    let table = table();
    biome
        .and_then(|b| table.biomes.get(bare(b))?.music.clone())
        .or_else(|| table.dimensions.get(bare(dimension))?.music.clone())
        .unwrap_or_default()
}

/// The ambience where the player is: the biome's, else the dimension's.
pub fn ambient(dimension: &str, biome: Option<&str>) -> AmbientSounds {
    let table = table();
    biome
        .and_then(|b| table.biomes.get(bare(b))?.ambient.clone())
        .or_else(|| table.dimensions.get(bare(dimension))?.ambient.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_reads_music_mood_loop_and_additions() {
        let json: Value = serde_json::from_str(
            r#"{"attributes":{
                "minecraft:audio/background_music":{
                    "default":{"sound":"minecraft:music.game","min_delay":12000,"max_delay":24000},
                    "underwater":{"sound":"minecraft:music.under_water","min_delay":1,"max_delay":2,"replace_current_music":true}
                },
                "minecraft:audio/ambient_sounds":{
                    "loop":"minecraft:ambient.crimson_forest.loop",
                    "mood":{"sound":"minecraft:ambient.cave","tick_delay":6000,"block_search_extent":8,"offset":2.0},
                    "additions":{"sound":"minecraft:ambient.crimson_forest.additions","tick_chance":0.0111}
                }
            }}"#,
        )
        .unwrap();
        let layer = layer(&json);
        let music = layer.music.unwrap();
        assert_eq!(music.select(false, false).unwrap().event, "music.game");
        let under = music.select(true, true).unwrap();
        assert_eq!(under.event, "music.under_water");
        assert!(under.replace_current);
        assert_eq!(music.select(true, false).unwrap().event, "music.game");
        let ambient = layer.ambient.unwrap();
        assert_eq!(
            ambient.looping.as_deref(),
            Some("ambient.crimson_forest.loop")
        );
        assert_eq!(ambient.mood.unwrap().tick_delay, 6000);
        assert_eq!(ambient.additions[0].tick_chance, 0.0111);
    }
}
