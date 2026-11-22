// GPU culling shared by every scene (terrain, entities): the frame's cull
// inputs, frustum and Hi-Z tests, and reserving indirect commands.

import common;

// The frame's cull inputs, matching `CullView` in `src/vk/terrain.rs`.
// Read field by field: loading it whole crashed slangc 2026.1.
static const uint VIEW_VIEW_PROJ = 0;
static const uint VIEW_PREV_VIEW_PROJ = 64;
static const uint VIEW_CAMERA = 128;
static const uint VIEW_HIZ = 144;
static const uint VIEW_FLAGS = 160;
// x: the hole's half size in view_proj's clip x, y (CULL_HOLE), as bits.
static const uint VIEW_HOLE = 176;
// The environment (phase 5): fog colour, fog ranges (environmental start
// and end, render-distance start and end), and x the lightmap's texture
// index (none: 0xFFFFFFFF), y 1 when fog is on.
static const uint VIEW_FOG_COLOR = 192;
static const uint VIEW_FOG_RANGES = 208;
static const uint VIEW_ENV = 224;
// The eye culls test from (xyz; w the CULL_BELOW floor): the camera's, or
// a frozen cull's.
static const uint VIEW_CULL_EYE = 240;

static const uint CULL_HIZ = 1;
static const uint CULL_VISIBILITY = 2;
static const uint CULL_BUCKETS = 4;
static const uint CULL_FRUSTUM = 8;
// One atomic per lane instead of per subgroup (`vkview --no-wave`).
static const uint CULL_NO_WAVE = 16;
// Drop boxes wholly under camera.w (the shadow pass's caster floor).
static const uint CULL_BELOW = 32;
// Drop boxes whose clip x, y lie wholly inside the hole (VIEW_HOLE): the
// outer shadow cascade, where the inner one covers.
static const uint CULL_HOLE = 64;
// CULL_BUCKETS for the cutout layer too (Cutout Face Culling).
static const uint CULL_CUTOUT_BUCKETS = 128;
// The cutout octant buckets too (Angled Face Culling).
static const uint CULL_ANGLED_BUCKETS = 256;
// The solid axis buckets' plane bands (Exact Face Culling).
static const uint CULL_FACE_BANDS = 512;
// Load a slot's draw ranges and bands only once it is kept (Android).
static const uint CULL_LAZY = 1024;

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

// The apex of `m`'s view pyramid: where its x, y and w rows are all zero.
float3 pyramid_apex(Columns m) {
    float4 r0 = m.row(0);
    float4 r1 = m.row(1);
    float4 r3 = m.row(3);
    float3 a = cross(r1.xyz, r3.xyz);
    float3 b = cross(r3.xyz, r0.xyz);
    float3 c = cross(r0.xyz, r1.xyz);
    return -(r0.w * a + r1.w * b + r3.w * c) / dot(r0.xyz, a);
}

bool in_pyramid(Columns clip, float3 p) {
    float4 q = clip.apply(p);
    float slack = q.w * 1e-4 + 1e-4;
    return q.w > 0 && abs(q.x) <= q.w + slack && abs(q.y) <= q.w + slack;
}

