//! Fixed terrain: the mobile path (docs/ANDROID_VS_PC.md,
//! docs/ANDROID_SPECIFICS.md "CPU-culled terrain").
//!
//! The CPU culls each slot (cave visibility, frustum, the solid face
//! buckets) and the terrain draws one `vkCmdDrawIndexed` per run, with the
//! pool's vertices through fixed-function input
//! (`shaders/terrain_mobile.slang`). No cull compute, no indirect counts,
//! no Hi-Z. Needs plain vertices: packing and greedy merging off.
//!
//! The first instance of every draw is its slot, as the GPU cull's
//! commands.

use ash::vk;
use bevy::math::{Mat4, Vec3, Vec4};

use super::{SLOT_LIVE, SLOT_TRANSLUCENT, Slot};

/// `PackedVertex` at binding 0, as `FixedIn` in
/// `shaders/terrain_mobile.slang`.
pub(super) const ATTRIBUTES: &[(u32, vk::Format, u32)] = &[
    (0, vk::Format::R16G16B16A16_SINT, 0),
    (1, vk::Format::R16G16_UNORM, 8),
    (2, vk::Format::R8G8B8A8_UINT, 12),
    (3, vk::Format::R8G8B8A8_UNORM, 16),
];
pub(super) const STRIDE: u32 = 20;
const _: () = assert!(std::mem::size_of::<crate::terrain::mesh::PackedVertex>() == STRIDE as usize);

/// When the fixed path draws (`MC_NATIVE_TERRAIN`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The GPU-driven path only.
    Off,
    /// The fixed path only.
    On,
    /// Flips every two seconds (A GPU-driven, B fixed), both on plain
    /// vertices, for an in-run A/B.
    Ab,
    /// The GPU cull's indirect draws, solid and cutout through
    /// fixed-function input (translucent stays pulled: its one draw spans
    /// pools).
    Input,
    /// `Input` as an in-run A/B (A pulled, B input).
    InputAb,
}

/// One direct draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Draw {
    pub pool: u32,
    pub first: u32,
    pub count: u32,
    pub base: i32,
    pub slot: u32,
}

/// A frame's draws: solid and cutout by pool, translucent far to near.
#[derive(Default)]
pub(super) struct Lists {
    pub solid: Vec<Draw>,
    pub cutout: Vec<Draw>,
    pub translucent: Vec<Draw>,
}

/// The clip planes of a reversed, infinite projection: left, right,
/// bottom, top, near (no far).
fn planes(view_proj: Mat4) -> [Vec4; 5] {
    let r = [
        view_proj.row(0),
        view_proj.row(1),
        view_proj.row(2),
        view_proj.row(3),
    ];
    [
        r[3] + r[0],
        r[3] - r[0],
        r[3] + r[1],
        r[3] - r[1],
        r[3] - r[2],
    ]
}

/// Whether the box can be in view: some corner inside every plane.
fn in_frustum(planes: &[Vec4; 5], lo: Vec3, hi: Vec3) -> bool {
    planes.iter().all(|p| {
        let corner = Vec3::new(
            if p.x >= 0.0 { hi.x } else { lo.x },
            if p.y >= 0.0 { hi.y } else { lo.y },
            if p.z >= 0.0 { hi.z } else { lo.z },
        );
        p.truncate().dot(corner) + p.w >= 0.0
    })
}

/// Runs of the drawn buckets from `at` (an empty bucket between two drawn
/// ones doesn't split them), as `emit_runs` in `shaders/terrain.slang`.
fn runs(counts: &[u32], drawn: &[bool], mut at: u32, mut out: impl FnMut(u32, u32)) -> u32 {
    let mut run: Option<(u32, u32)> = None;
    for (&count, &on) in counts.iter().zip(drawn) {
        if count == 0 {
            continue;
        }
        if on {
            run = Some(run.map_or((at, at + count), |(s, _)| (s, at + count)));
        } else if let Some((s, e)) = run.take() {
            out(s, e - s);
        }
        at += count;
    }
    if let Some((s, e)) = run {
        out(s, e - s);
    }
    at
}

