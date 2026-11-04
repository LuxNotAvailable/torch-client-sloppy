//! A development viewer for the native Vulkan renderer
//! (VULKAN_REWRITE_CHECKLIST.md): it runs the new renderer on its own, in a
//! window of its own, so each piece can be checked before the game uses it.
//!
//! `vkview [--frames N] [--device NAME] [--vsync] [--size WxH] [--stress]
//!         [--view shaded|normals|texture|depth] [--screenshot FILE.png]
//!         [--terrain DIR [--camera X,Y,Z,YAW,PITCH] [--no-hiz] [--no-vis]
//!          [--no-buckets] [--budget MB] [--hiz-level N] [--greedy]]`
//!
//! The scene is the test scene, or with `--terrain` a terrain dump the game
//! wrote (`MC_DUMP_TERRAIN=DIR`), seen from `--camera` or, without one,
//! from above, turning. In the terrain, `--view texture` colours each
//! section apart and `--view hiz` shows a level of the occlusion pyramid;
//! the `--no-*` flags turn a culling stage off, for comparing. The dump
//! streams in under the upload budget, and `--frames` starts counting once
//! it has. `--sheet FILE.png [--sheet-tile WxH] [--sheet-cols N]
//! [--sheet-every N]` gathers every Nth measured frame, shrunk to a tile,
//! into one contact sheet, and diffs each tile against the one before:
//! with a still `--camera`, any change between frames is a flicker, listed
//! by frame and drawn amplified in FILE-diff.png. `--step S` moves the
//! scene's clock S seconds a frame, so two runs (say, with and without a
//! culling stage) give the same frames to compare. `--greedy` only measures how many of the dump's quads greedy
//! meshing would merge (3.9), and exits.
//!
//! With `--frames`, it draws that many frames, prints a summary (device,
//! frame times, GPU time per pass, barriers, memory, validation-layer errors
//! and warnings) and exits, failing if the validation layer reported any
//! error. `--screenshot` saves the last frame. `--stress` resizes,
//! minimises and restores the window during the run, to exercise swapchain
//! recreation. Set `MC_VK_VALIDATION=1` for the layer in release builds.

use std::time::Instant;

use ash::vk as v;
use gpu_allocator::MemoryLocation;
use torch_client::vk::bindless::Bindless;
use torch_client::vk::graph::{Graph, Use};
use torch_client::vk::memory::{Buffer, Memory};
use torch_client::vk::pipeline::Pipelines;
use torch_client::vk::terrain::{self, Camera, TerrainScene};
use torch_client::vk::test_scene::{TestScene, View};
use torch_client::vk::timestamps::Timestamps;
use torch_client::vk::upload::Uploads;
use torch_client::vk::{self, Context, Frames, Swapchain, frame};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowId};

struct Options {
    frames: Option<u64>,
    device: Option<String>,
    vsync: bool,
    size: (u32, u32),
    stress: bool,
    view: View,
    screenshot: Option<String>,
    terrain: Option<std::path::PathBuf>,
    camera: Option<Camera>,
    terrain_options: terrain::Options,
    greedy: bool,
    sheet: Option<String>,
    sheet_tile: (u32, u32),
    sheet_cols: u32,
    sheet_every: u64,
    step: Option<f32>,
}

