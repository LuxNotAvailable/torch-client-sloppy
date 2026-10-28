// Torch Vibrant: only the stars; the deferred pass paints the sky itself.

/* RENDERTARGETS: 0 */

#include "/lib/common.wgsl"

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) @interpolate(flat) stage: i32,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0));
    out.color = a.color;
    out.stage = a.render_stage;
    return out;
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    if (v.stage != MC_RENDER_STAGE_STARS) {
        discard;
    }
    return vec4<f32>(to_linear(v.color.rgb) * 1.5, v.color.a);
}
