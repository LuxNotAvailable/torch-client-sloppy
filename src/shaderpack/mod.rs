//! Shader packs: discover, `shaders.properties`, preprocess, options, program
//! pipeline, and the compiler both renderers draw from (`compile`, `install`).

pub(crate) const ROOT: &str = "shaders";

pub(crate) const SOURCE_EXTENSIONS: &[&str] = &[".glsl", ".vsh", ".fsh", ".gsh", ".csh", ".wgsl"];

pub(crate) mod backend;
pub(crate) mod blockids;
pub(crate) mod builtin;
pub(crate) mod compile;
pub(crate) mod customimages;
pub(crate) mod directives;
pub(crate) mod discover;
pub(crate) mod endflash;
pub(crate) mod expressions;
pub(crate) mod features;
pub(crate) mod frame_state;
pub(crate) mod include;
pub(crate) mod inputs;
pub(crate) mod install;
pub(crate) mod lang;
pub(crate) mod literal;
pub(crate) mod macros;
pub(crate) mod options;
pub(crate) mod packvertex;
pub(crate) mod pipeline;
pub(crate) mod preprocess;
pub(crate) mod programs;
pub(crate) mod properties;
pub(crate) mod sources;
pub(crate) mod textures;
pub(crate) mod transform;
pub(crate) mod uniforms;
pub(crate) mod usage;
