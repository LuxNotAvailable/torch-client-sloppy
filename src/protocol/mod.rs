//! Multi-version support: packet tables, id remaps, hops (`hop`, recipe at
//! top). Tables in `data/` from `tools/protogen.py`.

#![allow(
    dead_code,
    reason = "the tables carry lookups no hop written yet reads"
)]

mod entity_data;
pub(crate) mod hop;
mod modded;
pub(crate) mod neoforge;
pub(crate) mod packets;
pub(crate) mod registries;
pub(crate) mod remap;
mod v767;
mod v767_items;
mod v775;
pub(crate) mod version;
pub(crate) mod wire;

pub(crate) use version::joinable;

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, Ordering};

static SESSION: AtomicI32 = AtomicI32::new(0);

static FORCED: AtomicI32 = AtomicI32::new(0);

pub(crate) fn forced() -> Option<i32> {
    match FORCED.load(Ordering::Relaxed) {
        0 => None,
        protocol => Some(protocol),
    }
}

/// The Online Options toggle for experimental versions. Turning it off
/// drops a picked experimental version.
pub(crate) fn set_experimental(on: bool) {
    version::set_experimental(on);
    if !on && version::is_experimental(FORCED.load(Ordering::Relaxed)) {
        FORCED.store(0, Ordering::Relaxed);
    }
}

pub(crate) fn set_forced(protocol: Option<i32>) {
    let protocol = protocol.filter(|p| joinable(*p)).unwrap_or(0);
    FORCED.store(protocol, Ordering::Relaxed);
}

pub(crate) fn preselected() -> i32 {
    forced()
        .or_else(from_env)
        .unwrap_or(version::NATIVE.protocol)
}

static SEEN: OnceLock<Mutex<HashMap<String, i32>>> = OnceLock::new();

fn seen() -> &'static Mutex<HashMap<String, i32>> {
    SEEN.get_or_init(Default::default)
}

fn key(address: &str) -> String {
    address.trim().to_ascii_lowercase()
}

pub(crate) fn remember(address: &str, protocol: i32) {
    seen().lock().unwrap().insert(key(address), protocol);
}

pub(crate) async fn detect(address: &str) {
    if let Some(forced) = forced().or_else(from_env) {
        set_session(Some(forced));
        return;
    }
    if let Some(known) = seen().lock().unwrap().get(&key(address)).copied() {
        set_session(Some(known));
        return;
    }
    let protocol = crate::gui::ping::status_once(address, false)
        .await
        .ok()
        .map(|status| status.protocol);
    if let Some(protocol) = protocol {
        remember(address, protocol);
    }
    set_session(protocol);
}

fn from_env() -> Option<i32> {
    static CACHED: OnceLock<Option<i32>> = OnceLock::new();
    *CACHED.get_or_init(parse_env)
}

fn parse_env() -> Option<i32> {
    let raw = raw_override()?;
    let Ok(protocol) = raw.trim().parse::<i32>() else {
        crate::log_warn!("net", "MC_PROTOCOL={raw} is not a number; ignoring it");
        return None;
    };
    if !joinable(protocol) {
        crate::log_warn!(
            "net",
            "MC_PROTOCOL={protocol} is not a version this client can translate; ignoring it"
        );
        return None;
    }
    Some(protocol)
}

fn raw_override() -> Option<String> {
    crate::platform::env::var("MC_PROTOCOL").ok()
}

pub(crate) fn set_session(protocol: Option<i32>) {
    let protocol = protocol
        .filter(|p| joinable(*p))
        .unwrap_or(version::NATIVE.protocol);
    SESSION.store(protocol, Ordering::Relaxed);
    WIRE_IDS.write().unwrap().clear();
    MODDED_ITEMS.write().unwrap().clear();
    MODDED_ENTITIES.write().unwrap().clear();
    crate::blocks::modded::clear();
    azalea::join::HANDSHAKE_PROTOCOL.store(protocol, Ordering::Relaxed);
    if protocol != version::NATIVE.protocol {
        let name = version::ProtocolVersion::from_protocol(protocol)
            .map(|v| v.name)
            .unwrap_or("?");
        crate::log_info!("net", "translating this connection as {name} ({protocol})");
        if version::is_experimental(protocol) {
            crate::log_warn!(
                "net",
                "{name} translation is experimental: ids are remapped, most packet shapes are not"
            );
        }
    }
}

pub(crate) fn session() -> i32 {
    match SESSION.load(Ordering::Relaxed) {
        0 => version::NATIVE.protocol,
        protocol => protocol,
    }
}

pub(crate) fn entity_metadata_is_native() -> bool {
    version::metadata_matches_native(session())
}

