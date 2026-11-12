// The GUI on the native renderer (VULKAN_REWRITE_CHECKLIST.md 7.1), as
// `src/gui/gui.wgsl`: the painter's vertices (`gui::pool::GuiVertex`, 32
// bytes: position in clip space, uv, colour), pulled from a buffer.
//
// Push: ids.x the vertex buffer; more.x the atlas, more.y the unihex font,
// more.z the glint, more.w the inventory figure (texture indices).

import common;

// UV offsets the painter marks quads with (`gui::draw`).
static const float FIGURE_UV_BIAS = 2.0;
static const float FIGURE_UV_TEST = -0.5;
static const float UNIHEX_UV_BIAS = 2.0;
static const float UNIHEX_UV_TEST = 1.5;
static const float GLINT_UV_BIAS = 2.0;
static const float GLINT_UV_TEST = 1.5;
// A 16 px item covers 1/64 of a 1024 px atlas.
static const float GLINT_SCALE = 0.125;
static const float GLINT_STRENGTH = 0.75;

struct GuiOut {
    float4 position : SV_Position;
    float2 uv;
    float4 color;
};

[shader("vertex")]
GuiOut gui_vertex(uint vertex : SV_VertexID) {
    ByteAddressBuffer v = buffer_ro(pc.ids.x);
    uint at = vertex * 32;
    GuiOut o;
    float2 p = asfloat(v.Load2(at));
    // The painter's y points up; Vulkan's clip y points down.
    o.position = float4(p.x, -p.y, 0, 1);
    o.uv = asfloat(v.Load2(at + 8));
    o.color = asfloat(v.Load4(at + 16));
    return o;
}

float3 srgb_encode(float3 c) {
    return select(c <= 0.0031308, c * 12.92, 1.055 * pow(c, 1.0 / 2.4) - 0.055);
}

float3 srgb_decode(float3 c) {
    return select(c <= 0.04045, c / 12.92, pow((c + 0.055) / 1.055, 2.4));
}

// Vanilla's item glint, as gui.wgsl's.
float3 glint(float2 uv, float2 phase) {
    float s = sin(3.14159265 / 18.0);
    float c = cos(3.14159265 / 18.0);
    float2 p = uv * GLINT_SCALE;
    float2 st = float2(c * p.x - s * p.y, s * p.x + c * p.y) + float2(-phase.x, phase.y);
    return texture_at(pc.more.z).SampleLevel(g_samplers[SAMPLER_LINEAR], st, 0).rgb * GLINT_STRENGTH;
}

[shader("fragment")]
float4 gui_fragment(GuiOut i) : SV_Target {
    SamplerState nearest = g_samplers[SAMPLER_NEAREST];
    if (i.uv.x < FIGURE_UV_TEST) {
        float2 uv = float2(i.uv.x + FIGURE_UV_BIAS, i.uv.y);
        // The figure is drawn at 2x: a linear sample averages 2x2 texels.
        return i.color * texture_at(pc.more.w).SampleLevel(g_samplers[SAMPLER_LINEAR], uv, 0);
    }
    if (i.uv.y >= GLINT_UV_TEST) {
        float2 uv = float2(i.uv.x, i.uv.y - GLINT_UV_BIAS);
        float4 icon = texture_at(pc.more.x).SampleLevel(nearest, uv, 0);
        float3 g = glint(i.color.ba, i.color.rg);
        float3 lifted = srgb_decode(min(srgb_encode(saturate(icon.rgb)) + g * g, 1.0));
        return float4(lifted, icon.a);
    }
    if (i.uv.x >= UNIHEX_UV_TEST) {
        float2 uv = float2(i.uv.x - UNIHEX_UV_BIAS, i.uv.y);
        float a = texture_at(pc.more.y).SampleLevel(nearest, uv, 0).r;
        return float4(i.color.rgb, i.color.a * a);
    }
    return i.color * texture_at(pc.more.x).SampleLevel(nearest, i.uv, 0);
}
