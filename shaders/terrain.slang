// Terrain (VULKAN_REWRITE_CHECKLIST.md phase 3): sections in shared vertex
// and index pools, culled on the GPU into indirect draws.
//
// The vertex is the mesher's 20-byte `PackedVertex`, read straight from the
// slot's vertex pool (vertex pulling): position as 4 × i16 (xyz in 1/256
// block from the section's origin; w unused here), uv as 2 × u16
// normalised, light as 4 × u8 (block, sky as level × 16), colour as 4 × u8
// (tint, and ambient occlusion in alpha).
//
// A section's opaque indices are sorted into seven face buckets (+x, -x,
// +y, -y, +z, -z, and faces on no axis), then its cutout indices follow.
// The cull draws only the buckets that can face the camera (3.3).

import common;

// One section layer's slot, matching `Slot` in `src/vk/terrain.rs`.
// Counts stay integers: index offsets kept as float bits are denormals
// below 2^23, which the GPU may flush to zero.
struct Slot {
    float4 lo;           // xyz box min from the origin
    float4 hi;           // xyz box max
    int4 origin_base;    // xyz origin, w base vertex
    uint4 info;          // x pool, y vertex buffer, z flags, w unused
    uint4 ranges;        // x first index, y cutout (or translucent) count
    uint4 buckets0;      // +x, -x, +y, -y index counts
    uint4 buckets1;      // +z, -z, no axis, unused
    uint4 spare;
};

static const uint SLOT_SIZE = 128;
static const uint SLOT_LIVE = 1;
static const uint SLOT_TRANSLUCENT = 2;

// The frame's cull inputs, matching `CullView` in `src/vk/terrain.rs`.
// Read field by field: loading it whole crashed slangc 2026.1.
static const uint VIEW_VIEW_PROJ = 0;
static const uint VIEW_PREV_VIEW_PROJ = 64;
static const uint VIEW_CAMERA = 128;
static const uint VIEW_HIZ = 144;
static const uint VIEW_FLAGS = 160;

static const uint CULL_HIZ = 1;
static const uint CULL_VISIBILITY = 2;
static const uint CULL_BUCKETS = 4;
static const uint CULL_FRUSTUM = 8;

// VkDrawIndexedIndirectCommand.
struct DrawCommand {
    uint index_count;
    uint instance_count;
    uint first_index;
    int vertex_offset;
    uint first_instance;
};

static const uint COMMAND_SIZE = 20;

// A column-major matrix stored by `glam::Mat4::to_cols_array`.
struct Columns {
    float4 c[4];

    float4 apply(float3 p) {
        return c[0] * p.x + c[1] * p.y + c[2] * p.z + c[3];
    }

    float4 row(uint i) {
        return float4(c[0][i], c[1][i], c[2][i], c[3][i]);
    }
};

Columns load_columns(RWByteAddressBuffer b, uint at) {
    Columns m;
    for (uint i = 0; i < 4; i++) {
        m.c[i] = asfloat(b.Load4(at + i * 16));
    }
    return m;
}

// Whether the box `lo`..`hi` is inside the frustum, with the planes taken
// from the matrix's rows (Gribb and Hartmann). Reversed depth: the near
// plane is z <= w, and the far plane (z >= 0) never culls with the infinite
// projection.
bool in_frustum(Columns m, float3 lo, float3 hi) {
    float4 r0 = m.row(0), r1 = m.row(1), r2 = m.row(2), r3 = m.row(3);
    float4 planes[6] = { r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2 };
    for (uint i = 0; i < 6; i++) {
        float4 p = planes[i];
        float3 v = float3(p.x > 0 ? hi.x : lo.x, p.y > 0 ? hi.y : lo.y, p.z > 0 ? hi.z : lo.z);
        if (dot(p.xyz, v) + p.w < 0) {
            return false;
        }
    }
    return true;
}

// The pyramid's levels: level 1 is half the depth's size (rounded up),
// each next one half the last; level 0 (the depth itself) isn't stored.
uint2 level_size(uint level, uint2 size0) {
    uint2 s = size0;
    for (uint i = 0; i < level; i++) {
        s = (s + 1) / 2;
    }
    return s;
}

