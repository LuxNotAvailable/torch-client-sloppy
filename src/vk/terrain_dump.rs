//! A dump of the terrain the game meshes, for the Vulkan renderer's viewer
//! (VULKAN_REWRITE_CHECKLIST.md phase 3): with `MC_DUMP_TERRAIN=<folder>`,
//! the game writes every section it stores (`sections.bin`) and the block
//! atlas (`atlas.png`), and `vkview --terrain <folder>` draws them, so the
//! new terrain path is tested on exactly what the game draws.
//!
//! `sections.bin`: the magic `TVKD`, a version, then records: origin
//! (3 × i32), layer (u8: 0 opaque, 1 translucent, `DROPPED` for a column
//! unloaded), the section's visibility set (u64, `renderer::visgraph`),
//! vertex, index and cutout index counts (3 × u32), the 20-byte vertices,
//! then the indices and cutout indices (u32 each). A section stored again
//! (re-meshed) appears again, empty layers too, so replaying the records in
//! order ends with what the game held at the end, and the replay itself
//! is the game's stream of section loads, re-meshes and unloads.

use std::io::Write;
use std::sync::Mutex;

use crate::renderer::mesh::{MeshBuf, PackedVertex};
use crate::renderer::visgraph::VisibilitySet;

pub const MAGIC: &[u8; 4] = b"TVKD";
/// 4: each record also carries its pack quads (6.3).
pub const VERSION: u32 = 4;

/// The layer byte of a record saying a column was unloaded.
const DROPPED: u8 = 0xFF;
const HEAD: usize = 37;

static WRITER: Mutex<Option<std::io::BufWriter<std::fs::File>>> = Mutex::new(None);

pub(crate) fn folder() -> Option<std::path::PathBuf> {
    static FOLDER: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            crate::platform::env::var("MC_DUMP_TERRAIN")
                .ok()
                .map(std::path::PathBuf::from)
        })
        .clone()
}

/// The live renderer's feed (`vk::live`): records since its last frame.
static LIVE: Mutex<Vec<Record>> = Mutex::new(Vec::new());
/// The block atlas, for the live renderer: width, height, RGBA.
static LIVE_ATLAS: Mutex<Option<(u32, u32, Vec<u8>)>> = Mutex::new(None);

/// Records the game stored or dropped since the last call.
pub fn take_live() -> Vec<Record> {
    LIVE.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}

/// The block atlas, once the game has made it.
pub fn live_atlas() -> Option<(u32, u32, Vec<u8>)> {
    LIVE_ATLAS.lock().ok()?.clone()
}

fn live(record: Record) {
    if super::live::wanted()
        && let Ok(mut l) = LIVE.lock()
    {
        l.push(record);
    }
}

/// Saves the block atlas's top level, once at startup.
pub fn atlas(width: u32, height: u32, rgba: &[u8]) {
    if super::live::wanted()
        && let Ok(mut a) = LIVE_ATLAS.lock()
    {
        *a = Some((width, height, rgba.to_vec()));
    }
    let Some(dir) = folder() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let bytes = (width * height * 4) as usize;
    if rgba.len() >= bytes {
        let _ = image::save_buffer(
            dir.join("atlas.png"),
            &rgba[..bytes],
            width,
            height,
            image::ColorType::Rgba8,
        );
    }
}

/// Appends one stored section's opaque and translucent meshes.
pub fn section(origin: [i32; 3], vis: VisibilitySet, meshes: &[MeshBuf; 2]) {
    if super::live::wanted() {
        for (layer, mesh) in meshes.iter().enumerate() {
            live(Record::Layer(Section {
                origin,
                water: layer == 1,
                vis,
                verts: mesh.verts.clone(),
                idx: mesh.idx.clone(),
                cutout_idx: mesh.cutout_idx.clone(),
                #[cfg(feature = "shader_support")]
                pack: bytemuck::cast_slice(&mesh.pack).to_vec(),
                #[cfg(not(feature = "shader_support"))]
                pack: Vec::new(),
            }));
        }
    }
    for (layer, mesh) in meshes.iter().enumerate() {
        record(origin, layer as u8, vis.bits(), mesh);
    }
}

