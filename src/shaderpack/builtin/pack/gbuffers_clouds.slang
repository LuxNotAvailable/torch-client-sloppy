// Torch Vibrant: the cloud layer, lit by the sky around it rather than as a
// textured surface. Light goes by height up the cloud alone (bright tops,
// dark bases, walls blending between), so faces meet with no seam, plus a
// glow looking toward the sun through the cloud. Far off, clouds melt into the
// sky behind them instead of fading out, and low on the horizon into its
// haze. CLOUD_STYLE 1 keeps the plain forward lighting.
//
// Clouds here stand 8 blocks tall, not vanilla's 4 (`scene::clouds`'s
// PACK_CELL_HEIGHT). torchCloudUpper adds a second, thinner layer 30
// blocks above the first's top, from other
// cells of the cloud map (`scene::clouds`). The mesh's uv carries the
// layer (x) and the height up the cell (y).

/* RENDERTARGETS: 0,2 */

/*
const bool torchCloudUpper = true; // [true false]
*/

#include "lib/common.slang"

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
    float3 player : TEXCOORD2;
    float3 normal : TEXCOORD3;
};

struct Lit {
    float4 color : SV_Target0;
    float4 texel : SV_Target1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    float4 view = mul(frame.iris_ModelViewMatrix, float4(a.position, 1.0));
    float3 view_normal = mul(frame.iris_ModelViewMatrix, float4(a.normal, 0.0)).xyz;
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ProjectionMatrix, view));
    o.uv = a.uv;
    o.color = a.color;
    o.player = mul(frame.gbufferModelViewInverse, view).xyz;
    o.normal = mul(frame.gbufferModelViewInverse, float4(view_normal, 0.0)).xyz;
    return o;
}

/// Night clouds keep at least this much light (linear luma), so they never
/// turn into black holes in the sky.
static const float CLOUD_FLOOR = 0.012;

/// The old look: the cloud lit as a forward surface.
float3 classic(float3 albedo, float3 player) {
    float3 above = sky_color(float3(0.0, 1.0, 0.0));
    float3 around = sky_color(normalize(float3(player.x, 0.05, player.z)));
    return albedo * (direct_light() * 0.55 + above * 0.5 + around * 0.6 + float3(0.01));
}

/// The cloud's shade at its base and top (times its light): the base sees
/// little sky and light thins through the cloud to it.
static const float CLOUD_BASE_SHADE = 0.38;
static const float CLOUD_TOP_SHADE = 1.0;

/// The cloud's opacity where the texture has cloud (vanilla's is about
/// 0.8), before the fade toward the layer's rim.
static const float CLOUD_OPACITY = 0.95;

/// The upper layer's opacity, and the share of CLOUD_BLEND_DISTANCE it
/// fades over: thinner and nearer than the lower, so the sky stays calm.
static const float UPPER_OPACITY = 0.6;
static const float UPPER_REACH = 1.0;

/// Lit by height (`rise`, 0 at the base to 1 at the top) and view alone, never
/// the face's normal, so a wall's edges match the top and base they meet.
float3 soft(float3 albedo, float3 dir, float rise) {
    float3 l = light_dir();
    float h = smoothstep(0.0, 1.0, rise);
    // The sky overhead lights the tops, the horizon's the base.
    float3 above = sky_color(float3(0.0, 1.0, 0.0));
    float3 around = sky_color(normalize(float3(dir.x, 0.08, dir.z)));
    float3 ambient = lerp(around, above, h) * 0.85;

    // The sun on the top when high, less down the cloud; and a glow
    // looking toward it through the cloud.
    float mu = saturate(dot(dir, l));
    float glow = (pow(mu, 6.0) * 0.6 + pow(mu, 40.0) * 0.8) * CLOUD_SCATTER;
    float3 sun = direct_light() * (lerp(0.1, 0.35 + 0.5 * saturate(l.y), h) + glow);

    float3 c = albedo * (ambient + sun) * lerp(CLOUD_BASE_SHADE, CLOUD_TOP_SHADE, h);
    float y = luma(c);
    if (y < CLOUD_FLOOR) {
        c *= CLOUD_FLOOR / max(y, 1e-4);
    }
    return lerp(c, float3(luma(c)) * 0.8, frame.rainStrength * 0.6);
}

Lit fragment(Varyings v) {
    float4 c = gtexture.Sample(gtexture_sampler, v.uv);
    float alpha = c.a * v.color.a;
    if (!iris_alpha_pass(alpha)) {
        discard;
    }
    float3 albedo = to_linear(c.rgb);
    float distance = max(length(v.player), 1e-4);
    float3 dir = v.player / distance;
    float3 lit;
    if (CLOUD_STYLE == 1) {
        lit = classic(albedo * to_linear(v.color.rgb), v.player);
    } else {
        bool upper = v.uv.x > 0.5;
        float rise = saturate(v.uv.y);
        alpha = (c.a > 0.0 ? CLOUD_OPACITY : 0.0) * v.color.a;
        lit = soft(albedo, dir, rise);
        // Far away, and low toward the horizon, the cloud turns into the sky
        // behind it; the upper layer sooner, and it thins out as it goes.
        float blend = CLOUD_BLEND_DISTANCE * (upper ? UPPER_REACH : 1.0);
        float reach = saturate(1.0 - length(v.player.xz) / blend);
        float near = reach * sqrt(reach);
        if (upper) {
            lit *= 0.85;
            alpha *= UPPER_OPACITY * near;
        }
        float haze = 1.0 - smoothstep(-0.02, 0.2, dir.y);
        lit = lerp(sky_color(dir), lit, near * (1.0 - 0.85 * haze));
    }
    Lit o;
    o.color = float4(lit, alpha);
    o.texel = TEXEL_LIT;
    return o;
}
