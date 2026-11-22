//! The pack's render passes for tile-based GPUs (Android): consecutive draw
//! passes into the same attachments share one render pass, and each
//! attachment's load and store ops follow what the frame does with it, so
//! tiles stay on chip instead of round-tripping through memory.
//!
//! - Clears wait for an image's first use: a `CLEAR` load op when it is
//!   drawn into, a transfer clear just before when it is read first.
//! - A full-screen pass that writes every pixel unblended (no discard)
//!   doesn't load what it overwrites.
//! - An image nothing later in the frame uses, and that isn't kept for the
//!   next frame, isn't stored.
//!
//! On PC every pass records alone with LOAD and STORE, as before
//! (`MC_PACK_TILED=1` forces this path on, to compare images).
//!
//! `MC_PACK_AB=<feature>` alternates one Android pack feature every two
//! seconds (B on) and logs each side's GPU time: `tiled` (this), `hiz`
//! (occlusion from the pack's depth, `TerrainScene::pack_pyramid`).

use std::collections::{HashMap, HashSet};

use ash::vk;

use crate::vk::graph::{Graph, Use};

/// Android (or forced): see the module doc.
fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        cfg!(target_os = "android")
            || crate::platform::env::var("MC_PACK_TILED").is_ok_and(|v| v == "1")
    })
}

/// The feature `MC_PACK_AB` alternates, if any.
fn ab_feature() -> Option<&'static str> {
    static AB: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    AB.get_or_init(|| crate::platform::env::var("MC_PACK_AB").ok())
        .as_deref()
}

fn ab_mode() -> bool {
    ab_feature().is_some()
}

/// Whether `feature` is on this frame: always, but on side A of its A/B.
pub(crate) fn feature_on(feature: &str) -> bool {
    if ab_feature() != Some(feature) {
        return true;
    }
    let side = crate::vk::terrain::ab_clock().0;
    static SIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
    if SIDE.swap(u8::from(side), std::sync::atomic::Ordering::Relaxed) != u8::from(side) {
        bevy::log::info!("pack {feature} A/B: side {}", if side { "B" } else { "A" });
    }
    side
}

/// The A/B's side for timing (true: B), past a settle after each flip
/// (results read back frames later); `None` outside the A/B or settling.
fn ab_side() -> Option<bool> {
    let (side, into) = crate::vk::terrain::ab_clock();
    (ab_mode() && into >= 0.6).then_some(side)
}

/// A frame's GPU total, for the A/B: logs each side's mean every 10 s.
pub(crate) fn note_gpu(total_ms: f32) {
    use std::sync::Mutex;
    static SIDES: Mutex<([(f64, u32); 2], Option<std::time::Instant>)> =
        Mutex::new(([(0.0, 0); 2], None));
    let Some(side) = ab_side() else {
        return;
    };
    let Ok(mut s) = SIDES.lock() else {
        return;
    };
    let entry = &mut s.0[usize::from(side)];
    entry.0 += f64::from(total_ms);
    entry.1 += 1;
    let since = *s.1.get_or_insert_with(std::time::Instant::now);
    if since.elapsed().as_secs_f32() >= 10.0 {
        let mean = |(sum, n): (f64, u32)| sum / f64::from(n.max(1));
        bevy::log::info!(
            "pack {} A/B: gpu A {:.2} ms ({} frames), B {:.2} ms ({} frames)",
            ab_feature().unwrap_or_default(),
            mean(s.0[0]),
            s.0[0].1,
            mean(s.0[1]),
            s.0[1].1
        );
        *s = ([(0.0, 0); 2], Some(std::time::Instant::now()));
    }
}

/// This frame records tiled.
pub(super) fn on() -> bool {
    enabled() && feature_on("tiled")
}

/// Whether SPIR-V `words` can discard a fragment.
pub(super) fn discards(words: &[u32]) -> bool {
    const KILL: u32 = 252;
    const TERMINATE: u32 = 4416;
    const DEMOTE: u32 = 5380;
    let mut at = 5;
    while at < words.len() {
        let (count, op) = ((words[at] >> 16) as usize, words[at] & 0xFFFF);
        if matches!(op, KILL | TERMINATE | DEMOTE) {
            return true;
        }
        at += count.max(1);
    }
    false
}