fn options() -> Options {
    let mut options = Options {
        frames: None,
        device: None,
        vsync: false,
        size: (1280, 720),
        stress: false,
        view: View::Shaded,
        screenshot: None,
        terrain: None,
        camera: None,
        terrain_options: terrain::Options::default(),
        greedy: false,
        sheet: None,
        sheet_tile: (160, 90),
        sheet_cols: 16,
        sheet_every: 1,
        step: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => options.frames = args.next().and_then(|v| v.parse().ok()),
            "--device" => options.device = args.next(),
            "--vsync" => options.vsync = true,
            "--stress" => options.stress = true,
            "--screenshot" => options.screenshot = args.next(),
            "--terrain" => options.terrain = args.next().map(Into::into),
            "--greedy" => options.greedy = true,
            "--sheet" => options.sheet = args.next(),
            "--step" => options.step = args.next().and_then(|v| v.parse().ok()),
            "--sheet-tile" => {
                if let Some((w, h)) = args.next().as_deref().and_then(|v| v.split_once('x')) {
                    options.sheet_tile = (w.parse().unwrap_or(160), h.parse().unwrap_or(90));
                }
            }
            "--sheet-cols" => {
                options.sheet_cols = args.next().and_then(|v| v.parse().ok()).unwrap_or(16)
            }
            "--sheet-every" => {
                options.sheet_every = args.next().and_then(|v| v.parse().ok()).unwrap_or(1)
            }
            "--no-hiz" => options.terrain_options.hiz = false,
            "--no-vis" => options.terrain_options.visibility = false,
            "--no-buckets" => options.terrain_options.buckets = false,
            "--no-wave" => options.terrain_options.wave = false,
            "--budget" => {
                if let Some(mb) = args.next().and_then(|v| v.parse::<f64>().ok()) {
                    options.terrain_options.budget = (mb * 1048576.0) as u64;
                }
            }
            "--hiz-level" => {
                if let Some(level) = args.next().and_then(|v| v.parse().ok()) {
                    options.terrain_options.hiz_level = level;
                }
            }
            "--camera" => {
                let text = args.next().unwrap_or_default();
                options.camera = Camera::parse(&text);
                if options.camera.is_none() {
                    eprintln!("vkview: --camera wants X,Y,Z,YAW,PITCH, not {text}");
                }
            }
            "--view" => {
                let name = args.next().unwrap_or_default();
                options.view = View::parse(&name).unwrap_or_else(|| {
                    eprintln!("vkview: unknown view {name}");
                    View::Shaded
                });
            }
            "--size" => {
                if let Some((w, h)) = args.next().as_deref().and_then(|v| v.split_once('x')) {
                    options.size = (w.parse().unwrap_or(1280), h.parse().unwrap_or(720));
                }
            }
            other => eprintln!("vkview: unknown argument {other}"),
        }
    }
    options
}

/// What the viewer draws.
enum Scene {
    Test(TestScene),
    Terrain(TerrainScene, Option<Camera>),
}

/// The `--sheet` contact sheet: a shrunk copy of each sampled frame, in a
/// host-visible buffer of `cols` x `rows` tiles.
struct Sheet {
    tile_image: torch_client::vk::memory::Image,
    buffer: Buffer,
    tile: (u32, u32),
    cols: u32,
    rows: u32,
    filled: u32,
    format: v::Format,
}

/// A frame's image copied back for `--screenshot`.
struct Readback {
    buffer: Buffer,
    extent: v::Extent2D,
    format: v::Format,
}

struct Renderer {
    // Everything made from the device is destroyed in `Drop`, before the
    // context.
    scene: Option<Scene>,
    timestamps: Option<Timestamps>,
    uploads: Option<Uploads>,
    pipelines: Option<Pipelines>,
    bindless: Option<Bindless>,
    readback: Option<Readback>,
    sheet: Option<Sheet>,
    memory: Option<Memory>,
    swapchain: Option<Swapchain>,
    frames: Option<Frames>,
    ctx: Context,
    window: Window,
    stale: bool,
    frame: u64,
    started: Instant,
    barriers: u32,
    /// `--step`: scene seconds per frame instead of the clock, so runs
    /// repeat frame for frame.
    step: Option<f32>,
}

