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
    if (keep && (flags & CULL_BELOW) != 0) {
        keep = hi.y >= asfloat(view.Load(VIEW_CAMERA + 12));
    }
    if (keep && (flags & CULL_HOLE) != 0) {
        float hole = asfloat(view.Load(VIEW_HOLE));
        Columns m = load_columns(view, VIEW_VIEW_PROJ);
        float2 reach = float2(0.0, 0.0);
        for (uint i = 0; i < 8; i++) {
            float3 corner = float3((i & 1) != 0 ? hi.x : lo.x, (i & 2) != 0 ? hi.y : lo.y, (i & 4) != 0 ? hi.z : lo.z);
            float4 c = m.apply(corner);
            reach = max(reach, abs(c.xy / c.w));
        }
        keep = max(reach.x, reach.y) >= hole;
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
    // Bit 0: a leaves face (the mesher's leaf material). Bits 1-2 the
    // quad's kind (0 plain, 1 packed, 2 merged), bit 3 its drawn diagonal
    // (both for the outlines view), bits 8-15 and 16-23 its size in cells.
    // Merged: bit 4 aggressive, bits 24-31 the uv edges by step code.
    nointerpolation uint flags;
    // A merged record (`vk::packed::merge`), so its pixels read only their
    // cell: x its pool, y its cells' byte address + 1 (0 for any other
    // quad), zw its uv rectangle (u0 | u1 << 16, v0 | v1 << 16). Its
    // position in cells is in color.xy.
    nointerpolation uint4 merged;
};

int sign16(uint bits) {
    return int(bits << 16) >> 16;
}

// Indices from here up name a packed quad's corner: PACKED_BASE + 4 ×
// record + corner (`src/vk/packed.rs`, whose `unpack` this mirrors).
static const uint PACKED_BASE = 1u << 28;
static const uint MERGED = 1u << 26;
static const uint AGGRESSIVE = 1u << 29;
// An exact merge (`vk::packed::EXACT`): shaded by its vertices, its size in
// the next record slot; only its texture repeats per cell.
static const uint EXACT = 1u << 30;
// Merged pixels light each corner and blend (three lightmap lookups, as
// the vertex path), or blend light and look up once (false, cheaper).
static const bool MERGED_EXACT_LIGHT = false;
// Growth per block of distance: about half a pixel at 1600 wide, 70°.
static const float MERGED_GROW = 0.0004;

