// Debug Options > Culling View: Culled sections. Every section the
// occlusion test hid this frame, its box projected through the very matrix
// the test used. Where the box's nearest point is in front of everything
// the pyramid holds at a pixel, the section should have shown there: those
// pixels are filled bright red (a wrong cull). Elsewhere the box is only
// outlined faintly (rightly hidden).

struct SlotMeta {
    min: vec3<f32>,
    first_index: u32,
    max: vec3<f32>,
    index_count: u32,
    origin: vec3<i32>,
    base_vertex: u32,
    solid_count: u32,
    flags: u32,
    pool: u32,
    cutout_first: u32,
}

struct Params {
    // The clip matrix the test used (last frame's camera).
    clip: mat4x4<f32>,
    // The first word of the culled-section bits in `counts`.
    bits: u32,
    // 1 when the matrix's y runs down the screen (a shader pack's).
    flip: u32,
    // 1 when the matrix's depth is reversed (Bevy's).
    reverse: u32,
    pad1: u32,
}

@group(0) @binding(0) var<storage, read> metas: array<SlotMeta>;
@group(0) @binding(1) var<storage, read> counts: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;
// The pyramid's finest level (standard depth: 0 near, 1 far), from the same
// frame's depth as the test's matrix when the camera is still.
@group(0) @binding(3) var level0: texture_2d<f32>;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) edge: vec2<f32>,
    // The point in the pyramid's own screen space (before any flip), and
    // the box's nearest depth.
    @location(1) ndc: vec2<f32>,
    @location(2) @interpolate(flat) nearest: f32,
}

@vertex
fn vertex(@builtin(vertex_index) corner: u32, @builtin(instance_index) slot: u32) -> Out {
    var out: Out;
    // Off screen unless this section was culled and projects cleanly.
    out.position = vec4<f32>(2.0, 2.0, 0.0, 1.0);
    out.edge = vec2<f32>(0.0);
    let word = counts[params.bits + slot / 32u];
    if ((word >> (slot % 32u)) & 1u) == 0u {
        return out;
    }
    let m = metas[slot];
    let lo = vec3<f32>(m.origin) + m.min;
    let hi = vec3<f32>(m.origin) + m.max;
    var mn = vec2<f32>(1e9);
    var mx = vec2<f32>(-1e9);
    var nearest = 1.0;
    for (var i = 0u; i < 8u; i++) {
        let p = vec3<f32>(
            select(lo.x, hi.x, (i & 1u) != 0u),
            select(lo.y, hi.y, (i & 2u) != 0u),
            select(lo.z, hi.z, (i & 4u) != 0u),
        );
        let c = params.clip * vec4<f32>(p, 1.0);
        if c.w <= 0.05 {
            return out;
        }
        mn = min(mn, c.xy / c.w);
        mx = max(mx, c.xy / c.w);
        let z = c.z / c.w;
        nearest = min(nearest, select(z, 1.0 - z, params.reverse != 0u));
    }
    // Two triangles over the rectangle.
    let quad = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    let q = quad[corner];
    var ndc = mix(mn, mx, q);
    out.ndc = ndc;
    out.nearest = nearest;
    if params.flip != 0u {
        ndc.y = -ndc.y;
    }
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.edge = q;
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    // A solid outline and a light fill, so overlapping boxes stay legible.
    let border = min(min(in.edge.x, 1.0 - in.edge.x), min(in.edge.y, 1.0 - in.edge.y));
    let w = fwidth(in.edge);
    let line = border < max(w.x, w.y) * 1.5;
    let dims = vec2<f32>(textureDimensions(level0));
    let uv = vec2<f32>(in.ndc.x * 0.5 + 0.5, 0.5 - in.ndc.y * 0.5);
    let texel = clamp(vec2<i32>(uv * dims), vec2<i32>(0), vec2<i32>(dims) - 1);
    let scene = textureLoad(level0, texel, 0).r;
    if in.nearest < scene {
        // Nothing in front of the section here: it should have shown.
        return vec4<f32>(1.0, 0.0, 0.0, 0.85);
    }
    return select(vec4<f32>(0.0), vec4<f32>(1.0, 0.6, 0.6, 0.35), line);
}
