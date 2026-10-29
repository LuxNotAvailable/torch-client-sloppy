//! Each particle type's recipe: how it starts (sprite, motion, colour, life), per vanilla provider.

#![allow(unused_imports)]

use super::*;

/// `BreakingItemParticle`: a quarter of the item's icon, flung at the
/// given speed with a little scatter, falling under full gravity.
pub(super) fn item_recipe(
    stack: &azalea_inventory::ItemStack,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Recipe> {
    let azalea_inventory::ItemStack::Present(data) = stack else {
        return None;
    };
    let name = data.kind.to_string();
    let [u0, v0, u1, v1] = item_icon_uv(name.strip_prefix("minecraft:").unwrap_or(&name))?;
    let (tw, th) = ((u1 - u0) / 4.0, (v1 - v0) / 4.0);
    let uo = rng.next_f32() * 3.0;
    let vo = rng.next_f32() * 3.0;
    // `getU0` is the right edge of the window and `getU1` the left, so the
    // piece is mirrored, as vanilla draws it.
    let uv = [
        u0 + (uo + 1.0) * tw,
        v0 + vo * th,
        u0 + uo * tw,
        v0 + (vo + 1.0) * th,
    ];
    let mut r = base_recipe(Sprite::Fixed(uv), rng);
    r.velocity = base_velocity(Vec3::ZERO, rng) * 0.1 + arg;
    r.size /= 2.0;
    r.flags = PHYSICS | ITEMS;
    Some(r)
}

pub(super) struct Recipe {
    pub(super) behavior: Behavior,
    pub(super) velocity: Vec3,
    pub(super) gravity: f32,
    pub(super) friction: f32,
    pub(super) lifetime: u32,
    pub(super) size: f32,
    pub(super) tint: [f32; 4],
    pub(super) grow: Grow,
    pub(super) sprite: Sprite,
    pub(super) flags: u8,
}

pub(super) fn base_recipe(sprite: Sprite, rng: &mut JavaRandom) -> Recipe {
    Recipe {
        behavior: Behavior::None,
        velocity: Vec3::ZERO,
        gravity: 1.0,
        friction: 0.98,
        lifetime: (4.0 / (rng.next_f32() * 0.9 + 0.1)) as u32,
        size: 0.1 * (rng.next_f32() * 0.5 + 0.5) * 2.0,
        tint: [1.0, 1.0, 1.0, 1.0],
        grow: Grow::Fixed,
        sprite,
        flags: PHYSICS,
    }
}

pub(super) fn short_lifetime(rng: &mut JavaRandom) -> u32 {
    (8.0 / (rng.next_f32() * 0.8 + 0.2)) as u32
}

pub(super) fn particle_name(particle: &azalea::entity::particle::Particle) -> &'static str {
    use azalea::entity::particle::Particle;
    match particle {
        Particle::AngryVillager => "angry_villager",
        Particle::Block(_) => "block",
        Particle::BlockMarker(_) => "block_marker",
        Particle::Bubble => "bubble",
        Particle::Cloud => "cloud",
        Particle::CopperFireFlame => "copper_fire_flame",
        Particle::Crit => "crit",
        Particle::DamageIndicator => "damage_indicator",
        Particle::DragonBreath(_) => "dragon_breath",
        Particle::DrippingLava => "dripping_lava",
        Particle::FallingLava => "falling_lava",
        Particle::LandingLava => "landing_lava",
        Particle::DrippingWater => "dripping_water",
        Particle::FallingWater => "falling_water",
        Particle::Dust(_) => "dust",
        Particle::DustColorTransition(_) => "dust_color_transition",
        Particle::Effect(_) => "effect",
        Particle::ElderGuardian => "elder_guardian",
        Particle::EnchantedHit => "enchanted_hit",
        Particle::Enchant => "enchant",
        Particle::EndRod => "end_rod",
        Particle::EntityEffect(_) => "entity_effect",
        Particle::ExplosionEmitter => "explosion_emitter",
        Particle::Explosion => "explosion",
        Particle::Gust => "gust",
        Particle::SmallGust => "small_gust",
        Particle::GustEmitterLarge => "gust_emitter_large",
        Particle::GustEmitterSmall => "gust_emitter_small",
        Particle::SonicBoom => "sonic_boom",
        Particle::FallingDust(_) => "falling_dust",
        Particle::Firework => "firework",
        Particle::Fishing => "fishing",
        Particle::Flame => "flame",
        Particle::Infested => "infested",
        Particle::CherryLeaves => "cherry_leaves",
        Particle::PaleOakLeaves => "pale_oak_leaves",
        Particle::RedPoplarLeaves => "red_poplar_leaves",
        Particle::OrangePoplarLeaves => "orange_poplar_leaves",
        Particle::YellowPoplarLeaves => "yellow_poplar_leaves",
        Particle::SulfurBubbles => "sulfur_bubbles",
        Particle::NoxiousGas => "noxious_gas",
        Particle::NoxiousGasCloud => "noxious_gas_cloud",
        Particle::Geyser(_) => "geyser",
        Particle::GeyserBase(_) => "geyser_base",
        Particle::GeyserPoof(_) => "geyser_poof",
        Particle::GeyserPlume(_) => "geyser_plume",
        Particle::SulfurCubeGoo => "sulfur_cube_goo",
        Particle::TintedLeaves(_) => "tinted_leaves",
        Particle::SculkSoul => "sculk_soul",
        Particle::SculkCharge(_) => "sculk_charge",
        Particle::SculkChargePop => "sculk_charge_pop",
        Particle::SoulFireFlame => "soul_fire_flame",
        Particle::Soul => "soul",
        Particle::Flash(_) => "flash",
        Particle::HappyVillager => "happy_villager",
        Particle::Composter => "composter",
        Particle::Heart => "heart",
        Particle::InstantEffect(_) => "instant_effect",
        Particle::Item(_) => "item",
        Particle::Vibration(_) => "vibration",
        Particle::Trail(_) => "trail",
        Particle::PauseMobGrowth => "pause_mob_growth",
        Particle::ResetMobGrowth => "reset_mob_growth",
        Particle::ItemSlime => "item_slime",
        Particle::ItemCobweb => "item_cobweb",
        Particle::ItemSnowball => "item_snowball",
        Particle::LargeSmoke => "large_smoke",
        Particle::Lava => "lava",
        Particle::Mycelium => "mycelium",
        Particle::Note => "note",
        Particle::Poof => "poof",
        Particle::Portal => "portal",
        Particle::Rain => "rain",
        Particle::Smoke => "smoke",
        Particle::WhiteSmoke => "white_smoke",
        Particle::Sneeze => "sneeze",
        Particle::Spit => "spit",
        Particle::SquidInk => "squid_ink",
        Particle::SweepAttack => "sweep_attack",
        Particle::TotemOfUndying => "totem_of_undying",
        Particle::Underwater => "underwater",
        Particle::Splash => "splash",
        Particle::Witch => "witch",
        Particle::BubblePop => "bubble_pop",
        Particle::CurrentDown => "current_down",
        Particle::BubbleColumnUp => "bubble_column_up",
        Particle::Nautilus => "nautilus",
        Particle::Dolphin => "dolphin",
        Particle::CampfireCosySmoke => "campfire_cosy_smoke",
        Particle::CampfireSignalSmoke => "campfire_signal_smoke",
        Particle::DrippingHoney => "dripping_honey",
        Particle::FallingHoney => "falling_honey",
        Particle::LandingHoney => "landing_honey",
        Particle::FallingNectar => "falling_nectar",
        Particle::FallingSporeBlossom => "falling_spore_blossom",
        Particle::Ash => "ash",
        Particle::CrimsonSpore => "crimson_spore",
        Particle::WarpedSpore => "warped_spore",
        Particle::SporeBlossomAir => "spore_blossom_air",
        Particle::DrippingObsidianTear => "dripping_obsidian_tear",
        Particle::FallingObsidianTear => "falling_obsidian_tear",
        Particle::LandingObsidianTear => "landing_obsidian_tear",
        Particle::ReversePortal => "reverse_portal",
        Particle::WhiteAsh => "white_ash",
        Particle::SmallFlame => "small_flame",
        Particle::Snowflake => "snowflake",
        Particle::DrippingDripstoneLava => "dripping_dripstone_lava",
        Particle::FallingDripstoneLava => "falling_dripstone_lava",
        Particle::DrippingDripstoneWater => "dripping_dripstone_water",
        Particle::FallingDripstoneWater => "falling_dripstone_water",
        Particle::GlowSquidInk => "glow_squid_ink",
        Particle::Glow => "glow",
        Particle::WaxOn => "wax_on",
        Particle::WaxOff => "wax_off",
        Particle::ElectricSpark => "electric_spark",
        Particle::Scrape => "scrape",
        Particle::Shriek(_) => "shriek",
        Particle::EggCrack => "egg_crack",
        Particle::DustPlume => "dust_plume",
        Particle::TrialSpawnerDetection => "trial_spawner_detection",
        Particle::TrialSpawnerDetectionOminous => "trial_spawner_detection_ominous",
        Particle::VaultConnection => "vault_connection",
        Particle::DustPillar(_) => "dust_pillar",
        Particle::OminousSpawning => "ominous_spawning",
        Particle::RaidOmen => "raid_omen",
        Particle::TrialOmen => "trial_omen",
        Particle::BlockCrumble(_) => "block_crumble",
        Particle::Firefly => "firefly",
    }
}

