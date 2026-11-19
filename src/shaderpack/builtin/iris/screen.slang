// Screen passes' input (deferred, composite, final): one triangle over the
// whole screen.

struct IrisVertexIn {
    uint vertex_index : SV_VulkanVertexID;
};

IrisAttributes iris_attributes(IrisVertexIn input) {
    float2 corner = float2(float((input.vertex_index << 1) & 2), float(input.vertex_index & 2));
    IrisAttributes a;
    a.position = float3(corner, 0.0);
    a.uv = corner;
    a.light = float2(240.0);
    a.color = float4(1.0);
    a.normal = float3(0.0, 0.0, 1.0);
    a.entity = float4(-1.0, 0.0, 0.0, 1.0);
    a.mid_block = float4(0.0);
    a.entity_color = float4(0.0);
    a.entity_id = -1;
    a.block_entity_id = -1;
    a.item_id = -1;
    a.render_stage = MC_RENDER_STAGE_NONE;
    return a;
}

/// The whole screen, for a pass's vertex stage.
float4 iris_screen(IrisVertexIn input) {
    float2 corner = iris_attributes(input).uv;
    return iris_clip(float4(corner * 2.0 - 1.0, 0.0, 1.0));
}
