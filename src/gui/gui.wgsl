@group(0) @binding(0) var atlas_texture: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
@group(0) @binding(2) var unihex_texture: texture_2d<f32>;
@group(0) @binding(3) var unihex_sampler: sampler;
@group(0) @binding(4) var glint_texture: texture_2d<f32>;
@group(0) @binding(5) var glint_sampler: sampler;

const UNIHEX_UV_BIAS: f32 = 2.0;
const UNIHEX_UV_TEST: f32 = 1.5;

struct Vertex {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4(vertex.position, 0.0, 1.0);
    out.uv = vertex.uv;
    out.color = vertex.color;
    return out;
}

const GLINT_UV_BIAS: f32 = 2.0;
const GLINT_UV_TEST: f32 = 1.5;
// A 16 px item covers 1/64 of a 1024 px atlas, so the icon's own 0..1
// coordinates scale by 8 / 64.
const GLINT_SCALE: f32 = 0.125;
const GLINT_STRENGTH: f32 = 0.75;

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(high, low, c <= vec3(0.04045));
}

// Vanilla's item glint: the glint texture through
// translate(-f, g) * rotateZ(pi / 18) * scale(8) of the item's atlas UVs, at
// the default glint strength. The painter hands f and g over in red and green,
// and the icon-local coordinates in blue and alpha.
fn glint(uv: vec2<f32>, phase: vec2<f32>) -> vec3<f32> {
    let s = sin(3.14159265 / 18.0);
    let c = cos(3.14159265 / 18.0);
    let p = uv * GLINT_SCALE;
    let st = vec2(c * p.x - s * p.y, s * p.x + c * p.y) + vec2(-phase.x, phase.y);
    return textureSampleLevel(glint_texture, glint_sampler, st, 0.0).rgb * GLINT_STRENGTH;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if in.uv.y >= GLINT_UV_TEST {
        // Drawn over the icon itself, so it repaints the icon with the glint
        // added as vanilla's (SRC_COLOR, ONE) blend adds it: the glint squared
        // onto the sRGB-encoded pixel.
        let uv = vec2(in.uv.x, in.uv.y - GLINT_UV_BIAS);
        let icon = textureSampleLevel(atlas_texture, atlas_sampler, uv, 0.0);
        let g = glint(in.color.ba, in.color.rg);
        let lifted = srgb_decode(min(srgb_encode(clamp(icon.rgb, vec3(0.0), vec3(1.0))) + g * g, vec3(1.0)));
        return vec4(lifted, icon.a);
    }
    if in.uv.x >= UNIHEX_UV_TEST {
        let uv = vec2(in.uv.x - UNIHEX_UV_BIAS, in.uv.y);
        let a = textureSampleLevel(unihex_texture, unihex_sampler, uv, 0.0).r;
        return vec4(in.color.rgb, in.color.a * a);
    }
    return in.color * textureSampleLevel(atlas_texture, atlas_sampler, in.uv, 0.0);
}
