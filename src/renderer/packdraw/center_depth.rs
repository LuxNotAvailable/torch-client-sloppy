//! `centerDepthSmooth`: the depth at the middle of the screen, as Iris reads
//! it for depth of field. A fullscreen fragment pass — depth loads are plain
//! fragment-stage work that every driver supports, unlike the compute path
//! that a few mobile GPU compilers choke on — writes that one texel into a
//! 1×1 float texture, which is copied into one of a few read-back buffers;
//! once that copy is on the GPU queue the buffer is mapped, and the value
//! lands a frame or two later, which the smoothing hides.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use bevy::prelude::*;
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutEntries, Buffer, BufferDescriptor,
    BufferUsages, CommandEncoder, MapMode, ShaderStages, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
    binding_types::texture_depth_2d,
};
use bevy::render::renderer::RenderDevice;

use super::fullscreen::{fullscreen_corner, fullscreen_pass, fullscreen_pipeline};

/// The latest read-back depth, as `f32` bits; the far plane until one lands.
static SAMPLE: AtomicU32 = AtomicU32::new(0x3F80_0000);

pub(crate) fn sample() -> f32 {
    f32::from_bits(SAMPLE.load(Ordering::Relaxed))
}

const SHADER: &str = concat!(
    fullscreen_corner!(),
    "
@group(0) @binding(0) var depth: texture_depth_2d;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(corner(index) * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = textureDimensions(depth);
    return vec4<f32>(textureLoad(depth, vec2<i32>(size / 2u), 0), 0.0, 0.0, 1.0);
}
"
);

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
    pipeline: wgpu::RenderPipeline,
    layout: BindGroupLayout,
    scratch: Texture,
    scratch_view: TextureView,
    slots: Vec<Slot>,
    bound: Mutex<Option<Bound>>,
}

impl FromWorld for CenterDepth {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        let layout = device.create_bind_group_layout(
            "shaderpack center depth",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_depth_2d(),)),
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
        let pipeline = fullscreen_pipeline(
            device,
            "shaderpack center depth",
            &module,
            &pipeline_layout,
            TextureFormat::R32Float,
        );
        let scratch = device.create_texture(&TextureDescriptor {
            label: Some("shaderpack center depth scratch"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let scratch_view = scratch.create_view(&TextureViewDescriptor {
            format: Some(TextureFormat::R32Float),
            ..Default::default()
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
            scratch,
            scratch_view,
            slots,
            bound: Mutex::new(None),
        }
    }
}

impl CenterDepth {
    /// Writes the centre texel of `depth` and copies it into a free read-back
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
                    &BindGroupEntries::sequential((depth,)),
                ),
            });
        }
        let Some(bound) = bound.as_ref() else {
            return;
        };
        fullscreen_pass(
            encoder,
            "shaderpack center depth",
            &self.scratch_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &self.pipeline,
            &bound.group,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.scratch,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &slot.buffer,
                // One texel, one row: a row pitch, if given, would have to
                // be a multiple of 256.
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: None,
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
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
                slot.buffer
                    .slice(..)
                    .map_async(MapMode::Read, move |result| {
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
