//! A dump of the terrain the game meshes, for the Vulkan renderer's viewer
//! (VULKAN_REWRITE_CHECKLIST.md phase 3): with `MC_DUMP_TERRAIN=<folder>`,
//! the game writes every section it stores (`sections.bin`) and the block
//! atlas (`atlas.png`), and `vkview --terrain <folder>` draws them, so the
//! new terrain path is tested on exactly what the game draws.
//!
//! `sections.bin`: the magic `TVKD`, a version, then per section:
//! origin (3 × i32), water (u8), vertex, index and cutout index counts
//! (3 × u32), the 20-byte vertices, then the indices and cutout indices
//! (u32 each). A section stored again (re-meshed) appears again; the
//! reader keeps the last.

use std::io::Write;
use std::sync::Mutex;

use crate::renderer::mesh::{MeshBuf, PackedVertex};

pub const MAGIC: &[u8; 4] = b"TVKD";
pub const VERSION: u32 = 2;

/// The layer byte of a record saying a column was unloaded.
const DROPPED: u8 = 0xFF;

static WRITER: Mutex<Option<std::io::BufWriter<std::fs::File>>> = Mutex::new(None);

fn folder() -> Option<std::path::PathBuf> {
    static FOLDER: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            crate::platform::env::var("MC_DUMP_TERRAIN")
                .ok()
                .map(std::path::PathBuf::from)
        })
        .clone()
}

/// Saves the block atlas's top level, once at startup.
pub fn atlas(width: u32, height: u32, rgba: &[u8]) {
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
pub fn section(origin: [i32; 3], meshes: &[MeshBuf; 2]) {
    for (layer, mesh) in meshes.iter().enumerate() {
        record(origin, layer as u8, mesh);
    }
}

/// Records that the column at chunk `(cx, cz)` was unloaded.
pub fn column_dropped(cx: i32, cz: i32) {
    record([cx * 16, 0, cz * 16], DROPPED, &MeshBuf::default());
}

fn record(origin: [i32; 3], layer: u8, mesh: &MeshBuf) {
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
    for n in [mesh.verts.len(), mesh.idx.len(), mesh.cutout_idx.len()] {
        write(&(n as u32).to_le_bytes());
    }
    write(bytemuck::cast_slice::<PackedVertex, u8>(&mesh.verts));
    write(bytemuck::cast_slice(&mesh.idx));
    write(bytemuck::cast_slice(&mesh.cutout_idx));
    let _ = writer.flush();
}

/// One section read back from a dump.
pub struct Section {
    pub origin: [i32; 3],
    pub water: bool,
    pub verts: Vec<PackedVertex>,
    pub idx: Vec<u32>,
    pub cutout_idx: Vec<u32>,
}

/// Reads a dump: the sections the game held when it stopped writing.
pub fn read(bytes: &[u8]) -> Result<Vec<Section>, String> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err("not a terrain dump".to_owned());
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != VERSION {
        return Err(format!("dump version {version}, expected {VERSION}"));
    }
    let mut at = 8;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let slice = bytes.get(at..at + n).ok_or("the dump ends mid-section")?;
        at += n;
        Ok(slice)
    };
    let mut sections = std::collections::HashMap::new();
    while let Ok(head) = take(25) {
        let int = |i: usize| i32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap());
        let origin = [int(0), int(1), int(2)];
        let layer = head[12];
        let count = |i: usize| {
            u32::from_le_bytes(head[13 + i * 4..17 + i * 4].try_into().unwrap()) as usize
        };
        let (verts, idx, cutout) = (count(0), count(1), count(2));
        let verts: Vec<PackedVertex> = bytemuck::pod_collect_to_vec(take(verts * 20)?);
        let idx: Vec<u32> = bytemuck::pod_collect_to_vec(take(idx * 4)?);
        let cutout_idx: Vec<u32> = bytemuck::pod_collect_to_vec(take(cutout * 4)?);
        if layer == DROPPED {
            sections.retain(|&(o, _): &([i32; 3], bool), _| o[0] != origin[0] || o[2] != origin[2]);
            continue;
        }
        let water = layer != 0;
        if verts.is_empty() {
            sections.remove(&(origin, water));
            continue;
        }
        sections.insert(
            (origin, water),
            Section {
                origin,
                water,
                verts,
                idx,
                cutout_idx,
            },
        );
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
        for n in [verts, verts, 0] {
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
    }
}