pub(super) fn recipe(
    particle: &azalea::entity::particle::Particle,
    atlas: &Atlas,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Recipe> {
    use azalea::entity::particle::Particle as P;

    let name = particle_name(particle);
    // Tried in order, as the arms of one match would be: the first
    // family that knows the particle decides.
    if let Some(recipe) = terrain_fire_and_smoke(particle, atlas, name, arg, rng) {
        return recipe;
    }
    if let Some(recipe) = ported_classes(particle, atlas, name, arg, rng) {
        return recipe;
    }
    if let Some(recipe) = explosions_water_and_dust(particle, atlas, name, arg, rng) {
        return recipe;
    }
    if let Some(recipe) = moods_and_magic(particle, atlas, name, arg, rng) {
        return recipe;
    }
    if let Some(recipe) = glows_and_hits(particle, atlas, name, arg, rng) {
        return recipe;
    }
    generic_recipe(atlas, name, arg, rng)
}

/// Block pieces, flames and smoke. `None` when the particle isn't one of these.
#[allow(clippy::redundant_closure_call)]
fn terrain_fire_and_smoke(
    particle: &azalea::entity::particle::Particle,
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Option<Recipe>> {
    use azalea::entity::particle::Particle as P;
    Some(match particle {
        P::Block(b) | P::BlockMarker(b) | P::FallingDust(b) => {
            (|| -> Option<Recipe> { terrain_recipe(b.block_state, arg, rng) })()
        }

        P::Flame | P::SoulFireFlame | P::CopperFireFlame => {
            (|| -> Option<Recipe> { flame_recipe(atlas, name, arg, rng, 1.0) })()
        }
        P::SmallFlame => (|| -> Option<Recipe> { flame_recipe(atlas, name, arg, rng, 0.5) })(),

        P::Smoke | P::WhiteSmoke => {
            (|| -> Option<Recipe> { ash_smoke_recipe(atlas, name, arg, rng, 1.0, 0.3) })()
        }
        P::LargeSmoke => {
            (|| -> Option<Recipe> { ash_smoke_recipe(atlas, name, arg, rng, 2.5, 0.3) })()
        }
        P::Ash | P::WhiteAsh | P::CrimsonSpore | P::WarpedSpore | P::SporeBlossomAir => {
            (|| -> Option<Recipe> { ash_smoke_recipe(atlas, name, arg, rng, 1.0, 1.0) })()
        }
        P::CampfireCosySmoke => {
            (|| -> Option<Recipe> { campfire_recipe(atlas, name, arg, rng, 80, 0.9) })()
        }
        P::CampfireSignalSmoke => {
            (|| -> Option<Recipe> { campfire_recipe(atlas, name, arg, rng, 280, 0.95) })()
        }
        _ => return None,
    })
}

/// Particles ported from their own vanilla classes. `None` when the particle isn't one of these.
#[allow(clippy::redundant_closure_call)]
fn ported_classes(
    particle: &azalea::entity::particle::Particle,
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Option<Recipe>> {
    use azalea::entity::particle::Particle as P;
    Some(match particle {
        P::Poof => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = arg
                + Vec3::new(
                    (rng.next_f32() - 0.5) * 0.1,
                    (rng.next_f32() - 0.5) * 0.1,
                    (rng.next_f32() - 0.5) * 0.1,
                );
            let grey = rng.next_f32() * 0.3 + 0.7;
            r.tint = [grey, grey, grey, 1.0];
            r.size = 0.1 * (rng.next_f32() * rng.next_f32() * 6.0 + 1.0);
            r.lifetime = (16.0 / (rng.next_f32() * 0.8 + 0.2)) as u32 + 2;
            r.gravity = -0.1;
            r.friction = 0.9;
            r.grow = Grow::In;
            Some(r)
        })(),
        // `AttackSweepParticle`: still, full bright, four ticks through its
        // eight frames, a grey of 0.4 to 1, and half the explosion's size.
        P::SweepAttack => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = Vec3::ZERO;
            r.gravity = 0.0;
            r.lifetime = 4;
            let grey = rng.next_f32() * 0.6 + 0.4;
            r.tint = [grey, grey, grey, 1.0];
            r.size = 1.0 - arg.x * 0.5;
            r.flags = FULLBRIGHT;
            Some(r)
        })(),
        _ if Drip::of(particle).is_some() => {
            (|| -> Option<Recipe> { drip_recipe(atlas, name, Drip::of(particle)?, rng) })()
        }
        P::CherryLeaves => (|| -> Option<Recipe> {
            leaf_recipe(atlas, name, rng, 0.25, 2.0, false, true, 1.0, 0.0)
        })(),
        P::PaleOakLeaves | P::RedPoplarLeaves | P::OrangePoplarLeaves | P::YellowPoplarLeaves => {
            (|| -> Option<Recipe> {
                leaf_recipe(atlas, name, rng, 0.07, 10.0, true, false, 2.0, 0.021)
            })()
        }
        P::Snowflake => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = arg
                + Vec3::new(
                    (rng.next_f32() * 2.0 - 1.0) * 0.05,
                    (rng.next_f32() * 2.0 - 1.0) * 0.05,
                    (rng.next_f32() * 2.0 - 1.0) * 0.05,
                );
            r.gravity = 0.225;
            r.friction = 1.0;
            r.size = 0.1 * (rng.next_f32() * rng.next_f32() + 1.0);
            r.lifetime = (16.0 / (rng.next_f32() * 0.8 + 0.2)) as u32 + 2;
            r.tint = [0.923, 0.964, 0.999, 1.0];
            r.behavior = Behavior::Snowflake;
            Some(r)
        })(),
        // `EmissiveRisingParticle`: a soul, and the sculk's glowing one.
        P::Soul | P::SculkSoul => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = base_velocity(arg, rng) * 0.01 + arg;
            r.gravity = 0.0;
            r.friction = 0.96;
            r.size *= 1.5;
            r.lifetime = (8.0 / (rng.next_f32() * 0.8 + 0.2)) as u32 + 4;
            r.flags = PHYSICS
                | TRANSLUCENT
                | if matches!(particle, P::SculkSoul) {
                    FULLBRIGHT
                } else {
                    0
                };
            Some(r)
        })(),
        P::Gust | P::SmallGust => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.gravity = 0.0;
            r.lifetime = 12 + rng.next_int(4);
            r.size = if matches!(particle, P::SmallGust) {
                0.15
            } else {
                1.0
            };
            r.flags = FULLBRIGHT;
            Some(r)
        })(),
        P::SonicBoom => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.gravity = 0.0;
            r.lifetime = 16;
            let grey = rng.next_f32() * 0.6 + 0.4;
            r.tint = [grey, grey, grey, 1.0];
            r.size = 1.5;
            r.flags = FULLBRIGHT;
            Some(r)
        })(),
        // `ExplodeParticle` with gravity: a llama's spit.
        P::Spit => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = arg
                + Vec3::new(
                    (rng.next_f32() * 2.0 - 1.0) * 0.05,
                    (rng.next_f32() * 2.0 - 1.0) * 0.05,
                    (rng.next_f32() * 2.0 - 1.0) * 0.05,
                );
            r.gravity = 0.5;
            r.friction = 0.9;
            let grey = rng.next_f32() * 0.3 + 0.7;
            r.tint = [grey, grey, grey, 1.0];
            r.size = 0.1 * (rng.next_f32() * rng.next_f32() * 6.0 + 1.0);
            r.lifetime = (16.0 / (rng.next_f32() * 0.8 + 0.2)) as u32 + 2;
            Some(r)
        })(),
        P::SquidInk | P::GlowSquidInk if matches!(particle, P::SquidInk) => {
            (|| -> Option<Recipe> {
                let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
                r.velocity = arg;
                r.gravity = 0.0;
                r.friction = 0.92;
                r.size = 0.5;
                r.tint = [0.0, 0.0, 0.0, 1.0];
                r.lifetime = ((0.5 * 12.0 / (rng.next_f32() * 0.8 + 0.2)) as u32).max(1);
                r.flags = TRANSLUCENT | FADE_OUT;
                r.behavior = Behavior::SquidInk;
                Some(r)
            })()
        }
        // `WakeParticle`: a fishing bobber's trail.
        P::Fishing => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = arg;
            r.gravity = 0.0;
            r.friction = 0.98;
            r.lifetime = (8.0 / (rng.next_f32() * 0.8 + 0.2)) as u32;
            r.flags = PHYSICS;
            Some(r)
        })(),
        P::SculkChargePop => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = arg;
            r.gravity = 0.0;
            r.friction = 0.96;
            r.lifetime = rng.next_int(4) + 6;
            r.flags = TRANSLUCENT | FULLBRIGHT;
            Some(r)
        })(),
        P::PauseMobGrowth | P::ResetMobGrowth => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            let up = if matches!(particle, P::ResetMobGrowth) {
                0.03
            } else {
                -0.03
            };
            r.velocity = arg + Vec3::new(0.0, up, 0.0);
            r.gravity = 0.0;
            r.size *= rng.next_f32() * 0.6 + 0.5;
            r.lifetime = 8;
            r.flags = PHYSICS;
            Some(r)
        })(),
        P::TrialSpawnerDetection | P::TrialSpawnerDetectionOminous => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            let v = base_velocity(Vec3::ZERO, rng);
            r.velocity = Vec3::new(0.0, v.y * 0.9, 0.0) + arg;
            r.gravity = -0.1;
            r.friction = 0.96;
            r.size *= 0.75 * 1.5;
            r.lifetime = ((8.0 / (0.5 + rng.next_f32() * 0.5) * 1.5) as u32).max(1);
            r.grow = Grow::In;
            r.flags = PHYSICS | FULLBRIGHT;
            Some(r)
        })(),
        P::OminousSpawning => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            r.gravity = 0.0;
            r.friction = 1.0;
            r.size = 0.1 * (rng.next_f32() * 0.5 + 0.2) * (3.0 + rng.next_f32() * 2.0);
            r.lifetime = (rng.next_f32() * 5.0) as u32 + 25;
            r.flags = FULLBRIGHT;
            // The spot is set when spawned: see `FlyStraight`.
            r.behavior = Behavior::FlyStraight {
                start: [0.0; 3],
                offset: arg.to_array(),
                from: 0xFF45_5BFE,
                to: 0xFFFF_FFFF,
            };
            Some(r)
        })(),
        P::Firefly => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            let seed = Vec3::new(
                0.5 - rng.next_f64() as f32,
                if rng.next_int(2) == 0 { arg.y } else { -arg.y },
                0.5 - rng.next_f64() as f32,
            );
            r.velocity = base_velocity(seed, rng) * 0.8;
            r.gravity = 0.0;
            r.friction = 0.96;
            r.size *= 0.75 * 1.5;
            r.lifetime = 200 + rng.next_int(101);
            r.tint[3] = 0.0;
            r.flags = PHYSICS | TRANSLUCENT | FULLBRIGHT;
            r.behavior = Behavior::Firefly;
            Some(r)
        })(),
        P::NoxiousGas => (|| -> Option<Recipe> {
            let mut r = ash_smoke_recipe(atlas, name, arg, rng, 3.0, 0.3)?;
            r.tint = [1.0, 1.0, 1.0, 1.0];
            r.gravity = -0.02;
            r.lifetime = ((6.0 / (rng.next_f32() * 0.5 + 0.5) * 3.0) as u32).max(1);
            r.flags |= TRANSLUCENT | FADE_OUT | PHYSICS;
            Some(r)
        })(),
        P::SulfurBubbles => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            r.velocity = Vec3::new(
                arg.x * 0.2 + (rng.next_f32() * 2.0 - 1.0) * 0.02,
                0.0,
                arg.y * 0.2 + (rng.next_f32() * 2.0 - 1.0) * 0.02,
            );
            r.gravity = -0.04;
            r.friction = 0.85;
            let size_start = 0.02 + 0.02 * rng.next_f32();
            r.size = size_start;
            r.lifetime = 20 * 60;
            r.behavior = Behavior::Bubble {
                y_start: 0.0,
                y_end: 0.0,
                y_prev: f32::MIN,
                size_start,
            };
            Some(r)
        })(),
        // `ElderGuardianParticle`: the face, drawn over the screen.
        P::ElderGuardian => (|| -> Option<Recipe> {
            crate::play::event_particles::elder_guardian();
            None
        })(),
        _ => return None,
    })
}

