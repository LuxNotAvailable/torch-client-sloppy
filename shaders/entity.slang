// Entities, block entities and the other `EntityMaterial` draws
// (VULKAN_REWRITE_CHECKLIST.md phase 4): one pipeline family over shared
// mesh pools, posed on the GPU from the rig-part buffer, culled by compute
// into indirect draws. Ports `src/renderer/entity.wgsl`.
//
// Buffers (byte offsets, matching `src/vk/entities.rs`):
// - instance (184 bytes, `EntityInstance` in `src/vk/entity_dump.rs`)
// - mesh table (16 bytes): first index, index count, base vertex, rigged
// - bounds (32 bytes): the instance's world box, lo and hi
// - vertex pool (52 bytes, `EntityVertex`)
// - part rows: three float4 rows per part matrix

import common;
import cull;

static const uint INSTANCE_SIZE = 184;
static const uint I_MESH = 0;
static const uint I_TEXTURE = 4;
static const uint I_TAG = 8;
static const uint I_ALPHA = 12;
static const uint I_TINT = 24;
static const uint I_LIGHT = 40;
static const uint I_LIGHT0 = 56;
static const uint I_LIGHT1 = 72;
static const uint I_SWITCHES = 88;
static const uint I_LEVELS = 104;
static const uint I_MODE = 112;
static const uint I_TRANSFORM = 120;

static const uint MODE_CARDINAL = 0;
static const uint MODE_FLAT = 1;
static const uint MODE_EMISSIVE = 2;
static const uint MODE_TEXT = 3;

static const uint ALPHA_BLEND = 2;
static const uint ALPHA_ADD = 3;

static const uint NO_TEXTURE = 0xFFFFFFFF;

Columns instance_transform(ByteAddressBuffer instances, uint at) {
    Columns m;
    for (uint i = 0; i < 4; i++) {
        m.c[i] = asfloat(instances.Load4(at + I_TRANSFORM + i * 16));
    }
    return m;
}

// Push: ids.x instances, ids.y mesh table, ids.z commands, ids.w counts,
//       more.x instance count, more.y solid capacity, more.z the cull view,
//       more.w where the blended commands start; params.x the blend ranks
//       buffer, params.y the bounds buffer (both as bits).
// Solid (opaque and cutout) instances append to one list; blended ones
// write their command at their rank (far to near, from the CPU), culled or
// not, so they blend in order.
[shader("compute")]
[numthreads(64, 1, 1)]
void cull_entities(uint3 id: SV_DispatchThreadID) {
    uint index = id.x;
    bool live = index < pc.more.x;
    RWByteAddressBuffer view = buffer_at(pc.more.z);
    uint flags = view.Load(VIEW_FLAGS);
    ByteAddressBuffer instances = buffer_ro(pc.ids.x);
    uint at = index * INSTANCE_SIZE;
    uint mesh = live ? instances.Load(at + I_MESH) : 0;
    uint alpha = live ? instances.Load(at + I_ALPHA) : 0;
    uint4 range = buffer_ro(pc.ids.y).Load4(mesh * 16);
    float3 lo = 0;
    float3 hi = 0;
    if (live) {
        ByteAddressBuffer bounds = buffer_ro(asuint(pc.params.y));
        lo = asfloat(bounds.Load3(index * 32));
        hi = asfloat(bounds.Load3(index * 32 + 16));
    }
    bool keep = live && range.y > 0;
    if (keep && (flags & CULL_FRUSTUM) != 0) {
        keep = in_frustum(load_columns(view, VIEW_VIEW_PROJ), lo, hi);
    }
    if (keep && (flags & CULL_HIZ) != 0) {
        keep = !hiz_occluded(view, lo, hi);
    }
    bool blended = live && alpha >= ALPHA_BLEND;
    if (blended) {
        uint rank = buffer_ro(asuint(pc.params.x)).Load(index * 4);
        write_command(pc.ids.z, pc.more.w + rank, range.y, range.x, int(range.z), index, keep ? 1 : 0);
    }
    uint solid = keep && !blended ? 1 : 0;
    uint slot = reserve(pc.ids.w, 0, solid, (flags & CULL_NO_WAVE) == 0);
    if (solid > 0 && slot < pc.more.y) {
        write_command(pc.ids.z, slot, range.y, range.x, int(range.z), index, 1);
    }
}

