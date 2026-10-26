use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use azalea_protocol::packets::game::c_recipe_book_add::RecipeDisplayEntry;
use azalea_protocol::packets::game::c_recipe_book_settings::RecipeBookSettings;
use azalea_protocol::packets::game::s_recipe_book_change_settings::RecipeBookType;
use azalea_registry::builtin::RecipeBookCategory as Category;

use super::display::ResolvedDisplay;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BookType {
    Crafting,
    Furnace,
    BlastFurnace,
    Smoker,
}

impl BookType {
    pub(crate) fn wire(self) -> RecipeBookType {
        match self {
            Self::Crafting => RecipeBookType::Crafting,
            Self::Furnace => RecipeBookType::Furnace,
            Self::BlastFurnace => RecipeBookType::BlastFurnace,
            Self::Smoker => RecipeBookType::Smoker,
        }
    }

    pub fn search_categories(self) -> &'static [Category] {
        match self {
            Self::Crafting => &[
                Category::CraftingEquipment,
                Category::CraftingBuildingBlocks,
                Category::CraftingMisc,
                Category::CraftingRedstone,
            ],
            Self::Furnace => &[
                Category::FurnaceFood,
                Category::FurnaceBlocks,
                Category::FurnaceMisc,
            ],
            Self::BlastFurnace => &[Category::BlastFurnaceBlocks, Category::BlastFurnaceMisc],
            Self::Smoker => &[Category::SmokerFood],
        }
    }

    pub fn tabs(self) -> &'static [TabInfo] {
        const fn t(tab: Tab, primary: &'static str, secondary: Option<&'static str>) -> TabInfo {
            TabInfo {
                tab,
                primary,
                secondary,
            }
        }
        use Category as C;
        use Tab::{Category as Cat, Search};
        const CRAFTING: &[TabInfo] = &[
            t(Search(BookType::Crafting), "compass", None),
            t(Cat(C::CraftingEquipment), "iron_axe", Some("golden_sword")),
            t(Cat(C::CraftingBuildingBlocks), "bricks", None),
            t(Cat(C::CraftingMisc), "lava_bucket", Some("apple")),
            t(Cat(C::CraftingRedstone), "redstone", None),
        ];
        const FURNACE: &[TabInfo] = &[
            t(Search(BookType::Furnace), "compass", None),
            t(Cat(C::FurnaceFood), "porkchop", None),
            t(Cat(C::FurnaceBlocks), "stone", None),
            t(Cat(C::FurnaceMisc), "lava_bucket", Some("emerald")),
        ];
        const BLAST_FURNACE: &[TabInfo] = &[
            t(Search(BookType::BlastFurnace), "compass", None),
            t(Cat(C::BlastFurnaceBlocks), "redstone_ore", None),
            t(
                Cat(C::BlastFurnaceMisc),
                "iron_shovel",
                Some("golden_leggings"),
            ),
        ];
        const SMOKER: &[TabInfo] = &[
            t(Search(BookType::Smoker), "compass", None),
            t(Cat(C::SmokerFood), "porkchop", None),
        ];
        match self {
            Self::Crafting => CRAFTING,
            Self::Furnace => FURNACE,
            Self::BlastFurnace => BLAST_FURNACE,
            Self::Smoker => SMOKER,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Search(BookType),
    Category(Category),
}

