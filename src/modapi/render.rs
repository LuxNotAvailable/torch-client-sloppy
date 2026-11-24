//! Draws plugins' rigs: each part a mesh (a block model, a whole block's
//! model, a cuboid, or an item's model) lit like a block entity at its
//! position, moved every frame by its animation. Parts are ordinary `EntityMaterial` meshes, so
//! both renderers draw them (`scene::entity_draw`). A scrolling texture
//! swaps between meshes built per quarter texel of its offset.

use std::collections::HashMap;

use bevy::math::Affine3A;
use bevy::prelude::*;
use torch_mod_api::{Animation, Geometry, Part, Scroll};

use crate::blocks::BakedQuad;
use crate::scene::entity_material::{EntityMaterial, LightMode, Lit, diffuse_lights};

/// Rigs farther than this from the player aren't drawn.
const DRAW_DISTANCE: f32 = 96.0;
/// Scroll offsets a tile: a quarter texel of a 16 px sprite.
const SCROLL_STEPS: f32 = 64.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Live>().add_systems(
        Update,
        (sync_rigs, sync_entity_rigs)
            .chain()
            .after(crate::app::FrameViewSystems)
            .run_if(in_state(crate::app::AppState::InGame)),
    );
}

/// An entity's rig: parts are its children, in its own frame.
#[derive(Component)]
struct ModEntityRoot;

struct LiveEntity {
    root: Entity,
    parts: Vec<LivePart>,
}

#[derive(Component)]
struct ModPart;

struct LivePart {
    entity: Entity,
    transform: torch_mod_api::Transform,
    /// An item's display transform, after the part's; identity otherwise.
    display: Transform,
    animation: Animation,
    /// The part, while its texture scrolls, and the step its mesh shows.
    scroll: Option<(Part, u32)>,
}

struct LiveRig {
    rev: u64,
    levels: (u8, u8),
    parts: Vec<LivePart>,
}

#[derive(Resource, Default)]
struct Live {
    rigs: HashMap<[i32; 3], LiveRig>,
    meshes: HashMap<String, Option<Handle<Mesh>>>,
    materials: HashMap<(u8, u8), Handle<EntityMaterial>>,
    /// Item parts' materials, by light levels and the item's texture.
    item_materials: HashMap<(u8, u8, AssetId<Image>), Handle<EntityMaterial>>,
    atlas: Option<Handle<Image>>,
    lights: crate::lighting::lightmap::LightCells,
    /// Entities a plugin draws, by entity id.
    entities: HashMap<i32, LiveEntity>,
}

