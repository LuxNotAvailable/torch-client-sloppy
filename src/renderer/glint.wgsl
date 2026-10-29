#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var mask_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var glint_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var glint_sampler: sampler;

// Vanilla's item glint: the glint texture through
// translate(-f, g) * rotateZ(pi / 18) * scale(8) of the item's atlas UVs,
// f and g cycling every 110 s and 30 s of `millis * glintSpeed * 8` (glint
// speed 0.5), times the default glint strength of 0.75. A 16 px item covers
// 1/64 of a 1024 px atlas, so its own 0..1 UVs scale by 8 / 64.
const GLINT_SCALE: f32 = 0.125;
const GLINT_STRENGTH: f32 = 0.75;
const GLINT_TICKS_PER_SEC: f32 = 4000.0;

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

fn glint(uv: vec2<f32>) -> vec3<f32> {
    let ticks = globals.time * GLINT_TICKS_PER_SEC;
    let f = fract(ticks / 110000.0);
    let g = fract(ticks / 30000.0);
    let s = sin(3.14159265 / 18.0);
    let c = cos(3.14159265 / 18.0);
    let p = uv * GLINT_SCALE;
    let st = vec2(c * p.x - s * p.y, s * p.x + c * p.y) + vec2(-f, g);
    return textureSampleLevel(glint_texture, glint_sampler, st, 0.0).rgb * GLINT_STRENGTH;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(mask_texture, mask_sampler, in.uv);
    if texel.a < 0.1 {
        discard;
    }
    // Vanilla adds the glint squared to the sRGB-encoded pixel; this pass adds
    // in linear light, so it adds what that does to the item's own texel.
    let g = glint(in.uv);
    let base = clamp(texel.rgb, vec3(0.0), vec3(1.0));
    let lifted = srgb_decode(min(srgb_encode(base) + g * g, vec3(1.0)));
    return vec4(lifted - base, 0.0);
}
