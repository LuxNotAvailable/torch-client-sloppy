//! Part matrices for GPU-posed model rigs.
//!
//! A posed model keeps one static mesh per model, each vertex tagged with its
//! part index, and every live rig layer owns a fixed run of part matrices.
//! The renderer's vertex shader reads `parts[MeshTag + part]` (the rows go
//! out in `draw::Entities`), so posing a mob writes a few matrices instead of
//! re-baking its whole mesh, and every mob of a model shares one mesh.

use bevy::math::Mat4;
use bevy::mesh::{MeshVertexAttribute, VertexFormat};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

pub const ATTRIBUTE_PART: MeshVertexAttribute =
    MeshVertexAttribute::new("RigPart", 0x5c1e_7a0d_93b4_2f61, VertexFormat::Uint32);

/// Matrices the renderer holds; a rig that would overflow it falls back to the
/// CPU bake.
pub const CAPACITY: u32 = 1 << 16;

/// The top three rows of an affine part matrix.
pub type PartRows = [[f32; 4]; 3];

const ZERO_ROWS: PartRows = [[0.0; 4]; 3];

/// Whether new model rigs pose on the GPU. Off under `MC_CPU_RIGS=1`. Shader packs pose them too:
/// their entity programs compile a skinned vertex stage
/// (`shaderpack::compile::preludes::entity_prelude`).
pub fn enabled() -> bool {
    static FORCED_CPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let forced = *FORCED_CPU.get_or_init(|| {
        matches!(
            crate::platform::env::var("MC_CPU_RIGS").as_deref(),
            Ok("1") | Ok("true")
        )
    });
    !forced
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PartSlot {
    pub offset: u32,
    pub len: u32,
}

#[derive(Resource, Default)]
pub struct RigParts {
    rows: Vec<PartRows>,
    free: HashMap<u32, Vec<u32>>,
}

impl RigParts {
    pub fn alloc(&mut self, len: u32) -> Option<PartSlot> {
        if let Some(offset) = self.free.get_mut(&len).and_then(Vec::pop) {
            return Some(PartSlot { offset, len });
        }
        let offset = self.rows.len() as u32;
        if offset + len > CAPACITY {
            return None;
        }
        self.rows.resize(self.rows.len() + len as usize, ZERO_ROWS);
        Some(PartSlot { offset, len })
    }

    pub fn release(&mut self, slot: PartSlot) {
        let start = slot.offset as usize;
        self.rows[start..start + slot.len as usize].fill(ZERO_ROWS);
        self.free.entry(slot.len).or_default().push(slot.offset);
    }

    /// Every part's rows, by offset (the entity dump copies them).
    pub fn rows(&self) -> &[PartRows] {
        &self.rows
    }

    /// Writes one matrix per part; `None` hides that part.
    pub fn write(&mut self, slot: PartSlot, mats: impl IntoIterator<Item = Option<Mat4>>) {
        let start = slot.offset as usize;
        let run = &mut self.rows[start..start + slot.len as usize];
        for (out, mat) in run.iter_mut().zip(mats) {
            *out = part_rows(mat);
        }
    }
}

pub fn part_rows(mat: Option<Mat4>) -> PartRows {
    match mat {
        Some(m) => [
            m.row(0).to_array(),
            m.row(1).to_array(),
            m.row(2).to_array(),
        ],
        None => ZERO_ROWS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_released_slot_is_reused_for_the_same_length() {
        let mut parts = RigParts::default();
        let a = parts.alloc(6).unwrap();
        let b = parts.alloc(6).unwrap();
        assert_ne!(a.offset, b.offset);
        parts.release(a);
        assert_eq!(parts.alloc(6), Some(a));
        assert_eq!(parts.alloc(4).unwrap().offset, 12);
    }

    #[test]
    fn allocation_past_capacity_fails() {
        let mut parts = RigParts::default();
        assert!(parts.alloc(CAPACITY).is_some());
        assert!(parts.alloc(1).is_none());
    }

    #[test]
    fn written_rows_are_the_matrix_rows() {
        let mut parts = RigParts::default();
        let slot = parts.alloc(2).unwrap();
        let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        parts.write(slot, [Some(m), None]);
        assert_eq!(parts.rows[0][0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(parts.rows[0][2], [0.0, 0.0, 1.0, 3.0]);
        assert_eq!(parts.rows[1], ZERO_ROWS);
    }
}
