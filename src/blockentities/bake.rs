//! Static block entities baked into the terrain mesh, after BetterBlockEntities.
//!
//! A closed chest or shulker box is plain geometry, so its model goes into the
//! section mesh like any block: no rig, no per-frame transforms, culled and
//! occluded with the terrain. While one animates, its position is marked live;
//! the mesher leaves it out and the regular rig draws it until the lid settles
//! and a fresh section mesh has the baked copy back.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use azalea::block::BlockState;
use azalea_registry::builtin::BlockEntityKind;
use bevy::math::{Mat3, Mat4, Vec3};

use super::feed::{BlockEntityData, BlockStateInfo};
use super::{BeGeom, BeRegistry, BeState};
use crate::blocks::BakedQuad;
use crate::entities::Blend;
use crate::platform::time::Instant;

const BAKED: [BlockEntityKind; 4] = [
    BlockEntityKind::Chest,
    BlockEntityKind::TrappedChest,
    BlockEntityKind::EnderChest,
    BlockEntityKind::ShulkerBox,
];

/// Entity texture directories packed into the block atlas for baking.
pub const TEXTURE_DIRS: [&str; 2] = ["entity/chest", "entity/shulker"];

/// A rig keeps drawing this long after it asked for its baked copy back, so a
/// mesh job already in flight cannot leave a frame with neither.
const HANDOVER: std::time::Duration = std::time::Duration::from_millis(100);

type Regions = HashMap<String, [f32; 4]>;

static REGIONS: RwLock<Option<Arc<Regions>>> = RwLock::new(None);
static QUADS: RwLock<Option<HashMap<u16, Option<Arc<[BakedQuad]>>>>> = RwLock::new(None);
static LIVE: Mutex<Option<HashSet<[i32; 3]>>> = Mutex::new(None);
static HANDING_BACK: Mutex<Vec<([i32; 3], Instant)>> = Mutex::new(Vec::new());

fn registry() -> &'static BeRegistry {
    static REGISTRY: OnceLock<BeRegistry> = OnceLock::new();
    REGISTRY.get_or_init(BeRegistry::build)
}

/// Where each packed entity texture (`entity/chest/normal`) sits in the block
/// atlas, as a `[u0, v0, u1, v1]` rect. Replacing it drops every baked model.
pub fn set_regions(regions: Regions) {
    *REGIONS.write().unwrap() = Some(Arc::new(regions));
    *QUADS.write().unwrap() = None;
}

/// Whether blocks in this state may be baked: a kind on the list, with its
/// textures in the atlas. Cached per state, as the mesher asks for every block.
pub fn bakes(state: u16) -> bool {
    if cfg!(target_arch = "wasm32") {
        // The web mesher runs in a worker that cannot see the live set.
        return false;
    }
    const UNKNOWN: u8 = 0;
    const NO: u8 = 1;
    const YES: u8 = 2;
    const STATES: usize = BlockState::MAX_STATE as usize + 1;
    static KIND: [AtomicU8; STATES] = [const { AtomicU8::new(UNKNOWN) }; STATES];
    let Some(slot) = KIND.get(state as usize) else {
        return false;
    };
    let known = match slot.load(Ordering::Relaxed) {
        UNKNOWN => {
            let yes = BlockState::try_from(state as u32).ok().is_some_and(|s| {
                let block = crate::blocks::info::info(s);
                super::feed::kind_for_block(block.id).is_some_and(|k| BAKED.contains(&k))
            });
            slot.store(if yes { YES } else { NO }, Ordering::Relaxed);
            yes
        }
        v => v == YES,
    };
    known && REGIONS.read().unwrap().is_some()
}

/// The baked quads for a block entity at `pos`, block-local, or `None` when it
/// draws as a rig instead.
pub fn quads_at(state: u16, pos: [i32; 3]) -> Option<Arc<[BakedQuad]>> {
    if !bakes(state) || is_live(pos) {
        return None;
    }
    if let Some(cached) = QUADS.read().unwrap().as_ref().and_then(|m| m.get(&state)) {
        return cached.clone();
    }
    let regions = REGIONS.read().unwrap().clone()?;
    let built = build(state, &regions).map(Arc::from);
    QUADS
        .write()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(state, built.clone());
    built
}

