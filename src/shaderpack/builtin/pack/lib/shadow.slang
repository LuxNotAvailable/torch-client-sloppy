// Torch Vibrant: sun shadows. Include after common.slang, in programs that
// do not draw to the shadow map.

#ifdef TORCH_SLANG
[SpecializationConstant] const float SHADOW_SOFTNESS = 1.0;
[SpecializationConstant] const int TEXEL_DENOISE = 8;
[SpecializationConstant] const int TEXEL_DENOISE_RES = 8;
[SpecializationConstant] const int TEXEL_TEMPORAL = 32;
[SpecializationConstant] const int TEXEL_TEMPORAL_POINTS = 2;
#else
#define SHADOW_SOFTNESS 1.0 // [0.5 1.0 1.5 2.0]
// Coarse texels' shadow denoising: how many points across the texel its
// shade averages (1 is the middle only), for texels of TEXEL_DENOISE_RES
// texels per block or fewer.
#define TEXEL_DENOISE 8 // [1 2 4 8 16]
#define TEXEL_DENOISE_RES 8 // [1 2 4 8]
// Temporal denoising of those texels (the texel cache only): each frame a
// few points scattered anew across the texel, averaged into its history
// over up to TEXEL_TEMPORAL frames at 1 texel per block, half that at 2,
// and so on (finer texels keep less history, so they follow a moving
// shadow more closely). 0 is off (TEXEL_DENOISE fixed points a frame).
#define TEXEL_TEMPORAL 32 // [0 8 16 32 64]
#define TEXEL_TEMPORAL_POINTS 2 // [1 2 4]
#endif

static const float SHADOW_DISTORTION = 0.85;

/// More of the map near the camera, less far away. The shadow pass draws
/// with the same warp.
float2 shadow_distort(float2 p) {
    return p / (length(p) * SHADOW_DISTORTION + (1.0 - SHADOW_DISTORTION));
}

static const float2 SHADOW_TAPS[8] = {
    float2(-0.326, -0.406),
    float2(-0.840, -0.074),
    float2(-0.696, 0.457),
    float2(-0.203, 0.621),
    float2(0.962, -0.195),
    float2(0.473, -0.480),
    float2(0.519, 0.767),
    float2(0.185, -0.893),
};

/// Two directions across a face with normal `n` (axis-aligned).
void face_tangents(float3 n, out float3 t0, out float3 t1) {
    float3 a = abs(n);
    if (a.x > 0.5) {
        t0 = float3(0.0, 1.0, 0.0);
        t1 = float3(0.0, 0.0, 1.0);
    } else if (a.y > 0.5) {
        t0 = float3(1.0, 0.0, 0.0);
        t1 = float3(0.0, 0.0, 1.0);
    } else {
        t0 = float3(1.0, 0.0, 0.0);
        t1 = float3(0.0, 1.0, 0.0);
    }
}

/// The `i`th of a texel's denoising points, as a fraction of its width from
/// its middle (each axis -0.5 to 0.5). The R2 sequence: any first N points
/// cover the square evenly, so every TEXEL_DENOISE setting is balanced.
float2 denoise_point(uint i) {
    return frac(float2(0.5) + float(i) * float2(0.7548776662, 0.5698402910)) - 0.5;
}

/// The `i`th of a frame's scattered points: the R2 sequence turned by an
/// offset that steps along R2 itself from frame to frame, so over the
/// frames the points cover the texel evenly, not the same few spots.
float2 scattered_point(uint i, uint frame_index) {
    float2 turn = frac(float(frame_index) * float2(0.7548776662, 0.5698402910));
    return frac(float2(0.5) + float(i) * float2(0.7548776662, 0.5698402910) + turn) - 0.5;
}

/// One comparison in the current map (`previous` false) or the last sun
/// step's (shadowtex1, kept for Sun Step Blending).
float shadow_compare(bool previous, float2 uv, float reference) {
    if (previous) {
        return shadowtex1HW.SampleCmpLevelZero(shadowtex1HW_sampler, uv, reference);
    }
    return shadowtex0HW.SampleCmpLevelZero(shadowtex0HW_sampler, uv, reference);
}

/// One hardware-filtered lookup at a point already in the shadow map's clip
/// space (before the warp); 1 outside the map.
float shadow_at(float3 clip) {
    if (length(clip.xy) > 0.98 || abs(clip.z) > 0.98) {
        return 1.0;
    }
    float2 uv = shadow_distort(clip.xy) * 0.5 + 0.5;
    return shadowtex0HW.SampleCmpLevelZero(shadowtex0HW_sampler, uv, clip.z * 0.5 + 0.5 - 0.00005);
}

