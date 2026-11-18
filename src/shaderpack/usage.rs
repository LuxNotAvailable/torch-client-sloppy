use super::compile::{EntityKind, ProgramKind, Step};
use super::install::{Installed, InstalledProgram};
use super::sources::{Depth, TextureSource};
use crate::shaderpack::directives::{MAX_COLOR_TARGETS, SampleKind};
use crate::shaderpack::pipeline::Binding;
use crate::shaderpack::transform::ImageAccess;

pub(crate) struct Usage {
    pub(crate) shadow: bool,
    pub(crate) shadow_filtered: [bool; 2],
    pub(crate) image_conflicts: Vec<[u16; 2]>,
    pub(crate) snapshots: u16,
    pub(crate) mip_targets: u16,
    pub(crate) cleared: u16,
    pub(crate) copy_depth: bool,
    pub(crate) copy_no_hand: bool,
    pub(crate) shadow_voxelizes: bool,
    pub(crate) shadow_copy_opaque: bool,
    pub(crate) kind_blocks: [Option<usize>; EntityKind::COUNT],
}

impl Usage {
    /// `vertex_storage`: the GPU writes storage from vertex stages;
    /// `depth_copy`: it can copy depth for filtered shadow sampling.
    pub(crate) fn of(vertex_storage: bool, depth_copy: bool, installed: &Installed) -> Usage {
        let shadow = installed.shadow.is_some_and(|index| {
            let usable = vertex_storage || !writes_images_from_vertex(&installed.programs[index]);
            if !usable {
                crate::log_warn!(
                    "shaders",
                    "no shadows: the shadow program writes images from its vertex stage, which this GPU cannot"
                );
            }
            usable
        });
        let shadow_filtered = if shadow {
            shadow_filtering(depth_copy, installed)
        } else {
            [false; 2]
        };
        let image_conflicts: Vec<[u16; 2]> = installed
            .programs
            .iter()
            .map(|p| match &p.compute_parity {
                Some(variants) => variants
                    .each_ref()
                    .map(|(_, used)| image_conflicts(installed, p, Some(used))),
                None => [image_conflicts(installed, p, p.compute_used.as_deref()); 2],
            })
            .collect();
        let reads = |wanted: &dyn Fn(ProgramKind, TextureSource) -> bool| {
            installed
                .programs
                .iter()
                .any(|p| p.textures.iter().any(|(_, source)| wanted(p.kind, *source)))
        };
        Usage {
            shadow,
            shadow_filtered,
            snapshots: image_conflicts.iter().flatten().fold(0, |mask, m| mask | m),
            image_conflicts,
            mip_targets: mip_targets(installed),
            cleared: (0..MAX_COLOR_TARGETS as u8)
                .filter(|n| {
                    installed.used_targets & (1u16 << *n) != 0
                        && installed.targets.settings[*n as usize].clear
                })
                .fold(0, |mask, n| mask | 1 << n),
            copy_depth: reads(&|kind, source| match source {
                TextureSource::Depth(Depth::Opaque) => true,
                TextureSource::Depth(Depth::All) => kind != ProgramKind::Screen,
                _ => false,
            }),
            copy_no_hand: reads(&|_, source| source == TextureSource::DepthNoHand),
            shadow_voxelizes: installed.shadow.is_some_and(|index| {
                installed.programs[index]
                    .layout
                    .bindings
                    .iter()
                    .any(|b| matches!(b, Binding::Image { .. }))
            }),
            shadow_copy_opaque: !shadow_filtered[1]
                && reads(&|kind, source| {
                    !kind.is_shadow()
                        && matches!(
                            source,
                            TextureSource::ShadowDepth(Depth::Opaque)
                                | TextureSource::ShadowCompare(Depth::Opaque)
                        )
                }),
            kind_blocks: {
                let mut extra = installed.programs.len();
                std::array::from_fn(|at| match installed.entities[at] {
                    Some(_) if EntityKind::ALL[at].is_hand() => {
                        extra += 1;
                        Some(extra - 1)
                    }
                    other => other,
                })
            },
        }
    }
}

fn shadow_filtering(depth_copy: bool, installed: &Installed) -> [bool; 2] {
    if installed.shadow.is_none() || !depth_copy {
        return [false; 2];
    }
    [Depth::All, Depth::Opaque].map(|depth| {
        !installed.constants.shadow_nearest[depth_index(depth)]
            && installed.programs.iter().any(|p| {
                !p.kind.is_shadow()
                    && p.textures
                        .iter()
                        .any(|(_, s)| *s == TextureSource::ShadowDepth(depth))
            })
    })
}

pub(crate) fn depth_index(depth: Depth) -> usize {
    match depth {
        Depth::All => 0,
        Depth::Opaque => 1,
    }
}

fn image_conflicts(installed: &Installed, program: &InstalledProgram, used: Option<&[u32]>) -> u16 {
    let reached = |binding: u32| used.is_none_or(|used| used.contains(&binding));
    let written = |index: usize| {
        let name = &installed.custom_images[index].name;
        program.layout.bindings.iter().any(|b| {
            matches!(b, Binding::Image { name: n, access, .. } if n == name && *access != ImageAccess::Read)
                && reached(b.binding())
        })
    };
    program
        .textures
        .iter()
        .filter(|(binding, _)| reached(*binding))
        .filter_map(|(_, source)| match source {
            TextureSource::Image { index, .. } if written(usize::from(*index)) => {
                Some(1u16 << index)
            }
            _ => None,
        })
        .fold(0, |mask, bit| mask | bit)
}

fn writes_images_from_vertex(program: &InstalledProgram) -> bool {
    program.layout.bindings.iter().any(|b| {
        matches!(b, Binding::Image { vertex: true, access, .. } if *access != ImageAccess::Read)
            || matches!(
                b,
                Binding::Buffer {
                    vertex: true,
                    read_only: false,
                    ..
                }
            )
    })
}

pub(crate) fn mip_targets(installed: &Installed) -> u16 {
    let screen = installed
        .stages
        .iter()
        .flatten()
        .filter_map(|step| match step {
            Step::Screen(index) => Some(index),
            Step::Compute(_) => None,
        })
        .chain(&installed.final_pass)
        .fold(0u16, |mask, index| {
            mask | installed.programs[*index].mipmapped
        });
    (0..MAX_COLOR_TARGETS)
        .filter(|n| {
            screen & (1 << n) != 0
                && installed.used_targets & (1 << n) != 0
                && installed.targets.settings[*n].format.sample_kind() == SampleKind::Float
        })
        .fold(0, |mask, n| mask | 1 << n)
}