fn is_live(pos: [i32; 3]) -> bool {
    LIVE.lock()
        .unwrap()
        .as_ref()
        .is_some_and(|live| live.contains(&pos))
}

/// Whether the rig at `pos` should draw this frame. Moving block entities go
/// live (out of the mesh); settled ones hand back to the mesh, the rig staying
/// up until a rebuild of their section lands.
pub fn rig_wanted(state: u16, pos: [i32; 3], moving: bool) -> bool {
    if !bakes(state) {
        return true;
    }
    let mut live = LIVE.lock().unwrap();
    let live = live.get_or_insert_with(HashSet::new);
    let mut back = HANDING_BACK.lock().unwrap();
    if moving {
        back.retain(|(p, _)| *p != pos);
        if live.insert(pos) {
            crate::client::worldsync::remesh_block(pos);
        }
        return true;
    }
    if live.remove(&pos) {
        back.push((pos, Instant::now()));
        crate::client::worldsync::remesh_block(pos);
        return true;
    }
    back.iter().any(|(p, _)| *p == pos)
}

/// A mesh for the section at `origin` landed: block entities handed back in it
/// are drawn by the terrain again.
pub fn section_rebuilt(origin: [i32; 3]) {
    let mut back = HANDING_BACK.lock().unwrap();
    if back.is_empty() {
        return;
    }
    back.retain(|(p, at)| {
        let section = [
            p[0].div_euclid(16) * 16,
            p[1].div_euclid(16) * 16,
            p[2].div_euclid(16) * 16,
        ];
        section != origin || at.elapsed() < HANDOVER
    });
}

pub fn clear() {
    *LIVE.lock().unwrap() = None;
    HANDING_BACK.lock().unwrap().clear();
}

/// Poses every model layer of the block entity at rest and flattens it into
/// block quads with atlas UVs. `None` if any layer cannot be baked.
fn build(state: u16, regions: &Regions) -> Option<Vec<BakedQuad>> {
    let block_state = BlockState::try_from(state as u32).ok()?;
    let block = crate::blocks::info::info(block_state);
    let kind = super::feed::kind_for_block(block.id)?;
    let props = block
        .map()
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    let st = BeState {
        pos: [0; 3],
        kind,
        state: Arc::new(BlockStateInfo::new(state, block.id.to_owned(), props)),
        data: Arc::new(BlockEntityData::None),
        anim: super::sim::Anim::new(kind, &BlockEntityData::None),
        partial: 0.0,
        game_time: 0,
        camera: Default::default(),
        open: 0.0,
        draw_outline: false,
    };

    let registry = registry();
    let mut out = Vec::new();
    let mut posed = Vec::new();
    for &idx in registry.specs_for(kind) {
        let spec = &registry.specs[idx];
        let BeGeom::Model(geom) = &spec.geom else {
            return None;
        };
        if !matches!(spec.blend, Blend::Cutout) {
            return None;
        }
        let [u0, v0, u1, v1] = *regions.get(&(geom.texture)(&st))?;
        let model = crate::entities::geom::bake(&(geom.layer)(&st));
        let mut states = model.rest_pose();
        (geom.setup)(&model, &mut states, &st);
        model.pose_parts(&states, (spec.transform)(&st).to_matrix(), &mut posed);
        for ((part, at), state) in model.parts.iter().zip(&posed).zip(&states) {
            if !at.visible || state.skip_draw {
                continue;
            }
            let Some(arrays) = part.arrays() else {
                continue;
            };
            let atlas = |uv: [f32; 2]| [u0 + uv[0] * (u1 - u0), v0 + uv[1] * (v1 - v0)];
            push_part(&mut out, at.mat, &arrays, atlas);
        }
    }
    Some(out)
}

fn push_part(
    out: &mut Vec<BakedQuad>,
    mat: Mat4,
    arrays: &crate::entities::geom::PartArrays<'_>,
    atlas: impl Fn([f32; 2]) -> [f32; 2],
) {
    let normal_mat = Mat3::from_mat4(mat).inverse().transpose();
    let flipped = mat.determinant() < 0.0;
    for quad in arrays.indices.chunks_exact(6) {
        let corners = [quad[0], quad[1], quad[2], quad[5]].map(|i| i as usize);
        let mut pos = corners.map(|i| mat.transform_point3(Vec3::from(arrays.positions[i])));
        let mut uv = corners.map(|i| atlas(arrays.uvs[i]));
        if flipped {
            pos.reverse();
            uv.reverse();
        }
        let normal = (normal_mat * Vec3::from(arrays.normals[corners[0]])).normalize_or_zero();
        out.push(BakedQuad {
            pos: pos.map(|p| p.to_array()),
            uv,
            normal: normal.to_array(),
            face: nearest_face(normal),
            cull: None,
            shaded: true,
            tint: -1,
            opaque: false,
        });
    }
}

