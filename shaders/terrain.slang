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
    uint4 ranges;        // x first index, y cutout (or translucent) count,
                         // zw cutout bucket counts as u16 pairs
    uint4 buckets0;      // +x, -x, +y, -y index counts
    uint4 buckets1;      // +z, -z, no axis, torches (closing cutout)
    uint4 spare;         // x packed records' offset, yzw cutout pairs
    uint4 more;          // xy cutout pairs, zw band counts (`band_sort` in
                         // src/vk/terrain.rs), on at SLOT_BANDS
};

static const uint SLOT_SIZE = 320;
// Where a slot's band counts go on after `more.zw` (not in `Slot`, so the
// cull's whole-slot load stays small).
static const uint SLOT_BANDS = 144;
static const uint SLOT_LIVE = 1;
static const uint SLOT_TRANSLUCENT = 2;

// Writes the drawn buckets of one range (starting at index `at`) as
// commands in `list`, neighbours merged (an empty bucket between two drawn
// ones doesn't split them). At most four runs: past that the last one
// stretches over the hidden buckets between (drawn, just not culled).
// Returns where the range ends. Called from uniform control flow
// (`reserve`).
static const uint BUCKETS = 16;
uint emit_runs(uint counts[BUCKETS], bool drawn[BUCKETS], uint at, uint list, int base, uint index, bool wave) {
    uint run_first[4];
    uint run_end[4];
    uint runs = 0;
    bool open = false;
    for (uint b = 0; b < BUCKETS; b++) {
        if (drawn[b]) {
            if (!open && runs < 4) {
                run_first[runs] = at;
                runs++;
            }
            open = true;
            run_end[runs - 1] = at + counts[b];
        } else if (counts[b] > 0) {
            open = false;
        }
        at += counts[b];
    }
    uint start = reserve(pc.ids.w, list, runs, wave);
    for (uint r = 0; r < runs; r++) {
        if (start + r < pc.more.y) {
            write_command(pc.ids.z, list * pc.more.y + start + r, run_end[r] - run_first[r], run_first[r], base, index, 1);
        }
    }
    return at;
}

// A slot's u16 band count `i` (`band_sort`): the first two in `more.zw`,
// the rest at SLOT_BANDS.
uint band_count(Slot s, RWByteAddressBuffer slots, uint bands_at, uint i) {
    uint w = i / 2;
    uint word = w == 0 ? s.more.z : (w == 1 ? s.more.w : slots.Load(bands_at + (w - 2) * 4));
    return (word >> ((i % 2) * 16)) & 0xFFFF;
}

// Index count through bucket `b`'s first `k` stored bands (0 to 16).
uint through(Slot s, RWByteAddressBuffer slots, uint bands_at, uint b, uint k, uint total) {
    if (k == 0) {
        return 0;
    }
    if (k >= 16) {
        return total;
    }
    return band_count(s, slots, bands_at, b * 15 + k - 1);
}

