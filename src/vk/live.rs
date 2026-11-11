//! The native renderer in the game (VULKAN_REWRITE_CHECKLIST.md phase 8),
//! on with `MC_RENDERER=native`: Bevy's renderer starts with no backend,
//! and this plugin takes the primary window, draws each frame and presents.
//!
//! Drawn: the terrain (`TerrainScene`, fed the game's section stream by
//! `terrain_dump::take_live`, seen from the world camera), then the GUI
//! (`gui::pool`'s painter output). Entities, sky and packs follow.

use std::mem::ManuallyDrop;
use std::sync::OnceLock;

use ash::vk;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RawHandleWrapper};
use gpu_allocator::MemoryLocation;

use super::bindless::Bindless;
use super::frame::{FRAMES_IN_FLIGHT, FrameError, Frames};
use super::graph::{Graph, Use};
use super::memory::{Buffer, Image, Memory};
use super::pipeline::{GraphicsDesc, Pipelines, Push, push};
use super::terrain::{Camera, TerrainScene};
use super::upload::Uploads;
use super::{Context, SetupError, Swapchain};

/// Whether this run draws with the native renderer (`MC_RENDERER=native`).
pub fn wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| std::env::var("MC_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("native")))
}

pub struct LivePlugin;

impl Plugin for LivePlugin {
    fn build(&self, app: &mut App) {
        if !wanted() {
            return;
        }
        app.add_systems(Last, draw_frame);
    }
}

/// Everything the renderer owns; main-thread only (the window's surface).
/// Device children are `ManuallyDrop`, freed in order in `Drop` before
/// `ctx` goes.
struct Live {
    ctx: Context,
    swapchain: Option<Swapchain>,
    frames: ManuallyDrop<Frames>,
    memory: ManuallyDrop<Memory>,
    bindless: ManuallyDrop<Bindless>,
    pipelines: ManuallyDrop<Pipelines>,
    gui: ManuallyDrop<GuiPass>,
    uploads: ManuallyDrop<Uploads>,
    /// Made once the game has its block atlas.
    terrain: Option<TerrainScene>,
    /// Scenes replaced by newer snapshots, freed once frames in flight are
    /// past them: the frame they were replaced on.
    retired: Vec<(Retired, u64)>,
    /// The world camera's eye last frame.
    eye: bevy::math::Vec3,
    /// The inventory's player figure, drawn offscreen for the GUI.
    figure: Figure,
    frame: u64,
    size: (u32, u32),
    stale: bool,
}

fn setup(world: &mut World) -> Result<Live, SetupError> {
    let mut windows = world.query_filtered::<(&RawHandleWrapper, &Window), With<PrimaryWindow>>();
    let (handles, window) = windows
        .iter(world)
        .next()
        .ok_or_else(|| SetupError("no window yet".into()))?;
    let size = (window.physical_width(), window.physical_height());
    let ctx = Context::new(Some((handles.get_display_handle(), handles.get_window_handle())), None)?;
    info!(
        "native renderer: {} ({:?}), Vulkan {}.{}.{}",
        ctx.info.name, ctx.info.kind, ctx.info.api.0, ctx.info.api.1, ctx.info.api.2
    );
    let swapchain = Swapchain::new(&ctx, size, true, None)?;
    let frames = Frames::new(&ctx)?;
    let memory = Memory::new(&ctx)?;
    let mut bindless = Bindless::new(&ctx)?;
    let cache = std::env::temp_dir().join("torch-native-pipelines.bin");
    let pipelines = Pipelines::new(&ctx, bindless.layout, Some(cache))?;
    let format = swapchain
        .as_ref()
        .map_or(vk::Format::B8G8R8A8_SRGB, |s| s.format.format);
    let gui = GuiPass::new(&ctx, &memory, &mut bindless, &pipelines, format)?;
    let uploads = Uploads::new(&ctx, &memory, 16 << 20)?;
    Ok(Live {
        ctx,
        swapchain,
        frames: ManuallyDrop::new(frames),
        memory: ManuallyDrop::new(memory),
        bindless: ManuallyDrop::new(bindless),
        pipelines: ManuallyDrop::new(pipelines),
        gui: ManuallyDrop::new(gui),
        uploads: ManuallyDrop::new(uploads),
        terrain: None,
        retired: Vec::new(),
        eye: bevy::math::Vec3::ZERO,
        figure: Figure::default(),
        frame: 0,
        size,
        stale: false,
    })
}

