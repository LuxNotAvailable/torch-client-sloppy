//! 1.21 and 1.21.1 (protocol 767) on the wire, 1.21.11 (774) natively;
//! the 774 and 775 hops carry it on to 26.3.
//!
//! Experimental: packet and registry ids are remapped and login works;
//! most other payload shapes are not rewritten yet. The rewrites to
//! add are listed in `docs/MULTIVERSION_PLAN.md`, section 4.

use super::entity_data;
use super::hop::{self, HopSpec, Ids, Rewritten, body};
use super::packets::{Direction, PacketTable, Phase};
use super::remap::{IdSpace, Remap};
use super::v767_items::{Unreadable, exact_components, stack, stacks_until};
use super::wire::{Reader, write_varint, write_varlong};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

use azalea_buf::AzBuf;
use azalea_core::delta::LpVec3;
use azalea_core::position::Vec3;

/// The dimension the last `login` or `respawn` named: 1.21.1's spawn
/// position doesn't say, 1.21.9's does.
static DIMENSION: Mutex<String> = Mutex::new(String::new());

/// This client's entity id (`login`), which 1.21.1's sneak commands name.
static PLAYER_ID: AtomicI32 = AtomicI32::new(0);

/// Each spawned entity's 1.21.1 type, which its entity data is laid out by.
static ENTITIES: Mutex<Option<HashMap<i32, i32>>> = Mutex::new(None);

/// The sneak flag last told to the server.
static SNEAKING: AtomicBool = AtomicBool::new(false);

pub(super) const SPEC: HopSpec = HopSpec {
    wire: PacketTable::v1_21_1,
    native: PacketTable::v1_21_11,
    to_native: Remap::v767_to_v774,
    from_native: Remap::v774_to_v767,
    // No rewrite picks a payload shape by azalea's numbering yet.
    kinds: None,
    patch,
    inbound: &[
        (Phase::Login, "login_finished", body!(login_finished)),
        (Phase::Configuration, "update_tags", body!(hop::update_tags)),
        (Phase::Game, "update_tags", body!(hop::update_tags)),
        (Phase::Game, "login", body!(login)),
        (Phase::Game, "level_chunk_with_light", body!(level_chunk)),
        (Phase::Game, "block_update", body!(hop::block_update)),
        (
            Phase::Game,
            "section_blocks_update",
            body!(hop::section_blocks_update),
        ),
        (Phase::Game, "respawn", body!(respawn)),
        (Phase::Game, "player_position", body!(player_position)),
        (Phase::Game, "teleport_entity", teleport_entity),
        (Phase::Game, "add_entity", body!(add_entity)),
        // Same shape since 1.21.1, but attribute ids moved: 1.21.2 dropped
        // their `generic.` prefixes and the registry is sorted by name.
        (
            Phase::Game,
            "update_attributes",
            body!(super::v775::update_attributes),
        ),
        (Phase::Game, "sound", body!(hop::sound)),
        (Phase::Game, "sound_entity", body!(hop::sound)),
        (Phase::Game, "level_event", body!(hop::level_event)),
        (Phase::Game, "block_event", body!(hop::block_event)),
        (Phase::Game, "award_stats", body!(hop::award_stats)),
        (Phase::Game, "block_entity_data", body!(block_entity_data)),
        (Phase::Game, "set_time", body!(set_time)),
        (Phase::Game, "cooldown", body!(cooldown)),
        (Phase::Game, "level_particles", body!(level_particles)),
        (Phase::Game, "explode", body!(explode)),
        (Phase::Game, "set_player_team", body!(set_player_team)),
        (Phase::Game, "update_recipes", body!(update_recipes)),
        (Phase::Game, "chunks_biomes", body!(chunks_biomes)),
        (Phase::Game, "merchant_offers", body!(merchant_offers)),
        (Phase::Game, "commands", body!(logged_commands)),
        (
            Phase::Game,
            "update_advancements",
            body!(logged_advancements),
        ),
        (
            Phase::Game,
            "container_set_content",
            body!(container_set_content),
        ),
        (Phase::Game, "container_set_slot", body!(container_set_slot)),
        (Phase::Game, "set_equipment", body!(set_equipment)),
        (Phase::Game, "set_entity_data", body!(entity_data)),
        (Phase::Game, "custom_payload", custom_payload),
        (Phase::Game, "set_entity_motion", body!(set_entity_motion)),
        (
            Phase::Game,
            "set_default_spawn_position",
            body!(set_default_spawn_position),
        ),
    ],
    outbound: &[
        (
            Phase::Configuration,
            "client_information",
            "client_information",
            client_information,
        ),
        (
            Phase::Game,
            "client_information",
            "client_information",
            client_information,
        ),
        (Phase::Game, "chat", "chat", last_seen_checksum),
        (
            Phase::Game,
            "player_command",
            "player_command",
            player_command,
        ),
        (Phase::Game, "use_item_on", "use_item_on", use_item_on),
        (
            Phase::Game,
            "move_player_pos",
            "move_player_pos",
            move_flags,
        ),
        (
            Phase::Game,
            "move_player_pos_rot",
            "move_player_pos_rot",
            move_flags,
        ),
        (
            Phase::Game,
            "move_player_rot",
            "move_player_rot",
            move_flags,
        ),
        (
            Phase::Game,
            "move_player_status_only",
            "move_player_status_only",
            move_flags,
        ),
        (Phase::Game, "move_vehicle", "move_vehicle", move_vehicle),
        (
            Phase::Game,
            "container_click",
            "container_click",
            container_click,
        ),
        (
            Phase::Game,
            "set_creative_mode_slot",
            "set_creative_mode_slot",
            creative_slot,
        ),
        (Phase::Game, "place_recipe", "place_recipe", unsent),
        (
            Phase::Game,
            "recipe_book_seen_recipe",
            "recipe_book_seen_recipe",
            unsent,
        ),
        (
            Phase::Game,
            "chat_command_signed",
            "chat_command_signed",
            last_seen_checksum,
        ),
    ],
    outbound_as: &[(Phase::Game, "player_input", player_input)],
    renamed: &[
        (
            Phase::Login,
            Direction::Clientbound,
            "game_profile",
            "login_finished",
        ),
        // Byte to VarInt, which match below 128: hotbar slots, and
        // container ids (vanilla counts them 1 to 100).
        (
            Phase::Game,
            Direction::Clientbound,
            "set_carried_item",
            "set_held_slot",
        ),
        (
            Phase::Game,
            Direction::Clientbound,
            "horse_screen_open",
            "mount_screen_open",
        ),
    ],
};

/// 1.21.1 ends the profile with `strictErrorHandling`, gone in 1.21.2.
fn login_finished(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let (_strict, profile) = body.split_last()?;
    Some(profile.into())
}

/// 1.21.2 appended the particle setting (an enum, one VarInt byte); 1.21.1
/// rejects the extra byte (Velocity disconnects over it).
fn client_information(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let (&particles, rest) = body.split_last()?;
    (particles < 0x80).then(|| rest.into())
}

fn copy(r: &mut Reader, out: &mut Vec<u8>, n: usize) -> Option<()> {
    out.extend_from_slice(r.take(n)?);
    Some(())
}

fn copy_varint(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    write_varint(out, r.varint()?);
    Some(())
}

fn f64(r: &mut Reader) -> Option<f64> {
    Some(f64::from_bits(r.u64()?))
}

fn f32(r: &mut Reader) -> Option<f32> {
    Some(f32::from_be_bytes(r.take(4)?.try_into().ok()?))
}

fn read_identifier(r: &mut Reader) -> Option<String> {
    let len = r.count()?;
    String::from_utf8(r.take(len)?.to_vec()).ok()
}

/// 1.21.2 ended the spawn info with the dimension's sea level, which 1.21.1
/// leaves to the client's own dimension data.
fn spawn_info(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    copy_varint(r, out)?;
    let dimension = read_identifier(r)?;
    write_varint(out, dimension.len() as i32);
    out.extend_from_slice(dimension.as_bytes());
    copy(r, out, 8 + 1 + 1 + 1 + 1)?;
    let death = r.u8()?;
    out.push(death);
    if death != 0 {
        hop::identifier(r, out)?;
        copy(r, out, 8)?;
    }
    copy_varint(r, out)?;
    let sea_level = match dimension.as_str() {
        "minecraft:the_nether" => 32,
        "minecraft:the_end" => 0,
        _ => 63,
    };
    write_varint(out, sea_level);
    *DIMENSION.lock().unwrap() = dimension;
    Some(())
}

fn login(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 2);
    let id = r.take(4)?;
    let player = i32::from_be_bytes(id.try_into().ok()?);
    PLAYER_ID.store(player, Ordering::Relaxed);
    SNEAKING.store(false, Ordering::Relaxed);
    let mut entities = HashMap::new();
    if let Some(kind) = entity_tables().old.type_id("minecraft:player") {
        entities.insert(player, kind);
    }
    *ENTITIES.lock().unwrap() = Some(entities);
    out.extend_from_slice(id);
    copy(&mut r, &mut out, 1)?;
    let levels = r.count()?;
    write_varint(&mut out, levels as i32);
    for _ in 0..levels {
        hop::identifier(&mut r, &mut out)?;
    }
    for _ in 0..3 {
        copy_varint(&mut r, &mut out)?;
    }
    copy(&mut r, &mut out, 3)?;
    spawn_info(&mut r, &mut out)?;
    copy(&mut r, &mut out, 1)?;
    (r.remaining() == 0).then(|| out.into())
}

fn respawn(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 2);
    spawn_info(&mut r, &mut out)?;
    copy(&mut r, &mut out, 1)?;
    (r.remaining() == 0).then(|| out.into())
}

/// `PositionMoveRotation` (1.21.2): position, delta, yaw, pitch.
fn position_move_rotation(
    out: &mut Vec<u8>,
    pos: [f64; 3],
    delta: [f64; 3],
    y_rot: f32,
    x_rot: f32,
) {
    for v in pos.into_iter().chain(delta) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&y_rot.to_be_bytes());
    out.extend_from_slice(&x_rot.to_be_bytes());
}

/// Relative axes kept their bits; a relative axis also kept its velocity in
/// 1.21.1, which 1.21.2 says with the matching `DELTA_` bit.
fn relatives(old: u8) -> i32 {
    let axes = i32::from(old & 0b1_1111);
    axes | ((axes & 0b111) << 5)
}

fn player_position(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let pos = [f64(&mut r)?, f64(&mut r)?, f64(&mut r)?];
    let (y_rot, x_rot) = (f32(&mut r)?, f32(&mut r)?);
    let flags = r.u8()?;
    let id = r.varint()?;
    let mut out = Vec::with_capacity(64);
    write_varint(&mut out, id);
    position_move_rotation(&mut out, pos, [0.0; 3], y_rot, x_rot);
    out.extend_from_slice(&relatives(flags).to_be_bytes());
    (r.remaining() == 0).then(|| out.into())
}

/// 1.21.1's only absolute entity move, a server's resync (an item
/// landing, every 400 ticks) as much as a teleport: since 1.21.2 that's
/// `entity_position_sync`, which also resets the base later relative moves
/// count from. As a 1.21.2 `teleport_entity` the base stayed put, and the
/// next move put the entity back near where it was last synced.
fn teleport_entity(_ids: &Ids, body: &[u8]) -> Option<Rewritten> {
    let mut r = Reader::new(body);
    let id = r.varint()?;
    let pos = [f64(&mut r)?, f64(&mut r)?, f64(&mut r)?];
    let angle = |b: u8| f32::from(b as i8) * 360.0 / 256.0;
    let (y_rot, x_rot) = (angle(r.u8()?), angle(r.u8()?));
    let on_ground = r.u8()?;
    let mut out = Vec::with_capacity(70);
    write_varint(&mut out, id);
    position_move_rotation(&mut out, pos, [0.0; 3], y_rot, x_rot);
    out.push(on_ground);
    // Bytes past 1.21.1's fields are a mod's own; they're dropped.
    Some(Rewritten::As("entity_position_sync", out.into()))
}

