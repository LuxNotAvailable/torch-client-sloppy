// Torch Vibrant: untextured lines (the block outline, leashes). Unlit.

/* RENDERTARGETS: 0,2 */

#include "lib/common.slang"

struct Varyings {
    float4 clip : SV_Position;
    float4 color : TEXCOORD0;
};

struct Lit {
    float4 color : SV_Target0;
    TexelWords texel : SV_Target1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0)));
    o.color = a.color;
    return o;
}

Lit fragment(Varyings v) {
    if (!iris_alpha_pass(v.color.a)) {
        discard;
    }
    Lit o;
    o.color = float4(to_linear(v.color.rgb), v.color.a);
    o.texel = TEXEL_LIT;
    return o;
}
