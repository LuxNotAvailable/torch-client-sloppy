// The engine's shared shader interface (VULKAN_REWRITE_CHECKLIST.md 2.3,
// 2.4): one bindless descriptor set that every pipeline binds, and one push
// constant block. Kept in step with `src/vk/bindless.rs` and `Push` in
// `src/vk/pipeline.rs`; the build's reflection check compares them.

// Set 0: every sampled image, sampler and storage buffer, by index. Storage
// buffers are byte-address buffers, so one binding serves every layout:
// `g_buffers[i].Load<T>(offset)`.
[[vk::binding(0, 0)]] Texture2D g_textures[];
[[vk::binding(1, 0)]] SamplerState g_samplers[];
[[vk::binding(2, 0)]] RWByteAddressBuffer g_buffers[];
// The same buffers, read-only, for shaders that only read: the driver may
// take a faster load path (vertex pulling, `buffer_ro`).
[[vk::binding(2, 0)]] ByteAddressBuffer g_buffers_ro[];

// Per-draw values. `ids` and `more` carry bindless indices and small
// counts; what each slot means is up to the pass.
struct Push {
    uint4 ids;
    uint4 more;
    float4 params;
    float4x4 view_proj;
};

[[vk::push_constant]] ConstantBuffer<Push> pc;

// The sampler slots `bindless.rs` fills at startup.
static const uint SAMPLER_NEAREST = 0;
static const uint SAMPLER_LINEAR = 1;

Texture2D texture_at(uint index) {
    return g_textures[NonUniformResourceIndex(index)];
}

RWByteAddressBuffer buffer_at(uint index) {
    return g_buffers[NonUniformResourceIndex(index)];
}

ByteAddressBuffer buffer_ro(uint index) {
    return g_buffers_ro[NonUniformResourceIndex(index)];
}

// Debug view modes, as `vkview --view` names them.
static const uint VIEW_SHADED = 0;
static const uint VIEW_NORMALS = 1;
static const uint VIEW_TEXTURE_INDEX = 2;
static const uint VIEW_DEPTH = 3;
