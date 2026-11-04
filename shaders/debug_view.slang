// Debug views shared by every scene (`vkview --view`): a full-screen
// triangle and the depth buffer shown as distance fog.

import common;

// A full-screen view of a depth image (more.x its texture index), reversed
// depth with an infinite far plane: near / depth is the distance.
// params.x near plane, params.y the distance shown as mid-grey.
[shader("vertex")]
float4 fullscreen_vertex(uint vertex: SV_VertexID): SV_Position {
    float2 c = float2((vertex << 1) & 2, vertex & 2);
    return float4(c * 2 - 1, 0, 1);
}

[shader("fragment")]
float4 depth_fragment(float4 position: SV_Position): SV_Target {
    float d = texture_at(pc.more.x).Load(int3(int2(position.xy), 0)).r;
    if (d <= 0) {
        return float4(0.1, 0.0, 0.15, 1);
    }
    float distance = pc.params.x / d;
    float shade = exp(-distance / pc.params.y);
    return float4(shade, shade, shade, 1);
}

// One level of the terrain's occlusion pyramid (`src/vk/terrain.rs`),
// stretched over the screen: each texel the farthest depth of its area,
// shaded as distance like the depth view, so holes and stale levels show.
// more.x the pyramid buffer, more.yz the depth's size, more.w the level;
// params.x near plane, params.y the distance shown as mid-grey.
[shader("fragment")]
float4 hiz_fragment(float4 position: SV_Position): SV_Target {
    uint2 size0 = pc.more.yz;
    uint level = pc.more.w;
    uint2 size = size0;
    uint offset = 0;
    for (uint i = 1; i <= level; i++) {
        size = (size + 1) / 2;
        if (i < level) {
            offset += size.x * size.y;
        }
    }
    uint2 texel = min(uint2(position.xy) >> level, size - 1);
    float d = asfloat(buffer_at(pc.more.x).Load((offset + texel.y * size.x + texel.x) * 4));
    if (d <= 0) {
        return float4(0.1, 0.0, 0.15, 1);
    }
    float shade = exp(-(pc.params.x / d) / pc.params.y);
    return float4(shade, shade * 0.8, shade * 0.5, 1);
}
