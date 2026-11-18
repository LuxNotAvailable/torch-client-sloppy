use std::sync::Arc;

use super::compile::{self, Frame, ProgramKind};
use super::sources::TextureSource;
use crate::shaderpack::directives::{Constants, Targets};
use crate::shaderpack::expressions::Customs;
use crate::shaderpack::pipeline::Layout;

/// Labels every pack shader module and GPU object, so errors name the pack.
pub(crate) const GPU_LABEL: &str = "shaderpack";

impl InstalledProgram {
    pub(crate) fn fragment_for(&self, default: Option<compile::AlphaTest>) -> &Module {
        compile::alpha_for(self.alpha_override, default)
            .and_then(|test| self.fragment_tests.iter().find(|(t, _)| *t == test))
            .map_or(&self.fragment, |(_, module)| module)
    }

    pub(crate) fn texture_source(&self, binding: u32) -> TextureSource {
        self.textures
            .iter()
            .find(|(b, _)| *b == binding)
            .map(|(_, source)| *source)
            .expect("compilation resolved every texture binding")
    }
}

pub(crate) struct Module {
    pub(crate) label: String,
    pub(crate) wgsl: String,
}

pub(crate) struct InstalledProgram {
    pub(crate) name: String,
    pub(crate) kind: ProgramKind,
    pub(crate) vertex: Module,
    /// An entity program's vertex stage for GPU-posed rigs' meshes.
    #[allow(dead_code, reason = "the native pack rewrite picks it up")]
    pub(crate) vertex_skinned: Option<Module>,
    pub(crate) fragment: Module,
    pub(crate) fragment_tests: Vec<(compile::AlphaTest, Module)>,
    pub(crate) alpha_override: Option<Option<compile::AlphaTest>>,
    pub(crate) fragment_linear: Option<Module>,
    pub(crate) compute: Option<Module>,
    #[allow(dead_code, reason = "the native pack rewrite picks it up")]
    pub(crate) compute_used: Option<Vec<u32>>,
    pub(crate) compute_parity: Option<[(Module, Vec<u32>); 2]>,
    pub(crate) dispatch: compile::Dispatch,
    pub(crate) layout: Layout,
    pub(crate) draw_buffers: Vec<u8>,
    pub(crate) textures: Vec<(u32, TextureSource)>,
    pub(crate) mipmapped: u16,
}

#[allow(dead_code, reason = "pack data the wgpu path read; the native pack rewrite picks it up")]
pub(crate) struct Installed {
    pub(crate) generation: u32,
    pub(crate) direct: bool,
    pub(crate) programs: Vec<InstalledProgram>,
    pub(crate) geometry: [Option<usize>; 3],
    pub(crate) stages: [Vec<super::compile::Step>; 4],
    pub(crate) final_computes: Vec<usize>,
    pub(crate) final_pass: Option<usize>,
    pub(crate) setup: Vec<usize>,
    pub(crate) shadow: Option<usize>,
    pub(crate) shadow_streams: [Option<usize>; 3],
    pub(crate) shadowcomp: Vec<usize>,
    pub(crate) entities: [Option<usize>; super::compile::EntityKind::COUNT],
    pub(crate) shadow_casters: super::compile::ShadowCasters,
    pub(crate) shadow_culling: super::compile::ShadowCulling,
    pub(crate) blends: super::compile::Blends,
    pub(crate) flips: super::compile::FlipOverrides,
    pub(crate) targets: Targets,
    pub(crate) constants: Constants,
    pub(crate) customs: Customs,
    pub(crate) used_targets: u16,
    pub(crate) used_shadow: u8,
    pub(crate) images: Vec<super::textures::CustomTexture>,
    pub(crate) block_ids: Option<Arc<crate::shaderpack::packvertex::BlockIds>>,
    pub(crate) custom_images: Vec<crate::shaderpack::customimages::CustomImage>,
    pub(crate) buffers: Vec<crate::shaderpack::customimages::BufferObject>,
    pub(crate) entity_ids: std::collections::HashMap<String, i32>,
    pub(crate) item_ids: std::collections::HashMap<String, i32>,
    pub(crate) old_hand_light: bool,
}

/// A compiled pack ready to draw: each stage's WGSL labelled for its
/// renderer. Device free, so the native renderer installs packs the same
/// way (`vk::pack`).
pub(crate) fn install(pack: &str, generation: u32, direct: bool, frame: Frame) -> Installed {
    let add = |program: &str, stage: &str, wgsl: String| Module {
        label: format!("{GPU_LABEL}/{pack}/{generation}/{program}.{stage}.wgsl"),
        wgsl,
    };
    let programs = frame
        .programs
        .into_iter()
        .map(|p| InstalledProgram {
            vertex: add(&p.name, "vsh", p.vertex),
            vertex_skinned: p.vertex_skinned.map(|v| add(&p.name, "skinned.vsh", v)),
            fragment: add(&p.name, "fsh", p.fragment),
            compute: p.compute.map(|c| add(&p.name, "csh", c)),
            compute_used: p.compute_used,
            compute_parity: p
                .compute_parity
                .map(|[(even, even_used), (odd, odd_used)]| {
                    [
                        (add(&p.name, "even.csh", even), even_used),
                        (add(&p.name, "odd.csh", odd), odd_used),
                    ]
                }),
            dispatch: p.dispatch,
            fragment_tests: p
                .fragment_tests
                .into_iter()
                .enumerate()
                .map(|(n, (test, f))| (test, add(&p.name, &format!("alpha{n}.fsh"), f)))
                .collect(),
            alpha_override: p.alpha_override,
            fragment_linear: p.fragment_linear.map(|f| add(&p.name, "linear.fsh", f)),
            name: p.name,
            kind: p.kind,
            layout: p.layout,
            draw_buffers: p.draw_buffers,
            textures: p.textures,
            mipmapped: p.mipmapped,
        })
        .collect();
    Installed {
        generation,
        direct,
        programs,
        geometry: frame.geometry,
        stages: frame.stages,
        final_computes: frame.final_computes,
        final_pass: frame.final_pass,
        setup: frame.setup,
        shadow: frame.shadow,
        shadow_streams: frame.shadow_streams,
        shadowcomp: frame.shadowcomp,
        entities: frame.entities,
        shadow_casters: frame.shadow_casters,
        shadow_culling: frame.shadow_culling,
        blends: frame.blends,
        flips: frame.flips,
        targets: frame.targets,
        constants: frame.constants,
        customs: frame.customs,
        used_targets: frame.used_targets,
        used_shadow: frame.used_shadow,
        images: frame.images,
        block_ids: frame.block_ids,
        item_ids: frame.item_ids,
        old_hand_light: frame.old_hand_light,
        entity_ids: frame.entity_ids,
        custom_images: frame.custom_images,
        buffers: frame.buffers,
    }
}

