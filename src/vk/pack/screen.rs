//! A pack's screen passes on the native renderer (6.2, step 2b): begin,
//! prepare, deferred and composite, then final into the output.
//!
//! Flips follow `packdraw::plan`: a pass samples each target's current half
//! (the other half for one it also draws), writes the other, then flips.
//! Until the geometry passes come (6.3), the scene's frame stands in for
//! what the gbuffers would write to `colortex0`. Compute steps are skipped.

use ash::vk;
use bevy::math::{Mat4, UVec2};
use gpu_allocator::MemoryLocation;

use super::bindless::{Slot, SlotKind};
use super::targets::{DEPTH_FORMAT, PackTargets};
use crate::renderer::packdraw::compile::{ScreenStage, Step};
use crate::renderer::packdraw::install::{InstalledProgram, Installed};
use crate::renderer::packdraw::sources::{Depth, Stub, TextureSource};
use crate::renderer::packdraw::uniforms::{BlockSpec, FrameValues, History, UniformBytes};
use crate::shaderpack::directives::{MAX_COLOR_TARGETS, SampleKind};
use crate::shaderpack::expressions::CustomState;
use crate::vk::bindless::{self as bl, Bindless};
use crate::vk::frame::FRAMES_IN_FLIGHT;
use crate::vk::graph::{Graph, Use};
use crate::vk::memory::{Buffer, Image, Memory};
use crate::vk::pipeline::{GraphicsDesc, Pipelines, Push, push};
use crate::vk::{Context, SetupError};

fn failed(what: impl std::fmt::Display) -> SetupError {
    SetupError(what.to_string())
}

/// Bit `n` set: target `n` reads half 1.
#[derive(Clone, Copy, Default)]
struct Flips(u16);

impl Flips {
    fn current(self, n: u8) -> usize {
        ((self.0 >> n) & 1) as usize
    }
    fn other(self, n: u8) -> usize {
        1 - self.current(n)
    }
    fn flip(&mut self, buffers: &[u8]) {
        for b in buffers {
            self.0 ^= 1 << b;
        }
    }
}

/// 1x1 stand-ins for sources the frame lacks.
struct Stubs {
    white: (Image, u32),
    black: (Image, u32),
    flat_normal: (Image, u32),
    uint: (Image, u32),
    depth: (Image, u32),
    noise: (Image, u32),
}

/// The bindless index each image is registered under.
struct Indexed {
    /// Per colour target, its halves.
    color: [Option<[u32; 2]>; MAX_COLOR_TARGETS],
    depth: u32,
    depth_opaque: u32,
    depth_no_hand: Option<u32>,
    shadow_depth: Option<[u32; 2]>,
    shadow_color: Vec<Option<u32>>,
}

struct Pass {
    program: usize,
    pipeline: vk::Pipeline,
    modules: Vec<vk::ShaderModule>,
    base: u32,
    /// Target and half drawn, or `None` for the output.
    attachments: Option<Vec<(u8, usize)>>,
    sampled: Vec<vk::Image>,
    /// The program's uniform block and where its slot sits in the table.
    uniform: Option<(usize, usize)>,
    extent: vk::Extent2D,
}

pub(crate) struct PackScreen {
    installed: Installed,
    targets: PackTargets,
    indexed: Indexed,
    stubs: Stubs,
    buffers: Vec<(Buffer, u32)>,
    passes: Vec<Pass>,
    /// Per program with a uniform block: its bytes and a buffer per frame.
    uniforms: Vec<(UniformBytes, Vec<(Buffer, u32)>)>,
    table: Vec<u32>,
    tables: Vec<(Buffer, u32)>,
    staging: Option<(Buffer, Option<u64>)>,
    settled: bool,
    customs: CustomState,
    history: History,
    frames: u32,
    last: Option<f32>,
    pub(crate) skipped_computes: usize,
}

