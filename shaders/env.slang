// The sky and clouds (VULKAN_REWRITE_CHECKLIST.md 5.1): the game's sky
// parts (the sky disc, sun, moon, stars, the sunrise fan, the End sky) and
// its cloud layer, as `sky_part.wgsl` and the cloud material draw them.
//
// Parts (128 bytes, `EnvPart` in `src/vk/entity_dump.rs`): mesh, texture,
// alpha, kind, bias, then the UV scale and offset at 32, the tint at 48 and
// the transform at 64. Meshes and vertices as the entities'
// (`EntityVertex`, 52 bytes). Screen overlays (kind 2, the underwater,
// wall, fire, vignette and pumpkin quads of `screen_effects.rs`) carry
// their clip-space transform and draw last over everything.

import common;
import cull;
import world;

static const uint P_MESH = 0;
static const uint P_TEXTURE = 4;
static const uint P_KIND = 12;
static const uint P_UV = 32;
static const uint P_TINT = 48;
static const uint P_TRANSFORM = 64;
static const uint PART_SIZE = 128;
static const uint KIND_SKY = 0;
static const uint KIND_SCREEN = 2;
static const uint NO_TEXTURE = 0xFFFFFFFF;

struct VertexOut {
    float4 position: SV_Position;
    float2 uv;
    float4 color;
    float3 world;
    nointerpolation uint part;
};

// Push: ids.x parts, ids.z vertex pool, more.x the view buffer, view_proj
//       the camera.
[shader("vertex")]
VertexOut env_vertex(uint vertex: SV_VulkanVertexID, uint part: SV_VulkanInstanceID) {
    ByteAddressBuffer parts = buffer_ro(pc.ids.x);
    uint at = part * PART_SIZE;
    ByteAddressBuffer pool = buffer_ro(pc.ids.z);
    float3 position = asfloat(pool.Load3(vertex * 52));
    float2 uv = asfloat(pool.Load2(vertex * 52 + 24));
    float4 color = asfloat(pool.Load4(vertex * 52 + 32));
    float4 c0 = asfloat(parts.Load4(at + P_TRANSFORM));
    float4 c1 = asfloat(parts.Load4(at + P_TRANSFORM + 16));
    float4 c2 = asfloat(parts.Load4(at + P_TRANSFORM + 32));
    float4 c3 = asfloat(parts.Load4(at + P_TRANSFORM + 48));
    uint kind = parts.Load(at + P_KIND);
    bool sky = kind == KIND_SKY;
    float4 uv_transform = asfloat(parts.Load4(at + P_UV));
    RWByteAddressBuffer view = buffer_at(pc.more.x);
    // The sky moves with the camera: its rotation, scale and offset from
    // the eye as the game left them, around this frame's eye.
    float3 origin = sky ? c3.xyz + asfloat(view.Load3(VIEW_CAMERA)) : c3.xyz;
    float3 world = c0.xyz * position.x + c1.xyz * position.y + c2.xyz * position.z + origin;

    VertexOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    if (kind == KIND_SCREEN) {
        // Clip from local already; no depth test draws it.
        // view_proj holds only the display's pre-rotation here.
        float4x4 clip = transpose(float4x4(c0, c1, c2, c3));
        out.position = mul(pc.view_proj, mul(clip, float4(position, 1)));
    }
    // Vanilla draws the sky before the world: pinned to the far plane (0
    // with reversed depth), anything in the world covers it.
    if (sky) {
        out.position.z = 0;
    }
    out.uv = uv * uv_transform.xy + uv_transform.zw;
    out.color = color;
    out.world = world;
    out.part = part;
    return out;
}

float4 env_color(VertexOut in) {
    ByteAddressBuffer parts = buffer_ro(pc.ids.x);
    uint at = in.part * PART_SIZE;
    float4 color = asfloat(parts.Load4(at + P_TINT)) * in.color;
    uint texture = parts.Load(at + P_TEXTURE);
    if (texture != NO_TEXTURE) {
        color *= texture_at(texture).Sample(g_samplers[SAMPLER_NEAREST], in.uv);
    }
    return color;
}

// Blended sky parts (the sky disc, the sunrise fan).
[shader("fragment")]
float4 sky_blend(VertexOut in): SV_Target {
    return env_color(in);
}

// Added sky parts (sun, moon, stars): the colour scaled by its alpha, added.
[shader("fragment")]
float4 sky_add(VertexOut in): SV_Target {
    float4 color = env_color(in);
    return float4(color.rgb * color.a, 0);
}

// Screen overlays, blended over the finished frame.
[shader("fragment")]
float4 screen(VertexOut in): SV_Target {
    return env_color(in);
}

// The cloud layer, blended at its own depth. Its material turns fog off.
[shader("fragment")]
float4 clouds(VertexOut in): SV_Target {
    return env_color(in);
}
