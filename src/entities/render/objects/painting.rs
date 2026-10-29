//! Paintings: `PaintingRenderer`. The variant's picture on the front, the
//! shared back texture on the back and the edges, a block per cell. The
//! front and the back are two layers, as each has its own texture.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use azalea_registry::builtin::EntityKind;
use bevy::math::{Quat, Vec3};
use bevy::prelude::{Mesh, Transform};

use crate::entities::quads::QuadMesh;
use crate::entities::registry::Registry;
use crate::entities::state::EntityState;
use crate::entities::{RenderSpec, RootPose, TexturePath};
use crate::items::model::strip_namespace;

/// Half the painting's depth.
const EDGE: f32 = 0.03125;

pub fn register(registry: &mut Registry) {
    registry.add(
        EntityKind::Painting,
        RenderSpec::built("painting_front", front_key, front_mesh, front_texture)
            .with_root(painting_root)
            .with_visible(has_variant),
    );
    registry.add(
        EntityKind::Painting,
        RenderSpec::built("painting_back", back_key, back_mesh, back_texture)
            .with_root(painting_root)
            .with_visible(has_variant),
    );
}

/// A painting variant: its picture and its size in blocks.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub asset: String,
    pub width: u32,
    pub height: u32,
}

/// The variant a painting shows, from the `painting_variant` registry.
pub fn variant(st: &EntityState) -> Option<Variant> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Variant>>>> = OnceLock::new();
    let key = strip_namespace(st.extras.variant.as_deref()?).to_owned();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache
        .entry(key.clone())
        .or_insert_with(|| {
            let json = crate::util::datapack::entry("painting_variant", &key)?;
            Some(Variant {
                asset: strip_namespace(json.get("asset_id")?.as_str()?).to_owned(),
                width: json.get("width")?.as_u64()? as u32,
                height: json.get("height")?.as_u64()? as u32,
            })
        })
        .clone()
}

fn has_variant(st: &EntityState) -> bool {
    variant(st).is_some()
}

/// `offsetForPaintingSize`: an even size centres on a block edge.
fn size_offset(size: u32) -> f32 {
    if size % 2 == 0 { 0.5 } else { 0.0 }
}

/// The painting's centre, from the block it hangs in, as
/// `HangingEntity.calculateBoundingBox` places it: against the wall, and
/// half a block left and up for even sizes. `pos` is the block, which is
/// what the server sends for a painting.
pub fn centre(pos: [f32; 3], facing: crate::direction::Direction, width: u32, height: u32) -> Vec3 {
    let normal = Vec3::from(facing.normal());
    // `getCounterClockWise`: north to west, south to east.
    let left = Vec3::new(normal.z, 0.0, -normal.x);
    Vec3::new(pos[0].floor() + 0.5, pos[1].floor() + 0.5, pos[2].floor() + 0.5) - normal * 0.46875
        + left * size_offset(width)
        + Vec3::Y * size_offset(height)
}

fn painting_root(st: &EntityState) -> RootPose {
    let facing = st.extras.item_frame_direction;
    let (width, height) = variant(st).map_or((1, 1), |v| (v.width, v.height));
    let at = centre(st.pos, facing, width, height);
    RootPose {
        scale: 1.0,
        hook: Transform {
            translation: at - Vec3::from(st.pos),
            rotation: Quat::from_rotation_y((180.0 - facing.to_y_rot()).to_radians()),
            scale: Vec3::ONE,
        },
        ..RootPose::default()
    }
}

fn front_key(st: &EntityState) -> TexturePath {
    variant(st).map_or_else(
        || "painting_front".into(),
        |v| format!("painting_front:{}:{}x{}", v.asset, v.width, v.height).into(),
    )
}

fn back_key(st: &EntityState) -> TexturePath {
    variant(st).map_or_else(
        || "painting_back".into(),
        |v| format!("painting_back:{}x{}", v.width, v.height).into(),
    )
}

fn front_texture(st: &EntityState) -> Option<TexturePath> {
    Some(format!("painting/{}", variant(st)?.asset).into())
}

fn back_texture(_st: &EntityState) -> Option<TexturePath> {
    Some("painting/back".into())
}

