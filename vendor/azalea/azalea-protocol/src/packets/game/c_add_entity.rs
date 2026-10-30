use std::io::{self, Cursor, Write};

use azalea_buf::{AzBuf, AzBufVar, BufReadError};
use azalea_core::{delta::LpVec3, entity_id::MinecraftEntityId, position::Vec3};
use azalea_protocol_macros::ClientboundGamePacket;
use azalea_registry::builtin::EntityKind;
#[cfg(feature = "bevy_ecs")]
use azalea_world::WorldName;
use uuid::Uuid;

#[derive(ClientboundGamePacket, Clone, Debug, PartialEq)]
pub struct ClientboundAddEntity {
    pub id: MinecraftEntityId,
    pub uuid: Uuid,
    /// The entity's type; [`EntityKind::Marker`] (no model, no physics) for
    /// a type from a mod this client doesn't have, whose id is then
    /// `modded_type`.
    pub entity_type: EntityKind,
    pub modded_type: Option<u32>,
    pub position: Vec3,
    pub movement: LpVec3,
    pub x_rot: i8,
    pub y_rot: i8,
    pub y_head_rot: i8,
    pub data: i32,
}

impl AzBuf for ClientboundAddEntity {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, BufReadError> {
        use azalea_registry::Registry;
        let id = MinecraftEntityId::azalea_read_var(buf)?;
        let uuid = Uuid::azalea_read(buf)?;
        let raw = u32::azalea_read_var(buf)?;
        let (entity_type, modded_type) = match EntityKind::from_u32(raw) {
            Some(kind) => (kind, None),
            None => (EntityKind::Marker, Some(raw)),
        };
        Ok(ClientboundAddEntity {
            id,
            uuid,
            entity_type,
            modded_type,
            position: Vec3::azalea_read(buf)?,
            movement: LpVec3::azalea_read(buf)?,
            x_rot: i8::azalea_read(buf)?,
            y_rot: i8::azalea_read(buf)?,
            y_head_rot: i8::azalea_read(buf)?,
            data: i32::azalea_read_var(buf)?,
        })
    }

    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        use azalea_registry::Registry;
        self.id.azalea_write_var(buf)?;
        self.uuid.azalea_write(buf)?;
        self.modded_type
            .unwrap_or_else(|| self.entity_type.to_u32())
            .azalea_write_var(buf)?;
        self.position.azalea_write(buf)?;
        self.movement.azalea_write(buf)?;
        self.x_rot.azalea_write(buf)?;
        self.y_rot.azalea_write(buf)?;
        self.y_head_rot.azalea_write(buf)?;
        self.data.azalea_write_var(buf)
    }
}

impl ClientboundAddEntity {
    #[cfg(feature = "bevy_ecs")]
    pub fn as_entity_bundle(&self, world_name: WorldName) -> azalea_entity::EntityBundle {
        azalea_entity::EntityBundle::new(self.uuid, self.position, self.entity_type, world_name)
    }

    #[cfg(feature = "bevy_ecs")]
    pub fn apply_metadata(&self, entity: &mut bevy_ecs::system::EntityCommands) {
        azalea_entity::metadata::apply_default_metadata(entity, self.entity_type);
    }
}