// A packed corner as the vertex words the mesher would have written: the
// position (x, y, z in 1/256 block), uv, light and colour.
struct Corner {
    int3 local;
    float2 uv;
    float2 light;
    float4 color;
    uint leaf;
    // Merged: the corner in cells (0 to width, 0 to height), and its way
    // out of the quad in the plane (zero for any other quad).
    float2 cells;
    float3 outward;
    float2 outward_cells;
    // Outlines view: kind, diagonal and size (see `VertexOut.flags`).
    uint outline;
    // Merged: its cells' byte address.
    uint table;
    // Exact merged: the uv step per cell along each in-plane axis.
    float2 du;
    float2 dv;
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
    uint2 size = uint2(1, 1);
    bool exact = (w0 & EXACT) != 0;
    if (exact) {
        uint ext = pool.Load(record_at + 32);
        size = uint2((ext & 15) + 1, ((ext >> 4) & 15) + 1);
    } else if ((w0 & MERGED) != 0) {
        size = uint2((r0.w & 15) + 1, ((r0.w >> 4) & 15) + 1);
    }
    float2 cells = float2(steps & 1, (steps >> 1) & 1) * float2(size);
    pos[b] += int(cells.x) * 256;
    pos[c] += int(cells.y) * 256;
    float3 outward = float3(0, 0, 0);
    if ((w0 & MERGED) != 0) {
        outward[b] = (steps & 1) != 0 ? 1.0 : -1.0;
        outward[c] = (steps & 2) != 0 ? 1.0 : -1.0;
    }
    float2 outward_cells = float2(outward[b], outward[c]);
    // `VertexOut.flags` without the leaf bit.
    // Merged also: bit 3 aggressive, bits 23-30 the uv edges by step code.
    // Exact merged: kind 3, bit 4 its texture repeats per cell.
    uint kind = exact ? 3 : (w0 & MERGED) != 0 ? 2 : 1;
    uint outline = kind | (((w0 >> 27) & 1) << 2) | (size.x << 7) | (size.y << 15);
    float2 code_uv[4] = { float2(0, 0), float2(0, 0), float2(0, 0), float2(0, 0) };
    if ((w0 & MERGED) != 0) {
        uint code_edges = 0;
        for (uint k = 0; k < 4; k++) {
            uint e = (r1.z >> (24 + 2 * k)) & 3;
            uint code = (w0 >> (17 + 2 * k)) & 3;
            code_edges |= e << (2 * code);
            code_uv[code] = float2((e & 1) != 0 ? r0.y >> 16 : r0.y & 0xFFFF,
                                   (e & 2) != 0 ? r0.z >> 16 : r0.z & 0xFFFF) / 65535.0;
        }
        outline |= ((w0 & AGGRESSIVE) != 0 ? 1u : 0u) << 3 | (exact ? 1u : 0u) << 4 | (code_edges << 23);
    }
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
    out.du = code_uv[1] - code_uv[0];
    out.dv = code_uv[2] - code_uv[0];
    if (exact) {
        // Unwrapped across the cells; the pixel shader repeats it.
        out.uv = code_uv[0] + cells.x * out.du + cells.y * out.dv;
    }
    out.light = float2(light & 0xFF, light >> 8);
    out.color = float4(tinted(tint.x, grey), tinted(tint.y, grey), tinted(tint.z, grey), alpha) / 255.0;
    out.leaf = leaf;
    out.cells = cells;
    out.outward = outward;
    out.outward_cells = outward_cells;
    out.outline = outline;
    out.table = record_at + r1.x;
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
    uint4 merged = uint4(0, 0, 0, 0);
    float3 outward = float3(0, 0, 0);
    float2 outward_cells = float2(0, 0);
    // Exact merged: the uv steps per cell, for growing it.
    float2 du = float2(0, 0);
    float2 dv = float2(0, 0);
    bool grows = false;
    uint outline;
    // The draw adds the base vertex to every index; a packed one is
    // recognised past it.
    uint named = vertex - uint(origin_base.w);
    if (named >= PACKED_BASE) {
        uint quad = (named - PACKED_BASE) >> 2;
        uint records = slots.Load(instance * SLOT_SIZE + 112);
        uint record_at = records + quad * 32;
        Corner corner = packed_corner(pool, record_at, named & 3);
        local = corner.local;
        uv = corner.uv;
        light = corner.light;
        color = corner.color;
        leaf = corner.leaf;
        outline = corner.outline;
        uint w0 = pool.Load(record_at);
        if ((w0 & MERGED) != 0) {
            grows = true;
            outward = corner.outward;
            outward_cells = corner.outward_cells;
            uint2 rect = uint2(pool.Load(record_at + 4), pool.Load(record_at + 8));
            if ((w0 & EXACT) != 0) {
                // Shaded by its vertices: only the uv rectangle, to repeat.
                merged = uint4(0, 0, rect);
                du = corner.du;
                dv = corner.dv;
            } else {
                merged = uint4(slots.Load(instance * SLOT_SIZE + 52), corner.table + 1, rect);
                color = float4(corner.cells, 0, 0);
            }
        }
    } else {
        uint4 raw = pool.Load4(vertex * 20);
        uint tail = pool.Load(vertex * 20 + 16);
        local = int3(sign16(raw.x & 0xFFFF), sign16(raw.x >> 16), sign16(raw.y & 0xFFFF));
        uv = float2(raw.z & 0xFFFF, raw.z >> 16) / 65535.0;
        light = float2(raw.w & 0xFF, (raw.w >> 8) & 0xFF);
        leaf = ((raw.w >> 16) & 0xFF) == 1 ? 1 : 0;
        color = float4(tail & 0xFF, (tail >> 8) & 0xFF, (tail >> 16) & 0xFF, tail >> 24) / 255.0;
        // Plain quads: four vertices in winding order, split 0-2 (most
        // are; the outlines are a debug view).
        outline = (1 << 7) | (1 << 15);
    }

