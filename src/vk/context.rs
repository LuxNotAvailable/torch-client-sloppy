//! The Vulkan instance, the chosen device and its queue, and the window's
//! surface.

use std::ffi::{CStr, c_void};

use ash::vk;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::SetupError;
use super::requirements::{
    DESKTOP_API, ExtFeatures, FLOOR_API, Features, Offered, PATH_EXTENSIONS, Path,
    device_extensions, extensions_of, indirect_count_usable, path_for,
};

/// What the device is, for logs and the performance report.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub kind: vk::PhysicalDeviceType,
    pub api: (u32, u32, u32),
    pub driver: String,
    pub vendor_id: u32,
    pub subgroup_size: u32,
    /// Whether pipeline statistics queries are on (optional; the viewer
    /// counts vertex invocations with them).
    pub pipeline_statistics: bool,
    /// Where the 1.2 and 1.3 features come from.
    pub path: Path,
    /// Whether draws take their count from a buffer; without it, lists are
    /// drawn at full capacity with their unused commands zeroed.
    pub indirect_count: bool,
}

pub struct Context {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub surface_fn: ash::khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
    pub swapchain_fn: ash::khr::swapchain::Device,
    pub info: DeviceInfo,
}

impl Context {
    /// Sets Vulkan up for a window, or with `None` headless (no surface;
    /// the viewer draws offscreen, as on Android over adb). `prefer` picks a
    /// device whose name contains it (case-insensitive), as an override;
    /// otherwise a discrete GPU is preferred.
    pub fn new(
        window: Option<(RawDisplayHandle, RawWindowHandle)>,
        prefer: Option<&str>,
    ) -> Result<Context, SetupError> {
        // SAFETY: loads the system's Vulkan loader; nothing else runs yet.
        let entry = unsafe { ash::Entry::load() }
            .map_err(|e| SetupError(format!("no Vulkan loader on this system ({e})")))?;

        let validation = super::validation_wanted();
        let layer = c"VK_LAYER_KHRONOS_validation";
        // SAFETY: a plain query of the loader.
        let layers_present = unsafe { entry.enumerate_instance_layer_properties() }?
            .iter()
            .any(|l| l.layer_name_as_c_str() == Ok(layer));
        let validation = validation && layers_present;
        if super::validation_wanted() && !layers_present {
            eprintln!("vk: validation asked for, but the Khronos layer isn't installed");
        }

        let mut extensions = match window {
            Some((display, _)) => ash_window::enumerate_required_extensions(display)?.to_vec(),
            None => Vec::new(),
        };
        if validation {
            extensions.push(ash::ext::debug_utils::NAME.as_ptr());
        }
        let layers = if validation {
            vec![layer.as_ptr()]
        } else {
            vec![]
        };
        let app = vk::ApplicationInfo::default()
            .application_name(c"Torch Client")
            .engine_name(c"torch-vk")
            .api_version(instance_api(&entry));
        let create = vk::InstanceCreateInfo::default()
            .application_info(&app)
            .enabled_extension_names(&extensions)
            .enabled_layer_names(&layers);
        // SAFETY: the create info points at locals that outlive the call.
        let instance = unsafe { entry.create_instance(&create, None) }?;

        let debug = validation
            .then(|| {
                let loader = ash::ext::debug_utils::Instance::new(&entry, &instance);
                let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                    .message_severity(
                        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                            | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                    )
                    .message_type(
                        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                    )
                    .pfn_user_callback(Some(debug_callback));
                // SAFETY: the callback is a plain function with no state.
                unsafe { loader.create_debug_utils_messenger(&info, None) }
                    .ok()
                    .map(|messenger| (loader, messenger))
            })
            .flatten();

        // SAFETY: the handles come from a live window that outlives the
        // context (the caller drops the context first).
        let surface = match window {
            Some((display, window)) => {
                unsafe { ash_window::create_surface(&entry, &instance, display, window, None) }?
            }
            None => vk::SurfaceKHR::null(),
        };
        let surface_fn = ash::khr::surface::Instance::new(&entry, &instance);

        let (physical, queue_family, info) =
            choose_device(&instance, &surface_fn, surface, prefer)?;

        let priorities = [1.0f32];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&priorities)];
        let mut features = Features::required();
        features.core = features
            .core
            .pipeline_statistics_query(info.pipeline_statistics);
        // Headless, there is nothing to present to.
        let extension_names: Vec<*const std::ffi::c_char> = if surface == vk::SurfaceKHR::null() {
            Vec::new()
        } else {
            device_extensions().iter().map(|e| e.as_ptr()).collect()
        };
        let mut extension_names = extension_names;
        if info.path == Path::Extensions {
            extension_names.extend(PATH_EXTENSIONS.iter().map(|e| e.as_ptr()));
            if info.indirect_count {
                extension_names.push(ash::khr::draw_indirect_count::NAME.as_ptr());
            }
        }
        features.v12.draw_indirect_count = if info.indirect_count {
            vk::TRUE
        } else {
            vk::FALSE
        };
        let mut ext = ExtFeatures::from_core(&features);
        let mut features2 = vk::PhysicalDeviceFeatures2::default().features(features.core);
        features2 = match info.path {
            Path::Core => features2
                .push_next(&mut features.v11)
                .push_next(&mut features.v12)
                .push_next(&mut features.v13),
            Path::Extensions => features2
                .push_next(&mut ext.draw_parameters)
                .push_next(&mut ext.indexing)
                .push_next(&mut ext.timeline)
                .push_next(&mut ext.scalar)
                .push_next(&mut ext.query_reset)
                .push_next(&mut ext.dynamic_rendering)
                .push_next(&mut ext.synchronization2),
        };
        let create = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&extension_names)
            .push_next(&mut features2);
        // SAFETY: the create info and its feature chain are locals that
        // outlive the call; the device was checked to offer every feature.
        let device = unsafe { instance.create_device(physical, &create, None) }?;
        let device = match info.path {
            Path::Core => device,
            // SAFETY: the device was just made with the extensions enabled.
            Path::Extensions => unsafe { load_aliased(&instance, device.handle()) },
        };
        // SAFETY: the family was created with one queue.
        let queue = unsafe { device.get_device_queue(queue_family, 0) };
        let swapchain_fn = ash::khr::swapchain::Device::new(&instance, &device);

        Ok(Context {
            entry,
            instance,
            debug,
            surface_fn,
            surface,
            physical,
            device,
            queue,
            queue_family,
            swapchain_fn,
            info,
        })
    }

    /// An indexed indirect draw counted by `count`, or without
    /// `drawIndirectCount` all `max` commands (the unused ones zeroed by
    /// `zero_unused`).
    ///
    /// # Safety
    /// As `vkCmdDrawIndexedIndirectCount`, on a recording `cmd`.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn draw_indexed_indirect_count(
        &self,
        cmd: vk::CommandBuffer,
        commands: vk::Buffer,
        offset: u64,
        count: vk::Buffer,
        count_offset: u64,
        max: u32,
        stride: u32,
    ) {
        // SAFETY: the caller's.
        unsafe {
            if self.info.indirect_count {
                self.device.cmd_draw_indexed_indirect_count(
                    cmd,
                    commands,
                    offset,
                    count,
                    count_offset,
                    max,
                    stride,
                );
            } else {
                self.device
                    .cmd_draw_indexed_indirect(cmd, commands, offset, max, stride);
            }
        }
    }

    /// The non-indexed `draw_indexed_indirect_count`.
    ///
    /// # Safety
    /// As `vkCmdDrawIndirectCount`, on a recording `cmd`.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn draw_indirect_count(
        &self,
        cmd: vk::CommandBuffer,
        commands: vk::Buffer,
        offset: u64,
        count: vk::Buffer,
        count_offset: u64,
        max: u32,
        stride: u32,
    ) {
        // SAFETY: the caller's.
        unsafe {
            if self.info.indirect_count {
                self.device.cmd_draw_indirect_count(
                    cmd,
                    commands,
                    offset,
                    count,
                    count_offset,
                    max,
                    stride,
                );
            } else {
                self.device
                    .cmd_draw_indirect(cmd, commands, offset, max, stride);
            }
        }
    }

    /// Without `drawIndirectCount`, zeroes `size` bytes of commands before
    /// a cull appends to them, so the commands past its count draw nothing.
    /// The caller's pass declares `commands` a transfer destination.
    ///
    /// # Safety
    /// On a recording `cmd`, inside that pass.
    pub unsafe fn zero_unused(&self, cmd: vk::CommandBuffer, commands: vk::Buffer, size: u64) {
        if !self.info.indirect_count && size > 0 {
            // SAFETY: the caller's.
            unsafe { self.device.cmd_fill_buffer(cmd, commands, 0, size, 0) };
        }
    }

    /// Replaces the window surface: `None` drops it (an Android app being
    /// paused), `Some` makes one for a new window (resumed). The caller
    /// destroys the swapchain first.
    pub fn set_surface(
        &mut self,
        window: Option<(RawDisplayHandle, RawWindowHandle)>,
    ) -> Result<(), SetupError> {
        if self.surface != vk::SurfaceKHR::null() {
            // SAFETY: the caller destroyed the swapchain made on it.
            unsafe { self.surface_fn.destroy_surface(self.surface, None) };
            self.surface = vk::SurfaceKHR::null();
        }
        if let Some((display, window)) = window {
            // SAFETY: the handles come from a live window that outlives the
            // surface (dropped on the next suspend or with the context).
            self.surface = unsafe {
                ash_window::create_surface(&self.entry, &self.instance, display, window, None)
            }?;
        }
        Ok(())
    }

    /// Waits for the device to finish everything submitted.
    pub fn idle(&self) {
        // SAFETY: no other thread submits to this device.
        let _ = unsafe { self.device.device_wait_idle() };
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: everything made from the device (swapchain, frames) is
        // dropped before the context, and the device is idled first.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
            if self.surface != vk::SurfaceKHR::null() {
                self.surface_fn.destroy_surface(self.surface, None);
            }
            if let Some((loader, messenger)) = &self.debug {
                loader.destroy_debug_utils_messenger(*messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

fn choose_device(
    instance: &ash::Instance,
    surface_fn: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    prefer: Option<&str>,
) -> Result<(vk::PhysicalDevice, u32, DeviceInfo), SetupError> {
    // SAFETY: a plain enumeration on a live instance.
    let devices = unsafe { instance.enumerate_physical_devices() }?;
    let required = Features::required();
    let mut refusals = Vec::new();
    let mut best: Option<(i32, vk::PhysicalDevice, u32, DeviceInfo)> = None;
    for device in devices {
        let info = device_info(instance, device);
        let mut missing: Vec<String> = Vec::new();
        let api = vk::make_api_version(0, info.api.0, info.api.1, info.api.2);
        // A 1.3 device under an older loader still takes the extension path.
        let path = if instance_api_cached() < DESKTOP_API {
            Path::Extensions
        } else {
            path_for(api)
        };
        if api < FLOOR_API {
            missing.push(format!("Vulkan 1.1 (it has {}.{})", info.api.0, info.api.1));
        }
        if path == Path::Extensions {
            let names = extensions_of(instance, device);
            for e in PATH_EXTENSIONS {
                if !names.iter().any(|n| n.as_c_str() == e) {
                    missing.push(e.to_string_lossy().into_owned());
                }
            }
        }
        let offered = Offered::read(instance, device, path);
        let mut info = info;
        info.path = path;
        info.indirect_count = indirect_count_usable(offered.v12.draw_indirect_count == vk::TRUE);
        missing.extend(offered.missing(&required).into_iter().map(str::to_owned));
        // SAFETY: a plain query on an enumerated device.
        let families = unsafe { instance.get_physical_device_queue_family_properties(device) };
        let family = families.iter().enumerate().find_map(|(i, f)| {
            let both = vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE;
            // SAFETY: as above.
            // Headless, nothing is presented.
            let presents = surface == vk::SurfaceKHR::null()
                || unsafe {
                    surface_fn.get_physical_device_surface_support(device, i as u32, surface)
                }
                .unwrap_or(false);
            (f.queue_flags.contains(both) && presents).then_some(i as u32)
        });
        if family.is_none() {
            missing.push("a queue that draws, computes and presents to the window".to_owned());
        }
        if !missing.is_empty() {
            refusals.push(format!("{}: {}", info.name, missing.join(", ")));
            continue;
        }
        let mut score = match info.kind {
            vk::PhysicalDeviceType::DISCRETE_GPU => 1000,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 100,
            _ => 0,
        };
        if prefer.is_some_and(|p| info.name.to_lowercase().contains(&p.to_lowercase())) {
            score += 10_000;
        }
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, device, family.expect("checked above"), info));
        }
    }
    match best {
        Some((_, device, family, info)) => Ok((device, family, info)),
        None => Err(SetupError(format!(
            "No graphics card here can run Torch's renderer. {}",
            if refusals.is_empty() {
                "No Vulkan device was found.".to_owned()
            } else {
                format!("Missing: {}", refusals.join("; "))
            }
        ))),
    }
}