/// Records that the column at chunk `(cx, cz)` was unloaded.
/// Records that every section was dropped (live only; dumps start anew).
pub fn cleared() {
    live(Record::Cleared);
}

pub fn column_dropped(cx: i32, cz: i32) {
    live(Record::Dropped { cx, cz });
    record([cx * 16, 0, cz * 16], DROPPED, 0, &MeshBuf::default());
}

fn record(origin: [i32; 3], layer: u8, vis: u64, mesh: &MeshBuf) {
    let Some(dir) = folder() else { return };
    let Ok(mut slot) = WRITER.lock() else { return };
    if slot.is_none() {
        let _ = std::fs::create_dir_all(&dir);
        let Ok(file) = std::fs::File::create(dir.join("sections.bin")) else {
            return;
        };
        let mut writer = std::io::BufWriter::new(file);
        let _ = writer.write_all(MAGIC);
        let _ = writer.write_all(&VERSION.to_le_bytes());
        *slot = Some(writer);
    }
    let Some(writer) = slot.as_mut() else { return };
    let mut write = |bytes: &[u8]| {
        let _ = writer.write_all(bytes);
    };
    for c in origin {
        write(&c.to_le_bytes());
    }
    write(&[layer]);
    write(&vis.to_le_bytes());
    // A pack's quads only while one is on: one per four vertices, or none.
    #[cfg(feature = "shader_support")]
    let pack: &[u8] = bytemuck::cast_slice(&mesh.pack);
    #[cfg(not(feature = "shader_support"))]
    let pack: &[u8] = &[];
    for n in [mesh.verts.len(), mesh.idx.len(), mesh.cutout_idx.len(), pack.len() / 16] {
        write(&(n as u32).to_le_bytes());
    }
    write(bytemuck::cast_slice::<PackedVertex, u8>(&mesh.verts));
    write(bytemuck::cast_slice(&mesh.idx));
    write(bytemuck::cast_slice(&mesh.cutout_idx));
    write(pack);
    let _ = writer.flush();
}

/// One section layer from a dump.
pub struct Section {
    pub origin: [i32; 3],
    /// The translucent layer (water, glass, ice and the like).
    pub water: bool,
    pub vis: VisibilitySet,
    pub verts: Vec<PackedVertex>,
    pub idx: Vec<u32>,
    pub cutout_idx: Vec<u32>,
    /// Pack quads (`packvertex::PackQuad` words), one per four vertices;
    /// empty when no pack was on.
    pub pack: Vec<[u32; 4]>,
}

/// One record of a dump, in the order the game wrote them.
pub enum Record {
    /// A section layer stored; empty when the re-mesh left it empty.
    Layer(Section),
    /// The column at chunk `(cx, cz)` unloaded.
    Dropped { cx: i32, cz: i32 },
    /// Every section dropped (a new world or dimension); live only.
    Cleared,
}

/// Checks a dump's header; its records start after it.
fn header(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err("not a terrain dump".to_owned());
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != VERSION {
        return Err(format!(
            "dump version {version}, expected {VERSION}: record it again"
        ));
    }
    Ok(8)
}