impl Renderer {
    fn new(window: Window, options: &Options) -> Result<Renderer, vk::SetupError> {
        let display = window
            .display_handle()
            .map_err(|e| vk::SetupError(format!("no display handle: {e}")))?
            .as_raw();
        let handle = window
            .window_handle()
            .map_err(|e| vk::SetupError(format!("no window handle: {e}")))?
            .as_raw();
        let ctx = Context::new(display, handle, options.device.as_deref())?;
        let info = &ctx.info;
        println!(
            "vkview: {} ({:?}), Vulkan {}.{}.{}, driver {}, subgroup {}",
            info.name,
            info.kind,
            info.api.0,
            info.api.1,
            info.api.2,
            info.driver,
            info.subgroup_size
        );
        let size = window.inner_size();
        let swapchain = Swapchain::new(&ctx, (size.width, size.height), options.vsync, None)?;
        if let Some(s) = &swapchain {
            println!("vkview: present mode {:?}", s.present_mode);
        }
        let frames = Frames::new(&ctx)?;
        let memory = Memory::new(&ctx)?;
        let mut bindless = Bindless::new(&ctx)?;
        let cache_path = std::env::temp_dir().join("torch-vkview-pipelines.bin");
        let cache_had = cache_path.exists();
        let compile = Instant::now();
        let pipelines = Pipelines::new(&ctx, bindless.layout, Some(cache_path))?;
        let uploads = Uploads::new(&ctx, &memory, 16 << 20)?;
        let timestamps = Timestamps::new(&ctx)?;
        let format = swapchain
            .as_ref()
            .map_or(v::Format::B8G8R8A8_SRGB, |s| s.format.format);
        let scene = match &options.terrain {
            Some(dir) => Scene::Terrain(
                TerrainScene::new(
                    &ctx,
                    &memory,
                    &mut bindless,
                    &pipelines,
                    format,
                    dir,
                    options.terrain_options,
                )?,
                options.camera,
            ),
            None => Scene::Test(TestScene::new(
                &ctx,
                &memory,
                &mut bindless,
                &pipelines,
                format,
            )?),
        };
        println!(
            "vkview: pipelines ready in {:.1} ms ({} cache)",
            compile.elapsed().as_secs_f32() * 1000.0,
            if cache_had { "warm" } else { "cold" }
        );
        Ok(Renderer {
            scene: Some(scene),
            timestamps: Some(timestamps),
            uploads: Some(uploads),
            pipelines: Some(pipelines),
            bindless: Some(bindless),
            readback: None,
            sheet: None,
            memory: Some(memory),
            swapchain,
            frames: Some(frames),
            ctx,
            window,
            stale: false,
            frame: 0,
            started: Instant::now(),
            barriers: 0,
            step: options.step,
        })
    }

    fn recreate(&mut self, vsync: bool) -> Result<(), vk::SetupError> {
        self.ctx.idle();
        let size = self.window.inner_size();
        let new = Swapchain::new(
            &self.ctx,
            (size.width, size.height),
            vsync,
            self.swapchain.as_ref(),
        )?;
        if let Some(old) = std::mem::replace(&mut self.swapchain, new) {
            old.destroy(&self.ctx);
        }
        self.stale = false;
        Ok(())
    }

