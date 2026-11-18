//! Shader packs on the native renderer (VULKAN_REWRITE_CHECKLIST.md phase
//! 6). A pack is compiled exactly as the game compiles it
//! (`shaderpack::compile::compile_frame`, then `install`), and its
//! stages go to SPIR-V through `shaderpack::backend::wgsl_to_spirv` (6.1).
//! This module turns that description into Vulkan objects:
//!
//! - `targets`: the colour targets (each a pair the passes flip between),
//!   the depth copies and the shadow maps, at the formats the pack asks for
//!   or the closest the device can blend and filter (6.2, step 1).
//!
//! Passes, pipelines and bindings follow in the next steps.

pub mod bindless;
pub mod screen;
pub mod targets;

use crate::client::dimension::Dimension;
use crate::shaderpack::install::{Installed, install};

/// Compiles and installs the pack named `name` (a folder or zip in the
/// shaders folder, or the built-in pack's name), as the game would for the
/// overworld.
pub(crate) fn load(name: &str, direct: bool) -> Result<Installed, String> {
    // Terrain programs pull from the native renderer's pools.
    let frame = crate::shaderpack::compile::with_vulkan_terrain(|| {
        crate::shaderpack::compile::compile_frame(name, direct, None, Dimension::Overworld)
    })?;
    Ok(install(name, 0, direct, frame))
}
