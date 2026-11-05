// The frame's last full-screen pass (VULKAN_REWRITE_CHECKLIST.md 5.3):
// reads the scene (drawn larger under SSAA), applies the server's post
// effects as `post_effects.wgsl` does, then FXAA, and writes the output.
//
// Push: more.x the scene's texture index, more.y the effect flags, more.z
// 1 for FXAA; params.xy the scene's texel size, params.zw the output's
// size in pixels.

import common;

static const uint CREEPER = 1;
static const uint SPIDER = 2;
static const uint INVERT = 4;
static const uint BLUR = 8;

static const float GOLDEN_ANGLE = 2.39996323;
static const int TAPS = 24;

[shader("vertex")]
float4 post_vertex(uint vertex: SV_VertexID): SV_Position {
    float2 c = float2((vertex << 1) & 2, vertex & 2);
    return float4(c * 2 - 1, 0, 1);
}

float3 tap(float2 uv) {
    return texture_at(pc.more.x).SampleLevel(g_samplers[SAMPLER_LINEAR], saturate(uv), 0).rgb;
}

// Disc blur on a golden-angle spiral, `radius_px` in output pixels.
float3 blurred(float2 uv, float radius_px) {
    float2 texel = 1.0 / pc.params.zw;
    float3 sum = 0;
    for (int i = 0; i < TAPS; i++) {
        float r = sqrt((float(i) + 0.5) / float(TAPS)) * radius_px;
        float a = float(i) * GOLDEN_ANGLE;
        sum += tap(uv + float2(cos(a), sin(a)) * r * texel);
    }
    return sum / float(TAPS);
}

// A spider's compound view: magnified eyes over a blurred field, tinted red.
float3 spider(float2 uv) {
    float3 color = blurred(uv, 16.0);
    float aspect = pc.params.z / pc.params.w;
    float3 eyes[5] = {
        float3(0.5, 0.45, 0.30),
        float3(0.22, 0.32, 0.17),
        float3(0.78, 0.32, 0.17),
        float3(0.33, 0.75, 0.14),
        float3(0.67, 0.75, 0.14),
    };
    for (int i = 0; i < 5; i++) {
        float3 eye = eyes[i];
        float edge = length((uv - eye.xy) * float2(aspect, 1.0)) / eye.z;
        if (edge < 1.0) {
            float3 view = tap(0.5 + (uv - eye.xy) / (eye.z * 2.0));
            color = lerp(view, color, smoothstep(0.7, 1.0, edge));
        }
    }
    return color * float3(1.0, 0.8, 0.8);
}

// The scene with the post effects, at `uv`.
float3 graded(float2 uv) {
    uint flags = pc.more.y;
    if ((flags & CREEPER) != 0) {
        float2 cells = pc.params.zw / 4.0;
        uv = (floor(uv * cells) + 0.5) / cells;
    }
    float3 color;
    if ((flags & SPIDER) != 0) {
        color = spider(uv);
    } else if ((flags & BLUR) != 0) {
        color = blurred(uv, 8.0);
    } else {
        color = tap(uv);
    }
    if ((flags & CREEPER) != 0) {
        // Green-only luma, posterised to 16 levels.
        float luma = dot(color, float3(0.3, 0.59, 0.11));
        color = float3(0.0, floor(luma * 16.0) / 16.0, 0.0);
    }
    if ((flags & INVERT) != 0) {
        color = lerp(color, 1.0 - color, 0.8);
    }
    return color;
}

// Perceived brightness for FXAA's edge search (gamma-ish, from linear).
float luma(float3 c) {
    return sqrt(dot(c, float3(0.299, 0.587, 0.114)));
}

