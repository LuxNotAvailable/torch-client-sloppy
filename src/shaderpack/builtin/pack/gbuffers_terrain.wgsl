// Torch Vibrant: opaque and cutout terrain. Writes the surface for the
// deferred pass to light: albedo, normal and light levels, and the block-face
// texel each pixel shows, which keys the texel cache. Light levels and vertex
// colour (ambient occlusion) are taken at the texel's middle, so lighting
// steps texel by texel instead of blending across them.

/*
const int colortex0Format = RGBA16F;
const int colortex2Format = RGBA32F;
const int colortex3Format = RGBA16F;
const int colortex4Format = RGBA16F;
const vec4 colortex0ClearColor = vec4(0.0, 0.0, 0.0, 0.0);
const vec4 colortex2ClearColor = vec4(0.0, 0.0, 0.0, -1.0);
const vec4 colortex3ClearColor = vec4(0.0, 0.0, 0.0, 0.0);
const int colortex5Format = RGBA16F;
const bool colortex4Clear = false;
const bool colortex5Clear = false;
*/

/* RENDERTARGETS: 0,2 */

#include "/lib/common.wgsl"

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light: vec2<f32>,
    @location(3) player: vec3<f32>,
    @location(4) @interpolate(flat) normal: vec3<f32>,
    @location(5) @interpolate(flat) material: f32,
}

struct Gbuffer {
    @location(0) albedo: vec4<f32>,
    @location(1) texel: vec4<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ModelViewProjectionMatrix * vec4<f32>(a.position, 1.0));
    out.uv = a.uv;
    out.color = a.color;
    out.light = clamp(a.light / 240.0, vec2<f32>(0.0), vec2<f32>(1.0));
    out.player = a.position;
    out.normal = a.normal;
    out.material = material_of(a.entity.x);
    return out;
}

@fragment
fn fragment(v: Varyings) -> Gbuffer {
    let tex = textureSample(gtexture, gtexture_sampler, v.uv);
    let dpx = dpdx(v.player);
    let dpy = dpdy(v.player);
    let dlx = dpdx(v.light);
    let dly = dpdy(v.light);
    let dcx = dpdx(v.color.rgb);
    let dcy = dpdy(v.color.rgb);
    if (!iris_alpha_pass(tex.a * v.color.a)) {
        discard;
    }

    let face = face_of(v.normal);
    let n = face_normal(u32(face));
    let local = v.player + frame.cameraPositionFract;
    let cell = floor(local * 16.0 - n * 0.5);
    let centre = texel_centre(cell, n) - frame.cameraPositionFract;

    // Step from this pixel to the texel's middle along the surface, as a
    // mix of the screen's two directions, and carry the light and colour
    // gradients the same way.
    let d = centre - v.player;
    let m00 = dot(dpx, dpx);
    let m01 = dot(dpx, dpy);
    let m11 = dot(dpy, dpy);
    let det = m00 * m11 - m01 * m01;
    var light = v.light;
    var color = v.color.rgb;
    if (det > 1e-24) {
        let r0 = dot(d, dpx);
        let r1 = dot(d, dpy);
        let a = (r0 * m11 - r1 * m01) / det;
        let b = (r1 * m00 - r0 * m01) / det;
        light = clamp(v.light + a * dlx + b * dly, vec2<f32>(0.0), vec2<f32>(1.0));
        color = clamp(v.color.rgb + a * dcx + b * dcy, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    var out: Gbuffer;
    out.albedo = vec4<f32>(to_linear(tex.rgb * color), 1.0);
    out.texel = vec4<f32>(cell, pack_surface(face, v.material, light));
    return out;
}
