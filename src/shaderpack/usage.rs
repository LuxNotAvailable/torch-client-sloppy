//! What an installed pack's passes need of its targets.

use super::compile::Step;
use super::install::Installed;
use crate::shaderpack::directives::{MAX_COLOR_TARGETS, SampleKind};

/// The color targets a screen pass samples with mipmaps, as bits.
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
