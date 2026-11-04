use bevy::render::render_resource::{
    BindGroup, BindGroupLayout, BindGroupLayoutEntry, BindingType, ColorTargetState, ColorWrites,
    RenderPipeline, ShaderStages, TextureFormat, TextureSampleType, TextureView,
    TextureViewDimension,
};
use bevy::render::renderer::RenderDevice;

macro_rules! fullscreen_corner {
    () => {
        "fn corner(index: u32) -> vec2<f32> {
    return vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
}
"
    };
}
pub(super) use fullscreen_corner;

pub(super) fn opaque_target(format: TextureFormat) -> Option<ColorTargetState> {
    Some(ColorTargetState {
        format,
        blend: None,
        write_mask: ColorWrites::ALL,
    })
}

pub(super) fn fullscreen_pipeline(
    device: &RenderDevice,
    label: &str,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: TextureFormat,
) -> wgpu::RenderPipeline {
    device
        .wgpu_device()
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[opaque_target(format)],
            }),
            multiview: None,
            cache: None,
        })
}

pub(super) fn fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    target: &TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    group: &BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &**group, &[]);
    pass.draw(0..3, 0..1);
}

const BLIT_WGSL: &str = concat!(
    fullscreen_corner!(),
    r#"
@group(0) @binding(0) var source: texture_2d<f32>;

struct Out {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Out {
    var out: Out;
    out.position = vec4<f32>(corner(index) * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

fn decode(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    let at = vec2<i32>(i32(in.position.x), size.y - 1 - i32(in.position.y));
    let color = textureLoad(source, at, 0);
    return vec4<f32>({FINISH}, 1.0);
}
"#
);

fn blit_source(decode: bool) -> String {
    let finish = if decode {
        "decode(clamp(color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)))"
    } else {
        "color.rgb"
    };
    BLIT_WGSL.replace("{FINISH}", finish)
}

pub(super) struct Blit {
    pub(super) layout: BindGroupLayout,
    pipelines: Vec<(TextureFormat, RenderPipeline)>,
}

impl Blit {
    pub(super) fn new(device: &RenderDevice) -> Blit {
        Blit {
            layout: device.create_bind_group_layout(
                gpu_label!("blit"),
                &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            ),
            pipelines: Vec::new(),
        }
    }

    pub(super) fn pipeline(&self, format: TextureFormat) -> Option<&RenderPipeline> {
        self.pipelines
            .iter()
            .find(|(f, _)| *f == format)
            .map(|(_, pipeline)| pipeline)
    }

    pub(super) fn add(&mut self, device: &RenderDevice, format: TextureFormat) {
        let source = blit_source(!stores_display(format));
        let module = device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(gpu_label!("blit")),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(gpu_label!("blit")),
            bind_group_layouts: &[&self.layout],
            push_constant_ranges: &[],
        });
        let pipeline = fullscreen_pipeline(device, gpu_label!("blit"), &module, &layout, format);
        self.pipelines
            .push((format, RenderPipeline::from(pipeline)));
    }
}

const UPSCALE_WGSL: &str = concat!(
    fullscreen_corner!(),
    r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var linear: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Out {
    var out: Out;
    let c = corner(index);
    out.position = vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(c.x, 1.0 - c.y);
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, linear, in.uv, 0.0);
}
"#
);

/// Scales a shader pack's finished frame, drawn below the window's size
/// (the render scale option), up to the view with bilinear filtering. The
/// source has the view's own format, so the colours pass through as stored.
pub(super) struct Upscale {
    pub(super) layout: BindGroupLayout,
    sampler: bevy::render::render_resource::Sampler,
    pipelines: Vec<(TextureFormat, RenderPipeline)>,
}

/// The texture a scaled shader pack's final pass draws into, at the pack's
/// size and the view's format, with its bind group for the upscale.
pub(super) struct ScaledOut {
    pub(super) size: bevy::math::UVec2,
    pub(super) format: TextureFormat,
    pub(super) view: TextureView,
    pub(super) group: BindGroup,
}

impl Upscale {
    pub(super) fn new(device: &RenderDevice) -> Upscale {
        Upscale {
            layout: device.create_bind_group_layout(
                gpu_label!("upscale"),
                &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            sample_type: TextureSampleType::Float { filterable: true },
                            view_dimension: TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Sampler(
                            bevy::render::render_resource::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                ],
            ),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(gpu_label!("upscale")),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            pipelines: Vec::new(),
        }
    }

    pub(super) fn pipeline(&self, format: TextureFormat) -> Option<&RenderPipeline> {
        self.pipelines
            .iter()
            .find(|(f, _)| *f == format)
            .map(|(_, pipeline)| pipeline)
    }

    fn add(&mut self, device: &RenderDevice, format: TextureFormat) {
        let module = device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(gpu_label!("upscale")),
            source: wgpu::ShaderSource::Wgsl(UPSCALE_WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(gpu_label!("upscale")),
            bind_group_layouts: &[&self.layout],
            push_constant_ranges: &[],
        });
        let pipeline = fullscreen_pipeline(device, gpu_label!("upscale"), &module, &layout, format);
        self.pipelines
            .push((format, RenderPipeline::from(pipeline)));
    }

    /// The target for a pack drawing at `size` into a view of `full` size
    /// and `format`, reusing `current` when it still fits; `None` at full
    /// size, where the final pass draws straight into the view.
    pub(super) fn target(
        &mut self,
        device: &RenderDevice,
        current: Option<ScaledOut>,
        size: bevy::math::UVec2,
        full: bevy::math::UVec2,
        format: TextureFormat,
    ) -> Option<ScaledOut> {
        if size == full {
            return None;
        }
        if self.pipeline(format).is_none() {
            self.add(device, format);
        }
        if let Some(current) = current.filter(|c| c.size == size && c.format == format) {
            return Some(current);
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(gpu_label!("scaled final")),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(
            gpu_label!("upscale"),
            &self.layout,
            &[
                bevy::render::render_resource::BindGroupEntry {
                    binding: 0,
                    resource: bevy::render::render_resource::BindingResource::TextureView(&view),
                },
                bevy::render::render_resource::BindGroupEntry {
                    binding: 1,
                    resource: bevy::render::render_resource::BindingResource::Sampler(
                        &self.sampler,
                    ),
                },
            ],
        );
        Some(ScaledOut {
            size,
            format,
            view,
            group,
        })
    }
}

pub(super) fn stores_display(format: TextureFormat) -> bool {
    matches!(
        format,
        TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm
    )
}

#[cfg(test)]
mod tests {
    fn validate(source: &str) {
        let module = naga::front::wgsl::parse_str(source).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validates");
    }

    #[test]
    fn the_upscale_and_blit_shaders_validate() {
        validate(super::UPSCALE_WGSL);
        validate(&super::blit_source(true));
        validate(&super::blit_source(false));
    }
}
