#import bevy_pbr::mesh_view_bindings::{view, fog}
#import bevy_pbr::view_transformations::position_world_to_clip


#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

fn vanilla_fog_ramp(distance: f32, start: f32, end: f32) -> f32 {
    if distance <= start {
        return 0.0;
    } else if distance >= end {
        return 1.0;
    }
    return (distance - start) / (end - start);
}

fn vanilla_fog(color: vec4<f32>, rel: vec3<f32>) -> vec4<f32> {
    let spherical = length(rel);
    let cylindrical = max(length(rel.xz), abs(rel.y));
    let environmental = fog.directional_light_color.xy;
    let value = max(
        vanilla_fog_ramp(spherical, environmental.x, environmental.y),
        vanilla_fog_ramp(cylindrical, fog.be.x, fog.be.y),
    );
    let mixed = mix(encode_srgb(color.rgb), encode_srgb(fog.base_color.rgb), value * fog.base_color.a);
    return vec4(decode_srgb(mixed), color.a);
}

fn encode_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3(0.0)), vec3(1.0 / 2.2));
}

fn decode_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3(2.2));
}

@group(2) @binding(0) var atlas_texture: texture_2d<f32>;
@group(2) @binding(1) var atlas_sampler: sampler;
@group(2) @binding(2) var lightmap_texture: texture_2d<f32>;
@group(2) @binding(3) var lightmap_sampler: sampler;
@group(2) @binding(4) var<uniform> params: vec4<f32>;

struct SlotMeta { min: vec3<f32>, first_index: u32, max: vec3<f32>, index_count: u32, origin: vec3<i32>, base_vertex: u32, solid_count: u32, flags: u32, pool: u32, cutout_first: u32 }
#ifdef META_TEXTURE
const ORIGIN_ROW: i32 = 256;
@group(2) @binding(5) var origins: texture_2d<i32>;
#else
@group(2) @binding(5) var<storage, read> metas: array<SlotMeta>;
#endif
@group(2) @binding(6) var anim_table: texture_2d<u32>;

const ATLAS_COLS: u32 = 16u;
const ANIM_ROW: u32 = 256u;

fn tile_cell(tile: u32) -> vec2<f32> {
    return vec2(f32(tile % ATLAS_COLS), f32(tile / ATLAS_COLS));
}

// Samples the atlas through the animation table: the tile a mesh names is
// swapped for whichever frame is showing, and blended into the next one for
// interpolated textures. Unanimated tiles map to themselves.
fn sample_atlas(uv: vec2<f32>, bias: f32) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(atlas_texture, 0));
    let tiles = vec2(f32(ATLAS_COLS), size.y * f32(ATLAS_COLS) / size.x);
    let grid = uv * tiles;
    let cell = clamp(floor(grid), vec2(0.0), tiles - 1.0);
    let local = grid - cell;
    let scale = exp2(bias);
    let ddx = dpdx(uv) * scale;
    let ddy = dpdy(uv) * scale;

    let tile = u32(cell.y) * ATLAS_COLS + u32(cell.x);
    let rows = textureDimensions(anim_table, 0).y;
    var entry = vec4(tile, tile, 0u, 0u);
    if tile / ANIM_ROW < rows {
        entry = textureLoad(anim_table, vec2<i32>(i32(tile % ANIM_ROW), i32(tile / ANIM_ROW)), 0);
    }
    let a = textureSampleGrad(atlas_texture, atlas_sampler, (tile_cell(entry.x) + local) / tiles, ddx, ddy);
    if entry.z == 0u {
        return a;
    }
    let b = textureSampleGrad(atlas_texture, atlas_sampler, (tile_cell(entry.y) + local) / tiles, ddx, ddy);
    return mix(a, b, f32(entry.z) / 65535.0);
}

fn section_origin(instance_index: u32, slot_lane: i32) -> vec3<f32> {
#ifdef META_TEXTURE
    let slot = slot_lane & 0xFFFF;
    let texel = textureLoad(origins, vec2<i32>(slot % ORIGIN_ROW, slot / ORIGIN_ROW), 0);
    return vec3<f32>(texel.xyz);
#else
    return vec3<f32>(metas[instance_index].origin);
#endif
}

const MATERIAL_LEAF: f32 = 1.0;
const MATERIAL_PLANT: f32 = 2.0;
const MATERIAL_WATER: f32 = 3.0;
const MATERIAL_LAVA: f32 = 4.0;
const MATERIAL_EMISSIVE_BASE: f32 = 5.0;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec4<i32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: vec4<f32>,
    @location(3) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

fn sample_lightmap(uv: vec2<f32>) -> vec4<f32> {
    let coord = clamp(uv / 256.0 + 0.5 / 16.0, vec2(0.5 / 16.0), vec2(15.5 / 16.0));
    return textureSampleLevel(lightmap_texture, lightmap_sampler, coord, 0.0);
}


@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.world_position = vec4(
        section_origin(vertex.instance_index, vertex.position.w)
            + vec3<f32>(vertex.position.xyz) / 256.0, 1.0);


    out.clip_position = position_world_to_clip(out.world_position.xyz);
    out.uv = vertex.uv;

    let shaded = vec4(vertex.color.rgb * vertex.color.a, 1.0);
    out.color = shaded * sample_lightmap(vertex.light.xy * 255.0);

    return out;
}


@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = sample_atlas(in.uv, params.y);
    var color = texel * in.color;
#ifdef ALPHA_CUTOUT
    if color.a < params.x {
        discard;
    }
#endif
#ifndef WATER
    color.a = 1.0;
#endif

    let world_position = in.world_position.xyz;
    let view_distance = length(world_position - view.world_position);


    if fog.mode != 0u {
        color = vanilla_fog(color, world_position - view.world_position);
    }
#ifdef TONEMAP_IN_SHADER
    color = tone_mapping(color, view.color_grading);
#endif
    return color;
}
