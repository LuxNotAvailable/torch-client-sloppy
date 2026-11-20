// Torch Vibrant: water, glass, ice and the other translucent blocks, lit as
// drawn and blended over the lit scene. Water takes its colour from how deep
// the view looks through it, ripples on the block-texel grid, and mirrors
// the scene by a screen-space ray traced here, on water pixels only, against
// the opaque scene (colortex5, copied by deferred, and depthtex1).

/* RENDERTARGETS: 0,3 */

#include "lib/common.slang"
#include "lib/shadow.slang"

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : TEXCOORD1;
    float2 light : TEXCOORD2;
    float3 player : TEXCOORD3;
    nointerpolation float3 normal : TEXCOORD4;
    nointerpolation float material : TEXCOORD5;
};

struct Lit {
    float4 color : SV_Target0;
    float4 surface : SV_Target1;
};

Varyings vertex(IrisVertexIn input) {
    IrisAttributes a = iris_attributes(input);
    Varyings o;
    o.clip = iris_clip(mul(frame.iris_ModelViewProjectionMatrix, float4(a.position, 1.0)));
    o.uv = a.uv;
    o.color = a.color;
    o.light = clamp(a.light / 240.0, 0.0, 1.0);
    o.player = a.position;
    o.normal = a.normal;
    o.material = material_of(a.entity.x);
    return o;
}

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

/// Two hashes for a cell, for a feature point that moves within it.
float2 hash2(int2 i) {
    return float2(hash(i), hash(i + int2(113, 271)));
}

/// Bright where two moving cell points are near equally close: the web
/// lines of a Voronoi pattern, which read as caustic light.
float caustic_cells(float2 p, float t) {
    float2 cell = floor(p);
    float2 f = p - cell;
    float d1 = 8.0;
    float d2 = 8.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            int2 g = int2(x, y);
            float2 o = 0.5 + 0.4 * sin(t + 6.2831853 * hash2(int2(cell) + g));
            float2 r = float2(g) + o - f;
            float d = dot(r, r);
            if (d < d1) {
                d2 = d1;
                d1 = d;
            } else if (d < d2) {
                d2 = d;
            }
        }
    }
    return 1.0 - smoothstep(0.0, 0.2, sqrt(d2) - sqrt(d1));
}

/// Caustic light on the floor at `floor_world`, `deep` blocks under the
/// surface. Traced up along the light to where it entered, then snapped to
/// the block-texel grid so the pattern stays pixel art and holds still.
float caustic(float2 floor_world, float3 l, float deep, float t) {
    float2 entry = floor_world + l.xz / max(l.y, 0.2) * deep;
    float2 p = (floor(entry * 16.0) + 0.5) / 16.0;
    float c = caustic_cells(p * 0.75, t * 0.7) * 0.65 + caustic_cells(p * 1.35 + float2(7.3, 2.9), t * 1.05) * 0.35;
    return c * c * 1.3 + c * 0.15;
}

/// View-space distance of a depth-buffer value; `a`, `b` are the inverse
/// projection's last two columns, so a lookup is a multiply-add.
float linear_depth(float depth, float4 a, float4 b) {
    float4 p = a + b * (depth * 2.0 - 1.0);
    return -p.z / p.w;
}

/// The opaque scene at `uv` as seen `dist` blocks away along `dir`: hazed as
/// composite2 hazes ground, and sky texels kept apart from ground ones so a
/// skyline does not haze into a halo.
float3 mirrored_color(float2 uv, float2 size, float3 dir, float dist) {
    float2 texel = uv * size - 0.5;
    int2 corner = int2(floor(texel));
    float2 f = texel - floor(texel);
    int2 top = int2(size) - 1;
    float3 ground = float3(0.0);
    float3 sky = float3(0.0);
    float ground_w = 0.0;
    float sky_w = 0.0;
    for (int i = 0; i < 4; i++) {
        int2 o = int2(i & 1, i >> 1);
        float w = (o.x == 1 ? f.x : 1.0 - f.x) * (o.y == 1 ? f.y : 1.0 - f.y);
        int3 px = int3(clamp(corner + o, int2(0), top), 0);
        float3 c = colortex5.Load(px).rgb * w;
        if (depthtex1.Load(px).r < 1.0) {
            ground += c;
            ground_w += w;
        } else {
            sky += c;
            sky_w += w;
        }
    }
    ground /= max(ground_w, 1e-4);
    float haze = 1.0 - exp(-dist * 0.0008 * FOG_DENSITY);
    float edge = smoothstep(frame.far * 0.75, frame.far, dist);
    ground = lerp(ground, sky_color(dir), max(haze * 0.45, edge));
    return sky_w > 0.0 ? lerp(ground, sky / max(sky_w, 1e-4), sky_w) : ground;
}

