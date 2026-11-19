// Torch Vibrant: the sun and moon, added to the sky by the deferred pass.

/* RENDERTARGETS: 0 */

#include "lib/common.slang"

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
    nointerpolation int stage : TEXCOORD2;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0)));
    o.uv = a.uv;
    o.color = a.color;
    o.stage = a.render_stage;
    return o;
}

float4 fragment(Varyings v) : SV_Target0 {
    float4 c = gtexture.Sample(gtexture_sampler, v.uv) * v.color;
    float strength = v.stage == MC_RENDER_STAGE_SUN ? 3.0 : 1.0;
    return float4(to_linear(c.rgb) * strength, c.a);
}