uint level_offset(uint level, uint2 size0) {
    uint2 s = size0;
    uint offset = 0;
    for (uint i = 1; i < level; i++) {
        s = (s + 1) / 2;
        offset += s.x * s.y;
    }
    return offset;
}

// Whether the box is hidden behind last frame's depth (3.4): projected with
// last frame's camera, so the pyramid lines up, its nearest depth is
// farther than the farthest depth over the area it covers. Anything that
// was partly off last frame's screen, or crosses its near plane, counts as
// visible.
bool hiz_occluded(RWByteAddressBuffer view, float3 lo, float3 hi) {
    uint4 hiz = view.Load4(VIEW_HIZ);
    Columns m = load_columns(view, VIEW_PREV_VIEW_PROJ);
    float2 mn = 1e9;
    float2 mx = -1e9;
    float nearest = 0;
    for (uint i = 0; i < 8; i++) {
        float3 corner = float3((i & 1) != 0 ? hi.x : lo.x, (i & 2) != 0 ? hi.y : lo.y, (i & 4) != 0 ? hi.z : lo.z);
        float4 c = m.apply(corner);
        if (c.w <= 0.05) {
            return false;
        }
        float3 ndc = c.xyz / c.w;
        float2 uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        mn = min(mn, uv);
        mx = max(mx, uv);
        nearest = max(nearest, ndc.z);
    }
    if (any(mn < 0) || any(mx > 1)) {
        return false;
    }
    uint2 size0 = hiz.zw;
    float2 px_lo = mn * float2(size0);
    float2 px_hi = mx * float2(size0);
    float extent = max(px_hi.x - px_lo.x, px_hi.y - px_lo.y);
    uint level = clamp(uint(ceil(log2(max(extent, 1.0)))), 1u, hiz.y);
    uint2 size = level_size(level, size0);
    float scale = exp2(-float(level));
    int2 a = min(int2(px_lo * scale), int2(size) - 1);
    int2 b = min(int2(px_hi * scale), int2(size) - 1);
    RWByteAddressBuffer pyramid = buffer_at(hiz.x);
    uint offset = level_offset(level, size0);
    float far = 1;
    for (int y = a.y; y <= min(b.y, a.y + 2); y++) {
        for (int x = a.x; x <= min(b.x, a.x + 2); x++) {
            far = min(far, asfloat(pyramid.Load((offset + y * size.x + x) * 4)));
        }
    }
    return nearest < far;
}

// Reserves `n` commands in `list` and returns where this lane's start. The
// lanes of a subgroup that name the same list share one atomic; the loop
// takes one list at a time.
uint reserve(uint counts, uint list, uint n) {
    for (;;) {
        uint leader = WaveReadLaneFirst(list);
        if (list == leader) {
            uint total = WaveActiveSum(n);
            uint prefix = WavePrefixSum(n);
            uint base = 0;
            if (WaveIsFirstLane() && total > 0) {
                buffer_at(counts).InterlockedAdd(list * 4, total, base);
            }
            return WaveReadLaneFirst(base) + prefix;
        }
    }
    return 0;
}

void write_command(uint commands, uint at, uint count, uint first, int base, uint slot, uint instances) {
    DrawCommand command;
    command.index_count = count;
    command.instance_count = instances;
    command.first_index = first;
    command.vertex_offset = base;
    command.first_instance = slot;
    buffer_at(commands).Store<DrawCommand>(at * COMMAND_SIZE, command);
}

