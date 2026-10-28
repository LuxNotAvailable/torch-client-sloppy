// Torch Vibrant: reflections on water, traced at the Reflection Resolution
// into colortex5 (the mirrored colour, and how much of it shows) for
// composite2 to resolve. A screen-space ray finds what the surface mirrors;
// where it leaves the screen the sky stands in, dimmed where the sky does not
// reach.

/* RENDERTARGETS: 5 */

#include "/lib/common.wgsl"

#define REFLECTION_STEPS 24 // [12 16 24 32 48]

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

fn screen_of(view: vec3<f32>) -> vec3<f32> {
    let clip = frame.gbufferProjection * vec4<f32>(view, 1.0);
    let ndc = clip.xyz / clip.w;
    return vec3<f32>(ndc.xy * 0.5 + 0.5, ndc.z * 0.5 + 0.5);
}

/// The screen position the ray from `start` along `dir` (view space) hits,
/// and how far to trust it, 0 (use the sky) to 1. Trust falls off smoothly
/// rather than switching, so neighbouring pixels whose rays land either side
/// of a threshold do not flicker between the scene and the sky: with how far
/// the hit lies from the ray (a thin edge it grazed), how much of the march
/// it took, how near the screen's edge it is, and how directly the ray
/// heads back at the eye, where the screen holds nothing it could see.
fn trace(start: vec3<f32>, dir: vec3<f32>, size: vec2<f32>, dither: f32) -> vec3<f32> {
    let toward_eye = 1.0 - smoothstep(0.1, 0.7, dir.z);
    if (toward_eye <= 0.0) {
        return vec3<f32>(0.0);
    }
    var step = dir * (0.15 + 0.03 * length(start));
    var p = start + step * dither;
    for (var i = 0; i < REFLECTION_STEPS; i++) {
        p += step;
        let s = screen_of(p);
        if (any(s.xy < vec2<f32>(0.0)) || any(s.xy > vec2<f32>(1.0)) || s.z >= 1.0) {
            return vec3<f32>(0.0);
        }
        let scene = textureLoad(depthtex0, vec2<i32>(s.xy * size), 0).r;
        if (s.z > scene) {
            let gap = length(view_pos(s.xy, scene)) - length(p);
            let thickness = length(step) * 1.5 + 0.2;
            if (abs(gap) < thickness) {
                // Halve back toward the surface for a sharper contact.
                var lo = p - step;
                var hi = p;
                for (var j = 0; j < 4; j++) {
                    let mid = (lo + hi) * 0.5;
                    let m = screen_of(mid);
                    let d = textureLoad(depthtex0, vec2<i32>(m.xy * size), 0).r;
                    if (m.z > d) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                let hit = screen_of(hi).xy;
                if (is_hand(textureLoad(colortex2, vec2<i32>(hit * size), 0).w)) {
                    return vec3<f32>(0.0);
                }
                let contact = 1.0 - smoothstep(0.4, 1.0, abs(gap) / thickness);
                let march = 1.0 - smoothstep(0.55, 1.0, f32(i) / f32(REFLECTION_STEPS));
                let edge = min(min(hit.x, 1.0 - hit.x), min(hit.y, 1.0 - hit.y));
                let border = smoothstep(0.0, 0.1, edge);
                return vec3<f32>(hit, contact * march * border * toward_eye);
            }
        }
        step *= 1.12;
    }
    return vec3<f32>(0.0);
}

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    // This pass may run at a fraction of the screen; read the full-size
    // targets at the pixel this one stands for.
    let size = vec2<f32>(textureDimensions(colortex0));
    let px = vec2<i32>(v.uv * size);
    let surface = textureLoad(colortex3, px, 0);
    if (surface.z < 0.5 || frame.isEyeInWater == 1 || is_hand(textureLoad(colortex2, px, 0).w)) {
        return vec4<f32>(0.0);
    }
    let depth = textureLoad(depthtex0, px, 0).r;
    let view = view_pos(v.uv, depth);
    let n_player = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), oct_decode(surface.xy), 0.55));
    let n = normalize((frame.gbufferModelView * vec4<f32>(n_player, 0.0)).xyz);
    let eye = normalize(view);
    let r = reflect(eye, n);
    let facing = max(dot(-eye, n), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - facing, 5.0);

    let sky = sky_color(player_dir(r)) * smoothstep(0.15, 0.7, surface.w);
    var mirrored = sky;
    let hit = trace(view, r, size, ign(v.clip.xy));
    if (hit.z > 0.0) {
        let seen = textureSampleLevel(colortex0, colortex0_sampler, hit.xy, 0.0).rgb;
        mirrored = mix(sky, seen, hit.z);
    }
    return vec4<f32>(mirrored, fresnel * 0.7);
}
