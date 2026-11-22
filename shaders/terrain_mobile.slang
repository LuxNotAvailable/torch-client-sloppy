// Mobile terrain vertex stage (docs/ANDROID_VS_PC.md, "Fixed terrain"):
// the mesher's 20-byte `PackedVertex` through fixed-function vertex input,
// not pulled from a storage buffer. Plain vertices only (packing off);
// the fragment stages stay `terrain.slang`'s.
//
// Attributes (binding 0, stride 20, as `FIXED_ATTRIBUTES` in
// `src/vk/terrain_fixed.rs`):
// 0 pos   R16G16B16A16_SINT  xyz in 1/256 block from the origin
// 1 uv    R16G16_UNORM
// 2 light R8G8B8A8_UINT      block, sky, material
// 3 color R8G8B8A8_UNORM     tint, AO in alpha
// The draw's first instance is the slot.

import common;
import world;
import terrain;

struct FixedIn {
    [[vk::location(0)]] int4 pos: POSITION;
    [[vk::location(1)]] float2 uv: TEXCOORD0;
    [[vk::location(2)]] uint4 light: TEXCOORD1;
    [[vk::location(3)]] float4 color: COLOR0;
};

[shader("vertex")]
VertexOut terrain_vertex_fixed(FixedIn v, uint instance: SV_VulkanInstanceID) {
    ByteAddressBuffer slots = buffer_ro(pc.ids.x);
    int3 origin = asint(slots.Load4(instance * SLOT_SIZE + 32)).xyz;
    float3 world = float3(origin) + float3(v.pos.xyz) / 256.0;
    float3 lit = lightmap_at(buffer_at(pc.ids.y), float2(v.light.xy));
    // As `terrain_vertex`'s plain branch: leaf material 1, split 0-2.
    uint leaf = v.light.z == 1 ? 1 : 0;
    uint outline = (1 << 7) | (1 << 15);

    VertexOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    out.uv = v.uv;
    out.color = float4(v.color.rgb * v.color.a * lit, 1);
    out.world = world;
    out.slot = instance;
    out.flags = leaf | (outline << 1);
    out.merged = uint4(0, 0, 0, 0);
    return out;
}
