//! Features screen tooltips, one per feature, rated by ban risk. See
//! `setting_tip`.

use crate::gui::setting_tip::{Risk, Tip};

pub const ZOOM: Tip = Tip::risk(
    "Hold the Zoom key (C) to look closer, like a spyglass. The scroll wheel zooms further. Only your view changes; the server never knows.",
    Risk::Safe,
);
pub const BLOCK_INFO: Tip = Tip::risk(
    "A small panel naming the block or mob under your crosshair, with mob health, mining progress, whether your tool can harvest it, and the mod or pack it comes from. It only shows what your game already knows.",
    Risk::Safe,
);
pub const FOOD_INFO: Tip = Tip::risk(
    "Shows the hidden saturation on your hunger bar, previews how much a held food restores, and adds hunger and saturation to food tooltips. Display only.",
    Risk::Safe,
);
pub const EDIT_HUD: Tip = Tip::risk(
    "Move and resize HUD pieces, and turn on extras such as coordinates and facing direction. Display only.",
    Risk::Safe,
);
pub const INVENTORY_SORT: Tip = Tip::risk(
    "Adds small buttons to inventories: Sort, Store Matching Items and Store Everything. Each press makes many item clicks in a moment, faster than a hand could, which some anticheats flag.",
    Risk::Risky,
);
pub const RECIPE_VIEWER: Tip = Tip::risk(
    "A searchable list of every item beside your inventory. Click an item for its recipes, right-click for its uses, A to bookmark. Browsing is display only, but a recipe's \"+\" fills the crafting grid with a quick burst of clicks.",
    Risk::Risky,
);
#[cfg(feature = "click_gui")]
pub const MODULES: Tip = Tip::risk(
    "Cheat modules: aim assist, triggerbot, flight, no fall, ESP and more (also on the Click GUI key). Most send movement or attacks no fair player could, and anticheats are built to catch them. Use only where they are allowed.",
    Risk::Dangerous,
);
