// Torch Vibrant: the water surface's waves and its World Traced reflections,
// shared by the water (gbuffers_water) and the passes that trace and lay
// its reflections (composite_a, composite). Needs lib/common.slang and
// lib/shadow.slang first.

float hash(int2 i) {
    uint h = (asuint(i.x) * 0x8DA6B343u) ^ (asuint(i.y) * 0xD8163841u);
    h = (h ^ (h >> 13u)) * 0x5BD1E995u;
    h ^= h >> 15u;
    return float(h) * (1.0 / 4294967295.0);
}

/// The slope of smooth value noise at `p`, worked out alongside it, so a
/// normal costs one lookup of the four corners instead of three.
float2 noise_slope(float2 p) {
    float2 cell = floor(p);
    float2 f = p - cell;
    float2 u = f * f * (3.0 - 2.0 * f);
    float2 du = 6.0 * f * (1.0 - f);
    int2 i = int2(cell);
    float a = hash(i);
    float b = hash(i + int2(1, 0));
    float c = hash(i + int2(0, 1));
    float d = hash(i + int2(1, 1));
    float k = a - b - c + d;
    return float2(du.x * (b - a + k * u.y), du.y * (c - a + k * u.x));
}

/// How much of a ripple of `wavelength` blocks to keep when one pixel spans
/// `footprint` blocks: all of it from four pixels a wave, none below two.
/// A ripple finer than that cannot be drawn, only aliased into sparkle.
float resolvable(float wavelength, float footprint) {
    return smoothstep(2.0 * footprint, 4.0 * footprint, wavelength);
}

/// The slope of the swell: octaves of noise, each turned and drifting its
/// own way, so the surface never lines up into rows. Octaves too fine for
/// the pixel they land in are left out (`footprint`, blocks per pixel).
float2 wave_slope(float2 p, float t, float footprint) {
    // Rotations, written by rows.
    float2x2 r1 = float2x2(0.8, -0.6, 0.6, 0.8);
    float2x2 r2 = float2x2(0.28, 0.96, -0.96, 0.28);
    float2x2 r12 = mul(r1, r2);
    // Each octave samples q = r * p * scale; its slope in p is r^T * slope(q) * scale,
    // and its wavelength is 1 / scale blocks.
    float2 g = noise_slope(p * 0.32 + float2(t * 0.22, t * 0.05))
        * (0.11 * 0.32 * resolvable(1.0 / 0.32, footprint));
    g += mul(noise_slope(mul(r1, p) * 0.75 + float2(-t * 0.31, t * 0.17)), r1)
        * (0.05 * 0.75 * resolvable(1.0 / 0.75, footprint));
    g += mul(noise_slope(mul(r2, p) * 1.6 + float2(t * 0.12, -t * 0.38)), r2)
        * (0.022 * 1.6 * resolvable(1.0 / 1.6, footprint));
    g += mul(noise_slope(mul(r12, p) * 3.1 + float2(-t * 0.45, -t * 0.2)), r12)
        * (0.01 * 3.1 * resolvable(1.0 / 3.1, footprint));
    return g;
}

/// The ripple's normal, taken at the middle of the block texel `world` is
/// in, so the surface shades in texel-sized facets. Once a texel is smaller
/// than a pixel the facets themselves would alias, so they give way to the
/// unsnapped swell.
float3 wave_normal(float3 world, float footprint) {
    float facets = 1.0 - smoothstep(0.5 / 16.0, 1.5 / 16.0, footprint);
    float2 snapped = (floor(world.xz * 16.0) + 0.5) / 16.0;
    float2 p = lerp(world.xz, snapped, facets);
    float2 slope = wave_slope(p, frame.frameTimeCounter, footprint);
    return normalize(float3(-slope.x, 1.0, -slope.y));
}

static const uint VOXEL_MAGIC = 0x56584c32u;

