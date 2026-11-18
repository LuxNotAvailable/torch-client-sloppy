// Torch Vibrant: every terrain pixel near the camera claims the block-face
// texel it shows. The first pixel to reach a texel adds it to the list
// deferred_b shades, so each texel is shaded once however many pixels cover
// it. Texels out of the full table's reach claim in the coarse one.

/*
const vec2 workGroupsRender = vec2(1.0, 1.0);
*/

#include "/lib/common.wgsl"
#include "/lib/cache.wgsl"

@compute @workgroup_size(8, 8)
fn compute(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(colortex2);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let texel = read_texel(textureLoad(colortex2, vec2<i32>(id.xy), 0));
    if (!texel.terrain) {
        return;
    }
    let face = texel.surface.face;
    // Fast foliage keeps its shade, not a cell, in the texel.
    if (face == FACE_DIAGONAL && FOLIAGE_SHADING == 1) {
        return;
    }
    let key = cache_key(texel.cell, f32(face));
    if (key != CACHE_EMPTY) {
        claim(key);
        return;
    }
    let coarse = coarse_key(texel.cell, face);
    if (coarse != CACHE_EMPTY) {
        claim_coarse(coarse, coarse_along(texel.cell, coarse));
    }
}

fn claim(key: u32) {
    var slot = cache_hash(key);
    for (var probe = 0u; probe < CACHE_PROBES; probe++) {
        let seen = atomicLoad(&cache.keys[slot]);
        if (seen == key) {
            return;
        }
        if (seen == CACHE_EMPTY) {
            let swap = atomicCompareExchangeWeak(&cache.keys[slot], CACHE_EMPTY, key);
            if (swap.exchanged) {
                let at = atomicAdd(&cache.count, 1u);
                if (at < CACHE_SIZE) {
                    cache.work[at] = slot;
                }
                return;
            }
            if (swap.old_value == key) {
                return;
            }
            if (swap.old_value == CACHE_EMPTY) {
                // A spurious failure: try the same slot again.
                continue;
            }
        }
        slot = (slot + 1u) & CACHE_MASK;
    }
}

/// As `claim`, in the coarse table; the claimer leaves its along-normal
/// offset (flagged unshaded) for deferred_b.
fn claim_coarse(key: u32, along: u32) {
    var slot = cache_hash(key);
    for (var probe = 0u; probe < CACHE_PROBES; probe++) {
        let seen = atomicLoad(&cache.ckeys[slot]);
        if (seen == key) {
            return;
        }
        if (seen == CACHE_EMPTY) {
            let swap = atomicCompareExchangeWeak(&cache.ckeys[slot], CACHE_EMPTY, key);
            if (swap.exchanged) {
                cache.cvalues[slot] = along | COARSE_WORK;
                let at = atomicAdd(&cache.count, 1u);
                if (at < CACHE_SIZE) {
                    cache.work[at] = slot | COARSE_WORK;
                }
                return;
            }
            if (swap.old_value == key) {
                return;
            }
            if (swap.old_value == CACHE_EMPTY) {
                continue;
            }
        }
        slot = (slot + 1u) & CACHE_MASK;
    }
}
