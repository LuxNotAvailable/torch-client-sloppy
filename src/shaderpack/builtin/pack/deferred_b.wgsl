// Torch Vibrant: shades each texel claimed this frame, once, at its middle:
// a soft eight-tap shadow lookup that would be too dear for every pixel.

/*
const ivec3 workGroups = ivec3(16384, 1, 1);
*/

#include "/lib/common.wgsl"
#include "/lib/cache.wgsl"
#include "/lib/shadow.wgsl"

@compute @workgroup_size(64)
fn compute(@builtin(global_invocation_id) id: vec3<u32>) {
    let count = min(atomicLoad(&cache.count), CACHE_SIZE);
    if (id.x >= count) {
        return;
    }
    let slot = cache.work[id.x];
    let key = atomicLoad(&cache.keys[slot]);
    let n = face_normal(cache_face(key));
    let centre = texel_centre(cache_cell(key), n) - frame.cameraPositionFract;
    let vis = shadow_visibility(centre, n, 8u);
    cache.values[slot] = u32(clamp(vis, 0.0, 1.0) * 65535.0 + 0.5);
}
