use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;

use super::{ATLAS_COLS, TILE_PX};

static OPAQUE_TEXTURES: OnceLock<HashSet<String>> = OnceLock::new();

pub fn texture_is_opaque(stem: &str) -> bool {
    OPAQUE_TEXTURES.get().is_some_and(|set| set.contains(stem))
}

fn is_fully_opaque(rgba: &image::RgbaImage) -> bool {
    rgba.pixels().all(|p| p[3] == 255)
}

/// The block atlas's missing-texture tile, which modded blocks wear
/// (`util::block_model::modded_visual`).
pub const MISSING_TEXTURE: &str = "missingno";

/// Vanilla's missing texture: magenta and black in 8-pixel squares.
fn missing_texture() -> image::RgbaImage {
    image::RgbaImage::from_fn(TILE_PX, TILE_PX, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            image::Rgba([248, 0, 248, 255])
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    })
}

pub struct Textures {
    pub block: BTreeMap<String, image::RgbaImage>,
}

impl Textures {
    pub fn blocks(textures_dir: &str) -> Textures {
        let mut block = decode_dir(&format!("{textures_dir}/block"));
        if !block.is_empty() {
            block
                .entry(MISSING_TEXTURE.to_owned())
                .or_insert_with(missing_texture);
        }
        Textures { block }
    }
}