#[allow(clippy::too_many_arguments)]
fn sync_rigs(
    mut commands: Commands,
    view: Res<crate::app::FrameView>,
    mut live: ResMut<Live>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    lightmap: Res<crate::app::systems::LightmapState>,
    chunk_material: Option<Res<crate::app::systems::ChunkMaterial>>,
    mut standard: ResMut<Assets<crate::draw::SimpleMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut item_assets: Option<ResMut<crate::scene::item_assets::ItemAssets>>,
    gui: Option<Res<crate::gui::render::GuiAssets>>,
    mut transforms: Query<(&mut Transform, &mut Mesh3d), With<ModPart>>,
) {
    let live = &mut *live;
    let rigs = super::store::rigs();
    let player = Vec3::from(view.player_pos);
    let in_reach = |pos: &[i32; 3]| {
        Vec3::new(
            pos[0] as f32 + 0.5,
            pos[1] as f32 + 0.5,
            pos[2] as f32 + 0.5,
        )
        .distance(player)
            < DRAW_DISTANCE
    };
    // Drop what's gone, changed, or out of reach.
    live.rigs.retain(|pos, rig| {
        let keep = in_reach(pos) && rigs.get(pos).is_some_and(|(_, rev)| *rev == rig.rev);
        if !keep {
            for part in &rig.parts {
                commands.entity(part.entity).despawn();
            }
        }
        keep
    });
    if rigs.is_empty() {
        return;
    }
    if live.atlas.is_none() {
        live.atlas = chunk_material
            .as_ref()
            .and_then(|m| standard.get(&m.0))
            .and_then(|m| m.base_color_texture.clone());
    }
    let Some(atlas) = live.atlas.clone() else {
        return;
    };
    let light_map = crate::client::worldsync::light_map().read();
    live.lights.begin(&lightmap.current);
    let dirs = diffuse_lights(crate::client::dimension::current());
    let ticks = view.game_time as f32 + view.partial;

    for (pos, (rig, rev)) in rigs.iter() {
        if !in_reach(pos) {
            continue;
        }
        let levels =
            crate::lighting::lightmap::resolved_levels(light_map.at(pos[0], pos[1], pos[2]));
        let relight = live.rigs.get(pos).is_some_and(|r| r.levels != levels);
        if !live.rigs.contains_key(pos) || relight {
            if let Some(old) = live.rigs.remove(pos) {
                for part in &old.parts {
                    commands.entity(part.entity).despawn();
                }
            }
            let cell = live.lights.get(&lightmap.current, levels.0, levels.1);
            let bright = live.lights.get(&lightmap.current, 15, 15);
            let parts = spawn_parts(
                &mut commands,
                live,
                Assets3 {
                    gui: gui.as_deref(),
                    meshes: &mut meshes,
                    materials: &mut materials,
                    standard: &mut standard,
                    images: &mut images,
                    items: item_assets.as_deref_mut(),
                },
                &atlas,
                (levels, cell, bright, dirs),
                &rig.parts,
                (*pos, None),
                ticks,
            );
            live.rigs.insert(
                *pos,
                LiveRig {
                    rev: *rev,
                    levels,
                    parts,
                },
            );
            continue;
        }
        // Moving parts: placed again this frame; scrolling ones re-meshed.
        let Some(rig) = live.rigs.get_mut(pos) else {
            continue;
        };
        for part in &mut rig.parts {
            let moving = part.animation != Animation::None;
            if !moving && part.scroll.is_none() {
                continue;
            }
            let Ok((mut t, mut mesh)) = transforms.get_mut(part.entity) else {
                continue;
            };
            if moving {
                *t =
                    place(*pos, part.transform, &part.animation, ticks).mul_transform(part.display);
            }
            if let Some((source, shown)) = &mut part.scroll {
                let step = source.scroll.as_ref().and_then(|s| scroll_step(s, ticks));
                if let Some(step) = step.filter(|s| s != shown)
                    && let Some(next) = part_mesh(&mut live.meshes, &mut meshes, source, Some(step))
                {
                    *shown = step;
                    mesh.0 = next;
                }
            }
        }
    }
    // Materials hold their light; a stale level set is rebuilt next time.
    if live.materials.len() > 256 {
        live.materials.clear();
    }
    if live.item_materials.len() > 1024 {
        live.item_materials.clear();
    }
}

