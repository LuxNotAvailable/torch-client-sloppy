//! Pipelines (VULKAN_REWRITE_CHECKLIST.md 2.4, 2.5): one layout shared by
//! every pipeline (the bindless set and a push constant block), a pipeline
//! cache saved to disk so a second launch skips most compilation, and small
//! builders for graphics and compute pipelines on dynamic rendering.

use std::path::PathBuf;

use ash::vk;

use super::{Context, SetupError};

/// Per-draw values, matching `Push` in `shaders/common.slang`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Push {
    pub ids: [u32; 4],
    pub more: [u32; 4],
    pub params: [f32; 4],
    pub view_proj: [[f32; 4]; 4],
}

pub const PUSH_SIZE: u32 = std::mem::size_of::<Push>() as u32;
const _: () = assert!(PUSH_SIZE <= 128, "the guaranteed push constant size");

pub struct Pipelines {
    pub layout: vk::PipelineLayout,
    cache: vk::PipelineCache,
    cache_path: Option<PathBuf>,
}

/// What a graphics pipeline draws to and how.
pub struct GraphicsDesc<'a> {
    pub module: vk::ShaderModule,
    pub vertex: &'a std::ffi::CStr,
    pub fragment: &'a std::ffi::CStr,
    pub color: &'a [vk::Format],
    pub depth: Option<vk::Format>,
    pub depth_test: Option<vk::CompareOp>,
    pub depth_write: bool,
    pub cull: vk::CullModeFlags,
    pub topology: vk::PrimitiveTopology,
}

impl Pipelines {
    /// The shared layout and the cache, loaded from `cache_path` if present.
    pub fn new(
        ctx: &Context,
        bindless: vk::DescriptorSetLayout,
        cache_path: Option<PathBuf>,
    ) -> Result<Pipelines, SetupError> {
        let set_layouts = [bindless];
        let push = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::ALL)
            .offset(0)
            .size(PUSH_SIZE)];
        let initial = cache_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .unwrap_or_default();
        // SAFETY: plain creation; the driver checks the cache blob's header
        // and ignores one from another device or driver.
        unsafe {
            let layout = ctx.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&push),
                None,
            )?;
            let cache = ctx
                .device
                .create_pipeline_cache(
                    &vk::PipelineCacheCreateInfo::default().initial_data(&initial),
                    None,
                )
                .or_else(|_| {
                    ctx.device
                        .create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None)
                })?;
            Ok(Pipelines {
                layout,
                cache,
                cache_path,
            })
        }
    }

    pub fn module(&self, ctx: &Context, spirv: &[u8]) -> Result<vk::ShaderModule, SetupError> {
        let words = ash::util::read_spv(&mut std::io::Cursor::new(spirv))
            .map_err(|e| SetupError(format!("bad SPIR-V: {e}")))?;
        // SAFETY: SPIR-V from our build, validated by slangc.
        Ok(unsafe {
            ctx.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)?
        })
    }

    pub fn compute(
        &self,
        ctx: &Context,
        module: vk::ShaderModule,
        entry: &std::ffi::CStr,
    ) -> Result<vk::Pipeline, SetupError> {
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(module)
            .name(entry);
        let info = [vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(self.layout)];
        // SAFETY: the module and layout are live.
        unsafe { ctx.device.create_compute_pipelines(self.cache, &info, None) }
            .map(|p| p[0])
            .map_err(|(_, e)| e.into())
    }

    pub fn graphics(&self, ctx: &Context, desc: &GraphicsDesc) -> Result<vk::Pipeline, SetupError> {
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(desc.module)
                .name(desc.vertex),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(desc.module)
                .name(desc.fragment),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(desc.topology);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(desc.cull)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(desc.depth_test.is_some())
            .depth_write_enable(desc.depth_write)
            .depth_compare_op(desc.depth_test.unwrap_or(vk::CompareOp::ALWAYS));
        let blend_attachments: Vec<_> = desc
            .color
            .iter()
            .map(|_| {
                vk::PipelineColorBlendAttachmentState::default()
                    .color_write_mask(vk::ColorComponentFlags::RGBA)
            })
            .collect();
        let blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let mut rendering = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(desc.color)
            .depth_attachment_format(desc.depth.unwrap_or(vk::Format::UNDEFINED));
        let info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(self.layout)
            .push_next(&mut rendering)];
        // SAFETY: every state struct is a local that outlives the call.
        unsafe {
            ctx.device
                .create_graphics_pipelines(self.cache, &info, None)
        }
        .map(|p| p[0])
        .map_err(|(_, e)| e.into())
    }

    /// Writes the cache back to disk.
    pub fn save_cache(&self, ctx: &Context) {
        let Some(path) = &self.cache_path else { return };
        // SAFETY: a plain query of a live cache.
        if let Ok(data) = unsafe { ctx.device.get_pipeline_cache_data(self.cache) } {
            let _ = std::fs::write(path, data);
        }
    }

    pub fn destroy(self, ctx: &Context) {
        self.save_cache(ctx);
        // SAFETY: the caller idles the device first.
        unsafe {
            ctx.device.destroy_pipeline_cache(self.cache, None);
            ctx.device.destroy_pipeline_layout(self.layout, None);
        }
    }
}

/// Pushes `push` for every stage.
pub fn push(ctx: &Context, commands: vk::CommandBuffer, layout: vk::PipelineLayout, push: &Push) {
    // SAFETY: recorded into a command buffer the caller is recording, with
    // the shared layout's full-size range.
    unsafe {
        ctx.device.cmd_push_constants(
            commands,
            layout,
            vk::ShaderStageFlags::ALL,
            0,
            bytemuck::bytes_of(push),
        );
    }
}
