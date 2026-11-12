//! Particles on the native renderer (VULKAN_REWRITE_CHECKLIST.md 4.6):
//! one sprite per live particle in a storage buffer, billboarded in the
//! vertex shader (`shaders/particle.slang`), six vertices a sprite with no
//! index buffer. The game builds a mesh per batch on the CPU every frame
//! instead (`util::particles::draw::write_batch_mesh`); here a frame's
//! particles are one buffer write and two draws: the opaque layer, cut out
//! and writing depth, then the translucent layer, blended, oldest first as
//! vanilla draws it.
//!
//! For now the sprites come from the game's particle snapshot
//! (`entity_dump::read_particles`); feeding them each frame comes with the
//! game.

use ash::vk;
use bevy::math::Vec3;
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::depth_view::DEPTH_FORMAT;
use super::entity_dump::{PARTICLE_TRANSLUCENT, ParticleSnapshot, ParticleSprite};
use super::frame::FRAMES_IN_FLIGHT;
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsSpec, Pending, Pipelines, Push, Spec, push};
use super::{Context, SetupError};

/// What the particles drew, for the viewer's summary.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParticleStats {
    pub sprites: usize,
    pub translucent: usize,
    pub textures: usize,
}

#[derive(Clone, Copy)]
struct Pipes {
    cutout: vk::Pipeline,
    blend: vk::Pipeline,
}

pub struct ParticleScene {
    module: vk::ShaderModule,
    pipes: Option<Pipes>,
    pending: Option<Pending>,
    sprites: (Buffer, u32),
    textures: Vec<(Image, u32)>,
    texture_sizes: Vec<(u32, u32)>,
    /// The staging, the textures' offsets in it, and the frame it was
    /// copied in.
    staging: Option<(Buffer, Vec<u64>, Option<u64>)>,
    /// Opaque sprites first, then translucent: how many of each.
    cutout: u32,
    translucent: u32,
    pub stats: ParticleStats,
    /// The live renderer's state (`update`), for a scene kept across frames.
    live: Option<Live>,
}

/// A live scene's textures (kept, by stable index) and what this frame
/// uploads.
#[derive(Default)]
struct Live {
    /// Per stable texture index: its bindless index.
    textures: Vec<Option<u32>>,
    /// New textures waiting for `prepare`: place in `textures`, size, texels.
    images: Vec<(usize, u32, u32, Vec<u8>)>,
    /// This frame's sprite bytes.
    sprites: Option<Vec<u8>>,
    /// Per frame in flight: a staging buffer, grown to fit.
    staging: Vec<Option<Buffer>>,
    retired: Vec<(Buffer, u64)>,
}

fn full() -> SetupError {
    SetupError("the bindless set is full".to_owned())
}