// Push: ids.x slots, ids.y visible-slot bits, ids.z commands, ids.w counts,
//       more.x slot count, more.y commands per list, more.z the cull view,
//       more.w where the translucent commands start (in commands); params.x
//       the translucent rank buffer (as bits).
// Lists: pool * 2 for solid, pool * 2 + 1 for cutout. Translucent slots
// write their command at their rank (far to near, from the CPU), culled or
// not, so translucent layers draw back to front across sections (3.5, 3.7).
[shader("compute")]
[numthreads(64, 1, 1)]
void cull(uint3 id: SV_DispatchThreadID) {
    uint index = id.x;
    RWByteAddressBuffer view = buffer_at(pc.more.z);
    uint flags = view.Load(VIEW_FLAGS);
    float3 camera = asfloat(view.Load3(VIEW_CAMERA));
    Slot s = (Slot)0;
    if (index < pc.more.x) {
        s = buffer_at(pc.ids.x).Load<Slot>(index * SLOT_SIZE);
    }
    bool live = (s.info.z & SLOT_LIVE) != 0;
    bool translucent = live && (s.info.z & SLOT_TRANSLUCENT) != 0;
    float3 origin = float3(s.origin_base.xyz);
    float3 lo = origin + s.lo.xyz;
    float3 hi = origin + s.hi.xyz;
    bool keep = live;
    if (keep && (flags & CULL_VISIBILITY) != 0) {
        uint word = buffer_at(pc.ids.y).Load((index / 32) * 4);
        keep = ((word >> (index % 32)) & 1) != 0;
    }
    if (keep && (flags & CULL_FRUSTUM) != 0) {
        keep = in_frustum(load_columns(view, VIEW_VIEW_PROJ), lo, hi);
    }
    if (keep && (flags & CULL_HIZ) != 0) {
        keep = !hiz_occluded(view, lo, hi);
    }
    int base = s.origin_base.w;
    uint first = s.ranges.x;

    if (translucent) {
        uint rank = buffer_at(asuint(pc.params.x)).Load(index * 4);
        write_command(pc.ids.z, pc.more.w + rank, s.ranges.y, first, base, index, keep ? 1 : 0);
    }

    // Solid: the visible buckets, neighbours merged into one command (an
    // empty bucket between two visible ones doesn't split them).
    bool solid = keep && !translucent;
    uint counts[7] = { s.buckets0.x, s.buckets0.y, s.buckets0.z, s.buckets0.w, s.buckets1.x, s.buckets1.y, s.buckets1.z };
    bool facing[7] = {
        camera.x > lo.x, camera.x < hi.x,
        camera.y > lo.y, camera.y < hi.y,
        camera.z > lo.z, camera.z < hi.z,
        true,
    };
    uint run_first[4];
    uint run_count[4];
    uint runs = 0;
    uint at = first;
    bool open = false;
    for (uint b = 0; b < 7; b++) {
        bool drawn = solid && counts[b] > 0 && ((flags & CULL_BUCKETS) == 0 || facing[b]);
        if (drawn) {
            if (!open) {
                run_first[runs] = at;
                run_count[runs] = 0;
                runs++;
                open = true;
            }
            run_count[runs - 1] += counts[b];
        } else if (counts[b] > 0) {
            open = false;
        }
        at += counts[b];
    }
    uint pool = s.info.x;
    uint start = reserve(pc.ids.w, pool * 2, runs);
    for (uint r = 0; r < runs; r++) {
        if (start + r < pc.more.y) {
            write_command(pc.ids.z, (pool * 2) * pc.more.y + start + r, run_count[r], run_first[r], base, index, 1);
        }
    }

    // Cutout, after the buckets.
    uint cutout = solid && s.ranges.y > 0 ? 1 : 0;
    uint slot_at = reserve(pc.ids.w, pool * 2 + 1, cutout);
    if (cutout > 0 && slot_at < pc.more.y) {
        write_command(pc.ids.z, (pool * 2 + 1) * pc.more.y + slot_at, s.ranges.y, at, base, index, 1);
    }
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

// Push: ids.x slots, more.x atlas texture, more.y view mode, view_proj the
//       camera.
[shader("vertex")]
VertexOut terrain_vertex(uint vertex: SV_VulkanVertexID, uint instance: SV_VulkanInstanceID) {
    // Only the two fields a vertex needs, not the whole slot.
    ByteAddressBuffer slots = buffer_ro(pc.ids.x);
    int4 origin_base = asint(slots.Load4(instance * SLOT_SIZE + 32));
    ByteAddressBuffer pool = buffer_ro(slots.Load(instance * SLOT_SIZE + 52));
    uint4 raw = pool.Load4(vertex * 20);
    uint tail = pool.Load(vertex * 20 + 16);
    int3 local = int3(sign16(raw.x & 0xFFFF), sign16(raw.x >> 16), sign16(raw.y & 0xFFFF));
    float2 uv = float2(raw.z & 0xFFFF, raw.z >> 16) / 65535.0;
    float2 light = float2(raw.w & 0xFF, (raw.w >> 8) & 0xFF);
    float4 color = float4(tail & 0xFF, (tail >> 8) & 0xFF, (tail >> 16) & 0xFF, tail >> 24) / 255.0;

    float3 world = float3(origin_base.xyz) + float3(local) / 256.0;

    // Until the lightmap is ported (phase 5): daylight from the brighter of
    // block and sky light, on vanilla's brightness curve.
    float level = max(light.x, light.y) / 240.0;
    float bright = level / (4.0 - 3.0 * level);
    float shade = color.a * lerp(0.05, 1.0, saturate(bright));

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

// Vanilla's translucent layer: water, stained glass, ice, slime, honey and
// the portal, blended with the texture's own alpha.
[shader("fragment")]
float4 terrain_translucent(VertexOut in): SV_Target {
    float4 c = terrain_shade(in, false);
    return pc.more.y == VIEW_SHADED ? c : float4(c.rgb, 0.6);
}

groupshared float tile[32][32];

float source_depth(int2 p, uint2 size, uint level) {
    if (any(p < 0) || any(p >= int2(size))) {
        // Outside the image: nearest, which never wins the farthest.
        return 1;
    }
    if (level == 0) {
        return texture_at(pc.ids.y).Load(int3(p, 0)).r;
    }
    uint2 size0 = pc.more.yz;
    return asfloat(buffer_at(pc.ids.x).Load((level_offset(level, size0) + p.y * size.x + p.x) * 4));
}

void store_level(uint level, int2 p, float depth) {
    uint2 size0 = pc.more.yz;
    uint2 size = level_size(level, size0);
    if (level <= pc.more.w && all(p < int2(size))) {
        buffer_at(pc.ids.x).Store((level_offset(level, size0) + p.y * size.x + p.x) * 4, asuint(depth));
    }
}

// Builds six pyramid levels above `level` (more.x) in one dispatch, the way
// AMD's single-pass downsampler does: each group reduces a 64 × 64 tile
// through shared memory. Two dispatches cover a 4096-wide frame. Each
// texel keeps the farthest depth (the smallest, with reversed depth).
// Push: ids.x pyramid buffer, ids.y depth texture, more.x source level,
//       more.yz the depth's size, more.w the top level.
[shader("compute")]
[numthreads(16, 16, 1)]
void hiz_reduce(uint3 group: SV_GroupID, uint3 thread: SV_GroupThreadID) {
    uint level = pc.more.x;
    uint2 size = level_size(level, pc.more.yz);
    int2 t = int2(thread.xy);
    for (int dy = 0; dy < 2; dy++) {
        for (int dx = 0; dx < 2; dx++) {
            int2 o = int2(group.xy) * 32 + t * 2 + int2(dx, dy);
            float far = 1;
            for (int y = 0; y < 2; y++) {
                for (int x = 0; x < 2; x++) {
                    far = min(far, source_depth(o * 2 + int2(x, y), size, level));
                }
            }
            tile[t.y * 2 + dy][t.x * 2 + dx] = far;
            store_level(level + 1, o, far);
        }
    }
    for (uint step = 2; step <= 6; step++) {
        uint n = 32u >> (step - 1);
        GroupMemoryBarrierWithGroupSync();
        float far = 1;
        bool active = all(thread.xy < n);
        if (active) {
            far = min(min(tile[t.y * 2][t.x * 2], tile[t.y * 2][t.x * 2 + 1]),
                      min(tile[t.y * 2 + 1][t.x * 2], tile[t.y * 2 + 1][t.x * 2 + 1]));
        }
        GroupMemoryBarrierWithGroupSync();
        if (active) {
            tile[t.y][t.x] = far;
            store_level(level + step, int2(group.xy) * int(n) + t, far);
        }
    }
}
