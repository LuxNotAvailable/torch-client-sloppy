use std::collections::HashMap;
use std::sync::OnceLock;

use azalea_registry::builtin::EntityKind;
use serde_json::Value;

use crate::entities::geom::{BakedModel, LayerDef, PartState};
use crate::entities::models::humanoid::armor;
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::state::glint_bit;
use crate::entities::{RenderSpec, TexturePath};

pub type ArmorLayers = [fn() -> LayerDef; 4];

pub const HUMANOID: ArmorLayers = [
    armor::humanoid_helmet,
    armor::humanoid_chestplate,
    armor::humanoid_leggings,
    armor::humanoid_boots,
];

pub const HUSK: ArmorLayers = [
    armor::husk_helmet,
    armor::husk_chestplate,
    armor::husk_leggings,
    armor::husk_boots,
];

pub const GIANT: ArmorLayers = [
    armor::giant_helmet,
    armor::giant_chestplate,
    armor::giant_leggings,
    armor::giant_boots,
];

pub const PIGLIN: ArmorLayers = [
    armor::piglin_helmet,
    armor::piglin_chestplate,
    armor::piglin_leggings,
    armor::piglin_boots,
];

pub const ZOMBIE_VILLAGER: ArmorLayers = [
    armor::zombie_villager_helmet,
    armor::zombie_villager_chestplate,
    armor::zombie_villager_leggings,
    armor::zombie_villager_boots,
];

pub const ARMOR_STAND: ArmorLayers = [
    armor::armor_stand_helmet,
    armor::armor_stand_chestplate,
    armor::armor_stand_leggings,
    armor::armor_stand_boots,
];

pub const ARMOR_STAND_SMALL: ArmorLayers = [
    armor::armor_stand_small_helmet,
    armor::armor_stand_small_chestplate,
    armor::armor_stand_small_leggings,
    armor::armor_stand_small_boots,
];

#[allow(clippy::too_many_arguments)]
pub fn register_set(
    registry: &mut Registry,
    kinds: &[EntityKind],
    names: [&'static str; 4],
    layers: ArmorLayers,
    setup: fn(&BakedModel, &mut [PartState], &EntityState),
    visible: [fn(&EntityState) -> bool; 4],
) {
    const TEXTURES: [fn(&EntityState) -> TexturePath; 4] = [
        helmet_texture,
        chestplate_texture,
        leggings_texture,
        boots_texture,
    ];
    const GLINT: [fn(&EntityState) -> bool; 4] = [
        |st| st.extras.armor_glint & glint_bit::HEAD != 0,
        |st| st.extras.armor_glint & glint_bit::CHEST != 0,
        |st| st.extras.armor_glint & glint_bit::LEGS != 0,
        |st| st.extras.armor_glint & glint_bit::FEET != 0,
    ];
    for slot in 0..4 {
        registry.add_many(
            kinds,
            RenderSpec::new(names[slot], layers[slot], TEXTURES[slot], setup)
                .with_visible(visible[slot])
                .with_glint(GLINT[slot]),
        );
    }
    super::elytra::register(registry, kinds);
}

pub(crate) fn equipment_asset(item: Option<&str>) -> &'static str {
    let Some(id) = item else {
        return "iron";
    };
    let id = id.rsplit(':').next().unwrap_or(id);
    if id == "turtle_helmet" {
        return "turtle_scute";
    }
    match id.split('_').next().unwrap_or("") {
        "leather" => "leather",
        "chainmail" => "chainmail",
        "gold" | "golden" => "gold",
        "diamond" => "diamond",
        "netherite" => "netherite",
        "copper" => "copper",
        _ => "iron",
    }
}

struct EquipmentLayer {
    texture: String,
    dye: Option<u32>,
}

struct EquipmentAsset {
    layers: HashMap<String, Vec<EquipmentLayer>>,
    /// Trim material -> palette, for materials matching the armour itself.
    trim_overrides: HashMap<String, String>,
}

/// An armour trim, by pattern and material asset name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Trim {
    pub pattern: &'static str,
    pub material: &'static str,
}

