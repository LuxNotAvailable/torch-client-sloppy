// Torch Vibrant: definitions every program shares. Nothing here samples a
// texture, so any program can include it without binding one it draws to.
//
// Options are declared twice: as `#define`s, which the options screen reads
// (and nothing compiles), and as specialization constants, which the engine
// sets to the chosen values when it loads the pack.

#ifdef TORCH_SLANG
[SpecializationConstant] const float SUN_BRIGHTNESS = 2.4;
[SpecializationConstant] const float TORCH_BRIGHTNESS = 2.2;
[SpecializationConstant] const float FOG_DENSITY = 1.0;
[SpecializationConstant] const float WATER_CLARITY = 1.0;
[SpecializationConstant] const float WATER_SURFACE_OPACITY = 0.2;
[SpecializationConstant] const int WATER_CAUSTICS_ENABLED = 1;
[SpecializationConstant] const float WATER_CAUSTICS = 0.1;
[SpecializationConstant] const float WATER_CAUSTICS_DEPTH = 20.0;
[SpecializationConstant] const float WATER_CAUSTICS_SHADE = 0.5;
[SpecializationConstant] const int WATER_STYLE = 0;
[SpecializationConstant] const float EXPOSURE = 1.0;
[SpecializationConstant] const int SCALED_RESOLVE = 0;
[SpecializationConstant] const int TEXEL_FULL_DISTANCE = 24;
[SpecializationConstant] const int TEXEL_RING_WIDTH = 16;
[SpecializationConstant] const int TEXEL_MIN_RES = 2;
[SpecializationConstant] const int FOLIAGE_SHADING = 0;
[SpecializationConstant] const int CLOUD_STYLE = 0;
[SpecializationConstant] const float CLOUD_BLEND_DISTANCE = 384.0;
[SpecializationConstant] const float CLOUD_SCATTER = 1.0;
#else
#define SUN_BRIGHTNESS 2.4 // [1.6 2.0 2.4 2.8 3.2]
#define TORCH_BRIGHTNESS 2.2 // [1.4 1.8 2.2 2.6 3.0]
#define FOG_DENSITY 1.0 // [0.0 0.5 1.0 1.5 2.0]
// How far you see into water: lower is murkier, higher clearer.
#define WATER_CLARITY 1.0 // [0.25 0.5 0.75 1.0 1.25 1.5 2.0]
// How much of the water's surface is its own colour however shallow it is,
// so a pool a block deep still reads as water.
#define WATER_SURFACE_OPACITY 0.2 // [0.0 0.1 0.2 0.3 0.4 0.5 0.6 0.7 0.8]
// Light the surface focuses on the floor: 0 off, 1 on.
#define WATER_CAUSTICS_ENABLED 1 // [0 1]
// Strength of that light.
#define WATER_CAUSTICS 0.1 // [0.0 0.02 0.05 0.08 0.1 0.15 0.2 0.25 0.3 0.4 0.5]
// Depth in blocks by which caustics have faded out.
#define WATER_CAUSTICS_DEPTH 20.0 // [2.0 4.0 6.0 8.0 12.0 16.0 20.0 28.0 40.0]
// How much shadow on the surface dims caustics: 0 not at all, 1 fully.
#define WATER_CAUSTICS_SHADE 0.5 // [0.0 0.25 0.5 0.75 1.0]
// 0 Waves, rippled normals; 1 Flat, a still surface glinting on the
// texture's bright texels (cheaper).
#define WATER_STYLE 0 // [0 1]
#define EXPOSURE 1.0 // [0.6 0.8 1.0 1.2 1.4]
// The fraction of the screen reflections and light shafts are traced at
// (1/1 to 1/4; shaders.properties sizes their targets), and how that is
// brought back to full resolution: 0 upsample, 1 dither.
#define REFLECTION_RES 2 // [1 2 3 4]
#define VOLUMETRIC_RES 2 // [1 2 3 4]
#define SCALED_RESOLVE 0 // [0 1]
// Shading texels by distance: full detail (16 per block) out to
// TEXEL_FULL_DISTANCE blocks, then each ring TEXEL_RING_WIDTH blocks wide
// halves the texels per block, down to TEXEL_MIN_RES. Fewer texels mean
// fewer texel-cache entries to shade.
#define TEXEL_FULL_DISTANCE 24 // [8 16 24 32 48 64 96]
#define TEXEL_RING_WIDTH 16 // [8 16 24 32 48 64]
#define TEXEL_MIN_RES 2 // [1 2 4 8 16]
// How plants off the block grid (cross plants, sugar cane) are shadowed:
// 0 Fancy, texels on the plant's own plane through the texel cache; 1 Fast,
// a lookup per vertex; 2 Potato, one shade per plant block.
#define FOLIAGE_SHADING 0 // [0 1 2]
// Clouds: 0 Soft, lit by the sky around them and melting into it far off;
// 1 Classic, lit as a plain surface.
#define CLOUD_STYLE 0 // [0 1]
// Blocks out by which Soft clouds have melted into the sky.
#define CLOUD_BLEND_DISTANCE 384.0 // [128.0 192.0 256.0 384.0 512.0 768.0]
// Strength of the glow looking toward the sun through a cloud.
#define CLOUD_SCATTER 1.0 // [0.0 0.5 1.0 1.5 2.0]
#endif

