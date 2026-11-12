//! Compact solid quads for the terrain pools (VULKAN_REWRITE_CHECKLIST.md
//! 11.2, after Horizon's `mesher/packed.rs`): most of a section's opaque and
//! cutout quads are whole faces of a block on the block grid, and those are
//! stored as one 32-byte record instead of four 20-byte vertices. Their
//! indices stay (the face buckets, the cull and the draws are unchanged) but
//! name `PACKED_BASE + 4 × record + corner`; `terrain_vertex` in
//! `shaders/terrain.slang` rebuilds the corner from the record, giving back
//! exactly the vertex the mesher made. A quad that a record can't give back
//! bit for bit keeps its vertices.
//!
//! In a dump of the hub at render distance 32, 67% of the opaque quads pack,
//! and the opaque terrain's vertex and index bytes fall by 47%.
//!
//! A record, as eight words:
//! - 0: the quad's low corner in blocks from the origin (x, y, z, 5 bits
//!   each), the axis it faces along (2 bits), and per corner which of the
//!   two in-plane steps it takes (2 bits each);
//! - 1, 2: the UV rectangle's u0 and u1, v0 and v1 (16 bits each);
//! - 3, 4: each corner's block and sky light (a byte each);
//! - 5: each corner's grey level (the brightest of its red, green, blue);
//! - 6: the tint (red, green, blue) and per corner which edge of the UV
//!   rectangle its u and v take (2 bits each);
//! - 7: each corner's alpha (ambient occlusion).
//!
//! A corner's colour is `(tint × grey + 127) / 255` per channel.

use crate::renderer::mesh::PackedVertex;

/// Indices from here up name packed corners: `PACKED_BASE + 4 × record +
/// corner`. Must match `PACKED_BASE` in `shaders/terrain.slang`.
pub const PACKED_BASE: u32 = 1 << 28;

/// One packed quad (see the module comment).
pub type Record = [u32; 8];

/// A section's solid mesh with its whole-face quads packed.
pub struct Packed {
    pub records: Vec<Record>,
    pub verts: Vec<PackedVertex>,
    pub indices: Vec<u32>,
}

/// Packs the quads of `indices` (six a quad, naming `verts`) that a record
/// gives back exactly.
pub fn pack(verts: &[PackedVertex], indices: &[u32]) -> Packed {
    let mut records = Vec::new();
    let mut out_indices = Vec::with_capacity(indices.len());
    // Vertices still named by quads left as they are, renumbered.
    let mut remap = vec![u32::MAX; verts.len()];
    let mut out_verts = Vec::new();
    for quad in indices.chunks(6) {
        if quad.len() == 6
            && let Some((record, corners)) = record_of(verts, quad)
        {
            let r = records.len() as u32;
            records.push(record);
            out_indices.extend(quad.iter().map(|i| {
                PACKED_BASE + 4 * r + corners.iter().position(|c| c == i).unwrap() as u32
            }));
            continue;
        }
        for &i in quad {
            let slot = &mut remap[i as usize];
            if *slot == u32::MAX {
                *slot = out_verts.len() as u32;
                out_verts.push(verts[i as usize]);
            }
            out_indices.push(*slot);
        }
    }
    Packed {
        records,
        verts: out_verts,
        indices: out_indices,
    }
}

