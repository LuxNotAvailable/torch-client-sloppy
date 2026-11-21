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
//!
//! Greedy meshing (checklist 3.9, `merge`): neighbouring records in one
//! plane with the same texture and uv layout merge into one record of up to
//! 16 × 16 blocks. Its light, colour and AO move to a cell table after the
//! records, which the fragment shader reads per pixel, so a merged quad
//! shades exactly as its cells did. A merged record sets bit 26 of word 0;
//! word 3 holds width − 1 and height − 1 (4 bits each), word 4 the table's
//! byte offset from the record. A cell (`CELL_WORDS`), taken from its
//! quad's vertices:
//! - 0 to 3: the vertex colour (RGBA bytes) at in-plane step code 0 to 3
//!   (bit 0 along the first in-plane axis, bit 1 the second);
//! - 4, 5: their block and sky light (16 bits each, codes 0 and 1, then 2
//!   and 3);
//! - 6: the diagonal (0: corners 0 and 3, 1: corners 1 and 2), so the
//!   shader interpolates the cell's two triangles as the vertices would
//!   have been.

use std::collections::HashMap;

use crate::terrain::mesh::PackedVertex;

/// Indices from here up name packed corners: `PACKED_BASE + 4 × record +
/// corner`. Must match `PACKED_BASE` in `shaders/terrain.slang`.
pub const PACKED_BASE: u32 = 1 << 28;

/// One packed quad (see the module comment).
pub type Record = [u32; 8];

/// Word 0's bit for a merged record.
pub const MERGED: u32 = 1 << 26;
/// Word 0's bit for the quad's drawn diagonal (as a cell's bit 24), read
/// only by the outlines view.
pub const DIAGONAL: u32 = 1 << 27;
/// Word 0's bit for a face whose texture rotation was random (the mesher's
/// `VERTEX_FREE_ROTATION`): merging may give it its rectangle's.
pub const FREE: u32 = 1 << 28;
/// Word 0's bit for an aggressive merge (`merge`'s `aggressive`): its cells
/// have different textures, so each is `AGGRESSIVE_CELL_WORDS`, the last three its uv
/// rectangle (words 1 and 2 of its record) and its uv edges by step code
/// (2 bits each).
pub const AGGRESSIVE: u32 = 1 << 29;
/// Word 0's bit for an exact merge (`merge` without `aggressive`): its
/// cells' light and colour vary along neither axis it spans more than one
/// cell, so the vertices interpolate it exactly as the cells were; no cell
/// table. The record takes two slots, the second's word 0 its width - 1
/// and height - 1 (4 bits each).
pub const EXACT: u32 = 1 << 30;

/// Low bits of grey and tint an exact merge ignores: neighbours that far
/// apart (3/255) merge, the rectangle shaded as its first cell.
const TINT_SLACK_BITS: u32 = 2;

/// A section's solid mesh with its whole-face quads packed.
pub struct Packed {
    pub records: Vec<Record>,
    /// Merged records' cells (`CELL_WORDS` each, `AGGRESSIVE_CELL_WORDS`
    /// for an aggressive record), stored after the records.
    pub cells: Vec<u32>,
    pub verts: Vec<PackedVertex>,
    pub indices: Vec<u32>,
    /// Each record's vertices, in corner order (for its cells).
    sources: Vec<[PackedVertex; 4]>,
    /// Records whose colour a record can't give back (`pack`'s `loose`):
    /// drawn only merged, else as their vertices again.
    loose: Vec<bool>,
}

/// Words in a cell: each corner's colour (RGBA, by step code), their light
/// (16 bits each, two words), and the diagonal.
pub const CELL_WORDS: usize = 7;
/// An aggressive record's cell adds its uv rectangle and edges.
pub const AGGRESSIVE_CELL_WORDS: usize = 10;

/// Packs the quads of `indices` (six a quad, naming `verts`) that a record
/// gives back exactly.
///
/// `slack`: how far (in 1/255) a corner's colour may miss what the record
/// rebuilds; 0 for exact. Smooth lighting's AO on a tinted face often
/// misses by a rounding step, which would keep the face out of merging.
///
/// `loose` also makes records of whole faces whose colour no record gives
/// back (a tint clipped at full brightness, say), for `merge`, which draws
/// them from exact per-cell colours or puts their vertices back.
pub fn pack(verts: &[PackedVertex], indices: &[u32], slack: u8, loose: bool) -> Packed {
    let mut records = Vec::new();
    let mut sources = Vec::new();
    let mut loose_records = Vec::new();
    let mut out_indices = Vec::with_capacity(indices.len());
    // Vertices still named by quads left as they are, renumbered.
    let mut remap = vec![u32::MAX; verts.len()];
    let mut out_verts = Vec::new();
    for quad in indices.chunks(6) {
        if quad.len() == 6
            && let Some((mut record, corners, is_loose)) = record_of(verts, quad, slack, loose)
        {
            let r = records.len() as u32;
            let named: Vec<u32> = quad
                .iter()
                .map(|i| PACKED_BASE + 4 * r + corners.iter().position(|c| c == i).unwrap() as u32)
                .collect();
            // The drawn diagonal, for the outlines view.
            record[0] |= diagonal(&record, &named).unwrap_or(0) << 27;
            records.push(record);
            sources.push(corners.map(|c| verts[c as usize]));
            loose_records.push(is_loose);
            out_indices.extend(named);
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
        cells: Vec::new(),
        verts: out_verts,
        indices: out_indices,
        sources,
        loose: loose_records,
    }
}

/// What two records must share to merge: bucket, axis, plane, uv rectangle,
/// uv edges by step code (records order corners as their indices do, so
/// the steps themselves may differ) and the leaf bit.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MergeKey {
    bucket: usize,
    plane: u32,
    axis_leaf: u32,
    uv: [u32; 2],
    edges: [u32; 4],
    /// Exact: each corner's light, grey and alpha by step code, and the
    /// tint; zero for aggressive.
    values: [u32; 4],
    tint: u32,
}

