// Torch Vibrant: block-breaking cracks, drawn onto the terrain's albedo
// before the deferred pass lights it.

/* RENDERTARGETS: 0 */

#include "lib/common.slang"

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0)));
    o.uv = a.uv;
    o.color = a.color;
    return o;
}

float4 fragment(Varyings v) : SV_Target0 {
    float4 c = gtexture.Sample(gtexture_sampler, v.uv) * v.color;
    if (!iris_alpha_pass(c.a)) {
        discard;
    }
    return float4(to_linear(c.rgb), c.a);
}