/// Entities of a type a plugin draws (`Plugin::entity`): each rig asked for
/// when the entity appears, placed at it and turned by its yaw each frame.
#[allow(clippy::too_many_arguments)]
fn sync_entity_rigs(
    mut commands: Commands,
    view: Res<crate::app::FrameView>,
    mut live: ResMut<Live>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    lightmap: Res<crate::app::systems::LightmapState>,
    chunk_material: Option<Res<crate::app::systems::ChunkMaterial>>,
    mut standard: ResMut<Assets<crate::draw::SimpleMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut item_assets: Option<ResMut<crate::scene::item_assets::ItemAssets>>,
    gui: Option<Res<crate::gui::render::GuiAssets>>,
    mut roots: Query<&mut Transform, (With<ModEntityRoot>, Without<ModPart>)>,
    mut part_transforms: Query<&mut Transform, (With<ModPart>, Without<ModEntityRoot>)>,
) {
    let live = &mut *live;
    let modded = crate::client::tracking::modded_entity_types()
        .lock()
        .unwrap()
        .clone();
    // Modded entities in view whose type a plugin draws.
    let drawn: HashMap<i32, (usize, &'static str)> = view
        .entities
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let name = crate::protocol::modded_entity(*modded.get(&a.id)?)?;
            super::draws_entity(name).then_some((a.id, (i, name)))
        })
        .collect();
    super::keep_entity_data(|id| modded.contains_key(&id));
    // Rigs a mod packet or new entity data changed: dropped here, asked for
    // again below.
    let redraw = super::take_entity_redraws();
    live.entities.retain(|id, e| {
        let keep = drawn.contains_key(id) && !redraw.contains(id);
        if !keep {
            commands.entity(e.root).despawn();
            if !drawn.contains_key(id) {
                super::forget_entity(*id);
            }
        }
        keep
    });
    if drawn.is_empty() {
        return;
    }
    if live.atlas.is_none() {
        live.atlas = chunk_material
            .as_ref()
            .and_then(|m| standard.get(&m.0))
            .and_then(|m| m.base_color_texture.clone());
    }
    let Some(atlas) = live.atlas.clone() else {
        return;
    };
    let ticks = view.game_time as f32 + view.partial;
    let light_map = crate::client::worldsync::light_map().read();
    live.lights.begin(&lightmap.current);
    let dirs = diffuse_lights(crate::client::dimension::current());
    for (id, (index, name)) in &drawn {
        let anim = &view.entities[*index];
        let pos = anim.position(view.partial);
        let (yaw, _) = anim.look(view.partial);
        // +Z (the rig's front) turned to where the entity faces.
        let root_at = Transform::from_translation(Vec3::from(pos))
            .with_rotation(Quat::from_rotation_y((-yaw).to_radians()));
        if let Some(e) = live.entities.get(id) {
            if let Ok(mut t) = roots.get_mut(e.root) {
                *t = root_at;
            }
            for part in e.parts.iter().filter(|p| p.animation != Animation::None) {
                if let Ok(mut t) = part_transforms.get_mut(part.entity) {
                    *t = place([0; 3], part.transform, &part.animation, ticks)
                        .mul_transform(part.display);
                }
            }
            continue;
        }
        let size = crate::entities::modded::size(name).unwrap_or(crate::entities::modded::FALLBACK);
        let entity_view = torch_mod_api::EntityView {
            kind: (*name).to_owned(),
            id: *id,
            width: size.width,
            height: size.height,
            time: view.game_time as i64,
            data: super::entity_data(*id),
        };
        let rig = match super::entity(&entity_view) {
            Some(Ok(rig)) => rig,
            Some(Err(e)) => {
                crate::log_warn!("modapi", "{name} #{id}: {e}");
                torch_mod_api::Rig::default()
            }
            None => continue,
        };
        let root = commands
            .spawn((root_at, Visibility::Inherited, ModEntityRoot))
            .id();
        let block = pos.map(|v| v.floor() as i32);
        let levels =
            crate::lighting::lightmap::resolved_levels(light_map.at(block[0], block[1], block[2]));
        let cell = live.lights.get(&lightmap.current, levels.0, levels.1);
        let bright = live.lights.get(&lightmap.current, 15, 15);
        let parts = spawn_parts(
            &mut commands,
            live,
            Assets3 {
                gui: gui.as_deref(),
                meshes: &mut meshes,
                materials: &mut materials,
                standard: &mut standard,
                images: &mut images,
                items: item_assets.as_deref_mut(),
            },
            &atlas,
            (levels, cell, bright, dirs),
            &rig.parts,
            ([0; 3], Some(root)),
            ticks,
        );
        live.entities.insert(*id, LiveEntity { root, parts });
    }
}

/// The assets parts are built from.
struct Assets3<'a> {
    /// The font, for text parts.
    gui: Option<&'a crate::gui::render::GuiAssets>,
    meshes: &'a mut Assets<Mesh>,
    materials: &'a mut Assets<EntityMaterial>,
    standard: &'a mut Assets<crate::draw::SimpleMaterial>,
    images: &'a mut Assets<Image>,
    items: Option<&'a mut crate::scene::item_assets::ItemAssets>,
}