impl Trim {
    pub fn new(pattern: &str, material: &str) -> Trim {
        Trim {
            pattern: intern(pattern),
            material: intern(material),
        }
    }
}

/// Trim names come from the server's registries, so they are interned once.
fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
        OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().unwrap();
    if let Some(&known) = names.get(name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.into());
    names.insert(leaked);
    leaked
}

impl EquipmentAsset {
    fn layers(&self, layer_type: &str) -> &[EquipmentLayer] {
        self.layers.get(layer_type).map_or(&[], Vec::as_slice)
    }
}

pub(crate) const LAYER_HUMANOID: &str = "humanoid";
const LAYER_HUMANOID_LEGGINGS: &str = "humanoid_leggings";
pub(crate) const LAYER_WINGS: &str = "wings";

const LAYER_TYPES: [&str; 3] = [LAYER_HUMANOID, LAYER_HUMANOID_LEGGINGS, LAYER_WINGS];

pub(crate) const STACK_PREFIX: &str = "equipment_stack/";

fn equipment_assets() -> &'static HashMap<String, EquipmentAsset> {
    static ASSETS: OnceLock<HashMap<String, EquipmentAsset>> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let dir = crate::assets_root().join("equipment");
        let mut out = HashMap::new();
        for path in crate::platform::assets::read_dir(&dir) {
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(text) = crate::platform::assets::read_to_string(&path) else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let layers = json.get("layers");
            let read_layer = |name: &str| -> Vec<EquipmentLayer> {
                let Some(entries) = layers.and_then(|l| l.get(name)).and_then(|v| v.as_array())
                else {
                    return Vec::new();
                };
                entries
                    .iter()
                    .filter_map(|entry| {
                        Some(EquipmentLayer {
                            texture: crate::util::datapack::bare_id(entry.get("texture")?)
                                .map(str::to_owned)?,
                            dye: entry.get("dyeable").map(|d| {
                                match d.get("color_when_undyed").and_then(Value::as_i64) {
                                    Some(argb) => argb as u32 & 0x00FF_FFFF,
                                    None => 0x00FF_FFFF,
                                }
                            }),
                        })
                    })
                    .collect()
            };
            let trim_overrides = json
                .get("trim_overrides")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    let material = crate::util::datapack::bare_id(o.get("when")?.get("material")?)?;
                    let palette = crate::util::datapack::bare_id(o.get("palette")?)?;
                    Some((material.to_owned(), palette.to_owned()))
                })
                .collect();
            out.insert(
                id.to_owned(),
                EquipmentAsset {
                    trim_overrides,
                    layers: LAYER_TYPES
                        .iter()
                        .filter_map(|name| {
                            let list = read_layer(name);
                            (!list.is_empty()).then(|| ((*name).to_owned(), list))
                        })
                        .collect(),
                },
            );
        }
        crate::log_info!("equipment", "{} equipment assets loaded", out.len());
        out
    })
}

pub(crate) fn slot_texture(item: Option<&str>, layer_type: &str) -> String {
    trimmed_texture(item, layer_type, None)
}

/// Trims bake into the armour sheet, so a trimmed piece always goes through
/// [`stack`], keyed by pattern and palette.
pub(crate) fn trimmed_texture(item: Option<&str>, layer_type: &str, trim: Option<Trim>) -> String {
    let asset = equipment_asset(item);
    if let Some(trim) = trim.filter(|_| item.is_some()) {
        let palette = equipment_assets()
            .get(asset)
            .and_then(|a| a.trim_overrides.get(trim.material))
            .cloned()
            .unwrap_or_else(|| format!("trim/{}", trim.material));
        return format!(
            "{STACK_PREFIX}{layer_type}/{asset}+{}+{palette}",
            trim.pattern
        );
    }
    let Some(layers) = equipment_assets().get(asset).map(|a| a.layers(layer_type)) else {
        return format!("entity/equipment/{layer_type}/{asset}");
    };
    match layers {
        [only] if only.dye.is_none() => {
            format!("entity/equipment/{layer_type}/{}", only.texture)
        }
        [] => format!("entity/equipment/{layer_type}/{asset}"),
        _ => format!("{STACK_PREFIX}{layer_type}/{asset}"),
    }
}

