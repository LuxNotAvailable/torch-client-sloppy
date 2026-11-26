//! 1.21.1 item stacks as 1.21.11 sends them: the component patch walker for
//! the 767 hop. Components are matched by name (`data/data_component_type-
//! <ver>.json`), and each 1.21.1 shape is read and rewritten:
//!
//! - unchanged shapes are copied, with registry ids (items, blocks, effects,
//!   potions, attributes) moved through the hop
//! - every `show_in_tooltip` flag (gone in 1.21.5) and `hide_tooltip` are
//!   gathered into one `tooltip_display`
//! - `fire_resistant` becomes `damage_resistant` (`#is_fire`), a custom
//!   model number becomes 1.21.4's float list, a profile 1.21.9's partial
//!   one, and fields added since (a potion's name, a tool's creative flag,
//!   an attribute's display, a block predicate's component matchers) are
//!   written empty
//! - shapes this walker can't carry (entity and block entity data, which
//!   became typed; goat horns; inline holders) are read and dropped
//!
//! A component it can't read at all (a modded one) ends the walk: the stack
//! keeps its item and count, and the caller stops reading its packet's
//! stacks, since nothing marks where the unreadable bytes end.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::hop::{self, Ids};
use super::registries::RegistryId;
use super::remap::IdSpace;
use super::wire::{Reader, write_varint};

/// A stack's components couldn't be read to their end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreadable {
    /// A component this walker doesn't know (a mod's): its bytes go on.
    Unknown,
    /// Bytes that don't read as a stack at all.
    Malformed,
}

type Walk = Result<(), Unreadable>;

struct Names {
    old: Vec<String>,
    new: HashMap<String, i32>,
}

fn names() -> &'static Names {
    static NAMES: OnceLock<Names> = OnceLock::new();
    NAMES.get_or_init(|| {
        let parse = |json: &str| -> Vec<String> {
            serde_json::from_str(json).expect("embedded component names")
        };
        let old = parse(include_str!("data/data_component_type-1.21.1.json"));
        let new = parse(include_str!("data/data_component_type-1.21.11.json"))
            .into_iter()
            .enumerate()
            .map(|(id, name)| (name, id as i32))
            .collect();
        Names { old, new }
    })
}

fn native(name: &str) -> Option<i32> {
    names().new.get(name).copied()
}

trait Need<T> {
    fn need(self) -> Result<T, Unreadable>;
}

impl<T> Need<T> for Option<T> {
    fn need(self) -> Result<T, Unreadable> {
        self.ok_or(Unreadable::Malformed)
    }
}

fn copy(r: &mut Reader, out: &mut Vec<u8>, n: usize) -> Walk {
    out.extend_from_slice(r.take(n).need()?);
    Ok(())
}

fn varint(r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    write_varint(out, r.varint().need()?);
    Ok(())
}

fn string(r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    let len = r.count().need()?;
    write_varint(out, len as i32);
    copy(r, out, len)
}

fn nbt(r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    let start = r.pos();
    r.skip_nbt().need()?;
    out.extend_from_slice(r.slice_from(start));
    Ok(())
}

fn flag(r: &mut Reader, out: &mut Vec<u8>) -> Result<bool, Unreadable> {
    let b = r.u8().need()?;
    out.push(b);
    Ok(b != 0)
}

fn optional(
    r: &mut Reader,
    out: &mut Vec<u8>,
    inner: impl FnOnce(&mut Reader, &mut Vec<u8>) -> Walk,
) -> Walk {
    if flag(r, out)? {
        inner(r, out)?;
    }
    Ok(())
}

fn list(
    r: &mut Reader,
    out: &mut Vec<u8>,
    mut each: impl FnMut(&mut Reader, &mut Vec<u8>) -> Walk,
) -> Walk {
    let n = r.count().need()?;
    write_varint(out, n as i32);
    for _ in 0..n {
        each(r, out)?;
    }
    Ok(())
}

fn moved(ids: &Ids, space: IdSpace, r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    write_varint(out, hop::shifted(ids, space, r.varint().need()?).need()?);
    Ok(())
}

/// A registry holder: id + 1, or 0 and the value inline (not carried).
fn holder(r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    match r.varint().need()? {
        0 => Err(Unreadable::Malformed),
        id => {
            write_varint(out, id);
            Ok(())
        }
    }
}

/// `HolderSet<Block>`: 0 and a tag, or the count + 1 and the ids.
fn block_set(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    let n = r.varint().need()?;
    write_varint(out, n);
    if n == 0 {
        return hop::identifier(r, out).need();
    }
    for _ in 0..n - 1 {
        moved(ids, IdSpace::Block, r, out)?;
    }
    Ok(())
}

