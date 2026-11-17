//! What a right-click does to a block on the client, before the server
//! answers: vanilla's `useWithoutItem` / `useItemOn` where they change the
//! block on both sides. Levers, note blocks and daylight detectors change
//! only on the server, so they aren't here.

use azalea::block::fluid_state::{FluidKind, FluidState};
use azalea::block::{BlockState, BlockTrait};
use azalea_core::direction::Direction;
#[cfg(test)]
use azalea_registry::builtin::BlockKind;
use azalea_registry::builtin::ItemKind;

use crate::blocks::facing::name as direction_name;

/// What the player brings to the click.
#[derive(Clone, Copy, Debug)]
pub struct User {
    /// The main-hand item, `None` for an empty hand.
    pub held: Option<ItemKind>,
    /// The way the player faces (`getDirection`).
    pub facing: Direction,
    /// `Abilities.mayBuild`: off in adventure and spectator.
    pub may_build: bool,
    /// `Player.canEat(false)`: hungry, or invulnerable.
    pub can_eat: bool,
}

/// The block's state after the click, or `None` where it doesn't change
/// on the client. Doors take their other half from `door_other_half`.
pub fn used(state: BlockState, user: User) -> Option<BlockState> {
    let id = crate::util::block_model::block_id(state);
    let prop = |name: &str| crate::blocks::info::property(state, name);
    let mut block = Box::<dyn BlockTrait>::from(state);
    let mut set = |name: &str, value: &str| block.set_property(name, value).ok();

    if let Some(opened) = crate::play::placement::toggled_open(state) {
        return Some(opened);
    }
    if user.held == Some(ItemKind::FlintAndSteel) {
        return lit(state);
    }
    match id {
        // `FenceGateBlock.useWithoutItem`: opening turns it away from the
        // player if it faced them.
        _ if id.ends_with("fence_gate") => {
            if prop("open") == Some("true") {
                set("open", "false")?;
            } else {
                if prop("facing") == Some(direction_name(user.facing.opposite())) {
                    set("facing", direction_name(user.facing))?;
                }
                set("open", "true")?;
            }
        }
        // `ButtonBlock.press`; the server releases it.
        _ if id.ends_with("_button") => {
            if prop("powered") != Some("false") {
                return None;
            }
            set("powered", "true")?;
        }
        "repeater" if user.may_build => {
            let delay: u8 = prop("delay")?.parse().ok()?;
            set("delay", &(delay % 4 + 1).to_string())?;
        }
        "comparator" if user.may_build => {
            let next = if prop("mode") == Some("subtract") {
                "compare"
            } else {
                "subtract"
            };
            set("mode", next)?;
        }
        // `CakeBlock.eat`, unless a candle goes on it instead.
        "cake" => {
            let candle = user.held.is_some_and(|k| {
                k.to_str()
                    .trim_start_matches("minecraft:")
                    .ends_with("candle")
            });
            if candle || !user.can_eat {
                return None;
            }
            let bites: u8 = prop("bites")?.parse().ok()?;
            if bites >= 6 {
                return Some(BlockState::AIR);
            }
            set("bites", &(bites + 1).to_string())?;
        }
        // `AbstractCandleBlock.extinguish` by an empty hand.
        _ if (id.ends_with("candle") || id.ends_with("candle_cake"))
            && user.held.is_none()
            && user.may_build
            && prop("lit") == Some("true") =>
        {
            set("lit", "false")?;
        }
        _ => return None,
    }
    Some(block.as_block_state())
}

/// `FlintAndSteelItem.useOn` lighting a campfire, candle or candle cake
/// (`canLight`). Setting fire beside a block isn't predicted.
pub fn lit(state: BlockState) -> Option<BlockState> {
    let id = crate::util::block_model::block_id(state);
    let lightable =
        id.ends_with("campfire") || id.ends_with("candle") || id.ends_with("candle_cake");
    let prop = |name: &str| crate::blocks::info::property(state, name);
    if !lightable || prop("lit") != Some("false") || prop("waterlogged") == Some("true") {
        return None;
    }
    if FluidState::from(state).kind != FluidKind::Empty {
        return None;
    }
    let mut block = Box::<dyn BlockTrait>::from(state);
    block.set_property("lit", "true").ok()?;
    Some(block.as_block_state())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(held: Option<ItemKind>) -> User {
        User {
            held,
            facing: Direction::South,
            may_build: true,
            can_eat: true,
        }
    }

    fn with(kind: BlockKind, props: &[(&str, &str)]) -> BlockState {
        let mut b = Box::<dyn BlockTrait>::from(BlockState::from(kind));
        for (k, v) in props {
            b.set_property(k, v).unwrap();
        }
        b.as_block_state()
    }

    fn prop(state: BlockState, name: &str) -> Option<&'static str> {
        crate::blocks::info::property(state, name)
    }

    #[test]
    fn a_fence_gate_opens_away_from_the_player() {
        let gate = with(
            BlockKind::OakFenceGate,
            &[("facing", "north"), ("open", "false")],
        );
        let opened = used(gate, user(None)).unwrap();
        assert_eq!(prop(opened, "open"), Some("true"));
        assert_eq!(prop(opened, "facing"), Some("south"));
        let closed = used(opened, user(None)).unwrap();
        assert_eq!(prop(closed, "open"), Some("false"));
    }

    #[test]
    fn buttons_repeaters_and_comparators_change() {
        let button = with(BlockKind::StoneButton, &[("powered", "false")]);
        assert_eq!(
            prop(used(button, user(None)).unwrap(), "powered"),
            Some("true")
        );
        let pressed = with(BlockKind::StoneButton, &[("powered", "true")]);
        assert!(used(pressed, user(None)).is_none());

        let repeater = with(BlockKind::Repeater, &[("delay", "4")]);
        assert_eq!(
            prop(used(repeater, user(None)).unwrap(), "delay"),
            Some("1")
        );
        let comparator = with(BlockKind::Comparator, &[("mode", "compare")]);
        assert_eq!(
            prop(used(comparator, user(None)).unwrap(), "mode"),
            Some("subtract")
        );

        let adventure = User {
            may_build: false,
            ..user(None)
        };
        assert!(used(repeater, adventure).is_none());
    }

    #[test]
    fn a_cake_is_eaten_a_slice_at_a_time() {
        let cake = with(BlockKind::Cake, &[("bites", "5")]);
        assert_eq!(prop(used(cake, user(None)).unwrap(), "bites"), Some("6"));
        let last = with(BlockKind::Cake, &[("bites", "6")]);
        assert_eq!(used(last, user(None)), Some(BlockState::AIR));
        let full = User {
            can_eat: false,
            ..user(None)
        };
        assert!(used(cake, full).is_none());
        assert!(used(cake, user(Some(ItemKind::Candle))).is_none());
    }

    #[test]
    fn candles_go_out_by_hand_and_light_with_flint() {
        let lit_candle = with(BlockKind::Candle, &[("lit", "true")]);
        assert_eq!(
            prop(used(lit_candle, user(None)).unwrap(), "lit"),
            Some("false")
        );
        let campfire = with(BlockKind::Campfire, &[("lit", "false")]);
        let lighted = used(campfire, user(Some(ItemKind::FlintAndSteel))).unwrap();
        assert_eq!(prop(lighted, "lit"), Some("true"));
        assert!(used(BlockState::from(BlockKind::Lever), user(None)).is_none());
    }
}
