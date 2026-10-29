//! Gliding and the pushes vanilla gives the local player on the client: the
//! elytra start (`LocalPlayer.aiStep`'s `tryToStartFallFlying`), a firework
//! rocket's boost while gliding (`FireworkRocketEntity.tick`), and the
//! riptide launch (`TridentItem.releaseUsing`). The server trusts this
//! client's movement, so each has to happen here.

use azalea::app::{App, Plugin};
use azalea::ecs::prelude::*;
use azalea::entity::inventory::Inventory;
use azalea::entity::metadata::{AttachedToTarget, FallFlying};
use azalea::entity::{
    ActiveEffects, EntityKindComponent, LocalEntity, LookDirection, OnClimbable, Physics,
    PlayerAbilities, view_vector,
};
use azalea::packet::game::SendGamePacketEvent;
use azalea::physics::{PhysicsSystems, ai_step, travel::travel};
use azalea_core::entity_id::MinecraftEntityId;
use azalea_core::position::Vec3;
use azalea_core::tick::GameTick;
use azalea_inventory::ItemStack;
use azalea_inventory::components::{Damage, Glider, MaxDamage};
use azalea_protocol::packets::game::s_player_command::{Action, ServerboundPlayerCommand};
use azalea_registry::builtin::{EntityKind, MobEffect};

use crate::session::MOVE_JUMP;

/// The player menu's armour slots: head, chest, legs, feet.
const ARMOR_SLOTS: std::ops::RangeInclusive<usize> = 5..=8;

pub struct GlidingPlugin;

impl Plugin for GlidingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            GameTick,
            (
                start_gliding.in_set(PhysicsSystems).before(ai_step),
                firework_boost
                    .in_set(PhysicsSystems)
                    .after(ai_step)
                    .before(travel),
            ),
        );
    }
}

/// `canGlideUsing`: a glider that one more use would not break.
fn usable_glider(stack: &ItemStack) -> bool {
    if stack.get_component::<Glider>().is_none() {
        return false;
    }
    match (
        stack.get_component::<Damage>(),
        stack.get_component::<MaxDamage>(),
    ) {
        (Some(damage), Some(max)) => damage.amount < max.amount - 1,
        _ => true,
    }
}

/// Everything `tryToStartFallFlying` and `canGlide` ask of the player,
/// beyond the jump itself.
#[derive(Clone, Copy, Debug, Default)]
struct GlideCheck {
    on_ground: bool,
    on_climbable: bool,
    riding: bool,
    levitating: bool,
    flying: bool,
    in_liquid: bool,
    already_gliding: bool,
    has_glider: bool,
}

impl GlideCheck {
    fn may_start(&self) -> bool {
        !self.on_ground
            && !self.on_climbable
            && !self.riding
            && !self.levitating
            && !self.flying
            && !self.in_liquid
            && !self.already_gliding
            && self.has_glider
    }
}

#[allow(clippy::type_complexity)]
fn start_gliding(
    mut local: Query<
        (
            Entity,
            &MinecraftEntityId,
            &Physics,
            &OnClimbable,
            &ActiveEffects,
            &PlayerAbilities,
            &Inventory,
            Option<&FallFlying>,
        ),
        With<LocalEntity>,
    >,
    mut was_jumping: Local<bool>,
    mut commands: Commands,
) {
    let Some(shared) = crate::SHARED.get() else {
        return;
    };
    let (jumping, toggled_flight) = {
        let s = shared.lock().unwrap();
        (s.move_flags & MOVE_JUMP != 0, s.session.fly_toggle)
    };
    let pressed = jumping && !std::mem::replace(&mut *was_jumping, jumping);
    if !pressed || toggled_flight {
        return;
    }
    let Ok((entity, id, physics, on_climbable, effects, abilities, inventory, gliding)) =
        local.single_mut()
    else {
        return;
    };
    let check = GlideCheck {
        on_ground: physics.on_ground(),
        on_climbable: **on_climbable,
        riding: crate::client::tracking::local_vehicle_id().is_some(),
        levitating: effects.get(MobEffect::Levitation).is_some(),
        flying: abilities.flying,
        in_liquid: physics.is_in_water() || physics.is_in_lava(),
        already_gliding: gliding.is_some_and(|g| **g),
        has_glider: ARMOR_SLOTS
            .filter_map(|i| inventory.inventory_menu.slot(i))
            .any(usable_glider),
    };
    if !check.may_start() {
        return;
    }
    // `startFallFlying` sets the flag on the client at once; the server's
    // metadata confirms it, or clears it again.
    commands.entity(entity).insert(FallFlying(true));
    commands.trigger(SendGamePacketEvent::new(
        entity,
        ServerboundPlayerCommand {
            id: *id,
            action: Action::StartFallFlying,
            data: 0,
        },
    ));
}