// FXAA in the style of 3.11's quality preset, on the output's pixels: find
// the local edge's direction from the 3×3 lumas, walk along it to both
// ends, and blend across it by how far this pixel sits from the nearer one.
float3 fxaa(float2 uv) {
    float2 px = 1.0 / pc.params.zw;
    float3 center = graded(uv);
    float m = luma(center);
    float n = luma(graded(uv + float2(0, -px.y)));
    float s = luma(graded(uv + float2(0, px.y)));
    float e = luma(graded(uv + float2(px.x, 0)));
    float w = luma(graded(uv + float2(-px.x, 0)));
    float lo = min(m, min(min(n, s), min(e, w)));
    float hi = max(m, max(max(n, s), max(e, w)));
    float range = hi - lo;
    if (range < max(0.0312, hi * 0.125)) {
        return center;
    }
    float nw = luma(graded(uv + float2(-px.x, -px.y)));
    float ne = luma(graded(uv + float2(px.x, -px.y)));
    float sw = luma(graded(uv + float2(-px.x, px.y)));
    float se = luma(graded(uv + float2(px.x, px.y)));

    float horizontal = abs(nw + ne - 2 * n) + 2 * abs(w + e - 2 * m) + abs(sw + se - 2 * s);
    float vertical = abs(nw + sw - 2 * w) + 2 * abs(n + s - 2 * m) + abs(ne + se - 2 * e);
    bool is_horizontal = horizontal >= vertical;

    // Step across the edge towards its steeper side.
    float l1 = is_horizontal ? n : w;
    float l2 = is_horizontal ? s : e;
    float g1 = abs(l1 - m);
    float g2 = abs(l2 - m);
    float step_len = is_horizontal ? px.y : px.x;
    float edge_luma;
    float gradient;
    if (g1 >= g2) {
        step_len = -step_len;
        edge_luma = (l1 + m) * 0.5;
        gradient = g1;
    } else {
        edge_luma = (l2 + m) * 0.5;
        gradient = g2;
    }
    float2 edge_uv = uv;
    float2 along = is_horizontal ? float2(px.x, 0) : float2(0, px.y);
    if (is_horizontal) {
        edge_uv.y += step_len * 0.5;
    } else {
        edge_uv.x += step_len * 0.5;
    }

    // Walk both ways until the luma leaves the edge's.
    float scaled = gradient * 0.25;
    float2 uv_a = edge_uv - along;
    float2 uv_b = edge_uv + along;
    float end_a = luma(graded(uv_a)) - edge_luma;
    float end_b = luma(graded(uv_b)) - edge_luma;
    static const float STEPS[10] = { 1.0, 1.0, 1.0, 1.0, 1.5, 2.0, 2.0, 2.0, 4.0, 8.0 };
    for (int i = 0; i < 10; i++) {
        bool done_a = abs(end_a) >= scaled;
        bool done_b = abs(end_b) >= scaled;
        if (done_a && done_b) {
            break;
        }
        if (!done_a) {
            uv_a -= along * STEPS[i];
            end_a = luma(graded(uv_a)) - edge_luma;
        }
        if (!done_b) {
            uv_b += along * STEPS[i];
            end_b = luma(graded(uv_b)) - edge_luma;
        }
    }
    float dist_a = is_horizontal ? uv.x - uv_a.x : uv.y - uv_a.y;
    float dist_b = is_horizontal ? uv_b.x - uv.x : uv_b.y - uv.y;
    bool nearer_a = dist_a < dist_b;
    float nearest = min(dist_a, dist_b);
    float span = dist_a + dist_b;
    // Only blend when the nearer end turns the way this pixel does.
    bool m_below = m - edge_luma < 0;
    bool good = ((nearer_a ? end_a : end_b) < 0) != m_below;
    float offset = good ? 0.5 - nearest / span : 0.0;

    // Sub-pixel aliasing: a lone bright or dark pixel.
    float average = (2 * (n + s + e + w) + nw + ne + sw + se) / 12.0;
    float sub = saturate(abs(average - m) / range);
    sub = smoothstep(0.0, 1.0, sub);
    offset = max(offset, sub * sub * 0.75);

    float2 final_uv = uv;
    if (is_horizontal) {
        final_uv.y += offset * step_len;
    } else {
        final_uv.x += offset * step_len;
    }
    return graded(final_uv);
}

[shader("fragment")]
float4 post_fragment(float4 position: SV_Position): SV_Target {
    float2 uv = position.xy / pc.params.zw;
    float3 color = pc.more.z != 0 ? fxaa(uv) : graded(uv);
    return float4(color, 1);
}
