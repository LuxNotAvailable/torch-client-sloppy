// Torch Vibrant: what is lit as it is drawn, not by the deferred pass:
// entities, block entities, particles, clouds and the hand. Include after
// common.slang and shadow.slang.

/* RENDERTARGETS: 0,2 */

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
    float2 light : TEXCOORD2;
    float3 player : TEXCOORD3;
    float3 normal : TEXCOORD4;
    nointerpolation float4 overlay : TEXCOORD5;
    nointerpolation int stage : TEXCOORD6;
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
    o.light = clamp(a.light / 240.0, 0.0, 1.0);
    o.player = mul(frame.gbufferModelViewInverse, view).xyz;
    o.normal = mul(frame.gbufferModelViewInverse, float4(view_normal, 0.0)).xyz;
    o.overlay = a.entity_color;
    o.stage = a.render_stage;
    return o;
}

/// Where the middle of the texture pixel drawn here lies on the surface, so
/// shadows step texel by texel as on terrain. Found by stepping across the
/// screen (exact on a flat face, however many pixels the texel spans);
/// left where the step is unsure, at silhouettes or a degenerate mapping.
float3 texel_point(Varyings v, float3 dpx, float3 dpy, float2 dux, float2 duy) {
    float2 atlas = float2(iris_size(gtexture));
    float2 want = (floor(v.uv * atlas) + 0.5) / atlas - v.uv;
    float det = dux.x * duy.y - dux.y * duy.x;
    if (abs(det) < 1e-14) {
        return v.player;
    }
    float a = (want.x * duy.y - want.y * duy.x) / det;
    float b = (dux.x * want.y - dux.y * want.x) / det;
    float3 step = a * dpx + b * dpy;
    // A texel is at most a block or so across on any model.
    if (dot(step, step) > 1.0) {
        return v.player;
    }
    return v.player + step;
}

Lit fragment(Varyings v) {
    float4 c = gtexture.Sample(gtexture_sampler, v.uv) * v.color;
    float3 dpx = ddx(v.player);
    float3 dpy = ddy(v.player);
    float2 dux = ddx(v.uv);
    float2 duy = ddy(v.uv);
    if (!iris_alpha_pass(c.a)) {
        discard;
    }
    c = float4(lerp(c.rgb, v.overlay.rgb, v.overlay.a), c.a);
    float3 albedo = to_linear(c.rgb);
    float3 n = dot(v.normal, v.normal) > 1e-6 ? normalize(v.normal) : float3(0.0, 1.0, 0.0);
    float3 lit;
    if (v.stage == MC_RENDER_STAGE_CLOUDS) {
        float3 above = sky_color(float3(0.0, 1.0, 0.0));
        float3 around = sky_color(normalize(float3(v.player.x, 0.05, v.player.z)));
        lit = albedo * (direct_light() * 0.55 + above * 0.5 + around * 0.6 + float3(0.01));
    } else {
        float vis = shadow_visibility(texel_point(v, dpx, dpy, dux, duy), n, 2);
        lit = surface_light(albedo, n, v.light, vis, false);
    }
    Lit o;
    o.color = float4(lit, c.a);
    bool hand = v.stage == MC_RENDER_STAGE_HAND_SOLID || v.stage == MC_RENDER_STAGE_HAND_TRANSLUCENT;
    o.texel = hand ? TEXEL_HAND : TEXEL_LIT;
    return o;
}
