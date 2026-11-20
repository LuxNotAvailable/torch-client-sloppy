// Entity programs' input (entities, block entities, particles, the hand,
// the sky's sun and moon): each vertex built from the native renderer's
// buffers, posed meshes posed (`vk::entities`). An instance is 46 words, a
// vertex 13.

struct IrisVertexIn {
    uint vertex_index : SV_VulkanVertexID;
    uint instance_index : SV_VulkanInstanceID;
};

[SpecializationConstant] const uint IRIS_SLOT_iris_vk_inst = 0;
[SpecializationConstant] const uint IRIS_SLOT_iris_vk_mesh = 0;
[SpecializationConstant] const uint IRIS_SLOT_iris_vk_vert = 0;
[SpecializationConstant] const uint IRIS_SLOT_iris_vk_part = 0;

struct IrisVkIn {
    float3 position;
    float3 normal;
    float2 uv;
    float4 color;
    float4 model0;
    float4 model1;
    float4 model2;
    float4 tint;
    float4 overlay;
    float2 light;
    int entity_id;
    int block_entity_id;
    int item_id;
    int render_stage;
    uint flags;
};

uint iris_vk_inst(uint word) { return iris_buffer(IRIS_SLOT_iris_vk_inst).Load(4 * word); }
float4 iris_vk_f4(uint word) { return asfloat(iris_buffer(IRIS_SLOT_iris_vk_inst).Load4(4 * word)); }

IrisVkIn iris_vk_in(IrisVertexIn raw) {
    ByteAddressBuffer vert = iris_buffer(IRIS_SLOT_iris_vk_vert);
    uint at = raw.instance_index * 46;
    uint v = 4 * raw.vertex_index * 13;
    IrisVkIn e;
    float3 p = asfloat(vert.Load3(v));
    float3 n = asfloat(vert.Load3(v + 12));
    uint mesh = iris_vk_inst(at);
    if (iris_buffer(IRIS_SLOT_iris_vk_mesh).Load4(16 * mesh).w != 0) {
        ByteAddressBuffer parts = iris_buffer(IRIS_SLOT_iris_vk_part);
        uint row = (iris_vk_inst(at + 2) + vert.Load(v + 48)) * 3;
        float4 r0 = asfloat(parts.Load4(16 * row));
        float4 r1 = asfloat(parts.Load4(16 * (row + 1)));
        float4 r2 = asfloat(parts.Load4(16 * (row + 2)));
        float4 h = float4(p, 1.0);
        p = float3(dot(r0, h), dot(r1, h), dot(r2, h));
        // The normal through the cofactor matrix, the inverse transpose up
        // to the determinant.
        float3 c0 = float3(r0.x, r1.x, r2.x);
        float3 c1 = float3(r0.y, r1.y, r2.y);
        float3 c2 = float3(r0.z, r1.z, r2.z);
        float3 sn = cross(c1, c2) * n.x + cross(c2, c0) * n.y + cross(c0, c1) * n.z;
        float det = dot(c0, cross(c1, c2));
        if (dot(sn, sn) > 1e-12) {
            n = normalize(sn) * float(sign(det));
        }
    }
    e.position = p;
    e.normal = n;
    e.uv = asfloat(vert.Load2(v + 24));
    e.color = asfloat(vert.Load4(v + 32));
    e.tint = iris_vk_f4(at + 6);
    float4 light0 = iris_vk_f4(at + 14);
    float4 light1 = iris_vk_f4(at + 18);
    e.overlay = float4(light0.w < 0.5 ? float3(1.0, 0.0, 0.0) : float3(1.0), clamp(1.0 - light1.w, 0.0, 1.0));
    uint mode = iris_vk_inst(at + 28);
    e.light = mode == 2 ? float2(240.0) : asfloat(uint2(iris_vk_inst(at + 26), iris_vk_inst(at + 27)));
    e.flags = (mode == 1 || mode == 2) ? 1u : 0u;
    float4 m0 = iris_vk_f4(at + 30);
    float4 m1 = iris_vk_f4(at + 34);
    float4 m2 = iris_vk_f4(at + 38);
    float4 m3 = iris_vk_f4(at + 42);
    float3 eye = frame.iris_CameraPosition.xyz;
    e.model0 = float4(m0.x, m1.x, m2.x, m3.x - eye.x);
    e.model1 = float4(m0.y, m1.y, m2.y, m3.y - eye.y);
    e.model2 = float4(m0.z, m1.z, m2.z, m3.z - eye.z);
    e.entity_id = -1;
    e.block_entity_id = -1;
    e.item_id = 0;
    uint kind = iris_vk_inst(at + 5) & 0xFFFFu;
    e.render_stage = kind >= 16 ? int(kind) - 16 : 0;
    return e;
}

float3 iris_model(IrisVkIn input, float4 v) {
    return float3(dot(input.model0, v), dot(input.model1, v), dot(input.model2, v));
}

IrisAttributes iris_attributes(IrisVertexIn raw) {
    IrisVkIn input = iris_vk_in(raw);
    IrisAttributes a;
    a.position = iris_model(input, float4(input.position, 1.0));
    a.uv = input.uv;
    a.light = input.light;
    float4 color = input.color;
    if ((input.flags & 1u) != 0) {
        color.a = 1.0;
    }
    // Engine colours are linear; packs expect sRGB (`gl_Color`).
    float4 lin = color * input.tint;
    float3 lo = lin.rgb * 12.92;
    float3 hi = 1.055 * pow(max(lin.rgb, 0.0), 1.0 / 2.4) - 0.055;
    a.color = float4(select(lin.rgb <= 0.0031308, lo, hi), lin.a);
    float3 n = iris_model(input, float4(input.normal, 0.0));
    a.normal = dot(n, n) > 1e-8 ? normalize(n) : float3(0.0, 1.0, 0.0);
    a.entity = float4(-1.0, 0.0, 0.0, 1.0);
    a.mid_block = float4(0.0);
    a.entity_color = input.overlay;
    a.entity_id = input.entity_id;
    a.block_entity_id = input.block_entity_id;
    a.item_id = input.item_id;
    a.render_stage = input.render_stage;
    return a;
}
