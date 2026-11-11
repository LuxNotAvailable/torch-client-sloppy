// Particles (VULKAN_REWRITE_CHECKLIST.md 4.6): each live particle is one
// sprite, billboarded here from its centre and size, six vertices a sprite
// with no index buffer. Replaces the game's CPU-built batch meshes
// (`util::particles::draw::write_batch_mesh`), shading as their
// `LightMode::Text` material does: the sprite's colour, the lightmap at its
// levels, then fog.
//
// Sprite buffer (80 bytes, `ParticleSprite` in `src/vk/entity_dump.rs`):
// centre (float3), half side, uv rect (u0 v0 u1 v1), colour, roll, texture,
// levels (float2, times 16), flags.

import common;
import cull;
import world;

static const uint SPRITE_SIZE = 80;
static const uint S_CENTRE = 0;
static const uint S_UV = 16;
static const uint S_COLOR = 32;
static const uint S_ROLL = 48;
static const uint S_TEXTURE = 52;
static const uint S_LEVELS = 56;

// A sprite's texture that is the block atlas, which the terrain holds.
static const uint TEXTURE_ATLAS = 0xFFFFFFFE;

struct SpriteOut {
    float4 position: SV_Position;
    float2 uv;
    float4 color;
    float3 world;
    nointerpolation uint sprite;
};

// The quad's corners, two triangles, as `write_batch_mesh` winds them.
static const float2 CORNERS[6] = {
    float2(-1, -1), float2(1, -1), float2(1, 1),
    float2(-1, -1), float2(1, 1), float2(-1, 1),
};

// Push: ids.x sprites, ids.y the view buffer, ids.z the block atlas,
//       more.x view mode, more.yzw the camera's up (as bits);
//       params.xyz the camera's right; view_proj the camera.
[shader("vertex")]
SpriteOut particle_vertex(uint vertex: SV_VulkanVertexID, uint sprite: SV_VulkanInstanceID) {
    ByteAddressBuffer sprites = buffer_ro(pc.ids.x);
    uint at = sprite * SPRITE_SIZE;
    float4 centre = asfloat(sprites.Load4(at + S_CENTRE));
    float4 uv = asfloat(sprites.Load4(at + S_UV));
    float roll = asfloat(sprites.Load(at + S_ROLL));
    float3 right = pc.params.xyz * centre.w;
    float3 up = asfloat(pc.more.yzw) * centre.w;
    if (roll != 0.0) {
        float s = sin(roll);
        float c = cos(roll);
        float3 r = right * c + up * s;
        up = up * c - right * s;
        right = r;
    }
    float2 corner = CORNERS[vertex % 6];
    float3 world = centre.xyz + right * corner.x + up * corner.y;

    SpriteOut out;
    out.position = mul(pc.view_proj, float4(world, 1));
    // Left is u0, the bottom v1 (`write_batch_mesh`).
    out.uv = float2(corner.x < 0 ? uv.x : uv.z, corner.y < 0 ? uv.w : uv.y);
    out.color = asfloat(sprites.Load4(at + S_COLOR));
    out.world = world;
    out.sprite = sprite;
    return out;
}

float4 sprite_shade(SpriteOut in) {
    ByteAddressBuffer sprites = buffer_ro(pc.ids.x);
    uint at = in.sprite * SPRITE_SIZE;
    if (pc.more.x == VIEW_TEXTURE_INDEX) {
        uint i = in.sprite * 2654435761u;
        return float4(float3(i & 255, (i >> 8) & 255, (i >> 16) & 255) / 255.0 * 0.8 + 0.2, 1);
    }
    uint texture = sprites.Load(at + S_TEXTURE);
    if (texture == TEXTURE_ATLAS) {
        texture = pc.ids.z;
    }
    float4 color = texture_at(texture).SampleLevel(g_samplers[SAMPLER_NEAREST], in.uv, 0) * in.color;
    RWByteAddressBuffer view = buffer_at(pc.ids.y);
    color.rgb *= lightmap_texel(view, asfloat(sprites.Load2(at + S_LEVELS)));
    return vanilla_fog(view, color, in.world - asfloat(view.Load3(VIEW_CAMERA)));
}

// The opaque particle layer: cut out at vanilla's 0.1.
[shader("fragment")]
float4 particle_cutout(SpriteOut in): SV_Target {
    float4 color = sprite_shade(in);
    if (color.a < 0.1) {
        discard;
    }
    return float4(color.rgb, 1);
}

// The translucent layer: blended, and nothing fully clear drawn.
[shader("fragment")]
float4 particle_blend(SpriteOut in): SV_Target {
    float4 color = sprite_shade(in);
    if (color.a <= 0.0) {
        discard;
    }
    return color;
}