/// A render pass waiting to be recorded, maybe with the next ones.
pub(super) struct Member<'a> {
    pub name: &'static str,
    pub images: Vec<(vk::Image, Use)>,
    pub buffers: Vec<(vk::Buffer, Use)>,
    pub colors: Vec<(vk::Image, vk::ImageView)>,
    pub depth: Option<(vk::Image, vk::ImageView)>,
    pub extent: vk::Extent2D,
    /// Writes every pixel of its colour attachments unblended.
    pub full_cover: bool,
    /// A draw pass that may share a render pass with the next.
    pub mergeable: bool,
    pub sampled: Vec<vk::Image>,
    /// Images a later pass of the frame may read or draw into.
    pub later: HashSet<vk::Image>,
    /// Everything after `vkCmdBeginRendering`, up to its end.
    pub body: Box<dyn FnOnce(vk::CommandBuffer) + 'a>,
}

/// The frame's pending render pass and its waiting clears.
pub(super) struct Batch<'a> {
    tiled: bool,
    pending: Vec<Member<'a>>,
    /// Cleared images whose clear waits for their first use, and whether
    /// each is depth.
    deferred: HashMap<vk::Image, (vk::ClearValue, bool)>,
    /// Images whose contents outlive the frame (history targets, shadows).
    kept: HashSet<vk::Image>,
    /// Debug views read any target: store everything.
    store_all: bool,
    /// Debug (`MC_PACK_TILED_OFF`: merge, store, load, clear): parts off.
    off: Vec<String>,
}

impl<'a> Batch<'a> {
    pub fn new(tiled: bool, kept: HashSet<vk::Image>) -> Self {
        Batch {
            tiled,
            pending: Vec::new(),
            deferred: HashMap::new(),
            kept,
            store_all: std::env::var_os("MC_PACK_SHOW").is_some(),
            off: crate::platform::env::var("MC_PACK_TILED_OFF")
                .map(|v| v.split(',').map(str::to_owned).collect())
                .unwrap_or_default(),
        }
    }

    /// Always stores `image` (the frame's output, read after the pack).
    pub fn keep(&mut self, image: vk::Image) {
        self.kept.insert(image);
    }

    /// Takes `image`'s clear (`color`, or depth 1 for `None`) to its first
    /// use, when tiled; false to clear it now.
    pub fn defer_clear(&mut self, image: vk::Image, color: Option<[f32; 4]>) -> bool {
        if self.tiled && !self.off.iter().any(|o| o == "clear") {
            let value = match color {
                Some(c) => vk::ClearValue {
                    color: vk::ClearColorValue { float32: c },
                },
                None => vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            };
            self.deferred.insert(image, (value, color.is_none()));
        }
        self.tiled && !self.off.iter().any(|o| o == "clear")
    }

    /// Before work outside a render pass: records the pending one, and
    /// clears the waiting `reads` (it reads them) or forgets the waiting
    /// `overwrites` (it writes all of them).
    pub fn before(
        &mut self,
        graph: &mut Graph,
        device: &ash::Device,
        reads: &[vk::Image],
        overwrites: &[vk::Image],
    ) {
        self.flush(graph, device);
        for image in overwrites {
            self.deferred.remove(image);
        }
        self.clear_now(graph, device, reads);
    }

    fn clear_now(&mut self, graph: &mut Graph, device: &ash::Device, images: &[vk::Image]) {
        let due: Vec<(vk::Image, (vk::ClearValue, bool))> = images
            .iter()
            .filter_map(|i| self.deferred.remove(i).map(|v| (*i, v)))
            .collect();
        if due.is_empty() {
            return;
        }
        let uses: Vec<_> = due.iter().map(|d| (d.0, Use::TransferDst)).collect();
        graph.pass("pack clears (late)", &uses, &[], |cmd| unsafe {
            // SAFETY: clears of images in TRANSFER_DST.
            for (image, (value, depth)) in &due {
                if *depth {
                    device.cmd_clear_depth_stencil_image(
                        cmd,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &value.depth_stencil,
                        &[crate::vk::memory::range(vk::ImageAspectFlags::DEPTH)],
                    );
                } else {
                    device.cmd_clear_color_image(
                        cmd,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &value.color,
                        &[crate::vk::memory::range(vk::ImageAspectFlags::COLOR)],
                    );
                }
            }
        });
    }

    /// Queues `member`, recording the pending pass first unless it can join.
    pub fn add(&mut self, graph: &mut Graph, device: &ash::Device, member: Member<'a>) {
        let joins = self.tiled
            && !self.off.iter().any(|o| o == "merge")
            && member.mergeable
            && self.pending.last().is_some_and(|last| {
                last.mergeable
                    && last.colors == member.colors
                    && last.depth == member.depth
                    && last.extent == member.extent
            })
            && {
                let attached: Vec<vk::Image> = member
                    .colors
                    .iter()
                    .map(|c| c.0)
                    .chain(member.depth.map(|d| d.0))
                    .collect();
                !member.sampled.iter().any(|s| attached.contains(s))
            };
        if !joins {
            self.flush(graph, device);
        }
        // A pass reading a waiting image clears it first (it isn't drawn).
        let reads: Vec<vk::Image> = member.sampled.clone();
        if !joins && reads.iter().any(|r| self.deferred.contains_key(r)) {
            self.clear_now(graph, device, &reads);
        }
        self.pending.push(member);
    }

