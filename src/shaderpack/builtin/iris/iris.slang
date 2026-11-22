// The engine's side of a native Slang program, included before the pack's
// file (build.rs, `compile_torch_pack`). The pack's resources live on the
// shared bindless set, as `vk::pack::bindless` lays them out: textures at
// binding 0, samplers at 1, buffers at 2. A resource's index comes from the
// pass's slot table, `table[base + slot]`, with `table` and `base` in the
// push constants. Everything the engine decides per pass or per option
// (slots, uniform offsets, options, the alpha test) is a specialization
// constant it sets by name (`shaderpack::spirv`).

[[vk::binding(0, 0)]] Texture2D iris_textures[];
[[vk::binding(0, 0)]] Texture2D<float> iris_depth_textures[];
#ifdef MC_OS_ANDROID
// Integer targets (Android's RGBA16UI texel target), the same descriptors.
[[vk::binding(0, 0)]] Texture2D<uint4> iris_utextures[];
#endif
[[vk::binding(1, 0)]] SamplerState iris_samplers[];
[[vk::binding(1, 0)]] SamplerComparisonState iris_comparison_samplers[];
[[vk::binding(2, 0)]] ByteAddressBuffer iris_buffers[];
[[vk::binding(2, 0)]] RWByteAddressBuffer iris_rw_buffers[];

struct IrisBindlessPush {
    uint table;
    uint base;
    /// The draw's own texture, for a slot set to `IRIS_PER_DRAW`.
    uint texture;
    /// Shadow passes: the cascade drawn, as its zoom on the light's clip
    /// space (float bits; 1 the outer, more the inner), 0 for no cascades.
    uint cascade;
    /// Shadow passes: 1 where leaves cast solid (coarse cascades, whose
    /// texels span several leaf pixels and would only flicker).
    uint solid_leaves;
};
[[vk::push_constant]] ConstantBuffer<IrisBindlessPush> iris_bindless;

static const uint IRIS_PER_DRAW = 0xFFFFFFFFu;

uint iris_index(uint slot) {
    if (slot == IRIS_PER_DRAW) {
        return iris_bindless.texture;
    }
    return iris_buffers[iris_bindless.table].Load(4 * (iris_bindless.base + slot));
}

Texture2D iris_texture(uint slot) { return iris_textures[iris_index(slot)]; }
Texture2D<float> iris_depth_texture(uint slot) { return iris_depth_textures[iris_index(slot)]; }
#ifdef MC_OS_ANDROID
Texture2D<uint4> iris_utexture(uint slot) { return iris_utextures[iris_index(slot)]; }
// An integer target by name, as `iris_uint(colortex2)`: the name stays in
// the program's text, where the engine looks for what it reads.
#define iris_uint(name) iris_utexture(IRIS_SLOT_##name)
#endif
SamplerState iris_sampler(uint slot) { return iris_samplers[iris_index(slot)]; }
SamplerComparisonState iris_comparison_sampler(uint slot) { return iris_comparison_samplers[iris_index(slot)]; }
ByteAddressBuffer iris_buffer(uint slot) { return iris_buffers[iris_index(slot)]; }

[SpecializationConstant] const uint IRIS_SLOT_iris_vk_voxels = 0;

/// The voxel window traced reflections read (`vk::voxels`); every kind of
/// program can name it.
ByteAddressBuffer iris_vk_voxels() { return iris_buffer(IRIS_SLOT_iris_vk_voxels); }
RWByteAddressBuffer iris_rw_buffer(uint slot) { return iris_rw_buffers[iris_index(slot)]; }
/// A buffer by its own bindless index (the terrain's per-section pools).
ByteAddressBuffer iris_buffer_at(uint index) { return iris_buffers[NonUniformResourceIndex(index)]; }

uint2 iris_size(Texture2D t) {
    uint w, h;
    t.GetDimensions(w, h);
    return uint2(w, h);
}

uint2 iris_size(Texture2D<float> t) {
    uint w, h;
    t.GetDimensions(w, h);
    return uint2(w, h);
}

#include "slots.slang"
#include "frame.slang"

// The draw's variant: an upright target (final), linear output to an sRGB
// swapchain, and the alpha test of the layer drawn (0 none, then
// `AlphaFunction`'s operators: < == <= > != >=).
[SpecializationConstant] const bool IRIS_UPRIGHT = false;
[SpecializationConstant] const bool IRIS_LINEAR_OUTPUT = false;
[SpecializationConstant] const uint IRIS_ALPHA_FUNC = 0;
[SpecializationConstant] const float IRIS_ALPHA_REF = -1.0;

bool iris_alpha_pass(float a) {
    switch (IRIS_ALPHA_FUNC) {
    case 1: return a < IRIS_ALPHA_REF;
    case 2: return a == IRIS_ALPHA_REF;
    case 3: return a <= IRIS_ALPHA_REF;
    case 4: return a > IRIS_ALPHA_REF;
    case 5: return a != IRIS_ALPHA_REF;
    case 6: return a >= IRIS_ALPHA_REF;
    default: return true;
    }
}

/// GL clip space to Vulkan's: y down unless the target is upright, z 0..1.
float4 iris_clip(float4 p) {
    float4 o = p;
    if (IRIS_UPRIGHT) {
        o.y = -o.y;
    }
    o.z = (o.z + o.w) * 0.5;
    return o;
}

float4 iris_output(float4 c) {
    if (!IRIS_LINEAR_OUTPUT) {
        return c;
    }
    float3 d = clamp(c.rgb, 0.0, 1.0);
    float3 high = pow((d + 0.055) / 1.055, 2.4);
    return float4(select(d <= 0.04045, d / 12.92, high), c.a);
}

struct IrisAttributes {
    float3 position;
    float2 uv;
    float2 light;
    float4 color;
    float3 normal;
    float4 entity;
    float4 mid_block;
    float4 entity_color;
    int entity_id;
    int block_entity_id;
    int item_id;
    int render_stage;
};

// `features::RENDER_STAGES`, from the engine's defines.
[SpecializationConstant] const int MC_RENDER_STAGE_NONE = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_SKY = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_SUNSET = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_CUSTOM_SKY = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_SUN = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_MOON = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_STARS = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_VOID = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_TERRAIN_SOLID = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_TERRAIN_CUTOUT_MIPPED = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_TERRAIN_CUTOUT = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_ENTITIES = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_BLOCK_ENTITIES = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_DESTROY = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_OUTLINE = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_DEBUG = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_HAND_SOLID = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_TERRAIN_TRANSLUCENT = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_TRIPWIRE = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_PARTICLES = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_CLOUDS = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_RAIN_SNOW = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_WORLD_BORDER = 0;
[SpecializationConstant] const int MC_RENDER_STAGE_HAND_TRANSLUCENT = 0;