/// An item stack (`OPTIONAL_STREAM_CODEC`), its item moved and its patch
/// walked. On `Err` a whole stack is still written: without its patch, or
/// empty when even its item didn't read.
pub(super) fn stack(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    let mut head = Vec::new();
    let read_head = |r: &mut Reader, head: &mut Vec<u8>| -> Result<bool, Unreadable> {
        let count = r.varint().need()?;
        write_varint(head, count);
        if count <= 0 {
            return Ok(false);
        }
        moved(ids, IdSpace::Item, r, head)?;
        Ok(true)
    };
    match read_head(r, &mut head) {
        Ok(false) => {
            out.extend_from_slice(&head);
            return Ok(());
        }
        Ok(true) => {}
        Err(e) => {
            write_varint(out, 0);
            return Err(e);
        }
    }
    let mut patch_out = Vec::new();
    let walked = patch(ids, r, &mut patch_out);
    out.extend_from_slice(&head);
    match walked {
        Ok(()) => out.extend_from_slice(&patch_out),
        Err(_) => {
            write_varint(out, 0);
            write_varint(out, 0);
        }
    }
    walked
}

/// Reading budget for a `Search`: steps over one packet, so a hopeless one
/// gives up instead of searching on.
const SEARCH_STEPS: usize = 200_000;

/// A guess at a mod component's encoding. Most are one of these: a marker,
/// a number, a few fixed bytes, an NBT tag or a string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Unit,
    VarInt,
    Byte,
    Int,
    Long,
    Nbt,
    String,
    Uuid,
}

/// Guessing order. A marker (`Unit`) fits anywhere, so it goes last: tried
/// early it sends the search through the next component's bytes.
const SHAPES: [Shape; 8] = [
    Shape::Int,
    Shape::VarInt,
    Shape::Byte,
    Shape::Long,
    Shape::Uuid,
    Shape::String,
    Shape::Nbt,
    Shape::Unit,
];

impl Shape {
    fn skip(self, r: &mut Reader) -> Option<()> {
        match self {
            Shape::Unit => Some(()),
            Shape::VarInt => r.varint().map(drop),
            Shape::Byte => r.take(1).map(drop),
            Shape::Int => r.take(4).map(drop),
            Shape::Long => r.take(8).map(drop),
            Shape::Uuid => r.take(16).map(drop),
            Shape::Nbt => r.skip_nbt(),
            Shape::String => {
                let len = r.count()?;
                r.take(len).map(drop)
            }
        }
    }
}

/// Mod components' shapes learned from packets that read to their end, by
/// component name. A learned shape lets any later stack skip that component
/// without a search.
static LEARNED: std::sync::LazyLock<std::sync::Mutex<HashMap<String, Shape>>> =
    std::sync::LazyLock::new(Default::default);

fn learned(name: &str) -> Option<Shape> {
    LEARNED.lock().unwrap().get(name).copied()
}

/// Shapes to try for a mod component whose bytes start with `lead`: the
/// learned one first, then NBT when `lead` is a tag type a component's tag
/// starts with, or a byte when it reads as a flag.
fn guesses(name: &str, lead: Option<u8>) -> Vec<Shape> {
    let mut order: Vec<Shape> = learned(name).into_iter().collect();
    match lead {
        Some(8..=10) => order.push(Shape::Nbt),
        // Most likely a flag: tried as an int it swallows what follows.
        Some(0 | 1) => order.push(Shape::Byte),
        _ => {}
    }
    for shape in SHAPES {
        if !order.contains(&shape) {
            order.push(shape);
        }
    }
    order
}

/// `n` stacks starting at `start` that read exactly to the end of `buf`.
/// A mod component's encoding isn't known, so each is guessed (`Shape`, in
/// `guesses` order) and the first guess whose reading reaches the packet's
/// end exactly stands. Each state's result is remembered, so the search
/// stays near linear.
///
/// `max_count` bounds a stack's count where the slots can't hold more (the
/// player's inventory: 99), which rejects most misaligned reads.
pub(super) fn stacks_to_end(
    ids: &Ids,
    buf: &[u8],
    start: usize,
    n: usize,
    max_count: Option<i32>,
) -> Option<Vec<u8>> {
    let end = |at: usize| at == buf.len();
    stacks_until(ids, buf, start, n, max_count, &end).map(|(bytes, _)| bytes)
}

