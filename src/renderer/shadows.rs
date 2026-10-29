//! Entities' round shadows: `EntityRenderer.extractShadow` and
//! `ShadowFeatureRenderer`. A disc of `misc/shadow.png` laid on the tops of
//! the full blocks under an entity, fading with the camera's distance, with
//! the drop to each block, and in the dark.

use azalea_core::position::BlockPos;
use azalea_registry::builtin::EntityKind;
use bevy::math::Vec3;

/// `shadowRadius`, per entity, from vanilla's renderers; 0 casts none.
pub fn radius(kind: EntityKind) -> f32 {
    use EntityKind as K;
    match kind {
        K::AcaciaBoat => 0.8,
        K::AcaciaChestBoat => 0.8,
        K::Allay => 0.4,
        K::Armadillo => 0.4,
        K::Axolotl => 0.5,
        K::BambooChestRaft => 0.8,
        K::BambooRaft => 0.8,
        K::Bat => 0.25,
        K::Bee => 0.4,
        K::BirchBoat => 0.8,
        K::BirchChestBoat => 0.8,
        K::Blaze => 0.5,
        K::Bogged => 0.5,
        K::Breeze => 0.5,
        K::Camel => 0.7,
        K::CamelHusk => 0.7,
        K::Cat => 0.4,
        K::CaveSpider => 0.56,
        K::CherryBoat => 0.8,
        K::CherryChestBoat => 0.8,
        K::ChestMinecart => 0.7,
        K::Chicken => 0.3,
        K::Cod => 0.3,
        K::CommandBlockMinecart => 0.7,
        K::CopperGolem => 0.5,
        K::Cow => 0.7,
        K::Creaking => 0.6,
        K::Creeper => 0.5,
        K::DarkOakBoat => 0.8,
        K::DarkOakChestBoat => 0.8,
        K::Dolphin => 0.7,
        K::Drowned => 0.5,
        K::Enderman => 0.5,
        K::Endermite => 0.3,
        K::EnderDragon => 0.5,
        K::EndCrystal => 0.5,
        K::Evoker => 0.5,
        K::ExperienceOrb => 0.15,
        K::FallingBlock => 0.5,
        K::Fox => 0.4,
        K::Frog => 0.3,
        K::FurnaceMinecart => 0.7,
        K::Ghast => 1.5,
        K::HappyGhast => 2.0,
        K::Goat => 0.7,
        K::Hoglin => 0.7,
        K::HopperMinecart => 0.7,
        K::Horse => 0.75,
        K::Husk => 0.5,
        K::Illusioner => 0.5,
        K::IronGolem => 0.7,
        K::Item => 0.15,
        K::JungleBoat => 0.8,
        K::JungleChestBoat => 0.8,
        K::Llama => 0.7,
        K::MagmaCube => 0.25,
        K::MangroveBoat => 0.8,
        K::MangroveChestBoat => 0.8,
        K::Minecart => 0.7,
        K::Mooshroom => 0.7,
        K::Nautilus => 0.7,
        K::OakBoat => 0.8,
        K::OakChestBoat => 0.8,
        K::Ocelot => 0.4,
        K::PaleOakBoat => 0.8,
        K::PaleOakChestBoat => 0.8,
        K::Panda => 0.9,
        K::Parched => 0.5,
        K::Parrot => 0.3,
        K::Phantom => 0.75,
        K::Pig => 0.7,
        K::Pillager => 0.5,
        K::PolarBear => 0.9,
        K::PoplarBoat => 0.8,
        K::PoplarChestBoat => 0.8,
        K::Pufferfish => 0.2,
        K::Rabbit => 0.3,
        K::Ravager => 1.1,
        K::Salmon => 0.4,
        K::Sheep => 0.7,
        K::Silverfish => 0.3,
        K::Skeleton => 0.5,
        K::Slime => 0.25,
        K::Sniffer => 1.1,
        K::SnowGolem => 0.5,
        K::SpawnerMinecart => 0.7,
        K::Spider => 0.8,
        K::SpruceBoat => 0.8,
        K::SpruceChestBoat => 0.8,
        K::Stray => 0.5,
        K::Strider => 0.5,
        K::SulfurCube => 0.25,
        K::Tadpole => 0.14,
        K::Tnt => 0.5,
        K::TntMinecart => 0.7,
        K::TraderLlama => 0.7,
        K::TropicalFish => 0.15,
        K::Turtle => 0.7,
        K::Vex => 0.3,
        K::Villager => 0.5,
        K::Vindicator => 0.5,
        K::WanderingTrader => 0.5,
        K::Warden => 0.9,
        K::Witch => 0.5,
        K::Wither => 1.0,
        K::WitherSkeleton => 0.5,
        K::Wolf => 0.5,
        K::Zoglin => 0.7,
        K::Zombie => 0.5,
        K::ZombieNautilus => 0.7,
        K::Guardian => 0.5,
        K::ElderGuardian => 1.2,
        K::ZombieVillager => 0.5,
        K::Giant => 3.0,
        _ => 0.0,
    }
}

