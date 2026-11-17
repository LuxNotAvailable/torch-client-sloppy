use crate::entities::registry::Registry;

pub mod armor;
pub mod armor_stand;
pub mod elytra;
pub mod held;
pub mod illager;
pub mod piglin;
pub mod skeleton;
pub mod villager;
pub mod zombie;

pub fn register(registry: &mut Registry) {
    zombie::register(registry);
    skeleton::register(registry);
    villager::register(registry);
    illager::register(registry);
    piglin::register(registry);
    armor_stand::register(registry);
    {
        use azalea_registry::builtin::EntityKind as K;
        held::register(
            registry,
            &[
                K::Zombie,
                K::Husk,
                K::Giant,
                K::Skeleton,
                K::WitherSkeleton,
                K::Stray,
                K::Bogged,
                K::Parched,
                K::Piglin,
                K::PiglinBrute,
                K::ZombifiedPiglin,
                K::Pillager,
                K::Vindicator,
                K::Illusioner,
                K::Vex,
            ],
            None,
        );
        held::register(registry, &[K::Drowned], Some(zombie::drowned_root));
    }
}