pub(crate) fn has_layer(item: Option<&str>, layer_type: &str) -> bool {
    let Some(item) = item else { return false };
    equipment_assets()
        .get(equipment_asset(Some(item)))
        .is_some_and(|asset| !asset.layers(layer_type).is_empty())
}

/// Sheet for armour slot `slot` (0 helmet .. 3 boots).
pub(crate) fn armor_slot_texture(slot: usize, item: Option<&str>, trim: Option<Trim>) -> String {
    let layer = if slot == 2 {
        LAYER_HUMANOID_LEGGINGS
    } else {
        LAYER_HUMANOID
    };
    trimmed_texture(item, layer, trim)
}

pub(crate) fn stack(rest: &str) -> Option<image::RgbaImage> {
    let (layer_type, rest) = rest.split_once('/')?;
    let mut parts = rest.split('+');
    let asset = parts.next()?;
    let mut base = layered(layer_type, asset)?;
    if let (Some(pattern), Some(palette)) = (parts.next(), parts.next()) {
        match trim_overlay(layer_type, pattern, palette) {
            Some(trim) if trim.dimensions() == base.dimensions() => over_onto(&mut base, &trim),
            Some(trim) => crate::log_warn!(
                "equipment",
                "trim {pattern} is {:?}, armour is {:?}",
                trim.dimensions(),
                base.dimensions()
            ),
            None => crate::log_warn!(
                "equipment",
                "trim not found: {layer_type}/{pattern}+{palette}"
            ),
        }
    }
    Some(base)
}

/// Recolours a grey trim sheet through `palette`: each grey of the shared
/// base palette maps to the colour at the same index.
fn trim_overlay(layer_type: &str, pattern: &str, palette: &str) -> Option<image::RgbaImage> {
    let open = |path: String| {
        crate::platform::assets::open_image(&crate::assets_root().join(path))
            .ok()
            .map(|img| img.to_rgba8())
    };
    let mut sheet = open(format!("textures/trims/entity/{layer_type}/{pattern}.png"))?;
    let keys = open("textures/palettes/trim_base.png".into())?;
    let colours = open(format!("textures/palettes/{palette}.png"))?;
    let map: HashMap<[u8; 3], [u8; 3]> = keys
        .pixels()
        .zip(colours.pixels())
        .map(|(k, c)| ([k[0], k[1], k[2]], [c[0], c[1], c[2]]))
        .collect();
    for pixel in sheet.pixels_mut() {
        if let Some(c) = map.get(&[pixel[0], pixel[1], pixel[2]]) {
            pixel[0] = c[0];
            pixel[1] = c[1];
            pixel[2] = c[2];
        }
    }
    Some(sheet)
}

fn layered(layer_type: &str, asset: &str) -> Option<image::RgbaImage> {
    let layers = equipment_assets()
        .get(asset)
        .map_or(&[][..], |a| a.layers(layer_type));
    let Some((first, others)) = layers.split_first() else {
        let file = crate::assets_root().join(format!(
            "textures/entity/equipment/{layer_type}/{asset}.png"
        ));
        return crate::platform::assets::open_image(&file)
            .ok()
            .map(|img| img.to_rgba8());
    };

    let sheet = |layer: &EquipmentLayer| {
        let file = crate::assets_root().join(format!(
            "textures/entity/equipment/{layer_type}/{}.png",
            layer.texture
        ));
        crate::platform::assets::open_image(&file)
            .ok()
            .map(|img| img.to_rgba8())
    };

    let mut base = sheet(first)?;
    if let Some(dye) = first.dye {
        tint(&mut base, dye);
    }
    for layer in others {
        let Some(mut over) = sheet(layer) else {
            crate::log_warn!(
                "equipment",
                "layer not found: {layer_type}/{}",
                layer.texture
            );
            continue;
        };
        if over.dimensions() != base.dimensions() {
            crate::log_warn!(
                "equipment",
                "layer {layer_type}/{} is {:?}, base is {:?}",
                layer.texture,
                over.dimensions(),
                base.dimensions()
            );
            continue;
        }
        if let Some(dye) = layer.dye {
            tint(&mut over, dye);
        }
        over_onto(&mut base, &over);
    }
    Some(base)
}