/// Shorts in 1/8000 of a block per tick, to 1.21.9's packed `LpVec3`.
fn set_entity_motion(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let id = r.varint()?;
    let mut axis = || Some(f64::from(r.u16()? as i16) / 8000.0);
    let motion = Vec3::new(axis()?, axis()?, axis()?);
    let mut out = Vec::with_capacity(16);
    write_varint(&mut out, id);
    LpVec3::from(motion).azalea_write(&mut out).ok()?;
    (r.remaining() == 0).then(|| out.into())
}

/// A block position and yaw, to 1.21.9's `RespawnData`: dimension,
/// position, yaw and pitch.
fn set_default_spawn_position(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let pos = r.take(8)?;
    let yaw = r.take(4)?;
    let dimension = DIMENSION.lock().unwrap().clone();
    let dimension = if dimension.is_empty() {
        "minecraft:overworld".to_string()
    } else {
        dimension
    };
    let mut out = Vec::with_capacity(48);
    write_varint(&mut out, dimension.len() as i32);
    out.extend_from_slice(dimension.as_bytes());
    out.extend_from_slice(pos);
    out.extend_from_slice(yaw);
    out.extend_from_slice(&0f32.to_be_bytes());
    (r.remaining() == 0).then(|| out.into())
}

/// 1.21.5 ended `LastSeenMessages.Update`, the last field of a chat message
/// and a signed command, with a checksum byte.
fn last_seen_checksum(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let (_checksum, rest) = body.split_last()?;
    Some(rest.into())
}

/// 1.21.5's heightmap kinds (`Heightmap.Types`), by the NBT key 1.21.1 used.
fn heightmap_kind(name: &[u8]) -> Option<i32> {
    Some(match name {
        b"WORLD_SURFACE_WG" => 0,
        b"WORLD_SURFACE" => 1,
        b"OCEAN_FLOOR_WG" => 2,
        b"OCEAN_FLOOR" => 3,
        b"MOTION_BLOCKING" => 4,
        b"MOTION_BLOCKING_NO_LEAVES" => 5,
        _ => return None,
    })
}

/// The heightmaps compound (nameless network NBT of long arrays) to
/// 1.21.5's list of `(kind, longs)`.
fn heightmaps(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    if r.u8()? != 10 {
        return None;
    }
    let mut maps = Vec::new();
    loop {
        let tag = r.u8()?;
        if tag == 0 {
            break;
        }
        let name_len = usize::from(r.u16()?);
        let name = r.take(name_len)?;
        if tag != 12 {
            return None;
        }
        let longs = usize::try_from(i32::from_be_bytes(r.take(4)?.try_into().ok()?)).ok()?;
        let data = r.take(longs.checked_mul(8)?)?;
        if let Some(kind) = heightmap_kind(name) {
            maps.push((kind, longs, data));
        }
    }
    write_varint(out, maps.len() as i32);
    for (kind, longs, data) in maps {
        write_varint(out, kind);
        write_varint(out, longs as i32);
        out.extend_from_slice(data);
    }
    Some(())
}

/// One 1.21.1 paletted container, with its data array's length prefix
/// (gone in 1.21.5) dropped.
fn unprefix_container(r: &mut Reader, out: &mut Vec<u8>, indirect_max: u8) -> Option<()> {
    let bits = r.u8()?;
    out.push(bits);
    if bits == 0 {
        copy_varint(r, out)?;
    } else if bits <= indirect_max {
        let entries = r.count()?;
        write_varint(out, entries as i32);
        for _ in 0..entries {
            copy_varint(r, out)?;
        }
    }
    let longs = r.count()?;
    copy(r, out, longs.checked_mul(8)?)
}

/// Every section in 1.21.11's layout and ids: prefixes dropped, then the
/// shared section remap.
fn sections(ids: &Ids, blob: &[u8]) -> Option<Vec<u8>> {
    let mut r = Reader::new(blob);
    let mut plain = Vec::with_capacity(blob.len());
    while r.remaining() > 0 {
        copy(&mut r, &mut plain, 2)?;
        unprefix_container(&mut r, &mut plain, hop::STATES_INDIRECT_MAX)?;
        unprefix_container(&mut r, &mut plain, hop::BIOMES_INDIRECT_MAX)?;
    }
    let mut r = Reader::new(&plain);
    let mut out = Vec::with_capacity(plain.len() + 64);
    while r.remaining() > 0 {
        copy(&mut r, &mut out, 2)?;
        hop::container(
            ids,
            &mut r,
            &mut out,
            hop::SECTION_STATES,
            hop::STATES_INDIRECT_MAX,
            true,
        )
        .ok()?;
        hop::container(
            ids,
            &mut r,
            &mut out,
            hop::SECTION_BIOMES,
            hop::BIOMES_INDIRECT_MAX,
            false,
        )
        .ok()?;
    }
    Some(out)
}

/// Heightmaps to the typed list, sections without length prefixes and in
/// 1.21.11's state ids, block entity types remapped; light data as it is.
fn level_chunk(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 64);
    copy(&mut r, &mut out, 8)?;
    heightmaps(&mut r, &mut out)?;
    let length = r.count()?;
    let blob = sections(ids, r.take(length)?)?;
    write_varint(&mut out, blob.len() as i32);
    out.extend_from_slice(&blob);
    let entities = r.count()?;
    write_varint(&mut out, entities as i32);
    for _ in 0..entities {
        copy(&mut r, &mut out, 1 + 2)?;
        let kind = hop::shifted(ids, IdSpace::BlockEntityType, r.varint()?)?;
        write_varint(&mut out, kind);
        let start = r.pos();
        r.skip_nbt()?;
        out.extend_from_slice(&body[start..r.pos()]);
    }
    out.extend_from_slice(r.rest());
    Some(out.into())
}

/// 1.21.6 dropped the two sneak actions that led the list.
const SNEAK_ACTIONS: i32 = 2;
const PRESS_SHIFT_KEY: i32 = 0;
const RELEASE_SHIFT_KEY: i32 = 1;

fn player_command(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 1);
    copy_varint(&mut r, &mut out)?;
    write_varint(&mut out, r.varint()? + SNEAK_ACTIONS);
    out.extend_from_slice(r.rest());
    Some(out.into())
}

/// 1.21.2's input flags (forward 1, back 2, left 4, right 8, jump 16, shift
/// 32, sprint 64) to 1.21.1's: sideways and forward impulses, then jump 1
/// and shift 2. Sneaking was a `player_command` in 1.21.1, so a change of
/// the shift flag goes out as that instead (1.21.1 reads this packet only
/// for steering vehicles, and the next input change sends it again).
fn player_input(_ids: &Ids, body: &[u8]) -> Option<(&'static str, Box<[u8]>)> {
    let flags = *body.first()?;
    let shift = flags & 32 != 0;
    if SNEAKING.swap(shift, Ordering::Relaxed) != shift {
        let mut out = Vec::with_capacity(6);
        write_varint(&mut out, PLAYER_ID.load(Ordering::Relaxed));
        write_varint(
            &mut out,
            if shift {
                PRESS_SHIFT_KEY
            } else {
                RELEASE_SHIFT_KEY
            },
        );
        write_varint(&mut out, 0);
        return Some(("player_command", out.into()));
    }
    Some(("player_input", input_1_21_1(flags)))
}

fn input_1_21_1(flags: u8) -> Box<[u8]> {
    let axis = |plus: u8, minus: u8| f32::from(flags & plus != 0) - f32::from(flags & minus != 0);
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&axis(4, 8).to_be_bytes());
    out.extend_from_slice(&axis(1, 2).to_be_bytes());
    out.push(u8::from(flags & 16 != 0) | (u8::from(flags & 32 != 0) << 1));
    out.into()
}

/// 1.21.2 added the world border flag after `inside` in a block hit.
fn use_item_on(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_varint(&mut r, &mut out)?; // hand
    copy(&mut r, &mut out, 8)?; // position
    copy_varint(&mut r, &mut out)?; // face
    copy(&mut r, &mut out, 12 + 1)?; // cursor, inside
    r.take(1)?; // world border
    copy_varint(&mut r, &mut out)?; // sequence
    (r.remaining() == 0).then(|| out.into())
}

/// 1.21.2 packed horizontal collision beside on-ground; 1.21.1 reads any
/// nonzero byte as on-ground. The flags end every movement packet.
fn move_flags(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let (&flags, rest) = body.split_last()?;
    let mut out = rest.to_vec();
    out.push(flags & 1);
    Some(out.into())
}

/// 1.21.3 ended a vehicle move with on-ground.
fn move_vehicle(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let (_on_ground, rest) = body.split_last()?;
    Some(rest.into())
}

/// 1.21.5 sends hashed stacks; 1.21.1 wants whole ones. Claiming no slot
/// changed and an empty cursor makes the server resend what did change.
fn container_click(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(16);
    let container = r.varint()?;
    out.push(u8::try_from(container).ok()?);
    copy_varint(&mut r, &mut out)?; // state id
    copy(&mut r, &mut out, 2 + 1)?; // slot, button
    copy_varint(&mut r, &mut out)?; // click type
    write_varint(&mut out, 0); // changed slots
    write_varint(&mut out, 0); // carried: empty
    Some(out.into())
}

/// The item and count only: components changed shape across 1.21.2 to
/// 1.21.5, and this hop has no walker back for them yet.
fn creative_slot(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(12);
    copy(&mut r, &mut out, 2)?;
    let count = r.varint()?;
    write_varint(&mut out, count);
    if count > 0 {
        // A modded item too: shifted back past 1.21.1's vanilla items.
        write_varint(&mut out, hop::unshifted(ids, IdSpace::Item, r.varint()?)?);
        write_varint(&mut out, 0);
        write_varint(&mut out, 0);
    }
    Some(out.into())
}

/// Recipes are named differently since 1.21.2 and aren't translated.
fn unsent(_ids: &Ids, _body: &[u8]) -> Option<Box<[u8]>> {
    None
}

struct EntityTables {
    old: entity_data::Layouts,
    new: entity_data::Layouts,
    /// Per 1.21.1 type id, per index: where the field goes. Indices are
    /// 26.3's (the 774 and 775 hops convert values, never indices, and 26.1
    /// and 26.3 moved fields: `AgeableMob.AGE_LOCKED`, slimes made
    /// ageable); serializers and values stay 1.21.11's for those hops.
    moves: HashMap<i32, Vec<Option<entity_data::Moved>>>,
    /// 1.21.1 types split since: the 1.21.11 type they become (the wood or
    /// the throw that picked it isn't known at spawn).
    split: HashMap<i32, i32>,
    falling_block: Option<i32>,
}

/// Types renamed or split after 1.21.1.
const TYPE_RENAMES: [(&str, &str); 3] = [
    ("minecraft:boat", "minecraft:oak_boat"),
    ("minecraft:chest_boat", "minecraft:oak_chest_boat"),
    ("minecraft:potion", "minecraft:splash_potion"),
];

fn entity_tables() -> &'static EntityTables {
    static TABLES: OnceLock<EntityTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let parse = |json: &str, which: &str| {
            entity_data::Layouts::parse(json)
                .unwrap_or_else(|e| panic!("embedded {which} entity data: {e}"))
        };
        let old = parse(include_str!("data/entity-data-1.21.1.json"), "1.21.1");
        let new = parse(include_str!("data/entity-data-1.21.11.json"), "1.21.11");
        let native = parse(include_str!("data/entity-data-26.3.json"), "26.3");
        let moves = entity_data::map(&old, &new, &native, &TYPE_RENAMES);
        let split = TYPE_RENAMES
            .iter()
            .filter_map(|(from, to)| Some((old.type_id(from)?, new.type_id(to)?)))
            .collect();
        let falling_block = old.type_id("minecraft:falling_block");
        EntityTables {
            old,
            new,
            moves,
            split,
            falling_block,
        }
    })
}

