//! What the renderer needs from a device, checked before it's chosen, so an
//! unsuitable GPU is refused at startup with the missing pieces named rather
//! than failing later.

use ash::vk;

/// The lowest Vulkan version on the desktop. Android may go lower, with the
/// same features from extensions (checklist phase 9).
pub const DESKTOP_API: u32 = vk::API_VERSION_1_3;

/// Device extensions the renderer always enables.
pub fn device_extensions() -> Vec<&'static std::ffi::CStr> {
    vec![ash::khr::swapchain::NAME]
}

/// The features the renderer turns on, each named for the refusal message.
pub struct Features {
    pub core: vk::PhysicalDeviceFeatures,
    pub v11: vk::PhysicalDeviceVulkan11Features<'static>,
    pub v12: vk::PhysicalDeviceVulkan12Features<'static>,
    pub v13: vk::PhysicalDeviceVulkan13Features<'static>,
}

impl Features {
    /// The set the renderer requires and enables.
    pub fn required() -> Features {
        Features {
            core: vk::PhysicalDeviceFeatures::default()
                .multi_draw_indirect(true)
                .draw_indirect_first_instance(true)
                .sampler_anisotropy(true)
                .fill_mode_non_solid(true)
                .independent_blend(true),
            v11: vk::PhysicalDeviceVulkan11Features::default().shader_draw_parameters(true),
            v12: vk::PhysicalDeviceVulkan12Features::default()
                .descriptor_indexing(true)
                .runtime_descriptor_array(true)
                .descriptor_binding_partially_bound(true)
                .descriptor_binding_sampled_image_update_after_bind(true)
                .descriptor_binding_variable_descriptor_count(true)
                .shader_sampled_image_array_non_uniform_indexing(true)
                .draw_indirect_count(true)
                .timeline_semaphore(true)
                .scalar_block_layout(true)
                .host_query_reset(true),
            v13: vk::PhysicalDeviceVulkan13Features::default()
                .dynamic_rendering(true)
                .synchronization2(true),
        }
    }
}

/// What a device offers, read once.
pub struct Offered {
    pub core: vk::PhysicalDeviceFeatures,
    pub v11: vk::PhysicalDeviceVulkan11Features<'static>,
    pub v12: vk::PhysicalDeviceVulkan12Features<'static>,
    pub v13: vk::PhysicalDeviceVulkan13Features<'static>,
    pub subgroup: vk::PhysicalDeviceSubgroupProperties<'static>,
}

impl Offered {
    pub fn read(instance: &ash::Instance, device: vk::PhysicalDevice) -> Offered {
        let mut v11 = vk::PhysicalDeviceVulkan11Features::default();
        let mut v12 = vk::PhysicalDeviceVulkan12Features::default();
        let mut v13 = vk::PhysicalDeviceVulkan13Features::default();
        let core = {
            let mut features = vk::PhysicalDeviceFeatures2::default()
                .push_next(&mut v11)
                .push_next(&mut v12)
                .push_next(&mut v13);
            // SAFETY: a query into structures we own, on a device the
            // instance enumerated.
            unsafe { instance.get_physical_device_features2(device, &mut features) };
            features.features
        };
        let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
        {
            let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut subgroup);
            // SAFETY: as above.
            unsafe { instance.get_physical_device_properties2(device, &mut properties) };
        }
        // The queries are done; nothing keeps pointers into the chain.
        v11.p_next = std::ptr::null_mut();
        v12.p_next = std::ptr::null_mut();
        v13.p_next = std::ptr::null_mut();
        subgroup.p_next = std::ptr::null_mut();
        Offered {
            core,
            v11,
            v12,
            v13,
            subgroup,
        }
    }

    /// The required features and properties this device lacks, by name.
    pub fn missing(&self, required: &Features) -> Vec<&'static str> {
        let mut missing = Vec::new();
        let mut need = |wanted: vk::Bool32, has: vk::Bool32, name: &'static str| {
            if wanted == vk::TRUE && has != vk::TRUE {
                missing.push(name);
            }
        };
        let (r, o) = (&required.core, &self.core);
        need(
            r.multi_draw_indirect,
            o.multi_draw_indirect,
            "multiDrawIndirect",
        );
        need(
            r.draw_indirect_first_instance,
            o.draw_indirect_first_instance,
            "drawIndirectFirstInstance",
        );
        need(
            r.sampler_anisotropy,
            o.sampler_anisotropy,
            "samplerAnisotropy",
        );
        need(
            r.fill_mode_non_solid,
            o.fill_mode_non_solid,
            "fillModeNonSolid",
        );
        need(r.independent_blend, o.independent_blend, "independentBlend");
        need(
            required.v11.shader_draw_parameters,
            self.v11.shader_draw_parameters,
            "shaderDrawParameters",
        );
        let (r, o) = (&required.v12, &self.v12);
        need(
            r.descriptor_indexing,
            o.descriptor_indexing,
            "descriptorIndexing",
        );
        need(
            r.runtime_descriptor_array,
            o.runtime_descriptor_array,
            "runtimeDescriptorArray",
        );
        need(
            r.descriptor_binding_partially_bound,
            o.descriptor_binding_partially_bound,
            "descriptorBindingPartiallyBound",
        );
        need(
            r.descriptor_binding_sampled_image_update_after_bind,
            o.descriptor_binding_sampled_image_update_after_bind,
            "descriptorBindingSampledImageUpdateAfterBind",
        );
        need(
            r.descriptor_binding_variable_descriptor_count,
            o.descriptor_binding_variable_descriptor_count,
            "descriptorBindingVariableDescriptorCount",
        );
        need(
            r.shader_sampled_image_array_non_uniform_indexing,
            o.shader_sampled_image_array_non_uniform_indexing,
            "shaderSampledImageArrayNonUniformIndexing",
        );
        need(
            r.draw_indirect_count,
            o.draw_indirect_count,
            "drawIndirectCount",
        );
        need(
            r.timeline_semaphore,
            o.timeline_semaphore,
            "timelineSemaphore",
        );
        need(
            r.scalar_block_layout,
            o.scalar_block_layout,
            "scalarBlockLayout",
        );
        need(r.host_query_reset, o.host_query_reset, "hostQueryReset");
        need(
            required.v13.dynamic_rendering,
            self.v13.dynamic_rendering,
            "dynamicRendering",
        );
        need(
            required.v13.synchronization2,
            self.v13.synchronization2,
            "synchronization2",
        );
        let wanted_ops = vk::SubgroupFeatureFlags::BASIC
            | vk::SubgroupFeatureFlags::BALLOT
            | vk::SubgroupFeatureFlags::ARITHMETIC;
        if !self.subgroup.supported_operations.contains(wanted_ops)
            || !self
                .subgroup
                .supported_stages
                .contains(vk::ShaderStageFlags::COMPUTE)
        {
            missing.push("subgroup basic, ballot and arithmetic operations in compute");
        }
        missing
    }
}
