//! Tests that a modded server's ids (blocks, entities and items this client
//! doesn't have) decode instead of failing their packet. The decoding lives
//! in the vendored azalea crates, whose own tests can't run from this
//! workspace.

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use azalea::block::BlockState;
    use azalea_buf::{AzBuf, AzBufVar};
    use azalea_core::delta::LpVec3;
    use azalea_core::entity_id::MinecraftEntityId;
    use azalea_core::position::Vec3;
    use azalea_protocol::packets::game::ClientboundAddEntity;
    use azalea_registry::builtin::EntityKind;
    use azalea_world::palette::PaletteKind;

    #[test]
    fn a_modded_state_in_a_palette_is_kept_not_made_air() {
        let modded = BlockState::MAX_STATE as u32 + 7;
        let mut bytes = Vec::new();
        vec![1u32, modded].azalea_write_var(&mut bytes).unwrap();
        let palette = PaletteKind::Linear
            .read::<BlockState>(&mut Cursor::new(&bytes[..]))
            .unwrap();
        assert_eq!(palette.value_for(0), BlockState::try_from(1u32).unwrap());
        assert_eq!(palette.value_for(1).id() as u32, modded);
        assert!(!palette.value_for(1).is_vanilla());
    }

    fn add_entity(entity_type: EntityKind, modded_type: Option<u32>) -> ClientboundAddEntity {
        ClientboundAddEntity {
            id: MinecraftEntityId(5),
            uuid: uuid::Uuid::from_u128(7),
            entity_type,
            modded_type,
            position: Vec3::ZERO,
            movement: LpVec3::default(),
            x_rot: 1,
            y_rot: 2,
            y_head_rot: 3,
            data: 4,
        }
    }

    fn round_trip(packet: &ClientboundAddEntity) -> ClientboundAddEntity {
        let mut bytes = Vec::new();
        packet.azalea_write(&mut bytes).unwrap();
        ClientboundAddEntity::azalea_read(&mut Cursor::new(&bytes[..])).unwrap()
    }

    #[test]
    fn a_known_entity_type_round_trips() {
        let packet = add_entity(EntityKind::Cow, None);
        assert_eq!(round_trip(&packet), packet);
    }

    #[test]
    fn a_modded_entity_type_becomes_a_marker_and_keeps_its_id() {
        let packet = add_entity(EntityKind::Marker, Some(60_000));
        let back = round_trip(&packet);
        assert_eq!(back.entity_type, EntityKind::Marker);
        assert_eq!(back.modded_type, Some(60_000));
        assert_eq!(back, packet);
    }

    #[test]
    fn a_modded_block_reads_as_a_full_solid_cube() {
        use azalea::physics::collision::BlockWithShape;
        let modded = BlockState::from_network(BlockState::MAX_STATE as u32 + 3);
        assert!(modded.is_collision_shape_full());
        assert!(!modded.is_collision_shape_empty());
        assert!(crate::util::block_model::is_solid(modded));
    }
}
