//! The global bindless descriptor set (VULKAN_REWRITE_CHECKLIST.md 2.3):
//! every sampled image, sampler and storage buffer gets a stable index when
//! registered, and shaders pick resources by index (`shaders/common.slang`).
//! One set, bound once per command buffer, for every pipeline.

use ash::vk;

use super::frame::FRAMES_IN_FLIGHT;
use super::{Context, SetupError};

pub const MAX_TEXTURES: u32 = 16384;
pub const MAX_SAMPLERS: u32 = 16;
pub const MAX_BUFFERS: u32 = 4096;

const BINDING_TEXTURES: u32 = 0;
const BINDING_SAMPLERS: u32 = 1;
const BINDING_BUFFERS: u32 = 2;

/// The sampler slots `common.slang` names.
pub const SAMPLER_NEAREST: u32 = 0;
pub const SAMPLER_LINEAR: u32 = 1;
/// Clamped, for pack targets (`packdraw::targets::Statics`).
pub const SAMPLER_NEAREST_CLAMP: u32 = 2;
pub const SAMPLER_LINEAR_CLAMP: u32 = 3;
/// Shadow comparisons (less or equal), and one that always passes.
pub const SAMPLER_COMPARE_LINEAR: u32 = 4;
pub const SAMPLER_COMPARE_NEAREST: u32 = 5;
pub const SAMPLER_COMPARE_ALWAYS: u32 = 6;
/// Linear across mip levels, clamped.
pub const SAMPLER_MIP_LINEAR: u32 = 7;

pub struct Bindless {
    pub layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    pub set: vk::DescriptorSet,
    textures: Slots,
    buffers: Slots,
    samplers: Vec<vk::Sampler>,
}

/// Index allocation with deferred frees: an index is reused only after the
/// frames that might still read it are done.
struct Slots {
    next: u32,
    max: u32,
    free: Vec<u32>,
    retiring: Vec<(u32, u64)>,
}

impl Slots {
    fn new(max: u32) -> Slots {
        Slots {
            next: 0,
            max,
            free: Vec::new(),
            retiring: Vec::new(),
        }
    }

    fn take(&mut self) -> Option<u32> {
        self.free.pop().or_else(|| {
            (self.next < self.max).then(|| {
                self.next += 1;
                self.next - 1
            })
        })
    }

    fn release(&mut self, index: u32, frame: u64) {
        self.retiring.push((index, frame));
    }

    fn recycle(&mut self, frame: u64) {
        let free = &mut self.free;
        self.retiring.retain(|&(index, at)| {
            let done = frame >= at + FRAMES_IN_FLIGHT as u64;
            if done {
                free.push(index);
            }
            !done
        });
    }
}