/// 1.21.1 ended a spawn with its velocity in shorts; 1.21.9 sends it packed
/// after the position. The type is remapped (a split type to its plain
/// kind) and remembered, since entity data is laid out by it.
fn add_entity(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let tables = entity_tables();
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 4);
    let id = r.varint()?;
    write_varint(&mut out, id);
    copy(&mut r, &mut out, 16)?;
    let kind = r.varint()?;
    let native = match tables.split.get(&kind) {
        Some(&to) => to,
        None => hop::shifted(ids, IdSpace::EntityType, kind)?,
    };
    write_varint(&mut out, native);
    if let Some(entities) = ENTITIES.lock().unwrap().as_mut() {
        entities.insert(id, kind);
    }
    copy(&mut r, &mut out, 3 * 8)?;
    let angles = r.take(3)?;
    let data = r.varint()?;
    let mut axis = || Some(f64::from(r.u16()? as i16) / 8000.0);
    let motion = Vec3::new(axis()?, axis()?, axis()?);
    LpVec3::from(motion).azalea_write(&mut out).ok()?;
    out.extend_from_slice(angles);
    let data = if Some(kind) == tables.falling_block {
        hop::state(ids, data)?
    } else {
        data
    };
    write_varint(&mut out, data);
    Some(out.into())
}

/// One entity data value as 1.21.1 sent it.
enum Value {
    Bytes(Vec<u8>),
    /// Read, but with no counterpart: the entry is left out.
    Skip,
    /// A shape this hop can't measure yet (particles, an inline variant):
    /// the entries after it are lost.
    Stop,
    /// Written, but where it ended isn't known (a stack with an unreadable
    /// component, kept without it): the entries after it are lost.
    Last(Vec<u8>),
}

fn copy_nbt(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    let start = r.pos();
    r.skip_nbt()?;
    out.extend_from_slice(r.slice_from(start));
    Some(())
}

/// Reads one value of 1.21.1's serializer `name`, already in 1.21.11's ids.
fn read_value(ids: &Ids, name: &str, r: &mut Reader) -> Option<Value> {
    let mut out = Vec::new();
    match name {
        "BYTE" | "BOOLEAN" => copy(r, &mut out, 1)?,
        "FLOAT" => copy(r, &mut out, 4)?,
        "BLOCK_POS" => copy(r, &mut out, 8)?,
        "ROTATIONS" | "VECTOR3" => copy(r, &mut out, 12)?,
        "QUATERNION" => copy(r, &mut out, 16)?,
        "PARTICLE" => particle(ids, r, &mut out)?,
        "PARTICLES" => {
            let n = r.count()?;
            write_varint(&mut out, n as i32);
            for _ in 0..n {
                particle(ids, r, &mut out)?;
            }
        }
        "INT"
        | "DIRECTION"
        | "OPTIONAL_UNSIGNED_INT"
        | "POSE"
        | "CAT_VARIANT"
        | "FROG_VARIANT"
        | "SNIFFER_STATE"
        | "ARMADILLO_STATE" => copy_varint(r, &mut out)?,
        "LONG" => write_varlong(&mut out, r.varlong()?),
        "VILLAGER_DATA" => {
            for _ in 0..3 {
                copy_varint(r, &mut out)?;
            }
        }
        "STRING" => {
            let len = r.count()?;
            write_varint(&mut out, len as i32);
            copy(r, &mut out, len)?;
        }
        "COMPONENT" | "COMPOUND_TAG" => copy_nbt(r, &mut out)?,
        "OPTIONAL_COMPONENT" => {
            let present = r.u8()?;
            out.push(present);
            if present != 0 {
                copy_nbt(r, &mut out)?;
            }
        }
        "OPTIONAL_BLOCK_POS" | "OPTIONAL_UUID" => {
            let present = r.u8()?;
            out.push(present);
            if present != 0 {
                copy(r, &mut out, if name == "OPTIONAL_UUID" { 16 } else { 8 })?;
            }
        }
        "OPTIONAL_GLOBAL_POS" => {
            let present = r.u8()?;
            out.push(present);
            if present != 0 {
                hop::identifier(r, &mut out)?;
                copy(r, &mut out, 8)?;
            }
        }
        "BLOCK_STATE" => write_varint(&mut out, hop::state(ids, r.varint()?)?),
        "OPTIONAL_BLOCK_STATE" => {
            let state = r.varint()?;
            let state = if state == 0 {
                0
            } else {
                hop::state(ids, state)?
            };
            write_varint(&mut out, state);
        }
        // A holder (id + 1, or 0 and the variant inline) to 1.21.5's plain
        // registry id.
        "WOLF_VARIANT" => match r.varint()? {
            0 => return Some(Value::Stop),
            id => write_varint(&mut out, id - 1),
        },
        "PAINTING_VARIANT" => match r.varint()? {
            0 => {
                r.varint()?;
                r.varint()?;
                hop::identifier(r, &mut Vec::new())?;
                return Some(Value::Skip);
            }
            id => write_varint(&mut out, id),
        },
        "ITEM_STACK" => {
            if stack(ids, r, &mut out).is_err() {
                return Some(Value::Last(out));
            }
        }
        _ => return Some(Value::Stop),
    }
    Some(Value::Bytes(out))
}

/// A value of 1.21.1's serializer `old` as 1.21.11's `new`, if it converts.
fn convert(old: &str, new: &str, bytes: Vec<u8>) -> Option<Vec<u8>> {
    match (old, new) {
        _ if old == new => Some(bytes),
        ("OPTIONAL_UUID", "OPTIONAL_LIVING_ENTITY_REFERENCE") => Some(bytes),
        // The main hand byte became an enum; left 0, right 1 either way.
        ("BYTE", "HUMANOID_ARM") => {
            let mut out = Vec::new();
            write_varint(&mut out, i32::from(*bytes.first()?));
            Some(out)
        }
        // The mooshroom's type went from its name to its id.
        ("STRING", "INT") => {
            let mut out = Vec::new();
            write_varint(&mut out, i32::from(bytes.ends_with(b"brown")));
            Some(out)
        }
        _ => None,
    }
}

/// Entries re-indexed by the entity's generated layouts, serializers and
/// values converted; fields 1.21.11 dropped are left out. An entity whose
/// type isn't known (a modded one) keeps no entries.
fn entity_data(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let tables = entity_tables();
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 8);
    let id = r.varint()?;
    write_varint(&mut out, id);
    let kind = ENTITIES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|e| e.get(&id).copied());
    let moves = kind.and_then(|kind| tables.moves.get(&kind));
    // A modded entity: the values this hop can read go to the mod API
    // (`modapi::on_entity_data`), by data index, before they're dropped.
    let mut modded: Vec<(u8, serde_json::Value)> = Vec::new();
    loop {
        let index = r.u8()?;
        if index == 0xFF {
            break;
        }
        let serializer = r.varint()?;
        let old = tables
            .old
            .serializers
            .get(usize::try_from(serializer).ok()?)?;
        let (bytes, last) = match read_value(ids, old, &mut r)? {
            Value::Bytes(bytes) => (bytes, false),
            Value::Last(bytes) => (bytes, true),
            Value::Skip => continue,
            Value::Stop => break,
        };
        let Some(moved) = moves.and_then(|m| m.get(usize::from(index))?.as_ref()) else {
            if kind.is_none()
                && let Some(value) = data_json(old, &bytes)
            {
                modded.push((index, value));
            }
            if last {
                break;
            }
            continue;
        };
        let Some(bytes) = convert(old, &moved.serializer, bytes) else {
            if last {
                break;
            }
            continue;
        };
        out.push(moved.index);
        write_varint(&mut out, tables.new.serializer_id(&moved.serializer)?);
        out.extend_from_slice(&bytes);
        if last {
            break;
        }
    }
    out.push(0xFF);
    if !modded.is_empty() {
        crate::modapi::on_entity_data(id, modded);
    }
    Some(out.into())
}

/// An entity data value as JSON, for a mod port: the plain kinds (numbers,
/// text, positions, vectors, UUIDs, NBT, poses); `None` for the rest.
fn data_json(serializer: &str, bytes: &[u8]) -> Option<serde_json::Value> {
    use serde_json::{Value as J, json};
    let mut r = torch_mod_api::buf::Reader::new(bytes);
    let floats = |r: &mut torch_mod_api::buf::Reader, n: usize| -> Option<J> {
        (0..n)
            .map(|_| r.f32().map(|f| json!(f)))
            .collect::<Option<Vec<_>>>()
            .map(J::Array)
    };
    Some(match serializer {
        "BYTE" => json!(r.u8()?),
        "BOOLEAN" => json!(r.bool()?),
        "INT" | "DIRECTION" | "POSE" | "BLOCK_STATE" => json!(r.varint()?),
        "LONG" => json!(r.varlong()?),
        "FLOAT" => json!(r.f32()?),
        "STRING" => json!(r.string()?),
        "ROTATIONS" | "VECTOR3" => floats(&mut r, 3)?,
        "QUATERNION" => floats(&mut r, 4)?,
        "BLOCK_POS" => json!(r.block_pos()?),
        "OPTIONAL_BLOCK_POS" => match r.bool()? {
            true => json!(r.block_pos()?),
            false => J::Null,
        },
        "OPTIONAL_UUID" => match r.bool()? {
            true => json!(format!("{:032x}", r.uuid()?)),
            false => J::Null,
        },
        "OPTIONAL_UNSIGNED_INT" | "OPTIONAL_BLOCK_STATE" => match r.varint()? {
            0 => J::Null,
            v => json!(v - 1),
        },
        "COMPOUND_TAG" => r.nbt()?,
        _ => return None,
    })
}

/// Stacks walked one by one. Once one can't be read to its end, the rest
/// of the slots arrive empty: nothing marks where its bytes stop.
fn stacks(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>, count: usize) -> bool {
    let mut readable = true;
    for _ in 0..count {
        if readable {
            readable = stack(ids, r, out).is_ok();
        } else {
            write_varint(out, 0);
        }
    }
    readable
}

/// Container id (a byte, 1.21.2's VarInt: the same below 128), state, the
/// slots, then the cursor. Slots past a modded component are recovered by
/// reading on to the packet's end (`stacks_to_end`); failing that they
/// arrive empty.
fn container_set_content(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    let container = r.u8()?;
    out.push(container);
    copy_varint(&mut r, &mut out)?;
    let count = r.count()?;
    write_varint(&mut out, count as i32);
    let max = (container == 0).then_some(99);
    let found = super::v767_items::stacks_to_end(ids, body, r.pos(), count + 1, max);
    if found.is_none() {
        dump_once(body, false);
    }
    match found {
        Some(stacks) => out.extend_from_slice(&stacks),
        None => {
            stacks(ids, &mut r, &mut out, count);
            write_varint(&mut out, 0);
        }
    }
    Some(out.into())
}

/// The first packet of each kind a rewrite couldn't read, as raw bytes, so
/// the shape it missed can be worked out from the log.
fn dump_failed(kind: &'static str, body: &[u8]) {
    static DUMPED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut dumped = DUMPED.lock().unwrap();
    if dumped.contains(&kind) {
        return;
    }
    dumped.push(kind);
    let hex: String = body
        .iter()
        .take(65536)
        .map(|b| format!("{b:02x}"))
        .collect();
    crate::log_warn!(
        "proto",
        "1.21.1 {kind} didn't translate, {} bytes: {hex}",
        body.len()
    );
}

fn logged_commands(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let out = commands(ids, body);
    if out.is_none() {
        dump_failed("commands", body);
    }
    out
}

fn logged_advancements(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let out = update_advancements(ids, body);
    if out.is_none() {
        dump_failed("update_advancements", body);
    }
    out
}

/// The first inventory that needed recovering, as raw bytes, so a misread
/// can be worked out from the log.
fn dump_once(body: &[u8], recovered: bool) {
    static DUMPED: AtomicBool = AtomicBool::new(false);
    if DUMPED.swap(true, Ordering::Relaxed) {
        return;
    }
    let hex: String = body.iter().take(8192).map(|b| format!("{b:02x}")).collect();
    crate::log_warn!(
        "proto",
        "1.21.1 inventory needed recovery (recovered: {recovered}), {} bytes: {hex}",
        body.len()
    );
}

fn container_set_slot(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy(&mut r, &mut out, 1)?;
    copy_varint(&mut r, &mut out)?;
    copy(&mut r, &mut out, 2)?;
    let max = (body.first() == Some(&0)).then_some(99);
    match super::v767_items::stacks_to_end(ids, body, r.pos(), 1, max) {
        Some(stack) => out.extend_from_slice(&stack),
        None => {
            stacks(ids, &mut r, &mut out, 1);
        }
    }
    Some(out.into())
}

