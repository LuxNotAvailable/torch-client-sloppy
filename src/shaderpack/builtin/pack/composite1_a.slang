// Torch Vibrant: empties the reflection table (lib/water.slang) once
// composite has laid this frame's reflections, for the next frame's water
// to claim into.

/*
const ivec3 workGroups = ivec3(2048, 1, 1);
*/

#include "lib/common.slang"
#include "lib/shadow.slang"
#include "lib/water.slang"

[numthreads(256, 1, 1)]
void compute(uint3 id : SV_DispatchThreadID) {
    if (id.x == 0) {
        refl().Store(REFL_COUNT, 0u);
    }
    if (id.x < REFL_SIZE) {
        refl().Store(REFL_KEYS + 4 * id.x, REFL_EMPTY);
    }
}
