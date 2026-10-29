// Torch Vibrant: water, glass, ice and the other translucent blocks, lit as
// drawn and blended over the lit scene. Water takes its colour from how deep
// the view looks through it, ripples on the block-texel grid, and leaves its
// normal in colortex3 for the reflection pass.

/* RENDERTARGETS: 0,3 */

#include "/lib/common.wgsl"
#include "/lib/shadow.wgsl"

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light: vec2<f32>,
    @location(3) player: vec3<f32>,
    @location(4) @interpolate(flat) normal: vec3<f32>,
    @location(5) @interpolate(flat) material: f32,
}

struct Lit {
    @location(0) color: vec4<f32>,
    @location(1) surface: vec4<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0));
    out.uv = a.uv;
    out.color = a.color;
    out.light = clamp(a.light / 240.0, vec2<f32>(0.0), vec2<f32>(1.0));
    out.player = a.position;
    out.normal = a.normal;
    out.material = material_of(a.entity.x);
    return out;
}

fn hash(i: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(i.x) * 0x8DA6B343u) ^ (bitcast<u32>(i.y) * 0xD8163841u);
    h = (h ^ (h >> 13u)) * 0x5BD1E995u;
    h ^= h >> 15u;
    return f32(h) * (1.0 / 4294967295.0);
}

/// The slope of smooth value noise at `p`, worked out alongside it, so a
/// normal costs one lookup of the four corners instead of three.
fn noise_slope(p: vec2<f32>) -> vec2<f32> {
    let cell = floor(p);
    let f = p - cell;
    let u = f * f * (3.0 - 2.0 * f);
    let du = 6.0 * f * (1.0 - f);
    let i = vec2<i32>(cell);
    let a = hash(i);
    let b = hash(i + vec2<i32>(1, 0));
    let c = hash(i + vec2<i32>(0, 1));
    let d = hash(i + vec2<i32>(1, 1));
    let k = a - b - c + d;
    return vec2<f32>(du.x * (b - a + k * u.y), du.y * (c - a + k * u.x));
}

/// How much of a ripple of `wavelength` blocks to keep when one pixel spans
/// `footprint` blocks: all of it from four pixels a wave, none below two.
/// A ripple finer than that cannot be drawn, only aliased into sparkle.
fn resolvable(wavelength: f32, footprint: f32) -> f32 {
    return smoothstep(2.0 * footprint, 4.0 * footprint, wavelength);
}

/// The slope of the swell: octaves of noise, each turned and drifting its
/// own way, so the surface never lines up into rows. Octaves too fine for
/// the pixel they land in are left out (`footprint`, blocks per pixel).
fn wave_slope(p: vec2<f32>, t: f32, footprint: f32) -> vec2<f32> {
    let r1 = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    let r2 = mat2x2<f32>(0.28, -0.96, 0.96, 0.28);
    let r12 = r1 * r2;
    // Each octave samples q = r * p * scale; its slope in p is r^T * slope(q) * scale,
    // and its wavelength is 1 / scale blocks.
    var g = noise_slope(p * 0.32 + vec2<f32>(t * 0.22, t * 0.05))
        * (0.11 * 0.32 * resolvable(1.0 / 0.32, footprint));
    g += (noise_slope(r1 * p * 0.75 + vec2<f32>(-t * 0.31, t * 0.17)) * r1)
        * (0.05 * 0.75 * resolvable(1.0 / 0.75, footprint));
    g += (noise_slope(r2 * p * 1.6 + vec2<f32>(t * 0.12, -t * 0.38)) * r2)
        * (0.022 * 1.6 * resolvable(1.0 / 1.6, footprint));
    g += (noise_slope(r12 * p * 3.1 + vec2<f32>(-t * 0.45, -t * 0.2)) * r12)
        * (0.01 * 3.1 * resolvable(1.0 / 3.1, footprint));
    return g;
}

/// The ripple's normal, taken at the middle of the block texel `world` is
/// in, so the surface shades in texel-sized facets. Once a texel is smaller
/// than a pixel the facets themselves would alias, so they give way to the
/// unsnapped swell.
fn wave_normal(world: vec3<f32>, footprint: f32) -> vec3<f32> {
    let facets = 1.0 - smoothstep(0.5 / 16.0, 1.5 / 16.0, footprint);
    let snapped = (floor(world.xz * 16.0) + 0.5) / 16.0;
    let p = mix(world.xz, snapped, facets);
    let slope = wave_slope(p, frame.frameTimeCounter, footprint);
    return normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
}

fn ggx(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let h = normalize(v + l);
    let ndh = max(dot(n, h), 0.0);
    let a2 = roughness * roughness * roughness * roughness;
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    let ndl = max(dot(n, l), 0.0);
    return a2 / (PI * d * d) * ndl * 0.25;
}

@fragment
fn fragment(v: Varyings) -> Lit {
    let tex = textureSample(gtexture, gtexture_sampler, v.uv) * v.color;
    // Blocks of surface one pixel spans, along its longer side.
    let footprint = max(length(dpdx(v.player)), length(dpdy(v.player)));
    if (!iris_alpha_pass(tex.a)) {
        discard;
    }
    let face = normalize(v.normal);
    let vis = shadow_visibility(v.player, face, 2u);
    var out: Lit;

    if (v.material != MATERIAL_WATER) {
        let lit = surface_light(to_linear(tex.rgb), face, v.light, vis, false);
        out.color = vec4<f32>(lit, tex.a);
        out.surface = vec4<f32>(oct_encode(face), 0.0, v.light.y);
        return out;
    }

    let world = v.player + frame.cameraPosition;
    var n = face;
    if (face.y > 0.5) {
        n = wave_normal(world, footprint);
    }
    let view_dir = normalize(-v.player);

    // How far the eye looks through the water to what is under it.
    let size = vec2<f32>(textureDimensions(depthtex1));
    let px = vec2<i32>(v.clip.xy);
    let behind = textureLoad(depthtex1, px, 0).r;
    let floor_pos = player_pos(view_pos(v.clip.xy / size, behind));
    var depth = length(floor_pos) - length(v.player);
    if (behind >= 1.0) {
        depth = 64.0;
    }
    depth = max(depth, 0.0);
    let absorbed = exp(-depth * vec3<f32>(0.2, 0.05, 0.04));
    let through = (absorbed.r + absorbed.g + absorbed.b) / 3.0;

    // The biome's water colour, pulled toward a clear teal.
    let biome = to_linear(v.color.rgb);
    let tint = mix(vec3<f32>(luma(biome)), biome, 0.55) * 0.6 + vec3<f32>(0.02, 0.09, 0.1);
    let sky = v.light.y;
    let ambient = sky_ambient() * (sky * sky) + TORCH * pow(v.light.x, 2.6) * TORCH_BRIGHTNESS * 0.5;
    let scatter = tint * 0.45 * (ambient + direct_light() * vis * 0.2);
    let shine = direct_light() * ggx(n, view_dir, light_dir(), 0.28) * vis * smoothstep(0.05, 0.45, sky) * 0.5;

    let alpha = clamp((1.0 - through) * 0.7, 0.05, 0.65);
    // Blending scales the colour by alpha; the glint is light off the
    // surface, so undo that, but not without bound for clear water.
    out.color = vec4<f32>(scatter + min(shine, vec3<f32>(2.0)) / max(alpha, 0.35), alpha);
    out.surface = vec4<f32>(oct_encode(n), 1.0, sky);
    return out;
}
