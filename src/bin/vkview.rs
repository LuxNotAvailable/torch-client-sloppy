//! A development viewer for the native Vulkan renderer
//! (VULKAN_REWRITE_CHECKLIST.md): it runs the new renderer on its own, in a
//! window of its own, so each piece can be checked before the game uses it.
//!
//! `vkview [--frames N] [--device NAME] [--vsync] [--size WxH] [--stress]`
//!
//! `--stress` resizes, minimises and restores the window during the run, to
//! exercise swapchain recreation under the validation layer.
//!
//! With `--frames`, it draws that many frames, prints a summary (device,
//! frame times, validation-layer errors and warnings) and exits, failing if
//! the validation layer reported any error. Set `MC_VK_VALIDATION=1` for the
//! layer in release builds.

use std::time::Instant;

use torch_client::vk::{self, Context, Frames, Swapchain, frame};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowId};

/// Vanilla's daytime sky colour, as the first thing to clear to.
const SKY: [f32; 4] = [0.47, 0.65, 1.0, 1.0];

struct Options {
    frames: Option<u64>,
    device: Option<String>,
    vsync: bool,
    size: (u32, u32),
    stress: bool,
}

fn options() -> Options {
    let mut options = Options {
        frames: None,
        device: None,
        vsync: false,
        size: (1280, 720),
        stress: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => options.frames = args.next().and_then(|v| v.parse().ok()),
            "--device" => options.device = args.next(),
            "--vsync" => options.vsync = true,
            "--stress" => options.stress = true,
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

struct Renderer {
    // Dropped in this order: the swapchain and frames before the context.
    swapchain: Option<Swapchain>,
    frames: Option<Frames>,
    ctx: Context,
    window: Window,
    stale: bool,
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
        let frames = Frames::new(&ctx)?;
        Ok(Renderer {
            swapchain,
            frames: Some(frames),
            ctx,
            window,
            stale: false,
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
    /// swapchain had to be remade).
    fn draw(&mut self, vsync: bool) -> bool {
        if self.stale && self.recreate(vsync).is_err() {
            return false;
        }
        let (Some(swapchain), Some(frames)) = (&self.swapchain, &mut self.frames) else {
            return false;
        };
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
        let device = &self.ctx.device;
        let image = swapchain.images[recording.image_index as usize];
        let view = swapchain.views[recording.image_index as usize];
        use ash::vk as v;
        frame::transition(
            device,
            recording.commands,
            image,
            (
                v::ImageLayout::UNDEFINED,
                v::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                v::AccessFlags2::NONE,
            ),
            (
                v::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                v::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                v::AccessFlags2::COLOR_ATTACHMENT_WRITE,
            ),
        );
        let attachments = [v::RenderingAttachmentInfo::default()
            .image_view(view)
            .image_layout(v::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(v::AttachmentLoadOp::CLEAR)
            .store_op(v::AttachmentStoreOp::STORE)
            .clear_value(v::ClearValue {
                color: v::ClearColorValue { float32: SKY },
            })];
        let rendering = v::RenderingInfo::default()
            .render_area(v::Rect2D {
                offset: v::Offset2D::default(),
                extent: swapchain.extent,
            })
            .layer_count(1)
            .color_attachments(&attachments);
        // SAFETY: recording into this frame's command buffer, to an image
        // just moved to the attachment layout.
        unsafe {
            device.cmd_begin_rendering(recording.commands, &rendering);
            device.cmd_end_rendering(recording.commands);
        }
        frame::transition(
            device,
            recording.commands,
            image,
            (
                v::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                v::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                v::AccessFlags2::COLOR_ATTACHMENT_WRITE,
            ),
            (
                v::ImageLayout::PRESENT_SRC_KHR,
                v::PipelineStageFlags2::BOTTOM_OF_PIPE,
                v::AccessFlags2::NONE,
            ),
        );
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
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.ctx.idle();
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
    started: Option<Instant>,
    frame_ms: Vec<f32>,
    last: Option<Instant>,
    failed: Option<String>,
    begun: Option<Instant>,
    stress_done: usize,
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
            Ok(r) => self.renderer = Some(r),
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
                if r.draw(self.options.vsync) {
                    let now = Instant::now();
                    if let Some(last) = self.last {
                        self.frame_ms.push((now - last).as_secs_f32() * 1000.0);
                    }
                    self.last = Some(now);
                    self.started.get_or_insert(now);
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
    let event_loop = EventLoop::new().expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        options,
        renderer: None,
        drawn: 0,
        started: None,
        frame_ms: Vec::new(),
        last: None,
        failed: None,
        begun: None,
        stress_done: 0,
    };
    let _ = event_loop.run_app(&mut app);
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
