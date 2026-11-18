//! What the renderer needs from a device, checked before it's chosen, so an
//! unsuitable GPU is refused at startup with the missing pieces named rather
//! than failing later.

use ash::vk;

/// The Vulkan version with every feature in core. Older devices (1.1 and
/// up, as on Android) take them from extensions (`Path::Extensions`).
pub const DESKTOP_API: u32 = vk::API_VERSION_1_3;
/// The lowest version the extension path accepts.
pub const FLOOR_API: u32 = vk::API_VERSION_1_1;

/// Device extensions the renderer always enables.
pub fn device_extensions() -> Vec<&'static std::ffi::CStr> {
    vec![ash::khr::swapchain::NAME]
}

/// Where the 1.2 and 1.3 features come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// Vulkan 1.3 core.
    Core,
    /// Vulkan 1.1 plus extensions.
    Extensions,
}

/// The extensions the extension path needs, with their dependencies.
pub const PATH_EXTENSIONS: [&std::ffi::CStr; 10] = [
    ash::khr::dynamic_rendering::NAME,
    ash::khr::depth_stencil_resolve::NAME,
    ash::khr::create_renderpass2::NAME,
    ash::khr::synchronization2::NAME,
    ash::ext::descriptor_indexing::NAME,
    ash::khr::maintenance3::NAME,
    ash::khr::timeline_semaphore::NAME,
    ash::ext::scalar_block_layout::NAME,
    ash::ext::host_query_reset::NAME,
    ash::khr::shader_draw_parameters::NAME,
];

/// The path a device takes: core at 1.3, unless `MC_VK_EXTENSION_PATH=1`
/// forces extensions (to test the Android path on the desktop).
pub fn path_for(api: u32) -> Path {
    let forced = std::env::var("MC_VK_EXTENSION_PATH").is_ok_and(|v| v == "1");
    if api >= DESKTOP_API && !forced {
        Path::Core
    } else {
        Path::Extensions
    }
}

/// The device's extension names.
pub fn extensions_of(
    instance: &ash::Instance,
    device: vk::PhysicalDevice,
) -> Vec<std::ffi::CString> {
    // SAFETY: a plain query on an enumerated device.
    unsafe { instance.enumerate_device_extension_properties(device) }
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e.extension_name_as_c_str().ok().map(std::ffi::CStr::to_owned))
        .collect()
}

/// Whether draws take their count from a buffer (`drawIndirectCount`).
/// Without it each list is drawn at full capacity, unused commands zeroed.
/// `MC_VK_NO_INDIRECT_COUNT=1` forces that fallback.
pub fn indirect_count_usable(offered: bool) -> bool {
    offered && !std::env::var("MC_VK_NO_INDIRECT_COUNT").is_ok_and(|v| v == "1")
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
                .independent_blend(true)
                // Vertex shaders read the bindless storage buffers, which
                // are declared writable for compute.
                .vertex_pipeline_stores_and_atomics(true)
                // And so do fragment shaders (the debug views).
                .fragment_stores_and_atomics(true),
            v11: vk::PhysicalDeviceVulkan11Features::default().shader_draw_parameters(true),
            v12: vk::PhysicalDeviceVulkan12Features::default()
                .descriptor_indexing(true)
                .runtime_descriptor_array(true)
                .descriptor_binding_partially_bound(true)
                .descriptor_binding_sampled_image_update_after_bind(true)
                .descriptor_binding_storage_buffer_update_after_bind(true)
                .descriptor_binding_variable_descriptor_count(true)
                .shader_sampled_image_array_non_uniform_indexing(true)
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
    pub fn read(instance: &ash::Instance, device: vk::PhysicalDevice, path: Path) -> Offered {
        match path {
            Path::Core => Self::read_core(instance, device),
            Path::Extensions => Self::read_extensions(instance, device),
        }
    }

    /// The features read through the extensions' own structures, laid out
    /// as the core ones so `missing` reads either path alike.
    fn read_extensions(instance: &ash::Instance, device: vk::PhysicalDevice) -> Offered {
        let names = extensions_of(instance, device);
        let has = |name: &std::ffi::CStr| names.iter().any(|e| e.as_c_str() == name);
        let mut ext = ExtFeatures::default();
        let core = {
            let mut features =
                vk::PhysicalDeviceFeatures2::default().push_next(&mut ext.draw_parameters);
            if has(ash::ext::descriptor_indexing::NAME) {
                features = features.push_next(&mut ext.indexing);
            }
            if has(ash::khr::timeline_semaphore::NAME) {
                features = features.push_next(&mut ext.timeline);
            }
            if has(ash::ext::scalar_block_layout::NAME) {
                features = features.push_next(&mut ext.scalar);
            }
            if has(ash::ext::host_query_reset::NAME) {
                features = features.push_next(&mut ext.query_reset);
            }
            if has(ash::khr::dynamic_rendering::NAME) {
                features = features.push_next(&mut ext.dynamic_rendering);
            }
            if has(ash::khr::synchronization2::NAME) {
                features = features.push_next(&mut ext.synchronization2);
            }
            // SAFETY: a query into structures we own, each chained only
            // where the device has its extension.
            unsafe { instance.get_physical_device_features2(device, &mut features) };
            features.features
        };
        let mut offered = ext.as_core(core);
        let bit = |b: bool| if b { vk::TRUE } else { vk::FALSE };
        // No feature structure: the extension present means usable.
        offered.v12.descriptor_indexing = bit(
            has(ash::ext::descriptor_indexing::NAME) && has(ash::khr::maintenance3::NAME),
        );
        offered.v12.draw_indirect_count = bit(has(ash::khr::draw_indirect_count::NAME));
        if !(has(ash::khr::depth_stencil_resolve::NAME) && has(ash::khr::create_renderpass2::NAME))
        {
            offered.v13.dynamic_rendering = vk::FALSE;
        }
        offered.subgroup = read_subgroup(instance, device);
        offered
    }

    fn read_core(instance: &ash::Instance, device: vk::PhysicalDevice) -> Offered {
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
        // The query is done; nothing keeps pointers into the chain.
        v11.p_next = std::ptr::null_mut();
        v12.p_next = std::ptr::null_mut();
        v13.p_next = std::ptr::null_mut();
        Offered {
            core,
            v11,
            v12,
            v13,
            subgroup: read_subgroup(instance, device),
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
            r.vertex_pipeline_stores_and_atomics,
            o.vertex_pipeline_stores_and_atomics,
            "vertexPipelineStoresAndAtomics",
        );
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
            r.descriptor_binding_storage_buffer_update_after_bind,
            o.descriptor_binding_storage_buffer_update_after_bind,
            "descriptorBindingStorageBufferUpdateAfterBind",
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

fn read_subgroup(
    instance: &ash::Instance,
    device: vk::PhysicalDevice,
) -> vk::PhysicalDeviceSubgroupProperties<'static> {
    let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
    {
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut subgroup);
        // SAFETY: a query into a structure we own.
        unsafe { instance.get_physical_device_properties2(device, &mut properties) };
    }
    subgroup.p_next = std::ptr::null_mut();
    subgroup
}