fn draw_frame(world: &mut World) {
    if world.get_non_send_resource::<Live>().is_none() {
        match setup(world) {
            Ok(live) => world.insert_non_send_resource(live),
            Err(e) => {
                // No window yet is normal on the first frames; the rest is fatal.
                if e.0 != "no window yet" {
                    error!("native renderer: {}", e.0);
                    std::process::exit(3);
                }
                return;
            }
        }
    }
    let size = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map(|w| (w.physical_width(), w.physical_height()));
    let gui_frame = world
        .get_resource::<crate::gui::pool::GuiFrames>()
        .map(|f| std::mem::take(&mut *f.0.lock()));
    let wanted = world
        .get_non_send_resource::<Live>()
        .map_or((true, true), |l| (l.gui.textures.is_none(), l.gui.glint.is_none()));
    let textures = match wanted {
        (false, false) => (None, None),
        _ => gui_textures(world),
    };
    // Bevy's render world would take these; nothing does now.
    if let Some(ops) = world.get_resource::<crate::renderer::terrain_pool::TerrainOps>() {
        ops.0.lock().clear();
    }
    let camera = world_camera(world);
    let figure_view = figure_view(world);
    let Some(mut live) = world.remove_non_send_resource::<Live>() else {
        return;
    };
    if let Some(size) = size
        && size != live.size
    {
        live.size = size;
        live.stale = true;
    }
    if let Some(c) = camera {
        live.eye = c.eye;
    }
    live.feed_terrain();
    live.figure.view = figure_view;
    live.draw(gui_frame.unwrap_or_default(), textures, camera);
    world.insert_non_send_resource(live);
}

/// The figure camera's view-projection, eye and the texture size the GUI
/// wants, while the inventory shows the figure.
fn figure_view(world: &mut World) -> Option<(bevy::math::Mat4, bevy::math::Vec3, vk::Extent2D)> {
    use crate::renderer::inventory_figure::{FigureCamera, FigureImage, ready};
    if !ready() {
        return None;
    }
    let handle = world.get_resource::<FigureImage>()?.0.clone()?;
    let size = world.get_resource::<Assets<bevy::image::Image>>()?.get(&handle)?.texture_descriptor.size;
    let mut q = world.query_filtered::<(&GlobalTransform, &Projection), With<FigureCamera>>();
    let (transform, projection) = q.iter(world).next()?;
    let Projection::Orthographic(o) = projection else {
        return None;
    };
    let (w, h) = match o.scaling_mode {
        bevy::camera::ScalingMode::Fixed { width, height } => (width, height),
        _ => return None,
    };
    // Bevy's orthographic projection, depth reversed as the world's.
    let proj = bevy::math::Mat4::orthographic_rh(-w / 2.0, w / 2.0, -h / 2.0, h / 2.0, o.far, o.near);
    let view = transform.to_matrix().inverse();
    Some((proj * view, transform.translation(), vk::Extent2D { width: size.width, height: size.height }))
}

/// The world camera as `terrain::Camera` (as `TerrainScene::game_camera`).
fn world_camera(world: &mut World) -> Option<Camera> {
    let mut q = world.query_filtered::<(&GlobalTransform, &Projection), With<crate::renderer::systems::WorldCamera>>();
    let (transform, projection) = q.iter(world).next()?;
    let Projection::Perspective(p) = projection else {
        return None;
    };
    let eye = transform.translation();
    // The camera looks down its -z.
    let forward = transform.forward().as_vec3();
    Some(Camera {
        eye,
        yaw: (-forward.x).atan2(forward.z).to_degrees(),
        pitch: forward.y.clamp(-1.0, 1.0).asin().to_degrees(),
        fov: p.fov.to_degrees(),
    })
}

type TextureData = (u32, u32, vk::Format, Vec<u8>);

/// The GUI's textures as the main world holds them: atlas and unihex font,
/// then the item glint (the game assets' `GLINT_ITEM`) once it has loaded.
fn gui_textures(world: &World) -> (Option<[TextureData; 2]>, Option<TextureData>) {
    let images = world.get_resource::<Assets<bevy::image::Image>>();
    let glint = images.and_then(|i| image_data(i, &crate::renderer::entity_material::GLINT_ITEM));
    (gui_atlas(world), glint)
}

