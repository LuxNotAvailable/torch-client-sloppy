//! Item models: definitions (`model`), runtime dispatch, icons, meshes,
//! potions, recipe index.

pub mod dispatch;
pub mod icons;
pub(crate) mod json;
pub mod mesh;
pub mod modded;
pub mod model;
pub mod potions;
pub mod raster;
pub mod recipe_index;

/// A stack's item id string: vanilla ones bare (`stone`), a mod's
/// namespaced (`create:cogwheel`) when the server synced its item registry;
/// otherwise its stand-in's (`barrier`).
pub fn stack_id(data: &azalea_inventory::ItemStackData) -> &'static str {
    data.modded_kind
        .and_then(|raw| crate::protocol::modded_item(raw.get()))
        .unwrap_or_else(|| data.kind.to_str().trim_start_matches("minecraft:"))
}