/// Slot bytes (bit 7 marks another to follow) and stacks. A stack that
/// can't be read to its end becomes the last.
fn set_equipment(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_varint(&mut r, &mut out)?;
    loop {
        let slot = r.u8()?;
        let at = out.len();
        out.push(slot);
        if stack(ids, &mut r, &mut out).is_err() {
            out[at] &= 0x7F;
            break;
        }
        if slot & 0x80 == 0 {
            break;
        }
    }
    Some(out.into())
}

/// Position, then the block entity type (ids moved), then its tag.
fn block_entity_data(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy(&mut r, &mut out, 8)?;
    write_varint(
        &mut out,
        hop::shifted(ids, IdSpace::BlockEntityType, r.varint()?)?,
    );
    out.extend_from_slice(r.rest());
    Some(out.into())
}

/// 1.21.1 marks a frozen day time by sending it negative (-1 for 0);
/// 1.21.2 sends it as is with a "ticking" flag.
fn set_time(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(17);
    copy(&mut r, &mut out, 8)?;
    let day = r.u64()? as i64;
    let ticking = day >= 0;
    let day = match day {
        -1 => 0,
        d if d < 0 => -d,
        d => d,
    };
    out.extend_from_slice(&day.to_be_bytes());
    out.push(u8::from(ticking));
    Some(out.into())
}

/// An item's cooldown became its cooldown group's name (1.21.2), which by
/// default is the item's own.
fn cooldown(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    use azalea_registry::Registry as _;
    let mut r = Reader::new(body);
    let item = r.varint()?;
    let name = match crate::protocol::wire_registry_name(
        crate::protocol::registries::RegistryId::Item,
        u32::try_from(item).ok()?,
    ) {
        Some(name) => name?,
        None => {
            let native = hop::shifted(ids, IdSpace::Item, item)?;
            azalea_registry::builtin::ItemKind::from_u32(u32::try_from(native).ok()?)?.to_string()
        }
    };
    let mut out = Vec::with_capacity(name.len() + 4);
    write_varint(&mut out, name.len() as i32);
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(r.rest());
    Some(out.into())
}

/// 1.21.4 added "always show" after "override limiter".
fn level_particles(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 4);
    copy(&mut r, &mut out, 1)?;
    out.push(0);
    copy(&mut r, &mut out, 3 * 8 + 4 * 4 + 4)?;
    particle(ids, &mut r, &mut out)?;
    Some(out.into())
}

fn particle_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        serde_json::from_str(include_str!("data/particle_type-1.21.1.json"))
            .expect("embedded particle names")
    })
}

/// Three float channels to 1.21.2's packed RGB.
fn rgb(r: &mut Reader) -> Option<i32> {
    let mut packed = 0;
    for _ in 0..3 {
        let channel = (f32(r)?.clamp(0.0, 1.0) * 255.0).round() as i32;
        packed = (packed << 8) | channel;
    }
    Some(packed)
}

/// A particle and its options, 1.21.1's shape to 1.21.11's.
fn particle(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    let old = r.varint()?;
    write_varint(out, hop::shifted(ids, IdSpace::ParticleType, old)?);
    let name = particle_names().get(usize::try_from(old).ok()?)?;
    match name.as_str() {
        "minecraft:block"
        | "minecraft:block_marker"
        | "minecraft:falling_dust"
        | "minecraft:dust_pillar" => write_varint(out, hop::state(ids, r.varint()?)?),
        "minecraft:dust" => {
            out.extend_from_slice(&rgb(r)?.to_be_bytes());
            copy(r, out, 4)?;
        }
        "minecraft:dust_color_transition" => {
            out.extend_from_slice(&rgb(r)?.to_be_bytes());
            out.extend_from_slice(&rgb(r)?.to_be_bytes());
            copy(r, out, 4)?;
        }
        "minecraft:entity_effect" | "minecraft:sculk_charge" => copy(r, out, 4)?,
        "minecraft:shriek" => copy_varint(r, out)?,
        "minecraft:item" => stack(ids, r, out).ok()?,
        "minecraft:vibration" => {
            match r.varint()? {
                0 => {
                    write_varint(out, 0);
                    copy(r, out, 8)?;
                }
                kind => {
                    write_varint(out, kind);
                    copy_varint(r, out)?;
                    copy(r, out, 4)?;
                }
            }
            copy_varint(r, out)?;
        }
        _ => {}
    }
    Some(())
}

/// 1.21.2 rebuilt explosions: centre, radius, how many blocks broke, the
/// player's knockback, one particle (the client used to pick small or
/// large), the sound and weighted block particles (none here).
fn explode(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy(&mut r, &mut out, 3 * 8)?;
    let power = f32(&mut r)?;
    out.extend_from_slice(&power.to_be_bytes());
    let blocks = r.count()?;
    r.take(blocks.checked_mul(3)?)?;
    out.extend_from_slice(&(blocks as i32).to_be_bytes());
    let knockback = [f32(&mut r)?, f32(&mut r)?, f32(&mut r)?];
    out.push(1);
    for k in knockback {
        out.extend_from_slice(&f64::from(k).to_be_bytes());
    }
    let interaction = r.varint()?;
    let mut small = Vec::new();
    particle(ids, &mut r, &mut small)?;
    let mut large = Vec::new();
    particle(ids, &mut r, &mut large)?;
    // As 1.21.1's client chose: large past power 2 unless nothing broke
    // (`BlockInteraction.KEEP` is 0).
    out.extend_from_slice(if power >= 2.0 && interaction != 0 {
        &large
    } else {
        &small
    });
    hop::sound_holder(ids, &mut r, &mut out)?;
    write_varint(&mut out, 0);
    Some(out.into())
}

/// Name-tag visibility and collision went from names to ids (1.21.5).
fn set_player_team(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    hop::identifier(&mut r, &mut out)?;
    let method = r.u8()?;
    out.push(method);
    if matches!(method, 0 | 2) {
        copy_nbt(&mut r, &mut out)?;
        copy(&mut r, &mut out, 1)?;
        let mut name = || -> Option<String> {
            let len = r.count()?;
            String::from_utf8(r.take(len)?.to_vec()).ok()
        };
        let visibility = match name()?.as_str() {
            "never" => 1,
            "hideForOtherTeams" => 2,
            "hideForOwnTeam" => 3,
            _ => 0,
        };
        let collision = match name()?.as_str() {
            "never" => 1,
            "pushOtherTeams" => 2,
            "pushOwnTeam" => 3,
            _ => 0,
        };
        write_varint(&mut out, visibility);
        write_varint(&mut out, collision);
        copy_varint(&mut r, &mut out)?;
        copy_nbt(&mut r, &mut out)?;
        copy_nbt(&mut r, &mut out)?;
    }
    out.extend_from_slice(r.rest());
    Some(out.into())
}

/// 1.21.1 sent every recipe; 1.21.2 sends the property sets and stonecutter
/// recipes the client predicts with. Recipes aren't translated yet, so both
/// arrive empty.
fn update_recipes(_ids: &Ids, _body: &[u8]) -> Option<Box<[u8]>> {
    Some(vec![0, 0].into())
}

/// Biome containers per chunk, with the data arrays' length prefixes
/// dropped (1.21.5). Biome ids are the server's synced registry's.
fn chunks_biomes(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    let chunks = r.count()?;
    write_varint(&mut out, chunks as i32);
    for _ in 0..chunks {
        copy(&mut r, &mut out, 8)?;
        let len = r.count()?;
        let mut sections = Reader::new(r.take(len)?);
        let mut plain = Vec::with_capacity(len);
        while sections.remaining() > 0 {
            unprefix_container(&mut sections, &mut plain, hop::BIOMES_INDIRECT_MAX)?;
        }
        write_varint(&mut out, plain.len() as i32);
        out.extend_from_slice(&plain);
    }
    Some(out.into())
}

fn item_cost(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    write_varint(out, hop::shifted(ids, IdSpace::Item, r.varint()?)?);
    copy_varint(r, out)?;
    exact_components(ids, r, out).ok()
}

fn merchant_offers(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_varint(&mut r, &mut out)?;
    let offers = r.count()?;
    write_varint(&mut out, offers as i32);
    for _ in 0..offers {
        item_cost(ids, &mut r, &mut out)?;
        stack(ids, &mut r, &mut out).ok()?;
        let has_b = r.u8()?;
        out.push(has_b);
        if has_b != 0 {
            item_cost(ids, &mut r, &mut out)?;
        }
        copy(&mut r, &mut out, 1 + 4 * 4 + 4 + 4)?;
    }
    out.extend_from_slice(r.rest());
    Some(out.into())
}

fn argument_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        serde_json::from_str(include_str!("data/command_argument_type-1.21.1.json"))
            .expect("embedded argument names")
    })
}

/// A parser's properties, by its 1.21.1 name.
fn argument_properties(name: &str, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    let ranged = |r: &mut Reader, out: &mut Vec<u8>, width: usize| -> Option<()> {
        let flags = r.u8()?;
        out.push(flags);
        if flags & 1 != 0 {
            copy(r, out, width)?;
        }
        if flags & 2 != 0 {
            copy(r, out, width)?;
        }
        Some(())
    };
    match name {
        "brigadier:float" | "brigadier:integer" => ranged(r, out, 4),
        "brigadier:double" | "brigadier:long" => ranged(r, out, 8),
        "brigadier:string" => copy_varint(r, out),
        "minecraft:entity" | "minecraft:score_holder" => copy(r, out, 1),
        "minecraft:time" => copy(r, out, 4),
        "minecraft:resource_or_tag"
        | "minecraft:resource_or_tag_key"
        | "minecraft:resource"
        | "minecraft:resource_key" => hop::identifier(r, out),
        _ => Some(()),
    }
}

/// A parser's name by the server's id: its synced registry's when it sent
/// one, else vanilla 1.21.1's.
fn parser_name(id: i32) -> Option<String> {
    let id = u32::try_from(id).ok()?;
    match crate::protocol::wire_registry_name(
        crate::protocol::registries::RegistryId::CommandArgumentType,
        id,
    ) {
        Some(name) => name,
        None => argument_names().get(id as usize).cloned(),
    }
}

fn is_vanilla_parser(name: &str) -> bool {
    argument_names().iter().any(|n| n == name)
}

/// Mod parsers' property shapes learned from earlier command trees.
static PARSER_SHAPES: Mutex<Vec<(String, Vec<usize>)>> = Mutex::new(Vec::new());

/// A parser nested in another's properties (CC: Tweaked's repeat
/// argument): its id, then its own properties; a mod's has none known.
fn nested_parser(r: &mut Reader) -> Option<()> {
    let name = parser_name(r.varint()?)?;
    if is_vanilla_parser(&name) {
        argument_properties(&name, r, &mut Vec::new())?;
    }
    Some(())
}

/// The pieces a mod parser's properties are most likely built from, as
/// readers that skip them: a flag, a VarInt, an int, a long, a string, an
/// NBT tag (a text component), a nested parser.
const PARSER_PIECES: [fn(&mut Reader) -> Option<()>; 7] = [
    |r| r.take(1).map(drop),
    |r| r.varint().map(drop),
    |r| r.take(4).map(drop),
    |r| r.take(8).map(drop),
    |r| {
        let len = r.count()?;
        r.take(len).map(drop)
    },
    |r| r.skip_nbt(),
    nested_parser,
];

/// Shapes to try, shortest first: none, then every sequence of one to
/// three pieces (CC: Tweaked's repeat argument is a flag, the inner
/// parser's id and a text component).
fn parser_guesses() -> impl Iterator<Item = Vec<usize>> {
    let n = PARSER_PIECES.len();
    let ones = (0..n).map(|a| vec![a]);
    let twos = (0..n * n).map(move |i| vec![i / n, i % n]);
    let threes = (0..n * n * n).map(move |i| vec![i / (n * n), i / n % n, i % n]);
    std::iter::once(Vec::new())
        .chain(ones)
        .chain(twos)
        .chain(threes)
}

fn skip_pieces(r: &mut Reader, shape: &[usize]) -> Option<()> {
    shape.iter().try_for_each(|piece| PARSER_PIECES[*piece](r))
}

/// One node's fields up to its parser's properties, read and checked
/// against a tree of `total` nodes: where the properties start, and the
/// parser's name and suggestion flag when it's an argument.
struct NodeHead {
    props: usize,
    parser: Option<(String, bool)>,
}

