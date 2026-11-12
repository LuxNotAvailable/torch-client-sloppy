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

/// Where the native renderer's frame goes, in nanoseconds summed since
/// the last report (`MC_NATIVE_TIMING`): the entity and particle captures,
/// the scene renewals, the frame's recording and submission, the whole
/// renderer system, and frames counted.
static TIMES: [std::sync::atomic::AtomicU64; 13] = [const { std::sync::atomic::AtomicU64::new(0) }; 13];
const TIME_NAMES: [&str; 12] = [
    "capture entities+env",
    "capture particles",
    "renew scenes",
    "record+submit",
    "renderer system",
    "env",
    "entities",
    "particles",
    "acquire (fence)",
    "terrain record",
    "gui record",
    "submit+present",
];
/// The slot `report_times` counts frames in.
const FRAMES_SLOT: usize = 12;

/// New meshes and textures sent since the last report, and their sizes.
static NEW: [std::sync::atomic::AtomicU64; 4] = [const { std::sync::atomic::AtomicU64::new(0) }; 4];

/// Counts a new mesh (0, `size` vertices) or texture (1, `size` bytes).
pub fn count_new(kind: usize, size: usize) {
    NEW[kind].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    NEW[kind + 2].fetch_add(size as u64, std::sync::atomic::Ordering::Relaxed);
}

/// Times its scope into `TIMES[slot]`.
pub struct Timed(usize, std::time::Instant);

impl Timed {
    pub fn new(slot: usize) -> Timed {
        Timed(slot, std::time::Instant::now())
    }
}

