// The viewer's test scene (VULKAN_REWRITE_CHECKLIST.md 2.9): a field of
// textured tiles. A compute pass culls them and compacts the survivors with
// subgroup operations into an indirect draw, and the draw reads them back,
// so one scene exercises bindless textures and buffers, compute, indirect
// count, depth and the render graph's barriers.

import common;

// One tile: centre (xyz) and half size (w).
struct Tile {
    float4 centre_size;
};

// The indirect command the cull fills: vertexCount, instanceCount,
// firstVertex, firstInstance (VkDrawIndirectCommand).
static const uint COMMAND_INSTANCES = 4;

// Push slots for the cull:
//   ids.x tiles in, ids.y tiles kept, ids.z indirect command, more.x count
//   params.xyz camera, params.w keep radius
[shader("compute")]
[numthreads(64, 1, 1)]
void cull(uint3 id: SV_DispatchThreadID) {
    uint total = pc.more.x;
    bool keep = false;
    Tile tile;
    if (id.x < total) {
        tile = buffer_at(pc.ids.x).Load<Tile>(id.x * 16);
        keep = distance(tile.centre_size.xyz, pc.params.xyz) <= pc.params.w;
    }
    // One atomic per subgroup reserves room for all its survivors.
    uint count = WaveActiveCountBits(keep);
    uint base = 0;
    if (WaveIsFirstLane() && count > 0) {
        buffer_at(pc.ids.z).InterlockedAdd(COMMAND_INSTANCES, count, base);
    }
    base = WaveReadLaneFirst(base);
    if (keep) {
        uint slot = base + WavePrefixCountBits(keep);
        buffer_at(pc.ids.y).Store<Tile>(slot * 16, tile);
    }
}

struct VertexOut {
    float4 position: SV_Position;
    float2 uv;
    float3 normal;
    nointerpolation uint texture;
};

// Push slots for the draw:
//   ids.y tiles kept, more.x texture, more.y view mode
//   view_proj camera
[shader("vertex")]
VertexOut tile_vertex(uint vertex: SV_VertexID, uint instance: SV_InstanceID) {
    static const float2 corners[6] = {
        float2(0, 0), float2(1, 0), float2(1, 1),
        float2(0, 0), float2(1, 1), float2(0, 1),
    };
    Tile tile = buffer_at(pc.ids.y).Load<Tile>(instance * 16);
    float2 c = corners[vertex];
    float3 world = tile.centre_size.xyz + float3(c.x * 2 - 1, 0, c.y * 2 - 1) * tile.centre_size.w;
    VertexOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    out.uv = c;
    out.normal = float3(0, 1, 0);
    out.texture = pc.more.x;
    return out;
}

[shader("fragment")]
float4 tile_fragment(VertexOut in): SV_Target {
    if (pc.more.y == VIEW_NORMALS) {
        return float4(in.normal * 0.5 + 0.5, 1);
    }
    if (pc.more.y == VIEW_TEXTURE_INDEX) {
        uint i = in.texture;
        return float4(frac(float3(i * 0.37, i * 0.61, i * 0.83)) * 0.8 + 0.2, 1);
    }
    return texture_at(in.texture).Sample(g_samplers[SAMPLER_NEAREST], in.uv * 4);
}

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