/// The extension path's feature structures, chained into device creation
/// in place of the 1.1 to 1.3 ones.
#[derive(Default)]
pub struct ExtFeatures {
    pub draw_parameters: vk::PhysicalDeviceShaderDrawParametersFeatures<'static>,
    pub indexing: vk::PhysicalDeviceDescriptorIndexingFeatures<'static>,
    pub timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures<'static>,
    pub scalar: vk::PhysicalDeviceScalarBlockLayoutFeatures<'static>,
    pub query_reset: vk::PhysicalDeviceHostQueryResetFeatures<'static>,
    pub dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures<'static>,
    pub synchronization2: vk::PhysicalDeviceSynchronization2Features<'static>,
}

impl ExtFeatures {
    /// The features `f`'s core structures ask for.
    pub fn from_core(f: &Features) -> ExtFeatures {
        let (v12, v13) = (&f.v12, &f.v13);
        ExtFeatures {
            draw_parameters: vk::PhysicalDeviceShaderDrawParametersFeatures {
                shader_draw_parameters: f.v11.shader_draw_parameters,
                ..Default::default()
            },
            indexing: vk::PhysicalDeviceDescriptorIndexingFeatures {
                runtime_descriptor_array: v12.runtime_descriptor_array,
                descriptor_binding_partially_bound: v12.descriptor_binding_partially_bound,
                descriptor_binding_sampled_image_update_after_bind: v12
                    .descriptor_binding_sampled_image_update_after_bind,
                descriptor_binding_storage_buffer_update_after_bind: v12
                    .descriptor_binding_storage_buffer_update_after_bind,
                descriptor_binding_variable_descriptor_count: v12
                    .descriptor_binding_variable_descriptor_count,
                shader_sampled_image_array_non_uniform_indexing: v12
                    .shader_sampled_image_array_non_uniform_indexing,
                ..Default::default()
            },
            timeline: vk::PhysicalDeviceTimelineSemaphoreFeatures {
                timeline_semaphore: v12.timeline_semaphore,
                ..Default::default()
            },
            scalar: vk::PhysicalDeviceScalarBlockLayoutFeatures {
                scalar_block_layout: v12.scalar_block_layout,
                ..Default::default()
            },
            query_reset: vk::PhysicalDeviceHostQueryResetFeatures {
                host_query_reset: v12.host_query_reset,
                ..Default::default()
            },
            dynamic_rendering: vk::PhysicalDeviceDynamicRenderingFeatures {
                dynamic_rendering: v13.dynamic_rendering,
                ..Default::default()
            },
            synchronization2: vk::PhysicalDeviceSynchronization2Features {
                synchronization2: v13.synchronization2,
                ..Default::default()
            },
        }
    }

    /// Laid out as the core structures, for `Offered::missing`.
    fn as_core(&self, core: vk::PhysicalDeviceFeatures) -> Offered {
        let i = &self.indexing;
        Offered {
            core,
            v11: vk::PhysicalDeviceVulkan11Features {
                shader_draw_parameters: self.draw_parameters.shader_draw_parameters,
                ..Default::default()
            },
            v12: vk::PhysicalDeviceVulkan12Features {
                runtime_descriptor_array: i.runtime_descriptor_array,
                descriptor_binding_partially_bound: i.descriptor_binding_partially_bound,
                descriptor_binding_sampled_image_update_after_bind: i
                    .descriptor_binding_sampled_image_update_after_bind,
                descriptor_binding_storage_buffer_update_after_bind: i
                    .descriptor_binding_storage_buffer_update_after_bind,
                descriptor_binding_variable_descriptor_count: i
                    .descriptor_binding_variable_descriptor_count,
                shader_sampled_image_array_non_uniform_indexing: i
                    .shader_sampled_image_array_non_uniform_indexing,
                timeline_semaphore: self.timeline.timeline_semaphore,
                scalar_block_layout: self.scalar.scalar_block_layout,
                host_query_reset: self.query_reset.host_query_reset,
                ..Default::default()
            },
            v13: vk::PhysicalDeviceVulkan13Features {
                dynamic_rendering: self.dynamic_rendering.dynamic_rendering,
                synchronization2: self.synchronization2.synchronization2,
                ..Default::default()
            },
            subgroup: Default::default(),
        }
    }
}