/// Spawns a rig's parts, lit at `light` (levels, cell, directions), at
/// `origin` (a block's corner) or, with a parent, in the parent's frame.
fn spawn_parts(
    commands: &mut Commands,
    live: &mut Live,
    mut a: Assets3,
    atlas: &Handle<Image>,
    light: (
        (u8, u8),
        crate::lighting::lightmap::CellLight,
        crate::lighting::lightmap::CellLight,
        [Vec3; 2],
    ),
    rig_parts: &[Part],
    (origin, parent): ([i32; 3], Option<Entity>),
    ticks: f32,
) -> Vec<LivePart> {
    // The light at the part, and full bright (glowing text).
    let (levels, cell, bright, dirs) = light;
    let lit = Lit {
        tint: [1.0; 4],
        light: cell,
        dirs,
        overlay_alpha: 1.0,
        overlay_white: false,
        glint: false,
    };
    let material = live
        .materials
        .entry(levels)
        .or_insert_with(|| {
            a.materials.add(EntityMaterial::new(
                Some(atlas.clone()),
                LightMode::Cardinal,
                crate::draw::Alpha::Mask(0.1),
                0.1,
                lit,
                0,
            ))
        })
        .clone();
    let mut parts = Vec::with_capacity(rig_parts.len());
    for part in rig_parts {
        let step = part.scroll.as_ref().and_then(|s| scroll_step(s, ticks));
        let (mesh, material, transform, display) = match &part.geometry {
            Geometry::Text {
                lines,
                color,
                shadow,
                glowing,
            } => {
                let Some(gui) = a.gui else {
                    continue;
                };
                let spans: Vec<_> = lines
                    .iter()
                    .map(|l| crate::text::parse_formatted(l))
                    .collect();
                let Some(mesh) =
                    crate::blockentities::text::text_mesh(&gui.atlas, &spans, *color, *shadow)
                else {
                    continue;
                };
                // Glowing text is lit as full bright.
                let text_levels = if *glowing { (15, 15) } else { levels };
                let text_lit = if *glowing {
                    Lit {
                        light: bright,
                        ..lit
                    }
                } else {
                    lit
                };
                let texture = gui.image.clone();
                let material = live
                    .item_materials
                    .entry((text_levels.0, text_levels.1, texture.id()))
                    .or_insert_with(|| {
                        a.materials.add(EntityMaterial::new(
                            Some(texture),
                            LightMode::Cardinal,
                            crate::draw::Alpha::Mask(0.1),
                            0.1,
                            text_lit,
                            0,
                        ))
                    })
                    .clone();
                // Font pixels to sixteenths, y up as a sign's.
                let display = Transform::from_scale(Vec3::new(1.0 / 16.0, -1.0 / 16.0, 1.0 / 16.0));
                (a.meshes.add(mesh), material, part.transform, display)
            }
            Geometry::Item { item, flat } => {
                let Some(items) = a.items.as_deref_mut() else {
                    continue;
                };
                let id = item.strip_prefix("minecraft:").unwrap_or(item);
                let Some(gpu) = items.get(id, "fixed", false, a.meshes, a.images, a.standard)
                else {
                    continue;
                };
                let texture = gpu.texture.clone();
                let material = live
                    .item_materials
                    .entry((levels.0, levels.1, texture.id()))
                    .or_insert_with(|| {
                        a.materials.add(EntityMaterial::new(
                            Some(texture),
                            LightMode::Cardinal,
                            crate::draw::Alpha::Mask(0.1),
                            0.1,
                            lit,
                            0,
                        ))
                    })
                    .clone();
                let transform = match flat {
                    Some(flat) if gpu.flat => *flat,
                    _ => part.transform,
                };
                let display = crate::entities::item_display_transform(&gpu.transform("fixed"));
                (gpu.mesh.clone(), material, transform, display)
            }
            _ => {
                let Some(mesh) = part_mesh(&mut live.meshes, a.meshes, part, step) else {
                    continue;
                };
                (mesh, material.clone(), part.transform, Transform::IDENTITY)
            }
        };
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                crate::draw::Material3d(material),
                place(origin, transform, &part.animation, ticks).mul_transform(display),
                Visibility::Inherited,
                ModPart,
            ))
            .id();
        if let Some(parent) = parent {
            commands.entity(parent).add_child(entity);
        }
        parts.push(LivePart {
            entity,
            transform,
            display,
            animation: part.animation.clone(),
            scroll: step.map(|step| (part.clone(), step)),
        });
    }
    parts
}

/// A sixteenths point as blocks.
fn blocks(v: [f32; 3]) -> Vec3 {
    Vec3::from(v) / 16.0
}

/// About `pivot` (blocks): `inner` applied there.
fn about(pivot: Vec3, inner: Affine3A) -> Affine3A {
    Affine3A::from_translation(pivot) * inner * Affine3A::from_translation(-pivot)
}