fn device_info(instance: &ash::Instance, device: vk::PhysicalDevice) -> DeviceInfo {
    // SAFETY: a plain query on an enumerated device.
    let api = unsafe { instance.get_physical_device_properties(device) }.api_version;
    // Driver properties are 1.2 (or VK_KHR_driver_properties).
    let driver_known = api >= vk::API_VERSION_1_2
        || extensions_of(instance, device)
            .iter()
            .any(|e| e.as_c_str() == ash::khr::driver_properties::NAME);
    let mut driver = vk::PhysicalDeviceDriverProperties::default();
    let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
    let properties = {
        let mut properties2 = vk::PhysicalDeviceProperties2::default().push_next(&mut subgroup);
        if driver_known {
            properties2 = properties2.push_next(&mut driver);
        }
        // SAFETY: a query into structures we own.
        unsafe { instance.get_physical_device_properties2(device, &mut properties2) };
        properties2.properties
    };
    let api = properties.api_version;
    DeviceInfo {
        name: properties.device_name_as_c_str().map_or_else(
            |_| "unknown".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        ),
        kind: properties.device_type,
        api: (
            vk::api_version_major(api),
            vk::api_version_minor(api),
            vk::api_version_patch(api),
        ),
        driver: format!(
            "{} {}",
            driver
                .driver_name_as_c_str()
                .map_or_else(|_| String::new(), |n| n.to_string_lossy().into_owned()),
            driver
                .driver_info_as_c_str()
                .map_or_else(|_| String::new(), |n| n.to_string_lossy().into_owned())
        ),
        vendor_id: properties.vendor_id,
        subgroup_size: subgroup.subgroup_size,
        // SAFETY: a plain query on a device the instance enumerated.
        pipeline_statistics: unsafe { instance.get_physical_device_features(device) }
            .pipeline_statistics_query
            == vk::TRUE,
        path: Path::Core,
        indirect_count: true,
    }
}

