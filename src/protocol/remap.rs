use std::sync::OnceLock;

use crate::protocol::version::ProtocolVersion;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IdSpace {
    Attribute,
    Block,
    BlockEntityType,
    BlockState,
    DataComponentType,
    EntityDataSerializer,
    EntityType,
    GameEvent,
    Item,
    Menu,
    MobEffect,
    ParticleType,
    Potion,
    SoundEvent,
    VillagerProfession,
    VillagerType,
}

impl IdSpace {
    pub(crate) const ALL: [IdSpace; 16] = [
        IdSpace::Attribute,
        IdSpace::Block,
        IdSpace::BlockEntityType,
        IdSpace::BlockState,
        IdSpace::DataComponentType,
        IdSpace::EntityDataSerializer,
        IdSpace::EntityType,
        IdSpace::GameEvent,
        IdSpace::Item,
        IdSpace::Menu,
        IdSpace::MobEffect,
        IdSpace::ParticleType,
        IdSpace::Potion,
        IdSpace::SoundEvent,
        IdSpace::VillagerProfession,
        IdSpace::VillagerType,
    ];

    fn key(self) -> &'static str {
        match self {
            IdSpace::Attribute => "attribute",
            IdSpace::Block => "block",
            IdSpace::BlockEntityType => "block_entity_type",
            IdSpace::BlockState => "block_state",
            IdSpace::DataComponentType => "data_component_type",
            IdSpace::EntityDataSerializer => "entity_data_serializer",
            IdSpace::EntityType => "entity_type",
            IdSpace::GameEvent => "game_event",
            IdSpace::Item => "item",
            IdSpace::Menu => "menu",
            IdSpace::MobEffect => "mob_effect",
            IdSpace::ParticleType => "particle_type",
            IdSpace::Potion => "potion",
            IdSpace::SoundEvent => "sound_event",
            IdSpace::VillagerProfession => "villager_profession",
            IdSpace::VillagerType => "villager_type",
        }
    }
}

struct Runs {
    runs: Vec<(i32, Option<i32>)>,
    count: i32,
}

impl Runs {
    fn get(&self, id: i32) -> Option<i32> {
        if id < 0 || id >= self.count {
            return None;
        }
        let slot = self.runs.partition_point(|(start, _)| *start <= id);
        Some(id + self.runs[slot - 1].1?)
    }

    fn is_identity(&self) -> bool {
        self.runs.iter().all(|(_, offset)| *offset == Some(0))
    }
}

pub(crate) struct Remap {
    from: ProtocolVersion,
    to: ProtocolVersion,
    spaces: Vec<Runs>,
}

impl Remap {
    pub(crate) fn map(&self, space: IdSpace, id: i32) -> Option<i32> {
        self.spaces[space as usize].get(id)
    }

    pub(crate) fn is_identity(&self, space: IdSpace) -> bool {
        self.spaces[space as usize].is_identity()
    }

    /// How many ids this side of the remap has in `space`.
    pub(crate) fn count(&self, space: IdSpace) -> i32 {
        self.spaces[space as usize].count
    }

    pub(crate) fn from(&self) -> ProtocolVersion {
        self.from
    }

    pub(crate) fn to(&self) -> ProtocolVersion {
        self.to
    }

    pub(crate) fn v774_to_v775() -> &'static Remap {
        &pair_774().0
    }

    pub(crate) fn v775_to_v774() -> &'static Remap {
        &pair_774().1
    }

    pub(crate) fn v775_to_v777() -> &'static Remap {
        &pair_775().0
    }

    pub(crate) fn v777_to_v775() -> &'static Remap {
        &pair_775().1
    }

    fn parse(json: &str) -> Result<(Remap, Remap), String> {
        let file: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let version = |key: &str| -> Result<ProtocolVersion, String> {
            let name = file[key]["version"]
                .as_str()
                .ok_or_else(|| format!("{key}.version is missing"))?;
            ProtocolVersion::from_name(name).ok_or_else(|| format!("unknown version {name}"))
        };
        let from = version("from")?;
        let to = version("to")?;

        let direction = |field: &str| -> Result<Vec<Runs>, String> {
            let mut out = Vec::with_capacity(IdSpace::ALL.len());
            for space in IdSpace::ALL {
                let key = space.key();
                let table = &file[field][key];
                let count = table["count"]
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .ok_or_else(|| format!("{field}.{key}.count is missing"))?;
                let pairs = table["runs"]
                    .as_array()
                    .ok_or_else(|| format!("{field}.{key}.runs is missing"))?;

                let mut runs = Vec::with_capacity(pairs.len());
                for pair in pairs {
                    let start = pair[0]
                        .as_i64()
                        .and_then(|n| i32::try_from(n).ok())
                        .ok_or_else(|| format!("{field}.{key} holds a malformed start"))?;
                    let offset = match &pair[1] {
                        serde_json::Value::Null => None,
                        value => Some(
                            value
                                .as_i64()
                                .and_then(|n| i32::try_from(n).ok())
                                .ok_or_else(|| format!("{field}.{key} holds a malformed offset"))?,
                        ),
                    };
                    runs.push((start, offset));
                }
                if runs.first().map(|(start, _)| *start) != Some(0) {
                    return Err(format!("{field}.{key} does not start at id 0"));
                }
                if runs.windows(2).any(|w| w[0].0 >= w[1].0) {
                    return Err(format!("{field}.{key} is not sorted"));
                }
                out.push(Runs { runs, count });
            }
            Ok(out)
        };

        Ok((
            Remap {
                from,
                to,
                spaces: direction("forward")?,
            },
            Remap {
                from: to,
                to: from,
                spaces: direction("back")?,
            },
        ))
    }
}