/// An image's texels, in a format the GUI samples.
fn image_data(images: &Assets<bevy::image::Image>, handle: &Handle<bevy::image::Image>) -> Option<TextureData> {
    use bevy::render::render_resource::TextureFormat as F;
    let image = images.get(handle)?;
    let format = match image.texture_descriptor.format {
        F::Rgba8UnormSrgb => vk::Format::R8G8B8A8_SRGB,
        F::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        F::R8Unorm => vk::Format::R8_UNORM,
        _ => return None,
    };
    let size = image.texture_descriptor.size;
    Some((size.width, size.height, format, image.data.clone()?))
}

fn gui_atlas(world: &World) -> Option<[TextureData; 2]> {
    let handles = world.get_resource::<crate::gui::pool::GuiTextures>()?;
    let images = world.get_resource::<Assets<bevy::image::Image>>()?;
    Some([image_data(images, &handles.atlas)?, image_data(images, &handles.unihex)?])
}

impl Drop for Live {
    fn drop(&mut self) {
        self.ctx.idle();
        // SAFETY: each field is taken once, here, and not used after.
        unsafe {
            let memory = ManuallyDrop::take(&mut self.memory);
            if let Some(t) = self.terrain.take() {
                t.destroy(&self.ctx, &memory);
            }
            for (scene, _) in self.retired.drain(..) {
                scene.destroy(&self.ctx, &memory);
            }
            std::mem::take(&mut self.figure).destroy(&self.ctx, &memory);
            ManuallyDrop::take(&mut self.uploads).destroy(&self.ctx, &memory);
            ManuallyDrop::take(&mut self.gui).destroy(&self.ctx, &memory);
            ManuallyDrop::take(&mut self.pipelines).destroy(&self.ctx);
            ManuallyDrop::take(&mut self.bindless).destroy(&self.ctx);
            if let Some(s) = self.swapchain.take() {
                s.destroy(&self.ctx);
            }
            ManuallyDrop::take(&mut self.frames).destroy(&self.ctx);
            // The allocator's memory goes before the device.
            drop(memory);
        }
    }
}

impl Live {
    /// Makes the terrain once the atlas exists, then queues the game's
    /// section changes into it.
    fn feed_terrain(&mut self) {
        if self.terrain.is_none() {
            let Some((w, h, rgba)) = super::terrain_dump::live_atlas() else {
                return;
            };
            let Some(atlas) = image::RgbaImage::from_raw(w, h, rgba) else {
                return;
            };
            let format = self
                .swapchain
                .as_ref()
                .map_or(vk::Format::B8G8R8A8_SRGB, |s| s.format.format);
            match TerrainScene::new(
                &self.ctx,
                &self.memory,
                &mut self.bindless,
                &self.pipelines,
                format,
                super::terrain::Source::Live { atlas },
                terrain_options(),
            ) {
                Ok(t) => self.terrain = Some(t),
                Err(e) => {
                    error!("native renderer: terrain: {}", e.0);
                    return;
                }
            }
        }
        // This frame's snapshots of what's in and around the world.
        self.renew_scenes();
        let records = super::terrain_dump::take_live();
        if let Some(t) = &mut self.terrain
            && !records.is_empty()
        {
            t.push(records);
        }
    }

    /// Renews the environment, entities and particles from the snapshots
    /// the game delivered this frame; old scenes retire.
    fn renew_scenes(&mut self) {
        use super::entity_dump as dump;
        let Some(t) = &mut self.terrain else { return };
        let format = self
            .swapchain
            .as_ref()
            .map_or(vk::Format::B8G8R8A8_SRGB, |s| s.format.format);
        let (ctx, memory, pipelines) = (&self.ctx, &*self.memory, &*self.pipelines);
        let bindless = &mut *self.bindless;
        let mut old: Vec<Retired> = Vec::new();
        let fail = |what: &str, e: String| error!("native renderer: {what}: {e}");
        if let Some(bytes) = dump::take_live("env.bin") {
            match dump::read_env(&bytes) {
                Ok(env) => match t.renew_env(ctx, memory, bindless, pipelines, format, env, self.eye) {
                    Ok(o) => old.extend(o.map(Retired::Env)),
                    Err(e) => fail("env", e.0),
                },
                Err(e) => fail("env", e),
            }
        }
        if let Some(bytes) = dump::take_live("entities.bin") {
            match dump::read(&bytes) {
                Ok(snapshot) => match t.renew_entities(ctx, memory, bindless, pipelines, format, snapshot) {
                    Ok(o) => old.extend(o.map(Retired::Entities)),
                    Err(e) => fail("entities", e.0),
                },
                Err(e) => fail("entities", e),
            }
        }
        if let Some(bytes) = dump::take_live("particles.bin") {
            match dump::read_particles(&bytes) {
                Ok(snapshot) => match t.renew_particles(ctx, memory, bindless, pipelines, format, snapshot) {
                    Ok(o) => old.extend(o.map(Retired::Particles)),
                    Err(e) => fail("particles", e.0),
                },
                Err(e) => fail("particles", e),
            }
        }
        if let Some(bytes) = dump::take_live("figure.bin") {
            match dump::read(&bytes) {
                Ok(snapshot) => {
                    let mut prev = self.figure.scene.take();
                    match super::entities::EntityScene::new(ctx, memory, bindless, pipelines, format, snapshot, prev.as_mut()) {
                        Ok(scene) => {
                            self.figure.scene = Some(scene);
                            old.extend(prev.map(Retired::Entities));
                        }
                        Err(e) => {
                            self.figure.scene = prev;
                            fail("figure", e.0);
                        }
                    }
                }
                Err(e) => fail("figure", e),
            }
        }
        for scene in old {
            scene.release(bindless, self.frame);
            self.retired.push((scene, self.frame));
        }
    }