/// Culls `slots` from `eye` into direct draws. `visible`: the flood fill's
/// bits, when on; `rank_of`: each translucent slot's far-to-near rank.
#[allow(clippy::too_many_arguments, reason = "one frame's cull inputs")]
pub(super) fn cull(
    slots: &[Slot],
    visible: Option<&[u32]>,
    rank_of: &[u32],
    view_proj: Mat4,
    eye: Vec3,
    buckets: bool,
    torch_reach_sq: f32,
    lists: &mut Lists,
) {
    lists.solid.clear();
    lists.cutout.clear();
    lists.translucent.clear();
    let planes = planes(view_proj);
    let mut ranked = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        let flags = s.info[2];
        if flags & SLOT_LIVE == 0 {
            continue;
        }
        let slot = i as u32;
        if let Some(bits) = visible
            && bits.get(i / 32).is_none_or(|w| (w >> (i % 32)) & 1 == 0)
        {
            continue;
        }
        let origin = Vec3::new(
            s.origin_base[0] as f32,
            s.origin_base[1] as f32,
            s.origin_base[2] as f32,
        );
        let lo = origin + Vec3::from_slice(&s.lo[..3]);
        let hi = origin + Vec3::from_slice(&s.hi[..3]);
        if !in_frustum(&planes, lo, hi) {
            continue;
        }
        let (pool, base, first) = (s.info[0], s.origin_base[3], s.ranges[0]);
        let draw = |first, count| Draw {
            pool,
            first,
            count,
            base,
            slot,
        };
        if flags & SLOT_TRANSLUCENT != 0 {
            if s.ranges[1] > 0 {
                ranked.push((
                    rank_of.get(i).copied().unwrap_or(0),
                    draw(first, s.ranges[1]),
                ));
            }
            continue;
        }
        let counts = &s.buckets[..7];
        let drawn = [
            !buckets || eye.x > lo.x,
            !buckets || eye.x < hi.x,
            !buckets || eye.y > lo.y,
            !buckets || eye.y < hi.y,
            !buckets || eye.z > lo.z,
            !buckets || eye.z < hi.z,
            true,
        ];
        let end = runs(counts, &drawn, first, |f, c| lists.solid.push(draw(f, c)));
        // Cutout after the buckets; its torches close it, dropped past the
        // torch reach.
        let mut cut = s.ranges[1];
        if torch_reach_sq > 0.0 {
            let d = eye.clamp(lo, hi) - eye;
            if d.length_squared() > torch_reach_sq {
                cut -= s.buckets[7].min(cut);
            }
        }
        if cut > 0 {
            lists.cutout.push(draw(end, cut));
        }
    }
    // By pool, so each pool binds once.
    lists.solid.sort_unstable_by_key(|d| d.pool);
    lists.cutout.sort_unstable_by_key(|d| d.pool);
    ranked.sort_unstable_by_key(|r| r.0);
    lists.translucent.extend(ranked.into_iter().map(|r| r.1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_buckets_split_runs_and_empty_ones_dont() {
        let mut got = Vec::new();
        let end = runs(
            &[3, 0, 2, 4, 5],
            &[true, false, true, false, true],
            10,
            |f, c| got.push((f, c)),
        );
        assert_eq!(got, vec![(10, 5), (19, 5)]);
        assert_eq!(end, 24);
    }

    #[test]
    fn boxes_behind_the_eye_are_culled() {
        let proj = Mat4::perspective_infinite_reverse_rh(1.2, 1.0, 0.05)
            * Mat4::look_to_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        let p = planes(proj);
        assert!(in_frustum(
            &p,
            Vec3::new(-1.0, -1.0, -20.0),
            Vec3::new(1.0, 1.0, -18.0)
        ));
        assert!(!in_frustum(
            &p,
            Vec3::new(-1.0, -1.0, 18.0),
            Vec3::new(1.0, 1.0, 20.0)
        ));
        assert!(!in_frustum(
            &p,
            Vec3::new(100.0, -1.0, -5.0),
            Vec3::new(101.0, 1.0, -4.0)
        ));
    }
}