    /// Draws one frame; false when nothing was drawn (minimised, or the
    /// swapchain had to be remade). `capture` copies the frame back for a
    /// screenshot.
    fn draw(&mut self, vsync: bool, view: View, capture: bool, sample: bool) -> bool {
        if self.stale && self.recreate(vsync).is_err() {
            return false;
        }
        let (Some(swapchain), Some(frames)) = (&self.swapchain, &mut self.frames) else {
            return false;
        };
        let slot = frames.index;
        let recording = match frames.begin(&self.ctx, swapchain) {
            Ok(r) => r,
            Err(frame::FrameError::OutOfDate) => {
                self.stale = true;
                return false;
            }
            Err(frame::FrameError::Failed(e)) => {
                eprintln!("vkview: frame failed: {e:?}");
                return false;
            }
        };
        let (
            Some(memory),
            Some(bindless),
            Some(pipelines),
            Some(uploads),
            Some(timestamps),
            Some(scene),
        ) = (
            &self.memory,
            &mut self.bindless,
            &self.pipelines,
            &mut self.uploads,
            &mut self.timestamps,
            &mut self.scene,
        )
        else {
            return false;
        };
        bindless.recycle(self.frame);
        uploads.begin(slot);
        timestamps.begin_frame(&self.ctx, slot);

        let image = swapchain.images[recording.image_index as usize];
        let image_view = swapchain.views[recording.image_index as usize];
        let extent = swapchain.extent;
        let mut graph = Graph::new(&self.ctx, recording.commands, Some(timestamps));
        graph.image(image, v::ImageAspectFlags::COLOR, v::ImageLayout::UNDEFINED);
        let seconds = match self.step {
            Some(step) => self.frame as f32 * step,
            None => self.started.elapsed().as_secs_f32(),
        };
        let recorded = match scene {
            Scene::Test(scene) => scene.record(
                &self.ctx,
                memory,
                bindless,
                pipelines,
                uploads,
                &mut graph,
                (image, image_view),
                extent,
                self.frame,
                seconds,
                view,
            ),
            Scene::Terrain(scene, camera) => {
                let camera = camera.unwrap_or_else(|| scene.default_camera(seconds));
                scene.record(
                    &self.ctx,
                    memory,
                    bindless,
                    pipelines,
                    uploads,
                    &mut graph,
                    (image, image_view),
                    extent,
                    self.frame,
                    slot,
                    camera,
                    view,
                )
            }
        };
        if let Err(e) = recorded {
            eprintln!("vkview: scene failed: {e}");
        }
        if capture && self.readback.is_none() {
            let bytes = extent.width as u64 * extent.height as u64 * 4;
            if let Ok(buffer) = memory.buffer(
                &self.ctx,
                "screenshot",
                bytes,
                v::BufferUsageFlags::TRANSFER_DST,
                MemoryLocation::GpuToCpu,
            ) {
                let device = &self.ctx.device;
                let target = buffer.raw;
                graph.pass(
                    "screenshot",
                    &[(image, Use::TransferSrc)],
                    &[],
                    |cmd| unsafe {
                        // SAFETY: the image is in TRANSFER_SRC; the buffer is
                        // sized for it.
                        device.cmd_copy_image_to_buffer(
                            cmd,
                            image,
                            v::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            target,
                            &[v::BufferImageCopy {
                                buffer_offset: 0,
                                buffer_row_length: 0,
                                buffer_image_height: 0,
                                image_subresource: v::ImageSubresourceLayers {
                                    aspect_mask: v::ImageAspectFlags::COLOR,
                                    mip_level: 0,
                                    base_array_layer: 0,
                                    layer_count: 1,
                                },
                                image_offset: v::Offset3D::default(),
                                image_extent: v::Extent3D {
                                    width: extent.width,
                                    height: extent.height,
                                    depth: 1,
                                },
                            }],
                        );
                    },
                );
                self.readback = Some(Readback {
                    buffer,
                    extent,
                    format: swapchain.format.format,
                });
            }
        }
        if let Some(sheet) = &mut self.sheet
            && sample
            && sheet.filled < sheet.cols * sheet.rows
        {
            let device = &self.ctx.device;
            let (tw, th) = sheet.tile;
            let tile = sheet.tile_image.raw;
            graph.image(tile, v::ImageAspectFlags::COLOR, v::ImageLayout::UNDEFINED);
            let layers = v::ImageSubresourceLayers {
                aspect_mask: v::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };
            graph.pass(
                "sheet blit",
                &[(image, Use::TransferSrc), (tile, Use::TransferDst)],
                &[],
                |cmd| unsafe {
                    // SAFETY: both images are in the transfer layouts the
                    // graph set; the swapchain allows transfer reads.
                    device.cmd_blit_image(
                        cmd,
                        image,
                        v::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        tile,
                        v::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[v::ImageBlit {
                            src_subresource: layers,
                            src_offsets: [
                                v::Offset3D::default(),
                                v::Offset3D {
                                    x: extent.width as i32,
                                    y: extent.height as i32,
                                    z: 1,
                                },
                            ],
                            dst_subresource: layers,
                            dst_offsets: [
                                v::Offset3D::default(),
                                v::Offset3D {
                                    x: tw as i32,
                                    y: th as i32,
                                    z: 1,
                                },
                            ],
                        }],
                        v::Filter::LINEAR,
                    );
                },
            );
            let (col, row) = (sheet.filled % sheet.cols, sheet.filled / sheet.cols);
            let width = sheet.cols * tw;
            let offset = ((row * th) as u64 * width as u64 + (col * tw) as u64) * 4;
            let target = sheet.buffer.raw;
            graph.pass(
                "sheet copy",
                &[(tile, Use::TransferSrc)],
                &[(target, Use::TransferDst)],
                |cmd| unsafe {
                    // SAFETY: the tile's region of the sheet, which no other
                    // frame writes.
                    device.cmd_copy_image_to_buffer(
                        cmd,
                        tile,
                        v::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        target,
                        &[v::BufferImageCopy {
                            buffer_offset: offset,
                            buffer_row_length: width,
                            buffer_image_height: 0,
                            image_subresource: layers,
                            image_offset: v::Offset3D::default(),
                            image_extent: v::Extent3D {
                                width: tw,
                                height: th,
                                depth: 1,
                            },
                        }],
                    );
                },
            );
            sheet.filled += 1;
        }
        graph.pass("present", &[(image, Use::Present)], &[], |_| {});
        self.barriers = graph.barriers;
        self.frame += 1;
        match frames.end(&self.ctx, swapchain, recording) {
            Ok(()) => true,
            Err(frame::FrameError::OutOfDate) => {
                self.stale = true;
                true
            }
            Err(frame::FrameError::Failed(e)) => {
                eprintln!("vkview: present failed: {e:?}");
                false
            }
        }
    }

