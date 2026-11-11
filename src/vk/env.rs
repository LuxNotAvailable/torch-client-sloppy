//! What surrounds the world on the native renderer
//! (VULKAN_REWRITE_CHECKLIST.md 5.1): the clear colour, the fog, the
//! lightmap, the sky parts and the cloud layer, from the game's
//! environment snapshot (`entity_dump::read_env`).
//!
//! The sky parts draw first in the terrain pass, pinned to the far plane
//! and ordered by their bias, as vanilla draws the sky before the world.
//! The clouds draw last, blended at their own depth. The lightmap and fog
//! reach the terrain and entity shaders through the frame's view buffer
//! (`terrain::CullView`, `shaders/world.slang`).

use ash::vk;
use bevy::math::{Mat4, Vec3};
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::depth_view::DEPTH_FORMAT;
use super::entity_dump::{EnvPart, Environment};
use super::frame::FRAMES_IN_FLIGHT;
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsSpec, Pending, Pipelines, Push, Spec, push};
use super::{Context, SetupError};

#[derive(Clone, Copy)]
struct Pipes {
    sky_blend: vk::Pipeline,
    sky_add: vk::Pipeline,
    clouds: vk::Pipeline,
    screen: vk::Pipeline,
}

/// One part's draw: its pipeline (0 blended sky, 1 added sky, 2 clouds,
/// 3 a screen overlay), part index, and index range.
#[derive(Clone, Copy)]
struct Draw {
    pipe: u8,
    part: u32,
    first: u32,
    count: u32,
    base: i32,
}

pub struct EnvScene {
    pub clear: [f32; 4],
    pub fog_color: [f32; 4],
    pub fog_ranges: [f32; 4],
    /// The Water Clarity scale on the translucent layer's alpha.
    pub water_alpha: f32,
    /// The portal and nausea warp, after the projection's view.
    pub warp: Mat4,
    /// The server's post effects (`renderer::post_effects` flags).
    pub post: u32,
    module: vk::ShaderModule,
    pipes: Option<Pipes>,
    pending: Option<Pending>,
    parts: (Buffer, u32),
    vertices: (Buffer, u32),
    indices: Buffer,
    textures: Vec<(Image, u32, u32, u32)>,
    pub(crate) lightmap: (Image, u32),
    /// Everything staged at load: the buffer, each texture's offset, the
    /// lightmap's, the parts', the indices', and the frame it was copied in.
    staging: Option<(Buffer, Vec<u64>, [u64; 3], Option<u64>)>,
    /// The lightmap's offset in the staging.
    staged_lightmap: u64,
    /// The sky parts, lowest bias first, then the clouds, then the
    /// screen overlays.
    draws: Vec<Draw>,
    /// Sky pieces for a pack (eye-relative): part, index range.
    sky: Vec<(EnvPart, (u32, u32, u32))>,
    /// A pack's sky instances and mesh table (`pack_sky`).
    pack: Option<PackSky>,
    /// The snapshot's eye.
    eye: Vec3,
    /// Each texture's bindless index and its UNORM view's, for packs.
    raw_indices: Vec<(u32, u32)>,
}

struct PackSky {
    instances: (Buffer, u32),
    meshes: (Buffer, u32),
    /// Per piece: first index, count, base vertex, texture, additive.
    draws: Vec<(u32, u32, i32, u32, bool)>,
    /// Pieces gbuffers_skytextured draws.
    textured: Vec<u32>,
}

fn full() -> SetupError {
    SetupError("the bindless set is full".to_owned())
}

