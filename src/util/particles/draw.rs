//! Drawing the live particles: batches by texture and blend, one mesh each.

#![allow(unused_imports)]

use super::*;

#[derive(Component)]
pub(crate) struct ParticleBatch;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_particles(
    gui_assets: Option<Res<crate::gui::render::GuiAssets>>,
    lightmap: Res<LightmapState>,
    chunk_material: Res<ChunkMaterial>,
    standard: Res<Assets<StandardMaterial>>,
    atlas: Res<ParticleAtlas>,
    live: Res<LiveParticles>,
    mut ent_materials: ResMut<Assets<EntityMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut material_cache: ResMut<ParticleMaterialCache>,
    mut batches: ResMut<ParticleBatches>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
    mut drawn: Query<
        (
            &mut Transform,
            &mut MeshMaterial3d<EntityMaterial>,
            &mut Visibility,
        ),
        With<ParticleBatch>,
    >,
    mut commands: Commands,
) {
    let Some(camera) = camera.iter().next() else {
        return;
    };
    if live.0.is_empty() {
        material_cache.0.clear();
        hide_from(&batches.pool, 0, &mut drawn);
        return;
    }

    let dim = crate::renderer::dimension::current();
    let dirs = diffuse_lights(dim);
    let terrain_atlas = particle_atlas(&chunk_material, &standard);
    let item_atlas = gui_assets.as_ref().map(|a| a.image.clone());
    let sheet = atlas.0.as_ref();
    let origin = (camera.translation() / 16.0).floor() * 16.0;
    let rotation = camera.rotation();
    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let normal = (rotation * Vec3::Z).to_array();

    let used = {
        let ParticleBatches {
            index,
            keys,
            groups,
            ..
        } = &mut *batches;
        index.clear();
        let mut used = 0usize;
        for (i, particle) in live.0.iter().enumerate() {
            if particle.tick_acc < 0.0 {
                continue;
            }
            let source = if particle.flags & TERRAIN != 0 {
                Source::Terrain
            } else if particle.flags & ITEMS != 0 {
                Source::Items
            } else {
                Source::Sheet
            };
            let missing = match source {
                Source::Terrain => terrain_atlas.is_none(),
                Source::Items => item_atlas.is_none(),
                Source::Sheet => sheet.is_none(),
            };
            if missing {
                continue;
            }
            let (block_light, sky_light) = if particle.flags & FULLBRIGHT != 0 {
                (15, 15)
            } else {
                crate::renderer::lightmap::levels_at([
                    particle.pos.x,
                    particle.pos.y,
                    particle.pos.z,
                ])
            };
            let key = BatchKey {
                block_light,
                sky_light,
                dim: dim as u8,
                source,
                translucent: particle.flags & TRANSLUCENT != 0,
            };
            let slot = match index.get(&key) {
                Some(&slot) => slot,
                None => {
                    if used >= MAX_PARTICLE_BATCHES {
                        continue;
                    }
                    let slot = used;
                    used += 1;
                    if groups.len() <= slot {
                        groups.push(Vec::new());
                        keys.push(key);
                    } else {
                        keys[slot] = key;
                    }
                    groups[slot].clear();
                    index.insert(key, slot);
                    slot
                }
            };
            groups[slot].push(i as u32);
        }
        used
    };

    for slot in 0..used {
        let key = batches.keys[slot];
        let image = match key.source {
            Source::Terrain => match terrain_atlas.as_ref() {
                Some(atlas) => atlas.clone(),
                None => continue,
            },
            Source::Items => match item_atlas.as_ref() {
                Some(atlas) => atlas.clone(),
                None => continue,
            },
            Source::Sheet => match sheet {
                Some(sheet) => sheet.image.clone(),
                None => continue,
            },
        };
        let material = batch_material(
            &mut material_cache,
            &mut ent_materials,
            &lightmap.current,
            &image,
            dirs,
            key,
        );
        if batches.pool.len() <= slot {
            let mesh = meshes.add(empty_particle_mesh());
            let entity = commands
                .spawn((
                    ParticleBatch,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(origin),
                    NoFrustumCulling,
                ))
                .id();
            batches.pool.push((entity, mesh));
        } else if let Ok((mut transform, mut mesh_material, mut visibility)) =
            drawn.get_mut(batches.pool[slot].0)
        {
            if transform.translation != origin {
                transform.translation = origin;
            }
            if mesh_material.0 != material {
                mesh_material.0 = material;
            }
            if *visibility != Visibility::Inherited {
                *visibility = Visibility::Inherited;
            }
        }
        let handle = batches.pool[slot].1.clone();
        let Some(mesh) = meshes.get_mut(&handle) else {
            continue;
        };
        write_batch_mesh(
            mesh,
            &live.0,
            &batches.groups[slot],
            origin,
            right,
            up,
            normal,
            sheet,
        );
    }
    hide_from(&batches.pool, used, &mut drawn);

    for (key, handle) in material_cache.0.iter() {
        let Some(material) = ent_materials.get_mut(handle) else {
            continue;
        };
        material.params.set(Lit {
            tint: [1.0; 4],
            light: crate::renderer::lightmap::cell_light(
                &lightmap.current,
                key.block_light,
                key.sky_light,
            ),
            dirs,
            overlay_alpha: 1.0,
            overlay_white: false,
            glint: false,
        });
    }
}