/// As `stacks_to_end`, but the reading ends wherever `ends_here` accepts
/// (what follows in a packet too long to read to its end). The stacks'
/// bytes and the position after them.
pub(super) fn stacks_until(
    ids: &Ids,
    buf: &[u8],
    start: usize,
    n: usize,
    max_count: Option<i32>,
    ends_here: &dyn Fn(usize) -> bool,
) -> Option<(Vec<u8>, usize)> {
    let mut search = Search {
        ids,
        buf,
        max_count,
        ends_here,
        slots_memo: HashMap::new(),
        patch_memo: HashMap::new(),
        steps: SEARCH_STEPS,
    };
    let best = search.slots(start, n)?;
    let mut cache = LEARNED.lock().unwrap();
    for (name, shape) in &best.score.shapes {
        // A guess can misread the stacks after it: each is worth a known
        // shape in `mod_component`.
        if cache.insert(name.clone(), *shape).is_none() {
            crate::log_warn!("proto", "guessed mod component {name} as {shape:?}");
        }
    }
    Some((best.bytes.clone(), best.end))
}

/// The guesses a reading made.
#[derive(Clone, Default)]
struct Score {
    shapes: Vec<(String, Shape)>,
}

/// The best reading of `n` stacks from some position to the end.
struct Slots {
    bytes: Vec<u8>,
    score: Score,
    /// Where the reading ended.
    end: usize,
}

/// The best reading of the rest of a patch, then of the stacks after it.
struct PatchTail {
    entries: Vec<(i32, Vec<u8>)>,
    gone: Vec<i32>,
    tooltip: Tooltip,
    rest: Rc<Slots>,
    score: Score,
}

use std::rc::Rc;

struct Search<'a, 'i> {
    ids: &'i Ids,
    buf: &'a [u8],
    max_count: Option<i32>,
    ends_here: &'i dyn Fn(usize) -> bool,
    /// `(position, stacks left)`.
    slots_memo: HashMap<(usize, usize), Option<Rc<Slots>>>,
    /// `(position, components left, removed, stacks left)`.
    patch_memo: HashMap<(usize, usize, usize, usize), Option<Rc<PatchTail>>>,
    steps: usize,
}