    fn recreate(&mut self) {
        self.ctx.idle();
        match Swapchain::new(&self.ctx, self.size, true, self.swapchain.as_ref()) {
            Ok(Some(new)) => {
                if let Some(old) = self.swapchain.replace(new) {
                    old.destroy(&self.ctx);
                }
                self.stale = false;
            }
            // No area (minimised): try again next frame.
            Ok(None) => {}
            Err(e) => error!("native renderer: swapchain: {}", e.0),
        }
    }

    fn draw(
        &mut self,
        gui: crate::gui::pool::GuiFrame,
        textures: (Option<[TextureData; 2]>, Option<TextureData>),
        camera: Option<Camera>,
    ) {
        if self.stale {
            self.recreate();
            if self.stale {
                return;
            }
        }
        let Some(swapchain) = &self.swapchain else {
            return;
        };
        let recording = match self.frames.begin(&self.ctx, swapchain) {
            Ok(r) => r,
            Err(FrameError::OutOfDate) => {
                self.stale = true;
                return;
            }
            Err(FrameError::Failed(e)) => {
                error!("native renderer: frame failed: {e:?}");
                if e == vk::Result::ERROR_DEVICE_LOST {
                    std::process::exit(3);
                }
                return;
            }
        };
        let slot = self.frames.index;
        self.bindless.recycle(self.frame);
        // Scenes no frame in flight still reads (this slot's fence passed).
        let now = self.frame;
        let (done, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.retired)
            .into_iter()
            .partition(|(_, at)| now >= at + FRAMES_IN_FLIGHT as u64);
        self.retired = keep;
        for (scene, _) in done {
            scene.destroy(&self.ctx, &self.memory);
        }
        self.uploads.begin(slot);
        let rotation = swapchain.pre_rotation();
        let image = swapchain.images[recording.image_index as usize];
        let view = swapchain.views[recording.image_index as usize];
        let extent = swapchain.extent;
        let mut graph = Graph::new(&self.ctx, recording.commands, None);
        graph.image(image, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
        // The world, when there's one to see.
        let mut drew_world = false;
        if let (Some(terrain), Some(camera)) = (&mut self.terrain, camera) {
            match terrain.record(
                &self.ctx,
                &self.memory,
                &mut self.bindless,
                &self.pipelines,
                &mut self.uploads,
                &mut graph,
                (image, view),
                extent,
                rotation,
                self.frame,
                slot,
                camera,
                super::test_scene::View::Shaded,
            ) {
                Ok(()) => drew_world = true,
                Err(e) => error!("native renderer: terrain: {}", e.0),
            }
        }
        let figure = match self.figure.record(
            &self.ctx,
            &self.memory,
            &mut self.bindless,
            &self.pipelines,
            &mut self.uploads,
            &mut graph,
            self.frame,
            slot,
        ) {
            Ok(f) => f,
            Err(e) => {
                error!("native renderer: figure: {}", e.0);
                None
            }
        };
        if let Err(e) = self.gui.record(
            &self.ctx,
            &self.memory,
            &mut self.bindless,
            &self.pipelines,
            &mut graph,
            slot,
            (image, view, extent),
            gui,
            textures,
            !drew_world,
            figure,
        ) {
            error!("native renderer: gui: {}", e.0);
        }
        graph.pass("present", &[(image, Use::Present)], &[], |_| {});
        drop(graph);
        match self.frames.end(&self.ctx, swapchain, recording) {
            Ok(()) => {}
            Err(FrameError::OutOfDate) => self.stale = true,
            Err(FrameError::Failed(e)) => error!("native renderer: present failed: {e:?}"),
        }
        self.frame += 1;
    }
}

/// The GUI painter's output, drawn over the frame (blended, no depth).
struct GuiPass {
    module: vk::ShaderModule,
    pipeline: vk::Pipeline,
    /// Per frame in flight: vertices and indices (host-visible, grown).
    buffers: Vec<Option<((Buffer, u32), Buffer)>>,
    /// Atlas, unihex font, white (glint and figure stand-ins).
    textures: Option<[(Image, u32); 2]>,
    /// The item glint, once the assets have it.
    glint: Option<(Image, u32)>,
    white: (Image, u32),
    /// The white texel's upload, until it's done.
    white_pending: Option<Buffer>,
    /// Staging buffers of past uploads, freed with the pass.
    spent: Vec<Buffer>,
}

impl GuiPass {
    fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        format: vk::Format,
    ) -> Result<GuiPass, SetupError> {
        let module = pipelines.module(ctx, super::shaders::GUI)?;
        let pipeline = pipelines.graphics(
            ctx,
            &GraphicsDesc {
                module,
                vertex: c"gui_vertex",
                fragment: c"gui_fragment",
                color: &[format],
                depth: None,
                depth_test: None,
                depth_write: false,
                cull: vk::CullModeFlags::NONE,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: true,
                additive: false,
                fragment_module: None,
            },
        )?;
        let one = vk::Extent2D { width: 1, height: 1 };
        let image = memory.image(
            ctx,
            "gui white",
            one,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let index = bindless
            .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .ok_or_else(|| SetupError("the bindless set is full".into()))?;
        let mut staging = memory.buffer(ctx, "gui white staging", 4, vk::BufferUsageFlags::TRANSFER_SRC, MemoryLocation::CpuToGpu)?;
        staging.write(0, &[255; 4]);
        Ok(GuiPass {
            module,
            pipeline,
            buffers: (0..FRAMES_IN_FLIGHT).map(|_| None).collect(),
            textures: None,
            glint: None,
            white: (image, index),
            white_pending: Some(staging),
            spent: Vec::new(),
        })
    }

    fn destroy(self, ctx: &Context, memory: &Memory) {
        // SAFETY: the device is idle.
        unsafe {
            ctx.device.destroy_pipeline(self.pipeline, None);
            ctx.device.destroy_shader_module(self.module, None);
        }
        for ((v, _), i) in self.buffers.into_iter().flatten() {
            memory.free_buffer(ctx, v);
            memory.free_buffer(ctx, i);
        }
        for b in self.spent.into_iter().chain(self.white_pending) {
            memory.free_buffer(ctx, b);
        }
        for (image, _) in self.textures.into_iter().flatten().chain(self.glint) {
            memory.free_image(ctx, image);
        }
        memory.free_image(ctx, self.white.0);
    }

    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        slot: usize,
        output: (vk::Image, vk::ImageView, vk::Extent2D),
        frame: crate::gui::pool::GuiFrame,
        (textures, glint_data): (Option<[TextureData; 2]>, Option<TextureData>),
        clear: bool,
        figure: Option<(vk::Image, u32)>,
    ) -> Result<(), SetupError> {
        let device = &ctx.device;
        // Uploads: the white texel once, the GUI textures once they exist.
        let mut uploads: Vec<(Buffer, vk::Image, vk::Extent2D)> = Vec::new();
        // Textures already uploaded sit sampled since their first frame.
        let sampled = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        if self.white_pending.is_none() {
            graph.image(self.white.0.raw, vk::ImageAspectFlags::COLOR, sampled);
        }
        for (image, _) in self.textures.iter().flatten().chain(&self.glint) {
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, sampled);
        }
        if let Some(staging) = self.white_pending.take() {
            graph.image(self.white.0.raw, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
            uploads.push((staging, self.white.0.raw, vk::Extent2D { width: 1, height: 1 }));
        }
        if self.textures.is_none()
            && let Some(data) = textures
        {
            let mut made = Vec::new();
            for (name, (w, h, format, bytes)) in ["gui atlas", "gui unihex"].into_iter().zip(data) {
                let extent = vk::Extent2D { width: w, height: h };
                let image = memory.image(
                    ctx,
                    name,
                    extent,
                    format,
                    vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                    vk::ImageAspectFlags::COLOR,
                )?;
                let index = bindless
                    .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .ok_or_else(|| SetupError("the bindless set is full".into()))?;
                let mut staging = memory.buffer(
                    ctx,
                    "gui texture staging",
                    bytes.len() as u64,
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    MemoryLocation::CpuToGpu,
                )?;
                staging.write(0, &bytes);
                graph.image(image.raw, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
                uploads.push((staging, image.raw, extent));
                made.push((image, index));
            }
            let mut made = made.into_iter();
            if let (Some(a), Some(b)) = (made.next(), made.next()) {
                self.textures = Some([a, b]);
            }
        }
        if self.glint.is_none()
            && let Some((w, h, format, bytes)) = glint_data
        {
            let extent = vk::Extent2D { width: w, height: h };
            let image = memory.image(
                ctx,
                "gui glint",
                extent,
                format,
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                vk::ImageAspectFlags::COLOR,
            )?;
            let index = bindless
                .add_texture(ctx, image.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(|| SetupError("the bindless set is full".into()))?;
            let mut staging = memory.buffer(ctx, "gui glint staging", bytes.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC, MemoryLocation::CpuToGpu)?;
            staging.write(0, &bytes);
            graph.image(image.raw, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
            uploads.push((staging, image.raw, extent));
            self.glint = Some((image, index));
        }
        if !uploads.is_empty() {
            let uses: Vec<_> = uploads.iter().map(|(_, i, _)| (*i, Use::TransferDst)).collect();
            graph.pass("gui uploads", &uses, &[], |cmd| unsafe {
                // SAFETY: copies into images in TRANSFER_DST, each staging
                // buffer sized for its image.
                for (staging, image, extent) in &uploads {
                    device.cmd_copy_buffer_to_image(
                        cmd,
                        staging.raw,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::BufferImageCopy {
                            image_subresource: vk::ImageSubresourceLayers {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                mip_level: 0,
                                base_array_layer: 0,
                                layer_count: 1,
                            },
                            image_extent: vk::Extent3D {
                                width: extent.width,
                                height: extent.height,
                                depth: 1,
                            },
                            ..Default::default()
                        }],
                    );
                }
            });
            // Kept until the pass goes: a handful of one-off uploads.
            self.spent.extend(uploads.into_iter().map(|(staging, ..)| staging));
        }

        // This frame's vertices and indices, into the slot's buffers.
        let (vbytes, ibytes): (&[u8], &[u8]) = (bytemuck::cast_slice(&frame.vertices), bytemuck::cast_slice(&frame.indices));
        let fits = self.buffers[slot]
            .as_ref()
            .is_some_and(|((v, _), i)| v.size >= vbytes.len() as u64 && i.size >= ibytes.len() as u64);
        if !fits {
            if let Some(((v, vi), i)) = self.buffers[slot].take() {
                // Frames in flight are done with this slot (its fence).
                bindless.remove_buffer(vi, 0);
                memory.free_buffer(ctx, v);
                memory.free_buffer(ctx, i);
            }
            let size = |n: usize| (n.max(1) as u64).next_power_of_two().max(1 << 16);
            let v = memory.buffer(ctx, "gui vertices", size(vbytes.len()), vk::BufferUsageFlags::STORAGE_BUFFER, MemoryLocation::CpuToGpu)?;
            let vi = bindless
                .add_buffer(ctx, v.raw)
                .ok_or_else(|| SetupError("the bindless set is full".into()))?;
            let i = memory.buffer(ctx, "gui indices", size(ibytes.len()), vk::BufferUsageFlags::INDEX_BUFFER, MemoryLocation::CpuToGpu)?;
            self.buffers[slot] = Some(((v, vi), i));
        }
        let Some(((v, vi), i)) = &mut self.buffers[slot] else {
            return Ok(());
        };
        v.write(0, vbytes);
        i.write(0, ibytes);
        let (vertex_index, index_buffer) = (*vi, i.raw);

        let (image, view, extent) = output;
        let tex = self.textures.as_ref().map(|t| [t[0].1, t[1].1]);
        let white = self.white.1;
        let mut uses = vec![(image, Use::ColorAttachment)];
        uses.push((self.white.0.raw, Use::SampledFragment));
        if let Some(t) = &self.textures {
            uses.extend(t.iter().map(|(i, _)| (i.raw, Use::SampledFragment)));
        }
        if let Some((g, _)) = &self.glint {
            uses.push((g.raw, Use::SampledFragment));
        }
        let glint = self.glint.as_ref().map_or(self.white.1, |g| g.1);
        let figure_index = match figure {
            Some((image, index)) => {
                uses.push((image, Use::SampledFragment));
                index
            }
            None => self.white.1,
        };
        let ranges = frame.ranges;
        let pipeline = self.pipeline;
        graph.pass("gui", &uses, &[], |cmd| unsafe {
            // SAFETY: the output in COLOR_ATTACHMENT, textures sampled; the
            // shared layout and set.
            let color = [vk::RenderingAttachmentInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(if clear { vk::AttachmentLoadOp::CLEAR } else { vk::AttachmentLoadOp::LOAD })
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] },
                })
                .store_op(vk::AttachmentStoreOp::STORE)];
            let info = vk::RenderingInfo::default()
                .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent })
                .layer_count(1)
                .color_attachments(&color);
            device.cmd_begin_rendering(cmd, &info);
            if let Some([atlas, unihex]) = tex
                && !ranges.is_empty()
            {
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
                device.cmd_set_scissor(cmd, 0, &[vk::Rect2D { offset: vk::Offset2D::default(), extent }]);
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, pipelines.layout, 0, &[bindless.set], &[]);
                device.cmd_bind_index_buffer(cmd, index_buffer, 0, vk::IndexType::UINT32);
                push(
                    ctx,
                    cmd,
                    pipelines.layout,
                    &Push {
                        ids: [vertex_index, 0, 0, 0],
                        more: [atlas, unihex, glint, figure_index],
                        params: [0.0; 4],
                        view_proj: [[0.0; 4]; 4],
                    },
                );
                for r in &ranges {
                    device.cmd_draw_indexed(cmd, r.index_count, 1, r.first_index, r.base_vertex, 0);
                }
            }
            device.cmd_end_rendering(cmd);
        });
        Ok(())
    }
}

