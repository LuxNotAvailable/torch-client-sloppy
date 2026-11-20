// Torch Vibrant: volumetric fog and light shafts, at half resolution. Marches
// from the eye to the surface through a haze that thins with height, lit by
// the sun wherever the shadow map sees it.

/* RENDERTARGETS: 4 */

#include "lib/common.slang"
#include "lib/shadow.slang"

#ifdef TORCH_SLANG
[SpecializationConstant] const int GOD_RAY_STEPS = 6;
#else
#define GOD_RAY_STEPS 6 // [6 8 12 16 24]
#endif

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
};

Varyings vertex(IrisVertexIn input) {
    Varyings o;
    o.clip = iris_screen(input);
    o.uv = iris_attributes(input).uv;
    return o;
}

/// The haze at sea level; it thins above it (`density`).
float haze() {
    float morning = 1.0 - smoothstep(0.0, 0.35, abs(sun_dir().y));
    return (0.0012 + 0.0014 * morning + 0.003 * frame.rainStrength) * FOG_DENSITY;
}

float density(float base, float y) {
    return base * exp(-max(y - 63.0, 0.0) / 36.0);
}

/// No step is longer than this, so short rays take fewer steps for the same
/// detail.
static const float MAX_STEP = 8.0;

/// How strongly the haze scatters light toward the eye, 0.3 away from the
/// light up to 1.5 looking into it.
float phase(float cos_angle) {
    float toward = max(cos_angle, 0.0);
    return 0.3 + 0.7 * pow(toward, 4.0) + 0.5 * pow(toward, 32.0);
}

/// Water's extinction per block, as composite2's murk.
static const float3 WATER_EXTINCTION = float3(0.32, 0.09, 0.07);

/// Light shafts under water: sunlight the water scatters toward the eye,
/// dimmed by the water between. Alpha stays 1: composite2's murk does the
/// extinction.
float4 water_shafts(float2 uv, float2 clip_xy) {
    float2 size = float2(iris_size(depthtex0));
    float depth = depthtex0.Load(int3(int2(uv * size), 0)).r;
    float3 target_pos = player_pos(view_pos(uv, min(depth, 0.99999)));
    float reach = min(length(target_pos), 48.0);
    float3 dir = normalize(target_pos);
    int steps = GOD_RAY_STEPS;
    float seg = reach / float(steps);
    float dither = ign(clip_xy);
    float4x4 to_shadow = mul(frame.shadowProjection, frame.shadowModelView);
    float3 start = mul(to_shadow, float4(0.0, 0.0, 0.0, 1.0)).xyz;
    float3 along = mul(to_shadow, float4(dir * seg, 0.0)).xyz;
    float open = clamp(float(frame.eyeBrightnessSmooth.y) / 240.0, 0.0, 1.0);
    float3 sun = direct_light() * phase(dot(dir, light_dir())) * daylight() * open;
    float3 tint = float3(0.25, 0.8, 0.9);
    float3 scatter = float3(0.0);
    for (int i = 0; i < steps; i++) {
        float k = float(i) + dither;
        float vis = shadow_at(start + along * k);
        scatter += exp(-WATER_EXTINCTION * seg * k) * vis * seg;
    }
    return float4(sun * tint * scatter * 0.012 * FOG_DENSITY, 1.0);
}

float4 fragment(Varyings v) : SV_Target0 {
    if (!has_sky() || frame.isEyeInWater > 1) {
        return float4(0.0, 0.0, 0.0, 1.0);
    }
    if (frame.isEyeInWater == 1) {
        return water_shafts(v.uv, v.clip.xy);
    }
    float2 size = float2(iris_size(depthtex0));
    float depth = depthtex0.Load(int3(int2(v.uv * size), 0)).r;
    float3 target_pos = player_pos(view_pos(v.uv, min(depth, 0.99999)));
    float reach = min(length(target_pos), depth >= 1.0 ? 160.0 : frame.far);
    float3 dir = normalize(target_pos);

    int steps = clamp(int(ceil(reach / MAX_STEP)), 4, GOD_RAY_STEPS);
    float seg = reach / float(steps);
    float dither = ign(v.clip.xy);
    // The shadow map is an orthographic projection, so the ray is a straight
    // line in its clip space too: transform the ends once, not every step.
    float4x4 to_shadow = mul(frame.shadowProjection, frame.shadowModelView);
    float3 start = mul(to_shadow, float4(0.0, 0.0, 0.0, 1.0)).xyz;
    float3 along = mul(to_shadow, float4(dir * seg, 0.0)).xyz;
    float base = haze();
    float3 sun = direct_light() * phase(dot(dir, light_dir())) * lerp(0.15, 1.0, daylight());
    // Light in caves is not the sky's: fade the haze's sky light by what the
    // eye sees of the sky.
    float open = clamp(float(frame.eyeBrightnessSmooth.y) / 240.0, 0.0, 1.0);
    float3 ambient = sky_ambient() * 0.6 * open;

    float3 scatter = float3(0.0);
    float through = 1.0;
    for (int i = 0; i < steps; i++) {
        float k = float(i) + dither;
        float d = density(base, frame.cameraPosition.y + dir.y * seg * k);
        float vis = shadow_at(start + along * k);
        float step_through = exp(-d * seg);
        scatter += through * (1.0 - step_through) * (sun * vis * open + ambient);
        through *= step_through;
    }
    return float4(scatter, through);
}
