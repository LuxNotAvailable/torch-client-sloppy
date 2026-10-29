use bevy::prelude::*;
use bevy::render::Extract;

use super::*;

fn ancestors<'a>(
    parents: &'a Query<&ChildOf>,
    entity: Entity,
) -> impl Iterator<Item = Entity> + 'a {
    std::iter::successors(Some(entity), move |e| {
        parents.get(*e).ok().map(ChildOf::parent)
    })
    .take(8)
}

#[allow(clippy::type_complexity)]
pub(crate) fn extract_entity_draws(
    meshes: Extract<
        Query<(
            Entity,
            &Mesh3d,
            &MeshMaterial3d<crate::renderer::entity_material::EntityMaterial>,
            &GlobalTransform,
            &ViewVisibility,
            &InheritedVisibility,
            Option<&bevy::camera::visibility::RenderLayers>,
            Has<crate::blockentities::BeRigNode>,
            Has<crate::util::particles::ParticleBatch>,
            Has<crate::blockentities::BeamNode>,
            Has<crate::renderer::tethers::WeatherLayer>,
        )>,
    >,
    materials: Extract<Res<Assets<crate::renderer::entity_material::EntityMaterial>>>,
    rigs: Extract<Option<Res<crate::entities::EntityRigs>>>,
    player_rigs: Extract<Option<Res<crate::renderer::player_model::PlayerRigs>>>,
    parents: Extract<Query<&ChildOf>>,
    visibilities: Extract<Query<&Visibility>>,
    render: Option<ResMut<PackRender>>,
) {
    crate::prof_span!("render:pack_extract_entity_draws");
    let Some(mut render) = render else { return };
    let Some(install) = render.install.as_deref_mut() else {
        return;
    };
    let (installed, frame) = (&*install.installed, &mut install.frame);
    frame.entity_draws.clear();
    let programs = installed.entities;
    let shadow_usable = install.usage.shadow;
    let pass_of = |kind: EntityKind| {
        std::iter::once(kind)
            .chain(kind.fallback())
            .find(|k| programs[k.index()].is_some())
    };
    let casters = installed.shadow_casters;
    let caster_pass = programs[EntityKind::ShadowCaster.index()]
        .filter(|_| shadow_usable)
        .map(|_| EntityKind::ShadowCaster);
    let local_root = player_rigs
        .as_deref()
        .and_then(|r| r.local_root())
        .filter(|_| casters.player || casters.entities);
    let caster_reach = installed.constants.shadow_distance * 1.75;
    let shown_below = |entity: Entity, root: Entity| {
        for node in ancestors(&parents, entity) {
            if node == root {
                return true;
            }
            if visibilities
                .get(node)
                .is_ok_and(|v| *v == Visibility::Hidden)
            {
                return false;
            }
        }
        false
    };
    let under = |entity: Entity, root: Entity| ancestors(&parents, entity).any(|node| node == root);
    let world_kinds = [EntityKind::Entity, EntityKind::Block, EntityKind::Particle];
    if caster_pass.is_none()
        && world_kinds
            .iter()
            .all(|k| pass_of(*k).is_none() && pass_of(k.translucent()).is_none())
    {
        return;
    }
    frame.entity_roots.clear();
    if let Some(rigs) = rigs.as_deref()
        && !installed.entity_ids.is_empty()
    {
        for (root, kind) in rigs.roots() {
            let name = kind.to_str();
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            if let Some(id) = installed.entity_ids.get(name) {
                frame.entity_roots.insert(root, *id);
            }
        }
    }
    let entity_id = |entity: Entity, roots: &std::collections::HashMap<Entity, i32>| {
        ancestors(&parents, entity)
            .find_map(|node| roots.get(&node).copied())
            .unwrap_or(-1)
    };
    let world = bevy::camera::visibility::RenderLayers::default();
    for (entity, mesh, material, transform, visible, inherited, layers, block, particle, beam, weather) in
        &meshes
    {
        if layers.is_some_and(|l| !l.intersects(&world)) {
            continue;
        }
        let kind = match (block, particle) {
            (true, _) => EntityKind::Block,
            (_, true) => EntityKind::Particle,
            _ => EntityKind::Entity,
        };
        let local = local_root.filter(|root| kind == EntityKind::Entity && under(entity, *root));
        let casts = caster_pass.is_some()
            && !weather
            && match kind {
                EntityKind::Block => casters.block_entities,
                EntityKind::Particle => false,
                _ => casters.entities || local.is_some(),
            }
            && match local {
                Some(root) => shown_below(entity, root),
                None => inherited.get(),
            }
            && transform.translation().distance(frame.camera) <= caster_reach;
        let Some(material) = materials.get(&material.0) else {
            continue;
        };
        use crate::renderer::entity_material::LightMode;
        let blend = blended(material.alpha_mode);
        let glowing = material.params.mode == LightMode::Emissive as u32;
        let weather = weather && pass_of(EntityKind::Weather).is_some();
        let drawn_as = match kind {
            _ if weather => EntityKind::Weather,
            EntityKind::Entity if glowing && programs[EntityKind::SpiderEyes.index()].is_some() => {
                EntityKind::SpiderEyes
            }
            EntityKind::Block if beam && pass_of(EntityKind::BeaconBeam).is_some() => {
                EntityKind::BeaconBeam
            }
            _ if blend => kind.translucent(),
            _ => kind,
        };
        let pass = pass_of(drawn_as).filter(|_| visible.get());
        if pass.is_none() && !casts {
            continue;
        }
        // Weather is shaded in its vertex colours; the sky lights the rest.
        let light = if weather {
            Vec2::new(0.0, 240.0)
        } else if glowing {
            Vec2::splat(240.0)
        } else {
            Vec2::new(material.params.light_block.w, material.params.light_sky.w)
        };
        let draw = EntityDraw {
            mesh: mesh.0.id(),
            texture: material.texture.as_ref().map(|t| t.id()),
            model: transform.to_matrix(),
            tint: material.params.tint,
            light,
            blend,
            pass: kind,
            overlay: overlay(&material.params),
            entity_id: if frame.entity_roots.is_empty() {
                -1
            } else {
                entity_id(entity, &frame.entity_roots)
            },
            item_id: 0,
            stage: drawn_as.render_stage(),
            // Layers over the same surface draw after what they cover.
            order: material.depth_bias.clamp(0, i32::from(u8::MAX)) as u8,
            relative: false,
            flags: if !weather
                && [LightMode::Flat as u32, LightMode::Emissive as u32]
                    .contains(&material.params.mode)
            {
                ENTITY_SHADE_IN_ALPHA
            } else {
                0
            },
        };
        if let Some(caster) = caster_pass.filter(|_| casts) {
            frame.entity_draws.push(EntityDraw {
                pass: caster,
                blend: false,
                ..draw
            });
        }
        if let Some(pass) = pass {
            frame.entity_draws.push(EntityDraw { pass, ..draw });
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn extract_standard_draws(
    sky: Extract<
        Query<(
            &crate::renderer::sky::SkyPart,
            &Mesh3d,
            &crate::renderer::sky::SkySource,
            &GlobalTransform,
            &ViewVisibility,
        )>,
    >,
    sky_root: Extract<Query<&GlobalTransform, With<crate::renderer::sky::SkyRoot>>>,
    clouds: Extract<
        Query<
            (
                &Mesh3d,
                &MeshMaterial3d<StandardMaterial>,
                &GlobalTransform,
                &ViewVisibility,
            ),
            With<crate::renderer::clouds::CloudLayer>,
        >,
    >,
    hand: Extract<
        Query<(
            Entity,
            &Mesh3d,
            &MeshMaterial3d<StandardMaterial>,
            &GlobalTransform,
            &ViewVisibility,
            &bevy::camera::visibility::RenderLayers,
        )>,
    >,
    hand_camera: Extract<Query<&GlobalTransform, With<crate::renderer::hand::HandCamera>>>,
    overlays: Extract<
        Query<
            (
                &Mesh3d,
                &MeshMaterial3d<StandardMaterial>,
                &GlobalTransform,
                &ViewVisibility,
                Has<crate::renderer::overlays::BreakOverlayMarker>,
            ),
            Or<(
                With<crate::renderer::overlays::BreakOverlayMarker>,
                With<crate::renderer::overlays::BlockOutlineMarker>,
            )>,
        >,
    >,
    materials: Extract<Res<Assets<StandardMaterial>>>,
    render: Option<ResMut<PackRender>>,
) {
    crate::prof_span!("render:pack_extract_standard_draws");
    use crate::renderer::sky::SkyPart;
    use crate::shaderpack::features::render_stage;
    let Some(mut render) = render else { return };
    let Some(install) = render.install.as_deref_mut() else {
        return;
    };
    let (installed, frame) = (&*install.installed, &mut install.frame);
    let has = |kind: EntityKind| installed.entities[kind.index()].is_some();
    frame.hand_entities.clear();
    let mut hand_entities = std::mem::take(&mut frame.hand_entities);
    let inputs = frame.inputs;
    let mut push = |kind: EntityKind,
                    mesh: &Mesh3d,
                    material: &StandardMaterial,
                    model: Mat4,
                    relative: bool,
                    stage: i32,
                    order: u8,
                    second: bool| {
        let (light, item_id) = if kind.is_hand() {
            (inputs.eye_light.as_vec2() * 16.0, inputs.held_items[0])
        } else {
            (Vec2::splat(240.0), 0)
        };
        frame.entity_draws.push(EntityDraw {
            mesh: mesh.0.id(),
            texture: material.base_color_texture.as_ref().map(|t| t.id()),
            model,
            tint: if kind.is_hand() || kind == EntityKind::DamagedBlock {
                Vec4::ONE
            } else {
                Vec4::from_array(material.base_color.to_srgba().to_f32_array())
            },
            light,
            blend: second,
            pass: kind,
            overlay: Vec4::ZERO,
            entity_id: -1,
            item_id,
            stage,
            order,
            relative,
            flags: 0,
        });
    };

    if (has(EntityKind::SkyBasic) || has(EntityKind::SkyTextured))
        && let Ok(root) = sky_root.single()
    {
        let (scale, _, translation) = root.to_scale_rotation_translation();
        let local = Mat4::from_scale(scale.recip()) * Mat4::from_translation(-translation);
        // `sunPathRotation` tilts the celestial sphere, and Iris draws the
        // sun, moon and stars on it, so they stay where `sunPosition` (and
        // a pack's sun glow) says. The uniforms put the tilt between the
        // sphere's yaw and its daily turn; in the sphere's own frame that is
        // this rotation, applied around the camera.
        let tilt = Mat4::from_rotation_y((-90.0f32).to_radians())
            * Mat4::from_rotation_z(installed.constants.sun_path_rotation.to_radians())
            * Mat4::from_rotation_y(90.0f32.to_radians());
        let parts = installed.sky_parts;
        for (part, mesh, material, transform, visible) in &sky {
            let (kind, stage, order, wanted) = match part {
                SkyPart::SkyDisc => (EntityKind::SkyBasic, "SKY", 0, parts.sky),
                SkyPart::SunriseFan => (EntityKind::SkyBasic, "SUNSET", 1, parts.sky),
                SkyPart::Sun => (EntityKind::SkyTextured, "SUN", 2, parts.sun),
                SkyPart::Moon => (EntityKind::SkyTextured, "MOON", 3, parts.moon),
                SkyPart::Stars => (EntityKind::SkyBasic, "STARS", 4, parts.stars),
                SkyPart::DarkDisc => (EntityKind::SkyBasic, "VOID", 5, parts.sky),
                SkyPart::EndSky => (EntityKind::SkyTextured, "SKY", 0, parts.sky),
            };
            if !visible.get() || !wanted || !has(kind) {
                continue;
            }
            let Some(material) = materials.get(&material.0) else {
                continue;
            };
            let second = material.alpha_mode == AlphaMode::Add;
            let celestial = matches!(part, SkyPart::Sun | SkyPart::Moon | SkyPart::Stars);
            let model = local * transform.to_matrix();
            push(
                kind,
                mesh,
                material,
                if celestial { tilt * model } else { model },
                true,
                render_stage(stage),
                order,
                second,
            );
        }
    }

    if has(EntityKind::Clouds) {
        for (mesh, material, transform, visible) in &clouds {
            if !visible.get() {
                continue;
            }
            let Some(material) = materials.get(&material.0) else {
                continue;
            };
            let fast = material.cull_mode.is_none();
            push(
                EntityKind::Clouds,
                mesh,
                material,
                transform.to_matrix(),
                false,
                EntityKind::Clouds.render_stage(),
                0,
                fast,
            );
        }
    }

    for (mesh, material, transform, visible, cracks) in &overlays {
        let kind = if cracks {
            EntityKind::DamagedBlock
        } else {
            EntityKind::Line
        };
        if !visible.get() || !has(kind) {
            continue;
        }
        let Some(material) = materials.get(&material.0) else {
            continue;
        };
        push(
            kind,
            mesh,
            material,
            transform.to_matrix(),
            false,
            kind.render_stage(),
            0,
            false,
        );
    }

    if EntityKind::ALL.iter().any(|k| k.is_hand() && has(*k))
        && let Ok(camera) = hand_camera.single()
    {
        let view = camera.to_matrix().inverse();
        let layer =
            bevy::camera::visibility::RenderLayers::layer(crate::renderer::hand::HAND_LAYER);
        for (entity, mesh, material, transform, visible, layers) in &hand {
            if !visible.get() || !layers.intersects(&layer) {
                continue;
            }
            let Some(material) = materials.get(&material.0) else {
                continue;
            };
            let translucent = blended(material.alpha_mode);
            let kind = if translucent && has(EntityKind::HandWater) {
                EntityKind::HandWater
            } else {
                EntityKind::Hand
            };
            if !has(kind) {
                continue;
            }
            hand_entities.insert(entity, mesh.0.id());
            push(
                kind,
                mesh,
                material,
                view * transform.to_matrix(),
                true,
                kind.render_stage(),
                0,
                translucent,
            );
        }
    }
    frame.hand_entities = hand_entities;
}


fn overlay(params: &crate::renderer::entity_material::EntityParams) -> Vec4 {
    let strength = (1.0 - params.light1.w).clamp(0.0, 1.0);
    let colour = if params.light0.w < 0.5 {
        Vec3::X
    } else {
        Vec3::ONE
    };
    colour.extend(strength)
}