impl<'a> Search<'a, '_> {
    fn reader(&self, at: usize) -> Option<Reader<'a>> {
        let mut r = Reader::new(self.buf);
        r.take(at)?;
        Some(r)
    }

    fn step(&mut self) -> Option<()> {
        self.steps = self.steps.checked_sub(1)?;
        Some(())
    }

    /// `n` stacks from `at` to the end.
    fn slots(&mut self, at: usize, n: usize) -> Option<Rc<Slots>> {
        if let Some(done) = self.slots_memo.get(&(at, n)) {
            return done.clone();
        }
        self.step()?;
        let found = if n == 0 {
            (self.ends_here)(at).then(|| {
                Rc::new(Slots {
                    bytes: Vec::new(),
                    score: Score::default(),
                    end: at,
                })
            })
        } else {
            self.slot(at, n).map(Rc::new)
        };
        self.slots_memo.insert((at, n), found.clone());
        found
    }

    fn slot(&mut self, at: usize, n: usize) -> Option<Slots> {
        let mut r = self.reader(at)?;
        let count = r.varint()?;
        let mut bytes = Vec::new();
        write_varint(&mut bytes, count);
        if count <= 0 {
            let rest = self.slots(r.pos(), n - 1)?;
            bytes.extend_from_slice(&rest.bytes);
            return Some(Slots {
                bytes,
                score: rest.score.clone(),
                end: rest.end,
            });
        }
        if self.max_count.is_some_and(|max| count > max) {
            return None;
        }
        let item = r.varint()?;
        let known = u32::try_from(item)
            .ok()
            .and_then(|id| crate::protocol::wire_registry_has(RegistryId::Item, id));
        if known == Some(false) {
            return None;
        }
        write_varint(&mut bytes, hop::shifted(self.ids, IdSpace::Item, item)?);
        let added = r.count()?;
        let removed = r.count()?;
        let tail = self.patch(r.pos(), added, removed, n - 1, &mut Vec::new())?;
        write_patch(
            &mut bytes,
            tail.entries.clone(),
            tail.gone.clone(),
            &tail.tooltip,
        );
        bytes.extend_from_slice(&tail.rest.bytes);
        Some(Slots {
            bytes,
            score: tail.score.clone(),
            end: tail.rest.end,
        })
    }

    /// The rest of a patch from `at` (`left` components, then `removed`
    /// ids), then `slots` more stacks to the end. `seen` holds the patch's
    /// component ids so far: a patch names each once, which rules out most
    /// misreads. It isn't part of the memo key, so a remembered state can
    /// rarely skip that check; keyed by it the memo hardly ever hits.
    fn patch(
        &mut self,
        at: usize,
        left: usize,
        removed: usize,
        slots: usize,
        seen: &mut Vec<i32>,
    ) -> Option<Rc<PatchTail>> {
        let key = (at, left, removed, slots);
        if let Some(done) = self.patch_memo.get(&key) {
            return done.clone();
        }
        self.step()?;
        let mark = seen.len();
        let found = self.patch_step(at, left, removed, slots, seen).map(Rc::new);
        seen.truncate(mark);
        self.patch_memo.insert(key, found.clone());
        found
    }

    fn patch_step(
        &mut self,
        at: usize,
        left: usize,
        removed: usize,
        slots: usize,
        seen: &mut Vec<i32>,
    ) -> Option<PatchTail> {
        let mut r = self.reader(at)?;
        if left == 0 {
            let mut gone = Vec::with_capacity(removed);
            for _ in 0..removed {
                let id = r.varint()?;
                if seen.contains(&id) {
                    return None;
                }
                seen.push(id);
                gone.extend(native(&component_name(id).ok()?));
            }
            let rest = self.slots(r.pos(), slots)?;
            let score = rest.score.clone();
            return Some(PatchTail {
                entries: Vec::new(),
                gone,
                tooltip: Tooltip::default(),
                rest,
                score,
            });
        }
        let id = r.varint()?;
        if seen.contains(&id) {
            return None;
        }
        seen.push(id);
        let name = component_name(id).ok()?;
        let start = r.pos();
        let mut body = Vec::new();
        let mut own = Tooltip::default();
        match component(self.ids, &name, &mut r, &mut body, &mut own) {
            Ok(kept) => {
                let next = self.patch(r.pos(), left - 1, removed, slots, seen)?;
                let mut entries = Vec::with_capacity(next.entries.len() + 1);
                entries.extend(kept.and_then(native).map(|id| (id, body)));
                entries.extend(next.entries.iter().cloned());
                let mut tooltip = next.tooltip.clone();
                tooltip.hide |= own.hide;
                tooltip.hidden.extend(own.hidden);
                Some(PatchTail {
                    entries,
                    gone: next.gone.clone(),
                    tooltip,
                    rest: next.rest.clone(),
                    score: next.score.clone(),
                })
            }
            // A mod's component: guess its shape, and leave it out.
            Err(Unreadable::Unknown) if r.pos() == start => {
                let mut best: Option<(Rc<PatchTail>, Shape)> = None;
                for shape in guesses(&name, self.buf.get(start).copied()) {
                    let mut r = self.reader(start)?;
                    if shape.skip(&mut r).is_none() {
                        continue;
                    }
                    let Some(next) = self.patch(r.pos(), left - 1, removed, slots, seen) else {
                        continue;
                    };
                    // The first shape (in `guesses` order) that reads to the
                    // end. Ranking every reading favours misreads that turn
                    // a mod's bytes into many small stacks.
                    best = Some((next, shape));
                    break;
                }
                let (next, shape) = best?;
                let mut score = next.score.clone();
                score.shapes.push((name, shape));
                Some(PatchTail {
                    entries: next.entries.clone(),
                    gone: next.gone.clone(),
                    tooltip: next.tooltip.clone(),
                    rest: next.rest.clone(),
                    score,
                })
            }
            Err(_) => None,
        }
    }
}

/// A component's name by the server's id: from its synced registry when it
/// sent one (a NeoForge server numbers components its own way), else
/// vanilla 1.21.1's order. An id the server doesn't have is a misaligned
/// read.
fn component_name(id: i32) -> Result<String, Unreadable> {
    use crate::protocol::registries::RegistryId;
    let id = u32::try_from(id).ok().need()?;
    match crate::protocol::wire_registry_name(RegistryId::DataComponentType, id) {
        Some(name) => name.need(),
        None => names().old.get(id as usize).cloned().need(),
    }
}

/// What `tooltip_display` should say, gathered over one patch.
#[derive(Default, Clone)]
struct Tooltip {
    hide: bool,
    hidden: Vec<&'static str>,
}

impl Tooltip {
    /// A 1.21.1 `show_in_tooltip` flag for `name`.
    fn shown(&mut self, r: &mut Reader, name: &'static str) -> Walk {
        if r.u8().need()? == 0 {
            self.hidden.push(name);
        }
        Ok(())
    }
}

