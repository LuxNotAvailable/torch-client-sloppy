//! Inventory sorting and bulk transfers, planned as ordinary clicks.
//!
//! Each request becomes the clicks a player could make by hand: pickups to
//! merge and reorder stacks, shift-clicks to move them across. The plan runs
//! against a copy of the inventory as it goes, with the same simulation that
//! later sends each click, so every click is chosen from the state the
//! server will see. Nothing happens while the cursor holds an item.
//!
//! A started request runs a few clicks a tick (`tick`), planning afresh
//! whenever the cursor is empty, so a server correction mid-way is planned
//! around. The GUI ignores clicks while one runs (`busy`).

use std::cmp::Reverse;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use azalea::entity::PlayerAbilities;
use azalea::entity::inventory::Inventory;
use azalea_inventory::item::MaxStackSizeExt;
use azalea_inventory::operations::{ClickOperation, PickupClick, QuickMoveClick};
use azalea_inventory::{ItemStack, ItemStackData, Menu};
use azalea_registry::Registry as _;

use crate::session::{SortRequest, SortSide};

/// Clicks sent per tick, about what a quick hand manages.
const CLICKS_PER_TICK: usize = 6;
/// Clicks after which a request gives up, should the server keep undoing it.
const MAX_CLICKS: usize = 1500;