/// Merges the records of the six axis buckets (`counts`, quads in order
/// from the start of `indices`) and of the cutout quads after them (leaves)
/// into rectangles, updating `counts`.
///
/// `aggressive` merges every whole face of a plane, whatever its texture
/// or rotation (`AGGRESSIVE`).
pub fn merge(packed: &mut Packed, counts: &mut [u32; 7], aggressive: bool) {
    let indices = std::mem::take(&mut packed.indices);
    let mut out = Vec::with_capacity(indices.len());
    let mut merged: Vec<(Record, Option<Record>)> = Vec::new();
    let mut at = 0usize;
    for (bucket, count) in counts.iter_mut().enumerate() {
        let quads = &indices[at..at + *count as usize];
        at += *count as usize;
        if bucket == 6 {
            out.extend_from_slice(quads);
            continue;
        }
        let run = merge_run(packed, &mut merged, quads, Some(bucket), aggressive);
        *count = run.len() as u32;
        out.extend(run);
    }
    out.extend(merge_run(
        packed,
        &mut merged,
        &indices[at..],
        None,
        aggressive,
    ));

    // Keep only the records still named, merged ones included; a loose
    // record left unmerged goes back to its vertices.
    let plain = packed.records.len();
    let mut second: Vec<Option<Record>> = vec![None; plain];
    for (m, ext) in merged {
        packed.records.push(m);
        second.push(ext);
    }
    let mut remap = vec![u32::MAX; packed.records.len()];
    let mut records = Vec::new();
    for i in &mut out {
        if *i < PACKED_BASE {
            continue;
        }
        let r = record(*i);
        if r < plain && packed.loose[r] {
            if remap[r] == u32::MAX {
                remap[r] = packed.verts.len() as u32;
                packed.verts.extend_from_slice(&packed.sources[r]);
            }
            *i = remap[r] + (*i & 3);
            continue;
        }
        if remap[r] == u32::MAX {
            remap[r] = records.len() as u32;
            records.push(packed.records[r]);
            // An exact merge's second slot, right after it.
            records.extend(second[r]);
        }
        *i = PACKED_BASE + 4 * remap[r] + (*i & 3);
    }
    let table = records.len() as u32 * 32;
    let mut skip = false;
    for (r, rec) in records.iter_mut().enumerate() {
        if std::mem::take(&mut skip) {
            continue;
        }
        if rec[0] & EXACT != 0 {
            skip = true;
            continue;
        }
        if rec[0] & MERGED != 0 {
            rec[4] = table + rec[4] * 4 - r as u32 * 32;
        }
    }
    packed.records = records;
    packed.indices = out;
}