/// What the surface at `start` (view space) mirrors along `dir`: the opaque
/// scene's colour and how far to trust it (0 none). The ray steps in clip
/// x, y, w, where it is a straight line; it is first cut where it leaves a
/// little past the screen, doubles its stride while clear of the scene and,
/// once it passes behind something, halves back to the contact.
float4 trace_mirror(float3 start, float3 dir, float2 size) {
    float4 a = mul(frame.gbufferProjectionInverse, float4(0.0, 0.0, 0.0, 1.0));
    float4 b = mul(frame.gbufferProjectionInverse, float4(0.0, 0.0, 1.0, 0.0));
    float3 o = mul(frame.gbufferProjection, float4(start, 1.0)).xyw;
    float3 d = mul(frame.gbufferProjection, float4(dir, 0.0)).xyw;
    float surface = length(start);

    float reach = 1e5;
    if (d.z < 0.0) {
        reach = (o.z - 0.06) / -d.z;
    } else {
        reach = (frame.far * 2.0 - o.z) / max(d.z, 1e-6);
    }
    float4 bound = float4(1.2, 1.1, 1.2, 1.1);
    float4 grow = float4(d.xy, -d.xy) - bound * d.z;
    float4 room = bound * o.z + float4(-o.xy, o.xy);
    float4 exits = select(grow > 0.0, room / max(grow, float4(1e-6)), float4(1e5));
    reach = min(reach, min(min(exits.x, exits.y), min(exits.z, exits.w)));

    // Reflection Quality: Low, Normal, High.
    int budget = REFLECTION_QUALITY == 0 ? 16 : (REFLECTION_QUALITY == 2 ? 38 : 30);
    int halvings = REFLECTION_QUALITY == 0 ? 3 : (REFLECTION_QUALITY == 2 ? 10 : 6);
    float pixel = 2.0 / size.y;
    float span = 0.1 + surface * 0.02;
    float lo = 0.0;
    float hi = -1.0;
    float shell = 0.0;
    float hit_behind = 0.0;
    float2 hit_uv = float2(0.0);
    int halved = 0;
    for (int i = 0; i < 38; i++) {
        bool bisect = hi > 0.0;
        if (i >= budget || (bisect && halved >= halvings) || (!bisect && lo >= reach)) {
            break;
        }
        float probe = bisect ? lo + span : min(lo + span, reach);
        float3 c = o + d * probe;
        float2 s = c.xy / c.z * 0.5 + 0.5;
        // Past the screen's edge the edge pixel stands in (as a clamped
        // sampler would), so a ray leaving the screen still lands on what
        // is there and fades out, rather than cutting to the sky.
        float behind = -1e5;
        float scene = depthtex1.Load(int3(clamp(int2(s * size), int2(0), int2(size) - 1), 0)).r;
        if (scene < 1.0) {
            behind = c.z - linear_depth(scene, a, b);
        }
        float contact = c.z * pixel;
        if (bisect) {
            if (behind > -contact && behind < shell) {
                hi = probe;
                hit_uv = s;
                hit_behind = behind;
            } else {
                lo = probe;
            }
            span *= 0.5;
            halved++;
            continue;
        }
        float thickness = abs(d.z) * (probe - lo) * 1.5 + 0.4 + c.z * 0.015;
        if (behind > -contact && behind < thickness) {
            hi = probe;
            hit_uv = s;
            hit_behind = behind;
            shell = thickness;
            span = (probe - lo) * 0.5;
        } else {
            if (behind > 0.0 || behind <= -2.0 * thickness) {
                span *= 2.0;
            }
            lo = probe;
        }
    }
    if (hi <= 0.0 || hit_behind >= 0.6 + 0.02 * (o.z + d.z * hi) + abs(d.z) * (hi - lo) * 2.0) {
        return float4(0.0);
    }
    float2 edge = abs(hit_uv - 0.5);
    // Fades well inside the screen's edge, so where neighbouring rays leave
    // it the reflection has already given way to the sky.
    float fade = (1.0 - smoothstep(0.36, 0.6, edge.x)) * (1.0 - smoothstep(0.28, 0.55, edge.y));
    float3 at = start + dir * hi;
    float dist = length(at);
    // A hit nearer the eye than the surface is something in front of it on
    // screen, not what the surface sees.
    fade *= saturate((dist - surface) * 0.33333 + 1.0);
    if (fade <= 0.001) {
        return float4(0.0);
    }
    float3 world_dir = normalize(mul(frame.gbufferModelViewInverse, float4(at, 0.0)).xyz);
    return float4(mirrored_color(saturate(hit_uv), size, world_dir, dist), fade);
}

