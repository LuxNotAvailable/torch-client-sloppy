// Mobile terrain vertex stage (docs/ANDROID_VS_PC.md, "Fixed terrain"):
// the mesher's 20-byte `PackedVertex` through fixed-function vertex input,
// not pulled from a storage buffer, and a slimmer vertex-to-fragment
// interface (`MobileOut`). Plain vertices only (packing off); the shading
// is `terrain.slang`'s.
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

// What the fragment stage reads of a plain quad: a tiler writes every
// output per vertex to its parameter buffer, so `VertexOut`'s slot and
// merged record (always 0 here) stay out. 9 values, not 15.
struct MobileOut {
    float4 position: SV_Position;
    float2 uv;
    float3 color;
    float3 world;
    nointerpolation uint flags;
};

// The full interface for the shared shading; slot only colours the
// texture-index debug view.
VertexOut widen(MobileOut m) {
    VertexOut out;
    out.position = m.position;
    out.uv = m.uv;
    out.color = float4(m.color, 1);
    out.world = m.world;
    out.slot = 0;
    out.flags = m.flags;
    out.merged = uint4(0, 0, 0, 0);
    return out;
}

[shader("vertex")]
MobileOut terrain_vertex_fixed(FixedIn v, uint instance: SV_VulkanInstanceID) {
    ByteAddressBuffer slots = buffer_ro(pc.ids.x);
    int3 origin = asint(slots.Load4(instance * SLOT_SIZE + 32)).xyz;
    float3 world = float3(origin) + float3(v.pos.xyz) / 256.0;
    float3 lit = lightmap_at(buffer_at(pc.ids.y), float2(v.light.xy));
    // As `terrain_vertex`'s plain branch: leaf material 1, split 0-2.
    uint leaf = v.light.z == 1 ? 1 : 0;
    uint outline = (1 << 7) | (1 << 15);

    MobileOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    out.uv = v.uv;
    out.color = v.color.rgb * v.color.a * lit;
    out.world = world;
    out.flags = leaf | (outline << 1);
    return out;
}

// `terrain.slang`'s fragment entries over `MobileOut`.
[shader("fragment")]
float4 terrain_solid_mobile(MobileOut m): SV_Target {
    return float4(terrain_shade(widen(m), false).rgb, 1);
}

[shader("fragment")]
float4 terrain_cutout_mobile(MobileOut m): SV_Target {
    float4 c = terrain_shade(widen(m), true);
    if (c.a < 0) {
        discard;
    }
    return float4(c.rgb, 1);
}

[shader("fragment")]
float4 terrain_translucent_mobile(MobileOut m): SV_Target {
    float4 c = terrain_shade(widen(m), false);
    if (pc.more.y != VIEW_SHADED && pc.more.y != VIEW_OUTLINES) {
        return float4(c.rgb, 0.6);
    }
    return float4(c.rgb, saturate(c.a * pc.params.w));
}
