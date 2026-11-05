// The terrain occlusion pyramid: the frame's depth reduced level by level,
// each texel holding the farthest depth of the area it covers. The cull
// tests next frame's sections against it. Depth is stored in the standard
// convention (0 near, 1 far) whatever the source used, so the test has one
// rule.

struct Params {
    // 1 when the source depth is reversed (1 near, 0 far), as Bevy's is.
    reverse: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var source_depth: texture_depth_2d;
@group(0) @binding(1) var level0: texture_storage_2d<r32float, write>;
@group(0) @binding(2) var<uniform> params: Params;

fn standard(d: f32) -> f32 {
    return select(d, 1.0 - d, params.reverse != 0u);
}

/// Level 0, at half the depth's size: the farthest of each 2×2 (and the
/// extra row or column at an odd edge, so nothing is left uncovered).
@compute @workgroup_size(8, 8)
fn from_depth(@builtin(global_invocation_id) id: vec3<u32>) {
    let out_size = textureDimensions(level0);
    if (id.x >= out_size.x || id.y >= out_size.y) {
        return;
    }
    let size = vec2<i32>(textureDimensions(source_depth));
    let base = vec2<i32>(id.xy) * 2;
    let extra_x = select(1, 2, (size.x & 1) == 1 && id.x == out_size.x - 1u);
    let extra_y = select(1, 2, (size.y & 1) == 1 && id.y == out_size.y - 1u);
    var far = 0.0;
    for (var y = 0; y <= extra_y; y++) {
        for (var x = 0; x <= extra_x; x++) {
            let p = min(base + vec2<i32>(x, y), size - 1);
            far = max(far, standard(textureLoad(source_depth, p, 0)));
        }
    }
    textureStore(level0, vec2<i32>(id.xy), vec4<f32>(far, 0.0, 0.0, 0.0));
}

@group(0) @binding(5) var source_depth_ms: texture_depth_multisampled_2d;

/// Level 0 from a multisampled depth (MSAA): as `from_depth`, over every
/// sample of each pixel.
@compute @workgroup_size(8, 8)
fn from_depth_ms(@builtin(global_invocation_id) id: vec3<u32>) {
    let out_size = textureDimensions(level0);
    if (id.x >= out_size.x || id.y >= out_size.y) {
        return;
    }
    let size = vec2<i32>(textureDimensions(source_depth_ms));
    let samples = i32(textureNumSamples(source_depth_ms));
    let base = vec2<i32>(id.xy) * 2;
    let extra_x = select(1, 2, (size.x & 1) == 1 && id.x == out_size.x - 1u);
    let extra_y = select(1, 2, (size.y & 1) == 1 && id.y == out_size.y - 1u);
    var far = 0.0;
    for (var y = 0; y <= extra_y; y++) {
        for (var x = 0; x <= extra_x; x++) {
            let p = min(base + vec2<i32>(x, y), size - 1);
            for (var s = 0; s < samples; s++) {
                far = max(far, standard(textureLoad(source_depth_ms, p, s)));
            }
        }
    }
    textureStore(level0, vec2<i32>(id.xy), vec4<f32>(far, 0.0, 0.0, 0.0));
}

@group(0) @binding(3) var source_level: texture_2d<f32>;
@group(0) @binding(4) var next_level: texture_storage_2d<r32float, write>;

/// One level from the one below: the farthest of each 2×2, with the odd
/// edge's extra row or column.
@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let out_size = textureDimensions(next_level);
    if (id.x >= out_size.x || id.y >= out_size.y) {
        return;
    }
    let size = vec2<i32>(textureDimensions(source_level));
    let base = vec2<i32>(id.xy) * 2;
    let extra_x = select(1, 2, (size.x & 1) == 1 && id.x == out_size.x - 1u);
    let extra_y = select(1, 2, (size.y & 1) == 1 && id.y == out_size.y - 1u);
    var far = 0.0;
    for (var y = 0; y <= extra_y; y++) {
        for (var x = 0; x <= extra_x; x++) {
            let p = min(base + vec2<i32>(x, y), size - 1);
            far = max(far, textureLoad(source_level, p, 0).r);
        }
    }
    textureStore(next_level, vec2<i32>(id.xy), vec4<f32>(far, 0.0, 0.0, 0.0));
}