// Push: ids.x slots, ids.y visible-slot bits, ids.z commands, ids.w counts,
//       more.x slot count, more.y commands per list, more.z the cull view,
//       more.w where the translucent commands start (in commands); params.x
//       the translucent rank buffer (as bits), y the slot order buffer
//       and z its length (near first: nearest slots reserve first; 0 off,
//       and slots past it, added since the sort, keep their place).
// Lists: pool * 2 for solid, pool * 2 + 1 for cutout. Translucent slots
// write their command at their rank (far to near, from the CPU), culled or
// not, so translucent layers draw back to front across sections (3.5, 3.7).
[shader("compute")]
[numthreads(64, 1, 1)]
void cull(uint3 id: SV_DispatchThreadID) {
    uint index = id.x;
    RWByteAddressBuffer view = buffer_at(pc.more.z);
    uint flags = view.Load(VIEW_FLAGS);
    if ((flags & CULL_LIST) != 0) {
        // Past the list: no slot (an empty one, as past the slot count).
        index = index < asuint(pc.params.z) ? buffer_at(asuint(pc.params.y)).Load(index * 4) : 0xFFFFFFFFu;
    } else if (index < asuint(pc.params.z)) {
        index = buffer_at(asuint(pc.params.y)).Load(index * 4);
    }
    // Load4, not Load3: slangc turns a Load3 at an offset divisible by 12
    // into a uint3 array index (240 / 12) over a 16-byte stride, reading
    // past the view (zeros).
    float3 camera = asfloat(view.Load4(VIEW_CULL_EYE).xyz);
    Slot s = (Slot)0;
    bool lazy = (flags & CULL_LAZY) != 0;
    if (index < pc.more.x) {
        RWByteAddressBuffer at = buffer_at(pc.ids.x);
        uint o = index * SLOT_SIZE;
        if (lazy) {
            // The box, origin and flags; the rest once kept (below).
            s.lo = asfloat(at.Load4(o));
            s.hi = asfloat(at.Load4(o + 16));
            s.origin_base = asint(at.Load4(o + 32));
            s.info = at.Load4(o + 48);
        } else {
            s = at.Load<Slot>(o);
        }
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
        keep = hi.y >= asfloat(view.Load(VIEW_CULL_EYE + 12));
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
    if (lazy && (keep || translucent)) {
        RWByteAddressBuffer at = buffer_at(pc.ids.x);
        uint o = index * SLOT_SIZE;
        s.ranges = at.Load4(o + 64);
        s.buckets0 = at.Load4(o + 80);
        s.buckets1 = at.Load4(o + 96);
        s.spare = at.Load4(o + 112);
        s.more = at.Load4(o + 128);
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
    uint pool = s.info.x;
    bool wave = (flags & CULL_NO_WAVE) == 0;
    bool any_side = (flags & CULL_BUCKETS) == 0;
    // Each axis bucket as two entries, split at a plane band: its quads
    // are sorted far side first (`band_sort`), so a +a bucket's seen faces
    // (planes below the eye) close it and a -a bucket's (above) open it.
    // Unbanded buckets split at their ends by the section test.
    float3 eye = camera - float3(s.origin_base.xyz);
    RWByteAddressBuffer slot_words = buffer_at(pc.ids.x);
    uint bands_at = index * SLOT_SIZE + SLOT_BANDS;
    bool banded = (flags & CULL_FACE_BANDS) != 0 && !any_side;
    uint solid_counts[BUCKETS];
    bool solid_drawn[BUCKETS];
    for (uint b = 0; b < 6; b++) {
        uint total = counts[b];
        // Counts through the first k stored bands: 0, the 15 stored, total.
        // Culled under CULL_LAZY: no band words (and `through` reads none).
        uint first_band = lazy && !solid ? 0xFFFFu : band_count(s, slot_words, bands_at, b * 15);
        bool has = banded && first_band != 0xFFFF;
        bool seen = any_side || facing[b];
        float e = b < 2 ? eye.x : (b < 4 ? eye.y : eye.z);
        uint split;
        bool plus = (b % 2) == 0;
        if (plus) {
            // Seen: planes below the eye, the last n bands.
            uint n = has ? uint(clamp(ceil(e), 0.0, 16.0)) : (seen ? 16u : 0u);
            split = through(s, slot_words, bands_at, b, 16 - n, total);
        } else {
            // Seen: planes above the eye, the first 16 - m bands.
            uint m = has ? uint(clamp(floor(e), 0.0, 16.0)) : (seen ? 0u : 16u);
            split = through(s, slot_words, bands_at, b, 16 - m, total);
        }
        // Never past the bucket, whatever the counts say.
        split = min(split, total);
        solid_counts[b * 2] = split;
        solid_counts[b * 2 + 1] = total - split;
        solid_drawn[b * 2] = solid && split > 0 && !plus;
        solid_drawn[b * 2 + 1] = solid && total > split && plus;
    }
    solid_counts[12] = counts[6];
    solid_drawn[12] = solid && counts[6] > 0;
    for (uint b = 13; b < BUCKETS; b++) {
        solid_counts[b] = 0;
        solid_drawn[b] = false;
    }
    uint at = emit_runs(solid_counts, solid_drawn, first, pool * 2, base, index, wave);

    // Cutout, after the buckets: its ring of vertical faces (+x, two
    // octants, +z, ... as `CUTOUT_BUCKETS` in `src/vk/terrain.rs`), +y, -y,
    // the no-axis rest, then its torches, dropped past the torch reach
    // (squared, 0 for none) from the box's nearest point.
    uint torches = s.buckets1.w;
    uint pairs[7] = { s.ranges.z, s.ranges.w, s.spare.y, s.spare.z, s.spare.w, s.more.x, s.more.y };
    uint cut[BUCKETS];
    uint listed = 0;
    for (uint i = 0; i < 14; i++) {
        cut[i] = (pairs[i / 2] >> ((i % 2) * 16)) & 0xFFFF;
        listed += cut[i];
    }
    cut[14] = s.ranges.y - torches - listed;
    cut[15] = torches;
    bool torch_near = true;
    float torch_reach = asfloat(view.Load(VIEW_FLAGS + 4));
    if (torch_reach > 0) {
        float3 d = clamp(camera, lo, hi) - camera;
        torch_near = dot(d, d) <= torch_reach;
    }
    // Whether some of the box faces the camera from direction d (xz, each
    // component -1, 0 or 1): the camera past the box's back plane for d.
    // The ring's eight directions: +x, +x+z, +z, -x+z, -x, -x-z, -z, +x-z.
    bool cutout_sides = (flags & CULL_CUTOUT_BUCKETS) != 0;
    bool angled_sides = cutout_sides && (flags & CULL_ANGLED_BUCKETS) != 0;
    bool seen[8];
    for (uint k = 0; k < 8; k++) {
        float2 d = float2(k == 0 || k == 1 || k == 7 ? 1 : (k >= 3 && k <= 5 ? -1 : 0),
                          k >= 1 && k <= 3 ? 1 : (k >= 5 ? -1 : 0));
        float back = (d.x > 0 ? lo.x : hi.x) * d.x + (d.y > 0 ? lo.z : hi.z) * d.y;
        seen[k] = dot(camera.xz, d) > back;
    }
    bool cut_drawn[BUCKETS];
    for (uint b = 0; b < BUCKETS; b++) {
        bool side = true;
        if (b < 12) {
            // Ring slot b: an axis (b % 3 == 0, direction 2 * b / 3) or the
            // octant after it, between that axis and its diagonal (1) or the
            // diagonal and the next axis (2).
            uint k = (b / 3) * 2;
            uint r = b % 3;
            if (r == 0) {
                side = !cutout_sides || seen[k];
            } else {
                uint d0 = (k + r - 1) % 8;
                uint d1 = (k + r) % 8;
                side = !angled_sides || seen[d0] || seen[d1];
            }
        } else if (b == 12) {
            side = !cutout_sides || facing[2];
        } else if (b == 13) {
            side = !cutout_sides || facing[3];
        } else if (b == 15) {
            side = torch_near;
        }
        cut_drawn[b] = solid && cut[b] > 0 && side;
    }
    emit_runs(cut, cut_drawn, at, pool * 2 + 1, base, index, wave);
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

// A drawn index's corner position (1/256 block from the section origin).
int3 corner_at(uint index, int base, ByteAddressBuffer pool, uint records) {
    if (index >= PACKED_BASE) {
        uint quad = (index - PACKED_BASE) >> 2;
        return packed_corner(pool, records + quad * 32, index & 3).local;
    }
    uint2 raw = pool.Load2((index + uint(base)) * 20);
    return int3(sign16(raw.x & 0xFFFF), sign16(raw.x >> 16), sign16(raw.y & 0xFFFF));
}

groupshared uint kept_indices;

// Per-Face Culling: one group per command slot (x) of a pool's solid or
// cutout list (y), after the cull. Each quad whose back the cull eye sees
// goes; the rest move to the filtered indices at the command's own offset,
// and its count becomes theirs.
// Push: ids.x slots, ids.y commands, ids.z counts, ids.w the view buffer;
//       more.x commands per list, more.y the pool's solid list, more.z its
//       indices, more.w its filtered indices.
[shader("compute")]
[numthreads(64, 1, 1)]
void face_filter(uint3 group: SV_GroupID, uint3 thread: SV_GroupThreadID) {
    uint list = pc.more.y + group.y;
    if (group.x >= buffer_at(pc.ids.z).Load(list * 4)) {
        return;
    }
    RWByteAddressBuffer commands = buffer_at(pc.ids.y);
    uint at = (list * pc.more.x + group.x) * COMMAND_SIZE;
    uint count = commands.Load(at);
    uint first = commands.Load(at + 8);
    int base = asint(commands.Load(at + 12));
    uint slot = commands.Load(at + 16);
    ByteAddressBuffer slots = buffer_ro(pc.ids.x);
    float3 origin = float3(asint(slots.Load4(slot * SLOT_SIZE + 32)).xyz);
    ByteAddressBuffer pool = buffer_ro(slots.Load(slot * SLOT_SIZE + 52));
    uint records = slots.Load(slot * SLOT_SIZE + 112);
    float3 eye = (asfloat(buffer_at(pc.ids.w).Load4(VIEW_CULL_EYE).xyz) - origin) * 256.0;
    ByteAddressBuffer source = buffer_ro(pc.more.z);
    RWByteAddressBuffer kept = buffer_at(pc.more.w);
    if (thread.x == 0) {
        kept_indices = 0;
    }
    GroupMemoryBarrierWithGroupSync();
    for (uint q = thread.x; q < count / 6; q += 64) {
        uint from = (first + q * 6) * 4;
        uint i0 = source.Load(from);
        uint i1 = source.Load(from + 4);
        uint i2 = source.Load(from + 8);
        int3 p0 = corner_at(i0, base, pool, records);
        float3 n = cross(float3(corner_at(i1, base, pool, records) - p0), float3(corner_at(i2, base, pool, records) - p0));
        // Kept when the eye is on its front side (or the quad is degenerate).
        if (dot(eye - float3(p0), n) > 0 || all(n == 0)) {
            uint to;
            InterlockedAdd(kept_indices, 6, to);
            for (uint k = 0; k < 6; k++) {
                kept.Store((first + to + k) * 4, source.Load(from + k * 4));
            }
        }
    }
    GroupMemoryBarrierWithGroupSync();
    if (thread.x == 0) {
        commands.Store(at, kept_indices);
    }
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

// The overdraw view: each fragment adds heat (red, then yellow, then
// white as layers stack), whatever its depth.
[shader("fragment")]
float4 terrain_overdraw(VertexOut in): SV_Target {
    return float4(1.0 / 8, 1.0 / 32, 1.0 / 128, 0);
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

float source_depth(int2 p, uint2 size, uint level, bool pack) {
    if (any(p < 0) || any(p >= int2(size))) {
        // Outside the image: nearest, which never wins the farthest.
        return 1;
    }
    if (level == 0 && pack) {
        // A pack's depth (`hiz_reduce_pack`): 0..1 from near to its far
        // plane, bottom row first; as the terrain's reversed depth it is
        // params.z - params.w * d. Its clear (1) is open sky: 0.
        float d = texture_at(pc.ids.y).Load(int3(p.x, int(size.y) - 1 - p.y, 0)).r;
        return d >= 1.0 ? 0.0 : pc.params.z - pc.params.w * d;
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
//       more.yz the depth's size, more.w the top level; params.x > 0 reads
//       one depth texel per 2 × 2 (quarter-res, not conservative);
//       params.y > 0 reads each full-res 2 × 2 with one gather (Android;
//       the same farthest depth).
[shader("compute")]
[numthreads(16, 16, 1)]
void hiz_reduce(uint3 group: SV_GroupID, uint3 thread: SV_GroupThreadID) {
    reduce(group, thread, false);
}

// `hiz_reduce` from a shader pack's depth (Android: the pack draws the
// world, so its depth before the hand is the occluder): converted to the
// terrain's reversed depth with params.z - params.w * d, rows flipped.
[shader("compute")]
[numthreads(16, 16, 1)]
void hiz_reduce_pack(uint3 group: SV_GroupID, uint3 thread: SV_GroupThreadID) {
    reduce(group, thread, true);
}

void reduce(uint3 group, uint3 thread, bool pack) {
    uint level = pc.more.x;
    uint2 size = level_size(level, pc.more.yz);
    int2 t = int2(thread.xy);
    int n0 = level == 0 && pc.params.x > 0 && !pack ? 1 : 2;
    for (int dy = 0; dy < 2; dy++) {
        for (int dx = 0; dx < 2; dx++) {
            int2 o = int2(group.xy) * 32 + t * 2 + int2(dx, dy);
            float far = 1;
            if (level == 0 && pack && pc.params.y > 0) {
                // A pack's depth, rows flipped: the gather at the corner
                // between rows H-2-2y and H-1-2y covers this block (the
                // last odd row clamps to row 0, as the loop's past-edge 1
                // never wins); each texel converted, then the farthest.
                if (all(o * 2 < int2(size))) {
                    float2 uv = float2(float(o.x * 2 + 1), float(int(size.y) - 1 - o.y * 2)) / float2(size);
                    float4 g = texture_at(pc.ids.y).GatherRed(g_samplers[SAMPLER_NEAREST_CLAMP], uv);
                    float4 r = select(g >= 1.0, float4(0.0), pc.params.z - pc.params.w * g);
                    far = min(min(r.x, r.y), min(r.z, r.w));
                }
            } else if (level == 0 && n0 == 2 && pc.params.y > 0 && !pack) {
                // Clamped to the edge: a texel past it repeats one inside,
                // so the min is the loop's; blocks wholly outside stay 1.
                if (all(o * 2 < int2(size))) {
                    float4 g = texture_at(pc.ids.y).GatherRed(
                        g_samplers[SAMPLER_NEAREST_CLAMP], (float2(o * 2) + 1.0) / float2(size));
                    far = min(min(g.x, g.y), min(g.z, g.w));
                }
            } else {
                for (int y = 0; y < n0; y++) {
                    for (int x = 0; x < n0; x++) {
                        far = min(far, source_depth(o * 2 + int2(x, y), size, level, pack));
                    }
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
