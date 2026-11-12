//! A pack's screen passes on the native renderer (6.2, step 2b): begin,
//! prepare, deferred and composite, then final into the output.
//!
//! Flips follow `packdraw::plan`: a pass samples each target's current half
//! (the other half for one it also draws), writes the other, then flips.
//! Terrain draws through the pack's geometry programs before deferred
//! (solid, cutout) and after it (translucent), reusing the terrain's culled
//! commands (6.3). Compute steps are skipped for now.

use ash::vk;
use bevy::math::{Mat4, UVec2};
use gpu_allocator::MemoryLocation;

use super::bindless::{Slot, SlotKind};
use super::targets::{DEPTH_FORMAT, PackTargets};
use crate::renderer::packdraw::compile::{EntityKind, ScreenStage, Step};
use crate::renderer::packdraw::install::{InstalledProgram, Installed};
use crate::renderer::packdraw::sources::{Depth, Stub, TextureSource};
use crate::renderer::packdraw::uniforms::{BlockSpec, FrameValues, History, UniformBytes};
use crate::shaderpack::directives::{MAX_COLOR_TARGETS, SampleKind};
use crate::shaderpack::expressions::CustomState;
use crate::vk::bindless::{self as bl, Bindless};
use crate::vk::frame::FRAMES_IN_FLIGHT;
use crate::vk::graph::{Graph, Use};
use crate::vk::memory::{Buffer, Image, Memory};
use crate::vk::pipeline::{GraphicsDesc, Pipelines, Push, push};
use crate::vk::{Context, SetupError};

fn failed(what: impl std::fmt::Display) -> SetupError {
    SetupError(what.to_string())
}

/// Bit `n` set: target `n` reads half 1.
#[derive(Clone, Copy, Default)]
struct Flips(u16);

impl Flips {
    fn current(self, n: u8) -> usize {
        ((self.0 >> n) & 1) as usize
    }
    fn other(self, n: u8) -> usize {
        1 - self.current(n)
    }
    fn flip(&mut self, buffers: &[u8]) {
        for b in buffers {
            self.0 ^= 1 << b;
        }
    }
}

/// 1x1 stand-ins for sources the frame lacks.
struct Stubs {
    white: (Image, u32),
    black: (Image, u32),
    flat_normal: (Image, u32),
    uint: (Image, u32),
    depth: (Image, u32),
    noise: (Image, u32),
}

/// The bindless index each image is registered under.
struct Indexed {
    /// Per colour target, its halves.
    color: [Option<[u32; 2]>; MAX_COLOR_TARGETS],
    /// Mipmapped targets' halves, all levels.
    color_mip: [Option<[u32; 2]>; MAX_COLOR_TARGETS],
    depth: u32,
    depth_opaque: u32,
    depth_no_hand: Option<u32>,
    shadow_depth: Option<[u32; 2]>,
    shadow_color: Vec<Option<u32>>,
}

struct Pass {
    program: usize,
    pipeline: vk::Pipeline,
    modules: Vec<vk::ShaderModule>,
    base: u32,
    /// Target and half drawn, or `None` for the output.
    attachments: Option<Vec<(u8, usize)>>,
    sampled: Vec<vk::Image>,
    /// The program's uniform block and where its slot sits in the table.
    uniform: Option<(usize, usize)>,
    extent: vk::Extent2D,
    /// A geometry pass's slot (0 solid, 1 cutout, 2 translucent).
    geometry: Option<usize>,
    /// Draws the shadow map (shadow colours by index, shadowtex0).
    shadow: Option<Vec<u8>>,
    /// Draws the entities: `Some(true)` the blended ones.
    entity: Option<bool>,
    /// A compute step: its work groups.
    compute: Option<[u32; 3]>,
    /// Draws the sky pieces: (textured kind, added ones).
    sky: Option<(bool, bool)>,
    /// Mip chains to rebuild first: (target, half).
    mipmaps: Vec<(u8, usize)>,
    /// Draws the first-person hand (with `entity`).
    hand: bool,
}

/// Which instance set an entity-style pass draws.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Entities,
    Sky,
    Hand,
}

pub(crate) struct PackScreen {
    installed: Installed,
    targets: PackTargets,
    indexed: Indexed,
    stubs: Stubs,
    buffers: Vec<(Buffer, u32)>,
    passes: Vec<Pass>,
    /// Before which pass `depthtex0` is copied to `depthtex1`.
    copy_depth_at: Option<usize>,
    /// Where depthtex0 is copied to depthtex2, before the hand.
    copy_no_hand_at: Option<usize>,
    /// The plans frames alternate between (the game's two when a frame
    /// ends flipped, so history carries over): pass range, start flips.
    plans: Vec<(std::ops::Range<usize>, Flips)>,
    /// Before which pass `shadowtex0` is copied to `shadowtex1`.
    copy_shadow_at: Option<usize>,
    /// This frame's values, from `begin_frame`.
    values: Option<FrameValues>,
    /// The block atlas (`gtexture` for terrain) and the game's lightmap.
    atlas: Option<(vk::Image, u32)>,
    lightmap: Option<(vk::Image, u32)>,
    /// Table words that hold the terrain's slots buffer.
    terrain_slots: Vec<usize>,
    /// Table entries for the block animation table.
    anim_slots: Vec<usize>,
    /// Table words for the entity buffers: 0 instances, 1 meshes, 2
    /// vertices, 3 parts.
    entity_slots: Vec<(usize, u8, Source)>,
    /// While building an entity pass: whether it blends.
    building_entity: Option<bool>,
    /// While building a hand pass.
    building_hand: bool,
    /// While building a sky pass: its kind, and whether it adds.
    building_sky: Option<(EntityKind, bool)>,
    /// While building a compute pass.
    building_compute: bool,
    /// Per program with a uniform block: its bytes and a buffer per frame.
    uniforms: Vec<(UniformBytes, Vec<(Buffer, u32)>)>,
    table: Vec<u32>,
    tables: Vec<(Buffer, u32)>,
    staging: Option<(Buffer, Option<u64>)>,
    settled: bool,
    customs: CustomState,
    history: History,
    frames: u32,
    last: Option<f32>,
    /// The game's frame inputs (time of day, sky), from the snapshot.
    pub(crate) inputs: crate::renderer::packdraw::inputs::Inputs,
    pub(crate) skipped_computes: usize,
}

fn sampler_for(installed: &Installed, source: TextureSource, comparison: bool) -> u32 {
    if comparison {
        return if installed.shadow.is_some() {
            bl::SAMPLER_COMPARE_LINEAR
        } else {
            bl::SAMPLER_COMPARE_ALWAYS
        };
    }
    match source {
        TextureSource::Noise => bl::SAMPLER_LINEAR,
        TextureSource::Atlas | TextureSource::Lightmap | TextureSource::EntityTexture => {
            bl::SAMPLER_NEAREST_CLAMP
        }
        TextureSource::Custom(i) | TextureSource::Custom3d(i) => {
            match installed.images.get(usize::from(i)).map(|im| (im.blur, im.clamp)) {
                Some((true, true)) => bl::SAMPLER_LINEAR_CLAMP,
                Some((false, true)) => bl::SAMPLER_NEAREST_CLAMP,
                Some((true, false)) => bl::SAMPLER_LINEAR,
                _ => bl::SAMPLER_NEAREST,
            }
        }
        _ => match source.sample_kind(&installed.targets) {
            SampleKind::Float => bl::SAMPLER_LINEAR_CLAMP,
            _ => bl::SAMPLER_NEAREST_CLAMP,
        },
    }
}

