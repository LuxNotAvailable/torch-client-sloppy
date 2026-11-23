//! Connected textures from resource packs: OptiFine's CTM format
//! (`assets/minecraft/optifine/ctm/**/*.properties`, also read by
//! Continuity). Each rule matches blocks or textures and swaps a face's
//! texture for one of its tiles by how the face's neighbours connect.
//!
//! Methods: `ctm` (47 tiles), `horizontal`, `vertical` (and the `+`
//! orderings), `top`, `random`, `repeat`, `fixed`. `ctm_compact` and
//! overlays are not supported yet.
//!
//! Converted mods add Create's connected textures (`connected.json` in a
//! `.torchmod`, `set_package_rules`): a face showing a texture takes a tile
//! of its `_connected` sheet, picked by Create's own type (`CreateType`).
//!
//! The built-in Torch pack adds connected glass. Its 47 tiles are cut from
//! each glass texture at startup (the frame erased on connected sides), so
//! no art ships and any pack's glass texture connects.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Create's `AllCTTypes`: how a sheet's tiles are picked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CreateType {
    Horizontal,
    HorizontalKryppers,
    Vertical,
    Omnidirectional,
    Roof,
    RoofStair,
    Cross,
    Rectangle,
}

impl CreateType {
    fn parse(id: &str) -> Option<CreateType> {
        Some(match id.strip_prefix("create:")? {
            "horizontal" => CreateType::Horizontal,
            "horizontal_kryppers" => CreateType::HorizontalKryppers,
            "vertical" => CreateType::Vertical,
            "omnidirectional" => CreateType::Omnidirectional,
            "roof" => CreateType::Roof,
            "roof_stair" => CreateType::RoofStair,
            "cross" => CreateType::Cross,
            "rectangle" => CreateType::Rectangle,
            _ => return None,
        })
    }
}

/// Which neighbours connect, as Create's `CTContext` has them.
#[derive(Clone, Copy, Default, Debug)]
struct Context {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    top_left: bool,
    top_right: bool,
    bottom_left: bool,
    bottom_right: bool,
}