/// The record for a quad and its four vertex indices in corner order, if a
/// record gives the quad back exactly.
fn record_of(verts: &[PackedVertex], quad: &[u32]) -> Option<(Record, [u32; 4])> {
    let mut corners = [u32::MAX; 4];
    let mut n = 0;
    for &i in quad {
        if !corners[..n].contains(&i) {
            if n == 4 {
                return None;
            }
            corners[n] = i;
            n += 1;
        }
    }
    if n != 4 {
        return None;
    }
    let vs = corners.map(|i| verts.get(i as usize).copied());
    let vs: [PackedVertex; 4] = [vs[0]?, vs[1]?, vs[2]?, vs[3]?];

    // On the block grid, a unit square facing along one axis.
    if vs
        .iter()
        .any(|v| v.pos[..3].iter().any(|c| c.rem_euclid(256) != 0))
    {
        return None;
    }
    let lo = [0, 1, 2].map(|a| vs.iter().map(|v| v.pos[a]).min().unwrap());
    let hi = [0, 1, 2].map(|a| vs.iter().map(|v| v.pos[a]).max().unwrap());
    let span = [0, 1, 2].map(|a| hi[a] - lo[a]);
    let axis = (0..3).find(|&a| span[a] == 0)?;
    let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
    if span[b] != 256 || span[c] != 256 {
        return None;
    }
    let block = lo.map(|p| p / 256);
    if block.iter().any(|&p| !(0..=16).contains(&p)) {
        return None;
    }
    let mut steps = 0u32;
    for (k, v) in vs.iter().enumerate() {
        let sb = u32::from(v.pos[b] != lo[b]);
        let sc = u32::from(v.pos[c] != lo[c]);
        steps |= (sb | (sc << 1)) << (2 * k);
    }

    // A UV rectangle.
    let (u0, u1) = min_max(vs.iter().map(|v| v.uv[0]));
    let (v0, v1) = min_max(vs.iter().map(|v| v.uv[1]));
    let mut uv_edges = 0u32;
    for (k, v) in vs.iter().enumerate() {
        if (v.uv[0] != u0 && v.uv[0] != u1) || (v.uv[1] != v0 && v.uv[1] != v1) {
            return None;
        }
        let eu = u32::from(v.uv[0] == u1 && u1 != u0);
        let ev = u32::from(v.uv[1] == v1 && v1 != v0);
        uv_edges |= (eu | (ev << 1)) << (2 * k);
    }
    // A degenerate rectangle (u0 == u1) gives back u0 whichever edge.

    // One tint, scaled by each corner's grey level.
    let top = vs
        .iter()
        .map(|v| v.color)
        .max_by_key(|c| c[0].max(c[1]).max(c[2]))?;
    let top_grey = u32::from(top[0].max(top[1]).max(top[2]));
    if top_grey == 0 {
        return None;
    }
    let tint = [0, 1, 2].map(|i| ((u32::from(top[i]) * 255 + top_grey / 2) / top_grey).min(255));
    let mut greys = 0u32;
    let mut alphas = 0u32;
    for (k, v) in vs.iter().enumerate() {
        let grey = u32::from(v.color[0].max(v.color[1]).max(v.color[2]));
        if (0..3).any(|i| tinted(tint[i], grey) != u32::from(v.color[i])) {
            return None;
        }
        greys |= grey << (8 * k);
        alphas |= u32::from(v.color[3]) << (8 * k);
    }
    let light = |k: usize| u32::from(vs[k].light[0]) | (u32::from(vs[k].light[1]) << 8);

    let record = [
        block[0] as u32
            | ((block[1] as u32) << 5)
            | ((block[2] as u32) << 10)
            | ((axis as u32) << 15)
            | (steps << 17)
            | (u32::from(vs[0].light[2] == crate::renderer::mesh::MATERIAL_LEAF) << 25),
        u32::from(u0) | (u32::from(u1) << 16),
        u32::from(v0) | (u32::from(v1) << 16),
        light(0) | (light(1) << 16),
        light(2) | (light(3) << 16),
        greys,
        tint[0] | (tint[1] << 8) | (tint[2] << 16) | (uv_edges << 24),
        alphas,
    ];
    // Whatever the shader ignores (position w, the light's last two bytes)
    // needn't round-trip; everything it reads must.
    for (k, v) in vs.iter().enumerate() {
        if !same_drawn(&unpack(&record, k as u32), v) {
            return None;
        }
    }
    Some((record, corners))
}

fn min_max(values: impl Iterator<Item = u16> + Clone) -> (u16, u16) {
    (values.clone().min().unwrap_or(0), values.max().unwrap_or(0))
}

fn tinted(tint: u32, grey: u32) -> u32 {
    (tint * grey + 127) / 255
}

