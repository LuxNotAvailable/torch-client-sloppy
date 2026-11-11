// The title screen's panorama (VULKAN_REWRITE_CHECKLIST.md 4.6), as
// `renderer::panorama` draws it: six faces of a cube around the camera,
// each with its own image, seen through an 85 degree lens that turns
// slowly with a 10 degree downward pitch. Thirty-six vertices from the
// vertex index; no buffers.

import common;

struct PanoramaOut {
    float4 position: SV_Position;
    float2 uv;
    nointerpolation uint face;
};

// Each face's normal, right and up (`renderer::panorama::FACES`).
static const float3 NORMALS[6] = {
    float3(0, 0, -1), float3(1, 0, 0), float3(0, 0, 1),
    float3(-1, 0, 0), float3(0, 1, 0), float3(0, -1, 0),
};
static const float3 RIGHTS[6] = {
    float3(1, 0, 0), float3(0, 0, 1), float3(-1, 0, 0),
    float3(0, 0, -1), float3(1, 0, 0), float3(1, 0, 0),
};
static const float3 UPS[6] = {
    float3(0, 1, 0), float3(0, 1, 0), float3(0, 1, 0),
    float3(0, 1, 0), float3(0, 0, 1), float3(0, 0, -1),
};
// The face's two triangles over its corners: top left, top right, bottom
// right, bottom left (`face_mesh`'s 0, 2, 1, 0, 3, 2).
static const uint CORNER_ORDER[6] = { 0, 2, 1, 0, 3, 2 };
static const float2 CORNERS[4] = {
    float2(-1, 1), float2(1, 1), float2(1, -1), float2(-1, -1),
};

// Push: ids and more.xy the six faces' textures; view_proj the camera
// (its rotation and lens, no translation).
[shader("vertex")]
PanoramaOut panorama_vertex(uint vertex: SV_VulkanVertexID) {
    uint face = vertex / 6;
    float2 corner = CORNERS[CORNER_ORDER[vertex % 6]];
    float3 p = NORMALS[face] + RIGHTS[face] * corner.x + UPS[face] * corner.y;
    PanoramaOut out;
    out.position = mul(pc.view_proj, float4(p, 1));
    out.uv = float2(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    out.face = face;
    return out;
}

[shader("fragment")]
float4 panorama_fragment(PanoramaOut in): SV_Target {
    uint texture = in.face < 4 ? pc.ids[in.face] : pc.more[in.face - 4];
    // The shared linear sampler repeats; the game's clamps. Keep half a
    // texel in from each edge so a face never blends in its far side.
    uint w, h;
    texture_at(texture).GetDimensions(w, h);
    float2 inset = 0.5 / float2(max(w, 1), max(h, 1));
    float2 uv = clamp(in.uv, inset, 1.0 - inset);
    return float4(texture_at(texture).SampleLevel(g_samplers[SAMPLER_LINEAR], uv, 0).rgb, 1);
}
