use std::{
    fmt::{self, Debug},
    io::{self, Cursor, Write},
};

use azalea_buf::{AzBuf, AzBufVar, BufReadError};
use azalea_registry::builtin::BlockKind;

use crate::BlockTrait;

pub type BlockStateIntegerRepr = u16;

#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
pub struct BlockState {
    id: BlockStateIntegerRepr,
}

impl BlockState {
    pub const AIR: BlockState = BlockState { id: 0 };

    #[inline]
    pub(crate) const fn new_const(id: BlockStateIntegerRepr) -> Self {
        assert!(Self::is_valid_state(id));
        Self { id }
    }

    #[inline]
    pub const fn is_valid_state(state_id: BlockStateIntegerRepr) -> bool {
        state_id <= Self::MAX_STATE
    }

    #[inline]
    pub fn is_air(&self) -> bool {
        *self == Self::AIR
    }

    /// The state's network id. A state from a modded server can lie past
    /// [`BlockState::MAX_STATE`]; see [`BlockState::is_vanilla`].
    #[inline]
    pub const fn id(&self) -> BlockStateIntegerRepr {
        self.id
    }

    /// A state as a server sent it, kept even when it's past vanilla's
    /// states (a modded block); ids too large to store become the last one.
    #[inline]
    pub const fn from_network(id: u32) -> Self {
        let id = if id > BlockStateIntegerRepr::MAX as u32 {
            BlockStateIntegerRepr::MAX
        } else {
            id as BlockStateIntegerRepr
        };
        BlockState { id }
    }

    /// Whether this is one of vanilla's states. Others come from mods the
    /// client doesn't have: it can't tell which block they are, so the
    /// lookups treat them as a full solid cube.
    #[inline]
    pub const fn is_vanilla(&self) -> bool {
        Self::is_valid_state(self.id)
    }
}

impl TryFrom<u32> for BlockState {
    type Error = ();

    fn try_from(state_id: u32) -> Result<Self, Self::Error> {
        let state_id = state_id as BlockStateIntegerRepr;
        if Self::is_valid_state(state_id) {
            Ok(BlockState { id: state_id })
        } else {
            Err(())
        }
    }
}
impl TryFrom<i32> for BlockState {
    type Error = ();

    fn try_from(state_id: i32) -> Result<Self, Self::Error> {
        Self::try_from(state_id as u32)
    }
}

impl TryFrom<u16> for BlockState {
    type Error = ();

    fn try_from(id: u16) -> Result<Self, Self::Error> {
        let id = id as BlockStateIntegerRepr;
        if !Self::is_valid_state(id) {
            return Err(());
        }
        Ok(BlockState { id })
    }
}
impl From<BlockState> for u32 {
    fn from(value: BlockState) -> Self {
        value.id as u32
    }
}

impl AzBuf for BlockState {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, BufReadError> {
        Ok(Self::from_network(u32::azalea_read_var(buf)?))
    }
    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        u32::azalea_write_var(&(self.id as u32), buf)
    }
}

impl Debug for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.is_vanilla() {
            return write!(f, "BlockState(id: {}, modded)", self.id);
        }
        write!(
            f,
            "BlockState(id: {}, {:?})",
            self.id,
            Box::<dyn BlockTrait>::from(*self)
        )
    }
}

impl From<BlockState> for BlockKind {
    fn from(block_state: BlockState) -> Self {
        block_state.as_block_kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_u32() {
        assert_eq!(
            BlockState::try_from(0 as BlockStateIntegerRepr).unwrap(),
            BlockState::AIR
        );

        assert!(BlockState::try_from(BlockState::MAX_STATE).is_ok());
        assert!(BlockState::try_from(BlockState::MAX_STATE + 1).is_err());
    }

    #[test]
    fn a_modded_state_keeps_its_id_and_reads_as_stone() {
        let modded = BlockState::from_network(BlockState::MAX_STATE as u32 + 5);
        assert!(!modded.is_vanilla());
        assert_eq!(modded.id(), BlockState::MAX_STATE + 5);
        assert_eq!(modded.as_block_kind(), BlockKind::Stone);
        assert_eq!(Box::<dyn BlockTrait>::from(modded).id(), "stone");
        assert_eq!(
            BlockState::from_network(u32::MAX).id(),
            BlockStateIntegerRepr::MAX
        );
        assert!(BlockState::from_network(1).is_vanilla());
    }

    #[test]
    fn test_from_blockstate() {
        let block: Box<dyn BlockTrait> = Box::<dyn BlockTrait>::from(BlockState::AIR);
        assert_eq!(block.id(), "air");

        let block: Box<dyn BlockTrait> =
            Box::<dyn BlockTrait>::from(BlockState::from(BlockKind::FloweringAzalea));
        assert_eq!(block.id(), "flowering_azalea");
    }

    #[test]
    fn test_debug_blockstate() {
        let formatted = format!("{:?}", BlockState::from(BlockKind::FloweringAzalea));
        assert!(formatted.ends_with(", FloweringAzalea)"), "{}", formatted);

        let formatted = format!("{:?}", BlockState::from(BlockKind::BigDripleafStem));
        assert!(
            formatted.ends_with(", BigDripleafStem { facing: North, waterlogged: false })"),
            "{}",
            formatted
        );
    }
}