fn decode_dir(dir: &str) -> BTreeMap<String, image::RgbaImage> {
    let paths: Vec<std::path::PathBuf> = crate::platform::assets::read_dir(dir)
        .into_iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
        .collect();

    let decode = |path: &std::path::Path| -> Option<(String, image::RgbaImage)> {
        let stem = path.file_stem()?.to_str()?.to_owned();
        let bytes = crate::platform::assets::read(path)?;
        let decoded = image::load_from_memory(&bytes).ok()?;
        Some((stem, decoded.to_rgba8()))
    };

    #[cfg(target_arch = "wasm32")]
    {
        paths
            .iter()
            .filter_map(|p| decode(p))
            .collect::<BTreeMap<String, image::RgbaImage>>()
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 12);
        let chunk = paths.len().div_ceil(threads).max(1);
        let decode = &decode;
        std::thread::scope(|scope| {
            let handles: Vec<_> = paths
                .chunks(chunk)
                .map(|slice| {
                    scope.spawn(move || {
                        slice
                            .iter()
                            .filter_map(|p| decode(p))
                            .collect::<BTreeMap<String, image::RgbaImage>>()
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).fold(
                BTreeMap::<String, image::RgbaImage>::new(),
                |mut acc, part| {
                    acc.extend(part);
                    acc
                },
            )
        })
    }
}

#[cfg(feature = "shader_support")]
pub(crate) type UntintedTiles = Vec<(u32, Vec<Vec<u8>>)>;

#[cfg(feature = "shader_support")]
static UNTINTED: std::sync::RwLock<Option<std::sync::Arc<UntintedTiles>>> =
    std::sync::RwLock::new(None);

#[cfg(feature = "shader_support")]
pub(crate) fn untinted_tiles() -> Option<std::sync::Arc<UntintedTiles>> {
    UNTINTED.read().ok().and_then(|t| t.clone())
}

#[cfg(feature = "shader_support")]
fn publish_untinted(
    entries: &[(&str, &image::RgbaImage)],
    biome_tinted: &[&str],
    tiles: &HashMap<String, u32>,
    n_mips: u32,
    biases: &[f32],
) {
    let (level0, atlas_w, atlas_h, _) = pack_tiles(entries);
    let tile_px = atlas_w / ATLAS_COLS;
    let chain = if n_mips > 1 {
        build_mip_chain(&level0, atlas_w, atlas_h, n_mips, biases)
    } else {
        level0
    };
    let mut out: UntintedTiles = Vec::with_capacity(biome_tinted.len());
    for stem in biome_tinted {
        let Some(&tile) = tiles.get(*stem) else {
            continue;
        };
        let (tx, ty) = (tile % ATLAS_COLS, tile / ATLAS_COLS);
        let mut offset = 0usize;
        let mut levels = Vec::with_capacity(n_mips as usize);
        for level in 0..n_mips {
            let (lw, lh, ts) = (atlas_w >> level, atlas_h >> level, tile_px >> level);
            let mut bytes = Vec::with_capacity((ts * ts * 4) as usize);
            for row in 0..ts {
                let start = offset + (((ty * ts + row) * lw + tx * ts) * 4) as usize;
                bytes.extend_from_slice(&chain[start..start + (ts * 4) as usize]);
            }
            levels.push(bytes);
            offset += (lw * lh * 4) as usize;
        }
        out.push((tile, levels));
    }
    if let Ok(mut slot) = UNTINTED.write() {
        *slot = Some(std::sync::Arc::new(out));
    }
}

pub fn build_block_atlas(textures: &Textures, textures_dir: &str) -> (Image, HashMap<String, u32>) {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let colormap_dir = format!("{}/colormap", textures_dir);

    crate::util::biome_color::init(&colormap_dir);
    let grass_tint = crate::util::biome_color::plains_grass();
    let foliage_tint = crate::util::biome_color::plains_foliage();
    let dry_foliage_tint = crate::util::biome_color::plains_dry_foliage();

    if textures.block.is_empty() {
        eprintln!("Cannot read block texture dir {textures_dir}/block");
        return (placeholder_atlas(), HashMap::new());
    }
    let mut opaque: HashSet<String> = HashSet::new();
    let mut recoloured: BTreeMap<&str, image::RgbaImage> = BTreeMap::new();
    #[cfg(feature = "shader_support")]
    let mut biome_tinted: Vec<&str> = Vec::new();
    for (stem, rgba) in &textures.block {
        if stem == "grass_block_side_overlay" {
            continue;
        }
        if let Some((new, biome)) = tinted(
            rgba,
            stem,
            grass_tint,
            foliage_tint,
            dry_foliage_tint,
            &textures.block,
        ) {
            recoloured.insert(stem.as_str(), new);
            #[cfg(feature = "shader_support")]
            if biome {
                biome_tinted.push(stem.as_str());
            }
            #[cfg(not(feature = "shader_support"))]
            let _ = biome;
        }
    }
    let mut entries: Vec<(&str, &image::RgbaImage)> = Vec::with_capacity(textures.block.len());
    for (stem, rgba) in &textures.block {
        if stem == "grass_block_side_overlay" {
            continue;
        }
        let rgba = recoloured.get(stem.as_str()).unwrap_or(rgba);
        if is_fully_opaque(rgba) {
            opaque.insert(stem.clone());
        }
        entries.push((stem.as_str(), rgba));
    }
    let opaque_count = opaque.len();
    OPAQUE_TEXTURES.set(opaque).ok();

    let (frames, strips) = animation_frames(&entries, textures_dir);
    let base_count = entries.len();
    let mut packed = entries.clone();
    packed.extend(frames.iter().map(|f| (f.name.as_str(), &f.image)));
    let (atlas_data, atlas_w, atlas_h, mut tex_to_tile) = pack_tiles(&packed);
    let biases = cutout_biases(
        &packed.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        textures_dir,
    );
    publish_animations(&frames, strips, &mut tex_to_tile, base_count + 1);
    let entity_textures = crate::blockentities::bake::textures(textures_dir);
    let (atlas_data, atlas_h, regions) =
        append_regions(atlas_data, atlas_w, atlas_h, &entity_textures);
    crate::blockentities::bake::set_regions(regions);
    #[cfg(feature = "shader_support")]
    let untinted_entries: Vec<(&str, &image::RgbaImage)> = entries
        .iter()
        .map(|(stem, rgba)| match textures.block.get(*stem) {
            Some(source) if biome_tinted.contains(stem) => (*stem, source),
            _ => (*stem, *rgba),
        })
        .collect();

    let n_mips = (atlas_w / ATLAS_COLS).trailing_zeros() + 1;
    let mip_data = if n_mips > 1 {
        build_mip_chain(&atlas_data, atlas_w, atlas_h, n_mips, &biases)
    } else {
        atlas_data
    };
    #[cfg(feature = "shader_support")]
    publish_untinted(&untinted_entries, &biome_tinted, &tex_to_tile, n_mips, &biases);

    let mut image = Image::new_uninit(
        Extent3d {
            width: atlas_w,
            height: atlas_h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = n_mips;
    image.data = Some(mip_data);
    image.sampler = block_atlas_sampler();
    #[cfg(feature = "shader_support")]
    super::packdraw::allow_display_view(&mut image);
    println!(
        "[Atlas] {} block textures loaded ({} fully opaque), atlas {}×{}, {} mip levels",
        entries.len(),
        opaque_count,
        atlas_w,
        atlas_h,
        n_mips
    );
    (image, tex_to_tile)
}

struct AnimationFrame {
    /// `<stem>#<frame index>`, only ever a key while packing.
    name: String,
    stem: String,
    image: image::RgbaImage,
}

struct AnimatedStrip {
    stem: String,
    meta: super::block_anim::Meta,
}

/// Crops every frame but the first out of each animated strip. The first is
/// what `blit_tile` already copies into the texture's own tile. The strips
/// come back with the frames, so builds running at once never share them.
fn animation_frames(
    entries: &[(&str, &image::RgbaImage)],
    textures_dir: &str,
) -> (Vec<AnimationFrame>, Vec<AnimatedStrip>) {
    let mut frames = Vec::new();
    let mut strips = Vec::new();
    for (stem, rgba) in entries {
        let side = rgba.width();
        if side == 0 || rgba.height() < side * 2 {
            continue;
        }
        let count = rgba.height() / side;
        let png = std::path::Path::new(textures_dir)
            .join("block")
            .join(format!("{stem}.png"));
        let Some(meta) = super::block_anim::read_meta(&png, count) else {
            continue;
        };
        let mut seen = std::collections::BTreeSet::new();
        for &(index, _) in &meta.frames {
            if index == 0 || !seen.insert(index) {
                continue;
            }
            let image = image::imageops::crop_imm(*rgba, 0, index * side, side, side).to_image();
            frames.push(AnimationFrame {
                name: format!("{stem}#{index}"),
                stem: (*stem).to_owned(),
                image,
            });
        }
        strips.push(AnimatedStrip {
            stem: (*stem).to_owned(),
            meta,
        });
    }
    (frames, strips)
}

/// Takes the frame tiles back out of the texture map, which only names real
/// textures, and hands the playback order to the shader's remap table.
fn publish_animations(
    frames: &[AnimationFrame],
    strips: Vec<AnimatedStrip>,
    tiles: &mut HashMap<String, u32>,
    tile_count: usize,
) {
    let mut frame_tiles: HashMap<(&str, u32), u32> = HashMap::new();
    for frame in frames {
        if let Some(tile) = tiles.remove(&frame.name) {
            let index = frame
                .name
                .rsplit('#')
                .next()
                .and_then(|i| i.parse().ok())
                .unwrap_or(0);
            frame_tiles.insert((frame.stem.as_str(), index), tile);
        }
    }
    let mut animations = Vec::with_capacity(strips.len());
    for strip in &strips {
        let Some(&base) = tiles.get(&strip.stem) else {
            continue;
        };
        let resolve = |index: u32| {
            if index == 0 {
                Some(base)
            } else {
                frame_tiles.get(&(strip.stem.as_str(), index)).copied()
            }
        };
        let frames: Option<Vec<(u16, u32)>> = strip
            .meta
            .frames
            .iter()
            .map(|&(index, time)| Some((u16::try_from(resolve(index)?).ok()?, time)))
            .collect();
        let (Some(frames), Ok(base)) = (frames, u16::try_from(base)) else {
            continue;
        };
        animations.push(super::block_anim::BlockAnimation {
            base,
            frames: frames.into_boxed_slice(),
            interpolate: strip.meta.interpolate,
        });
    }
    let total = tile_count + frames.len();
    super::block_anim::publish(total as u32, animations);
}

fn block_atlas_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("block_atlas".into()),
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    })
}

/// Each tile's `alpha_cutoff_bias` from its `.mcmeta` (kelp's is 0.1), by
/// atlas tile: tile 0 is the placeholder, `names[i]` is tile `i + 1`. An
/// animation frame (`stem#n`) takes its texture's.
fn cutout_biases(names: &[&str], textures_dir: &str) -> Vec<f32> {
    let mut by_stem: HashMap<&str, f32> = HashMap::new();
    let mut out = Vec::with_capacity(names.len() + 1);
    out.push(0.0);
    for name in names {
        let stem = name.split('#').next().unwrap_or(name);
        let bias = *by_stem.entry(stem).or_insert_with(|| {
            let meta = std::path::Path::new(textures_dir)
                .join("block")
                .join(format!("{stem}.png.mcmeta"));
            crate::platform::assets::read_to_string(meta)
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|json| json.get("texture")?.get("alpha_cutoff_bias")?.as_f64())
                .unwrap_or(0.0) as f32
        });
        out.push(bias);
    }
    out
}

