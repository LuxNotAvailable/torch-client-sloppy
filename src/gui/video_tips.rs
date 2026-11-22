//! Video Settings tooltips, one per setting. See `setting_tip`.

use crate::gui::setting_tip::{Impact, Tip};

pub const PRESET: Tip = Tip::new(
    "Sets every Performance and Quality setting at once, from Potato PC (integrated graphics) up to I Paid for the Whole GPU. Changing any of them shows Custom.",
    Impact::Varies,
);
pub const MAX_FPS: Tip = Tip::new(
    "Caps how many frames are drawn each second. A cap saves power and heat; Unlimited draws as fast as your GPU can.",
    Impact::Varies,
);
pub const GUI_MAX_FPS: Tip = Tip::new(
    "Caps the framerate while an inventory, chat or menu is open. Never above Max Framerate; menus outside a world stay at 60 or less.",
    Impact::Varies,
);
pub const VSYNC: Tip = Tip::new(
    "Matches frames to your monitor's refresh rate so the picture doesn't tear. Can add a little input delay.",
    Impact::Low,
);
pub const WINDOW_SIZE: Tip = Tip::new("Cycles the window through common sizes.", Impact::None);
pub const FULLSCREEN: Tip = Tip::new("Fills the whole screen with the game.", Impact::None);
pub const GUI_SCALE: Tip = Tip::new(
    "How big menus and the HUD are. Auto picks the largest size that fits the window.",
    Impact::None,
);
pub const BRIGHTNESS: Tip = Tip::new(
    "How bright dark places look, from Moody (vanilla's darkest) to Bright.",
    Impact::None,
);
pub const RENDER_DISTANCE: Tip = Tip::new(
    "How many chunks out the world is drawn. The biggest single cost to frame rate and memory.",
    Impact::High,
);
pub const ENTITY_DISTANCE: Tip = Tip::new(
    "How far away mobs, items and other entities still draw, scaled by their size. 100% is vanilla.",
    Impact::Medium,
);
pub const SIGN_DISTANCE: Tip = Tip::new(
    "How far away sign text still draws. The sign itself always draws. Vanilla is 64 blocks.",
    Impact::Low,
);
pub const TORCH_DISTANCE: Tip = Tip::new(
    "How far away torches still draw. Their light stays. Unlimited is vanilla.",
    Impact::Low,
);
pub const TUNE_SETTINGS: Tip = Tip::new(
    "Times each performance setting in this world and suggests what helps. Gate 1 tries settings that keep the picture; if you're still under 60 fps, Gate 2 goes on through ones that cost looks, mildest first, down to render distance and fullbright. Takes several minutes with input off; you keep or drop each change at the end. In a world only.",
    Impact::None,
);
pub const ADVANCED_PERFORMANCE: Tip = Tip::new(
    "Culling options: what's skipped by facing or distance.",
    Impact::None,
);
pub const CUTOUT_FACE_CULLING: Tip = Tip::new(
    "Skips the far sides of signs, lanterns, leaves and other see-through blocks before they reach the GPU.",
    Impact::Low,
);
pub const ANGLED_FACE_CULLING: Tip = Tip::new(
    "Skips the far sides of grass, flowers, crops and turned signs. Needs Cutout Face Culling.",
    Impact::Low,
);
pub const EXACT_FACE_CULLING: Tip = Tip::new(
    "Skips block faces pointing away from you in the chunks around you too, not just whole chunks behind you.",
    Impact::Low,
);
pub const ENTITY_AWAY_FACES: Tip = Tip::new(
    "Skips the hidden inner sides of solid mob and block entity models. Models with see-through parts keep both sides.",
    Impact::Low,
);
pub const PER_FACE_CULLING: Tip = Tip::new(
    "WARNING: usually makes the game SLOWER. Checks every block face on the GPU each frame and drops the ones facing away from you. The check costs more than it saves: measured slower on a GeForce MX250 laptop and a Pixel 10 Pro. Experimental; leave it off. Off under shader packs.",
    Impact::High,
);
#[cfg(not(target_os = "android"))]
pub const NEAR_FIRST: Tip = Tip::new(
    "WARNING: usually makes the game slightly SLOWER. Draws the chunks nearest you first, so hidden terrain behind them is skipped sooner. Occlusion culling already skips most of it, so the extra sorting costs more than it saves: measured slower on a GeForce MX250 laptop and a Pixel 10 Pro. Experimental; leave it off.",
    Impact::Low,
);
pub const HIZ_EVERY_OTHER: Tip = Tip::new(
    "Rebuilds the depth map that occlusion culling checks every second frame instead of every frame. Saves about 1 ms a frame on phones (measured on a Pixel 10 Pro); no gain on desktop graphics cards. Turning quickly can show a chunk a frame late at the screen edge. On by default on Android.",
    Impact::Medium,
);
pub const HIZ_QUARTER: Tip = Tip::new(
    "Builds the depth map that occlusion culling checks from a quarter of the screen's pixels. Saves about 0.8 ms a frame on phones (0.4 ms with Half-Rate Occlusion; measured on a Pixel 10 Pro); no gain on desktop graphics cards. Not exact: chunks seen only through gaps one pixel wide, like between leaves far away, can vanish. Experimental.",
    Impact::Medium,
);
pub const ENTITY_CULLING: Tip = Tip::new(
    "Skips drawing mobs, items and block entities hidden behind solid blocks. Turning it off costs frames in busy areas; only do so if something vanishes when it shouldn't.",
    Impact::Medium,
);
pub const ENTITY_BACK_FACES: Tip = Tip::new(
    "Hidden skips the inward-facing sides of mob, item and block entity models, saving GPU work on very weak systems. Thin parts such as capes, wings and banners can vanish when seen from behind. Native renderer only.",
    Impact::Low,
);
pub const LIGHTING_ENGINE: Tip = Tip::new(
    "Works out block and sky light. Off is fullbright: everything is evenly lit, a large saving on weak PCs.",
    Impact::Medium,
);
#[cfg(not(target_os = "android"))]
pub const GREEDY_MESHING: Tip = Tip::new(
    "Joins neighbouring block faces into large rectangles, so the GPU draws fewer triangles. Standard joins only faces with the same texture whose lighting the joined face reproduces exactly, so it costs nothing per pixel and should never be slower. Not vanilla: blocks whose texture is turned at random (grass, sand, stone, dirt and others) all face one way within each rectangle, so their random rotation and mirroring are lost. Aggressive (experimental) joins every flat face regardless of texture or lighting, keeping each block's own texture and rotation, for far fewer triangles but more work per pixel: for phones and other GPUs limited by geometry; it can be slower on desktop graphics cards. Leaves and other cut-out faces merge too. Off under shader packs. Changing it reloads the chunks.",
    Impact::Low,
);
pub const RENDER_SCALE: Tip = Tip::new(
    "Draws the world (with or without a shader pack) at a share of the window's resolution, then scales it up; menus stay sharp. Lower is faster but blurrier.",
    Impact::High,
);
pub const SHADER_SHADOWS: Tip = Tip::new(
    "How often a shader pack redraws its shadows. Every 2nd or 3rd frame is faster; moving shadows may trail slightly.",
    Impact::Medium,
);
pub const SMOOTH_LIGHTING: Tip = Tip::new(
    "Soft light and shading across block faces (ambient occlusion). Off gives flat, blocky lighting.",
    Impact::Low,
);
pub const BIOME_BLEND: Tip = Tip::new(
    "How far grass, leaf and water colours blend between biomes. Wider is smoother but slows chunk loading.",
    Impact::Medium,
);
pub const ANTIALIASING: Tip = Tip::new(
    "Smooths jagged edges. FXAA is cheap and slightly soft; MSAA is sharper and costs more; Supersampling draws at a higher resolution and costs the most.",
    Impact::High,
);
pub const AA_LEVEL: Tip = Tip::new(
    "Samples per pixel for MSAA or Supersampling. Higher is smoother and slower.",
    Impact::High,
);
pub const CLOUDS: Tip = Tip::new(
    "Fancy clouds are 3D, Fast clouds are flat, OFF hides them.",
    Impact::Low,
);
pub const PARTICLES: Tip = Tip::new(
    "How many particles appear: rain splashes, smoke, block breaking. Decreased and Minimal skip some.",
    Impact::Medium,
);
pub const LIGHT_RAYS: Tip = Tip::new(
    "Beams of sun and moonlight falling through gaps in leaves and overhangs.",
    Impact::Low,
);
pub const LIGHT_RAY_DAY: Tip = Tip::new(
    "How visible the beams of sunlight are. 0 hides them.",
    Impact::Low,
);
pub const LIGHT_RAY_NIGHT: Tip = Tip::new(
    "How visible the beams of moonlight are. 0 hides them.",
    Impact::Low,
);
pub const LIGHT_RAY_TINT: Tip = Tip::new(
    "How much the beams take on the biome's colour instead of the sun's.",
    Impact::Low,
);
pub const LIGHT_RAY_FREQUENCY: Tip = Tip::new(
    "How often new beams of sunlight appear. Higher shows more at once.",
    Impact::Low,
);
pub const UNDERWATER_LIGHT_RAYS: Tip = Tip::new(
    "Beams of light slanting down through the water while you swim.",
    Impact::Low,
);
pub const UNDERWATER_LIGHT_RAY_OPACITY: Tip = Tip::new(
    "How visible the beams under water are. 0 hides them.",
    Impact::Low,
);
pub const UNDERWATER_LIGHT_RAY_FREQUENCY: Tip = Tip::new(
    "How often new beams appear under water. Higher shows more at once.",
    Impact::Low,
);
pub const LEAVES: Tip = Tip::new(
    "How leaves are drawn: fancy, fast or a mix by distance, and which leaf faces are hidden.",
    Impact::Medium,
);
pub const ENTITY_SHADOWS: Tip = Tip::new("Round shadows under mobs and items.", Impact::Low);
pub const SHADOW_OPACITY: Tip = Tip::new("How dark entity shadows are.", Impact::None);
pub const WATER_CLARITY: Tip = Tip::new(
    "How far you can see through water, from above and below. Vanilla is the default.",
    Impact::None,
);
pub const INVENTORY_MODEL: Tip = Tip::new(
    "Renders the inventory player model at 2x resolution for a smoother, less jagged look.",
    Impact::Low,
);
pub const ITEM_ICONS: Tip = Tip::new(
    "Renders block and model item icons at 2x resolution for smoother edges. Applies after a restart.",
    Impact::Low,
);
pub const VIEW_BOBBING: Tip = Tip::new("Your view sways as you walk.", Impact::None);
pub const FOV_EFFECTS: Tip = Tip::new(
    "How much sprinting, speed and potions widen or narrow your field of view.",
    Impact::None,
);
pub const SIGN_GLOW: Tip = Tip::new(
    "How glowing sign text is outlined: vanilla's outline, a soft shadow, or none.",
    Impact::None,
);
pub const BLOCK_OUTLINE: Tip = Tip::new(
    "The outline around the block you're looking at: its width, colour and opacity.",
    Impact::None,
);
pub const SHADER_PACKS: Tip = Tip::new("Pick and configure shader packs.", Impact::High);
pub const RESET: Tip = Tip::new(
    "Puts every video setting back to its default.",
    Impact::None,
);

pub const LEAF_MODE: Tip = Tip::new(
    "Fancy leaves are see-through; Fast leaves are solid; Hybrid is fancy nearby and fast beyond the Hybrid Distance.",
    Impact::Medium,
);
pub const LEAF_DISTANCE: Tip = Tip::new(
    "Within this many chunks, Hybrid leaves are fancy.",
    Impact::Medium,
);
pub const LEAF_BACKS: Tip = Tip::new(
    "See-through leaves also draw the inside of their faces, so trees look fuller.",
    Impact::Low,
);
pub const LEAF_CULLING: Tip = Tip::new(
    "Hides leaf faces pressed against other leaves. Smart keeps a full outer layer; Full hides the most and is fastest; OFF is vanilla.",
    Impact::Medium,
);
