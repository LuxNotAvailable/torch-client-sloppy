// Torch Vibrant: the shadow map, from the sun or moon. Terrain and entities
// both draw with it. With cascades (torchShadowCascades) it draws once per
// cascade into its square of the atlas, zoomed by the push's scale, as
// lib/shadow.slang reads it; without, it warps the map as that reads it.
// Past the inner cascade leaves cast solid (the push's solid_leaves).

/*
const int shadowMapResolution = 2048; // [512 1024 1536 2048 3072 4096]
const float shadowDistance = 128.0; // [48.0 64.0 96.0 128.0 160.0 192.0 256.0]
const float sunPathRotation = -25.0;
const bool torchShadowCache = true; // [true false]
const float torchShadowStep = 0.02; // [0.02 0.05 0.1 0.2 0.5 1.0 2.0]
const bool torchShadowLerp = false; // [true false]
const bool torchShadowCascades = true;
*/

#include "lib/common.slang"

static const float SHADOW_DISTORTION = 0.85;

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float alpha : TEXCOORD1;
    nointerpolation float leaves : TEXCOORD2;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    float4 clip = mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0));
    float scale = asfloat(iris_bindless.cascade);
    float2 xy = scale > 0.0
        ? clip.xy * scale
        : clip.xy / (length(clip.xy) * SHADOW_DISTORTION + (1.0 - SHADOW_DISTORTION));
    Varyings o;
    o.clip = iris_clip(float4(xy, clip.zw));
    o.uv = a.uv;
    o.alpha = a.color.a;
    o.leaves = is_leaves(a.entity.x) ? 1.0 : 0.0;
    return o;
}

float4 fragment(Varyings v) : SV_Target0 {
    float alpha = gtexture.Sample(gtexture_sampler, v.uv).a * v.alpha;
    bool solid = iris_bindless.solid_leaves != 0 && v.leaves > 0.5;
    if (!solid && !iris_alpha_pass(alpha)) {
        discard;
    }
    return float4(1.0);
}
