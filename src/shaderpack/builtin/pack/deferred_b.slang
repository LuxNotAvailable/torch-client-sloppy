// Torch Vibrant: shades each texel claimed this frame, once, at its middle:
// a soft eight-tap shadow lookup that would be too dear for every pixel.
// Coarse texels average into a history kept across frames (TEXEL_TEMPORAL):
// a few points scattered anew each frame, blended in at 1 / their count,
// the count capped by the texel's resolution.

/*
const ivec3 workGroups = ivec3(16384, 1, 1);
*/

#include "lib/common.slang"
#include "lib/cache.slang"
#include "lib/shadow.slang"

// The temporal history (bufferObject.1): world-anchored tags (0 empty) and
// packed values, open-addressed as the texel cache.
RWByteAddressBuffer history() { return iris_rw_buffer(IRIS_BUFFER_1); } // read_write

static const uint HISTORY_TAGS = 0;
static const uint HISTORY_VALUES = 4 * CACHE_SIZE;
static const uint HISTORY_STALE = 8;

/// The texel's tag in the world, not relative to the camera's block, so a
/// step into the next block keeps the history. Never 0 (empty).
uint history_tag(float3 cell, uint face) {
    int3 world = int3(cell) + frame.cameraPositionInt * 16;
    uint h = uint(world.x) * 0x9E3779B1u ^ uint(world.y) * 0x85EBCA77u ^ uint(world.z) * 0xC2B2AE3Du ^ face * 0x27D4EB2Fu;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return max(h, 1u);
}

/// Blends `fresh` into the texel's history and returns the result, or
/// `fresh` when no slot is free.
float temporal(uint tag, float fresh, uint window, uint now) {
    uint slot = cache_hash(tag);
    for (uint probe = 0; probe < CACHE_PROBES; probe++) {
        uint seen;
        history().InterlockedOr(HISTORY_TAGS + 4 * slot, 0u, seen);
        uint packed = history().Load(HISTORY_VALUES + 4 * slot);
        uint age = (now - (packed >> 24u)) & 255u;
        bool mine = seen == tag;
        bool start = false;
        if (!mine && (seen == 0 || age > HISTORY_STALE)) {
            // Take the empty or stale slot over (another texel may race us
            // for it; whoever swaps first keeps it).
            uint before;
            history().InterlockedCompareExchange(HISTORY_TAGS + 4 * slot, seen, tag, before);
            mine = before == seen;
            start = mine;
        }
        if (mine) {
            uint count = 0;
            float shade = fresh;
            if (!start && age <= HISTORY_STALE) {
                count = (packed >> 16u) & 255u;
                float earlier = float(packed & 65535u) / 65535.0;
                count = min(count + 1, window);
                shade = earlier + (fresh - earlier) / float(count);
            } else {
                count = 1;
            }
            uint quant = uint(clamp(shade, 0.0, 1.0) * 65535.0 + 0.5);
            history().Store(HISTORY_VALUES + 4 * slot, quant | (count << 16u) | ((now & 255u) << 24u));
            return shade;
        }
        slot = (slot + 1) & CACHE_MASK;
    }
    return fresh;
}

[numthreads(64, 1, 1)]
void compute(uint3 id : SV_DispatchThreadID) {
    uint count = min(cache_atomic_load(CACHE_COUNT, 0), CACHE_SIZE);
    if (id.x >= count) {
        return;
    }
    uint work = cache_load(CACHE_WORK, id.x);
    uint slot = work & CACHE_MASK;
    bool coarse = (work & COARSE_WORK) != 0;
    uint face;
    float3 cell;
    if (coarse) {
        uint key = cache_atomic_load(CACHE_CKEYS, slot);
        face = coarse_face(key);
        cell = coarse_cell_of(key, cache_load(CACHE_CVALUES, slot) & 15u);
    } else {
        uint key = cache_atomic_load(CACHE_KEYS, slot);
        face = cache_face(key);
        cell = cache_cell(key);
    }
    float3 n = face_normal(face);
    float3 centre = texel_centre(cell, n) - frame.cameraPositionFract;
    float res = cell_res(cell);
    float vis;
    if (face == FACE_DIAGONAL) {
        vis = foliage_visibility(centre, 8);
    } else if (temporal_texel(res)) {
        uint now = uint(frame.frameCounter);
        float fresh = texel_visibility_scattered(centre, n, res, 8, now);
        vis = temporal(history_tag(cell, face), fresh, temporal_window(res), now);
    } else {
        vis = texel_visibility(centre, n, res, 8);
    }
    uint shade = uint(clamp(vis, 0.0, 1.0) * 65535.0 + 0.5);
    if (coarse) {
        cache_store(CACHE_CVALUES, slot, shade);
    } else {
        cache_store(CACHE_VALUES, slot, shade);
    }
}