fn tint(image: &mut image::RgbaImage, rgb: u32) {
    let channel = [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF];
    for pixel in image.pixels_mut() {
        for i in 0..3 {
            pixel[i] = ((pixel[i] as u32 * channel[i]) / 255) as u8;
        }
    }
}

fn over_onto(under: &mut image::RgbaImage, over: &image::RgbaImage) {
    for (dst, src) in under.pixels_mut().zip(over.pixels()) {
        let a = src[3] as u32;
        if a == 0 {
            continue;
        }
        if a == 255 {
            *dst = *src;
            continue;
        }
        for i in 0..3 {
            dst[i] = ((src[i] as u32 * a + dst[i] as u32 * (255 - a)) / 255) as u8;
        }
        dst[3] = (a + dst[3] as u32 * (255 - a) / 255).min(255) as u8;
    }
}

fn helmet_texture(st: &EntityState) -> TexturePath {
    armor_slot_texture(0, st.extras.helmet.as_deref(), st.extras.armor_trims[0]).into()
}

fn chestplate_texture(st: &EntityState) -> TexturePath {
    armor_slot_texture(1, st.extras.chestplate.as_deref(), st.extras.armor_trims[1]).into()
}

fn leggings_texture(st: &EntityState) -> TexturePath {
    armor_slot_texture(2, st.extras.leggings.as_deref(), st.extras.armor_trims[2]).into()
}

fn boots_texture(st: &EntityState) -> TexturePath {
    armor_slot_texture(3, st.extras.boots.as_deref(), st.extras.armor_trims[3]).into()
}

pub const FILLED: [fn(&EntityState) -> bool; 4] =
    [has_helmet, has_chestplate, has_leggings, has_boots];

pub const ADULT: [fn(&EntityState) -> bool; 4] =
    [adult_helmet, adult_chestplate, adult_leggings, adult_boots];

pub const BIG_STAND: [fn(&EntityState) -> bool; 4] =
    [big_helmet, big_chestplate, big_leggings, big_boots];

pub const SMALL_STAND: [fn(&EntityState) -> bool; 4] =
    [small_helmet, small_chestplate, small_leggings, small_boots];

fn has_helmet(st: &EntityState) -> bool {
    st.extras.helmet.is_some()
}
fn has_chestplate(st: &EntityState) -> bool {
    st.extras.chestplate.is_some()
}
fn has_leggings(st: &EntityState) -> bool {
    st.extras.leggings.is_some()
}
fn has_boots(st: &EntityState) -> bool {
    st.extras.boots.is_some()
}

fn adult_helmet(st: &EntityState) -> bool {
    !st.extras.is_baby && has_helmet(st)
}
fn adult_chestplate(st: &EntityState) -> bool {
    !st.extras.is_baby && has_chestplate(st)
}
fn adult_leggings(st: &EntityState) -> bool {
    !st.extras.is_baby && has_leggings(st)
}
fn adult_boots(st: &EntityState) -> bool {
    !st.extras.is_baby && has_boots(st)
}

fn big_helmet(st: &EntityState) -> bool {
    !st.extras.small && has_helmet(st)
}
fn big_chestplate(st: &EntityState) -> bool {
    !st.extras.small && has_chestplate(st)
}
fn big_leggings(st: &EntityState) -> bool {
    !st.extras.small && has_leggings(st)
}
fn big_boots(st: &EntityState) -> bool {
    !st.extras.small && has_boots(st)
}

