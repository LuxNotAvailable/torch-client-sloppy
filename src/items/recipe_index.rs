//! The recipe viewer's recipes (as Just Enough Items shows them): vanilla's
//! own, read from the game data's `recipe/` folder, with item tags from
//! `tags/item/`. Since 1.21.2 a server sends a client only the recipes it has
//! unlocked, so these are what the client can know for itself.
//!
//! Recipes are indexed by what they make (an item's recipes, R) and by what
//! goes into them (its uses, U). A tag ingredient is resolved to its items,
//! and every slot keeps its alternatives for the viewer to cycle through.
//! Special crafting (fireworks, banner copying, dyeing and the like) has no
//! fixed ingredients and is left out.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::OnceLock;

use serde_json::Value;

use crate::generated_items::item;

/// A recipe viewer category, in the order the viewer lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Crafting,
    Smelting,
    Blasting,
    Smoking,
    Campfire,
    Stonecutting,
    Smithing,
}

impl Category {
    /// The category's title, as its workstation's name.
    pub fn title(self) -> &'static str {
        match self {
            Category::Crafting => "Crafting",
            Category::Smelting => "Smelting",
            Category::Blasting => "Blasting",
            Category::Smoking => "Smoking",
            Category::Campfire => "Campfire Cooking",
            Category::Stonecutting => "Stonecutting",
            Category::Smithing => "Smithing",
        }
    }

    /// The block whose item stands for the category on its tab.
    pub fn icon(self) -> &'static str {
        match self {
            Category::Crafting => "crafting_table",
            Category::Smelting => "furnace",
            Category::Blasting => "blast_furnace",
            Category::Smoking => "smoker",
            Category::Campfire => "campfire",
            Category::Stonecutting => "stonecutter",
            Category::Smithing => "smithing_table",
        }
    }
}

/// One slot's ingredient: the items that fit it (empty for an empty slot).
pub type Slot = Vec<&'static str>;

#[derive(Clone, Debug)]
pub struct Recipe {
    /// The recipe's name, `oak_planks` for `minecraft:oak_planks`.
    pub id: String,
    pub category: Category,
    /// For crafting: the grid's width, 1 to 3, and `inputs` row by row; 0
    /// for shapeless, its ingredients in order.
    pub width: u8,
    /// Crafting: the grid (or the shapeless ingredients). Cooking and
    /// stonecutting: the one input. Smithing: template, base, addition.
    pub inputs: Vec<Slot>,
    pub output: &'static str,
    pub count: u8,
    /// Cooking time in ticks, and the experience it gives.
    pub cooking: Option<(u32, f32)>,
}

impl Recipe {
    pub fn shapeless(&self) -> bool {
        self.category == Category::Crafting && self.width == 0
    }
}

#[derive(Default)]
pub struct RecipeIndex {
    pub recipes: Vec<Recipe>,
    by_output: HashMap<&'static str, Vec<usize>>,
    by_input: HashMap<&'static str, Vec<usize>>,
    /// Item tags, `minecraft:logs`, resolved to their items.
    tags: HashMap<String, Vec<&'static str>>,
}

impl RecipeIndex {
    /// Reads recipes and item tags from a datapack's namespace folder (the
    /// one holding `recipe/` and `tags/`).
    pub fn load(data: &Path) -> RecipeIndex {
        let tags = load_tags(&data.join("tags").join("item"));
        let mut index = RecipeIndex {
            tags,
            ..RecipeIndex::default()
        };
        let mut files = crate::platform::assets::read_dir(data.join("recipe"));
        files.sort();
        for path in files {
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(text) = crate::platform::assets::read_to_string(&path) else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            if let Some(recipe) = parse_recipe(id, &json, &index.tags) {
                index.add(recipe);
            }
        }
        index
    }

    fn add(&mut self, recipe: Recipe) {
        let at = self.recipes.len();
        self.by_output.entry(recipe.output).or_default().push(at);
        let mut inputs: BTreeSet<&'static str> = BTreeSet::new();
        for slot in &recipe.inputs {
            inputs.extend(slot.iter().copied());
        }
        for input in inputs {
            self.by_input.entry(input).or_default().push(at);
        }
        self.recipes.push(recipe);
    }

    /// The recipes that make `item`, by category then file name.
    pub fn recipes_for(&self, item: &str) -> Vec<&Recipe> {
        self.sorted(self.by_output.get(item))
    }

    /// The recipes `item` goes into, by category then file name.
    pub fn uses_of(&self, item: &str) -> Vec<&Recipe> {
        self.sorted(self.by_input.get(item))
    }

    #[cfg(test)]
    pub fn tag(&self, name: &str) -> &[&'static str] {
        self.tags.get(name).map_or(&[], Vec::as_slice)
    }