/// Vanilla's `MipmapGenerator.alphaTestCoverage`: the share of a tile that
/// passes an alpha test at `alpha_ref`, bilinearly subsampled 4x4 between
/// each 2x2 of texels, its alpha scaled by `scale`.
fn alpha_coverage(alpha: &[f32], side: usize, alpha_ref: f32, scale: f32) -> f32 {
    if side < 2 {
        return f32::NAN;
    }
    let at = |x: usize, y: usize| (alpha[y * side + x] * scale).clamp(0.0, 1.0);
    let mut coverage = 0.0;
    for y in 0..side - 1 {
        for x in 0..side - 1 {
            let (a00, a10, a01, a11) = (at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1));
            let mut hits = 0.0;
            for sy in 0..4 {
                let fy = (sy as f32 + 0.5) / 4.0;
                for sx in 0..4 {
                    let fx = (sx as f32 + 0.5) / 4.0;
                    let a = a00 * (1.0 - fx) * (1.0 - fy)
                        + a10 * fx * (1.0 - fy)
                        + a01 * (1.0 - fx) * fy
                        + a11 * fx * fy;
                    if a > alpha_ref {
                        hits += 1.0;
                    }
                }
            }
            coverage += hits / 16.0;
        }
    }
    coverage / ((side - 1) * (side - 1)) as f32
}