static const float PI = 3.14159265;

// Block ids from block.properties.

/// Leaf blocks (block.properties 10004), foliage that casts shadows.
bool is_leaves(float id) {
    return id > 10003.5 && id < 10004.5;
}
static const float MATERIAL_WATER = 1.0;
static const float MATERIAL_FOLIAGE = 2.0;
static const float MATERIAL_EMISSIVE = 3.0;

float material_of(float id) {
    if (id > 10000.5 && id < 10003.5) {
        return id - 10000.0;
    }
    if (is_leaves(id)) {
        return MATERIAL_FOLIAGE;
    }
    return 0.0;
}

float3 to_linear(float3 c) {
    return pow(max(c, 0.0), 2.2);
}

float luma(float3 c) {
    return dot(c, float3(0.2126, 0.7152, 0.0722));
}

float2 oct_encode(float3 n) {
    float2 p = n.xy / (abs(n.x) + abs(n.y) + abs(n.z));
    if (n.z >= 0.0) {
        return p;
    }
    return (1.0 - abs(p.yx)) * select(p >= 0.0, float2(1.0), float2(-1.0));
}

float3 oct_decode(float2 e) {
    float3 n = float3(e, 1.0 - abs(e.x) - abs(e.y));
    float t = max(-n.z, 0.0);
    n.x += n.x >= 0.0 ? -t : t;
    n.y += n.y >= 0.0 ? -t : t;
    return normalize(n);
}

/// 0..5: +x, -x, +y, -y, +z, -z, by the normal's largest axis.
float face_of(float3 n) {
    float3 a = abs(n);
    if (a.x >= a.y && a.x >= a.z) {
        return n.x < 0.0 ? 1.0 : 0.0;
    }
    if (a.y >= a.z) {
        return n.y < 0.0 ? 3.0 : 2.0;
    }
    return n.z < 0.0 ? 5.0 : 4.0;
}

/// The face code of a surface off the block grid. Its texel holds the cell
/// of the texture pixel's middle (Fancy), its shade (Fast) or its block's
/// top middle (Potato).
static const uint FACE_DIAGONAL = 6;

/// Fancy: the texel cell, on the plane through `local` with normal `n`, at
/// 1/16 steps across x and up, so its middle lies on the plant.
float3 plane_cell(float3 local, float3 n) {
    float3 block = floor(local);
    float3 q = local - block;
    float s = clamp(floor(q.x * 16.0), 0.0, 15.0);
    float t = clamp(floor(q.y * 16.0), 0.0, 15.0);
    float x = block.x + (s + 0.5) / 16.0;
    float2 h = normalize(n.xz);
    float z = local.z - h.x * (x - local.x) / (abs(h.y) < 1e-4 ? 1e-4 : h.y);
    return floor(float3(x, block.y + (t + 0.5) / 16.0, z) * 16.0);
}