pub(super) fn hide_from(
    pool: &[(Entity, Handle<Mesh>)],
    first: usize,
    drawn: &mut Query<
        (
            &mut Transform,
            &mut MeshMaterial3d<EntityMaterial>,
            &mut Visibility,
        ),
        With<ParticleBatch>,
    >,
) {
    for (entity, _) in pool.iter().skip(first) {
        if let Ok((_, _, mut visibility)) = drawn.get_mut(*entity)
            && *visibility != Visibility::Hidden
        {
            *visibility = Visibility::Hidden;
        }
    }
}

pub(super) fn batch_material(
    cache: &mut ParticleMaterialCache,
    materials: &mut Assets<EntityMaterial>,
    lightmap: &crate::renderer::lightmap::State,
    image: &Handle<Image>,
    dirs: [Vec3; 2],
    key: BatchKey,
) -> Handle<EntityMaterial> {
    if let Some(handle) = cache.0.get(&key) {
        return handle.clone();
    }
    if cache.0.len() >= MAX_CACHED_MATERIALS {
        cache.0.clear();
    }
    let cutoff = if key.translucent { 0.0 } else { 0.1 };
    let handle = materials.add(EntityMaterial {
        texture: Some(image.clone()),
        params: EntityParams::new(
            LightMode::Text,
            cutoff,
            Lit {
                tint: [1.0; 4],
                light: crate::renderer::lightmap::cell_light(
                    lightmap,
                    key.block_light,
                    key.sky_light,
                ),
                dirs,
                overlay_alpha: 1.0,
                overlay_white: false,
                glint: false,
            },
        ),
        #[cfg(not(target_arch = "wasm32"))]
        rig_parts: crate::renderer::rig_parts::PARTS,
        glint: crate::renderer::entity_material::GLINT,
        alpha_mode: if key.translucent {
            AlphaMode::Blend
        } else {
            AlphaMode::Mask(0.1)
        },
        depth_bias: 0,
        see_through: false,
    });
    cache.0.insert(key, handle.clone());
    handle
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_batch_mesh(
    mesh: &mut Mesh,
    particles: &[Particle],
    group: &[u32],
    origin: Vec3,
    right: Vec3,
    up: Vec3,
    normal: [f32; 3],
    sheet: Option<&Atlas>,
) {
    let mut positions = match mesh.remove_attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(v)) => v,
        _ => Vec::new(),
    };
    let mut normals = match mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(v)) => v,
        _ => Vec::new(),
    };
    let mut uvs = match mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(v)) => v,
        _ => Vec::new(),
    };
    let mut colors = match mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(v)) => v,
        _ => Vec::new(),
    };
    let mut indices = match mesh.remove_indices() {
        Some(Indices::U32(v)) => v,
        _ => Vec::new(),
    };
    positions.clear();
    normals.clear();
    uvs.clear();
    colors.clear();
    indices.clear();

    for &i in group {
        let p = &particles[i as usize];
        let lifetime = p.lifetime.max(1);
        let frac = (p.tick_acc / TICK).clamp(0.0, 1.0);
        let centre = p.prev.lerp(p.pos, frac) - origin;
        let t = (p.age as f32 + frac) / lifetime as f32;
        let scale = p.size
            * match p.grow {
                Grow::Fixed => 1.0,
                Grow::In => (t * 32.0).clamp(0.0, 1.0),
                Grow::Flame => 1.0 - t * t * 0.5,
                Grow::Lava => 1.0 - t * t,
                Grow::Portal => 1.0 - (1.0 - t) * (1.0 - t),
                Grow::Flash => {
                    let age = p.age as f32 + frac - 1.0;
                    7.1 * (age * 0.25 * std::f32::consts::PI).sin() / p.size.max(1e-6)
                }
            };
        if scale <= 0.0 {
            continue;
        }
        // A twinkling spark blinks through its last two thirds.
        if matches!(p.behavior, Behavior::Spark(s) if s.twinkle)
            && p.age >= p.lifetime / 3
            && (p.age + p.lifetime) / 3 % 2 != 0
        {
            continue;
        }
        let [u0, v0, u1, v1] = match p.sprite {
            Sprite::Fixed(uv) => uv,
            Sprite::Animated(index) => {
                let Some(sheet) = sheet else { continue };
                let frames = sheet.sheets[index as usize].len();
                let frame = p.age as usize * frames.saturating_sub(1) / lifetime as usize;
                sheet.frame_uv(index, frame)
            }
        };
        let base = positions.len() as u32;
        let (mut dx, mut dy) = (right * scale, up * scale);
        if let Behavior::Leaf(leaf) = p.behavior {
            let roll = leaf.old_roll + (leaf.roll - leaf.old_roll) * frac;
            let (sin, cos) = roll.sin_cos();
            (dx, dy) = (dx * cos + dy * sin, dy * cos - dx * sin);
        }
        for (corner, uv) in [
            (centre - dx - dy, [u0, v1]),
            (centre + dx - dy, [u1, v1]),
            (centre + dx + dy, [u1, v0]),
            (centre - dx + dy, [u0, v0]),
        ] {
            positions.push(corner.to_array());
            normals.push(normal);
            uvs.push(uv);
            colors.push(if p.grow == Grow::Flash {
                let age = p.age as f32 + frac - 1.0;
                [
                    p.tint[0],
                    p.tint[1],
                    p.tint[2],
                    (0.6 - age * 0.125).clamp(0.0, 1.0),
                ]
            } else {
                p.tint
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
}

pub(super) fn empty_particle_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, Vec::<[f32; 2]>::new());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new());
    mesh.insert_indices(Indices::U32(Vec::new()));
    mesh
}