fn small_helmet(st: &EntityState) -> bool {
    st.extras.small && has_helmet(st)
}
fn small_chestplate(st: &EntityState) -> bool {
    st.extras.small && has_chestplate(st)
}
fn small_leggings(st: &EntityState) -> bool {
    st.extras.small && has_leggings(st)
}
fn small_boots(st: &EntityState) -> bool {
    st.extras.small && has_boots(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: [u8; 4]) -> image::RgbaImage {
        image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba))
    }

    #[test]
    fn a_dye_multiplies_rgb_and_leaves_alpha_alone() {
        let mut img = pixel([255, 128, 64, 200]);
        tint(&mut img, 0x00A0_6540);
        assert_eq!(img.get_pixel(0, 0).0, [160, 50, 16, 200]);
    }

    #[test]
    fn a_white_dye_changes_nothing() {
        let mut img = pixel([13, 200, 7, 255]);
        tint(&mut img, 0x00FF_FFFF);
        assert_eq!(img.get_pixel(0, 0).0, [13, 200, 7, 255]);
    }

    #[test]
    fn an_opaque_overlay_pixel_replaces_and_a_clear_one_does_not() {
        let mut under = pixel([10, 20, 30, 255]);
        over_onto(&mut under, &pixel([1, 2, 3, 255]));
        assert_eq!(under.get_pixel(0, 0).0, [1, 2, 3, 255]);

        let mut under = pixel([10, 20, 30, 255]);
        over_onto(&mut under, &pixel([1, 2, 3, 0]));
        assert_eq!(under.get_pixel(0, 0).0, [10, 20, 30, 255]);
    }

    #[test]
    fn a_partly_transparent_overlay_pixel_blends_toward_it() {
        let mut under = pixel([0, 0, 0, 255]);
        over_onto(&mut under, &pixel([200, 100, 50, 128]));
        let got = under.get_pixel(0, 0).0;
        assert_eq!([got[0], got[1], got[2]], [100, 50, 25]);
        assert_eq!(got[3], 255);
    }

    #[test]
    fn only_a_layered_or_dyed_asset_takes_a_stack_name() {
        assert!(!armor_slot_texture(0, Some("iron_helmet"), None).starts_with(STACK_PREFIX));
        assert!(!armor_slot_texture(0, None, None).starts_with(STACK_PREFIX));
        if equipment_assets().contains_key("leather") {
            assert_eq!(
                armor_slot_texture(0, Some("leather_boots"), None),
                format!("{STACK_PREFIX}humanoid/leather")
            );
            assert_eq!(
                armor_slot_texture(2, Some("leather_leggings"), None),
                format!("{STACK_PREFIX}humanoid_leggings/leather")
            );
            assert!(stack("humanoid/leather").is_some());
        }
    }

    #[test]
    fn every_vanilla_armour_material_resolves_to_its_equipment_asset() {
        for (item, asset) in [
            ("minecraft:iron_helmet", "iron"),
            ("golden_boots", "gold"),
            ("chainmail_chestplate", "chainmail"),
            ("netherite_leggings", "netherite"),
            ("copper_helmet", "copper"),
            ("leather_boots", "leather"),
            ("turtle_helmet", "turtle_scute"),
        ] {
            assert_eq!(equipment_asset(Some(item)), asset, "{item}");
        }
        assert_eq!(equipment_asset(None), "iron");
    }

    #[test]
    fn a_trim_on_its_own_material_takes_the_darker_palette_and_bakes_in() {
        let trim = Some(Trim::new("bolt", "gold"));
        let path = armor_slot_texture(1, Some("golden_chestplate"), trim);
        assert_eq!(
            path,
            format!("{STACK_PREFIX}humanoid/gold+bolt+trim/gold_darker")
        );
        let iron = armor_slot_texture(2, Some("iron_leggings"), trim);
        assert_eq!(
            iron,
            format!("{STACK_PREFIX}humanoid_leggings/iron+bolt+trim/gold")
        );

        let plain = layered("humanoid", "gold").expect("gold armour sheet");
        let trimmed = stack("humanoid/gold+bolt+trim/gold_darker").expect("trimmed sheet");
        assert_eq!(plain.dimensions(), trimmed.dimensions());
        assert_ne!(plain, trimmed);
        let grey = |p: &image::Rgba<u8>| p[3] > 0 && p[0] == p[1] && p[1] == p[2];
        let overlay = trim_overlay("humanoid", "bolt", "trim/gold_darker").unwrap();
        assert!(!overlay.pixels().any(grey), "every trim grey is recoloured");
    }
}
