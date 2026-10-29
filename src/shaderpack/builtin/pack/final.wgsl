// Torch Vibrant: exposure and a filmic curve, to the display.

#include "/lib/common.wgsl"

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    var out: Varyings;
    out.clip = iris_screen(input);
    out.uv = iris_attributes(input).uv;
    return out;
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    var c = textureSampleLevel(colortex0, colortex0_sampler, v.uv, 0.0).rgb;
    // The eye opens up at night and underground.
    let open = clamp(f32(frame.eyeBrightnessSmooth.y) / 240.0, 0.0, 1.0);
    let bright = daylight() * open;
    c *= EXPOSURE * mix(1.7, 0.8, bright);
    c = aces(c);
    let grey = vec3<f32>(luma(c));
    c = clamp(mix(grey, c, 1.12), vec3<f32>(0.0), vec3<f32>(1.0));
    return iris_output(vec4<f32>(pow(c, vec3<f32>(1.0 / 2.2)), 1.0));
}
