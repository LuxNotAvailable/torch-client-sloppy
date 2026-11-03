//! Network ID to name tables for the built-in registries. A vanilla server
//! numbers entries the way this client does; a modded one adds its own and
//! may renumber the rest, then syncs the order it uses (NeoForge's frozen
//! registry snapshots). Each session starts on vanilla's tables, and a
//! server's sync replaces the ones it sends.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{Arc, LazyLock};

use azalea_registry::Registry;
use azalea_registry::builtin::{
    Attribute, BlockEntityKind, BlockKind, CommandArgumentKind, DataComponentKind, EntityKind,
    Fluid, ItemKind, MenuKind, MobEffect, ParticleKind, SoundEvent,
};
use azalea_registry::identifier::Identifier;

/// A built-in registry whose entries packets name by network ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RegistryId {
    Block,
    Item,
    EntityType,
    BlockEntityType,
    Fluid,
    ParticleType,
    Menu,
    MobEffect,
    SoundEvent,
    DataComponentType,
    Attribute,
    CommandArgumentType,
}

impl RegistryId {
    pub(crate) const ALL: [RegistryId; 12] = [
        RegistryId::Block,
        RegistryId::Item,
        RegistryId::EntityType,
        RegistryId::BlockEntityType,
        RegistryId::Fluid,
        RegistryId::ParticleType,
        RegistryId::Menu,
        RegistryId::MobEffect,
        RegistryId::SoundEvent,
        RegistryId::DataComponentType,
        RegistryId::Attribute,
        RegistryId::CommandArgumentType,
    ];

    /// The registry's own name, as a sync names it.
    pub(crate) fn key(self) -> &'static str {
        match self {
            RegistryId::Block => "minecraft:block",
            RegistryId::Item => "minecraft:item",
            RegistryId::EntityType => "minecraft:entity_type",
            RegistryId::BlockEntityType => "minecraft:block_entity_type",
            RegistryId::Fluid => "minecraft:fluid",
            RegistryId::ParticleType => "minecraft:particle_type",
            RegistryId::Menu => "minecraft:menu",
            RegistryId::MobEffect => "minecraft:mob_effect",
            RegistryId::SoundEvent => "minecraft:sound_event",
            RegistryId::DataComponentType => "minecraft:data_component_type",
            RegistryId::Attribute => "minecraft:attribute",
            RegistryId::CommandArgumentType => "minecraft:command_argument_type",
        }
    }

    pub(crate) fn from_key(key: &Identifier) -> Option<RegistryId> {
        let key = key.to_string();
        RegistryId::ALL.into_iter().find(|id| id.key() == key)
    }

    fn vanilla(self) -> &'static Arc<RegistryTable> {
        static TABLES: LazyLock<Vec<Arc<RegistryTable>>> = LazyLock::new(|| {
            RegistryId::ALL
                .into_iter()
                .map(|id| Arc::new(RegistryTable::new(id.vanilla_names())))
                .collect()
        });
        &TABLES[self as usize]
    }

    fn vanilla_names(self) -> Vec<Identifier> {
        match self {
            RegistryId::Block => names::<BlockKind>(),
            RegistryId::Item => names::<ItemKind>(),
            RegistryId::EntityType => names::<EntityKind>(),
            RegistryId::BlockEntityType => names::<BlockEntityKind>(),
            RegistryId::Fluid => names::<Fluid>(),
            RegistryId::ParticleType => names::<ParticleKind>(),
            RegistryId::Menu => names::<MenuKind>(),
            RegistryId::MobEffect => names::<MobEffect>(),
            RegistryId::SoundEvent => names::<SoundEvent>(),
            RegistryId::DataComponentType => names::<DataComponentKind>(),
            RegistryId::Attribute => names::<Attribute>(),
            RegistryId::CommandArgumentType => names::<CommandArgumentKind>(),
        }
    }
}

