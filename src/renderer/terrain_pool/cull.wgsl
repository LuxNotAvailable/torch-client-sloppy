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

struct DrawIndexedIndirectArgs {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
}

struct CullView {
    planes: array<vec4<f32>, 13>,
    within_min: vec4<f32>,
    within_max: vec4<f32>,
    always_min: vec4<f32>,
    always_max: vec4<f32>,
    slot_cap: u32,
    view_index: u32,
    pool_count: u32,
    compact: u32,
    flags: u32,
    plane_count: u32,
    pad0: u32,
    pad1: u32,
    // The occlusion pyramid's view: the clip matrix of the frame it was
    // built from, whether that depth was reversed, and its level count.
    hiz_clip: mat4x4<f32>,
    hiz_reverse: u32,
    hiz_levels: u32,
    hiz_pad0: u32,
    hiz_pad1: u32,
}

const STREAM_SOLID: u32 = 0u;
const STREAM_CUTOUT: u32 = 1u;
const STREAMS: u32 = 3u;
const META_LIVE: u32 = 1u;
const META_WATER: u32 = 2u;
const VIEW_UNOCCLUDED: u32 = 2u;
const VIEW_WITHIN: u32 = 4u;
const VIEW_ALWAYS: u32 = 8u;
const VIEW_HIZ: u32 = 16u;

@group(0) @binding(0) var<storage, read> metas: array<SlotMeta>;
@group(0) @binding(1) var<storage, read> vis: array<u32>;
@group(0) @binding(2) var<storage, read_write> commands: array<DrawIndexedIndirectArgs>;
@group(0) @binding(3) var<storage, read_write> counts: array<atomic<u32>>;
@group(0) @binding(4) var<uniform> cull: CullView;
@group(0) @binding(5) var hiz: texture_2d<f32>;

/// Whether last frame's depth hides a box entirely: its nearest point, seen
/// from last frame's camera, lies behind the farthest depth over the area
/// it covers. Conservative wherever unsure: a corner behind the camera, or
/// a box even partly off the pyramid, counts as visible.
// How far past its meshed box a section may draw: shader packs sway crops,
// leaves and plants in their vertex shaders. Swayed geometry nearer than
// the box landed in the pyramid in front of it, so the section hid itself,
// came back the next frame (with nothing in front any more), and popped in
// and out (farms, trees, roofs at the hub).
const HIZ_MARGIN: f32 = 1.0;

fn hiz_occluded(box_lo: vec3<f32>, box_hi: vec3<f32>) -> bool {
    if (cull.flags & VIEW_HIZ) == 0u || cull.hiz_levels == 0u {
        return false;
    }
    let lo = box_lo - vec3<f32>(HIZ_MARGIN);
    let hi = box_hi + vec3<f32>(HIZ_MARGIN);
    var mn = vec2<f32>(1e9);
    var mx = vec2<f32>(-1e9);
    var nearest = 1.0;
    for (var i = 0u; i < 8u; i = i + 1u) {
        let corner = vec3<f32>(
            select(lo.x, hi.x, (i & 1u) != 0u),
            select(lo.y, hi.y, (i & 2u) != 0u),
            select(lo.z, hi.z, (i & 4u) != 0u),
        );
        let c = cull.hiz_clip * vec4<f32>(corner, 1.0);
        if (c.w <= 0.05) {
            return false;
        }
        let ndc = c.xyz / c.w;
        // Past a shader pack's far plane (Bevy's projection has none): the
        // depth there is above even an empty sky's, so the box would be
        // culled against nothing. Which sections cross that plane changes
        // as the view turns, so distant terrain popped in and out.
        if (cull.hiz_reverse == 0u && ndc.z > 1.0) {
            return false;
        }
        let depth = select(ndc.z, 1.0 - ndc.z, cull.hiz_reverse != 0u);
        nearest = min(nearest, depth);
        mn = min(mn, ndc.xy);
        mx = max(mx, ndc.xy);
    }
    // Partly off last frame's screen: the pyramid knows nothing of the
    // part outside, which turning may have brought into view. Testing only
    // the part inside (clamping to the screen) culled sections whose
    // hidden part was showing, so they popped in and out as the view
    // turned (seen behind the hub's fountain).
    if (any(mn < vec2<f32>(-1.0)) || any(mx > vec2<f32>(1.0))) {
        return false;
    }
    // Screen space, y down.
    let uv_min = vec2<f32>(mn.x * 0.5 + 0.5, 0.5 - mx.y * 0.5);
    let uv_max = vec2<f32>(mx.x * 0.5 + 0.5, 0.5 - mn.y * 0.5);
    if (any(uv_max <= uv_min)) {
        return false;
    }
    let size0 = vec2<f32>(textureDimensions(hiz, 0));
    let extent = (uv_max - uv_min) * size0;
    let level = min(u32(ceil(log2(max(max(extent.x, extent.y), 1.0)))), cull.hiz_levels - 1u);
    let dims = vec2<i32>(textureDimensions(hiz, level));
    let p0 = clamp(vec2<i32>(uv_min * vec2<f32>(dims)), vec2<i32>(0), dims - 1);
    let p1 = clamp(vec2<i32>(uv_max * vec2<f32>(dims)), vec2<i32>(0), dims - 1);
    var far = 0.0;
    for (var y = p0.y; y <= p1.y; y = y + 1) {
        for (var x = p0.x; x <= p1.x; x = x + 1) {
            far = max(far, textureLoad(hiz, vec2<i32>(x, y), i32(level)).r);
        }
    }
    return nearest > far;
}

