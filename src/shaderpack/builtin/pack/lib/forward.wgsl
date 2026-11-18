// Torch Vibrant: what is lit as it is drawn, not by the deferred pass:
// entities, block entities, particles, clouds and the hand. Include after
// common.wgsl and shadow.wgsl.

/* RENDERTARGETS: 0,2 */

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light: vec2<f32>,
    @location(3) player: vec3<f32>,
    @location(4) normal: vec3<f32>,
    @location(5) @interpolate(flat) overlay: vec4<f32>,
    @location(6) @interpolate(flat) stage: i32,
}

struct Lit {
    @location(0) color: vec4<f32>,
    @location(1) texel: vec4<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    let a = iris_attributes(input);
    let view = frame.iris_ModelViewMatrix * vec4<f32>(a.position, 1.0);
    let view_normal = (frame.iris_ModelViewMatrix * vec4<f32>(a.normal, 0.0)).xyz;
    var out: Varyings;
    out.clip = iris_clip(frame.iris_ProjectionMatrix * view);
    out.uv = a.uv;
    out.color = a.color;
    out.light = clamp(a.light / 240.0, vec2<f32>(0.0), vec2<f32>(1.0));
    out.player = (frame.gbufferModelViewInverse * view).xyz;
    out.normal = (frame.gbufferModelViewInverse * vec4<f32>(view_normal, 0.0)).xyz;
    out.overlay = a.entity_color;
    out.stage = a.render_stage;
    return out;
}

@fragment
fn fragment(v: Varyings) -> Lit {
    var c = textureSample(gtexture, gtexture_sampler, v.uv) * v.color;
    let dpx = dpdx(v.player);
    let dpy = dpdy(v.player);
    let dux = dpdx(v.uv);
    let duy = dpdy(v.uv);
    if (!iris_alpha_pass(c.a)) {
        discard;
    }
    c = vec4<f32>(mix(c.rgb, v.overlay.rgb, v.overlay.a), c.a);
    let albedo = to_linear(c.rgb);
    let n = select(vec3<f32>(0.0, 1.0, 0.0), normalize(v.normal), dot(v.normal, v.normal) > 1e-6);
    var lit: vec3<f32>;
    if (v.stage == MC_RENDER_STAGE_CLOUDS) {
        let above = sky_color(vec3<f32>(0.0, 1.0, 0.0));
        let around = sky_color(normalize(vec3<f32>(v.player.x, 0.05, v.player.z)));
        lit = albedo * (direct_light() * 0.55 + above * 0.5 + around * 0.6 + vec3<f32>(0.01));
    } else {
        let vis = shadow_visibility(texel_point(v, dpx, dpy, dux, duy), n, 2u);
        lit = surface_light(albedo, n, v.light, vis, false);
    }
    var out: Lit;
    out.color = vec4<f32>(lit, c.a);
    let hand = v.stage == MC_RENDER_STAGE_HAND_SOLID || v.stage == MC_RENDER_STAGE_HAND_TRANSLUCENT;
    out.texel = select(TEXEL_LIT, TEXEL_HAND, hand);
    return out;
}

/// Where the middle of the texture pixel drawn here lies on the surface, so
/// shadows step texel by texel as on terrain. Found by stepping across the
/// screen (exact on a flat face, however many pixels the texel spans);
/// left where the step is unsure, at silhouettes or a degenerate mapping.
fn texel_point(v: Varyings, dpx: vec3<f32>, dpy: vec3<f32>, dux: vec2<f32>, duy: vec2<f32>) -> vec3<f32> {
    let atlas = vec2<f32>(textureDimensions(gtexture));
    let want = (floor(v.uv * atlas) + 0.5) / atlas - v.uv;
    let det = dux.x * duy.y - dux.y * duy.x;
    if (abs(det) < 1e-14) {
        return v.player;
    }
    let a = (want.x * duy.y - want.y * duy.x) / det;
    let b = (dux.x * want.y - dux.y * want.x) / det;
    let step = a * dpx + b * dpy;
    // A texel is at most a block or so across on any model.
    if (dot(step, step) > 1.0) {
        return v.player;
    }
    return v.player + step;
}