fn pair_774() -> &'static (Remap, Remap) {
    static PAIR: OnceLock<(Remap, Remap)> = OnceLock::new();
    PAIR.get_or_init(|| {
        Remap::parse(include_str!("data/remap-774-775.json"))
            .unwrap_or_else(|e| panic!("embedded 774 -> 775 remap: {e}"))
    })
}

fn pair_775() -> &'static (Remap, Remap) {
    static PAIR: OnceLock<(Remap, Remap)> = OnceLock::new();
    PAIR.get_or_init(|| {
        Remap::parse(include_str!("data/remap-775-777.json"))
            .unwrap_or_else(|e| panic!("embedded 775 -> 777 remap: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_774_identities() {
        let r = Remap::v774_to_v775();
        for space in [
            IdSpace::Attribute,
            IdSpace::BlockEntityType,
            IdSpace::EntityType,
            IdSpace::GameEvent,
            IdSpace::Menu,
            IdSpace::MobEffect,
            IdSpace::Potion,
            IdSpace::VillagerProfession,
            IdSpace::VillagerType,
        ] {
            assert!(
                r.is_identity(space),
                "{space:?} was expected to be identity"
            );
        }
        for space in [
            IdSpace::Block,
            IdSpace::BlockState,
            IdSpace::DataComponentType,
            IdSpace::EntityDataSerializer,
            IdSpace::Item,
            IdSpace::ParticleType,
            IdSpace::SoundEvent,
        ] {
            assert!(!r.is_identity(space), "{space:?} was expected to shift");
        }
    }

    #[test]
    fn the_775_pair_reaches_the_native_version() {
        let forward = Remap::v775_to_v777();
        assert_eq!(forward.from().protocol, 775);
        assert_eq!(
            forward.to().protocol,
            super::super::version::NATIVE.protocol
        );
        assert_eq!(Remap::v777_to_v775().to().protocol, 775);
    }

    #[test]
    fn the_775_block_states_land_on_azaleas_ids() {
        use azalea_registry::Registry as _;
        use azalea_registry::builtin::BlockKind;
        let forward = Remap::v775_to_v777();
        assert_eq!(
            forward.map(IdSpace::BlockState, 1),
            Some(1),
            "stone did not move"
        );
        assert_eq!(
            forward.map(IdSpace::BlockState, 10_771),
            Some(12_514),
            "the first acacia button state, from both versions' blocks.json"
        );
        let poplar = BlockKind::PoplarPlanks.to_u32() as i32;
        assert_eq!(Remap::v777_to_v775().map(IdSpace::Block, poplar), None);
    }

    #[test]
    fn what_777_added_that_775_cannot_receive() {
        let back = Remap::v777_to_v775();
        let missing = |space: IdSpace| {
            (0..back.spaces[space as usize].count)
                .filter(|id| back.map(space, *id).is_none())
                .count()
        };
        assert_eq!(missing(IdSpace::Block), 1286 - 1168);
        assert_eq!(missing(IdSpace::Item), 1658 - 1506);
        assert_eq!(missing(IdSpace::EntityType), 161 - 157);
        assert_eq!(missing(IdSpace::BlockState), 35_723 - 29_873);
        let removed = Remap::v775_to_v777();
        let dropped = |space: IdSpace| {
            (0..removed.spaces[space as usize].count)
                .filter(|id| removed.map(space, *id).is_none())
                .count()
        };
        assert_eq!(
            dropped(IdSpace::BlockEntityType),
            1,
            "beds stopped being block entities"
        );
        assert_eq!(
            dropped(IdSpace::DataComponentType),
            2,
            "map_color and swing_animation"
        );
    }

    #[test]
    fn the_endpoints_are_the_two_versions() {
        let r = Remap::v774_to_v775();
        assert_eq!(r.from().protocol, 774);
        assert_eq!(r.to().protocol, 775);
        assert_eq!(Remap::v775_to_v774().from().protocol, 775);
        assert_eq!(Remap::v775_to_v774().to().protocol, 774);
    }

    #[test]
    fn known_boundaries() {
        let r = Remap::v774_to_v775();
        assert_eq!(r.map(IdSpace::BlockState, 0), Some(0));
        assert_eq!(r.map(IdSpace::BlockState, 1380), Some(1380));
        assert_eq!(r.map(IdSpace::BlockState, 1381), Some(1581));
        assert_eq!(r.map(IdSpace::BlockState, 2122), Some(2323));
        assert_eq!(r.map(IdSpace::BlockState, 10441), Some(10643));
        assert_eq!(r.map(IdSpace::Item, 229), Some(229));
        assert_eq!(r.map(IdSpace::Item, 230), Some(231));
        assert_eq!(r.map(IdSpace::EntityDataSerializer, 21), Some(21));
        assert_eq!(r.map(IdSpace::EntityDataSerializer, 22), Some(23));
        assert_eq!(r.map(IdSpace::EntityDataSerializer, 28), Some(32));
        assert_eq!(r.map(IdSpace::DataComponentType, 40), Some(40));
        assert_eq!(r.map(IdSpace::DataComponentType, 41), Some(42));
    }

    #[test]
    fn an_id_out_of_range_does_not_map() {
        let r = Remap::v774_to_v775();
        assert_eq!(r.map(IdSpace::BlockState, 29670), Some(29872));
        assert_eq!(r.map(IdSpace::BlockState, 29671), None);
        assert_eq!(r.map(IdSpace::BlockState, -1), None);
        assert_eq!(r.map(IdSpace::Item, 1505), None);
    }

    #[test]
    fn every_id_round_trips() {
        let forward = Remap::v774_to_v775();
        let back = Remap::v775_to_v774();
        for space in IdSpace::ALL {
            let count = forward.spaces[space as usize].count;
            for id in 0..count {
                let there = forward
                    .map(space, id)
                    .unwrap_or_else(|| panic!("{space:?} id {id} does not map"));
                assert_eq!(
                    back.map(space, there),
                    Some(id),
                    "{space:?} id {id} -> {there} did not come back"
                );
            }
        }
    }

    #[test]
    fn no_two_ids_share_a_target() {
        let r = Remap::v774_to_v775();
        for space in IdSpace::ALL {
            let count = r.spaces[space as usize].count;
            let mut taken = std::collections::HashMap::new();
            for id in 0..count {
                let there = r.map(space, id).unwrap();
                if let Some(first) = taken.insert(there, id) {
                    panic!("{space:?}: ids {first} and {id} both land on {there}");
                }
            }
        }
    }

    #[test]
    fn sound_events_were_reordered_not_just_inserted() {
        let r = Remap::v774_to_v775();
        assert_eq!(r.map(IdSpace::SoundEvent, 280), Some(291));
        assert_eq!(
            r.map(IdSpace::SoundEvent, 281),
            Some(288),
            "281 lands below 280, which is what makes the map non-monotonic"
        );
        let count = r.spaces[IdSpace::SoundEvent as usize].count;
        let descents = (1..count)
            .filter(|id| r.map(IdSpace::SoundEvent, *id) <= r.map(IdSpace::SoundEvent, id - 1))
            .count();
        assert_eq!(descents, 8);
    }

    #[test]
    fn an_id_775_added_has_nowhere_to_go_back_to() {
        let back = Remap::v775_to_v774();
        assert_eq!(back.spaces[IdSpace::SoundEvent as usize].count, 1902);
        assert_eq!(back.spaces[IdSpace::BlockState as usize].count, 29_873);
        assert_eq!(back.map(IdSpace::BlockState, 29_872), Some(29_670));
        assert_eq!(back.map(IdSpace::BlockState, 29_873), None);
        assert_eq!(back.map(IdSpace::Item, 230), None, "the item 775 inserted");
        assert_eq!(back.map(IdSpace::Item, 231), Some(230));
    }

    #[test]
    fn what_775_added_that_774_cannot_receive() {
        let back = Remap::v775_to_v774();
        let missing = |space: IdSpace| {
            (0..back.spaces[space as usize].count)
                .filter(|id| back.map(space, *id).is_none())
                .count()
        };
        assert_eq!(missing(IdSpace::SoundEvent), 1902 - 1838);
        assert_eq!(missing(IdSpace::Item), 1);
        assert_eq!(missing(IdSpace::DataComponentType), 110 - 104);
        assert_eq!(missing(IdSpace::EntityDataSerializer), 4);
        assert_eq!(missing(IdSpace::BlockState), 29_873 - 29_671);
        assert_eq!(missing(IdSpace::EntityType), 0, "identity has no holes");
    }
}