/// The record at `*at`, moving `*at` past it.
fn next_record(bytes: &[u8], at: &mut usize) -> Option<Record> {
    let mut take = |n: usize| {
        let slice = bytes.get(*at..*at + n)?;
        *at += n;
        Some(slice)
    };
    let head = take(HEAD)?;
    let int = |i: usize| i32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap());
    let origin = [int(0), int(1), int(2)];
    let layer = head[12];
    let vis = u64::from_le_bytes(head[13..21].try_into().unwrap());
    let count =
        |i: usize| u32::from_le_bytes(head[21 + i * 4..25 + i * 4].try_into().unwrap()) as usize;
    let (verts, idx, cutout, pack) = (count(0), count(1), count(2), count(3));
    let verts: Vec<PackedVertex> = bytemuck::pod_collect_to_vec(take(verts * 20)?);
    let idx: Vec<u32> = bytemuck::pod_collect_to_vec(take(idx * 4)?);
    let cutout_idx: Vec<u32> = bytemuck::pod_collect_to_vec(take(cutout * 4)?);
    let pack: Vec<[u32; 4]> = bytemuck::pod_collect_to_vec(take(pack * 16)?);
    Some(if layer == DROPPED {
        Record::Dropped {
            cx: origin[0] >> 4,
            cz: origin[2] >> 4,
        }
    } else {
        Record::Layer(Section {
            origin,
            water: layer != 0,
            vis: VisibilitySet::from_bits(vis),
            verts,
            idx,
            cutout_idx,
            pack,
        })
    })
}

/// The records of a dump, in order.
pub fn records(bytes: &[u8]) -> Result<impl Iterator<Item = Record> + '_, String> {
    let mut at = header(bytes)?;
    Ok(std::iter::from_fn(move || next_record(bytes, &mut at)))
}

/// A dump's records, owning its bytes: the game's section stream, replayed.
pub struct Replay {
    bytes: Vec<u8>,
    at: usize,
}

impl Replay {
    pub fn new(bytes: Vec<u8>) -> Result<Replay, String> {
        let at = header(&bytes)?;
        Ok(Replay { bytes, at })
    }

    /// How far through the dump the replay is, from 0 to 1.
    pub fn progress(&self) -> f32 {
        self.at as f32 / self.bytes.len().max(1) as f32
    }
}

impl Iterator for Replay {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        next_record(&self.bytes, &mut self.at)
    }
}

/// Reads a dump: the section layers the game held when it stopped writing.
pub fn read(bytes: &[u8]) -> Result<Vec<Section>, String> {
    let mut sections = std::collections::HashMap::new();
    for record in records(bytes)? {
        match record {
            Record::Dropped { cx, cz } => {
                sections.retain(|&(o, _): &([i32; 3], bool), _| o[0] >> 4 != cx || o[2] >> 4 != cz);
            }
            Record::Cleared => sections.clear(),
            Record::Layer(section) if section.verts.is_empty() => {
                sections.remove(&(section.origin, section.water));
            }
            Record::Layer(section) => {
                sections.insert((section.origin, section.water), section);
            }
        }
    }
    Ok(sections.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(out: &mut Vec<u8>, origin: [i32; 3], layer: u8, verts: u32) {
        for c in origin {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.push(layer);
        out.extend_from_slice(&u64::MAX.to_le_bytes());
        for n in [verts, verts, 0, 0] {
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend(std::iter::repeat_n(0u8, verts as usize * 20));
        out.extend(std::iter::repeat_n(0u8, verts as usize * 4));
    }

    #[test]
    fn replay_keeps_what_the_game_held_last() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        push(&mut bytes, [0, 0, 0], 0, 3);
        push(&mut bytes, [0, 0, 0], 1, 3);
        // Re-meshed: the opaque layer grows, the translucent one empties.
        push(&mut bytes, [0, 0, 0], 0, 6);
        push(&mut bytes, [0, 0, 0], 1, 0);
        push(&mut bytes, [16, 32, 0], 0, 3);
        push(&mut bytes, [16, 48, 0], 1, 3);
        // The column at chunk (1, 0) unloads.
        push(&mut bytes, [16, 0, 0], DROPPED, 0);
        let sections = read(&bytes).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].origin, [0, 0, 0]);
        assert!(!sections[0].water);
        assert_eq!(sections[0].verts.len(), 6);
        assert_eq!(sections[0].vis, VisibilitySet::from_bits(u64::MAX));
        assert_eq!(records(&bytes).unwrap().count(), 7);
    }
}
