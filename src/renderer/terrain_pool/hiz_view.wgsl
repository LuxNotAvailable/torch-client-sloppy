// Debug Options > Culling View: one level of the terrain's occlusion
// pyramid (`hiz.wgsl`, standard depth: 0 near, 1 far) stretched over the
// frame. Near is light, far dark; texels with nothing drawn (the sky, 1.0)
// are dark blue. A section the pyramid culls lies behind the texels its box
// covers.

struct Params {
    // The frame's size in pixels.
    size: vec2<f32>,
    pad: vec2<f32>,
};

@group(0) @binding(0) var level: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let c = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(c * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(level));
    let texel = min(vec2<i32>(position.xy / params.size * dims), vec2<i32>(dims) - 1);
    let depth = textureLoad(level, texel, 0).r;
    if depth >= 1.0 {
        return vec4<f32>(0.05, 0.05, 0.2, 1.0);
    }
    // Standard depth from an infinite reversed projection: distance is
    // near / (1 - depth); shade by it, half grey at 32 blocks.
    let distance = 0.1 / max(1.0 - depth, 1e-6);
    let shade = exp(-distance / 46.0);
    return vec4<f32>(shade, shade, shade, 1.0);
}
