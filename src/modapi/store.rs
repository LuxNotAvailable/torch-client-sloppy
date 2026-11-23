//! Modded block entities: what the last protocol hop took out of chunk and
//! `block_entity_data` packets (azalea can't hold a modded type), kept by
//! position. Each tick, `tick` asks the owning plugin for a rig wherever
//! something changed, and publishes every rig for the renderer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Map, Value};

/// A modded block entity as the server sent it.
struct Entry {
    kind: String,
    nbt: Value,
    /// The block state it had when its rig was last made; a change re-asks.
    state: Option<u16>,
    dirty: bool,
}

/// A rig for the renderer, with the revision it was made at.
pub struct Placed {
    pub rig: Arc<torch_mod_api::Rig>,
    pub rev: u64,
}

#[derive(Default)]
struct Store {
    entries: HashMap<[i32; 3], Entry>,
    rigs: HashMap<[i32; 3], Placed>,
    next_rev: u64,
    /// Types no loaded plugin handles, logged once each.
    unhandled: HashSet<String>,
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(Default::default);

/// Every rig, by block position; replaced each tick something changes.
static PUBLISHED: std::sync::LazyLock<
    parking_lot::RwLock<Arc<HashMap<[i32; 3], (Arc<torch_mod_api::Rig>, u64)>>>,
> = std::sync::LazyLock::new(Default::default);

/// The rigs to draw, by block position, each with its revision.
pub fn rigs() -> Arc<HashMap<[i32; 3], (Arc<torch_mod_api::Rig>, u64)>> {
    PUBLISHED.read().clone()
}

/// A modded block entity arrived (from a chunk or `block_entity_data`):
/// its type id and its network NBT.
pub fn on_block_entity(pos: [i32; 3], kind: String, nbt: &[u8]) {
    let nbt = read_network_nbt(nbt).unwrap_or(Value::Null);
    STORE.lock().entries.insert(
        pos,
        Entry {
            kind,
            nbt,
            state: None,
            dirty: true,
        },
    );
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
    *PUBLISHED.write() = Arc::default();
}

/// Asks plugins for the rigs of block entities that changed, then
/// publishes. `world` answers which block stands where.
pub fn tick(world: &azalea_world::World, game_time: u64) {
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
    let mut gone = Vec::new();
    for (pos, entry) in store.entries.iter_mut() {
        let Some(state) =
            world.get_block_state(azalea_core::position::BlockPos::new(pos[0], pos[1], pos[2]))
        else {
            gone.push(*pos);
            continue;
        };
        if entry.state != Some(state.id()) {
            entry.state = Some(state.id());
            entry.dirty = true;
        }
        if !entry.dirty {
            continue;
        }
        entry.dirty = false;
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
        };
        let rig = match super::block_entity(&view, near(*pos)) {
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
                store.next_rev += 1;
                store.rigs.insert(
                    *pos,
                    Placed {
                        rig: Arc::new(rig),
                        rev: store.next_rev,
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
                .map(|(pos, placed)| (*pos, (placed.rig.clone(), placed.rev)))
                .collect(),
        );
    }
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
    let mut at = 0usize;
    let kind = *bytes.first()?;
    at += 1;
    if kind == 0 {
        return Some(Value::Null);
    }
    let value = read_payload(bytes, &mut at, kind, 0)?;
    Some(value)
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, n: usize) -> Option<&'a [u8]> {
    let slice = bytes.get(*at..at.checked_add(n)?)?;
    *at += n;
    Some(slice)
}

fn be_i32(bytes: &[u8], at: &mut usize) -> Option<i32> {
    Some(i32::from_be_bytes(take(bytes, at, 4)?.try_into().ok()?))
}

fn string(bytes: &[u8], at: &mut usize) -> Option<String> {
    let len = u16::from_be_bytes(take(bytes, at, 2)?.try_into().ok()?) as usize;
    // Modified UTF-8: close enough to UTF-8 for ids and text a port reads.
    Some(String::from_utf8_lossy(take(bytes, at, len)?).into_owned())
}

fn read_payload(bytes: &[u8], at: &mut usize, kind: u8, depth: u32) -> Option<Value> {
    if depth > 64 {
        return None;
    }
    let number = |v: f64| serde_json::Number::from_f64(v).map(Value::Number);
    Some(match kind {
        1 => Value::from(take(bytes, at, 1)?[0] as i8),
        2 => Value::from(i16::from_be_bytes(take(bytes, at, 2)?.try_into().ok()?)),
        3 => Value::from(be_i32(bytes, at)?),
        4 => Value::from(i64::from_be_bytes(take(bytes, at, 8)?.try_into().ok()?)),
        5 => number(f32::from_be_bytes(take(bytes, at, 4)?.try_into().ok()?) as f64)?,
        6 => number(f64::from_be_bytes(take(bytes, at, 8)?.try_into().ok()?))?,
        7 => {
            let n = usize::try_from(be_i32(bytes, at)?).ok()?;
            Value::Array(
                take(bytes, at, n)?
                    .iter()
                    .map(|b| Value::from(*b as i8))
                    .collect(),
            )
        }
        8 => Value::String(string(bytes, at)?),
        9 => {
            let element = take(bytes, at, 1)?[0];
            let n = usize::try_from(be_i32(bytes, at)?).ok()?;
            let mut out = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                out.push(read_payload(bytes, at, element, depth + 1)?);
            }
            Value::Array(out)
        }
        10 => {
            let mut map = Map::new();
            loop {
                let tag = take(bytes, at, 1)?[0];
                if tag == 0 {
                    break;
                }
                let name = string(bytes, at)?;
                let value = read_payload(bytes, at, tag, depth + 1)?;
                map.insert(name, value);
            }
            Value::Object(map)
        }
        11 => {
            let n = usize::try_from(be_i32(bytes, at)?).ok()?;
            let mut out = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                out.push(Value::from(be_i32(bytes, at)?));
            }
            Value::Array(out)
        }
        12 => {
            let n = usize::try_from(be_i32(bytes, at)?).ok()?;
            let mut out = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                out.push(Value::from(i64::from_be_bytes(
                    take(bytes, at, 8)?.try_into().ok()?,
                )));
            }
            Value::Array(out)
        }
        _ => return None,
    })
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
