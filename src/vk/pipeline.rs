//! Pipelines (VULKAN_REWRITE_CHECKLIST.md 2.4, 2.5): one layout shared by
//! every pipeline (the bindless set and a push constant block), a pipeline
//! cache saved to disk so a second launch skips most compilation, and small
//! builders for graphics and compute pipelines on dynamic rendering.
//!
//! `build_on_worker` compiles a batch on a worker thread, so the frame loop
//! keeps going (streaming, drawing what it can) while the driver works.
//! The cache is internally synchronized and pipeline creation needs no
//! other lock, so workers and the main thread can build at once.

use std::ffi::CString;
use std::path::PathBuf;
use std::thread::JoinHandle;

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
    /// Straight alpha blending on every colour attachment.
    pub blend: bool,
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
        build_compute(&ctx.device, self.cache, self.layout, module, entry)
    }

    pub fn graphics(&self, ctx: &Context, desc: &GraphicsDesc) -> Result<vk::Pipeline, SetupError> {
        build_graphics(&ctx.device, self.cache, self.layout, desc)
    }

    /// Starts building `specs` on a worker thread. The modules must live
    /// until the result is taken, and the pipelines go before `destroy`.
    pub fn build_on_worker(&self, ctx: &Context, specs: Vec<Spec>) -> Pending {
        let device = ctx.device.clone();
        let (cache, layout) = (self.cache, self.layout);
        let started = std::time::Instant::now();
        let thread = std::thread::Builder::new()
            .name("vk pipelines".to_owned())
            .spawn(move || {
                let mut built = Vec::with_capacity(specs.len());
                for spec in &specs {
                    let pipeline = match spec {
                        Spec::Compute { module, entry } => {
                            build_compute(&device, cache, layout, *module, entry)
                        }
                        Spec::Graphics(g) => build_graphics(
                            &device,
                            cache,
                            layout,
                            &GraphicsDesc {
                                module: g.module,
                                vertex: &g.vertex,
                                fragment: &g.fragment,
                                color: &g.color,
                                depth: g.depth,
                                depth_test: g.depth_test,
                                depth_write: g.depth_write,
                                cull: g.cull,
                                topology: g.topology,
                                blend: g.blend,
                            },
                        ),
                    };
                    match pipeline {
                        Ok(p) => built.push(p),
                        Err(e) => {
                            // SAFETY: pipelines this worker made and nothing
                            // else has seen.
                            for p in built {
                                unsafe { device.destroy_pipeline(p, None) };
                            }
                            return Err(e);
                        }
                    }
                }
                Ok((built, started.elapsed().as_secs_f32() * 1000.0))
            })
            .expect("a pipeline thread");
        Pending {
            thread: Some(thread),
        }
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

fn build_graphics(
    device: &ash::Device,
    cache: vk::PipelineCache,
    layout: vk::PipelineLayout,
    desc: &GraphicsDesc,
) -> Result<vk::Pipeline, SetupError> {
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
                .blend_enable(desc.blend)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD)
        })
        .collect();
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
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
        .layout(layout)
        .push_next(&mut rendering)];
    // SAFETY: every state struct is a local that outlives the call.
    unsafe { device.create_graphics_pipelines(cache, &info, None) }
        .map(|p| p[0])
        .map_err(|(_, e)| e.into())
}

fn build_compute(
    device: &ash::Device,
    cache: vk::PipelineCache,
    layout: vk::PipelineLayout,
    module: vk::ShaderModule,
    entry: &std::ffi::CStr,
) -> Result<vk::Pipeline, SetupError> {
    let stage = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(module)
        .name(entry);
    let info = [vk::ComputePipelineCreateInfo::default()
        .stage(stage)
        .layout(layout)];
    // SAFETY: the module and layout are live.
    unsafe { device.create_compute_pipelines(cache, &info, None) }
        .map(|p| p[0])
        .map_err(|(_, e)| e.into())
}

