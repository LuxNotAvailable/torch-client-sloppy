//! The window's swapchain: its images, their views, and remaking it when
//! the window changes size or the surface reports it out of date.

use std::sync::atomic::{AtomicU8, Ordering};

use ash::vk;
use bevy::math::Mat4;

use super::{Context, SetupError};

/// How a frame is turned for the display (pre-rotation): `clip` goes after
/// the projection, and `aspect` is the shape the player sees, which with a
/// quarter turn is the image's turned on its side.
#[derive(Clone, Copy, Debug)]
pub struct PreRotation {
    pub clip: Mat4,
    pub aspect: f32,
}

impl PreRotation {
    /// No turn, for an image of `extent`.
    pub fn none(extent: vk::Extent2D) -> PreRotation {
        PreRotation {
            clip: Mat4::IDENTITY,
            aspect: extent.width as f32 / extent.height.max(1) as f32,
        }
    }
}

pub struct Swapchain {
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub format: vk::SurfaceFormatKHR,
    pub extent: vk::Extent2D,
    pub present_mode: vk::PresentModeKHR,
    /// The rotation the display needs, which the renderer applies itself
    /// (pre-rotation) instead of leaving it to the compositor.
    pub transform: vk::SurfaceTransformFlagsKHR,
    /// The surface's reported extent, before any swap for rotation.
    pub surface_extent: vk::Extent2D,
    /// Whether the display's native orientation is portrait (learnt once,
    /// carried to the swapchains that replace this one).
    pub native_portrait: bool,
    /// One per image: signalled when that image's frame finishes drawing,
    /// waited on by its present. Per image rather than per frame in flight,
    /// since an image can be re-acquired before its last present is done.
    pub render_finished: Vec<vk::Semaphore>,
}