/// Terrain options, with stages turned off by `MC_NATIVE_TERRAIN`
/// (comma list: `nohiz`, `novis`, `nobuckets`, `nopacked`), for bisecting.
fn terrain_options() -> super::terrain::Options {
    let mut o = super::terrain::Options::default();
    let flags = std::env::var("MC_NATIVE_TERRAIN").unwrap_or_default();
    for flag in flags.split(',').map(str::trim) {
        match flag {
            "nohiz" => o.hiz = false,
            "novis" => o.visibility = false,
            "nobuckets" => o.buckets = false,
            "nopacked" => o.packed = false,
            _ => {}
        }
    }
    if !flags.is_empty() {
        info!("native renderer: terrain options {o:?}");
    }
    o
}

/// A scene replaced by a newer snapshot, waiting out the frames in flight.
enum Retired {
    Env(super::env::EnvScene),
    Entities(super::entities::EntityScene),
    Particles(super::particles::ParticleScene),
}

impl Retired {
    fn release(&self, bindless: &mut Bindless, frame: u64) {
        match self {
            Retired::Env(s) => s.release(bindless, frame),
            Retired::Entities(s) => s.release(bindless, frame),
            Retired::Particles(s) => s.release(bindless, frame),
        }
    }

    fn destroy(self, ctx: &Context, memory: &Memory) {
        match self {
            Retired::Env(s) => s.destroy(ctx, memory),
            Retired::Entities(s) => s.destroy(ctx, memory),
            Retired::Particles(s) => s.destroy(ctx, memory),
        }
    }
}

