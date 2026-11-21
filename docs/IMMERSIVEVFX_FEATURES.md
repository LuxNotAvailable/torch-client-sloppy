# ImmersiveVFX 2.3.2 features

This lists every feature in the mod's options menu. The list and the
descriptions come from the mod's own `en_us.json`.

For each feature, mark it `[x]` to implement it or `[-]` to skip it.

**Have** says what Torch Vibrant already does:
- **yes**: we have it.
- **part**: we have part of it.
- **no**: we don't have it.

## General: Look

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Smooth Block Place | Blocks grow into place instead of popping in. | no |
| [ ] | Held Torch Light | Torches, lanterns and other glowing blocks held in the hand light up the area around you. | no |
| [ ] | Modded Biomes | Gives biomes from other mods fitting fog colours. | no |
| [ ] | Natural Foliage Colors | Softer, more natural colours for grass and leaves. | no |

## Sky and Clouds

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Sky Mode (Realistic / Vanilla) | Realistic draws a computed sky. Vanilla keeps the familiar colours. | part (our sky is computed) |
| [ ] | Custom Skies | Replaces the sky, sun and moon with the mod's own. | no |
| [ ] | Shooting Stars | Stars streak across the night sky now and then. | no |
| [ ] | Cloud Rendering | How clouds are drawn, or turns them off. | yes |
| [ ] | Cloud Formation | The shape of the cloud layer. | no |
| [ ] | Multi Cloud Layers | A second cloud layer about 30 blocks above the first. | yes |
| [ ] | Cloud Layer Height (both layers) | Sets each layer's height. | no |
| [ ] | Double Clouds | Clouds get a smaller second tier on top. | no |

## Fog and Mist

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Fog Rendering Mode | Immersive uses biome fog, Chunk Distance follows render distance, or off. | part |
| [ ] | Atmospheric Fog and its strength | A haze that thickens with distance and height. | part (distance haze) |
| [ ] | Dynamic Fog | The haze takes the colour and thickness of each biome. | no |
| [ ] | Low Lying Fog | Fog banks that hug the ground. | no |
| [ ] | Ground Mist | Wisps of mist drifting over the ground (particles). | no |
| [ ] | Weather Mist | Extra mist while it rains or snows. | no |

## Lighting

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Bloom and its intensity | Light sources get a soft halo in their light's colour. | no |
| [ ] | Enhanced Light Features | Glowing blocks look hotter and their dark parts darker. | part (emissive) |
| [ ] | Colored Light, its intensity and radius | Light takes the colour of its block, read from the texture. | no |
| [ ] | Dungeons Fire | Fire gets lit smoke, embers, burn marks and warm light. | no |
| [ ] | Light Rays and their intensity | Rays of sunlight falling through the air. | yes (god rays) |
| [ ] | Underwater Light Rays and their intensity | Light shafts while you are under water. | no |
| [ ] | Dynamic Color Grading | A slight blue tone at night; warm lights stay warm. | no |
| [ ] | Terrain Glint | All blocks and the water surface catch the sun while you are above water. | part (water only) |
| [ ] | Glint Smoothness | Sharp sparkling pixels through to a soft sheen. | yes |
| [ ] | Underwater Glint | Blocks catch the sun while you are under water. | no |

## Shadows

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Shadows | The sun and the moon cast shadows. | yes |
| [ ] | Shadow Mode (Enhanced / Normal) | Enhanced also relights the world. | yes |
| [ ] | Shadow Resolution (Potato to High) | Potato makes leaves cast solid shadows. | yes |
| [ ] | Shadow Strength | How dark shadows are. | no |
| [ ] | Shadow Vibrancy | More saturated sun and moon light colours. | no |
| [ ] | Shadow Boost | Darker shadows on top of Shadow Strength. | no |
| [ ] | Shadow Distance | How far shadows are drawn. | yes |
| [ ] | Mob Shadows | Mobs and players cast shadows. | yes |
| [ ] | Smooth Shadow Movement | Blends two shadow maps while the sun moves. | part (sun step) |
| [ ] | Depth Calculation | A depth prepass for leaves and plants. | skipped (perf item 7) |
| [ ] | Underwater Shadows | Shadows on the sea floor while you are under water. | no |