impl PackScreen {
    #[allow(clippy::too_many_arguments, reason = "setup's whole context")]
    pub(crate) fn new(
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        installed: Installed,
        size: vk::Extent2D,
        output: vk::Format,
        terrain: Option<&crate::vk::terrain::PackGeometry>,
    ) -> Result<PackScreen, SetupError> {
        let targets = PackTargets::new(ctx, memory, &installed, size, true)?;
        let full = || failed("the bindless set is full");
        let texture = |bindless: &mut Bindless, image: &Image| {
            bindless
                .add_texture(ctx, image.view, sampled_layout(image.format == DEPTH_FORMAT))
                .ok_or_else(full)
        };

        let mut color = [None; MAX_COLOR_TARGETS];
        let mut color_mip = [None; MAX_COLOR_TARGETS];
        for (n, slot) in targets.color.iter().enumerate() {
            if let Some((pair, _)) = slot {
                color[n] = Some([texture(bindless, &pair[0])?, texture(bindless, &pair[1])?]);
                if let (Some(a), Some(b)) = (pair[0].mip_view, pair[1].mip_view) {
                    let add = |bindless: &mut Bindless, v| {
                        bindless.add_texture(ctx, v, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL).ok_or_else(full)
                    };
                    color_mip[n] = Some([add(bindless, a)?, add(bindless, b)?]);
                }
            }
        }
        let (shadow_depth, shadow_color) = match &targets.shadow {
            Some(s) => (
                Some([texture(bindless, &s.depth)?, texture(bindless, &s.depth_opaque)?]),
                s.color
                    .iter()
                    .map(|c| c.as_ref().map(|c| texture(bindless, c)).transpose())
                    .collect::<Result<_, _>>()?,
            ),
            None => (None, Vec::new()),
        };
        let indexed = Indexed {
            color,
            color_mip,
            depth: texture(bindless, &targets.depth)?,
            depth_opaque: texture(bindless, &targets.depth_opaque)?,
            depth_no_hand: targets
                .depth_no_hand
                .as_ref()
                .map(|d| texture(bindless, d))
                .transpose()?,
            shadow_depth,
            shadow_color,
        };

        // Stubs and noise, uploaded on the first frame.
        let one = vk::Extent2D { width: 1, height: 1 };
        let sampled = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
        let color_aspect = vk::ImageAspectFlags::COLOR;
        let mut stub = |name: &str, format: vk::Format, aspect, extent| -> Result<(Image, u32), SetupError> {
            let image = memory.image(ctx, name, extent, format, sampled, aspect)?;
            let index = texture(bindless, &image)?;
            Ok((image, index))
        };
        let noise_size = installed.constants.noise_resolution.clamp(1, 1024);
        let stubs = Stubs {
            white: stub("pack white", vk::Format::R8G8B8A8_UNORM, color_aspect, one)?,
            black: stub("pack black", vk::Format::R8G8B8A8_UNORM, color_aspect, one)?,
            flat_normal: stub("pack flat normal", vk::Format::R8G8B8A8_UNORM, color_aspect, one)?,
            uint: stub("pack uint", vk::Format::R32_UINT, color_aspect, one)?,
            depth: stub("pack depth stub", DEPTH_FORMAT, vk::ImageAspectFlags::DEPTH, one)?,
            noise: stub(
                "pack noise",
                vk::Format::R8G8B8A8_UNORM,
                color_aspect,
                vk::Extent2D {
                    width: noise_size,
                    height: noise_size,
                },
            )?,
        };
        let mut staging_bytes = vec![255u8, 255, 255, 255, 0, 0, 0, 255, 127, 127, 255, 255];
        staging_bytes.extend(noise_bytes(noise_size));
        let mut staging = memory.buffer(
            ctx,
            "pack staging",
            staging_bytes.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        staging.write(0, &staging_bytes);

        let buffers = installed
            .buffers
            .iter()
            .map(|b| -> Result<(Buffer, u32), SetupError> {
                let bytes = b.bytes([size.width, size.height]).div_ceil(4) * 4;
                let buffer = memory.buffer(
                    ctx,
                    "pack buffer object",
                    bytes.max(16),
                    vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                    MemoryLocation::GpuOnly,
                )?;
                let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
                Ok((buffer, index))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut screen = PackScreen {
            targets,
            indexed,
            stubs,
            buffers,
            passes: Vec::new(),
            copy_depth_at: None,
            copy_no_hand_at: None,
            plans: Vec::new(),
            copy_shadow_at: None,
            values: None,
            atlas: terrain.map(|t| t.atlas),
            lightmap: terrain.and_then(|t| t.lightmap),
            terrain_slots: Vec::new(),
            anim_slots: Vec::new(),
            entity_slots: Vec::new(),
            building_entity: None,
            building_hand: false,
            building_sky: None,
            building_compute: false,
            uniforms: Vec::new(),
            table: Vec::new(),
            tables: Vec::new(),
            staging: Some((staging, None)),
            settled: false,
            customs: installed.customs.state(),
            history: History::default(),
            frames: 0,
            last: None,
            inputs: Default::default(),
            skipped_computes: 0,
            installed,
        };

        // The plan: begin and prepare, opaque geometry, deferred, translucent
        // geometry, composite, then final.
        let mut start = Flips::default();
        loop {
        let first = screen.passes.len();
        let mut flips = start;
        for stage in ScreenStage::ALL {
            if stage == ScreenStage::Prepare
                && screen.installed.shadow.is_some()
                && screen.targets.shadow.is_some()
            {
                for slot in [0, 1, 2] {
                    if slot == 2 {
                        screen.copy_shadow_at = Some(screen.passes.len() - first);
                    }
                    if let Some(index) = screen.installed.shadow_streams[slot] {
                        let pass = screen.pass(ctx, memory, bindless, pipelines, index, flips, None, Some(slot), true)?;
                        screen.passes.push(pass);
                    }
                }
                for index in screen.installed.shadowcomp.clone() {
                    let pass = screen.pass_compute(ctx, memory, bindless, pipelines, index, flips)?;
                    screen.passes.push(pass);
                }
            }
            if stage == ScreenStage::Deferred {
                // The sky first, as vanilla draws it before the world.
                for kind in [EntityKind::SkyBasic, EntityKind::SkyTextured] {
                    if let Some(index) = screen.installed.entities[kind.index()] {
                        for additive in [false, true] {
                            let pass = screen.pass_sky(ctx, memory, bindless, pipelines, index, flips, kind, additive)?;
                            screen.passes.push(pass);
                        }
                    }
                }
                for slot in [0, 1] {
                    if let Some(index) = screen.installed.geometry[slot] {
                        let pass = screen.pass(ctx, memory, bindless, pipelines, index, flips, None, Some(slot), false)?;
                        screen.passes.push(pass);
                    }
                }
                if let Some(index) = screen.installed.entities[EntityKind::Entity.index()] {
                    let mut pass = screen.pass_entity(ctx, memory, bindless, pipelines, index, flips, false)?;
                    pass.entity = Some(false);
                    screen.passes.push(pass);
                }
                // depthtex2 before the hand, depthtex1 after it.
                if let Some(index) = screen.installed.entities[EntityKind::Hand.index()] {
                    screen.copy_no_hand_at = Some(screen.passes.len() - first);
                    let pass = screen.pass_hand(ctx, memory, bindless, pipelines, index, flips, false)?;
                    screen.passes.push(pass);
                }
                screen.copy_depth_at = Some(screen.passes.len() - first);
            }
            if stage == ScreenStage::Composite
                && let Some(index) = screen.installed.geometry[2]
            {
                let pass = screen.pass(ctx, memory, bindless, pipelines, index, flips, None, Some(2), false)?;
                screen.passes.push(pass);
            }
            if stage == ScreenStage::Composite
                && let Some(index) = screen.installed.entities[EntityKind::EntityTranslucent.index()]
            {
                let mut pass = screen.pass_entity(ctx, memory, bindless, pipelines, index, flips, true)?;
                pass.entity = Some(true);
                screen.passes.push(pass);
            }
            if stage == ScreenStage::Composite
                && let Some(index) = screen.installed.entities[EntityKind::HandWater.index()]
            {
                let pass = screen.pass_hand(ctx, memory, bindless, pipelines, index, flips, true)?;
                screen.passes.push(pass);
            }
            let steps: Vec<Step> = screen.installed.stages[stage.index()].clone();
            for step in steps {
                match step {
                    Step::Screen(index) => {
                        let pass = screen.pass(ctx, memory, bindless, pipelines, index, flips, None, None, false)?;
                        screen.passes.push(pass);
                        let program = &screen.installed.programs[index];
                        flips.flip(&program.draw_buffers);
                        let toggles = screen
                            .installed
                            .flips
                            .toggles(&program.name, &program.draw_buffers);
                        flips.flip(&toggles);
                    }
                    Step::Compute(index) => {
                        let pass = screen.pass_compute(ctx, memory, bindless, pipelines, index, flips)?;
                        screen.passes.push(pass);
                    }
                }
            }
        }
        for index in screen.installed.final_computes.clone() {
            let pass = screen.pass_compute(ctx, memory, bindless, pipelines, index, flips)?;
            screen.passes.push(pass);
        }
        // Setup computes (once, at load) aren't run yet.
        screen.skipped_computes = screen.installed.setup.len();
        if let Some(index) = screen.installed.final_pass {
            let pass = screen.pass(ctx, memory, bindless, pipelines, index, flips, Some(output), None, false)?;
            screen.passes.push(pass);
        }
        screen.plans.push((first..screen.passes.len(), start));
        // A frame that ends flipped gets a second plan from there.
        if screen.plans.len() == 2 || flips.0 == start.0 {
            break;
        }
        start = flips;
        }

        // One slot table per frame in flight; their uniform slots differ.
        for _ in 0..FRAMES_IN_FLIGHT {
            let buffer = memory.buffer(
                ctx,
                "pack slot table",
                (screen.table.len().max(4) * 4) as u64,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            let index = bindless.add_buffer(ctx, buffer.raw).ok_or_else(full)?;
            screen.tables.push((buffer, index));
        }
        Ok(screen)
    }

    fn texture_index(
        &self,
        source: TextureSource,
        flips: Flips,
        drawing: u16,
        ty: &str,
        geometry: bool,
    ) -> (u32, vk::Image) {
        let stub = |s: &(Image, u32)| (s.1, s.0.raw);
        let wants_depth = ty == "texture_depth_2d";
        let wants_uint = ty.contains("<u32>");
        let t = &self.targets;
        let picked = match source {
            TextureSource::Color(n) => match (&t.color[n as usize], self.indexed.color[n as usize]) {
                (Some((pair, _)), Some(idx)) => {
                    let half = if drawing & (1 << n) != 0 {
                        flips.other(n)
                    } else {
                        flips.current(n)
                    };
                    Some((idx[half], pair[half].raw))
                }
                _ => None,
            },
            // Geometry passes draw depthtex0, so read the opaque copy.
            TextureSource::Depth(Depth::All) if geometry => {
                Some((self.indexed.depth_opaque, t.depth_opaque.raw))
            }
            TextureSource::Depth(Depth::All) => Some((self.indexed.depth, t.depth.raw)),
            TextureSource::Atlas => self.atlas.map(|(image, i)| (i, image)),
            TextureSource::Lightmap => self.lightmap.map(|(image, i)| (i, image)),
            TextureSource::Depth(Depth::Opaque) => Some((self.indexed.depth_opaque, t.depth_opaque.raw)),
            TextureSource::DepthNoHand => t
                .depth_no_hand
                .as_ref()
                .zip(self.indexed.depth_no_hand)
                .map(|(d, i)| (i, d.raw)),
            TextureSource::ShadowDepth(d) | TextureSource::ShadowCompare(d) => {
                t.shadow.as_ref().zip(self.indexed.shadow_depth).map(|(s, i)| match d {
                    Depth::All => (i[0], s.depth.raw),
                    Depth::Opaque => (i[1], s.depth_opaque.raw),
                })
            }
            TextureSource::ShadowColor(n) => t.shadow.as_ref().and_then(|s| {
                let image = s.color.get(n as usize)?.as_ref()?;
                Some((self.indexed.shadow_color.get(n as usize).copied().flatten()?, image.raw))
            }),
            TextureSource::Noise => Some(stub(&self.stubs.noise)),
            TextureSource::Stub(Stub::FlatNormal) => Some(stub(&self.stubs.flat_normal)),
            TextureSource::Stub(Stub::Black2d) => Some(stub(&self.stubs.black)),
            _ => None,
        };
        // Keep the slot's declared kind: a mismatch gets a stub of that kind.
        let is_depth = |image: vk::Image| {
            [t.depth.raw, t.depth_opaque.raw, self.stubs.depth.0.raw].contains(&image)
                || t.depth_no_hand.as_ref().is_some_and(|d| d.raw == image)
                || t.shadow
                    .as_ref()
                    .is_some_and(|s| s.depth.raw == image || s.depth_opaque.raw == image)
        };
        match picked {
            Some(p) if wants_depth == is_depth(p.1) && !wants_uint => p,
            _ if wants_depth => stub(&self.stubs.depth),
            _ if wants_uint => stub(&self.stubs.uint),
            Some(p) => p,
            None => stub(&self.stubs.white),
        }
    }

    /// An entity pass: `pass` as geometry, drawing the entities.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn pass_entity(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        index: usize,
        flips: Flips,
        translucent: bool,
    ) -> Result<Pass, SetupError> {
        self.building_entity = Some(translucent);
        let pass = self.pass(ctx, memory, bindless, pipelines, index, flips, None, Some(0), false);
        self.building_entity = None;
        pass
    }

    /// A compute pass: samples current halves, writes buffers.
    fn pass_compute(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        index: usize,
        flips: Flips,
    ) -> Result<Pass, SetupError> {
        self.building_compute = true;
        let pass = self.pass(ctx, memory, bindless, pipelines, index, flips, None, None, false);
        self.building_compute = false;
        pass
    }

    /// A hand pass: the first-person hand, solid or translucent parts.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn pass_hand(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        index: usize,
        flips: Flips,
        translucent: bool,
    ) -> Result<Pass, SetupError> {
        self.building_hand = true;
        let pass = self.pass_entity(ctx, memory, bindless, pipelines, index, flips, translucent);
        self.building_hand = false;
        let mut pass = pass?;
        pass.entity = Some(translucent);
        pass.hand = true;
        Ok(pass)
    }

    /// A sky pass: `kind`'s pieces, blended or added, no depth test.
    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn pass_sky(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        index: usize,
        flips: Flips,
        kind: EntityKind,
        additive: bool,
    ) -> Result<Pass, SetupError> {
        self.building_sky = Some((kind, additive));
        let pass = self.pass(ctx, memory, bindless, pipelines, index, flips, None, Some(0), false);
        self.building_sky = None;
        let mut pass = pass?;
        pass.sky = Some((kind == EntityKind::SkyTextured, additive));
        Ok(pass)
    }

    #[allow(clippy::too_many_arguments, reason = "one pass's whole context")]
    fn pass(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &mut Bindless,
        pipelines: &Pipelines,
        index: usize,
        flips: Flips,
        output: Option<vk::Format>,
        geometry: Option<usize>,
        shadow: bool,
    ) -> Result<Pass, SetupError> {
        use crate::renderer::packdraw::compile::geometry_alpha;
        use crate::shaderpack::programs::Geometry;
        let program: &InstalledProgram = &self.installed.programs[index];
        let entity = self.building_entity;
        let sky = self.building_sky;
        let fragment = match (output, &program.fragment_linear, geometry) {
            (_, _, Some(_)) if let Some((kind, _)) = sky => program.fragment_for(kind.alpha_tests()[0]),
            (_, _, Some(_)) if entity.is_some() => {
                let kind = match (self.building_hand, entity == Some(true)) {
                    (true, true) => EntityKind::HandWater,
                    (true, false) => EntityKind::Hand,
                    (false, true) => EntityKind::EntityTranslucent,
                    (false, false) => EntityKind::Entity,
                };
                program.fragment_for(kind.alpha_tests()[0])
            }
            (_, _, Some(slot)) => program.fragment_for(geometry_alpha(Geometry::ALL[slot], shadow)),
            (Some(f), Some(linear), _) if format!("{f:?}").ends_with("SRGB") => linear,
            _ => &program.fragment,
        };
        // One slot list for both stages, so a shared resource keeps its index.
        let mut slots: Vec<Slot> = Vec::new();
        let mut modules = Vec::new();
        let compute = self.building_compute;
        // TODO: odd-frame parity variant; even only for now.
        let compute_module = program
            .compute_parity
            .as_ref()
            .map(|v| &v[0].0)
            .or(program.compute.as_ref());
        let stages: Vec<&crate::renderer::packdraw::install::Module> = match (compute, compute_module) {
            (true, Some(m)) => vec![m],
            (true, None) => return Err(failed(format!("{}: no compute module", program.name))),
            _ => vec![&program.vertex, fragment],
        };
        for module in stages {
            // An entity's own texture comes with its draw.
            let per_draw = |slot: &Slot| {
                (entity.is_some() || sky.is_some())
                    && slot.group == 0
                    && matches!(slot.kind, SlotKind::Texture(_))
                    && program.textures.iter().any(|(b, s)| *b == slot.binding && *s == TextureSource::EntityTexture)
            };
            let wgsl = super::bindless::rewrite_shared(&module.wgsl, &mut slots, &per_draw);
            let words = crate::shaderpack::backend::wgsl_to_spirv_aliased(&wgsl)
                .map_err(|e| failed(format!("{}: {e}", module.label)))?;
            modules.push(pipelines.module(ctx, bytemuck::cast_slice(&words))?);
        }

        // Final draws to the output, never into a target it samples.
        // As the game's binds: only geometry passes (they draw current
        // halves) read a target they draw from its other half.
        let drawing: u16 = if geometry.is_none() || shadow || output.is_some() || compute {
            0
        } else {
            program.draw_buffers.iter().fold(0, |m, b| m | 1 << b)
        };
        // Screen passes sample mipmapped targets by mip: their current
        // halves' chains, rebuilt first (the game's `rebuilt`).
        let mip_mask = if geometry.is_none() && !shadow {
            program.mipmapped & self.targets.mipmapped
        } else {
            0
        };
        let mipmaps: Vec<(u8, usize)> = (0..MAX_COLOR_TARGETS as u8)
            .filter(|n| mip_mask & (1 << n) != 0)
            .map(|n| (n, flips.current(n)))
            .collect();
        let base = self.table.len() as u32;
        let mut sampled = Vec::new();
        let mut uniform = None;
        for slot in &slots {
            let word = match &slot.kind {
                // Only the pack's own set has sources; the terrain's pack
                // quads (set 2) read zeros until the snapshot carries them.
                SlotKind::Texture(ty) if slot.group != 0 => {
                    let (i, image) = (self.stubs.uint.1, self.stubs.uint.0.raw);
                    let _ = ty;
                    sampled.push(image);
                    i
                }
                SlotKind::Texture(ty) => {
                    let source = program.texture_source(slot.binding);
                    let (i, image) = match source {
                        TextureSource::Color(n) if mip_mask & drawing & (1 << n) == 0 && mip_mask & (1 << n) != 0 => {
                            let h = flips.current(n);
                            let pair = self.targets.color[n as usize].as_ref().map(|(p, _)| p[h].raw);
                            match (self.indexed.color_mip[n as usize], pair) {
                                (Some(idx), Some(raw)) => (idx[h], raw),
                                _ => self.texture_index(source, flips, drawing, ty, geometry.is_some()),
                            }
                        }
                        _ => self.texture_index(source, flips, drawing, ty, geometry.is_some()),
                    };
                    sampled.push(image);
                    i
                }
                SlotKind::Sampler | SlotKind::Comparison if slot.group != 0 => bl::SAMPLER_NEAREST_CLAMP,
                SlotKind::Sampler | SlotKind::Comparison => {
                    let source = program.texture_source(slot.binding.saturating_sub(1));
                    match source {
                        TextureSource::Color(n) if mip_mask & (1 << n) != 0 => bl::SAMPLER_MIP_LINEAR,
                        _ => sampler_for(&self.installed, source, slot.kind == SlotKind::Comparison),
                    }
                }
                SlotKind::Buffer { ty, .. } if ty.starts_with("IrisVk") && ["IrisVkInstances", "IrisVkMeshes", "IrisVkVertices", "IrisVkParts"].contains(&ty.as_str()) => {
                    let which = ["IrisVkInstances", "IrisVkMeshes", "IrisVkVertices", "IrisVkParts"]
                        .iter()
                        .position(|t| *t == ty.as_str())
                        .unwrap_or(0) as u8;
                    let at = self.table.len();
                    let source = if self.building_sky.is_some() {
                        Source::Sky
                    } else if self.building_hand {
                        Source::Hand
                    } else {
                        Source::Entities
                    };
                    self.entity_slots.push((at, which, source));
                    0
                }
                SlotKind::Buffer { ty, .. } if ty == "IrisVkAnim" => {
                    // The animation table: filled per frame (`record`).
                    let at = self.table.len();
                    self.anim_slots.push(at);
                    0
                }
                SlotKind::Buffer { ty, .. }
                    if ty == "IrisVkSlots" || ty == "IrisVkPool" || ty == "IrisVkQuads" =>
                {
                    // Filled per frame from the terrain (`record`).
                    let at = self.table.len();
                    self.terrain_slots.push(at);
                    0
                }
                SlotKind::Buffer { ty, .. } if ty.contains("IrisFrame") || ty.starts_with("Iris") && !ty.contains("Slot") => {
                    // The frame's uniform block: filled per frame.
                    let bytes = UniformBytes::new(
                        [BlockSpec {
                            layout: &program.layout.uniforms,
                            // The hand draws with its own projection.
                            set: if self.building_hand {
                                crate::renderer::packdraw::uniforms::MatrixSet::Hand
                            } else {
                                program.kind.matrices()
                            },
                            render_stage: program.kind.render_stage(),
                        }],
                        &self.installed.customs,
                        16,
                    );
                    let mut per_frame = Vec::new();
                    for _ in 0..FRAMES_IN_FLIGHT {
                        let buffer = memory.buffer(
                            ctx,
                            "pack uniforms",
                            bytes.bytes.len().max(16) as u64,
                            vk::BufferUsageFlags::STORAGE_BUFFER,
                            MemoryLocation::CpuToGpu,
                        )?;
                        let i = bindless
                            .add_buffer(ctx, buffer.raw)
                            .ok_or_else(|| failed("the bindless set is full"))?;
                        per_frame.push((buffer, i));
                    }
                    self.uniforms.push((bytes, per_frame));
                    uniform = Some((self.uniforms.len() - 1, self.table.len() + slots.iter().position(|s| s == slot).unwrap_or(0)));
                    0
                }
                SlotKind::Buffer { .. } => {
                    // A buffer object, by the binding's buffer index.
                    let at = program.layout.bindings.iter().find_map(|b| match b {
                        crate::shaderpack::pipeline::Binding::Buffer { binding, index, .. }
                            if *binding == slot.binding =>
                        {
                            Some(*index)
                        }
                        _ => None,
                    });
                    at.and_then(|i| {
                        self.installed
                            .buffers
                            .iter()
                            .position(|b| b.index == i)
                    })
                    .map_or(0, |p| self.buffers[p].1)
                }
            };
            self.table.push(word);
        }

        let shadow_side = self.targets.shadow.as_ref().map_or(1, |s| s.size);
        let shadow_colors: Vec<u8> = if shadow {
            program
                .draw_buffers
                .iter()
                .copied()
                .filter(|n| {
                    self.targets
                        .shadow
                        .as_ref()
                        .is_some_and(|s| s.color.get(*n as usize).is_some_and(Option::is_some))
                })
                .collect()
        } else {
            Vec::new()
        };
        let (attachments, formats, extent) = match output {
            _ if shadow => (
                Some(Vec::new()),
                shadow_colors
                    .iter()
                    .filter_map(|n| self.targets.shadow.as_ref()?.color[*n as usize].as_ref().map(|c| c.format))
                    .collect(),
                vk::Extent2D {
                    width: shadow_side,
                    height: shadow_side,
                },
            ),
            Some(format) => (None, vec![format], self.targets.size),
            None => {
                // Geometry draws the current halves; screen passes the other.
                let list: Vec<(u8, usize)> = program
                    .draw_buffers
                    .iter()
                    .map(|&n| (n, if geometry.is_some() { flips.current(n) } else { flips.other(n) }))
                    .collect();
                let formats = list
                    .iter()
                    .map(|(n, _)| {
                        self.targets.color[*n as usize]
                            .as_ref()
                            .map_or(vk::Format::R8G8B8A8_UNORM, |(_, f)| *f)
                    })
                    .collect();
                let extent = list
                    .first()
                    .and_then(|(n, h)| self.targets.color[*n as usize].as_ref().map(|(p, _)| p[*h].extent))
                    .unwrap_or(self.targets.size);
                (Some(list), formats, extent)
            }
        };
        if compute {
            let pipeline = pipelines.compute(ctx, modules[0], c"main")?;
            return Ok(Pass {
                program: index,
                pipeline,
                modules,
                base,
                attachments: Some(Vec::new()),
                sampled,
                uniform,
                extent: self.targets.size,
                geometry: None,
                shadow: None,
                entity: None,
                compute: Some(program.dispatch.groups([self.targets.size.width, self.targets.size.height])),
                sky: None,
            mipmaps: mipmaps.clone(),
            hand: false,
            });
        }
        let pipeline = pipelines.graphics(
            ctx,
            &GraphicsDesc {
                module: modules[0],
                fragment_module: Some(modules[1]),
                vertex: c"main",
                fragment: c"main",
                color: &formats,
                depth: geometry.map(|_| DEPTH_FORMAT),
                depth_test: geometry.filter(|_| sky.is_none()).map(|_| {
                    if shadow {
                        vk::CompareOp::LESS
                    } else {
                        vk::CompareOp::LESS_OR_EQUAL
                    }
                }),
                depth_write: geometry.is_some() && entity != Some(true) && sky.is_none(),
                // Terrain culls back faces as the game's (clockwise front, its
                // y flipped like wgpu's: FRONT under this CCW pipeline), so
                // water's underside doesn't fight its top.
                cull: if geometry.is_some() && !shadow && entity.is_none() && sky.is_none() {
                    vk::CullModeFlags::FRONT
                } else {
                    vk::CullModeFlags::NONE
                },
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                blend: (geometry == Some(2) && !shadow) || entity == Some(true) || sky.is_some(),
                additive: sky.is_some_and(|(_, add)| add),
            },
        )?;
        Ok(Pass {
            program: index,
            pipeline,
            modules,
            base,
            attachments,
            sampled,
            uniform,
            extent,
            geometry,
            shadow: shadow.then_some(shadow_colors),
            entity: None,
            compute: None,
            sky: None,
            mipmaps: mipmaps.clone(),
            hand: false,
        })
    }

    fn all_images(&self) -> Vec<(vk::Image, vk::ImageAspectFlags)> {
        let color = vk::ImageAspectFlags::COLOR;
        let depth = vk::ImageAspectFlags::DEPTH;
        let t = &self.targets;
        let mut out: Vec<(vk::Image, vk::ImageAspectFlags)> = t
            .color
            .iter()
            .flatten()
            .flat_map(|(pair, _)| pair.iter().map(|i| (i.raw, color)))
            .collect();
        out.push((t.depth.raw, depth));
        out.push((t.depth_opaque.raw, depth));
        out.extend(t.depth_no_hand.iter().map(|d| (d.raw, depth)));
        if let Some(s) = &t.shadow {
            out.push((s.depth.raw, depth));
            out.push((s.depth_opaque.raw, depth));
            out.extend(s.color.iter().flatten().map(|c| (c.raw, color)));
        }
        let s = &self.stubs;
        for (image, aspect) in [
            (&s.white.0, color),
            (&s.black.0, color),
            (&s.flat_normal.0, color),
            (&s.uint.0, color),
            (&s.depth.0, depth),
            (&s.noise.0, color),
        ] {
            out.push((image.raw, aspect));
        }
        out
    }

    /// This frame's uniform values; the light's world-to-clip when the pack
    /// draws a shadow map (for the terrain's shadow cull).
    pub(crate) fn begin_frame(
        &mut self,
        camera: (Mat4, f32),
        extent: vk::Extent2D,
        seconds: f32,
    ) -> Option<(Mat4, bevy::math::Vec3)> {
        let dt = self.last.map_or(1.0 / 60.0, |l| (seconds - l).max(0.0));
        self.last = Some(seconds);
        self.frames += 1;
        let (world_from_view, fov) = camera;
        let values = FrameValues::of_camera(
            world_from_view,
            1.0 / (fov * 0.5).tan(),
            UVec2::new(extent.width, extent.height),
            seconds,
            dt,
            self.frames,
            // The game's render-distance fog end, else 16 chunks.
            if self.inputs.fog_range.y > 0.0 { self.inputs.fog_range.y } else { 256.0 },
            self.inputs,
            &self.installed.constants,
            &mut self.history,
        );
        self.installed
            .customs
            .evaluate(&mut self.customs, dt, &|id| values.input(id));
        let shadow = self
            .passes
            .iter()
            .any(|p| p.shadow.is_some())
            .then(|| (values.shadow_world_clip(), values.camera()));
        self.values = Some(values);
        shadow
    }

    /// Runs the chain over `output` (holding the scene's frame) and draws
    /// the final pass back into it.
    #[allow(clippy::too_many_arguments, reason = "one frame's whole context")]
    pub(crate) fn record(
        &mut self,
        ctx: &Context,
        memory: &Memory,
        bindless: &Bindless,
        pipelines: &Pipelines,
        graph: &mut Graph,
        output: (vk::Image, vk::ImageView, vk::Extent2D),
        camera: (Mat4, f32),
        frame: u64,
        slot: usize,
        seconds: f32,
        terrain: Option<&crate::vk::terrain::PackGeometry>,
        mut overlay: Option<&mut dyn FnMut(&mut Graph, Overlay)>,
    ) {
        let device = &ctx.device;
        let images = self.all_images();
        for (image, aspect) in &images {
            let layout = if self.settled {
                sampled_layout(*aspect == vk::ImageAspectFlags::DEPTH)
            } else {
                vk::ImageLayout::UNDEFINED
            };
            graph.image(*image, *aspect, layout);
        }

        // First frame: the stubs' and noise's texels.
        if let Some((staging, copied)) = &mut self.staging {
            match copied {
                None => {
                    *copied = Some(frame);
                    let source = staging.raw;
                    let s = &self.stubs;
                    let one = vk::Extent3D { width: 1, height: 1, depth: 1 };
                    let copies = [
                        (s.white.0.raw, 0u64, one),
                        (s.black.0.raw, 4, one),
                        (s.flat_normal.0.raw, 8, one),
                        (
                            s.noise.0.raw,
                            12,
                            vk::Extent3D {
                                width: s.noise.0.extent.width,
                                height: s.noise.0.extent.height,
                                depth: 1,
                            },
                        ),
                    ];
                    let uses: Vec<_> = copies.iter().map(|c| (c.0, Use::TransferDst)).collect();
                    graph.pass("pack stubs", &uses, &[], |cmd| unsafe {
                        // SAFETY: copies from the staging into images in
                        // TRANSFER_DST, as the graph set.
                        for (image, offset, extent) in copies {
                            device.cmd_copy_buffer_to_image(
                                cmd,
                                source,
                                image,
                                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                &[vk::BufferImageCopy {
                                    buffer_offset: offset,
                                    image_subresource: layers(vk::ImageAspectFlags::COLOR),
                                    image_extent: extent,
                                    ..Default::default()
                                }],
                            );
                        }
                    });
                }
                Some(at) if frame >= *at + FRAMES_IN_FLIGHT as u64 => {
                    if let Some((staging, _)) = self.staging.take() {
                        memory.free_buffer(ctx, staging);
                    }
                }
                _ => {}
            }
        }

        // Clears: cleared colour targets' read halves, and every depth.
        let f = self.inputs.fog_color;
        let fog = [f.x, f.y, f.z, 1.0];
        let mut clears: Vec<(vk::Image, Option<[f32; 4]>)> = Vec::new();
        for (n, slot) in self.targets.color.iter().enumerate() {
            if let Some((pair, _)) = slot {
                let settings = &self.installed.targets.settings[n];
                if settings.clear {
                    let start = self.plans[self.frames as usize % self.plans.len()].1;
                    clears.push((pair[start.current(n as u8)].raw, Some(settings.clear_color(n, fog))));
                }
                // First frame: zero what's never cleared, as wgpu's new
                // textures (a pack's history starts from 0, not garbage).
                if !self.settled {
                    for (h, image) in pair.iter().enumerate() {
                        if !settings.clear || h != self.plans[self.frames as usize % self.plans.len()].1.current(n as u8) {
                            clears.push((image.raw, Some([0.0; 4])));
                        }
                    }
                }
            }
        }
        let t = &self.targets;
        clears.push((t.depth.raw, None));
        clears.push((t.depth_opaque.raw, None));
        clears.extend(t.depth_no_hand.iter().map(|d| (d.raw, None)));
        clears.push((self.stubs.depth.0.raw, None));
        clears.push((self.stubs.uint.0.raw, Some([0.0; 4])));
        if let Some(s) = &t.shadow {
            clears.push((s.depth.raw, None));
            clears.push((s.depth_opaque.raw, None));
            clears.extend(s.color.iter().flatten().map(|c| (c.raw, Some([1.0; 4]))));
        }
        let uses: Vec<_> = clears.iter().map(|c| (c.0, Use::TransferDst)).collect();
        graph.pass("pack clears", &uses, &[], |cmd| unsafe {
            // SAFETY: clears of images in TRANSFER_DST.
            for (image, color) in &clears {
                match color {
                    Some(c) => device.cmd_clear_color_image(
                        cmd,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearColorValue { float32: *c },
                        &[crate::vk::memory::range(vk::ImageAspectFlags::COLOR)],
                    ),
                    None => device.cmd_clear_depth_stencil_image(
                        cmd,
                        *image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 },
                        &[crate::vk::memory::range(vk::ImageAspectFlags::DEPTH)],
                    ),
                }
            }
        });

        // Without terrain to draw, the scene's frame stands in for colortex0
        // (bottom row first, as pack targets are stored).
        let draws_terrain = terrain.is_some() && self.passes.iter().any(|p| p.geometry.is_some());
        if !draws_terrain
            && let Some((pair, _)) = &self.targets.color[0]
        {
            let target = &pair[0];
            let (src, dst) = (output.2, target.extent);
            graph.pass(
                "pack scene copy",
                &[(output.0, Use::TransferSrc), (target.raw, Use::TransferDst)],
                &[],
                |cmd| unsafe {
                    // SAFETY: a blit between images in transfer layouts.
                    let corner = |e: vk::Extent2D| vk::Offset3D {
                        x: e.width as i32,
                        y: e.height as i32,
                        z: 1,
                    };
                    device.cmd_blit_image(
                        cmd,
                        output.0,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        target.raw,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::ImageBlit {
                            src_subresource: layers(vk::ImageAspectFlags::COLOR),
                            src_offsets: [vk::Offset3D::default(), corner(src)],
                            dst_subresource: layers(vk::ImageAspectFlags::COLOR),
                            dst_offsets: [
                                vk::Offset3D {
                                    x: 0,
                                    y: dst.height as i32,
                                    z: 0,
                                },
                                vk::Offset3D {
                                    x: dst.width as i32,
                                    y: 0,
                                    z: 1,
                                },
                            ],
                        }],
                        vk::Filter::LINEAR,
                    );
                },
            );
        }

        // This frame's uniforms and slot table.
        if self.values.is_none() {
            self.begin_frame(camera, output.2, seconds);
        }
        let Some(values) = self.values.take() else {
            return;
        };
        let mut table = self.table.clone();
        if let Some(t) = terrain {
            for &at in &self.terrain_slots {
                table[at] = t.slots.1;
            }
            // The terrain writes a table every frame before the pack draws.
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| bevy::log::info!("pack anim: {} slots, table {}", self.anim_slots.len(), t.anim));
            for &at in &self.anim_slots {
                table[at] = if t.anim > 0 { t.anim - 1 } else { t.slots.1 };
            }
            for &(at, which, source) in &self.entity_slots {
                let set = match source {
                    Source::Entities => &t.entities,
                    Source::Sky => &t.sky,
                    Source::Hand => &t.hand,
                };
                if let Some(e) = set {
                    table[at] = [e.instances.1, e.meshes.1, e.vertices.1, e.parts.1][which as usize];
                }
            }
        }
        for pass in &self.passes {
            if let Some((u, at)) = pass.uniform {
                table[at] = self.uniforms[u].1[slot].1;
            }
        }
        for (bytes, per_frame) in &mut self.uniforms {
            bytes.update(&values, &self.customs);
            per_frame[slot].0.write(0, &bytes.bytes);
        }
        self.tables[slot].0.write(0, bytemuck::cast_slice(&table));
        let table_index = self.tables[slot].1;
        let mut buffers: Vec<(vk::Buffer, Use)> = vec![(self.tables[slot].0.raw, Use::SampledFragment)];
        buffers.extend(self.uniforms.iter().map(|(_, f)| (f[slot].0.raw, Use::SampledFragment)));
        buffers.extend(self.buffers.iter().map(|(b, _)| (b.raw, Use::SampledFragment)));

