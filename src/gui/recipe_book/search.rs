use std::collections::HashMap;

use crate::client::recipes::RecipeBook;
use crate::session::SlotStack;

#[derive(Default)]
pub(super) struct Terms {
    names: Vec<String>,
    ids: Vec<&'static str>,
}

impl Terms {
    pub(super) fn new(names: Vec<String>, ids: Vec<&'static str>) -> Self {
        Self {
            names: names.into_iter().map(|n| n.to_lowercase()).collect(),
            ids,
        }
    }

    pub(super) fn matches(&self, query: &str) -> bool {
        let named = |q: &str| self.names.iter().any(|n| n.contains(q));
        match query.split_once(':') {
            None => named(query),
            Some((ns, path)) => {
                let (ns, path) = (ns.trim(), path.trim());
                !self.ids.is_empty()
                    && "minecraft".contains(ns)
                    && (self.ids.iter().any(|id| id.contains(path)) || named(path))
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Cache {
    version: Option<u64>,
    terms: HashMap<usize, Terms>,
}

impl Cache {
    pub(super) fn matches(&mut self, book: &RecipeBook, collection: usize, query: &str) -> bool {
        if self.version != Some(book.version()) {
            self.version = Some(book.version());
            self.terms.clear();
        }
        self.terms
            .entry(collection)
            .or_insert_with(|| {
                let stacks = book
                    .collection(collection)
                    .iter()
                    .flat_map(|&e| book.entry(e).display.result.iter());
                let (names, ids) = stacks.map(|s| (hover_name(s), s.item)).unzip();
                Terms::new(names, ids)
            })
            .matches(query)
    }
}

fn hover_name(stack: &SlotStack) -> String {
    crate::gui::tooltip::styled_hover_name(stack)
        .iter()
        .map(|s| s.text.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(names: &[&str], ids: &[&'static str]) -> Terms {
        Terms::new(names.iter().map(|s| s.to_string()).collect(), ids.to_vec())
    }

    #[test]
    fn plain_text_matches_any_name_substring() {
        let t = terms(
            &["Oak Planks", "Birch Planks"],
            &["oak_planks", "birch_planks"],
        );
        assert!(t.matches("birch"));
        assert!(t.matches("k pla"));
        assert!(!t.matches("oak_planks"));
        assert!(!t.matches("spruce"));
    }

    #[test]
    fn namespaced_query_needs_namespace_and_path_or_name() {
        let t = terms(&["Stick"], &["stick"]);
        assert!(t.matches("minecraft:stick"));
        assert!(t.matches("mine : sti"));
        assert!(t.matches(":stick"));
        assert!(t.matches("minecraft:"));
        assert!(!t.matches("other:stick"));
        assert!(!t.matches("minecraft:planks"));
        let t = terms(&["Wooden Sword"], &["wooden_sword"]);
        assert!(t.matches("minecraft:wooden sw"));
    }
}
