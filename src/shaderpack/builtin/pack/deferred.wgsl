// Torch Vibrant: lights the terrain the gbuffers left, paints the sky, and
// passes through what was lit as it was drawn. Near the camera the shadow
// comes from the texel cache; farther out (or if the cache is full) it is
// looked up here, still at the texel's middle.

/* RENDERTARGETS: 0 */

#include "/lib/common.wgsl"
#include "/lib/shadow.wgsl"

struct CacheRead {
    count: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
    keys: array<u32, 1048576>,
    values: array<u32, 1048576>,
    work: array<u32, 1048576>,
}

@group(0) @binding(IRIS_BUFFER_0) var<storage, read> cache: CacheRead;

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

fn cached(key: u32) -> f32 {
    var slot = cache_hash(key);
    for (var probe = 0u; probe < CACHE_PROBES; probe++) {
        let seen = cache.keys[slot];
        if (seen == key) {
            return f32(cache.values[slot]) / 65535.0;
        }
        if (seen == CACHE_EMPTY) {
            return -1.0;
        }
        slot = (slot + 1u) & CACHE_MASK;
    }
    return -1.0;
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    let px = vec2<i32>(v.clip.xy);
    let base = textureLoad(colortex0, px, 0);
    let depth = textureLoad(depthtex0, px, 0).r;
    if (depth >= 1.0) {
        let dir = player_dir(view_pos(v.uv, 0.5));
        return vec4<f32>(sky_color(dir) + base.rgb, 1.0);
    }
    let texel = textureLoad(colortex2, px, 0);
    if (texel.w < 0.0) {
        return base;
    }
    let surface = unpack_surface(texel.w);
    let n = face_normal(surface.face);
    let material = surface.material;

    var vis = -1.0;
    let key = cache_key(texel.xyz, f32(surface.face));
    if (key != CACHE_EMPTY) {
        vis = cached(key);
    }
    if (vis < 0.0) {
        let centre = texel_centre(texel.xyz, n) - frame.cameraPositionFract;
        vis = shadow_visibility(centre, n, 2u);
    }

    var lit = surface_light(base.rgb, n, surface.light, vis, material == MATERIAL_FOLIAGE);
    if (material == MATERIAL_EMISSIVE) {
        lit = max(lit, base.rgb * 1.3);
    }
    return vec4<f32>(lit, 1.0);
}