/// `shadowStrength`: dropped items and orbs cast fainter shadows.
pub fn strength(kind: EntityKind) -> f32 {
    match kind {
        EntityKind::Item | EntityKind::ExperienceOrb => 0.75,
        _ => 1.0,
    }
}

/// One square of shadow: the block top's corners (x0, y, z0, x1, z1) and its
/// opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x0: f32,
    pub y: f32,
    pub z0: f32,
    pub x1: f32,
    pub z1: f32,
    pub alpha: f32,
}

/// `Lightmap.getBrightness` for the overworld: a light level's brightness.
pub(crate) fn brightness(level: u8) -> f32 {
    let f = level as f32 / 15.0;
    f / (4.0 - 3.0 * f)
}

/// The pieces of a shadow of `radius` cast from `pos`, `dist_sq` from the
/// camera. `solid_top` says whether the block below a cell shows a full top,
/// `light` the light at a cell.
pub fn pieces(
    pos: Vec3,
    radius: f32,
    strength: f32,
    dist_sq: f32,
    solid_top: impl Fn(BlockPos) -> bool,
    light: impl Fn(BlockPos) -> u8,
) -> Vec<Piece> {
    let mut out = Vec::new();
    let radius = radius.min(32.0);
    if radius <= 0.0 {
        return out;
    }
    let pow = 1.0 - dist_sq / 256.0;
    let pow = pow * strength;
    if pow <= 0.0 {
        return out;
    }
    let depth = (pow / 0.5 - 1.0).min(radius);
    let (x0, x1) = ((pos.x - radius).floor() as i32, (pos.x + radius).floor() as i32);
    let (z0, z1) = ((pos.z - radius).floor() as i32, (pos.z + radius).floor() as i32);
    let (y0, y1) = ((pos.y - depth).floor() as i32, pos.y.floor() as i32);
    for z in z0..=z1 {
        for x in x0..=x1 {
            for y in y0..=y1 {
                let cell = BlockPos::new(x, y, z);
                let level = light(cell);
                if level <= 3 || !solid_top(cell.down(1)) {
                    continue;
                }
                let power = pow - (pos.y - y as f32) * 0.5;
                let alpha = (power * 0.5 * brightness(level)).clamp(0.0, 1.0);
                if alpha <= 0.0 {
                    continue;
                }
                out.push(Piece {
                    x0: x as f32,
                    y: y as f32,
                    z0: z as f32,
                    x1: x as f32 + 1.0,
                    z1: z as f32 + 1.0,
                    alpha,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_and_cows_cast_shadows_and_arrows_do_not() {
        assert_eq!(radius(EntityKind::Cow), 0.7);
        assert_eq!(radius(EntityKind::Zombie), 0.5);
        assert_eq!(radius(EntityKind::Arrow), 0.0);
    }

    #[test]
    fn a_shadow_lies_on_the_ground_and_fades_with_distance() {
        let floor = |p: BlockPos| p.y < 64;
        let lit = |_: BlockPos| 15;
        let near = pieces(Vec3::new(0.5, 64.0, 0.5), 0.5, 1.0, 4.0, floor, lit);
        assert!(!near.is_empty());
        // On the ground's top, and (as vanilla, which only looks at the block
        // below each cell) one buried a block lower.
        assert!(near.iter().any(|p| p.y == 64.0));
        assert!(near.iter().all(|p| p.y == 64.0 || p.y == 63.0));
        let far = pieces(Vec3::new(0.5, 64.0, 0.5), 0.5, 1.0, 200.0, floor, lit);
        let top = |ps: &[Piece]| ps.iter().filter(|p| p.y == 64.0).map(|p| p.alpha).fold(0.0, f32::max);
        assert!(top(&far) < top(&near));
        assert!(pieces(Vec3::new(0.5, 64.0, 0.5), 0.5, 1.0, 300.0, floor, lit).is_empty());
        // Too dark: none.
        assert!(pieces(Vec3::new(0.5, 64.0, 0.5), 0.5, 1.0, 4.0, floor, |_| 2).is_empty());
    }
}