/// Explosions, water, bubbles and dust. `None` when the particle isn't one of these.
#[allow(clippy::redundant_closure_call)]
fn explosions_water_and_dust(
    particle: &azalea::entity::particle::Particle,
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Option<Recipe>> {
    use azalea::entity::particle::Particle as P;
    Some(match particle {
        P::Explosion => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = Vec3::ZERO;
            r.gravity = 0.0;
            r.lifetime = 6 + rng.next_int(4);
            let grey = rng.next_f32() * 0.6 + 0.4;
            r.tint = [grey, grey, grey, 1.0];
            r.size = 2.0 * (1.0 - arg.x.clamp(0.0, 1.0) * 0.5);
            r.flags = FULLBRIGHT;
            Some(r)
        })(),

        P::Splash | P::Rain => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            r.gravity = if matches!(particle, P::Splash) {
                0.04
            } else {
                0.06
            };
            r.velocity = Vec3::new(arg.x, rng.next_f32() * 0.2 + 0.1, arg.z);
            r.size = 0.01;
            r.lifetime = short_lifetime(rng);
            Some(r)
        })(),
        P::Bubble | P::BubblePop | P::CurrentDown | P::BubbleColumnUp | P::Underwater => {
            (|| -> Option<Recipe> {
                let uv = random_frame(atlas, name, rng)?;
                let mut r = base_recipe(Sprite::Fixed(uv), rng);
                r.size = 0.02 * (rng.next_f32() * 0.6 + 0.2);
                r.velocity = arg * 0.2
                    + Vec3::new(
                        (rng.next_f32() - 0.5) * 0.04,
                        (rng.next_f32() - 0.5) * 0.04,
                        (rng.next_f32() - 0.5) * 0.04,
                    );
                r.gravity = -0.05;
                r.friction = 0.85;
                r.lifetime = short_lifetime(rng);
                Some(r)
            })()
        }

        P::Dust(dust) => (|| -> Option<Recipe> {
            let color = [
                dust.color.red() as f32 / 255.0,
                dust.color.green() as f32 / 255.0,
                dust.color.blue() as f32 / 255.0,
            ];
            dust_recipe(atlas, name, arg, rng, color, dust.scale)
        })(),
        P::DustColorTransition(dust) => (|| -> Option<Recipe> {
            let color = [
                dust.from.red() as f32 / 255.0,
                dust.from.green() as f32 / 255.0,
                dust.from.blue() as f32 / 255.0,
            ];
            dust_recipe(atlas, name, arg, rng, color, dust.scale)
        })(),
        P::DustPlume => {
            (|| -> Option<Recipe> { dust_recipe(atlas, name, arg, rng, [0.75, 0.65, 0.5], 1.0) })()
        }
        _ => return None,
    })
}

