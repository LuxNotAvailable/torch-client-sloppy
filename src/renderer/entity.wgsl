#import bevy_pbr::mesh_functions
#import bevy_pbr::mesh_view_bindings::{view, fog, globals}
#import bevy_pbr::view_transformations::position_world_to_clip

#ifdef FANCY_SHADERS
#import bevy_pbr::mesh_view_bindings::lights
#import bevy_pbr::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT
#import bevy_pbr::shadows
#endif

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
    let mixed = mix(srgb_encode(color.rgb), srgb_encode(fog.base_color.rgb), value * fog.base_color.a);
    return vec4(srgb_decode(mixed), color.a);
}

const MINECRAFT_LIGHT_POWER: f32 = 0.6;
const MINECRAFT_AMBIENT_LIGHT: f32 = 0.4;

const MODE_CARDINAL: u32 = 0u;
const MODE_FLAT: u32 = 1u;
const MODE_EMISSIVE: u32 = 2u;
const MODE_TEXT: u32 = 3u;

struct Params {
    tint: vec4<f32>,
    light: vec4<f32>,
    light0: vec4<f32>,
    light1: vec4<f32>,
    light_floor: vec4<f32>,
    light_block: vec4<f32>,
    light_sky: vec4<f32>,
    mode: u32,
};

#ifdef FANCY_SHADERS
const SUN_GAIN: f32 = 1.60;
const AMBIENT_GAIN: f32 = 0.31;
const BLOCK_GAIN: f32 = 2.20;
const AMBIENT_SHADE_MIX: f32 = 0.55;
const GROUND_TINT: vec3<f32> = vec3(0.85, 0.80, 0.68);
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var entity_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var entity_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> params: Params;
#ifdef RIGGED
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<storage, read> rig_parts: array<vec4<f32>>;
#endif
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var glint_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var glint_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
#ifdef RIGGED
    @location(4) part: u32,
#endif
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) accum: vec2<f32>,
#ifdef FANCY_SHADERS
    @location(4) world_normal: vec3<f32>,
#endif
};

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3(0.0031308));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(high, low, c <= vec3(0.04045));
}

fn mix_light_separate(light: vec2<f32>) -> f32 {
    let clamped = max(vec2(0.0), light);
    return min(
        1.0,
        (clamped.x + clamped.y) * MINECRAFT_LIGHT_POWER + MINECRAFT_AMBIENT_LIGHT,
    );
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    var local_position = vertex.position;
    var local_normal = vertex.normal;
#ifdef RIGGED
    // The part's matrix, stored as its top three rows; the normal goes through
    // the cofactor matrix, the inverse transpose up to the determinant.
    let row = (mesh_functions::get_tag(vertex.instance_index) + vertex.part) * 3u;
    let r0 = rig_parts[row];
    let r1 = rig_parts[row + 1u];
    let r2 = rig_parts[row + 2u];
    let p = vec4(vertex.position, 1.0);
    local_position = vec3(dot(r0, p), dot(r1, p), dot(r2, p));
    let c0 = vec3(r0.x, r1.x, r2.x);
    let c1 = vec3(r0.y, r1.y, r2.y);
    let c2 = vec3(r0.z, r1.z, r2.z);
    let n = mat3x3(cross(c1, c2), cross(c2, c0), cross(c0, c1)) * vertex.normal;
    let det = dot(c0, cross(c1, c2));
    local_normal = select(vertex.normal, normalize(n) * sign(det), dot(n, n) > 1e-12);
#endif
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position =
        mesh_functions::mesh_position_local_to_world(world_from_local, vec4(local_position, 1.0));
    out.clip_position = position_world_to_clip(out.world_position.xyz);
    out.uv = vertex.uv;

    let normal = mesh_functions::mesh_normal_local_to_world(local_normal, vertex.instance_index);
    out.color = vertex.color;
    if params.mode == MODE_CARDINAL {
        let light = vec2(dot(params.light0.xyz, normal), dot(params.light1.xyz, normal));
        out.accum = vec2(mix_light_separate(light), mix_light_separate(-light));
    } else if params.mode == MODE_TEXT {
        out.accum = vec2(1.0, 1.0);
        out.color = vertex.color;
    } else {
        out.accum = vec2(vertex.color.a, vertex.color.a);
        out.color = vec4(vertex.color.rgb, 1.0);
    }
#ifdef FANCY_SHADERS
    out.world_normal = normal;
#endif
    return out;
}

