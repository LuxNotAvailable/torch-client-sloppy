//! Block tags as the server sends them (`ClientboundUpdateTags`), so checks
//! follow the server's datapacks rather than a copy of vanilla's lists.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use azalea::block::BlockState;
use azalea_registry::Registry;

static TAGS: RwLock<Option<HashMap<String, HashSet<u32>>>> = RwLock::new(None);

/// Replaces the block tags with the `minecraft:block` registry's entries.
pub(crate) fn set(entries: impl IntoIterator<Item = (String, Vec<i32>)>) {
    let tags = entries
        .into_iter()
        .map(|(name, ids)| {
            let name = name.strip_prefix("minecraft:").unwrap_or(&name).to_owned();
            (name, ids.into_iter().map(|id| id as u32).collect())
        })
        .collect();
    *TAGS.write().unwrap() = Some(tags);
}

/// Whether the block is in `#minecraft:<tag>`; false before the server sent
/// its tags.
pub fn has(state: BlockState, tag: &str) -> bool {
    let id = state.as_block_kind().to_u32();
    TAGS.read()
        .unwrap()
        .as_ref()
        .and_then(|tags| tags.get(tag))
        .is_some_and(|ids| ids.contains(&id))
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