## Water

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Water Reflections | Water mirrors the world and the sky. | yes |
| [ ] | Ice Reflections | Ice mirrors the world and the sky. | no (needs an ice material) |
| [ ] | Reflection Quality and Strength | Low, Normal or High trace; how strong the reflection is. | yes |
| [ ] | Water Color Mapping | Water colour follows the biome and fades with depth. | yes |
| [ ] | Water Colors (Vanilla / Realistic) | Realistic: deeper colours and lighting under water. | no |
| [ ] | Shore Foam | Foam along the edges of water. | no |
| [ ] | Lava Foam | A glowing crust along the edges of lava. | no |
| [ ] | Enhanced Water (waterfall mist) | Falling water runs white, foams where it lands and throws up mist. | no |
| [ ] | Water Leaves | Leaves float on water near trees. | no |
| [ ] | Underwater Caustics | Rippling light on the ground under water. | part (seen from above only) |
| [ ] | Underwater Depth Blur | Distant things under water look soft and blurred. | no |

## Vegetation

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Moving Grass | Grass and tall grass sway in the wind. | no |
| [ ] | Moving Plants | Flowers, ferns, saplings and crops sway gently. | no |
| [ ] | Moving Leaves | Tree leaves move softly in the wind. | no |
| [ ] | GPU Grass, with density, height and distance | Small pixel-art tufts on grass blocks near you that sway and take light. | no |

## Particles

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Particle Rate | How many ambient particles appear. | no |
| [ ] | Snow and Sand Particle Style | The look of snow and sand particles. | no |
| [ ] | Fireflies | Glowing fireflies at night. | no |
| [ ] | Specks | Tiny specks of dust in the air, set per biome. | no |
| [ ] | Pollen and Spores | Pollen over flowers, spores in forests and caves. | no |
| [ ] | Falling Leaves | Leaves falling from trees. | no |
| [ ] | Wind Gusts | Streaks of wind passing through the air. | no |

## Weather and Events

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Weather Visuals | The look of rain, snow and storms. | no |
| [ ] | Sand Wind | Sand blowing across deserts. | no |
| [ ] | Sand Storm | Sand storms in deserts during rain. | no |
| [ ] | Always Winter | Every biome looks like winter. | no |
| [ ] | Aurora Borealis | Northern lights in cold biomes at night. | no |
| [ ] | Meteor Shower | Showers of meteors on some nights. | no |
| [ ] | Fireworks | Fireworks in the distance on some nights. | no |

## Outlines

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Block Outlines and their thickness | A contour along block edges. | no |
| [ ] | Mob Outlines and their thickness | A contour around mobs and players. | no |
| [ ] | Outline Style and Colour | Black Dungeon, White Dungeon or a custom RGB colour, with opacity. | no |
| [ ] | Enchantment Outlines | Enchanted items in hand get a glowing contour. | no |
| [ ] | Dynamic Item Outlines | The contour takes its colour from the item. | no |
| [ ] | Enchantment Outline Settings | Intensity, thickness, hue shift and saturation. | no |

## Color Grading

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | Grading Mode (Vanilla / Enhanced) | One grade, or one each for dawn, midday, dusk and night, blended with the sun. | no |
| [ ] | Tone | Exposure, contrast, shadows, midtones, highlights. | part (exposure) |
| [ ] | Color | Saturation, vibrance, hue shift, temperature, tint. | no |
| [ ] | Color Channels | Red, green and blue strength and hue, set separately. | no |
| [ ] | Presets | Vanilla, Medieval, Vintage, Realistic. | no |

## Dimensions

| | Feature | What it does | Have |
|---|---|---|---|
| [ ] | End Color Grading | Tints the picture in the End. | no |
| [ ] | End Distance Fog | Dark fog at the edge of view. | no |
| [ ] | End Particles | Particles, mist and specks in the End. | no |
| [ ] | End Aura | A glowing aura over the End sky. | no |
