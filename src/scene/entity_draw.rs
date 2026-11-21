//! Puts every entity mesh drawn with `EntityMaterial` (mobs, players, block
//! entities, dropped items, beams) in `draw::Entities`, for the world and
//! the inventory figure.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::draw::{EntityDraw, EntityInstance, KIND_SHADOW_ONLY};
use crate::scene::entity_material::EntityMaterial;

type Drawn<'a> = (
    Entity,
    &'a Mesh3d,
    &'a crate::draw::Material3d<EntityMaterial>,
    &'a GlobalTransform,
    &'a InheritedVisibility,
    Option<&'a RenderLayers>,
    Option<&'a bevy::mesh::MeshTag>,
    (
        Has<crate::blockentities::BeRigNode>,
        Has<crate::util::particles::draw::ParticleBatch>,
        Has<crate::blockentities::BeamNode>,
        Has<crate::scene::tethers::WeatherLayer>,
    ),
);

pub fn plugin(app: &mut App) {
    app.init_resource::<crate::draw::Entities>().add_systems(
        PostUpdate,
        fill_draw_list
            .in_set(crate::draw::DrawSystems)
            .after(bevy::transform::TransformSystems::Propagate)
            .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    );
}

#[allow(clippy::too_many_arguments)]
fn fill_draw_list(
    drawn: Query<Drawn>,
    materials: Res<Assets<EntityMaterial>>,
    meshes: Res<crate::draw::bridge::Meshes>,
    parts: Res<crate::scene::rig_parts::RigParts>,
    ancestors: Query<&ChildOf>,
    local_roots: Query<(), With<crate::scene::player_model::LocalRigRoot>>,
    view: Res<crate::client::view::View>,
    mut out: ResMut<crate::draw::Entities>,
) {
    let out = &mut *out;
    out.world.clear();
    out.figure.clear();
    out.rows.clear();
    out.rows
        .extend_from_slice(bytemuck::cast_slice::<_, [f32; 12]>(parts.rows()));
    let player_shadow = crate::shaderpack::packvertex::player_shadow();
    // Whether a mesh sits in the local player's rig.
    let under_local_rig = |mut at: Entity| {
        for _ in 0..8 {
            if local_roots.contains(at) {
                return true;
            }
            match ancestors.get(at) {
                Ok(parent) => at = parent.parent(),
                Err(_) => return false,
            }
        }
        false
    };
    let world_layer = RenderLayers::default();
    let figure_layer = RenderLayers::layer(crate::client::view::FIGURE_LAYER);
    for (
        entity,
        mesh,
        material,
        transform,
        visible,
        layers,
        tag,
        (block, particle, beam, weather),
    ) in &drawn
    {
        // Particles are sprites (`draw::Particles`).
        if particle {
            continue;
        }
        let layers = layers.cloned().unwrap_or_default();
        let in_world = layers.intersects(&world_layer);
        let in_figure = view.figure_ready && layers.intersects(&figure_layer);
        if !in_world && !in_figure {
            continue;
        }
        // The hidden local player still casts a pack's shadow.
        let shadow_only = !visible.get();
        if shadow_only && !(player_shadow && under_local_rig(entity)) {
            continue;
        }
        let (Some(material), Some(data)) = (materials.get(&material.0), meshes.get(mesh.0.id()))
        else {
            continue;
        };
        let p = &material.params;
        let draw = EntityDraw {
            mesh: data.clone(),
            texture: material.texture.clone(),
            instance: EntityInstance {
                mesh: 0,
                texture: u32::MAX,
                tag: tag.map_or(0, |t| t.0),
                alpha: material.alpha_mode.code(),
                depth_bias: material.depth_bias,
                kind: match (block, beam, weather) {
                    (true, ..) => 1,
                    (_, true, _) => 3,
                    (.., true) => 4,
                    _ => 0,
                } | if shadow_only { KIND_SHADOW_ONLY } else { 0 }
                    | if material.alpha_mode.code() == 0 {
                        crate::draw::KIND_CULL
                    } else {
                        0
                    },
                tint: p.tint.to_array(),
                light: p.light.to_array(),
                light0: p.light0.to_array(),
                light1: p.light1.to_array(),
                switches: p.switches.to_array(),
                levels: [p.light_block.w, p.light_sky.w],
                mode: p.mode,
                see_through: u32::from(material.see_through),
                transform: transform.to_matrix().to_cols_array(),
            },
        };
        if in_world && in_figure {
            out.figure.push(draw.clone());
        } else if in_figure {
            out.figure.push(draw);
            continue;
        }
        out.world.push(draw);
    }
}