/// What a ray from `origin` (player space) along `dir` meets in the voxel
/// window (`vk::voxels`): the lit block face and 1, or -1 where it meets
/// nothing the window knows (out of it, not loaded or uploaded, out of
/// steps), left to the sky.
/// `surface` is the origin's distance from the eye, for haze. `width` is
/// the ray's footprint at the origin (blocks): it widens with distance as
/// the eye's pixel cone does, and the hit samples the atlas mip that
/// footprint covers, so far hits blur instead of shimmering. The ray
/// walks block by block (a DDA) from the camera's block, so it stays exact
/// far from the origin, and crosses an empty section in one jump, so long
/// rays through air stay cheap.
float4 trace_voxels(float3 origin, float3 dir, float surface, float width) {
    ByteAddressBuffer vox = iris_vk_voxels();
    if (vox.Load(0) != VOXEL_MAGIC) {
        return float4(0.0, 0.0, 0.0, -1.0);
    }
    float3 p = origin + frac(frame.cameraPosition);
    int3 base = int3(floor(frame.cameraPosition));
    int3 cell = int3(floor(p));
    int3 step = int3(sign(dir));
    float3 inv = 1.0 / max(abs(dir), float3(1e-6));
    float3 next = select(dir > 0.0, float3(cell) + 1.0 - p, p - float3(cell)) * inv;
    uint side = vox.Load(4);
    uint height = vox.Load(8);
    uint header = 16u + side * side * height * 16u;
    // Reach is measured from the camera, across: the window is centred on
    // it, so a reflection stops at the same range wherever the water is.
    float reach = float(REFLECTION_DISTANCE);
    int3 section = int3(0x7fffffff);
    uint state = 0u;
    uint records = 0u;
    int axis = 1;
    float t = 0.0;
    bool jumped = false;
    for (int i = 0; i < 1024; i++) {
        // Step first: the origin's own block (a lily pad, the shore) is
        // the surface's, not what it mirrors. After a jump the ray already
        // stands in its next block.
        if (jumped) {
            jumped = false;
        } else if (next.x < next.y && next.x < next.z) {
            t = next.x;
            next.x += inv.x;
            cell.x += step.x;
            axis = 0;
        } else if (next.y < next.z) {
            t = next.y;
            next.y += inv.y;
            cell.y += step.y;
            axis = 1;
        } else {
            t = next.z;
            next.z += inv.z;
            cell.z += step.z;
            axis = 2;
        }
        // `cell` is relative to the camera's block.
        float2 across = float2(cell.xz) + 0.5 - frac(frame.cameraPosition.xz);
        if (dot(across, across) > reach * reach) {
            break;
        }
        int3 w = cell + base;
        int3 s = w >> 4;
        if (any(s != section)) {
            section = s;
            int3 m = int3(int(side), int(height), int(side));
            uint3 q = uint3(((s % m) + m) % m);
            uint slot = q.x + q.z * side + q.y * side * side;
            uint4 h = vox.Load4(16u + slot * 16u);
            uint word = all(int3(h.xyz) == s) ? h.w : 0u;
            state = word & 3u;
            records = header + (word >> 2u) * 32768u;
            if (state == 0u) {
                return float4(0.0, 0.0, 0.0, -1.0);
            }
        }
        if (state == 1u) {
            // Empty: jump to where the ray leaves the section.
            float3 lo = float3(s * 16 - base);
            float3 exits = (select(dir > 0.0, lo + 16.0, lo) - p) * sign(dir) * inv;
            exits = select(abs(dir) > 1e-6, exits, float3(1e9));
            float leave = min(exits.x, min(exits.y, exits.z));
            axis = exits.x <= exits.y && exits.x <= exits.z ? 0 : (exits.y <= exits.z ? 1 : 2);
            t = leave;
            float3 at = p + dir * (leave + 1e-3);
            cell = int3(floor(at));
            next = select(dir > 0.0, float3(cell) + 1.0 - at, at - float3(cell)) * inv + leave + 1e-3;
            jumped = true;
            continue;
        }
        int3 l = w & 15;
        uint2 r = vox.Load2(records + uint((l.y << 8) | (l.z << 4) | l.x) * 8u);
        uint flags = r.x >> 24u;
        if ((flags & 1u) == 0u) {
            continue;
        }
        // The face it entered by, and where on it.
        float3 hit = p + dir * t;
        float3 nrm = float3(0.0);
        nrm[axis] = -float(step[axis]);
        bool bottom = axis == 1 && step.y > 0;
        uint tile = bottom ? (r.x >> 12u) & 4095u : r.x & 4095u;
        bool tinted = (flags & (bottom ? 4u : 2u)) != 0u;
        float2 f = axis == 1 ? frac(hit.xz) : float2(frac(axis == 0 ? hit.z : hit.x), 1.0 - frac(hit.y));
        float2 atlas = float2(iris_size(gtexture));
        float tile_px = atlas.x / 16.0;
        float2 corner = float2(float(tile % 16u), float(tile / 16u)) * tile_px;
        int2 texel = int2(corner + clamp(floor(f * tile_px), float2(0.0), float2(tile_px - 1.0)));
        float4 c = gtexture.Load(int3(texel, 0));
        if ((flags & 8u) != 0u && c.a < 0.5) {
            continue;
        }
        // Filtered: the mip the footprint covers, kept inside the tile.
        float spread = width * (1.0 + t / max(surface, 1.0));
        float lod = clamp(log2(max(spread * tile_px, 1e-4)), 0.0, log2(tile_px) - 1.0);
        // Cutouts keep their texel: their mips mix in see-through pixels.
        if (lod > 0.0 && (flags & 8u) == 0u) {
            float pad = exp2(lod) * 0.5;
            float2 at_px = corner + clamp(f * tile_px, float2(pad), float2(tile_px - pad));
            c.rgb = gtexture.SampleLevel(gtexture_sampler, at_px / atlas, lod).rgb;
        }
        float3 tint = tinted ? float3(float(r.y & 255u), float((r.y >> 8u) & 255u), float((r.y >> 16u) & 255u)) / 255.0 : float3(1.0);
        float3 albedo = to_linear(c.rgb * tint);
        float2 light = float2(float(r.y >> 28u), float((r.y >> 24u) & 15u)) / 15.0;
        float3 at = hit - frac(frame.cameraPosition);
        float vis = dot(nrm, light_dir()) > 0.0 ? shadow_visibility(at, nrm, 1) : 0.0;
        float3 col = surface_light(albedo, nrm, light, vis, false);
        if ((flags & 16u) != 0u) {
            col += albedo * 0.6;
        }
        float dist = surface + t;
        float haze = 1.0 - exp(-dist * 0.0008 * FOG_DENSITY);
        float edge = smoothstep(frame.far * 0.75, frame.far, dist);
        return float4(lerp(col, sky_color(dir), max(haze * 0.45, edge)), 1.0);
    }
    // Out of steps in known air: what lies past is unknown.
    return float4(0.0, 0.0, 0.0, -1.0);
}