struct VertexOut {
    float4 position: SV_Position;
    float2 uv;
    float4 color;
    float2 accum;
    nointerpolation uint instance;
};

static const float LIGHT_POWER = 0.6;
static const float AMBIENT_LIGHT = 0.4;

float mix_light_separate(float2 light) {
    float2 clamped = max(0, light);
    return min(1.0, (clamped.x + clamped.y) * LIGHT_POWER + AMBIENT_LIGHT);
}

// Push: ids.x instances, ids.y mesh table, ids.z vertex pool, ids.w part
//       rows, more.y view mode, more.z glint texture, params.x seconds,
//       view_proj the camera.
[shader("vertex")]
VertexOut entity_vertex(uint vertex: SV_VulkanVertexID, uint instance: SV_VulkanInstanceID) {
    ByteAddressBuffer instances = buffer_ro(pc.ids.x);
    uint at = instance * INSTANCE_SIZE;
    ByteAddressBuffer pool = buffer_ro(pc.ids.z);
    uint v = vertex * 52;
    float3 position = asfloat(pool.Load3(v));
    float3 normal = asfloat(pool.Load3(v + 12));
    float2 uv = asfloat(pool.Load2(v + 24));
    float4 color = asfloat(pool.Load4(v + 32));
    uint part = pool.Load(v + 48);

    uint mesh = instances.Load(at + I_MESH);
    bool rigged = buffer_ro(pc.ids.y).Load(mesh * 16 + 12) != 0;
    if (rigged) {
        // The part's matrix, stored as its top three rows; the normal goes
        // through the cofactor matrix, the inverse transpose up to the
        // determinant.
        ByteAddressBuffer parts = buffer_ro(pc.ids.w);
        uint row = (instances.Load(at + I_TAG) + part) * 48;
        float4 r0 = asfloat(parts.Load4(row));
        float4 r1 = asfloat(parts.Load4(row + 16));
        float4 r2 = asfloat(parts.Load4(row + 32));
        float4 p = float4(position, 1);
        position = float3(dot(r0, p), dot(r1, p), dot(r2, p));
        float3 c0 = float3(r0.x, r1.x, r2.x);
        float3 c1 = float3(r0.y, r1.y, r2.y);
        float3 c2 = float3(r0.z, r1.z, r2.z);
        float3 n = cross(c1, c2) * normal.x + cross(c2, c0) * normal.y + cross(c0, c1) * normal.z;
        float det = dot(c0, cross(c1, c2));
        normal = dot(n, n) > 1e-12 ? normalize(n) * sign(det) : normal;
    }
    Columns world = instance_transform(instances, at);
    float4 world_position = world.apply(position);
    float3 world_normal = normalize(world.c[0].xyz * normal.x + world.c[1].xyz * normal.y + world.c[2].xyz * normal.z);

    VertexOut out;
    out.position = mul(pc.view_proj, float4(world_position.xyz, 1));
    out.uv = uv;
    out.instance = instance;
    uint mode = instances.Load(at + I_MODE);
    if (mode == MODE_CARDINAL) {
        float3 l0 = asfloat(instances.Load3(at + I_LIGHT0));
        float3 l1 = asfloat(instances.Load3(at + I_LIGHT1));
        float2 light = float2(dot(l0, world_normal), dot(l1, world_normal));
        out.accum = float2(mix_light_separate(light), mix_light_separate(-light));
        out.color = color;
    } else if (mode == MODE_TEXT) {
        out.accum = 1;
        out.color = color;
    } else {
        out.accum = color.a;
        out.color = float4(color.rgb, 1);
    }
    return out;
}

