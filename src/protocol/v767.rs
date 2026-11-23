//! 1.21 and 1.21.1 (protocol 767) on the wire, 1.21.11 (774) natively;
//! the 774 and 775 hops carry it on to 26.3.
//!
//! Experimental: packet and registry ids are remapped and login works;
//! most other payload shapes are not rewritten yet. The rewrites to
//! add are listed in `docs/MULTIVERSION_PLAN.md`, section 4.

use super::hop::{self, HopSpec, Ids, Rewritten, body};
use super::packets::{Direction, PacketTable, Phase};
use super::remap::{IdSpace, Remap};
use super::wire::{Reader, write_varint};

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use azalea_buf::AzBuf;
use azalea_core::delta::LpVec3;
use azalea_core::position::Vec3;

/// The dimension the last `login` or `respawn` named: 1.21.1's spawn
/// position doesn't say, 1.21.9's does.
static DIMENSION: Mutex<String> = Mutex::new(String::new());

/// This client's entity id (`login`), which 1.21.1's sneak commands name.
static PLAYER_ID: AtomicI32 = AtomicI32::new(0);

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
        (Phase::Game, "teleport_entity", body!(teleport_entity)),
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
    PLAYER_ID.store(i32::from_be_bytes(id.try_into().ok()?), Ordering::Relaxed);
    SNEAKING.store(false, Ordering::Relaxed);
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

fn teleport_entity(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let id = r.varint()?;
    let pos = [f64(&mut r)?, f64(&mut r)?, f64(&mut r)?];
    let angle = |b: u8| f32::from(b as i8) * 360.0 / 256.0;
    let (y_rot, x_rot) = (angle(r.u8()?), angle(r.u8()?));
    let on_ground = r.u8()?;
    let mut out = Vec::with_capacity(70);
    write_varint(&mut out, id);
    position_move_rotation(&mut out, pos, [0.0; 3], y_rot, x_rot);
    out.extend_from_slice(&0i32.to_be_bytes());
    out.push(on_ground);
    // Bytes past 1.21.1's fields are a mod's own; they're dropped.
    Some(out.into())
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
        write_varint(&mut out, ids.from_native.map(IdSpace::Item, r.varint()?)?);
        write_varint(&mut out, 0);
        write_varint(&mut out, 0);
    }
    Some(out.into())
}

/// Recipes are named differently since 1.21.2 and aren't translated.
fn unsent(_ids: &Ids, _body: &[u8]) -> Option<Box<[u8]>> {
    None
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
        use azalea_protocol::packets::game::c_set_entity_motion::ClientboundSetEntityMotion;
        use azalea_protocol::packets::game::c_teleport_entity::ClientboundTeleportEntity;
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
        let _: ClientboundTeleportEntity = through("teleport_entity", &tp);
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
        assert!(unsent(ids, &[1]).is_none());
    }
}