/// How much reflection mixes in at view cosine `cos_view`: the mod's
/// curve, exp(-k cos) rescaled to 1 at grazing and 0 straight down (k 3.6;
/// Voxel takes Reflection Falloff), from 0.15 up, times Reflection Strength
/// (100% is half).
float reflection_blend(float cos_view) {
    float k = REFLECTION_METHOD == 1 ? REFLECTION_FALLOFF : 3.6;
    float ek = exp(-k);
    float grazing = max((exp(-k * cos_view) - ek) / (1.0 - ek), 0.0);
    return lerp(0.15, 1.0, grazing) * float(REFLECTION_STRENGTH) / 200.0;
}

/// A voxel reflection from `from` (player space) off normal `rn`: the
/// reflected colour (the sky where the window knows nothing, `covered`
/// under cover) and its blend.
float4 voxel_reflection(float3 from, float3 rn, float width, float3 covered, float open) {
    float3 rv = normalize(-from);
    float3 r = reflect(-rv, rn);
    float4 m = trace_voxels(from + rn * 0.03, r, length(from), width);
    float3 col = lerp(lerp(covered, sky_color(r), open), m.rgb, max(m.a, 0.0));
    return float4(col, reflection_blend(saturate(dot(rn, rv))));
}

/// One reflection texel, at world `xz` on a surface at height `y`.
float4 voxel_texel(float2 xz, float y, float width, float footprint, float3 covered, float open) {
    float3 at = float3(xz.x, y, xz.y);
    float3 rn = WATER_STYLE == 0 ? wave_normal(at, footprint) : float3(0.0, 1.0, 0.0);
    return voxel_reflection(at - frame.cameraPosition, rn, width, covered, open);
}


