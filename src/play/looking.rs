//! What the crosshair is on, for the block information panel (as the Jade
//! mod shows it): a block with its state, how far mining it has got and
//! whether the held item harvests it, or an entity with its health. Read
//! each game tick, drawn each frame.

use std::sync::Mutex;

use azalea::Client;
use azalea_core::hit_result::HitResult;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Looking {
    Block {
        /// The block's id without its namespace (`oak_log`).
        id: &'static str,
        pos: [i32; 3],
        /// The state's properties, sorted by name.
        properties: Vec<(&'static str, &'static str)>,
        /// Mining progress, 0 to 1, while this block is being mined.
        progress: Option<f32>,
        /// Whether the held item gets drops from it; `None` when the block
        /// drops with anything.
        harvestable: Option<bool>,
    },
    Entity {
        /// The entity type's id without its namespace (`zombie`).
        id: &'static str,
        /// A custom name, if it has one.
        name: Option<String>,
        /// Health and maximum health, for living entities.
        health: Option<(f32, f32)>,
    },
}

static LOOKING: Mutex<Option<Looking>> = Mutex::new(None);

/// Whether the player sneaks, which shows a block's full state (Jade's
/// extended view).
static SNEAKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn sneaking() -> bool {
    SNEAKING.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn current() -> Option<Looking> {
    LOOKING.lock().ok().and_then(|l| l.clone())
}

pub(crate) fn clear() {
    if let Ok(mut looking) = LOOKING.lock() {
        *looking = None;
    }
}

/// Each game tick: what the crosshair is on now.
pub(crate) fn tick(bot: &Client) {
    let crouching = bot
        .component::<azalea::entity::Pose>()
        .is_ok_and(|p| *p == azalea::entity::Pose::Crouching);
    SNEAKING.store(crouching, std::sync::atomic::Ordering::Relaxed);
    let found = match bot.hit_result() {
        Ok(HitResult::Block(hit)) if !hit.miss => block(bot, hit.block_pos),
        Ok(HitResult::Entity(hit)) => entity(bot, hit.entity),
        _ => None,
    };
    if let Ok(mut looking) = LOOKING.lock() {
        *looking = found;
    }
}

fn block(bot: &Client, pos: azalea_core::position::BlockPos) -> Option<Looking> {
    use azalea::block::BlockTrait;
    use azalea::mining::{MineBlockPos, MineProgress};
    let state = bot.world().ok()?.read().get_block_state(pos)?;
    if state.is_air() {
        return None;
    }
    let facts = crate::blocks::info::info(state);
    let properties: Vec<(&'static str, &'static str)> = facts.props().to_vec();
    let progress = (bot.component::<MineBlockPos>().ok().and_then(|p| p.0) == Some(pos))
        .then(|| {
            bot.component::<MineProgress>()
                .ok()
                .map(|p| p.0.clamp(0.0, 1.0))
        })
        .flatten();
    let harvestable = facts.needs_tool.then(|| {
        let block = Box::<dyn BlockTrait>::from(state);
        bot.component::<azalea::entity::inventory::Inventory>()
            .is_ok_and(|inventory| {
                azalea::entity::mining::has_correct_tool_for_drops(
                    block.as_ref(),
                    inventory.held_item(),
                )
            })
    });
    Some(Looking::Block {
        id: facts.id,
        pos: [pos.x, pos.y, pos.z],
        properties,
        progress,
        harvestable,
    })
}

fn entity(bot: &Client, entity: azalea::ecs::entity::Entity) -> Option<Looking> {
    use azalea::entity::{EntityKindComponent, metadata};
    let kind = bot.entity_component::<EntityKindComponent>(entity).ok()?;
    let id = kind.to_str();
    let id = id.strip_prefix("minecraft:").unwrap_or(id);
    let name = bot
        .entity_component::<metadata::CustomName>(entity)
        .ok()
        .and_then(|n| (**n).clone())
        .map(|text| text.to_string());
    let health = bot
        .entity_component::<metadata::Health>(entity)
        .ok()
        .map(|h| h.0)
        .map(|health| {
            let max = bot
                .entity_component::<azalea_core::entity_id::MinecraftEntityId>(entity)
                .ok()
                .and_then(|id| {
                    crate::client::tracking::max_healths()
                        .lock()
                        .unwrap()
                        .get(&id.0)
                        .copied()
                })
                .unwrap_or_else(|| default_max_health(id));
            (health, max.max(health))
        });
    Some(Looking::Entity { id, name, health })
}

/// A mob's maximum health when the server hasn't sent its `max_health`
/// attribute: vanilla's `createAttributes` value for the type (generated
/// from 26.3's `DefaultAttributes`), 20 (the attribute's default) for any
/// other. Horses, llamas and their kin give the top of their random range;
/// slimes and magma cubes, whose health follows their size, give 20.
pub(crate) fn default_max_health(id: &str) -> f32 {
    match id {
        "armadillo" => 12.0,
        "axolotl" => 14.0,
        "bat" => 6.0,
        "bee" => 10.0,
        "bogged" => 16.0,
        "cat" => 10.0,
        "camel" => 32.0,
        "camel_husk" => 32.0,
        "cave_spider" => 12.0,
        "chicken" => 4.0,
        "cod" => 3.0,
        "copper_golem" => 12.0,
        "cow" => 10.0,
        "creaking" => 1.0,
        "dolphin" => 10.0,
        "donkey" => 53.0,
        "elder_guardian" => 80.0,
        "enderman" => 40.0,
        "endermite" => 8.0,
        "ender_dragon" => 200.0,
        "evoker" => 24.0,
        "breeze" => 30.0,
        "fox" => 10.0,
        "frog" => 10.0,
        "ghast" => 10.0,
        "giant" => 100.0,
        "glow_squid" => 10.0,
        "goat" => 10.0,
        "guardian" => 30.0,
        "hoglin" => 40.0,
        "horse" => 53.0,
        "illusioner" => 32.0,
        "iron_golem" => 100.0,
        "llama" => 53.0,
        "mooshroom" => 10.0,
        "mule" => 53.0,
        "nautilus" => 15.0,
        "ocelot" => 10.0,
        "parched" => 16.0,
        "parrot" => 6.0,
        "pig" => 10.0,
        "piglin" => 16.0,
        "piglin_brute" => 50.0,
        "pillager" => 24.0,
        "polar_bear" => 30.0,
        "pufferfish" => 3.0,
        "rabbit" => 3.0,
        "ravager" => 100.0,
        "salmon" => 3.0,
        "sheep" => 8.0,
        "shulker" => 30.0,
        "silverfish" => 8.0,
        "skeleton_horse" => 15.0,
        "sniffer" => 14.0,
        "snow_golem" => 4.0,
        "spider" => 16.0,
        "squid" => 10.0,
        "tadpole" => 6.0,
        "trader_llama" => 53.0,
        "tropical_fish" => 3.0,
        "turtle" => 30.0,
        "vex" => 14.0,
        "vindicator" => 24.0,
        "warden" => 500.0,
        "witch" => 26.0,
        "wither" => 300.0,
        "wolf" => 8.0,
        "zoglin" => 40.0,
        "zombie_horse" => 25.0,
        "zombie_nautilus" => 15.0,
        _ => 20.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobs_without_a_sent_maximum_use_vanillas() {
        assert_eq!(default_max_health("cow"), 10.0);
        assert_eq!(default_max_health("iron_golem"), 100.0);
        assert_eq!(default_max_health("warden"), 500.0);
        assert_eq!(default_max_health("zombie"), 20.0);
    }
}
