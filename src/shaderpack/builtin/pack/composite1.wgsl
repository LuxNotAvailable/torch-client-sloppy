// Torch Vibrant: volumetric fog and light shafts, at half resolution. Marches
// from the eye to the surface through a haze that thins with height, lit by
// the sun wherever the shadow map sees it.

/* RENDERTARGETS: 4 */

#include "/lib/common.wgsl"
#include "/lib/shadow.wgsl"

#define GOD_RAY_STEPS 12 // [6 8 12 16 24]

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex(input: IrisVertexIn) -> Varyings {
    var out: Varyings;
    out.clip = iris_screen(input);
    out.uv = iris_attributes(input).uv;
    return out;
}

/// The haze at sea level; it thins above it (`density`).
fn haze() -> f32 {
    let morning = 1.0 - smoothstep(0.0, 0.35, abs(sun_dir().y));
    return (0.0012 + 0.0014 * morning + 0.003 * frame.rainStrength) * FOG_DENSITY;
}

fn density(base: f32, y: f32) -> f32 {
    return base * exp(-max(y - 63.0, 0.0) / 36.0);
}

/// No step is longer than this, so short rays take fewer steps for the same
/// detail.
const MAX_STEP: f32 = 8.0;

/// How strongly the haze scatters light toward the eye, 0.3 away from the
/// light up to 1.5 looking into it.
fn phase(cos_angle: f32) -> f32 {
    let toward = max(cos_angle, 0.0);
    return 0.3 + 0.7 * pow(toward, 4.0) + 0.5 * pow(toward, 32.0);
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    if (!has_sky() || frame.isEyeInWater != 0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let size = vec2<f32>(textureDimensions(depthtex0));
    let depth = textureLoad(depthtex0, vec2<i32>(v.uv * size), 0).r;
    let target_pos = player_pos(view_pos(v.uv, min(depth, 0.99999)));
    let reach = min(length(target_pos), select(frame.far, 160.0, depth >= 1.0));
    let dir = normalize(target_pos);

    let steps = clamp(i32(ceil(reach / MAX_STEP)), 4, GOD_RAY_STEPS);
    let seg = reach / f32(steps);
    let dither = ign(v.clip.xy);
    // The shadow map is an orthographic projection, so the ray is a straight
    // line in its clip space too: transform the ends once, not every step.
    let to_shadow = frame.shadowProjection * frame.shadowModelView;
    let start = (to_shadow * vec4<f32>(0.0, 0.0, 0.0, 1.0)).xyz;
    let along = (to_shadow * vec4<f32>(dir * seg, 0.0)).xyz;
    let base = haze();
    let sun = direct_light() * phase(dot(dir, light_dir())) * mix(0.15, 1.0, daylight());
    // Light in caves is not the sky's: fade the haze's sky light by what the
    // eye sees of the sky.
    let open = clamp(f32(frame.eyeBrightnessSmooth.y) / 240.0, 0.0, 1.0);
    let ambient = sky_ambient() * 0.6 * open;

    var scatter = vec3<f32>(0.0);
    var through = 1.0;
    for (var i = 0; i < steps; i++) {
        let k = f32(i) + dither;
        let d = density(base, frame.cameraPosition.y + dir.y * seg * k);
        let vis = shadow_at(start + along * k);
        let step_through = exp(-d * seg);
        scatter += through * (1.0 - step_through) * (sun * vis * open + ambient);
        through *= step_through;
    }
    return vec4<f32>(scatter, through);
}
