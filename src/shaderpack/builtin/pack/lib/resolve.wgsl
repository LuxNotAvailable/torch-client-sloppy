// Torch Vibrant: brings an effect traced at a fraction of the screen's
// resolution back to every pixel. Include after common.wgsl, in a pass that
// reads depthtex0.
//
// Upsample (SCALED_RESOLVE 0) blends the four nearest low-resolution samples,
// weighted by how close each is and how near its depth is to this pixel's, so
// an edge does not bleed across an edge in the scene. Dither (1) takes one of
// those four, chosen by an ordered pattern in the same proportions: crisp,
// stippled where they differ, never blurred.

const BAYER: array<f32, 16> = array<f32, 16>(
    0.0, 8.0, 2.0, 10.0,
    12.0, 4.0, 14.0, 6.0,
    3.0, 11.0, 1.0, 9.0,
    15.0, 7.0, 13.0, 5.0,
);

fn bayer(frag: vec2<f32>) -> f32 {
    let p = vec2<u32>(frag) & vec2<u32>(3u);
    return (BAYER[p.y * 4u + p.x] + 0.5) / 16.0;
}

fn linear_depth(depth: f32) -> f32 {
    let z = depth * 2.0 - 1.0;
    return 2.0 * frame.near * frame.far / (frame.far + frame.near - z * (frame.far - frame.near));
}

/// `low` at this pixel. `masked`: a sample whose alpha is 0 has nothing to
/// give (a reflection off the water's edge) and is left out.
fn resolve(low: texture_2d<f32>, uv: vec2<f32>, frag: vec2<f32>, depth: f32, masked: bool) -> vec4<f32> {
    let low_size = vec2<f32>(textureDimensions(low));
    let full_size = vec2<f32>(textureDimensions(depthtex0));
    let f = uv * low_size - 0.5;
    let corner = floor(f);
    let frac = f - corner;
    let here = linear_depth(depth);

    var values: array<vec4<f32>, 4>;
    var weights: array<f32, 4>;
    var total = 0.0;
    for (var i = 0u; i < 4u; i++) {
        let o = vec2<f32>(f32(i & 1u), f32(i >> 1u));
        let t = clamp(corner + o, vec2<f32>(0.0), low_size - 1.0);
        values[i] = textureLoad(low, vec2<i32>(t), 0);
        let at = vec2<i32>((t + 0.5) / low_size * full_size);
        let there = linear_depth(textureLoad(depthtex0, at, 0).r);
        let similar = 1.0 / (0.02 + abs(there - here) / max(here, 1e-3));
        let near = mix(1.0 - frac, frac, o);
        var w = near.x * near.y * similar + 1e-6;
        if (masked && values[i].a <= 0.0) {
            w = 0.0;
        }
        weights[i] = w;
        total += w;
    }
    if (total <= 0.0) {
        return vec4<f32>(0.0);
    }
    if (SCALED_RESOLVE == 0) {
        var sum = vec4<f32>(0.0);
        for (var i = 0u; i < 4u; i++) {
            sum += values[i] * weights[i];
        }
        return sum / total;
    }
    let pick = bayer(frag) * total;
    var running = 0.0;
    for (var i = 0u; i < 4u; i++) {
        running += weights[i];
        if (pick <= running) {
            return values[i];
        }
    }
    return values[3];
}