float3 srgb_encode(float3 c) {
    float3 low = c * 12.92;
    float3 high = 1.055 * pow(c, 1.0 / 2.4) - 0.055;
    return select(c <= 0.0031308, low, high);
}

float3 srgb_decode(float3 c) {
    float3 low = c / 12.92;
    float3 high = pow((c + 0.055) / 1.055, 2.4);
    return select(c <= 0.04045, low, high);
}

// Vanilla's armor glint (see entity.wgsl): the glint texture through
// translate(-f, g) * rotateZ(pi / 18) * scale(0.16).
float3 glint(float2 uv) {
    float ticks = pc.params.x * 4000.0;
    float f = frac(ticks / 110000.0);
    float g = frac(ticks / 30000.0);
    float s = sin(3.14159265 / 18.0);
    float c = cos(3.14159265 / 18.0);
    float2 p = uv * 0.16;
    float2 st = float2(c * p.x - s * p.y, s * p.x + c * p.y) + float2(-f, g);
    return texture_at(pc.more.z).SampleLevel(g_samplers[SAMPLER_LINEAR], st, 0).rgb * 0.75;
}

// The light at the instance's levels. Until the lightmap is ported
// (phase 5): daylight from the brighter of block and sky light, on
// vanilla's brightness curve, as the terrain does.
float3 cell_light(float4 switches, float2 levels) {
    if (switches.x < 0.5) {
        return 1;
    }
    float level = max(levels.x, levels.y) / 240.0;
    float bright = level / (4.0 - 3.0 * level);
    return lerp(0.05, 1.0, saturate(bright));
}

float4 entity_shade(VertexOut in, bool front) {
    ByteAddressBuffer instances = buffer_ro(pc.ids.x);
    uint at = in.instance * INSTANCE_SIZE;
    if (pc.more.y == VIEW_TEXTURE_INDEX) {
        uint i = in.instance * 2654435761u;
        return float4(float3(i & 255, (i >> 8) & 255, (i >> 16) & 255) / 255.0 * 0.8 + 0.2, 1);
    }
    uint texture = instances.Load(at + I_TEXTURE);
    float4 color = texture == NO_TEXTURE
        ? float4(1, 1, 1, 1)
        : texture_at(texture).SampleLevel(g_samplers[SAMPLER_NEAREST], in.uv, 0);
    float4 light = asfloat(instances.Load4(at + I_LIGHT));
    if (color.a < light.a) {
        discard;
    }
    float4 tint = asfloat(instances.Load4(at + I_TINT));
    float4 light0 = asfloat(instances.Load4(at + I_LIGHT0));
    float4 light1 = asfloat(instances.Load4(at + I_LIGHT1));
    float4 switches = asfloat(instances.Load4(at + I_SWITCHES));
    float shade = front ? in.accum.x : in.accum.y;
    color = color * in.color * tint;
    color.rgb *= shade;
    if (light1.w < 1.0) {
        float3 overlay = lerp(float3(1, 0, 0), float3(1, 1, 1), light0.w);
        color.rgb = srgb_decode(lerp(overlay, srgb_encode(color.rgb), light1.w));
    }
    if (instances.Load(at + I_MODE) != MODE_EMISSIVE) {
        color.rgb *= cell_light(switches, asfloat(instances.Load2(at + I_LEVELS)));
    }
    if (switches.w > 0.0 && pc.more.z != NO_TEXTURE) {
        // Vanilla adds the glint's colour squared to the encoded value.
        float3 g = glint(in.uv);
        color.rgb = srgb_decode(min(srgb_encode(saturate(color.rgb)) + g * g, 1.0));
    }
    return color;
}

[shader("fragment")]
float4 entity_solid(VertexOut in, bool front: SV_IsFrontFace): SV_Target {
    return float4(entity_shade(in, front).rgb, 1);
}

[shader("fragment")]
float4 entity_blend(VertexOut in, bool front: SV_IsFrontFace): SV_Target {
    return entity_shade(in, front);
}
