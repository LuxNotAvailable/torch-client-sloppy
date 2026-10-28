// Torch Vibrant: every terrain pixel near the camera claims the block-face
// texel it shows. The first pixel to reach a texel adds it to the list
// deferred_b shades, so each texel is shaded once however many pixels cover
// it.

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
    let texel = textureLoad(colortex2, vec2<i32>(id.xy), 0);
    if (texel.w < 0.0) {
        return;
    }
    let key = cache_key(texel.xyz, f32(unpack_surface(texel.w).face));
    if (key == CACHE_EMPTY) {
        return;
    }
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