fn patch(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    let added = r.count().need()?;
    let removed = r.count().need()?;
    let mut tooltip = Tooltip::default();
    let mut entries: Vec<(i32, Vec<u8>)> = Vec::with_capacity(added + 1);
    for _ in 0..added {
        let name = component_name(r.varint().need()?)?;
        let start = r.pos();
        let mut body = Vec::new();
        match component(ids, &name, r, &mut body, &mut tooltip) {
            Ok(Some(new_name)) => entries.extend(native(new_name).map(|id| (id, body))),
            Ok(None) => {}
            // A mod's component whose shape an earlier packet taught.
            // Unlearned, it stays unknown: the caller may search for it.
            Err(Unreadable::Unknown) if r.pos() == start => match learned(&name) {
                Some(shape) => shape.skip(r).need()?,
                None => return Err(Unreadable::Unknown),
            },
            Err(e) => return Err(e),
        }
    }
    let mut gone = Vec::with_capacity(removed);
    for _ in 0..removed {
        let name = component_name(r.varint().need()?)?;
        gone.extend(native(&name));
    }
    write_patch(out, entries, gone, &tooltip);
    Ok(())
}

/// Added entries (with a `tooltip_display` for any hidden flags), then the
/// removed ids.
fn write_patch(
    out: &mut Vec<u8>,
    mut entries: Vec<(i32, Vec<u8>)>,
    gone: Vec<i32>,
    tooltip: &Tooltip,
) {
    if (tooltip.hide || !tooltip.hidden.is_empty())
        && let Some(id) = native("minecraft:tooltip_display")
    {
        let mut body = vec![u8::from(tooltip.hide)];
        let hidden: Vec<i32> = tooltip.hidden.iter().filter_map(|n| native(n)).collect();
        write_varint(&mut body, hidden.len() as i32);
        for id in hidden {
            write_varint(&mut body, id);
        }
        entries.push((id, body));
    }
    write_varint(out, entries.len() as i32);
    write_varint(out, gone.len() as i32);
    for (id, body) in entries {
        write_varint(out, id);
        out.extend_from_slice(&body);
    }
    for id in gone {
        write_varint(out, id);
    }
}

/// An exact component predicate (a trade's cost): the components, each
/// rewritten as in a patch. A mod's component can't be carried.
pub(super) fn exact_components(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    let n = r.count().need()?;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let name = component_name(r.varint().need()?)?;
        let mut body = Vec::new();
        let mut tooltip = Tooltip::default();
        if let Some(id) = component(ids, &name, r, &mut body, &mut tooltip)?.and_then(native) {
            entries.push((id, body));
        }
    }
    write_varint(out, entries.len() as i32);
    for (id, body) in entries {
        write_varint(out, id);
        out.extend_from_slice(&body);
    }
    Ok(())
}

fn mob_effect(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    moved(ids, IdSpace::MobEffect, r, out)?;
    effect_details(r, out)
}

/// Amplifier, duration, three flags, then the hidden effect beneath it.
fn effect_details(r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    varint(r, out)?;
    varint(r, out)?;
    copy(r, out, 3)?;
    optional(r, out, effect_details)
}

fn block_predicate(ids: &Ids, r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    optional(r, out, |r, out| block_set(ids, r, out))?;
    optional(r, out, |r, out| {
        list(r, out, |r, out| {
            string(r, out)?;
            if flag(r, out)? {
                string(r, out)
            } else {
                optional(r, out, string)?;
                optional(r, out, string)
            }
        })
    })?;
    optional(r, out, nbt)?;
    // 1.21.5's component matchers: no exact components, no predicates.
    write_varint(out, 0);
    write_varint(out, 0);
    Ok(())
}