static INSTANCE_API: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(DESKTOP_API);

/// The instance version: 1.3, or what an older loader offers (at least 1.1).
fn instance_api(entry: &ash::Entry) -> u32 {
    // SAFETY: a plain query of the loader.
    let offered = unsafe { entry.try_enumerate_instance_version() }
        .ok()
        .flatten()
        .unwrap_or(vk::API_VERSION_1_0);
    let api = offered.clamp(FLOOR_API, DESKTOP_API);
    INSTANCE_API.store(api, std::sync::atomic::Ordering::Relaxed);
    api
}

fn instance_api_cached() -> u32 {
    INSTANCE_API.load(std::sync::atomic::Ordering::Relaxed)
}

/// Loads the device's functions with each 1.2 and 1.3 entry point taken
/// from its extension (`vkCmdBeginRendering` from `vkCmdBeginRenderingKHR`,
/// and so on), so the renderer calls one set of names on either path.
///
/// # Safety
/// `device` was made by `instance` with the extension path's extensions.
unsafe fn load_aliased(instance: &ash::Instance, device: vk::Device) -> ash::Device {
    let get = instance.fp_v1_0().get_device_proc_addr;
    let lookup = |name: &CStr| -> *const c_void {
        // SAFETY: a lookup by a NUL-terminated name on a live device.
        unsafe { get(device, name.as_ptr()) }.map_or(std::ptr::null(), |f| f as *const c_void)
    };
    // SAFETY: each pointer is the device's own, for the name ash asks for or
    // the extension function with the same signature.
    unsafe {
        ash::Device::load_with(
            |name| {
                for suffix in ["KHR", "EXT"] {
                    let mut with = name.to_bytes().to_vec();
                    with.extend_from_slice(suffix.as_bytes());
                    with.push(0);
                    let alias = CStr::from_bytes_with_nul(&with).expect("one NUL");
                    let f = lookup(alias);
                    if !f.is_null() {
                        return f;
                    }
                }
                lookup(name)
            },
            device,
        )
    }
}

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    kind: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut c_void,
) -> vk::Bool32 {
    use std::sync::atomic::Ordering;
    // SAFETY: the layer passes a valid callback structure for the call.
    let message = unsafe {
        data.as_ref()
            .and_then(|d| (!d.p_message.is_null()).then(|| CStr::from_ptr(d.p_message)))
            .map_or_else(String::new, |m| m.to_string_lossy().into_owned())
    };
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        super::VALIDATION_ERRORS.fetch_add(1, Ordering::Relaxed);
        eprintln!("vk error ({kind:?}): {message}");
    } else {
        super::VALIDATION_WARNINGS.fetch_add(1, Ordering::Relaxed);
        eprintln!("vk warning ({kind:?}): {message}");
    }
    vk::FALSE
}
