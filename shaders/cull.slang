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
// The environment (phase 5): fog colour, fog ranges (environmental start
// and end, render-distance start and end), and x the lightmap's texture
// index (none: 0xFFFFFFFF), y 1 when fog is on.
static const uint VIEW_FOG_COLOR = 192;
static const uint VIEW_FOG_RANGES = 208;
static const uint VIEW_ENV = 224;

static const uint CULL_HIZ = 1;
static const uint CULL_VISIBILITY = 2;
static const uint CULL_BUCKETS = 4;
static const uint CULL_FRUSTUM = 8;
// One atomic per lane instead of per subgroup (`vkview --no-wave`).
static const uint CULL_NO_WAVE = 16;

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

