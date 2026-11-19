// Torch Vibrant: rain, snow and entities' shadows, blended over the lit
// world. Only the colour target: the texel target keeps what lies beneath.

#include "lib/common.slang"

/* RENDERTARGETS: 0 */

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
    float2 light : TEXCOORD2;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    float4 view = mul(frame.iris_ModelViewMatrix, float4(a.position, 1.0));
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ProjectionMatrix, view));
    o.uv = a.uv;
    o.color = a.color;
    o.light = clamp(a.light / 240.0, 0.0, 1.0);
    return o;
}

float4 fragment(Varyings v) : SV_Target0 {
    float4 c = gtexture.Sample(gtexture_sampler, v.uv) * v.color;
    if (!iris_alpha_pass(c.a)) {
        discard;
    }
    // The vertex colour already carries the block's light; the sky's colour
    // and the sun tint it.
    float3 ambient = sky_color(float3(0.0, 1.0, 0.0)) * 0.6 + direct_light() * 0.4;
    return float4(to_linear(c.rgb) * ambient, c.a);
}
