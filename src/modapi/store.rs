//! Modded block entities: what the last protocol hop took out of chunk and
//! `block_entity_data` packets (azalea can't hold a modded type), kept by
//! position. Each tick, `tick` asks the owning plugin for a rig wherever
//! something changed (its NBT, its block state, or a face neighbour's
//! state), ticks the types a plugin simulates client-side (`Plugin::tick`)
//! near the player, and publishes every rig for the renderer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

/// A modded block entity as the server sent it.
struct Entry {
    kind: String,
    nbt: Value,
    /// The game time its NBT arrived (the first tick after it did).
    nbt_time: Option<i64>,
    /// The block state it had when its rig was last made; a change re-asks.
    state: Option<u16>,
    /// Its six face neighbours' states then (`FACES` order); a change
    /// re-asks, for connection logic.
    around: Option<[u16; 6]>,
    dirty: bool,
}

/// Face neighbour offsets: down, up, north, south, west, east.
const FACES: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

/// A rig for the renderer, with the revision it was made at.
#[derive(Clone)]
pub struct Placed {
    pub rig: Arc<torch_mod_api::Rig>,
    pub rev: u64,
    /// Bumped each tick a ticking rig's transforms move (same parts).
    pub frame: u64,
    /// The parts' transforms the tick before, to blend from.
    pub prev: Option<Arc<[torch_mod_api::Transform]>>,
}

/// `MC_MODAPI_TRACE=<file>`: every view made from the server's data goes
/// to `file` as a JSON line, so `torch-port-check --trace` can replay a
/// session against a port's client-side simulation.
static TRACE: std::sync::LazyLock<Mutex<Option<std::io::BufWriter<std::fs::File>>>> =
    std::sync::LazyLock::new(|| {
        let file = crate::platform::env::var("MC_MODAPI_TRACE")
            .ok()
            .and_then(|path| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .inspect_err(|e| crate::log_warn!("modapi", "trace {path}: {e}"))
                    .ok()
            });
        Mutex::new(file.map(std::io::BufWriter::new))
    });

