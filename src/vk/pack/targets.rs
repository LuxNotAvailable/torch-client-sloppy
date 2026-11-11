//! A pack's render targets on the native renderer
//! (VULKAN_REWRITE_CHECKLIST.md 6.2, step 1), as `renderer::packdraw::targets`
//! makes them for wgpu:
//!
//! - each colour target the pack uses (`colortex0` to `colortex15`) as a
//!   pair the passes flip between, at its own size and format;
//! - `depthtex0` (the depth the world writes) and `depthtex1` (the copy
//!   before translucents), and `depthtex2` (before the hand) when asked;
//! - with a shadow pass, `shadowtex0` and `shadowtex1` and the shadow
//!   colour targets it uses, square at the pack's shadow resolution.
//!
//! A format the device cannot draw, blend and filter falls back as the
//! game's does (a snorm or 16-bit unorm to a half float, R11G11B10 to
//! RGBA16F). Mipmapped targets get one level for now; their chains come
//! with the mipmap pass (step 4).

use ash::vk;

use crate::shaderpack::directives::{MAX_COLOR_TARGETS, MAX_SHADOW_COLORS, TargetFormat};
use crate::vk::memory::{Image, Memory};
use crate::vk::{Context, SetupError};

pub(crate) const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
/// Shadow maps above this many texels a side are clamped, as the game's.
const MAX_SHADOW_MAP: u32 = 8192;

/// The Vulkan format a pack's target format names.
pub(crate) fn vk_format(format: TargetFormat) -> vk::Format {
    use TargetFormat as F;
    use vk::Format as V;
    match format {
        F::R16Unorm => V::R16_UNORM,
        F::Rg16Unorm => V::R16G16_UNORM,
        F::Rgba16Unorm => V::R16G16B16A16_UNORM,
        F::R16Snorm => V::R16_SNORM,
        F::Rg16Snorm => V::R16G16_SNORM,
        F::Rgba16Snorm => V::R16G16B16A16_SNORM,
        F::Rg11b10Ufloat => V::B10G11R11_UFLOAT_PACK32,
        F::R8Unorm => V::R8_UNORM,
        F::Rg8Unorm => V::R8G8_UNORM,
        F::Rgba8Unorm => V::R8G8B8A8_UNORM,
        F::R8Snorm => V::R8_SNORM,
        F::Rg8Snorm => V::R8G8_SNORM,
        F::Rgba8Snorm => V::R8G8B8A8_SNORM,
        F::R16Float => V::R16_SFLOAT,
        F::Rg16Float => V::R16G16_SFLOAT,
        F::Rgba16Float => V::R16G16B16A16_SFLOAT,
        F::R32Float => V::R32_SFLOAT,
        F::Rg32Float => V::R32G32_SFLOAT,
        F::Rgba32Float => V::R32G32B32A32_SFLOAT,
        F::Rgb10a2Unorm => V::A2B10G10R10_UNORM_PACK32,
        F::R8Uint => V::R8_UINT,
        F::Rg8Uint => V::R8G8_UINT,
        F::Rgba8Uint => V::R8G8B8A8_UINT,
        F::R8Sint => V::R8_SINT,
        F::Rg8Sint => V::R8G8_SINT,
        F::Rgba8Sint => V::R8G8B8A8_SINT,
        F::R16Uint => V::R16_UINT,
        F::Rg16Uint => V::R16G16_UINT,
        F::Rgba16Uint => V::R16G16B16A16_UINT,
        F::R16Sint => V::R16_SINT,
        F::Rg16Sint => V::R16G16_SINT,
        F::Rgba16Sint => V::R16G16B16A16_SINT,
        F::R32Uint => V::R32_UINT,
        F::Rg32Uint => V::R32G32_UINT,
        F::Rgba32Uint => V::R32G32B32A32_UINT,
        F::R32Sint => V::R32_SINT,
        F::Rg32Sint => V::R32G32_SINT,
        F::Rgba32Sint => V::R32G32B32A32_SINT,
        F::Rgb10a2Uint => V::A2B10G10R10_UINT_PACK32,
    }
}

/// Integer formats are drawn and sampled but never blended or filtered.
fn is_integer(format: vk::Format) -> bool {
    use vk::Format as V;
    matches!(
        format,
        V::R8_UINT
            | V::R8G8_UINT
            | V::R8G8B8A8_UINT
            | V::R8_SINT
            | V::R8G8_SINT
            | V::R8G8B8A8_SINT
            | V::R16_UINT
            | V::R16G16_UINT
            | V::R16G16B16A16_UINT
            | V::R16_SINT
            | V::R16G16_SINT
            | V::R16G16B16A16_SINT
            | V::R32_UINT
            | V::R32G32_UINT
            | V::R32G32B32A32_UINT
            | V::R32_SINT
            | V::R32G32_SINT
            | V::R32G32B32A32_SINT
            | V::A2B10G10R10_UINT_PACK32
    )
}

