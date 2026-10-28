// Torch Vibrant: the shadow map, from the sun or moon. Terrain and entities
// both draw with it; the warp matches lib/shadow.wgsl.

/*
const int shadowMapResolution = 2048;
const float shadowDistance = 128.0;
const float sunPathRotation = -25.0;
*/

#include "/lib/common.wgsl"

const SHADOW_DISTORTION: f32 = 0.85;

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    let clip = frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0);
    let warped = clip.xy / (length(clip.xy) * SHADOW_DISTORTION + (1.0 - SHADOW_DISTORTION));
    var out: Varyings;
    out.clip = iris_clip(vec4<f32>(warped, clip.zw));
    out.uv = a.uv;
    out.alpha = a.color.a;
    return out;
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    let alpha = textureSample(gtexture, gtexture_sampler, v.uv).a * v.alpha;
    if (!iris_alpha_pass(alpha)) {
        discard;
    }
    return vec4<f32>(1.0);
}
