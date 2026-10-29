// Torch Vibrant: rain, snow and entities' shadows, blended over the lit
// world. Only the colour target: the texel target keeps what lies beneath.

#include "/lib/common.wgsl"

/* RENDERTARGETS: 0 */

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light: vec2<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    let view = frame.iris_ModelViewMatrix * vec4<f32>(a.position, 1.0);
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ProjectionMatrix * view);
    out.uv = a.uv;
    out.color = a.color;
    out.light = clamp(a.light / 240.0, vec2<f32>(0.0), vec2<f32>(1.0));
    return out;
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    let c = textureSample(gtexture, gtexture_sampler, v.uv) * v.color;
    if (!iris_alpha_pass(c.a)) {
        discard;
    }
    // The vertex colour already carries the block's light; the sky's colour
    // and the sun tint it.
    let ambient = sky_color(vec3<f32>(0.0, 1.0, 0.0)) * 0.6 + direct_light() * 0.4;
    return vec4<f32>(to_linear(c.rgb) * ambient, c.a);
}