    /// Records the pending render pass.
    pub fn flush(&mut self, graph: &mut Graph, device: &ash::Device) {
        if self.pending.is_empty() {
            return;
        }
        let members = std::mem::take(&mut self.pending);
        let first = &members[0];
        let last = &members[members.len() - 1];
        let store_off = self.off.iter().any(|o| o == "store");
        let load_off = self.off.iter().any(|o| o == "load");
        let store = |image: vk::Image| {
            if !self.tiled
                || store_off
                || self.store_all
                || self.kept.contains(&image)
                || last.later.contains(&image)
            {
                vk::AttachmentStoreOp::STORE
            } else {
                vk::AttachmentStoreOp::DONT_CARE
            }
        };
        let mut load = |image: vk::Image, full_cover: bool| match self.deferred.remove(&image) {
            Some((value, _)) => (vk::AttachmentLoadOp::CLEAR, value),
            None if self.tiled && full_cover && !load_off => {
                (vk::AttachmentLoadOp::DONT_CARE, vk::ClearValue::default())
            }
            None => (vk::AttachmentLoadOp::LOAD, vk::ClearValue::default()),
        };
        let colors: Vec<(
            vk::ImageView,
            vk::AttachmentLoadOp,
            vk::ClearValue,
            vk::AttachmentStoreOp,
        )> = first
            .colors
            .iter()
            .map(|&(image, view)| {
                let (op, value) = load(image, first.full_cover);
                (view, op, value, store(image))
            })
            .collect();
        let depth = first.depth.map(|(image, view)| {
            let (op, value) = load(image, false);
            (view, op, value, store(image))
        });
        let extent = first.extent;
        let name = if members.len() == 1 {
            first.name
        } else {
            let names: Vec<&str> = members
                .iter()
                .map(|m| m.name.trim_start_matches("pack "))
                .collect();
            super::screen::intern(&names.join("+"))
        };
        // Each image and buffer once, a write over a read.
        let mut images: Vec<(vk::Image, Use)> = Vec::new();
        let mut buffers: Vec<(vk::Buffer, Use)> = Vec::new();
        for m in &members {
            for &(image, usage) in &m.images {
                match images.iter_mut().find(|u| u.0 == image) {
                    Some(u) if usage.writes() => u.1 = usage,
                    Some(_) => {}
                    None => images.push((image, usage)),
                }
            }
            for &(buffer, usage) in &m.buffers {
                match buffers.iter_mut().find(|u| u.0 == buffer) {
                    Some(u) if usage.writes() => u.1 = usage,
                    Some(_) => {}
                    None => buffers.push((buffer, usage)),
                }
            }
        }
        let bodies: Vec<Box<dyn FnOnce(vk::CommandBuffer) + 'a>> =
            members.into_iter().map(|m| m.body).collect();
        graph.pass(name, &images, &buffers, |cmd| unsafe {
            // SAFETY: attachments and sampled images in the layouts the
            // graph set; each body records only inside this render pass.
            let attachments: Vec<vk::RenderingAttachmentInfo> = colors
                .iter()
                .map(|&(view, load_op, clear_value, store_op)| {
                    vk::RenderingAttachmentInfo::default()
                        .image_view(view)
                        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                        .load_op(load_op)
                        .store_op(store_op)
                        .clear_value(clear_value)
                })
                .collect();
            let depth = depth.map(|(view, load_op, clear_value, store_op)| {
                vk::RenderingAttachmentInfo::default()
                    .image_view(view)
                    .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                    .load_op(load_op)
                    .store_op(store_op)
                    .clear_value(clear_value)
            });
            let mut info = vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                })
                .layer_count(1)
                .color_attachments(&attachments);
            if let Some(depth) = depth.as_ref() {
                info = info.depth_attachment(depth);
            }
            device.cmd_begin_rendering(cmd, &info);
            for body in bodies {
                body(cmd);
            }
            device.cmd_end_rendering(cmd);
        });
    }

    /// The frame's end: records what is pending. Waiting clears no pass
    /// used are dropped: the next frame clears those images again.
    pub fn finish(mut self, graph: &mut Graph, device: &ash::Device) {
        self.flush(graph, device);
    }
}