    /// The item tags `item` is in, for searching by tag.
    pub fn tags_of(&self, item: &str) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .tags
            .iter()
            .filter(|(_, items)| items.contains(&item))
            .map(|(name, _)| name.as_str())
            .collect();
        out.sort_unstable();
        out
    }

    fn sorted(&self, at: Option<&Vec<usize>>) -> Vec<&Recipe> {
        let mut out: Vec<&Recipe> = at
            .map(|at| at.iter().map(|&i| &self.recipes[i]).collect())
            .unwrap_or_default();
        out.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.id.cmp(&b.id)));
        out
    }
}

/// The vanilla recipes, read once from the game data (in the background
/// from startup: `warm`).
pub fn index() -> &'static RecipeIndex {
    static INDEX: OnceLock<RecipeIndex> = OnceLock::new();
    INDEX.get_or_init(|| RecipeIndex::load(crate::datapack_root()))
}

/// Starts reading the recipes on a thread, so the first look is instant.
pub fn warm() {
    std::thread::Builder::new()
        .name("recipe-index".into())
        .spawn(|| {
            let _ = index();
        })
        .ok();
}

fn strip(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// The items an item id names, if it is a known item.
fn known(id: &str) -> Option<&'static str> {
    item(strip(id)).map(|d| d.id)
}

/// Every item tag under `dir`, resolved through the tags it includes.
fn load_tags(dir: &Path) -> HashMap<String, Vec<&'static str>> {
    let mut raw: HashMap<String, Vec<String>> = HashMap::new();
    for path in crate::platform::assets::walk(dir) {
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(rel) = path.strip_prefix(dir) else {
            continue;
        };
        let name = rel.with_extension("").to_string_lossy().replace('\\', "/");
        let Some(text) = crate::platform::assets::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let values = json
            .get("values")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|v| v.as_str().or_else(|| v.get("id")?.as_str()))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        raw.insert(format!("minecraft:{name}"), values);
    }

    fn resolve(
        name: &str,
        raw: &HashMap<String, Vec<String>>,
        depth: u32,
        out: &mut Vec<&'static str>,
    ) {
        let Some(values) = raw.get(name) else {
            return;
        };
        for value in values {
            match value.strip_prefix('#') {
                Some(tag) if depth < 16 => resolve(tag, raw, depth + 1, out),
                Some(_) => {}
                None => {
                    if let Some(id) = known(value)
                        && !out.contains(&id)
                    {
                        out.push(id);
                    }
                }
            }
        }
    }

    raw.keys()
        .map(|name| {
            let mut items = Vec::new();
            resolve(name, &raw, 0, &mut items);
            (name.clone(), items)
        })
        .collect()
}

/// One ingredient: an item, a `#tag`, or a list of either.
fn ingredient(value: &Value, tags: &HashMap<String, Vec<&'static str>>) -> Slot {
    let mut out: Slot = Vec::new();
    let mut add = |id: &str| {
        if let Some(tag) = id.strip_prefix('#') {
            let name = if tag.contains(':') {
                tag.to_owned()
            } else {
                format!("minecraft:{tag}")
            };
            for item in tags.get(&name).into_iter().flatten() {
                if !out.contains(item) {
                    out.push(item);
                }
            }
        } else if let Some(item) = known(id)
            && !out.contains(&item)
        {
            out.push(item);
        }
    };
    match value {
        Value::String(id) => add(id),
        Value::Array(list) => {
            for v in list {
                if let Some(id) = v.as_str() {
                    add(id);
                }
            }
        }
        _ => {}
    }
    out
}

fn result(json: &Value) -> Option<(&'static str, u8)> {
    let result = json.get("result")?;
    let id = result.as_str().or_else(|| result.get("id")?.as_str())?;
    let count = result.get("count").and_then(Value::as_u64).unwrap_or(1);
    Some((known(id)?, count.clamp(1, 99) as u8))
}

