// Terrain (VULKAN_REWRITE_CHECKLIST.md phase 3): sections in shared vertex
// and index pools, culled on the GPU into indirect draws.
//
// The vertex is the mesher's 20-byte `PackedVertex`, read straight from the
// pool (vertex pulling): position as 4 × i16 (xyz in 1/256 block from the
// section's origin; w unused here), uv as 2 × u16 normalised, light as 4 ×
// u8 (block, sky), colour as 4 × u8 (tint, and ambient occlusion in alpha).

import common;

// One section's slot, matching `SlotMeta` in `src/vk/terrain.rs`.
// Counts stay integers: index offsets kept as float bits are denormals
// below 2^23, which the GPU may flush to zero.
struct Slot {
    float4 lo;           // xyz box min from the origin
    float4 hi;           // xyz box max
    int4 origin_base;    // xyz origin, w base vertex
    uint4 counts;        // x first index, y index count, z solid count, w flags
};

static const uint SLOT_LIVE = 1;
static const uint SLOT_WATER = 2;

// VkDrawIndexedIndirectCommand.
struct DrawCommand {
    uint index_count;
    uint instance_count;
    uint first_index;
    int vertex_offset;
    uint first_instance;
};

static const uint STREAM_SOLID = 0;
static const uint STREAM_CUTOUT = 1;
static const uint STREAM_WATER = 2;

// Whether the box `lo`..`hi` is inside the frustum of `view_proj`, with
// the planes taken from the matrix's rows (Gribb and Hartmann). Reversed
// depth: the near plane is z <= w, and the far plane (z >= 0) never culls
// with the infinite projection.
bool in_frustum(float4x4 view_proj, float3 lo, float3 hi) {
    float4 planes[6] = {
        view_proj[3] + view_proj[0],
        view_proj[3] - view_proj[0],
        view_proj[3] + view_proj[1],
        view_proj[3] - view_proj[1],
        view_proj[2],
        view_proj[3] - view_proj[2],
    };
    for (uint i = 0; i < 6; i++) {
        float4 p = planes[i];
        float3 v = float3(p.x > 0 ? hi.x : lo.x, p.y > 0 ? hi.y : lo.y, p.z > 0 ? hi.z : lo.z);
        if (dot(p.xyz, v) + p.w < 0) {
            return false;
        }
    }
    return true;
}

// Appends a command to `stream`'s list, one atomic per subgroup; `stream`
// must be the same in every lane.
void append(uint commands, uint counts, uint capacity, uint stream, bool keep, DrawCommand command) {
    uint n = WaveActiveCountBits(keep);
    uint base = 0;
    if (WaveIsFirstLane() && n > 0) {
        buffer_at(counts).InterlockedAdd(stream * 4, n, base);
    }
    base = WaveReadLaneFirst(base);
    if (keep) {
        uint slot = base + WavePrefixCountBits(keep);
        if (slot < capacity) {
            buffer_at(commands).Store<DrawCommand>((stream * capacity + slot) * 20, command);
        }
    }
}

// Push: ids.x slots, ids.z commands, ids.w counts, more.x slot count,
//       more.y capacity per stream, view_proj the camera
[shader("compute")]
[numthreads(64, 1, 1)]
void cull(uint3 id: SV_DispatchThreadID) {
    uint index = id.x;
    bool live = index < pc.more.x;
    Slot s = (Slot)0;
    bool keep = false;
    if (live) {
        s = buffer_at(pc.ids.x).Load<Slot>(index * 64);
        float3 origin = float3(s.origin_base.xyz);
        keep = (s.counts.w & SLOT_LIVE) != 0
            && in_frustum(pc.view_proj, origin + s.lo.xyz, origin + s.hi.xyz);
    }
    bool water = keep && (s.counts.w & SLOT_WATER) != 0;
    uint first = s.counts.x;
    uint total = s.counts.y;
    uint solid = s.counts.z;

    DrawCommand command;
    command.instance_count = 1;
    command.vertex_offset = s.origin_base.w;
    command.first_instance = index;

    // One stream per call: the subgroup shares one atomic, so every lane
    // must append to the same list.
    command.index_count = solid;
    command.first_index = first;
    append(pc.ids.z, pc.ids.w, pc.more.y, STREAM_SOLID, keep && !water && solid > 0, command);

    command.index_count = total;
    append(pc.ids.z, pc.ids.w, pc.more.y, STREAM_WATER, water && total > 0, command);

    command.index_count = total - solid;
    command.first_index = first + solid;
    append(pc.ids.z, pc.ids.w, pc.more.y, STREAM_CUTOUT, keep && !water && total > solid, command);
}

struct VertexOut {
    float4 position: SV_Position;
    float2 uv;
    float4 color;
    float3 world;
    nointerpolation uint slot;
};

int sign16(uint bits) {
    return int(bits << 16) >> 16;
}

// Push: ids.x slots, ids.z vertex pool, more.x atlas texture, more.y view
//       mode, view_proj the camera
[shader("vertex")]
VertexOut terrain_vertex(uint vertex: SV_VulkanVertexID, uint instance: SV_VulkanInstanceID) {
    RWByteAddressBuffer pool = buffer_at(pc.ids.z);
    uint4 raw = pool.Load4(vertex * 20);
    uint tail = pool.Load(vertex * 20 + 16);
    int3 local = int3(sign16(raw.x & 0xFFFF), sign16(raw.x >> 16), sign16(raw.y & 0xFFFF));
    float2 uv = float2(raw.z & 0xFFFF, raw.z >> 16) / 65535.0;
    float2 light = float2(raw.w & 0xFF, (raw.w >> 8) & 0xFF);
    float4 color = float4(tail & 0xFF, (tail >> 8) & 0xFF, (tail >> 16) & 0xFF, tail >> 24) / 255.0;

    Slot s = buffer_at(pc.ids.x).Load<Slot>(instance * 64);
    float3 world = float3(s.origin_base.xyz) + float3(local) / 256.0;

    // Until the lightmap is ported: daylight from the brighter of block
    // and sky light, on vanilla's brightness curve.
    float level = max(light.x, light.y) / 255.0;
    float bright = level / (4.0 - 3.0 * level);
    float shade = color.a * lerp(0.05, 1.0, bright);

    VertexOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    out.uv = uv;
    out.color = float4(color.rgb * shade, 1);
    out.world = world;
    out.slot = instance;
    return out;
}

float4 terrain_shade(VertexOut in, bool cutout) {
    if (pc.more.y == VIEW_NORMALS) {
        float3 n = normalize(cross(ddx(in.world), ddy(in.world)));
        return float4(n * 0.5 + 0.5, 1);
    }
    if (pc.more.y == VIEW_TEXTURE_INDEX) {
        uint i = in.slot * 2654435761u;
        return float4(float3(i & 255, (i >> 8) & 255, (i >> 16) & 255) / 255.0 * 0.8 + 0.2, 1);
    }
    float4 texel = texture_at(pc.more.x).Sample(g_samplers[SAMPLER_NEAREST], in.uv);
    if (cutout && texel.a < 0.1) {
        discard;
    }
    return float4(texel.rgb * in.color.rgb, texel.a);
}

[shader("fragment")]
float4 terrain_solid(VertexOut in): SV_Target {
    return float4(terrain_shade(in, false).rgb, 1);
}

[shader("fragment")]
float4 terrain_cutout(VertexOut in): SV_Target {
    return float4(terrain_shade(in, true).rgb, 1);
}

[shader("fragment")]
float4 terrain_water(VertexOut in): SV_Target {
    float4 c = terrain_shade(in, false);
    return float4(c.rgb, c.a * 0.8);
}
