// Torch Vibrant: reflections on water, traced at the Reflection Resolution
// into colortex5 (the mirrored colour, and how much of it shows) for
// composite2 to resolve. A screen-space ray finds what the surface mirrors;
// where it leaves the screen the sky stands in, dimmed where the sky does not
// reach.
//
// The ray's start is dithered by a Bayer pattern stepped every frame, and
// last frame's reflection (colortex5 keeps between frames), moved to where
// this surface was then, is blended in, so the steps average out over a few
// frames instead of showing as a stipple.

/* RENDERTARGETS: 5 */

#include "lib/common.slang"

#ifdef TORCH_SLANG
[SpecializationConstant] const int REFLECTION_STEPS = 24;
[SpecializationConstant] const int REFLECTION_SAMPLES = 4;
#else
#define REFLECTION_STEPS 24 // [12 16 24 32 48]
#define REFLECTION_SAMPLES 4 // [1 2 3 4 5 6 7 8]
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

float3 screen_of(float3 view) {
    float4 clip = mul(frame.gbufferProjection, float4(view, 1.0));
    float3 ndc = clip.xyz / clip.w;
    return float3(ndc.xy * 0.5 + 0.5, ndc.z * 0.5 + 0.5);
}

/// The screen position the ray from `start` along `dir` (view space) hits,
/// and how far to trust it, 0 (use the sky) to 1. Trust falls off smoothly
/// rather than switching, so neighbouring pixels whose rays land either side
/// of a threshold do not flicker between the scene and the sky: with how far
/// the hit lies from the ray (a thin edge it grazed), how much of the march
/// it took, how near the screen's edge it is, and how directly the ray
/// heads back at the eye, where the screen holds nothing it could see.
float3 trace(float3 start, float3 dir, float2 size, float dither) {
    float toward_eye = 1.0 - smoothstep(0.1, 0.7, dir.z);
    if (toward_eye <= 0.0) {
        return float3(0.0);
    }
    float3 step = dir * (0.15 + 0.03 * length(start));
    float3 p = start + step * dither;
    for (int i = 0; i < REFLECTION_STEPS; i++) {
        p += step;
        float3 s = screen_of(p);
        if (any(s.xy < 0.0) || any(s.xy > 1.0) || s.z >= 1.0) {
            return float3(0.0);
        }
        float scene = depthtex0.Load(int3(int2(s.xy * size), 0)).r;
        if (s.z > scene) {
            float gap = length(view_pos(s.xy, scene)) - length(p);
            float thickness = length(step) * 1.5 + 0.2;
            if (abs(gap) < thickness) {
                // Halve back toward the surface for a sharper contact.
                float3 lo = p - step;
                float3 hi = p;
                for (int j = 0; j < 4; j++) {
                    float3 mid = (lo + hi) * 0.5;
                    float3 m = screen_of(mid);
                    float d = depthtex0.Load(int3(int2(m.xy * size), 0)).r;
                    if (m.z > d) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                float2 hit = screen_of(hi).xy;
                if (is_hand(colortex2.Load(int3(int2(hit * size), 0)).w)) {
                    return float3(0.0);
                }
                float contact = 1.0 - smoothstep(0.4, 1.0, abs(gap) / thickness);
                float march = 1.0 - smoothstep(0.55, 1.0, float(i) / float(REFLECTION_STEPS));
                float edge = min(min(hit.x, 1.0 - hit.x), min(hit.y, 1.0 - hit.y));
                float border = smoothstep(0.0, 0.1, edge);
                return float3(hit, contact * march * border * toward_eye);
            }
        }
        step *= 1.12;
    }
    return float3(0.0);
}

/// Last frame's reflection where this surface point was on screen then,
/// or nothing (alpha 0) where it was off screen or not water.
float4 reflection_history(float3 view) {
    float3 moved = frame.cameraPosition - frame.previousCameraPosition;
    float3 then = player_pos(view) + moved;
    float4 clip = mul(mul(frame.gbufferPreviousProjection, frame.gbufferPreviousModelView), float4(then, 1.0));
    if (clip.w <= 0.0) {
        return float4(0.0);
    }
    float2 uv = clip.xy / clip.w * 0.5 + 0.5;
    if (any(uv < 0.0) || any(uv > 1.0)) {
        return float4(0.0);
    }
    float2 low = float2(iris_size(colortex5));
    return colortex5.Load(int3(int2(uv * low), 0));
}

float4 fragment(Varyings v) : SV_Target0 {
    // This pass may run at a fraction of the screen; read the full-size
    // targets at the pixel this one stands for.
    float2 size = float2(iris_size(colortex0));
    int3 px = int3(int2(v.uv * size), 0);
    float4 surface = colortex3.Load(px);
    if (surface.z < 0.5 || frame.isEyeInWater == 1 || is_hand(colortex2.Load(px).w)) {
        return float4(0.0);
    }
    float depth = depthtex0.Load(px).r;
    float3 view = view_pos(v.uv, depth);
    float3 n_player = normalize(lerp(float3(0.0, 1.0, 0.0), oct_decode(surface.xy), 0.55));
    float3 n = normalize(mul(frame.gbufferModelView, float4(n_player, 0.0)).xyz);
    float3 eye = normalize(view);
    float3 r = reflect(eye, n);
    float facing = max(dot(-eye, n), 0.0);
    float fresnel = 0.02 + 0.98 * pow(1.0 - facing, 5.0);

    float3 sky = sky_color(player_dir(r)) * smoothstep(0.15, 0.7, surface.w);
    float3 mirrored = sky;
    float3 hit = trace(view, r, size, bayer_temporal(v.clip.xy));
    if (hit.z > 0.0) {
        float3 seen = colortex0.SampleLevel(colortex0_sampler, hit.xy, 0.0).rgb;
        mirrored = lerp(sky, seen, hit.z);
    }
    float4 current = float4(mirrored, fresnel * 0.7);
    // Each frame's trace is one sample of REFLECTION_SAMPLES: the history
    // keeps the rest, so the dither's 16 offsets average out (1 is off).
    if (REFLECTION_SAMPLES <= 1) {
        return current;
    }
    float4 history = reflection_history(view);
    if (history.a <= 0.0) {
        return current;
    }
    return lerp(history, current, 1.0 / float(REFLECTION_SAMPLES));
}
