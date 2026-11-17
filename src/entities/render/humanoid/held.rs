//! `ItemInHandLayer` for humanoid mobs: the items in their hands, posed on
//! the arms. Each hand is an item layer whose placement comes from the
//! mob's own posed model (the arm's matrix), then vanilla's hand offsets.

use std::collections::HashMap;
use std::sync::Mutex;

use azalea_registry::builtin::EntityKind;
use bevy::math::{Mat4, Quat, Vec3};
use bevy::prelude::Transform;

use crate::entities::geom::{BakedModel, LayerDef, PartState, bake};
use crate::entities::models::humanoid::{illager, piglin, skeleton, vex, zombie};
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::{MODEL_Y_OFFSET, RenderSpec, RootPose, TexturePath};

type Setup = fn(&BakedModel, &mut [PartState], &EntityState);

/// The model and setup the mob's body draws with (adult, baby).
fn rig(kind: EntityKind, baby: bool) -> Option<(fn() -> LayerDef, Setup)> {
    use EntityKind as K;
    Some(match (kind, baby) {
        (K::Zombie, false) => (zombie::zombie_layer, zombie::setup_anim),
        (K::Zombie | K::Husk, true) => (zombie::baby_zombie_layer, zombie::setup_anim),
        (K::Husk, false) => (zombie::husk_layer, zombie::setup_anim),
        (K::Drowned, false) => (zombie::drowned_layer, zombie::drowned_setup_anim),
        (K::Drowned, true) => (zombie::baby_drowned_layer, zombie::drowned_setup_anim),
        (K::Giant, _) => (zombie::giant_layer, zombie::setup_anim),
        (K::Skeleton | K::WitherSkeleton | K::Stray, _) => {
            (skeleton::skeleton_layer, skeleton::setup_anim)
        }
        (K::Bogged, _) => (skeleton::bogged_layer, skeleton::bogged_setup_anim),
        (K::Parched, _) => (skeleton::parched_layer, skeleton::setup_anim),
        (K::Piglin | K::PiglinBrute, false) => {
            (piglin::adult_piglin_layer, piglin::piglin_setup_anim)
        }
        (K::Piglin | K::PiglinBrute, true) => {
            (piglin::baby_piglin_layer, piglin::piglin_setup_anim)
        }
        (K::ZombifiedPiglin, false) => (
            piglin::adult_piglin_layer,
            piglin::zombified_piglin_setup_anim,
        ),
        (K::ZombifiedPiglin, true) => (
            piglin::baby_piglin_layer,
            piglin::zombified_piglin_setup_anim,
        ),
        (K::Pillager, _) => (illager::illager_layer, illager::pillager_setup_anim),
        (K::Vindicator, _) => (illager::illager_layer, illager::vindicator_setup_anim),
        (K::Illusioner, _) => (illager::illager_layer, illager::illusioner_setup_anim),
        (K::Vex, _) => (vex::vex_layer, vex::setup_anim),
        _ => return None,
    })
}

/// Baked models by kind and age, made once.
fn model(kind: EntityKind, baby: bool, layer: fn() -> LayerDef) -> &'static BakedModel {
    static MODELS: Mutex<Option<HashMap<(EntityKind, bool), &'static BakedModel>>> =
        Mutex::new(None);
    let mut models = MODELS.lock().unwrap_or_else(|e| e.into_inner());
    models
        .get_or_insert_with(HashMap::new)
        .entry((kind, baby))
        .or_insert_with(|| Box::leak(Box::new(bake(&layer()))))
}

/// The item in the right (`left`: left) hand: the main hand's unless the
/// mob is left-handed.
fn hand_item(st: &EntityState, left: bool) -> &crate::renderer::anim::HeldItem {
    let equipment = &st.extras.equipment;
    if left != equipment.left_handed {
        &equipment.off_hand
    } else {
        &equipment.main_hand
    }
}

/// Vanilla's per-renderer conditions on top of holding something.
fn shows_items(st: &EntityState) -> bool {
    match st.kind {
        // `VindicatorRenderer`: only when aggressive.
        EntityKind::Vindicator => st.extras.aggressive,
        // `IllusionerRenderer`: casting or aggressive.
        EntityKind::Illusioner => st.extras.aggressive || st.extras.spell != 0,
        _ => true,
    }
}

fn right_visible(st: &EntityState) -> bool {
    shows_items(st) && !hand_item(st, false).is_empty()
}

