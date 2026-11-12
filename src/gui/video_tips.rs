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
pub const RENDERER: Tip = Tip::new(
    "The graphics backend. Native is Torch's Vulkan renderer; Legacy is the old fallback. Takes effect after a restart.",
    Impact::None,
);
pub const RENDER_DISTANCE: Tip = Tip::new(
    "How many chunks out the world is drawn. The biggest single cost to frame rate and memory.",
    Impact::High,
);
pub const ENTITY_CULLING: Tip = Tip::new(
    "Skips drawing mobs, items and block entities hidden behind solid blocks. Turning it off costs frames in busy areas; only do so if something vanishes when it shouldn't.",
    Impact::Medium,
);
pub const LIGHTING_ENGINE: Tip = Tip::new(
    "Works out block and sky light. Off is fullbright: everything is evenly lit, a large saving on weak PCs.",
    Impact::Medium,
);
pub const SHADER_SCALE: Tip = Tip::new(
    "Draws the shader pack at a share of the window's resolution, then scales it up. Lower is faster but blurrier.",
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