/// One component of 1.21.1's `name`, rewritten into `out`. The name it
/// goes out under, or `None` to leave it out.
fn component(
    ids: &Ids,
    name: &str,
    r: &mut Reader,
    out: &mut Vec<u8>,
    tooltip: &mut Tooltip,
) -> Result<Option<&'static str>, Unreadable> {
    let same = |n: &'static str| Ok(Some(n));
    match name {
        "minecraft:max_stack_size" => varint(r, out).and(same("minecraft:max_stack_size")),
        "minecraft:max_damage" => varint(r, out).and(same("minecraft:max_damage")),
        "minecraft:damage" => varint(r, out).and(same("minecraft:damage")),
        "minecraft:repair_cost" => varint(r, out).and(same("minecraft:repair_cost")),
        "minecraft:rarity" => varint(r, out).and(same("minecraft:rarity")),
        "minecraft:map_id" => varint(r, out).and(same("minecraft:map_id")),
        "minecraft:map_post_processing" => {
            varint(r, out).and(same("minecraft:map_post_processing"))
        }
        "minecraft:base_color" => varint(r, out).and(same("minecraft:base_color")),
        "minecraft:ominous_bottle_amplifier" => {
            varint(r, out).and(same("minecraft:ominous_bottle_amplifier"))
        }
        "minecraft:map_color" => copy(r, out, 4).and(same("minecraft:map_color")),
        "minecraft:enchantment_glint_override" => {
            copy(r, out, 1).and(same("minecraft:enchantment_glint_override"))
        }
        "minecraft:creative_slot_lock" => same("minecraft:creative_slot_lock"),
        "minecraft:custom_name" => nbt(r, out).and(same("minecraft:custom_name")),
        "minecraft:item_name" => nbt(r, out).and(same("minecraft:item_name")),
        "minecraft:lore" => list(r, out, nbt).and(same("minecraft:lore")),
        "minecraft:note_block_sound" => {
            hop::identifier(r, out).need()?;
            same("minecraft:note_block_sound")
        }
        // Persistent only: sent as NBT in both versions.
        "minecraft:custom_data" => nbt(r, out).and(same("minecraft:custom_data")),
        "minecraft:intangible_projectile" => {
            nbt(r, out).and(same("minecraft:intangible_projectile"))
        }
        "minecraft:map_decorations" => nbt(r, out).and(same("minecraft:map_decorations")),
        "minecraft:debug_stick_state" => nbt(r, out).and(same("minecraft:debug_stick_state")),
        "minecraft:recipes" => nbt(r, out).and(same("minecraft:recipes")),
        "minecraft:container_loot" => nbt(r, out).and(same("minecraft:container_loot")),
        // The lock became an item predicate.
        "minecraft:lock" => nbt(r, &mut Vec::new()).map(|()| None),

        "minecraft:unbreakable" => {
            tooltip.shown(r, "minecraft:unbreakable")?;
            same("minecraft:unbreakable")
        }
        "minecraft:enchantments" | "minecraft:stored_enchantments" => {
            let name = if name == "minecraft:enchantments" {
                "minecraft:enchantments"
            } else {
                "minecraft:stored_enchantments"
            };
            list(r, out, |r, out| {
                varint(r, out)?;
                varint(r, out)
            })?;
            tooltip.shown(r, name)?;
            same(name)
        }
        "minecraft:can_place_on" | "minecraft:can_break" => {
            let name = if name == "minecraft:can_place_on" {
                "minecraft:can_place_on"
            } else {
                "minecraft:can_break"
            };
            list(r, out, |r, out| block_predicate(ids, r, out))?;
            tooltip.shown(r, name)?;
            same(name)
        }
        "minecraft:attribute_modifiers" => {
            list(r, out, |r, out| {
                moved(ids, IdSpace::Attribute, r, out)?;
                hop::identifier(r, out).need()?;
                copy(r, out, 8)?;
                varint(r, out)?;
                varint(r, out)?;
                // 1.21.6's display: the default.
                write_varint(out, 0);
                Ok(())
            })?;
            tooltip.shown(r, "minecraft:attribute_modifiers")?;
            same("minecraft:attribute_modifiers")
        }
        "minecraft:dyed_color" => {
            copy(r, out, 4)?;
            tooltip.shown(r, "minecraft:dyed_color")?;
            same("minecraft:dyed_color")
        }
        "minecraft:trim" => {
            holder(r, out)?;
            holder(r, out)?;
            tooltip.shown(r, "minecraft:trim")?;
            same("minecraft:trim")
        }
        "minecraft:jukebox_playable" => {
            if flag(r, out)? {
                holder(r, out)?;
            } else {
                hop::identifier(r, out).need()?;
            }
            tooltip.shown(r, "minecraft:jukebox_playable")?;
            same("minecraft:jukebox_playable")
        }
        "minecraft:hide_tooltip" => {
            tooltip.hide = true;
            Ok(None)
        }
        "minecraft:hide_additional_tooltip" => Ok(None),
        "minecraft:fire_resistant" => {
            let tag = b"minecraft:is_fire";
            write_varint(out, tag.len() as i32);
            out.extend_from_slice(tag);
            same("minecraft:damage_resistant")
        }
        // An int to 1.21.4's floats, flags, strings and colours.
        "minecraft:custom_model_data" => {
            let value = r.varint().need()?;
            write_varint(out, 1);
            out.extend_from_slice(&(value as f32).to_be_bytes());
            for _ in 0..3 {
                write_varint(out, 0);
            }
            same("minecraft:custom_model_data")
        }
        // Eating time, leftovers and effects moved to `consumable` (1.21.2);
        // the item's defaults stand in for them.
        "minecraft:food" => {
            varint(r, out)?;
            copy(r, out, 4 + 1)?;
            let skip = &mut Vec::new();
            copy(r, skip, 4)?;
            optional(r, skip, |r, out| stack(ids, r, out))?;
            list(r, skip, |r, out| {
                mob_effect(ids, r, out)?;
                copy(r, out, 4)
            })?;
            same("minecraft:food")
        }
        "minecraft:tool" => {
            list(r, out, |r, out| {
                block_set(ids, r, out)?;
                optional(r, out, |r, out| copy(r, out, 4))?;
                optional(r, out, |r, out| copy(r, out, 1))
            })?;
            copy(r, out, 4)?;
            varint(r, out)?;
            // 1.21.5's "can destroy blocks in creative".
            out.push(1);
            same("minecraft:tool")
        }
        "minecraft:potion_contents" => {
            optional(r, out, |r, out| moved(ids, IdSpace::Potion, r, out))?;
            optional(r, out, |r, out| copy(r, out, 4))?;
            list(r, out, |r, out| mob_effect(ids, r, out))?;
            // 1.21.2's custom name: none.
            out.push(0);
            same("minecraft:potion_contents")
        }
        "minecraft:suspicious_stew_effects" => {
            list(r, out, |r, out| {
                moved(ids, IdSpace::MobEffect, r, out)?;
                varint(r, out)
            })?;
            same("minecraft:suspicious_stew_effects")
        }
        "minecraft:writable_book_content" => {
            list(r, out, |r, out| {
                string(r, out)?;
                optional(r, out, string)
            })?;
            same("minecraft:writable_book_content")
        }
        "minecraft:written_book_content" => {
            string(r, out)?;
            optional(r, out, string)?;
            string(r, out)?;
            varint(r, out)?;
            list(r, out, |r, out| {
                nbt(r, out)?;
                optional(r, out, nbt)
            })?;
            copy(r, out, 1)?;
            same("minecraft:written_book_content")
        }
        "minecraft:charged_projectiles" | "minecraft:bundle_contents" | "minecraft:container" => {
            let name = match name {
                "minecraft:charged_projectiles" => "minecraft:charged_projectiles",
                "minecraft:bundle_contents" => "minecraft:bundle_contents",
                _ => "minecraft:container",
            };
            list(r, out, |r, out| stack(ids, r, out))?;
            same(name)
        }
        "minecraft:lodestone_tracker" => {
            optional(r, out, |r, out| {
                hop::identifier(r, out).need()?;
                copy(r, out, 8)
            })?;
            copy(r, out, 1)?;
            same("minecraft:lodestone_tracker")
        }
        "minecraft:firework_explosion" => {
            firework_explosion(r, out)?;
            same("minecraft:firework_explosion")
        }
        "minecraft:fireworks" => {
            varint(r, out)?;
            list(r, out, firework_explosion)?;
            same("minecraft:fireworks")
        }
        // Name, id and properties became 1.21.9's partial profile (the
        // `either`'s right side), followed by an empty skin patch.
        "minecraft:profile" => {
            out.push(0);
            optional(r, out, string)?;
            optional(r, out, |r, out| copy(r, out, 16))?;
            list(r, out, |r, out| {
                string(r, out)?;
                string(r, out)?;
                optional(r, out, string)
            })?;
            out.extend_from_slice(&[0; 4]);
            same("minecraft:profile")
        }
        "minecraft:banner_patterns" => {
            list(r, out, |r, out| {
                holder(r, out)?;
                varint(r, out)
            })?;
            same("minecraft:banner_patterns")
        }
        "minecraft:pot_decorations" => {
            list(r, out, |r, out| moved(ids, IdSpace::Item, r, out))?;
            same("minecraft:pot_decorations")
        }
        "minecraft:block_state" => {
            list(r, out, |r, out| {
                string(r, out)?;
                string(r, out)
            })?;
            same("minecraft:block_state")
        }
        "minecraft:bucket_entity_data" => nbt(r, out).and(same("minecraft:bucket_entity_data")),
        // Entity and block entity data became typed (the type's id beside
        // the tag, 1.21.9), and the horn's instrument data-driven.
        "minecraft:entity_data" | "minecraft:block_entity_data" => {
            nbt(r, &mut Vec::new())?;
            Ok(None)
        }
        "minecraft:bees" => {
            list(r, &mut Vec::new(), |r, out| {
                nbt(r, out)?;
                varint(r, out)?;
                varint(r, out)
            })?;
            Ok(None)
        }
        "minecraft:instrument" => {
            holder(r, &mut Vec::new())?;
            Ok(None)
        }
        other => mod_component(ids, other, r).map(|()| None),
    }
}