    /// Makes the contact sheet for `samples` frames.
    fn make_sheet(&mut self, options: &Options, samples: u64) -> Result<(), vk::SetupError> {
        let (Some(memory), Some(swapchain)) = (&self.memory, &self.swapchain) else {
            return Ok(());
        };
        let cols = options.sheet_cols.max(1);
        let rows = (samples.div_ceil(cols as u64) as u32).clamp(1, 128);
        let (tw, th) = options.sheet_tile;
        let format = swapchain.format.format;
        let tile_image = memory.image(
            &self.ctx,
            "sheet tile",
            v::Extent2D {
                width: tw,
                height: th,
            },
            format,
            // Sampled only because every image gets a view.
            v::ImageUsageFlags::TRANSFER_DST
                | v::ImageUsageFlags::TRANSFER_SRC
                | v::ImageUsageFlags::SAMPLED,
            v::ImageAspectFlags::COLOR,
        )?;
        let buffer = memory.buffer(
            &self.ctx,
            "sheet",
            (cols * tw) as u64 * (rows * th) as u64 * 4,
            v::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuToCpu,
        )?;
        self.sheet = Some(Sheet {
            tile_image,
            buffer,
            tile: (tw, th),
            cols,
            rows,
            filled: 0,
            format,
        });
        Ok(())
    }