impl ParticleScene {
    pub fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        color_format: vk::Format,
        snapshot: ParticleSnapshot,
        reuse: Option<&mut ParticleScene>,
    ) -> Result<ParticleScene, SetupError> {
        // Take the old scene's shader and pipelines instead of building them.
        let reused = reuse.map(|old| {
            (
                std::mem::replace(&mut old.module, vk::ShaderModule::null()),
                old.pipes.take(),
                old.pending.take(),
            )
        });
        let module = match &reused {
            Some((m, ..)) => *m,
            None => pipelines.module(ctx, super::shaders::PARTICLE)?,
        };
        let stream = |fragment: &std::ffi::CStr, depth_write, compare, blend| {
            Spec::Graphics(GraphicsSpec {
                module,
                vertex: c"particle_vertex".to_owned(),
                fragment: fragment.to_owned(),
                color: vec![color_format],
                depth: Some(DEPTH_FORMAT),
                depth_test: Some(compare),
                depth_write,
                // Sprites face the camera; either winding draws.
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend,
                additive: false,
                fragment_module: None,
            })
        };
        // A scene renewed each frame (live) keeps its pipelines.
        let pending = match reused {
            Some(_) => None,
            None => Some(pipelines.build_on_worker(
                ctx,
                vec![
                    stream(
                        c"particle_cutout",
                        true,
                        vk::CompareOp::GREATER_OR_EQUAL,
                        false,
                    ),
                    // Vanilla's translucent particles test depth and leave it.
                    stream(
                        c"particle_blend",
                        false,
                        vk::CompareOp::GREATER_OR_EQUAL,
                        true,
                    ),
                ],
            )),
        };

        // Opaque sprites first, then translucent, each in the snapshot's
        // (spawn) order.
        let (mut sprites, translucent): (Vec<ParticleSprite>, Vec<ParticleSprite>) = snapshot
            .sprites
            .into_iter()
            .partition(|s| s.flags & PARTICLE_TRANSLUCENT == 0);
        let cutout = sprites.len() as u32;
        let translucent_count = translucent.len() as u32;
        sprites.extend(translucent);

        let mut textures = Vec::new();
        let mut texture_sizes = Vec::new();
        for (w, h, _) in &snapshot.textures {
            let image = memory.image(
                ctx,
                "particle texture",
                vk::Extent2D {
                    width: *w,
                    height: *h,
                },
                vk::Format::R8G8B8A8_SRGB,
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                vk::ImageAspectFlags::COLOR,
            )?;
            let index = bindless
                .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(full)?;
            textures.push((image, index));
            texture_sizes.push((*w, *h));
        }
        // The sprites name textures by snapshot id; the shader by bindless
        // index (the block atlas stays a marker the draw resolves).
        for sprite in &mut sprites {
            if let Some((_, index)) = textures.get(sprite.texture as usize) {
                sprite.texture = *index;
            }
        }

        let sprite_bytes: &[u8] = bytemuck::cast_slice(&sprites);
        let mut total = (sprite_bytes.len() as u64).next_multiple_of(16);
        let mut texture_offsets = Vec::new();
        for (_, _, rgba) in &snapshot.textures {
            texture_offsets.push(total);
            total = (total + rgba.len() as u64).next_multiple_of(16);
        }
        let mut staging = memory.buffer(
            ctx,
            "particle staging",
            total.max(16),
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        staging.write(0, sprite_bytes);
        for (k, (_, _, rgba)) in snapshot.textures.iter().enumerate() {
            staging.write(texture_offsets[k] as usize, rgba);
        }
        let buffer = memory.buffer(
            ctx,
            "particle sprites",
            (sprite_bytes.len() as u64).max(16),
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
        )?;
        let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;

        let stats = ParticleStats {
            sprites: sprites.len(),
            translucent: translucent_count as usize,
            textures: textures.len(),
        };
        Ok(ParticleScene {
            module,
            pipes: reused.as_ref().and_then(|r| r.1),
            pending: match reused {
                Some((_, _, p)) => p,
                None => pending,
            },
            sprites: (buffer, index),
            textures,
            texture_sizes,
            staging: Some((staging, texture_offsets, None)),
            cutout,
            translucent: translucent_count,
            stats,
            live: None,
        })
    }

    fn ready(&mut self) -> Result<Option<Pipes>, SetupError> {
        if self.pipes.is_none()
            && let Some(result) = self.pending.as_mut().and_then(|p| p.take())
        {
            let (built, _) = result?;
            self.pipes = Some(Pipes {
                cutout: built[0],
                blend: built[1],
            });
            self.pending = None;
        }
        Ok(self.pipes)
    }

    /// Declares the textures to `graph`, copies everything in on the first
    /// frame, and frees the staging once that frame is done. Also picks up
    /// the pipelines once they are built.
    pub fn prepare(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        graph: &mut Graph,
        frame: u64,
        slot: usize,
    ) -> Result<(), SetupError> {
        self.ready()?;
        if self.live.is_some() {
            if let Some((staging, _, _)) = self.staging.take() {
                memory.free_buffer(ctx, staging);
            }
            return self.prepare_live(ctx, memory, bindless, graph, frame, slot);
        }
        let copied = self.staging.as_ref().map(|(_, _, c)| *c);
        let ready_layout = match copied {
            None | Some(Some(_)) => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            Some(None) => vk::ImageLayout::UNDEFINED,
        };
        for (image, _) in &self.textures {
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, ready_layout);
        }
        if let Some((_, _, Some(at))) = &self.staging
            && frame >= at + FRAMES_IN_FLIGHT as u64
            && let Some((staging, _, _)) = self.staging.take()
        {
            memory.free_buffer(ctx, staging);
        }
        let Some((staging, texture_offsets, copied @ None)) = &mut self.staging else {
            return Ok(());
        };
        *copied = Some(frame);
        let source = staging.raw;
        let device = &ctx.device;
        let target = (self.sprites.0.raw, self.sprites.0.size);
        let images: Vec<(vk::Image, Use)> = self
            .textures
            .iter()
            .map(|(i, _)| (i.raw, Use::TransferDst))
            .collect();
        let textures = &self.textures;
        let sizes = &self.texture_sizes;
        graph.pass(
            "particle upload",
            &images,
            &[(target.0, Use::TransferDst)],
            |cmd| unsafe {
                // SAFETY: copies from the staging into the sprite buffer and
                // the textures, in the transfer states the graph set; the
                // sprite buffer is the sprites' size (at least 16 bytes) and
                // its section of the staging is padded to 16.
                device.cmd_copy_buffer(
                    cmd,
                    source,
                    target.0,
                    &[vk::BufferCopy {
                        src_offset: 0,
                        dst_offset: 0,
                        size: target.1,
                    }],
                );
                for (k, ((image, _), (w, h))) in textures.iter().zip(sizes).enumerate() {
                    device.cmd_copy_buffer_to_image(
                        cmd,
                        source,
                        image.raw,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::BufferImageCopy {
                            buffer_offset: texture_offsets[k],
                            buffer_row_length: 0,
                            buffer_image_height: 0,
                            image_subresource: vk::ImageSubresourceLayers {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                mip_level: 0,
                                base_array_layer: 0,
                                layer_count: 1,
                            },
                            image_offset: vk::Offset3D::default(),
                            image_extent: vk::Extent3D {
                                width: *w,
                                height: *h,
                                depth: 1,
                            },
                        }],
                    );
                }
            },
        );
        Ok(())
    }

    /// What the terrain pass must declare for the particle draws in it.
    pub fn uses(&self, images: &mut Vec<(vk::Image, Use)>, buffers: &mut Vec<(vk::Buffer, Use)>) {
        for (image, _) in &self.textures {
            images.push((image.raw, Use::SampledFragment));
        }
        buffers.push((self.sprites.0.raw, Use::VertexRead));
    }

    #[allow(clippy::too_many_arguments, reason = "one draw's whole context")]
    fn draw(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        pipeline: vk::Pipeline,
        (view_proj, camera): ([[f32; 4]; 4], bevy::math::Mat4),
        (mode, view_index, atlas_index): (u32, u32, u32),
        (first, count): (u32, u32),
    ) {
        if count == 0 {
            return;
        }
        let right: Vec3 = camera.x_axis.truncate().normalize_or_zero();
        let up: Vec3 = camera.y_axis.truncate().normalize_or_zero();
        // SAFETY: inside the terrain pass, with the shared layout and set
        // bound by the caller.
        unsafe {
            ctx.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline)
        };
        push(
            ctx,
            cmd,
            pipelines.layout,
            &Push {
                ids: [self.sprites.1, view_index, atlas_index, 0],
                more: [mode, up.x.to_bits(), up.y.to_bits(), up.z.to_bits()],
                params: [right.x, right.y, right.z, 0.0],
                view_proj,
            },
        );
        // SAFETY: as above; six vertices a sprite, instanced over the
        // sprites from `first`.
        unsafe { ctx.device.cmd_draw(cmd, 6, count, 0, first) };
    }

    /// Draws the opaque particle layer (inside the terrain pass, with the
    /// entities' solid draws).
    #[allow(clippy::too_many_arguments, reason = "one draw's whole context")]
    pub fn draw_cutout(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_proj: [[f32; 4]; 4],
        camera: bevy::math::Mat4,
        mode: u32,
        view_index: u32,
        atlas_index: u32,
    ) {
        let Some(pipes) = self.pipes else { return };
        self.draw(
            ctx,
            cmd,
            pipelines,
            pipes.cutout,
            (view_proj, camera),
            (mode, view_index, atlas_index),
            (0, self.cutout),
        );
    }

    /// Draws the translucent particle layer (after the terrain's
    /// translucent, as Iris and vanilla do).
    #[allow(clippy::too_many_arguments, reason = "one draw's whole context")]
    pub fn draw_blend(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_proj: [[f32; 4]; 4],
        camera: bevy::math::Mat4,
        mode: u32,
        view_index: u32,
        atlas_index: u32,
    ) {
        let Some(pipes) = self.pipes else { return };
        self.draw(
            ctx,
            cmd,
            pipelines,
            pipes.blend,
            (view_proj, camera),
            (mode, view_index, atlas_index),
            (self.cutout, self.translucent),
        );
    }

    /// Gives the scene's bindless slots back (see `EntityScene::release`).
    pub fn release(&self, bindless: &mut Bindless, frame: u64) {
        bindless.remove_buffer(self.sprites.1, frame);
        for (_, i) in &self.textures {
            bindless.remove_texture(*i, frame);
        }
    }

    pub fn destroy(mut self, ctx: &Context, memory: &Memory) {
        let mut pipelines: Vec<vk::Pipeline> = self
            .pipes
            .map(|p| vec![p.cutout, p.blend])
            .unwrap_or_default();
        if let Some(Ok((built, _))) = self.pending.as_mut().and_then(|p| p.wait()) {
            pipelines.extend(built);
        }
        if let Some(live) = self.live.take() {
            for b in live
                .staging
                .into_iter()
                .flatten()
                .chain(live.retired.into_iter().map(|(b, _)| b))
            {
                memory.free_buffer(ctx, b);
            }
        }
        // SAFETY: the caller idles the device first.
        unsafe {
            for p in pipelines {
                ctx.device.destroy_pipeline(p, None);
            }
            ctx.device.destroy_shader_module(self.module, None);
        }
        memory.free_buffer(ctx, self.sprites.0);
        if let Some((staging, _, _)) = self.staging {
            memory.free_buffer(ctx, staging);
        }
        for (image, _) in self.textures {
            memory.free_image(ctx, image);
        }
    }
}