impl EnvScene {
    pub fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        color_format: vk::Format,
        env: Environment,
        eye: Vec3,
        atlas_index: u32,
    ) -> Result<EnvScene, SetupError> {
        let module = pipelines.module(ctx, super::shaders::ENV)?;
        let spec = |fragment: &std::ffi::CStr, depth_test, additive, depth| {
            Spec::Graphics(GraphicsSpec {
                module,
                vertex: c"env_vertex".to_owned(),
                fragment: fragment.to_owned(),
                color: vec![color_format],
                depth,
                depth_test,
                depth_write: false,
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: true,
                additive,
                fragment_module: None,
            })
        };
        let pending = pipelines.build_on_worker(
            ctx,
            vec![
                spec(c"sky_blend", None, false, Some(DEPTH_FORMAT)),
                spec(c"sky_add", None, true, Some(DEPTH_FORMAT)),
                spec(
                    c"clouds",
                    Some(vk::CompareOp::GREATER),
                    false,
                    Some(DEPTH_FORMAT),
                ),
                // The overlays draw in a pass of their own on the output.
                spec(c"screen", None, false, None),
            ],
        );

        // Meshes into one pool, as the entities'.
        let mut vertex_bytes = Vec::new();
        let mut index_bytes = Vec::new();
        let mut ranges = Vec::new();
        let (mut base, mut first) = (0u32, 0u32);
        for (verts, indices, _) in &env.meshes {
            ranges.push((first, indices.len() as u32, base));
            vertex_bytes.extend_from_slice(bytemuck::cast_slice(verts));
            index_bytes.extend_from_slice(bytemuck::cast_slice(indices));
            base += verts.len() as u32;
            first += indices.len() as u32;
        }
        let mut draws: Vec<(f32, Draw)> = env
            .parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let (first, count, base) = *ranges.get(p.mesh as usize)?;
                let pipe = match (p.kind, p.alpha) {
                    (2, _) => 3,
                    (1, _) => 2,
                    (_, 3) => 1,
                    _ => 0,
                };
                // Clouds after every sky part, as their bias already says.
                Some((
                    p.bias + p.kind as f32 * 1e9,
                    Draw {
                        pipe,
                        part: i as u32,
                        first,
                        count,
                        base: base as i32,
                    },
                ))
            })
            .collect();
        draws.sort_by(|a, b| a.0.total_cmp(&b.0));
        let draws = draws.into_iter().map(|(_, d)| d).collect();

        let mut textures = Vec::new();
        let mut raw_indices: Vec<(u32, u32)> = Vec::new();
        for (w, h, _) in &env.textures {
            let image = memory.image_with_raw(
                ctx,
                "sky texture",
                vk::Extent2D {
                    width: *w,
                    height: *h,
                },
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            )?;
            let index = bindless
                .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(full)?;
            // Packs sample the stored (sRGB-encoded) texels.
            let raw = bindless
                .add_texture(ctx, image.raw_view.expect("made with a raw view"), vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(full)?;
            raw_indices.push((index, raw));
            textures.push((image, index, *w, *h));
        }
        let lightmap_image = memory.image(
            ctx,
            "lightmap",
            vk::Extent2D {
                width: 16,
                height: 16,
            },
            vk::Format::R8G8B8A8_SRGB,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let lightmap_index = bindless
            .add_texture(
                ctx,
                lightmap_image.view,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            )
            .ok_or_else(full)?;

        // The parts name textures by snapshot id; the shader by bindless
        // index.
        // Sky parts follow the camera: their place kept relative to the eye
        // the snapshot was taken from (the sun and moon sit far off it).
        let mut parts: Vec<EnvPart> = env.parts.clone();
        for part in &mut parts {
            if part.kind == 0 {
                for (t, e) in part.transform[12..15].iter_mut().zip(eye.to_array()) {
                    *t -= e;
                }
            }
            if part.texture == super::entity_dump::TEXTURE_ATLAS {
                part.texture = atlas_index;
            } else if let Some((_, index, _, _)) = textures.get(part.texture as usize) {
                part.texture = *index;
            }
        }
        let sky = parts
            .iter()
            .filter(|p| p.kind == 0 && p.pad[0] > 0.0)
            .filter_map(|p| Some((*p, *ranges.get(p.mesh as usize)?)))
            .collect();
        let parts_bytes: &[u8] = bytemuck::cast_slice(&parts);

        let mut total = 0u64;
        let mut place = |len: usize| {
            let at = total;
            total = (total + len as u64).next_multiple_of(16);
            at
        };
        let vertices_at = place(vertex_bytes.len());
        let indices_at = place(index_bytes.len());
        let parts_at = place(parts_bytes.len());
        let lightmap_at = place(env.lightmap.len());
        let texture_at: Vec<u64> = env
            .textures
            .iter()
            .map(|(_, _, t)| place(t.len()))
            .collect();
        let mut staging = memory.buffer(
            ctx,
            "env staging",
            total.max(16),
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        staging.write(vertices_at as usize, &vertex_bytes);
        staging.write(indices_at as usize, &index_bytes);
        staging.write(parts_at as usize, parts_bytes);
        staging.write(lightmap_at as usize, &env.lightmap);
        for ((_, _, rgba), at) in env.textures.iter().zip(&texture_at) {
            staging.write(*at as usize, rgba);
        }

        let storage = vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST;
        let mut make = |name, size: u64| -> Result<(Buffer, u32), SetupError> {
            let buffer =
                memory.buffer(ctx, name, size.max(16), storage, MemoryLocation::GpuOnly)?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            Ok((buffer, index))
        };
        let vertices = make("env vertices", vertex_bytes.len() as u64)?;
        let parts_buffer = make("env parts", parts_bytes.len() as u64)?;
        let indices = memory.buffer(
            ctx,
            "env indices",
            (index_bytes.len() as u64).max(16),
            vk::BufferUsageFlags::INDEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
        )?;
        Ok(EnvScene {
            clear: env.clear,
            fog_color: env.fog_color,
            fog_ranges: env.fog_ranges,
            water_alpha: env.water_alpha,
            warp: Mat4::from_cols_array(&env.warp),
            post: env.post,
            module,
            pipes: None,
            pending: Some(pending),
            parts: parts_buffer,
            vertices,
            indices,
            textures,
            lightmap: (lightmap_image, lightmap_index),
            staging: Some((
                staging,
                texture_at,
                [vertices_at, indices_at, parts_at],
                None,
            )),
            staged_lightmap: lightmap_at,
            draws,
            sky,
            pack: None,
            eye,
            raw_indices,
        })
    }

    /// Builds a pack's sky: one entity instance per piece, in the sky
    /// root's unit frame, sun, moon and stars tilted by `sunPathRotation`.
    pub fn pack_sky(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        sun_path_rotation: f32,
    ) -> Result<(), SetupError> {
        use super::entity_dump::{EntityInstance, SKY_PIECES, SKY_SCALE};
        let tilt = Mat4::from_rotation_y((-90.0f32).to_radians())
            * Mat4::from_rotation_z(sun_path_rotation.to_radians())
            * Mat4::from_rotation_y(90.0f32.to_radians());
        let mut instances = Vec::new();
        let mut meshes: Vec<[u32; 4]> = Vec::new();
        let mut draws = Vec::new();
        let mut textured = Vec::new();
        for (part, (first, count, base)) in &self.sky {
            let piece = SKY_PIECES[(part.pad[0] as usize - 1).min(SKY_PIECES.len() - 1)];
            let stage = crate::shaderpack::features::render_stage(if piece == "END_SKY" { "SKY" } else { piece });
            let local = Mat4::from_scale(Vec3::splat(SKY_SCALE.recip())) * Mat4::from_cols_array(&part.transform);
            let model = if matches!(piece, "SUN" | "MOON" | "STARS") { tilt * local } else { local };
            // World again: the entity input takes the eye off.
            let model = Mat4::from_translation(self.eye) * model;
            if matches!(piece, "SUN" | "MOON" | "END_SKY") {
                textured.push(draws.len() as u32);
            }
            meshes.push([*first, *count, *base, 0]);
            instances.push(EntityInstance {
                mesh: meshes.len() as u32 - 1,
                texture: part.texture,
                alpha: part.alpha,
                // The pack's renderStage, past the native kinds.
                kind: 16 + stage as u32,
                tint: part.tint,
                light0: [0.0, 0.0, 0.0, 1.0],
                light1: [0.0, 0.0, 0.0, 1.0],
                levels: [240.0, 240.0],
                transform: model.to_cols_array(),
                ..Default::default()
            });
            let texture = self.raw_indices.iter().find(|(s, _)| *s == part.texture).map_or(part.texture, |(_, r)| *r);
            draws.push((*first, *count, *base as i32, texture, part.alpha == 3));
        }
        let mut upload = |name: &str, bytes: &[u8]| -> Result<(Buffer, u32), SetupError> {
            let mut buffer = memory.buffer(
                ctx,
                name,
                bytes.len().max(16) as u64,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            buffer.write(0, bytes);
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            Ok((buffer, index))
        };
        self.pack = Some(PackSky {
            instances: upload("pack sky instances", bytemuck::cast_slice(&instances))?,
            meshes: upload("pack sky meshes", bytemuck::cast_slice(&meshes))?,
            draws,
            textured,
        });
        Ok(())
    }

    /// The pack's sky as entity draws (`pack_sky` first).
    pub fn pack_entities(&self) -> Option<super::entities::PackEntities> {
        let pack = self.pack.as_ref()?;
        Some(super::entities::PackEntities {
            instances: (pack.instances.0.raw, pack.instances.1),
            meshes: (pack.meshes.0.raw, pack.meshes.1),
            vertices: (self.vertices.0.raw, self.vertices.1),
            // Never read: no piece is posed.
            parts: (pack.instances.0.raw, pack.instances.1),
            indices: self.indices.raw,
            draws: pack.draws.clone(),
            // For the sky: the textured pieces.
            blended: pack.textured.clone(),
            textures: self.textures.iter().map(|t| t.0.raw).collect(),
        })
    }

    /// The lightmap's bindless index.
    pub fn lightmap_index(&self) -> u32 {
        self.lightmap.1
    }

    /// Declares the images to `graph`, copies everything in on the first
    /// frame, and frees the staging once that frame is done.
    pub fn prepare(&mut self, ctx: &Context, memory: &Memory, graph: &mut Graph, frame: u64) {
        if self.pipes.is_none()
            && let Some(Ok((built, _))) = self.pending.as_mut().and_then(|p| p.take())
        {
            self.pipes = Some(Pipes {
                sky_blend: built[0],
                sky_add: built[1],
                clouds: built[2],
                screen: built[3],
            });
            self.pending = None;
        }
        let uploaded = self.staging.as_ref().is_none_or(|(.., c)| c.is_some());
        let layout = if uploaded {
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
        } else {
            vk::ImageLayout::UNDEFINED
        };
        for image in self
            .textures
            .iter()
            .map(|t| &t.0)
            .chain(std::iter::once(&self.lightmap.0))
        {
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, layout);
        }
        if let Some((.., Some(at))) = &self.staging
            && frame >= at + FRAMES_IN_FLIGHT as u64
            && let Some((staging, ..)) = self.staging.take()
        {
            memory.free_buffer(ctx, staging);
        }
        let Some((staging, texture_at, [vertices_at, indices_at, parts_at], copied @ None)) =
            &mut self.staging
        else {
            return;
        };
        *copied = Some(frame);
        let source = staging.raw;
        let device = &ctx.device;
        let buffers = [
            (self.vertices.0.raw, *vertices_at, self.vertices.0.size),
            (self.indices.raw, *indices_at, self.indices.size),
            (self.parts.0.raw, *parts_at, self.parts.0.size),
        ];
        let mut images: Vec<(vk::Image, u64, u32, u32)> = self
            .textures
            .iter()
            .zip(texture_at.iter())
            .map(|((image, _, w, h), at)| (image.raw, *at, *w, *h))
            .collect();
        images.push((self.lightmap.0.raw, self.staged_lightmap, 16, 16));
        let image_uses: Vec<(vk::Image, Use)> =
            images.iter().map(|i| (i.0, Use::TransferDst)).collect();
        let buffer_uses: Vec<(vk::Buffer, Use)> =
            buffers.iter().map(|b| (b.0, Use::TransferDst)).collect();
        graph.pass("env upload", &image_uses, &buffer_uses, |cmd| unsafe {
            // SAFETY: copies from the staging into buffers and images in the
            // transfer states the graph set.
            for (target, from, size) in buffers {
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
            for (image, from, w, h) in &images {
                device.cmd_copy_buffer_to_image(
                    cmd,
                    source,
                    *image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy {
                        buffer_offset: *from,
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

    /// What the terrain pass must declare for the draws in it.
    pub fn uses(&self, images: &mut Vec<(vk::Image, Use)>, buffers: &mut Vec<(vk::Buffer, Use)>) {
        for (image, ..) in &self.textures {
            images.push((image.raw, Use::SampledFragment));
        }
        images.push((self.lightmap.0.raw, Use::SampledFragment));
        buffers.push((self.vertices.0.raw, Use::VertexRead));
        buffers.push((self.parts.0.raw, Use::VertexRead));
        buffers.push((self.indices.raw, Use::IndexRead));
    }

    fn draw(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_index: u32,
        view_proj: [[f32; 4]; 4],
        stage: u8,
    ) {
        let Some(pipes) = self.pipes else { return };
        let device = &ctx.device;
        push(
            ctx,
            cmd,
            pipelines.layout,
            &Push {
                ids: [self.parts.1, 0, self.vertices.1, 0],
                more: [view_index, 0, 0, 0],
                params: [0.0; 4],
                view_proj,
            },
        );
        // SAFETY: inside the terrain pass, with the shared layout and set
        // bound by the caller; each draw names a part's own range.
        unsafe {
            device.cmd_bind_index_buffer(cmd, self.indices.raw, 0, vk::IndexType::UINT32);
            for d in self.draws.iter().filter(|d| d.pipe.max(1) - 1 == stage) {
                let pipeline = match d.pipe {
                    0 => pipes.sky_blend,
                    1 => pipes.sky_add,
                    2 => pipes.clouds,
                    _ => pipes.screen,
                };
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_draw_indexed(cmd, d.count, 1, d.first, d.base, d.part);
            }
        }
    }

    /// The sky parts, first in the terrain pass.
    pub fn draw_sky(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_index: u32,
        view_proj: [[f32; 4]; 4],
    ) {
        self.draw(ctx, cmd, pipelines, view_index, view_proj, 0);
    }

    /// The clouds, last in the terrain pass.
    pub fn draw_clouds(
        &self,
        ctx: &Context,
        cmd: vk::CommandBuffer,
        pipelines: &Pipelines,
        view_index: u32,
        view_proj: [[f32; 4]; 4],
    ) {
        self.draw(ctx, cmd, pipelines, view_index, view_proj, 1);
    }

    /// The screen overlays, in a pass of their own over the finished
    /// `output`; `clip` the display's pre-rotation.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    pub fn record_screen(
        &self,
        ctx: &Context,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        output: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        clip: Mat4,
        atlas: vk::Image,
        view_index: u32,
    ) {
        if self.pipes.is_none() || !self.draws.iter().any(|d| d.pipe == 3) {
            return;
        }
        let mut images = vec![
            (output.0, Use::ColorAttachment),
            (atlas, Use::SampledFragment),
        ];
        let mut buffers = Vec::new();
        self.uses(&mut images, &mut buffers);
        let device = &ctx.device;
        graph.pass("screen overlays", &images, &buffers, |cmd| unsafe {
            // SAFETY: the graph moved the output to an attachment and the
            // textures to sampled layouts.
            let colors = [vk::RenderingAttachmentInfo::default()
                .image_view(output.1)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE)];
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
            super::depth_view::set_viewport(ctx, cmd, extent);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                pipelines.layout,
                0,
                &[bindless.set],
                &[],
            );
            self.draw(ctx, cmd, pipelines, view_index, clip.to_cols_array_2d(), 2);
            device.cmd_end_rendering(cmd);
        });
    }

    pub fn destroy(mut self, ctx: &Context, memory: &Memory) {
        let mut pipelines: Vec<vk::Pipeline> = self
            .pipes
            .map(|p| vec![p.sky_blend, p.sky_add, p.clouds, p.screen])
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
        for b in [self.parts.0, self.vertices.0, self.indices] {
            memory.free_buffer(ctx, b);
        }
        if let Some(p) = self.pack {
            memory.free_buffer(ctx, p.instances.0);
            memory.free_buffer(ctx, p.meshes.0);
        }
        if let Some((staging, ..)) = self.staging {
            memory.free_buffer(ctx, staging);
        }
        for (image, ..) in self.textures {
            memory.free_image(ctx, image);
        }
        memory.free_image(ctx, self.lightmap.0);
    }
}
