//! Shader packs on the native renderer (VULKAN_REWRITE_CHECKLIST.md phase
//! 6). A pack is compiled exactly as the game compiles it
//! (`shaderpack::compile::compile_frame`, then `install`), and its
//! stages go to SPIR-V through `shaderpack::backend::wgsl_to_spirv` (6.1).
//! This module turns that description into Vulkan objects:
//!
//! - `targets`: the colour targets (each a pair the passes flip between),
//!   the depth copies and the shadow maps, at the formats the pack asks for
//!   or the closest the device can blend and filter (6.2, step 1).
//! - `bindless`: each program's resources moved onto the shared bindless set.
//! - `screen`: the passes, pipelines and bindings: shadow, geometry (the
//!   terrain's culled commands), deferred, composite, final; and the depth
//!   the terrain's next occlusion cull reads (`hiz_source`).
//! - `tiled`: render passes merged, with load and store ops to suit tile
//!   GPUs (Android; `MC_PACK_TILED=1` on desktop), and the in-run A/B
//!   switches (`MC_PACK_AB`).

pub mod bindless;
pub mod screen;
pub mod targets;
pub mod tiled;

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
