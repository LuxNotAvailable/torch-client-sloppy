//! `shaderpack::uniforms` on wgpu: the blocks in one uniform buffer, and
//! frame values from Bevy's extracted view.

use bevy::render::render_resource::{Buffer, BufferDescriptor, BufferUsages};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::view::ExtractedView;

use crate::shaderpack::directives::Constants;
use crate::shaderpack::expressions::{CustomState, Customs};
use crate::shaderpack::inputs::Inputs;
use crate::shaderpack::uniforms::{BlockSpec, FrameValues, History, UniformBytes};

/// `UniformBytes` in one wgpu uniform buffer.
pub(crate) struct PackUniforms {
    pub(crate) buffer: Buffer,
    data: UniformBytes,
}

impl PackUniforms {
    pub(crate) fn new<'a>(
        device: &RenderDevice,
        label: &str,
        specs: impl IntoIterator<Item = BlockSpec<'a>>,
        customs: &Customs,
    ) -> PackUniforms {
        let data = UniformBytes::new(
            specs,
            customs,
            device.limits().min_uniform_buffer_offset_alignment,
        );
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: data.bytes.len() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        PackUniforms { buffer, data }
    }

    pub(crate) fn range(&self, index: usize) -> (u64, u64) {
        self.data.range(index)
    }

    pub(crate) fn write(
        &mut self,
        values: &FrameValues,
        customs: &CustomState,
        queue: &RenderQueue,
    ) {
        self.data.update(values, customs);
        queue.write_buffer(&self.buffer, 0, &self.data.bytes);
    }
}

/// `FrameValues::of_camera` from Bevy's extracted view.
#[allow(clippy::too_many_arguments)]
pub(crate) fn frame_values(
    view: &ExtractedView,
    time_counter: f32,
    frame_time: f32,
    frames: u32,
    far: f32,
    inputs: Inputs,
    constants: &Constants,
    history: &mut History,
) -> FrameValues {
    FrameValues::of_camera(
        view.world_from_view.to_matrix(),
        view.clip_from_view.y_axis.y,
        bevy::math::UVec2::new(view.viewport.z, view.viewport.w),
        time_counter,
        frame_time,
        frames,
        far,
        inputs,
        constants,
        history,
    )
}
