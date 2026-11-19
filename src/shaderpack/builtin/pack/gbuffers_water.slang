// Torch Vibrant: water, glass, ice and the other translucent blocks, lit as
// drawn and blended over the lit scene. Water takes its colour from how deep
// the view looks through it, ripples on the block-texel grid, and leaves its
// normal in colortex3 for the reflection pass.

/* RENDERTARGETS: 0,3 */

#include "lib/common.slang"
#include "lib/shadow.slang"

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
    float2 light : TEXCOORD2;
    float3 player : TEXCOORD3;
    nointerpolation float3 normal : TEXCOORD4;
    nointerpolation float material : TEXCOORD5;
};

struct Lit {
    float4 color : SV_Target0;
    float4 surface : SV_Target1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0)));
    o.uv = a.uv;
    o.color = a.color;
    o.light = clamp(a.light / 240.0, 0.0, 1.0);
    o.player = a.position;
    o.normal = a.normal;
    o.material = material_of(a.entity.x);
    return o;
}

float hash(int2 i) {
    uint h = (asuint(i.x) * 0x8DA6B343u) ^ (asuint(i.y) * 0xD8163841u);
    h = (h ^ (h >> 13u)) * 0x5BD1E995u;
    h ^= h >> 15u;
    return float(h) * (1.0 / 4294967295.0);
}

/// The slope of smooth value noise at `p`, worked out alongside it, so a
/// normal costs one lookup of the four corners instead of three.
float2 noise_slope(float2 p) {
    float2 cell = floor(p);
    float2 f = p - cell;
    float2 u = f * f * (3.0 - 2.0 * f);
    float2 du = 6.0 * f * (1.0 - f);
    int2 i = int2(cell);
    float a = hash(i);
    float b = hash(i + int2(1, 0));
    float c = hash(i + int2(0, 1));
    float d = hash(i + int2(1, 1));
    float k = a - b - c + d;
    return float2(du.x * (b - a + k * u.y), du.y * (c - a + k * u.x));
}

/// How much of a ripple of `wavelength` blocks to keep when one pixel spans
/// `footprint` blocks: all of it from four pixels a wave, none below two.
/// A ripple finer than that cannot be drawn, only aliased into sparkle.
float resolvable(float wavelength, float footprint) {
    return smoothstep(2.0 * footprint, 4.0 * footprint, wavelength);
}

/// The slope of the swell: octaves of noise, each turned and drifting its
/// own way, so the surface never lines up into rows. Octaves too fine for
/// the pixel they land in are left out (`footprint`, blocks per pixel).
float2 wave_slope(float2 p, float t, float footprint) {
    // Rotations, written by rows.
    float2x2 r1 = float2x2(0.8, -0.6, 0.6, 0.8);
    float2x2 r2 = float2x2(0.28, 0.96, -0.96, 0.28);
    float2x2 r12 = mul(r1, r2);
    // Each octave samples q = r * p * scale; its slope in p is r^T * slope(q) * scale,
    // and its wavelength is 1 / scale blocks.
    float2 g = noise_slope(p * 0.32 + float2(t * 0.22, t * 0.05))
        * (0.11 * 0.32 * resolvable(1.0 / 0.32, footprint));
    g += mul(noise_slope(mul(r1, p) * 0.75 + float2(-t * 0.31, t * 0.17)), r1)
        * (0.05 * 0.75 * resolvable(1.0 / 0.75, footprint));
    g += mul(noise_slope(mul(r2, p) * 1.6 + float2(t * 0.12, -t * 0.38)), r2)
        * (0.022 * 1.6 * resolvable(1.0 / 1.6, footprint));
    g += mul(noise_slope(mul(r12, p) * 3.1 + float2(-t * 0.45, -t * 0.2)), r12)
        * (0.01 * 3.1 * resolvable(1.0 / 3.1, footprint));
    return g;
}

