// Torch Vibrant: sun shadows. Include after common.wgsl, in programs that
// do not draw to the shadow map.

#define SHADOW_SOFTNESS 1.0 // [0.5 1.0 1.5 2.0]
// Coarse texels' shadow denoising: how many points across the texel its
// shade averages (1 is the middle only), for texels of TEXEL_DENOISE_RES
// texels per block or fewer.
#define TEXEL_DENOISE 8 // [1 2 4 8 16]
#define TEXEL_DENOISE_RES 8 // [1 2 4 8]

const SHADOW_DISTORTION: f32 = 0.85;

/// More of the map near the camera, less far away. The shadow pass draws
/// with the same warp.
fn shadow_distort(p: vec2<f32>) -> vec2<f32> {
    return p / (length(p) * SHADOW_DISTORTION + (1.0 - SHADOW_DISTORTION));
}

const SHADOW_TAPS: array<vec2<f32>, 8> = array<vec2<f32>, 8>(
    vec2<f32>(-0.326, -0.406),
    vec2<f32>(-0.840, -0.074),
    vec2<f32>(-0.696, 0.457),
    vec2<f32>(-0.203, 0.621),
    vec2<f32>(0.962, -0.195),
    vec2<f32>(0.473, -0.480),
    vec2<f32>(0.519, 0.767),
    vec2<f32>(0.185, -0.893),
);

/// Two directions across a face with normal `n` (axis-aligned).
fn face_tangents(n: vec3<f32>) -> array<vec3<f32>, 2> {
    let a = abs(n);
    if (a.x > 0.5) {
        return array<vec3<f32>, 2>(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    }
    if (a.y > 0.5) {
        return array<vec3<f32>, 2>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    }
    return array<vec3<f32>, 2>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0));
}

/// The `i`th of a texel's denoising points, as a fraction of its width from
/// its middle (each axis -0.5 to 0.5). The R2 sequence: any first N points
/// cover the square evenly, so every TEXEL_DENOISE setting is balanced.
fn denoise_point(i: u32) -> vec2<f32> {
    return fract(vec2<f32>(0.5) + f32(i) * vec2<f32>(0.7548776662, 0.5698402910)) - 0.5;
}

/// The direct light reaching a texel shaded at `res` texels per block, from
/// `taps` comparisons. A full-detail texel, or one above the denoise
/// threshold, is looked up at its middle, as always. A coarser one averages
/// TEXEL_DENOISE points spread over its area, so its shade is the share of
/// the texel in light: a single point there falls in or out of a gap in the
/// leaves as the shadow map moves, and the whole texel flickers with it.
/// The comparisons are shared out among the points, at least one each.
fn texel_visibility(centre: vec3<f32>, n: vec3<f32>, res: f32, taps: u32) -> f32 {
    let points = u32(TEXEL_DENOISE);
    if (res >= 16.0 || res > f32(TEXEL_DENOISE_RES) || points <= 1u) {
        return shadow_visibility(centre, n, taps);
    }
    let t = face_tangents(n);
    let width = 1.0 / res;
    let each = max(taps / points, 1u);
    var lit = 0.0;
    for (var i = 0u; i < points; i++) {
        let p = denoise_point(i) * width;
        lit += shadow_visibility(centre + t[0] * p.x + t[1] * p.y, n, each);
    }
    return lit / f32(points);
}

/// One hardware-filtered lookup at a point already in the shadow map's clip
/// space (before the warp); 1 outside the map.
fn shadow_at(clip: vec3<f32>) -> f32 {
    if (length(clip.xy) > 0.98 || abs(clip.z) > 0.98) {
        return 1.0;
    }
    let uv = shadow_distort(clip.xy) * 0.5 + 0.5;
    return textureSampleCompareLevel(shadowtex0HW, shadowtex0HW_sampler, uv, clip.z * 0.5 + 0.5 - 0.00005);
}

/// How much of the direct light reaches `player` (camera-relative), from
/// `taps` comparisons; 1 outside the shadow map.
fn shadow_visibility(player: vec3<f32>, n: vec3<f32>, taps: u32) -> f32 {
    let clip = (frame.shadowProjection * frame.shadowModelView * vec4<f32>(player, 1.0)).xyz;
    let reach = length(clip.xy);
    if (reach > 0.98 || abs(clip.z) > 0.98) {
        return 1.0;
    }
    // Farther out a shadow texel covers more ground; lean off the surface
    // in proportion, so it never shadows itself.
    let lean = 0.015 + 0.05 * reach;
    let moved = (frame.shadowProjection * frame.shadowModelView * vec4<f32>(player + n * lean, 1.0)).xyz;
    let uv = shadow_distort(moved.xy) * 0.5 + 0.5;
    let reference = moved.z * 0.5 + 0.5 - 0.00005;
    let texel = 1.0 / vec2<f32>(textureDimensions(shadowtex0HW));
    let radius = SHADOW_SOFTNESS * (1.0 + reach * 2.0);
    if (taps <= 1u) {
        return textureSampleCompareLevel(shadowtex0HW, shadowtex0HW_sampler, uv, reference);
    }
    var lit = 0.0;
    let count = min(taps, 8u);
    for (var i = 0u; i < count; i++) {
        let offset = SHADOW_TAPS[i] * texel * radius;
        lit += textureSampleCompareLevel(shadowtex0HW, shadowtex0HW_sampler, uv + offset, reference);
    }
    return lit / f32(count);
}