fn trace(view: &torch_mod_api::BlockEntityView) {
    use std::io::Write;
    let mut trace = TRACE.lock();
    if let Some(out) = trace.as_mut() {
        if let Ok(line) = serde_json::to_string(view) {
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
    }
}

/// Ticking types are simulated this close to the player, in blocks.
const TICK_DISTANCE: f64 = 96.0;

/// Whether two rigs have the same parts, transforms aside: a ticking rig
/// whose parts only moved blends instead of drawing afresh.
pub(super) fn same_parts(a: &torch_mod_api::Rig, b: &torch_mod_api::Rig) -> bool {
    a.parts.len() == b.parts.len()
        && a.parts.iter().zip(&b.parts).all(|(x, y)| {
            x.geometry == y.geometry && x.animation == y.animation && x.scroll == y.scroll
        })
}

#[derive(Default)]
struct Store {
    entries: HashMap<[i32; 3], Entry>,
    rigs: HashMap<[i32; 3], Placed>,
    next_rev: u64,
    /// Types no loaded plugin handles, logged once each.
    unhandled: HashSet<String>,
    /// Rigs logged so far, by type: the first few of each go to the log.
    logged: HashMap<String, u32>,
    /// The game time ticking types were last ticked at.
    ticked: Option<u64>,
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(Default::default);

/// Every rig, by block position; replaced each tick something changes.
static PUBLISHED: std::sync::LazyLock<parking_lot::RwLock<Arc<HashMap<[i32; 3], Placed>>>> =
    std::sync::LazyLock::new(Default::default);

/// The rigs to draw, by block position.
pub fn rigs() -> Arc<HashMap<[i32; 3], Placed>> {
    PUBLISHED.read().clone()
}

/// A modded block entity arrived (from a chunk or `block_entity_data`):
/// its type id and its network NBT.
pub fn on_block_entity(pos: [i32; 3], kind: String, nbt: &[u8]) {
    let nbt = read_network_nbt(nbt).unwrap_or(Value::Null);
    let mut store = STORE.lock();
    let new = store
        .entries
        .insert(
            pos,
            Entry {
                kind,
                nbt,
                nbt_time: None,
                state: None,
                around: None,
                dirty: true,
            },
        )
        .is_none();
    // A neighbour's connection logic may care what block entity is here.
    if new {
        for [dx, dy, dz] in FACES {
            if let Some(e) = store
                .entries
                .get_mut(&[pos[0] + dx, pos[1] + dy, pos[2] + dz])
            {
                e.dirty = true;
            }
        }
    }
}

/// Asks again for the rigs of the block entities at `positions` (a mod
/// packet changed what the plugin knows about them).
pub fn redraw(positions: &[[i32; 3]]) {
    if positions.is_empty() {
        return;
    }
    let mut store = STORE.lock();
    for pos in positions {
        if let Some(entry) = store.entries.get_mut(pos) {
            entry.dirty = true;
        }
    }
}

/// A chunk left: its block entities go with it.
pub fn on_forget_chunk(chunk_x: i32, chunk_z: i32) {
    let mut store = STORE.lock();
    let inside = |pos: &[i32; 3]| pos[0] >> 4 == chunk_x && pos[2] >> 4 == chunk_z;
    store.entries.retain(|pos, _| !inside(pos));
    store.rigs.retain(|pos, _| !inside(pos));
}

/// The type a captured block entity has, for tests.
#[cfg(test)]
pub fn kind_at(pos: [i32; 3]) -> Option<String> {
    STORE.lock().entries.get(&pos).map(|e| e.kind.clone())
}

/// A new connection: nothing from the last one stays.
pub fn reset() {
    *STORE.lock() = Store::default();
    super::ENTITY_DATA.lock().clear();
    super::screens::reset();
    *PUBLISHED.write() = Arc::default();
}

/// Asks plugins for the rigs of block entities that changed, ticks the
/// client-simulated ones near `player`, then publishes. `world` answers
/// which block stands where.
pub fn tick(world: &azalea_world::World, game_time: u64, player: Option<[f64; 3]>) {
    let mut store = STORE.lock();
    let store = &mut *store;
    if store.entries.is_empty() && store.rigs.is_empty() {
        return;
    }
    let mut changed = false;
    // What the plugin may see: the blocks within `NEAR` of it.
    let near = |pos: [i32; 3]| -> super::runtime::Near {
        let r = super::runtime::NEAR;
        let mut out = super::runtime::Near::new();
        for dx in -r..=r {
            for dy in -r..=r {
                for dz in -r..=r {
                    let at = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                    let block = azalea_core::position::BlockPos::new(at[0], at[1], at[2]);
                    if let Some(state) = world.get_block_state(block) {
                        out.insert(at, state.id());
                    }
                }
            }
        }
        out
    };
    let state_at = |at: [i32; 3]| {
        world.get_block_state(azalea_core::position::BlockPos::new(at[0], at[1], at[2]))
    };
    let mut gone = Vec::new();
    let mut todo = Vec::new();
    // Ticking types advance once per game tick, however often this runs.
    let new_tick = store.ticked != Some(game_time);
    store.ticked = Some(game_time);
    for (pos, entry) in store.entries.iter_mut() {
        let Some(state) = state_at(*pos) else {
            gone.push(*pos);
            continue;
        };
        let around = FACES.map(|[dx, dy, dz]| {
            state_at([pos[0] + dx, pos[1] + dy, pos[2] + dz]).map_or(u16::MAX, |s| s.id())
        });
        if entry.state != Some(state.id()) || entry.around != Some(around) {
            entry.state = Some(state.id());
            entry.around = Some(around);
            entry.dirty = true;
        }
        let nbt_time = *entry.nbt_time.get_or_insert(game_time as i64);
        if entry.dirty {
            entry.dirty = false;
            todo.push((*pos, state, nbt_time, false));
        } else if new_tick
            && super::ticks(&entry.kind)
            && player.is_some_and(|p| {
                let d = [0, 1, 2].map(|i| f64::from(pos[i]) + 0.5 - p[i]);
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < TICK_DISTANCE * TICK_DISTANCE
            })
        {
            todo.push((*pos, state, nbt_time, true));
        }
    }
    for (pos, state, nbt_time, ticking) in todo {
        let pos = &pos;
        let entry = &store.entries[pos];
        let info = crate::blocks::info::info(state);
        let view = torch_mod_api::BlockEntityView {
            kind: entry.kind.clone(),
            pos: *pos,
            block: full_id(info.id),
            props: info
                .props()
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            nbt: entry.nbt.clone(),
            time: game_time as i64,
            nbt_time,
        };
        let answer = if ticking {
            super::tick_block_entity(&view, near(*pos))
        } else {
            trace(&view);
            super::block_entity(&view, near(*pos))
        };
        let rig = match answer {
            Some(Ok(rig)) => Some(rig),
            Some(Err(e)) => {
                crate::log_warn!("modapi", "{} at {pos:?}: {e}", entry.kind);
                None
            }
            None => {
                if store.unhandled.insert(entry.kind.clone()) {
                    crate::log_info!("modapi", "no plugin draws {}", entry.kind);
                }
                None
            }
        };
        changed = true;
        match rig {
            Some(rig) if !rig.parts.is_empty() => {
                let seen = store.logged.entry(entry.kind.clone()).or_insert(0);
                if *seen < 3 {
                    *seen += 1;
                    let nbt = entry.nbt.to_string();
                    let nbt = &nbt[..nbt.len().min(240)];
                    let parts: Vec<String> = rig
                        .parts
                        .iter()
                        .map(|p| serde_json::to_string(p).unwrap_or_default())
                        .map(|p| p[..p.len().min(200)].to_owned())
                        .collect();
                    crate::log_info!(
                        "modapi",
                        "{} at {pos:?} ({}): nbt {nbt}; parts {}",
                        entry.kind,
                        view.block,
                        parts.join(" | ")
                    );
                }
                // Only transforms moved: keep the parts and blend.
                if let Some(old) = store.rigs.get_mut(pos)
                    && same_parts(&old.rig, &rig)
                {
                    if old.rig.parts != rig.parts {
                        old.prev = Some(old.rig.parts.iter().map(|p| p.transform).collect());
                        old.rig = Arc::new(rig);
                        old.frame += 1;
                    }
                    continue;
                }
                store.next_rev += 1;
                store.rigs.insert(
                    *pos,
                    Placed {
                        rig: Arc::new(rig),
                        rev: store.next_rev,
                        frame: 0,
                        prev: None,
                    },
                );
            }
            _ => {
                store.rigs.remove(pos);
            }
        }
    }
    for pos in gone {
        store.entries.remove(&pos);
        store.rigs.remove(&pos);
        changed = true;
    }
    if changed {
        *PUBLISHED.write() = Arc::new(
            store
                .rigs
                .iter()
                .map(|(pos, placed)| (*pos, placed.clone()))
                .collect(),
        );
    }
}

/// The plugin's view of the block entity at `pos`, if one was captured
/// and its block is loaded.
fn view_at(
    store: &Store,
    world: &azalea_world::World,
    pos: [i32; 3],
) -> Option<torch_mod_api::BlockEntityView> {
    let entry = store.entries.get(&pos)?;
    let state =
        world.get_block_state(azalea_core::position::BlockPos::new(pos[0], pos[1], pos[2]))?;
    let info = crate::blocks::info::info(state);
    let time = store.ticked.unwrap_or(0) as i64;
    Some(torch_mod_api::BlockEntityView {
        kind: entry.kind.clone(),
        pos,
        block: full_id(info.id),
        props: info
            .props()
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        nbt: entry.nbt.clone(),
        time,
        nbt_time: entry.nbt_time.unwrap_or(time),
    })
}

/// Asks the plugin of a block entity that moves entities (a belt) under or
/// around the local player's feet what it does to the player this tick,
/// and hands that to physics (`azalea_physics::ExternalPush`).
pub fn push_local(world: &azalea_world::World, entity: torch_mod_api::MovingEntity) {
    let [x, y, z] = entity.pos;
    let feet = [x.floor() as i32, y.floor() as i32, z.floor() as i32];
    let below = [feet[0], (y - 0.25).floor() as i32, feet[2]];
    let view = {
        let store = STORE.lock();
        [feet, below].into_iter().find_map(|at| {
            let kind = &store.entries.get(&at)?.kind;
            super::moves_entities(kind)
                .then(|| view_at(&store, world, at))
                .flatten()
        })
    };
    let push = view
        .and_then(|block_entity| {
            super::entity_motion(&torch_mod_api::EntityMotionView {
                block_entity,
                entity,
            })
        })
        .filter(|m| m.move_by.is_some() || m.velocity.is_some())
        .map(|m| {
            let vec = |v: [f64; 3]| azalea_core::position::Vec3::new(v[0], v[1], v[2]);
            azalea_physics::ExternalPush {
                move_by: m.move_by.map(vec),
                velocity: m.velocity.map(vec),
            }
        });
    azalea_physics::ExternalPush::set(push);
}

/// A block id with its namespace (`stone` as `minecraft:stone`).
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// Network NBT (a nameless root tag) as JSON: compounds as objects, lists
/// and arrays as arrays, numbers as numbers.
pub(crate) fn read_network_nbt(bytes: &[u8]) -> Option<Value> {
    torch_mod_api::buf::Reader::new(bytes).nbt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_nbt_reads_as_json() {
        // {Speed: 32.0f, Network: {Id: 12L}, Name: "x", Ids: [1, 2]}
        let mut b = vec![10u8];
        let named = |b: &mut Vec<u8>, tag: u8, name: &str| {
            b.push(tag);
            b.extend_from_slice(&(name.len() as u16).to_be_bytes());
            b.extend_from_slice(name.as_bytes());
        };
        named(&mut b, 5, "Speed");
        b.extend_from_slice(&32.0f32.to_be_bytes());
        named(&mut b, 10, "Network");
        named(&mut b, 4, "Id");
        b.extend_from_slice(&12i64.to_be_bytes());
        b.push(0);
        named(&mut b, 8, "Name");
        b.extend_from_slice(&1u16.to_be_bytes());
        b.push(b'x');
        named(&mut b, 11, "Ids");
        b.extend_from_slice(&2i32.to_be_bytes());
        b.extend_from_slice(&1i32.to_be_bytes());
        b.extend_from_slice(&2i32.to_be_bytes());
        b.push(0);
        let v = read_network_nbt(&b).unwrap();
        assert_eq!(v["Speed"], 32.0);
        assert_eq!(v["Network"]["Id"], 12);
        assert_eq!(v["Name"], "x");
        assert_eq!(v["Ids"], serde_json::json!([1, 2]));
        assert!(read_network_nbt(&b[..b.len() - 3]).is_none());
    }
}