fn in_frustum(lo: vec3<f32>, hi: vec3<f32>) -> bool {
    if (cull.flags & VIEW_WITHIN) != 0u
        && (any(hi < cull.within_min.xyz) || any(lo > cull.within_max.xyz)) {
        return false;
    }
    if (cull.flags & VIEW_ALWAYS) != 0u
        && all(hi >= cull.always_min.xyz) && all(lo <= cull.always_max.xyz) {
        return true;
    }
    for (var i = 0u; i < cull.plane_count; i = i + 1u) {
        let p = cull.planes[i];
        let v = select(lo, hi, p.xyz > vec3<f32>(0.0));
        if (dot(p.xyz, v) + p.w < 0.0) {
            return false;
        }
    }
    return true;
}

fn write_zero(region: u32, k: u32) {
    commands[region * cull.slot_cap + k] = DrawIndexedIndirectArgs(0u, 1u, 0u, 0i, 0u);
}

fn write_cmd(region: u32, k: u32, first_index: u32, count: u32, base_vertex: u32, slot: u32) {
    commands[region * cull.slot_cap + k] = DrawIndexedIndirectArgs(
        count, 1u, first_index, i32(base_vertex), slot,
    );
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let slot = id.x;
    if (slot >= cull.slot_cap) {
        return;
    }

    let compact = cull.compact != 0u;

    var live = false;
    var visible = false;
    var m: SlotMeta;
    let word = slot / 32u;
    if (cull.flags & VIEW_UNOCCLUDED) != 0u || word >= arrayLength(&vis) || ((vis[word] >> (slot % 32u)) & 1u) != 0u {
        visible = true;
    }
    if (slot < arrayLength(&metas)) {
        m = metas[slot];
        if ((m.flags & META_LIVE) != 0u) {
            live = true;
        }
    }

    var inside = false;
    if (live && visible) {
        let lo = vec3<f32>(m.origin) + m.min;
        let hi = vec3<f32>(m.origin) + m.max;
        inside = in_frustum(lo, hi) && !hiz_occluded(lo, hi);
    }

    let draws = live && visible && inside;
    let is_water = draws && (m.flags & META_WATER) != 0u;
    let has_solid = draws && !is_water && m.solid_count > 0u;
    let has_cutout = draws && !is_water && m.index_count > m.solid_count;

    if (!compact) {
        for (var pool = 0u; pool < cull.pool_count; pool = pool + 1u) {
            let base = ((cull.view_index * cull.pool_count) + pool) * STREAMS;
            if (draws && m.pool == pool) {
                if (has_solid) {
                    write_cmd(base + STREAM_SOLID, slot, m.first_index, m.solid_count, m.base_vertex, slot);
                } else {
                    write_zero(base + STREAM_SOLID, slot);
                }
                if (has_cutout) {
                    write_cmd(base + STREAM_CUTOUT, slot, m.cutout_first,
                        m.index_count - m.solid_count, m.base_vertex, slot);
                } else {
                    write_zero(base + STREAM_CUTOUT, slot);
                }
            } else {
                write_zero(base + STREAM_SOLID, slot);
                write_zero(base + STREAM_CUTOUT, slot);
            }
        }
        return;
    }

    if (!draws) {
        return;
    }
    let base = ((cull.view_index * cull.pool_count) + m.pool) * STREAMS;
    if (has_solid) {
        let region = base + STREAM_SOLID;
        let k = atomicAdd(&counts[region], 1u);
        write_cmd(region, k, m.first_index, m.solid_count, m.base_vertex, slot);
    }
    if (has_cutout) {
        let region = base + STREAM_CUTOUT;
        let k = atomicAdd(&counts[region], 1u);
        write_cmd(region, k, m.cutout_first, m.index_count - m.solid_count, m.base_vertex, slot);
    }
}