    /// Writes the contact sheet to `path`, diffs each tile against the one
    /// before, and writes the amplified differences beside it.
    fn save_sheet(&mut self, path: &str) {
        let Some(mut sheet) = self.sheet.take() else {
            return;
        };
        self.ctx.idle();
        let (tw, th) = sheet.tile;
        let (w, h) = (sheet.cols * tw, sheet.rows * th);
        let bgra = matches!(
            sheet.format,
            v::Format::B8G8R8A8_SRGB | v::Format::B8G8R8A8_UNORM
        );
        let mut rgba = sheet
            .buffer
            .mapped()
            .map(|b| b[..(w * h * 4) as usize].to_vec())
            .unwrap_or_default();
        for px in rgba.chunks_exact_mut(4) {
            if bgra {
                px.swap(0, 2);
            }
            px[3] = 255;
        }
        let cols = sheet.cols;
        let at = |t: u32, x: u32, y: u32| {
            let (c, r) = (t % cols, t / cols);
            (((r * th + y) * w + c * tw + x) * 4) as usize
        };
        let mut diff = vec![0u8; rgba.len()];
        let mut flagged = Vec::new();
        let mut worst = (0.0f32, 0u32);
        for t in 1..sheet.filled {
            let (mut changed, mut sum) = (0u32, 0u64);
            for y in 0..th {
                for x in 0..tw {
                    let (a, b) = (at(t - 1, x, y), at(t, x, y));
                    let d = (0..3)
                        .map(|c| rgba[a + c].abs_diff(rgba[b + c]))
                        .max()
                        .unwrap_or(0);
                    sum += d as u64;
                    if d > 24 {
                        changed += 1;
                    }
                    let shown = d.saturating_mul(4);
                    diff[b..b + 3].copy_from_slice(&[shown, shown, shown]);
                    diff[b + 3] = 255;
                }
            }
            let share = changed as f32 / (tw * th) as f32;
            if share > worst.0 {
                worst = (share, t);
            }
            if share > 0.002 {
                flagged.push((t, share, sum as f32 / (tw * th) as f32));
            }
        }
        let filled = sheet.filled;
        let save = |file: &str, bytes: &[u8]| match image::save_buffer(
            file,
            bytes,
            w,
            h,
            image::ColorType::Rgba8,
        ) {
            Ok(()) => println!("vkview: sheet {file} ({filled} tiles of {tw}x{th})"),
            Err(e) => eprintln!("vkview: sheet failed: {e}"),
        };
        save(path, &rgba);
        save(&path.replace(".png", "-diff.png"), &diff);
        println!(
            "vkview: sheet: {} of {} frame-to-frame changes over 0.2% of the tile; worst {:.2}% at tile {}",
            flagged.len(),
            filled.saturating_sub(1),
            worst.0 * 100.0,
            worst.1
        );
        for (t, share, mean) in flagged.iter().take(40) {
            println!(
                "vkview:   tile {t}: {:.2}% changed, mean {mean:.1}",
                share * 100.0
            );
        }
        if let Some(memory) = &self.memory {
            memory.free_buffer(&self.ctx, sheet.buffer);
            memory.free_image(&self.ctx, sheet.tile_image);
        }
    }

    /// Writes the captured frame to `path` as a PNG.
    fn save_screenshot(&mut self, path: &str) {
        let Some(mut readback) = self.readback.take() else {
            eprintln!("vkview: no frame was captured");
            return;
        };
        self.ctx.idle();
        let (w, h) = (readback.extent.width, readback.extent.height);
        if let Some(bytes) = readback.buffer.mapped() {
            let bgra = matches!(
                readback.format,
                v::Format::B8G8R8A8_SRGB | v::Format::B8G8R8A8_UNORM
            );
            let mut rgba = bytes[..(w * h * 4) as usize].to_vec();
            if bgra {
                for px in rgba.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
            }
            match image::save_buffer(path, &rgba, w, h, image::ColorType::Rgba8) {
                Ok(()) => println!("vkview: screenshot {path} ({w}x{h})"),
                Err(e) => eprintln!("vkview: screenshot failed: {e}"),
            }
        }
        if let Some(memory) = &self.memory {
            memory.free_buffer(&self.ctx, readback.buffer);
        }
    }

    /// Whether the scene is ready to be measured: the terrain has streamed
    /// in.
    fn ready(&self) -> bool {
        match &self.scene {
            Some(Scene::Terrain(scene, _)) => scene.loaded(),
            _ => true,
        }
    }

