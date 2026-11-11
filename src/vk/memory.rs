//! Device memory through `gpu-allocator` (VULKAN_REWRITE_CHECKLIST.md 2.1):
//! buffers and images with their allocations, freed together.

use std::sync::Mutex;

use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{
    Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};

use super::{Context, SetupError};

pub struct Memory {
    allocator: Mutex<Allocator>,
}

/// A buffer and its memory. Host-visible ones stay mapped.
pub struct Buffer {
    pub raw: vk::Buffer,
    pub size: u64,
    allocation: Option<Allocation>,
}

/// An image, a view of all of it, and its memory.
pub struct Image {
    pub raw: vk::Image,
    pub view: vk::ImageView,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub aspect: vk::ImageAspectFlags,
    /// An sRGB image's UNORM view (`image_with_raw`): the texels as
    /// stored, as packs sample them.
    pub raw_view: Option<vk::ImageView>,
    /// Mip levels; `view` shows level 0, `mip_view` all of them.
    pub levels: u32,
    pub mip_view: Option<vk::ImageView>,
    allocation: Option<Allocation>,
}

impl Memory {
    pub fn new(ctx: &Context) -> Result<Memory, SetupError> {
        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: ctx.instance.clone(),
            device: ctx.device.clone(),
            physical_device: ctx.physical,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })
        .map_err(|e| SetupError(format!("no memory allocator: {e}")))?;
        Ok(Memory {
            allocator: Mutex::new(allocator),
        })
    }

    pub fn buffer(
        &self,
        ctx: &Context,
        name: &str,
        size: u64,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
    ) -> Result<Buffer, SetupError> {
        let info = vk::BufferCreateInfo::default()
            .size(size.max(4))
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: plain creation on a live device; the memory is bound
        // before the buffer is returned.
        unsafe {
            let raw = ctx.device.create_buffer(&info, None)?;
            let requirements = ctx.device.get_buffer_memory_requirements(raw);
            let allocation = self.allocate(name, requirements, location, true)?;
            ctx.device
                .bind_buffer_memory(raw, allocation.memory(), allocation.offset())?;
            Ok(Buffer {
                raw,
                size,
                allocation: Some(allocation),
            })
        }
    }

    pub fn image(
        &self,
        ctx: &Context,
        name: &str,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<Image, SetupError> {
        self.image_inner(ctx, name, extent, format, usage, aspect, None, 1)
    }

    /// A colour image with a full mip chain (`mip_view` sees it all).
    pub fn image_mipmapped(
        &self,
        ctx: &Context,
        name: &str,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
    ) -> Result<Image, SetupError> {
        let levels = 32 - extent.width.max(extent.height).max(1).leading_zeros();
        self.image_inner(ctx, name, extent, format, usage, vk::ImageAspectFlags::COLOR, None, levels)
    }

    /// An `R8G8B8A8_SRGB` image that also has a UNORM view (`raw_view`).
    pub fn image_with_raw(
        &self,
        ctx: &Context,
        name: &str,
        extent: vk::Extent2D,
        usage: vk::ImageUsageFlags,
    ) -> Result<Image, SetupError> {
        self.image_inner(
            ctx,
            name,
            extent,
            vk::Format::R8G8B8A8_SRGB,
            usage,
            vk::ImageAspectFlags::COLOR,
            Some(vk::Format::R8G8B8A8_UNORM),
            1,
        )
    }

    #[allow(clippy::too_many_arguments, reason = "an image's whole description")]
    fn image_inner(
        &self,
        ctx: &Context,
        name: &str,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
        raw_format: Option<vk::Format>,
        levels: u32,
    ) -> Result<Image, SetupError> {
        let flags = if raw_format.is_some() {
            vk::ImageCreateFlags::MUTABLE_FORMAT
        } else {
            vk::ImageCreateFlags::empty()
        };
        let info = vk::ImageCreateInfo::default()
            .flags(flags)
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1,
            })
            .mip_levels(levels)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        // SAFETY: as for buffers; the view covers the whole image.
        unsafe {
            let raw = ctx.device.create_image(&info, None)?;
            let requirements = ctx.device.get_image_memory_requirements(raw);
            let allocation = self.allocate(name, requirements, MemoryLocation::GpuOnly, false)?;
            ctx.device
                .bind_image_memory(raw, allocation.memory(), allocation.offset())?;
            let view = ctx.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(raw)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(range(aspect)),
                None,
            )?;
            let raw_view = match raw_format {
                Some(f) => Some(ctx.device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(raw)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(f)
                        .subresource_range(range(aspect)),
                    None,
                )?),
                None => None,
            };
            let mip_view = if levels > 1 {
                Some(ctx.device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(raw)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(vk::ImageSubresourceRange {
                            level_count: levels,
                            ..range(aspect)
                        }),
                    None,
                )?)
            } else {
                None
            };
            Ok(Image {
                raw,
                view,
                format,
                extent,
                aspect,
                raw_view,
                levels,
                mip_view,
                allocation: Some(allocation),
            })
        }
    }

    fn allocate(
        &self,
        name: &str,
        requirements: vk::MemoryRequirements,
        location: MemoryLocation,
        linear: bool,
    ) -> Result<Allocation, SetupError> {
        self.allocator
            .lock()
            .map_err(|_| SetupError("the allocator is poisoned".to_owned()))?
            .allocate(&AllocationCreateDesc {
                name,
                requirements,
                location,
                linear,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| SetupError(format!("out of GPU memory for {name}: {e}")))
    }

    /// Frees a buffer. Nothing in flight may use it.
    pub fn free_buffer(&self, ctx: &Context, mut buffer: Buffer) {
        // SAFETY: the caller guarantees the GPU is done with it.
        unsafe { ctx.device.destroy_buffer(buffer.raw, None) };
        if let (Some(a), Ok(mut allocator)) = (buffer.allocation.take(), self.allocator.lock()) {
            let _ = allocator.free(a);
        }
    }

    /// Frees an image. Nothing in flight may use it.
    pub fn free_image(&self, ctx: &Context, mut image: Image) {
        // SAFETY: as above.
        unsafe {
            ctx.device.destroy_image_view(image.view, None);
            for v in [image.raw_view, image.mip_view].into_iter().flatten() {
                ctx.device.destroy_image_view(v, None);
            }
            ctx.device.destroy_image(image.raw, None);
        }
        if let (Some(a), Ok(mut allocator)) = (image.allocation.take(), self.allocator.lock()) {
            let _ = allocator.free(a);
        }
    }

    /// Bytes allocated and reserved, for the performance report.
    pub fn usage(&self) -> (u64, u64) {
        self.allocator.lock().map_or((0, 0), |a| {
            let report = a.generate_report();
            (report.total_allocated_bytes, report.total_reserved_bytes)
        })
    }
}

impl Buffer {
    /// The mapped bytes of a host-visible buffer.
    pub fn mapped(&mut self) -> Option<&mut [u8]> {
        self.allocation
            .as_mut()
            .and_then(Allocation::mapped_slice_mut)
    }

    /// Writes `bytes` at `offset` into a host-visible buffer.
    pub fn write(&mut self, offset: usize, bytes: &[u8]) {
        if let Some(slice) = self.mapped() {
            slice[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
    }
}

pub fn range(aspect: vk::ImageAspectFlags) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: aspect,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}
