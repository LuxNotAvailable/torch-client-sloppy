// Torch Vibrant: the shadow map, from the sun or moon. Terrain and entities
// both draw with it; the warp matches lib/shadow.slang.

/*
const int shadowMapResolution = 2048; // [512 1024 1536 2048 3072 4096]
const float shadowDistance = 128.0; // [48.0 64.0 96.0 128.0 160.0 192.0 256.0]
const float sunPathRotation = -25.0;
const bool torchShadowCache = true; // [true false]
const float torchShadowStep = 0.02; // [0.02 0.05 0.1 0.2 0.5 1.0 2.0]
const bool torchShadowLerp = false; // [true false]
*/

#include "lib/common.slang"

static const float SHADOW_DISTORTION = 0.85;

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float alpha : TEXCOORD1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    float4 clip = mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0));
    float2 warped = clip.xy / (length(clip.xy) * SHADOW_DISTORTION + (1.0 - SHADOW_DISTORTION));
    Varyings o;
    o.clip = iris_clip(float4(warped, clip.zw));
    o.uv = a.uv;
    o.alpha = a.color.a;
    return o;
}

float4 fragment(Varyings v) : SV_Target0 {
    float alpha = gtexture.Sample(gtexture_sampler, v.uv).a * v.alpha;
    if (!iris_alpha_pass(alpha)) {
        discard;
    }
    return float4(1.0);
}