/// The vertex a record's corner gives back, as `terrain_vertex` builds it.
pub fn unpack(record: &Record, corner: u32) -> PackedVertex {
    let w0 = record[0];
    let block = [w0 & 31, (w0 >> 5) & 31, (w0 >> 10) & 31];
    let axis = ((w0 >> 15) & 3) as usize;
    let steps = (w0 >> (17 + 2 * corner)) & 3;
    let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut pos = block.map(|p| (p * 256) as i16);
    pos[b] += (steps & 1) as i16 * 256;
    pos[c] += ((steps >> 1) & 1) as i16 * 256;
    let edges = (record[6] >> (24 + 2 * corner)) & 3;
    let u = if edges & 1 != 0 {
        record[1] >> 16
    } else {
        record[1] & 0xFFFF
    };
    let v = if edges & 2 != 0 {
        record[2] >> 16
    } else {
        record[2] & 0xFFFF
    };
    let light = (record[3 + (corner as usize / 2)] >> (16 * (corner % 2))) & 0xFFFF;
    let grey = (record[5] >> (8 * corner)) & 0xFF;
    let tint = [
        record[6] & 0xFF,
        (record[6] >> 8) & 0xFF,
        (record[6] >> 16) & 0xFF,
    ];
    PackedVertex {
        pos: [pos[0], pos[1], pos[2], 0],
        uv: [u as u16, v as u16],
        light: [
            light as u8,
            (light >> 8) as u8,
            ((w0 >> 25) & 1) as u8 * crate::renderer::mesh::MATERIAL_LEAF,
            0,
        ],
        color: [
            tinted(tint[0], grey) as u8,
            tinted(tint[1], grey) as u8,
            tinted(tint[2], grey) as u8,
            ((record[7] >> (8 * corner)) & 0xFF) as u8,
        ],
    }
}

/// Whether two vertices draw the same: every field the terrain shader reads.
fn same_drawn(a: &PackedVertex, b: &PackedVertex) -> bool {
    a.pos[..3] == b.pos[..3] && a.uv == b.uv && a.light[..3] == b.light[..3] && a.color == b.color
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(pos: [i16; 3], uv: [u16; 2], light: [u8; 2], color: [u8; 4]) -> PackedVertex {
        PackedVertex {
            pos: [pos[0], pos[1], pos[2], 0],
            uv,
            light: [light[0], light[1], 0, 0],
            color,
        }
    }

    fn face() -> Vec<PackedVertex> {
        // The top of the block at (3, 5, 7), grass-tinted, with smooth light.
        let tint = [0x79, 0xC0, 0x5A];
        let shade = |g: u32| tint.map(|t| ((t * g + 127) / 255) as u8);
        let c = |g: u32, a: u8| {
            let s = shade(g);
            [s[0], s[1], s[2], a]
        };
        vec![
            vertex([768, 1536, 1792], [100, 200], [16, 240], c(255, 255)),
            vertex([1024, 1536, 1792], [164, 200], [32, 224], c(200, 230)),
            vertex([1024, 1536, 2048], [164, 264], [48, 208], c(150, 200)),
            vertex([768, 1536, 2048], [100, 264], [64, 192], c(100, 180)),
        ]
    }

    #[test]
    fn a_whole_face_packs_and_gives_back_every_corner() {
        let verts = face();
        let indices = [0, 1, 2, 0, 2, 3];
        let packed = pack(&verts, &indices);
        assert_eq!(packed.records.len(), 1);
        assert!(packed.verts.is_empty());
        for (k, &i) in indices.iter().enumerate() {
            let named = packed.indices[k];
            assert!(named >= PACKED_BASE);
            let corner = (named - PACKED_BASE) % 4;
            assert!(same_drawn(
                &unpack(&packed.records[0], corner),
                &verts[i as usize]
            ));
        }
    }

    #[test]
    fn a_partial_quad_keeps_its_vertices() {
        // A slab's top: half a block up, so off the grid.
        let mut verts = face();
        for v in &mut verts {
            v.pos[1] = 1536 + 128;
        }
        let packed = pack(&verts, &[0, 1, 2, 0, 2, 3]);
        assert!(packed.records.is_empty());
        assert_eq!(packed.verts.len(), 4);
        assert_eq!(packed.indices, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn a_colour_no_tint_rebuilds_keeps_its_vertices() {
        let mut verts = face();
        verts[1].color = [10, 200, 30, 255];
        assert!(pack(&verts, &[0, 1, 2, 0, 2, 3]).records.is_empty());
    }

    #[test]
    fn packed_and_plain_quads_mix_in_index_order() {
        let mut verts = face();
        let mut slab = face();
        for v in &mut slab {
            v.pos[1] = 1536 + 128;
        }
        verts.extend(slab);
        let packed = pack(&verts, &[4, 5, 6, 4, 6, 7, 0, 1, 2, 0, 2, 3]);
        assert_eq!(packed.records.len(), 1);
        assert_eq!(&packed.indices[..6], &[0, 1, 2, 0, 2, 3]);
        assert!(packed.indices[6..].iter().all(|&i| i >= PACKED_BASE));
    }
}