/// Vanilla's `scaleAlphaToCoverage`: finds the alpha scale (a five-step
/// search) that keeps `desired` coverage at `alpha_ref`, then scales alpha by
/// it and adds `bias`, so a cutout texture keeps its shape at a distance
/// rather than thinning away mip by mip.
fn scale_alpha_to_coverage(alpha: &mut [f32], side: usize, desired: f32, alpha_ref: f32, bias: f32) {
    let (mut lo, mut hi, mut scale, mut best, mut best_error) = (0.0f32, 4.0f32, 1.0f32, 1.0f32, f32::MAX);
    for _ in 0..5 {
        let coverage = alpha_coverage(alpha, side, alpha_ref, scale);
        let error = (coverage - desired).abs();
        if error < best_error {
            best_error = error;
            best = scale;
        }
        if coverage < desired {
            lo = scale;
        } else if coverage > desired {
            hi = scale;
        } else {
            break;
        }
        scale = (lo + hi) * 0.5;
    }
    // Vanilla adds the bias (and 0.025) to every texel, which is harmless at
    // its own 0.5 cutoff; a shader pack tests at 0.1 (Iris), where that lifts
    // a cutout's empty texels over the test and a far kelp draws as a solid
    // tile. So the bias lifts only texels the cutout already covers (5% or
    // more); empty and barely-touched ones keep their scaled alpha.
    const COVERED: f32 = 0.05;
    for a in alpha.iter_mut() {
        let lift = if *a >= COVERED { bias + 0.025 } else { 0.0 };
        *a = (*a * best + lift).clamp(0.0, 1.0);
    }
}

