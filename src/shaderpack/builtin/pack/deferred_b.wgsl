// Torch Vibrant: shades each texel claimed this frame, once, at its middle:
// a soft eight-tap shadow lookup that would be too dear for every pixel.
// Coarse texels average into a history kept across frames (TEXEL_TEMPORAL):
// a few points scattered anew each frame, blended in at 1 / their count,
// the count capped by the texel's resolution.

/*
const ivec3 workGroups = ivec3(16384, 1, 1);
*/

#include "/lib/common.wgsl"
#include "/lib/cache.wgsl"
#include "/lib/shadow.wgsl"

// The temporal history (bufferObject.1): world-anchored tags (0 empty) and
// packed values, open-addressed as the texel cache.
struct History {
    tags: array<atomic<u32>, 1048576>,
    values: array<u32, 1048576>,
}

@group(0) @binding(IRIS_BUFFER_1) var<storage, read_write> history: History;

const HISTORY_STALE: u32 = 8u;

/// The texel's tag in the world, not relative to the camera's block, so a
/// step into the next block keeps the history. Never 0 (empty).
fn history_tag(cell: vec3<f32>, face: u32) -> u32 {
    let world = vec3<i32>(cell) + frame.cameraPositionInt * 16;
    var h = u32(world.x) * 0x9E3779B1u ^ u32(world.y) * 0x85EBCA77u ^ u32(world.z) * 0xC2B2AE3Du ^ face * 0x27D4EB2Fu;
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return max(h, 1u);
}

/// Blends `fresh` into the texel's history and returns the result, or
/// `fresh` when no slot is free.
fn temporal(tag: u32, fresh: f32, window: u32, now: u32) -> f32 {
    var slot = cache_hash(tag);
    for (var probe = 0u; probe < CACHE_PROBES; probe++) {
        let seen = atomicLoad(&history.tags[slot]);
        let packed = history.values[slot];
        let age = (now - (packed >> 24u)) & 255u;
        var mine = seen == tag;
        var start = false;
        if (!mine && (seen == 0u || age > HISTORY_STALE)) {
            // Take the empty or stale slot over (another texel may race us
            // for it; whoever swaps first keeps it).
            let swap = atomicCompareExchangeWeak(&history.tags[slot], seen, tag);
            mine = swap.exchanged;
            start = mine;
        }
        if (mine) {
            var count = 0u;
            var shade = fresh;
            if (!start && age <= HISTORY_STALE) {
                count = (packed >> 16u) & 255u;
                let before = f32(packed & 65535u) / 65535.0;
                count = min(count + 1u, window);
                shade = before + (fresh - before) / f32(count);
            } else {
                count = 1u;
            }
            let quant = u32(clamp(shade, 0.0, 1.0) * 65535.0 + 0.5);
            history.values[slot] = quant | (count << 16u) | ((now & 255u) << 24u);
            return shade;
        }
        slot = (slot + 1u) & CACHE_MASK;
    }
    return fresh;
}

@compute @workgroup_size(64)
fn compute(@builtin(global_invocation_id) id: vec3<u32>) {
    let count = min(atomicLoad(&cache.count), CACHE_SIZE);
    if (id.x >= count) {
        return;
    }
    let slot = cache.work[id.x];
    let key = atomicLoad(&cache.keys[slot]);
    let n = face_normal(cache_face(key));
    let cell = cache_cell(key);
    let centre = texel_centre(cell, n) - frame.cameraPositionFract;
    let res = cell_res(cell);
    var vis: f32;
    if (temporal_texel(res)) {
        let now = u32(frame.frameCounter);
        let fresh = texel_visibility_scattered(centre, n, res, 8u, now);
        vis = temporal(history_tag(cell, cache_face(key)), fresh, temporal_window(res), now);
    } else {
        vis = texel_visibility(centre, n, res, 8u);
    }
    cache.values[slot] = u32(clamp(vis, 0.0, 1.0) * 65535.0 + 0.5);
}
