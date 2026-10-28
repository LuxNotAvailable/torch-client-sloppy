//! 26.1 through 26.1.2 (protocol 775) on the wire, 26.3 (777) natively.

use azalea_registry::Registry as _;
use azalea_registry::builtin::{DataComponentKind, EntityKind};

use super::hop::{
    self, HopSpec, Ids, MAP_COLOR_775, POT_DECORATIONS_775, Rewritten, SWING_ANIMATION_775, body,
};
use super::packets::{PacketTable, Phase};
use super::remap::{IdSpace, Remap};
use super::wire::{Reader, write_varint};

pub(super) const SPEC: HopSpec = HopSpec {
    wire: PacketTable::v26_1,
    native: PacketTable::native,
    to_native: Remap::v775_to_v777,
    from_native: Remap::v777_to_v775,
    kinds: None,
    patch,
    inbound: &[
        (Phase::Login, "login_finished", body!(login_finished)),
        (Phase::Configuration, "update_tags", body!(hop::update_tags)),
        (Phase::Game, "update_tags", body!(hop::update_tags)),
        (Phase::Game, "login", body!(login)),
        (Phase::Game, "respawn", body!(respawn)),
        (Phase::Game, "level_chunk_with_light", body!(level_chunk)),
        (Phase::Game, "light_update", body!(light_update)),
        (Phase::Game, "block_update", body!(hop::block_update)),
        (
            Phase::Game,
            "section_blocks_update",
            body!(hop::section_blocks_update),
        ),
        (Phase::Game, "block_event", body!(hop::block_event)),
        (Phase::Game, "block_entity_data", block_entity_data),
        (Phase::Game, "level_event", body!(hop::level_event)),
        (Phase::Game, "add_entity", body!(add_entity)),
        (Phase::Game, "set_entity_data", body!(hop::entity_data)),
        (
            Phase::Game,
            "container_set_content",
            body!(hop::container_set_content),
        ),
        (
            Phase::Game,
            "container_set_slot",
            body!(hop::container_set_slot),
        ),
        (Phase::Game, "set_cursor_item", body!(hop::set_cursor_item)),
        (
            Phase::Game,
            "set_player_inventory",
            body!(hop::set_player_inventory),
        ),
        (Phase::Game, "set_equipment", body!(hop::set_equipment)),
        (Phase::Game, "merchant_offers", body!(merchant_offers)),
        (Phase::Game, "level_particles", body!(level_particles)),
        (Phase::Game, "explode", body!(explode)),
        (Phase::Game, "sound", body!(hop::sound)),
        (Phase::Game, "sound_entity", body!(hop::sound)),
        (Phase::Game, "award_stats", body!(hop::award_stats)),
        (Phase::Game, "animate", animate),
        (Phase::Game, "move_entity_pos", body!(move_entity_pos)),
        (
            Phase::Game,
            "move_entity_pos_rot",
            body!(move_entity_pos_rot),
        ),
        (Phase::Game, "move_entity_rot", body!(move_entity_rot)),
        (
            Phase::Game,
            "entity_position_sync",
            body!(entity_position_sync),
        ),
        (Phase::Game, "set_player_team", body!(set_player_team)),
        (
            Phase::Game,
            "update_advancements",
            body!(update_advancements),
        ),
        (Phase::Game, "update_attributes", body!(update_attributes)),
        (Phase::Game, "update_recipes", body!(update_recipes)),
        (Phase::Game, "recipe_book_add", body!(recipe_book_add)),
        (Phase::Game, "commands", body!(commands)),
    ],
    outbound: &[
        (Phase::Game, "punch", "swing", punch),
        (
            Phase::Game,
            "accept_teleportation",
            "accept_teleportation",
            accept_teleportation,
        ),
        (Phase::Game, "player_action", "player_action", player_action),
        (Phase::Game, "sign_update", "sign_update", sign_update),
        (
            Phase::Game,
            "container_click",
            "container_click",
            hop::container_click,
        ),
        (
            Phase::Game,
            "set_creative_mode_slot",
            "set_creative_mode_slot",
            hop::creative_slot,
        ),
    ],
};

fn copy(r: &mut Reader, out: &mut Vec<u8>, n: usize) -> Option<()> {
    out.extend_from_slice(r.take(n)?);
    Some(())
}

fn copy_varint(r: &mut Reader, out: &mut Vec<u8>) -> Option<i32> {
    let value = r.varint()?;
    write_varint(out, value);
    Some(value)
}

fn copy_nbt(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    out.extend_from_slice(nbt(r)?);
    Some(())
}

fn nbt<'a>(r: &mut Reader<'a>) -> Option<&'a [u8]> {
    let start = r.pos();
    r.skip_nbt()?;
    let all = r.slice_from(0);
    Some(&all[start..])
}

fn copy_list(
    r: &mut Reader,
    out: &mut Vec<u8>,
    mut each: impl FnMut(&mut Reader, &mut Vec<u8>) -> Option<()>,
) -> Option<()> {
    let len = r.count()?;
    write_varint(out, len as i32);
    for _ in 0..len {
        each(r, out)?;
    }
    Some(())
}

/// 26.3 names the login session; 26.1 has no such thing to send.
fn login_finished(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut out = body.to_vec();
    out.extend_from_slice(&[0; 16]);
    Some(out.into_boxed_slice())
}

/// Game modes went from bytes, with -1 for no previous mode, to VarInts where
/// the previous mode is an optional shifted up by one.
fn spawn_info(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    copy_varint(r, out)?;
    hop::identifier(r, out)?;
    copy(r, out, 8)?;
    write_varint(out, i32::from(r.u8()?));
    let previous = r.u8()? as i8;
    write_varint(
        out,
        if previous < 0 {
            0
        } else {
            i32::from(previous) + 1
        },
    );
    copy(r, out, 2)?;
    let death = r.u8()?;
    out.push(death);
    if death != 0 {
        hop::identifier(r, out)?;
        copy(r, out, 8)?;
    }
    copy_varint(r, out)?;
    copy_varint(r, out)?;
    Some(())
}