/// `AllCTTypes.getTextureIndex`: a tile of a `sheet`-wide sheet, row by row.
fn create_index(kind: CreateType, c: Context) -> usize {
    let b = usize::from;
    match kind {
        CreateType::Horizontal => b(c.right) + b(c.left) * 2,
        CreateType::HorizontalKryppers => match (c.left, c.right) {
            (false, false) => 0,
            (_, false) => 3,
            (false, _) => 2,
            _ => 1,
        },
        CreateType::Vertical => b(c.up) + b(c.down) * 2,
        CreateType::Omnidirectional => {
            let mut x = b(c.up) as i32 + b(c.down) as i32 * 2;
            let mut y = b(c.left) as i32 + b(c.right) as i32 * 2;
            let borders = [c.up, c.down, c.left, c.right]
                .iter()
                .filter(|s| !**s)
                .count();
            if borders == 0 {
                x += b(c.top_right) as i32 + b(c.top_left) as i32 * 2;
                y += b(c.bottom_right) as i32 * 2 + b(c.bottom_left) as i32;
            }
            if borders == 1 {
                if !c.right && (c.top_left || c.bottom_left) {
                    y = 4;
                    x = -1 + b(c.bottom_left) as i32 + b(c.top_left) as i32 * 2;
                }
                if !c.left && (c.top_right || c.bottom_right) {
                    y = 5;
                    x = -1 + b(c.bottom_right) as i32 + b(c.top_right) as i32 * 2;
                }
                if !c.down && (c.top_left || c.top_right) {
                    y = 6;
                    x = -1 + b(c.top_left) as i32 + b(c.top_right) as i32 * 2;
                }
                if !c.up && (c.bottom_left || c.bottom_right) {
                    y = 7;
                    x = -1 + b(c.bottom_left) as i32 + b(c.bottom_right) as i32 * 2;
                }
            }
            if borders == 2
                && (c.up && c.left && c.top_left
                    || c.down && c.left && c.bottom_left
                    || c.up && c.right && c.top_right
                    || c.down && c.right && c.bottom_right)
            {
                x += 3;
            }
            (x + 8 * y).max(0) as usize
        }
        CreateType::Roof => {
            let up_drops = c.down && !c.up && (c.left || c.right);
            let down_drops = !c.down && c.up && (c.left || c.right);
            let left_drops = !c.left && c.right && (c.up || c.down);
            let right_drops = c.left && !c.right && (c.up || c.down);
            if up_drops {
                if left_drops {
                    return if c.bottom_right { 0 } else { 5 };
                }
                if right_drops {
                    return if c.bottom_left { 2 } else { 5 };
                }
                return 1;
            }
            if down_drops {
                if left_drops {
                    return if c.top_right { 8 } else { 5 };
                }
                if right_drops {
                    return if c.top_left { 10 } else { 5 };
                }
                return 9;
            }
            if left_drops {
                return 4;
            }
            if right_drops {
                return 6;
            }
            if c.up && c.down && c.left && c.right {
                if c.bottom_left && c.top_right {
                    if c.top_left && !c.bottom_right {
                        return 12;
                    }
                    if c.bottom_right && !c.top_left {
                        return 15;
                    }
                    if !c.bottom_right && !c.top_left {
                        return 7;
                    }
                }
                if c.bottom_right && c.top_left {
                    if c.top_right && !c.bottom_left {
                        return 13;
                    }
                    if c.bottom_left && !c.top_right {
                        return 14;
                    }
                    if !c.bottom_left && !c.top_right {
                        return 11;
                    }
                }
            }
            5
        }
        CreateType::RoofStair => {
            const MAPPING: [[usize; 4]; 4] =
                [[1, 6, 9, 4], [14, 12, 13, 15], [2, 10, 8, 0], [5, 5, 5, 5]];
            MAPPING[b(c.up) * 2 + b(c.right)][b(c.left) * 2 + b(c.down)]
        }
        CreateType::Cross => b(c.up) + b(c.down) * 2 + b(c.left) * 4 + b(c.right) * 8,
        CreateType::Rectangle => {
            let x = match (c.left, c.right) {
                (true, true) => 2,
                (true, false) => 3,
                (false, true) => 1,
                _ => 0,
            };
            let y = match (c.up, c.down) {
                (true, true) => 1,
                (true, false) => 2,
                (false, true) => 0,
                _ => 3,
            };
            x + y * 4
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Method {
    Create(CreateType),
    Ctm,
    Horizontal,
    Vertical,
    Top,
    Random,
    Repeat(u32, u32),
    Fixed,
}

struct Rule {
    method: Method,
    /// Block names (`glass`), without namespace or properties.
    blocks: Vec<String>,
    /// Texture names (`glass`) the faces must show.
    textures: Vec<String>,
    /// Faces the rule applies to, by `Direction` order (down, up, north,
    /// south, west, east).
    faces: u8,
    /// The tiles' atlas keys, in order.
    tiles: Vec<String>,
}

/// A rule with its textures resolved to atlas tiles.
pub(crate) struct Resolved {
    method: Method,
    blocks: Vec<u32>,
    textures: Vec<u32>,
    faces: u8,
    tiles: Vec<u32>,
}

static RULES: OnceLock<Vec<Rule>> = OnceLock::new();

/// Packages' `connected.json` files, as `(package, text)`; read when the
/// atlas is built.
static PACKAGE_RULES: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

#[cfg_attr(
    not(resource_packs),
    allow(dead_code, reason = "packages mount with resource packs")
)]
pub(crate) fn set_package_rules(rules: Vec<(String, String)>) {
    *PACKAGE_RULES.lock().unwrap_or_else(|e| e.into_inner()) = rules;
}

/// Rules for packages' connected textures: each `original` texture's faces
/// take tiles cut from its `connected` sheet (`sheet` tiles a side).
fn package_rules(images: &mut Vec<(String, image::RgbaImage)>) -> Vec<Rule> {
    let packages = std::mem::take(&mut *PACKAGE_RULES.lock().unwrap_or_else(|e| e.into_inner()));
    let mut rules = Vec::new();
    let mut sheets: HashMap<String, Vec<String>> = HashMap::new();
    for (package, text) in packages {
        let Ok(serde_json::Value::Array(entries)) = serde_json::from_str(&text) else {
            crate::log_warn!("ctm", "{package}: connected.json doesn't read");
            continue;
        };
        for entry in entries {
            let (Some(original), Some(connected), Some(kind), Some(sheet)) = (
                entry["original"].as_str(),
                entry["connected"].as_str(),
                entry["type"].as_str().and_then(CreateType::parse),
                entry["sheet"].as_u64().map(|n| n as u32),
            ) else {
                continue;
            };
            let tiles = match sheets.get(connected) {
                Some(tiles) => tiles.clone(),
                None => {
                    let tiles = cut_sheet(connected, sheet, images);
                    sheets.insert(connected.to_owned(), tiles.clone());
                    tiles
                }
            };
            if tiles.len() != (sheet * sheet) as usize {
                continue;
            }
            rules.push(Rule {
                method: Method::Create(kind),
                blocks: Vec::new(),
                textures: vec![crate::blocks::bake::sprite_key(original).to_owned()],
                faces: 63,
                tiles,
            });
        }
    }
    if !rules.is_empty() {
        crate::log_info!("ctm", "{} connected texture(s) from packages", rules.len());
    }
    rules
}

/// A `sheet` x `sheet` sheet cut into tiles, row by row, as atlas keys.
fn cut_sheet(id: &str, sheet: u32, images: &mut Vec<(String, image::RgbaImage)>) -> Vec<String> {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    let Some(assets) = crate::assets_root().parent() else {
        return Vec::new();
    };
    let png = assets.join(ns).join("textures").join(format!("{path}.png"));
    let Some(image) = crate::platform::assets::read(&png)
        .and_then(|b| image::load_from_memory(&b).ok())
        .map(|i| i.to_rgba8())
    else {
        return Vec::new();
    };
    let side = image.width() / sheet.max(1);
    if side == 0 || image.height() < side * sheet {
        return Vec::new();
    }
    let mut keys = Vec::new();
    for i in 0..sheet * sheet {
        let (x, y) = (i % sheet, i / sheet);
        let key = format!("ctm!{id}#{i}");
        images.push((
            key.clone(),
            image::imageops::crop_imm(&image, x * side, y * side, side, side).to_image(),
        ));
        keys.push(key);
    }
    keys
}
static RESOLVED: OnceLock<(
    HashMap<u32, Vec<usize>>,
    HashMap<u32, Vec<usize>>,
    Vec<Resolved>,
)> = OnceLock::new();

/// The rules' tile images, for the block atlas (keys `ctm!...`). Reads the
/// mounted packs once. `block` holds the decoded block textures, which the
/// built-in glass tiles are cut from.
pub(crate) fn tile_images(
    block: &std::collections::BTreeMap<String, image::RgbaImage>,
) -> Vec<(String, image::RgbaImage)> {
    let mut images = Vec::new();
    RULES.get_or_init(|| {
        let mut rules = load(&mut images);
        rules.extend(package_rules(&mut images));
        if builtin_on() {
            rules.extend(builtin(block, &mut images));
        }
        rules
    });
    images
}

#[cfg(resource_packs)]
fn builtin_on() -> bool {
    crate::resourcepacks::builtin_enabled()
}

/// Builds without packs have no menu to turn it off from.
#[cfg(not(resource_packs))]
fn builtin_on() -> bool {
    true
}

/// The 16 dye colours, for the stained glass names.
const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// The built-in pack's rules: every glass, and its pane, connects.
fn builtin(
    block: &std::collections::BTreeMap<String, image::RgbaImage>,
    images: &mut Vec<(String, image::RgbaImage)>,
) -> Vec<Rule> {
    let mut glasses = vec![
        ("glass".to_owned(), Some("glass_pane".to_owned())),
        ("tinted_glass".to_owned(), None),
    ];
    for dye in DYES {
        glasses.push((
            format!("{dye}_stained_glass"),
            Some(format!("{dye}_stained_glass_pane")),
        ));
    }
    let masks = representative_masks();
    let mut rules = Vec::new();
    for (texture, pane) in glasses {
        let Some(base) = block.get(&texture) else {
            continue;
        };
        let mut tiles = Vec::with_capacity(masks.len());
        for (index, &mask) in masks.iter().enumerate() {
            let key = format!("ctm!torch/{texture}/{index}");
            images.push((key.clone(), connected_tile(base, mask)));
            tiles.push(key);
        }
        let mut blocks = vec![texture.clone()];
        blocks.extend(pane);
        rules.push(Rule {
            method: Method::Ctm,
            blocks,
            textures: vec![texture],
            faces: 63,
            tiles,
        });
    }
    rules
}

/// For each of the 47 tiles, a neighbour mask the `CTM` table maps to it.
fn representative_masks() -> [u8; 47] {
    let mut out = [0u8; 47];
    let mut seen = [false; 47];
    for mask in 0..=255u8 {
        let tile = usize::from(CTM[usize::from(mask)]);
        if !seen[tile] {
            seen[tile] = true;
            out[tile] = mask;
        }
    }
    out
}

/// `base` with its frame erased where `mask` connects (bits as in `CTM`):
/// a side's strip when that side connects, a corner when both its sides
/// and the diagonal do. Erased texels copy the nearest interior one.
fn connected_tile(base: &image::RgbaImage, mask: u8) -> image::RgbaImage {
    let (w, h) = base.dimensions();
    // Vanilla's frame is one texel of a 16-texel tile.
    let b = (w / 16).max(1);
    let has = |bit: u8| mask & (1 << bit) != 0;
    let (left, down, right, up) = (has(0), has(2), has(4), has(6));
    let mut out = base.clone();
    for y in 0..h {
        for x in 0..w {
            let (l, r, u, d) = (x < b, x >= w - b, y < b, y >= h - b);
            let erase = match (l, r, u, d) {
                (true, _, true, _) => left && up && has(7),
                (_, true, true, _) => right && up && has(5),
                (true, _, _, true) => left && down && has(1),
                (_, true, _, true) => right && down && has(3),
                (true, ..) => left,
                (_, true, ..) => right,
                (_, _, true, _) => up,
                (_, _, _, true) => down,
                _ => false,
            };
            if erase {
                let from = (x.clamp(b, w - b - 1), y.clamp(b, h - b - 1));
                out.put_pixel(x, y, *base.get_pixel(from.0, from.1));
            }
        }
    }
    out
}

#[cfg(resource_packs)]
fn load(images: &mut Vec<(String, image::RgbaImage)>) -> Vec<Rule> {
    use crate::platform::assets::archive::PACKS;
    let mut rules = Vec::new();
    for root in [
        "assets/minecraft/optifine/ctm",
        "assets/minecraft/mcpatcher/ctm",
    ] {
        for rest in PACKS.descendants(root) {
            if !rest.ends_with(".properties") {
                continue;
            }
            let key = format!("{root}/{rest}");
            let Some(text) = PACKS.get(&key).and_then(|b| std::str::from_utf8(b).ok()) else {
                continue;
            };
            let dir = key.rsplit_once('/').map_or("", |(d, _)| d);
            let stem = rest
                .rsplit('/')
                .next()
                .unwrap_or(rest)
                .trim_end_matches(".properties");
            if let Some(rule) = parse(text, dir, stem, images) {
                rules.push(rule);
            }
        }
    }
    if !rules.is_empty() {
        crate::log_info!("ctm", "{} connected texture rule(s)", rules.len());
    }
    rules
}

#[cfg(not(resource_packs))]
fn load(_images: &mut Vec<(String, image::RgbaImage)>) -> Vec<Rule> {
    Vec::new()
}

/// A name without its namespace and `block/` folder.
fn bare(name: &str) -> String {
    let name = name.rsplit(':').next().unwrap_or(name);
    name.trim_start_matches("block/")
        .trim_start_matches("blocks/")
        .to_owned()
}

#[cfg(resource_packs)]
fn parse(
    text: &str,
    dir: &str,
    stem: &str,
    images: &mut Vec<(String, image::RgbaImage)>,
) -> Option<Rule> {
    use crate::platform::assets::archive::PACKS;
    let props: HashMap<&str, &str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let get = |k: &str| props.get(k).copied().unwrap_or("");
    let method = match get("method") {
        "ctm" | "glass" => Method::Ctm,
        "horizontal" | "bookshelf" | "horizontal+vertical" | "h+v" => Method::Horizontal,
        "vertical" | "vertical+horizontal" | "v+h" => Method::Vertical,
        "top" => Method::Top,
        "random" => Method::Random,
        "repeat" => Method::Repeat(
            get("width").parse().unwrap_or(1).max(1),
            get("height").parse().unwrap_or(1).max(1),
        ),
        "fixed" => Method::Fixed,
        _ => return None,
    };
    // `matchBlocks` entries may carry properties (`glass_pane:east=true`);
    // they match the block alone.
    let blocks: Vec<String> = get("matchBlocks")
        .split_whitespace()
        .map(|b| {
            let b = b.strip_prefix("minecraft:").unwrap_or(b);
            bare(b.split(':').next().unwrap_or(b))
        })
        .collect();
    let mut textures: Vec<String> = get("matchTiles").split_whitespace().map(bare).collect();
    // A file named for a block or texture matches it when nothing else says.
    if blocks.is_empty() && textures.is_empty() {
        textures.push(bare(stem.trim_start_matches("block_")));
    }
    let faces = {
        let mut f = 0u8;
        for word in get("faces").split_whitespace() {
            f |= match word {
                "bottom" | "down" => 1,
                "top" | "up" => 2,
                "north" => 4,
                "south" => 8,
                "west" => 16,
                "east" => 32,
                "sides" => 4 | 8 | 16 | 32,
                _ => 63,
            };
        }
        if f == 0 { 63 } else { f }
    };
    // `tiles`: numbers and ranges (`0-46`) name pngs beside the file;
    // names are textures (`glass`, `minecraft:block/glass`).
    let mut tiles = Vec::new();
    for word in get("tiles").split_whitespace() {
        let range = word
            .split_once('-')
            .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)));
        let names: Vec<String> = match (range, word.parse::<u32>()) {
            (Some((a, b)), _) => (a..=b).map(|n| n.to_string()).collect(),
            (None, Ok(n)) => vec![n.to_string()],
            _ => vec![word.trim_end_matches(".png").to_owned()],
        };
        for name in names {
            let path = if name.contains(':') || name.contains('/') && !name.starts_with("./") {
                let bare_name = bare(&name);
                // A vanilla texture: its own tile.
                tiles.push(bare_name);
                continue;
            } else {
                format!("{dir}/{}.png", name.trim_start_matches("./"))
            };
            let Some(image) = PACKS
                .get(&path)
                .and_then(|b| image::load_from_memory(b).ok())
            else {
                continue;
            };
            let key = format!("ctm!{path}");
            images.push((key.clone(), image.to_rgba8()));
            tiles.push(key);
        }
    }
    let needed = match method {
        Method::Ctm => 47,
        Method::Horizontal | Method::Vertical => 4,
        Method::Repeat(w, h) => (w * h) as usize,
        _ => 1,
    };
    (tiles.len() >= needed).then_some(Rule {
        method,
        blocks,
        textures,
        faces,
        tiles,
    })
}