impl Drop for Timed {
    fn drop(&mut self) {
        TIMES[self.0].fetch_add(self.1.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Logs the average milliseconds per frame of each timed part, every two
/// seconds.
fn report_times(last: &mut Option<std::time::Instant>) {
    use std::sync::atomic::Ordering::Relaxed;
    TIMES[FRAMES_SLOT].fetch_add(1, Relaxed);
    let now = std::time::Instant::now();
    let Some(since) = *last else {
        *last = Some(now);
        return;
    };
    let secs = now.duration_since(since).as_secs_f32();
    if secs < 2.0 {
        return;
    }
    *last = Some(now);
    let frames = TIMES[FRAMES_SLOT].swap(0, Relaxed).max(1);
    let parts: Vec<String> = TIME_NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{n} {:.2}", TIMES[i].swap(0, Relaxed) as f64 / 1e6 / frames as f64))
        .collect();
    let new: Vec<u64> = NEW.iter().map(|n| n.swap(0, Relaxed)).collect();
    info!(
        "native renderer: {:.0} fps, ms/frame: {}; new meshes {} ({} verts), textures {} ({} KiB)",
        frames as f32 / secs,
        parts.join(", "),
        new[0],
        new[2],
        new[1],
        new[3] / 1024
    );
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
    /// The shader pack drawing the frame, when one is chosen: its name and
    /// screen, the size it was made for.
    pack: Option<(String, super::pack::screen::PackScreen, vk::Extent2D)>,
    /// When the chosen pack was last looked up, and the name found.
    pack_checked: Option<(std::time::Instant, Option<String>)>,
    /// The frame's pack inputs (time of day, weather, the eye's light).
    pack_inputs: Option<crate::renderer::packdraw::inputs::Inputs>,
    started: std::time::Instant,
    /// In a menu: the panorama draws.
    menu: bool,
    /// The title panorama, made the first time a menu shows.
    panorama: Option<super::panorama::PanoramaScene>,
    /// FXAA over a pack's frame (the pack draws into its scene image).
    pack_post: Option<super::post::PostPass>,
    /// The shader render scale (percent): packs draw at that share of the
    /// window, final scaling up.
    pack_scale: u32,
    /// The game's vsync option, and its anti-aliasing as the post pass's.
    vsync: bool,
    post: super::post::Settings,
    /// Screenshot entities to capture this frame.
    shots: Vec<Entity>,
    /// Frames copied back for screenshots: the buffer, its size and
    /// format, the entities waiting, and the frame it was copied on.
    readbacks: Vec<(Buffer, vk::Extent2D, vk::Format, Vec<Entity>, u64)>,
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
        pack: None,
        pack_checked: None,
        pack_inputs: None,
        started: std::time::Instant::now(),
        menu: false,
        panorama: None,
        shots: Vec::new(),
        readbacks: Vec::new(),
        vsync: true,
        pack_scale: 100,
        post: Default::default(),
        pack_post: None,
        frame: 0,
        size,
        stale: false,
    })
}

/// What the main thread hands the render thread each frame.
struct FrameInput {
    size: Option<(u32, u32)>,
    gui: crate::gui::pool::GuiFrame,
    textures: (Option<[TextureData; 2]>, Option<TextureData>),
    camera: Option<Camera>,
    figure_view: Option<(bevy::math::Mat4, bevy::math::Vec3, vk::Extent2D)>,
    menu: bool,
    shots: Vec<Entity>,
    post: super::post::Settings,
    vsync: bool,
    pack_scale: u32,
}

/// The render thread (8.5): it owns the renderer and draws frame N while
/// the main thread runs frame N + 1, as Bevy's pipelined rendering did.
struct RenderThread {
    input: Option<std::sync::mpsc::SyncSender<FrameInput>>,
    captures: std::sync::mpsc::Receiver<Vec<(Entity, bevy::image::Image)>>,
    /// Whether the GUI pass still wants its atlas and its glint.
    wants: std::sync::Arc<[std::sync::atomic::AtomicBool; 2]>,
    /// One message per frame the render thread finished.
    done: std::sync::mpsc::Receiver<()>,
    /// Frames drawn in step with the main thread (low frame rates), and
    /// the smoothed frame time that decides it.
    in_step: bool,
    frame_time: f32,
    /// Frames sent and not yet finished.
    outstanding: u32,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for RenderThread {
    fn drop(&mut self) {
        // Closing the channel ends the loop; the renderer frees itself there.
        self.input.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn spawn(mut live: Live) -> RenderThread {
    use std::sync::atomic::{AtomicBool, Ordering};
    // One frame in flight between the threads: the main thread runs at most
    // one frame ahead.
    let (input, frames) = std::sync::mpsc::sync_channel::<FrameInput>(1);
    let (send_captures, captures) = std::sync::mpsc::channel();
    let (send_done, done) = std::sync::mpsc::channel();
    let wants = std::sync::Arc::new([AtomicBool::new(true), AtomicBool::new(true)]);
    let flags = wants.clone();
    let handle = std::thread::Builder::new()
        .name("native renderer".into())
        .spawn(move || {
            while let Ok(f) = frames.recv() {
                live.frame(f);
                flags[0].store(live.gui.textures.is_none(), Ordering::Relaxed);
                flags[1].store(live.gui.glint.is_none(), Ordering::Relaxed);
                let captured = live.take_captures();
                if !captured.is_empty() {
                    let _ = send_captures.send(captured);
                }
                let _ = send_done.send(());
            }
        })
        .expect("the render thread starts");
    RenderThread {
        input: Some(input),
        captures,
        wants,
        done,
        in_step: false,
        frame_time: 0.0,
        outstanding: 0,
        handle: Some(handle),
    }
}

fn draw_frame(world: &mut World, mut last: Local<Option<std::time::Instant>>) {
    use std::sync::atomic::Ordering;
    report_times(&mut last);
    let _timed = Timed::new(4);
    if world.get_non_send_resource::<RenderThread>().is_none() {
        match setup(world) {
            Ok(live) => world.insert_non_send_resource(spawn(live)),
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
    let gui = world
        .get_resource::<crate::gui::pool::GuiFrames>()
        .map(|f| std::mem::take(&mut *f.0.lock()))
        .unwrap_or_default();
    let wanted = world
        .get_non_send_resource::<RenderThread>()
        .map_or((true, true), |t| (t.wants[0].load(Ordering::Relaxed), t.wants[1].load(Ordering::Relaxed)));
    let textures = match wanted {
        (false, false) => (None, None),
        _ => gui_textures(world),
    };
    // Bevy's render world would take these; nothing does now.
    if let Some(ops) = world.get_resource::<crate::renderer::terrain_pool::TerrainOps>() {
        ops.0.lock().clear();
    }
    // In menus the title panorama shows instead of the world, as the game
    // switches its cameras.
    let menu = world
        .query_filtered::<&bevy::camera::Camera, With<crate::renderer::panorama::PanoramaCamera>>()
        .iter(world)
        .next()
        .is_some_and(|c| c.is_active);
    let camera = if menu { None } else { world_camera(world) };
    let figure_view = figure_view(world);
    // Screenshots the game asked for (`renderer::screenshot`): captured by
    // the renderer, answered with the event Bevy's renderer would send.
    let mut requests = world.query_filtered::<Entity, (
        With<bevy::render::view::screenshot::Screenshot>,
        Without<NativeCapture>,
    )>();
    let shots: Vec<Entity> = requests.iter(world).collect();
    for &e in &shots {
        world.entity_mut(e).insert(NativeCapture);
    }
    // The game's video options.
    let pack_scale = world
        .get_resource::<crate::gui::GuiState>()
        .map_or(100, |g| g.options.pack_render_scale);
    let (post, vsync) = match world.get_resource::<crate::gui::GuiState>() {
        Some(gui) => {
            use crate::renderer::antialias::{AaMode, SSAA_SCALES};
            let aa = gui.options.antialiasing;
            let post = super::post::Settings {
                // MSAA has no native path: FXAA stands in.
                fxaa: matches!(aa.mode, AaMode::Fxaa | AaMode::Msaa),
                ssaa: if aa.mode == AaMode::Supersample {
                    SSAA_SCALES.get(aa.ssaa).copied().unwrap_or(1.0)
                } else {
                    1.0
                },
                effects: None,
            };
            SMOOTH_FIGURE.store(gui.options.smooth_gui_model, std::sync::atomic::Ordering::Relaxed);
            (post, gui.options.vsync)
        }
        None => (Default::default(), true),
    };
    let input = FrameInput {
        size,
        gui,
        textures,
        camera,
        figure_view,
        menu,
        shots,
        post,
        vsync,
        pack_scale,
    };
    let dt = world.get_resource::<Time>().map_or(0.0, |t| t.delta_secs());
    let Some(mut thread) = world.get_non_send_resource_mut::<RenderThread>() else {
        return;
    };
    // Pipelined (a frame behind) only while frames are quick; below about
    // 200 fps the frame is drawn in step, so input shows a frame sooner.
    thread.frame_time = thread.frame_time * 0.9 + dt * 0.1;
    // `MC_NATIVE_PIPELINE=always` or `never` overrides, for comparing.
    static FORCED: OnceLock<Option<bool>> = OnceLock::new();
    let forced = *FORCED.get_or_init(|| match std::env::var("MC_NATIVE_PIPELINE").as_deref() {
        Ok("always") => Some(false),
        Ok("never") => Some(true),
        _ => None,
    });
    thread.in_step = forced.unwrap_or(if thread.in_step {
        thread.frame_time > 1.0 / 220.0
    } else {
        thread.frame_time > 1.0 / 180.0
    });
    // Waits only while the renderer is still on the frame before last.
    if thread.input.as_ref().is_none_or(|i| i.send(input).is_err()) {
        error!("native renderer: the render thread stopped");
        std::process::exit(3);
    }
    thread.outstanding += 1;
    while thread.done.try_recv().is_ok() {
        thread.outstanding -= 1;
    }
    // In step: this frame is drawn before the main thread goes on.
    while thread.in_step && thread.outstanding > 0 {
        if thread.done.recv().is_err() {
            break;
        }
        thread.outstanding -= 1;
    }
    let captured: Vec<_> = thread.captures.try_iter().flatten().collect();
    for (entity, image) in captured {
        world.trigger(bevy::render::view::screenshot::ScreenshotCaptured { entity, image });
        if let Ok(e) = world.get_entity_mut(entity) {
            e.despawn();
        }
    }
}

impl Live {
    /// One frame on the render thread, from what the main thread gathered.
    fn frame(&mut self, f: FrameInput) {
        if let Some(size) = f.size
            && size != self.size
        {
            self.size = size;
            self.stale = true;
        }
        if let Some(c) = f.camera {
            self.eye = c.eye;
        }
        self.feed_terrain();
        self.update_pack();
        self.shots.extend(f.shots);
        if (self.post.fxaa, self.post.ssaa) != (f.post.fxaa, f.post.ssaa) {
            info!("native renderer: anti-aliasing -> fxaa {}, ssaa {}", f.post.fxaa, f.post.ssaa);
            // The post pass frees its targets on change; let the GPU finish with them.
            // SAFETY: only waits.
            let _ = unsafe { self.ctx.device.device_wait_idle() };
        }
        self.post = f.post;
        if f.vsync != self.vsync {
            self.vsync = f.vsync;
            self.stale = true;
        }
        self.pack_scale = f.pack_scale;
        self.figure.view = f.figure_view;
        self.menu = f.menu;
        let timed = Timed::new(3);
        self.draw(f.gui, f.textures, f.camera);
        drop(timed);
    }
}

/// A screenshot request the native renderer has taken on.
#[derive(Component)]
struct NativeCapture;

/// Hands the mesher a pack's block ids and has the chunks re-meshed with
/// its pack quads (`packdraw::install::sync_pack_vertices` does this for
/// the game's own compile, which the native renderer doesn't make).
fn publish_block_ids(ids: Option<std::sync::Arc<crate::renderer::packvertex::BlockIds>>) {
    let old = crate::renderer::packvertex::current();
    if old.as_deref() == ids.as_deref() {
        return;
    }
    crate::renderer::packvertex::set_current(ids);
    crate::client::bus::request(crate::client::bus::ClientRequest::ReloadChunks);
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
            if let Some(p) = self.panorama.take() {
                p.destroy(&self.ctx, &memory);
            }
            if let Some(p) = self.pack_post.take() {
                p.destroy(&self.ctx, &memory);
            }
            for (b, ..) in self.readbacks.drain(..) {
                memory.free_buffer(&self.ctx, b);
            }
            if let Some((_, pack, _)) = self.pack.take() {
                pack.destroy(&self.ctx, &memory);
            }
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
        let timed = Timed::new(2);
        self.renew_scenes();
        drop(timed);
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
        let timed = Timed::new(5);
        let mut env_renewed = false;
        if let Some(bytes) = dump::take_live("env.bin") {
            match dump::read_env(&bytes) {
                Ok(env) => match t.renew_env(ctx, memory, bindless, pipelines, format, env, self.eye) {
                    Ok(o) => {
                        old.extend(o.map(Retired::Env));
                        env_renewed = true;
                    }
                    Err(e) => fail("env", e.0),
                },
                Err(e) => fail("env", e),
            }
        }
        drop(timed);
        let timed = Timed::new(6);
        // Entities stay; each frame updates them (new meshes and textures,
        // then the instances).
        if let Some(frame) = dump::take_live_entities(dump::LIVE_WORLD)
            && let Err(e) = t.update_entities(ctx, memory, bindless, pipelines, format, frame)
        {
            fail("entities", e.0);
        }
        drop(timed);
        let _timed = Timed::new(7);
        if let Some(frame) = dump::take_live_particles()
            && let Err(e) = t.update_particles(ctx, memory, bindless, pipelines, format, frame)
        {
            fail("particles", e.0);
        }
        if let Some(frame) = dump::take_live_entities(dump::LIVE_FIGURE) {
            if self.figure.scene.is_none() {
                match super::entities::EntityScene::new(ctx, memory, bindless, pipelines, format, dump::Snapshot::empty(), None) {
                    Ok(scene) => self.figure.scene = Some(scene),
                    Err(e) => fail("figure", e.0),
                }
            }
            if let Some(scene) = &mut self.figure.scene
                && let Err(e) = scene.update(ctx, memory, bindless, frame)
            {
                fail("figure", e.0);
            }
        }
        if let Some(bytes) = dump::take_live("inputs.json")
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            self.pack_inputs = Some(crate::renderer::packdraw::inputs::Inputs::from_json(&value));
        }
        // A pack draws the sky and hand as its own entities: renewed with
        // the environment.
        // Only on a renewed environment: its release frees the last ones.
        if let Some((_, pack, _)) = &self.pack
            && env_renewed
            && let Err(e) = t.pack_sky(
                ctx,
                memory,
                bindless,
                pack.sun_path_rotation(),
                self.pack_inputs.as_ref().map_or([0.0, 15.0], |i| [i.eye_light.x as f32, i.eye_light.y as f32]),
            )
        {
            fail("pack sky", e.0);
        }
        for scene in old {
            scene.release(bindless, self.frame);
            self.retired.push((scene, self.frame));
        }
    }

    /// Keeps the pack in step with the one chosen (looked up once a
    /// second) and the swapchain's size.
    fn update_pack(&mut self) {
        let now = std::time::Instant::now();
        let stale = self
            .pack_checked
            .as_ref()
            .is_none_or(|(at, _)| now.duration_since(*at).as_secs_f32() > 1.0);
        if stale {
            let name = crate::platform::storage::SHADER_PACK
                .load()
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty());
            self.pack_checked = Some((now, name));
        }
        let wanted = self.pack_checked.as_ref().and_then(|(_, n)| n.clone());
        // The pack's targets at the render scale; final fills the window.
        let scale = self.pack_scale.clamp(1, 100);
        let extent = self.swapchain.as_ref().map(|s| vk::Extent2D {
            width: (s.extent.width * scale / 100).max(1),
            height: (s.extent.height * scale / 100).max(1),
        });
        let current = self.pack.as_ref().map(|(n, _, e)| (n.clone(), *e));
        if current.as_ref().map(|(n, e)| (Some(n.clone()), Some(*e))) == Some((wanted.clone(), extent))
            || (current.is_none() && wanted.is_none())
        {
            return;
        }
        let (Some(terrain), Some(extent), Some(swapchain)) = (&self.terrain, extent, &self.swapchain) else {
            return;
        };
        self.ctx.idle();
        if let Some((_, old, _)) = self.pack.take() {
            old.destroy(&self.ctx, &self.memory);
        }
        let Some(name) = wanted else {
            // No pack: the mesher drops its pack quads.
            publish_block_ids(None);
            return;
        };
        let installed = match super::pack::load(&name, false) {
            Ok(i) => i,
            Err(e) => {
                error!("native renderer: pack {name}: {e}");
                // Don't retry every frame.
                self.pack_checked = Some((now, None));
                return;
            }
        };
        match super::pack::screen::PackScreen::new(
            &self.ctx,
            &self.memory,
            &mut self.bindless,
            &self.pipelines,
            installed,
            extent,
            swapchain.format.format,
            Some(&terrain.pack_geometry()),
        ) {
            Ok(screen) => {
                info!("native renderer: pack {name}: {}", screen.describe());
                // The mesher's pack quads (normals, block ids) for this pack,
                // as the game's own compile would publish them.
                publish_block_ids(screen.block_ids());
                self.pack = Some((name, screen, extent));
            }
            Err(e) => {
                error!("native renderer: pack {name}: {}", e.0);
                self.pack_checked = Some((now, None));
            }
        }
    }

    /// Screenshots whose copies the GPU has finished, as images.
    fn take_captures(&mut self) -> Vec<(Entity, bevy::image::Image)> {
        let now = self.frame;
        let (done, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.readbacks)
            .into_iter()
            .partition(|r| now >= r.4 + FRAMES_IN_FLIGHT as u64);
        self.readbacks = keep;
        // Screenshots are rare: wait out the copy rather than trust frame counts.
        if !done.is_empty() {
            // SAFETY: only waits.
            let _ = unsafe { self.ctx.device.device_wait_idle() };
        }
        let mut out = Vec::new();
        for (mut buffer, extent, format, entities, _) in done {
            if let Some(bytes) = buffer.mapped() {
                let mut rgba = bytes[..(extent.width * extent.height * 4) as usize].to_vec();
                // Swapchains are mostly BGRA.
                if matches!(format, vk::Format::B8G8R8A8_SRGB | vk::Format::B8G8R8A8_UNORM) {
                    for px in rgba.chunks_exact_mut(4) {
                        px.swap(0, 2);
                    }
                }
                for entity in entities {
                    let image = bevy::image::Image::new(
                        bevy::render::render_resource::Extent3d {
                            width: extent.width,
                            height: extent.height,
                            depth_or_array_layers: 1,
                        },
                        bevy::render::render_resource::TextureDimension::D2,
                        rgba.clone(),
                        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                        bevy::asset::RenderAssetUsages::MAIN_WORLD,
                    );
                    out.push((entity, image));
                }
            }
            self.memory.free_buffer(&self.ctx, buffer);
        }
        out
    }

    fn recreate(&mut self) {
        self.ctx.idle();
        match Swapchain::new(&self.ctx, self.size, self.vsync, self.swapchain.as_ref()) {
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
        let timed = Timed::new(8);
        let begun = self.frames.begin(&self.ctx, swapchain);
        drop(timed);
        let recording = match begun {
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
        // The world, when there's one to see; in menus, the panorama.
        let mut drew_world = false;
        if self.menu {
            if self.panorama.is_none() {
                match super::panorama::PanoramaScene::new(&self.ctx, &self.memory, &mut self.bindless, &self.pipelines, swapchain.format.format) {
                    Ok(p) => self.panorama = Some(p),
                    Err(e) => {
                        error!("native renderer: panorama: {}", e.0);
                        self.menu = false;
                    }
                }
            }
            if let Some(p) = &mut self.panorama {
                match p.record(
                    &self.ctx,
                    &self.memory,
                    &self.bindless,
                    &self.pipelines,
                    &mut graph,
                    (image, view),
                    extent,
                    rotation,
                    self.frame,
                    self.started.elapsed().as_secs_f32(),
                ) {
                    Ok(()) => drew_world = true,
                    Err(e) => error!("native renderer: panorama: {}", e.0),
                }
            }
        }
        if let (Some(terrain), Some(camera)) = (&mut self.terrain, camera) {
            terrain.set_post(self.post);
            // Under a pack the terrain only culls; the pack draws.
            terrain.set_cull_only(self.pack.is_some());
            let _timed = Timed::new(9);
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
            if let Some((_, pack, pack_extent)) = &mut self.pack {
                let pack_extent = *pack_extent;
                if let Some(inputs) = self.pack_inputs {
                    pack.inputs = inputs;
                }
                let world_from_view =
                    bevy::math::Mat4::look_to_rh(camera.eye, camera.forward(), bevy::math::Vec3::Y).inverse();
                let pack_view = (world_from_view, camera.fov.to_radians());
                let seconds = self.started.elapsed().as_secs_f32();
                if let Some((light, eye)) = pack.begin_frame(pack_view, pack_extent, seconds)
                    && let Err(e) = terrain.record_shadow_cull(
                        &self.ctx,
                        &self.memory,
                        &mut self.bindless,
                        &self.pipelines,
                        &mut graph,
                        slot,
                        self.frame,
                        light,
                        eye,
                    )
                {
                    error!("native renderer: shadow cull: {}", e.0);
                }
                let geometry = terrain.pack_geometry();
                let set = self.bindless.set;
                // With FXAA (the game's only anti-aliasing under a pack) the
                // pack draws into the post pass's scene, which FXAA reads.
                let mut target = (image, view);
                if self.post.fxaa {
                    if self.pack_post.is_none() {
                        match super::post::PostPass::new(&self.ctx, &self.pipelines, swapchain.format.format) {
                            Ok(p) => self.pack_post = Some(p),
                            Err(e) => error!("native renderer: pack fxaa: {}", e.0),
                        }
                    }
                    if let Some(post) = &mut self.pack_post {
                        match post.scene(&self.ctx, &self.memory, &mut self.bindless, &mut graph, extent, self.frame) {
                            Ok(scene) => target = scene,
                            Err(e) => error!("native renderer: pack fxaa: {}", e.0),
                        }
                    }
                }
                pack.record(
                    &self.ctx,
                    &self.memory,
                    &mut self.bindless,
                    &self.pipelines,
                    &mut graph,
                    (target.0, target.1, extent),
                    pack_view,
                    self.frame,
                    slot,
                    seconds,
                    Some(&geometry),
                    Some(&mut |graph: &mut super::graph::Graph, o: super::pack::screen::Overlay| {
                        terrain.record_pack_overlays(&self.ctx, set, &self.pipelines, graph, o, slot);
                    }),
                );
                if target.0 != image
                    && let Some(post) = &self.pack_post
                {
                    post.record(&self.ctx, &self.bindless, &self.pipelines, &mut graph, (image, view), extent, 0, true);
                }
                // The screen overlays over the pack's frame, as without one.
                terrain.record_overlays(&self.ctx, &self.bindless, &self.pipelines, &mut graph, (image, view), extent, rotation, slot);
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
        let timed = Timed::new(10);
        let gui_result = self.gui.record(
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
        );
        drop(timed);
        if let Err(e) = gui_result {
            error!("native renderer: gui: {}", e.0);
        }
        // A screenshot: the finished frame copied back.
        if !self.shots.is_empty() {
            match self.memory.buffer(
                &self.ctx,
                "screenshot",
                extent.width as u64 * extent.height as u64 * 4,
                vk::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuToCpu,
            ) {
                Ok(buffer) => {
                    let (device, target) = (&self.ctx.device, buffer.raw);
                    graph.pass("screenshot", &[(image, Use::TransferSrc)], &[], |cmd| unsafe {
                        // SAFETY: the image is in TRANSFER_SRC; the buffer
                        // is sized for it.
                        device.cmd_copy_image_to_buffer(
                            cmd,
                            image,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            target,
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
                    });
                    let format = swapchain.format.format;
                    self.readbacks.push((buffer, extent, format, std::mem::take(&mut self.shots), self.frame));
                }
                Err(e) => error!("native renderer: screenshot: {}", e.0),
            }
        }
        graph.pass("present", &[(image, Use::Present)], &[], |_| {});
        drop(graph);
        let timed = Timed::new(11);
        let ended = self.frames.end(&self.ctx, swapchain, recording);
        drop(timed);
        match ended {
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
    // Packs read raw vertices, not the compact quad records.
    let mut o = super::terrain::Options {
        packed: false,
        ..Default::default()
    };
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
/// Whether the inventory figure is drawn at 2x (the game's option).
static SMOOTH_FIGURE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

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
        // Smoothed: drawn at twice the size; the GUI's linear sample
        // averages it down (at 1x the same sample is a plain copy).
        let scale = if SMOOTH_FIGURE.load(std::sync::atomic::Ordering::Relaxed) { 2 } else { 1 };
        let extent = vk::Extent2D { width: extent.width * scale, height: extent.height * scale };
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

        scene.prepare(ctx, memory, bindless, graph, frame, slot);
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