/// The animation's motion at `ticks`.
fn motion(animation: &Animation, ticks: f32) -> Affine3A {
    let turn = |axis: [f32; 3], pivot: [f32; 3], degrees: f32| {
        let axis = Vec3::from(axis).try_normalize().unwrap_or(Vec3::Y);
        about(
            blocks(pivot),
            Affine3A::from_axis_angle(axis, degrees.to_radians()),
        )
    };
    match animation {
        Animation::None => Affine3A::IDENTITY,
        Animation::Spin {
            axis,
            pivot,
            degrees_per_tick,
            phase,
        } => turn(*axis, *pivot, (phase + degrees_per_tick * ticks) % 360.0),
        Animation::Swing {
            axis,
            pivot,
            amplitude,
            period_ticks,
            phase,
        } => {
            let t = std::f32::consts::TAU * ticks / period_ticks.max(1e-3) + phase;
            turn(*axis, *pivot, amplitude * t.sin())
        }
        Animation::Slide {
            offset,
            period_ticks,
            phase,
        } => {
            let t = std::f32::consts::TAU * ticks / period_ticks.max(1e-3) + phase;
            Affine3A::from_translation(blocks(*offset) * (0.5 - 0.5 * t.cos()))
        }
        Animation::Path { points } => Affine3A::from_translation(blocks(path_at(points, ticks))),
    }
}

/// Where a path is at `ticks`: straight between its points, held past
/// either end.
fn path_at(points: &[[f32; 4]], ticks: f32) -> [f32; 3] {
    let at = |p: &[f32; 4]| [p[1], p[2], p[3]];
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return [0.0; 3];
    };
    if ticks <= first[0] {
        return at(first);
    }
    for leg in points.windows(2) {
        let (a, b) = (&leg[0], &leg[1]);
        if ticks < b[0] {
            let t = ((ticks - a[0]) / (b[0] - a[0]).max(1e-6)).clamp(0.0, 1.0);
            return std::array::from_fn(|i| a[i + 1] + (b[i + 1] - a[i + 1]) * t);
        }
    }
    at(last)
}

/// A part's world transform: the block's corner, then its animation, then
/// its static transform (scale and rotate about the pivot, translate).
fn place(
    pos: [i32; 3],
    t: torch_mod_api::Transform,
    animation: &Animation,
    ticks: f32,
) -> Transform {
    let pivot = blocks(t.pivot);
    let rotation = Quat::from_rotation_z(t.rotate[2].to_radians())
        * Quat::from_rotation_y(t.rotate[1].to_radians())
        * Quat::from_rotation_x(t.rotate[0].to_radians());
    let local = Affine3A::from_translation(blocks(t.translate))
        * about(
            pivot,
            Affine3A::from_rotation_translation(rotation, Vec3::ZERO)
                * Affine3A::from_scale(Vec3::from(t.scale)),
        );
    let corner = Affine3A::from_translation(Vec3::new(pos[0] as f32, pos[1] as f32, pos[2] as f32));
    Transform::from_matrix(Mat4::from(corner * motion(animation, ticks) * local))
}

/// The mesh for a part's geometry (at a scroll step), built once per
/// distinct one.
fn part_mesh(
    cache: &mut HashMap<String, Option<Handle<Mesh>>>,
    meshes: &mut Assets<Mesh>,
    part: &Part,
    step: Option<u32>,
) -> Option<Handle<Mesh>> {
    let mut key = serde_json::to_string(&part.geometry).ok()?;
    let scroll = part.scroll.as_ref().zip(step);
    if let Some((s, step)) = scroll {
        key.push_str(&format!("|{}>{}@{step}", s.from, s.to));
    }
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let built = geometry_quads(&part.geometry)
        .map(|(quads, tint)| match scroll {
            Some((s, step)) => (scrolled(&quads, s, step).unwrap_or(quads), tint),
            None => (quads, tint),
        })
        .and_then(|(quads, tint)| crate::entities::blockmodel::mesh_of(&quads, tint))
        .map(|mesh| meshes.add(mesh));
    cache.insert(key, built.clone());
    built
}

/// A sprite's atlas rect, by its id.
fn sprite_rect(id: &str) -> Option<[f32; 4]> {
    let rows = crate::ATLAS_ROWS.get().copied()?;
    let sprite = crate::items::model::strip_namespace(id);
    let tile = crate::TEXTURE_MAP
        .get()?
        .get(crate::blocks::bake::sprite_key(sprite))
        .copied()?;
    Some(crate::blocks::bake::tile_uv_rect(tile, rows))
}

