// Torch Vibrant: empties the texel cache before the frame's terrain claims
// its texels.

/*
const ivec3 workGroups = ivec3(4096, 1, 1);
*/

#include "/lib/common.wgsl"
#include "/lib/cache.wgsl"

@compute @workgroup_size(256)
fn compute(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x == 0u) {
        atomicStore(&cache.count, 0u);
    }
    if (id.x < CACHE_SIZE) {
        atomicStore(&cache.keys[id.x], CACHE_EMPTY);
    }
}
