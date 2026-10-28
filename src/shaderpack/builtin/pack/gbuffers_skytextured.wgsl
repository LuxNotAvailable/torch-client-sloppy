// Torch Vibrant: the sun and moon, added to the sky by the deferred pass.

/* RENDERTARGETS: 0 */

#include "/lib/common.wgsl"

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) stage: i32,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0));
    out.uv = a.uv;
    out.color = a.color;
    out.stage = a.render_stage;
    return out;
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    let c = textureSample(gtexture, gtexture_sampler, v.uv) * v.color;
    let strength = select(1.0, 3.0, v.stage == MC_RENDER_STAGE_SUN);
    return vec4<f32>(to_linear(c.rgb) * strength, c.a);
}