/// Whether a normal is off the block grid.
bool is_diagonal(float3 n) {
    float3 a = abs(n);
    return max(a.x, max(a.y, a.z)) < 0.9;
}

float3 face_normal(uint face) {
    // Diagonal surfaces light as if facing up, as foliage does.
    if (face == FACE_DIAGONAL) {
        return float3(0.0, 1.0, 0.0);
    }
    float sign = (face & 1u) == 1u ? -1.0 : 1.0;
    uint axis = face >> 1u;
    return float3(axis == 0 ? sign : 0.0, axis == 1 ? sign : 0.0, axis == 2 ? sign : 0.0);
}

/// A terrain pixel's surface: face, material, block and sky light. The
/// normal is the face's.
struct Surface {
    uint face;
    float material;
    float2 light;
};

/// A pixel of the texel target (colortex2): four 16-bit words, stored as
/// RGBA16 (UNORM), or RGBA32F on Android. Terrain keeps its cell (relative
/// to the camera's block: x and z in 15 bits, y in 14) and a 20-bit word:
/// a terrain flag, face (3 bits), material (2), block and sky light (7
/// each). The word's low 16 bits are the fourth word, its top 4 the cell
/// words' spare bits. All zero is lit as drawn; 2 in the fourth word alone
/// is the hand.
struct Texel {
    bool terrain;
    float3 cell;
    Surface surface;
};

static const float4 TEXEL_LIT = float4(0.0);
static const float4 TEXEL_HAND = float4(0.0, 0.0, 0.0, 2.0 / 65535.0);
static const int3 TEXEL_BIAS = int3(16384, 8192, 16384);

float4 pack_texel(float3 cell, float face, float material, float2 light) {
    uint2 l = uint2(round(clamp(light, 0.0, 1.0) * 127.0));
    uint s = 1u | (uint(face) << 1u) | (uint(material) << 4u) | (l.x << 6u) | (l.y << 13u);
    uint3 c = uint3(clamp(int3(cell) + TEXEL_BIAS, int3(0), int3(32767, 16383, 32767)));
    uint hi = s >> 16u;
    uint4 words = uint4(
        c.x | ((hi & 1u) << 15u),
        c.y | ((hi >> 2u) << 14u),
        c.z | (((hi >> 1u) & 1u) << 15u),
        s & 65535u);
    return float4(words) / 65535.0;
}

/// Fast foliage's texel: its shade in place of the cell.
float4 pack_foliage_texel(float vis, float material, float2 light) {
    float x = round(clamp(vis, 0.0, 1.0) * 16383.0) - 16384.0;
    return pack_texel(float3(x, 0.0, 0.0), float(FACE_DIAGONAL), material, light);
}

float foliage_of(Texel t) {
    return (t.cell.x + 16384.0) / 16383.0;
}

Texel read_texel(float4 texel) {
    uint4 w = uint4(round(texel * 65535.0));
    uint s = w.w | ((w.x >> 15u) << 16u) | ((w.z >> 15u) << 17u) | ((w.y >> 14u) << 18u);
    Texel t;
    t.terrain = (s & 1u) == 1u;
    t.cell = float3(int3(uint3(w.x & 32767u, w.y & 16383u, w.z & 32767u)) - TEXEL_BIAS);
    t.surface.face = (s >> 1u) & 7u;
    t.surface.material = float((s >> 4u) & 3u);
    t.surface.light = float2(float((s >> 6u) & 127u), float((s >> 13u) & 127u)) / 127.0;
    return t;
}

