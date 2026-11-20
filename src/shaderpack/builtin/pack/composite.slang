// Torch Vibrant: lays World Traced reflections over the water. Each pixel
// that claimed a reflection texel (colortex6) looks up what composite_a
// traced for it and mixes it in as the water would have before blending:
// alpha * blend * (reflected - surface colour).

/* RENDERTARGETS: 0 */

#include "lib/common.slang"
#include "lib/shadow.slang"
#include "lib/water.slang"

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

float4 fragment(Varyings v) : SV_Target0 {
    int3 px = int3(int2(v.clip.xy), 0);
    float3 c = colortex0.Load(px).rgb;
    float4 claim = colortex6.Load(px);
    if (claim.w < 0.5) {
        return float4(c, 1.0);
    }
    uint key = (uint(claim.x) << 16u) | uint(claim.y);
    uint4 value = refl_lookup(key);
    // Untraced (the table was full): this pixel traces its texel itself,
    // so a full table costs time, not holes.
    if (value.w == REFL_PENDING) {
        float depth = depthtex0.Load(px).r;
        value = refl_value(key, player_pos(view_pos(v.uv, depth)).y);
    }
    float4 surface = colortex3.Load(px);
    float4 m = refl_finish(value, colortex7.Load(px).rgb, refl_open(surface.w));
    c += claim.z * saturate(m.a) * (m.rgb - surface.rgb);
    return float4(c, 1.0);
}