fn left_visible(st: &EntityState) -> bool {
    shows_items(st) && !hand_item(st, true).is_empty()
}

/// The model the item in that hand draws now: a drawn bow or crossbow
/// while the mob uses it (its own use ticks), a loaded crossbow, or the
/// stack's own look.
fn hand_model(st: &EntityState, left: bool) -> TexturePath {
    use crate::items::dispatch;
    let item = hand_item(st, left);
    let main = left == st.extras.equipment.left_handed;
    let using = st.extras.using_item && st.extras.use_offhand != main;
    if !using && !item.charged {
        return match &item.variant {
            Some(_) => item.model_key().into(),
            None => crate::entities::render::objects::thrown::item_key(item),
        };
    }
    let view = dispatch::StackView {
        item: item.id,
        count: item.count,
        damage: 0,
        max_damage: 0,
        cooldown: 0,
        charged: item.charged,
        data: None,
    };
    let mut ctx = (*dispatch::context()).clone();
    ctx.using = using.then(|| (item.id.to_owned(), st.extras.ticks_using_item as i32));
    ctx.left_handed = st.extras.equipment.left_handed;
    match crate::items::icons::runtime_path(item.id, &view, &ctx) {
        Some(path) => dispatch::path_key(item.id, &path).into(),
        None => crate::entities::render::objects::thrown::item_key(item),
    }
}

fn right_item(st: &EntityState) -> TexturePath {
    hand_model(st, false)
}

fn left_item(st: &EntityState) -> TexturePath {
    hand_model(st, true)
}

/// `translateToHand`, the hand's turn and `ItemInHandLayer`'s offsets, in
/// the item layer's space (under the root hook).
fn hand_placement(st: &EntityState, left: bool, hook: Transform) -> Transform {
    let Some((layer, setup)) = rig(st.kind, st.extras.is_baby) else {
        return Transform::IDENTITY;
    };
    let model = model(st.kind, st.extras.is_baby, layer);
    let arm_name = if left { "left_arm" } else { "right_arm" };
    let Some(arm) = model.find(arm_name) else {
        return Transform::IDENTITY;
    };
    let mut states = model.rest_pose();
    model.reset_pose(&mut states);
    setup(model, &mut states, st);
    let mut posed = Vec::new();
    model.pose_parts(&states, Mat4::IDENTITY, &mut posed);

    let (x, y, z) = if st.extras.is_baby {
        (0.0, 1.0, -4.5)
    } else {
        (1.0, 2.0, -10.0)
    };
    let side = if left { -1.0 } else { 1.0 };
    let hand = Mat4::from_quat(
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
            * Quat::from_rotation_y(std::f32::consts::PI),
    ) * Mat4::from_translation(Vec3::new(side * x, y, z) / 16.0);
    // The body's own chain: flipped, hooked, lowered (as the model layer).
    let hook = hook.to_matrix();
    let body = Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * hook
        * Mat4::from_translation(Vec3::new(0.0, MODEL_Y_OFFSET, 0.0));
    Transform::from_matrix(hook.inverse() * body * posed[arm].mat * hand)
}

/// The root hook the body draws under (the drowned's swim turn).
fn body_hook(st: &EntityState) -> Transform {
    match st.kind {
        EntityKind::Drowned => super::zombie::drowned_root(st).hook,
        _ => Transform::IDENTITY,
    }
}

fn right_inner(st: &EntityState) -> Transform {
    hand_placement(st, false, body_hook(st))
}

fn left_inner(st: &EntityState) -> Transform {
    hand_placement(st, true, body_hook(st))
}

/// Both hands' layers for `kinds`, rooted as their bodies are.
pub fn register(
    registry: &mut Registry,
    kinds: &[EntityKind],
    root: Option<fn(&EntityState) -> RootPose>,
) {
    for &kind in kinds {
        let hands = [
            (
                "held_right",
                right_item as fn(&EntityState) -> TexturePath,
                "thirdperson_righthand",
                right_inner as fn(&EntityState) -> Transform,
                right_visible as fn(&EntityState) -> bool,
            ),
            (
                "held_left",
                left_item,
                "thirdperson_lefthand",
                left_inner,
                left_visible,
            ),
        ];
        for (name, item, display, inner, visible) in hands {
            let mut spec = RenderSpec::item(name, item, display, inner).with_visible(visible);
            if let Some(root) = root {
                spec = spec.with_root(root);
            }
            registry.add(kind, spec);
        }
    }
}