fn node_head(r: &mut Reader, total: usize) -> Option<NodeHead> {
    let index = |r: &mut Reader| -> Option<()> {
        (usize::try_from(r.varint()?).ok()? < total).then_some(())
    };
    let flags = r.u8()?;
    let kind = flags & 0x03;
    if flags & 0xE0 != 0 || kind == 3 {
        return None;
    }
    for _ in 0..r.count()? {
        index(r)?;
    }
    if flags & 0x08 != 0 {
        index(r)?;
    }
    if kind == 1 || kind == 2 {
        let len = r.count()?;
        let name = r.take(len)?;
        if name.is_empty() || !name.iter().all(|b| b.is_ascii_graphic()) {
            return None;
        }
    }
    let parser = if kind == 2 {
        Some((parser_name(r.varint()?)?, flags & 0x10 != 0))
    } else {
        None
    };
    Some(NodeHead {
        props: r.pos(),
        parser,
    })
}

/// A suggestion provider's id: a lowercase `namespace:path`.
fn suggestion_id(r: &mut Reader) -> Option<()> {
    let len = r.count()?;
    let id = r.take(len)?;
    (!id.is_empty()
        && id.contains(&b':')
        && id
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-./:".contains(b)))
    .then_some(())
}

/// A node with a mod's parser, and the shapes tried for its properties.
struct Pick {
    node: usize,
    /// Where the node starts, for the dead set.
    start: usize,
    props: usize,
    name: String,
    suggests: bool,
    tries: Vec<Vec<usize>>,
    next: usize,
}

/// For each node with a mod's parser, the shape of its properties that
/// lets the whole tree read to its end (the root index, then nothing).
/// Learned shapes go first, then shortest first; a shape is undone when a
/// later node stops reading. A depth-first search kept as a list of picks,
/// not recursion over thousands of nodes; a node whose every shape failed
/// from some position is remembered as dead there.
fn mod_parser_shapes(
    body: &[u8],
    start: usize,
    total: usize,
) -> Option<HashMap<usize, Vec<usize>>> {
    let all: Vec<Vec<usize>> = parser_guesses().collect();
    let mut picks: Vec<Pick> = Vec::new();
    let mut dead: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut budget = 1_000_000usize;
    loop {
        budget = budget.checked_sub(1)?;
        let read = read_on(body, start, total, &picks, &dead, &all);
        match read {
            Read::Done => break,
            Read::Pick(pick) => {
                picks.push(pick);
                continue;
            }
            Read::Failed => {}
        }
        // The last pick's next shape, or it's dead here and the one before
        // it moves on.
        loop {
            let pick = picks.last_mut()?;
            pick.next += 1;
            if pick.next < pick.tries.len() {
                break;
            }
            dead.insert((pick.node, pick.start));
            picks.pop();
        }
    }
    let mut learned = PARSER_SHAPES.lock().unwrap();
    let mut shapes = HashMap::new();
    for pick in picks {
        let shape = pick.tries[pick.next].clone();
        if !learned.iter().any(|(n, _)| *n == pick.name) {
            learned.push((pick.name.clone(), shape.clone()));
        }
        shapes.insert(pick.node, shape);
    }
    Some(shapes)
}

enum Read {
    Done,
    /// Reached a mod parser not yet picked.
    Pick(Pick),
    Failed,
}

/// Reads on from the last pick (with its current shape), or from the start.
fn read_on(
    body: &[u8],
    start: usize,
    total: usize,
    picks: &[Pick],
    dead: &std::collections::HashSet<(usize, usize)>,
    all: &[Vec<usize>],
) -> Read {
    let go = || -> Option<Read> {
        let mut r = Reader::new(body);
        let mut i = 0;
        match picks.last() {
            None => {
                r.take(start)?;
            }
            Some(pick) => {
                r.take(pick.props)?;
                skip_pieces(&mut r, &pick.tries[pick.next])?;
                if pick.suggests {
                    suggestion_id(&mut r)?;
                }
                i = pick.node + 1;
            }
        }
        while i < total {
            if dead.contains(&(i, r.pos())) {
                return None;
            }
            let node_start = r.pos();
            let head = node_head(&mut r, total)?;
            if let Some((name, suggests)) = head.parser {
                if !is_vanilla_parser(&name) {
                    // The shape this parser took last in this tree, then one
                    // learned from an earlier tree, then every shape.
                    let in_tree = picks
                        .iter()
                        .rev()
                        .find(|p| p.name == name)
                        .map(|p| p.tries[p.next].clone());
                    let learned = PARSER_SHAPES
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|(n, _)| *n == name)
                        .map(|(_, shape)| shape.clone());
                    let tries = in_tree
                        .into_iter()
                        .chain(learned)
                        .chain(all.iter().cloned())
                        .collect();
                    return Some(Read::Pick(Pick {
                        node: i,
                        start: node_start,
                        props: head.props,
                        name,
                        suggests,
                        tries,
                        next: 0,
                    }));
                }
                argument_properties(&name, &mut r, &mut Vec::new())?;
                if suggests {
                    suggestion_id(&mut r)?;
                }
            }
            i += 1;
        }
        let root = usize::try_from(r.varint()?).ok()?;
        (root < total && r.remaining() == 0).then_some(Read::Done)
    };
    go().unwrap_or(Read::Failed)
}

/// The command tree, its parsers moved to 1.21.11's ids. A mod's parser
/// (NeoForge sends them to clients that claim NeoForge) becomes a single
/// word, which the client can read; its properties are skipped by a
/// guessed shape that leaves the next nodes reading right.
fn commands(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    let nodes = r.count()?;
    write_varint(&mut out, nodes as i32);
    let word = argument_names()
        .iter()
        .position(|n| n == "brigadier:string")? as i32;
    let shapes = mod_parser_shapes(body, r.pos(), nodes)?;
    for i in 0..nodes {
        let flags = r.u8()?;
        out.push(flags);
        let children = r.count()?;
        write_varint(&mut out, children as i32);
        for _ in 0..children {
            copy_varint(&mut r, &mut out)?;
        }
        if flags & 0x08 != 0 {
            copy_varint(&mut r, &mut out)?;
        }
        let kind = flags & 0x03;
        if kind == 1 || kind == 2 {
            hop::identifier(&mut r, &mut out)?;
        }
        if kind == 2 {
            let name = parser_name(r.varint()?)?;
            if is_vanilla_parser(&name) {
                let old = argument_names().iter().position(|n| *n == name)? as i32;
                write_varint(
                    &mut out,
                    ids.to_native.map(IdSpace::CommandArgumentType, old)?,
                );
                argument_properties(&name, &mut r, &mut out)?;
            } else {
                write_varint(
                    &mut out,
                    ids.to_native.map(IdSpace::CommandArgumentType, word)?,
                );
                write_varint(&mut out, 0); // a single word
                skip_pieces(&mut r, shapes.get(&i)?)?;
            }
            if flags & 0x10 != 0 {
                hop::identifier(&mut r, &mut out)?;
            }
        }
    }
    copy_varint(&mut r, &mut out)?;
    Some(out.into())
}

/// Whether an advancement's fields after its icon read sensibly from `at`:
/// frame, flags, background, position, requirements, telemetry, and then
/// the next advancement's id (unless this was the `last`).
fn advancement_tail(buf: &[u8], at: usize, last: bool) -> bool {
    let identifier = |r: &mut Reader| -> Option<()> {
        let len = r.count()?;
        let bytes = r.take(len)?;
        let ok = (1..=256).contains(&len)
            && bytes.contains(&b':')
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-./:".contains(b));
        ok.then_some(())
    };
    let check = || -> Option<()> {
        let mut r = Reader::new(buf);
        r.take(at)?;
        if !(0..=2).contains(&r.varint()?) {
            return None;
        }
        let flags = u32::from_be_bytes(r.take(4)?.try_into().ok()?);
        if flags > 7 {
            return None;
        }
        if flags & 1 != 0 {
            identifier(&mut r)?;
        }
        for _ in 0..2 {
            let v = f32::from_be_bytes(r.take(4)?.try_into().ok()?);
            if !v.is_finite() || v.abs() > 10_000.0 {
                return None;
            }
        }
        let groups = r.count()?;
        for _ in 0..groups {
            for _ in 0..r.count()? {
                let len = r.count()?;
                r.take(len)?
                    .iter()
                    .all(|b| b.is_ascii_graphic())
                    .then_some(())?;
            }
        }
        if r.u8()? > 1 {
            return None;
        }
        if !last {
            identifier(&mut r)?;
        }
        Some(())
    };
    check().is_some()
}

/// Advancements: icons are stacks, and 1.21.? added "show advancements"
/// at the end.
fn update_advancements(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 1);
    copy(&mut r, &mut out, 1)?;
    let added = r.count()?;
    write_varint(&mut out, added as i32);
    for i in 0..added {
        hop::identifier(&mut r, &mut out)?;
        let parent = r.u8()?;
        out.push(parent);
        if parent != 0 {
            hop::identifier(&mut r, &mut out)?;
        }
        let display = r.u8()?;
        out.push(display);
        if display != 0 {
            copy_nbt(&mut r, &mut out)?;
            copy_nbt(&mut r, &mut out)?;
            let icon_start = r.pos();
            let mut icon = Vec::new();
            match stack(ids, &mut r, &mut icon) {
                Ok(()) => out.extend_from_slice(&icon),
                // A mod's component in the icon: find where the stack ends
                // by what must follow it.
                Err(Unreadable::Unknown) => {
                    let last = i + 1 == added;
                    let ends = |at: usize| advancement_tail(body, at, last);
                    let (icon, end) = stacks_until(ids, body, icon_start, 1, None, &ends)?;
                    out.extend_from_slice(&icon);
                    r = Reader::new(body);
                    r.take(end)?;
                }
                Err(Unreadable::Malformed) => return None,
            }
            copy_varint(&mut r, &mut out)?;
            let flags = r.take(4)?;
            out.extend_from_slice(flags);
            if flags[3] & 1 != 0 {
                hop::identifier(&mut r, &mut out)?;
            }
            copy(&mut r, &mut out, 8)?;
        }
        let groups = r.count()?;
        write_varint(&mut out, groups as i32);
        for _ in 0..groups {
            let names = r.count()?;
            write_varint(&mut out, names as i32);
            for _ in 0..names {
                hop::identifier(&mut r, &mut out)?;
            }
        }
        copy(&mut r, &mut out, 1)?;
    }
    let removed = r.count()?;
    write_varint(&mut out, removed as i32);
    for _ in 0..removed {
        hop::identifier(&mut r, &mut out)?;
    }
    let progress = r.count()?;
    write_varint(&mut out, progress as i32);
    for _ in 0..progress {
        hop::identifier(&mut r, &mut out)?;
        let criteria = r.count()?;
        write_varint(&mut out, criteria as i32);
        for _ in 0..criteria {
            hop::identifier(&mut r, &mut out)?;
            let obtained = r.u8()?;
            out.push(obtained);
            if obtained != 0 {
                copy(&mut r, &mut out, 8)?;
            }
        }
    }
    (r.remaining() == 0).then_some(())?;
    out.push(1);
    Some(out.into())
}

/// NeoForge opens a mod's menu with extra data (`AdvancedOpenScreenPayload`:
/// window, menu, title, bytes): azalea gets a plain `open_screen`, the
/// mod's screen gets the bytes (`modapi::screens`). Other payloads pass.
fn custom_payload(ids: &Ids, body: &[u8]) -> Option<Rewritten> {
    let mut r = Reader::new(body);
    let len = r.count()?;
    if r.take(len)? != b"neoforge:advanced_open_screen" {
        return Some(Rewritten::Body(body.into()));
    }
    let window = r.varint()?;
    let menu = hop::shifted(ids, IdSpace::Menu, r.varint()?)?;
    let start = r.pos();
    r.skip_nbt()?;
    let title = r.slice_from(start);
    let data_len = r.count()?;
    let data = r.take(data_len)?;
    crate::modapi::screens::on_open_data(window, data.to_vec());
    let mut out = Vec::with_capacity(10 + title.len());
    write_varint(&mut out, window);
    write_varint(&mut out, menu);
    out.extend_from_slice(title);
    Some(Rewritten::As("open_screen", out.into()))
}