    float3 world = float3(origin_base.xyz) + float3(local) / 256.0;
    // A merged quad's edges pass its neighbours' corners (T-junctions),
    // where rounding can leave pixel cracks: grow it in its plane by about
    // half a pixel at its distance, so it overlaps them.
    if (grows) {
        float3 camera = asfloat(buffer_at(pc.ids.y).Load3(VIEW_CAMERA));
        float grow = length(world - camera) * MERGED_GROW;
        world += outward * grow;
        if (merged.y != 0) {
            color.xy += outward_cells * grow;
        } else {
            uv += (outward_cells.x * du + outward_cells.y * dv) * grow;
        }
    }

    // The colour darkened by its ambient occlusion (alpha), lit by the
    // lightmap at the vertex's levels, as terrain.wgsl.
    float3 lit = lightmap_at(buffer_at(pc.ids.y), light);

    VertexOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    out.uv = uv;
    out.color = merged.y != 0 ? color : float4(color.rgb * color.a * lit, 1);
    out.world = world;
    out.slot = instance;
    out.flags = leaf | (outline << 1);
    out.merged = merged;
    return out;
}

// Hybrid leaves fade to fast over the last chunk before the line
// (params.x the distance in chunks, 0 for none; params.yz the camera's
// chunk): a dithered share of the see-through texels drawn filled.
// Fast leaves (params.x < 0) fill every hole.
bool leaf_filled(VertexOut in) {
    if ((in.flags & 1) == 0 || pc.params.x == 0.0) {
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

// The interpolated strips' blends (the table's blend list from word
// more.y): frame and next frame read from the atlas's raw texels (more.x),
// mixed, written as bytes to the out buffer (ids.y), a tile each (more.z
// texels a side) by z.
[shader("compute")]
[numthreads(8, 8, 1)]
void anim_blend(uint3 id: SV_DispatchThreadID) {
    uint px = pc.more.z;
    if (id.x >= px || id.y >= px) {
        return;
    }
    ByteAddressBuffer table = buffer_ro(pc.ids.x);
    uint4 e = table.Load4((pc.more.y + 1 + id.z * 4) * 4);
    Texture2D atlas = texture_at(pc.more.x);
    int2 pa = int2((e.x % ATLAS_COLS) * px + id.x, (e.x / ATLAS_COLS) * px + id.y);
    int2 pb = int2((e.y % ATLAS_COLS) * px + id.x, (e.y / ATLAS_COLS) * px + id.y);
    float4 c = lerp(atlas.Load(int3(pa, 0)), atlas.Load(int3(pb, 0)), float(e.z) / 65535.0);
    uint4 b = uint4(round(saturate(c) * 255.0));
    buffer_at(pc.ids.y).Store((id.z * px * px + id.y * px + id.x) * 4, b.x | (b.y << 8) | (b.z << 16) | (b.w << 24));
}

// The atlas through the animation table (more.z, its bindless index plus
// one): the tile a mesh names swapped for the frame showing, blended into
// the next for interpolated textures, as terrain.wgsl's `sample_atlas`.
static const uint ATLAS_COLS = 16;

float4 sample_atlas(float2 uv, float2 gx, float2 gy) {
    Texture2D atlas = texture_at(pc.more.x);
    SamplerState s = g_samplers[SAMPLER_NEAREST];
    if (pc.more.z == 0) {
        return atlas.SampleGrad(s, uv, gx, gy);
    }
    uint w, h, levels;
    atlas.GetDimensions(0, w, h, levels);
    float2 tiles = float2(ATLAS_COLS, float(h) * ATLAS_COLS / float(w));
    float2 grid = uv * tiles;
    float2 cell = clamp(floor(grid), 0, tiles - 1);
    float2 local = grid - cell;
    uint tile = uint(cell.y) * ATLAS_COLS + uint(cell.x);
    ByteAddressBuffer table = buffer_ro(pc.more.z - 1);
    // Tiles past the table's (block entity textures) are their own.
    if (tile >= table.Load(4)) {
        return atlas.SampleGrad(s, uv, gx, gy);
    }
    uint2 entry = table.Load2(8 + tile * 8);
    uint a = entry.x & 0xFFFF;
    uint b = entry.x >> 16;
    float2 ca = float2(a % ATLAS_COLS, a / ATLAS_COLS);
    float4 ta = atlas.SampleGrad(s, (ca + local) / tiles, gx, gy);
    if (entry.y == 0) {
        return ta;
    }
    float2 cb = float2(b % ATLAS_COLS, b / ATLAS_COLS);
    float4 tb = atlas.SampleGrad(s, (cb + local) / tiles, gx, gy);
    return lerp(ta, tb, float(entry.y) / 65535.0);
}

// One corner of a merged cell, lit as `terrain_vertex` lights a vertex.
// `rgba` the vertex's colour bytes, `light` its block and sky light.
float3 cell_corner(uint rgba, uint light, RWByteAddressBuffer view) {
    float4 c = float4(rgba & 0xFF, (rgba >> 8) & 0xFF, (rgba >> 16) & 0xFF, rgba >> 24) / 255.0;
    return c.rgb * c.a * lightmap_at(view, float2(light & 0xFF, (light >> 8) & 0xFF));
}

// A merged quad's pixel: the uv in its cell's tile (and the gradients of
// the uv unwrapped across cells, for the mip), and the colour its cell's
// two triangles would have interpolated.
void merged_pixel(VertexOut in, float2 dcx, float2 dcy, out float2 uv, out float2 gx, out float2 gy, out float3 color) {
    ByteAddressBuffer pool = buffer_ro(in.merged.x);
    uint2 size = uint2((in.flags >> 8) & 0xFF, (in.flags >> 16) & 0xFF);
    float2 st = in.color.xy;
    float2 cell = clamp(floor(st), 0, float2(size - 1));
    float2 f = saturate(st - cell);
    // An aggressive record's cells carry their own texture.
    bool aggressive = ((in.flags >> 4) & 1) != 0;
    uint cell_at = in.merged.y - 1 + (uint(cell.y) * size.x + uint(cell.x)) * (aggressive ? 40 : 28);
    // Each corner's uv, by its in-plane step code.
    uint3 t = aggressive ? pool.Load3(cell_at + 28) : uint3(in.merged.zw, in.flags >> 24);
    float2 corner_uv[4];
    for (uint code = 0; code < 4; code++) {
        uint edges = (t.z >> (2 * code)) & 3;
        corner_uv[code] = float2((edges & 1) != 0 ? t.x >> 16 : t.x & 0xFFFF,
                                 (edges & 2) != 0 ? t.y >> 16 : t.y & 0xFFFF) / 65535.0;
    }
    float2 du = corner_uv[1] - corner_uv[0];
    float2 dv = corner_uv[2] - corner_uv[0];
    uv = corner_uv[0] + f.x * du + f.y * dv;
    gx = dcx.x * du + dcx.y * dv;
    gy = dcy.x * du + dcy.y * dv;

    // The cell: each corner's colour, then their light, then the diagonal.
    uint4 c = pool.Load4(cell_at);
    uint3 l = pool.Load3(cell_at + 16);
    uint4 light = uint4(l.x & 0xFFFF, l.x >> 16, l.y & 0xFFFF, l.y >> 16);
    RWByteAddressBuffer view = buffer_at(pc.ids.y);
    // The cell's triangle under the pixel: its three corners (by step
    // code) and their weights, as the vertices would have interpolated.
    uint3 k;
    float3 w;
    if ((l.z & 1) == 0) {
        // Diagonal 0-3: triangles 0-1-3 (f.x >= f.y) and 0-2-3.
        k = f.x >= f.y ? uint3(0, 1, 3) : uint3(0, 2, 3);
        w = f.x >= f.y ? float3(1 - f.x, f.x - f.y, f.y) : float3(1 - f.y, f.y - f.x, f.x);
    } else {
        // Diagonal 1-2: triangles 0-1-2 (f.x + f.y <= 1) and 3-2-1.
        k = f.x + f.y <= 1 ? uint3(0, 1, 2) : uint3(3, 2, 1);
        // Corner 2 (0, 1) weighs 1 - f.x, corner 1 (1, 0) 1 - f.y.
        w = f.x + f.y <= 1 ? float3(1 - f.x - f.y, f.x, f.y) : float3(f.x + f.y - 1, 1 - f.x, 1 - f.y);
    }
    uint rgba[4] = { c.x, c.y, c.z, c.w };
    uint lights[4] = { light.x, light.y, light.z, light.w };
    if (MERGED_EXACT_LIGHT) {
        // Each corner lit, then blended: exactly the vertex path.
        color = w.x * cell_corner(rgba[k.x], lights[k.x], view)
              + w.y * cell_corner(rgba[k.y], lights[k.y], view)
              + w.z * cell_corner(rgba[k.z], lights[k.z], view);
        return;
    }
    // Colour and light blended, then one lightmap lookup: a shade off the
    // vertex path where light changes across a cell (AO corners).
    float3 shade = float3(0, 0, 0);
    float2 lit = float2(0, 0);
    for (uint i = 0; i < 3; i++) {
        uint code = k[i];
        float4 cc = float4(rgba[code] & 0xFF, (rgba[code] >> 8) & 0xFF, (rgba[code] >> 16) & 0xFF, rgba[code] >> 24) / 255.0;
        shade += w[i] * cc.rgb * cc.a;
        lit += w[i] * float2(lights[code] & 0xFF, (lights[code] >> 8) & 0xFF);
    }
    color = shade * lightmap_at(view, lit);
}

// Outlines view: the shaded colour tinted by the quad's kind (plain as it
// is, packed green, exact merged cyan, aggressive merged blue), with its triangles' edges in white and a
// merged quad's cells in grey.
float3 outlined(VertexOut in, float3 shaded) {
    uint kind = (in.flags >> 1) & 3;
    float2 size = float2((in.flags >> 8) & 0xFF, (in.flags >> 16) & 0xFF);
    // Merged: the position in cells. Others: the block-grid position in
    // the face's plane (a plain quad's diagonal is a guess).
    float2 g = in.color.xy;
    if (kind == 3) {
        // Exact merged: cells back from the unwrapped uv.
        float2 c[4];
        for (uint code = 0; code < 4; code++) {
            uint e = (in.flags >> (24 + 2 * code)) & 3;
            c[code] = float2((e & 1) != 0 ? in.merged.z >> 16 : in.merged.z & 0xFFFF,
                             (e & 2) != 0 ? in.merged.w >> 16 : in.merged.w & 0xFFFF) / 65535.0;
        }
        float2 du = c[1] - c[0];
        float2 dv = c[2] - c[0];
        g = float2(dot(in.uv - c[0], du) / dot(du, du), dot(in.uv - c[0], dv) / dot(dv, dv));
    } else if (kind != 2) {
        float3 n = abs(cross(ddx(in.world), ddy(in.world)));
        uint axis = n.x > n.y && n.x > n.z ? 0 : n.y > n.z ? 1 : 2;
        g = frac(float2(in.world[(axis + 1) % 3], in.world[(axis + 2) % 3]));
    }
    float2 w = max(fwidth(g), 1e-5);
    // Distance to the quad's edges and its diagonal, in pixels.
    float2 edge = min(g, size - g) / w;
    float2 n = g / size;
    float2 nw = w / size;
    float diag = ((in.flags >> 3) & 1) == 0
        ? abs(n.x - n.y) / (nw.x + nw.y)
        : abs(n.x + n.y - 1) / (nw.x + nw.y);
    float line = min(min(edge.x, edge.y), diag * 2);
    // Fade lines out once the quad (or cell) is a few pixels across.
    float2 quad_px = size / w;
    float fade = saturate((min(quad_px.x, quad_px.y) - 6) / 10);
    float3 tint = kind == 1 ? float3(0.4, 1, 0.4) : kind == 3 ? float3(0.3, 0.9, 1) : float3(0.4, 0.6, 1);
    float3 c = kind == 0 ? shaded : lerp(shaded, tint * (0.3 + dot(shaded, float3(0.3, 0.5, 0.2)) * 1.4), 0.7);
    if (kind >= 2) {
        float2 cell = abs(g - round(g)) / w;
        float cell_fade = saturate((1 / max(w.x, w.y) - 6) / 10);
        c = lerp(c, float3(0.55, 0.55, 0.55), saturate(1 - min(cell.x, cell.y)) * 0.6 * cell_fade);
    }
    return lerp(c, float3(1, 1, 1), saturate(1 - line) * fade);
}

// A cut-out hole returns alpha -1, for the entry point to discard: the
// PowerVR compiler fails on a discard inside a called function.
float4 terrain_shade(VertexOut in, bool cutout) {
    if (pc.more.y == VIEW_NORMALS) {
        float3 n = normalize(cross(ddx(in.world), ddy(in.world)));
        return float4(n * 0.5 + 0.5, 1);
    }
    if (pc.more.y == VIEW_TEXTURE_INDEX) {
        uint i = in.slot * 2654435761u;
        return float4(float3(i & 255, (i >> 8) & 255, (i >> 16) & 255) / 255.0 * 0.8 + 0.2, 1);
    }
    // Derivatives before any branch on the quad.
    float2 gx = ddx(in.uv);
    float2 gy = ddy(in.uv);
    float2 dcx = ddx(in.color.xy);
    float2 dcy = ddy(in.color.xy);
    float2 uv = in.uv;
    float3 color = in.color.rgb;
    if (in.merged.y != 0) {
        merged_pixel(in, dcx, dcy, uv, gx, gy, color);
    } else if ((in.flags & (1 << 5)) != 0) {
        // Exact merged: the unwrapped uv back into its tile (gradients
        // stay the unwrapped ones, for the mip).
        float2 lo = float2(in.merged.z & 0xFFFF, in.merged.w & 0xFFFF) / 65535.0;
        float2 span = float2(in.merged.z >> 16, in.merged.w >> 16) / 65535.0 - lo;
        uv = lo + frac((in.uv - lo) / span) * span;
    }
    float4 texel = sample_atlas(uv, gx, gy);
    if (cutout && texel.a < 0.1) {
        if (!leaf_filled(in)) {
            return float4(0, 0, 0, -1);
        }
        // A hole filled with the leaf's average colour (a small mip),
        // halfway to black: the shade of a leaf's shadowed inside.
        texel = float4(texture_at(pc.more.x).SampleLevel(g_samplers[SAMPLER_LINEAR], uv, 4).rgb * 0.5, 1);
    }
    RWByteAddressBuffer view = buffer_at(pc.ids.y);
    float3 camera = asfloat(view.Load3(VIEW_CAMERA));
    float3 rgb = texel.rgb * color;
    if (pc.more.y == VIEW_OUTLINES) {
        rgb = outlined(in, rgb);
    }
    return vanilla_fog(view, float4(rgb, texel.a), in.world - camera);
}

[shader("fragment")]
float4 terrain_solid(VertexOut in): SV_Target {
    return float4(terrain_shade(in, false).rgb, 1);
}

[shader("fragment")]
float4 terrain_cutout(VertexOut in): SV_Target {
    float4 c = terrain_shade(in, true);
    if (c.a < 0) {
        discard;
    }
    return float4(c.rgb, 1);
}

// Vanilla's translucent layer: water, stained glass, ice, slime, honey and
// the portal, blended with the texture's own alpha, scaled by the Water
// Clarity setting (params.w), as the game's water material does.
[shader("fragment")]
float4 terrain_translucent(VertexOut in): SV_Target {
    float4 c = terrain_shade(in, false);
    if (pc.more.y != VIEW_SHADED && pc.more.y != VIEW_OUTLINES) {
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
