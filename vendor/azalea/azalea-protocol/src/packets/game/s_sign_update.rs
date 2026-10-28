use azalea_buf::AzBuf;
use azalea_core::position::BlockPos;
use azalea_protocol_macros::ServerboundGamePacket;

#[derive(AzBuf, Clone, Debug, PartialEq, ServerboundGamePacket)]
pub struct ServerboundSignUpdate {
    pub pos: BlockPos,
    pub lines: [String; 4],
    /// Vanilla's `SignTextSlot` (back = 0, front = 1), which is byte-identical
    /// to a bool.
    pub is_front_text: bool,
}