/// The cells of a `width` by `height` painting, centred: `(x0, x1, y0, y1)`
/// with `x0` the cell's right edge and `y0` its top, as `renderPainting`
/// walks them, and the cell's column and row.
fn cells(width: u32, height: u32) -> impl Iterator<Item = (f32, f32, f32, f32, u32, u32)> {
    let (ox, oy) = (-(width as f32) / 2.0, -(height as f32) / 2.0);
    (0..width).flat_map(move |sx| {
        (0..height).map(move |sy| {
            (
                ox + (sx + 1) as f32,
                ox + sx as f32,
                oy + (sy + 1) as f32,
                oy + sy as f32,
                sx,
                sy,
            )
        })
    })
}

fn front_mesh(st: &EntityState) -> Mesh {
    let (w, h) = variant(st).map_or((1, 1), |v| (v.width, v.height));
    let mut out = QuadMesh::new();
    let (du, dv) = (1.0 / w as f32, 1.0 / h as f32);
    for (x0, x1, y0, y1, sx, sy) in cells(w, h) {
        let u0 = du * (w - sx) as f32;
        let u1 = du * (w - (sx + 1)) as f32;
        let v0 = dv * (h - sy) as f32;
        let v1 = dv * (h - (sy + 1)) as f32;
        out.quad(
            [[x0, y1, -EDGE], [x1, y1, -EDGE], [x1, y0, -EDGE], [x0, y0, -EDGE]],
            [[u1, v0], [u0, v0], [u0, v1], [u1, v1]],
            [0.0, 0.0, -1.0],
        );
    }
    out.finish()
}

fn back_mesh(st: &EntityState) -> Mesh {
    let (w, h) = variant(st).map_or((1, 1), |v| (v.width, v.height));
    // The back texture's strips: a 1/16 band for the top, bottom and sides.
    const STRIP: f32 = 0.0625;
    let mut out = QuadMesh::new();
    for (x0, x1, y0, y1, sx, sy) in cells(w, h) {
        out.quad(
            [[x0, y0, EDGE], [x1, y0, EDGE], [x1, y1, EDGE], [x0, y1, EDGE]],
            [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            [0.0, 0.0, 1.0],
        );
        if sy == h - 1 {
            out.quad(
                [[x0, y0, -EDGE], [x1, y0, -EDGE], [x1, y0, EDGE], [x0, y0, EDGE]],
                [[0.0, 0.0], [1.0, 0.0], [1.0, STRIP], [0.0, STRIP]],
                [0.0, 1.0, 0.0],
            );
        }
        if sy == 0 {
            out.quad(
                [[x0, y1, EDGE], [x1, y1, EDGE], [x1, y1, -EDGE], [x0, y1, -EDGE]],
                [[0.0, 0.0], [1.0, 0.0], [1.0, STRIP], [0.0, STRIP]],
                [0.0, -1.0, 0.0],
            );
        }
        if sx == w - 1 {
            out.quad(
                [[x0, y0, EDGE], [x0, y1, EDGE], [x0, y1, -EDGE], [x0, y0, -EDGE]],
                [[STRIP, 0.0], [STRIP, 1.0], [0.0, 1.0], [0.0, 0.0]],
                [-1.0, 0.0, 0.0],
            );
        }
        if sx == 0 {
            out.quad(
                [[x1, y0, -EDGE], [x1, y1, -EDGE], [x1, y1, EDGE], [x1, y0, EDGE]],
                [[STRIP, 0.0], [STRIP, 1.0], [0.0, 1.0], [0.0, 0.0]],
                [1.0, 0.0, 0.0],
            );
        }
    }
    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::direction::Direction;

    #[test]
    fn a_painting_hangs_against_its_wall() {
        // A 1x1 facing south hangs on the north side of its block.
        let c = centre([4.0, 64.0, 7.0], Direction::South, 1, 1);
        assert!((c - Vec3::new(4.5, 64.5, 7.03125)).length() < 1e-5, "{c:?}");
    }

    #[test]
    fn an_even_painting_centres_on_a_block_edge() {
        // 2x2 facing south: left (east) and up by half a block.
        let c = centre([0.0, 0.0, 0.0], Direction::South, 2, 2);
        assert!((c - Vec3::new(1.0, 1.0, 0.03125)).length() < 1e-5, "{c:?}");
        // Facing north, left is west.
        let c = centre([0.0, 0.0, 0.0], Direction::North, 2, 1);
        assert!((c.x - 0.0).abs() < 1e-5 && (c.y - 0.5).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn a_painting_has_a_front_quad_per_block() {
        let mut st = EntityState::new(0, EntityKind::Painting);
        st.extras.shared_mut().variant = Some("minecraft:kebab".into());
        let mesh = front_mesh(&st);
        assert_eq!(mesh.count_vertices(), 4);
    }
}
