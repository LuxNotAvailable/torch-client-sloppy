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
import cull;
import world;

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
    uint start = reserve(pc.ids.w, pool * 2, runs, (flags & CULL_NO_WAVE) == 0);
    for (uint r = 0; r < runs; r++) {
        if (start + r < pc.more.y) {
            write_command(pc.ids.z, (pool * 2) * pc.more.y + start + r, run_count[r], run_first[r], base, index, 1);
        }
    }

    // Cutout, after the buckets.
    uint cutout = solid && s.ranges.y > 0 ? 1 : 0;
    uint slot_at = reserve(pc.ids.w, pool * 2 + 1, cutout, (flags & CULL_NO_WAVE) == 0);
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
    // A leaves face (the mesher's leaf material).
    nointerpolation uint leaf;
};

int sign16(uint bits) {
    return int(bits << 16) >> 16;
}

// Indices from here up name a packed quad's corner: PACKED_BASE + 4 ×
// record + corner (`src/vk/packed.rs`, whose `unpack` this mirrors).
static const uint PACKED_BASE = 1u << 28;

// A packed corner as the vertex words the mesher would have written: the
// position (x, y, z in 1/256 block), uv, light and colour.
struct Corner {
    int3 local;
    float2 uv;
    float2 light;
    float4 color;
    uint leaf;
};

uint tinted(uint tint, uint grey) {
    return (tint * grey + 127) / 255;
}

Corner packed_corner(ByteAddressBuffer pool, uint record_at, uint corner) {
    uint4 r0 = pool.Load4(record_at);
    uint4 r1 = pool.Load4(record_at + 16);
    uint w0 = r0.x;
    uint3 block = uint3(w0 & 31, (w0 >> 5) & 31, (w0 >> 10) & 31);
    uint axis = (w0 >> 15) & 3;
    uint steps = (w0 >> (17 + 2 * corner)) & 3;
    uint leaf = (w0 >> 25) & 1;
    int3 pos = int3(block) * 256;
    uint b = (axis + 1) % 3;
    uint c = (axis + 2) % 3;
    pos[b] += int(steps & 1) * 256;
    pos[c] += int((steps >> 1) & 1) * 256;
    uint edges = (r1.z >> (24 + 2 * corner)) & 3;
    uint u = (edges & 1) != 0 ? r0.y >> 16 : r0.y & 0xFFFF;
    uint v = (edges & 2) != 0 ? r0.z >> 16 : r0.z & 0xFFFF;
    uint light = ((corner < 2 ? r0.w : r1.x) >> (16 * (corner % 2))) & 0xFFFF;
    uint grey = (r1.y >> (8 * corner)) & 0xFF;
    uint3 tint = uint3(r1.z & 0xFF, (r1.z >> 8) & 0xFF, (r1.z >> 16) & 0xFF);
    uint alpha = (r1.w >> (8 * corner)) & 0xFF;
    Corner out;
    out.local = pos;
    out.uv = float2(u, v) / 65535.0;
    out.light = float2(light & 0xFF, light >> 8);
    out.color = float4(tinted(tint.x, grey), tinted(tint.y, grey), tinted(tint.z, grey), alpha) / 255.0;
    out.leaf = leaf;
    return out;
}

// Push: ids.x slots, ids.y the view buffer, more.x atlas texture, more.y
//       view mode, view_proj the camera.
[shader("vertex")]
VertexOut terrain_vertex(uint vertex: SV_VulkanVertexID, uint instance: SV_VulkanInstanceID) {
    // Only the two fields a vertex needs, not the whole slot.
    ByteAddressBuffer slots = buffer_ro(pc.ids.x);
    int4 origin_base = asint(slots.Load4(instance * SLOT_SIZE + 32));
    ByteAddressBuffer pool = buffer_ro(slots.Load(instance * SLOT_SIZE + 52));
    int3 local;
    float2 uv;
    float2 light;
    float4 color;
    uint leaf = 0;
    // The draw adds the base vertex to every index; a packed one is
    // recognised past it.
    uint named = vertex - uint(origin_base.w);
    if (named >= PACKED_BASE) {
        uint quad = (named - PACKED_BASE) >> 2;
        uint records = slots.Load(instance * SLOT_SIZE + 112);
        Corner corner = packed_corner(pool, records + quad * 32, named & 3);
        local = corner.local;
        uv = corner.uv;
        light = corner.light;
        color = corner.color;
        leaf = corner.leaf;
    } else {
        uint4 raw = pool.Load4(vertex * 20);
        uint tail = pool.Load(vertex * 20 + 16);
        local = int3(sign16(raw.x & 0xFFFF), sign16(raw.x >> 16), sign16(raw.y & 0xFFFF));
        uv = float2(raw.z & 0xFFFF, raw.z >> 16) / 65535.0;
        light = float2(raw.w & 0xFF, (raw.w >> 8) & 0xFF);
        leaf = ((raw.w >> 16) & 0xFF) == 1 ? 1 : 0;
        color = float4(tail & 0xFF, (tail >> 8) & 0xFF, (tail >> 16) & 0xFF, tail >> 24) / 255.0;
    }

    float3 world = float3(origin_base.xyz) + float3(local) / 256.0;

    // The colour darkened by its ambient occlusion (alpha), lit by the
    // lightmap at the vertex's levels, as terrain.wgsl.
    float3 lit = lightmap_at(buffer_at(pc.ids.y), light);

    VertexOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    out.uv = uv;
    out.color = float4(color.rgb * color.a * lit, 1);
    out.world = world;
    out.slot = instance;
    out.leaf = leaf;
    return out;
}

// Hybrid leaves fade to fast over the last chunk before the line
// (params.x the distance in chunks, 0 for none; params.yz the camera's
// chunk): a dithered share of the see-through texels drawn filled.
// Fast leaves (params.x < 0) fill every hole.
bool leaf_filled(VertexOut in) {
    if (in.leaf == 0 || pc.params.x == 0.0) {
        return false;
    }
    float2 centre = pc.params.yz * 16.0 + 8.0;
    float2 d = abs(in.world.xz - centre);
    float reach = pc.params.x * 16.0 + 8.0;
    float t = pc.params.x < 0.0 ? 1.0 : saturate((max(d.x, d.y) - (reach - 16.0)) / 16.0);
    // Interleaved gradient noise on the pixel.
    float noise = frac(52.9829189 * frac(dot(in.position.xy, float2(0.06711056, 0.00583715))));
    return noise < t;
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
        if (!leaf_filled(in)) {
            discard;
        }
        // A hole filled with the leaf's average colour (a small mip),
        // halfway to black: the shade of a leaf's shadowed inside.
        texel = float4(texture_at(pc.more.x).SampleLevel(g_samplers[SAMPLER_LINEAR], in.uv, 4).rgb * 0.5, 1);
    }
    RWByteAddressBuffer view = buffer_at(pc.ids.y);
    float3 camera = asfloat(view.Load3(VIEW_CAMERA));
    return vanilla_fog(view, float4(texel.rgb * in.color.rgb, texel.a), in.world - camera);
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
// the portal, blended with the texture's own alpha, scaled by the Water
// Clarity setting (params.w), as the game's water material does.
[shader("fragment")]
float4 terrain_translucent(VertexOut in): SV_Target {
    float4 c = terrain_shade(in, false);
    if (pc.more.y != VIEW_SHADED) {
        return float4(c.rgb, 0.6);
    }
    return float4(c.rgb, saturate(c.a * pc.params.w));
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
