// Torch Vibrant: lays the volumetric fog over the scene (traced at a
// fraction of the resolution, see lib/resolve.slang), then the distance haze that hides the edge of the
// world, and the murk under water or lava.

/* RENDERTARGETS: 0 */

#include "lib/common.slang"
#include "lib/resolve.slang"
#include "lib/shadow.slang"
#include "lib/water.slang"

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

float4 fragment(Varyings v) : SV_Target0 {
    int3 px = int3(int2(v.clip.xy), 0);
    float3 c = colortex0.Load(px).rgb;
    float depth = depthtex0.Load(px).r;
    // The hand is drawn with its own projection, so its depth says nothing
    // of distance; it is in front of every fog.
    if (is_hand(texel_at(px))) {
        return float4(c, 1.0);
    }
    float3 player = player_pos(view_pos(v.uv, min(depth, 0.99999)));
    float dist = length(player);
    float3 dir = normalize(player);
    bool sky = depth >= 1.0;

    if (frame.isEyeInWater == 1) {
        // Caustics on floors seen from under water: the surface only draws
        // them when looked through from above.
        Texel t = read_texel(texel_at(px));
        if (WATER_CAUSTICS_ENABLED != 0 && WATER_CAUSTICS > 0.0 && !sky && t.terrain && t.surface.face == 2u && dist < 32.0) {
            float3 l = light_dir();
            float4x4 to_shadow = mul(frame.shadowProjection, frame.shadowModelView);
            float vis = shadow_at(mul(to_shadow, float4(player + float3(0.0, 0.05, 0.0), 1.0)).xyz);
            float shade = lerp(1.0 - WATER_CAUSTICS_SHADE, 1.0, vis);
            float strength = WATER_CAUSTICS * 4.0 * shade * smoothstep(0.3, 0.8, t.surface.light.y)
                * (1.0 - smoothstep(20.0, 32.0, dist)) * smoothstep(0.0, 0.3, l.y);
            if (strength > 0.002) {
                float2 floor_xz = player.xz + frame.cameraPosition.xz;
                float caust = caustic(floor_xz, l, 2.0, frame.frameTimeCounter);
                c += c * direct_light() * caust * strength;
            }
        }
        float open = clamp(float(frame.eyeBrightnessSmooth.y) / 240.0, 0.0, 1.0);
        float3 murk = float3(0.02, 0.12, 0.16) * (sky_ambient() * open + float3(0.02));
        float3 through = exp(-(sky ? 48.0 : dist) * float3(0.32, 0.09, 0.07));
        float3 shafts = resolve(colortex4, v.uv, v.clip.xy, depth, false).rgb;
        return float4(c * through + murk * (1.0 - through) + shafts, 1.0);
    }
    if (frame.isEyeInWater == 2) {
        float through = exp(-dist * 1.4);
        return float4(lerp(float3(1.2, 0.32, 0.04), c, through), 1.0);
    }

    float4 fog = resolve(colortex4, v.uv, v.clip.xy, depth, false);
    c = c * fog.a + fog.rgb;

    if (!sky) {
        float3 horizon = sky_color(dir);
        float haze = 1.0 - exp(-dist * 0.0008 * FOG_DENSITY);
        float edge = smoothstep(frame.far * 0.75, frame.far, dist);
        c = lerp(c, horizon, max(haze * 0.45, edge));
    }
    return float4(c, 1.0);
}