/// The middle of a block-face texel (1/16 of a block), relative to the block
/// the camera is in, given the cell inside the surface it covers.
float3 texel_centre(float3 cell, float3 n) {
    return (cell + 0.5) / 16.0 + n * (0.5 / 16.0);
}

/// Texels per block for a block `distance` blocks from the camera: 16 out to
/// TEXEL_FULL_DISTANCE, then halved every TEXEL_RING_WIDTH blocks, never
/// below TEXEL_MIN_RES.
float texel_res(float distance) {
    float past = max(distance - float(TEXEL_FULL_DISTANCE), 0.0);
    float level = past > 0.0 ? floor(past / float(TEXEL_RING_WIDTH)) + 1.0 : 0.0;
    return max(16.0 / exp2(min(level, 4.0)), float(TEXEL_MIN_RES));
}

/// The texels per block a 16-per-block `cell` shades at: the ring of its
/// block. The terrain pass picks the ring this way and the passes that light
/// the cell find it again from the cell alone.
float cell_res(float3 cell) {
    float3 block = floor(cell / 16.0) + 0.5 - frame.cameraPositionFract;
    return texel_res(length(block));
}

/// The 16-per-block cell a surface point shades as, at `res` texels per
/// block: across the face the point falls into a texel `16 / res` cells
/// wide, aligned to the block grid, and takes the cell at that texel's
/// middle, so every pixel of the coarse texel keys the same cache entry.
/// Along the normal it keeps the face's own cell.
float3 coarse_cell(float3 local, float3 n, float res) {
    float3 fine = floor(local * 16.0 - n * 0.5);
    float k = 16.0 / res;
    float3 coarse = floor(local * res) * k + floor(k * 0.5);
    float3 along = abs(n);
    return fine * along + coarse * (1.0 - along);
}

float ign(float2 p) {
    return frac(52.9829189 * frac(dot(p, float2(0.06711056, 0.00583715))));
}

// The 4x4 ordered (Bayer) pattern: every value once in each 4x4 block.
static const float BAYER[16] = {
    0.0, 8.0, 2.0, 10.0,
    12.0, 4.0, 14.0, 6.0,
    3.0, 11.0, 1.0, 9.0,
    15.0, 7.0, 13.0, 5.0,
};

float bayer(float2 frag) {
    uint2 p = uint2(frag) & 3u;
    return (BAYER[p.y * 4 + p.x] + 0.5) / 16.0;
}

/// The Bayer value here, stepped on every frame so each pixel visits all 16
/// levels in turn (in the pattern's own order, so successive frames land far
/// apart): an effect accumulated over a few frames sees each offset.
float bayer_temporal(float2 frag) {
    uint2 p = uint2(frag) & 3u;
    float step = BAYER[uint(frame.frameCounter) & 15u];
    return (fmod(BAYER[p.y * 4 + p.x] + step, 16.0) + 0.5) / 16.0;
}

float3 view_pos(float2 uv, float depth) {
    float4 v = mul(frame.gbufferProjectionInverse, float4(uv * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0));
    return v.xyz / v.w;
}

float3 player_pos(float3 view) {
    return mul(frame.gbufferModelViewInverse, float4(view, 1.0)).xyz;
}

float3 player_dir(float3 view_dir) {
    return normalize(mul(frame.gbufferModelViewInverse, float4(view_dir, 0.0)).xyz);
}

float3 sun_dir() {
    return player_dir(frame.sunPosition);
}

/// The sun by day, the moon by night: what the shadow map is drawn from.
float3 light_dir() {
    return player_dir(frame.shadowLightPosition);
}

bool has_sky() {
    return frame.hasSkylight != 0;
}

/// 0 at night, 1 by day, easing through sunrise and sunset.
float daylight() {
    return smoothstep(-0.12, 0.18, sun_dir().y);
}

