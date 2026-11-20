// Torch Vibrant: traces each World Traced reflection texel the water
// claimed this frame (lib/water.slang's table), once however many pixels
// show it. composite lays the results.

/*
const ivec3 workGroups = ivec3(8192, 1, 1);
*/

#include "lib/common.slang"
#include "lib/shadow.slang"
#include "lib/water.slang"

[numthreads(64, 1, 1)]
void compute(uint3 id : SV_DispatchThreadID) {
    if (WATER_REFLECTIONS == 0 || REFLECTION_METHOD != 1) {
        return;
    }
    uint count = min(refl().Load(REFL_COUNT), REFL_SIZE);
    if (id.x >= count) {
        return;
    }
    refl_trace(refl().Load(REFL_WORK + 4 * id.x));
}
