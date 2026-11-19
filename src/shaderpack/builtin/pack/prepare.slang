// Torch Vibrant: empties the texel cache before the frame's terrain claims
// its texels.

/*
const ivec3 workGroups = ivec3(4096, 1, 1);
*/

#include "lib/common.slang"
#include "lib/cache.slang"

[numthreads(256, 1, 1)]
void compute(uint3 id : SV_DispatchThreadID) {
    if (id.x == 0) {
        cache_atomic_store(CACHE_COUNT, 0, 0);
    }
    if (id.x < CACHE_SIZE) {
        cache_atomic_store(CACHE_KEYS, id.x, CACHE_EMPTY);
        cache_atomic_store(CACHE_CKEYS, id.x, CACHE_EMPTY);
    }
}
