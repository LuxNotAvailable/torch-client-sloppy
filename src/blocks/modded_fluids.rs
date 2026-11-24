//! How modded fluids look: their still and flowing sprites and tint.
//! NeoForge keeps these in client code (`IClientFluidTypeExtensions`), so
//! a plugin's `fluid` hook gives them; without one they're guessed from the
//! mod's texture names (`fluid/honey_still.png`, `block/honey_flow.png`).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

/// A fluid's look; sprites are block atlas ids (`create:fluid/honey_still`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FluidLook {
    pub still: String,
    pub flowing: String,
    /// `0xRRGGBB`.
    pub tint: u32,
}

static LOOKS: RwLock<Option<HashMap<String, Option<Arc<FluidLook>>>>> = RwLock::new(None);

/// Forgets every look (packages or plugins changed).
pub fn clear() {
    *LOOKS.write() = None;
}

/// The source fluid a fluid id names: `create:flowing_honey` and
/// `mymod:honey_flowing` are `create:honey` and `mymod:honey`.
pub fn source_of(fluid: &str) -> String {
    let (ns, path) = fluid.split_once(':').unwrap_or(("minecraft", fluid));
    let path = path
        .strip_prefix("flowing_")
        .or_else(|| path.strip_suffix("_flowing"))
        .unwrap_or(path);
    format!("{ns}:{path}")
}

/// A modded fluid's look, worked out once: its plugin's, else a guess.
pub fn look(fluid: &str) -> Option<Arc<FluidLook>> {
    if let Some(hit) = LOOKS.read().as_ref().and_then(|m| m.get(fluid)) {
        return hit.clone();
    }
    let source = source_of(fluid);
    let found = [fluid, source.as_str()]
        .into_iter()
        .find_map(crate::modapi::fluid)
        .map(|l| FluidLook {
            still: l.still,
            flowing: l.flowing,
            tint: l.tint & 0xFF_FFFF,
        })
        .or_else(|| guess(&source, texture_exists))
        .map(Arc::new);
    if found.is_none() {
        crate::log_info!("mods", "{fluid}: no fluid textures found; it won't draw");
    }
    LOOKS
        .write()
        .get_or_insert_with(HashMap::new)
        .insert(fluid.to_owned(), found.clone());
    found
}

fn texture_exists(ns: &str, path: &str) -> bool {
    let Some(assets) = crate::assets_root().parent() else {
        return false;
    };
    let file = assets.join(ns).join("textures").join(format!("{path}.png"));
    crate::platform::assets::read(file).is_some()
}

/// The look a mod's texture names suggest for `source` (`ns:name`).
fn guess(source: &str, exists: impl Fn(&str, &str) -> bool) -> Option<FluidLook> {
    let (ns, name) = source.split_once(':')?;
    for dir in [
        "fluid/",
        "block/fluid/",
        "fluids/",
        "block/fluids/",
        "block/",
    ] {
        for (still, flows) in [
            (
                format!("{dir}{name}_still"),
                [format!("{dir}{name}_flow"), format!("{dir}{name}_flowing")],
            ),
            (
                format!("{dir}{name}/still"),
                [format!("{dir}{name}/flow"), format!("{dir}{name}/flowing")],
            ),
        ] {
            if !exists(ns, &still) {
                continue;
            }
            let flowing = flows
                .into_iter()
                .find(|f| exists(ns, f))
                .unwrap_or_else(|| still.clone());
            return Some(FluidLook {
                still: format!("{ns}:{still}"),
                flowing: format!("{ns}:{flowing}"),
                tint: 0xFF_FFFF,
            });
        }
    }
    None
}

/// The block atlas key of a sprite id: vanilla's block sprites by their
/// bare name (`water_still`), others whole.
pub fn atlas_key(sprite: &str) -> &str {
    sprite
        .strip_prefix("minecraft:block/")
        .or_else(|| sprite.strip_prefix("block/"))
        .unwrap_or(sprite)
}

/// Which of six slots a modded fluid takes in the mesher's fluid grid, so
/// two different fluids side by side don't merge (`terrain::FluidKind`).
pub fn slot(fluid: &str) -> u8 {
    let source = source_of(fluid);
    let hash = source.bytes().fold(0x811C_9DC5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    (hash % 6) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowing_forms_name_their_source() {
        assert_eq!(source_of("create:flowing_honey"), "create:honey");
        assert_eq!(source_of("m:oil_flowing"), "m:oil");
        assert_eq!(source_of("m:oil"), "m:oil");
        assert_eq!(slot("create:flowing_honey"), slot("create:honey"));
    }

    #[test]
    fn a_look_is_guessed_from_texture_names() {
        let files = ["fluid/honey_still", "fluid/honey_flow", "block/oil/still"];
        let exists = |_: &str, p: &str| files.contains(&p);
        assert_eq!(
            guess("create:honey", exists),
            Some(FluidLook {
                still: "create:fluid/honey_still".into(),
                flowing: "create:fluid/honey_flow".into(),
                tint: 0xFF_FFFF,
            })
        );
        // No flowing texture: the still one moves.
        let oil = guess("m:oil", exists).unwrap();
        assert_eq!(oil.flowing, "m:block/oil/still");
        assert_eq!(guess("m:tar", exists), None);
        assert_eq!(atlas_key("minecraft:block/water_still"), "water_still");
        assert_eq!(atlas_key("m:fluid/oil"), "m:fluid/oil");
    }
}