impl Swapchain {
    /// Makes a swapchain for the window's current size, or `None` while it
    /// has none (minimised). `vsync` asks for FIFO; otherwise MAILBOX, else
    /// IMMEDIATE.
    pub fn new(
        ctx: &Context,
        size: (u32, u32),
        vsync: bool,
        old: Option<&Swapchain>,
    ) -> Result<Option<Swapchain>, SetupError> {
        // SAFETY: plain queries on the context's live surface and device.
        let (caps, formats, modes) = unsafe {
            (
                ctx.surface_fn
                    .get_physical_device_surface_capabilities(ctx.physical, ctx.surface)?,
                ctx.surface_fn
                    .get_physical_device_surface_formats(ctx.physical, ctx.surface)?,
                ctx.surface_fn
                    .get_physical_device_surface_present_modes(ctx.physical, ctx.surface)?,
            )
        };
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D {
                width: size
                    .0
                    .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: size
                    .1
                    .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        };
        if extent.width == 0 || extent.height == 0 {
            return Ok(None);
        }
        // With a quarter turn, the image takes the display's native shape
        // (Android's pre-rotation: the renderer turns the picture, not the
        // compositor). The surface's reported extent can't be trusted for
        // that shape mid-turn: the Pixel reported landscape with ROTATE_90
        // but portrait just after switching to ROTATE_270. So the native
        // shape is learnt once and kept across rebuilds: from an unturned
        // surface directly, or on a first quarter-turned one from the
        // extent on its side.
        let quarter = caps.current_transform.intersects(
            vk::SurfaceTransformFlagsKHR::ROTATE_90 | vk::SurfaceTransformFlagsKHR::ROTATE_270,
        );
        let native_portrait = match (quarter, old, NATIVE.load(Ordering::Relaxed)) {
            (false, _, _) => extent.height > extent.width,
            (true, Some(old), _) => old.native_portrait,
            // Remembered past a pause, which drops the swapchain.
            (true, None, NATIVE_PORTRAIT) => true,
            (true, None, NATIVE_LANDSCAPE) => false,
            (true, None, _) => extent.width > extent.height,
        };
        NATIVE.store(
            if native_portrait {
                NATIVE_PORTRAIT
            } else {
                NATIVE_LANDSCAPE
            },
            Ordering::Relaxed,
        );
        let extent = if quarter {
            let (short, long) = (
                extent.width.min(extent.height),
                extent.width.max(extent.height),
            );
            if native_portrait {
                vk::Extent2D {
                    width: short,
                    height: long,
                }
            } else {
                vk::Extent2D {
                    width: long,
                    height: short,
                }
            }
        } else {
            extent
        };
        let format = formats
            .iter()
            .copied()
            // Any 8-bit sRGB format: the shaders write linear colour and the
            // image encodes it. Desktops offer BGRA, Android RGBA; falling
            // back to a UNORM format drew everything too dark.
            .find(|f| {
                matches!(
                    f.format,
                    vk::Format::B8G8R8A8_SRGB
                        | vk::Format::R8G8B8A8_SRGB
                        | vk::Format::A8B8G8R8_SRGB_PACK32
                ) && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .or_else(|| formats.first().copied())
            .ok_or_else(|| SetupError("the window offers no image format".to_owned()))?;
        let present_mode = if vsync {
            vk::PresentModeKHR::FIFO
        } else {
            // Immediate first, as wgpu's no-vsync mode: mailbox on Windows
            // laptops (Optimus, composited) is held to the display's rate.
            [vk::PresentModeKHR::IMMEDIATE, vk::PresentModeKHR::MAILBOX]
                .into_iter()
                .find(|m| modes.contains(m))
                .unwrap_or(vk::PresentModeKHR::FIFO)
        };
        let mut count = caps.min_image_count + 1;
        if caps.max_image_count > 0 {
            count = count.min(caps.max_image_count);
        }
        let create = vk::SwapchainCreateInfoKHR::default()
            .surface(ctx.surface)
            .min_image_count(count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(
                vk::ImageUsageFlags::COLOR_ATTACHMENT
                    | vk::ImageUsageFlags::TRANSFER_DST
                    | vk::ImageUsageFlags::TRANSFER_SRC,
            )
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(old.map_or(vk::SwapchainKHR::null(), |o| o.handle));
        // SAFETY: the create info refers to the context's surface; the old
        // swapchain, if any, is retired by this call and destroyed after.
        let handle = unsafe { ctx.swapchain_fn.create_swapchain(&create, None) }?;
        // SAFETY: a plain query on the swapchain just made.
        let images = unsafe { ctx.swapchain_fn.get_swapchain_images(handle) }?;
        let views = images
            .iter()
            .map(|&image| {
                let info = vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format.format)
                    .subresource_range(color_range());
                // SAFETY: a view of a live swapchain image.
                unsafe { ctx.device.create_image_view(&info, None) }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let render_finished = images
            .iter()
            .map(|_| {
                // SAFETY: a plain semaphore.
                unsafe {
                    ctx.device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Swapchain {
            handle,
            images,
            views,
            format,
            extent,
            present_mode,
            transform: caps.current_transform,
            surface_extent: caps.current_extent,
            native_portrait,
            render_finished,
        }))
    }

    /// Destroys the swapchain. The device must be idle.
    pub fn destroy(self, ctx: &Context) {
        // SAFETY: the caller idles the device, so nothing uses these.
        unsafe {
            for semaphore in self.render_finished {
                ctx.device.destroy_semaphore(semaphore, None);
            }
            for view in self.views {
                ctx.device.destroy_image_view(view, None);
            }
            ctx.swapchain_fn.destroy_swapchain(self.handle, None);
        }
    }
}

impl Swapchain {
    /// The turn this swapchain's frames need. The image keeps the display's
    /// native orientation, so the picture is turned the other way from the
    /// transform the display will apply.
    pub fn pre_rotation(&self) -> PreRotation {
        use std::f32::consts::FRAC_PI_2;
        let angle = match self.transform {
            vk::SurfaceTransformFlagsKHR::ROTATE_90 => FRAC_PI_2,
            vk::SurfaceTransformFlagsKHR::ROTATE_180 => 2.0 * FRAC_PI_2,
            vk::SurfaceTransformFlagsKHR::ROTATE_270 => 3.0 * FRAC_PI_2,
            _ => 0.0,
        };
        let quarter = self.transform.intersects(
            vk::SurfaceTransformFlagsKHR::ROTATE_90 | vk::SurfaceTransformFlagsKHR::ROTATE_270,
        );
        let (w, h) = (self.extent.width as f32, self.extent.height.max(1) as f32);
        PreRotation {
            clip: Mat4::from_rotation_z(ROTATION_SIGN * angle),
            aspect: if quarter { h / w.max(1.0) } else { w / h },
        }
    }
}

/// The display's native orientation once learnt (there is one display).
static NATIVE: AtomicU8 = AtomicU8::new(0);
const NATIVE_PORTRAIT: u8 = 1;
const NATIVE_LANDSCAPE: u8 = 2;

/// Which way `pre_rotation` turns, checked against the Pixel's screen
/// (`adb exec-out screencap`).
const ROTATION_SIGN: f32 = -1.0;

pub fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}