/// A pipeline for `build_on_worker`, with what it borrows owned so it can
/// cross to the worker.
pub enum Spec {
    Compute {
        module: vk::ShaderModule,
        entry: CString,
    },
    Graphics(GraphicsSpec),
}

/// `GraphicsDesc`, owned.
pub struct GraphicsSpec {
    pub module: vk::ShaderModule,
    pub vertex: CString,
    pub fragment: CString,
    pub color: Vec<vk::Format>,
    pub depth: Option<vk::Format>,
    pub depth_test: Option<vk::CompareOp>,
    pub depth_write: bool,
    pub cull: vk::CullModeFlags,
    pub topology: vk::PrimitiveTopology,
    pub blend: bool,
}

/// A worker's batch: its pipelines and milliseconds, or why it failed.
pub type Built = Result<(Vec<vk::Pipeline>, f32), SetupError>;

/// Pipelines a worker is building: the pipelines in `Spec` order and the
/// milliseconds the batch took.
pub struct Pending {
    thread: Option<JoinHandle<Built>>,
}

impl Pending {
    /// The result once the worker is done, without waiting; `None` while it
    /// works (and after the result was taken).
    pub fn take(&mut self) -> Option<Built> {
        if !self.thread.as_ref()?.is_finished() {
            return None;
        }
        self.wait()
    }

    /// Waits for the result.
    pub fn wait(&mut self) -> Option<Built> {
        let thread = self.thread.take()?;
        Some(
            thread
                .join()
                .unwrap_or_else(|_| Err(SetupError("the pipeline thread panicked".to_owned()))),
        )
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

#[cfg(test)]
mod tests {
    use super::{PUSH_SIZE, Push};

    /// Every shader's bindless bindings and push block, as slangc reflects
    /// them, match `bindless.rs` and `Push`: a layout mismatch between the
    /// Slang interface and Rust fails here, not on the GPU.
    #[test]
    fn every_shader_matches_the_rust_layouts() {
        let fields = [
            ("ids", std::mem::offset_of!(Push, ids)),
            ("more", std::mem::offset_of!(Push, more)),
            ("params", std::mem::offset_of!(Push, params)),
            ("view_proj", std::mem::offset_of!(Push, view_proj)),
        ];
        assert!(!crate::vk::shaders::REFLECTIONS.is_empty());
        for (name, json) in crate::vk::shaders::REFLECTIONS {
            let reflection: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
            let parameters = reflection["parameters"].as_array().expect("parameters");
            let find = |wanted: &str| {
                parameters
                    .iter()
                    .find(|p| p["name"] == wanted)
                    .unwrap_or_else(|| panic!("{name}: no {wanted}"))
            };
            for (variable, index) in [("g_textures", 0), ("g_samplers", 1), ("g_buffers", 2)] {
                let binding = &find(variable)["binding"];
                assert_eq!(binding["kind"], "descriptorTableSlot", "{name}: {variable}");
                assert_eq!(binding["index"], index, "{name}: {variable}");
            }
            let push = find("pc");
            assert_eq!(push["binding"]["kind"], "pushConstantBuffer", "{name}");
            let reflected = push["type"]["elementType"]["fields"]
                .as_array()
                .expect("push fields");
            assert_eq!(reflected.len(), fields.len(), "{name}: push field count");
            let mut end = 0;
            for (field, (rust_name, rust_offset)) in reflected.iter().zip(fields) {
                assert_eq!(field["name"], rust_name, "{name}: push field order");
                assert_eq!(
                    field["binding"]["offset"].as_u64(),
                    Some(rust_offset as u64),
                    "{name}: {rust_name} offset"
                );
                end = rust_offset as u64 + field["binding"]["size"].as_u64().unwrap_or(0);
            }
            assert_eq!(end, PUSH_SIZE as u64, "{name}: push block size");
        }
    }
}