impl ParticleScene {
    /// Takes a live frame (`entity_dump::LiveParticles`): new textures join
    /// the scene's, this frame's sprites replace the last's. The uploads
    /// wait for `prepare`.
    pub fn update(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        frame_data: super::entity_dump::LiveParticles,
    ) -> Result<(), SetupError> {
        let live = self.live.get_or_insert_with(Default::default);
        for (index, w, h, rgba) in frame_data.textures {
            let image = memory.image(
                ctx,
                "particle texture",
                vk::Extent2D {
                    width: w,
                    height: h,
                },
                vk::Format::R8G8B8A8_SRGB,
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                vk::ImageAspectFlags::COLOR,
            )?;
            let bindless_index = bindless
                .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(full)?;
            let at = index as usize;
            if live.textures.len() <= at {
                live.textures.resize(at + 1, None);
            }
            live.textures[at] = Some(bindless_index);
            live.images.push((self.textures.len(), w, h, rgba));
            self.textures.push((image, bindless_index));
            self.texture_sizes.push((w, h));
        }
        // Opaque sprites first, then translucent, each in spawn order.
        let (mut sprites, translucent): (Vec<ParticleSprite>, Vec<ParticleSprite>) = frame_data
            .sprites
            .into_iter()
            .partition(|s| s.flags & PARTICLE_TRANSLUCENT == 0);
        self.cutout = sprites.len() as u32;
        self.translucent = translucent.len() as u32;
        sprites.extend(translucent);
        for sprite in &mut sprites {
            if sprite.texture != super::entity_dump::TEXTURE_ATLAS {
                sprite.texture = live
                    .textures
                    .get(sprite.texture as usize)
                    .copied()
                    .flatten()
                    .unwrap_or(0);
            }
        }
        self.stats.sprites = sprites.len();
        self.stats.translucent = self.translucent as usize;
        self.stats.textures = self.textures.len();
        live.sprites = Some(bytemuck::cast_slice(&sprites).to_vec());
        Ok(())
    }

