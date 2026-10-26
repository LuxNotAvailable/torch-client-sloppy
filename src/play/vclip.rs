use azalea::app::{App, Plugin};
use azalea::ecs::prelude::*;
use azalea::entity::{HasClientLoaded, LastSentPosition, LocalEntity, Physics, Position};
use azalea::packet::game::SendGamePacketEvent;
use azalea_core::position::Vec3;
use azalea_core::tick::GameTick;
use azalea_protocol::common::movements::MoveFlags;
use azalea_protocol::packets::game::s_move_player_pos::ServerboundMovePlayerPos;
use azalea_protocol::packets::game::s_move_player_status_only::ServerboundMovePlayerStatusOnly;

use crate::modules::vclip as module;

pub struct VClipPlugin;

impl Plugin for VClipPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            GameTick,
            vclip
                .after(azalea::movement::send_position)
                .before(crate::play::flight::flight_anti_kick),
        );
    }
}

fn vclip(
    mut query: Query<
        (Entity, &Physics, &mut Position, &mut LastSentPosition),
        (With<LocalEntity>, With<HasClientLoaded>),
    >,
    mut commands: Commands,
) {
    let Some(blocks) = module::take() else {
        return;
    };

    for (entity, physics, mut position, mut last_sent) in &mut query {
        let filler = MoveFlags {
            on_ground: physics.on_ground(),
            horizontal_collision: physics.horizontal_collision,
        };
        for _ in 1..module::packets(blocks) {
            commands.trigger(SendGamePacketEvent::new(
                entity,
                ServerboundMovePlayerStatusOnly { flags: filler },
            ));
        }
        let pos = Vec3::new(position.x, position.y + blocks, position.z);
        commands.trigger(SendGamePacketEvent::new(
            entity,
            ServerboundMovePlayerPos {
                pos,
                flags: MoveFlags {
                    on_ground: true,
                    ..filler
                },
            },
        ));
        *position = Position::new(pos);
        **last_sent = pos;
    }
}