pub(super) fn particle_atlas(
    chunk_material: &ChunkMaterial,
    standard: &Assets<StandardMaterial>,
) -> Option<Handle<Image>> {
    standard
        .get(&chunk_material.0)
        .and_then(|m| m.base_color_texture.clone())
}

pub(crate) fn particle_uv(state: azalea::block::BlockState) -> Option<[f32; 4]> {
    use azalea::block::BlockTrait;

    if state.is_air() {
        return None;
    }
    let block: Box<dyn BlockTrait> = Box::<dyn BlockTrait>::from(state);
    let name = block.id();
    let props = block.property_map();
    let baked = crate::blocks::registry().baked(state.id(), name, &props);
    let model = baked.parts.first()?.choices.first()?;
    if let Some(uv) = model.particle_uv {
        return Some(uv);
    }
    let quad = model.quads.first()?;
    let us = [quad.uv[0][0], quad.uv[1][0], quad.uv[2][0], quad.uv[3][0]];
    let vs = [quad.uv[0][1], quad.uv[1][1], quad.uv[2][1], quad.uv[3][1]];
    Some([
        us.into_iter().fold(f32::INFINITY, f32::min),
        vs.into_iter().fold(f32::INFINITY, f32::min),
        us.into_iter().fold(f32::NEG_INFINITY, f32::max),
        vs.into_iter().fold(f32::NEG_INFINITY, f32::max),
    ])
}

pub(super) fn tint_ratio(
    state: azalea::block::BlockState,
    pos: [i32; 3],
    world: Option<&azalea_world::World>,
) -> [f32; 3] {
    use crate::renderer::TintKind;
    use azalea::block::BlockTrait;
    use azalea_registry::DataRegistry;

    let block: Box<dyn BlockTrait> = Box::<dyn BlockTrait>::from(state);
    if block.id() == "grass_block" {
        return crate::util::biome_color::NEUTRAL;
    }

    let kind = crate::util::block_model::block_visual(state).tint_kind;
    if let TintKind::Redstone(power) = kind {
        return crate::util::biome_color::redstone_ratio(power);
    }
    if kind == TintKind::None {
        return crate::util::biome_color::NEUTRAL;
    }

    let Some(world) = world else {
        return crate::util::biome_color::NEUTRAL;
    };
    let block_pos = azalea_core::position::BlockPos {
        x: pos[0],
        y: pos[1],
        z: pos[2],
    };
    let Some(biome) = world.get_biome(block_pos) else {
        return crate::util::biome_color::NEUTRAL;
    };
    let table = crate::client::worker::biome_table(&world.registries);
    let idx = table
        .get(biome.protocol_id() as usize)
        .copied()
        .unwrap_or(u8::MAX);

    match kind {
        TintKind::None => unreachable!(),
        TintKind::Redstone(_) => unreachable!(),
        TintKind::Grass => crate::util::biome_color::grass_ratio(idx),
        TintKind::Foliage => crate::util::biome_color::foliage_ratio(idx),
        TintKind::DryFoliage => crate::util::biome_color::dry_foliage_ratio(idx),
        TintKind::Water => crate::util::biome_color::water_ratio(idx),
    }
}

pub(super) fn random_sub_uv(tile: [f32; 4], rng: &mut JavaRandom) -> [f32; 4] {
    let [u0, v0, u1, v1] = tile;
    let tw = (u1 - u0) / 4.0;
    let th = (v1 - v0) / 4.0;
    let cu = rng.next_int(4) as f32;
    let cv = rng.next_int(4) as f32;
    [
        u0 + cu * tw,
        v0 + cv * th,
        u0 + (cu + 1.0) * tw,
        v0 + (cv + 1.0) * th,
    ]
}
