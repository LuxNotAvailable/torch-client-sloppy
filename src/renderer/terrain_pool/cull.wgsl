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
    solid_groups: array<u32, 8>,
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
    region_cap: u32,
    order_len: u32,
    // The occlusion pyramid's view: the clip matrix of the frame it was
    // built from, whether that depth was reversed, and its level count.
    hiz_clip: mat4x4<f32>,
    hiz_reverse: u32,
    hiz_levels: u32,
    hiz_pad0: u32,
    hiz_pad1: u32,
    // The camera; w 1 when solid quads facing away from it are left out.
    eye: vec4<f32>,
    // Where vanilla's fog hides everything: x cylindrical, y spherical.
    fog: vec4<f32>,
}

const FACE_GROUPS: u32 = 7u;

const STREAM_SOLID: u32 = 0u;
const STREAM_CUTOUT: u32 = 1u;
const STREAMS: u32 = 3u;
const META_LIVE: u32 = 1u;
const META_WATER: u32 = 2u;
const VIEW_UNOCCLUDED: u32 = 2u;
const VIEW_WITHIN: u32 = 4u;
const VIEW_ALWAYS: u32 = 8u;
const VIEW_HIZ: u32 = 16u;
// u32s after the regions' counts holding the Hi-Z counters.
const HIZ_STATS: u32 = 4u;

@group(0) @binding(0) var<storage, read> metas: array<SlotMeta>;
@group(0) @binding(1) var<storage, read> vis: array<u32>;
@group(0) @binding(2) var<storage, read_write> commands: array<DrawIndexedIndirectArgs>;
@group(0) @binding(3) var<storage, read_write> counts: array<atomic<u32>>;
@group(0) @binding(4) var<uniform> cull: CullView;
@group(0) @binding(5) var hiz: texture_2d<f32>;
// Slots nearest the camera first; the dispatch walks it so commands come out
// roughly front to back.
@group(0) @binding(6) var<storage, read> order: array<u32>;

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
    // Address the level through level 0: a level is level 0 halved and
    // floored, its odd edge's extra row or column folded into its last
    // texel, so texel i covers level-0 texels i << level up to the next.
    // Indexing it by uv * its own size instead lands up to half a texel up
    // and left of the box wherever a size isn't a multiple of 1 << level,
    // and read nearer depth from above the box: sections near the bottom
    // and right of the screen were culled behind what was above them.
    let dims = vec2<i32>(textureDimensions(hiz, level));
    let shift = vec2<u32>(level);
    let p0 = clamp(vec2<i32>(uv_min * size0) >> shift, vec2<i32>(0), dims - 1);
    let p1 = clamp(vec2<i32>(uv_max * size0) >> shift, vec2<i32>(0), dims - 1);
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
    commands[region * cull.region_cap + k] = DrawIndexedIndirectArgs(0u, 1u, 0u, 0i, 0u);
}

/// Whether a facing group's quads can face the eye from anywhere in the
/// section's box (Sodium's face culling): a +Y face shows only from above
/// the section's bottom, and so on. The last group always draws.
fn group_faces_eye(group: u32, lo: vec3<f32>, hi: vec3<f32>) -> bool {
    if cull.eye.w == 0.0 {
        return true;
    }
    let e = cull.eye.xyz;
    switch group {
        case 0u: { return e.y > lo.y; }
        case 1u: { return e.y < hi.y; }
        case 2u: { return e.z < hi.z; }
        case 3u: { return e.z > lo.z; }
        case 4u: { return e.x < hi.x; }
        case 5u: { return e.x > lo.x; }
        default: { return true; }
    }
}

/// Whether vanilla's fog covers the whole box from the eye (the terrain
/// shader's formula at the box's nearest point), for a camera view.
fn fogged(lo: vec3<f32>, hi: vec3<f32>) -> bool {
    if (cull.eye.w == 0.0) {
        return false;
    }
    let d = clamp(cull.eye.xyz, lo, hi) - cull.eye.xyz;
    let cylindrical = max(length(d.xz), abs(d.y));
    return cylindrical >= cull.fog.x || length(d) >= cull.fog.y;
}

fn write_cmd(region: u32, k: u32, first_index: u32, count: u32, base_vertex: u32, slot: u32) {
    commands[region * cull.region_cap + k] = DrawIndexedIndirectArgs(
        count, 1u, first_index, i32(base_vertex), slot,
    );
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    // This invocation's place in the walk, and the slot there.
    let at = id.x;
    if (at >= cull.slot_cap) {
        return;
    }
    var slot = at;
    if (at < cull.order_len && at < arrayLength(&order)) {
        slot = order[at];
    }
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
        inside = in_frustum(lo, hi) && !fogged(lo, hi);
        if (inside && (cull.flags & VIEW_HIZ) != 0u) {
            // The report's Hi-Z counters, after the regions' counts:
            // sections tested against the pyramid, and those it hid.
            // After the counters, a bit per section the test hid, for
            // the Culled sections view.
            let bit_words = (cull.slot_cap + 31u) / 32u;
            let stats = arrayLength(&counts) - bit_words - HIZ_STATS;
            atomicAdd(&counts[stats], 1u);
            if (hiz_occluded(lo, hi)) {
                atomicAdd(&counts[stats + 1u], 1u);
                atomicOr(&counts[stats + HIZ_STATS + slot / 32u], 1u << (slot % 32u));
                inside = false;
            }
        }
    }

    let draws = live && visible && inside;
    let is_water = draws && (m.flags & META_WATER) != 0u;
    let has_solid = draws && !is_water && m.solid_count > 0u;
    let has_cutout = draws && !is_water && m.index_count > m.solid_count;

    let lo = vec3<f32>(m.origin) + m.min;
    let hi = vec3<f32>(m.origin) + m.max;
    if (!compact) {
        // Each slot owns FACE_GROUPS commands: the solid groups facing the
        // eye, the cutout run in the first, the rest zeroed.
        let own = at * FACE_GROUPS;
        for (var pool = 0u; pool < cull.pool_count; pool = pool + 1u) {
            let base = ((cull.view_index * cull.pool_count) + pool) * STREAMS;
            let mine = draws && m.pool == pool;
            var first = m.first_index;
            for (var g = 0u; g < FACE_GROUPS; g = g + 1u) {
                let count = m.solid_groups[g];
                if (mine && has_solid && count > 0u && group_faces_eye(g, lo, hi)) {
                    write_cmd(base + STREAM_SOLID, own + g, first, count, m.base_vertex, slot);
                } else {
                    write_zero(base + STREAM_SOLID, own + g);
                }
                first = first + count;
                if (g == 0u && mine && has_cutout) {
                    write_cmd(base + STREAM_CUTOUT, own, m.cutout_first,
                        m.index_count - m.solid_count, m.base_vertex, slot);
                } else {
                    write_zero(base + STREAM_CUTOUT, own + g);
                }
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
        var first = m.first_index;
        for (var g = 0u; g < FACE_GROUPS; g = g + 1u) {
            let count = m.solid_groups[g];
            if (count > 0u && group_faces_eye(g, lo, hi)) {
                let k = atomicAdd(&counts[region], 1u);
                write_cmd(region, k, first, count, m.base_vertex, slot);
            }
            first = first + count;
        }
    }
    if (has_cutout) {
        let region = base + STREAM_CUTOUT;
        let k = atomicAdd(&counts[region], 1u);
        write_cmd(region, k, m.cutout_first, m.index_count - m.solid_count, m.base_vertex, slot);
    }
}