#[derive(Clone, Copy, Debug)]
pub struct TabInfo {
    pub tab: Tab,
    pub primary: &'static str,
    pub secondary: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeSettings {
    pub open: bool,
    pub filtering: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BookSettings([TypeSettings; 4]);

impl BookSettings {
    pub fn get(&self, book: BookType) -> TypeSettings {
        self.0[book as usize]
    }

    pub fn set(&mut self, book: BookType, settings: TypeSettings) {
        self.0[book as usize] = settings;
    }

    pub(crate) fn from_wire(s: &RecipeBookSettings) -> Self {
        let t = |open, filtering| TypeSettings { open, filtering };
        Self([
            t(s.gui_open, s.filtering_craftable),
            t(s.furnace_gui_open, s.furnace_filtering_craftable),
            t(
                s.blast_furnace_gui_open,
                s.blast_furnace_filtering_craftable,
            ),
            t(s.smoker_gui_open, s.smoker_filtering_craftable),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ingredient(pub(crate) Box<[&'static str]>);

impl Ingredient {
    pub fn new(mut items: Vec<&'static str>) -> Self {
        items.sort_unstable();
        items.dedup();
        Self(items.into())
    }

    pub fn accepts(&self, item: &str) -> bool {
        self.0.binary_search(&item).is_ok()
    }
}

#[derive(Debug)]
pub struct RecipeEntry {
    pub id: u32,
    pub group: Option<u32>,
    pub category: Category,
    pub display: ResolvedDisplay,
    pub requirements: Option<Box<[Ingredient]>>,
    pub(crate) raw: Arc<RecipeDisplayEntry>,
}

#[derive(Debug, Default)]
struct Collections {
    all: Vec<Box<[usize]>>,
    by_tab: HashMap<Tab, Box<[usize]>>,
}

#[derive(Clone, Debug, Default)]
pub struct RecipeBook {
    entries: Vec<Arc<RecipeEntry>>,
    index: HashMap<u32, usize>,
    highlight: HashSet<u32>,
    settings: BookSettings,
    collections: Arc<Collections>,
    version: u64,
}

impl RecipeBook {
    pub fn version(&self) -> u64 {
        self.version
    }

    fn touch(&mut self) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        self.version = NEXT.fetch_add(1, Ordering::Relaxed);
    }

    pub fn entries(&self) -> &[Arc<RecipeEntry>] {
        &self.entries
    }

    pub fn entry(&self, index: usize) -> &RecipeEntry {
        &self.entries[index]
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.index.get(&id).copied()
    }

    pub fn collection(&self, collection: usize) -> &[usize] {
        &self.collections.all[collection]
    }

    pub fn tab(&self, tab: Tab) -> &[usize] {
        self.collections.by_tab.get(&tab).map_or(&[], |c| c)
    }

    pub fn will_highlight(&self, id: u32) -> bool {
        self.highlight.contains(&id)
    }

    pub fn settings(&self) -> BookSettings {
        self.settings
    }

    pub(crate) fn set_settings(&mut self, settings: BookSettings) {
        self.settings = settings;
        self.touch();
    }

    pub(crate) fn remove_highlight(&mut self, id: u32) -> bool {
        self.touch();
        self.highlight.remove(&id)
    }

    pub(crate) fn add_highlight(&mut self, id: u32) {
        self.highlight.insert(id);
        self.touch();
    }

    pub(crate) fn clear(&mut self) {
        self.touch();
        self.entries.clear();
        self.index.clear();
        self.highlight.clear();
    }

    pub(crate) fn add(&mut self, entry: Arc<RecipeEntry>) {
        self.touch();
        match self.index.get(&entry.id) {
            Some(&i) => self.entries[i] = entry,
            None => {
                self.index.insert(entry.id, self.entries.len());
                self.entries.push(entry);
            }
        }
    }

    pub(crate) fn replace_resolved(&mut self, entry: Arc<RecipeEntry>) {
        if let Some(&i) = self.index.get(&entry.id)
            && Arc::ptr_eq(&self.entries[i].raw, &entry.raw)
        {
            self.entries[i] = entry;
            self.touch();
        }
    }

    pub(crate) fn remove(&mut self, ids: &[u32]) {
        self.touch();
        let gone: HashSet<u32> = ids.iter().copied().collect();
        self.entries.retain(|e| !gone.contains(&e.id));
        self.highlight.retain(|id| !gone.contains(id));
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
    }

    pub(crate) fn rebuild_collections(&mut self) {
        self.touch();
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_unstable_by_key(|&i| self.entries[i].id);

        let mut all: Vec<Vec<usize>> = Vec::new();
        let mut by_category: HashMap<Category, Vec<usize>> = HashMap::new();
        let mut groups: HashMap<(Category, u32), usize> = HashMap::new();
        for i in order {
            let e = &self.entries[i];
            let slot = match e.group {
                Some(g) => match groups.get(&(e.category, g)) {
                    Some(&c) => {
                        all[c].push(i);
                        continue;
                    }
                    None => {
                        groups.insert((e.category, g), all.len());
                        all.len()
                    }
                },
                None => all.len(),
            };
            all.push(vec![i]);
            by_category.entry(e.category).or_default().push(slot);
        }

        let mut by_tab: HashMap<Tab, Box<[usize]>> = HashMap::new();
        for book in [
            BookType::Crafting,
            BookType::Furnace,
            BookType::BlastFurnace,
            BookType::Smoker,
        ] {
            let search: Vec<usize> = book
                .search_categories()
                .iter()
                .flat_map(|c| by_category.get(c).into_iter().flatten().copied())
                .collect();
            by_tab.insert(Tab::Search(book), search.into());
        }
        for (category, list) in by_category {
            by_tab.insert(Tab::Category(category), list.into());
        }
        self.collections = Arc::new(Collections {
            all: all.into_iter().map(Vec::into_boxed_slice).collect(),
            by_tab,
        });
    }
}
