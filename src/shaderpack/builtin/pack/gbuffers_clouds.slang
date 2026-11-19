// Torch Vibrant: the cloud layer, lit by the sky around it rather than as a
// textured surface. Each face takes the sky's light by how it faces (tops
// brightest, bases darkest), the sun's by how it faces the sun, plus a glow
// looking toward the sun through the cloud. Far off, clouds melt into the
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

float3 soft(float3 albedo, float3 n, float3 dir, float rise) {
    float3 l = light_dir();
    // How much open sky the face sees: all of it on top, little underneath.
    float sky_share = lerp(0.42, 1.0, n.y * 0.5 + 0.5);
    float3 above = sky_color(float3(0.0, 1.0, 0.0));
    float3 around = sky_color(normalize(float3(dir.x, 0.08, dir.z)));
    float3 ambient = lerp(around, above, 0.5) * 0.85 * sky_share;

    // Wrapped, so faces turned away still catch light scattered through.
    float wrap = saturate(dot(n, l) * 0.5 + 0.5);
    float facing = pow(wrap, 0.8);
    float mu = saturate(dot(dir, l));
    float glow = (pow(mu, 6.0) * 0.6 + pow(mu, 40.0) * 0.8) * CLOUD_SCATTER;
    float3 sun = direct_light() * (0.12 + 0.75 * facing + glow);

    float3 c = albedo * (ambient + sun);
    // Walls darken toward their base, as light thins into the cloud.
    if (abs(n.y) < 0.5) {
        c *= lerp(0.72, 1.0, rise);
    }
    float y = luma(c);
    if (y < CLOUD_FLOOR) {
        c *= CLOUD_FLOOR / max(y, 1e-4);
    }
    return lerp(c, float3(luma(c)) * 0.8, frame.rainStrength * 0.6);
}

/// The face's normal from the screen-space slope of its position (the
/// cloud mesh carries none), snapped to the nearest axis and turned toward
/// the camera.
float3 face_from_slope(float3 player) {
    float3 n = cross(ddx(player), ddy(player));
    float3 a = abs(n);
    float3 axis = a.x > a.y && a.x > a.z ? float3(sign(n.x), 0.0, 0.0)
                : a.y > a.z             ? float3(0.0, sign(n.y), 0.0)
                                        : float3(0.0, 0.0, sign(n.z));
    return dot(axis, player) > 0.0 ? -axis : axis;
}

Lit fragment(Varyings v) {
    // Before any discard: derivatives need the whole quad.
    float3 slope_n = face_from_slope(v.player);
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
        float3 n = slope_n;
        bool upper = v.uv.x > 0.5;
        float rise = saturate(v.uv.y);
        lit = soft(albedo, n, dir, rise);
        if (upper) {
            lit *= 0.85;
            alpha *= 0.8;
        }
        // Far away, and low toward the horizon, the cloud turns into the sky
        // behind it.
        float reach = saturate(1.0 - length(v.player.xz) / CLOUD_BLEND_DISTANCE);
        float near = reach * sqrt(reach);
        float haze = 1.0 - smoothstep(-0.02, 0.2, dir.y);
        lit = lerp(sky_color(dir), lit, near * (1.0 - 0.85 * haze));
    }
    Lit o;
    o.color = float4(lit, alpha);
    o.texel = TEXEL_LIT;
    return o;
}
