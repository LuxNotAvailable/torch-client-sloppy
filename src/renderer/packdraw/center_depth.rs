//! `centerDepthSmooth`: the depth at the middle of the screen, as Iris reads
//! it for depth of field. A one-thread compute pass loads that texel of the
//! pack's depth into a small storage buffer (a depth texture can only be copied
//! whole), which is copied into one of a few read-back buffers; once that copy
//! is on the GPU queue the buffer is mapped, and the value lands a frame or two
//! later, which the smoothing hides.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use bevy::prelude::*;
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutEntries, Buffer,
    BufferDescriptor, BufferUsages, CommandEncoder, ComputePipeline, MapMode, ShaderStages,
    TextureView,
    binding_types::{storage_buffer_sized, texture_depth_2d},
};
use bevy::render::renderer::RenderDevice;

/// The latest read-back depth, as `f32` bits; the far plane until one lands.
static SAMPLE: AtomicU32 = AtomicU32::new(0x3F80_0000);

pub(crate) fn sample() -> f32 {
    f32::from_bits(SAMPLE.load(Ordering::Relaxed))
}

const SHADER: &str = "
@group(0) @binding(0) var depth: texture_depth_2d;
@group(0) @binding(1) var<storage, read_write> centre: array<f32, 1>;

@compute @workgroup_size(1)
fn main() {
    let size = textureDimensions(depth);
    centre[0] = textureLoad(depth, vec2<i32>(size / 2u), 0);
}
";

const SLOTS: usize = 3;
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const MAPPED: u8 = 3;

struct Slot {
    buffer: Buffer,
    state: std::sync::Arc<AtomicU8>,
}

struct Bound {
    view: bevy::render::render_resource::TextureViewId,
    group: BindGroup,
}

#[derive(Resource)]
pub(super) struct CenterDepth {
    pipeline: ComputePipeline,
    layout: BindGroupLayout,
    centre: Buffer,
    slots: Vec<Slot>,
    bound: Mutex<Option<Bound>>,
}

impl FromWorld for CenterDepth {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        let layout = device.create_bind_group_layout(
            "shaderpack center depth",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    texture_depth_2d(),
                    storage_buffer_sized(false, std::num::NonZeroU64::new(4)),
                ),
            ),
        );
        let module = device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shaderpack center depth"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shaderpack center depth"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("shaderpack center depth"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let centre = device.create_buffer(&BufferDescriptor {
            label: Some("shaderpack center depth"),
            size: 4,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&BufferDescriptor {
                    label: Some("shaderpack center depth read-back"),
                    size: 4,
                    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Default::default(),
            })
            .collect();
        CenterDepth {
            pipeline,
            layout,
            centre,
            slots,
            bound: Mutex::new(None),
        }
    }
}

impl CenterDepth {
    /// Loads the centre texel of `depth` and copies it into a free read-back
    /// buffer, if one is free.
    pub(super) fn record(
        &self,
        device: &RenderDevice,
        encoder: &mut CommandEncoder,
        depth: &TextureView,
    ) {
        let Some(slot) = self
            .slots
            .iter()
            .find(|s| s.state.load(Ordering::Acquire) == IDLE)
        else {
            return;
        };
        let Ok(mut bound) = self.bound.lock() else {
            return;
        };
        if bound.as_ref().is_none_or(|b| b.view != depth.id()) {
            *bound = Some(Bound {
                view: depth.id(),
                group: device.create_bind_group(
                    "shaderpack center depth",
                    &self.layout,
                    &BindGroupEntries::sequential((depth, self.centre.as_entire_binding())),
                ),
            });
        }
        let Some(bound) = bound.as_ref() else {
            return;
        };
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("shaderpack center depth"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bound.group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&self.centre, 0, &slot.buffer, 0, 4);
        slot.state.store(COPIED, Ordering::Release);
    }
}

/// After the frame is submitted: maps the buffers copied into, and reads the
/// ones whose mapping finished.
pub(super) fn read_back(center: Option<Res<CenterDepth>>, device: Res<RenderDevice>) {
    let Some(center) = center else { return };
    for slot in &center.slots {
        match slot.state.load(Ordering::Acquire) {
            COPIED => {
                slot.state.store(MAPPING, Ordering::Release);
                let state = slot.state.clone();
                slot.buffer.slice(..).map_async(MapMode::Read, move |result| {
                    state.store(
                        if result.is_ok() { MAPPED } else { IDLE },
                        Ordering::Release,
                    );
                });
            }
            MAPPED => {
                let bytes = slot.buffer.slice(..).get_mapped_range();
                let depth = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                drop(bytes);
                slot.buffer.unmap();
                if depth.is_finite() {
                    SAMPLE.store(depth.to_bits(), Ordering::Relaxed);
                }
                slot.state.store(IDLE, Ordering::Release);
            }
            _ => {}
        }
    }
    let _ = device.poll(bevy::render::render_resource::PollType::Poll);
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_centre_shader_validates() {
        let module = naga::front::wgsl::parse_str(super::SHADER).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }
}