/// Moods, notes, portals, spells and fireworks. `None` when the particle isn't one of these.
#[allow(clippy::redundant_closure_call)]
fn moods_and_magic(
    particle: &azalea::entity::particle::Particle,
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Option<Recipe>> {
    use azalea::entity::particle::Particle as P;
    Some(match particle {
        P::HappyVillager | P::Composter | P::EggCrack => {
            (|| -> Option<Recipe> { suspended_recipe(atlas, name, arg, rng, [1.0, 1.0, 1.0, 1.0]) })(
            )
        }
        P::Mycelium => (|| -> Option<Recipe> {
            let grey = rng.next_f32() * 0.1 + 0.2;
            suspended_recipe(atlas, name, arg, rng, [grey, grey, grey, 1.0])
        })(),
        P::Dolphin => (|| -> Option<Recipe> {
            let alpha = 1.0 - rng.next_f32() * 0.7;
            suspended_recipe(atlas, name, arg, rng, [0.3, 0.5, 1.0, alpha])
        })(),

        P::Heart | P::AngryVillager => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            r.velocity = arg * 0.01 + Vec3::Y * 0.1;
            r.gravity = 0.0;
            r.friction = 0.86;
            r.lifetime = 16;
            r.size *= 1.5;
            r.grow = Grow::In;
            r.flags = 0;
            Some(r)
        })(),
        P::Note => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            let hue = arg.x;
            let channel = |offset: f32| {
                (((hue + offset) * std::f32::consts::TAU).sin() * 0.65 + 0.35).max(0.0)
            };
            r.tint = [channel(0.0), channel(1.0 / 3.0), channel(2.0 / 3.0), 1.0];
            r.velocity = Vec3::new(arg.x * 0.01, arg.y * 0.01 + 0.2, arg.z * 0.01);
            r.gravity = 0.0;
            r.friction = 0.66;
            r.lifetime = 6;
            r.size *= 1.5;
            r.grow = Grow::In;
            r.flags = 0;
            Some(r)
        })(),

        P::Portal | P::ReversePortal => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            let br = rng.next_f32() * 0.6 + 0.4;
            r.tint = [0.9 * br, 0.3 * br, br, 1.0];
            r.size = 0.1 * (rng.next_f32() * 0.2 + 0.5);
            r.lifetime = (rng.next_f32() * 10.0) as u32 + 40;
            r.velocity = arg * 0.05;
            r.gravity = 0.0;
            r.friction = 0.93;
            r.grow = Grow::Portal;
            r.flags = FULLBRIGHT;
            Some(r)
        })(),
        P::Enchant | P::Nautilus | P::VaultConnection => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            let br = rng.next_f32() * 0.6 + 0.4;
            r.tint = [0.9 * br, 0.9 * br, br, 1.0];
            r.size = 0.1 * (rng.next_f32() * 0.5 + 0.2);
            r.lifetime = (rng.next_f32() * 10.0) as u32 + 30;
            r.velocity = -arg * 0.03;
            r.gravity = 0.0;
            r.friction = 0.95;
            r.grow = Grow::Portal;
            r.flags = FULLBRIGHT;
            Some(r)
        })(),

        P::Lava => (|| -> Option<Recipe> {
            let uv = random_frame(atlas, name, rng)?;
            let mut r = base_recipe(Sprite::Fixed(uv), rng);
            r.velocity = Vec3::new(
                (rng.next_f32() - 0.5) * 0.16,
                rng.next_f32() * 0.4 + 0.05,
                (rng.next_f32() - 0.5) * 0.16,
            );
            r.gravity = 0.75;
            r.friction = 0.999;
            r.size *= rng.next_f32() * 2.0 + 0.2;
            r.lifetime = (16.0 / (rng.next_f32() * 0.8 + 0.2)) as u32;
            r.grow = Grow::Lava;
            r.flags = PHYSICS | FULLBRIGHT;
            Some(r)
        })(),

        P::Cloud | P::Sneeze => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
            r.velocity = arg + Vec3::splat(0.1) * (rng.next_f32() - 0.5);
            let grey = 1.0 - rng.next_f32() * 0.3;
            r.tint = if matches!(particle, P::Sneeze) {
                [0.22, 1.0, 0.53, 0.4]
            } else {
                [grey, grey, grey, 1.0]
            };
            r.size *= 1.875;
            r.friction = 0.96;
            r.gravity = 0.0;
            r.lifetime = (((8.0 / (rng.next_f32() * 0.8 + 0.3)) * 2.5) as u32).max(1);
            r.grow = Grow::In;
            r.flags = TRANSLUCENT;
            Some(r)
        })(),

        P::Effect(_) | P::InstantEffect(_) | P::Infested | P::RaidOmen | P::TrialOmen => {
            (|| -> Option<Recipe> { spell_recipe(atlas, name, arg, rng, [1.0, 1.0, 1.0, 1.0]) })()
        }
        P::EntityEffect(color) => (|| -> Option<Recipe> {
            spell_recipe(
                atlas,
                name,
                arg,
                rng,
                [
                    color.color.red() as f32 / 255.0,
                    color.color.green() as f32 / 255.0,
                    color.color.blue() as f32 / 255.0,
                    1.0,
                ],
            )
        })(),
        P::Witch => (|| -> Option<Recipe> {
            let br = rng.next_f32() * 0.5 + 0.35;
            spell_recipe(atlas, name, arg, rng, [br, 0.0, br, 1.0])
        })(),

        P::EndRod => (|| -> Option<Recipe> {
            let lifetime = 60 + rng.next_int(12);
            animated_recipe(atlas, name, rng, arg, 0.0125, lifetime, 0.75)
        })(),
        P::TotemOfUndying => (|| -> Option<Recipe> {
            let lifetime = 60 + rng.next_int(12);
            let mut r = animated_recipe(atlas, name, rng, arg, 1.25, lifetime, 0.75)?;
            r.friction = 0.6;
            let v = rng.next_f32();
            r.tint = if rng.next_int(4) == 0 {
                [0.6 + v * 0.2, 0.6 + v * 0.3, v * 0.2, 1.0]
            } else {
                [0.1 + v * 0.2, 0.4 + v * 0.3, v * 0.2, 1.0]
            };
            Some(r)
        })(),
        P::Firework => (|| -> Option<Recipe> {
            let lifetime = 20 + rng.next_int(20);
            animated_recipe(atlas, name, rng, arg, 0.1, lifetime, 1.0)
        })(),
        _ => return None,
    })
}