// The reflection table (bufferObject.2): World Traced reflections, one ray
// per reflection texel. The water claims the texels its pixels show and
// lists them (`refl_claim`), composite_a traces each listed texel once
// (`refl_trace`), composite looks each pixel's texel up and blends it in,
// and composite1_a empties the table for the next frame. Layout: a count
// and three pads, then REFL_SIZE keys, REFL_SIZE four-word values and the
// REFL_SIZE-long work list.

RWByteAddressBuffer refl() { return iris_rw_buffer(IRIS_BUFFER_2); } // read_write

static const uint REFL_SIZE = 524288;
static const uint REFL_MASK = 524287;
static const uint REFL_PROBES = 16;
static const uint REFL_EMPTY = 0u;
/// A claimed texel's last value word until composite_a traces it.
static const uint REFL_PENDING = 0xFFFFFFFFu;
static const uint REFL_COUNT = 0;
static const uint REFL_KEYS = 16;
static const uint REFL_VALUES = REFL_KEYS + 4 * REFL_SIZE;
static const uint REFL_WORK = REFL_VALUES + 16 * REFL_SIZE;

/// The texels per block a reflection is traced at, by level (0 is 16).
float refl_res(uint level) {
    return 16.0 / float(1u << level);
}

/// The level for a surface `across` blocks per pixel across the view:
/// texels no narrower than REFLECTION_TEXEL_PIXELS pixels, at most 16 a
/// block.
uint refl_level(float across) {
    float per_block = 1.0 / max(across * float(REFLECTION_TEXEL_PIXELS), 1e-4);
    float res = min(exp2(floor(log2(max(per_block, 1.0)))), 16.0);
    return uint(round(log2(16.0 / res)));
}

/// The key of the reflection texel `world` (a water surface) falls in at
/// `level`: its texel x and z from the camera's block (11 bits each), its
/// block y from it (7) and the level (3), plus one so no key is
/// REFL_EMPTY; REFL_EMPTY out of reach.
uint refl_key(float3 world, uint level) {
    float res = refl_res(level);
    int2 t = int2(floor(world.xz * res) - floor(frame.cameraPosition.xz) * res) + 1024;
    int y = int(floor(world.y) - floor(frame.cameraPosition.y)) + 64;
    if (any(t < 0) || any(t > 2047) || y < 0 || y > 127) {
        return REFL_EMPTY;
    }
    return (uint(t.x) | (uint(t.y) << 11u) | (uint(y) << 22u) | (level << 29u)) + 1u;
}

uint refl_hash(uint key) {
    uint h = key * 0x9E3779B1u;
    h ^= h >> 15u;
    return h & REFL_MASK;
}