fn login(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 4);
    copy(&mut r, &mut out, 4 + 1)?;
    copy_list(&mut r, &mut out, |r, out| hop::identifier(r, out))?;
    for _ in 0..3 {
        copy_varint(&mut r, &mut out)?;
    }
    copy(&mut r, &mut out, 3)?;
    spawn_info(&mut r, &mut out)?;
    // 26.1 does not say whether it authenticates players; most servers do.
    out.push(1);
    copy(&mut r, &mut out, 1)?;
    Some(out.into_boxed_slice())
}

fn respawn(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 2);
    spawn_info(&mut r, &mut out)?;
    copy(&mut r, &mut out, 1)?;
    Some(out.into_boxed_slice())
}

/// 26.3 sends a `BitSet` as its bytes rather than its longs.
fn bit_set(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    let longs = r.count()?;
    let mut bytes = Vec::with_capacity(longs.checked_mul(8)?);
    for _ in 0..longs {
        bytes.extend_from_slice(&r.u64()?.to_le_bytes());
    }
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    write_varint(out, bytes.len() as i32);
    out.extend_from_slice(&bytes);
    Some(())
}

fn light_data(r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    for _ in 0..4 {
        bit_set(r, out)?;
    }
    for _ in 0..2 {
        copy_list(r, out, |r, out| {
            let len = r.count()?;
            write_varint(out, len as i32);
            copy(r, out, len)
        })?;
    }
    Some(())
}

fn light_update(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_varint(&mut r, &mut out)?;
    copy_varint(&mut r, &mut out)?;
    light_data(&mut r, &mut out)?;
    Some(out.into_boxed_slice())
}

fn level_chunk(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 2048);
    let x = i32::from_be_bytes(r.take(4)?.try_into().ok()?);
    let z = i32::from_be_bytes(r.take(4)?.try_into().ok()?);
    out.extend_from_slice(&x.to_be_bytes());
    out.extend_from_slice(&z.to_be_bytes());
    copy_list(&mut r, &mut out, |r, out| {
        copy_varint(r, out)?;
        let longs = r.count()?;
        write_varint(out, longs as i32);
        copy(r, out, longs.checked_mul(8)?)
    })?;

    let length = r.count()?;
    let blob = r.take(length)?;
    let sections = match hop::remap_sections(ids, blob, true) {
        Ok(sections) => sections,
        Err(why) => {
            crate::log_error!(
                "proto",
                "chunk {x},{z} was not translated: {why}; the column will be missing"
            );
            return None;
        }
    };
    write_varint(&mut out, i32::try_from(sections.len()).ok()?);
    out.extend_from_slice(&sections);

    let entities = r.count()?;
    let mut kept = Vec::with_capacity(entities * 8);
    let mut count = 0;
    for _ in 0..entities {
        let head = r.take(1 + 2)?;
        let kind = ids.to_native.map(IdSpace::BlockEntityType, r.varint()?);
        let tag = nbt(&mut r)?;
        // Beds are baked into the block model since 26.3 and carry no entity.
        if let Some(kind) = kind {
            kept.extend_from_slice(head);
            write_varint(&mut kept, kind);
            kept.extend_from_slice(tag);
            count += 1;
        }
    }
    write_varint(&mut out, count);
    out.extend_from_slice(&kept);

    light_data(&mut r, &mut out)?;
    Some(out.into_boxed_slice())
}

fn block_entity_data(ids: &Ids, body: &[u8]) -> Option<Rewritten> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy(&mut r, &mut out, 8)?;
    let Some(kind) = ids.to_native.map(IdSpace::BlockEntityType, r.varint()?) else {
        return Some(Rewritten::Drop);
    };
    write_varint(&mut out, kind);
    out.extend_from_slice(r.rest());
    Some(Rewritten::Body(out.into_boxed_slice()))
}

fn add_entity(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 2);
    copy_varint(&mut r, &mut out)?;
    copy(&mut r, &mut out, 16)?;
    let kind = ids.to_native.map(IdSpace::EntityType, r.varint()?)?;
    write_varint(&mut out, kind);
    let start = r.pos();
    r.take(3 * 8)?;
    hop::skip_lp_vec3(&mut r)?;
    r.take(3)?;
    out.extend_from_slice(&r.slice_from(0)[start..]);
    let data = r.varint()?;
    let data = if kind == EntityKind::FallingBlock.to_u32() as i32 {
        hop::state(ids, data)?
    } else {
        data
    };
    write_varint(&mut out, data);
    Some(out.into_boxed_slice())
}

/// The fields were reordered around the particle, the speed became one per
/// axis, and the count became a VarInt.
fn level_particles(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let flags = r.take(2)?;
    let position = r.take(3 * 8)?;
    let spread = r.take(3 * 4)?;
    let speed = r.take(4)?;
    let count = i32::from_be_bytes(r.take(4)?.try_into().ok()?);

    let mut out = Vec::with_capacity(body.len() + 12);
    hop::particle(ids, &mut r, &mut out, &mut Vec::new())?;
    out.extend_from_slice(flags);
    out.extend_from_slice(position);
    out.extend_from_slice(spread);
    for _ in 0..3 {
        out.extend_from_slice(speed);
    }
    write_varint(&mut out, count.max(0));
    write_varint(&mut out, 0);
    Some(out.into_boxed_slice())
}

fn explode(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut out = hop::explode(ids, body)?.into_vec();
    out.push(1);
    Some(out.into_boxed_slice())
}

const SWING_ANIMATION_WHACK: i32 = 1;
const SWING_ANIMATION_DEFAULT_TICKS: i32 = 6;