/// As `shadow_visibility`, in one map, seen through `view`.
float shadow_visibility_in(float4x4 view, bool previous, float3 player, float3 n, uint taps) {
    float4x4 to_shadow = mul(frame.shadowProjection, view);
    float3 clip = mul(to_shadow, float4(player, 1.0)).xyz;
    float reach = length(clip.xy);
    if (reach > 0.98 || abs(clip.z) > 0.98) {
        return 1.0;
    }
    // Farther out a shadow texel covers more ground; lean off the surface
    // in proportion, so it never shadows itself.
    float lean = 0.015 + 0.05 * reach;
    float3 moved = mul(to_shadow, float4(player + n * lean, 1.0)).xyz;
    float2 uv = shadow_distort(moved.xy) * 0.5 + 0.5;
    float reference = moved.z * 0.5 + 0.5 - 0.00005;
    float2 texel = 1.0 / float2(iris_size(shadowtex0HW));
    float radius = SHADOW_SOFTNESS * (1.0 + reach * 2.0);
    if (taps <= 1) {
        return shadow_compare(previous, uv, reference);
    }
    float lit = 0.0;
    uint count = min(taps, 8u);
    for (uint i = 0; i < count; i++) {
        float2 offset = SHADOW_TAPS[i] * texel * radius;
        lit += shadow_compare(previous, uv + offset, reference);
    }
    return lit / float(count);
}

/// How much of the direct light reaches `player` (camera-relative), from
/// `taps` comparisons; 1 outside the shadow map. With Sun Step Blending,
/// faded in from the last step's map (`torchShadowBlend` below 1).
float shadow_visibility(float3 player, float3 n, uint taps) {
    float now = shadow_visibility_in(frame.shadowModelView, false, player, n, taps);
    float blend = frame.torchShadowBlend;
    if (blend >= 1.0) {
        return now;
    }
    float before = shadow_visibility_in(frame.torchShadowModelViewPrevious, true, player, n, taps);
    return lerp(before, now, blend);
}

/// A texel's shade from TEXEL_TEMPORAL_POINTS points scattered for
/// `frame_index`, one frame's sample for the temporal history.
float texel_visibility_scattered(float3 centre, float3 n, float res, uint taps, uint frame_index) {
    uint points = uint(TEXEL_TEMPORAL_POINTS);
    float3 t0, t1;
    face_tangents(n, t0, t1);
    float width = 1.0 / res;
    uint each = max(taps / points, 1u);
    float lit = 0.0;
    for (uint i = 0; i < points; i++) {
        float2 p = scattered_point(i, frame_index) * width;
        lit += shadow_visibility(centre + t0 * p.x + t1 * p.y, n, each);
    }
    return lit / float(points);
}

/// The frames a texel at `res` texels per block keeps in its history:
/// TEXEL_TEMPORAL at 1, halved for each doubling.
uint temporal_window(float res) {
    return max(uint(TEXEL_TEMPORAL) >> uint(log2(max(res, 1.0))), 2u);
}

/// Whether a texel at `res` takes the temporal path.
bool temporal_texel(float res) {
    return TEXEL_TEMPORAL > 0 && res < 16.0 && res <= float(TEXEL_DENOISE_RES);
}

/// The direct light reaching a texel shaded at `res` texels per block, from
/// `taps` comparisons. A full-detail texel, or one above the denoise
/// threshold, is looked up at its middle, as always. A coarser one averages
/// TEXEL_DENOISE points spread over its area, so its shade is the share of
/// the texel in light: a single point there falls in or out of a gap in the
/// leaves as the shadow map moves, and the whole texel flickers with it.
/// The comparisons are shared out among the points, at least one each.
float texel_visibility(float3 centre, float3 n, float res, uint taps) {
    uint points = uint(TEXEL_DENOISE);
    if (res >= 16.0 || res > float(TEXEL_DENOISE_RES) || points <= 1) {
        return shadow_visibility(centre, n, taps);
    }
    float3 t0, t1;
    face_tangents(n, t0, t1);
    float width = 1.0 / res;
    uint each = max(taps / points, 1u);
    float lit = 0.0;
    for (uint i = 0; i < points; i++) {
        float2 p = denoise_point(i) * width;
        lit += shadow_visibility(centre + t0 * p.x + t1 * p.y, n, each);
    }
    return lit / float(points);
}

/// A plant's shade at `p`: leaning toward the light, so its own plane never
/// shadows it.
float foliage_visibility(float3 p, uint taps) {
    float3 l = light_dir();
    return shadow_visibility(p + l * 0.04, l, taps);
}
