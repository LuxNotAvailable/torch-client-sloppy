// Torch Vibrant: sun shadows. Include after common.wgsl, in programs that
// do not draw to the shadow map.

#define SHADOW_SOFTNESS 1.0 // [0.5 1.0 1.5 2.0]

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
