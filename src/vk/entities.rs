//! Entities on the native renderer (VULKAN_REWRITE_CHECKLIST.md 4.1 to
//! 4.3): every `EntityMaterial` draw (mobs and players, block entities,
//! items, particles) through one pipeline family (`shaders/entity.slang`).
//!
//! All meshes share one vertex buffer and one index buffer, with a table
//! of each mesh's range (4.2). Rigs are posed on the GPU from the part-row
//! buffer, as `rig_parts` does today. A compute pass culls every instance
//! against the frustum and last frame's depth pyramid (4.3). Survivors
//! become indirect commands: opaque and cutout instances in one list, drawn
//! with one `vkCmdDrawIndexedIndirectCount`; blended and additive ones at
//! their rank, far to near, in one `vkCmdDrawIndexedIndirect` per run of
//! ranks that share a pipeline. The armor glint samples vanilla's
//! `enchanted_glint_armor` texture, loaded from the assets.
//!
//! For now the instances come from the game's entity snapshot
//! (`entity_dump`); feeding them each frame comes with the game.

use ash::vk;
use bevy::math::{Mat4, Vec3};
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::depth_view::DEPTH_FORMAT;
use super::entity_dump::Snapshot;
use super::frame::FRAMES_IN_FLIGHT;
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsSpec, Pending, Pipelines, Push, Spec, push};
use super::upload::Uploads;
use super::{Context, SetupError};

const COMMAND_SIZE: u64 = 20;

/// The entity pipelines, in the order `EntityScene::new` asks for them.
#[derive(Clone, Copy)]
struct Pipes {
    cull: vk::Pipeline,
    solid: vk::Pipeline,
    blend: vk::Pipeline,
    add: vk::Pipeline,
}

/// What the entities drew last frame, for the viewer's summary.
#[derive(Clone, Copy, Debug, Default)]
pub struct EntityStats {
    pub instances: usize,
    pub meshes: usize,
    pub textures: usize,
    pub rigged: usize,
    pub blended: usize,
    /// Last finished frame's solid commands (instances kept by the cull).
    pub solid_drawn: u32,
}

pub struct EntityScene {
    /// The game's camera when it wrote the snapshot (world transform,
    /// vertical fov in radians).
    pub camera: (Mat4, f32),
    module: vk::ShaderModule,
    pipes: Option<Pipes>,
    pending: Option<Pending>,
    instances: (Buffer, u32),
    meshes: (Buffer, u32),
    bounds: (Buffer, u32),
    vertices: (Buffer, u32),
    indices: Buffer,
    parts: (Buffer, u32),
    ranks: (Buffer, u32),
    commands: (Buffer, u32),
    counts: (Buffer, u32),
    readback: Vec<Buffer>,
    textures: Vec<(Image, u32)>,
    glint: Option<(Image, u32)>,
    /// Everything staged at load, the textures' offsets in it, and the
    /// frame it was copied in.
    staging: Option<(Buffer, Vec<u64>, Option<u64>)>,
    /// Offsets in the staging of the vertices, indices, mesh table,
    /// instances, part rows and bounds.
    staged_at: [u64; 6],
    texture_sizes: Vec<(u32, u32)>,
    /// Instances that blend, their centres for sorting, their depth bias,
    /// and whether they add rather than mix.
    blended: Vec<(u32, Vec3, i32, bool)>,
    /// This frame's blended draws: runs of ranks (first, count) and whether
    /// the run adds.
    runs: Vec<(u32, u32, bool)>,
    count: u32,
    rank_of: Vec<u32>,
    pub stats: EntityStats,
}