/// One run of quads merged: `bucket` for a face bucket, `None` for the
/// cutout quads (grouped by their facing instead). Merged records go to
/// `merged`, their cells to `packed.cells`.
fn merge_run(
    packed: &mut Packed,
    merged: &mut Vec<(Record, Option<Record>)>,
    quads: &[u32],
    bucket: Option<usize>,
    aggressive: bool,
) -> Vec<u32> {
    let mut out = Vec::with_capacity(quads.len());
    // key -> cell -> quad
    let mut planes: HashMap<MergeKey, HashMap<(u32, u32), usize>> = HashMap::new();
    let mut kept = vec![true; quads.len() / 6];
    for (q, quad) in quads.chunks_exact(6).enumerate() {
        let Some((key, cell)) = mergeable(packed, quad, bucket, aggressive) else {
            continue;
        };
        if let std::collections::hash_map::Entry::Vacant(e) =
            planes.entry(key).or_default().entry(cell)
        {
            e.insert(q);
            kept[q] = false;
        }
    }
    for (key, cells) in &planes {
        // Exact: a rectangle may span cells only along an axis its values
        // don't change along (step code bit 0 the first axis, bit 1 the
        // second).
        let v = key.values;
        let (along_x, along_y) = if aggressive {
            (true, true)
        } else {
            (v[0] == v[1] && v[2] == v[3], v[0] == v[2] && v[1] == v[3])
        };
        for rect in rectangles(cells, along_x, along_y) {
            if rect.w * rect.h == 1 {
                kept[rect.quads[0]] = true;
                continue;
            }
            let first = &quads[rect.quads[0] * 6..rect.quads[0] * 6 + 6];
            let base = packed.records[record(first[0])];
            let r = (packed.records.len() + merged.len()) as u32;
            let mut m = base;
            if !aggressive {
                m[0] |= MERGED | EXACT;
                let size = (rect.w - 1) | ((rect.h - 1) << 4);
                merged.push((m, Some([size, 0, 0, 0, 0, 0, 0, 0])));
                out.extend(first.iter().map(|&i| PACKED_BASE + 4 * r + (i & 3)));
                continue;
            }
            m[0] |= MERGED | AGGRESSIVE;
            m[3] = (rect.w - 1) | ((rect.h - 1) << 4);
            // The cells' first word for now; a byte offset once records
            // settle.
            m[4] = packed.cells.len() as u32;
            m[5] = 0;
            m[7] = 0;
            merged.push((m, None));
            for &q in &rect.quads {
                let quad = &quads[q * 6..q * 6 + 6];
                let r = record(quad[0]);
                let rec = packed.records[r];
                packed.cells.extend(cell_of(&rec, &packed.sources[r], quad));
                {
                    let edges = by_code(&rec, |k| (rec[6] >> (24 + 2 * k)) & 3);
                    packed.cells.extend([
                        rec[1],
                        rec[2],
                        edges[0] | (edges[1] << 2) | (edges[2] << 4) | (edges[3] << 6),
                    ]);
                }
            }
            out.extend(first.iter().map(|&i| PACKED_BASE + 4 * r + (i & 3)));
        }
    }
    for (q, quad) in quads.chunks_exact(6).enumerate() {
        if kept[q] {
            out.extend_from_slice(quad);
        }
    }
    out
}

fn record(index: u32) -> usize {
    ((index - PACKED_BASE) / 4) as usize
}

/// A quad's merge key and cell in its plane, if it may merge.
fn mergeable(
    packed: &Packed,
    quad: &[u32],
    bucket: Option<usize>,
    aggressive: bool,
) -> Option<(MergeKey, (u32, u32))> {
    if quad.iter().any(|&i| i < PACKED_BASE) || quad.iter().any(|&i| record(i) != record(quad[0])) {
        return None;
    }
    let r = record(quad[0]);
    // A loose record's colour needs cells (aggressive).
    if !aggressive && packed.loose[r] {
        return None;
    }
    let rec = &packed.records[r];
    let w0 = rec[0];
    if w0 & MERGED != 0 {
        return None;
    }
    diagonal(rec, quad)?;
    // A uv rectangle laid out affinely over the face, so it tiles.
    let (u0, u1) = (rec[1] & 0xFFFF, rec[1] >> 16);
    let (v0, v1) = (rec[2] & 0xFFFF, rec[2] >> 16);
    if u0 == u1 || v0 == v1 {
        return None;
    }
    let edges = by_code(rec, |k| (rec[6] >> (24 + 2 * k)) & 3);
    for bit in [1, 2] {
        let e = edges.map(|e| e & bit);
        if e[0] + e[3] != e[1] + e[2] {
            return None;
        }
    }
    let axis = (w0 >> 15) & 3;
    let block = [w0 & 31, (w0 >> 5) & 31, (w0 >> 10) & 31];
    let (b, c) = ((axis as usize + 1) % 3, (axis as usize + 2) % 3);
    // Cutout quads aren't bucketed: group them by facing.
    let bucket = bucket.unwrap_or_else(|| 7 + axis as usize * 2 + facing(rec, quad));
    Some((
        MergeKey {
            bucket,
            plane: block[axis as usize],
            // DIAGONAL is left out: cells keep their own.
            axis_leaf: w0 & ((3 << 15) | (1 << 25) | if aggressive { 0 } else { FREE }),
            // Aggressive: each cell keeps its own texture and rotation.
            uv: if aggressive { [0; 2] } else { [rec[1], rec[2]] },
            // A face whose rotation was random takes its rectangle's.
            edges: if aggressive || w0 & FREE != 0 {
                [0; 4]
            } else {
                edges
            },
            // Light and AO exact; grey and tint to `TINT_SLACK_BITS`, so
            // grass whose blended biome colour drifts a little still merges
            // (the rectangle takes its first cell's).
            values: if aggressive {
                [0; 4]
            } else {
                by_code(rec, |k| {
                    let light = (rec[3 + k as usize / 2] >> (16 * (k % 2))) & 0xFFFF;
                    let grey = ((rec[5] >> (8 * k)) & 0xFF) >> TINT_SLACK_BITS;
                    light | (grey << 16) | (((rec[7] >> (8 * k)) & 0xFF) << 24)
                })
            },
            tint: if aggressive {
                0
            } else {
                let t = rec[6] & 0xFF_FFFF;
                let q = |c: u32| ((t >> c) & 0xFF) >> TINT_SLACK_BITS;
                q(0) | (q(8) << 8) | (q(16) << 16)
            },
        },
        (block[b], block[c]),
    ))
}