        let (range, _) = self.plans[self.frames as usize % self.plans.len()].clone();
        for (at, pass) in self.passes[range].iter().enumerate() {
            // depthtex2 takes depthtex0 before the hand, depthtex1 after it
            // (both at once without a hand pass).
            let mut to: Vec<vk::Image> = Vec::new();
            let no_hand = self.targets.depth_no_hand.iter().map(|d| d.raw);
            if self.copy_no_hand_at == Some(at) {
                to.extend(no_hand.clone());
            }
            if self.copy_depth_at == Some(at) {
                to.push(self.targets.depth_opaque.raw);
                if self.copy_no_hand_at.is_none() {
                    to.extend(no_hand);
                }
            }
            if !to.is_empty() {
                let from = self.targets.depth.raw;
                let mut uses = vec![(from, Use::TransferSrc)];
                uses.extend(to.iter().map(|d| (*d, Use::TransferDst)));
                let extent = self.targets.size;
                graph.pass("pack depth copy", &uses, &[], |cmd| unsafe {
                    // SAFETY: copies between depth images in transfer layouts.
                    for (to, _) in &uses[1..] {
                        device.cmd_copy_image(
                            cmd,
                            from,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            *to,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            &[vk::ImageCopy {
                                src_subresource: layers(vk::ImageAspectFlags::DEPTH),
                                dst_subresource: layers(vk::ImageAspectFlags::DEPTH),
                                extent: vk::Extent3D {
                                    width: extent.width,
                                    height: extent.height,
                                    depth: 1,
                                },
                                ..Default::default()
                            }],
                        );
                    }
                });
            }
            if self.copy_shadow_at == Some(at)
                && let Some(s) = &self.targets.shadow
            {
                let (from, to, side) = (s.depth.raw, s.depth_opaque.raw, s.size);
                graph.pass(
                    "pack shadow depth copy",
                    &[(from, Use::TransferSrc), (to, Use::TransferDst)],
                    &[],
                    |cmd| unsafe {
                        // SAFETY: a copy between depth images in transfer layouts.
                        device.cmd_copy_image(
                            cmd,
                            from,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            to,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            &[vk::ImageCopy {
                                src_subresource: layers(vk::ImageAspectFlags::DEPTH),
                                dst_subresource: layers(vk::ImageAspectFlags::DEPTH),
                                extent: vk::Extent3D {
                                    width: side,
                                    height: side,
                                    depth: 1,
                                },
                                ..Default::default()
                            }],
                        );
                    },
                );
            }
            // The world's lines and cracks, into the colortex0 the final
            // pass reads, against depthtex0.
            if pass.attachments.is_none()
                && pass.compute.is_none()
                && let Some(draw) = overlay.take()
                && let Some((pair, format)) = &self.targets.color[0]
                && let Some(color) = pair.iter().find(|i| pass.sampled.contains(&i.raw))
            {
                // Pack clip: depth 0..1, eye-relative (targets are stored
                // GL-style, bottom row first).
                let fix = Mat4::from_cols(
                    bevy::math::Vec4::new(1.0, 0.0, 0.0, 0.0),
                    bevy::math::Vec4::new(0.0, 1.0, 0.0, 0.0),
                    bevy::math::Vec4::new(0.0, 0.0, 0.5, 0.0),
                    bevy::math::Vec4::new(0.0, 0.0, 0.5, 1.0),
                );
                let view_proj = fix * values.camera_clip() * Mat4::from_translation(-values.camera());
                draw(
                    graph,
                    Overlay {
                        color: (color.raw, color.view, *format),
                        depth: (self.targets.depth.raw, self.targets.depth.view),
                        extent: self.targets.size,
                        view_proj,
                    },
                );
            }
            for &(n, h) in &pass.mipmaps {
                if let Some((pair, _)) = &self.targets.color[n as usize] {
                    record_mipmaps(device, graph, &pair[h]);
                }
            }
            if let Some(groups) = pass.compute {
                let uses: Vec<(vk::Image, Use)> = pass.sampled.iter().map(|i| (*i, Use::ComputeRead)).collect();
                let writes: Vec<(vk::Buffer, Use)> = buffers
                    .iter()
                    .map(|(b, u)| (*b, if self.buffers.iter().any(|(o, _)| o.raw == *b) { Use::ComputeWrite } else { *u }))
                    .collect();
                let (pipeline, base) = (pass.pipeline, pass.base);
                let name = intern(&self.installed.programs[pass.program].name);
                graph.pass(name, &uses, &writes, |cmd| unsafe {
                    // SAFETY: the shared layout and set; sampled images read-only.
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline);
                    device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, pipelines.layout, 0, &[bindless.set], &[]);
                    push(
                        ctx,
                        cmd,
                        pipelines.layout,
                        &Push {
                            ids: [table_index, base, 0, 0],
                            more: [0; 4],
                            params: [0.0; 4],
                            view_proj: [[0.0; 4]; 4],
                        },
                    );
                    device.cmd_dispatch(cmd, groups[0], groups[1], groups[2]);
                });
                continue;
            }
            let draw = match (pass.geometry, terrain) {
                (Some(_), Some(t)) if pass.entity.is_some() && t.entities.is_none() => continue,
                (Some(_), Some(t)) if pass.sky.is_some() && t.sky.is_none() => continue,
                (Some(_), Some(t)) if pass.hand && t.hand.is_none() => continue,
                (Some(slot), Some(t)) => Some((slot, t)),
                (Some(_), None) => continue,
                (None, _) => None,
            };
            let mut uses: Vec<(vk::Image, Use)> = pass
                .sampled
                .iter()
                .map(|i| (*i, Use::SampledFragment))
                .collect();
            let views: Vec<vk::ImageView> = match (&pass.shadow, &pass.attachments) {
                (Some(colors), _) => colors
                    .iter()
                    .filter_map(|n| self.targets.shadow.as_ref()?.color[*n as usize].as_ref())
                    .map(|image| {
                        uses.retain(|u| u.0 != image.raw);
                        uses.push((image.raw, Use::ColorAttachment));
                        image.view
                    })
                    .collect(),
                (None, Some(list)) => list
                    .iter()
                    .filter_map(|(n, h)| self.targets.color[*n as usize].as_ref().map(|(p, _)| &p[*h]))
                    .map(|image| {
                        uses.retain(|u| u.0 != image.raw);
                        uses.push((image.raw, Use::ColorAttachment));
                        image.view
                    })
                    .collect(),
                (None, None) => {
                    uses.push((output.0, Use::ColorAttachment));
                    vec![output.1]
                }
            };
            let mut pass_buffers = buffers.clone();
            // The shadow pass draws from the shadow cull, into the shadow map.
            let (depth_image, depth_image_view, commands) = match (&pass.shadow, &self.targets.shadow, draw) {
                (Some(_), Some(s), Some((_, t))) => match t.shadow {
                    Some(c) => (s.depth.raw, s.depth.view, c),
                    None => continue,
                },
                (_, _, Some((_, t))) => (self.targets.depth.raw, self.targets.depth.view, (t.commands, t.counts)),
                _ => (self.targets.depth.raw, self.targets.depth.view, (vk::Buffer::null(), vk::Buffer::null())),
            };
            uses.retain(|u| u.0 != depth_image);
            let depth_view = match draw {
                Some((_, t)) => {
                    uses.push((depth_image, Use::DepthAttachment));
                    uses.push((t.atlas.0, Use::SampledFragment));
                    pass_buffers.push((t.slots.0, Use::VertexRead));
                    pass_buffers.push((commands.0, Use::IndirectRead));
                    pass_buffers.push((commands.1, Use::IndirectRead));
                    pass_buffers.push((t.translucent.0, Use::IndexRead));
                    for (vertices, indices) in &t.pools {
                        pass_buffers.push((*vertices, Use::VertexRead));
                        pass_buffers.push((*indices, Use::IndexRead));
                    }
                    pass_buffers.extend(t.quads.iter().map(|q| (*q, Use::VertexRead)));
                    let source = if pass.sky.is_some() {
                        &t.sky
                    } else if pass.hand {
                        &t.hand
                    } else {
                        &t.entities
                    };
                    if let (true, Some(e)) = (pass.entity.is_some() || pass.sky.is_some(), source) {
                        for b in [e.instances.0, e.meshes.0, e.vertices.0, e.parts.0] {
                            pass_buffers.push((b, Use::VertexRead));
                        }
                        pass_buffers.push((e.indices, Use::IndexRead));
                        for image in &e.textures {
                            uses.retain(|u| u.0 != *image);
                            uses.push((*image, Use::SampledFragment));
                        }
                    }
                    Some(depth_image_view)
                }
                None => None,
            };
            // Final draws the whole output, whatever size the pack's targets
            // are (its render scale): it samples them by coordinate.
            let extent = if pass.attachments.is_none() { output.2 } else { pass.extent };
            let (pipeline, base) = (pass.pipeline, pass.base);
            let entity_pass = pass.entity;
            let hand_pass = pass.hand;
            let sky_pass = pass.sky;
            let white = self.stubs.white.1;
            let name = intern(&self.installed.programs[pass.program].name);
            graph.pass(name, &uses, &pass_buffers, |cmd| unsafe {
                // SAFETY: attachments and sampled images in the layouts the
                // graph set; the shared layout and set.
                let colors: Vec<vk::RenderingAttachmentInfo> = views
                    .iter()
                    .map(|v| {
                        vk::RenderingAttachmentInfo::default()
                            .image_view(*v)
                            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                            .load_op(vk::AttachmentLoadOp::LOAD)
                            .store_op(vk::AttachmentStoreOp::STORE)
                    })
                    .collect();
                let depth = depth_view.map(|view| {
                    vk::RenderingAttachmentInfo::default()
                        .image_view(view)
                        .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                        .load_op(vk::AttachmentLoadOp::LOAD)
                        .store_op(vk::AttachmentStoreOp::STORE)
                });
                let mut info = vk::RenderingInfo::default()
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent,
                    })
                    .layer_count(1)
                    .color_attachments(&colors);
                if let Some(depth) = depth.as_ref() {
                    info = info.depth_attachment(depth);
                }
                device.cmd_begin_rendering(cmd, &info);
                // A plain viewport, as wgpu's: the SPIR-V flips clip y.
                device.cmd_set_viewport(
                    cmd,
                    0,
                    &[vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: extent.width as f32,
                        height: extent.height as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                );
                device.cmd_set_scissor(
                    cmd,
                    0,
                    &[vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent,
                    }],
                );
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.layout,
                    0,
                    &[bindless.set],
                    &[],
                );
                push(
                    ctx,
                    cmd,
                    pipelines.layout,
                    &Push {
                        ids: [table_index, base, 0, 0],
                        more: [0; 4],
                        params: [0.0; 4],
                        view_proj: [[0.0; 4]; 4],
                    },
                );
                match draw {
                    // Sky pieces: one draw each, in the snapshot's order.
                    Some((_, t)) if sky_pass.is_some() => {
                        let Some(e) = &t.sky else { return };
                        device.cmd_bind_index_buffer(cmd, e.indices, 0, vk::IndexType::UINT32);
                        let (textured, adds) = sky_pass.unwrap_or_default();
                        for (i, &(first, count, base_vertex, texture, add)) in e.draws.iter().enumerate() {
                            // `blended` lists the textured pieces here.
                            if add != adds || e.blended.contains(&(i as u32)) != textured {
                                continue;
                            }
                            let texture = if texture == u32::MAX { white } else { texture };
                            push(
                                ctx,
                                cmd,
                                pipelines.layout,
                                &Push {
                                    ids: [table_index, base, texture, 0],
                                    more: [0; 4],
                                    params: [0.0; 4],
                                    view_proj: [[0.0; 4]; 4],
                                },
                            );
                            device.cmd_draw_indexed(cmd, count, 1, first, base_vertex, i as u32);
                        }
                    }
                    // Entities: one draw each, its texture in the push.
                    Some((_, t)) if entity_pass.is_some() => {
                        let Some(e) = (if hand_pass { &t.hand } else { &t.entities }) else { return };
                        device.cmd_bind_index_buffer(cmd, e.indices, 0, vk::IndexType::UINT32);
                        let order: Vec<u32> = if entity_pass == Some(true) {
                            e.blended.clone()
                        } else {
                            (0..e.draws.len() as u32).filter(|&i| !e.draws[i as usize].4).collect()
                        };
                        for i in order {
                            let (first, count, base_vertex, texture, _) = e.draws[i as usize];
                            push(
                                ctx,
                                cmd,
                                pipelines.layout,
                                &Push {
                                    ids: [table_index, base, texture, 0],
                                    more: [0; 4],
                                    params: [0.0; 4],
                                    view_proj: [[0.0; 4]; 4],
                                },
                            );
                            device.cmd_draw_indexed(cmd, count, 1, first, base_vertex, i);
                        }
                    }
                    // The terrain's culled commands: per pool for solid and
                    // cutout, one far-to-near list for translucent.
                    Some((slot, t)) if slot < 2 => {
                        let size = crate::vk::terrain::PACK_COMMAND_SIZE;
                        for (p, (_, indices)) in t.pools.iter().enumerate() {
                            let list = p as u64 * 2 + slot as u64;
                            device.cmd_bind_index_buffer(cmd, *indices, 0, vk::IndexType::UINT32);
                            device.cmd_draw_indexed_indirect_count(
                                cmd,
                                commands.0,
                                list * u64::from(t.capacity) * size,
                                commands.1,
                                list * 4,
                                t.capacity,
                                size as u32,
                            );
                        }
                    }
                    Some((_, t)) => {
                        let (indices, start, count) = t.translucent;
                        if count > 0 {
                            let size = crate::vk::terrain::PACK_COMMAND_SIZE;
                            device.cmd_bind_index_buffer(cmd, indices, 0, vk::IndexType::UINT32);
                            device.cmd_draw_indexed_indirect(
                                cmd,
                                commands.0,
                                u64::from(start) * size,
                                count,
                                size as u32,
                            );
                        }
                    }
                    None => device.cmd_draw(cmd, 3, 1, 0, 0),
                }
                device.cmd_end_rendering(cmd);
            });
        }

        // Debug: MC_PACK_SHOW shows a target instead (shadowcolor0, or
        // colortex<n>.<half>).
        let shown = std::env::var("MC_PACK_SHOW").ok().and_then(|v| {
            if v == "shadowcolor0" {
                return self.targets.shadow.as_ref()?.color[0].as_ref();
            }
            let (n, half) = v.strip_prefix("colortex")?.split_once('.')?;
            let (n, half): (usize, usize) = (n.parse().ok()?, half.parse().ok()?);
            self.targets.color.get(n)?.as_ref().map(|(pair, _)| &pair[half.min(1)])
        });
        if let Some(image) = shown {
            let (src, dst) = (image.extent, output.2);
            graph.pass(
                "pack show",
                &[(image.raw, Use::TransferSrc), (output.0, Use::TransferDst)],
                &[],
                |cmd| unsafe {
                    // SAFETY: a blit between images in transfer layouts.
                    let corner = |e: vk::Extent2D| vk::Offset3D {
                        x: e.width as i32,
                        y: e.height as i32,
                        z: 1,
                    };
                    device.cmd_blit_image(
                        cmd,
                        image.raw,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        output.0,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[vk::ImageBlit {
                            src_subresource: layers(vk::ImageAspectFlags::COLOR),
                            src_offsets: [vk::Offset3D::default(), corner(src)],
                            dst_subresource: layers(vk::ImageAspectFlags::COLOR),
                            dst_offsets: [vk::Offset3D::default(), corner(dst)],
                        }],
                        vk::Filter::LINEAR,
                    );
                },
            );
        }

        // Leave every pack image sampled-ready for the next frame.
        let settle: Vec<_> = images.iter().map(|(i, _)| (*i, Use::SampledFragment)).collect();
        graph.pass("pack settle", &settle, &[], |_| {});
        self.settled = true;
    }

    /// The pack's block ids, for the mesher's pack quads.
    pub(crate) fn block_ids(&self) -> Option<std::sync::Arc<crate::renderer::packvertex::BlockIds>> {
        self.installed.block_ids.clone()
    }

    /// The pack's `sunPathRotation`, in degrees.
    pub(crate) fn sun_path_rotation(&self) -> f32 {
        self.installed.constants.sun_path_rotation
    }

    pub(crate) fn describe(&self) -> String {
        let names: Vec<&str> = self.passes[self.plans[0].0.clone()]
            .iter()
            .map(|p| self.installed.programs[p.program].name.as_str())
            .collect();
        format!(
            "{} plan(s) of {} passes ({}), {} compute steps skipped",
            self.plans.len(),
            self.plans[0].0.len(),
            names.join(" "),
            self.skipped_computes
        )
    }

    pub(crate) fn destroy(self, ctx: &Context, memory: &Memory) {
        // SAFETY: the caller idles the device first.
        unsafe {
            for pass in &self.passes {
                ctx.device.destroy_pipeline(pass.pipeline, None);
                for m in &pass.modules {
                    ctx.device.destroy_shader_module(*m, None);
                }
            }
        }
        let s = self.stubs;
        for image in [s.white.0, s.black.0, s.flat_normal.0, s.uint.0, s.depth.0, s.noise.0] {
            memory.free_image(ctx, image);
        }
        for (b, _) in self.buffers.into_iter().chain(self.tables) {
            memory.free_buffer(ctx, b);
        }
        for (_, per_frame) in self.uniforms {
            for (b, _) in per_frame {
                memory.free_buffer(ctx, b);
            }
        }
        if let Some((staging, _)) = self.staging {
            memory.free_buffer(ctx, staging);
        }
        self.targets.destroy(ctx, memory);
    }
}

