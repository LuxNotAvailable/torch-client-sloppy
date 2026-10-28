use std::io::{self, Cursor, Write};

use azalea_buf::{AzBuf, AzBufVar, BufReadError, BufReadErrorRepr};
use azalea_chat::translatable_component::TranslatableComponent;
use bevy_ecs::component::Component;
use tracing::debug;

#[cfg_attr(feature = "bevy_ecs", derive(Component))]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameMode {
    pub fn to_id(&self) -> u8 {
        match self {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
            GameMode::Adventure => 2,
            GameMode::Spectator => 3,
        }
    }

    pub fn to_optional_id<T: Into<Option<GameMode>>>(game_type: T) -> i8 {
        match game_type.into() {
            Some(game_type) => game_type.to_id() as i8,
            None => -1,
        }
    }

    pub fn from_id(id: u8) -> Option<GameMode> {
        Some(match id {
            0 => GameMode::Survival,
            1 => GameMode::Creative,
            2 => GameMode::Adventure,
            3 => GameMode::Spectator,
            _ => return None,
        })
    }

    pub fn from_optional_id(id: i8) -> Option<OptionalGameType> {
        Some(
            match id {
                -1 => None,
                id => Some(GameMode::from_id(id as u8)?),
            }
            .into(),
        )
    }

    pub fn short_name(&self) -> TranslatableComponent {
        TranslatableComponent::new(format!("selectWorld.gameMode.{}", self.name()), vec![])
    }

    pub fn long_name(&self) -> TranslatableComponent {
        TranslatableComponent::new(format!("gameMode.{}", self.name()), vec![])
    }

    pub fn from_name(name: &str) -> GameMode {
        match name {
            "survival" => GameMode::Survival,
            "creative" => GameMode::Creative,
            "adventure" => GameMode::Adventure,
            "spectator" => GameMode::Spectator,
            _ => panic!("Unknown game type name: {name}"),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
            GameMode::Adventure => "adventure",
            GameMode::Spectator => "spectator",
        }
    }
}

impl GameMode {
    pub fn is_block_placing_restricted(&self) -> bool {
        matches!(self, GameMode::Adventure | GameMode::Spectator)
    }
}

impl AzBuf for GameMode {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, BufReadError> {
        let id = u32::azalea_read_var(buf)?;
        let id = id.try_into().unwrap_or_else(|_| {
            debug!("Unknown game mode id {id}, defaulting to survival");
            0
        });
        Ok(GameMode::from_id(id).unwrap_or_else(|| {
            debug!("Unknown game mode id {id}, defaulting to survival");
            GameMode::Survival
        }))
    }
    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        u32::from(self.to_id()).azalea_write_var(buf)
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq)]
pub struct OptionalGameType(pub Option<GameMode>);

impl From<Option<GameMode>> for OptionalGameType {
    fn from(game_type: Option<GameMode>) -> Self {
        OptionalGameType(game_type)
    }
}

impl From<OptionalGameType> for Option<GameMode> {
    fn from(optional_game_type: OptionalGameType) -> Self {
        optional_game_type.0
    }
}

/// Encoded as a VarInt where 0 means "none" and anything else is the game
/// mode id plus one.
impl AzBuf for OptionalGameType {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, BufReadError> {
        let id = u32::azalea_read_var(buf)?;
        let Some(id) = id.checked_sub(1) else {
            return Ok(OptionalGameType(None));
        };
        u8::try_from(id)
            .ok()
            .and_then(GameMode::from_id)
            .map(|mode| OptionalGameType(Some(mode)))
            .ok_or(BufReadErrorRepr::UnexpectedEnumVariant { id: id as i32 }.into())
    }
    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        self.0
            .map_or(0, |mode| u32::from(mode.to_id()) + 1)
            .azalea_write_var(buf)
    }
}