/// A rocket's push on a gliding player, with the look `lookAngle` and the
/// current velocity.
fn boost(look: Vec3, velocity: Vec3) -> Vec3 {
    let axis = |look: f64, v: f64| look * 0.1 + (look * 1.5 - v) * 0.5;
    Vec3::new(
        velocity.x + axis(look.x, velocity.x),
        velocity.y + axis(look.y, velocity.y),
        velocity.z + axis(look.z, velocity.z),
    )
}

#[allow(clippy::type_complexity)]
fn firework_boost(
    rockets: Query<(&EntityKindComponent, &AttachedToTarget), Without<LocalEntity>>,
    mut local: Query<
        (
            &MinecraftEntityId,
            &mut Physics,
            &LookDirection,
            Option<&FallFlying>,
        ),
        With<LocalEntity>,
    >,
) {
    let Ok((id, mut physics, direction, gliding)) = local.single_mut() else {
        return;
    };
    if !gliding.is_some_and(|g| **g) {
        return;
    }
    let look = view_vector(*direction);
    for (kind, attached) in &rockets {
        if **kind == EntityKind::FireworkRocket && attached.0.0 == Some(id.0 as u32) {
            physics.velocity = boost(look, physics.velocity);
        }
    }
}

/// `getTridentSpinAttackStrength`: riptide's `trident_spin_attack_strength`,
/// 1.5 and 0.75 a level above the first.
pub(crate) fn riptide_strength(level: i32) -> f32 {
    if level <= 0 {
        0.0
    } else {
        1.5 + 0.75 * (level - 1) as f32
    }
}

/// The riptide launch's push, along the look, as long as the strength.
pub(crate) fn riptide_push(yaw: f32, pitch: f32, strength: f32) -> Vec3 {
    let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
    let x = -yaw.sin() * pitch.cos();
    let y = -pitch.sin();
    let z = yaw.cos() * pitch.cos();
    let scale = strength / (x * x + y * y + z * z).sqrt();
    Vec3::new((x * scale) as f64, (y * scale) as f64, (z * scale) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glide_starts_only_in_the_air_with_a_working_elytra() {
        let ready = GlideCheck {
            has_glider: true,
            ..GlideCheck::default()
        };
        assert!(ready.may_start());
        for blocked in [
            GlideCheck {
                on_ground: true,
                ..ready
            },
            GlideCheck {
                riding: true,
                ..ready
            },
            GlideCheck {
                flying: true,
                ..ready
            },
            GlideCheck {
                in_liquid: true,
                ..ready
            },
            GlideCheck {
                levitating: true,
                ..ready
            },
            GlideCheck {
                on_climbable: true,
                ..ready
            },
            GlideCheck {
                already_gliding: true,
                ..ready
            },
            GlideCheck {
                has_glider: false,
                ..ready
            },
        ] {
            assert!(!blocked.may_start(), "{blocked:?}");
        }
    }

    #[test]
    fn a_nearly_broken_elytra_does_not_glide() {
        use azalea_registry::builtin::ItemKind;
        let elytra = ItemStack::new(ItemKind::Elytra, 1);
        assert!(usable_glider(&elytra));
        let worn = elytra.clone().with_component(Damage { amount: 431 });
        assert!(!usable_glider(&worn));
        assert!(!usable_glider(&ItemStack::new(
            ItemKind::DiamondChestplate,
            1
        )));
    }

    #[test]
    fn a_rocket_drives_the_glide_toward_one_and_a_half_blocks_a_tick() {
        let look = Vec3::new(0.0, 0.0, 1.0);
        let mut v = Vec3::ZERO;
        for _ in 0..100 {
            v = boost(look, v);
        }
        // The fixed point of v + 0.1 + (1.5 - v) * 0.5.
        assert!((v.z - 1.7).abs() < 1e-6, "{v:?}");
    }

    #[test]
    fn riptide_pushes_along_the_look() {
        let push = riptide_push(0.0, 0.0, riptide_strength(1));
        assert!((push.z - 1.5).abs() < 1e-6 && push.x.abs() < 1e-6);
        let up = riptide_push(0.0, -90.0, riptide_strength(3));
        assert!((up.y - 3.0).abs() < 1e-5, "{up:?}");
        assert_eq!(riptide_strength(0), 0.0);
    }
}
