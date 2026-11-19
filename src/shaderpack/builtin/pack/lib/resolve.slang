// Torch Vibrant: brings an effect traced at a fraction of the screen's
// resolution back to every pixel. Include after common.slang, in a pass that
// reads depthtex0.
//
// Upsample (SCALED_RESOLVE 0) blends the four nearest low-resolution samples,
// weighted by how close each is and how near its depth is to this pixel's, so
// an edge does not bleed across an edge in the scene. Dither (1) takes one of
// those four, chosen by an ordered pattern in the same proportions: crisp,
// stippled where they differ, never blurred.

float linear_depth(float depth) {
    float z = depth * 2.0 - 1.0;
    return 2.0 * frame.near * frame.far / (frame.far + frame.near - z * (frame.far - frame.near));
}

/// `low` at this pixel. `masked`: a sample whose alpha is 0 has nothing to
/// give (a reflection off the water's edge) and is left out.
float4 resolve(Texture2D low, float2 uv, float2 frag, float depth, bool masked) {
    float2 low_size = float2(iris_size(low));
    float2 full_size = float2(iris_size(depthtex0));
    float2 f = uv * low_size - 0.5;
    float2 corner = floor(f);
    float2 fr = f - corner;
    float here = linear_depth(depth);

    float4 values[4];
    float weights[4];
    float total = 0.0;
    for (uint i = 0; i < 4; i++) {
        float2 o = float2(float(i & 1u), float(i >> 1u));
        float2 t = clamp(corner + o, 0.0, low_size - 1.0);
        values[i] = low.Load(int3(int2(t), 0));
        int2 at = int2((t + 0.5) / low_size * full_size);
        float there = linear_depth(depthtex0.Load(int3(at, 0)).r);
        float similar = 1.0 / (0.02 + abs(there - here) / max(here, 1e-3));
        float2 near = lerp(1.0 - fr, fr, o);
        float w = near.x * near.y * similar + 1e-6;
        if (masked && values[i].a <= 0.0) {
            w = 0.0;
        }
        weights[i] = w;
        total += w;
    }
    if (total <= 0.0) {
        return float4(0.0);
    }
    if (SCALED_RESOLVE == 0) {
        float4 sum = float4(0.0);
        for (uint i = 0; i < 4; i++) {
            sum += values[i] * weights[i];
        }
        return sum / total;
    }
    float pick = bayer(frag) * total;
    float running = 0.0;
    for (uint i = 0; i < 4; i++) {
        running += weights[i];
        if (pick <= running) {
            return values[i];
        }
    }
    return values[3];
}
