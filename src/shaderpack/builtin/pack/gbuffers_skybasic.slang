// Torch Vibrant: only the stars; the deferred pass paints the sky itself.

/* RENDERTARGETS: 0 */

#include "lib/common.slang"

struct Varyings {
    float4 clip : SV_Position;
    float4 color : TEXCOORD0;
    nointerpolation int stage : TEXCOORD1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0)));
    o.color = a.color;
    o.stage = a.render_stage;
    return o;
}

float4 fragment(Varyings v) : SV_Target0 {
    if (v.stage != MC_RENDER_STAGE_STARS) {
        discard;
    }
    return float4(to_linear(v.color.rgb) * 1.5, v.color.a);
}
