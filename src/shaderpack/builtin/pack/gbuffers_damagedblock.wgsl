// Torch Vibrant: block-breaking cracks, drawn onto the terrain's albedo
// before the deferred pass lights it.

/* RENDERTARGETS: 0 */

#include "/lib/common.wgsl"

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0));
    out.uv = a.uv;
    out.color = a.color;
    return out;
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    let c = textureSample(gtexture, gtexture_sampler, v.uv) * v.color;
    if (!iris_alpha_pass(c.a)) {
        discard;
    }
    return vec4<f32>(to_linear(c.rgb), c.a);
}