/// The game's fallbacks for formats a device may not draw with blending
/// and filtering (`packdraw::formats::OPTIONAL_FORMATS`).
fn fallback(format: vk::Format) -> Option<vk::Format> {
    use vk::Format as V;
    Some(match format {
        V::R8_SNORM | V::R16_UNORM | V::R16_SNORM => V::R16_SFLOAT,
        V::R8G8_SNORM | V::R16G16_UNORM | V::R16G16_SNORM => V::R16G16_SFLOAT,
        V::R8G8B8A8_SNORM
        | V::R16G16B16A16_UNORM
        | V::R16G16B16A16_SNORM
        | V::B10G11R11_UFLOAT_PACK32 => V::R16G16B16A16_SFLOAT,
        _ => return None,
    })
}

/// `format` if the device can draw it (and, unless it is an integer
/// format, blend and filter it), else the game's fallback.
pub(crate) fn usable(ctx: &Context, format: vk::Format) -> vk::Format {
    let mut need = vk::FormatFeatureFlags::COLOR_ATTACHMENT | vk::FormatFeatureFlags::SAMPLED_IMAGE;
    if !is_integer(format) {
        need |= vk::FormatFeatureFlags::COLOR_ATTACHMENT_BLEND
            | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR;
    }
    // SAFETY: a query on the physical device the context holds.
    let features = unsafe {
        ctx.instance
            .get_physical_device_format_properties(ctx.physical, format)
            .optimal_tiling_features
    };
    if features.contains(need) {
        format
    } else {
        fallback(format).unwrap_or(format)
    }
}

/// The shadow pass's targets, square at the pack's resolution.
pub(crate) struct ShadowTargets {
    pub size: u32,
    pub depth: Image,
    pub depth_opaque: Image,
    pub color: [Option<Image>; MAX_SHADOW_COLORS],
}

pub(crate) struct PackTargets {
    pub size: vk::Extent2D,
    /// Each used colour target's pair, and its format.
    pub color: [Option<([Image; 2], vk::Format)>; MAX_COLOR_TARGETS],
    pub depth: Image,
    pub depth_opaque: Image,
    pub depth_no_hand: Option<Image>,
    pub shadow: Option<ShadowTargets>,
}

impl PackTargets {
    /// Every target `installed` uses, for a screen of `size`; `no_hand`
    /// when a program samples `depthtex2`.
    pub(crate) fn new(
        ctx: &Context,
        memory: &Memory,
        installed: &crate::renderer::packdraw::install::Installed,
        size: vk::Extent2D,
        no_hand: bool,
    ) -> Result<PackTargets, SetupError> {
        let color_usage = vk::ImageUsageFlags::COLOR_ATTACHMENT
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST;
        let depth_usage = vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST;
        let mut color: [Option<([Image; 2], vk::Format)>; MAX_COLOR_TARGETS] = Default::default();
        for (index, slot) in color.iter_mut().enumerate() {
            if installed.used_targets & (1 << index) == 0 {
                continue;
            }
            let settings = &installed.targets.settings[index];
            let format = usable(ctx, vk_format(settings.format));
            let [width, height] = settings.resolve_size([size.width, size.height]);
            let extent = vk::Extent2D {
                width: width.max(1),
                height: height.max(1),
            };
            let half = |n: usize| {
                memory.image(
                    ctx,
                    &format!("colortex{index}.{n}"),
                    extent,
                    format,
                    color_usage,
                    vk::ImageAspectFlags::COLOR,
                )
            };
            *slot = Some(([half(0)?, half(1)?], format));
        }
        let depth = |name: &str, extent: vk::Extent2D| {
            memory.image(ctx, name, extent, DEPTH_FORMAT, depth_usage, vk::ImageAspectFlags::DEPTH)
        };
        let shadow = match installed.shadow {
            Some(_) => {
                let side = installed.constants.shadow_map_resolution.clamp(1, MAX_SHADOW_MAP);
                let square = vk::Extent2D {
                    width: side,
                    height: side,
                };
                let mut colors: [Option<Image>; MAX_SHADOW_COLORS] = Default::default();
                for (index, slot) in colors.iter_mut().enumerate() {
                    if installed.used_shadow & (1 << index) != 0 {
                        *slot = Some(memory.image(
                            ctx,
                            &format!("shadowcolor{index}"),
                            square,
                            usable(ctx, vk_format(installed.targets.shadow[index].format)),
                            color_usage,
                            vk::ImageAspectFlags::COLOR,
                        )?);
                    }
                }
                Some(ShadowTargets {
                    size: side,
                    depth: depth("shadowtex0", square)?,
                    depth_opaque: depth("shadowtex1", square)?,
                    color: colors,
                })
            }
            None => None,
        };
        Ok(PackTargets {
            size,
            color,
            depth: depth("depthtex0", size)?,
            depth_opaque: depth("depthtex1", size)?,
            depth_no_hand: if no_hand {
                Some(depth("depthtex2", size)?)
            } else {
                None
            },
            shadow,
        })
    }

    pub(crate) fn destroy(self, ctx: &Context, memory: &Memory) {
        let mut images = vec![self.depth, self.depth_opaque];
        images.extend(self.depth_no_hand);
        for (pair, _) in self.color.into_iter().flatten() {
            images.extend(pair);
        }
        if let Some(shadow) = self.shadow {
            images.push(shadow.depth);
            images.push(shadow.depth_opaque);
            images.extend(shadow.color.into_iter().flatten());
        }
        for image in images {
            memory.free_image(ctx, image);
        }
    }
}
