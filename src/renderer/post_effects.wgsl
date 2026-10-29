// One pass for every vanilla server post effect. Sampling effects (spider,
// blur, the creeper mosaic) pick where to read; colour effects then grade.

struct Input {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct PostEffects {
    flags: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var screen: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> effects: PostEffects;

const CREEPER: u32 = 1u;
const SPIDER: u32 = 2u;
const INVERT: u32 = 4u;
const BLUR: u32 = 8u;

const GOLDEN_ANGLE: f32 = 2.39996323;
const TAPS: i32 = 24;

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(screen, screen_sampler, clamp(uv, vec2(0.0), vec2(1.0)), 0.0).rgb;
}

// Disc blur on a golden-angle spiral: one pass instead of vanilla's
// separable box pair.
fn blurred(uv: vec2<f32>, radius_px: f32) -> vec3<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(screen));
    var sum = vec3(0.0);
    for (var i = 0; i < TAPS; i++) {
        let r = sqrt((f32(i) + 0.5) / f32(TAPS)) * radius_px;
        let a = f32(i) * GOLDEN_ANGLE;
        sum += tap(uv + vec2(cos(a), sin(a)) * r * texel);
    }
    return sum / f32(TAPS);
}

// A spider's compound view: a few magnified eyes over a blurred field,
// each fading out at its rim, the whole thing tinted red.
fn spider(uv: vec2<f32>) -> vec3<f32> {
    var color = blurred(uv, 16.0);
    let aspect = f32(textureDimensions(screen).x) / f32(textureDimensions(screen).y);
    let eyes = array<vec3<f32>, 5>(
        vec3(0.5, 0.45, 0.30),
        vec3(0.22, 0.32, 0.17),
        vec3(0.78, 0.32, 0.17),
        vec3(0.33, 0.75, 0.14),
        vec3(0.67, 0.75, 0.14),
    );
    for (var i = 0; i < 5; i++) {
        let eye = eyes[i];
        let d = (uv - eye.xy) * vec2(aspect, 1.0);
        let edge = length(d) / eye.z;
        if edge < 1.0 {
            let view = tap(vec2(0.5) + (uv - eye.xy) / (eye.z * 2.0));
            color = mix(view, color, smoothstep(0.7, 1.0, edge));
        }
    }
    return color * vec3(1.0, 0.8, 0.8);
}

@fragment
fn fragment(in: Input) -> @location(0) vec4<f32> {
    let flags = effects.flags;
    var uv = in.uv;
    if (flags & CREEPER) != 0u {
        let cells = vec2<f32>(textureDimensions(screen)) / 4.0;
        uv = (floor(uv * cells) + 0.5) / cells;
    }

    var color: vec3<f32>;
    if (flags & SPIDER) != 0u {
        color = spider(uv);
    } else if (flags & BLUR) != 0u {
        color = blurred(uv, 8.0);
    } else {
        color = tap(uv);
    }

    if (flags & CREEPER) != 0u {
        // Green-only luma, posterised to 16 levels.
        let luma = dot(color, vec3(0.3, 0.59, 0.11));
        color = vec3(0.0, floor(luma * 16.0) / 16.0, 0.0);
    }
    if (flags & INVERT) != 0u {
        color = mix(color, vec3(1.0) - color, 0.8);
    }
    return vec4(color, 1.0);
}