impl Bindless {
    pub fn new(ctx: &Context) -> Result<Bindless, SetupError> {
        let all = vk::ShaderStageFlags::ALL;
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_TEXTURES)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(MAX_TEXTURES)
                .stage_flags(all),
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_SAMPLERS)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(MAX_SAMPLERS)
                .stage_flags(all),
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_BUFFERS)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(MAX_BUFFERS)
                .stage_flags(all),
        ];
        let flags = vk::DescriptorBindingFlags::PARTIALLY_BOUND
            | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND;
        let binding_flags = [flags, flags, flags];
        let mut flags_info =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&binding_flags);
        let sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: MAX_TEXTURES,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: MAX_SAMPLERS,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: MAX_BUFFERS,
            },
        ];
        // SAFETY: plain creation on a live device; the infos are locals.
        unsafe {
            let layout = ctx.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .bindings(&bindings)
                    .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
                    .push_next(&mut flags_info),
                None,
            )?;
            let pool = ctx.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )?;
            let layouts = [layout];
            let set = ctx.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )?[0];
            let mut bindless = Bindless {
                layout,
                pool,
                set,
                textures: Slots::new(MAX_TEXTURES),
                buffers: Slots::new(MAX_BUFFERS),
                samplers: Vec::new(),
            };
            for filter in [vk::Filter::NEAREST, vk::Filter::LINEAR] {
                let sampler = ctx.device.create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(filter)
                        .min_filter(filter)
                        .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                        .address_mode_u(vk::SamplerAddressMode::REPEAT)
                        .address_mode_v(vk::SamplerAddressMode::REPEAT)
                        .max_lod(vk::LOD_CLAMP_NONE),
                    None,
                )?;
                bindless.add_sampler(ctx, sampler);
            }
            // The pack samplers, in the order of their constants.
            let clamp = vk::SamplerAddressMode::CLAMP_TO_EDGE;
            let base = |filter: vk::Filter, mip: vk::SamplerMipmapMode| {
                vk::SamplerCreateInfo::default()
                    .mag_filter(filter)
                    .min_filter(filter)
                    .mipmap_mode(mip)
                    .address_mode_u(clamp)
                    .address_mode_v(clamp)
                    .address_mode_w(clamp)
                    .max_lod(vk::LOD_CLAMP_NONE)
            };
            let nearest_mip = vk::SamplerMipmapMode::NEAREST;
            let compare = |filter: vk::Filter, op: vk::CompareOp| {
                base(filter, nearest_mip).compare_enable(true).compare_op(op)
            };
            for info in [
                base(vk::Filter::NEAREST, nearest_mip),
                base(vk::Filter::LINEAR, nearest_mip),
                compare(vk::Filter::LINEAR, vk::CompareOp::LESS_OR_EQUAL),
                compare(vk::Filter::NEAREST, vk::CompareOp::LESS_OR_EQUAL),
                compare(vk::Filter::NEAREST, vk::CompareOp::ALWAYS),
                base(vk::Filter::LINEAR, vk::SamplerMipmapMode::LINEAR),
            ] {
                let sampler = ctx.device.create_sampler(&info, None)?;
                bindless.add_sampler(ctx, sampler);
            }
            Ok(bindless)
        }
    }

    fn add_sampler(&mut self, ctx: &Context, sampler: vk::Sampler) {
        let index = self.samplers.len() as u32;
        self.samplers.push(sampler);
        let info = [vk::DescriptorImageInfo::default().sampler(sampler)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(BINDING_SAMPLERS)
            .dst_array_element(index)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .image_info(&info);
        // SAFETY: update-after-bind slot not yet read by any submitted work.
        unsafe { ctx.device.update_descriptor_sets(&[write], &[]) };
    }

    /// Registers an image view for sampling in `layout`; returns its index.
    pub fn add_texture(
        &mut self,
        ctx: &Context,
        view: vk::ImageView,
        layout: vk::ImageLayout,
    ) -> Option<u32> {
        let index = self.textures.take()?;
        let info = [vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(layout)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(BINDING_TEXTURES)
            .dst_array_element(index)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(&info);
        // SAFETY: a fresh (or fully retired) slot, which no work in flight
        // reads, in an update-after-bind set.
        unsafe { ctx.device.update_descriptor_sets(&[write], &[]) };
        Some(index)
    }

    /// Registers a whole buffer as a storage buffer; returns its index.
    pub fn add_buffer(&mut self, ctx: &Context, buffer: vk::Buffer) -> Option<u32> {
        let index = self.buffers.take()?;
        let info = [vk::DescriptorBufferInfo::default()
            .buffer(buffer)
            .offset(0)
            .range(vk::WHOLE_SIZE)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(BINDING_BUFFERS)
            .dst_array_element(index)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&info);
        // SAFETY: as for textures.
        unsafe { ctx.device.update_descriptor_sets(&[write], &[]) };
        Some(index)
    }

    /// Frees a texture index once frame `frame` and those before it are done.
    pub fn remove_texture(&mut self, index: u32, frame: u64) {
        self.textures.release(index, frame);
    }

    pub fn remove_buffer(&mut self, index: u32, frame: u64) {
        self.buffers.release(index, frame);
    }

    /// Makes retired indices reusable, at the start of frame `frame`.
    pub fn recycle(&mut self, frame: u64) {
        self.textures.recycle(frame);
        self.buffers.recycle(frame);
    }

    pub fn destroy(self, ctx: &Context) {
        // SAFETY: the caller idles the device first.
        unsafe {
            for sampler in self.samplers {
                ctx.device.destroy_sampler(sampler, None);
            }
            ctx.device.destroy_descriptor_pool(self.pool, None);
            ctx.device.destroy_descriptor_set_layout(self.layout, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Slots;

    #[test]
    fn an_index_is_reused_only_after_the_frames_in_flight() {
        let mut slots = Slots::new(4);
        let a = slots.take().unwrap();
        slots.release(a, 10);
        slots.recycle(10);
        assert_ne!(slots.take(), Some(a), "still possibly in flight");
        slots.recycle(10 + super::FRAMES_IN_FLIGHT as u64);
        assert_eq!(slots.take(), Some(a));
    }

    #[test]
    fn slots_run_out_at_the_maximum() {
        let mut slots = Slots::new(2);
        assert!(slots.take().is_some() && slots.take().is_some());
        assert_eq!(slots.take(), None);
    }
}