/// The atlas's mip levels. Each halves the last, colour weighted by alpha.
/// A tile with fully transparent texels is cutout (vanilla's `AUTO`
/// strategy) and keeps its alpha-test coverage at every level, with its
/// `alpha_cutoff_bias` from `biases` (by tile; past its end, none).
fn build_mip_chain(
    level0: &[u8],
    atlas_w: u32,
    atlas_h: u32,
    n_mips: u32,
    biases: &[f32],
) -> Vec<u8> {
    const CUTOUT_REF: f32 = 0.5;
    let mut out = Vec::with_capacity(level0.len() * 2);
    out.extend_from_slice(level0);

    let tile_px = (atlas_w / ATLAS_COLS) as usize;
    let tiles_y = atlas_h as usize / tile_px.max(1);
    let tile_alpha = |data: &[u8], width: usize, side: usize, tx: usize, ty: usize| -> Vec<f32> {
        let mut alpha = Vec::with_capacity(side * side);
        for y in 0..side {
            for x in 0..side {
                let i = ((ty * side + y) * width + tx * side + x) * 4 + 3;
                alpha.push(data[i] as f32 / 255.0);
            }
        }
        alpha
    };
    // Which tiles are cutout, and their full-size coverage.
    let mut cutout: Vec<Option<(f32, f32)>> = vec![None; tiles_y * ATLAS_COLS as usize];
    for (tile, slot) in cutout.iter_mut().enumerate() {
        let Some(&bias) = biases.get(tile) else {
            break;
        };
        let (tx, ty) = (tile % ATLAS_COLS as usize, tile / ATLAS_COLS as usize);
        let alpha = tile_alpha(level0, atlas_w as usize, tile_px, tx, ty);
        if alpha.iter().any(|a| *a == 0.0) {
            *slot = Some((alpha_coverage(&alpha, tile_px, CUTOUT_REF, 1.0), bias));
        }
    }

    let mut last = level0.to_vec();
    let (mut lw, mut lh) = (atlas_w as usize, atlas_h as usize);
    for _ in 1..n_mips {
        let (w, h) = (lw / 2, lh / 2);
        let mut buf = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let (mut sr, mut sg, mut sb, mut sa) = (0u32, 0u32, 0u32, 0u32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let si = ((y * 2 + dy) * lw + x * 2 + dx) * 4;
                    let a = last[si + 3] as u32;
                    sr += last[si] as u32 * a;
                    sg += last[si + 1] as u32 * a;
                    sb += last[si + 2] as u32 * a;
                    sa += a;
                }
                let di = (y * w + x) * 4;
                if sa > 0 {
                    buf[di] = (sr / sa) as u8;
                    buf[di + 1] = (sg / sa) as u8;
                    buf[di + 2] = (sb / sa) as u8;
                }
                buf[di + 3] = (sa / 4) as u8;
            }
        }
        let side = (tile_px * w) / atlas_w as usize;
        if side > 0 {
            for (tile, entry) in cutout.iter().enumerate() {
                let Some((coverage, bias)) = *entry else {
                    continue;
                };
                let (tx, ty) = (tile % ATLAS_COLS as usize, tile / ATLAS_COLS as usize);
                let mut alpha = tile_alpha(&buf, w, side, tx, ty);
                scale_alpha_to_coverage(&mut alpha, side, coverage, CUTOUT_REF, bias);
                for yy in 0..side {
                    for xx in 0..side {
                        let i = ((ty * side + yy) * w + tx * side + xx) * 4 + 3;
                        buf[i] = (alpha[yy * side + xx] * 255.0).round() as u8;
                    }
                }
            }
        }
        out.extend_from_slice(&buf);
        last = buf;
        (lw, lh) = (w, h);
    }
    out
}

const FIXED_TINTED: &[(&str, [u8; 4])] = &[
    ("spruce_leaves", [0x61, 0x99, 0x61, 255]),
    ("birch_leaves", [0x80, 0xa7, 0x55, 255]),
    ("lily_pad", [0x20, 0x80, 0x30, 255]),
];