/// Block face index (down, up, north, south, west, east) closest to `n`.
fn nearest_face(n: Vec3) -> u8 {
    let a = n.abs();
    if a.y >= a.x && a.y >= a.z {
        if n.y < 0.0 { 0 } else { 1 }
    } else if a.z >= a.x {
        if n.z < 0.0 { 2 } else { 3 }
    } else if n.x < 0.0 {
        4
    } else {
        5
    }
}

/// Square entity textures to pack, keyed by their path under `textures/`.
pub fn textures(textures_dir: &str) -> Vec<(String, image::RgbaImage)> {
    let mut out = Vec::new();
    for dir in TEXTURE_DIRS {
        let path = format!("{textures_dir}/{dir}");
        for file in crate::platform::assets::read_dir(&path) {
            if file.extension().and_then(|e| e.to_str()) != Some("png") {
                continue;
            }
            let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(image) = crate::platform::assets::read(&file)
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|i| i.to_rgba8())
            else {
                continue;
            };
            if image.width() == image.height() {
                out.push((format!("{dir}/{stem}"), image));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// UV of `pixel` (in a `side`-pixel texture) inside its atlas rect.
#[cfg(test)]
fn region_uv(rect: [f32; 4], pixel: bevy::math::Vec2, side: f32) -> bevy::math::Vec2 {
    bevy::math::Vec2::new(
        rect[0] + pixel.x / side * (rect[2] - rect[0]),
        rect[1] + pixel.y / side * (rect[3] - rect[1]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    #[test]
    fn faces_follow_the_block_order() {
        assert_eq!(nearest_face(Vec3::NEG_Y), 0);
        assert_eq!(nearest_face(Vec3::Y), 1);
        assert_eq!(nearest_face(Vec3::NEG_Z), 2);
        assert_eq!(nearest_face(Vec3::Z), 3);
        assert_eq!(nearest_face(Vec3::NEG_X), 4);
        assert_eq!(nearest_face(Vec3::X), 5);
    }

    #[test]
    fn a_closed_chest_bakes_into_its_block_with_atlas_uvs() {
        let rect = [0.5, 0.25, 0.75, 0.5];
        let regions: Regions = [
            ("entity/chest/normal".to_owned(), rect),
            ("entity/chest/christmas".to_owned(), rect),
        ]
        .into();
        let state = BlockState::from(azalea_registry::builtin::BlockKind::Chest).id();
        let quads = build(state, &regions).expect("a chest bakes");
        assert!(!quads.is_empty());
        for q in &quads {
            for p in q.pos {
                assert!(
                    p.iter().all(|v| (-0.01..=1.01).contains(v)),
                    "{p:?} leaves the block"
                );
            }
            for uv in q.uv {
                assert!(
                    (rect[0]..=rect[2]).contains(&uv[0]) && (rect[1]..=rect[3]).contains(&uv[1])
                );
            }
        }
        let top = quads.iter().filter(|q| q.face == 1).count();
        assert!(top >= 2, "the lid and the knob have tops");
        assert_eq!(
            region_uv(rect, Vec2::new(32.0, 32.0), 64.0),
            Vec2::new(0.625, 0.375)
        );
    }

    #[test]
    fn a_lid_that_moves_goes_live_and_hands_back_after_a_rebuild() {
        clear();
        set_regions(Regions::new());
        let state = BlockState::from(azalea_registry::builtin::BlockKind::Chest).id();
        let pos = [3, 70, -5];
        assert!(rig_wanted(state, pos, true));
        assert!(quads_at(state, pos).is_none(), "live: left out of the mesh");
        assert!(
            rig_wanted(state, pos, false),
            "handing back: the rig stays up"
        );
        std::thread::sleep(HANDOVER);
        section_rebuilt([0, 64, -16]);
        assert!(!rig_wanted(state, pos, false));
        clear();
        *REGIONS.write().unwrap() = None;
    }
}