struct Job {
    request: SortRequest,
    window: i32,
    /// The rest of the transaction under way (until the cursor empties).
    pending: VecDeque<ClickOperation>,
    sent: usize,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

/// Whether a sort or transfer is running; the GUI takes no clicks then.
pub(crate) fn busy() -> bool {
    BUSY.load(Ordering::Relaxed)
}

/// Starts `request` in menu `window`, replacing any running one.
pub(crate) fn start(request: SortRequest, window: i32) {
    *JOB.lock().unwrap() = Some(Job {
        request,
        window,
        pending: VecDeque::new(),
        sent: 0,
    });
    BUSY.store(true, Ordering::Relaxed);
}

pub(crate) fn cancel() {
    *JOB.lock().unwrap() = None;
    BUSY.store(false, Ordering::Relaxed);
}

/// The clicks to send this tick; empty once the job is done, which ends it.
/// `inventory` is the live menu, as each sent click leaves it.
pub(crate) fn next_click(
    inventory: &Inventory,
    abilities: &PlayerAbilities,
    sent_this_tick: usize,
) -> Option<ClickOperation> {
    let mut guard = JOB.lock().unwrap();
    let job = guard.as_mut()?;
    let done = |guard: &mut Option<Job>| {
        *guard = None;
        BUSY.store(false, Ordering::Relaxed);
        None
    };
    if inventory.id != job.window || job.sent >= MAX_CLICKS {
        return done(&mut guard);
    }
    if sent_this_tick >= CLICKS_PER_TICK {
        return None;
    }
    if job.pending.is_empty() {
        if inventory.carried.is_present() {
            // Not ours: the server put something on the cursor.
            return done(&mut guard);
        }
        job.pending = first_transaction(
            inventory,
            abilities,
            plan(inventory, abilities, job.request),
        );
    }
    match job.pending.pop_front() {
        Some(op) => {
            job.sent += 1;
            Some(op)
        }
        None => done(&mut guard),
    }
}

/// The leading clicks of `ops` up to the first that leaves the cursor empty.
fn first_transaction(
    inventory: &Inventory,
    abilities: &PlayerAbilities,
    ops: Vec<ClickOperation>,
) -> VecDeque<ClickOperation> {
    let mut sim = inventory.clone();
    let mut out = VecDeque::new();
    for op in ops {
        sim.simulate_click(&op, abilities);
        out.push_back(op);
        if sim.carried.is_empty() {
            break;
        }
    }
    out
}

/// The storage slots of a menu that has them: chests, barrels, ender chests,
/// shulker boxes, dispensers and hoppers.
fn container_slots(menu: &Menu) -> Option<Range<usize>> {
    match menu {
        Menu::Generic9x1 { .. }
        | Menu::Generic9x2 { .. }
        | Menu::Generic9x3 { .. }
        | Menu::Generic9x4 { .. }
        | Menu::Generic9x5 { .. }
        | Menu::Generic9x6 { .. }
        | Menu::Generic3x3 { .. }
        | Menu::ShulkerBox { .. }
        | Menu::Hopper { .. } => Some(0..*menu.player_slots_range().start()),
        _ => None,
    }
}

static INCLUDE_HOTBAR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the player's side takes in the hotbar (the Include Hotbar
/// setting), as the GUI last set it.
pub(crate) fn set_include_hotbar(include: bool) {
    INCLUDE_HOTBAR.store(include, std::sync::atomic::Ordering::Relaxed);
}

/// The player's side: the main inventory, and the hotbar when the setting
/// takes it in. Left out, sorting and storing leave the hotbar alone.
fn main_slots(menu: &Menu) -> Range<usize> {
    if INCLUDE_HOTBAR.load(std::sync::atomic::Ordering::Relaxed) {
        return player_slots(menu);
    }
    let r = menu.player_slots_without_hotbar_range();
    *r.start()..*r.end() + 1
}

/// Everything the player carries in this menu, hotbar included.
fn player_slots(menu: &Menu) -> Range<usize> {
    let r = menu.player_slots_range();
    *r.start()..*r.end() + 1
}

fn is_bundle(stack: &ItemStackData) -> bool {
    stack
        .get_component::<azalea_inventory::components::BundleContents>()
        .is_some()
}

fn max_stack(stack: &ItemStackData) -> i32 {
    stack
        .get_component::<azalea_inventory::components::MaxStackSize>()
        .map_or_else(|| stack.kind.max_stack_size(), |m| m.count)
}

/// The clicks that carry out `request` on `inventory`; none when the
/// request does not apply to the open menu or the cursor holds an item.
pub(crate) fn plan(
    inventory: &Inventory,
    abilities: &PlayerAbilities,
    request: SortRequest,
) -> Vec<ClickOperation> {
    if inventory.carried.is_present() {
        return Vec::new();
    }
    let mut planner = Planner {
        sim: inventory.clone(),
        abilities,
        ops: Vec::new(),
    };
    let menu = inventory.menu();
    let side = |side: SortSide| match side {
        SortSide::Container => container_slots(menu),
        SortSide::Player => Some(main_slots(menu)),
    };
    match request {
        SortRequest::Sort(which) => {
            if let Some(slots) = side(which) {
                // A pickup click onto or with a bundle fills the bundle
                // instead of swapping, so bundles stay where they are.
                let slots: Vec<usize> = slots
                    .filter(|&i| !planner.present(i).is_some_and(is_bundle))
                    .collect();
                planner.sort(&slots);
            }
        }
        SortRequest::MoveMatching { from } => {
            let (Some(source), Some(_)) = (side(from), container_slots(menu)) else {
                return Vec::new();
            };
            let other = match from {
                SortSide::Container => player_slots(menu),
                SortSide::Player => container_slots(menu).unwrap_or(0..0),
            };
            let kinds: Vec<_> = other
                .filter_map(|i| menu.slot(i).and_then(ItemStack::as_present))
                .map(|s| s.kind)
                .collect();
            for i in source {
                if planner.present(i).is_some_and(|s| kinds.contains(&s.kind)) {
                    planner.shift(i);
                }
            }
        }
        SortRequest::MoveAll { from } => {
            let (Some(source), Some(_)) = (side(from), container_slots(menu)) else {
                return Vec::new();
            };
            for i in source {
                if planner.present(i).is_some() {
                    planner.shift(i);
                }
            }
        }
    }
    planner.ops
}

/// The clicks that move `sets` of a recipe's ingredients into the open
/// menu: each slot in `clear` shift-clicked out, then for each target slot
/// stacks of any of its names picked up from the player's slots. A stack
/// that fits whole goes down with a left click; the last one is placed an
/// item at a time with right clicks and the rest put back. None if the
/// cursor holds an item, a slot will not empty, or an ingredient runs out,
/// so nothing half happens.
pub(crate) fn fill_recipe(
    inventory: &Inventory,
    abilities: &PlayerAbilities,
    clear: &[u16],
    targets: &[(u16, Vec<&'static str>)],
    sets: u8,
) -> Vec<ClickOperation> {
    if inventory.carried.is_present() {
        return Vec::new();
    }
    let mut planner = Planner {
        sim: inventory.clone(),
        abilities,
        ops: Vec::new(),
    };
    let player = player_slots(inventory.menu());
    for &slot in clear {
        let slot = slot as usize;
        if planner.present(slot).is_some() {
            planner.shift(slot);
            if planner.present(slot).is_some() {
                return Vec::new();
            }
        }
    }
    for (slot, names) in targets {
        let matches = |s: &ItemStackData| {
            let name = s.kind.to_str();
            let name = name.trim_start_matches("minecraft:");
            names.iter().any(|n| *n == name)
        };
        let mut need = sets.max(1) as i32;
        while need > 0 {
            let Some(source) = player
                .clone()
                .find(|&i| planner.present(i).is_some_and(matches))
            else {
                return Vec::new();
            };
            planner.left(source);
            let carried = planner.sim.carried.count();
            if carried <= need {
                planner.left(*slot as usize);
                if planner.sim.carried.is_present() {
                    // The slot would not take it all.
                    planner.left(source);
                    return Vec::new();
                }
                need -= carried;
            } else {
                for _ in 0..need {
                    planner.click(PickupClick::Right { slot: Some(*slot) }.into());
                }
                planner.left(source);
                need = 0;
            }
        }
    }
    planner.ops
}

struct Planner<'a> {
    sim: Inventory,
    abilities: &'a PlayerAbilities,
    ops: Vec<ClickOperation>,
}

impl Planner<'_> {
    fn click(&mut self, op: ClickOperation) {
        self.sim.simulate_click(&op, self.abilities);
        self.ops.push(op);
    }

    fn left(&mut self, slot: usize) {
        self.click(
            PickupClick::Left {
                slot: Some(slot as u16),
            }
            .into(),
        );
    }

    /// A shift-click, left out when it would move nothing (a full other
    /// side), so a fresh plan doesn't repeat it forever.
    fn shift(&mut self, slot: usize) {
        let before = self.sim.menu().slots();
        self.click(QuickMoveClick::Left { slot: slot as u16 }.into());
        if self.sim.menu().slots() == before {
            self.ops.pop();
        }
    }

    fn present(&self, slot: usize) -> Option<&ItemStackData> {
        self.sim.menu().slot(slot).and_then(ItemStack::as_present)
    }

    fn holds(&self, slot: usize, target: Option<&ItemStackData>) -> bool {
        match (self.present(slot), target) {
            (None, None) => true,
            (Some(a), Some(b)) => a.is_same_item_and_components(b),
            _ => false,
        }
    }

    /// Merges, then orders, then puts each kind's part stack last.
    fn sort(&mut self, slots: &[usize]) {
        self.merge(slots);
        let targets = self.targets(slots);
        self.arrange(slots, &targets);
        self.part_stacks_last(slots, &targets);
    }

    /// Tops up part stacks of the same item from one another until each item
    /// has at most one part stack. Each round leaves one fewer.
    fn merge(&mut self, slots: &[usize]) {
        let partial = |p: &Self, i: usize| p.present(i).filter(|s| s.count < max_stack(s)).cloned();
        for _ in 0..slots.len() * 2 {
            let pair = slots.iter().enumerate().find_map(|(n, &keep)| {
                let a = partial(self, keep)?;
                slots[n + 1..]
                    .iter()
                    .find(|&&take| {
                        partial(self, take).is_some_and(|b| a.is_same_item_and_components(&b))
                    })
                    .map(|&take| (keep, take))
            });
            let Some((keep, take)) = pair else { return };
            self.left(take);
            self.left(keep);
            if self.sim.carried.is_present() {
                self.left(take);
            }
        }
    }

    /// Where every stack goes: by item, then by components (an order that
    /// doesn't depend on where stacks sit, so plans made mid-sort agree),
    /// full stacks before the part stack, then empty slots.
    fn targets(&self, slots: &[usize]) -> Vec<Option<ItemStackData>> {
        let mut stacks: Vec<(u32, String, Reverse<i32>, ItemStackData)> = Vec::new();
        for &i in slots {
            let Some(stack) = self.present(i) else {
                continue;
            };
            stacks.push((
                stack.kind.to_u32(),
                format!("{:?}", stack.component_patch),
                Reverse(stack.count),
                stack.clone(),
            ));
        }
        stacks.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
        let mut targets: Vec<Option<ItemStackData>> =
            stacks.into_iter().map(|(.., s)| Some(s)).collect();
        targets.resize(slots.len(), None);
        targets
    }

    /// Cycles stacks into place: pick one up, put it down where its kind
    /// belongs, carry what was there on, until a stack lands in an empty
    /// slot. A stack is only ever dropped on a different kind or nothing, so
    /// every click swaps or places and puts one more slot right.
    fn arrange(&mut self, slots: &[usize], targets: &[Option<ItemStackData>]) {
        for start in 0..slots.len() {
            if self.holds(slots[start], targets[start].as_ref())
                || self.present(slots[start]).is_none()
            {
                continue;
            }
            self.left(slots[start]);
            for _ in 0..=slots.len() {
                let Some(carried) = self.sim.carried.as_present().cloned() else {
                    break;
                };
                let dest = (0..slots.len()).find(|&j| {
                    targets[j]
                        .as_ref()
                        .is_some_and(|t| t.is_same_item_and_components(&carried))
                        && !self.holds(slots[j], targets[j].as_ref())
                });
                match dest {
                    Some(j) => self.left(slots[j]),
                    None => break,
                }
            }
            if self.sim.carried.is_present() {
                // Not expected; put it down rather than leave it held.
                if let Some(&empty) = slots.iter().find(|&&i| self.present(i).is_none()) {
                    self.left(empty);
                }
                return;
            }
        }
    }

    /// After `arrange`, a kind's part stack may sit before its full ones;
    /// topping it up from the last full one moves the remainder there.
    fn part_stacks_last(&mut self, slots: &[usize], targets: &[Option<ItemStackData>]) {
        let mut seen: Vec<&ItemStackData> = Vec::new();
        for target in targets.iter().flatten() {
            if seen.iter().any(|s| s.is_same_item_and_components(target)) {
                continue;
            }
            seen.push(target);
            let group: Vec<usize> = (0..slots.len())
                .filter(|&j| {
                    targets[j]
                        .as_ref()
                        .is_some_and(|t| t.is_same_item_and_components(target))
                })
                .map(|j| slots[j])
                .collect();
            let Some((&last, rest)) = group.split_last() else {
                continue;
            };
            let part = rest
                .iter()
                .copied()
                .find(|&i| self.present(i).is_some_and(|s| s.count < max_stack(s)));
            if let (Some(part), Some(full)) = (part, self.present(last).cloned())
                && full.count == max_stack(&full)
            {
                self.left(last);
                self.left(part);
                self.left(last);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use azalea_registry::builtin::{ItemKind, MenuKind};

    fn chest(contents: &[(usize, ItemKind, i32)]) -> Inventory {
        let mut menu = Menu::from_kind(MenuKind::Generic9x3);
        for &(slot, kind, count) in contents {
            *menu.slot_mut(slot).unwrap() = ItemStack::new(kind, count);
        }
        Inventory {
            id: 1,
            container_menu: Some(menu),
            ..Inventory::default()
        }
    }

    fn run(inventory: &mut Inventory, request: SortRequest) -> usize {
        let abilities = PlayerAbilities::default();
        let ops = plan(inventory, &abilities, request);
        for op in &ops {
            inventory.simulate_click(op, &abilities);
        }
        assert!(inventory.carried.is_empty(), "the cursor ends empty");
        ops.len()
    }

    fn contents(inventory: &Inventory, slots: Range<usize>) -> Vec<(ItemKind, i32)> {
        slots
            .filter_map(|i| inventory.menu().slot(i).unwrap().as_present())
            .map(|s| (s.kind, s.count))
            .collect()
    }

    fn total(inventory: &Inventory, kind: ItemKind) -> i32 {
        inventory
            .menu()
            .slots()
            .iter()
            .filter_map(ItemStack::as_present)
            .filter(|s| s.kind == kind)
            .map(|s| s.count)
            .sum()
    }

    #[test]
    fn sorting_merges_orders_and_packs_to_the_front() {
        let mut inv = chest(&[
            (3, ItemKind::Dirt, 10),
            (5, ItemKind::Stone, 64),
            (8, ItemKind::Dirt, 60),
            (12, ItemKind::Stone, 7),
            (20, ItemKind::Dirt, 5),
            (26, ItemKind::DiamondSword, 1),
        ]);
        run(&mut inv, SortRequest::Sort(SortSide::Container));
        // The dirt merges into one full stack and the 11 left over.
        let mut expected = vec![
            (ItemKind::Stone, 64),
            (ItemKind::Stone, 7),
            (ItemKind::Dirt, 64),
            (ItemKind::Dirt, 11),
            (ItemKind::DiamondSword, 1),
        ];
        expected.sort_by_key(|&(k, c)| (k.to_u32(), Reverse(c)));
        let got = contents(&inv, 0..27);
        assert_eq!(got, expected);
        // Packed to the front: the five stacks fill slots 0 to 4.
        assert!((0..5).all(|i| inv.menu().slot(i).unwrap().is_present()));
        assert!((5..27).all(|i| inv.menu().slot(i).unwrap().is_empty()));
    }

    #[test]
    fn a_sorted_chest_needs_no_clicks() {
        let mut inv = chest(&[(0, ItemKind::Stone, 64), (1, ItemKind::Stone, 3)]);
        run(&mut inv, SortRequest::Sort(SortSide::Container));
        assert_eq!(run(&mut inv, SortRequest::Sort(SortSide::Container)), 0);
    }

    #[test]
    fn a_part_stack_ahead_of_a_full_one_moves_behind_it() {
        let mut inv = chest(&[(0, ItemKind::Stone, 5), (1, ItemKind::Stone, 64)]);
        run(&mut inv, SortRequest::Sort(SortSide::Container));
        assert_eq!(
            contents(&inv, 0..27),
            vec![(ItemKind::Stone, 64), (ItemKind::Stone, 5)]
        );
    }

    #[test]
    fn sorting_the_player_leaves_the_chest_and_hotbar_alone() {
        // Generic9x3: the player's main inventory is 27..54, the hotbar 54..63.
        let mut inv = chest(&[
            (4, ItemKind::Dirt, 1),
            (40, ItemKind::Stone, 2),
            (30, ItemKind::Dirt, 3),
            (55, ItemKind::Torch, 9),
        ]);
        run(&mut inv, SortRequest::Sort(SortSide::Player));
        assert_eq!(contents(&inv, 0..27), vec![(ItemKind::Dirt, 1)]);
        assert_eq!(inv.menu().slot(55).unwrap().count(), 9);
        assert!(inv.menu().slot(27).unwrap().is_present());
        assert!(inv.menu().slot(28).unwrap().is_present());
    }

    #[test]
    fn storing_matching_moves_only_what_the_chest_holds() {
        let mut inv = chest(&[
            (0, ItemKind::Stone, 10),
            (30, ItemKind::Stone, 20),
            (31, ItemKind::Dirt, 20),
        ]);
        run(
            &mut inv,
            SortRequest::MoveMatching {
                from: SortSide::Player,
            },
        );
        assert_eq!(contents(&inv, 0..27), vec![(ItemKind::Stone, 30)]);
        assert_eq!(total(&inv, ItemKind::Dirt), 20);
        assert_eq!(contents(&inv, 27..63), vec![(ItemKind::Dirt, 20)]);
    }

    #[test]
    fn taking_everything_stops_when_the_inventory_is_full() {
        let mut stuff: Vec<_> = (27..63).map(|i| (i, ItemKind::Dirt, 64)).collect();
        stuff.push((0, ItemKind::Stone, 5));
        let mut inv = chest(&stuff);
        run(
            &mut inv,
            SortRequest::MoveAll {
                from: SortSide::Container,
            },
        );
        assert_eq!(contents(&inv, 0..27), vec![(ItemKind::Stone, 5)]);
        assert_eq!(total(&inv, ItemKind::Dirt), 36 * 64);
    }

    #[test]
    fn random_chests_sort_without_losing_anything() {
        let kinds = [
            ItemKind::Stone,
            ItemKind::Dirt,
            ItemKind::Torch,
            ItemKind::EnderPearl,
            ItemKind::DiamondSword,
            ItemKind::Cobblestone,
        ];
        let mut rng = crate::util::javarandom::JavaRandom::new(77);
        for _ in 0..300 {
            let mut stuff = Vec::new();
            for slot in 0..27 {
                if rng.next_int(3) == 0 {
                    continue;
                }
                let kind = kinds[rng.next_int(kinds.len() as u32) as usize];
                let max = kind.max_stack_size();
                stuff.push((slot, kind, 1 + rng.next_int(max as u32) as i32));
            }
            let mut inv = chest(&stuff);
            let before: Vec<i32> = kinds.iter().map(|&k| total(&inv, k)).collect();
            run(&mut inv, SortRequest::Sort(SortSide::Container));
            let after: Vec<i32> = kinds.iter().map(|&k| total(&inv, k)).collect();
            assert_eq!(before, after, "nothing lost or made");

            // In order, packed to the front, one part stack per item, last.
            let got = contents(&inv, 0..27);
            let mut sorted = got.clone();
            sorted.sort_by_key(|&(k, c)| (k.to_u32(), Reverse(c)));
            assert_eq!(got, sorted);
            assert!((got.len()..27).all(|i| inv.menu().slot(i).unwrap().is_empty()));
            for &kind in &kinds {
                let parts = got
                    .iter()
                    .filter(|&&(k, c)| k == kind && c < kind.max_stack_size())
                    .count();
                assert!(parts <= 1, "{kind:?} left {parts} part stacks");
            }
            assert_eq!(run(&mut inv, SortRequest::Sort(SortSide::Container)), 0);
        }
    }

    /// Runs `request` as the tick runner does, a transaction at a time from
    /// a fresh plan; the clicks it took.
    fn run_paced(inventory: &mut Inventory, request: SortRequest) -> usize {
        let abilities = PlayerAbilities::default();
        let mut clicks = 0;
        loop {
            let ops =
                first_transaction(inventory, &abilities, plan(inventory, &abilities, request));
            if ops.is_empty() {
                break;
            }
            for op in &ops {
                inventory.simulate_click(op, &abilities);
            }
            clicks += ops.len();
            assert!(clicks < MAX_CLICKS, "the plan never settles");
        }
        assert!(inventory.carried.is_empty(), "the cursor ends empty");
        clicks
    }

    #[test]
    fn paced_sorts_settle_where_a_whole_plan_does() {
        let kinds = [
            ItemKind::Stone,
            ItemKind::Dirt,
            ItemKind::Torch,
            ItemKind::EnderPearl,
            ItemKind::DiamondSword,
        ];
        let mut rng = crate::util::javarandom::JavaRandom::new(5);
        for _ in 0..200 {
            let mut stuff = Vec::new();
            for slot in 0..27 {
                if rng.next_int(3) == 0 {
                    continue;
                }
                let kind = kinds[rng.next_int(kinds.len() as u32) as usize];
                stuff.push((
                    slot,
                    kind,
                    1 + rng.next_int(kind.max_stack_size() as u32) as i32,
                ));
            }
            let mut whole = chest(&stuff);
            let mut paced = chest(&stuff);
            run(&mut whole, SortRequest::Sort(SortSide::Container));
            run_paced(&mut paced, SortRequest::Sort(SortSide::Container));
            assert_eq!(contents(&whole, 0..27), contents(&paced, 0..27));
        }
    }

    #[test]
    fn paced_transfers_stop_when_nothing_moves() {
        let mut stuff: Vec<_> = (27..63).map(|i| (i, ItemKind::Dirt, 64)).collect();
        stuff.push((0, ItemKind::Stone, 5));
        let mut inv = chest(&stuff);
        let request = SortRequest::MoveAll {
            from: SortSide::Container,
        };
        assert_eq!(run_paced(&mut inv, request), 0);
    }

    #[test]
    fn bundles_stay_put_and_the_rest_sorts_around_them() {
        let mut inv = chest(&[
            (0, ItemKind::Stone, 3),
            (1, ItemKind::Bundle, 1),
            (2, ItemKind::Dirt, 4),
            (3, ItemKind::Stone, 2),
        ]);
        assert!(
            inv.menu()
                .slot(1)
                .unwrap()
                .as_present()
                .is_some_and(is_bundle)
        );
        run(&mut inv, SortRequest::Sort(SortSide::Container));
        assert_eq!(inv.menu().slot(1).unwrap().kind(), ItemKind::Bundle);
        assert_eq!(total(&inv, ItemKind::Stone), 5);
        assert_eq!(total(&inv, ItemKind::Dirt), 4);
    }

    #[test]
    fn nothing_happens_while_an_item_is_held() {
        let mut inv = chest(&[(3, ItemKind::Dirt, 10)]);
        inv.carried = ItemStack::new(ItemKind::Stone, 1);
        assert!(
            plan(
                &inv,
                &PlayerAbilities::default(),
                SortRequest::Sort(SortSide::Container)
            )
            .is_empty()
        );
    }
}