/// Arm swings moved to a packet of their own; the remaining actions were
/// renumbered.
fn animate(_ids: &Ids, body: &[u8]) -> Option<Rewritten> {
    let mut r = Reader::new(body);
    let entity = r.varint()?;
    let action = r.u8()?;
    let mut out = Vec::with_capacity(6);
    write_varint(&mut out, entity);
    let hand = match action {
        0 => 0,
        3 => 1,
        _ => {
            let renumbered = match action {
                2 => 0,
                4 => 1,
                5 => 2,
                _ => return Some(Rewritten::Drop),
            };
            out.push(renumbered);
            return Some(Rewritten::Body(out.into_boxed_slice()));
        }
    };
    write_varint(&mut out, hand);
    write_varint(&mut out, SWING_ANIMATION_WHACK);
    write_varint(&mut out, SWING_ANIMATION_DEFAULT_TICKS);
    Some(Rewritten::As("swing_animation", out.into_boxed_slice()))
}

/// The on-ground flag joined the step count in a leading VarInt.
fn move_entity(body: &[u8], rotation: bool) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 1);
    copy_varint(&mut r, &mut out)?;
    let delta = r.take(3 * 2)?;
    let rot = if rotation { r.take(2)? } else { &[] };
    let on_ground = r.u8()? != 0;
    write_varint(&mut out, i32::from(on_ground));
    out.extend_from_slice(delta);
    out.extend_from_slice(rot);
    Some(out.into_boxed_slice())
}

fn move_entity_pos(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    move_entity(body, false)
}

fn move_entity_pos_rot(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    move_entity(body, true)
}

fn move_entity_rot(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_varint(&mut r, &mut out)?;
    let rot = r.take(2)?;
    out.push(r.u8()?);
    out.extend_from_slice(rot);
    Some(out.into_boxed_slice())
}

/// A sync became a position path with no velocity: a single linear step here.
fn entity_position_sync(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_varint(&mut r, &mut out)?;
    let position = r.take(3 * 8)?;
    r.take(3 * 8)?;
    write_varint(&mut out, 0);
    out.extend_from_slice(position);
    copy(&mut r, &mut out, 2 * 4 + 1)?;
    Some(out.into_boxed_slice())
}

const TEAM_ADD: u8 = 0;
const TEAM_CHANGE: u8 = 2;
const TEAM_COLORS: i32 = 16;

/// Parameters were reordered around the texts, and the colour became an
/// optional `TeamColor` instead of a `ChatFormatting` that used reset for none.
fn set_player_team(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 2);
    hop::identifier(&mut r, &mut out)?;
    let method = r.u8()?;
    out.push(method);
    if matches!(method, TEAM_ADD | TEAM_CHANGE) {
        let display = nbt(&mut r)?;
        let options = r.u8()?;
        let visibility = r.varint()?;
        let collision = r.varint()?;
        let color = r.varint()?;
        out.extend_from_slice(display);
        copy_nbt(&mut r, &mut out)?;
        copy_nbt(&mut r, &mut out)?;
        write_varint(&mut out, visibility);
        write_varint(&mut out, collision);
        if (0..TEAM_COLORS).contains(&color) {
            out.push(1);
            write_varint(&mut out, color);
        } else {
            out.push(0);
        }
        out.push(options);
    }
    out.extend_from_slice(r.rest());
    Some(out.into_boxed_slice())
}

const ADVANCEMENT_HAS_BACKGROUND: i32 = 1;

/// Tree positions moved from the display onto the entry, so advancements with
/// no display now carry a position too.
fn update_advancements(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy(&mut r, &mut out, 1)?;
    copy_list(&mut r, &mut out, |r, out| {
        hop::identifier(r, out)?;
        let parent = r.u8()?;
        out.push(parent);
        if parent != 0 {
            hop::identifier(r, out)?;
        }
        let display = r.u8()?;
        out.push(display);
        let mut position = [0u8; 8];
        if display != 0 {
            copy_nbt(r, out)?;
            copy_nbt(r, out)?;
            hop::template(ids, r, out)?;
            copy_varint(r, out)?;
            let flags_bytes = r.take(4)?;
            let flags = i32::from_be_bytes(flags_bytes.try_into().ok()?);
            out.extend_from_slice(flags_bytes);
            if flags & ADVANCEMENT_HAS_BACKGROUND != 0 {
                hop::identifier(r, out)?;
            }
            position.copy_from_slice(r.take(8)?);
        }
        copy_list(r, out, |r, out| {
            copy_list(r, out, |r, out| hop::identifier(r, out))
        })?;
        copy(r, out, 1)?;
        out.extend_from_slice(&position);
        Some(())
    })?;
    out.extend_from_slice(r.rest());
    Some(out.into_boxed_slice())
}

fn update_attributes(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_varint(&mut r, &mut out)?;
    let snapshots = r.count()?;
    let mut kept = Vec::with_capacity(body.len());
    let mut count = 0;
    for _ in 0..snapshots {
        let attribute = ids.to_native.map(IdSpace::Attribute, r.varint()?);
        let start = r.pos();
        r.take(8)?;
        let modifiers = r.count()?;
        for _ in 0..modifiers {
            let id = r.count()?;
            r.take(id + 8)?;
            r.varint()?;
        }
        if let Some(attribute) = attribute {
            write_varint(&mut kept, attribute);
            kept.extend_from_slice(&r.slice_from(0)[start..]);
            count += 1;
        }
    }
    write_varint(&mut out, count);
    out.extend_from_slice(&kept);
    Some(out.into_boxed_slice())
}

const SLOT_DISPLAY_WITH_ANY_POTION: i32 = 2;
const SLOT_DISPLAY_ONLY_WITH_COMPONENT: i32 = 3;
const SLOT_DISPLAY_ITEM: i32 = 4;
const SLOT_DISPLAY_ITEM_STACK: i32 = 5;
const SLOT_DISPLAY_TAG: i32 = 6;
const SLOT_DISPLAY_DYED: i32 = 7;
const SLOT_DISPLAY_SMITHING_TRIM: i32 = 8;
const SLOT_DISPLAY_WITH_REMAINDER: i32 = 9;
const SLOT_DISPLAY_COMPOSITE: i32 = 10;

