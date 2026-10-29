//! One table of the fixed facts about each entity kind, built once from the
//! vanilla-derived lookups scattered across the renderer and play code, so a
//! hot path reads a field instead of walking a large `match`, and a modded
//! kind later needs one row rather than a case in every lookup.

use std::sync::LazyLock;

use azalea::registry::builtin::EntityKind;
use azalea_registry::Registry as _;

/// What never changes about an entity kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KindFacts {
    /// `shadowRadius`; 0 casts none.
    pub shadow_radius: f32,
    /// `shadowStrength`.
    pub shadow_strength: f32,
    /// How fast the walk animation follows ground speed.
    pub walk_factor: f32,
    /// Boats and rafts, chest ones included.
    pub boat: bool,
    /// `TamableAnimal` subclasses.
    pub tamable: bool,
}

impl KindFacts {
    fn of(kind: EntityKind) -> Self {
        use EntityKind as K;
        KindFacts {
            shadow_radius: crate::renderer::shadows::radius(kind),
            shadow_strength: crate::renderer::shadows::strength(kind),
            walk_factor: crate::entities::feed::walk_factor(kind),
            boat: crate::play::boat::is_boat(kind),
            tamable: matches!(
                kind,
                K::Wolf | K::Cat | K::Parrot | K::Nautilus | K::ZombieNautilus
            ),
        }
    }
}

static TABLE: LazyLock<Vec<KindFacts>> = LazyLock::new(|| {
    (0u32..)
        .map_while(EntityKind::from_u32)
        .map(KindFacts::of)
        .collect()
});

/// The facts row for `kind`.
pub fn facts(kind: EntityKind) -> &'static KindFacts {
    &TABLE[kind as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_own_row() {
        let mut n = 0;
        while let Some(kind) = EntityKind::from_u32(n) {
            assert_eq!(*facts(kind), KindFacts::of(kind), "{kind:?}");
            n += 1;
        }
        assert_eq!(TABLE.len(), n as usize);
        assert!(facts(EntityKind::OakBoat).boat);
        assert!(facts(EntityKind::Wolf).tamable);
        assert_eq!(facts(EntityKind::Item).shadow_strength, 0.75);
    }
}