fn tinted(
    src: &image::RgbaImage,
    stem: &str,
    grass_tint: [u8; 4],
    foliage_tint: [u8; 4],
    dry_foliage_tint: [u8; 4],
    block: &BTreeMap<String, image::RgbaImage>,
) -> Option<(image::RgbaImage, bool)> {
    const GRASS_TINTED: &[&str] = &[
        "grass_block_top",
        "short_grass",
        "fern",
        "tall_grass",
        "tall_grass_top",
        "tall_grass_bottom",
        "large_fern",
        "large_fern_top",
        "large_fern_bottom",
        "sugar_cane",
        "bush",
        "pink_petals_stem",
        "wildflowers_stem",
    ];
    const FOLIAGE_TINTED: &[&str] = &[
        "oak_leaves",
        "jungle_leaves",
        "acacia_leaves",
        "dark_oak_leaves",
        "mangrove_leaves",
        "vine",
    ];
    const UNTINTED_LEAVES: &[&str] = &[
        "pale_oak_leaves",
        "cherry_leaves",
        "azalea_leaves",
        "flowering_azalea_leaves",
        "red_poplar_leaves",
        "orange_poplar_leaves",
        "yellow_poplar_leaves",
    ];

    let flat = if stem == "leaf_litter" {
        Some(dry_foliage_tint)
    } else if let Some((_, fixed)) = FIXED_TINTED.iter().find(|(s, _)| *s == stem) {
        Some(*fixed)
    } else if UNTINTED_LEAVES.contains(&stem) {
        None
    } else if GRASS_TINTED.contains(&stem) {
        Some(grass_tint)
    } else if stem == "water_still" || stem == "water_flow" {
        Some([0x3F, 0x76, 0xE4, 0xFF])
    } else if FOLIAGE_TINTED.contains(&stem) || stem.ends_with("_leaves") {
        Some(foliage_tint)
    } else {
        None
    };
    if let Some(tint) = flat {
        let mut rgba = src.clone();
        tint_rgba(&mut rgba, tint);
        let biome = !FIXED_TINTED.iter().any(|(s, _)| *s == stem);
        return Some((rgba, biome));
    }
    if stem == "grass_block_side" {
        let overlay = block.get("grass_block_side_overlay")?;
        let mut overlay = overlay.clone();
        tint_rgba(&mut overlay, grass_tint);
        let mut rgba = src.clone();
        composite_over(&mut rgba, &overlay);
        return Some((rgba, false));
    }

    None
}

fn tint_rgba(img: &mut image::RgbaImage, tint: [u8; 4]) {
    let (tr, tg, tb) = (tint[0] as u32, tint[1] as u32, tint[2] as u32);
    for px in img.pixels_mut() {
        if px[3] > 0 {
            px[0] = ((px[0] as u32 * tr) / 255) as u8;
            px[1] = ((px[1] as u32 * tg) / 255) as u8;
            px[2] = ((px[2] as u32 * tb) / 255) as u8;
        }
    }
}

fn composite_over(base: &mut image::RgbaImage, overlay: &image::RgbaImage) {
    let (w, h) = base.dimensions();
    let (ow, oh) = overlay.dimensions();
    if ow == 0 || oh == 0 {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let s = overlay.get_pixel(x * ow / w, y * oh / h);
            if s[3] == 0 {
                continue;
            }
            let d = base.get_pixel_mut(x, y);
            let sa = s[3] as u32;
            let da = 255 - sa;
            d[0] = ((s[0] as u32 * sa + d[0] as u32 * da) / 255) as u8;
            d[1] = ((s[1] as u32 * sa + d[1] as u32 * da) / 255) as u8;
            d[2] = ((s[2] as u32 * sa + d[2] as u32 * da) / 255) as u8;
            d[3] = (sa + d[3] as u32 * da / 255).min(255) as u8;
        }
    }
}

fn blit_tile(atlas: &mut [u8], tile: u32, atlas_w: u32, rgba: &image::RgbaImage) {
    let tile_px = atlas_w / ATLAS_COLS;
    let col = tile % ATLAS_COLS;
    let row = tile / ATLAS_COLS;
    let (ox, oy) = (col * tile_px, row * tile_px);
    let frame = rgba.width().min(rgba.height());
    for py in 0..tile_px {
        for px in 0..tile_px {
            let sx = (px * frame / tile_px).min(rgba.width() - 1);
            let sy = (py * frame / tile_px).min(rgba.height() - 1);
            let p = rgba.get_pixel(sx, sy);
            let dst = (((oy + py) * atlas_w + (ox + px)) * 4) as usize;
            atlas[dst..dst + 4].copy_from_slice(&p.0);
        }
    }
}