fn parse_recipe(
    id: String,
    json: &Value,
    tags: &HashMap<String, Vec<&'static str>>,
) -> Option<Recipe> {
    let kind = strip(json.get("type")?.as_str()?);
    let one = |key: &str| {
        json.get(key)
            .map(|v| ingredient(v, tags))
            .unwrap_or_default()
    };
    let cooking = |category: Category, default_ticks: u32| -> Option<Recipe> {
        let (output, count) = result(json)?;
        let ticks = json
            .get("cookingtime")
            .and_then(Value::as_u64)
            .map_or(default_ticks, |t| t as u32);
        let xp = json
            .get("experience")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32;
        Some(Recipe {
            id: id.clone(),
            category,
            width: 1,
            inputs: vec![one("ingredient")],
            output,
            count,
            cooking: Some((ticks, xp)),
        })
    };
    match kind {
        "crafting_shaped" => {
            let (output, count) = result(json)?;
            let pattern: Vec<&str> = json
                .get("pattern")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .collect();
            let width = pattern
                .iter()
                .map(|row| row.chars().count())
                .max()?
                .clamp(1, 3);
            let key = json.get("key")?.as_object()?;
            let mut inputs = Vec::with_capacity(width * pattern.len());
            for row in &pattern {
                let mut chars: Vec<char> = row.chars().collect();
                chars.resize(width, ' ');
                for c in chars {
                    inputs.push(match key.get(&c.to_string()) {
                        Some(v) => ingredient(v, tags),
                        None => Vec::new(),
                    });
                }
            }
            Some(Recipe {
                id,
                category: Category::Crafting,
                width: width as u8,
                inputs,
                output,
                count,
                cooking: None,
            })
        }
        "crafting_shapeless" => {
            let (output, count) = result(json)?;
            let inputs = json
                .get("ingredients")?
                .as_array()?
                .iter()
                .map(|v| ingredient(v, tags))
                .collect();
            Some(Recipe {
                id,
                category: Category::Crafting,
                width: 0,
                inputs,
                output,
                count,
                cooking: None,
            })
        }
        // A dyed or recoloured copy: the item to change, then the material.
        "crafting_transmute" => {
            let (output, count) = result(json)?;
            Some(Recipe {
                id,
                category: Category::Crafting,
                width: 0,
                inputs: vec![one("input"), one("material")],
                output,
                count,
                cooking: None,
            })
        }
        "smelting" => cooking(Category::Smelting, 200),
        "blasting" => cooking(Category::Blasting, 100),
        "smoking" => cooking(Category::Smoking, 100),
        "campfire_cooking" => cooking(Category::Campfire, 100),
        "stonecutting" => {
            let (output, count) = result(json)?;
            Some(Recipe {
                id,
                category: Category::Stonecutting,
                width: 1,
                inputs: vec![one("ingredient")],
                output,
                count,
                cooking: None,
            })
        }
        "smithing_transform" => {
            let (output, count) = result(json)?;
            Some(Recipe {
                id,
                category: Category::Smithing,
                width: 3,
                inputs: vec![one("template"), one("base"), one("addition")],
                output,
                count,
                cooking: None,
            })
        }
        // A trim keeps the base item; the viewer shows the first one.
        "smithing_trim" => {
            let inputs = vec![one("template"), one("base"), one("addition")];
            let output = *inputs[1].first()?;
            Some(Recipe {
                id,
                category: Category::Smithing,
                width: 3,
                inputs,
                output,
                count: 1,
                cooking: None,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vanilla() -> RecipeIndex {
        RecipeIndex::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("reference/minecraft-26.3/data/minecraft"),
        )
    }

    #[test]
    fn vanilla_recipes_index_by_output_and_input() {
        let index = vanilla();
        assert!(index.recipes.len() > 1600, "only {}", index.recipes.len());

        let planks = index.recipes_for("oak_planks");
        let shapeless = planks
            .iter()
            .find(|r| r.id == "oak_planks")
            .expect("oak_planks");
        assert!(shapeless.shapeless());
        assert_eq!(shapeless.count, 4);
        assert!(
            shapeless.inputs[0].contains(&"oak_log"),
            "{:?}",
            shapeless.inputs
        );

        let boat = &index.recipes_for("acacia_boat")[0];
        assert_eq!(boat.width, 3);
        assert_eq!(boat.inputs.len(), 6);
        assert!(boat.inputs[1].is_empty(), "the gap in the boat's top row");

        let iron = index.recipes_for("iron_ingot");
        assert!(iron.iter().any(|r| r.category == Category::Smelting
            && r.inputs[0].contains(&"raw_iron")
            && r.cooking == Some((200, 0.7))));

        let uses = index.uses_of("raw_iron");
        assert!(uses.iter().any(|r| r.output == "iron_ingot"));
        assert!(uses.windows(2).all(|w| w[0].category <= w[1].category));
    }

    #[test]
    fn tags_resolve_through_the_tags_they_include() {
        let index = vanilla();
        let logs = index.tag("minecraft:logs");
        assert!(logs.contains(&"oak_log") && logs.contains(&"crimson_stem"));
        assert!(index.tags_of("oak_log").contains(&"minecraft:logs"));
    }

    #[test]
    fn smithing_keeps_template_base_and_addition() {
        let index = vanilla();
        let sword = index
            .recipes_for("netherite_sword")
            .into_iter()
            .find(|r| r.category == Category::Smithing)
            .expect("netherite sword smithing");
        assert_eq!(sword.inputs[0], vec!["netherite_upgrade_smithing_template"]);
        assert_eq!(sword.inputs[1], vec!["diamond_sword"]);
        assert!(sword.inputs[2].contains(&"netherite_ingot"));
    }
}