/// Component patches changed shape across 1.21.2 to 1.21.5; until the
/// walker exists, a stack that needs one fails its rewrite.
fn patch(_ids: &Ids, _r: &mut Reader, _out: &mut Vec<u8>) -> Option<()> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::hop::Hop;
    use crate::protocol::wire::write_varint;

    #[test]
    fn a_modded_entity_s_plain_data_reads_as_json() {
        use serde_json::json;
        assert_eq!(data_json("INT", &[0xAC, 0x02]), Some(json!(300)));
        assert_eq!(data_json("BOOLEAN", &[1]), Some(json!(true)));
        let mut floats = Vec::new();
        for f in [1.0f32, -2.5, 0.25] {
            floats.extend(f.to_be_bytes());
        }
        assert_eq!(
            data_json("VECTOR3", &floats),
            Some(json!([1.0, -2.5, 0.25]))
        );
        assert_eq!(
            data_json("OPTIONAL_UNSIGNED_INT", &[0]),
            Some(serde_json::Value::Null)
        );
        assert_eq!(data_json("OPTIONAL_UNSIGNED_INT", &[5]), Some(json!(4)));
        assert_eq!(data_json("PARTICLE", &[1, 2]), None);
    }

    #[test]
    fn neoforge_s_menu_open_becomes_open_screen() {
        let mut body = Vec::new();
        let channel = b"neoforge:advanced_open_screen";
        write_varint(&mut body, channel.len() as i32);
        body.extend_from_slice(channel);
        write_varint(&mut body, 7); // window
        write_varint(&mut body, 30); // a modded menu
        body.extend_from_slice(&[8, 0, 2, b'H', b'i']); // title "Hi"
        body.extend_from_slice(&[2, 0xAB, 0xCD]); // extra data
        let Some(Rewritten::As("open_screen", out)) = custom_payload(Hop::v767().ids(), &body)
        else {
            panic!("not an open_screen");
        };
        assert_eq!(&*out, &[7, 30, 8, 0, 2, b'H', b'i']);
        assert_eq!(
            crate::modapi::screens::take_open_data(7),
            Some(vec![0xAB, 0xCD])
        );

        let mut other = vec![3];
        other.extend_from_slice(b"a:b");
        other.push(9);
        assert!(matches!(
            custom_payload(Hop::v767().ids(), &other),
            Some(Rewritten::Body(b)) if *b == *other
        ));
    }

    /// Tests that reset or read the per-connection entity map run one at a time.
    static ENTITY_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn login_success_is_renamed_and_loses_its_trailing_flag() {
        let wire = PacketTable::v1_21_1()
            .id(Phase::Login, Direction::Clientbound, "game_profile")
            .unwrap();
        let native = PacketTable::v1_21_11()
            .id(Phase::Login, Direction::Clientbound, "login_finished")
            .unwrap();
        // uuid, name "Ab", no properties, strictErrorHandling.
        let mut body = vec![7u8; 16];
        body.extend_from_slice(&[2, b'A', b'b', 0, 1]);
        let mut raw = vec![wire as u8];
        raw.extend_from_slice(&body);

        let out = Hop::v767().inbound(
            azalea_protocol::packets::ConnectionProtocol::Login,
            raw.into(),
        );
        assert_eq!(out[0] as u32, native);
        assert_eq!(&out[1..], &body[..body.len() - 1]);
    }

    #[test]
    fn client_information_drops_the_particle_setting() {
        // "en_us", view 12, chat full, colors, all skin parts, right hand,
        // no filtering, listing allowed, particles "decreased".
        let mut body = vec![5];
        body.extend_from_slice(b"en_us");
        body.extend_from_slice(&[12, 0, 1, 0x7F, 1, 0, 1, 1]);
        assert_eq!(
            &*client_information(Hop::v767().ids(), &body).unwrap(),
            &body[..body.len() - 1]
        );
    }

    #[test]
    fn renamed_play_packets_keep_their_ids_mapped() {
        let hop = Hop::v767();
        for (wire, native) in [
            ("set_carried_item", "set_held_slot"),
            ("horse_screen_open", "mount_screen_open"),
        ] {
            let wire = PacketTable::v1_21_1()
                .id(Phase::Game, Direction::Clientbound, wire)
                .unwrap();
            let native = PacketTable::v1_21_11()
                .id(Phase::Game, Direction::Clientbound, native)
                .unwrap();
            let out = hop.inbound(
                azalea_protocol::packets::ConnectionProtocol::Game,
                vec![wire as u8, 3].into(),
            );
            assert_eq!(&out[..], &[native as u8, 3]);
        }
    }

    /// Runs a 1.21.1 game packet through every hop to 26.3 and decodes it as
    /// azalea's packet `T`.
    fn through<T: AzBuf>(name: &str, body: &[u8]) -> T {
        use azalea::connection::PacketTranslator as _;
        let id = PacketTable::v1_21_1()
            .id(Phase::Game, Direction::Clientbound, name)
            .unwrap();
        let mut raw = Vec::new();
        write_varint(&mut raw, id as i32);
        raw.extend_from_slice(body);
        let mut t = crate::protocol::hop::Translator::for_protocol(767).unwrap();
        let out = t.inbound(
            azalea_protocol::packets::ConnectionProtocol::Game,
            raw.into(),
        );
        let mut r = Reader::new(&out);
        r.varint().unwrap();
        let mut cursor = std::io::Cursor::new(r.rest());
        let packet = T::azalea_read(&mut cursor).unwrap();
        assert_eq!(
            cursor.position() as usize,
            r.rest().len(),
            "{name} left bytes"
        );
        packet
    }

    #[test]
    fn player_position_reads_as_26_3() {
        use azalea_protocol::packets::game::c_player_position::ClientboundPlayerPosition;
        let mut body = Vec::new();
        for v in [1.5f64, 64.0, -3.25] {
            body.extend_from_slice(&v.to_be_bytes());
        }
        body.extend_from_slice(&90f32.to_be_bytes());
        body.extend_from_slice(&10f32.to_be_bytes());
        body.push(0b0_1000); // relative yaw
        body.push(7); // teleport id
        let p: ClientboundPlayerPosition = through("player_position", &body);
        assert_eq!(p.id, 7);
        assert_eq!(p.change.pos.y, 64.0);
    }

    #[test]
    fn entity_motion_and_teleport_read_as_26_3() {
        use azalea_protocol::packets::game::c_entity_position_sync::ClientboundEntityPositionSync;
        use azalea_protocol::packets::game::c_set_entity_motion::ClientboundSetEntityMotion;
        let mut motion = vec![5];
        for v in [8000i16, -4000, 0] {
            motion.extend_from_slice(&v.to_be_bytes());
        }
        let _: ClientboundSetEntityMotion = through("set_entity_motion", &motion);

        let mut tp = vec![5];
        for v in [1.0f64, 2.0, 3.0] {
            tp.extend_from_slice(&v.to_be_bytes());
        }
        tp.extend_from_slice(&[64, 0, 1]);
        // A resync: it moves the base relative moves count from.
        let sync: ClientboundEntityPositionSync = through("teleport_entity", &tp);
        assert_eq!(sync.id.0, 5);
        let at = sync.position.end_position();
        assert_eq!((at.x, at.y, at.z), (1.0, 2.0, 3.0));
        assert!(sync.on_ground);
    }

    #[test]
    fn spawn_position_reads_as_26_3() {
        use azalea_protocol::packets::game::c_set_default_spawn_position::ClientboundSetDefaultSpawnPosition;
        let mut body = 0x0000_0040_0000_0000u64.to_be_bytes().to_vec();
        body.extend_from_slice(&45f32.to_be_bytes());
        let _: ClientboundSetDefaultSpawnPosition = through("set_default_spawn_position", &body);
    }

    #[test]
    fn login_reads_as_26_3() {
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        use azalea_protocol::packets::game::c_login::ClientboundLogin;
        let ident = |out: &mut Vec<u8>, s: &str| {
            write_varint(out, s.len() as i32);
            out.extend_from_slice(s.as_bytes());
        };
        let mut body = 42i32.to_be_bytes().to_vec();
        body.push(0); // hardcore
        write_varint(&mut body, 1);
        ident(&mut body, "minecraft:overworld");
        for v in [20, 10, 10] {
            write_varint(&mut body, v);
        }
        body.extend_from_slice(&[0, 1, 0]);
        write_varint(&mut body, 0); // dimension type
        ident(&mut body, "minecraft:the_nether");
        body.extend_from_slice(&7u64.to_be_bytes());
        body.extend_from_slice(&[1, 0xFF, 0, 0, 0]); // modes, debug, flat, no death
        write_varint(&mut body, 0); // portal cooldown
        body.push(1); // secure chat
        let login: ClientboundLogin = through("login", &body);
        assert_eq!(login.player_id.0, 42);
        assert_eq!(*DIMENSION.lock().unwrap(), "minecraft:the_nether");
    }

    /// A 1.21.1 chunk: one heightmap, one section (an indirect palette of
    /// stone and a modded state past 1.21.1's range, then a single biome),
    /// a chest, and empty light data.
    fn chunk_1_21_1(modded: i32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&3i32.to_be_bytes());
        body.extend_from_slice(&(-2i32).to_be_bytes());
        // { MOTION_BLOCKING: [L; 37 longs] }
        body.push(10);
        body.push(12);
        body.extend_from_slice(&15u16.to_be_bytes());
        body.extend_from_slice(b"MOTION_BLOCKING");
        body.extend_from_slice(&37i32.to_be_bytes());
        body.extend_from_slice(&[0x11; 37 * 8]);
        body.push(0);

        let mut section = 256i16.to_be_bytes().to_vec();
        section.push(4); // 4-bit palette
        write_varint(&mut section, 2);
        write_varint(&mut section, 1); // stone
        write_varint(&mut section, modded);
        write_varint(&mut section, 256); // 4096 / 16 per long
        section.extend_from_slice(&[0x10; 256 * 8]);
        section.push(0); // single biome
        write_varint(&mut section, 3);
        write_varint(&mut section, 0); // no data
        write_varint(&mut body, section.len() as i32);
        body.extend_from_slice(&section);

        write_varint(&mut body, 1); // one block entity
        body.extend_from_slice(&[0x12, 0, 70]);
        write_varint(&mut body, 1); // chest
        body.push(10);
        body.push(0);

        for _ in 0..4 {
            write_varint(&mut body, 0); // light masks
        }
        write_varint(&mut body, 0);
        write_varint(&mut body, 0);
        body
    }

    #[test]
    fn a_chunk_reads_as_26_3_and_keeps_a_modded_state() {
        use azalea_protocol::packets::game::c_level_chunk_with_light::ClientboundLevelChunkWithLight;
        let ids = Hop::v767().ids();
        let modded = ids.to_native.count(IdSpace::BlockState) + 5;
        let chunk: ClientboundLevelChunkWithLight =
            through("level_chunk_with_light", &chunk_1_21_1(modded));
        assert_eq!((chunk.x, chunk.z), (3, -2));
        assert_eq!(chunk.chunk_data.heightmaps.len(), 1);
        assert_eq!(chunk.chunk_data.block_entities.len(), 1);
        let buffer = &chunk.chunk_data.buffer;
        // block count, fluid count (26.1), 4 bits, two palette entries.
        let mut r = Reader::new(buffer);
        r.take(4).unwrap();
        assert_eq!(r.u8(), Some(4));
        assert_eq!(r.count(), Some(2), "two palette entries");
        assert_eq!(r.varint(), Some(1));
        let shifted = r.varint().unwrap();
        assert!(shifted as u32 > azalea::block::BlockState::MAX_STATE as u32);
    }

    #[test]
    fn input_flags_become_impulses_and_two_flags() {
        // forward, right, jump, shift
        let out = input_1_21_1(1 | 8 | 16 | 32);
        assert_eq!(f32::from_be_bytes(out[0..4].try_into().unwrap()), -1.0);
        assert_eq!(f32::from_be_bytes(out[4..8].try_into().unwrap()), 1.0);
        assert_eq!(out[8], 0b11);
        assert_eq!(out.len(), 9);
    }

    #[test]
    fn player_commands_move_past_the_sneak_actions() {
        // entity 5, START_SPRINTING (26.3: 1), data 0
        let out = player_command(Hop::v767().ids(), &[5, 1, 0]).unwrap();
        assert_eq!(&out[..], &[5, 3, 0]);
    }

    #[test]
    fn serverbound_shapes_match_1_21_1() {
        let ids = Hop::v767().ids();
        // main hand, pos, face up, cursor, inside, world border, sequence 9
        let mut use_on = vec![0];
        use_on.extend_from_slice(&[0; 8]);
        use_on.push(1);
        use_on.extend_from_slice(&[0; 12]);
        use_on.extend_from_slice(&[0, 1, 9]);
        let out = use_item_on(ids, &use_on).unwrap();
        assert_eq!(out.len(), use_on.len() - 1);
        assert_eq!(out[out.len() - 1], 9);

        assert_eq!(&*move_flags(ids, &[7, 0b10]).unwrap(), &[7, 0]);
        assert_eq!(&*move_flags(ids, &[0b11]).unwrap(), &[1]);

        // container 3, state 4, slot 5, button 0, pickup, two hashed slots...
        let click = container_click(ids, &[3, 4, 0, 5, 0, 0, 2, 0xFF, 0xFF]).unwrap();
        assert_eq!(&*click, &[3, 4, 0, 5, 0, 0, 0, 0]);

        // slot 36, one item with a component
        let stone = ids.to_native.map(IdSpace::Item, 1).unwrap() as u8;
        let creative = creative_slot(ids, &[0, 36, 1, stone, 1, 0, 9, 9]).unwrap();
        assert_eq!(&*creative, &[0, 36, 1, 1, 0, 0]);
        // A modded item, past 1.21.1's vanilla ones, goes back as it came.
        let wire = ids.to_native.count(IdSpace::Item) + 5;
        let native = hop::shifted(ids, IdSpace::Item, wire).unwrap();
        let mut body = vec![0, 36, 1];
        write_varint(&mut body, native);
        body.extend_from_slice(&[0, 0]);
        let mut want = vec![0, 36, 1];
        write_varint(&mut want, wire);
        want.extend_from_slice(&[0, 0]);
        assert_eq!(&*creative_slot(ids, &body).unwrap(), &want[..]);
        assert!(unsent(ids, &[1]).is_none());
    }

    /// A 1.21.1 spawn: entity 9 of type `kind` at the origin, still.
    fn spawn_1_21_1(kind: i32) -> Vec<u8> {
        let mut body = vec![9];
        body.extend_from_slice(&[0xAB; 16]);
        write_varint(&mut body, kind);
        body.extend_from_slice(&[0; 3 * 8 + 3]);
        write_varint(&mut body, 0);
        body.extend_from_slice(&[0; 6]);
        body
    }

    #[test]
    fn a_cow_spawns_and_its_data_reads_as_26_3() {
        use azalea::entity::EntityDataValue;
        use azalea_protocol::packets::game::c_add_entity::ClientboundAddEntity;
        use azalea_protocol::packets::game::c_set_entity_data::ClientboundSetEntityData;
        use azalea_registry::builtin::EntityKind;
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        *ENTITIES.lock().unwrap() = Some(HashMap::new());
        let cow = entity_tables().old.type_id("minecraft:cow").unwrap();

        let spawn: ClientboundAddEntity = through("add_entity", &spawn_1_21_1(cow));
        assert_eq!(spawn.entity_type, EntityKind::Cow);

        // flags (BYTE 0), health (FLOAT 3) 10, baby (BOOLEAN 8), a stray index
        let mut data = vec![9, 0, 0, 0, 9, 3];
        data.extend_from_slice(&10f32.to_be_bytes());
        data.extend_from_slice(&[16, 8, 1, 99, 8, 1, 0xFF]);
        let packet: ClientboundSetEntityData = through("set_entity_data", &data);
        let items = &packet.packed_items.0;
        assert_eq!(items.len(), 3, "the stray index is dropped");
        assert_eq!(items[1].index, 9);
        assert_eq!(items[1].value, EntityDataValue::Float(10.0));
        assert_eq!(items[2].index, 16);
        assert_eq!(items[2].value, EntityDataValue::Boolean(true));
    }

    #[test]
    fn the_main_hand_moves_and_becomes_an_arm() {
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let player = entity_tables().old.type_id("minecraft:player").unwrap();
        *ENTITIES.lock().unwrap() = Some(HashMap::from([(9, player)]));
        let arm = entity_tables().new.serializer_id("HUMANOID_ARM").unwrap() as u8;
        // main hand: index 18, BYTE, right
        let out = entity_data(Hop::v767().ids(), &[9, 18, 0, 1, 0xFF]).unwrap();
        assert_eq!(&*out, &[9, 15, arm, 1, 0xFF]);
    }

    #[test]
    fn an_unknown_entity_keeps_no_data() {
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        *ENTITIES.lock().unwrap() = Some(HashMap::new());
        let out = entity_data(Hop::v767().ids(), &[42, 0, 0, 5, 0xFF]).unwrap();
        assert_eq!(&*out, &[42, 0xFF]);
    }

    #[test]
    fn a_villagers_data_lands_past_age_locked() {
        use azalea::entity::EntityDataValue;
        use azalea_protocol::packets::game::c_set_entity_data::ClientboundSetEntityData;
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let villager = entity_tables().old.type_id("minecraft:villager").unwrap();
        *ENTITIES.lock().unwrap() = Some(HashMap::from([(9, villager)]));
        let data = entity_tables().old.serializer_id("VILLAGER_DATA").unwrap() as u8;
        // villager data at 1.21.1's index 18: plains, librarian, level 2
        let packet: ClientboundSetEntityData =
            through("set_entity_data", &[9, 18, data, 2, 9, 2, 0xFF]);
        let item = &packet.packed_items.0[0];
        assert_eq!(item.index, 19, "26.1 put AGE_LOCKED before it");
        assert!(matches!(item.value, EntityDataValue::VillagerData(_)));
    }

    #[test]
    fn a_modded_registry_id_lands_where_its_packets_do() {
        use crate::protocol::hop::native_registry_id;
        use azalea_registry::Registry as _;
        use azalea_registry::builtin::EntityKind;
        let ids = Hop::v767().ids();
        let wire = ids.to_native.count(IdSpace::EntityType);
        let vanilla = (0..).map_while(EntityKind::from_u32).count() as i32;
        let cow = entity_tables().old.type_id("minecraft:cow").unwrap();
        assert_eq!(
            native_registry_id(767, IdSpace::EntityType, cow),
            Some(EntityKind::Cow.to_u32() as i32)
        );
        let first = native_registry_id(767, IdSpace::EntityType, wire).unwrap();
        let modded = native_registry_id(767, IdSpace::EntityType, wire + 5).unwrap();
        assert_eq!(
            first, vanilla,
            "the first modded type follows the last vanilla one"
        );
        assert_eq!(modded - first, 5, "modded ids keep their spacing");
    }

    #[test]
    fn a_modded_spawn_carries_the_id_its_name_was_filed_under() {
        use crate::protocol::hop::native_registry_id;
        use azalea_protocol::packets::game::c_add_entity::ClientboundAddEntity;
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        *ENTITIES.lock().unwrap() = Some(HashMap::new());
        let wire = Hop::v767().ids().to_native.count(IdSpace::EntityType) + 3;
        let spawn: ClientboundAddEntity = through("add_entity", &spawn_1_21_1(wire));
        let filed = native_registry_id(767, IdSpace::EntityType, wire).unwrap();
        assert_eq!(spawn.modded_type, Some(filed as u32));
    }

    // 1.21.1 ids (reference/minecraft-1.21.1 registries.json).
    const DIAMOND_SWORD_1_21_1: i32 = 838;
    const SHULKER_BOX_1_21_1: i32 = 522;
    const STONE_1_21_1: i32 = 1;
    const DAMAGE_1_21_1: i32 = 3;
    const UNBREAKABLE_1_21_1: i32 = 4;
    const CUSTOM_NAME_1_21_1: i32 = 5;
    const CUSTOM_MODEL_DATA_1_21_1: i32 = 13;
    const CONTAINER_1_21_1: i32 = 52;

    fn set_content(slots: &[Vec<u8>]) -> Vec<u8> {
        let mut body = vec![0, 1];
        write_varint(&mut body, slots.len() as i32);
        for slot in slots {
            body.extend_from_slice(slot);
        }
        body.push(0); // empty cursor
        body
    }

    #[test]
    fn an_inventory_reads_as_26_3() {
        use azalea_inventory::ItemStack;
        use azalea_inventory::components::{
            Container, CustomModelData, CustomName, Damage, TooltipDisplay, Unbreakable,
        };
        use azalea_protocol::packets::game::c_container_set_content::ClientboundContainerSetContent;
        use azalea_registry::builtin::{DataComponentKind, ItemKind};
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        synced_components(57);

        let mut sword = vec![1];
        write_varint(&mut sword, DIAMOND_SWORD_1_21_1);
        sword.extend_from_slice(&[4, 0]);
        write_varint(&mut sword, DAMAGE_1_21_1);
        sword.push(5);
        write_varint(&mut sword, CUSTOM_NAME_1_21_1);
        sword.extend_from_slice(&[8, 0, 5]);
        sword.extend_from_slice(b"Blade");
        write_varint(&mut sword, UNBREAKABLE_1_21_1);
        sword.push(0); // not shown in the tooltip
        write_varint(&mut sword, CUSTOM_MODEL_DATA_1_21_1);
        sword.push(7);

        let mut shulker = vec![1];
        write_varint(&mut shulker, SHULKER_BOX_1_21_1);
        shulker.extend_from_slice(&[1, 0]);
        write_varint(&mut shulker, CONTAINER_1_21_1);
        shulker.extend_from_slice(&[1, 3]);
        write_varint(&mut shulker, STONE_1_21_1);
        shulker.extend_from_slice(&[0, 0]);

        let packet: ClientboundContainerSetContent =
            through("container_set_content", &set_content(&[sword, shulker]));
        let ItemStack::Present(sword) = &packet.items[0] else {
            panic!("the sword is gone");
        };
        assert_eq!(sword.kind, ItemKind::DiamondSword);
        let patch = &sword.component_patch;
        assert_eq!(patch.get::<Damage>().map(|d| d.amount), Some(5));
        assert!(patch.has::<CustomName>());
        assert!(patch.has::<Unbreakable>());
        assert_eq!(
            patch.get::<CustomModelData>().map(|c| c.floats.clone()),
            Some(vec![7.0])
        );
        let tooltip = patch
            .get::<TooltipDisplay>()
            .expect("the hidden flag moved here");
        assert!(
            tooltip
                .hidden_components
                .contains(&DataComponentKind::Unbreakable)
        );

        let ItemStack::Present(shulker) = &packet.items[1] else {
            panic!("the shulker box is gone");
        };
        let inside = &shulker.component_patch.get::<Container>().unwrap().items;
        assert!(
            matches!(&inside[0], ItemStack::Present(s) if s.kind == ItemKind::Stone && s.count == 3)
        );
    }

    #[test]
    fn a_modded_component_keeps_the_item_and_the_slots_after_it() {
        use azalea_inventory::ItemStack;
        use azalea_protocol::packets::game::c_container_set_content::ClientboundContainerSetContent;
        use azalea_registry::builtin::ItemKind;
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        synced_components(201);

        let mut modded = vec![1];
        write_varint(&mut modded, DIAMOND_SWORD_1_21_1);
        modded.extend_from_slice(&[1, 0]);
        write_varint(&mut modded, 200); // a mod's component
        modded.extend_from_slice(&[0xFF, 0x12, 0x34, 0x56]); // a colour
        let mut stone = vec![2];
        write_varint(&mut stone, STONE_1_21_1);
        stone.extend_from_slice(&[0, 0]);

        let packet: ClientboundContainerSetContent =
            through("container_set_content", &set_content(&[modded, stone]));
        assert!(
            matches!(&packet.items[0], ItemStack::Present(s) if s.kind == ItemKind::DiamondSword)
        );
        assert!(
            matches!(&packet.items[1], ItemStack::Present(s) if s.kind == ItemKind::Stone && s.count == 2),
            "found again past the mod's bytes"
        );
    }

    /// As a NeoForge server's snapshot would: vanilla's components in
    /// vanilla's order, then mods' up to `n`.
    fn synced_components(n: u32) {
        use crate::protocol::registries::RegistryId;
        let vanilla: Vec<String> =
            serde_json::from_str(include_str!("data/data_component_type-1.21.1.json")).unwrap();
        crate::protocol::native_snapshot(
            RegistryId::DataComponentType,
            (0..n)
                .map(|i| {
                    let name = vanilla
                        .get(i as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("somemod:thing_{i}"));
                    (i, azalea_registry::identifier::Identifier::new(name))
                })
                .collect(),
        );
    }

    #[test]
    fn text_after_an_unknown_component_is_not_read_as_items() {
        use azalea_inventory::ItemStack;
        use azalea_protocol::packets::game::c_container_set_content::ClientboundContainerSetContent;
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        synced_components(57); // vanilla's only: 200 is no mod's

        let mut modded = vec![1];
        write_varint(&mut modded, DIAMOND_SWORD_1_21_1);
        modded.extend_from_slice(&[1, 0]);
        write_varint(&mut modded, 200);
        modded.extend_from_slice(b"create:copper_casing tooltip text");
        let packet: ClientboundContainerSetContent = through(
            "container_set_content",
            &set_content(&[modded, vec![0], vec![0]]),
        );
        assert!(
            packet.items[1..]
                .iter()
                .all(|s| matches!(s, ItemStack::Empty)),
            "no copper out of letters"
        );
    }

    #[test]
    fn a_backpacks_mod_components_are_skipped_by_guessing_their_shapes() {
        use azalea_inventory::ItemStack;
        use azalea_protocol::packets::game::c_container_set_content::ClientboundContainerSetContent;
        use azalea_registry::builtin::ItemKind;
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        synced_components(120);

        // A backpack-like stack: a colour (int), an NBT tag that spells out
        // ids, an owner (uuid) and a flag (byte), all mods' components.
        let mut backpack = vec![1];
        write_varint(&mut backpack, 2000);
        backpack.extend_from_slice(&[4, 0]);
        backpack.extend_from_slice(&[88, 0xFF, 0xC1, 0xC1, 0xC1]);
        backpack.extend_from_slice(&[98, 10, 8, 0, 2]);
        backpack.extend_from_slice(b"id");
        backpack.extend_from_slice(&[0, 20]);
        backpack.extend_from_slice(b"somemod:upgrade_tier");
        backpack.push(0);
        backpack.push(90);
        backpack.extend_from_slice(&[0x14, 0x20, 0xAE, 0x6F, 0xE7, 0x24, 0x46, 0x32]);
        backpack.extend_from_slice(&[0xAF, 0x5F, 0x03, 0x3E, 0x3F, 0x48, 0x3A, 0xC2]);
        backpack.extend_from_slice(&[97, 0]);
        let mut stone = vec![5];
        write_varint(&mut stone, STONE_1_21_1);
        stone.extend_from_slice(&[0, 0]);
        let mut sword = vec![1];
        write_varint(&mut sword, DIAMOND_SWORD_1_21_1);
        sword.extend_from_slice(&[1, 0]);
        write_varint(&mut sword, DAMAGE_1_21_1);
        sword.push(64);

        let packet: ClientboundContainerSetContent = through(
            "container_set_content",
            &set_content(&[backpack, stone, vec![0], sword]),
        );
        assert!(matches!(&packet.items[0], ItemStack::Present(s) if s.count == 1));
        assert!(
            matches!(&packet.items[1], ItemStack::Present(s) if s.kind == ItemKind::Stone && s.count == 5)
        );
        assert!(matches!(packet.items[2], ItemStack::Empty));
        assert!(
            matches!(&packet.items[3], ItemStack::Present(s) if s.kind == ItemKind::DiamondSword)
        );
    }

    #[test]
    fn gravity_stays_gravity() {
        use azalea_protocol::packets::game::c_update_attributes::ClientboundUpdateAttributes;
        use azalea_registry::builtin::Attribute;
        // entity 9, one attribute: 1.21.1's `generic.gravity` (13), base
        // 0.08, no modifiers
        let mut body = vec![9, 1, 13];
        body.extend_from_slice(&0.08f64.to_be_bytes());
        body.push(0);
        let packet: ClientboundUpdateAttributes = through("update_attributes", &body);
        assert_eq!(packet.values[0].attribute, Attribute::Gravity);
        assert_eq!(packet.values[0].base, 0.08);
    }

    #[test]
    fn a_dust_particle_reads_as_26_3() {
        use azalea_protocol::packets::game::c_level_particles::ClientboundLevelParticles;
        let dust = particle_names()
            .iter()
            .position(|n| n == "minecraft:dust")
            .unwrap() as u8;
        let mut body = vec![0];
        body.extend_from_slice(&[0; 3 * 8 + 4 * 4]);
        body.extend_from_slice(&1f32.to_be_bytes()); // max speed
        body.extend_from_slice(&3i32.to_be_bytes()); // count
        body.push(dust);
        for channel in [1f32, 0.0, 0.0, 1.5] {
            body.extend_from_slice(&channel.to_be_bytes()); // red, scale 1.5
        }
        let _: ClientboundLevelParticles = through("level_particles", &body);
    }

    #[test]
    fn a_frozen_day_time_reads_as_stopped() {
        use azalea_protocol::packets::game::c_set_time::ClientboundSetTime;
        let mut body = 100u64.to_be_bytes().to_vec();
        body.extend_from_slice(&(-6000i64).to_be_bytes()); // frozen at 6000
        let packet: ClientboundSetTime = through("set_time", &body);
        let clock = packet.clock_updates.values().next().unwrap();
        assert_eq!(clock.total_ticks, 6000);
        assert_eq!(clock.rate, 0.0);
    }

    fn utf(out: &mut Vec<u8>, s: &str) {
        write_varint(out, s.len() as i32);
        out.extend_from_slice(s.as_bytes());
    }

    #[test]
    fn a_command_tree_reads_as_26_3() {
        use azalea_protocol::packets::game::c_commands::ClientboundCommands;
        // root -> literal "tp" -> argument "n" (brigadier:integer, min 1)
        let mut body = vec![3];
        body.extend_from_slice(&[0, 1, 1]); // root: one child, node 1
        body.extend_from_slice(&[1, 1, 2]); // literal: one child, node 2
        utf(&mut body, "tp");
        body.extend_from_slice(&[0x02 | 0x04, 0]); // executable argument
        utf(&mut body, "n");
        body.push(3); // brigadier:integer in 1.21.1
        body.push(1);
        body.extend_from_slice(&1i32.to_be_bytes());
        body.push(0); // root index
        let _: ClientboundCommands = through("commands", &body);
    }

    #[test]
    fn an_explosion_reads_as_26_3() {
        use azalea_protocol::packets::game::c_explode::ClientboundExplode;
        let poof = particle_names()
            .iter()
            .position(|n| n == "minecraft:explosion")
            .unwrap() as u8;
        let mut body = Vec::new();
        for v in [1.0f64, 64.0, 2.0] {
            body.extend_from_slice(&v.to_be_bytes());
        }
        body.extend_from_slice(&4f32.to_be_bytes());
        body.extend_from_slice(&[2, 0, 0, 0, 1, 0, 0]); // two blocks
        for k in [0.1f32, 0.2, 0.3] {
            body.extend_from_slice(&k.to_be_bytes());
        }
        body.push(1); // destroy
        body.extend_from_slice(&[poof, poof]);
        body.push(1); // sound id 0, by registry
        let _: ClientboundExplode = through("explode", &body);
    }

    #[test]
    fn a_team_reads_as_26_3() {
        use azalea_protocol::packets::game::c_set_player_team::{
            ClientboundSetPlayerTeam, Method, NameTagVisibility,
        };
        let mut body = Vec::new();
        utf(&mut body, "red");
        body.push(0); // add
        body.extend_from_slice(&[8, 0, 3]);
        body.extend_from_slice(b"Red");
        body.push(0);
        utf(&mut body, "hideForOtherTeams");
        utf(&mut body, "never");
        body.push(12); // red
        body.extend_from_slice(&[8, 0, 0, 8, 0, 0]);
        body.push(1);
        utf(&mut body, "_ImLux");
        let team: ClientboundSetPlayerTeam = through("set_player_team", &body);
        let Method::Add((params, _)) = team.method else {
            panic!("not an add");
        };
        assert_eq!(
            params.nametag_visibility,
            NameTagVisibility::HideForOtherTeams
        );
    }

    #[test]
    fn recipes_and_advancements_read_as_26_3() {
        use azalea_protocol::packets::game::c_update_advancements::ClientboundUpdateAdvancements;
        use azalea_protocol::packets::game::c_update_recipes::ClientboundUpdateRecipes;
        let _: ClientboundUpdateRecipes = through("update_recipes", &[5, 1, 2, 3]);

        let mut adv = vec![1, 1];
        utf(&mut adv, "minecraft:story/root");
        adv.push(0); // no parent
        adv.push(1); // display
        adv.extend_from_slice(&[8, 0, 1, b'T', 8, 0, 1, b'D']);
        adv.push(2); // two swords: a stack and a template differ in order
        write_varint(&mut adv, DIAMOND_SWORD_1_21_1);
        adv.extend_from_slice(&[0, 0]);
        adv.push(0); // task
        adv.extend_from_slice(&1i32.to_be_bytes()); // background
        utf(&mut adv, "minecraft:block/stone");
        adv.extend_from_slice(&[0; 8]);
        adv.extend_from_slice(&[1, 1]);
        utf(&mut adv, "has_stone");
        adv.push(0); // no telemetry
        adv.push(0); // none removed
        adv.push(0); // no progress
        let packet: ClientboundUpdateAdvancements = through("update_advancements", &adv);
        let display = packet.added[0].advancement.value.display.as_ref().unwrap();
        assert!(matches!(
            &display.icon,
            azalea_inventory::ItemStack::Present(s)
                if s.kind == azalea_registry::builtin::ItemKind::DiamondSword && s.count == 2
        ));
    }

    #[test]
    fn a_trade_reads_as_26_3() {
        use azalea_protocol::packets::game::c_merchant_offers::ClientboundMerchantOffers;
        let mut body = vec![2, 1];
        write_varint(&mut body, STONE_1_21_1);
        body.extend_from_slice(&[4, 0]); // 4 stone, no components
        body.push(1);
        write_varint(&mut body, DIAMOND_SWORD_1_21_1);
        body.extend_from_slice(&[0, 0]);
        body.push(0); // no second cost
        body.push(0);
        body.extend_from_slice(&[0; 4 * 4]);
        body.extend_from_slice(&0.05f32.to_be_bytes());
        body.extend_from_slice(&[0; 4]);
        body.extend_from_slice(&[1, 0, 1, 1]);
        let _: ClientboundMerchantOffers = through("merchant_offers", &body);
    }

    #[test]
    fn mod_parsers_are_measured_and_become_words() {
        use crate::protocol::registries::RegistryId;
        use azalea_protocol::packets::game::c_commands::ClientboundCommands;
        let _entities = ENTITY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        crate::protocol::native_snapshot(
            RegistryId::CommandArgumentType,
            (0..60u32)
                .map(|i| {
                    let name = argument_names()
                        .get(i as usize)
                        .cloned()
                        .unwrap_or_else(|| format!("somemod:parser_{i}"));
                    (i, azalea_registry::identifier::Identifier::new(name))
                })
                .collect(),
        );
        // root -> "a" (mod 56, one flag byte) -> "b" (mod 56 again) ->
        // "c" (mod 57: flag, nested brigadier:string + its kind, a text)
        let mut body = vec![4];
        body.extend_from_slice(&[0, 1, 1]);
        body.extend_from_slice(&[0x02, 1, 2]);
        utf(&mut body, "a");
        body.extend_from_slice(&[56, 1]);
        body.extend_from_slice(&[0x02, 1, 3]);
        utf(&mut body, "b");
        body.extend_from_slice(&[56, 0]);
        body.extend_from_slice(&[0x06, 0]);
        utf(&mut body, "c");
        body.extend_from_slice(&[57, 0, 5, 1]);
        body.extend_from_slice(&[8, 0, 4]);
        body.extend_from_slice(b"text");
        body.push(0); // root index
        let packet: ClientboundCommands = through("commands", &body);
        assert_eq!(packet.entries.len(), 4);
    }
}
