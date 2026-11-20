# ImmersiveVFX reference

ImmersiveVFX is closed source. Never name it in code, comments, lang or
commit messages; this file and other `.md` files are the only exception.
Its jar is the reference for comparing Torch Vibrant's methods to the mod.

## Where it is

- Jar: `Mods/ImmersiveVFX-2.3.2-neoforge.jar` (git-ignored).
- Unzip it into the scratchpad, never into the repo:
  `unzip -oq Mods/ImmersiveVFX-2.3.2-neoforge.jar -d <scratchpad>/ivfx`
- What is inside:
  - `assets/immersivevfx/shaders/core/*.fsh|.vsh`: GLSL core shaders
    (sky, clouds, water, fog, bloom, underwater rays).
  - `assets/immersivevfx/ivfx/particles/*.json`: Bedrock-format particle
    effects with Molang expressions.
  - `assets/immersivevfx/ambient/*.json`: rules for spawning ambient
    effects by biome, weather and time.
  - `com/shinyflvres/immersivevfx/**.class`: Java logic. Read with `javap -c -p`
    or a decompiler.

## How to compare

When asked how the mod does something, look in the jar first. Don't guess,
and don't ask the user to explain it. Search file names (`find . | grep -i
<topic>`), then read the shader, particle JSON or class.

## Findings

### Light shafts (sunbeams)

These are particles, not a shader pass.
- `ambient/sunbeams.json` sets when they appear: daytime, clear weather,
  the `lightRays` setting on, and not in the Nether, the End or caves.
  - Placement type `sun_gap`: 125 per second within a radius of 28, up to
    a distance of 30.
  - Beams fall where sunlight passes air, plants or glass and lands on
    leaves or mushroom blocks.
  - Each biome has its own tint.
- `ivfx/particles/light_shaft_spawner.json` starts the chain:
  - `light_shaft_probe_canopy` walks down along the sun direction from 40
    blocks up until it finds a gap in the canopy.
  - `light_shaft_probe_floor` walks on to where the beam hits the ground.
  - `light_shaft_beam_builder` sizes the beam and picks its tint.
- `light_shaft_beam.json` draws the beam as one additive billboard
  (`particles_add`, texture `textures/ivfx/light_shaft_beam`). It is
  aligned to the sun direction and as long as the gap-to-floor distance.
  - It fades in and out over 5 to 10 seconds.
  - Alpha falls off with camera distance.
  - `light_shaft_beam_motes` adds dust motes within 15 blocks.
- Under water, a separate `*_underwater` particle set is used, and
  `shaders/core/vfx_underwater_rays.fsh` adds a shader for the rays.
- Compared to Torch Vibrant: our `composite1` marches the shadow map
  through a thin haze, so shafts only appear as a faint volumetric glow.
  The mod's shafts are bright, discrete beams placed where gaps in leaves
  let light through.

### Light shaft gates (ported in `src/util/particles/shafts.rs`)

`ivfx/animation_controllers/controllers.json`,
`channel_light_rays`, spawns `light_shaft_spawner` only while all of
these hold:
- **Camera not in water.** Under water, `light_shaft_spawner_underwater`
  runs instead: 100 probes a second, a 30-block walk, and water-tinted
  beams.
- **Settings:** Light Rays is on, and Light Ray Intensity is above 0.
- **`p:light_shafts_visible`** (`SunProbe.lightRayClear`): a ray toward
  the sun, or the moon when the sun is down.
  - It runs from eye + 0.1 + dir·40 for `min(16·(render distance+1), 1000)`
    blocks.
  - Air, glass and glass panes let it through. Fluids and blocks with an
    empty outline block it; other blocks block it where it clips their
    outline shape.
- **Light elevation:** `|90 − |sun_angle − 180||` is above 17.5° to
  start, and above 15° to keep going. Moon beams count.
- **Profile** is not end, deep_dark, any nether, rain, thunder, snow,
  blizzard, or desert/mesa/savanna wind and sandstorm.
  - `Profiles.select` takes the first match from an ordered list: end,
    nether ×6, deep_dark, lush_caves, dripstone_caves, sulfur underground,
    caves, snow, blizzard, aurora, cold_biome, cold, frozen_or_grove, the
    desert/mesa/savanna weather variants, rain, thunder, then biomes.
  - Cave profiles beat rain, so an enclosed cave in rain still gets
    shafts.
  - Biome tags come from `vfx/biome_tags.json`, read at the player's feet
    and re-read after moving 3 blocks.
  - Weather: `isThundering` gives THUNDER, `isRaining` gives RAIN.
  - The caves profile uses `Metrics`. It casts 40 Fibonacci-sphere rays
    from eye + 0.1, two per tick, to `min(16·(rd+1), 100)` blocks, with
    3 cosine-hemisphere escape rays at each hit.
    - The rays (`Rays.solid`) pass air, leaves, bamboo and blocks with no
      collision.
    - The profile is a cave when enclosure > 30, outside exposure < 0.1
      and stone > 0.7.
- **Density:** 400 probes a second × the particle setting (1, or 0.5 at
  Decreased).
- **Beam alpha** is multiplied by `lightRayIntensity/100` and capped at 1.
  The default intensity is 200.
- **Tint** is set per profile id (see `light_shaft_beam_builder.json`).
  The moon's is `#bcefff`.
