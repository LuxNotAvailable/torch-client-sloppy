//! Converted mods' items (`items.json` in `.torchmod` packages): how each
//! is used. A server sends a stack's changes to its item's defaults, never
//! the defaults, and a modded item's are in no table the client has: its
//! use animation, ticks in use and food come from here instead.

use std::collections::HashMap;

use parking_lot::RwLock;
use serde_json::Value;

/// How an item is used (Java's `UseAnim`), as far as the client cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Eat,
    Drink,
    Block,
    Bow,
    Spear,
    Crossbow,
    Spyglass,
    TootHorn,
    Brush,
}

impl Use {
    fn parse(s: &str) -> Option<Use> {
        Some(match s {
            "eat" => Use::Eat,
            "drink" => Use::Drink,
            "block" => Use::Block,
            "bow" => Use::Bow,
            "spear" => Use::Spear,
            "crossbow" => Use::Crossbow,
            "spyglass" => Use::Spyglass,
            "toot_horn" => Use::TootHorn,
            "brush" => Use::Brush,
            _ => return None,
        })
    }
}

/// One modded item's use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemUse {
    pub animation: Option<Use>,
    /// Ticks in use before it finishes; 0 for an item with no use.
    pub ticks: i32,
    /// `Some(can_always_eat)` for food.
    pub food: Option<bool>,
    /// Its default `max_damage` (0: not damageable) and stack size.
    pub max_damage: u32,
    pub max_stack: u32,
}

static ITEMS: RwLock<Option<HashMap<String, ItemUse>>> = RwLock::new(None);

/// The packages' `items.json` files, as `(package, text)`. An item two
/// packages both have keeps the first.
pub fn load_packages(packages: &[(String, String)]) {
    let mut out = HashMap::new();
    for (package, text) in packages {
        let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) else {
            crate::log_warn!("mods", "{package}: its items.json doesn't read");
            continue;
        };
        for item in &items {
            let Some(name) = item["name"].as_str() else {
                continue;
            };
            let food = item
                .get("food")
                .map(|f| f["can_always_eat"].as_bool().unwrap_or(false));
            let entry = ItemUse {
                animation: item["use_animation"].as_str().and_then(Use::parse),
                ticks: item["use_ticks"].as_i64().unwrap_or(0) as i32,
                food,
                max_damage: item["max_damage"].as_u64().unwrap_or(0) as u32,
                max_stack: item["max_stack"].as_u64().unwrap_or(64).max(1) as u32,
            };
            out.entry(name.to_owned()).or_insert(entry);
        }
    }
    *ITEMS.write() = Some(out);
}

/// A modded item's use, by its id (`farmersdelight:tomato`).
pub fn get(id: &str) -> Option<ItemUse> {
    ITEMS.read().as_ref()?.get(id).copied()
}

/// The use of a stack that's a modded item, by its server-synced name.
pub fn of_stack(stack: &azalea_inventory::ItemStack) -> Option<ItemUse> {
    let azalea_inventory::ItemStack::Present(data) = stack else {
        return None;
    };
    data.modded_kind?;
    get(super::stack_id(data))
}

/// A modded stack's default max damage, when it has one (the server only
/// sends it changed).
pub fn max_damage(data: &azalea_inventory::ItemStackData) -> Option<i32> {
    data.modded_kind?;
    let u = get(super::stack_id(data))?;
    (u.max_damage > 0).then_some(u.max_damage as i32)
}

/// A modded stack's default stack size.
pub fn max_stack(data: &azalea_inventory::ItemStackData) -> Option<i32> {
    data.modded_kind?;
    Some(get(super::stack_id(data))?.max_stack as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_s_foods_and_drinks_read() {
        load_packages(&[(
            "fd.torchmod".into(),
            r#"[{"name": "fd:tomato", "use_animation": "eat", "use_ticks": 32,
                 "food": {"can_always_eat": false, "nutrition": 1}},
                {"name": "fd:cocoa", "use_animation": "drink", "use_ticks": 32},
                {"name": "fd:knife", "max_damage": 250, "max_stack": 1}]"#
                .into(),
        )]);
        let tomato = get("fd:tomato").unwrap();
        assert_eq!(
            (tomato.animation, tomato.ticks, tomato.food),
            (Some(Use::Eat), 32, Some(false))
        );
        assert_eq!(get("fd:cocoa").unwrap().animation, Some(Use::Drink));
        assert_eq!(get("fd:knife").unwrap().ticks, 0);
        assert_eq!(
            (
                get("fd:knife").unwrap().max_damage,
                get("fd:knife").unwrap().max_stack
            ),
            (250, 1)
        );
        assert_eq!(get("fd:tomato").unwrap().max_stack, 64);
        assert!(get("fd:nothing").is_none());
    }
}