/// The colour of the sun or moon's direct light.
float3 direct_light() {
    float s = sun_dir().y;
    float3 warm = float3(1.0, 0.52, 0.26);
    float3 noon = float3(1.0, 0.95, 0.88);
    float height = smoothstep(0.0, 0.5, s);
    float3 sun = lerp(warm, noon, height) * SUN_BRIGHTNESS * lerp(0.3, 1.0, height) * smoothstep(-0.02, 0.1, s);
    float3 moon = float3(0.6, 0.68, 0.85) * 0.12 * smoothstep(-0.02, 0.1, -s);
    return (sun + moon) * (1.0 - frame.rainStrength * 0.85);
}

/// The colour of the open sky's light on a surface facing up.
float3 sky_ambient() {
    float day = daylight();
    float3 night = float3(0.016, 0.02, 0.032);
    float3 noon = float3(0.34, 0.46, 0.66);
    float3 dusk = float3(0.42, 0.34, 0.36);
    float s = sun_dir().y;
    float3 lit = lerp(dusk, noon, smoothstep(0.05, 0.4, s));
    float3 c = lerp(night, lit, day);
    return lerp(c, float3(luma(c)) * 0.75, frame.rainStrength * 0.7);
}

/// Whether colortex2's fourth word marks the hand (which no fog or
/// reflection touches).
bool is_hand(float texel_w) {
    return round(texel_w * 65535.0) == 2.0;
}

static const float3 TORCH = float3(1.0, 0.6, 0.3);

/// The sky seen in direction `dir` (player space), without the sun's disc.
float3 sky_color(float3 dir) {
    if (!has_sky()) {
        return to_linear(frame.fogColor);
    }
    float3 sun = sun_dir();
    float s = sun.y;
    float day = daylight();
    float up = clamp(dir.y, 0.0, 1.0);
    float horizon = pow(1.0 - up, 4.0);

    float3 zenith = lerp(float3(0.004, 0.007, 0.018), float3(0.12, 0.34, 0.92), day);
    float3 rim = lerp(float3(0.012, 0.018, 0.04), float3(0.58, 0.76, 1.0), day);
    float3 c = lerp(zenith, rim, horizon);

    float mu = max(dot(dir, sun), 0.0);
    float low_sun = 1.0 - smoothstep(0.0, 0.32, abs(s));
    float toward = 0.25 + 0.75 * pow(mu, 3.0);
    c = lerp(c, float3(1.0, 0.42, 0.16) * 1.3, low_sun * horizon * toward * smoothstep(-0.2, 0.0, s));
    c += float3(1.0, 0.72, 0.42) * (pow(mu, 12.0) * 0.3 + pow(mu, 300.0) * 1.2) * smoothstep(-0.08, 0.05, s);

    // Below the horizon: the haze darkens toward the ground.
    c *= lerp(1.0, 0.45, smoothstep(0.0, -0.4, dir.y));
    c = lerp(c, float3(luma(c)) * 0.7, frame.rainStrength * 0.8);
    return c * 1.35;
}

/// Sky light at or under which `surface_light` drops the direct light.
static const float DIRECT_MIN_SKY = 0.05;

/// Whether any direct light reaches a surface, so its shadow lookup counts:
/// not without a sky, under cover (sky light too low), or on a face turned
/// from the light, unless it is foliage, which light passes through. Where
/// this is false `surface_light` zeroes the direct term whatever `vis` is.
bool direct_reaches(float3 n, float sky, bool foliage) {
    if (!has_sky() || sky <= DIRECT_MIN_SKY) {
        return false;
    }
    return foliage || dot(n, light_dir()) > 0.0;
}