/// Every entry of a vanilla registry, in network ID order.
fn names<R: Registry + Display>() -> Vec<Identifier> {
    (0..)
        .map_while(R::from_u32)
        .map(|entry| Identifier::new(entry.to_string()))
        .collect()
}

/// One registry's entries, in network ID order.
pub(crate) struct RegistryTable {
    names: Vec<Identifier>,
    ids: HashMap<Identifier, u32>,
}

impl RegistryTable {
    pub(crate) fn new(names: Vec<Identifier>) -> RegistryTable {
        let ids = names
            .iter()
            .enumerate()
            .map(|(id, name)| (name.clone(), id as u32))
            .collect();
        RegistryTable { names, ids }
    }

    pub(crate) fn name(&self, id: u32) -> Option<&Identifier> {
        self.names.get(id as usize)
    }

    pub(crate) fn id(&self, name: &Identifier) -> Option<u32> {
        self.ids.get(name).copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }
}

/// The tables a session reads: vanilla's, except where the server synced
/// its own.
#[derive(Default, Clone)]
pub(crate) struct Registries {
    synced: HashMap<RegistryId, Arc<RegistryTable>>,
}

impl Registries {
    pub(crate) fn table(&self, registry: RegistryId) -> &RegistryTable {
        self.synced
            .get(&registry)
            .unwrap_or_else(|| registry.vanilla())
    }

    pub(crate) fn name(&self, registry: RegistryId, id: u32) -> Option<&Identifier> {
        self.table(registry).name(id)
    }

    /// Takes the server's order for one registry, in network ID order.
    pub(crate) fn replace(&mut self, registry: RegistryId, names: Vec<Identifier>) {
        self.synced
            .insert(registry, Arc::new(RegistryTable::new(names)));
    }

    pub(crate) fn is_synced(&self, registry: RegistryId) -> bool {
        self.synced.contains_key(&registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_tables_number_entries_the_way_azalea_does() {
        let registries = Registries::default();
        for (kind, id) in [(EntityKind::Cow, RegistryId::EntityType)] {
            let name = registries.name(id, kind.to_u32()).unwrap();
            assert_eq!(name.to_string(), "minecraft:cow");
            assert_eq!(registries.table(id).id(name), Some(kind.to_u32()));
        }
        let stone = BlockKind::Stone.to_u32();
        assert_eq!(
            registries
                .name(RegistryId::Block, stone)
                .unwrap()
                .to_string(),
            "minecraft:stone"
        );
        for id in RegistryId::ALL {
            assert!(registries.table(id).len() > 0, "{}", id.key());
        }
    }

    #[test]
    fn a_synced_table_replaces_only_its_registry() {
        let mut registries = Registries::default();
        let vanilla_items = registries.table(RegistryId::Item).len();
        let mut names = RegistryId::EntityType.vanilla_names();
        names.insert(0, Identifier::new("examplemod:gremlin"));
        registries.replace(RegistryId::EntityType, names);

        assert!(registries.is_synced(RegistryId::EntityType));
        assert_eq!(
            registries
                .name(RegistryId::EntityType, 0)
                .unwrap()
                .to_string(),
            "examplemod:gremlin"
        );
        let cow = registries
            .table(RegistryId::EntityType)
            .id(&Identifier::new("minecraft:cow"));
        assert_eq!(cow, Some(EntityKind::Cow.to_u32() + 1));
        assert_eq!(registries.table(RegistryId::Item).len(), vanilla_items);
        assert!(registries.name(RegistryId::Item, u32::MAX).is_none());
    }

    #[test]
    fn a_registry_is_found_by_the_name_a_sync_uses() {
        for id in RegistryId::ALL {
            assert_eq!(RegistryId::from_key(&Identifier::new(id.key())), Some(id));
        }
        assert_eq!(
            RegistryId::from_key(&Identifier::new("examplemod:thing")),
            None
        );
    }
}
