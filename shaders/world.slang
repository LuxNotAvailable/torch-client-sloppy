// Lighting and fog shared by everything in the world (terrain, entities,
// clouds; VULKAN_REWRITE_CHECKLIST.md 5.1): the lightmap the game computes
// each frame, and vanilla's fog ramps. Both read the frame's view buffer
// (`CullView` in `src/vk/terrain.rs`).

import common;
import cull;

float3 srgb_encode(float3 c) {
    float3 low = c * 12.92;
    float3 high = 1.055 * pow(max(c, 0), 1.0 / 2.4) - 0.055;
    return select(c <= 0.0031308, low, high);
}

float3 srgb_decode(float3 c) {
    float3 low = c / 12.92;
    float3 high = pow((max(c, 0) + 0.055) / 1.055, 2.4);
    return select(c <= 0.04045, low, high);
}

// The lightmap's texture index, or NONE when the scene has no
// environment snapshot (then the stand-in curve below lights the world).
static const uint NO_LIGHTMAP = 0xFFFFFFFF;

uint lightmap_index(RWByteAddressBuffer view) {
    return view.Load(VIEW_ENV);
}

// The light at block and sky levels given as the mesher's bytes (level ×
// 16), filtered between texels as terrain.wgsl's `sample_lightmap`.
float3 lightmap_at(RWByteAddressBuffer view, float2 light) {
    uint lightmap = lightmap_index(view);
    if (lightmap == NO_LIGHTMAP) {
        // Daylight from the brighter of block and sky light, on vanilla's
        // brightness curve.
        float level = max(light.x, light.y) / 240.0;
        float bright = level / (4.0 - 3.0 * level);
        return lerp(0.05, 1.0, saturate(bright));
    }
    float2 coord = clamp(light / 256.0 + 0.5 / 16.0, 0.5 / 16.0, 15.5 / 16.0);
    return texture_at(lightmap).SampleLevel(g_samplers[SAMPLER_LINEAR], coord, 0).rgb;
}

// One texel of the lightmap, at whole levels (entities, as entity.wgsl's
// `cell_light`).
float3 lightmap_texel(RWByteAddressBuffer view, float2 levels_x16) {
    uint lightmap = lightmap_index(view);
    if (lightmap == NO_LIGHTMAP) {
        return lightmap_at(view, levels_x16);
    }
    int2 cell = clamp(int2(levels_x16 / 16.0), 0, 15);
    return texture_at(lightmap).Load(int3(cell, 0)).rgb;
}

float fog_ramp(float distance, float start, float end) {
    if (distance <= start) {
        return 0.0;
    }
    if (distance >= end) {
        return 1.0;
    }
    return (distance - start) / (end - start);
}

// Vanilla's fog: the larger of the environmental ramp (spherical) and the
// render-distance ramp (cylindrical), mixed in sRGB as the game does. `rel`
// is the point from the camera.
float4 vanilla_fog(RWByteAddressBuffer view, float4 color, float3 rel) {
    uint4 env = view.Load4(VIEW_ENV);
    if (env.y == 0) {
        return color;
    }
    float4 fog = asfloat(view.Load4(VIEW_FOG_COLOR));
    float4 ranges = asfloat(view.Load4(VIEW_FOG_RANGES));
    float spherical = length(rel);
    float cylindrical = max(length(rel.xz), abs(rel.y));
    float value = max(fog_ramp(spherical, ranges.x, ranges.y), fog_ramp(cylindrical, ranges.z, ranges.w));
    float3 mixed = lerp(srgb_encode(color.rgb), srgb_encode(fog.rgb), value * fog.a);
    return float4(srgb_decode(mixed), color.a);
}