/// A scroll target's tiles, top first: the sprite, then the squares the
/// atlas keeps under it (`<id>@<k>`, `scene::atlas::tall_pieces`).
fn scroll_tiles(to: &str) -> Vec<[f32; 4]> {
    let mut tiles: Vec<[f32; 4]> = sprite_rect(to).into_iter().collect();
    while let Some(rect) = sprite_rect(&format!("{to}@{}", tiles.len())) {
        tiles.push(rect);
    }
    tiles
}

/// The scroll's offset at `ticks`, in steps down its target; `None` when
/// the target isn't in the atlas.
fn scroll_step(scroll: &Scroll, ticks: f32) -> Option<u32> {
    let tiles = scroll_tiles(&scroll.to).len() as f32;
    if tiles == 0.0 {
        return None;
    }
    let cycle = (scroll.per_tick * ticks + scroll.phase).rem_euclid(1.0);
    let steps = tiles * SCROLL_STEPS;
    Some(((cycle * scroll.span * steps).round() as u32) % steps as u32)
}

/// `quads` with the texels of `scroll.from` moved `step` down `scroll.to`.
fn scrolled(quads: &[BakedQuad], scroll: &Scroll, step: u32) -> Option<Vec<BakedQuad>> {
    let from = sprite_rect(&scroll.from)?;
    let tiles = scroll_tiles(&scroll.to);
    (!tiles.is_empty()).then(|| shift_quads(quads, from, &tiles, step as f32 / SCROLL_STEPS))
}

