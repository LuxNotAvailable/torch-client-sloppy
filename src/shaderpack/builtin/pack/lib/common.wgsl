// Torch Vibrant: definitions every program shares. Nothing here samples a
// texture, so any program can include it without binding one it draws to.

#define SUN_BRIGHTNESS 2.4 // [1.6 2.0 2.4 2.8 3.2]
#define TORCH_BRIGHTNESS 2.2 // [1.4 1.8 2.2 2.6 3.0]
#define FOG_DENSITY 1.0 // [0.0 0.5 1.0 1.5 2.0]
#define EXPOSURE 1.0 // [0.6 0.8 1.0 1.2 1.4]
// The fraction of the screen reflections and light shafts are traced at
// (1/1 to 1/4; shaders.properties sizes their targets), and how that is
// brought back to full resolution: 0 upsample, 1 dither.
#define REFLECTION_RES 2 // [1 2 3 4]
#define VOLUMETRIC_RES 2 // [1 2 3 4]
#define SCALED_RESOLVE 0 // [0 1]

const PI: f32 = 3.14159265;

// Block ids from block.properties.
const MATERIAL_WATER: f32 = 1.0;
const MATERIAL_FOLIAGE: f32 = 2.0;
const MATERIAL_EMISSIVE: f32 = 3.0;

