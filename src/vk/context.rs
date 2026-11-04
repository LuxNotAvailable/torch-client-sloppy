//! The Vulkan instance, the chosen device and its queue, and the window's
//! surface.

use std::ffi::{CStr, c_void};

use ash::vk;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::SetupError;
use super::requirements::{DESKTOP_API, Features, Offered, device_extensions};

/// What the device is, for logs and the performance report.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub kind: vk::PhysicalDeviceType,
    pub api: (u32, u32, u32),
    pub driver: String,
    pub vendor_id: u32,
    pub subgroup_size: u32,
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
    /// Sets Vulkan up for a window. `prefer` picks a device whose name
    /// contains it (case-insensitive), as an override; otherwise a discrete
    /// GPU is preferred.
    pub fn new(
        display: RawDisplayHandle,
        window: RawWindowHandle,
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

        let mut extensions = ash_window::enumerate_required_extensions(display)?.to_vec();
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
            .api_version(DESKTOP_API);
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
        let surface =
            unsafe { ash_window::create_surface(&entry, &instance, display, window, None) }?;
        let surface_fn = ash::khr::surface::Instance::new(&entry, &instance);

        let (physical, queue_family, info) =
            choose_device(&instance, &surface_fn, surface, prefer)?;

        let priorities = [1.0f32];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&priorities)];
        let mut features = Features::required();
        let extension_names: Vec<*const i8> =
            device_extensions().iter().map(|e| e.as_ptr()).collect();
        let mut features2 = vk::PhysicalDeviceFeatures2::default()
            .features(features.core)
            .push_next(&mut features.v11)
            .push_next(&mut features.v12)
            .push_next(&mut features.v13);
        let create = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&extension_names)
            .push_next(&mut features2);
        // SAFETY: the create info and its feature chain are locals that
        // outlive the call; the device was checked to offer every feature.
        let device = unsafe { instance.create_device(physical, &create, None) }?;
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
            self.surface_fn.destroy_surface(self.surface, None);
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
        if vk::make_api_version(0, info.api.0, info.api.1, info.api.2) < DESKTOP_API {
            missing.push(format!("Vulkan 1.3 (it has {}.{})", info.api.0, info.api.1));
        }
        missing.extend(
            Offered::read(instance, device)
                .missing(&required)
                .into_iter()
                .map(str::to_owned),
        );
        // SAFETY: a plain query on an enumerated device.
        let families = unsafe { instance.get_physical_device_queue_family_properties(device) };
        let family = families.iter().enumerate().find_map(|(i, f)| {
            let both = vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE;
            // SAFETY: as above.
            let presents = unsafe {
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
    let mut driver = vk::PhysicalDeviceDriverProperties::default();
    let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
    let properties = {
        let mut properties2 = vk::PhysicalDeviceProperties2::default()
            .push_next(&mut driver)
            .push_next(&mut subgroup);
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
