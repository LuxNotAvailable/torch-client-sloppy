//! Block tags as the server sends them (`ClientboundUpdateTags`), so checks
//! follow the server's datapacks rather than a copy of vanilla's lists.

use std::collections::{HashMap, HashSet};

use azalea::block::BlockState;
use azalea_registry::Registry;

type Tags = Option<HashMap<String, HashSet<u32>>>;

#[cfg(not(test))]
static TAGS: std::sync::RwLock<Tags> = std::sync::RwLock::new(None);

// Each test thread has its own tags, so one test's can't leak into another.
#[cfg(test)]
thread_local! {
    static TAGS: std::cell::RefCell<Tags> = const { std::cell::RefCell::new(None) };
}

fn with_tags<R>(f: impl FnOnce(&Tags) -> R) -> R {
    #[cfg(not(test))]
    return f(&TAGS.read().unwrap());
    #[cfg(test)]
    TAGS.with(|t| f(&t.borrow()))
}

/// Replaces the block tags with the `minecraft:block` registry's entries.
pub(crate) fn set(entries: impl IntoIterator<Item = (String, Vec<i32>)>) {
    let tags = entries
        .into_iter()
        .map(|(name, ids)| {
            let name = name.strip_prefix("minecraft:").unwrap_or(&name).to_owned();
            (name, ids.into_iter().map(|id| id as u32).collect())
        })
        .collect();
    #[cfg(not(test))]
    {
        *TAGS.write().unwrap() = Some(tags);
    }
    #[cfg(test)]
    TAGS.with(|t| *t.borrow_mut() = Some(tags));
}

/// Whether the server has sent its block tags.
pub fn loaded() -> bool {
    with_tags(|t| t.is_some())
}

/// Whether the block is in `#minecraft:<tag>`; false before the server sent
/// its tags.
pub fn has(state: BlockState, tag: &str) -> bool {
    let id = state.as_block_kind().to_u32();
    with_tags(|t| {
        t.as_ref()
            .and_then(|tags| tags.get(tag))
            .is_some_and(|ids| ids.contains(&id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use azalea_registry::builtin::BlockKind;

    #[test]
    fn a_block_is_in_the_tags_the_server_lists_it_under() {
        let sand = BlockKind::Sand.to_u32() as i32;
        set([("minecraft:sand".to_owned(), vec![sand])]);
        assert!(has(BlockState::from(BlockKind::Sand), "sand"));
        assert!(!has(BlockState::from(BlockKind::Stone), "sand"));
        assert!(!has(BlockState::from(BlockKind::Sand), "logs"));
    }
}
