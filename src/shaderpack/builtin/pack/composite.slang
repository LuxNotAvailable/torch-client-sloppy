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

/// Turns a depth-buffer value into distance along the view axis. `a` and `b`
/// are the inverse projection's last two columns' worth, taken once per pixel
/// so a lookup is a multiply-add, not a matrix.
float linear_depth(float depth, float4 a, float4 b) {
    float4 v = a + b * (depth * 2.0 - 1.0);
    return -v.z / v.w;
}

/// The screen position the ray from `start` along `dir` (view space) hits,
/// and how far to trust it, 0 (use the sky) to 1. Trust falls off smoothly
/// rather than switching, so neighbouring pixels whose rays land either side
/// of a threshold do not flicker between the scene and the sky: with how far
/// the hit lies from the ray (a thin edge it grazed), how much of the march
/// it took, how near the screen's edge it is, and how directly the ray
/// heads back at the eye, where the screen holds nothing it could see.
///
/// The ray marches in clip space: projection is linear there, so a step is
/// an add, not a matrix. It is first cut to where it leaves the screen or
/// crosses the near plane, so no step lands off screen. Steps grow while the
/// ray is clear of the scene, and a hit is halved back to its contact.
float3 trace(float3 start, float3 dir, float2 size, float dither) {
    float toward_eye = 1.0 - smoothstep(0.1, 0.7, dir.z);
    if (toward_eye <= 0.0) {
        return float3(0.0);
    }
    float4 c0 = mul(frame.gbufferProjection, float4(start, 1.0));
    float4 cd = mul(frame.gbufferProjection, float4(dir, 0.0));
    float4 inv_a = mul(frame.gbufferProjectionInverse, float4(0.0, 0.0, 0.0, 1.0));
    float4 inv_b = mul(frame.gbufferProjectionInverse, float4(0.0, 0.0, 1.0, 0.0));

    // Where the ray leaves: each screen edge is |x| = w, the near plane w = 0.05.
    float reach = 512.0;
    if (cd.w < 0.0) {
        reach = min(reach, (c0.w - 0.05) / -cd.w);
    }
    float4 grow = float4(cd.x - cd.w, -cd.x - cd.w, cd.y - cd.w, -cd.y - cd.w);
    float4 room = float4(c0.w - c0.x, c0.w + c0.x, c0.w - c0.y, c0.w + c0.y);
    float4 exits = select(grow > 0.0, room / max(grow, 1e-6), float4(1e6));
    reach = min(reach, min(min(exits.x, exits.y), min(exits.z, exits.w)));
    if (reach <= 0.0) {
        return float3(0.0);
    }

    float span = 0.15 + 0.03 * length(start);
    float lo = span * dither;
    float hi = -1.0;
    float thickness = 0.0;
    float gap = 0.0;
    int steps = 0;
    for (int i = 0; i < REFLECTION_STEPS; i++) {
        float probe = min(lo + span, reach);
        float4 c = c0 + cd * probe;
        float2 s = c.xy / c.w * 0.5 + 0.5;
        float scene = depthtex0.Load(int3(int2(s * size), 0)).r;
        float behind = scene >= 1.0 ? -1e6 : c.w - linear_depth(scene, inv_a, inv_b);
        // A step's worth of slack, so a long step cannot skip a thin wall.
        thickness = abs(cd.w) * (probe - lo) * 1.5 + 0.2 + c.w * 0.015;
        steps = i;
        if (behind > 0.0 && behind < thickness) {
            hi = probe;
            gap = behind;
            break;
        }
        if (probe >= reach) {
            return float3(0.0);
        }
        // Well clear of the scene: stride out.
        if (behind < -2.0 * thickness) {
            span *= 1.6;
        }
        lo = probe;
    }
    if (hi < 0.0) {
        return float3(0.0);
    }

    // Halve back toward the surface for a sharper contact.
    for (int j = 0; j < 4; j++) {
        float mid = (lo + hi) * 0.5;
        float4 c = c0 + cd * mid;
        float2 s = c.xy / c.w * 0.5 + 0.5;
        float scene = depthtex0.Load(int3(int2(s * size), 0)).r;
        float behind = scene >= 1.0 ? -1e6 : c.w - linear_depth(scene, inv_a, inv_b);
        if (behind > 0.0) {
            hi = mid;
            gap = behind;
        } else {
            lo = mid;
        }
    }
    float4 ch = c0 + cd * hi;
    float2 hit = ch.xy / ch.w * 0.5 + 0.5;
    if (is_hand(colortex2.Load(int3(int2(hit * size), 0)).w)) {
        return float3(0.0);
    }
    float contact = 1.0 - smoothstep(0.4, 1.0, gap / thickness);
    float march = 1.0 - smoothstep(0.55, 1.0, float(steps) / float(REFLECTION_STEPS));
    float edge = min(min(hit.x, 1.0 - hit.x), min(hit.y, 1.0 - hit.y));
    float border = smoothstep(0.0, 0.1, edge);
    return float3(hit, contact * march * border * toward_eye);
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
