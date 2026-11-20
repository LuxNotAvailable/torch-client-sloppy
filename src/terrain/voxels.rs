//! Voxel records for traced reflections: per section, each visible block's
//! side and bottom atlas tiles, tint and light, as the built-in pack's
//! water reads them (`torchVoxels`, `vk::voxels`). The mesher fills them
//! only while a pack asks.
//!
//! A record is two words. Word 0: side tile (12 bits), bottom tile (12),
//! flags (8: `PRESENT`, `SIDE_TINT`, `BOTTOM_TINT`, `CUTOUT`, `EMISSIVE`).
//! Word 1: tint RGB8 (24), sky light (4), block light (4).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{BlockGeom, RenderedBlock};

pub const PRESENT: u32 = 1;
pub const SIDE_TINT: u32 = 2;
pub const BOTTOM_TINT: u32 = 4;
pub const CUTOUT: u32 = 8;
pub const EMISSIVE: u32 = 16;

const MAX_TILE: u32 = 4095;

static WANTED: AtomicBool = AtomicBool::new(false);

/// Whether the mesher fills voxel records.
pub fn wanted() -> bool {
    WANTED.load(Ordering::Relaxed)
}

/// Sets whether a pack traces voxels; turning it on re-meshes the world so
/// every section gets its records.
pub fn set_wanted(on: bool) {
    if WANTED.swap(on, Ordering::Relaxed) != on && on {
        crate::client::bus::request(crate::client::bus::ClientRequest::ReloadChunks);
    }
}

/// A section's records: the non-empty cells, by index `y << 8 | z << 4 | x`.
/// None at all is a known-empty section.
#[derive(Default, Debug)]
pub struct SectionVoxels {
    pub cells: Vec<(u16, [u32; 2])>,
}

impl SectionVoxels {
    /// The section as 4096 dense records.
    pub fn dense(&self) -> Vec<[u32; 2]> {
        let mut out = vec![[0u32; 2]; 4096];
        for &(i, r) in &self.cells {
            out[i as usize] = r;
        }
        out
    }
}

/// The atlas tile under a quad's middle.
fn tile_at(uv: &[[f32; 2]; 4], atlas_rows: u32) -> u32 {
    let u = uv.iter().map(|c| c[0]).sum::<f32>() * 0.25;
    let v = uv.iter().map(|c| c[1]).sum::<f32>() * 0.25;
    let col = (u * super::ATLAS_COLS as f32).floor().max(0.0) as u32;
    let row = (v * atlas_rows as f32).floor().max(0.0) as u32;
    row * super::ATLAS_COLS + col.min(super::ATLAS_COLS - 1)
}

/// The tint a pack sees for `rb` at a block.
#[cfg(feature = "shader_support")]
fn tint_of(rb: &RenderedBlock, occ: &super::Occupancy, (x, y, z): (i32, i32, i32)) -> [f32; 3] {
    use super::TintKind;
    use crate::util::biome_color as bc;
    let radius = super::biome_blend();
    match rb.tint_kind {
        TintKind::None => [1.0; 3],
        TintKind::Grass => occ.blend(x, y, z, radius, bc::pack_grass),
        TintKind::Foliage => occ.blend(x, y, z, radius, bc::pack_foliage),
        TintKind::DryFoliage => occ.blend(x, y, z, radius, bc::pack_dry_foliage),
        TintKind::Redstone(power) => bc::redstone_ratio(power),
        TintKind::Stem(age) => bc::stem_ratio(age),
        TintKind::Water => occ.blend(x, y, z, radius, bc::pack_water),
        TintKind::Modded(id) => crate::util::block_model::modded_tint(id),
    }
}

#[cfg(not(feature = "shader_support"))]
fn tint_of(_: &RenderedBlock, _: &super::Occupancy, _: (i32, i32, i32)) -> [f32; 3] {
    [1.0; 3]
}

