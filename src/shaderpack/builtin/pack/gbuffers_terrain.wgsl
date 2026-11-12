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
#if FOLIAGE_SHADING == 1
#include "/lib/shadow.wgsl"
#endif

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light: vec2<f32>,
    @location(3) player: vec3<f32>,
    @location(4) @interpolate(flat) normal: vec3<f32>,
    @location(5) @interpolate(flat) material: f32,
    /// Fast foliage shading: the shade at the vertex.
    @location(6) foliage: f32,
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
    out.foliage = 1.0;
#if FOLIAGE_SHADING == 1
    if (is_diagonal(a.normal)) {
        out.foliage = foliage_visibility(a.position, 1u);
    }
#endif
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
    let dux = dpdx(v.uv);
    let duy = dpdy(v.uv);
    if (!iris_alpha_pass(tex.a * v.color.a)) {
        discard;
    }

    let local = v.player + frame.cameraPositionFract;
    if (is_diagonal(v.normal)) {
        var out: Gbuffer;
        out.albedo = vec4<f32>(to_linear(tex.rgb * v.color.rgb), 1.0);
#if FOLIAGE_SHADING == 0
        // The middle of the texture pixel drawn here, found on the plant
        // by stepping across the screen, so texels follow the texture at
        // any angle and offset.
        let atlas = vec2<f32>(textureDimensions(gtexture));
        let want = (floor(v.uv * atlas) + 0.5) / atlas - v.uv;
        let det_uv = dux.x * duy.y - dux.y * duy.x;
        var cell = plane_cell(local, v.normal);
        if (abs(det_uv) > 1e-14) {
            let a = (want.x * duy.y - want.y * duy.x) / det_uv;
            let b = (dux.x * want.y - dux.y * want.x) / det_uv;
            let mid = v.player + a * dpx + b * dpy + frame.cameraPositionFract;
            cell = floor(mid * 16.0);
        }
        out.texel = vec4<f32>(cell, pack_surface(f32(FACE_DIAGONAL), v.material, v.light));
#elif FOLIAGE_SHADING == 1
        out.texel = vec4<f32>(v.foliage, 0.0, 0.0, pack_surface(f32(FACE_DIAGONAL), v.material, v.light));
#else
        // One shade per plant block, taken just above its top middle, where
        // its own blades can't shadow it.
        let cell = floor(local) * 16.0 + vec3<f32>(8.0, 16.0, 8.0);
        out.texel = vec4<f32>(cell, pack_surface(f32(FACE_DIAGONAL), v.material, v.light));
#endif
        return out;
    }
    let face = face_of(v.normal);
    let n = face_normal(u32(face));
    // The ring by the block's distance, not the pixel's, so a whole face
    // changes resolution at once.
    let fine = floor(local * 16.0 - n * 0.5);
    let cell = coarse_cell(local, n, cell_res(fine));
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
