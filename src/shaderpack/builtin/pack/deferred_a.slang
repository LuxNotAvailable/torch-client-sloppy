// Torch Vibrant: every terrain pixel near the camera claims the block-face
// texel it shows. The first pixel to reach a texel adds it to the list
// deferred_b shades, so each texel is shaded once however many pixels cover
// it. Texels out of the full table's reach claim in the coarse one.

/*
const vec2 workGroupsRender = vec2(1.0, 1.0);
*/

#include "lib/common.slang"
#include "lib/cache.slang"

void claim(uint key) {
    uint slot = cache_hash(key);
    for (uint probe = 0; probe < CACHE_PROBES; probe++) {
        uint seen = cache_atomic_load(CACHE_KEYS, slot);
        if (seen == key) {
            return;
        }
        if (seen == CACHE_EMPTY) {
            uint before = cache_compare_exchange(CACHE_KEYS, slot, CACHE_EMPTY, key);
            if (before == CACHE_EMPTY) {
                uint at = cache_add_count(1);
                if (at < CACHE_SIZE) {
                    cache_store(CACHE_WORK, at, slot);
                }
                return;
            }
            if (before == key) {
                return;
            }
        }
        slot = (slot + 1) & CACHE_MASK;
    }
}

/// As `claim`, in the coarse table; the claimer leaves its along-normal
/// offset (flagged unshaded) for deferred_b.
void claim_coarse(uint key, uint along) {
    uint slot = cache_hash(key);
    for (uint probe = 0; probe < CACHE_PROBES; probe++) {
        uint seen = cache_atomic_load(CACHE_CKEYS, slot);
        if (seen == key) {
            return;
        }
        if (seen == CACHE_EMPTY) {
            uint before = cache_compare_exchange(CACHE_CKEYS, slot, CACHE_EMPTY, key);
            if (before == CACHE_EMPTY) {
                cache_store(CACHE_CVALUES, slot, along | COARSE_WORK);
                uint at = cache_add_count(1);
                if (at < CACHE_SIZE) {
                    cache_store(CACHE_WORK, at, slot | COARSE_WORK);
                }
                return;
            }
            if (before == key) {
                return;
            }
        }
        slot = (slot + 1) & CACHE_MASK;
    }
}

[numthreads(8, 8, 1)]
void compute(uint3 id : SV_DispatchThreadID) {
    uint2 size = iris_size(colortex2);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    Texel texel = read_texel(texel_at(int3(int2(id.xy), 0)));
    if (!texel.terrain) {
        return;
    }
    uint face = texel.surface.face;
    // Fast foliage keeps its shade, not a cell, in the texel.
    if (face == FACE_DIAGONAL && FOLIAGE_SHADING == 1) {
        return;
    }
    // No direct light here: deferred skips the lookup, so shade nothing.
    bool foliage = face == FACE_DIAGONAL || texel.surface.material == MATERIAL_FOLIAGE;
    if (!direct_reaches(face_normal(face), texel.surface.light.y, foliage)) {
        return;
    }
    uint key = cache_key(texel.cell, float(face));
    if (key != CACHE_EMPTY) {
        claim(key);
        return;
    }
    uint coarse = coarse_key(texel.cell, face);
    if (coarse != CACHE_EMPTY) {
        claim_coarse(coarse, coarse_along(texel.cell, coarse));
    }
}
