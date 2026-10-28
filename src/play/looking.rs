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
    let block: Box<dyn BlockTrait> = Box::<dyn BlockTrait>::from(state);
    let mut properties: Vec<(&'static str, &'static str)> =
        block.property_map().into_iter().collect();
    properties.sort_unstable();
    let progress = (bot.component::<MineBlockPos>().ok().and_then(|p| p.0) == Some(pos))
        .then(|| bot.component::<MineProgress>().ok().map(|p| p.0.clamp(0.0, 1.0)))
        .flatten();
    let harvestable = block.behavior().requires_correct_tool_for_drops.then(|| {
        bot.component::<azalea::entity::inventory::Inventory>()
            .is_ok_and(|inventory| {
                azalea::entity::mining::has_correct_tool_for_drops(
                    block.as_ref(),
                    inventory.held_item(),
                )
            })
    });
    Some(Looking::Block {
        id: block.id(),
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
                .unwrap_or(20.0);
            (health, max.max(health))
        });
    Some(Looking::Entity { id, name, health })
}
