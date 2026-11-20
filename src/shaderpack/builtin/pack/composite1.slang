// Torch Vibrant: volumetric fog, at reduced resolution. Marches from the eye
// to the surface through a haze that thins with height, lit by the sun and
// sky. Light shafts are particles, not this pass.

/* RENDERTARGETS: 4 */

#include "lib/common.slang"

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

float4 fragment(Varyings v) : SV_Target0 {
    // Under water, light shafts are particles.
    if (!has_sky() || frame.isEyeInWater > 0) {
        return float4(0.0, 0.0, 0.0, 1.0);
    }
    float2 size = float2(iris_size(depthtex0));
    float depth = depthtex0.Load(int3(int2(v.uv * size), 0)).r;
    float3 target_pos = player_pos(view_pos(v.uv, min(depth, 0.99999)));
    float reach = min(length(target_pos), depth >= 1.0 ? 160.0 : frame.far);
    float3 dir = normalize(target_pos);

    int steps = clamp(int(ceil(reach / MAX_STEP)), 4, GOD_RAY_STEPS);
    float seg = reach / float(steps);
    float dither = ign(v.clip.xy);
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
        float step_through = exp(-d * seg);
        scatter += through * (1.0 - step_through) * (sun * open + ambient);
        through *= step_through;
    }
    return float4(scatter, through);
}
