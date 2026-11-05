//! The native Vulkan renderer (VULKAN_REWRITE_CHECKLIST.md), built
//! alongside the current Bevy/wgpu one behind the `vulkan` feature.
//!
//! For now it drives only the `vkview` development viewer
//! (`src/bin/vkview.rs`), which runs each piece on its own, with debug
//! views, before the game uses it.

pub mod bindless;
pub mod context;
pub mod depth_view;
pub mod entities;
pub mod entity_dump;
pub mod env;
pub mod frame;
pub mod graph;
pub mod greedy;
pub mod memory;
pub mod pipeline;
pub mod post;
pub mod requirements;
pub mod swapchain;
pub mod terrain;
pub mod terrain_dump;
pub mod test_scene;
pub mod timestamps;
pub mod upload;
pub mod viewer;

/// The engine's shaders, compiled from `shaders/*.slang` by `build.rs`.
pub mod shaders {
    include!(concat!(env!("OUT_DIR"), "/shaders.rs"));
}

pub use context::Context;
pub use frame::Frames;
pub use swapchain::Swapchain;

use std::sync::atomic::{AtomicU32, Ordering};

/// Validation-layer errors and warnings reported since startup. The viewer
/// fails its automated runs on any error, as each phase's gate requires.
static VALIDATION_ERRORS: AtomicU32 = AtomicU32::new(0);
static VALIDATION_WARNINGS: AtomicU32 = AtomicU32::new(0);

pub fn validation_errors() -> u32 {
    VALIDATION_ERRORS.load(Ordering::Relaxed)
}

pub fn validation_warnings() -> u32 {
    VALIDATION_WARNINGS.load(Ordering::Relaxed)
}

/// Whether to enable the Khronos validation layer: always in debug builds,
/// and in release builds when `MC_VK_VALIDATION=1`.
pub fn validation_wanted() -> bool {
    cfg!(debug_assertions) || crate::platform::env::var("MC_VK_VALIDATION").is_ok_and(|v| v == "1")
}

/// An error from setting the renderer up, in words a player can act on.
#[derive(Debug)]
pub struct SetupError(pub String);

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SetupError {}

impl From<ash::vk::Result> for SetupError {
    fn from(result: ash::vk::Result) -> SetupError {
        SetupError(format!("Vulkan call failed: {result:?}"))
    }
}