/// `f(corner)` placed at each corner's in-plane step code.
fn by_code(rec: &Record, f: impl Fn(u32) -> u32) -> [u32; 4] {
    let mut out = [0; 4];
    for k in 0..4 {
        out[((rec[0] >> (17 + 2 * k)) & 3) as usize] = f(k);
    }
    out
}

/// Which way the quad faces along its axis (0 or 1), from its first
/// triangle's winding.
fn facing(rec: &Record, quad: &[u32]) -> usize {
    let p = |i: u32| {
        let code = (rec[0] >> (17 + 2 * (i & 3))) & 3;
        [(code & 1) as i32, (code >> 1) as i32]
    };
    let (a, b, c) = (p(quad[0]), p(quad[1]), p(quad[2]));
    let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    usize::from(cross < 0)
}

/// The quad's diagonal: 0 when it joins step codes 0 and 3, 1 for 1 and 2.
fn diagonal(rec: &Record, quad: &[u32]) -> Option<u32> {
    let code = |i: u32| (rec[0] >> (17 + 2 * (i & 3))) & 3;
    let mut seen = [0u8; 4];
    for &i in quad {
        seen[code(i) as usize] += 1;
    }
    match seen {
        [2, 1, 1, 2] => Some(0),
        [1, 2, 2, 1] => Some(1),
        _ => None,
    }
}

/// A cell from its quad's vertices (`sources`, in corner order): exact
/// colours, so faces no record's tint gives back merge too.
fn cell_of(rec: &Record, vs: &[PackedVertex; 4], quad: &[u32]) -> [u32; CELL_WORDS] {
    let colour = by_code(rec, |k| u32::from_le_bytes(vs[k as usize].color));
    let light = by_code(rec, |k| {
        let v = &vs[k as usize];
        u32::from(v.light[0]) | (u32::from(v.light[1]) << 8)
    });
    [
        colour[0],
        colour[1],
        colour[2],
        colour[3],
        light[0] | (light[1] << 16),
        light[2] | (light[3] << 16),
        diagonal(rec, quad).unwrap_or(0),
    ]
}

/// A rectangle of cells and their quads, row by row.
struct Rect {
    w: u32,
    h: u32,
    quads: Vec<usize>,
}

/// A plane's cells: 17 × 17 (faces on a section's far side sit at 16),
/// each the quad there or `NONE`.
type Plane = [[usize; PLANE]; PLANE];
const PLANE: usize = 17;
const NONE: usize = usize::MAX;

/// Greedy cover of `cells` with rectangles up to 16 a side, by binary
/// greedy meshing: each row a bit mask, runs found with `trailing_zeros`
/// and grown down while the rows below hold the whole run. Sweeps rows and
/// columns both and keeps the cover with fewer rectangles.
///
/// `along_x` / `along_y`: whether a rectangle may span more than one cell
/// along the plane's first / second axis.
fn rectangles(cells: &HashMap<(u32, u32), usize>, along_x: bool, along_y: bool) -> Vec<Rect> {
    let mut plane: Plane = [[NONE; PLANE]; PLANE];
    for (&(x, y), &q) in cells {
        plane[y as usize][x as usize] = q;
    }
    let cap = |along: bool| if along { 16 } else { 1 };
    let rows = sweep(&plane, false, cap(along_x), cap(along_y) as usize);
    if rows.len() <= 1 {
        return rows;
    }
    let columns = sweep(&plane, true, cap(along_y), cap(along_x) as usize);
    if columns.len() < rows.len() {
        columns
    } else {
        rows
    }
}

/// One binary greedy sweep; `transposed` sweeps columns as rows. The
/// rectangles come back in the plane's own axes either way.
/// `run_cap` / `grow_cap`: the most cells along a row and down.
fn sweep(plane: &Plane, transposed: bool, run_cap: u32, grow_cap: usize) -> Vec<Rect> {
    let at = |r: usize, c: usize| if transposed { plane[c][r] } else { plane[r][c] };
    let mut masks = [0u32; PLANE];
    for (r, mask) in masks.iter_mut().enumerate() {
        for c in 0..PLANE {
            if at(r, c) != NONE {
                *mask |= 1 << c;
            }
        }
    }
    let mut out = Vec::new();
    for r in 0..PLANE {
        while masks[r] != 0 {
            let c = masks[r].trailing_zeros();
            let len = (masks[r] >> c).trailing_ones().min(run_cap);
            let run = ((1u32 << len) - 1) << c;
            let mut n = 1;
            while r + n < PLANE && n < grow_cap && masks[r + n] & run == run {
                n += 1;
            }
            for mask in &mut masks[r..r + n] {
                *mask &= !run;
            }
            // Back to plane axes: x along the plane's first axis.
            let (x, y, w, h) = if transposed {
                (r, c as usize, n as u32, len)
            } else {
                (c as usize, r, len, n as u32)
            };
            let mut quads = Vec::with_capacity((w * h) as usize);
            for j in 0..h as usize {
                for i in 0..w as usize {
                    quads.push(plane[y + j][x + i]);
                }
            }
            out.push(Rect { w, h, quads });
        }
    }
    out
}

