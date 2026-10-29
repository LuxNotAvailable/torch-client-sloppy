//! Part matrices for GPU-posed model rigs.
//!
//! A posed model keeps one static mesh per model, each vertex tagged with its
//! part index, and every live rig layer owns a fixed run of part matrices in
//! one shared storage buffer. The vertex shader reads
//! `parts[MeshTag + part]`, so posing a mob writes a few matrices instead of
//! re-baking and re-uploading its whole mesh, and every mob of a model shares
//! one mesh that Bevy can batch.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::asset::{RenderAssetUsages, uuid_handle};
use bevy::math::Mat4;
use bevy::mesh::{MeshVertexAttribute, VertexFormat};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::BufferUsages;
use bevy::render::renderer::RenderQueue;
use bevy::render::storage::{GpuShaderStorageBuffer, ShaderStorageBuffer};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

pub const ATTRIBUTE_PART: MeshVertexAttribute =
    MeshVertexAttribute::new("RigPart", 0x5c1e_7a0d_93b4_2f61, VertexFormat::Uint32);

pub const PARTS: Handle<ShaderStorageBuffer> = uuid_handle!("8f0d9b62-3c4e-4a71-9e25-6b1f7c0a4d38");

/// Matrices the buffer holds; a rig that would overflow it falls back to the
/// CPU bake.
pub const CAPACITY: u32 = 1 << 16;

/// The top three rows of an affine part matrix.
pub type PartRows = [[f32; 4]; 3];

const ZERO_ROWS: PartRows = [[0.0; 4]; 3];

pub static PACK_TAKEOVER: AtomicBool = AtomicBool::new(false);

/// Whether new model rigs pose on the GPU. Off on WebGL (no vertex-stage
/// storage buffers), under `MC_CPU_RIGS=1`, and while a shader pack draws
/// entities, since its programs read the mesh as already posed.
pub fn enabled() -> bool {
    if cfg!(target_arch = "wasm32") {
        return false;
    }
    static FORCED_CPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let forced = *FORCED_CPU.get_or_init(|| {
        matches!(std::env::var("MC_CPU_RIGS").as_deref(), Ok("1") | Ok("true"))
    });
    !forced && !PACK_TAKEOVER.load(Ordering::Relaxed)
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
    generation: u64,
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
        self.generation = self.generation.wrapping_add(1);
        Some(PartSlot { offset, len })
    }

    pub fn release(&mut self, slot: PartSlot) {
        let start = slot.offset as usize;
        self.rows[start..start + slot.len as usize].fill(ZERO_ROWS);
        self.free.entry(slot.len).or_default().push(slot.offset);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Writes one matrix per part; `None` hides that part.
    pub fn write(&mut self, slot: PartSlot, mats: impl IntoIterator<Item = Option<Mat4>>) {
        let start = slot.offset as usize;
        let run = &mut self.rows[start..start + slot.len as usize];
        for (out, mat) in run.iter_mut().zip(mats) {
            *out = part_rows(mat);
        }
        self.generation = self.generation.wrapping_add(1);
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

#[derive(Resource, Default)]
struct RigPartsUpload {
    rows: Vec<PartRows>,
    generation: u64,
    pending: bool,
}

fn extract_rig_parts(parts: Extract<Res<RigParts>>, mut upload: ResMut<RigPartsUpload>) {
    if parts.generation == upload.generation {
        return;
    }
    upload.generation = parts.generation;
    upload.rows.clear();
    upload.rows.extend_from_slice(&parts.rows);
    upload.pending = true;
}

fn upload_rig_parts(
    mut upload: ResMut<RigPartsUpload>,
    buffers: Res<RenderAssets<GpuShaderStorageBuffer>>,
    queue: Res<RenderQueue>,
) {
    if !upload.pending || upload.rows.is_empty() {
        return;
    }
    let Some(gpu) = buffers.get(PARTS.id()) else {
        return;
    };
    queue.write_buffer(&gpu.buffer, 0, bytemuck::cast_slice(&upload.rows));
    upload.pending = false;
}

pub struct RigPartsPlugin;

impl Plugin for RigPartsPlugin {
    fn build(&self, app: &mut App) {
        let mut storage = ShaderStorageBuffer::with_size(
            CAPACITY as usize * std::mem::size_of::<PartRows>(),
            RenderAssetUsages::default(),
        );
        storage.buffer_description.label = Some("rig_parts");
        storage.buffer_description.usage = BufferUsages::STORAGE | BufferUsages::COPY_DST;
        app.world_mut()
            .resource_mut::<Assets<ShaderStorageBuffer>>()
            .insert(&PARTS, storage)
            .expect("a uuid handle has no generation to go stale");
        app.init_resource::<RigParts>();

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<RigPartsUpload>()
            .add_systems(ExtractSchedule, extract_rig_parts)
            .add_systems(
                Render,
                upload_rig_parts.in_set(RenderSystems::PrepareResources),
            );
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
