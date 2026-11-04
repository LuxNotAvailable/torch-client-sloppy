//! The window's swapchain: its images, their views, and remaking it when
//! the window changes size or the surface reports it out of date.

use ash::vk;

use super::{Context, SetupError};

pub struct Swapchain {
    pub handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub format: vk::SurfaceFormatKHR,
    pub extent: vk::Extent2D,
    pub present_mode: vk::PresentModeKHR,
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
        let format = formats
            .iter()
            .copied()
            .find(|f| {
                f.format == vk::Format::B8G8R8A8_SRGB
                    && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
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

pub fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}