/// The inventory's player figure: its entity scene, drawn from the figure
/// camera into a texture the GUI samples.
#[derive(Default)]
struct Figure {
    scene: Option<super::entities::EntityScene>,
    /// This frame's view-projection, eye and texture size (none: hidden).
    view: Option<(bevy::math::Mat4, bevy::math::Vec3, vk::Extent2D)>,
    /// The target: colour (and its bindless index) and depth.
    target: Option<((Image, u32), Image)>,
    /// Per frame in flight: the view data, on the bindless set.
    views: Vec<(Buffer, u32)>,
    /// Targets and views given up, with the frame they were last used on.
    spent: Vec<(Image, u64)>,
}

impl Figure {
    /// Draws the figure; its texture (image, bindless index) for the GUI.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        uploads: &mut Uploads,
        graph: &mut Graph,
        frame: u64,
        slot: usize,
    ) -> Result<Option<(vk::Image, u32)>, SetupError> {
        // Old targets once no frame in flight reads them.
        let (done, keep): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.spent).into_iter().partition(|(_, at)| frame >= at + FRAMES_IN_FLIGHT as u64);
        self.spent = keep;
        for (image, _) in done {
            memory.free_image(ctx, image);
        }
        let (Some((view_proj, eye, extent)), Some(scene)) = (self.view, self.scene.as_mut()) else {
            return Ok(None);
        };
        let full = || SetupError("the bindless set is full".into());
        if self.views.is_empty() {
            for _ in 0..FRAMES_IN_FLIGHT {
                let buffer = memory.buffer(
                    ctx,
                    "figure view",
                    256,
                    vk::BufferUsageFlags::STORAGE_BUFFER,
                    MemoryLocation::CpuToGpu,
                )?;
                let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
                self.views.push((buffer, index));
            }
        }
        if self.target.as_ref().is_none_or(|((c, _), _)| c.extent != extent) {
            if let Some(((color, index), depth)) = self.target.take() {
                bindless.remove_texture(index, frame);
                self.spent.push((color, frame));
                self.spent.push((depth, frame));
            }
            let color = memory.image(
                ctx,
                "figure",
                extent,
                vk::Format::R8G8B8A8_SRGB,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                vk::ImageAspectFlags::COLOR,
            )?;
            let index = bindless
                .add_texture(ctx, color.view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .ok_or_else(full)?;
            let depth = memory.image(
                ctx,
                "figure depth",
                extent,
                super::depth_view::DEPTH_FORMAT,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
            )?;
            self.target = Some(((color, index), depth));
        }
        let Some(((color, index), depth)) = &self.target else {
            return Ok(None);
        };
        graph.image(color.raw, vk::ImageAspectFlags::COLOR, vk::ImageLayout::UNDEFINED);
        graph.image(depth.raw, vk::ImageAspectFlags::DEPTH, vk::ImageLayout::UNDEFINED);
        let (view_buffer, view_index) = &mut self.views[slot];
        view_buffer.write(0, &super::terrain::plain_view(view_proj, eye));
        let view_index = *view_index;

        scene.prepare(ctx, memory, graph, frame, slot);
        scene.record_cull(ctx, bindless, pipelines, uploads, graph, view_index, None, eye, slot)?;
        let mut images = vec![(color.raw, Use::ColorAttachment), (depth.raw, Use::DepthAttachment)];
        let mut buffers = vec![(view_buffer.raw, Use::SampledFragment)];
        scene.uses(&mut images, &mut buffers);
        let device = &ctx.device;
        let cols = view_proj.to_cols_array_2d();
        let seconds = frame as f32 / 60.0;
        let (color_view, depth_view) = (color.view, depth.view);
        let scene = &*scene;
        graph.pass("figure", &images, &buffers, |cmd| unsafe {
            // SAFETY: the figure's attachments in the layouts the graph set;
            // the shared layout and set.
            let colors = [vk::RenderingAttachmentInfo::default()
                .image_view(color_view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .clear_value(vk::ClearValue { color: vk::ClearColorValue { float32: [0.0; 4] } })
                .store_op(vk::AttachmentStoreOp::STORE)];
            let depth_attachment = vk::RenderingAttachmentInfo::default()
                .image_view(depth_view)
                .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .clear_value(vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue { depth: 0.0, stencil: 0 },
                })
                .store_op(vk::AttachmentStoreOp::DONT_CARE);
            device.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent })
                    .layer_count(1)
                    .color_attachments(&colors)
                    .depth_attachment(&depth_attachment),
            );
            super::depth_view::set_viewport(ctx, cmd, extent);
            device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, pipelines.layout, 0, &[bindless.set], &[]);
            let mode = super::test_scene::View::Shaded.mode();
            scene.draw_solid(ctx, cmd, pipelines, cols, mode, seconds, view_index);
            scene.draw_blend(ctx, cmd, pipelines, cols, mode, seconds, view_index);
            device.cmd_end_rendering(cmd);
        });
        Ok(Some((color.raw, *index)))
    }

    fn destroy(self, ctx: &Context, memory: &Memory) {
        if let Some(scene) = self.scene {
            scene.destroy(ctx, memory);
        }
        if let Some(((color, _), depth)) = self.target {
            memory.free_image(ctx, color);
            memory.free_image(ctx, depth);
        }
        for (b, _) in self.views {
            memory.free_buffer(ctx, b);
        }
        for (image, _) in self.spent {
            memory.free_image(ctx, image);
        }
    }
}