/// Vanilla's armor glint texture, as the game loads it; none if the assets
/// lack it (the glint is then off, as in the game).
fn load_glint() -> Option<(u32, u32, Vec<u8>)> {
    let file = crate::assets_root().join("textures/misc/enchanted_glint_armor.png");
    let rgba = crate::platform::assets::open_image(&file).ok()?.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

fn full() -> SetupError {
    SetupError("the bindless set is full".to_owned())
}

impl EntityScene {
    pub fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        color_format: vk::Format,
        snapshot: Snapshot,
    ) -> Result<EntityScene, SetupError> {
        let module = pipelines.module(ctx, super::shaders::ENTITY)?;
        let stream = |fragment: &std::ffi::CStr, depth_write, compare, blend, additive| {
            Spec::Graphics(GraphicsSpec {
                module,
                vertex: c"entity_vertex".to_owned(),
                fragment: fragment.to_owned(),
                color: vec![color_format],
                depth: Some(DEPTH_FORMAT),
                depth_test: Some(compare),
                depth_write,
                // Entity faces draw both sides and shade the back by the
                // opposite light, as entity.wgsl.
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend,
                additive,
                fragment_module: None,
            })
        };
        let pending = pipelines.build_on_worker(
            ctx,
            vec![
                Spec::Compute {
                    module,
                    entry: c"cull_entities".to_owned(),
                },
                stream(c"entity_solid", true, vk::CompareOp::GREATER, false, false),
                // Blended layers lie on each other (a banner's patterns):
                // equal depth passes, and nothing blended writes depth.
                stream(
                    c"entity_blend",
                    false,
                    vk::CompareOp::GREATER_OR_EQUAL,
                    true,
                    false,
                ),
                // Additive layers (beams, lightning): the shader premultiplies.
                stream(
                    c"entity_add",
                    false,
                    vk::CompareOp::GREATER_OR_EQUAL,
                    true,
                    true,
                ),
            ],
        );

        // One vertex and one index pool; a range per mesh (4.2).
        let mut vertex_bytes = Vec::new();
        let mut index_bytes = Vec::new();
        let mut table = Vec::new();
        let mut rigged_meshes = Vec::new();
        let (mut base, mut first) = (0u32, 0u32);
        for (verts, indices, rigged) in &snapshot.meshes {
            table.extend_from_slice(&[first, indices.len() as u32, base, u32::from(*rigged)]);
            vertex_bytes.extend_from_slice(bytemuck::cast_slice(verts));
            index_bytes.extend_from_slice(bytemuck::cast_slice(indices));
            base += verts.len() as u32;
            first += indices.len() as u32;
            rigged_meshes.push(*rigged);
        }

        // Each instance's world box from its posed vertices, for the cull.
        let mut bounds: Vec<[f32; 8]> = Vec::with_capacity(snapshot.instances.len());
        let mut blended = Vec::new();
        for (i, instance) in snapshot.instances.iter().enumerate() {
            let world = Mat4::from_cols_array(&instance.transform);
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            if let Some((verts, _, rigged)) = snapshot.meshes.get(instance.mesh as usize) {
                for v in verts {
                    let mut p = Vec3::from(v.pos);
                    if *rigged
                        && let Some(rows) = snapshot.parts.get((instance.tag + v.part) as usize)
                    {
                        let r = |k: usize| {
                            rows[k * 4] * p.x
                                + rows[k * 4 + 1] * p.y
                                + rows[k * 4 + 2] * p.z
                                + rows[k * 4 + 3]
                        };
                        p = Vec3::new(r(0), r(1), r(2));
                    }
                    let w = world.transform_point3(p);
                    lo = lo.min(w);
                    hi = hi.max(w);
                }
            }
            if lo.x > hi.x {
                (lo, hi) = (Vec3::ZERO, Vec3::ZERO);
            }
            bounds.push([lo.x, lo.y, lo.z, 0.0, hi.x, hi.y, hi.z, 0.0]);
            if instance.alpha >= 2 {
                blended.push((
                    i as u32,
                    (lo + hi) / 2.0,
                    instance.depth_bias,
                    instance.alpha == 3,
                ));
            }
        }
        let count = snapshot.instances.len() as u32;

        // Stage everything once; the first frame copies it in.
        let instance_bytes: &[u8] = bytemuck::cast_slice(&snapshot.instances);
        let parts_bytes: &[u8] = bytemuck::cast_slice(&snapshot.parts);
        let sections: [&[u8]; 5] = [
            &vertex_bytes,
            &index_bytes,
            bytemuck::cast_slice(&table),
            instance_bytes,
            parts_bytes,
        ];
        let bounds_bytes: &[u8] = bytemuck::cast_slice(&bounds);
        let mut offsets = [0u64; 5];
        let mut total = 0u64;
        for (k, bytes) in sections.iter().enumerate() {
            offsets[k] = total;
            total = (total + bytes.len() as u64).next_multiple_of(16);
        }
        let bounds_at = total;
        total = (total + bounds_bytes.len() as u64).next_multiple_of(16);
        let mut texture_offsets = Vec::new();
        for (_, _, rgba) in &snapshot.textures {
            texture_offsets.push(total);
            total = (total + rgba.len() as u64).next_multiple_of(16);
        }
        // The armor glint, only when something glints.
        let glint_rgba = snapshot
            .instances
            .iter()
            .any(|i| i.switches[3] > 0.0)
            .then(load_glint)
            .flatten();
        if let Some((_, _, rgba)) = &glint_rgba {
            texture_offsets.push(total);
            total = (total + rgba.len() as u64).next_multiple_of(16);
        }
        let mut staging = memory.buffer(
            ctx,
            "entity staging",
            total.max(16),
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        for (k, bytes) in sections.iter().enumerate() {
            staging.write(offsets[k] as usize, bytes);
        }
        staging.write(bounds_at as usize, bounds_bytes);
        for (k, (_, _, rgba)) in snapshot.textures.iter().enumerate() {
            staging.write(texture_offsets[k] as usize, rgba);
        }
        if let Some((_, _, rgba)) = &glint_rgba {
            staging.write(texture_offsets[snapshot.textures.len()] as usize, rgba);
        }

        let storage = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST;
        let mut make = |name, size: u64, usage| -> Result<(Buffer, u32), SetupError> {
            let buffer = memory.buffer(ctx, name, size.max(16), usage, MemoryLocation::GpuOnly)?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            Ok((buffer, index))
        };
        let vertices = make("entity vertices", vertex_bytes.len() as u64, storage)?;
        let meshes = make("entity meshes", (table.len() * 4) as u64, storage)?;
        let instances = make("entity instances", instance_bytes.len() as u64, storage)?;
        let parts = make("entity parts", parts_bytes.len() as u64, storage)?;
        let bounds_buffer = make("entity bounds", bounds_bytes.len() as u64, storage)?;
        let ranks = make("entity ranks", count as u64 * 4, storage)?;
        let indirect = storage | vk::BufferUsageFlags::INDIRECT_BUFFER;
        let commands = make("entity commands", count as u64 * 2 * COMMAND_SIZE, indirect)?;
        let counts = make(
            "entity counts",
            16,
            indirect | vk::BufferUsageFlags::TRANSFER_SRC,
        )?;
        let indices = memory.buffer(
            ctx,
            "entity indices",
            (index_bytes.len() as u64).max(16),
            vk::BufferUsageFlags::INDEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
        )?;
        let mut readback = Vec::new();
        for _ in 0..FRAMES_IN_FLIGHT {
            readback.push(memory.buffer(
                ctx,
                "entity counts readback",
                16,
                vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuToCpu,
            )?);
        }

        let mut textures = Vec::new();
        let mut texture_sizes = Vec::new();
        for (w, h, _) in &snapshot.textures {
            let image = memory.image(
                ctx,
                "entity texture",
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
        // The glint is data, not colour (as the game's Rgba8Unorm).
        let glint = match glint_rgba {
            Some((w, h, _)) => {
                let image = memory.image(
                    ctx,
                    "entity glint",
                    vk::Extent2D {
                        width: w,
                        height: h,
                    },
                    vk::Format::R8G8B8A8_UNORM,
                    vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                    vk::ImageAspectFlags::COLOR,
                )?;
                let index = bindless
                    .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .ok_or_else(full)?;
                texture_sizes.push((w, h));
                Some((image, index))
            }
            None => None,
        };
        // The instances name textures by snapshot id; the shader by
        // bindless index.
        let mut instances_cpu = snapshot.instances.clone();
        for instance in &mut instances_cpu {
            if let Some((_, index)) = textures.get(instance.texture as usize) {
                instance.texture = *index;
            }
        }
        staging.write(offsets[3] as usize, bytemuck::cast_slice(&instances_cpu));

        let stats = EntityStats {
            instances: count as usize,
            meshes: snapshot.meshes.len(),
            textures: textures.len(),
            rigged: snapshot
                .instances
                .iter()
                .filter(|i| rigged_meshes.get(i.mesh as usize).copied().unwrap_or(false))
                .count(),
            blended: blended.len(),
            solid_drawn: 0,
        };
        Ok(EntityScene {
            camera: (snapshot.camera, snapshot.fov),
            module,
            pipes: None,
            pending: Some(pending),
            instances,
            meshes,
            bounds: bounds_buffer,
            vertices,
            indices,
            parts,
            ranks,
            commands,
            counts,
            readback,
            textures,
            glint,
            staging: Some((staging, texture_offsets, None)),
            staged_at: [
                offsets[0], offsets[1], offsets[2], offsets[3], offsets[4], bounds_at,
            ],
            texture_sizes,
            blended,
            runs: Vec::new(),
            count,
            rank_of: vec![0; count as usize],
            stats,
        })
    }

    /// Whether the pipelines are ready to draw with.
    fn ready(&mut self) -> Result<Option<Pipes>, SetupError> {
        if self.pipes.is_none()
            && let Some(result) = self.pending.as_mut().and_then(|p| p.take())
        {
            let (built, _) = result?;
            self.pipes = Some(Pipes {
                cull: built[0],
                solid: built[1],
                blend: built[2],
                add: built[3],
            });
            self.pending = None;
        }
        Ok(self.pipes)
    }

    /// Declares the textures to `graph` and copies everything in on the
    /// first frame; frees the staging once that frame is done.
    pub fn prepare(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        graph: &mut Graph,
        frame: u64,
        slot: usize,
    ) {
        if let Some(bytes) = self.readback[slot].mapped() {
            self.stats.solid_drawn =
                u32::from_le_bytes(bytes[..4].try_into().unwrap_or([0; 4])).min(self.count);
        }
        let copied = self.staging.as_ref().map(|(_, _, c)| *c);
        let ready_layout = match copied {
            None | Some(Some(_)) => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            Some(None) => vk::ImageLayout::UNDEFINED,
        };
        for (image, _) in self.textures.iter().chain(self.glint.iter()) {
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, ready_layout);
        }
        if let Some((_, _, Some(at))) = &self.staging
            && frame >= at + FRAMES_IN_FLIGHT as u64
            && let Some((staging, _, _)) = self.staging.take()
        {
            memory.free_buffer(ctx, staging);
        }
        let Some((staging, texture_offsets, copied @ None)) = &mut self.staging else {
            return;
        };
        *copied = Some(frame);
        let source = staging.raw;
        let device = &ctx.device;
        // Each buffer is its data's size (at least 16 bytes), and each
        // staged section is padded to 16 bytes, so the copies stay inside.
        let at = self.staged_at;
        let regions = [
            (self.vertices.0.raw, at[0], self.vertices.0.size),
            (self.indices.raw, at[1], self.indices.size),
            (self.meshes.0.raw, at[2], self.meshes.0.size),
            (self.instances.0.raw, at[3], self.instances.0.size),
            (self.parts.0.raw, at[4], self.parts.0.size),
            (self.bounds.0.raw, at[5], self.bounds.0.size),
        ];
        let images: Vec<(vk::Image, Use)> = self
            .textures
            .iter()
            .chain(self.glint.iter())
            .map(|(i, _)| (i.raw, Use::TransferDst))
            .collect();
        let buffers: Vec<(vk::Buffer, Use)> = regions
            .iter()
            .map(|(b, _, _)| (*b, Use::TransferDst))
            .collect();
        let textures = &self.textures;
        let glint = self.glint.iter();
        let sizes = &self.texture_sizes;
        graph.pass("entity upload", &images, &buffers, |cmd| unsafe {
            // SAFETY: copies from the staging into buffers and images in the
            // transfer states the graph set.
            for (target, from, size) in regions {
                device.cmd_copy_buffer(
                    cmd,
                    source,
                    target,
                    &[vk::BufferCopy {
                        src_offset: from,
                        dst_offset: 0,
                        size,
                    }],
                );
            }
            for (k, ((image, _), (w, h))) in textures.iter().chain(glint).zip(sizes).enumerate() {
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
        });
    }

    /// Ranks the blended instances far to near from `eye` (ties by depth
    /// bias, lower first, as Bevy's sort) and culls everything into this
    /// frame's commands.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    pub fn record_cull(
        &mut self,
        ctx: &Context,
        bindless: &Bindless,
        pipelines: &Pipelines,
        uploads: &mut Uploads,
        graph: &mut Graph,
        view_index: u32,
        pyramid: Option<vk::Buffer>,
        eye: Vec3,
        slot: usize,
    ) -> Result<(), SetupError> {
        let Some(pipes) = self.ready()? else {
            return Ok(());
        };
        let mut order: Vec<(f32, i32, u32, bool)> = self
            .blended
            .iter()
            .map(|&(i, centre, bias, add)| (centre.distance_squared(eye), bias, i, add))
            .collect();
        order.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        self.runs.clear();
        for (rank, (_, _, i, add)) in order.iter().enumerate() {
            self.rank_of[*i as usize] = rank as u32;
            match self.runs.last_mut() {
                Some((_, len, run_add)) if *run_add == *add => *len += 1,
                _ => self.runs.push((rank as u32, 1, *add)),
            }
        }
        let device = &ctx.device;
        if let Some(staged) = uploads.stage(bytemuck::cast_slice(&self.rank_of)) {
            let target = self.ranks.0.raw;
            graph.pass(
                "entity ranks",
                &[],
                &[(target, Use::TransferDst)],
                |cmd| unsafe {
                    // SAFETY: a copy from this frame's staging slice.
                    device.cmd_copy_buffer(
                        cmd,
                        staged.buffer,
                        target,
                        &[vk::BufferCopy {
                            src_offset: staged.offset,
                            dst_offset: 0,
                            size: staged.size,
                        }],
                    );
                },
            );
        }
        let counts = self.counts.0.raw;
        graph.pass(
            "entity reset",
            &[],
            &[(counts, Use::TransferDst)],
            |cmd| unsafe {
                // SAFETY: a fill of a buffer this pass owns.
                device.cmd_fill_buffer(cmd, counts, 0, vk::WHOLE_SIZE, 0);
            },
        );
        let mut buffers = vec![
            (self.instances.0.raw, Use::ComputeRead),
            (self.meshes.0.raw, Use::ComputeRead),
            (self.bounds.0.raw, Use::ComputeRead),
            (self.ranks.0.raw, Use::ComputeRead),
            (self.commands.0.raw, Use::ComputeWrite),
            (counts, Use::ComputeWrite),
        ];
        if let Some(pyramid) = pyramid {
            buffers.push((pyramid, Use::ComputeRead));
        }
        let count = self.count;
        graph.pass("entity cull", &[], &buffers, |cmd| unsafe {
            // SAFETY: recording with the shared layout and the bindless set.
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipes.cull);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
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
                    ids: [
                        self.instances.1,
                        self.meshes.1,
                        self.commands.1,
                        self.counts.1,
                    ],
                    more: [count, count, view_index, count],
                    params: [
                        f32::from_bits(self.ranks.1),
                        f32::from_bits(self.bounds.1),
                        0.0,
                        0.0,
                    ],
                    view_proj: [[0.0; 4]; 4],
                },
            );
            device.cmd_dispatch(cmd, count.div_ceil(64).max(1), 1, 1);
        });
        let readback = self.readback[slot].raw;
        graph.pass(
            "entity counts readback",
            &[],
            &[(counts, Use::TransferSrc)],
            |cmd| unsafe {
                // SAFETY: a copy into this slot's readback buffer.
                device.cmd_copy_buffer(
                    cmd,
                    counts,
                    readback,
                    &[vk::BufferCopy {
                        src_offset: 0,
                        dst_offset: 0,
                        size: 4,
                    }],
                );
            },
        );
        Ok(())
    }

    /// What the terrain pass must declare for the entity draws in it.
    pub fn uses(&self, images: &mut Vec<(vk::Image, Use)>, buffers: &mut Vec<(vk::Buffer, Use)>) {
        for (image, _) in self.textures.iter().chain(self.glint.iter()) {
            images.push((image.raw, Use::SampledFragment));
        }
        for b in [
            self.instances.0.raw,
            self.meshes.0.raw,
            self.vertices.0.raw,
            self.parts.0.raw,
        ] {
            buffers.push((b, Use::VertexRead));
        }
        buffers.push((self.indices.raw, Use::IndexRead));
        buffers.push((self.commands.0.raw, Use::IndirectRead));
        buffers.push((self.counts.0.raw, Use::IndirectRead));
    }

    fn bind(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_proj: [[f32; 4]; 4],
        mode: u32,
        seconds: f32,
        view_index: u32,
    ) {
        push(
            ctx,
            cmd,
            pipelines.layout,
            &Push {
                ids: [
                    self.instances.1,
                    self.meshes.1,
                    self.vertices.1,
                    self.parts.1,
                ],
                more: [
                    view_index,
                    mode,
                    self.glint.as_ref().map_or(u32::MAX, |g| g.1),
                    0,
                ],
                params: [seconds, 0.0, 0.0, 0.0],
                view_proj,
            },
        );
        // SAFETY: inside the terrain pass, on its command buffer.
        unsafe {
            ctx.device
                .cmd_bind_index_buffer(cmd, self.indices.raw, 0, vk::IndexType::UINT32)
        };
    }

    /// Draws the opaque and cutout instances (inside the terrain pass,
    /// after the terrain's solid and cutout).
    #[allow(clippy::too_many_arguments, reason = "one draw's whole context")]
    pub fn draw_solid(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_proj: [[f32; 4]; 4],
        mode: u32,
        seconds: f32,
        view_index: u32,
    ) {
        let Some(pipes) = self.pipes else { return };
        // SAFETY: inside the terrain pass, with the shared layout and set
        // bound by the caller.
        unsafe {
            ctx.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipes.solid)
        };
        self.bind(ctx, cmd, pipelines, view_proj, mode, seconds, view_index);
        // SAFETY: as above; the commands and count the cull wrote.
        unsafe {
            ctx.device.cmd_draw_indexed_indirect_count(
                cmd,
                self.commands.0.raw,
                0,
                self.counts.0.raw,
                0,
                self.count,
                COMMAND_SIZE as u32,
            );
        }
    }

    /// Draws the blended and additive instances far to near (after the
    /// terrain's translucent layer), a draw per run of ranks that share a
    /// pipeline.
    pub fn draw_blend(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_proj: [[f32; 4]; 4],
        mode: u32,
        seconds: f32,
        view_index: u32,
    ) {
        let Some(pipes) = self.pipes else { return };
        if self.blended.is_empty() {
            return;
        }
        let mut bound = None;
        for &(first, len, add) in &self.runs {
            let pipeline = if add { pipes.add } else { pipes.blend };
            if bound != Some(pipeline) {
                // SAFETY: as in `draw_solid`.
                unsafe {
                    ctx.device
                        .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline)
                };
                if bound.is_none() {
                    self.bind(ctx, cmd, pipelines, view_proj, mode, seconds, view_index);
                }
                bound = Some(pipeline);
            }
            // SAFETY: as above; one command per instance, at its rank.
            unsafe {
                ctx.device.cmd_draw_indexed_indirect(
                    cmd,
                    self.commands.0.raw,
                    (self.count + first) as u64 * COMMAND_SIZE,
                    len,
                    COMMAND_SIZE as u32,
                );
            }
        }
    }

    pub fn destroy(mut self, ctx: &Context, memory: &Memory) {
        let mut pipelines: Vec<vk::Pipeline> = self
            .pipes
            .map(|p| vec![p.cull, p.solid, p.blend, p.add])
            .unwrap_or_default();
        if let Some(Ok((built, _))) = self.pending.as_mut().and_then(|p| p.wait()) {
            pipelines.extend(built);
        }
        // SAFETY: the caller idles the device first.
        unsafe {
            for p in pipelines {
                ctx.device.destroy_pipeline(p, None);
            }
            ctx.device.destroy_shader_module(self.module, None);
        }
        let mut buffers = vec![
            self.instances.0,
            self.meshes.0,
            self.bounds.0,
            self.vertices.0,
            self.indices,
            self.parts.0,
            self.ranks.0,
            self.commands.0,
            self.counts.0,
        ];
        buffers.extend(self.readback);
        if let Some((staging, _, _)) = self.staging {
            buffers.push(staging);
        }
        for b in buffers {
            memory.free_buffer(ctx, b);
        }
        for (image, _) in self.textures.into_iter().chain(self.glint) {
            memory.free_image(ctx, image);
        }
    }
}