#[cfg(not(resource_packs))]
fn parse(_: &str, _: &str, _: &str, _: &mut Vec<(String, image::RgbaImage)>) -> Option<Rule> {
    None
}

/// The rules with atlas tiles, and indices by texture tile and block kind.
fn resolved() -> Option<&'static (
    HashMap<u32, Vec<usize>>,
    HashMap<u32, Vec<usize>>,
    Vec<Resolved>,
)> {
    let rules = RULES.get().filter(|r| !r.is_empty())?;
    let map = crate::TEXTURE_MAP.get()?;
    Some(RESOLVED.get_or_init(|| {
        let mut by_tile: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut by_block: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut out = Vec::new();
        for rule in rules {
            let tiles: Option<Vec<u32>> = rule.tiles.iter().map(|t| map.get(t).copied()).collect();
            let Some(tiles) = tiles else {
                let missing = rule.tiles.iter().find(|t| !map.contains_key(*t));
                crate::log_warn!("ctm", "a rule's tile is not in the atlas: {missing:?}");
                continue;
            };
            let blocks: Vec<u32> = rule
                .blocks
                .iter()
                .filter_map(|b| b.parse::<azalea_registry::builtin::BlockKind>().ok())
                .map(|k| k as u32)
                .collect();
            let textures: Vec<u32> = rule
                .textures
                .iter()
                .filter_map(|t| map.get(t).copied())
                .collect();
            let at = out.len();
            for t in &textures {
                by_tile.entry(*t).or_default().push(at);
            }
            for b in &blocks {
                by_block.entry(*b).or_default().push(at);
            }
            out.push(Resolved {
                method: rule.method,
                blocks,
                textures,
                faces: rule.faces,
                tiles,
            });
        }
        crate::log_info!(
            "ctm",
            "{} of {} rule(s) resolved, {} by block, {} by texture",
            out.len(),
            rules.len(),
            by_block.len(),
            by_tile.len()
        );
        (by_tile, by_block, out)
    }))
}

