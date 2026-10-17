use std::collections::HashMap;
use std::sync::OnceLock;

pub const MAX_PATTERNS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BannerLayer {
    pub asset: Box<str>,
    pub color: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BannerData {
    pub layers: Vec<BannerLayer>,
}

pub fn base_color_of(block: &str) -> u8 {
    let name = block
        .strip_suffix("_wall_banner")
        .or_else(|| block.strip_suffix("_banner"))
        .unwrap_or("white");
    crate::util::dye::id_by_name(name).unwrap_or(0) as u8
}

pub fn is_wall(block: &str) -> bool {
    block.ends_with("_wall_banner")
}

fn assets() -> &'static HashMap<Box<str>, Box<str>> {
    static ASSETS: OnceLock<HashMap<Box<str>, Box<str>>> = OnceLock::new();
    ASSETS.get_or_init(|| {
        crate::util::datapack::entries("banner_pattern")
            .into_iter()
            .filter_map(|(id, json)| {
                let asset = crate::util::datapack::bare_id(json.get("asset_id")?)?;
                Some((Box::from(id.as_str()), Box::from(asset)))
            })
            .collect()
    })
}

pub fn asset_for(pattern: &str) -> Box<str> {
    assets()
        .get(pattern)
        .cloned()
        .unwrap_or_else(|| Box::from(pattern))
}

const KEY_SEP: char = '!';
const LAYER_SEP: char = ',';
const COLOR_SEP: char = '.';

pub fn model_key(item: &str, layers: &[BannerLayer]) -> String {
    let mut key = String::with_capacity(item.len() + layers.len() * 12);
    key.push_str(item);
    for (i, layer) in layers.iter().enumerate() {
        key.push(if i == 0 { KEY_SEP } else { LAYER_SEP });
        key.push_str(&layer.asset);
        key.push(COLOR_SEP);
        let color = layer.color.min(15);
        key.push((b'0' + color / 10) as char);
        key.push((b'0' + color % 10) as char);
    }
    key
}

pub fn key_item(key: &str) -> &str {
    key.split_once(KEY_SEP).map_or(key, |(item, _)| item)
}

pub fn is_model_key(key: &str) -> bool {
    key.contains(KEY_SEP)
}

pub fn parse_model_key(key: &str) -> Option<(&str, Vec<BannerLayer>)> {
    let (item, rest) = key.split_once(KEY_SEP)?;
    let mut layers = Vec::new();
    for part in rest.split(LAYER_SEP) {
        let (asset, color) = part.rsplit_once(COLOR_SEP)?;
        layers.push(BannerLayer {
            asset: Box::from(asset),
            color: color.parse::<u8>().ok()?.min(15),
        });
        if layers.len() == MAX_PATTERNS {
            break;
        }
    }
    (!layers.is_empty()).then_some((item, layers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(asset: &str, color: u8) -> BannerLayer {
        BannerLayer {
            asset: Box::from(asset),
            color,
        }
    }

    #[test]
    fn model_keys_round_trip() {
        let layers = [layer("creeper", 15), layer("border", 4)];
        let key = model_key("red_banner", &layers);
        assert_eq!(key, "red_banner!creeper.15,border.04");
        let (item, back) = parse_model_key(&key).unwrap();
        assert_eq!(item, "red_banner");
        assert_eq!(back, layers);
    }

    #[test]
    fn an_unpatterned_banner_keys_as_itself() {
        assert_eq!(model_key("white_banner", &[]), "white_banner");
        assert_eq!(parse_model_key("white_banner"), None);
        assert_eq!(parse_model_key("diamond_sword"), None);
    }

    #[test]
    fn a_dotted_asset_id_keeps_its_dots() {
        let key = model_key("gray_banner", &[layer("mod.pattern.one", 7)]);
        let (_, back) = parse_model_key(&key).unwrap();
        assert_eq!(back, [layer("mod.pattern.one", 7)]);
    }

    #[test]
    fn a_banner_block_names_its_base_colour() {
        assert_eq!(base_color_of("white_banner"), 0);
        assert_eq!(base_color_of("red_wall_banner"), 14);
        assert_eq!(base_color_of("light_gray_banner"), 8);
        assert_eq!(base_color_of("black_wall_banner"), 15);
    }

    #[test]
    fn a_wall_banner_is_not_a_standing_one() {
        assert!(is_wall("cyan_wall_banner"));
        assert!(!is_wall("cyan_banner"));
        assert_eq!(base_color_of("cyan_wall_banner"), 9);
    }
}