fn sampler_for(installed: &Installed, source: TextureSource, comparison: bool) -> u32 {
    if comparison {
        return if installed.shadow.is_some() {
            bl::SAMPLER_COMPARE_LINEAR
        } else {
            bl::SAMPLER_COMPARE_ALWAYS
        };
    }
    match source {
        TextureSource::Noise => bl::SAMPLER_LINEAR,
        TextureSource::Atlas | TextureSource::Lightmap | TextureSource::EntityTexture => {
            bl::SAMPLER_NEAREST_CLAMP
        }
        TextureSource::Custom(i) | TextureSource::Custom3d(i) => {
            match installed.images.get(usize::from(i)).map(|im| (im.blur, im.clamp)) {
                Some((true, true)) => bl::SAMPLER_LINEAR_CLAMP,
                Some((false, true)) => bl::SAMPLER_NEAREST_CLAMP,
                Some((true, false)) => bl::SAMPLER_LINEAR,
                _ => bl::SAMPLER_NEAREST,
            }
        }
        _ => match source.sample_kind(&installed.targets) {
            SampleKind::Float => bl::SAMPLER_LINEAR_CLAMP,
            _ => bl::SAMPLER_NEAREST_CLAMP,
        },
    }
}

impl PackScreen {
    #[allow(clippy::too_many_arguments, reason = "setup's whole context")]
    pub(crate) fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        installed: Installed,
        size: vk::Extent2D,
        output: vk::Format,
    ) -> Result<PackScreen, SetupError> {
        let targets = PackTargets::new(ctx, memory, &installed, size, true)?;
        let full = || failed("the bindless set is full");
        let texture = |bindless: &mut Bindless, image: &Image| {
            bindless
                .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(full)
        };

        let mut color = [None; MAX_COLOR_TARGETS];
        for (n, slot) in targets.color.iter().enumerate() {
            if let Some((pair, _)) = slot {
                color[n] = Some([texture(bindless, &pair[0])?, texture(bindless, &pair[1])?]);
            }
        }
        let (shadow_depth, shadow_color) = match &targets.shadow {
            Some(s) => (
                Some([texture(bindless, &s.depth)?, texture(bindless, &s.depth_opaque)?]),
                s.color
                    .iter()
                    .map(|c| c.as_ref().map(|c| texture(bindless, c)).transpose())
                    .collect::<Result<_, _>>()?,
            ),
            None => (None, Vec::new()),
        };
        let indexed = Indexed {
            color,
            depth: texture(bindless, &targets.depth)?,
            depth_opaque: texture(bindless, &targets.depth_opaque)?,
            depth_no_hand: targets
                .depth_no_hand
                .as_ref()
                .map(|d| texture(bindless, d))
                .transpose()?,
            shadow_depth,
            shadow_color,
        };

        // Stubs and noise, uploaded on the first frame.
        let one = vk::Extent2D { width: 1, height: 1 };
        let sampled = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
        let color_aspect = vk::ImageAspectFlags::COLOR;
        let mut stub = |name: &str, format: vk::Format, aspect, extent| -> Result<(Image, u32), SetupError> {
            let image = memory.image(ctx, name, extent, format, sampled, aspect)?;
            let index = texture(bindless, &image)?;
            Ok((image, index))
        };
        let noise_size = installed.constants.noise_resolution.clamp(1, 1024);
        let stubs = Stubs {
            white: stub("pack white", vk::Format::R8G8B8A8_UNORM, color_aspect, one)?,
            black: stub("pack black", vk::Format::R8G8B8A8_UNORM, color_aspect, one)?,
            flat_normal: stub("pack flat normal", vk::Format::R8G8B8A8_UNORM, color_aspect, one)?,
            uint: stub("pack uint", vk::Format::R32_UINT, color_aspect, one)?,
            depth: stub("pack depth stub", DEPTH_FORMAT, vk::ImageAspectFlags::DEPTH, one)?,
            noise: stub(
                "pack noise",
                vk::Format::R8G8B8A8_UNORM,
                color_aspect,
                vk::Extent2D {
                    width: noise_size,
                    height: noise_size,
                },
            )?,
        };
        let mut staging_bytes = vec![255u8, 255, 255, 255, 0, 0, 0, 255, 127, 127, 255, 255];
        staging_bytes.extend(noise_bytes(noise_size));
        let mut staging = memory.buffer(
            ctx,
            "pack staging",
            staging_bytes.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        staging.write(0, &staging_bytes);

        let buffers = installed
            .buffers
            .iter()
            .map(|b| -> Result<(Buffer, u32), SetupError> {
                let bytes = b.bytes([size.width, size.height]).div_ceil(4) * 4;
                let buffer = memory.buffer(
                    ctx,
                    "pack buffer object",
                    bytes.max(16),
                    vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                    MemoryLocation::GpuOnly,
                )?;
                let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
                Ok((buffer, index))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut screen = PackScreen {
            targets,
            indexed,
            stubs,
            buffers,
            passes: Vec::new(),
            uniforms: Vec::new(),
            table: Vec::new(),
            tables: Vec::new(),
            staging: Some((staging, None)),
            settled: false,
            customs: installed.customs.state(),
            history: History::default(),
            frames: 0,
            last: None,
            skipped_computes: 0,
            installed,
        };

        // The plan: each stage's screen steps in order, then final.
        let mut flips = Flips::default();
        for stage in ScreenStage::ALL {
            let steps: Vec<Step> = screen.installed.stages[stage.index()].clone();
            for step in steps {
                match step {
                    Step::Screen(index) => {
                        let pass = screen.pass(ctx, memory, bindless, pipelines, index, flips, None)?;
                        screen.passes.push(pass);
                        let program = &screen.installed.programs[index];
                        flips.flip(&program.draw_buffers);
                        let toggles = screen
                            .installed
                            .flips
                            .toggles(&program.name, &program.draw_buffers);
                        flips.flip(&toggles);
                    }
                    Step::Compute(_) => screen.skipped_computes += 1,
                }
            }
        }
        if let Some(index) = screen.installed.final_pass {
            let pass = screen.pass(ctx, memory, bindless, pipelines, index, flips, Some(output))?;
            screen.passes.push(pass);
        }

        // One slot table per frame in flight; their uniform slots differ.
        for _ in 0..FRAMES_IN_FLIGHT {
            let buffer = memory.buffer(
                ctx,
                "pack slot table",
                (screen.table.len().max(4) * 4) as u64,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            screen.tables.push((buffer, index));
        }
        Ok(screen)
    }

    fn texture_index(&self, source: TextureSource, flips: Flips, drawing: u16, ty: &str) -> (u32, vk::Image) {
        let stub = |s: &(Image, u32)| (s.1, s.0.raw);
        let wants_depth = ty == "texture_depth_2d";
        let wants_uint = ty.contains("<u32>");
        let t = &self.targets;
        let picked = match source {
            TextureSource::Color(n) => match (&t.color[n as usize], self.indexed.color[n as usize]) {
                (Some((pair, _)), Some(idx)) => {
                    let half = if drawing & (1 << n) != 0 {
                        flips.other(n)
                    } else {
                        flips.current(n)
                    };
                    Some((idx[half], pair[half].raw))
                }
                _ => None,
            },
            TextureSource::Depth(Depth::All) => Some((self.indexed.depth, t.depth.raw)),
            TextureSource::Depth(Depth::Opaque) => Some((self.indexed.depth_opaque, t.depth_opaque.raw)),
            TextureSource::DepthNoHand => t
                .depth_no_hand
                .as_ref()
                .zip(self.indexed.depth_no_hand)
                .map(|(d, i)| (i, d.raw)),
            TextureSource::ShadowDepth(d) | TextureSource::ShadowCompare(d) => {
                t.shadow.as_ref().zip(self.indexed.shadow_depth).map(|(s, i)| match d {
                    Depth::All => (i[0], s.depth.raw),
                    Depth::Opaque => (i[1], s.depth_opaque.raw),
                })
            }
            TextureSource::ShadowColor(n) => t.shadow.as_ref().and_then(|s| {
                let image = s.color.get(n as usize)?.as_ref()?;
                Some((self.indexed.shadow_color.get(n as usize).copied().flatten()?, image.raw))
            }),
            TextureSource::Noise => Some(stub(&self.stubs.noise)),
            TextureSource::Stub(Stub::FlatNormal) => Some(stub(&self.stubs.flat_normal)),
            TextureSource::Stub(Stub::Black2d) => Some(stub(&self.stubs.black)),
            _ => None,
        };
        // Keep the slot's declared kind: a mismatch gets a stub of that kind.
        let is_depth = |image: vk::Image| {
            [t.depth.raw, t.depth_opaque.raw, self.stubs.depth.0.raw].contains(&image)
                || t.depth_no_hand.as_ref().is_some_and(|d| d.raw == image)
                || t.shadow
                    .as_ref()
                    .is_some_and(|s| s.depth.raw == image || s.depth_opaque.raw == image)
        };
        match picked {
            Some(p) if wants_depth == is_depth(p.1) && !wants_uint => p,
            _ if wants_depth => stub(&self.stubs.depth),
            _ if wants_uint => stub(&self.stubs.uint),
            Some(p) => p,
            None => stub(&self.stubs.white),
        }
    }

    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn pass(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        index: usize,
        flips: Flips,
        output: Option<vk::Format>,
    ) -> Result<Pass, SetupError> {
        let program: &InstalledProgram = &self.installed.programs[index];
        let fragment = match (output, &program.fragment_linear) {
            (Some(f), Some(linear)) if format!("{f:?}").ends_with("SRGB") => linear,
            _ => &program.fragment,
        };
        // One slot list for both stages, so a shared resource keeps its index.
        let mut slots: Vec<Slot> = Vec::new();
        let mut modules = Vec::new();
        for module in [&program.vertex, fragment] {
            let wgsl = super::bindless::rewrite_shared(&module.wgsl, &mut slots);
            let words = crate::shaderpack::backend::wgsl_to_spirv_aliased(&wgsl)
                .map_err(|e| failed(format!("{}: {e}", module.label)))?;
            modules.push(pipelines.module(ctx, bytemuck::cast_slice(&words))?);
        }

        // Final draws to the output, never into a target it samples.
        let drawing: u16 = if output.is_some() {
            0
        } else {
            program.draw_buffers.iter().fold(0, |m, b| m | 1 << b)
        };
        let base = self.table.len() as u32;
        let mut sampled = Vec::new();
        let mut uniform = None;
        for slot in &slots {
            let word = match &slot.kind {
                SlotKind::Texture(ty) => {
                    let source = program.texture_source(slot.binding);
                    let (i, image) = self.texture_index(source, flips, drawing, ty);
                    sampled.push(image);
                    i
                }
                SlotKind::Sampler | SlotKind::Comparison => {
                    let source = program.texture_source(slot.binding.saturating_sub(1));
                    sampler_for(&self.installed, source, slot.kind == SlotKind::Comparison)
                }
                SlotKind::Buffer { ty, .. } if ty.contains("IrisFrame") || ty.starts_with("Iris") && !ty.contains("Slot") => {
                    // The frame's uniform block: filled per frame.
                    let bytes = UniformBytes::new(
                        [BlockSpec {
                            layout: &program.layout.uniforms,
                            set: program.kind.matrices(),
                            render_stage: program.kind.render_stage(),
                        }],
                        &self.installed.customs,
                        16,
                    );
                    let mut per_frame = Vec::new();
                    for _ in 0..FRAMES_IN_FLIGHT {
                        let buffer = memory.buffer(
                            ctx,
                            "pack uniforms",
                            bytes.bytes.len().max(16) as u64,
                            vk::BufferUsageFlags::STORAGE_BUFFER,
                            MemoryLocation::CpuToGpu,
                        )?;
                        let i = bindless
                            .add_buffer(ctx, buffer.raw)
                            .ok_or_else(|| failed("the bindless set is full"))?;
                        per_frame.push((buffer, i));
                    }
                    self.uniforms.push((bytes, per_frame));
                    uniform = Some((self.uniforms.len() - 1, self.table.len() + slots.iter().position(|s| s == slot).unwrap_or(0)));
                    0
                }
                SlotKind::Buffer { .. } => {
                    // A buffer object, by the binding's buffer index.
                    let at = program.layout.bindings.iter().find_map(|b| match b {
                        crate::shaderpack::pipeline::Binding::Buffer { binding, index, .. }
                            if *binding == slot.binding =>
                        {
                            Some(*index)
                        }
                        _ => None,
                    });
                    at.and_then(|i| {
                        self.installed
                            .buffers
                            .iter()
                            .position(|b| b.index == i)
                    })
                    .map_or(0, |p| self.buffers[p].1)
                }
            };
            self.table.push(word);
        }

        let (attachments, formats, extent) = match output {
            Some(format) => (None, vec![format], self.targets.size),
            None => {
                let list: Vec<(u8, usize)> = program
                    .draw_buffers
                    .iter()
                    .map(|&n| (n, flips.other(n)))
                    .collect();
                let formats = list
                    .iter()
                    .map(|(n, _)| {
                        self.targets.color[*n as usize]
                            .as_ref()
                            .map_or(vk::Format::R8G8B8A8_UNORM, |(_, f)| *f)
                    })
                    .collect();
                let extent = list
                    .first()
                    .and_then(|(n, h)| self.targets.color[*n as usize].as_ref().map(|(p, _)| p[*h].extent))
                    .unwrap_or(self.targets.size);
                (Some(list), formats, extent)
            }
        };
        let pipeline = pipelines.graphics(
            ctx,
            &GraphicsDesc {
                module: modules[0],
                fragment_module: Some(modules[1]),
                vertex: c"main",
                fragment: c"main",
                color: &formats,
                depth: None,
                depth_test: None,
                depth_write: false,
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: false,
                additive: false,
            },
        )?;
        Ok(Pass {
            program: index,
            pipeline,
            modules,
            base,
            attachments,
            sampled,
            uniform,
            extent,
        })
    }

    fn all_images(&self) -> Vec<(vk::Image, vk::ImageAspectFlags)> {
        let color = vk::ImageAspectFlags::COLOR;
        let depth = vk::ImageAspectFlags::DEPTH;
        let t = &self.targets;
        let mut out: Vec<(vk::Image, vk::ImageAspectFlags)> = t
            .color
            .iter()
            .flatten()
            .flat_map(|(pair, _)| pair.iter().map(|i| (i.raw, color)))
            .collect();
        out.push((t.depth.raw, depth));
        out.push((t.depth_opaque.raw, depth));
        out.extend(t.depth_no_hand.iter().map(|d| (d.raw, depth)));
        if let Some(s) = &t.shadow {
            out.push((s.depth.raw, depth));
            out.push((s.depth_opaque.raw, depth));
            out.extend(s.color.iter().flatten().map(|c| (c.raw, color)));
        }
        let s = &self.stubs;
        for (image, aspect) in [
            (&s.white.0, color),
            (&s.black.0, color),
            (&s.flat_normal.0, color),
            (&s.uint.0, color),
            (&s.depth.0, depth),
            (&s.noise.0, color),
        ] {
            out.push((image.raw, aspect));
        }
        out
    }

    /// Runs the chain over `output` (holding the scene's frame) and draws
    /// the final pass back into it.
    #[allow(clippy::too_many_arguments, reason = "one frame's whole context")]
    pub(crate) fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        output: (vk::Image, vk::ImageView, vk::Extent2D),
        camera: (Mat4, f32),
        frame: u64,
        slot: usize,
        seconds: f32,
    ) {
        let device = &ctx.device;
        let images = self.all_images();
        let layout = if self.settled {
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
        } else {
            vk::ImageLayout::UNDEFINED
        };
        for (image, aspect) in &images {
            graph.image(*image, *aspect, layout);
        }

        // First frame: the stubs' and noise's texels.
        if let Some((staging, copied)) = &mut self.staging {
            match copied {
                None => {
                    *copied = Some(frame);
                    let source = staging.raw;
                    let s = &self.stubs;
                    let one = vk::Extent3D { width: 1, height: 1, depth: 1 };
                    let copies = [
                        (s.white.0.raw, 0u64, one),
                        (s.black.0.raw, 4, one),
                        (s.flat_normal.0.raw, 8, one),
                        (
                            s.noise.0.raw,
                            12,
                            vk::Extent3D {
                                width: s.noise.0.extent.width,
                                height: s.noise.0.extent.height,
                                depth: 1,
                            },
                        ),
                    ];
                    let uses: Vec<_> = copies.iter().map(|c| (c.0, Use::TransferDst)).collect();
                    graph.pass("pack stubs", &uses, &[], |cmd| unsafe {
                        // SAFETY: copies from the staging into images in
                        // TRANSFER_DST, as the graph set.
                        for (image, offset, extent) in copies {
                            device.cmd_copy_buffer_to_image(
                                cmd,
                                source,
                                image,
                                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                &[vk::BufferImageCopy {
                                    buffer_offset: offset,
                                    image_subresource: layers(vk::ImageAspectFlags::COLOR),
                                    image_extent: extent,
                                    ..Default::default()
                                }],
                            );
                        }
                    });
                }
                Some(at) if frame >= *at + FRAMES_IN_FLIGHT as u64 => {
                    if let Some((staging, _)) = self.staging.take() {
                        memory.free_buffer(ctx, staging);
                    }
                }
                _ => {}
            }
        }

        // Clears: cleared colour targets' read halves, and every depth.
        let fog = [0.0, 0.0, 0.0, 1.0];
        let mut clears: Vec<(vk::Image, Option<[f32; 4]>)> = Vec::new();
        for (n, slot) in self.targets.color.iter().enumerate() {
            if let Some((pair, _)) = slot {
                let settings = &self.installed.targets.settings[n];
                if settings.clear {
                    clears.push((pair[0].raw, Some(settings.clear_color(n, fog))));
                }
            }
        }
        let t = &self.targets;
        clears.push((t.depth.raw, None));
        clears.push((t.depth_opaque.raw, None));
        clears.extend(t.depth_no_hand.iter().map(|d| (d.raw, None)));
        clears.push((self.stubs.depth.0.raw, None));
        clears.push((self.stubs.uint.0.raw, Some([0.0; 4])));
        if let Some(s) = &t.shadow {
            clears.push((s.depth.raw, None));
            clears.push((s.depth_opaque.raw, None));
        }
        let uses: Vec<_> = clears.iter().map(|c| (c.0, Use::TransferDst)).collect();
        graph.pass("pack clears", &uses, &[], |cmd| unsafe {
            // SAFETY: clears of images in TRANSFER_DST.
            for (image, color) in &clears {
                match color {
                    Some(c) => device.cmd_clear_color_image(
                        cmd,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearColorValue { float32: *c },
                        &[crate::vk::memory::range(vk::ImageAspectFlags::COLOR)],
                    ),
                    None => device.cmd_clear_depth_stencil_image(
                        cmd,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                        &[crate::vk::memory::range(vk::ImageAspectFlags::DEPTH)],
                    ),
                }
            }
        });

        // The scene's frame into colortex0, as the gbuffers would draw it:
        // bottom row first, as pack targets are stored.
        if let Some((pair, _)) = &self.targets.color[0] {
            let target = &pair[0];
            let (src, dst) = (output.2, target.extent);
            graph.pass(
                "pack scene copy",
                &[(output.0, Use::TransferSrc), (target.raw, Use::TransferDst)],
                &[],
                |cmd| unsafe {
                    // SAFETY: a blit between images in transfer layouts.
                    let corner = |e: vk::Extent2D| vk::Offset3D {
                        x: e.width as i32,
                        y: e.height as i32,
                        z: 1,
                    };
                    device.cmd_blit_image(
                        cmd,
                        output.0,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        target.raw,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::ImageBlit {
                            src_subresource: layers(vk::ImageAspectFlags::COLOR),
                            src_offsets: [vk::Offset3D::default(), corner(src)],
                            dst_subresource: layers(vk::ImageAspectFlags::COLOR),
                            dst_offsets: [
                                vk::Offset3D {
                                    x: 0,
                                    y: dst.height as i32,
                                    z: 0,
                                },
                                vk::Offset3D {
                                    x: dst.width as i32,
                                    y: 0,
                                    z: 1,
                                },
                            ],
                        }],
                        vk::Filter::LINEAR,
                    );
                },
            );
        }

        // This frame's uniforms and slot table.
        let dt = self.last.map_or(1.0 / 60.0, |l| (seconds - l).max(0.0));
        self.last = Some(seconds);
        self.frames += 1;
        let (world_from_view, fov) = camera;
        let values = FrameValues::of_camera(
            world_from_view,
            1.0 / (fov * 0.5).tan(),
            UVec2::new(output.2.width, output.2.height),
            seconds,
            dt,
            self.frames,
            256.0,
            Default::default(),
            &self.installed.constants,
            &mut self.history,
        );
        self.installed
            .customs
            .evaluate(&mut self.customs, dt, &|id| values.input(id));
        let mut table = self.table.clone();
        for pass in &self.passes {
            if let Some((u, at)) = pass.uniform {
                table[at] = self.uniforms[u].1[slot].1;
            }
        }
        for (bytes, per_frame) in &mut self.uniforms {
            bytes.update(&values, &self.customs);
            per_frame[slot].0.write(0, &bytes.bytes);
        }
        self.tables[slot].0.write(0, bytemuck::cast_slice(&table));
        let table_index = self.tables[slot].1;
        let mut buffers: Vec<(vk::Buffer, Use)> = vec![(self.tables[slot].0.raw, Use::SampledFragment)];
        buffers.extend(self.uniforms.iter().map(|(_, f)| (f[slot].0.raw, Use::SampledFragment)));
        buffers.extend(self.buffers.iter().map(|(b, _)| (b.raw, Use::SampledFragment)));

        for pass in &self.passes {
            let mut uses: Vec<(vk::Image, Use)> = pass
                .sampled
                .iter()
                .map(|i| (*i, Use::SampledFragment))
                .collect();
            let views: Vec<vk::ImageView> = match &pass.attachments {
                Some(list) => list
                    .iter()
                    .filter_map(|(n, h)| self.targets.color[*n as usize].as_ref().map(|(p, _)| &p[*h]))
                    .map(|image| {
                        uses.retain(|u| u.0 != image.raw);
                        uses.push((image.raw, Use::ColorAttachment));
                        image.view
                    })
                    .collect(),
                None => {
                    uses.push((output.0, Use::ColorAttachment));
                    vec![output.1]
                }
            };
            let (pipeline, base, extent) = (pass.pipeline, pass.base, pass.extent);
            graph.pass("pack screen", &uses, &buffers, |cmd| unsafe {
                // SAFETY: attachments and sampled images in the layouts the
                // graph set; the shared layout and set.
                let colors: Vec<vk::RenderingAttachmentInfo> = views
                    .iter()
                    .map(|v| {
                        vk::RenderingAttachmentInfo::default()
                            .image_view(*v)
                            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                            .load_op(vk::AttachmentLoadOp::LOAD)
                            .store_op(vk::AttachmentStoreOp::STORE)
                    })
                    .collect();
                device.cmd_begin_rendering(
                    cmd,
                    &vk::RenderingInfo::default()
                        .render_area(vk::Rect2D {
                            offset: vk::Offset2D::default(),
                            extent,
                        })
                        .layer_count(1)
                        .color_attachments(&colors),
                );
                // A plain viewport, as wgpu's: the SPIR-V flips clip y.
                device.cmd_set_viewport(
                    cmd,
                    0,
                    &[vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: extent.width as f32,
                        height: extent.height as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                );
                device.cmd_set_scissor(
                    cmd,
                    0,
                    &[vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent,
                    }],
                );
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.layout,
                    0,
                    &[bindless.set],
                    &[],
                );
                push(
                    ctx,
                    cmd,
                    pipelines.layout,
                    &Push {
                        ids: [table_index, base, 0, 0],
                        more: [0; 4],
                        params: [0.0; 4],
                        view_proj: [[0.0; 4]; 4],
                    },
                );
                device.cmd_draw(cmd, 3, 1, 0, 0);
                device.cmd_end_rendering(cmd);
            });
        }

        // Leave every pack image sampled-ready for the next frame.
        let settle: Vec<_> = images.iter().map(|(i, _)| (*i, Use::SampledFragment)).collect();
        graph.pass("pack settle", &settle, &[], |_| {});
        self.settled = true;
    }

    pub(crate) fn describe(&self) -> String {
        let names: Vec<&str> = self
            .passes
            .iter()
            .map(|p| self.installed.programs[p.program].name.as_str())
            .collect();
        format!(
            "{} passes ({}), {} compute steps skipped",
            self.passes.len(),
            names.join(" "),
            self.skipped_computes
        )
    }

    pub(crate) fn destroy(self, ctx: &Context, memory: &Memory) {
        // SAFETY: the caller idles the device first.
        unsafe {
            for pass in &self.passes {
                ctx.device.destroy_pipeline(pass.pipeline, None);
                for m in &pass.modules {
                    ctx.device.destroy_shader_module(*m, None);
                }
            }
        }
        let s = self.stubs;
        for image in [s.white.0, s.black.0, s.flat_normal.0, s.uint.0, s.depth.0, s.noise.0] {
            memory.free_image(ctx, image);
        }
        for (b, _) in self.buffers.into_iter().chain(self.tables) {
            memory.free_buffer(ctx, b);
        }
        for (_, per_frame) in self.uniforms {
            for (b, _) in per_frame {
                memory.free_buffer(ctx, b);
            }
        }
        if let Some((staging, _)) = self.staging {
            memory.free_buffer(ctx, staging);
        }
        self.targets.destroy(ctx, memory);
    }
}

fn layers(aspect: vk::ImageAspectFlags) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        aspect_mask: aspect,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// The game's noise (`packdraw::targets::noise_bytes`).
fn noise_bytes(size: u32) -> Vec<u8> {
    let mut state: u32 = 0x9e37_79b9;
    (0..size * size * 4)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 24) as u8
        })
        .collect()
}