/// One block's record, or `None` where a reflection sees nothing (water,
/// an empty model).
fn record(
    rb: &RenderedBlock,
    (x, y, z): (i32, i32, i32),
    atlas_rows: u32,
    occ: &super::Occupancy,
) -> Option<[u32; 2]> {
    let mut flags = PRESENT;
    let (side, bottom) = match &rb.geom {
        BlockGeom::Fluid { still, lava, .. } => {
            if !lava {
                return None;
            }
            flags |= EMISSIVE;
            (*still, *still)
        }
        BlockGeom::Model(baked) => {
            let model = baked.parts.first()?.choices.first()?;
            let (mut side, mut bottom, mut any) = (None, None, None);
            for quad in &model.quads {
                let tile = tile_at(&quad.uv, atlas_rows);
                let tinted = quad.tint >= 0;
                if !quad.opaque {
                    flags |= CUTOUT;
                }
                any.get_or_insert((tile, tinted));
                match quad.face {
                    0 => {
                        bottom.get_or_insert((tile, tinted));
                    }
                    2..=5 => {
                        // Outer faces before inner ones (a grass block's
                        // overlay is drawn over its side).
                        if side.is_none() || quad.cull.is_some() && !tinted {
                            side = Some((tile, tinted));
                        }
                    }
                    _ => {}
                }
            }
            let any = any?;
            let side = side.or(bottom).unwrap_or(any);
            let bottom = bottom.unwrap_or(side);
            if side.1 {
                flags |= SIDE_TINT;
            }
            if bottom.1 {
                flags |= BOTTOM_TINT;
            }
            (side.0, bottom.0)
        }
    };
    if side > MAX_TILE || bottom > MAX_TILE {
        return None;
    }
    if rb.emission > 0 || rb.emissive {
        flags |= EMISSIVE;
    }
    // Lit as the open cells around it: below and the four sides.
    let (mut sky, mut block) = (0u8, rb.emission.min(15));
    for (dx, dy, dz) in [(0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)] {
        let (nx, ny, nz) = (x + dx, y + dy, z + dz);
        if occ.is_solid(nx, ny, nz) {
            continue;
        }
        sky = sky.max(occ.sky_light(nx, ny, nz));
        block = block.max(occ.block_light(nx, ny, nz));
    }
    let tint = if flags & (SIDE_TINT | BOTTOM_TINT) != 0 {
        tint_of(rb, occ, (x, y, z))
    } else {
        [1.0; 3]
    };
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    Some([
        side | bottom << 12 | flags << 24,
        byte(tint[0])
            | byte(tint[1]) << 8
            | byte(tint[2]) << 16
            | u32::from(sky.min(15)) << 24
            | u32::from(block.min(15)) << 28,
    ])
}

/// The records of the blocks the mesher kept for a section at `base`
/// (enclosed blocks are left out: a ray reaches none of them).
pub fn build(
    blocks: &[(i32, i32, i32, &RenderedBlock)],
    base: (i32, i32, i32),
    atlas_rows: u32,
    occ: &super::Occupancy,
) -> Arc<SectionVoxels> {
    let mut cells = Vec::new();
    for &(x, y, z, rb) in blocks {
        let (lx, ly, lz) = (x - base.0, y - base.1, z - base.2);
        if !(0..16).contains(&lx) || !(0..16).contains(&ly) || !(0..16).contains(&lz) {
            continue;
        }
        if let Some(r) = record(rb, (x, y, z), atlas_rows, occ) {
            cells.push(((ly << 8 | lz << 4 | lx) as u16, r));
        }
    }
    Arc::new(SectionVoxels { cells })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quad_names_the_tile_under_its_middle() {
        // Tile (3, 2) of a 16 x 4 atlas.
        let (u0, v0) = (3.0 / 16.0, 2.0 / 4.0);
        let (u1, v1) = (4.0 / 16.0, 3.0 / 4.0);
        let uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
        assert_eq!(tile_at(&uv, 4), 2 * 16 + 3);
    }

    #[test]
    fn dense_places_cells_by_index() {
        let v = SectionVoxels {
            cells: vec![(5, [1, 2]), (4095, [3, 4])],
        };
        let d = v.dense();
        assert_eq!(d[5], [1, 2]);
        assert_eq!(d[4095], [3, 4]);
        assert_eq!(d[0], [0, 0]);
    }
}
