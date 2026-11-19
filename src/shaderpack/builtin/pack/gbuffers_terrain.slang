// Torch Vibrant: opaque and cutout terrain. Writes the surface for the
// deferred pass to light: albedo, normal and light levels, and the block-face
// texel each pixel shows, which keys the texel cache. Light levels and vertex
// colour (ambient occlusion) are taken at the texel's middle, so lighting
// steps texel by texel instead of blending across them.

/*
const int colortex0Format = RGBA16F;
const int colortex3Format = RGBA16F;
const int colortex4Format = RGBA16F;
const vec4 colortex0ClearColor = vec4(0.0, 0.0, 0.0, 0.0);
const vec4 colortex2ClearColor = vec4(0.0, 0.0, 0.0, 0.0);
const vec4 colortex3ClearColor = vec4(0.0, 0.0, 0.0, 0.0);
const int colortex5Format = RGBA16F;
const bool colortex4Clear = false;
const bool colortex5Clear = false;
*/

// The texel target: 8 bytes (see `Texel`). RGBA32F on Android, where
// RGBA16 may fall back to a float format that can't hold the words.
#ifdef MC_OS_ANDROID
/*
const int colortex2Format = RGBA32F;
*/
#else
/*
const int colortex2Format = RGBA16;
*/
#endif

/* RENDERTARGETS: 0,2 */

#include "lib/common.slang"
// Fast foliage shading (FOLIAGE_SHADING 1) looks the shadow up per vertex.
#include "lib/shadow.slang"

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
    float2 light : TEXCOORD2;
    float3 player : TEXCOORD3;
    nointerpolation float3 normal : TEXCOORD4;
    nointerpolation float material : TEXCOORD5;
    /// Fast foliage shading: the shade at the vertex.
    float foliage : TEXCOORD6;
};

struct Gbuffer {
    float4 albedo : SV_Target0;
    float4 texel : SV_Target1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0)));
    o.uv = a.uv;
    o.color = a.color;
    o.light = clamp(a.light / 240.0, 0.0, 1.0);
    o.player = a.position;
    o.normal = a.normal;
    o.material = material_of(a.entity.x);
    o.foliage = 1.0;
    if (FOLIAGE_SHADING == 1 && is_diagonal(a.normal)) {
        o.foliage = foliage_visibility(a.position, 1);
    }
    return o;
}

Gbuffer fragment(Varyings v) {
    float4 tex = gtexture.Sample(gtexture_sampler, v.uv);
    float3 dpx = ddx(v.player);
    float3 dpy = ddy(v.player);
    float2 dlx = ddx(v.light);
    float2 dly = ddy(v.light);
    float3 dcx = ddx(v.color.rgb);
    float3 dcy = ddy(v.color.rgb);
    float2 dux = ddx(v.uv);
    float2 duy = ddy(v.uv);
    if (!iris_alpha_pass(tex.a * v.color.a)) {
        discard;
    }

    float3 local = v.player + frame.cameraPositionFract;
    if (is_diagonal(v.normal)) {
        Gbuffer o;
        o.albedo = float4(to_linear(tex.rgb * v.color.rgb), 1.0);
        if (FOLIAGE_SHADING == 0) {
            // The middle of the texture pixel drawn here, found on the plant
            // by stepping across the screen, so texels follow the texture at
            // any angle and offset.
            float2 atlas = float2(iris_size(gtexture));
            float2 want = (floor(v.uv * atlas) + 0.5) / atlas - v.uv;
            float det_uv = dux.x * duy.y - dux.y * duy.x;
            float3 cell = plane_cell(local, v.normal);
            if (abs(det_uv) > 1e-14) {
                float a = (want.x * duy.y - want.y * duy.x) / det_uv;
                float b = (dux.x * want.y - dux.y * want.x) / det_uv;
                float3 mid = v.player + a * dpx + b * dpy + frame.cameraPositionFract;
                cell = floor(mid * 16.0);
            }
            o.texel = pack_texel(cell, float(FACE_DIAGONAL), v.material, v.light);
        } else if (FOLIAGE_SHADING == 1) {
            o.texel = pack_foliage_texel(v.foliage, v.material, v.light);
        } else {
            // One shade per plant block, taken just above its top middle,
            // where its own blades can't shadow it.
            float3 cell = floor(local) * 16.0 + float3(8.0, 16.0, 8.0);
            o.texel = pack_texel(cell, float(FACE_DIAGONAL), v.material, v.light);
        }
        return o;
    }
    float face = face_of(v.normal);
    float3 n = face_normal(uint(face));
    // The ring by the block's distance, not the pixel's, so a whole face
    // changes resolution at once.
    float3 fine = floor(local * 16.0 - n * 0.5);
    float3 cell = coarse_cell(local, n, cell_res(fine));
    float3 centre = texel_centre(cell, n) - frame.cameraPositionFract;

    // Step from this pixel to the texel's middle along the surface, as a
    // mix of the screen's two directions, and carry the light and colour
    // gradients the same way.
    float3 d = centre - v.player;
    float m00 = dot(dpx, dpx);
    float m01 = dot(dpx, dpy);
    float m11 = dot(dpy, dpy);
    float det = m00 * m11 - m01 * m01;
    float2 light = v.light;
    float3 color = v.color.rgb;
    if (det > 1e-24) {
        float r0 = dot(d, dpx);
        float r1 = dot(d, dpy);
        float a = (r0 * m11 - r1 * m01) / det;
        float b = (r1 * m00 - r0 * m01) / det;
        light = clamp(v.light + a * dlx + b * dly, 0.0, 1.0);
        color = clamp(v.color.rgb + a * dcx + b * dcy, 0.0, 1.0);
    }

    Gbuffer o;
    o.albedo = float4(to_linear(tex.rgb * color), 1.0);
    o.texel = pack_texel(cell, face, v.material, light);
    return o;
}
