// Torch Vibrant: lays the water's reflections and the volumetric fog over
// the scene (both traced at a fraction of the resolution, see
// lib/resolve.wgsl), then the distance haze that hides the edge of the
// world, and the murk under water or lava.

/* RENDERTARGETS: 0 */

#include "/lib/common.wgsl"
#include "/lib/resolve.wgsl"

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

@fragment
fn fragment(v: Varyings) -> @location(0) vec4<f32> {
    let px = vec2<i32>(v.clip.xy);
    var c = textureLoad(colortex0, px, 0).rgb;
    let depth = textureLoad(depthtex0, px, 0).r;
    // The hand is drawn with its own projection, so its depth says nothing
    // of distance; it is in front of every fog.
    if (is_hand(textureLoad(colortex2, px, 0).w)) {
        return vec4<f32>(c, 1.0);
    }
    let player = player_pos(view_pos(v.uv, min(depth, 0.99999)));
    let dist = length(player);
    let dir = normalize(player);
    let sky = depth >= 1.0;

    if (frame.isEyeInWater == 1) {
        let open = clamp(f32(frame.eyeBrightnessSmooth.y) / 240.0, 0.0, 1.0);
        let murk = vec3<f32>(0.02, 0.12, 0.16) * (sky_ambient() * open + vec3<f32>(0.02));
        let through = exp(-select(dist, 48.0, sky) * vec3<f32>(0.32, 0.09, 0.07));
        return vec4<f32>(c * through + murk * (1.0 - through), 1.0);
    }
    if (frame.isEyeInWater == 2) {
        let through = exp(-dist * 1.4);
        return vec4<f32>(mix(vec3<f32>(1.2, 0.32, 0.04), c, through), 1.0);
    }

    if (textureLoad(colortex3, px, 0).z >= 0.5) {
        let mirror = resolve(colortex5, v.uv, v.clip.xy, depth, true);
        c = mix(c, mirror.rgb, mirror.a);
    }
    let fog = resolve(colortex4, v.uv, v.clip.xy, depth, false);
    c = c * fog.a + fog.rgb;

    if (!sky) {
        let horizon = sky_color(dir);
        let haze = 1.0 - exp(-dist * 0.0008 * FOG_DENSITY);
        let edge = smoothstep(frame.far * 0.75, frame.far, dist);
        c = mix(c, horizon, max(haze * 0.45, edge));
    }
    return vec4<f32>(c, 1.0);
}