// Adds a world point, projected with `proj`, to a screen rect (uv) and the
// nearest depth; one at the eye's plane counts as nearest of all.
void add_point(Columns proj, float3 p, inout float2 mn, inout float2 mx, inout float nearest, inout uint count) {
    float4 c = proj.apply(p);
    count++;
    if (c.w <= 1e-4) {
        nearest = 2;
        return;
    }
    float2 uv = float2(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
    mn = min(mn, uv);
    mx = max(mx, uv);
    nearest = max(nearest, c.z / c.w);
}

// The screen rect and nearest depth, by `proj`, of the box's part inside
// `clip`'s view pyramid (its four side planes; no near plane): from that
// part's corners, which are the box corners inside, the box edges'
// crossings of the side planes and the pyramid's corner rays' entries into
// the box. For a box beside the camera that crosses its plane, whose own
// corners can't all be projected.
void clipped_extent(Columns clip, Columns proj, float3 lo, float3 hi, inout float2 mn, inout float2 mx, inout float nearest, inout uint count) {
    float3 corners[8];
    float4 q[8];
    for (uint i = 0; i < 8; i++) {
        corners[i] = float3((i & 1) != 0 ? hi.x : lo.x, (i & 2) != 0 ? hi.y : lo.y, (i & 4) != 0 ? hi.z : lo.z);
        q[i] = clip.apply(corners[i]);
        if (in_pyramid(clip, corners[i])) {
            add_point(proj, corners[i], mn, mx, nearest, count);
        }
    }
    for (uint i = 0; i < 8; i++) {
        for (uint bit = 1; bit <= 4; bit <<= 1) {
            if ((i & bit) != 0) {
                continue;
            }
            uint j = i | bit;
            for (uint k = 0; k < 4; k++) {
                float sgn = (k & 1) != 0 ? -1.0 : 1.0;
                float di = q[i].w + sgn * (k < 2 ? q[i].x : q[i].y);
                float dj = q[j].w + sgn * (k < 2 ? q[j].x : q[j].y);
                if (di * dj < 0) {
                    float3 p = lerp(corners[i], corners[j], di / (di - dj));
                    if (in_pyramid(clip, p)) {
                        add_point(proj, p, mn, mx, nearest, count);
                    }
                }
            }
        }
    }
    float3 eye = pyramid_apex(clip);
    float4 r0 = clip.row(0);
    float4 r1 = clip.row(1);
    float4 r3 = clip.row(3);
    for (uint k = 0; k < 4; k++) {
        float sx = (k & 1) != 0 ? 1.0 : -1.0;
        float sy = (k & 2) != 0 ? 1.0 : -1.0;
        float3 dir = cross((r0 - sx * r3).xyz, (r1 - sy * r3).xyz);
        if (dot(r3.xyz, dir) < 0) {
            dir = -dir;
        }
        float3 inv = 1.0 / dir;
        float3 t1 = (lo - eye) * inv;
        float3 t2 = (hi - eye) * inv;
        float3 tn = min(t1, t2);
        float3 tf = max(t1, t2);
        float enter = max(max(tn.x, tn.y), max(tn.z, 0.0));
        float leave = min(min(tf.x, tf.y), tf.z);
        if (leave >= enter) {
            add_point(proj, eye + dir * enter, mn, mx, nearest, count);
        }
    }
}

// Whether the box is hidden behind last frame's depth (3.4): projected with
// last frame's camera, so the pyramid lines up, its nearest depth is
// farther than the farthest depth over the area it covers. A box past a
// screen edge is tested on its on-screen part, but only past an edge it
// also crosses on this frame's screen (beyond it nothing shows now; inside
// it, last frame may have no depth). One crossing either near plane counts
// as visible, unless it lies beside the camera: then its part inside the
// view is tested (`clipped_extent`).
bool hiz_occluded(RWByteAddressBuffer view, float3 lo, float3 hi) {
    uint4 hiz = view.Load4(VIEW_HIZ);
    Columns m = load_columns(view, VIEW_PREV_VIEW_PROJ);
    Columns now = load_columns(view, VIEW_VIEW_PROJ);
    float2 mn = 1e9;
    float2 mx = -1e9;
    float2 now_mn = 1e9;
    float2 now_mx = -1e9;
    float nearest = 0;
    bool straddles = false;
    for (uint i = 0; i < 8; i++) {
        float3 corner = float3((i & 1) != 0 ? hi.x : lo.x, (i & 2) != 0 ? hi.y : lo.y, (i & 4) != 0 ? hi.z : lo.z);
        float4 c = m.apply(corner);
        float4 n = now.apply(corner);
        if (c.w <= 0.05 || n.w <= 0.05) {
            straddles = true;
            break;
        }
        float3 ndc = c.xyz / c.w;
        float2 uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        mn = min(mn, uv);
        mx = max(mx, uv);
        float2 now_uv = float2(n.x / n.w * 0.5 + 0.5, 0.5 - n.y / n.w * 0.5);
        now_mn = min(now_mn, now_uv);
        now_mx = max(now_mx, now_uv);
        nearest = max(nearest, ndc.z);
    }
    if (straddles) {
        // A box round the eye is always seen.
        float3 eye = pyramid_apex(m);
        if (all(eye >= lo - 1.0) && all(eye <= hi + 1.0)) {
            return false;
        }
        mn = 1e9;
        mx = -1e9;
        nearest = 0;
        uint count = 0;
        clipped_extent(m, m, lo, hi, mn, mx, nearest, count);
        // Not in last frame's view at all: nothing to test it against.
        if (count == 0) {
            return false;
        }
        // What shows of it now must have been on last frame's screen.
        float2 now_part_mn = 1e9;
        float2 now_part_mx = -1e9;
        float now_nearest = 0;
        uint now_count = 0;
        clipped_extent(now, m, lo, hi, now_part_mn, now_part_mx, now_nearest, now_count);
        if (now_nearest > 1 || any(now_part_mn < -1e-3) || any(now_part_mx > 1 + 1e-3)) {
            return false;
        }
    } else if (any(mn < 0 && now_mn >= 0) || any(mx > 1 && now_mx <= 1)) {
        // Past an edge last frame but not now: part of it may show uncovered.
        return false;
    }
    if (any(mx < 0) || any(mn > 1)) {
        return false;
    }
    mn = saturate(mn);
    mx = saturate(mx);
    uint2 size0 = hiz.zw;
    float2 px_lo = mn * float2(size0);
    float2 px_hi = mx * float2(size0);
    float extent = max(px_hi.x - px_lo.x, px_hi.y - px_lo.y);
    // Fine enough that the box spans at most 5 texels: a coarser level's
    // texels reach well past the box (into the sky near the horizon), and
    // the culling flickered in bands as the box slid across their grid.
    uint level = clamp(uint(ceil(log2(max(extent / 4.0, 1.0)))), 1u, hiz.y);
    uint2 size = level_size(level, size0);
    float scale = exp2(-float(level));
    int2 a = min(int2(px_lo * scale), int2(size) - 1);
    int2 b = min(int2(px_hi * scale), int2(size) - 1);
    if (any(b - a > 4)) {
        // Past the coarsest level's reach: not read in full, so kept.
        return false;
    }
    RWByteAddressBuffer pyramid = buffer_at(hiz.x);
    uint offset = level_offset(level, size0);
    float far = 1;
    for (int y = a.y; y <= b.y; y++) {
        for (int x = a.x; x <= b.x; x++) {
            far = min(far, asfloat(pyramid.Load((offset + y * size.x + x) * 4)));
        }
    }
    return nearest < far;
}

// Reserves `n` commands in `list` and returns where this lane's start.
// Called from uniform control flow. When the whole subgroup names one list
// (the usual case: neighbouring slots share a pool), one lane adds for all;
// otherwise each lane adds its own. Subgroup operations stay in uniform
// control flow: Vulkan doesn't promise how lanes regroup inside divergent
// loops without maximal reconvergence, and a loop over the distinct lists
// gave two sections one command on the laptop's Pascal GPU (flickering
// holes), while the desktop's Ampere happened to behave.
uint reserve(uint counts, uint list, uint n, bool wave) {
    if (wave && WaveActiveAllEqual(list)) {
        uint total = WaveActiveSum(n);
        uint prefix = WavePrefixSum(n);
        uint base = 0;
        if (WaveIsFirstLane() && total > 0) {
            buffer_at(counts).InterlockedAdd(list * 4, total, base);
        }
        return WaveReadLaneFirst(base) + prefix;
    }
    uint base = 0;
    if (n > 0) {
        buffer_at(counts).InterlockedAdd(list * 4, n, base);
    }
    return base;
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