float ggx(float3 n, float3 v, float3 l, float roughness) {
    float3 h = normalize(v + l);
    float ndh = max(dot(n, h), 0.0);
    float a2 = roughness * roughness * roughness * roughness;
    float d = ndh * ndh * (a2 - 1.0) + 1.0;
    float ndl = max(dot(n, l), 0.0);
    return a2 / (PI * d * d) * ndl * 0.25;
}

Lit fragment(Varyings v) {
    float4 tex = gtexture.Sample(gtexture_sampler, v.uv) * v.color;
    // Blocks of surface one pixel spans, along its longer side.
    float footprint = max(length(ddx(v.player)), length(ddy(v.player)));
    if (!iris_alpha_pass(tex.a)) {
        discard;
    }
    float3 face = normalize(v.normal);
    float vis = shadow_visibility(v.player, face, 2);
    Lit o;

    if (v.material != MATERIAL_WATER) {
        float3 lit = surface_light(to_linear(tex.rgb), face, v.light, vis, false);
        o.color = float4(lit, tex.a);
        o.surface = float4(oct_encode(face), 0.0, v.light.y);
        return o;
    }

    float3 world = v.player + frame.cameraPosition;
    float3 n = face;
    if (face.y > 0.5 && WATER_STYLE == 0) {
        n = wave_normal(world, footprint);
    }
    float3 view_dir = normalize(-v.player);

    // How far the eye looks through the water to what is under it.
    float2 size = float2(iris_size(depthtex1));
    int2 px = int2(v.clip.xy);
    float behind = depthtex1.Load(int3(px, 0)).r;
    float3 floor_pos = player_pos(view_pos(v.clip.xy / size, behind));
    float depth = length(floor_pos) - length(v.player);
    if (behind >= 1.0) {
        depth = 64.0;
    }
    depth = max(depth, 0.0);
    float3 absorbed = exp(-depth * float3(0.2, 0.05, 0.04) / WATER_CLARITY);
    float through = (absorbed.r + absorbed.g + absorbed.b) / 3.0;

    // The biome's water colour, pulled toward a clear teal.
    float3 biome = to_linear(v.color.rgb);
    float3 tint = lerp(float3(luma(biome)), biome, 0.55) * 0.6 + float3(0.02, 0.09, 0.1);
    float sky = v.light.y;
    float3 ambient = sky_ambient() * (sky * sky) + TORCH * pow(v.light.x, 2.6) * TORCH_BRIGHTNESS * 0.5;
    float3 scatter = tint * 0.45 * (ambient + direct_light() * vis * 0.2);
    // Immersive: the biome colour kept bright in green, shaded by the
    // texture's own brightness so its pixels show through.
    float3 texel = tex.rgb / max(v.color.rgb, float3(0.001));
    if (WATER_STYLE == 2) {
        float3 c = v.color.rgb;
        c.g = max(c.g, 0.39);
        c = sqrt(c) * float3(1.0, 0.85, 0.8) * 0.375;
        c = c * 1.15 - (c.r + c.g + c.b) / 3.0 * 0.15;
        c *= clamp(luma(texel) / 0.62, 0.0, 2.0);
        scatter = to_linear(saturate(c)) * 2.0 * (ambient + direct_light() * vis * 0.2);
        if (WATER_COLOR_MAPPING == 0) {
            scatter = surface_light(to_linear(tex.rgb), face, v.light, vis, false);
        }
    }
    float3 shine;
    if (WATER_STYLE == 0) {
        shine = direct_light() * ggx(n, view_dir, light_dir(), 0.28) * vis * smoothstep(0.05, 0.45, sky) * 0.5;
    } else {
        // Flat: the texture's bright texels carry the glint, wider like a
        // sun disc, strongest looking straight down.
        // Glint Smoothness: from single texels toward the texture's mean.
        float3 mean = gtexture.SampleLevel(gtexture_sampler, v.uv, 4.0).rgb;
        float3 t = lerp(texel, mean, float(GLINT_SMOOTHNESS) / 100.0);
        float power = dot(t, t) * 0.4;
        power *= power;
        power *= power;
        float edge = 1.0 - max(dot(n, view_dir), 0.0);
        float mask = min(power, 0.5) * (2.0 - 1.8 * edge * edge);
        shine = direct_light() * ggx(n, view_dir, light_dir(), 0.42) * mask * vis * smoothstep(0.05, 0.45, sky) * 1.5;
    }

    // Murkier water starts less see-through and can close up entirely.
    float murk = clamp(1.0 / WATER_CLARITY, 0.5, 4.0);
    float depth_alpha = clamp((1.0 - through) * 0.7, 0.05 * murk, min(0.65 * sqrt(murk), 0.95));
    // The surface's own share covers the rest of the way less what depth
    // already covers, so the two add up without passing opaque.
    float alpha = WATER_SURFACE_OPACITY + (1.0 - WATER_SURFACE_OPACITY) * depth_alpha;
    if (WATER_STYLE == 2) {
        // Clouds with depth from 0.59 toward 0.98, and closes up at grazing
        // angles (fresnel^4).
        float fog = 1.0 - exp(-depth * 0.075 / WATER_CLARITY);
        float edge = 1.0 - abs(dot(n, view_dir));
        edge *= edge;
        // From Water Surface Opacity in the shallows to 0.98 deep.
        alpha = lerp(lerp(WATER_SURFACE_OPACITY, 0.98, fog), 1.0, edge * edge);
        if (WATER_COLOR_MAPPING == 0) {
            alpha = tex.a;
        }
    }
    float3 surface_color = scatter;
    // Blending scales the colour by alpha; the glint is light off the
    // surface, so undo that, but not without bound for clear water.
    o.color = float4(scatter + min(shine, 2.0) / max(alpha, 0.35), alpha);

    // Reflection, from above only, as ImmersiveVFX: mixed into the surface's
    // colour by 0.15 looking down to 1 at grazing angles, times Reflection
    // Strength (100% is half), then blended by the surface's alpha.
    if (WATER_REFLECTIONS != 0 && REFLECTION_STRENGTH > 0 && frame.isEyeInWater != 1 && dot(face, view_dir) > 0.0) {
        float cos_view = saturate(dot(n, view_dir));
        float grazing = max(exp(-3.6 * cos_view) * 1.0281 - 0.0281, 0.0);
        float blend = lerp(0.15, 1.0, grazing) * float(REFLECTION_STRENGTH) / 200.0;
        float3 r = reflect(-view_dir, n);
        // Sky only under open sky; covered water mirrors itself, darkened.
        float open = saturate(sky * 3.33333 - 2.33333);
        open *= open;
        float3 reflected = lerp(surface_color * 0.5, sky_color(r), open);
        float3 start = mul(frame.gbufferModelView, float4(v.player + n * 0.03, 1.0)).xyz;
        float3 dir = normalize(mul(frame.gbufferModelView, float4(r, 0.0)).xyz);
        float4 mirror = trace_mirror(start, dir, size);
        reflected = lerp(reflected, mirror.rgb, mirror.a);
        o.color.rgb = lerp(o.color.rgb, reflected, saturate(blend));
    }

    // Caustics: light the surface focuses on the floor. Blending keeps
    // (1 - alpha) of the floor, so light added as (1 - alpha) / alpha lands
    // on it at full strength. Near the player only, where it can be seen.
    // Taken outside the branch: derivatives need every pixel of the quad.
    float3 floor_n = cross(ddx(floor_pos), ddy(floor_pos));
    float deep = v.player.y - floor_pos.y;
    float reach = 1.0 - smoothstep(20.0, 32.0, length(v.player.xz));
    if (WATER_CAUSTICS_ENABLED != 0 && WATER_CAUSTICS > 0.0 && face.y > 0.5 && behind < 1.0 && deep > 0.0 && reach > 0.0) {
        float3 l = light_dir();
        float lit = 0.0;
        if (dot(floor_n, floor_n) > 1e-12) {
            floor_n = normalize(floor_n);
            floor_n = dot(floor_n, floor_pos) > 0.0 ? -floor_n : floor_n;
            // Floors only, as in still water light pools on the bed; walls
            // under water would show the pattern smeared down them.
            lit = smoothstep(0.7, 0.9, floor_n.y) * smoothstep(0.0, 0.3, dot(floor_n, l));
        }
        float fade = pow(max(1.0 - deep / WATER_CAUSTICS_DEPTH, 0.0), 1.4);
        // Shade cuts the light by WATER_CAUSTICS_SHADE, not to nothing: the
        // shadow map is coarse next to the pattern, and vanilla-like ponds
        // under trees would go flat.
        float shade = lerp(1.0 - WATER_CAUSTICS_SHADE, 1.0, vis);
        float strength = WATER_CAUSTICS * fade * lit * reach * shade * smoothstep(0.3, 0.8, sky);
        if (strength > 0.002) {
            float c = caustic(floor_pos.xz + frame.cameraPosition.xz, l, deep, frame.frameTimeCounter);
            float3 light = direct_light() * absorbed * c * strength;
            // Immersive: the floor keeps its caustics at full strength
            // through the more opaque surface.
            if (WATER_STYLE == 2) {
                light /= max(1.0 - alpha, 0.25);
            }
            o.color.rgb += light * (1.0 - alpha) / max(alpha, 0.05);
        }
    }
    o.surface = float4(oct_encode(n), 1.0, sky);
    return o;
}