/// Slot display types kept their ids; a tag became a holder set.
fn slot_display(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    let kind = copy_varint(r, out)?;
    match kind {
        SLOT_DISPLAY_WITH_ANY_POTION => slot_display(ids, r, out)?,
        SLOT_DISPLAY_ONLY_WITH_COMPONENT => {
            slot_display(ids, r, out)?;
            let component = ids.to_native.map(IdSpace::DataComponentType, r.varint()?)?;
            write_varint(out, component);
        }
        SLOT_DISPLAY_ITEM => hop::item(ids, r, out)?,
        SLOT_DISPLAY_ITEM_STACK => hop::template(ids, r, out)?,
        SLOT_DISPLAY_TAG => {
            write_varint(out, 0);
            hop::identifier(r, out)?;
        }
        SLOT_DISPLAY_DYED | SLOT_DISPLAY_WITH_REMAINDER => {
            slot_display(ids, r, out)?;
            slot_display(ids, r, out)?;
        }
        SLOT_DISPLAY_SMITHING_TRIM => {
            slot_display(ids, r, out)?;
            slot_display(ids, r, out)?;
            if copy_varint(r, out)? == 0 {
                hop::identifier(r, out)?;
                copy_nbt(r, out)?;
                copy(r, out, 1)?;
            }
        }
        SLOT_DISPLAY_COMPOSITE => copy_list(r, out, |r, out| slot_display(ids, r, out))?,
        _ => {}
    }
    Some(())
}

fn update_recipes(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 64);
    copy_list(&mut r, &mut out, |r, out| {
        hop::identifier(r, out)?;
        copy_list(r, out, |r, out| hop::item(ids, r, out))
    })?;
    copy_list(&mut r, &mut out, |r, out| {
        hop::ingredient(ids, r, out)?;
        slot_display(ids, r, out)
    })?;
    (r.remaining() == 0).then(|| out.into_boxed_slice())
}

const RECIPE_SHAPELESS: i32 = 0;
const RECIPE_SHAPED: i32 = 1;
const RECIPE_FURNACE: i32 = 2;
const RECIPE_STONECUTTER: i32 = 3;
const RECIPE_SMITHING: i32 = 4;

fn recipe_display(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    let slots = |r: &mut Reader, out: &mut Vec<u8>, n: usize| -> Option<()> {
        for _ in 0..n {
            slot_display(ids, r, out)?;
        }
        Some(())
    };
    match copy_varint(r, out)? {
        RECIPE_SHAPELESS => {
            copy_list(r, out, |r, out| slot_display(ids, r, out))?;
            slots(r, out, 2)
        }
        RECIPE_SHAPED => {
            copy_varint(r, out)?;
            copy_varint(r, out)?;
            copy_list(r, out, |r, out| slot_display(ids, r, out))?;
            slots(r, out, 2)
        }
        RECIPE_FURNACE => {
            slots(r, out, 4)?;
            copy_varint(r, out)?;
            copy(r, out, 4)
        }
        RECIPE_STONECUTTER => slots(r, out, 3),
        RECIPE_SMITHING => slots(r, out, 5),
        _ => None,
    }
}

fn recipe_book_add(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 64);
    copy_list(&mut r, &mut out, |r, out| {
        copy_varint(r, out)?;
        recipe_display(ids, r, out)?;
        copy_varint(r, out)?;
        copy_varint(r, out)?;
        let requirements = r.u8()?;
        out.push(requirements);
        if requirements != 0 {
            copy_list(r, out, |r, out| hop::ingredient(ids, r, out))?;
        }
        copy(r, out, 1)
    })?;
    copy(&mut r, &mut out, 1)?;
    (r.remaining() == 0).then(|| out.into_boxed_slice())
}

const NODE_ARGUMENT: u8 = 2;
const NODE_REDIRECT: u8 = 0x08;
const NODE_SUGGESTIONS: u8 = 0x10;

/// Five argument types went in after `loot_modifier`, which moved `dialog`
/// and `uuid` up; `color` became `team_color` in the same slot.
fn argument_type_775_to_777(id: i32) -> i32 {
    match id {
        55 => 58,
        56 => 61,
        id => id,
    }
}

fn argument_properties(id: i32, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
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
    match id {
        1 => ranged(r, out, 4),
        2 => ranged(r, out, 8),
        3 => ranged(r, out, 4),
        4 => ranged(r, out, 8),
        5 => copy_varint(r, out).map(drop),
        6 | 31 => copy(r, out, 1),
        43 => copy(r, out, 4),
        44..=48 => hop::identifier(r, out),
        _ => Some(()),
    }
}

fn commands(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len());
    copy_list(&mut r, &mut out, |r, out| {
        let flags = r.u8()?;
        out.push(flags);
        copy_list(r, out, |r, out| copy_varint(r, out).map(drop))?;
        if flags & NODE_REDIRECT != 0 {
            copy_varint(r, out)?;
        }
        match flags & 3 {
            0 => {}
            NODE_ARGUMENT => {
                hop::identifier(r, out)?;
                let parser = r.varint()?;
                write_varint(out, argument_type_775_to_777(parser));
                argument_properties(parser, r, out)?;
            }
            _ => hop::identifier(r, out)?,
        }
        if flags & NODE_SUGGESTIONS != 0 {
            hop::identifier(r, out)?;
        }
        Some(())
    })?;
    copy_varint(&mut r, &mut out)?;
    (r.remaining() == 0).then(|| out.into_boxed_slice())
}

