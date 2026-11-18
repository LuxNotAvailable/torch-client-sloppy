use bevy::prelude::*;
use bytemuck::{Pod, Zeroable};
use parking_lot::Mutex;

#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
pub struct GuiVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

pub const GUI_VERTEX_STRIDE: u64 = 32;
const _: () = assert!(size_of::<GuiVertex>() as u64 == GUI_VERTEX_STRIDE);

#[derive(Clone, Copy)]
pub struct GuiRange {
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
}

#[derive(Default)]
pub struct GuiFrame {
    pub vertices: Vec<GuiVertex>,
    pub indices: Vec<u32>,
    pub ranges: Vec<GuiRange>,
}

#[derive(Resource, Default)]
pub struct GuiFrames(pub Mutex<GuiFrame>);

impl GuiFrames {
    #[allow(clippy::too_many_arguments, reason = "one range's whole description")]
    pub fn push(
        &self,
        positions: &[[f32; 3]],
        uvs: &[[f32; 2]],
        colors: &[[f32; 4]],
        indices: &[u32],
        vw: f32,
        vh: f32,
    ) {
        let mut frame = self.0.lock();
        let base_vertex = frame.vertices.len() as i32;
        let first_index = frame.indices.len() as u32;
        let (sx, sy) = (2.0 / vw, 2.0 / vh);
        frame.vertices.reserve(positions.len());
        for i in 0..positions.len() {
            let p = positions[i];
            frame.vertices.push(GuiVertex {
                pos: [p[0] * sx - 1.0, 1.0 - p[1] * sy],
                uv: uvs[i],
                color: colors[i],
            });
        }
        frame.indices.extend_from_slice(indices);
        frame.ranges.push(GuiRange {
            first_index,
            index_count: indices.len() as u32,
            base_vertex,
        });
    }
}

/// Quads the GUI drew last frame, for the performance report.
pub static QUADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Resource, Clone)]
pub struct GuiTextures {
    pub atlas: Handle<Image>,
    pub unihex: Handle<Image>,
}