/// Continuity's `CtmSpriteProvider` table: tile by the 8-neighbour mask
/// (bits: 0 left, 1 down-left, 2 down, 3 down-right, 4 right, 5 up-right,
/// 6 up, 7 up-left; a corner only with both its sides).
const CTM: [u8; 256] = [
    0, 3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1, 2, 1, 2, 13,
    31, 13, 14, 0, 3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1,
    2, 1, 2, 13, 31, 13, 14, 36, 17, 36, 17, 24, 19, 24, 43, 36, 17, 36, 17, 24, 19, 24, 43, 16,
    18, 16, 18, 6, 46, 6, 21, 16, 18, 16, 18, 28, 9, 28, 22, 36, 17, 36, 17, 24, 19, 24, 43, 36,
    17, 36, 17, 24, 19, 24, 43, 37, 40, 37, 40, 30, 8, 30, 34, 37, 40, 37, 40, 25, 23, 25, 45, 0,
    3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1, 2, 1, 2, 13, 31,
    13, 14, 0, 3, 0, 3, 12, 5, 12, 15, 0, 3, 0, 3, 12, 5, 12, 15, 1, 2, 1, 2, 4, 7, 4, 29, 1, 2, 1,
    2, 13, 31, 13, 14, 36, 39, 36, 39, 24, 41, 24, 27, 36, 39, 36, 39, 24, 41, 24, 27, 16, 42, 16,
    42, 6, 20, 6, 10, 16, 42, 16, 42, 28, 35, 28, 44, 36, 39, 36, 39, 24, 41, 24, 27, 36, 39, 36,
    39, 24, 41, 24, 27, 37, 38, 37, 38, 30, 11, 30, 32, 37, 38, 37, 38, 25, 33, 25, 26,
];

