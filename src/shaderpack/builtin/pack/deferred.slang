// Torch Vibrant: lights the terrain the gbuffers left, paints the sky, and
// passes through what was lit as it was drawn. Near the camera the shadow
// comes from the texel cache; farther out (or if the cache is full) it is
// looked up here, still at the texel's middle.
//
// On Android there is no cache: a driver there (PowerVR, on the Pixel 10
// Pro) aborts when a depth texture appears in a compute pipeline, which the
// cache's passes need, so every pixel takes an eight-tap lookup here, in the
// fragment stage.
//
// The lit scene is also copied to colortex5, which water samples for its
// reflections (it cannot read the target it blends into).

/* RENDERTARGETS: 0,5 */

#include "lib/common.slang"
#include "lib/shadow.slang"

#ifndef MC_OS_ANDROID
// The texel cache, read only; its layout is lib/cache.slang's.
ByteAddressBuffer cache() { return iris_buffer(IRIS_BUFFER_0); }

uint cache_read(uint table, uint i) {
    return cache().Load(table + 4 * i);
}

static const uint CACHE_KEYS = 16;
static const uint CACHE_VALUES = CACHE_KEYS + 4 * CACHE_SIZE;
static const uint CACHE_CKEYS = CACHE_VALUES + 8 * CACHE_SIZE;
static const uint CACHE_CVALUES = CACHE_CKEYS + 4 * CACHE_SIZE;
#endif

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
};

Varyings vertex(IrisVertexIn input) {
    Varyings o;
    o.clip = iris_screen(input);
    o.uv = iris_attributes(input).uv;
    return o;
}

#ifndef MC_OS_ANDROID
float cached(uint key) {
    uint slot = cache_hash(key);
    for (uint probe = 0; probe < CACHE_PROBES; probe++) {
        uint seen = cache_read(CACHE_KEYS, slot);
        if (seen == key) {
            return float(cache_read(CACHE_VALUES, slot)) / 65535.0;
        }
        if (seen == CACHE_EMPTY) {
            return -1.0;
        }
        slot = (slot + 1) & CACHE_MASK;
    }
    return -1.0;
}

/// As `cached`, from the coarse table. A slot still holding its claimer's
/// offset (COARSE_WORK set) went unshaded.
float cached_coarse(uint key) {
    uint slot = cache_hash(key);
    for (uint probe = 0; probe < CACHE_PROBES; probe++) {
        uint seen = cache_read(CACHE_CKEYS, slot);
        if (seen == key) {
            uint value = cache_read(CACHE_CVALUES, slot);
            return (value & COARSE_WORK) != 0 ? -1.0 : float(value) / 65535.0;
        }
        if (seen == CACHE_EMPTY) {
            return -1.0;
        }
        slot = (slot + 1) & CACHE_MASK;
    }
    return -1.0;
}
#endif

float4 shade(Varyings v);

struct Shaded {
    float4 color : SV_Target0;
    float4 copy : SV_Target1;
};

Shaded fragment(Varyings v) {
    float4 c = shade(v);
    Shaded o;
    o.color = c;
    o.copy = c;
    return o;
}

float4 shade(Varyings v) {
    int3 px = int3(int2(v.clip.xy), 0);
    float4 base = colortex0.Load(px);
    float depth = depthtex0.Load(px).r;
    if (depth >= 1.0) {
        float3 dir = player_dir(view_pos(v.uv, 0.5));
        return float4(sky_color(dir) + base.rgb, 1.0);
    }
    Texel texel = read_texel(texel_at(px));
    if (!texel.terrain) {
        return base;
    }
    Surface surface = texel.surface;
    float3 n = face_normal(surface.face);
    float material = surface.material;

    float vis = -1.0;
    bool diagonal = surface.face == FACE_DIAGONAL;
    // No direct light: the shade would be multiplied away, so skip it
    // (deferred_a skipped claiming it too).
    if (!direct_reaches(n, surface.light.y, diagonal || material == MATERIAL_FOLIAGE)) {
        vis = 0.0;
    }
    if (FOLIAGE_SHADING == 1 && diagonal) {
        vis = foliage_of(texel);
    }
#ifdef MC_OS_ANDROID
    if (diagonal && vis < 0.0) {
        vis = foliage_visibility(texel_centre(texel.cell, n) - frame.cameraPositionFract, 4);
    }
    if (vis < 0.0) {
        float3 centre = texel_centre(texel.cell, n) - frame.cameraPositionFract;
        vis = texel_visibility(centre, n, cell_res(texel.cell), 8);
    }
#else
    if (vis < 0.0) {
        uint key = cache_key(texel.cell, float(surface.face));
        if (key != CACHE_EMPTY) {
            vis = cached(key);
        } else {
            uint coarse = coarse_key(texel.cell, surface.face);
            if (coarse != CACHE_EMPTY) {
                vis = cached_coarse(coarse);
            }
        }
    }
    if (diagonal && vis < 0.0) {
        vis = foliage_visibility(texel_centre(texel.cell, n) - frame.cameraPositionFract, 2);
    }
    if (vis < 0.0) {
        float3 centre = texel_centre(texel.cell, n) - frame.cameraPositionFract;
        vis = texel_visibility(centre, n, cell_res(texel.cell), 2);
    }
#endif

    float3 lit = surface_light(base.rgb, n, surface.light, vis, material == MATERIAL_FOLIAGE);
    if (material == MATERIAL_EMISSIVE) {
        lit = max(lit, base.rgb * 1.3);
    }
    return float4(lit, 1.0);
}
