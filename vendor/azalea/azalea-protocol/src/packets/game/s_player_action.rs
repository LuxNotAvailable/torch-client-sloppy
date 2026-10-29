use azalea_buf::AzBuf;
use azalea_core::{direction::Direction, position::BlockPos};
use azalea_protocol_macros::ServerboundGamePacket;

#[derive(AzBuf, Clone, Debug, PartialEq, ServerboundGamePacket)]
pub struct ServerboundPlayerAction {
    pub action: Action,
    pub pos: BlockPos,
    pub direction: Direction,
    #[var]
    pub seq: u32,
}

#[derive(AzBuf, Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    StartDestroyBlock = 0,
    ChangeDestroyDirection = 1,
    AbortDestroyBlock = 2,
    StopDestroyBlock = 3,
    DropAllItems = 4,
    DropItem = 5,
    ReleaseUseItem = 6,
    SwapItemWithOffhand = 7,
    Stab = 8,
}