/// Glows, wax, sparks, crits and items. `None` when the particle isn't one of these.
#[allow(clippy::redundant_closure_call)]
fn glows_and_hits(
    particle: &azalea::entity::particle::Particle,
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Option<Recipe>> {
    use azalea::entity::particle::Particle as P;
    Some(match particle {
        P::Glow | P::GlowSquidInk | P::WaxOn | P::WaxOff | P::ElectricSpark | P::Scrape => {
            (|| -> Option<Recipe> {
                let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
                r.velocity = arg * 0.25;
                r.gravity = 0.0;
                r.friction = 0.96;
                r.size *= 0.75;
                r.lifetime = match particle {
                    P::ElectricSpark => rng.next_int(2) + 2,
                    P::Glow | P::GlowSquidInk => short_lifetime(rng),
                    _ => rng.next_int(30) + 10,
                };
                r.tint = match particle {
                    P::WaxOn => [0.91, 0.55, 0.08, 1.0],
                    P::WaxOff | P::ElectricSpark => [1.0, 0.9, 1.0, 1.0],
                    P::Scrape => [0.29, 0.58, 0.51, 1.0],
                    _ if rng.next_int(2) == 0 => [0.6, 1.0, 0.8, 1.0],
                    _ => [0.08, 0.4, 0.4, 1.0],
                };
                r.flags = FULLBRIGHT;
                Some(r)
            })()
        }

        P::Crit | P::EnchantedHit | P::DamageIndicator => (|| -> Option<Recipe> {
            let mut r = base_recipe(Sprite::Fixed(atlas.crit?), rng);
            let grey = rng.next_f32() * 0.3 + 0.6;
            r.tint = [grey, grey, grey, 1.0];
            if matches!(particle, P::EnchantedHit) {
                r.tint[0] *= 0.3;
                r.tint[1] *= 0.8;
            }
            r.velocity = arg * 0.4;
            r.gravity = 0.5;
            r.friction = 0.7;
            r.size *= 0.75;
            r.lifetime = ((6.0 / (rng.next_f32() * 0.8 + 0.6)) as u32).max(1);
            r.grow = Grow::In;
            r.flags = CRIT_DECAY;
            Some(r)
        })(),

        P::Item(item) => (|| -> Option<Recipe> { item_recipe(&item.item, arg, rng) })(),
        P::ItemSlime | P::ItemSnowball | P::ItemCobweb => (|| -> Option<Recipe> { None })(),
        P::GustEmitterLarge | P::GustEmitterSmall => (|| -> Option<Recipe> { None })(),
        _ => return None,
    })
}

pub(super) fn terrain_recipe(
    state: azalea::block::BlockState,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Recipe> {
    let tile = particle_uv(state)?;
    let mut r = base_recipe(Sprite::Fixed(random_sub_uv(tile, rng)), rng);
    r.velocity = arg * 0.1 + base_velocity(Vec3::ZERO, rng);
    r.tint = [0.6, 0.6, 0.6, 1.0];
    r.size /= 2.0;
    r.flags = PHYSICS | TERRAIN;
    Some(r)
}

pub(super) fn flame_recipe(
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
    scale: f32,
) -> Option<Recipe> {
    let uv = random_frame(atlas, name, rng)?;
    let mut r = base_recipe(Sprite::Fixed(uv), rng);
    r.velocity = arg;
    r.gravity = 0.0;
    r.friction = 0.96;
    r.size *= scale;
    r.lifetime = (8.0 / (rng.next_f32() * 0.8 + 0.2)) as u32 + 4;
    r.grow = Grow::Flame;
    r.flags = FULLBRIGHT;
    Some(r)
}

pub(super) fn ash_smoke_recipe(
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
    scale: f32,
    grey_range: f32,
) -> Option<Recipe> {
    let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
    r.velocity = arg
        + Vec3::new(
            (rng.next_f32() - 0.5) * 0.1,
            (rng.next_f32() - 0.5) * 0.1,
            (rng.next_f32() - 0.5) * 0.1,
        );
    let grey = rng.next_f32() * grey_range;
    r.tint = [grey, grey, grey, 1.0];
    r.size *= 0.75 * scale;
    r.gravity = -0.1;
    r.friction = 0.96;
    r.lifetime = (((8.0 / (rng.next_f32() * 0.8 + 0.2)) * scale) as u32).max(1);
    r.grow = Grow::In;
    Some(r)
}

pub(super) fn campfire_recipe(
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
    base_lifetime: u32,
    alpha: f32,
) -> Option<Recipe> {
    let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
    r.velocity = arg;
    r.size *= 3.0;
    r.gravity = 3.0e-6 / 0.04;
    r.friction = 0.96;
    r.lifetime = rng.next_int(50) + base_lifetime;
    r.tint = [1.0, 1.0, 1.0, alpha];
    r.flags = PHYSICS | TRANSLUCENT;
    Some(r)
}

pub(super) fn dust_recipe(
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
    color: [f32; 3],
    scale: f32,
) -> Option<Recipe> {
    let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
    let scale = scale.clamp(0.01, 4.0);
    let base = rng.next_f32() * 0.4 + 0.6;
    r.tint = [
        (rng.next_f32() * 0.2 + 0.8) * color[0] * base,
        (rng.next_f32() * 0.2 + 0.8) * color[1] * base,
        (rng.next_f32() * 0.2 + 0.8) * color[2] * base,
        1.0,
    ];
    r.velocity = arg * 0.1;
    r.size *= 0.75 * scale;
    r.gravity = 0.0;
    r.friction = 0.96;
    r.lifetime = ((short_lifetime(rng) as f32 * scale) as u32).max(1);
    r.grow = Grow::In;
    Some(r)
}

pub(super) fn suspended_recipe(
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
    tint: [f32; 4],
) -> Option<Recipe> {
    let uv = random_frame(atlas, name, rng)?;
    let mut r = base_recipe(Sprite::Fixed(uv), rng);
    r.tint = tint;
    r.velocity = arg * 0.02;
    r.size = 0.02 * (rng.next_f32() * 0.6 + 0.5);
    r.gravity = 0.0;
    r.friction = 0.99;
    r.lifetime = (20.0 / (rng.next_f32() * 0.8 + 0.2)) as u32;
    r.flags = 0;
    Some(r)
}

pub(super) fn spell_recipe(
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
    tint: [f32; 4],
) -> Option<Recipe> {
    let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
    r.tint = tint;
    let drift_x = if arg.x == 0.0 {
        0.5 - rng.next_f32()
    } else {
        arg.x
    };
    let drift_z = if arg.z == 0.0 {
        0.5 - rng.next_f32()
    } else {
        arg.z
    };
    r.velocity = Vec3::new(drift_x * 0.1, arg.y * 0.2, drift_z * 0.1);
    r.size *= 0.75;
    r.gravity = -0.1;
    r.friction = 0.96;
    r.lifetime = short_lifetime(rng);
    r.flags = TRANSLUCENT;
    Some(r)
}

pub(super) fn animated_recipe(
    atlas: &Atlas,
    name: &str,
    rng: &mut JavaRandom,
    arg: Vec3,
    gravity: f32,
    lifetime: u32,
    scale: f32,
) -> Option<Recipe> {
    let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
    r.velocity = arg;
    r.gravity = gravity;
    r.friction = 0.91;
    r.size *= scale;
    r.lifetime = lifetime.max(1);
    r.flags = TRANSLUCENT | FULLBRIGHT | FADE_OUT;
    Some(r)
}

/// `DripParticle` and its providers: colour, gravity (per tick, as vanilla
/// subtracts it whole) and lifetime per kind.
pub(super) fn drip_recipe(
    atlas: &Atlas,
    name: &str,
    drip: Drip,
    rng: &mut JavaRandom,
) -> Option<Recipe> {
    let mut r = base_recipe(Sprite::Animated(atlas.sheet_index(name)?), rng);
    let ranged = |rng: &mut JavaRandom, n: f32| (n / (rng.next_f32() * 0.8 + 0.2)) as u32;
    let mut gravity = 0.06;
    let (color, lifetime) = match drip {
        Drip::WaterHang | Drip::DripstoneWaterHang => {
            gravity *= 0.02;
            (Drip::WATER, 40)
        }
        Drip::LavaHang | Drip::DripstoneLavaHang => {
            gravity *= 0.02;
            ([1.0, 0.4, 0.5], 40)
        }
        Drip::HoneyHang => {
            gravity *= 0.02 * 0.01;
            ([0.622, 0.508, 0.082], 100)
        }
        Drip::TearHang => {
            gravity *= 0.02 * 0.01;
            (Drip::TEAR, 100)
        }
        Drip::WaterFall | Drip::DripstoneWaterFall => (Drip::WATER, ranged(rng, 64.0)),
        Drip::LavaFall | Drip::DripstoneLavaFall => (Drip::LAVA, ranged(rng, 64.0)),
        Drip::HoneyFall => {
            gravity = 0.01;
            ([0.582, 0.448, 0.082], ranged(rng, 64.0))
        }
        Drip::TearFall => {
            gravity = 0.01;
            (Drip::TEAR, ranged(rng, 64.0))
        }
        Drip::NectarFall => {
            gravity = 0.007;
            ([0.92, 0.782, 0.72], ranged(rng, 16.0))
        }
        Drip::SporeFall => {
            gravity = 0.005;
            (
                [0.32, 0.5, 0.22],
                (64.0 / (0.1 + rng.next_f32() * 0.8)) as u32,
            )
        }
        Drip::LavaLand => (Drip::LAVA, ranged(rng, 16.0)),
        Drip::HoneyLand => ([0.522, 0.408, 0.082], ranged(rng, 128.0)),
        Drip::TearLand => (Drip::TEAR, ranged(rng, 28.0)),
    };
    r.tint = [color[0], color[1], color[2], 1.0];
    // This engine subtracts 0.04 times its gravity a tick.
    r.gravity = gravity / 0.04;
    r.friction = 0.98;
    r.lifetime = lifetime.max(1);
    r.flags = PHYSICS | if drip.glows() { FULLBRIGHT } else { 0 };
    r.behavior = Behavior::Drip(drip);
    Some(r)
}

/// `FallingLeavesParticle`: its fall, wind and spin.
#[allow(clippy::too_many_arguments)]
pub(super) fn leaf_recipe(
    atlas: &Atlas,
    name: &str,
    rng: &mut JavaRandom,
    fall: f32,
    side: f32,
    swirl: bool,
    flow: bool,
    scale: f32,
    start: f32,
) -> Option<Recipe> {
    let uv = random_frame(atlas, name, rng)?;
    let mut r = base_recipe(Sprite::Fixed(uv), rng);
    let rot_speed = if rng.next_int(2) == 0 { -30.0f32 } else { 30.0 }.to_radians();
    let spin = if rng.next_int(2) == 0 { -5.0f32 } else { 5.0 }.to_radians();
    r.lifetime = 300;
    r.gravity = fall * 1.2 * 0.0025 / 0.04;
    r.size = scale * if rng.next_int(2) == 0 { 0.05 } else { 0.075 };
    r.friction = 1.0;
    r.velocity = Vec3::new(0.0, -start, 0.0);
    let random = rng.next_f32();
    r.flags = PHYSICS;
    r.behavior = Behavior::Leaf(Leaf {
        roll: 0.0,
        old_roll: 0.0,
        rot_speed,
        spin,
        wind: side,
        swirl,
        flow,
        x_flow: (random * 60.0).to_radians().cos() * side,
        z_flow: (random * 60.0).to_radians().sin() * side,
        swirl_period: (1000.0 + random * 3000.0).to_radians(),
    });
    Some(r)
}

pub(super) fn generic_recipe(
    atlas: &Atlas,
    name: &str,
    arg: Vec3,
    rng: &mut JavaRandom,
) -> Option<Recipe> {
    let sheet = atlas.sheet_index(name)?;
    let animated = atlas.sheets[sheet as usize].len() > 1;
    let sprite = if animated {
        Sprite::Animated(sheet)
    } else {
        Sprite::Fixed(atlas.frame_uv(sheet, 0))
    };
    let mut r = base_recipe(sprite, rng);
    r.velocity = arg * 0.1;
    r.gravity = 0.0;
    r.friction = 0.96;
    r.size *= 0.75;
    r.lifetime = short_lifetime(rng);
    r.flags = 0;
    Some(r)
}

pub(super) fn random_frame(atlas: &Atlas, name: &str, rng: &mut JavaRandom) -> Option<[f32; 4]> {
    let sheet = atlas.sheet_index(name)?;
    let len = atlas.sheets[sheet as usize].len();
    Some(atlas.frame_uv(sheet, rng.next_int(len as u32) as usize))
}

pub(super) fn base_velocity(seed: Vec3, rng: &mut JavaRandom) -> Vec3 {
    let jitter = Vec3::new(
        (rng.next_f32() * 2.0 - 1.0) * 0.4,
        (rng.next_f32() * 2.0 - 1.0) * 0.4,
        (rng.next_f32() * 2.0 - 1.0) * 0.4,
    );
    let mut d = seed + jitter;
    let speed = (rng.next_f32() + rng.next_f32() + 1.0) * 0.15;
    let len = d.length();
    if len > 1e-5 {
        d /= len;
    } else {
        d = Vec3::ZERO;
    }
    Vec3::new(
        d.x * speed * 0.4,
        d.y * speed * 0.4 + 0.1,
        d.z * speed * 0.4,
    )
}

pub(super) fn set_power(v: Vec3, power: f32) -> Vec3 {
    Vec3::new(v.x * power, (v.y - 0.1) * power + 0.1, v.z * power)
}
