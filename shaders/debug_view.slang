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