    fn summary(&self) {
        if let Some(Scene::Terrain(scene, _)) = &self.scene {
            let s = &scene.stats;
            println!(
                "vkview: terrain {} section layers in {} pools, {} vertices, {} indices; {} records, {:.1} MiB uploaded",
                s.sections,
                s.pools,
                s.vertices,
                s.indices,
                s.records,
                s.uploaded as f32 / 1048576.0
            );
            println!(
                "vkview: terrain draws {} solid, {} cutout commands, {} translucent; flood fill reached {} layers; Hi-Z on {} frames",
                s.commands[0], s.commands[1], s.commands[2], s.reached, s.hiz_frames
            );
            println!(
                "vkview: terrain buffers grew: translucent indices {} times, slots {} times",
                s.grown[0], s.grown[1]
            );
            if let (Some(v), Some(p)) = (s.vertex_invocations, s.primitives) {
                println!("vkview: terrain {v} vertex invocations, {p} primitives clipped");
            }
        }
        if let Some(t) = &self.timestamps {
            let passes: Vec<String> = t
                .last
                .iter()
                .map(|(name, ms)| format!("{name} {ms:.3}"))
                .collect();
            println!("vkview: GPU ms per pass: {}", passes.join(", "));
        }
        if let Some(memory) = &self.memory {
            let (used, reserved) = memory.usage();
            println!(
                "vkview: {} barriers a frame, GPU memory {:.1} MiB used of {:.1} MiB reserved",
                self.barriers,
                used as f32 / 1048576.0,
                reserved as f32 / 1048576.0
            );
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.ctx.idle();
        let memory = self.memory.take();
        match (self.scene.take(), &memory) {
            (Some(Scene::Test(scene)), Some(memory)) => scene.destroy(&self.ctx, memory),
            (Some(Scene::Terrain(scene, _)), Some(memory)) => scene.destroy(&self.ctx, memory),
            _ => {}
        }
        if let (Some(uploads), Some(memory)) = (self.uploads.take(), &memory) {
            uploads.destroy(&self.ctx, memory);
        }
        if let (Some(sheet), Some(memory)) = (self.sheet.take(), &memory) {
            memory.free_buffer(&self.ctx, sheet.buffer);
            memory.free_image(&self.ctx, sheet.tile_image);
        }
        if let (Some(readback), Some(memory)) = (self.readback.take(), &memory) {
            memory.free_buffer(&self.ctx, readback.buffer);
        }
        drop(memory);
        if let Some(t) = self.timestamps.take() {
            t.destroy(&self.ctx);
        }
        if let Some(p) = self.pipelines.take() {
            p.destroy(&self.ctx);
        }
        if let Some(b) = self.bindless.take() {
            b.destroy(&self.ctx);
        }
        if let Some(swapchain) = self.swapchain.take() {
            swapchain.destroy(&self.ctx);
        }
        if let Some(frames) = self.frames.take() {
            frames.destroy(&self.ctx);
        }
    }
}

struct App {
    options: Options,
    renderer: Option<Renderer>,
    drawn: u64,
    frame_ms: Vec<f32>,
    last: Option<Instant>,
    failed: Option<String>,
    begun: Option<Instant>,
    stress_done: usize,
    loaded_at: Option<f32>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Torch vkview")
            .with_inner_size(winit::dpi::PhysicalSize::new(
                self.options.size.0,
                self.options.size.1,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(w) => w,
            Err(e) => {
                self.failed = Some(format!("no window: {e}"));
                event_loop.exit();
                return;
            }
        };
        match Renderer::new(window, &self.options) {
            Ok(mut r) => {
                if self.options.sheet.is_some() {
                    let samples =
                        self.options.frames.unwrap_or(256) / self.options.sheet_every.max(1);
                    if let Err(e) = r.make_sheet(&self.options, samples) {
                        eprintln!("vkview: no sheet: {e}");
                    }
                }
                self.renderer = Some(r);
            }
            Err(e) => {
                self.failed = Some(e.0);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                if let Some(r) = &mut self.renderer {
                    r.stale = true;
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(r) = &mut self.renderer else { return };
                let last_frame = self.options.frames.is_some_and(|n| self.drawn + 1 >= n);
                let capture = last_frame && self.options.screenshot.is_some();
                let ready = r.ready();
                let sample = ready && self.drawn % self.options.sheet_every.max(1) == 0;
                if r.draw(
                    self.options.vsync,
                    self.options.view,
                    capture && ready,
                    sample,
                ) {
                    if !ready {
                        return;
                    }
                    if self.loaded_at.is_none() {
                        let at = r.started.elapsed().as_secs_f32();
                        self.loaded_at = Some(at);
                        println!("vkview: scene ready after {at:.1} s");
                    }
                    let now = Instant::now();
                    if let Some(last) = self.last {
                        self.frame_ms.push((now - last).as_secs_f32() * 1000.0);
                    }
                    self.last = Some(now);
                    self.drawn += 1;
                    if self.options.frames.is_some_and(|n| self.drawn >= n) {
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(r) = &self.renderer {
            if self.options.stress {
                let since = self.begun.get_or_insert_with(Instant::now).elapsed();
                self.stress_done =
                    stress_step(&r.window, since.as_millis() as u64, self.stress_done);
            }
            r.window.request_redraw();
        }
    }
}

/// The window changes `--stress` makes, half a second apart, by time so a
/// minimised window (which draws nothing) still gets restored. Returns how
/// many steps are done.
fn stress_step(window: &Window, millis: u64, done: usize) -> usize {
    let due = (millis / 500) as usize;
    if due <= done || done >= 5 {
        return done;
    }
    match done {
        0 => drop(window.request_inner_size(winit::dpi::PhysicalSize::new(800, 600))),
        1 => drop(window.request_inner_size(winit::dpi::PhysicalSize::new(1600, 900))),
        2 => window.set_minimized(true),
        3 => window.set_minimized(false),
        _ => drop(window.request_inner_size(winit::dpi::PhysicalSize::new(1280, 720))),
    }
    done + 1
}

fn main() {
    // Synchronisation validation too, when the layer is on.
    if vk::validation_wanted() && std::env::var_os("VK_KHRONOS_VALIDATION_VALIDATE_SYNC").is_none()
    {
        // SAFETY: set before any other thread starts.
        unsafe { std::env::set_var("VK_KHRONOS_VALIDATION_VALIDATE_SYNC", "true") };
    }
    let options = options();
    if options.greedy {
        let Some(dir) = &options.terrain else {
            eprintln!("vkview: --greedy measures a --terrain dump");
            std::process::exit(2);
        };
        let bytes = std::fs::read(dir.join("sections.bin")).unwrap_or_default();
        match torch_client::vk::terrain_dump::read(&bytes) {
            Ok(sections) => {
                let e = torch_client::vk::greedy::estimate(&sections);
                println!(
                    "vkview: {} opaque quads, {} whole evenly lit faces; greedy meshing leaves {} ({:.1}% fewer)",
                    e.quads,
                    e.candidates,
                    e.after,
                    e.saving() * 100.0
                );
            }
            Err(e) => eprintln!("vkview: {e}"),
        }
        return;
    }
    let event_loop = EventLoop::new().expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        options,
        renderer: None,
        drawn: 0,
        frame_ms: Vec::new(),
        last: None,
        failed: None,
        begun: None,
        stress_done: 0,
        loaded_at: None,
    };
    let _ = event_loop.run_app(&mut app);
    if let Some(r) = &mut app.renderer {
        if let Some(path) = app.options.sheet.clone() {
            r.save_sheet(&path);
        }
        if let Some(path) = app.options.screenshot.clone() {
            r.save_screenshot(&path);
        }
        r.ctx.idle();
        r.summary();
    }
    drop(app.renderer.take());

    if let Some(reason) = &app.failed {
        eprintln!("vkview: {reason}");
        std::process::exit(2);
    }
    let mut sorted = app.frame_ms.clone();
    sorted.sort_by(f32::total_cmp);
    let pick = |q: f32| {
        sorted
            .get(((sorted.len() as f32 - 1.0) * q) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let avg = sorted.iter().sum::<f32>() / sorted.len().max(1) as f32;
    println!(
        "vkview: {} frames, avg {:.2} ms ({:.0} fps), p50 {:.2}, p99 {:.2}, max {:.2}",
        app.drawn,
        avg,
        1000.0 / avg.max(0.001),
        pick(0.5),
        pick(0.99),
        pick(1.0)
    );
    println!(
        "vkview: validation {} errors, {} warnings",
        vk::validation_errors(),
        vk::validation_warnings()
    );
    if vk::validation_errors() > 0 {
        std::process::exit(1);
    }
}
