//! Each block state's id, properties and the behaviour we read, filled in
//! the first time a state is asked about and kept. azalea's own lookups go
//! through `Box<dyn BlockTrait>`, a heap allocation (and for properties a
//! fresh `HashMap`) on every call; this answers from a table instead.
//!
//! Only reads go through here. Building a changed state (`set_property`,
//! for placement prediction) still uses the boxed block.

use std::collections::HashMap;
use std::sync::OnceLock;

use azalea::block::{BlockState, BlockTrait};

/// What never changes about a block state.
#[derive(Debug)]
pub struct StateInfo {
    /// The block id without its namespace: `oak_stairs`.
    pub id: &'static str,
    /// `BlockBehavior.friction`.
    pub friction: f32,
    /// `BlockBehavior.destroy_time`; negative for unbreakable blocks.
    pub destroy_time: f32,
    /// `BlockBehavior.requires_correct_tool_for_drops`.
    pub needs_tool: bool,
    /// The properties, sorted by name (the order of vanilla's variant keys).
    props: Box<[(&'static str, &'static str)]>,
}

impl StateInfo {
    fn of(state: BlockState) -> StateInfo {
        let block = Box::<dyn BlockTrait>::from(state);
        let behavior = block.behavior();
        let mut props: Vec<_> = block.property_map().into_iter().collect();
        props.sort_unstable();
        StateInfo {
            id: block.id(),
            friction: behavior.friction,
            destroy_time: behavior.destroy_time,
            needs_tool: behavior.requires_correct_tool_for_drops,
            props: props.into_boxed_slice(),
        }
    }

    /// The value of property `name`, if the block has it.
    pub fn get(&self, name: &str) -> Option<&'static str> {
        self.props.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }

    /// Whether property `name` is `"true"`.
    pub fn flag(&self, name: &str) -> bool {
        self.get(name) == Some("true")
    }

    /// The properties, sorted by name.
    pub fn props(&self) -> &[(&'static str, &'static str)] {
        &self.props
    }

    /// The properties as a map, for APIs that take one.
    pub fn map(&self) -> HashMap<&'static str, &'static str> {
        self.props.iter().copied().collect()
    }
}

/// The facts about `state`. A modded state (past vanilla's: a block from a
/// mod this client doesn't have) shares one row, [`MODDED_ID`], with stone's
/// behaviour and no properties.
pub fn info(state: BlockState) -> &'static StateInfo {
    const STATES: usize = BlockState::MAX_STATE as usize + 1;
    static TABLE: OnceLock<Box<[OnceLock<StateInfo>]>> = OnceLock::new();
    static MODDED: OnceLock<StateInfo> = OnceLock::new();
    let table = TABLE.get_or_init(|| (0..STATES).map(|_| OnceLock::new()).collect());
    match table.get(state.id() as usize) {
        Some(slot) => slot.get_or_init(|| StateInfo::of(state)),
        None => MODDED.get_or_init(|| StateInfo {
            id: MODDED_ID,
            props: Box::new([]),
            ..StateInfo::of(stand_in(state))
        }),
    }
}

/// The id [`info`] gives every modded state.
pub const MODDED_ID: &str = "modded";

/// `state` itself when it's vanilla's, else stone's default state: what a
/// modded block stands in as wherever a vanilla block is needed (lighting,
/// collision, caches indexed by state).
pub fn stand_in(state: BlockState) -> BlockState {
    if state.is_vanilla() {
        state
    } else {
        BlockState::from(azalea_registry::builtin::BlockKind::Stone)
    }
}

/// The value of property `name` on `state`.
pub fn property(state: BlockState, name: &str) -> Option<&'static str> {
    info(state).get(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use azalea_registry::builtin::BlockKind;

    #[test]
    fn reads_match_the_boxed_block() {
        for id in (0..=BlockState::MAX_STATE).step_by(97) {
            let state = BlockState::try_from(id).unwrap();
            let block = Box::<dyn BlockTrait>::from(state);
            let facts = info(state);
            assert_eq!(facts.id, block.id());
            assert_eq!(facts.map(), block.property_map(), "{}", block.id());
            for (name, value) in facts.props() {
                assert_eq!(block.get_property(name), Some(*value));
            }
            assert_eq!(facts.friction, block.behavior().friction);
        }
    }

    #[test]
    fn a_modded_state_gets_the_shared_row() {
        let modded = BlockState::from_network(BlockState::MAX_STATE as u32 + 40);
        let facts = info(modded);
        assert_eq!(facts.id, MODDED_ID);
        assert!(facts.props().is_empty());
        assert_eq!(
            facts.friction,
            info(BlockState::from(BlockKind::Stone)).friction
        );
        assert_eq!(stand_in(modded), BlockState::from(BlockKind::Stone));
        let oak = BlockState::from(BlockKind::OakStairs);
        assert_eq!(stand_in(oak), oak);
    }

    #[test]
    fn properties_come_sorted_by_name() {
        let facts = info(BlockState::from(BlockKind::OakStairs));
        let names: Vec<_> = facts.props().iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert_eq!(facts.get("facing"), Some("north"));
        assert!(!facts.flag("waterlogged"));
        assert_eq!(facts.get("power"), None);
    }
}