type V = (i32, i32, i32);

/// A face's in-plane directions as the texture shows them: left, down,
/// right, up (face order down, up, north, south, west, east).
pub(crate) const DIRS: [[V; 4]; 6] = [
    [(-1, 0, 0), (0, 0, -1), (1, 0, 0), (0, 0, 1)],
    [(-1, 0, 0), (0, 0, 1), (1, 0, 0), (0, 0, -1)],
    [(1, 0, 0), (0, -1, 0), (-1, 0, 0), (0, 1, 0)],
    [(-1, 0, 0), (0, -1, 0), (1, 0, 0), (0, 1, 0)],
    [(0, 0, -1), (0, -1, 0), (0, 0, 1), (0, 1, 0)],
    [(0, 0, 1), (0, -1, 0), (0, 0, -1), (0, 1, 0)],
];
const NORMAL: [V; 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

fn add(a: V, b: V) -> V {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

/// A face's texture-space directions: left, down, right, up.
pub(crate) type Dirs = [V; 4];

/// The tile a face of the block at `pos` (kind `kind`) showing atlas tile
/// `tile` takes, if a rule swaps it. `main` is the model's particle tile:
/// a rule naming only blocks swaps just that texture (a pane's glass, not
/// its edge), as OptiFine does. `dirs` comes from the quad's uvs, so
/// mirrored faces (a pane's west side) connect the right way round. `flush`
/// is whether the face lies on the block's side.
pub(crate) fn tile_for(
    kind: u32,
    tile: u32,
    main: Option<u32>,
    face: u8,
    dirs: Option<Dirs>,
    pos: V,
    flush: bool,
    kind_at: impl Fn(V) -> u32,
) -> Option<u32> {
    let (by_tile, by_block, rules) = resolved()?;
    let face = face.min(5) as usize;
    // The earliest rule wins: packs in priority order, then the built-in.
    let index = by_tile
        .get(&tile)
        .into_iter()
        .chain(by_block.get(&kind))
        .flatten()
        .copied()
        .filter(|&i| {
            let r = &rules[i];
            let texture_ok = if r.textures.is_empty() {
                main.is_none_or(|m| m == tile)
            } else {
                r.textures.contains(&tile)
            };
            r.faces & (1 << face) != 0
                && (r.blocks.is_empty() || r.blocks.contains(&kind))
                && texture_ok
        })
        .min()?;
    let rule = &rules[index];
    let normal = NORMAL[face];
    // Same block, and (for a face on the block's side) that block's face
    // is open, not covered by another of it in front. A pane's face sits
    // mid-cell, so a pane of the crossing wall in front doesn't cover it.
    let connects = |at: V| kind_at(at) == kind && !(flush && kind_at(add(at, normal)) == kind);
    let dirs = dirs.unwrap_or(DIRS[face]);
    let index = match rule.method {
        Method::Create(kind) => {
            let at = |d: &[usize]| connects(d.iter().fold(pos, |p, &i| add(p, dirs[i])));
            // dirs: left, down, right, up.
            let c = Context {
                left: at(&[0]),
                down: at(&[1]),
                right: at(&[2]),
                up: at(&[3]),
                top_left: at(&[3, 0]),
                top_right: at(&[3, 2]),
                bottom_left: at(&[1, 0]),
                bottom_right: at(&[1, 2]),
            };
            create_index(kind, c)
        }
        Method::Ctm => {
            let mut mask = 0u8;
            for i in 0..4 {
                if connects(add(pos, dirs[i])) {
                    mask |= 1 << (i * 2);
                }
            }
            for i in 0..4 {
                let j = (i + 1) % 4;
                if mask & (1 << (i * 2)) != 0
                    && mask & (1 << (j * 2)) != 0
                    && connects(add(add(pos, dirs[i]), dirs[j]))
                {
                    mask |= 1 << (i * 2 + 1);
                }
            }
            usize::from(CTM[usize::from(mask)])
        }
        Method::Horizontal => {
            let (l, r) = (connects(add(pos, dirs[0])), connects(add(pos, dirs[2])));
            [3, 2, 0, 1][usize::from(l) | usize::from(r) << 1]
        }
        Method::Vertical => {
            let (d, u) = (connects(add(pos, dirs[1])), connects(add(pos, dirs[3])));
            [3, 2, 0, 1][usize::from(d) | usize::from(u) << 1]
        }
        Method::Top => {
            if face < 2 || kind_at(add(pos, (0, 1, 0))) != kind {
                return None;
            }
            0
        }
        Method::Random => {
            let h = (pos.0.wrapping_mul(3_129_871) ^ pos.2.wrapping_mul(116_129_781) ^ pos.1)
                .wrapping_mul(0x2C1B_3C6D) as u32;
            (h ^ (face as u32).wrapping_mul(0x9E37_79B9)) as usize % rule.tiles.len()
        }
        Method::Repeat(w, h) => {
            // The face's own coordinates along its right and up.
            let along = |d: V| pos.0 * d.0 + pos.1 * d.1 + pos.2 * d.2;
            let x = along(dirs[2]).rem_euclid(w as i32) as u32;
            let y = (h as i32 - 1 - along(dirs[3]).rem_euclid(h as i32)) as u32;
            (y * w + x) as usize
        }
        Method::Fixed => 0,
    };
    rule.tiles.get(index).copied()
}

/// Whether the face's texture-left and texture-right neighbours are another
/// kind, for pane strips that move onto the frame column at an open end.
pub(crate) fn open_sides(kind: u32, dirs: Dirs, pos: V, kind_at: impl Fn(V) -> u32) -> [bool; 2] {
    [
        kind_at(add(pos, dirs[0])) != kind,
        kind_at(add(pos, dirs[2])) != kind,
    ]
}

/// The block kinds any rule matches by block (so occupancy records them),
/// or all kinds when a rule matches by texture.
pub(crate) fn tracks_kinds() -> bool {
    resolved().is_some_and(|(_, _, r)| !r.is_empty())
}

/// A block state's kind, from a table built once.
pub(crate) fn kind_of(state: u16) -> u32 {
    static KINDS: OnceLock<Vec<u16>> = OnceLock::new();
    let kinds = KINDS.get_or_init(|| {
        (0..=azalea::block::BlockState::MAX_STATE)
            .map(|id| {
                azalea::block::BlockState::try_from(id).map_or(0, |s| {
                    let block: Box<dyn azalea::block::BlockTrait> =
                        Box::<dyn azalea::block::BlockTrait>::from(s);
                    block.as_block_kind() as u16
                })
            })
            .collect()
    });
    match kinds.get(usize::from(state)) {
        Some(&kind) => u32::from(kind),
        // A modded state: its block's own kind, when a package named it.
        None => crate::blocks::modded::with_state(state, |s| s.kind).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(up: bool, down: bool, left: bool, right: bool) -> Context {
        Context {
            up,
            down,
            left,
            right,
            ..Context::default()
        }
    }

    #[test]
    fn creates_rectangle_and_omni_tiles_match_its_tables() {
        // Rectangle: x by left/right, y by up/down (AllCTTypes.RECTANGLE).
        assert_eq!(
            create_index(CreateType::Rectangle, ctx(false, false, false, false)),
            3 * 4
        );
        assert_eq!(
            create_index(CreateType::Rectangle, ctx(true, true, true, true)),
            2 + 4
        );
        assert_eq!(
            create_index(CreateType::Rectangle, ctx(false, true, false, true)),
            1
        );
        // Omni: alone is tile 0; joined on all sides with every corner, 3 + 3 rows in.
        assert_eq!(
            create_index(CreateType::Omnidirectional, ctx(false, false, false, false)),
            0
        );
        let all = Context {
            top_left: true,
            top_right: true,
            bottom_left: true,
            bottom_right: true,
            ..ctx(true, true, true, true)
        };
        assert_eq!(create_index(CreateType::Omnidirectional, all), 6 + 8 * 6);
        assert_eq!(
            create_index(CreateType::Vertical, ctx(true, true, false, false)),
            3
        );
        assert_eq!(
            create_index(CreateType::Horizontal, ctx(false, false, true, false)),
            2
        );
        for kind in [
            CreateType::Roof,
            CreateType::RoofStair,
            CreateType::Cross,
            CreateType::HorizontalKryppers,
        ] {
            for bits in 0..256u32 {
                let c = Context {
                    up: bits & 1 != 0,
                    down: bits & 2 != 0,
                    left: bits & 4 != 0,
                    right: bits & 8 != 0,
                    top_left: bits & 16 != 0,
                    top_right: bits & 32 != 0,
                    bottom_left: bits & 64 != 0,
                    bottom_right: bits & 128 != 0,
                };
                assert!(create_index(kind, c) < 16);
            }
        }
    }

    #[test]
    fn every_tile_has_a_mask() {
        let masks = representative_masks();
        for (tile, mask) in masks.iter().enumerate() {
            assert_eq!(usize::from(CTM[usize::from(*mask)]), tile);
        }
    }

    fn framed() -> image::RgbaImage {
        image::RgbaImage::from_fn(16, 16, |x, y| {
            let edge = x == 0 || y == 0 || x == 15 || y == 15;
            image::Rgba(if edge {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 40]
            })
        })
    }

    #[test]
    fn a_lone_tile_keeps_its_frame_and_a_surrounded_one_loses_it() {
        let base = framed();
        assert_eq!(connected_tile(&base, 0), base);
        let open = connected_tile(&base, 0xFF);
        assert!(open.pixels().all(|p| p.0 == [0, 0, 0, 40]));
    }

    #[test]
    fn an_inside_corner_keeps_its_dot() {
        // Left and up connect, the up-left diagonal does not.
        let tile = connected_tile(&framed(), 0b0100_0001);
        assert_eq!(tile.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(tile.get_pixel(0, 5).0, [0, 0, 0, 40]);
        assert_eq!(tile.get_pixel(5, 0).0, [0, 0, 0, 40]);
        assert_eq!(tile.get_pixel(15, 5).0, [255, 255, 255, 255]);
    }
}