fn item_cost(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    hop::item(ids, r, out)?;
    copy_varint(r, out)?;
    let expected = r.count()?;
    let mut components = Vec::new();
    let mut kept = 0;
    for _ in 0..expected {
        if component(ids, r, &mut components)? {
            kept += 1;
        }
    }
    write_varint(out, kept);
    out.extend_from_slice(&components);
    Some(())
}

fn merchant_offers(ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(body.len() + 16);
    copy_varint(&mut r, &mut out)?;
    copy_list(&mut r, &mut out, |r, out| {
        item_cost(ids, r, out)?;
        hop::item_stack(ids, r, out)?;
        let second = r.u8()?;
        out.push(second);
        if second != 0 {
            item_cost(ids, r, out)?;
        }
        copy(r, out, 1 + 4 * 4 + 4 + 4)
    })?;
    out.extend_from_slice(r.rest());
    Some(out.into_boxed_slice())
}

/// Walks one component with its payload. `false` means 26.3 has no such
/// component and it was consumed without being written.
fn component(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Option<bool> {
    let wire = r.varint()?;
    match wire {
        MAP_COLOR_775 => {
            r.take(4)?;
            return Some(false);
        }
        // The attack swing kept its payload under a new name.
        SWING_ANIMATION_775 => {
            write_varint(out, DataComponentKind::AttackAnimation.to_u32() as i32);
            copy_varint(r, out)?;
            copy_varint(r, out)?;
            return Some(true);
        }
        _ => {}
    }
    let native = ids.to_native.map(IdSpace::DataComponentType, wire)?;
    write_varint(out, native);
    if wire == POT_DECORATIONS_775 {
        // A list of up to four sherd items became four optional stacks.
        let sides = r.count()?;
        let mut items = [None; 4];
        for side in 0..sides {
            let item = ids.to_native.map(IdSpace::Item, r.varint()?)?;
            if let Some(slot) = items.get_mut(side) {
                *slot = Some(item);
            }
        }
        for item in items {
            match item {
                Some(item) => {
                    out.push(1);
                    write_varint(out, item);
                    write_varint(out, 1);
                    write_varint(out, 0);
                    write_varint(out, 0);
                }
                None => out.push(0),
            }
        }
        return Some(true);
    }
    let kind = DataComponentKind::from_u32(u32::try_from(native).ok()?)?;
    hop::component_payload(ids, kind, r, out)?;
    Some(true)
}

fn patch(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Option<()> {
    let with_payload = r.count()?;
    let without = r.count()?;
    let mut present = Vec::new();
    let mut kept = 0;
    for _ in 0..with_payload {
        if component(ids, r, &mut present)? {
            kept += 1;
        }
    }
    let mut removed = Vec::new();
    let mut gone = 0;
    for _ in 0..without {
        if let Some(native) = ids.to_native.map(IdSpace::DataComponentType, r.varint()?) {
            write_varint(&mut removed, native);
            gone += 1;
        }
    }
    write_varint(out, kept);
    write_varint(out, gone);
    out.extend_from_slice(&present);
    out.extend_from_slice(&removed);
    Some(())
}

fn punch(_ids: &Ids, _body: &[u8]) -> Option<Box<[u8]>> {
    Some(Box::new([0]))
}

/// 26.1 only echoes the id back; the position and rotation are 26.3's check.
fn accept_teleportation(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let mut out = Vec::with_capacity(5);
    copy_varint(&mut r, &mut out)?;
    Some(out.into_boxed_slice())
}

const ACTION_CHANGE_DESTROY_DIRECTION: i32 = 1;

/// 26.3 put a new action in at 1, so every later one is a step lower in 26.1,
/// which has no way to say the mining face changed.
fn player_action(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let action = r.varint()?;
    let action = match action {
        ACTION_CHANGE_DESTROY_DIRECTION => return None,
        0 => 0,
        a => a - 1,
    };
    let mut out = Vec::with_capacity(body.len());
    write_varint(&mut out, action);
    out.extend_from_slice(r.rest());
    Some(out.into_boxed_slice())
}

/// The front-or-back slot moved from before the lines to after them.
fn sign_update(_ids: &Ids, body: &[u8]) -> Option<Box<[u8]>> {
    let mut r = Reader::new(body);
    let pos = r.take(8)?;
    let mut lines = Vec::with_capacity(body.len());
    for _ in 0..4 {
        hop::identifier(&mut r, &mut lines)?;
    }
    let front = r.varint()? != 0;
    let mut out = Vec::with_capacity(body.len());
    out.extend_from_slice(pos);
    out.push(u8::from(front));
    out.extend_from_slice(&lines);
    Some(out.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use azalea_buf::{AzBuf, AzBufVar};
    use azalea_protocol::packets::game::ClientboundGamePacket;
    use azalea_protocol::packets::{ConnectionProtocol, ProtocolPacket};

    use super::*;
    use crate::protocol::hop::Hop;
    use crate::protocol::packets::Direction;
    use crate::protocol::wire::write_varint;

    fn hop() -> &'static Hop {
        Hop::v775()
    }

    fn ids() -> &'static Ids {
        hop().ids()
    }

    fn wire_frame(name: &str, body: &[u8]) -> Box<[u8]> {
        let id = PacketTable::v26_1()
            .id(Phase::Game, Direction::Clientbound, name)
            .unwrap_or_else(|| panic!("775 has {name}"));
        hop::frame(id, body)
    }

    /// Runs a 26.1 frame through the hop and decodes it with azalea's 26.3
    /// types, which is the check that matters.
    fn decode(name: &str, body: &[u8]) -> ClientboundGamePacket {
        let out = hop().inbound(ConnectionProtocol::Game, wire_frame(name, body));
        assert!(!out.is_empty(), "{name} was dropped");
        let mut cursor = std::io::Cursor::new(&out[..]);
        let id = u32::azalea_read_var(&mut cursor).unwrap();
        let packet = ClientboundGamePacket::read(id, &mut cursor)
            .unwrap_or_else(|e| panic!("{name} does not decode as 777: {e}"));
        assert_eq!(
            cursor.position() as usize,
            out.len(),
            "{name} left bytes over"
        );
        packet
    }

    #[test]
    fn a_relative_move_decodes_with_its_ground_flag() {
        let mut body = Vec::new();
        write_varint(&mut body, 42);
        body.extend_from_slice(&[0, 1, 0, 2, 0, 3]);
        body.extend_from_slice(&[64, 32]);
        body.push(1);
        let ClientboundGamePacket::MoveEntityPosRot(p) = decode("move_entity_pos_rot", &body)
        else {
            panic!("wrong packet")
        };
        assert_eq!(p.entity_id.0, 42);
        assert!(p.properties.on_ground());
    }

    #[test]
    fn a_rotation_only_move_puts_the_ground_flag_first() {
        let mut body = Vec::new();
        write_varint(&mut body, 7);
        body.extend_from_slice(&[10, 20, 1]);
        let ClientboundGamePacket::MoveEntityRot(p) = decode("move_entity_rot", &body) else {
            panic!("wrong packet")
        };
        assert!(p.on_ground);
    }

    #[test]
    fn a_main_hand_swing_becomes_a_swing_animation() {
        let mut body = Vec::new();
        write_varint(&mut body, 9);
        body.push(0);
        let ClientboundGamePacket::SwingAnimation(p) = decode("animate", &body) else {
            panic!("a swing should change packet")
        };
        assert_eq!(p.entity_id.0, 9);
    }

    #[test]
    fn a_critical_hit_keeps_its_packet_under_its_new_number() {
        use azalea_protocol::packets::game::c_animate::AnimationAction;
        let mut body = Vec::new();
        write_varint(&mut body, 9);
        body.push(4);
        let ClientboundGamePacket::Animate(p) = decode("animate", &body) else {
            panic!("wrong packet")
        };
        assert_eq!(p.action, AnimationAction::CriticalHit);
    }

    #[test]
    fn particles_are_reordered_and_their_speed_is_spread_over_three_axes() {
        let mut body = vec![1, 0];
        body.extend_from_slice(&[0u8; 3 * 8]);
        body.extend_from_slice(&[0u8; 3 * 4]);
        body.extend_from_slice(&0.5f32.to_be_bytes());
        body.extend_from_slice(&12i32.to_be_bytes());
        write_varint(&mut body, 3);
        let ClientboundGamePacket::LevelParticles(p) = decode("level_particles", &body) else {
            panic!("wrong packet")
        };
        assert_eq!(p.count, 12);
        assert!(p.override_limiter);
        assert_eq!(p.max_speed.y, 0.5);
    }

    fn component_bytes(text: &str) -> Vec<u8> {
        let mut out = vec![8u8];
        out.extend_from_slice(&(text.len() as u16).to_be_bytes());
        out.extend_from_slice(text.as_bytes());
        out
    }

    #[test]
    fn team_parameters_are_reordered_and_reset_means_no_colour() {
        let mut body = Vec::new();
        write_varint(&mut body, 4);
        body.extend_from_slice(b"blue");
        body.push(TEAM_CHANGE);
        body.extend_from_slice(&component_bytes("Blue"));
        body.push(3);
        write_varint(&mut body, 1);
        write_varint(&mut body, 2);
        write_varint(&mut body, 21);
        body.extend_from_slice(&component_bytes("[B] "));
        body.extend_from_slice(&component_bytes(""));
        let ClientboundGamePacket::SetPlayerTeam(p) = decode("set_player_team", &body) else {
            panic!("wrong packet")
        };
        use azalea_protocol::packets::game::c_set_player_team::Method;
        let Method::Change(params) = p.method else {
            panic!("wrong method")
        };
        assert_eq!(params.color, None);
        assert_eq!(params.options, 3);
        assert_eq!(params.player_prefix.to_string(), "[B] ");
    }

    #[test]
    fn a_long_bitset_becomes_its_bytes() {
        let mut body = Vec::new();
        write_varint(&mut body, 1);
        body.extend_from_slice(&0x0000_0000_0000_0105u64.to_be_bytes());
        let mut out = Vec::new();
        bit_set(&mut Reader::new(&body), &mut out).unwrap();
        assert_eq!(
            out,
            [2, 0x05, 0x01],
            "little-endian, trailing zeros trimmed"
        );
    }

    #[test]
    fn previous_game_mode_becomes_an_optional_varint() {
        for (wire, expected) in [(0xFFu8, 0), (0u8, 1), (3u8, 4)] {
            let mut body = Vec::new();
            write_varint(&mut body, 0);
            write_varint(&mut body, 19);
            body.extend_from_slice(b"minecraft:overworld");
            body.extend_from_slice(&[0; 8]);
            body.push(1);
            body.push(wire);
            body.extend_from_slice(&[0, 0, 0]);
            write_varint(&mut body, 0);
            write_varint(&mut body, 63);
            let mut out = Vec::new();
            spawn_info(&mut Reader::new(&body), &mut out).unwrap();
            let mut r = Reader::new(&out);
            r.varint();
            r.count().and_then(|n| r.take(n));
            r.take(8);
            assert_eq!(r.varint(), Some(1), "creative");
            assert_eq!(r.varint(), Some(expected));
        }
    }

    #[test]
    fn a_bed_block_entity_is_dropped() {
        let dropped = (0..Remap::v775_to_v777().count(IdSpace::BlockEntityType))
            .find(|id| ids().to_native.map(IdSpace::BlockEntityType, *id).is_none())
            .expect("one 775 block entity type has no 777 id");
        let mut body = vec![0u8; 8];
        write_varint(&mut body, dropped);
        body.push(0);
        assert!(matches!(
            block_entity_data(ids(), &body),
            Some(Rewritten::Drop)
        ));
    }

    #[test]
    fn mining_actions_step_down_and_a_face_change_is_not_sent() {
        let mut body = Vec::new();
        write_varint(&mut body, 3);
        body.extend_from_slice(&[0; 8]);
        let out = player_action(ids(), &body).unwrap();
        assert_eq!(Reader::new(&out).varint(), Some(2), "stop destroying");
        let mut body = Vec::new();
        write_varint(&mut body, ACTION_CHANGE_DESTROY_DIRECTION);
        assert!(player_action(ids(), &body).is_none());
    }

    #[test]
    fn a_sign_update_moves_its_side_flag_back_to_the_front() {
        let mut body = vec![0u8; 8];
        for line in ["a", "", "b", ""] {
            write_varint(&mut body, line.len() as i32);
            body.extend_from_slice(line.as_bytes());
        }
        write_varint(&mut body, 1);
        let out = sign_update(ids(), &body).unwrap();
        assert_eq!(out[8], 1, "front");
        assert_eq!(&out[9..11], &[1, b'a']);
    }

    #[test]
    fn the_new_serverbound_packets_reach_a_775_server() {
        let native = PacketTable::native();
        let punch = native
            .id(Phase::Game, Direction::Serverbound, "punch")
            .unwrap();
        let out = hop().outbound(ConnectionProtocol::Game, hop::frame(punch, &[]));
        let mut r = Reader::new(&out);
        assert_eq!(
            PacketTable::v26_1().name_of(
                Phase::Game,
                Direction::Serverbound,
                r.varint().unwrap() as u32
            ),
            Some("swing")
        );
        assert_eq!(r.varint(), Some(0), "main hand");

        let spectator = native
            .id(Phase::Game, Direction::Serverbound, "spectator_action")
            .unwrap();
        assert!(
            hop()
                .outbound(ConnectionProtocol::Game, hop::frame(spectator, &[0]))
                .is_empty()
        );
    }

    #[test]
    fn a_pot_with_two_sherds_becomes_four_optional_stacks() {
        let mut body = Vec::new();
        write_varint(&mut body, 1);
        write_varint(&mut body, 0);
        write_varint(&mut body, POT_DECORATIONS_775);
        write_varint(&mut body, 2);
        write_varint(&mut body, 1);
        write_varint(&mut body, 2);
        let mut out = Vec::new();
        patch(ids(), &mut Reader::new(&body), &mut out).unwrap();

        let mut cursor = std::io::Cursor::new(&out[..]);
        let patch = azalea_inventory::DataComponentPatch::azalea_read(&mut cursor)
            .expect("26.3 reads the converted patch");
        let pot = patch
            .get::<azalea_inventory::components::PotDecorations>()
            .expect("the decorations survived");
        assert!(pot.back.is_some() && pot.left.is_some());
        assert!(pot.right.is_none() && pot.front.is_none());
    }

    #[test]
    fn a_removed_component_is_left_out_of_the_patch() {
        let mut body = Vec::new();
        write_varint(&mut body, 1);
        write_varint(&mut body, 0);
        write_varint(&mut body, MAP_COLOR_775);
        body.extend_from_slice(&0x00FF_00FFu32.to_be_bytes());
        let mut out = Vec::new();
        patch(ids(), &mut Reader::new(&body), &mut out).unwrap();
        assert_eq!(out, [0, 0]);
    }

    fn long_bitset(body: &mut Vec<u8>, bits: u64) {
        write_varint(body, 1);
        body.extend_from_slice(&bits.to_be_bytes());
    }

    #[test]
    fn a_chunk_moves_its_states_keeps_its_chest_and_reencodes_its_light() {
        let stone_775 = 1;
        let acacia_button_775 = 10_771;
        let mut blob = Vec::new();
        for i in 0..24 {
            blob.extend_from_slice(&[0, 1, 0, 0]);
            blob.push(0);
            write_varint(
                &mut blob,
                if i == 0 { acacia_button_775 } else { stone_775 },
            );
            blob.push(0);
            write_varint(&mut blob, 0);
        }

        let chest_775 = 1;
        let mut body = Vec::new();
        body.extend_from_slice(&3i32.to_be_bytes());
        body.extend_from_slice(&(-4i32).to_be_bytes());
        write_varint(&mut body, 0);
        write_varint(&mut body, blob.len() as i32);
        body.extend_from_slice(&blob);
        write_varint(&mut body, 1);
        body.push(0x12);
        body.extend_from_slice(&64i16.to_be_bytes());
        write_varint(&mut body, chest_775);
        body.push(10);
        body.push(0);
        long_bitset(&mut body, 0b10);
        long_bitset(&mut body, 0);
        long_bitset(&mut body, 0b1);
        write_varint(&mut body, 0);
        write_varint(&mut body, 1);
        write_varint(&mut body, 2048);
        body.extend_from_slice(&[0x11; 2048]);
        write_varint(&mut body, 0);

        let ClientboundGamePacket::LevelChunkWithLight(p) = decode("level_chunk_with_light", &body)
        else {
            panic!("wrong packet")
        };
        assert_eq!((p.x, p.z), (3, -4));
        assert_eq!(
            p.chunk_data.block_entities.len(),
            1,
            "a chest is still an entity"
        );
        assert_eq!(&p.light_data.sky_y_mask.data[..], &[0b10]);
        assert!(p.light_data.block_y_mask.data.is_empty());
        assert_eq!(p.light_data.sky_updates.len(), 1);

        let chunk = azalea_world::Chunk::read_with_dimension_height(
            &mut std::io::Cursor::new(&p.chunk_data.buffer[..]),
            384,
            -64,
            &[],
        )
        .expect("the sections decode at 26.3 ids");
        let button = chunk.sections[0].states.get_at_index(0);
        assert_eq!(button.id(), 12_514, "acacia_button moved");
    }

    #[test]
    fn a_direct_palette_widens_to_the_native_registry() {
        const BITS: usize = 15;
        let per_long = 64 / BITS;
        let high_775 = 29_000i64;
        let mut packed = vec![0u64; 4096usize.div_ceil(per_long)];
        for i in 0..4096 {
            packed[i / per_long] |= (high_775 as u64) << ((i % per_long) * BITS);
        }
        let mut blob = vec![0x10, 0x00, 0, 0, BITS as u8];
        for word in &packed {
            blob.extend_from_slice(&word.to_be_bytes());
        }
        blob.push(0);
        write_varint(&mut blob, 0);

        let out = hop::remap_sections(ids(), &blob, true).expect("a whole section");
        assert_eq!(out[4], 16, "26.3 has more states than fit in 15 bits");
        let expected = ids()
            .to_native
            .map(IdSpace::BlockState, high_775 as i32)
            .unwrap() as u64;
        assert!(
            expected > (1 << 15) - 1,
            "the test needs a state past 15 bits"
        );
        let first = u64::from_be_bytes(out[5..13].try_into().unwrap());
        assert_eq!(first & 0xFFFF, expected);
    }

    #[test]
    fn login_gains_an_online_mode_flag_and_decodes() {
        let mut body = Vec::new();
        body.extend_from_slice(&5i32.to_be_bytes());
        body.push(0);
        write_varint(&mut body, 1);
        write_varint(&mut body, 19);
        body.extend_from_slice(b"minecraft:overworld");
        for v in [20, 10, 8] {
            write_varint(&mut body, v);
        }
        body.extend_from_slice(&[0, 1, 0]);
        write_varint(&mut body, 0);
        write_varint(&mut body, 19);
        body.extend_from_slice(b"minecraft:overworld");
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(&[0, 0xFF, 0, 0, 0]);
        write_varint(&mut body, 0);
        write_varint(&mut body, 63);
        body.push(1);
        let ClientboundGamePacket::Login(p) = decode("login", &body) else {
            panic!("wrong packet")
        };
        assert!(p.online_mode);
        assert!(p.enforces_secure_chat);
        assert_eq!(p.common.previous_game_type.0, None);
    }

    #[test]
    fn an_advancement_position_moves_onto_its_entry() {
        let mut body = vec![1];
        write_varint(&mut body, 1);
        write_varint(&mut body, 6);
        body.extend_from_slice(b"a:root");
        body.push(0);
        body.push(1);
        body.extend_from_slice(&component_bytes("Root"));
        body.extend_from_slice(&component_bytes("The root"));
        write_varint(&mut body, 1);
        write_varint(&mut body, 1);
        write_varint(&mut body, 0);
        write_varint(&mut body, 0);
        write_varint(&mut body, 0);
        body.extend_from_slice(&0i32.to_be_bytes());
        body.extend_from_slice(&2.5f32.to_be_bytes());
        body.extend_from_slice(&(-1.0f32).to_be_bytes());
        write_varint(&mut body, 0);
        body.push(0);
        write_varint(&mut body, 0);
        write_varint(&mut body, 0);
        body.push(1);
        let ClientboundGamePacket::UpdateAdvancements(p) = decode("update_advancements", &body)
        else {
            panic!("wrong packet")
        };
        assert_eq!(p.added.len(), 1);
        assert_eq!((p.added[0].x, p.added[0].y), (2.5, -1.0));
        assert!(p.show_advancements);
    }

    #[test]
    fn a_uuid_argument_is_renumbered_in_the_command_tree() {
        let mut body = Vec::new();
        write_varint(&mut body, 2);
        body.push(0);
        write_varint(&mut body, 1);
        write_varint(&mut body, 1);
        body.push(NODE_ARGUMENT | 0x04);
        write_varint(&mut body, 0);
        write_varint(&mut body, 6);
        body.extend_from_slice(b"target");
        write_varint(&mut body, 56);
        write_varint(&mut body, 0);
        let ClientboundGamePacket::Commands(p) = decode("commands", &body) else {
            panic!("wrong packet")
        };
        use azalea_protocol::packets::game::c_commands::{BrigadierParser, NodeType};
        let NodeType::Argument { parser, .. } = &p.entries[1].node_type else {
            panic!("not an argument")
        };
        assert_eq!(*parser, BrigadierParser::Uuid);
    }

    #[test]
    fn a_tag_slot_display_becomes_a_holder_set() {
        let mut body = Vec::new();
        write_varint(&mut body, SLOT_DISPLAY_TAG);
        write_varint(&mut body, 3);
        body.extend_from_slice(b"a:b");
        let mut out = Vec::new();
        slot_display(ids(), &mut Reader::new(&body), &mut out).unwrap();
        let mut cursor = std::io::Cursor::new(&out[..]);
        let display =
            azalea_protocol::common::recipe::SlotDisplayData::azalea_read(&mut cursor).unwrap();
        assert!(matches!(
            display,
            azalea_protocol::common::recipe::SlotDisplayData::Tag(_)
        ));
        assert_eq!(cursor.position() as usize, out.len());
    }

    #[test]
    fn a_session_id_is_added_to_login_finished() {
        let id = PacketTable::v26_1()
            .id(Phase::Login, Direction::Clientbound, "login_finished")
            .unwrap();
        let mut body = vec![0u8; 16];
        write_varint(&mut body, 3);
        body.extend_from_slice(b"Bob");
        write_varint(&mut body, 0);
        let out = hop().inbound(ConnectionProtocol::Login, hop::frame(id, &body));
        let mut cursor = std::io::Cursor::new(&out[..]);
        let packet_id = u32::azalea_read_var(&mut cursor).unwrap();
        let packet =
            <azalea_protocol::packets::login::ClientboundLoginPacket as ProtocolPacket>::read(
                packet_id,
                &mut cursor,
            )
            .expect("26.3 reads it");
        assert!(matches!(
            packet,
            azalea_protocol::packets::login::ClientboundLoginPacket::LoginFinished(_)
        ));
    }
}