/// The ripple's normal, taken at the middle of the block texel `world` is
/// in, so the surface shades in texel-sized facets. Once a texel is smaller
/// than a pixel the facets themselves would alias, so they give way to the
/// unsnapped swell.
float3 wave_normal(float3 world, float footprint) {
    float facets = 1.0 - smoothstep(0.5 / 16.0, 1.5 / 16.0, footprint);
    float2 snapped = (floor(world.xz * 16.0) + 0.5) / 16.0;
    float2 p = lerp(world.xz, snapped, facets);
    float2 slope = wave_slope(p, frame.frameTimeCounter, footprint);
    return normalize(float3(-slope.x, 1.0, -slope.y));
}

float ggx(float3 n, float3 v, float3 l, float roughness) {
    float3 h = normalize(v + l);
    float ndh = max(dot(n, h), 0.0);
    float a2 = roughness * roughness * roughness * roughness;
    float d = ndh * ndh * (a2 - 1.0) + 1.0;
    float ndl = max(dot(n, l), 0.0);
    return a2 / (PI * d * d) * ndl * 0.25;
}

Lit fragment(Varyings v) {
    float4 tex = gtexture.Sample(gtexture_sampler, v.uv) * v.color;
    // Blocks of surface one pixel spans, along its longer side.
    float footprint = max(length(ddx(v.player)), length(ddy(v.player)));
    if (!iris_alpha_pass(tex.a)) {
        discard;
    }
    float3 face = normalize(v.normal);
    float vis = shadow_visibility(v.player, face, 2);
    Lit o;

    if (v.material != MATERIAL_WATER) {
        float3 lit = surface_light(to_linear(tex.rgb), face, v.light, vis, false);
        o.color = float4(lit, tex.a);
        o.surface = float4(oct_encode(face), 0.0, v.light.y);
        return o;
    }

    float3 world = v.player + frame.cameraPosition;
    float3 n = face;
    if (face.y > 0.5) {
        n = wave_normal(world, footprint);
    }
    float3 view_dir = normalize(-v.player);

    // How far the eye looks through the water to what is under it.
    float2 size = float2(iris_size(depthtex1));
    int2 px = int2(v.clip.xy);
    float behind = depthtex1.Load(int3(px, 0)).r;
    float3 floor_pos = player_pos(view_pos(v.clip.xy / size, behind));
    float depth = length(floor_pos) - length(v.player);
    if (behind >= 1.0) {
        depth = 64.0;
    }
    depth = max(depth, 0.0);
    float3 absorbed = exp(-depth * float3(0.2, 0.05, 0.04) / WATER_CLARITY);
    float through = (absorbed.r + absorbed.g + absorbed.b) / 3.0;

    // The biome's water colour, pulled toward a clear teal.
    float3 biome = to_linear(v.color.rgb);
    float3 tint = lerp(float3(luma(biome)), biome, 0.55) * 0.6 + float3(0.02, 0.09, 0.1);
    float sky = v.light.y;
    float3 ambient = sky_ambient() * (sky * sky) + TORCH * pow(v.light.x, 2.6) * TORCH_BRIGHTNESS * 0.5;
    float3 scatter = tint * 0.45 * (ambient + direct_light() * vis * 0.2);
    float3 shine = direct_light() * ggx(n, view_dir, light_dir(), 0.28) * vis * smoothstep(0.05, 0.45, sky) * 0.5;

    // Murkier water starts less see-through and can close up entirely.
    float murk = clamp(1.0 / WATER_CLARITY, 0.5, 4.0);
    float depth_alpha = clamp((1.0 - through) * 0.7, 0.05 * murk, min(0.65 * sqrt(murk), 0.95));
    // The surface's own share covers the rest of the way less what depth
    // already covers, so the two add up without passing opaque.
    float alpha = WATER_SURFACE_OPACITY + (1.0 - WATER_SURFACE_OPACITY) * depth_alpha;
    // Blending scales the colour by alpha; the glint is light off the
    // surface, so undo that, but not without bound for clear water.
    o.color = float4(scatter + min(shine, 2.0) / max(alpha, 0.35), alpha);
    o.surface = float4(oct_encode(n), 1.0, sky);
    return o;
}