/// A synced registry snapshot from a translated server, renumbered the way
/// this session's packets arrive (`hop::native_registry_id`). Entries with
/// no native counterpart are left out.
pub(crate) fn native_snapshot(
    registry: registries::RegistryId,
    entries: Vec<(u32, azalea_registry::identifier::Identifier)>,
) -> Vec<(u32, azalea_registry::identifier::Identifier)> {
    let wire: HashMap<u32, String> = entries
        .iter()
        .map(|(id, name)| (*id, name.to_string()))
        .collect();
    let protocol = session();
    if registry == registries::RegistryId::Block {
        crate::blocks::modded::on_block_sync(&wire, protocol);
    }
    WIRE_IDS.write().unwrap().insert(registry, wire);
    let entries: Vec<_> = match registry.id_space() {
        Some(space) => entries
            .into_iter()
            .filter_map(|(id, name)| {
                let id = hop::native_registry_id(protocol, space, i32::try_from(id).ok()?)?;
                Some((u32::try_from(id).ok()?, name))
            })
            .collect(),
        None => entries,
    };
    if registry == registries::RegistryId::EntityType {
        let mut names = MODDED_ENTITIES.write().unwrap();
        names.clear();
        for (id, name) in &entries {
            names.insert(*id, intern(&name.to_string()));
        }
    }
    if registry == registries::RegistryId::Item {
        let mut names = MODDED_ITEMS.write().unwrap();
        names.clear();
        for (id, name) in &entries {
            names.insert(*id, intern(&name.to_string()));
        }
    }
    entries
}

/// The server's item names by native id, for stacks of items this client
/// doesn't have (`ItemStackData::modded_kind`).
static MODDED_ITEMS: std::sync::LazyLock<std::sync::RwLock<HashMap<u32, &'static str>>> =
    std::sync::LazyLock::new(Default::default);

/// `name` as an item id string: vanilla ones bare (`stone`), others
/// namespaced (`create:cogwheel`). Kept for the process, so a reconnect
/// reuses them.
fn intern(name: &str) -> &'static str {
    static NAMES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> =
        std::sync::LazyLock::new(Default::default);
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let mut names = NAMES.lock().unwrap();
    if let Some(found) = names.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// The server's entity type names by native id (`entities::modded`).
static MODDED_ENTITIES: std::sync::LazyLock<std::sync::RwLock<HashMap<u32, &'static str>>> =
    std::sync::LazyLock::new(Default::default);

/// The id string of a modded entity type's native id, when the server
/// synced its entity type registry.
pub(crate) fn modded_entity(id: u32) -> Option<&'static str> {
    MODDED_ENTITIES.read().unwrap().get(&id).copied()
}

/// The id string of a modded item's native id, when the server synced its
/// item registry.
pub(crate) fn modded_item(id: u32) -> Option<&'static str> {
    MODDED_ITEMS.read().unwrap().get(&id).copied()
}

/// Where vanilla's blocks and states end in the server's own numbering.
pub(crate) fn wire_vanilla(protocol: i32) -> crate::blocks::modded::WireVanilla {
    let count =
        |space| hop::wire_vanilla_count(protocol, space).and_then(|n| u32::try_from(n).ok());
    crate::blocks::modded::WireVanilla {
        blocks: count(remap::IdSpace::Block).unwrap_or_else(|| {
            (0..)
                .map_while(
                    <azalea_registry::builtin::BlockKind as azalea_registry::Registry>::from_u32,
                )
                .count() as u32
        }),
        states: count(remap::IdSpace::BlockState)
            .unwrap_or(azalea::block::BlockState::MAX_STATE as u32 + 1),
    }
}

/// A block state id as the server sent it, in this client's numbering.
pub(crate) fn native_state_id(protocol: i32, id: u32) -> Option<u32> {
    let id = hop::native_registry_id(
        protocol,
        remap::IdSpace::BlockState,
        i32::try_from(id).ok()?,
    )?;
    u32::try_from(id).ok()
}

/// The ids a modded server's synced registries hold, in its own numbering,
/// for this session. Lets a hop tell a mod's component from garbage.
#[allow(clippy::type_complexity, reason = "one id-to-name map per registry")]
static WIRE_IDS: std::sync::LazyLock<
    std::sync::RwLock<HashMap<registries::RegistryId, HashMap<u32, String>>>,
> = std::sync::LazyLock::new(Default::default);

/// Whether the server's synced `registry` has `id` (its own numbering);
/// `None` when it synced none.
pub(crate) fn wire_registry_has(registry: registries::RegistryId, id: u32) -> Option<bool> {
    WIRE_IDS
        .read()
        .unwrap()
        .get(&registry)
        .map(|ids| ids.contains_key(&id))
}

/// The name the server's synced `registry` gives `id`: `None` when it
/// synced none, `Some(None)` when it has no such id.
pub(crate) fn wire_registry_name(
    registry: registries::RegistryId,
    id: u32,
) -> Option<Option<String>> {
    WIRE_IDS
        .read()
        .unwrap()
        .get(&registry)
        .map(|ids| ids.get(&id).cloned())
}

pub(crate) fn translator() -> Option<Box<dyn azalea::connection::PacketTranslator>> {
    hop::Translator::for_protocol(session()).map(|t| Box::new(t) as Box<_>)
}
