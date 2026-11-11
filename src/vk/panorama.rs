//! The title screen's panorama on the native renderer
//! (VULKAN_REWRITE_CHECKLIST.md 4.6): `renderer::panorama`'s six faces,
//! each with its own image, drawn in one 36-vertex draw from the vertex
//! index (`shaders/panorama.slang`). The camera sits at the cube's centre,
//! turning at vanilla's speed with its downward pitch, through an 85
//! degree lens. `vkview --panorama` shows it.

use ash::vk;
use bevy::math::{Mat4, Quat};
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::frame::FRAMES_IN_FLIGHT;
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsSpec, Pending, Pipelines, Push, Spec, push};
use super::{Context, SetupError};

const FACES: usize = 6;
/// `renderer::panorama`'s lens, pitch and turn.
const FOV_DEGREES: f32 = 85.0;
const PITCH_DEGREES: f32 = -10.0;
const SPIN_DEGREES_PER_SECOND: f32 = 2.0;
const NEAR: f32 = 0.05;
const FAR: f32 = 10.0;

pub struct PanoramaScene {
    module: vk::ShaderModule,
    pipeline: Option<vk::Pipeline>,
    pending: Option<Pending>,
    faces: Vec<(Image, u32)>,
    sizes: Vec<(u32, u32)>,
    staging: Option<(Buffer, Vec<u64>, Option<u64>)>,
}

fn failed(what: &str, e: &dyn std::fmt::Display) -> SetupError {
    SetupError(format!("{what}: {e}"))
}

impl PanoramaScene {
    pub fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        color_format: vk::Format,
    ) -> Result<PanoramaScene, SetupError> {
        let dir = crate::assets_root().join("textures/gui/title/background");
        let mut images = Vec::with_capacity(FACES);
        for face in 0..FACES {
            let path = dir.join(format!("panorama_{face}.png"));
            let image = crate::platform::assets::open_image(&path)
                .map_err(|e| failed(&path.display().to_string(), &e))?
                .to_rgba8();
            images.push((image.width(), image.height(), image.into_raw()));
        }

        let module = pipelines.module(ctx, super::shaders::PANORAMA)?;
        let pending = pipelines.build_on_worker(
            ctx,
            vec![Spec::Graphics(GraphicsSpec {
                module,
                vertex: c"panorama_vertex".to_owned(),
                fragment: c"panorama_fragment".to_owned(),
                color: vec![color_format],
                depth: None,
                depth_test: None,
                depth_write: false,
                // Seen from inside; draw both windings.
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: false,
                additive: false,
            })],
        );

        let mut offsets = Vec::with_capacity(FACES);
        let mut total = 0u64;
        for (_, _, rgba) in &images {
            offsets.push(total);
            total = (total + rgba.len() as u64).next_multiple_of(16);
        }
        let mut staging = memory.buffer(
            ctx,
            "panorama staging",
            total.max(16),
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        let mut faces = Vec::with_capacity(FACES);
        let mut sizes = Vec::with_capacity(FACES);
        for (k, (w, h, rgba)) in images.iter().enumerate() {
            staging.write(offsets[k] as usize, rgba);
            let image = memory.image(
                ctx,
                "panorama face",
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
                .ok_or_else(|| SetupError("the bindless set is full".to_owned()))?;
            faces.push((image, index));
            sizes.push((*w, *h));
        }
        Ok(PanoramaScene {
            module,
            pipeline: None,
            pending: Some(pending),
            faces,
            sizes,
            staging: Some((staging, offsets, None)),
        })
    }

    /// The camera `seconds` in: the turn, then the pitch, then the lens.
    fn view_proj(seconds: f32, rotation: super::swapchain::PreRotation) -> Mat4 {
        let spin = (seconds * SPIN_DEGREES_PER_SECOND).rem_euclid(360.0);
        let turn = Quat::from_rotation_y(-spin.to_radians())
            * Quat::from_rotation_x(PITCH_DEGREES.to_radians());
        rotation.clip
            * Mat4::perspective_rh(FOV_DEGREES.to_radians(), rotation.aspect, NEAR, FAR)
            * Mat4::from_quat(turn.inverse())
    }

    #[allow(clippy::too_many_arguments, reason = "one frame's whole context")]
    pub fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        color: (vk::Image, vk::ImageView),
        extent: vk::Extent2D,
        rotation: super::swapchain::PreRotation,
        frame: u64,
        seconds: f32,
    ) -> Result<(), SetupError> {
        if self.pipeline.is_none()
            && let Some(result) = self.pending.as_mut().and_then(|p| p.take())
        {
            let (built, _) = result?;
            self.pipeline = built.first().copied();
            self.pending = None;
        }
        let copied = self.staging.as_ref().map(|(_, _, c)| *c);
        let layout = match copied {
            None | Some(Some(_)) => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            Some(None) => vk::ImageLayout::UNDEFINED,
        };
        for (image, _) in &self.faces {
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, layout);
        }
        if let Some((_, _, Some(at))) = &self.staging
            && frame >= at + FRAMES_IN_FLIGHT as u64
            && let Some((staging, _, _)) = self.staging.take()
        {
            memory.free_buffer(ctx, staging);
        }
        let device = &ctx.device;
        if let Some((staging, offsets, copied @ None)) = &mut self.staging {
            *copied = Some(frame);
            let source = staging.raw;
            let targets: Vec<(vk::Image, Use)> = self
                .faces
                .iter()
                .map(|(i, _)| (i.raw, Use::TransferDst))
                .collect();
            let (faces, sizes) = (&self.faces, &self.sizes);
            graph.pass("panorama upload", &targets, &[], |cmd| unsafe {
                // SAFETY: copies from the staging into the faces, in the
                // transfer layout the graph set.
                for (k, ((image, _), (w, h))) in faces.iter().zip(sizes).enumerate() {
                    device.cmd_copy_buffer_to_image(
                        cmd,
                        source,
                        image.raw,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::BufferImageCopy {
                            buffer_offset: offsets[k],
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

        let mut images = vec![(color.0, Use::ColorAttachment)];
        images.extend(self.faces.iter().map(|(i, _)| (i.raw, Use::SampledFragment)));
        let index = |k: usize| self.faces[k].1;
        let ids = [index(0), index(1), index(2), index(3)];
        let more = [index(4), index(5), 0, 0];
        let view_proj = Self::view_proj(seconds, rotation).to_cols_array_2d();
        let pipeline = self.pipeline;
        graph.pass("panorama", &images, &[], |cmd| unsafe {
            // SAFETY: the colour attachment is in the layout the graph set;
            // the shared layout and the bindless set are bound here.
            let colors = [vk::RenderingAttachmentInfo::default()
                .image_view(color.1)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.0, 0.0, 0.0, 1.0],
                    },
                })];
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
            if let Some(pipeline) = pipeline {
                super::depth_view::set_viewport(ctx, cmd, extent);
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
                        ids,
                        more,
                        params: [0.0; 4],
                        view_proj,
                    },
                );
                device.cmd_draw(cmd, (FACES * 6) as u32, 1, 0, 0);
            }
            device.cmd_end_rendering(cmd);
        });
        Ok(())
    }

    pub fn destroy(mut self, ctx: &Context, memory: &Memory) {
        let mut pipelines: Vec<vk::Pipeline> = self.pipeline.into_iter().collect();
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
        if let Some((staging, _, _)) = self.staging {
            memory.free_buffer(ctx, staging);
        }
        for (image, _) in self.faces {
            memory.free_image(ctx, image);
        }
    }
}