/// Each quad on `from` mapped `shift` tiles down `tiles` (top first), cut
/// where it crosses a tile's edge; past the last tile it wraps.
fn shift_quads(
    quads: &[BakedQuad],
    from: [f32; 4],
    tiles: &[[f32; 4]],
    shift: f32,
) -> Vec<BakedQuad> {
    const EPS: f32 = 1e-4;
    let n = tiles.len() as f32;
    // A corner's place in `from`, 0..1 each way.
    let inside = |uv: [f32; 2]| {
        let s = (uv[0] - from[0]) / (from[2] - from[0]);
        let t = (uv[1] - from[1]) / (from[3] - from[1]);
        let fits = |x: f32| (-EPS..=1.0 + EPS).contains(&x);
        (fits(s) && fits(t)).then_some([s, t])
    };
    let mut out = Vec::with_capacity(quads.len());
    for q in quads {
        let st: Option<Vec<[f32; 2]>> = q.uv.iter().map(|uv| inside(*uv)).collect();
        let Some(st) = st else {
            out.push(q.clone());
            continue;
        };
        let st = [st[0], st[1], st[2], st[3]];
        // `t` runs along a (corner 0 to 3) or b (corner 0 to 1).
        let (da, db) = (st[3][1] - st[0][1], st[1][1] - st[0][1]);
        let along_a = da.abs() >= db.abs();
        let dt = if along_a { da } else { db };
        let flat = dt.abs() < EPS;
        let (lo, hi) = (
            st[0][1].min(st[0][1] + dt) + shift,
            st[0][1].max(st[0][1] + dt) + shift,
        );
        let mut cuts = vec![lo];
        let mut edge = lo.floor() + 1.0;
        while edge < hi - EPS {
            cuts.push(edge);
            edge += 1.0;
        }
        cuts.push(hi);
        for w in cuts.windows(2) {
            let tile = ((w[0] + w[1]) * 0.5).floor();
            let rect = tiles[tile.rem_euclid(n) as usize];
            let corners: [[f32; 2]; 4] = if flat {
                [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
            } else {
                let at = |t: f32| (t - shift - st[0][1]) / dt;
                let (p0, p1) = (at(w[0]).min(at(w[1])), at(w[0]).max(at(w[1])));
                if along_a {
                    [[p0, 0.0], [p0, 1.0], [p1, 1.0], [p1, 0.0]]
                } else {
                    [[0.0, p0], [0.0, p1], [1.0, p1], [1.0, p0]]
                }
            };
            let mut piece = q.clone();
            for (i, [a, b]) in corners.into_iter().enumerate() {
                piece.pos[i] = bilerp(&q.pos, a, b);
                let [s, t] = bilerp(&st, a, b);
                let local = (t + shift - tile).clamp(0.0, 1.0);
                piece.uv[i] = [
                    rect[0] + s * (rect[2] - rect[0]),
                    rect[1] + local * (rect[3] - rect[1]),
                ];
            }
            out.push(piece);
        }
    }
    out
}

/// A point on a quad, corners 0..3 at (a, b) = (0,0) (0,1) (1,1) (1,0).
fn bilerp<const N: usize>(c: &[[f32; N]; 4], a: f32, b: f32) -> [f32; N] {
    std::array::from_fn(|i| {
        let near = c[0][i] + (c[3][i] - c[0][i]) * a;
        let far = c[1][i] + (c[2][i] - c[1][i]) * a;
        near + (far - near) * b
    })
}

/// A geometry's quads (in blocks, 0..1 inside its cell) and the colour
/// they're multiplied by.
fn geometry_quads(geometry: &Geometry) -> Option<(Vec<BakedQuad>, [f32; 4])> {
    let rows = crate::ATLAS_ROWS.get().copied()?;
    let white = [1.0; 4];
    match geometry {
        Geometry::Model {
            model,
            x,
            y,
            uvlock,
        } => {
            let rot = crate::blocks::state::Rot {
                x: x % 4,
                y: y % 4,
                z: 0,
                uvlock: *uvlock,
            };
            let path = crate::items::model::strip_namespace(model);
            let baked =
                crate::blocks::bake::bake_model(crate::items::model::shared(), path, rot, rows);
            (!baked.quads.is_empty()).then(|| (baked.quads.clone(), white))
        }
        Geometry::Block { block, props } => {
            let name = crate::items::model::strip_namespace(block);
            let props: HashMap<&str, &str> = props
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let baked = crate::blocks::registry().baked_by_name(name, &props);
            let quads: Vec<BakedQuad> = baked
                .parts
                .iter()
                .filter_map(|slot| slot.choices.first())
                .flat_map(|m| m.quads.iter().cloned())
                .collect();
            (!quads.is_empty()).then_some((quads, white))
        }
        Geometry::Cuboid {
            from,
            to,
            texture,
            faces,
            tint,
            uv,
        } => {
            let sprite = crate::items::model::strip_namespace(texture);
            let tile = crate::TEXTURE_MAP
                .get()?
                .get(crate::blocks::bake::sprite_key(sprite))
                .copied()?;
            let [u0, v0, u1, v1] = crate::blocks::bake::tile_uv_rect(tile, rows);
            let rect = uv.unwrap_or([0.0, 0.0, 16.0, 16.0]);
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * (t / 16.0);
            let (fu0, fv0, fu1, fv1) = (
                lerp(u0, u1, rect[0]),
                lerp(v0, v1, rect[1]),
                lerp(u0, u1, rect[2]),
                lerp(v0, v1, rect[3]),
            );
            let (a, b) = (blocks(*from), blocks(*to));
            let mut quads = Vec::new();
            for dir in 0..6 {
                if faces & (1 << dir) == 0 {
                    continue;
                }
                let pos = crate::items::model::face_positions(a.to_array(), b.to_array(), dir);
                let uvs = [[fu0, fv0], [fu0, fv1], [fu1, fv1], [fu1, fv0]];
                let d = crate::blocks::rotation::DIR_VEC[dir];
                quads.push(BakedQuad {
                    pos,
                    uv: uvs,
                    normal: [d[0] as f32, d[1] as f32, d[2] as f32],
                    face: dir as u8,
                    cull: None,
                    shaded: true,
                    tint: -1,
                    opaque: false,
                });
            }
            let argb = *tint;
            let channel = |shift: u32| ((argb >> shift) & 0xFF) as f32 / 255.0;
            Some((quads, [channel(16), channel(8), channel(0), channel(24)]))
        }
        // Drawn from the item's own mesh or the font, not block quads.
        Geometry::Item { .. } | Geometry::Text { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_runs_its_legs_and_holds_at_the_ends() {
        let points = [
            [100.0, 0.0, 0.0, 0.0],
            [110.0, 16.0, 0.0, 0.0],
            [120.0, 16.0, 8.0, 0.0],
        ];
        assert_eq!(path_at(&points, 50.0), [0.0, 0.0, 0.0]);
        assert_eq!(path_at(&points, 105.0), [8.0, 0.0, 0.0]);
        assert_eq!(path_at(&points, 115.0), [16.0, 4.0, 0.0]);
        assert_eq!(path_at(&points, 500.0), [16.0, 8.0, 0.0]);
        assert_eq!(path_at(&[], 1.0), [0.0; 3]);
    }

    #[test]
    fn a_spin_turns_about_its_pivot() {
        let spin = Animation::Spin {
            axis: [0.0, 1.0, 0.0],
            pivot: [8.0, 8.0, 8.0],
            degrees_per_tick: 9.0,
            phase: 0.0,
        };
        // 10 ticks: a quarter turn about the block's centre line.
        let t = place(
            [10, 64, 0],
            torch_mod_api::Transform::default(),
            &spin,
            10.0,
        );
        let corner = t.transform_point(Vec3::ZERO);
        assert!(
            (corner - Vec3::new(10.0, 64.0, 1.0)).length() < 1e-4,
            "{corner}"
        );
        let centre = t.transform_point(Vec3::splat(0.5));
        assert!((centre - Vec3::new(10.5, 64.5, 0.5)).length() < 1e-4);
    }

    #[test]
    fn a_static_part_sits_in_its_block() {
        let t = place(
            [1, 2, 3],
            torch_mod_api::Transform::default(),
            &Animation::None,
            50.0,
        );
        assert!((t.transform_point(Vec3::ZERO) - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-5);
    }

    /// A unit quad whose `t` runs with b (corner 0 to 1), `v[0]` to `v[1]`.
    fn quad(v: [f32; 2]) -> BakedQuad {
        BakedQuad {
            pos: [
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
            ],
            uv: [[0.0, v[0]], [0.0, v[1]], [1.0, v[1]], [1.0, v[0]]],
            normal: [0.0, 0.0, 1.0],
            face: 3,
            cull: None,
            shaded: true,
            tint: -1,
            opaque: false,
        }
    }

    #[test]
    fn a_scroll_cuts_a_quad_at_the_tile_edge() {
        let from = [0.0, 0.0, 1.0, 1.0];
        let tiles = [[10.0, 0.0, 11.0, 1.0], [10.0, 1.0, 11.0, 2.0]];
        // A quarter down a two-tile target: three quarters in tile 0, a
        // quarter in tile 1.
        let out = shift_quads(&[quad([0.0, 1.0])], from, &tiles, 0.25);
        assert_eq!(out.len(), 2);
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(near(out[0].uv[0][1], 0.25) && near(out[0].uv[1][1], 1.0));
        assert!(near(out[0].pos[1][1], 0.75));
        assert!(near(out[1].uv[0][1], 1.0) && near(out[1].uv[1][1], 1.25));
        assert!(near(out[1].pos[0][1], 0.75) && near(out[1].pos[1][1], 1.0));
        assert!(out.iter().all(|q| q.uv.iter().all(|uv| uv[0] >= 10.0)));
        // A quad on another sprite stays.
        let other = BakedQuad {
            uv: [[5.0, 5.0]; 4],
            ..quad([0.0, 1.0])
        };
        assert_eq!(
            shift_quads(&[other], from, &tiles, 0.25)[0].uv,
            [[5.0, 5.0]; 4]
        );
    }

    #[test]
    fn a_scroll_past_the_end_wraps_to_the_top() {
        let from = [0.0, 0.0, 1.0, 1.0];
        let tiles = [[0.0, 0.0, 1.0, 1.0], [0.0, 1.0, 1.0, 2.0]];
        // `t` flipped, a tile and a half down: half in tile 1, half back in
        // tile 0.
        let out = shift_quads(&[quad([1.0, 0.0])], from, &tiles, 1.5);
        assert_eq!(out.len(), 2);
        let v: Vec<f32> = out.iter().flat_map(|q| q.uv.map(|uv| uv[1])).collect();
        assert!(v.iter().all(|v| (0.0..=2.0).contains(v)), "{v:?}");
        // Corner 0 (t = 1, so 2.5 down) sits in tile 0 at half.
        let first = out.iter().find(|q| q.pos[0][1] == 0.0).unwrap();
        assert!((first.uv[0][1] - 0.5).abs() < 1e-5, "{:?}", first.uv);
    }

    #[test]
    fn a_slide_goes_out_and_back() {
        let slide = Animation::Slide {
            offset: [0.0, 16.0, 0.0],
            period_ticks: 20.0,
            phase: 0.0,
        };
        assert!(motion(&slide, 0.0).translation.length() < 1e-5);
        assert!((motion(&slide, 10.0).translation.y - 1.0).abs() < 1e-5);
    }
}
