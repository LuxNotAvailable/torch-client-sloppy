use super::TERRAIN_SET;
use crate::renderer::terrain_pool::ORIGIN_ROW;
use crate::renderer::terrain_pool::draw::SLOT_TABLE_BINDING;

thread_local! {
    static VULKAN_TERRAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Compiles terrain programs for the native renderer while `f` runs.
pub(crate) fn with_vulkan_terrain<T>(f: impl FnOnce() -> T) -> T {
    VULKAN_TERRAIN.with(|v| v.set(true));
    let out = f();
    VULKAN_TERRAIN.with(|v| v.set(false));
    out
}

pub(crate) fn vulkan_terrain() -> bool {
    VULKAN_TERRAIN.with(std::cell::Cell::get)
}

/// The native renderer's terrain input: each vertex pulled from its
/// section's pool (`vk::terrain`, 128-byte slots: origin at word 2, pool's
/// bindless index at word 3's y). `IrisVkPool` is indexed by `iris_VkPool`
/// (`vk::pack::bindless`).
const VK_TERRAIN: &str = "layout(std430, set = 2, binding = 7) readonly buffer IrisVkSlots { uvec4 iris_VkSlot[]; };
layout(std430, set = 2, binding = 8) readonly buffer IrisVkPool { uint iris_VkWords[]; };
layout(std430, set = 2, binding = 9) readonly buffer IrisVkQuads { uvec4 iris_VkQuadWords[]; };
uint iris_VkPool;
uint iris_VkQuadPool;
ivec4 iris_RawPosition;
vec2 iris_RawUV0;
vec4 iris_RawLight;
vec4 iris_RawColor;
ivec3 iris_origin() {
    int slot = int(gl_InstanceIndex) * 8;
    iris_VkPool = iris_VkSlot[slot + 3].y;
    iris_VkQuadPool = iris_VkSlot[slot + 3].w;
    uint v = uint(gl_VertexIndex) * 5u;
    uint x = iris_VkWords[v];
    uint y = iris_VkWords[v + 1u];
    uint z = iris_VkWords[v + 2u];
    uint w = iris_VkWords[v + 3u];
    uint c = iris_VkWords[v + 4u];
    iris_RawPosition = ivec4(bitfieldExtract(int(x), 0, 16), bitfieldExtract(int(x), 16, 16),
        bitfieldExtract(int(y), 0, 16), bitfieldExtract(int(y), 16, 16));
    iris_RawUV0 = vec2(float(z & 65535u), float(z >> 16u)) / 65535.0;
    iris_RawLight = vec4(float(w & 255u), float((w >> 8u) & 255u), float((w >> 16u) & 255u), float(w >> 24u)) / 255.0;
    iris_RawColor = vec4(float(c & 255u), float((c >> 8u) & 255u), float((c >> 16u) & 255u), float(c >> 24u)) / 255.0;
    return ivec3(iris_VkSlot[slot + 2].xyz);
}
";

pub(crate) fn terrain_prelude(direct: bool) -> String {
    if vulkan_terrain() {
        return format!(
            "{VK_TERRAIN}ivec3 iris_Origin;\n\
             vec3 iris_position() {{\n    \
                 vec3 camera_block = floor(iris_CameraPosition.xyz);\n    \
                 return vec3(iris_Origin - ivec3(camera_block)) - (iris_CameraPosition.xyz - camera_block)\n        \
                     + vec3(iris_RawPosition.xyz) / 256.0;\n\
             }}\n\
             vec2 iris_uv0() {{ return iris_RawUV0; }}\n\
             vec2 iris_light() {{ return iris_RawLight.xy * 255.0; }}\n\
             vec4 iris_color() {{ return iris_RawColor; }}\n\
             {}",
            vk_pack_stream()
        );
    }
    let origin = if direct {
        format!(
            "layout(set = {TERRAIN_SET}, binding = {SLOT_TABLE_BINDING}) uniform itexture2D iris_Origins;\n\
             ivec3 iris_origin() {{\n    \
                 int slot = iris_RawPosition.w & 0xFFFF;\n    \
                 return texelFetch(iris_Origins, ivec2(slot % {ORIGIN_ROW}, slot / {ORIGIN_ROW}), 0).xyz;\n\
             }}\n"
        )
    } else {
        format!(
            "layout(std430, set = {TERRAIN_SET}, binding = {SLOT_TABLE_BINDING}) readonly buffer IrisSlotTable {{\n    \
                 ivec4 iris_SlotWords[];\n\
             }};\n\
             ivec3 iris_origin() {{\n    \
                 return iris_SlotWords[gl_InstanceIndex * 6 + 2].xyz;\n\
             }}\n"
        )
    };
    format!(
        "layout(location = 0) in ivec4 iris_RawPosition;\n\
         layout(location = 1) in vec2 iris_RawUV0;\n\
         layout(location = 2) in vec4 iris_RawLight;\n\
         layout(location = 3) in vec4 iris_RawColor;\n\
         {origin}\
         ivec3 iris_Origin;\n\
         vec3 iris_position() {{\n    \
             vec3 camera_block = floor(iris_CameraPosition.xyz);\n    \
             return vec3(iris_Origin - ivec3(camera_block)) - (iris_CameraPosition.xyz - camera_block)\n        \
                 + vec3(iris_RawPosition.xyz) / 256.0;\n\
         }}\n\
         vec2 iris_uv0() {{ return iris_RawUV0; }}\n\
         vec2 iris_light() {{ return iris_RawLight.xy * 255.0; }}\n\
         vec4 iris_color() {{ return iris_RawColor; }}\n\
         {PACK_STREAM}"
    )
}

pub(crate) const PACK_QUADS_SET: u32 = 2;

const PACK_STREAM: &str = concat!(
    "layout(set = 2, binding = 0) uniform utexture2D iris_PackQuads;\n",
    "// Decoded once, before the pack's `main` (`TERRAIN_BEGIN`).\n",
    "uvec4 iris_Quad;\n",
    "uvec4 iris_pack_quad() {\n",
    "    int quad = int(gl_VertexIndex) >> 2;\n",
    "    return texelFetch(iris_PackQuads, ivec2(quad % 2048, quad / 2048), 0);\n",
    "}\n",
    "vec3 iris_snorm3(uint v) {\n",
    "    ivec3 b = ivec3(int(v << 24u) >> 24, int(v << 16u) >> 24, int(v << 8u) >> 24);\n",
    "    return max(vec3(b) / 127.0, vec3(-1.0));\n",
    "}\n",
    "vec3 iris_normal() {\n",
    "    vec3 n = iris_snorm3(iris_Quad.x);\n",
    "    return dot(n, n) > 0.25 ? normalize(n) : vec3(0.0, 1.0, 0.0);\n",
    "}\n",
    "vec4 iris_tangent() {\n",
    "    uint word = iris_Quad.y;\n",
    "    vec3 t = iris_snorm3(word);\n",
    "    return dot(t, t) > 0.25 ? vec4(normalize(t), int(word) < 0 ? -1.0 : 1.0) : vec4(1.0, 0.0, 0.0, 1.0);\n",
    "}\n",
    "vec4 iris_mid_uv() {\n",
    "    uint word = iris_Quad.z;\n",
    "    return vec4(vec2(float(word & 65535u), float(word >> 16u)) / 65535.0, 0.0, 1.0);\n",
    "}\n",
    "vec4 iris_entity() {\n",
    "    uint id = iris_Quad.w & 65535u;\n",
    "    return vec4(id == 65535u ? -1.0 : float(id), 0.0, 0.0, 1.0);\n",
    "}\n",
    "vec4 iris_mid_block() {\n",
    "    uint word = iris_Quad.w;\n",
    "    vec3 cell = vec3(float((word >> 16u) & 15u), float((word >> 20u) & 15u), float((word >> 24u) & 15u));\n",
    "    return vec4((cell + 0.5 - vec3(iris_RawPosition.xyz) / 256.0) * 64.0, float(word >> 28u));\n",
    "}\n",
);

/// `PACK_STREAM` reading the section's pool of pack quads (a quad at its
/// section's base vertex + its own number), none without a pack.
fn vk_pack_stream() -> String {
    let texture = "layout(set = 2, binding = 0) uniform utexture2D iris_PackQuads;\n";
    let read = "    int quad = int(gl_VertexIndex) >> 2;\n    \
                return texelFetch(iris_PackQuads, ivec2(quad % 2048, quad / 2048), 0);\n";
    assert!(PACK_STREAM.contains(texture) && PACK_STREAM.contains(read));
    PACK_STREAM.replace(texture, "").replace(
        read,
        "    if (iris_VkQuadPool == 4294967295u) { return uvec4(0u, 0u, 0u, 65535u); }\n    \
             uint base = uint(iris_VkSlot[int(gl_InstanceIndex) * 8 + 2].w);\n    \
             return iris_VkQuadWords[base + ((uint(gl_VertexIndex) - base) >> 2)];\n",
    )
}

pub(super) const TERRAIN_BEGIN: &str =
    "iris_Origin = iris_origin();\n    iris_Quad = iris_pack_quad();\n    ";

const _: () = assert!(PACK_QUADS_SET == 2 && crate::renderer::packvertex::QUAD_ROW == 2048);

/// The vertex prelude of an entity program. `skinned` is the variant for a
/// GPU-posed rig's mesh: each vertex carries its part index, and the part's
/// matrix (three rows in the rig buffer, from the instance's part base) poses
/// it before the model matrix, as the client's own entity shader does.
/// The native renderer's entity input (`vk::entities`): instance (184
/// bytes, words below), mesh table, vertex pool (52 bytes) and rig parts,
/// read by `iris_vk_entity` (`VK_ENTITY_BEGIN`) into the globals the game's
/// entity prelude takes as attributes. Posed or not by the mesh's flag.
const VK_ENTITY: &str = "layout(std430, set = 2, binding = 10) readonly buffer IrisVkInstances { uint iris_VkInst[]; };
layout(std430, set = 2, binding = 11) readonly buffer IrisVkMeshes { uvec4 iris_VkMesh[]; };
layout(std430, set = 2, binding = 12) readonly buffer IrisVkVertices { uint iris_VkVert[]; };
layout(std430, set = 2, binding = 13) readonly buffer IrisVkParts { vec4 iris_VkPart[]; };
vec3 iris_EntityPosition;
vec3 iris_EntityNormal;
vec2 iris_EntityUV;
vec4 iris_EntityColor;
vec4 iris_ModelRow0;
vec4 iris_ModelRow1;
vec4 iris_ModelRow2;
vec4 iris_EntityTint;
vec4 entityColor;
vec2 iris_EntityLight;
int entityId;
int blockEntityId;
int currentRenderedItemId;
int renderStage;
uint iris_EntityFlags;
vec4 iris_GlintTransform;
uint iris_PartRow;
bool iris_Rigged;
float iris_vk_f(uint w) { return uintBitsToFloat(iris_VkInst[w]); }
vec4 iris_vk_f4(uint w) { return vec4(iris_vk_f(w), iris_vk_f(w + 1u), iris_vk_f(w + 2u), iris_vk_f(w + 3u)); }
void iris_vk_entity() {
    uint at = uint(gl_InstanceIndex) * 46u;
    uint v = uint(gl_VertexIndex) * 13u;
    iris_EntityPosition = uintBitsToFloat(uvec3(iris_VkVert[v], iris_VkVert[v + 1u], iris_VkVert[v + 2u]));
    iris_EntityNormal = uintBitsToFloat(uvec3(iris_VkVert[v + 3u], iris_VkVert[v + 4u], iris_VkVert[v + 5u]));
    iris_EntityUV = uintBitsToFloat(uvec2(iris_VkVert[v + 6u], iris_VkVert[v + 7u]));
    iris_EntityColor = uintBitsToFloat(uvec4(iris_VkVert[v + 8u], iris_VkVert[v + 9u], iris_VkVert[v + 10u], iris_VkVert[v + 11u]));
    uint mesh = iris_VkInst[at];
    iris_Rigged = iris_VkMesh[mesh].w != 0u;
    iris_PartRow = (iris_VkInst[at + 2u] + iris_VkVert[v + 12u]) * 3u;
    iris_EntityTint = iris_vk_f4(at + 6u);
    vec4 light0 = iris_vk_f4(at + 14u);
    vec4 light1 = iris_vk_f4(at + 18u);
    entityColor = vec4(light0.w < 0.5 ? vec3(1.0, 0.0, 0.0) : vec3(1.0), clamp(1.0 - light1.w, 0.0, 1.0));
    iris_EntityLight = vec2(iris_vk_f(at + 26u), iris_vk_f(at + 27u));
    uint mode = iris_VkInst[at + 28u];
    iris_EntityFlags = (mode == 1u || mode == 2u) ? 1u : 0u;
    if (mode == 2u) { iris_EntityLight = vec2(240.0); }
    vec4 c0 = iris_vk_f4(at + 30u);
    vec4 c1 = iris_vk_f4(at + 34u);
    vec4 c2 = iris_vk_f4(at + 38u);
    vec4 c3 = iris_vk_f4(at + 42u);
    // World transforms, made camera-relative as the game's draws are.
    vec3 eye = iris_CameraPosition.xyz;
    iris_ModelRow0 = vec4(c0.x, c1.x, c2.x, c3.x - eye.x);
    iris_ModelRow1 = vec4(c0.y, c1.y, c2.y, c3.y - eye.y);
    iris_ModelRow2 = vec4(c0.z, c1.z, c2.z, c3.z - eye.z);
    entityId = -1;
    blockEntityId = -1;
    currentRenderedItemId = 0;
    // Kinds past 16 carry a pack renderStage (the sky's pieces).
    uint kind = iris_VkInst[at + 5u];
    renderStage = kind >= 16u ? int(kind - 16u) : 0;
}
vec3 iris_local_position() {
    if (!iris_Rigged) { return iris_EntityPosition; }
    vec4 p = vec4(iris_EntityPosition, 1.0);
    return vec3(dot(iris_VkPart[iris_PartRow], p), dot(iris_VkPart[iris_PartRow + 1u], p), dot(iris_VkPart[iris_PartRow + 2u], p));
}
vec3 iris_local_normal() {
    if (!iris_Rigged) { return iris_EntityNormal; }
    vec4 r0 = iris_VkPart[iris_PartRow];
    vec4 r1 = iris_VkPart[iris_PartRow + 1u];
    vec4 r2 = iris_VkPart[iris_PartRow + 2u];
    vec3 c0 = vec3(r0.x, r1.x, r2.x);
    vec3 c1 = vec3(r0.y, r1.y, r2.y);
    vec3 c2 = vec3(r0.z, r1.z, r2.z);
    vec3 n = mat3(cross(c1, c2), cross(c2, c0), cross(c0, c1)) * iris_EntityNormal;
    float det = dot(c0, cross(c1, c2));
    return dot(n, n) > 1e-12 ? normalize(n) * sign(det) : iris_EntityNormal;
}
";

/// Before an entity program's `main` on the native renderer.
pub(super) const VK_ENTITY_BEGIN: &str = "iris_vk_entity();\n    ";

pub(super) fn entity_prelude(skinned: bool) -> String {
    if vulkan_terrain() {
        return format!("{VK_ENTITY}{ENTITY_FUNCTIONS}");
    }
    let local = if skinned {
        SKINNED_LOCAL
    } else {
        "vec3 iris_local_position() { return iris_EntityPosition; }
         vec3 iris_local_normal() { return iris_EntityNormal; }
"
    };
    format!("{ENTITY_INPUTS}{local}{ENTITY_FUNCTIONS}")
}

const ENTITY_INPUTS: &str = "layout(location = 0) in vec3 iris_EntityPosition;
layout(location = 1) in vec3 iris_EntityNormal;
layout(location = 2) in vec2 iris_EntityUV;
layout(location = 3) in vec4 iris_EntityColor;
layout(location = 4) in vec4 iris_ModelRow0;
layout(location = 5) in vec4 iris_ModelRow1;
layout(location = 6) in vec4 iris_ModelRow2;
layout(location = 7) in vec4 iris_EntityTint;
layout(location = 8) in vec4 entityColor;
layout(location = 9) in vec2 iris_EntityLight;
layout(location = 10) in int entityId;
layout(location = 11) in int blockEntityId;
layout(location = 12) in int currentRenderedItemId;
layout(location = 13) in int renderStage;
layout(location = 14) in uint iris_EntityFlags;
layout(location = 15) in uint iris_PartBase;
layout(location = 17) in vec4 iris_GlintTransform;
";

const _: () = assert!(
    crate::renderer::packdraw::entities::ENTITY_GLINT_LOCATION == 17
        && crate::renderer::packdraw::entities::ENTITY_GLINT == 2
        && crate::renderer::packdraw::entities::ENTITY_PART_BASE_LOCATION == 15
        && crate::renderer::packdraw::entities::ENTITY_PART_LOCATION == 16
        && crate::renderer::packdraw::entities::ENTITY_PARTS_SET == 1
);

/// The part's matrix, stored as its top three rows; the normal goes through
/// the cofactor matrix, the inverse transpose up to the determinant.
const SKINNED_LOCAL: &str = "layout(location = 16) in uint iris_EntityPart;
layout(std430, set = 1, binding = 0) readonly buffer IrisRigParts { vec4 iris_rig_parts[]; };
uint iris_part_row() { return (iris_PartBase + iris_EntityPart) * 3u; }
vec3 iris_local_position() {
        uint row = iris_part_row();
        vec4 p = vec4(iris_EntityPosition, 1.0);
        return vec3(dot(iris_rig_parts[row], p), dot(iris_rig_parts[row + 1u], p), dot(iris_rig_parts[row + 2u], p));
}
vec3 iris_local_normal() {
        uint row = iris_part_row();
        vec4 r0 = iris_rig_parts[row];
        vec4 r1 = iris_rig_parts[row + 1u];
        vec4 r2 = iris_rig_parts[row + 2u];
        vec3 c0 = vec3(r0.x, r1.x, r2.x);
        vec3 c1 = vec3(r0.y, r1.y, r2.y);
        vec3 c2 = vec3(r0.z, r1.z, r2.z);
        vec3 n = mat3(cross(c1, c2), cross(c2, c0), cross(c0, c1)) * iris_EntityNormal;
        float det = dot(c0, cross(c1, c2));
        return dot(n, n) > 1e-12 ? normalize(n) * sign(det) : iris_EntityNormal;
}
";

const ENTITY_FUNCTIONS: &str = "vec3 iris_model(vec4 v) { return vec3(dot(iris_ModelRow0, v), dot(iris_ModelRow1, v), dot(iris_ModelRow2, v)); }
vec3 iris_position() { return iris_model(vec4(iris_local_position(), 1.0)); }
vec2 iris_uv0() {
        if ((iris_EntityFlags & 2u) == 0u) { return iris_EntityUV; }
        vec4 g = iris_GlintTransform;
        vec2 uv = iris_EntityUV;
        return vec2(g.x * uv.x - g.y * uv.y, g.y * uv.x + g.x * uv.y) + g.zw;
}
vec2 iris_light() { return iris_EntityLight; }
vec4 iris_color() {
        vec4 color = iris_EntityColor;
        if ((iris_EntityFlags & 1u) != 0u) { color.a = 1.0; }
        return color * iris_EntityTint;
}
vec3 iris_normal() {
        vec3 n = iris_model(vec4(iris_local_normal(), 0.0));
        return dot(n, n) > 1e-8 ? normalize(n) : vec3(0.0, 1.0, 0.0);
}
vec4 iris_entity() { return vec4(-1.0, 0.0, 0.0, 1.0); }
vec4 iris_tangent() { return vec4(1.0, 0.0, 0.0, 1.0); }
vec4 iris_mid_uv() { return vec4(iris_EntityUV, 0.0, 1.0); }
vec4 iris_mid_block() { return vec4(0.0); }
";

pub(super) const ENTITY_UNIFORMS: [(&str, &str); 5] = [
    ("entityColor", "vec4"),
    ("entityId", "int"),
    ("blockEntityId", "int"),
    ("currentRenderedItemId", "int"),
    ("renderStage", "int"),
];

pub(super) fn entity_varyings(fragment: &str, first: u32) -> (String, String, String) {
    let tokens = crate::shaderpack::transform::lexer::lex(fragment);
    let (mut inputs, mut outputs, mut copies) = (String::new(), String::new(), String::new());
    let named = ENTITY_UNIFORMS
        .iter()
        .filter(|(name, _)| tokens.iter().any(|t| t.is_word(name)));
    for (location, (name, ty)) in (first..).zip(named) {
        inputs.push_str(&format!(
            "layout(location = {location}) flat in {ty} {name};\n"
        ));
        outputs.push_str(&format!(
            "layout(location = {location}) flat out {ty} iris_Flat_{name};\n"
        ));
        copies.push_str(&format!("iris_Flat_{name} = {name};\n    "));
    }
    (inputs, outputs, copies)
}

const _: () = assert!(crate::renderer::packvertex::UNMAPPED == 65535);

pub(crate) const SCREEN_PRELUDE: &str = "vec2 iris_corner() {\n    \
    return vec2(float((gl_VertexIndex << 1) & 2), float(gl_VertexIndex & 2));\n\
}\n\
vec3 iris_position() { return vec3(iris_corner(), 0.0); }\n\
vec2 iris_uv0() { return iris_corner(); }\n\
vec2 iris_light() { return vec2(240.0); }\n\
vec4 iris_color() { return vec4(1.0); }\n\
vec3 iris_normal() { return vec3(0.0, 0.0, 1.0); }\n";

#[cfg(test)]
mod tests {
    use super::super::PACK_SET;
    use super::*;
    use crate::shaderpack::backend;
    use crate::shaderpack::transform::{self, Stage};

    #[test]
    fn entity_values_reach_the_fragment_stage() {
        let vsh = "#version 130\nuniform int entityId;\nvarying vec2 texcoord;\n\
                   void main() { gl_Position = ftransform(); texcoord = gl_MultiTexCoord0.xy + float(entityId); }\n";
        let fsh = "#version 130\nuniform vec4 entityColor;\nuniform int entityId;\nvarying vec2 texcoord;\n\
                   void main() { gl_FragData[0] = mix(vec4(texcoord, 0.0, 1.0), entityColor, entityColor.a) * float(entityId); }\n";
        let mut shared = transform::Shared::of(vsh, fsh);
        shared
            .uniforms
            .retain(|u| !ENTITY_UNIFORMS.iter().any(|(name, _)| u.name == *name));
        let (inputs, outputs, copies) = entity_varyings(fsh, shared.varying_end);
        assert!(
            inputs.contains("layout(location = 1) flat in vec4 entityColor;"),
            "{inputs}"
        );
        assert!(
            inputs.contains("layout(location = 2) flat in int entityId;"),
            "{inputs}"
        );
        // Both variants: plain meshes, and GPU-posed rigs' (skinned).
        for skinned in [false, true] {
            let prelude = format!("{}{outputs}", entity_prelude(skinned));
            let target = transform::Target {
                set: PACK_SET,
                vertex_prelude: &prelude,
                fragment_prelude: &inputs,
                vertex_end: &copies,
                outputs: Some(1),
                gl_clip: true,
                attribute_defaults: true,
                ..Default::default()
            };
            for (source, stage, naga_stage) in [
                (vsh, Stage::Vertex, backend::ShaderStage::Vertex),
                (fsh, Stage::Fragment, backend::ShaderStage::Fragment),
            ] {
                let out = transform::transform(source, stage, &shared, &target);
                let wgsl = backend::to_wgsl(&out.source, naga_stage, &[])
                    .unwrap_or_else(|e| panic!("skinned {skinned}: {e}\n\n{}", out.source));
                assert!(wgsl.contains("@location(2) @interpolate(flat)"), "{wgsl}");
                if skinned && stage == Stage::Vertex {
                    assert!(wgsl.contains("@group(1) @binding(0)"), "{wgsl}");
                    assert!(wgsl.contains("@location(16)"), "{wgsl}");
                }
            }
        }
    }

    #[test]
    fn every_prelude_compiles() {
        let vsh = "#version 130\nattribute vec4 mc_Entity;\nvarying vec2 texcoord;\nvarying vec2 lmcoord;\nvarying vec4 tint;\n\
                   void main() { gl_Position = ftransform(); texcoord = (gl_TextureMatrix[0] * gl_MultiTexCoord0).xy;\n\
                   lmcoord = (gl_TextureMatrix[1] * gl_MultiTexCoord1).xy; tint = gl_Color * mc_Entity.x;\n\
                   vec3 n = gl_NormalMatrix * gl_Normal; tint.rgb *= max(n.y, 0.5); }\n";
        let shared = transform::Shared::of(vsh, "");
        let slots = format!("@group({TERRAIN_SET}) @binding({SLOT_TABLE_BINDING})");
        for (prelude, geometry, binding) in [
            (terrain_prelude(false), true, slots.as_str()),
            (terrain_prelude(true), true, slots.as_str()),
            // The native renderer's slots buffer instead.
            (with_vulkan_terrain(|| terrain_prelude(false)), true, "@group(2) @binding(7)"),
            (SCREEN_PRELUDE.to_owned(), false, ""),
        ] {
            let target = transform::Target {
                set: PACK_SET,
                vertex_prelude: &prelude,
                gl_clip: true,
                attribute_defaults: geometry,
                ..Default::default()
            };
            let out = transform::transform(vsh, Stage::Vertex, &shared, &target);
            let wgsl = backend::to_wgsl(&out.source, backend::ShaderStage::Vertex, &[])
                .unwrap_or_else(|e| panic!("{e}\n\n{}", out.source));
            if geometry {
                assert!(wgsl.contains(binding), "{wgsl}");
            }
        }
    }
}