/// Mods' components whose network shapes their sources give: read and left
/// out (this client has no such component). Guessing one of these misreads
/// the stacks after it. Any other mod's is `Unknown`, for the caller to
/// guess.
fn mod_component(ids: &Ids, name: &str, r: &mut Reader) -> Walk {
    let sink = &mut Vec::new();
    let (ns, path) = name.split_once(':').unwrap_or(("", name));
    match (ns, path) {
        // `ByteBufCodecs.INT` and `FLOAT`.
        (
            "sophisticatedcore",
            "number_of_inventory_slots"
            | "number_of_upgrade_slots"
            | "main_color"
            | "accent_color"
            | "open_tab_id"
            | "energy_stored"
            | "burn_time_total"
            | "cook_time_total"
            | "level"
            | "levels_to_store"
            | "levels_to_take"
            | "disc_slot_active"
            | "stored_xp",
        )
        | ("sophisticatedbackpacks", "columns_taken" | "loot_factor") => copy(r, sink, 4),
        // `ByteBufCodecs.BOOL`.
        (
            "sophisticatedcore",
            "linked_storage_primary_endpoint"
            | "shift_click_into_storage"
            | "refill_crafting_grid"
            | "refill_input"
            | "compact_non_uncraftable"
            | "should_work_in_gui"
            | "is_cooking"
            | "feed_immediately_when_hurt"
            | "is_playing"
            | "pickup_items"
            | "pickup_xp"
            | "is_input"
            | "interact_with_hand"
            | "interact_with_world"
            | "interact_with_fluid_handler"
            | "should_void_overflow"
            | "mend_items"
            | "enabled"
            | "shuffle"
            | "match_all_effects"
            | "match_effect_duration"
            | "match_effect_amplifier",
        )
        | (
            "sophisticatedbackpacks",
            "linked_mob_catcher" | "filter_by_inventory" | "should_swap_weapon",
        ) => copy(r, sink, 1),
        // `ByteBufCodecs.VAR_LONG`.
        (
            "sophisticatedcore",
            "linked_storage_render_revision"
            | "burn_time_finish"
            | "cook_time_finish"
            | "disc_finish_time",
        ) => r.varlong().map(drop).need(),
        // Enums (`NeoForgeStreamCodecs.enumCodec`, `Direction`) and `VAR_INT`.
        (
            "sophisticatedcore",
            "sort_by"
            | "void_type"
            | "automation_direction"
            | "repeat_mode"
            | "feed_at_hunger_level"
            | "direction",
        )
        | ("sophisticatedbackpacks", "inventory_order" | "tool_swap_mode")
        | ("createdieselgenerators", "oil_scanner_progress" | "oil_scanner_state") => {
            varint(r, sink)
        }
        ("sophisticatedcore", "storage_uuid") => copy(r, sink, 16),
        // `CustomData`: a tag.
        ("sophisticatedcore", "render_info_tag") => nbt(r, sink),
        // Identifiers and strings.
        ("sophisticatedcore", "recipe_id")
        | ("sophisticatedbackpacks", "loot_table" | "template_location" | "item_name")
        | ("createdieselgenerators", "mold_type") => string(r, sink),
        // `ItemContainerContents`: a list of stacks.
        ("sophisticatedcore", "cooking_inventory" | "lenient_container") => {
            list(r, sink, |r, out| stack(ids, r, out))
        }
        // `SimpleItemContent`: an optional stack.
        ("sophisticatedcore", "input_item" | "result_item") => stack(ids, r, sink),
        // `SimpleFluidContent`: an optional fluid stack.
        ("sophisticatedcore" | "createdieselgenerators", "fluid_contents") => fluid_stack(ids, r),
        ("sophisticatedcore", "fluid_filters") => list(r, sink, |r, _| fluid_stack(ids, r)),
        _ => Err(Unreadable::Unknown),
    }
}

/// NeoForge's `FluidStack.OPTIONAL_STREAM_CODEC`: an amount, then (when
/// above 0) the fluid's id and its component patch.
fn fluid_stack(ids: &Ids, r: &mut Reader) -> Walk {
    let amount = r.varint().need()?;
    if amount <= 0 {
        return Ok(());
    }
    r.varint().need()?;
    patch(ids, r, &mut Vec::new())
}

fn firework_explosion(r: &mut Reader, out: &mut Vec<u8>) -> Walk {
    varint(r, out)?;
    list(r, out, |r, out| copy(r, out, 4))?;
    list(r, out, |r, out| copy(r, out, 4))?;
    copy(r, out, 2)
}
