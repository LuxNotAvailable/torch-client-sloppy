//! Draws plugins' rigs: each part a mesh (a block model, a whole block's
//! model, or a cuboid) lit like a block entity at its position, moved every
//! frame by its animation. Parts are ordinary `EntityMaterial` meshes, so
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
        sync_rigs
            .after(crate::app::FrameViewSystems)
            .run_if(in_state(crate::app::AppState::InGame)),
    );
}

#[derive(Component)]
struct ModPart;

struct LivePart {
    entity: Entity,
    transform: torch_mod_api::Transform,
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
    atlas: Option<Handle<Image>>,
    lights: crate::lighting::lightmap::LightCells,
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
    standard: Res<Assets<crate::draw::SimpleMaterial>>,
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
            let material = live
                .materials
                .entry(levels)
                .or_insert_with(|| {
                    materials.add(EntityMaterial::new(
                        Some(atlas.clone()),
                        LightMode::Cardinal,
                        crate::draw::Alpha::Mask(0.1),
                        0.1,
                        Lit {
                            tint: [1.0; 4],
                            light: cell,
                            dirs,
                            overlay_alpha: 1.0,
                            overlay_white: false,
                            glint: false,
                        },
                        0,
                    ))
                })
                .clone();
            let mut parts = Vec::with_capacity(rig.parts.len());
            for part in &rig.parts {
                let step = part.scroll.as_ref().and_then(|s| scroll_step(s, ticks));
                let Some(mesh) = part_mesh(&mut live.meshes, &mut meshes, part, step) else {
                    continue;
                };
                let entity = commands
                    .spawn((
                        Mesh3d(mesh),
                        crate::draw::Material3d(material.clone()),
                        place(*pos, part.transform, part.animation, ticks),
                        Visibility::Inherited,
                        ModPart,
                    ))
                    .id();
                parts.push(LivePart {
                    entity,
                    transform: part.transform,
                    animation: part.animation,
                    scroll: step.map(|step| (part.clone(), step)),
                });
            }
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
                *t = place(*pos, part.transform, part.animation, ticks);
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
fn motion(animation: Animation, ticks: f32) -> Affine3A {
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
        } => turn(axis, pivot, (phase + degrees_per_tick * ticks) % 360.0),
        Animation::Swing {
            axis,
            pivot,
            amplitude,
            period_ticks,
            phase,
        } => {
            let t = std::f32::consts::TAU * ticks / period_ticks.max(1e-3) + phase;
            turn(axis, pivot, amplitude * t.sin())
        }
        Animation::Slide {
            offset,
            period_ticks,
            phase,
        } => {
            let t = std::f32::consts::TAU * ticks / period_ticks.max(1e-3) + phase;
            Affine3A::from_translation(blocks(offset) * (0.5 - 0.5 * t.cos()))
        }
    }
}

/// A part's world transform: the block's corner, then its animation, then
/// its static transform (scale and rotate about the pivot, translate).
fn place(
    pos: [i32; 3],
    t: torch_mod_api::Transform,
    animation: Animation,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spin_turns_about_its_pivot() {
        let spin = Animation::Spin {
            axis: [0.0, 1.0, 0.0],
            pivot: [8.0, 8.0, 8.0],
            degrees_per_tick: 9.0,
            phase: 0.0,
        };
        // 10 ticks: a quarter turn about the block's centre line.
        let t = place([10, 64, 0], torch_mod_api::Transform::default(), spin, 10.0);
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
            Animation::None,
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
        assert!(motion(slide, 0.0).translation.length() < 1e-5);
        assert!((motion(slide, 10.0).translation.y - 1.0).abs() < 1e-5);
    }
}
