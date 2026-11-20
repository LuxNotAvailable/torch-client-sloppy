// Terrain programs' input (gbuffers terrain and water, the shadow pass):
// each vertex pulled from its section's pool (`vk::terrain`, 128-byte
// slots: origin at word 2, the pool's bindless index at word 3's y, the
// quad pool's at word 3's w).

struct IrisVertexIn {
    uint vertex_index : SV_VulkanVertexID;
    uint instance_index : SV_VulkanInstanceID;
};

[SpecializationConstant] const uint IRIS_SLOT_iris_vk_slots = 0;
[SpecializationConstant] const uint IRIS_SLOT_iris_vk_anim = 0;
[SpecializationConstant] const uint IRIS_SLOT_iris_vk_voxels = 0;

/// The voxel window traced reflections read (`vk::voxels`).
ByteAddressBuffer iris_vk_voxels() { return iris_buffer(IRIS_SLOT_iris_vk_voxels); }

uint4 iris_vk_slot(uint instance, uint word) {
    return iris_buffer(IRIS_SLOT_iris_vk_slots).Load4(16 * (instance * 8 + word));
}

/// The block animation table (`block_anim`): the shift from the tile a quad
/// names (by its mid UV) to the frame showing.
float2 iris_anim_shift(uint4 quad) {
    ByteAddressBuffer anim = iris_buffer(IRIS_SLOT_iris_vk_anim);
    uint rows = anim.Load(0);
    uint count = anim.Load(4);
    if (rows == 0) {
        return float2(0.0);
    }
    float2 tiles = float2(16.0, float(rows));
    float2 mid = float2(float(quad.z & 65535u), float(quad.z >> 16u)) / 65535.0;
    float2 cell = floor(mid * tiles);
    uint tile = uint(cell.y) * 16 + uint(cell.x);
    if (tile >= count) {
        return float2(0.0);
    }
    uint a = anim.Load(4 * (2 + tile * 2)) & 65535u;
    return (float2(float(a % 16), float(a / 16)) - cell) / tiles;
}

uint4 iris_vk_quad(IrisVertexIn input) {
    uint pool = iris_vk_slot(input.instance_index, 3).w;
    if (pool == 0xFFFFFFFFu) {
        return uint4(0, 0, 0, 65535);
    }
    uint base = iris_vk_slot(input.instance_index, 2).w;
    return iris_buffer_at(pool).Load4(16 * (base + ((input.vertex_index - base) >> 2)));
}

int3 iris_origin(IrisVertexIn input) {
    return asint(iris_vk_slot(input.instance_index, 2).xyz);
}

float3 iris_snorm3(uint v) {
    int3 b = int3(asint(v << 24) >> 24, asint(v << 16) >> 24, asint(v << 8) >> 24);
    return max(float3(b) / 127.0, -1.0);
}

IrisAttributes iris_attributes(IrisVertexIn input) {
    ByteAddressBuffer pool = iris_buffer_at(iris_vk_slot(input.instance_index, 3).y);
    uint at = 4 * input.vertex_index * 5;
    uint x = pool.Load(at);
    uint y = pool.Load(at + 4);
    uint z = pool.Load(at + 8);
    uint l = pool.Load(at + 12);
    uint c = pool.Load(at + 16);
    int3 position = int3(asint(x << 16) >> 16, asint(x) >> 16, asint(y << 16) >> 16);
    float2 uv = float2(float(z & 65535u), float(z >> 16u)) / 65535.0;
    float2 light = float2(float(l & 255u), float((l >> 8u) & 255u)) / 255.0;
    float4 color = float4(float(c & 255u), float((c >> 8u) & 255u), float((c >> 16u) & 255u), float(c >> 24u)) / 255.0;
    uint4 quad = iris_vk_quad(input);

    float3 camera_block = floor(frame.iris_CameraPosition.xyz);
    float3 raw = float3(position) / 256.0;
    IrisAttributes a;
    a.position = float3(iris_origin(input) - int3(camera_block)) - (frame.iris_CameraPosition.xyz - camera_block) + raw;
    a.uv = uv + iris_anim_shift(quad);
    a.light = light * 255.0;
    a.color = color;
    float3 n = iris_snorm3(quad.x);
    a.normal = dot(n, n) > 0.25 ? normalize(n) : float3(0.0, 1.0, 0.0);
    uint id = quad.w & 65535u;
    a.entity = float4(id == 65535u ? -1.0 : float(id), 0.0, 0.0, 1.0);
    float3 cell = float3(float((quad.w >> 16u) & 15u), float((quad.w >> 20u) & 15u), float((quad.w >> 24u) & 15u));
    a.mid_block = float4((cell + 0.5 - raw) * 64.0, float(quad.w >> 28u));
    a.entity_color = float4(0.0);
    a.entity_id = -1;
    a.block_entity_id = -1;
    a.item_id = -1;
    a.render_stage = MC_RENDER_STAGE_TERRAIN_SOLID;
    return a;
}