/// Where the world's lines and cracks go under a pack.
pub(crate) struct Overlay {
    pub color: (vk::Image, vk::ImageView, vk::Format),
    pub depth: (vk::Image, vk::ImageView),
    pub extent: vk::Extent2D,
    pub view_proj: Mat4,
}

/// A program's name as a pass label for GPU timing (leaked once per name).
fn intern(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<(String, &'static str)>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, n)) = names.iter().find(|(k, _)| k == name) {
        return n;
    }
    let n: &'static str = Box::leak(format!("pack {name}").into_boxed_str());
    names.push((name.to_owned(), n));
    n
}

fn layers(aspect: vk::ImageAspectFlags) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        aspect_mask: aspect,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// The game's noise (`packdraw::targets::noise_bytes`).
fn noise_bytes(size: u32) -> Vec<u8> {
    let mut state: u32 = 0x9e37_79b9;
    (0..size * size * 4)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 24) as u8
        })
        .collect()
}

/// The layout the graph leaves sampled images in.
fn sampled_layout(depth: bool) -> vk::ImageLayout {
    if depth {
        vk::ImageLayout::DEPTH_READ_ONLY_OPTIMAL
    } else {
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
    }
}

/// Rebuilds `image`'s mip chain from level 0, each level blitted from the
/// one above; the graph sees it end in TRANSFER_DST.
fn record_mipmaps(device: &ash::Device, graph: &mut Graph, image: &crate::vk::memory::Image) {
    let (raw, levels, extent) = (image.raw, image.levels, image.extent);
    graph.pass("pack mipmaps", &[(raw, Use::TransferDst)], &[], |cmd| unsafe {
        // SAFETY: level barriers and blits within one image the graph put
        // in TRANSFER_DST; every level is back in TRANSFER_DST at the end.
        let level = |l: u32| vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: l,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        let barrier = |l: u32, from: vk::ImageLayout, to: vk::ImageLayout| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE | vk::AccessFlags2::TRANSFER_READ)
                .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE | vk::AccessFlags2::TRANSFER_READ)
                .old_layout(from)
                .new_layout(to)
                .image(raw)
                .subresource_range(level(l))
        };
        let size = |l: u32| vk::Offset3D {
            x: (extent.width >> l).max(1) as i32,
            y: (extent.height >> l).max(1) as i32,
            z: 1,
        };
        let layers = |l: u32| vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: l,
            base_array_layer: 0,
            layer_count: 1,
        };
        let (src, dst) = (vk::ImageLayout::TRANSFER_SRC_OPTIMAL, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
        for l in 1..levels {
            let b = [barrier(l - 1, dst, src)];
            device.cmd_pipeline_barrier2(cmd, &vk::DependencyInfo::default().image_memory_barriers(&b));
            device.cmd_blit_image(
                cmd,
                raw,
                src,
                raw,
                dst,
                &[vk::ImageBlit {
                    src_subresource: layers(l - 1),
                    src_offsets: [vk::Offset3D::default(), size(l - 1)],
                    dst_subresource: layers(l),
                    dst_offsets: [vk::Offset3D::default(), size(l)],
                }],
                vk::Filter::LINEAR,
            );
        }
        let back: Vec<_> = (0..levels.saturating_sub(1)).map(|l| barrier(l, src, dst)).collect();
        if !back.is_empty() {
            device.cmd_pipeline_barrier2(cmd, &vk::DependencyInfo::default().image_memory_barriers(&back));
        }
    });
}