fn blit_gray_placeholder(atlas: &mut [u8], tile: u32, atlas_w: u32, tile_px: u32) {
    let col = tile % ATLAS_COLS;
    let row = tile / ATLAS_COLS;
    let (ox, oy) = (col * tile_px, row * tile_px);
    let texel = tile_px / TILE_PX;
    for py in 0..tile_px {
        for px in 0..tile_px {
            let edge = px < texel || py < texel || px >= tile_px - texel || py >= tile_px - texel;
            let checker = ((px / (4 * texel)) + (py / (4 * texel))) % 2 == 0;
            let v: u8 = if edge {
                90
            } else if checker {
                150
            } else {
                165
            };
            let dst = (((oy + py) * atlas_w + (ox + px)) * 4) as usize;
            atlas[dst] = v;
            atlas[dst + 1] = v;
            atlas[dst + 2] = v;
            atlas[dst + 3] = 255;
        }
    }
}

pub fn placeholder_atlas() -> Image {
    #[cfg(feature = "shader_support")]
    if let Ok(mut slot) = UNTINTED.write() {
        *slot = None;
    }
    let mut data = vec![0u8; (TILE_PX * TILE_PX * 4) as usize];
    blit_gray_placeholder(&mut data, 0, TILE_PX, TILE_PX);
    #[cfg_attr(not(feature = "shader_support"), allow(unused_mut))]
    let mut image = super::rgba_image(TILE_PX, TILE_PX, data, RenderAssetUsages::RENDER_WORLD);
    #[cfg(feature = "shader_support")]
    super::packdraw::allow_display_view(&mut image);
    image
}

const MAX_TILE_PX: u32 = 64;

/// The largest tile a texture bigger than the rest can grow every tile to.
/// Vanilla's signs, shelves and dried ghast tentacles are 32 px, two texels
/// per model unit; squeezed into 16 px tiles they lose every other texel.
/// The taller atlas this makes (about 5000 px) is past what web and Android
/// GPUs allow, so they keep the common size.
const MAX_DETAIL_TILE_PX: u32 = if cfg!(any(target_arch = "wasm32", target_os = "android")) {
    TILE_PX
} else {
    32
};

/// The tile size: the most common texture size, or larger to keep the
/// bigger textures whole.
fn tile_px(entries: &[(&str, &image::RgbaImage)]) -> u32 {
    let mut counts: HashMap<u32, u32> = HashMap::new();
    let mut largest = TILE_PX;
    for (_, rgba) in entries {
        let side = rgba.width().min(rgba.height()).next_power_of_two();
        *counts.entry(side.clamp(TILE_PX, MAX_TILE_PX)).or_default() += 1;
        largest = largest.max(side.min(MAX_DETAIL_TILE_PX));
    }
    let common = counts
        .into_iter()
        .max_by_key(|&(side, count)| (count, side))
        .map_or(TILE_PX, |(side, _)| side);
    common.max(largest)
}

/// Tiles per side of a packed entity texture.
const REGION_TILES: u32 = 4;

/// Packs square entity textures as `REGION_TILES`-square blocks of tiles in new
/// rows under the block tiles, so a model can map its whole sheet linearly.
/// Returns the grown atlas, its height and each texture's UV rect.
fn append_regions(
    mut data: Vec<u8>,
    atlas_w: u32,
    atlas_h: u32,
    textures: &[(String, image::RgbaImage)],
) -> (Vec<u8>, u32, HashMap<String, [f32; 4]>) {
    if textures.is_empty() {
        return (data, atlas_h, HashMap::new());
    }
    let tile_px = atlas_w / ATLAS_COLS;
    let side = REGION_TILES * tile_px;
    let per_row = ATLAS_COLS / REGION_TILES;
    let band_rows = (textures.len() as u32).div_ceil(per_row);
    let new_h = atlas_h + band_rows * side;
    data.resize((atlas_w * new_h * 4) as usize, 0);

    let mut rects = HashMap::new();
    for (i, (name, image)) in textures.iter().enumerate() {
        let (col, row) = (i as u32 % per_row, i as u32 / per_row);
        let (ox, oy) = (col * side, atlas_h + row * side);
        let scaled;
        let image = if image.width() == side {
            image
        } else {
            scaled =
                image::imageops::resize(image, side, side, image::imageops::FilterType::Nearest);
            &scaled
        };
        for (y, line) in image.rows().enumerate() {
            let dst = (((oy + y as u32) * atlas_w + ox) * 4) as usize;
            for (x, p) in line.enumerate() {
                data[dst + x * 4..dst + x * 4 + 4].copy_from_slice(&p.0);
            }
        }
        let (w, h) = (atlas_w as f32, new_h as f32);
        rects.insert(
            name.clone(),
            [
                ox as f32 / w,
                oy as f32 / h,
                (ox + side) as f32 / w,
                (oy + side) as f32 / h,
            ],
        );
    }
    (data, new_h, rects)
}

