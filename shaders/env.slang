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
    // Overlays carry clip from local; the hand (kind 3) view from local,
    // and view_proj is the hand camera's projection.
    if (kind == KIND_SCREEN || kind == 3) {
        // Overlays: view_proj holds only the display's pre-rotation.
        float4x4 clip = transpose(float4x4(c0, c1, c2, c3));
        out.position = mul(pc.view_proj, mul(clip, float4(position, 1)));
    }
    // Vanilla draws the sky before the world: pinned to the far plane (0
    // with reversed depth), anything in the world covers it.
    if (sky) {
        out.position.z = 0;
    }
    // World lines: widened on screen, as vanilla's (2.5 px at 1920 wide).
    if (kind == 4) {
        float3 local = asfloat(pool.Load3(vertex * 52 + 12));
        float3 other = c0.xyz * local.x + c1.xyz * local.y + c2.xyz * local.z + origin;
        float2 size = max(pc.params.yz, float2(1, 1));
        float width = max(2.5, size.x / 1920.0 * 2.5) * max(asfloat(parts.Load(at + 20)), 1.0);
        // Pulled toward the eye by about the line's width, so the strip
        // doesn't sink into the faces it borders.
        float3 eye = asfloat(view.Load3(VIEW_CAMERA));
        float pull = width * 1.5 / size.y + 0.002;
        world = lerp(world, eye, pull);
        other = lerp(other, eye, pull);
        out.position = mul(pc.view_proj, float4(world, 1));
        float4 b = mul(pc.view_proj, float4(other, 1));
        float4 a = out.position;
        float2 dir = (b.xy / max(b.w, 1e-4) - a.xy / max(a.w, 1e-4)) * size;
        dir = length(dir) > 1e-6 ? normalize(dir) : float2(1, 0);
        // Square caps: each end pushed away from the other by half width.
        float2 offset = (float2(-dir.y, dir.x) * uv.x - dir) * width / size;
        out.position.xy += offset * a.w;
        uv = float2(0, 0);
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

// The first-person hand, unlit (the game sets its colour to the light).
[shader("fragment")]
float4 hand(VertexOut in): SV_Target {
    float4 color = env_color(in);
    if (color.a < 0.1) {
        discard;
    }
    return color;
}

float3 srgb_encode_env(float3 c) {
    return select(c <= 0.0031308, c * 12.92, 1.055 * pow(c, 1.0 / 2.4) - 0.055);
}

float3 srgb_decode_env(float3 c) {
    return select(c <= 0.04045, c / 12.92, pow((c + 0.055) / 1.055, 2.4));
}

// A held item's glint, as `renderer/glint.wgsl`: the item's texel as the
// mask, the glint texture (bindless index in pad[2]) scrolled and turned,
// added as vanilla adds it to the sRGB-encoded pixel. params.x: seconds.
[shader("fragment")]
float4 hand_glint(VertexOut in): SV_Target {
    ByteAddressBuffer parts = buffer_ro(pc.ids.x);
    uint at = in.part * PART_SIZE;
    float4 texel = texture_at(parts.Load(at + P_TEXTURE)).Sample(g_samplers[SAMPLER_NEAREST], in.uv);
    if (texel.a < 0.1) {
        discard;
    }
    float ticks = pc.params.x * 4000.0;
    float f = frac(ticks / 110000.0);
    float g = frac(ticks / 30000.0);
    float s = sin(3.14159265 / 18.0);
    float c = cos(3.14159265 / 18.0);
    float2 p = in.uv * 0.125;
    float2 st = float2(c * p.x - s * p.y, s * p.x + c * p.y) + float2(-f, g);
    float3 glint = texture_at(parts.Load(at + 28)).SampleLevel(g_samplers[SAMPLER_LINEAR], st, 0).rgb * 0.75;
    float3 base = saturate(texel.rgb);
    float3 lifted = srgb_decode_env(min(srgb_encode_env(base) + glint * glint, 1.0));
    return float4(lifted - base, 0);
}

// World lines (block outline, hitboxes, chunk borders): their colour.
[shader("fragment")]
float4 world_lines(VertexOut in): SV_Target {
    return env_color(in);
}

// The breaking cracks: vanilla multiplies them over the block. Blending
// black at 1 - the crack's grey multiplies the block by that grey.
[shader("fragment")]
float4 world_cracks(VertexOut in): SV_Target {
    float4 c = env_color(in);
    float grey = lerp(1.0, c.r, c.a);
    return float4(0, 0, 0, 1.0 - grey);
}

// The cloud layer, blended at its own depth. Its material turns fog off.
[shader("fragment")]
float4 clouds(VertexOut in): SV_Target {
    return env_color(in);
}