    fn prepare_live(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        graph: &mut Graph,
        frame: u64,
        slot: usize,
    ) -> Result<(), SetupError> {
        let Some(live) = self.live.as_mut() else {
            return Ok(());
        };
        let (done, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut live.retired)
            .into_iter()
            .partition(|(_, at)| frame >= at + FRAMES_IN_FLIGHT as u64);
        live.retired = keep;
        for (b, _) in done {
            memory.free_buffer(ctx, b);
        }
        if live.staging.is_empty() {
            live.staging = (0..FRAMES_IN_FLIGHT).map(|_| None).collect();
        }
        let images = std::mem::take(&mut live.images);
        for (k, (image, _)) in self.textures.iter().enumerate() {
            let layout = if images.iter().any(|(i, ..)| *i == k) {
                vk::ImageLayout::UNDEFINED
            } else {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            };
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, layout);
        }
        let sprites = live.sprites.take().unwrap_or_default();
        // The sprite buffer, grown to fit (its old contents aren't kept).
        if self.sprites.0.size < sprites.len() as u64 {
            let size = (sprites.len() as u64).next_power_of_two().max(4096);
            let buffer = memory.buffer(
                ctx,
                "particle sprites",
                size,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuOnly,
            )?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            let old = std::mem::replace(&mut self.sprites, (buffer, index));
            bindless.remove_buffer(old.1, frame);
            live.retired.push((old.0, frame));
        }
        let mut total = (sprites.len() as u64).next_multiple_of(16);
        let mut texture_at = Vec::new();
        for (_, _, _, rgba) in &images {
            texture_at.push(total);
            total = (total + rgba.len() as u64).next_multiple_of(16);
        }
        if total == 0 {
            return Ok(());
        }
        let staging = &mut live.staging[slot];
        if staging.as_ref().is_none_or(|b| b.size < total) {
            if let Some(old) = staging.take() {
                live.retired.push((old, frame));
            }
            *staging = Some(memory.buffer(
                ctx,
                "particle staging",
                total.next_power_of_two().max(1 << 16),
                vk::BufferUsageFlags::TRANSFER_SRC,
                MemoryLocation::CpuToGpu,
            )?);
        }
        let Some(staging) = staging.as_mut() else {
            return Ok(());
        };
        staging.write(0, &sprites);
        for ((.., rgba), at) in images.iter().zip(&texture_at) {
            staging.write(*at as usize, rgba);
        }
        let source = staging.raw;
        let target = self.sprites.0.raw;
        let device = &ctx.device;
        let copies: Vec<(vk::Image, u32, u32, u64)> = images
            .iter()
            .zip(&texture_at)
            .filter_map(|((k, w, h, _), at)| Some((self.textures.get(*k)?.0.raw, *w, *h, *at)))
            .collect();
        let image_uses: Vec<(vk::Image, Use)> = copies
            .iter()
            .map(|(i, ..)| (*i, Use::TransferDst))
            .collect();
        let sprite_bytes = sprites.len() as u64;
        graph.pass(
            "particle upload",
            &image_uses,
            &[(target, Use::TransferDst)],
            |cmd| unsafe {
                // SAFETY: copies from this slot's staging, in the transfer
                // states the graph set.
                if sprite_bytes > 0 {
                    device.cmd_copy_buffer(
                        cmd,
                        source,
                        target,
                        &[vk::BufferCopy {
                            src_offset: 0,
                            dst_offset: 0,
                            size: sprite_bytes,
                        }],
                    );
                }
                for (image, w, h, at) in &copies {
                    device.cmd_copy_buffer_to_image(
                        cmd,
                        source,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::BufferImageCopy {
                            buffer_offset: *at,
                            image_subresource: vk::ImageSubresourceLayers {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                mip_level: 0,
                                base_array_layer: 0,
                                layer_count: 1,
                            },
                            image_extent: vk::Extent3D {
                                width: *w,
                                height: *h,
                                depth: 1,
                            },
                            ..Default::default()
                        }],
                    );
                }
            },
        );
        Ok(())
    }
}