fn pack_tiles(entries: &[(&str, &image::RgbaImage)]) -> (Vec<u8>, u32, u32, HashMap<String, u32>) {
    let tile_px = tile_px(entries);
    let n_tiles = 1 + entries.len() as u32;
    let atlas_w = ATLAS_COLS * tile_px;
    let atlas_h = n_tiles.div_ceil(ATLAS_COLS) * tile_px;

    let mut data = vec![0u8; (atlas_w * atlas_h * 4) as usize];
    blit_gray_placeholder(&mut data, 0, atlas_w, tile_px);

    let mut tiles: HashMap<String, u32> = HashMap::new();
    for (idx, (stem, rgba)) in entries.iter().enumerate() {
        let tile = idx as u32 + 1;
        blit_tile(&mut data, tile, atlas_w, rgba);
        tiles.insert((*stem).to_owned(), tile);
    }

    (data, atlas_w, atlas_h, tiles)
}

#[cfg(test)]
mod mip_tests {
    use super::*;

    /// A one-row atlas of 16-pixel tiles: tile 1 is a cutout with a single
    /// opaque column (kelp-thin), the rest transparent.
    fn thin_line_atlas() -> (Vec<u8>, u32, u32) {
        let (w, h) = (ATLAS_COLS * 16, 16);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..16 {
            let i = ((y * w + 16 + 7) * 4) as usize;
            data[i..i + 4].copy_from_slice(&[40, 120, 40, 255]);
        }
        (data, w, h)
    }

    fn max_alpha_of_tile_1(chain: &[u8], w: u32, h: u32, level: u32) -> u8 {
        let mut offset = 0usize;
        let (mut lw, mut lh) = (w as usize, h as usize);
        for _ in 0..level {
            offset += lw * lh * 4;
            (lw, lh) = (lw / 2, lh / 2);
        }
        let side = 16 >> level;
        (0..side)
            .flat_map(|y| (0..side).map(move |x| (x, y)))
            .map(|(x, y)| chain[offset + (y * lw + side + x) * 4 + 3])
            .max()
            .unwrap()
    }

    #[test]
    fn a_thin_cutout_keeps_passing_the_alpha_test_at_a_distance() {
        let (data, w, h) = thin_line_atlas();
        let averaged = build_mip_chain(&data, w, h, 4, &[]);
        // Kelp's `alpha_cutoff_bias`.
        let kept = build_mip_chain(&data, w, h, 4, &[0.0, 0.1]);
        // Averaged, a one-texel line fades toward a pack's 0.1 alpha test
        // (Iris's) by the third level, where it flickers in and out as the
        // sampled level shifts; kept, every level stays well clear of it.
        assert!(max_alpha_of_tile_1(&averaged, w, h, 3) < 40);
        for level in 1..4 {
            let kept = max_alpha_of_tile_1(&kept, w, h, level);
            assert!(kept >= 50, "level {level}: {kept}");
            assert!(kept > max_alpha_of_tile_1(&averaged, w, h, level));
        }
    }
}

#[cfg(test)]
mod mip_bias_tests {
    use super::*;

    #[test]
    fn empty_texels_stay_under_a_packs_alpha_test_whatever_the_bias() {
        let mut alpha = vec![0.0, 0.0, 0.02, 0.6];
        scale_alpha_to_coverage(&mut alpha, 2, 0.3, 0.5, 0.1);
        assert_eq!(alpha[0], 0.0);
        assert!(alpha[2] < 0.1, "{:?}", alpha);
        assert!(alpha[3] > 0.6, "{:?}", alpha);
    }
}
