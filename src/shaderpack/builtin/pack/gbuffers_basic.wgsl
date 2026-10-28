// Torch Vibrant: untextured lines (the block outline, leashes). Unlit.

/* RENDERTARGETS: 0,2 */

#include "/lib/common.wgsl"

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
}

struct Lit {
    @location(0) color: vec4<f32>,
    @location(1) texel: vec4<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0));
    out.color = a.color;
    return out;
}

@fragment
fn fragment(v: Varyings) -> Lit {
    if (!iris_alpha_pass(v.color.a)) {
        discard;
    }
    var out: Lit;
    out.color = vec4<f32>(to_linear(v.color.rgb), v.color.a);
    out.texel = vec4<f32>(0.0, 0.0, 0.0, -1.0);
    return out;
}
