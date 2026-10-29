use std::{collections::HashMap, sync::LazyLock};

use compact_str::CompactString;

static STORAGE: LazyLock<Box<[(CompactString, CompactString)]>> = LazyLock::new(|| {
    let json =
        serde_json::from_str::<HashMap<CompactString, CompactString>>(include_str!("en_us.json"))
            .unwrap();
    let mut json = json.into_iter().collect::<Vec<_>>();

    json.sort_by(|a, b| a.0.cmp(&b.0));

    json.into_boxed_slice()
});

/// Strings laid over English: a chosen language, leaked so lookups can hand
/// out plain `&'static str`s (a language is chosen a handful of times).
static OVERRIDE: std::sync::RwLock<Option<&'static HashMap<String, String>>> = std::sync::RwLock::new(None);

/// Lays `strings` over English, or clears the overlay.
pub fn set_override(strings: Option<HashMap<String, String>>) {
    *OVERRIDE.write().unwrap() = strings.map(|s| &*Box::leak(Box::new(s)));
}

pub fn get(key: &str) -> Option<&str> {
    if let Some(map) = *OVERRIDE.read().unwrap()
        && let Some(value) = map.get(key)
    {
        return Some(value.as_str());
    }
    let key = CompactString::from(key);
    let storage = &*STORAGE;
    let index = storage.binary_search_by(|(k, _)| k.cmp(&key));
    index.ok().map(|i| storage[i].1.as_str())
}