/// The record for a quad and its four vertex indices in corner order, if a
/// record gives the quad back exactly.
fn record_of(
    verts: &[PackedVertex],
    quad: &[u32],
    slack: u8,
    loose: bool,
) -> Option<(Record, [u32; 4], bool)> {
    let r = try_record(verts, quad, slack);
    let r = match r {
        Err(Loose(record, corners)) if loose => Ok((record, corners, true)),
        Err(e) => Err(e.reason()),
        Ok((record, corners)) => Ok((record, corners, false)),
    };
    if let Some(stats) = REASONS.lock().ok().as_mut().and_then(|s| s.as_mut()) {
        let reason = match &r {
            Err(e) => *e,
            Ok((.., true)) => "loose record",
            Ok(_) => "record",
        };
        *stats.entry(reason).or_default() += 1;
    }
    r.ok()
}

/// Why quads did or didn't become records, counted while `Some`
/// (`reasons`).
static REASONS: std::sync::Mutex<Option<std::collections::BTreeMap<&'static str, u64>>> =
    std::sync::Mutex::new(None);

/// Starts counting why quads do or don't become records, and returns the
/// counts since the last call (none on the first).
pub fn reasons() -> Option<std::collections::BTreeMap<&'static str, u64>> {
    let mut slot = REASONS.lock().ok()?;
    slot.replace(Default::default())
}

/// A few quads whose colour wasn't one tint, as their corners' colours
/// and positions, while counting (`reasons`).
pub static COLOUR_SAMPLES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Stops counting (`reasons`).
pub fn stop_reasons() {
    if let Ok(mut slot) = REASONS.lock() {
        *slot = None;
    }
}

/// Why `try_record` made no record: a reason, or a face whose colour alone
/// a record can't give back (its record, colour words empty).
enum NoRecord {
    Reason(&'static str),
    Loose(Record, [u32; 4]),
}
use NoRecord::{Loose, Reason};

impl NoRecord {
    fn reason(&self) -> &'static str {
        match self {
            Reason(r) => r,
            Loose(..) => "colour isn't one tint",
        }
    }
}

impl From<&'static str> for NoRecord {
    fn from(r: &'static str) -> NoRecord {
        Reason(r)
    }
}

