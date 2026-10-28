#import bevy_pbr::mesh_functions
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::view_transformations::position_world_to_clip

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::tone_mapping
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> tint: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var sky_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var sky_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifdef SKY_UVS
    @location(1) uv: vec2<f32>,
#endif
#ifdef SKY_COLORS
    @location(2) color: vec4<f32>,
#endif
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world =
        mesh_functions::mesh_position_local_to_world(world_from_local, vec4(vertex.position, 1.0));
    out.clip_position = position_world_to_clip(world.xyz);
    // Vanilla draws the sky before the world, so nothing in the world is ever
    // behind it. Pin it to the far plane (0 under reversed Z): any terrain,
    // water or entity covers it, wherever its geometry happens to sit.
    out.clip_position.z = 0.0;
#ifdef SKY_UVS
    out.uv = vertex.uv;
#else
    out.uv = vec2(0.0);
#endif
#ifdef SKY_COLORS
    out.color = vertex.color;
#else
    out.color = vec4(1.0);
#endif
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = tint * in.color;
#ifdef SKY_UVS
    color = color * textureSample(sky_texture, sky_sampler, in.uv);
#endif
#ifdef TONEMAP_IN_SHADER
    color = tone_mapping(color, view.color_grading);
#endif
#ifdef BLEND_PREMULTIPLIED_ALPHA
    // Sky parts only use this pipeline for `AlphaMode::Add`.
    color = vec4(color.rgb * color.a, 0.0);
#endif
    return color;
}