/// Light on a surface. `light` is the block and sky light, 0..1; `vis` the
/// share of the direct light reaching it; `foliage` lets light through.
float3 surface_light(float3 albedo, float3 n, float2 light, float vis, bool foliage) {
    float3 l = light_dir();
    float ndl = max(dot(n, l), 0.0);
    if (foliage) {
        ndl = lerp(ndl, 0.65, 0.6);
    }
    float sky = light.y;
    // Shadow maps reach so far; past that, sky light decides.
    float open = smoothstep(DIRECT_MIN_SKY, 0.45, sky);
    float3 direct = direct_light() * ndl * vis * open;
    float3 ambient = sky_ambient() * sky * sky * (0.7 + 0.3 * n.y);
    float3 torch = TORCH * pow(light.x, 2.6) * TORCH_BRIGHTNESS;
    float3 floor_light = float3(0.01);
    return albedo * (direct + ambient + torch + floor_light);
}

// The texel cache (bufferObject.0): one entry per block-face texel near the
// camera, shaded once a frame and read by every pixel it covers.
static const uint CACHE_SIZE = 1048576;
static const uint CACHE_MASK = 1048575;
static const uint CACHE_PROBES = 12;
static const uint CACHE_EMPTY = 0xFFFFFFFFu;

/// The cache key of a texel cell (relative to the camera's block, in texels)
/// and face, or `CACHE_EMPTY` out of the cache's reach (32 blocks across,
/// 16 up and down).
uint cache_key(float3 cell, float face) {
    int3 c = int3(cell) + int3(512, 256, 512);
    if (any(c < 0) || c.x > 1023 || c.y > 511 || c.z > 1023) {
        return CACHE_EMPTY;
    }
    return uint(c.x) | (uint(c.z) << 10u) | (uint(c.y) << 20u) | (uint(face) << 29u);
}

float3 cache_cell(uint key) {
    int x = int(key & 1023u) - 512;
    int z = int((key >> 10u) & 1023u) - 512;
    int y = int((key >> 20u) & 511u) - 256;
    return float3(float(x), float(y), float(z));
}

uint cache_face(uint key) {
    return key >> 29u;
}

// The coarse table: texels below 16 per block out of the table above's
// reach, keyed in units of their own texel, so far rings shade once per
// texel too. Work entries for it carry COARSE_WORK.
static const uint COARSE_WORK = 0x80000000u;

/// The coarse key of a texel cell: the cell in units of its texel (9 bits
/// a side, reaching 16 blocks per texel-per-block halving), face and ring,
/// or `CACHE_EMPTY` at full detail, out of reach, or off the grid (plants).
uint coarse_key(float3 cell, uint face) {
    float res = cell_res(cell);
    if (res > 8.0 || face == FACE_DIAGONAL) {
        return CACHE_EMPTY;
    }
    uint level = uint(round(log2(16.0 / res)));
    int3 q = int3(floor(cell / float(1u << level))) + int3(256);
    if (any(q < 0) || any(q > 511)) {
        return CACHE_EMPTY;
    }
    return uint(q.x) | (uint(q.z) << 9u) | (uint(q.y) << 18u) | (face << 27u) | ((level - 1u) << 30u);
}

uint coarse_face(uint key) {
    return (key >> 27u) & 7u;
}

/// The offset along the face's normal within its texel unit, which the key
/// drops; the claimer keeps it beside the key.
uint coarse_along(float3 cell, uint key) {
    float k = float(1u << ((key >> 30u) + 1u));
    float along = cell[coarse_face(key) >> 1u];
    return uint(along - floor(along / k) * k);
}

/// The cell a coarse key stands for: across the face the texel's middle
/// (as `coarse_cell` makes it), along the normal the claimer's offset.
float3 coarse_cell_of(uint key, uint along) {
    int k = int(1u << ((key >> 30u) + 1u));
    int3 q = int3(int(key & 511u), int((key >> 18u) & 511u), int((key >> 9u) & 511u)) - int3(256);
    int3 c = q * k + int3(k / 2);
    uint axis = coarse_face(key) >> 1u;
    c[axis] = q[axis] * k + int(along);
    return float3(c);
}

uint cache_hash(uint key) {
    uint h = key * 0x9E3779B1u;
    h ^= h >> 15u;
    h *= 0x85EBCA77u;
    h ^= h >> 13u;
    return h & CACHE_MASK;
}