fn try_record(
    verts: &[PackedVertex],
    quad: &[u32],
    slack: u8,
) -> Result<(Record, [u32; 4]), NoRecord> {
    let mut corners = [u32::MAX; 4];
    let mut n = 0;
    for &i in quad {
        if !corners[..n].contains(&i) {
            if n == 4 {
                return Err(Reason("more than 4 corners"));
            }
            corners[n] = i;
            n += 1;
        }
    }
    if n != 4 {
        return Err(Reason("fewer than 4 corners"));
    }
    let vs = corners.map(|i| verts.get(i as usize).copied());
    let [Some(a), Some(b), Some(c), Some(d)] = vs else {
        return Err(Reason("missing vertex"));
    };
    let vs = [a, b, c, d];

    // On the block grid, a unit square facing along one axis.
    if vs
        .iter()
        .any(|v| v.pos[..3].iter().any(|c| c.rem_euclid(256) != 0))
    {
        return Err(Reason("off the block grid"));
    }
    let lo = [0, 1, 2].map(|a| vs.iter().map(|v| v.pos[a]).min().unwrap());
    let hi = [0, 1, 2].map(|a| vs.iter().map(|v| v.pos[a]).max().unwrap());
    let span = [0, 1, 2].map(|a| hi[a] - lo[a]);
    let axis = (0..3).find(|&a| span[a] == 0).ok_or("not axis aligned")?;
    let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
    if span[b] != 256 || span[c] != 256 {
        return Err(Reason("not one block"));
    }
    let block = lo.map(|p| p / 256);
    if block.iter().any(|&p| !(0..=16).contains(&p)) {
        return Err(Reason("outside the section"));
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
            return Err(Reason("uv not a rectangle"));
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
        .max_by_key(|c| c[0].max(c[1]).max(c[2]))
        .ok_or("no corners")?;
    let top_grey = u32::from(top[0].max(top[1]).max(top[2]));
    let mut colour_ok = top_grey != 0;
    let tint =
        [0, 1, 2].map(|i| ((u32::from(top[i]) * 255 + top_grey / 2) / top_grey.max(1)).min(255));
    let mut greys = 0u32;
    let mut alphas = 0u32;
    for (k, v) in vs.iter().enumerate() {
        let grey = u32::from(v.color[0].max(v.color[1]).max(v.color[2]));
        if (0..3).any(|i| tinted(tint[i], grey).abs_diff(u32::from(v.color[i])) > u32::from(slack))
        {
            if REASONS.lock().is_ok_and(|r| r.is_some())
                && let Ok(mut samples) = COLOUR_SAMPLES.lock()
                && samples.len() < 4
            {
                samples.push(format!(
                    "rgba {:?} at {:?}, tint {tint:?}",
                    vs.map(|v| v.color),
                    vs.map(|v| [v.pos[0] / 256, v.pos[1] / 256, v.pos[2] / 256])
                ));
            }
            colour_ok = false;
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
            | (u32::from(vs[0].light[2] == crate::terrain::mesh::MATERIAL_LEAF) << 25)
            | (u32::from(
                vs.iter()
                    .all(|v| v.light[3] & crate::terrain::mesh::VERTEX_FREE_ROTATION != 0),
            ) << 28),
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
    if !colour_ok {
        let mut record = record;
        record[5] = 0;
        record[6] &= 0xFF00_0000;
        record[7] = 0;
        return Err(Loose(record, corners));
    }
    for (k, v) in vs.iter().enumerate() {
        if !same_drawn(&unpack(&record, k as u32), v, slack) {
            return Err(Reason("doesn't round-trip"));
        }
    }
    Ok((record, corners))
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
            ((w0 >> 25) & 1) as u8 * crate::terrain::mesh::MATERIAL_LEAF,
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
fn same_drawn(a: &PackedVertex, b: &PackedVertex, slack: u8) -> bool {
    a.pos[..3] == b.pos[..3]
        && a.uv == b.uv
        && a.light[..3] == b.light[..3]
        && a.color
            .iter()
            .zip(b.color)
            .all(|(x, y)| x.abs_diff(y) <= slack)
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
        let packed = pack(&verts, &indices, 0, false);
        assert_eq!(packed.records.len(), 1);
        assert!(packed.verts.is_empty());
        for (k, &i) in indices.iter().enumerate() {
            let named = packed.indices[k];
            assert!(named >= PACKED_BASE);
            let corner = (named - PACKED_BASE) % 4;
            assert!(same_drawn(
                &unpack(&packed.records[0], corner),
                &verts[i as usize],
                0
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
        let packed = pack(&verts, &[0, 1, 2, 0, 2, 3], 0, false);
        assert!(packed.records.is_empty());
        assert_eq!(packed.verts.len(), 4);
        assert_eq!(packed.indices, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn a_colour_no_tint_rebuilds_keeps_its_vertices() {
        let mut verts = face();
        verts[1].color = [10, 200, 30, 255];
        assert!(
            pack(&verts, &[0, 1, 2, 0, 2, 3], 0, false)
                .records
                .is_empty()
        );
    }

    /// Tops of an n × n floor at y 5, each with its own light and a
    /// diagonal flipped on odd cells, uv layout shared.
    fn floor(n: i16) -> (Vec<PackedVertex>, Vec<u32>) {
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        for x in 0..n {
            for z in 0..n {
                let base = verts.len() as u32;
                let l = (x * n + z) as u8;
                let at = |dx: i16, dz: i16| [(x + dx) * 256, 1536, (z + dz) * 256];
                verts.push(vertex(at(0, 0), [100, 200], [l, 240], [255, 255, 255, 255]));
                verts.push(vertex(at(1, 0), [164, 200], [l, 224], [255, 255, 255, 200]));
                verts.push(vertex(at(1, 1), [164, 264], [l, 208], [255, 255, 255, 150]));
                verts.push(vertex(at(0, 1), [100, 264], [l, 192], [255, 255, 255, 100]));
                let q: [u32; 6] = if (x + z) % 2 == 0 {
                    [0, 1, 2, 0, 2, 3]
                } else {
                    [0, 1, 3, 1, 2, 3]
                };
                indices.extend(q.iter().map(|i| base + i));
            }
        }
        (verts, indices)
    }

    #[test]
    fn an_aggressive_floor_merges_into_one_record_whose_cells_keep_each_corner() {
        let (verts, indices) = floor(4);
        let mut packed = pack(&verts, &indices, 0, false);
        let plain: Vec<Record> = packed.records.clone();
        let mut counts = [0, 0, indices.len() as u32, 0, 0, 0, 0];
        merge(&mut packed, &mut counts, true);
        assert_eq!(packed.records.len(), 1);
        assert_eq!(counts[2], 6);
        assert_eq!(packed.indices.len(), 6);
        let rec = packed.records[0];
        assert_ne!(rec[0] & MERGED, 0);
        assert_eq!(rec[3], 3 | (3 << 4));
        assert_eq!(packed.cells.len(), 16 * AGGRESSIVE_CELL_WORDS);
        // Offset from the record to its table, just past the records.
        assert_eq!(rec[4], 32);
        // Each cell holds its own quad's corners, by step code, and diagonal.
        for (q, quad) in indices.chunks_exact(6).enumerate() {
            let x = q / 4;
            let z = q % 4;
            // Axis y: in-plane axes z (first) then x.
            let cell = &packed.cells
                [(x * 4 + z) * AGGRESSIVE_CELL_WORDS..(x * 4 + z + 1) * AGGRESSIVE_CELL_WORDS];
            let corner = |dz: usize, dx: usize| {
                let want = [(x + dx) as i16 * 256, 1536, (z + dz) as i16 * 256];
                quad.iter()
                    .map(|&i| verts[i as usize])
                    .find(|v| v.pos[..3] == want)
                    .unwrap()
            };
            for code in 0..4 {
                let v = corner(code & 1, code >> 1);
                assert_eq!(cell[code], u32::from_le_bytes(v.color));
                let light = (cell[4 + code / 2] >> (16 * (code % 2))) & 0xFFFF;
                assert_eq!(light, u32::from(v.light[0]) | (u32::from(v.light[1]) << 8));
            }
            assert_eq!(cell[6], ((x + z) % 2) as u32);
        }
    }

    /// A face whose colour is a tint clipped at full brightness (as the
    /// logs showed on AO-darkened grass): no exact record, but it merges
    /// as a loose one, and alone it gets its vertices back.
    #[test]
    fn clipped_colours_merge_loose_and_stay_vertices_alone() {
        let (mut verts, indices) = floor(2);
        for (i, v) in verts.iter_mut().enumerate() {
            v.color = if i % 4 == 0 {
                [222, 249, 255, 255]
            } else {
                [134, 150, 209, 255]
            };
        }
        let strict = pack(&verts, &indices, 2, false);
        assert!(strict.records.is_empty());
        let mut packed = pack(&verts, &indices, 2, true);
        assert_eq!(packed.records.len(), 4);
        let mut counts = [0, 0, indices.len() as u32, 0, 0, 0, 0];
        merge(&mut packed, &mut counts, true);
        assert_eq!(packed.records.len(), 1);
        assert_eq!(packed.indices.len(), 6);
        assert_eq!(packed.cells[0], u32::from_le_bytes([222, 249, 255, 255]));

        // One face alone: back to its four vertices.
        let mut one = pack(&verts[..4], &[0, 1, 2, 0, 2, 3], 2, true);
        let mut counts = [0, 0, 6, 0, 0, 0, 0];
        merge(&mut one, &mut counts, true);
        assert!(one.records.is_empty());
        assert_eq!(one.verts.len(), 4);
        assert!(one.indices.iter().all(|&i| i < 4));
    }

    /// Every corner of an n × n floor lit `light(dx, dz)` by its offset in
    /// the cell (so cells match their neighbours).
    fn lit_floor(n: i16, light: impl Fn(i16, i16) -> u8) -> (Vec<PackedVertex>, Vec<u32>) {
        let (mut verts, indices) = floor(n);
        for (i, v) in verts.iter_mut().enumerate() {
            let (dx, dz) = [(0, 0), (1, 0), (1, 1), (0, 1)][i % 4];
            v.light = [0, light(dx, dz), 0, 0];
            v.color = [255, 255, 255, 255];
        }
        (verts, indices)
    }

    fn exact(verts: &[PackedVertex], indices: &[u32]) -> (Packed, [u32; 7]) {
        let mut packed = pack(verts, indices, 0, false);
        let mut counts = [0, 0, indices.len() as u32, 0, 0, 0, 0];
        merge(&mut packed, &mut counts, false);
        (packed, counts)
    }

    #[test]
    fn an_evenly_lit_floor_merges_exactly_into_one_quad() {
        let (verts, indices) = lit_floor(4, |_, _| 240);
        let (packed, counts) = exact(&verts, &indices);
        assert_eq!(counts[2], 6);
        // Two slots: the record, then its size.
        assert_eq!(packed.records.len(), 2);
        assert_ne!(packed.records[0][0] & EXACT, 0);
        assert_eq!(packed.records[1][0], 3 | (3 << 4));
        assert!(packed.cells.is_empty());
    }

    #[test]
    fn exact_merges_run_only_along_an_axis_the_light_holds_on() {
        // Darker towards +z inside each cell (an AO strip): merges along x
        // only, one strip per row of z.
        let (verts, indices) = lit_floor(4, |_, dz| if dz == 0 { 240 } else { 128 });
        let (packed, counts) = exact(&verts, &indices);
        assert_eq!(counts[2] / 6, 4);
        assert!(packed.records.chunks(2).all(|r| r[0][0] & EXACT != 0));
        // Light changing both ways: nothing merges.
        let (verts, indices) = lit_floor(2, |dx, dz| (100 + 50 * dx + 20 * dz) as u8);
        let (packed, counts) = exact(&verts, &indices);
        assert_eq!(counts[2] / 6, 4);
        assert!(packed.records.iter().all(|r| r[0] & MERGED == 0));
    }

    #[test]
    fn different_textures_stay_apart() {
        let (mut verts, indices) = lit_floor(2, |_, _| 240);
        for v in &mut verts[..4] {
            v.uv[0] += 64;
        }
        let mut packed = pack(&verts, &indices, 0, false);
        let mut counts = [0, 0, indices.len() as u32, 0, 0, 0, 0];
        merge(&mut packed, &mut counts, false);
        // The odd cell alone, then the other three as an L: two rectangles
        // and one single.
        assert_eq!(counts[2] / 6, 3);
        assert_eq!(packed.indices.len(), 18);
    }

    #[test]
    fn packed_and_plain_quads_mix_in_index_order() {
        let mut verts = face();
        let mut slab = face();
        for v in &mut slab {
            v.pos[1] = 1536 + 128;
        }
        verts.extend(slab);
        let packed = pack(&verts, &[4, 5, 6, 4, 6, 7, 0, 1, 2, 0, 2, 3], 0, false);
        assert_eq!(packed.records.len(), 1);
        assert_eq!(&packed.indices[..6], &[0, 1, 2, 0, 2, 3]);
        assert!(packed.indices[6..].iter().all(|&i| i >= PACKED_BASE));
    }

    /// Why the dump's whole faces don't merge: records and merged quads at
    /// a few colour slacks. `cargo test --release --lib dump_merge -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_merge_by_slack() {
        let Ok(bytes) = std::fs::read("target/pixel-dump4/sections.bin") else {
            return;
        };
        let sections = crate::vk::terrain_dump::read(&bytes).unwrap();
        for slack in [0u8, 2, 4, 8] {
            let (mut quads, mut records, mut after) = (0usize, 0usize, 0usize);
            for s in sections.iter().filter(|s| !s.water) {
                let mut idx = s.idx.clone();
                let solid = idx.len() as u32;
                idx.extend_from_slice(&s.cutout_idx);
                quads += idx.len() / 6;
                let mut p = pack(&s.verts, &idx, slack, false);
                records += p.records.len();
                // One bucket is enough for counting (keys hold the axis).
                let mut counts = [0, 0, 0, 0, 0, 0, 0];
                counts[0] = solid;
                merge(&mut p, &mut counts, false);
                after += p.indices.len() / 6;
            }
            println!("slack {slack}: {quads} quads, {records} records, {after} after merging");
        }
    }

    /// Why the dump's quads aren't records. `cargo test --release --lib dump_reasons -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_reasons() {
        let Ok(bytes) = std::fs::read("target/pixel-dump4/sections.bin") else {
            return;
        };
        let sections = crate::vk::terrain_dump::read(&bytes).unwrap();
        let mut why: std::collections::BTreeMap<&str, usize> = Default::default();
        for s in sections.iter().filter(|s| !s.water) {
            for (layer, idx) in [("solid", &s.idx), ("cutout", &s.cutout_idx)] {
                for quad in idx.chunks_exact(6) {
                    let mut c: Vec<u32> = quad.to_vec();
                    c.sort_unstable();
                    c.dedup();
                    let reason = if c.len() != 4 {
                        "not 4 corners"
                    } else {
                        let vs: Vec<PackedVertex> =
                            c.iter().map(|&i| s.verts[i as usize]).collect();
                        let lo = [0, 1, 2].map(|a| vs.iter().map(|v| v.pos[a]).min().unwrap());
                        let hi = [0, 1, 2].map(|a| vs.iter().map(|v| v.pos[a]).max().unwrap());
                        let flat = (0..3).filter(|&a| lo[a] == hi[a]).count();
                        let grid = vs
                            .iter()
                            .all(|v| v.pos[..3].iter().all(|c| c.rem_euclid(256) == 0));
                        if flat != 1 {
                            "not axis aligned"
                        } else if !grid {
                            "off the block grid"
                        } else if (0..3).any(|a| hi[a] - lo[a] != 0 && hi[a] - lo[a] != 256) {
                            "not one block"
                        } else if record_of(&s.verts, quad, 0, false).is_some() {
                            "record"
                        } else if vs
                            .iter()
                            .any(|v| v.color[0].max(v.color[1]).max(v.color[2]) == 0)
                        {
                            "black corner"
                        } else {
                            "colour or uv"
                        }
                    };
                    *why.entry(if layer == "solid" {
                        reason
                    } else {
                        Box::leak(format!("cutout: {reason}").into_boxed_str())
                    })
                    .or_default() += 1;
                }
            }
        }
        for (k, v) in why {
            println!("{v:>9} {k}");
        }
    }
}