/// Claims `key`'s texel for the frame; the first claimer lists it, with
/// its surface's height (player space) for the trace.
void refl_claim(uint key, float y) {
    uint slot = refl_hash(key);
    for (uint probe = 0; probe < REFL_PROBES; probe++) {
        uint before;
        refl().InterlockedCompareExchange(REFL_KEYS + 4 * slot, REFL_EMPTY, key, before);
        if (before == REFL_EMPTY) {
            refl().Store4(REFL_VALUES + 16 * slot, uint4(asuint(y), 0u, 0u, REFL_PENDING));
            uint at;
            refl().InterlockedAdd(REFL_COUNT, 1u, at);
            if (at < REFL_SIZE) {
                refl().Store(REFL_WORK + 4 * at, slot);
            }
            return;
        }
        if (before == key) {
            return;
        }
        slot = (slot + 1) & REFL_MASK;
    }
}

/// `key`'s traced value, or REFL_PENDING in its last word where it is
/// missing or untraced.
uint4 refl_lookup(uint key) {
    uint slot = refl_hash(key);
    for (uint probe = 0; probe < REFL_PROBES; probe++) {
        uint seen = refl().Load(REFL_KEYS + 4 * slot);
        if (seen == key) {
            return refl().Load4(REFL_VALUES + 16 * slot);
        }
        if (seen == REFL_EMPTY) {
            break;
        }
        slot = (slot + 1) & REFL_MASK;
    }
    return uint4(0u, 0u, 0u, REFL_PENDING);
}

uint pack_half2(float a, float b) {
    return f32tof16(a) | (f32tof16(b) << 16u);
}

float2 unpack_half2(uint w) {
    return float2(f16tof32(w & 65535u), f16tof32(w >> 16u));
}

/// Traces the texel in work slot `slot`: what it mirrors, split so a pixel
/// can finish it with its own surface colour and sky light (`refl_finish`).
void refl_trace(uint slot) {
    uint key = refl().Load(REFL_KEYS + 4 * slot);
    float y = asfloat(refl().Load(REFL_VALUES + 16 * slot));
    refl().Store4(REFL_VALUES + 16 * slot, refl_value(key, y));
}

/// What texel `key` mirrors, its surface `y` high (player space), as
/// `refl_trace` stores it.
uint4 refl_value(uint key, float y) {
    key -= 1u;
    uint level = key >> 29u;
    float res = refl_res(level);
    float2 t = float2(float(key & 2047u), float((key >> 11u) & 2047u)) - 1024.0;
    float2 xz = (t + 0.5) / res + floor(frame.cameraPosition.xz);
    float3 at = float3(xz.x, y + frame.cameraPosition.y, xz.y);
    float width = 1.0 / res;
    float3 rn = WATER_STYLE == 0 ? wave_normal(at, width) : float3(0.0, 1.0, 0.0);
    float3 from = at - frame.cameraPosition;
    float3 rv = normalize(-from);
    float3 r = reflect(-rv, rn);
    float4 m = trace_voxels(from + rn * 0.03, r, length(from), width);
    float hit = max(m.a, 0.0);
    float3 q = m.rgb * hit;
    float3 sky = sky_color(r) * (1.0 - hit);
    float blend = reflection_blend(saturate(dot(rn, rv)));
    return uint4(
        pack_half2(q.r, q.g), pack_half2(q.b, blend),
        pack_half2(sky.r, sky.g), pack_half2(sky.b, 1.0 - hit));
}

/// A traced value finished for a pixel: the reflected colour (its own
/// `covered` colour under cover, by `open`) and the blend.
float4 refl_finish(uint4 v, float3 covered, float open) {
    float2 a = unpack_half2(v.x);
    float2 b = unpack_half2(v.y);
    float2 c = unpack_half2(v.z);
    float2 d = unpack_half2(v.w);
    float3 q = float3(a, b.x);
    float3 sky = float3(c, d.x);
    float miss = d.y;
    return float4(q + sky * open + covered * (miss * (1.0 - open)), b.y);
}

/// Open sky over a surface, from its sky light: covered water mirrors
/// itself, darkened, instead of the sky.
float refl_open(float sky) {
    float open = saturate(sky * 3.33333 - 2.33333);
    return open * open;
}