fn material_of(id: f32) -> f32 {
    if (id > 10000.5 && id < 10003.5) {
        return id - 10000.0;
    }
    return 0.0;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn oct_encode(n: vec3<f32>) -> vec2<f32> {
    let p = n.xy / (abs(n.x) + abs(n.y) + abs(n.z));
    if (n.z >= 0.0) {
        return p;
    }
    return (1.0 - abs(p.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
}

fn oct_decode(e: vec2<f32>) -> vec3<f32> {
    var n = vec3<f32>(e, 1.0 - abs(e.x) - abs(e.y));
    let t = max(-n.z, 0.0);
    n.x += select(t, -t, n.x >= 0.0);
    n.y += select(t, -t, n.y >= 0.0);
    return normalize(n);
}

/// 0..5: +x, -x, +y, -y, +z, -z, by the normal's largest axis.
fn face_of(n: vec3<f32>) -> f32 {
    let a = abs(n);
    if (a.x >= a.y && a.x >= a.z) {
        return select(0.0, 1.0, n.x < 0.0);
    }
    if (a.y >= a.z) {
        return select(2.0, 3.0, n.y < 0.0);
    }
    return select(4.0, 5.0, n.z < 0.0);
}

fn face_normal(face: u32) -> vec3<f32> {
    let sign = select(1.0, -1.0, (face & 1u) == 1u);
    let axis = face >> 1u;
    return vec3<f32>(
        select(0.0, sign, axis == 0u),
        select(0.0, sign, axis == 1u),
        select(0.0, sign, axis == 2u),
    );
}

/// A terrain pixel's surface, as the last channel of colortex2 carries it:
/// face (3 bits), material (2), block and sky light (8 each), an integer an
/// f32 holds exactly. The normal is the face's.
struct Surface {
    face: u32,
    material: f32,
    light: vec2<f32>,
}

fn pack_surface(face: f32, material: f32, light: vec2<f32>) -> f32 {
    let l = vec2<u32>(round(clamp(light, vec2<f32>(0.0), vec2<f32>(1.0)) * 255.0));
    return f32(u32(face) | (u32(material) << 3u) | (l.x << 5u) | (l.y << 13u));
}

fn unpack_surface(word: f32) -> Surface {
    let w = u32(word);
    var s: Surface;
    s.face = w & 7u;
    s.material = f32((w >> 3u) & 3u);
    s.light = vec2<f32>(f32((w >> 5u) & 255u), f32((w >> 13u) & 255u)) / 255.0;
    return s;
}

/// The middle of a block-face texel (1/16 of a block), relative to the block
/// the camera is in, given the cell inside the surface it covers.
fn texel_centre(cell: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    return (cell + 0.5) / 16.0 + n * (0.5 / 16.0);
}

fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

fn view_pos(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let v = frame.gbufferProjectionInverse * vec4<f32>(uv * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
    return v.xyz / v.w;
}

fn player_pos(view: vec3<f32>) -> vec3<f32> {
    return (frame.gbufferModelViewInverse * vec4<f32>(view, 1.0)).xyz;
}

fn player_dir(view_dir: vec3<f32>) -> vec3<f32> {
    return normalize((frame.gbufferModelViewInverse * vec4<f32>(view_dir, 0.0)).xyz);
}

fn sun_dir() -> vec3<f32> {
    return player_dir(frame.sunPosition);
}

/// The sun by day, the moon by night: what the shadow map is drawn from.
fn light_dir() -> vec3<f32> {
    return player_dir(frame.shadowLightPosition);
}

fn has_sky() -> bool {
    return frame.hasSkylight != 0;
}

/// 0 at night, 1 by day, easing through sunrise and sunset.
fn daylight() -> f32 {
    return smoothstep(-0.12, 0.18, sun_dir().y);
}

/// The colour of the sun or moon's direct light.
fn direct_light() -> vec3<f32> {
    let s = sun_dir().y;
    let warm = vec3<f32>(1.0, 0.52, 0.26);
    let noon = vec3<f32>(1.0, 0.95, 0.88);
    let height = smoothstep(0.0, 0.5, s);
    let sun = mix(warm, noon, height) * SUN_BRIGHTNESS * mix(0.3, 1.0, height) * smoothstep(-0.02, 0.1, s);
    let moon = vec3<f32>(0.6, 0.68, 0.85) * 0.12 * smoothstep(-0.02, 0.1, -s);
    return (sun + moon) * (1.0 - frame.rainStrength * 0.85);
}

/// The colour of the open sky's light on a surface facing up.
fn sky_ambient() -> vec3<f32> {
    let day = daylight();
    let night = vec3<f32>(0.016, 0.02, 0.032);
    let noon = vec3<f32>(0.34, 0.46, 0.66);
    let dusk = vec3<f32>(0.42, 0.34, 0.36);
    let s = sun_dir().y;
    let lit = mix(dusk, noon, smoothstep(0.05, 0.4, s));
    let c = mix(night, lit, day);
    return mix(c, vec3<f32>(luma(c)) * 0.75, frame.rainStrength * 0.7);
}

/// colortex2's last channel: >= 0 terrain for the deferred pass, -1 lit as
/// drawn, -2 the hand (which no fog or reflection touches).
const TEXEL_LIT: f32 = -1.0;
const TEXEL_HAND: f32 = -2.0;

fn is_hand(texel_w: f32) -> bool {
    return texel_w < -1.5;
}

const TORCH: vec3<f32> = vec3<f32>(1.0, 0.6, 0.3);

/// The sky seen in direction `dir` (player space), without the sun's disc.
fn sky_color(dir: vec3<f32>) -> vec3<f32> {
    if (!has_sky()) {
        return to_linear(frame.fogColor);
    }
    let sun = sun_dir();
    let s = sun.y;
    let day = daylight();
    let up = clamp(dir.y, 0.0, 1.0);
    let horizon = pow(1.0 - up, 4.0);

    let zenith = mix(vec3<f32>(0.004, 0.007, 0.018), vec3<f32>(0.12, 0.34, 0.92), day);
    let rim = mix(vec3<f32>(0.012, 0.018, 0.04), vec3<f32>(0.58, 0.76, 1.0), day);
    var c = mix(zenith, rim, horizon);

    let mu = max(dot(dir, sun), 0.0);
    let low_sun = 1.0 - smoothstep(0.0, 0.32, abs(s));
    let toward = 0.25 + 0.75 * pow(mu, 3.0);
    c = mix(c, vec3<f32>(1.0, 0.42, 0.16) * 1.3, low_sun * horizon * toward * smoothstep(-0.2, 0.0, s));
    c += vec3<f32>(1.0, 0.72, 0.42) * (pow(mu, 12.0) * 0.3 + pow(mu, 300.0) * 1.2) * smoothstep(-0.08, 0.05, s);

    // Below the horizon: the haze darkens toward the ground.
    c *= mix(1.0, 0.45, smoothstep(0.0, -0.4, dir.y));
    c = mix(c, vec3<f32>(luma(c)) * 0.7, frame.rainStrength * 0.8);
    return c * 1.35;
}

/// Light on a surface. `light` is the block and sky light, 0..1; `vis` the
/// share of the direct light reaching it; `foliage` lets light through.
fn surface_light(albedo: vec3<f32>, n: vec3<f32>, light: vec2<f32>, vis: f32, foliage: bool) -> vec3<f32> {
    let l = light_dir();
    var ndl = max(dot(n, l), 0.0);
    if (foliage) {
        ndl = mix(ndl, 0.65, 0.6);
    }
    let sky = light.y;
    // Shadow maps reach so far; past that, sky light decides.
    let open = smoothstep(0.05, 0.45, sky);
    let direct = direct_light() * ndl * vis * open;
    let ambient = sky_ambient() * sky * sky * (0.7 + 0.3 * n.y);
    let torch = TORCH * pow(light.x, 2.6) * TORCH_BRIGHTNESS;
    let floor_light = vec3<f32>(0.01);
    return albedo * (direct + ambient + torch + floor_light);
}

// The texel cache (bufferObject.0): one entry per block-face texel near the
// camera, shaded once a frame and read by every pixel it covers.
const CACHE_SIZE: u32 = 1048576u;
const CACHE_MASK: u32 = 1048575u;
const CACHE_PROBES: u32 = 12u;
const CACHE_EMPTY: u32 = 0xFFFFFFFFu;

/// The cache key of a texel cell (relative to the camera's block, in texels)
/// and face, or `CACHE_EMPTY` out of the cache's reach (32 blocks across,
/// 16 up and down).
fn cache_key(cell: vec3<f32>, face: f32) -> u32 {
    let c = vec3<i32>(cell) + vec3<i32>(512, 256, 512);
    if (any(c < vec3<i32>(0)) || c.x > 1023 || c.y > 511 || c.z > 1023) {
        return CACHE_EMPTY;
    }
    return u32(c.x) | (u32(c.z) << 10u) | (u32(c.y) << 20u) | (u32(face) << 29u);
}

fn cache_cell(key: u32) -> vec3<f32> {
    let x = i32(key & 1023u) - 512;
    let z = i32((key >> 10u) & 1023u) - 512;
    let y = i32((key >> 20u) & 511u) - 256;
    return vec3<f32>(f32(x), f32(y), f32(z));
}

fn cache_face(key: u32) -> u32 {
    return key >> 29u;
}

fn cache_hash(key: u32) -> u32 {
    var h = key * 0x9E3779B1u;
    h ^= h >> 15u;
    h *= 0x85EBCA77u;
    h ^= h >> 13u;
    return h & CACHE_MASK;
}