#ifdef FANCY_SHADERS
fn lighting(in: VertexOutput, is_front: bool, shade: f32) -> vec3<f32> {
    let n = normalize(select(-in.world_normal, in.world_normal, is_front));

    var sun_visibility = 0.0;
    var sun_color = vec3(0.0);
    if lights.n_directional_lights > 0u {
        let light = &lights.directional_lights[0];
        sun_color = (*light).color.rgb;
        let n_o_l = saturate(dot(n, (*light).direction_to_light));
        if n_o_l > 0.0 {
            var shadow = 1.0;
            if ((*light).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
                let view_z = dot(vec4(
                    view.view_from_world[0].z,
                    view.view_from_world[1].z,
                    view.view_from_world[2].z,
                    view.view_from_world[3].z,
                ), in.world_position);
                shadow = shadows::fetch_directional_shadow(
                    0u, in.world_position, n, view_z);
            }
            sun_visibility = n_o_l * shadow;
        }
    }

    let sky = params.light_sky.rgb;
    let hemisphere = mix(GROUND_TINT, lights.ambient_color.rgb, n.y * 0.5 + 0.5);
    let ambient = sky * AMBIENT_GAIN * hemisphere * mix(1.0, shade, AMBIENT_SHADE_MIX);
    let direct = sky * SUN_GAIN * sun_color * sun_visibility;
    return (params.light_floor.rgb + params.light_block.rgb * BLOCK_GAIN) * shade
        + ambient
        + direct;
}
#endif

// Vanilla's armor glint: the glint texture through
// translate(-f, g) * rotateZ(pi / 18) * scale(0.16), where f and g cycle every
// 110 s and 30 s of `millis * glintSpeed * 8` (glint speed 0.5), times the
// default glint strength of 0.75.
const GLINT_SCALE: f32 = 0.16;
const GLINT_STRENGTH: f32 = 0.75;
const GLINT_TICKS_PER_SEC: f32 = 4000.0;

fn glint(uv: vec2<f32>) -> vec3<f32> {
    let ticks = globals.time * GLINT_TICKS_PER_SEC;
    let f = fract(ticks / 110000.0);
    let g = fract(ticks / 30000.0);
    let s = sin(3.14159265 / 18.0);
    let c = cos(3.14159265 / 18.0);
    let p = uv * GLINT_SCALE;
    let st = vec2(c * p.x - s * p.y, s * p.x + c * p.y) + vec2(-f, g);
    return textureSampleLevel(glint_texture, glint_sampler, st, 0.0).rgb * GLINT_STRENGTH;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var color = textureSampleLevel(entity_texture, entity_sampler, in.uv, 0.0);
    if color.a < params.light.a {
        discard;
    }
    let shade = select(in.accum.y, in.accum.x, is_front);
    color = color * in.color * params.tint;
#ifndef FANCY_SHADERS
    color = vec4(color.rgb * shade, color.a);
#endif
    if params.light1.w < 1.0 {
        let overlay_color = mix(vec3(1.0, 0.0, 0.0), vec3(1.0), params.light0.w);
        let mixed_srgb = mix(overlay_color, srgb_encode(color.rgb), params.light1.w);
        color = vec4(srgb_decode(mixed_srgb), color.a);
    }
    if params.mode != MODE_EMISSIVE {
#ifdef FANCY_SHADERS
        color = vec4(color.rgb * lighting(in, is_front, shade), color.a);
#else
        color = vec4(color.rgb * params.light.rgb, color.a);
#endif
    }
    if params.light_floor.w > 0.0 {
        // Vanilla blends the glint with (SRC_COLOR, ONE) onto an sRGB
        // framebuffer: it adds its own color squared to the encoded value.
        let g = glint(in.uv);
        let encoded = srgb_encode(clamp(color.rgb, vec3(0.0), vec3(1.0))) + g * g;
        color = vec4(srgb_decode(min(encoded, vec3(1.0))), color.a);
    }
    if fog.mode != 0u {
        color = vanilla_fog(color, in.world_position.xyz - view.world_position);
    }
#ifdef TONEMAP_IN_SHADER
    color = tone_mapping(color, view.color_grading);
#endif
    return color;
}
