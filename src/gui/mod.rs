use crate::renderer::clouds::CloudStatus;

pub mod accountlist;
pub mod advancements;
#[cfg(feature = "asset_download")]
pub mod assetdownload;
pub mod atlas;
pub mod atlas_writes;
pub mod banner_icons;
pub mod block_info;
pub mod book;
pub mod chat;
pub mod clickevent;
#[cfg(feature = "click_gui")]
pub mod clickgui;
pub mod command_block;
pub mod command_history;
pub mod confirmlink;
pub mod container;
pub mod creative;
pub mod credits;
pub mod death;
pub mod dialog;
pub mod disconnected;
pub mod draw;
#[cfg(explore_modrinth)]
pub mod explore_packs;
pub mod focus;
pub mod health;
pub mod hud;
#[cfg(feature = "hud_editor")]
pub mod hud_editor;
pub mod hud_layout;
pub mod keybinds;
pub mod menu;
pub mod multiplayer;
pub mod options;
pub mod painter;
pub mod pause;
pub mod ping;
#[cfg(feature = "skins")]
pub mod player_faces;
pub mod pool;
#[cfg(test)]
pub mod preview;
pub mod profile;
pub mod render;
#[cfg(resource_packs)]
pub mod resourcepacks;
pub mod screens;
pub mod serverlist;
#[cfg(feature = "shader_support")]
pub mod shaderpacks;
pub mod sign_edit;
pub mod signedchat;
pub mod sleep;
pub mod slots;
pub mod spectator_menu;
pub mod suggestions;
pub mod tablist;
pub mod title;
#[cfg(voice)]
pub mod voice_chat;
pub mod toast;
pub mod tooltip;
pub mod widgets;

use bevy::prelude::*;

use crate::session::{ContainerKind, Gamemode, SlotStack};
pub use render::{GuiInput, GuiPlugin};

#[inline]
pub fn keyboard_elsewhere() -> bool {
    false
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    #[default]
    None,
    Title,
    #[cfg(feature = "asset_download")]
    AssetDownload,
    Accounts,
    EditProfile,
    Credits,
    #[cfg(feature = "online_mode")]
    AddAccount,
    #[cfg(feature = "online_mode")]
    MicrosoftLogin,
    Multiplayer,
    ManageServer,
    DirectConnect,
    Disconnected,
    Pause,
    Options,
    VideoSettings,
    Controls,
    MouseSettings,
    /// Signed chat and toasts.
    ChatSettings,
    /// The skin's layers and the main hand (vanilla's
    /// `SkinCustomizationScreen`).
    SkinCustomisation,
    ManageStorage,
    #[cfg(audio)]
    AudioSettings,
    /// The block information panel's settings.
    BlockInfoSettings,
    /// Each voice chat player's volume (Simple Voice Chat's
    /// `AdjustVolumesScreen`).
    #[cfg(voice)]
    VoicePlayers,
    /// Simple Voice Chat's settings.
    #[cfg(voice)]
    VoiceChat,
    /// Simple Voice Chat's groups: join, create, leave.
    #[cfg(voice)]
    VoiceGroups,
    #[cfg(resource_packs)]
    ResourcePacks,
    #[cfg(explore_modrinth)]
    ExplorePacks,
    #[cfg(feature = "shader_support")]
    ShaderPacks,
    #[cfg(feature = "shader_support")]
    ShaderOptions,
    Dialog,
    Advancements,

    Inventory,
    Creative,

    Container(ContainerKind),

    CommandBlock,
    SignEdit,
    BookView,
    BookEdit,
    BookSign,
    Chat,
    Sleep,
    Death,
    SignedChatPrompt,

    #[cfg(feature = "click_gui")]
    ClickGui,

    #[cfg(feature = "hud_editor")]
    HudEditor,
}

impl Screen {
    pub fn is_open(self) -> bool {
        self != Screen::None
    }

    pub fn is_modal(self) -> bool {
        matches!(self, Screen::Inventory | Screen::Creative) || self.is_container()
    }

    pub fn is_container(self) -> bool {
        matches!(self, Screen::Container(_))
    }

    pub fn is_menu(self) -> bool {
        if matches!(
            self,
            Screen::Title
                | Screen::Multiplayer
                | Screen::ManageServer
                | Screen::DirectConnect
                | Screen::Accounts
                | Screen::EditProfile
                | Screen::Credits
                | Screen::Disconnected
        ) {
            return true;
        }
        #[cfg(feature = "asset_download")]
        if self == Screen::AssetDownload {
            return true;
        }
        #[cfg(feature = "online_mode")]
        {
            matches!(self, Screen::AddAccount | Screen::MicrosoftLogin)
        }
        #[cfg(not(feature = "online_mode"))]
        {
            false
        }
    }

    pub fn is_local_overlay(self) -> bool {
        if matches!(self, Screen::Dialog | Screen::Advancements) {
            return true;
        }
        #[cfg(feature = "hud_editor")]
        if self == Screen::HudEditor {
            return true;
        }
        #[cfg(feature = "click_gui")]
        {
            self == Screen::ClickGui
        }
        #[cfg(not(feature = "click_gui"))]
        {
            false
        }
    }

    pub fn is_options(self) -> bool {
        #[cfg(feature = "shader_support")]
        if matches!(self, Screen::ShaderPacks | Screen::ShaderOptions) {
            return true;
        }
        #[cfg(audio)]
        if self == Screen::AudioSettings {
            return true;
        }
        #[cfg(voice)]
        if matches!(self, Screen::VoicePlayers | Screen::VoiceChat | Screen::VoiceGroups) {
            return true;
        }
        #[cfg(resource_packs)]
        if self == Screen::ResourcePacks {
            return true;
        }
        #[cfg(explore_modrinth)]
        if self == Screen::ExplorePacks {
            return true;
        }
        matches!(
            self,
            Screen::Options
                | Screen::BlockInfoSettings
                | Screen::VideoSettings
                | Screen::Controls
                | Screen::MouseSettings
                | Screen::ChatSettings
                | Screen::SkinCustomisation
                | Screen::ManageStorage
        )
    }

    pub fn tab_navigates(self) -> bool {
        if self.is_options() {
            return true;
        }
        #[cfg(feature = "asset_download")]
        if self == Screen::AssetDownload {
            return true;
        }
        #[cfg(feature = "online_mode")]
        if matches!(self, Screen::AddAccount | Screen::MicrosoftLogin) {
            return true;
        }
        matches!(
            self,
            Screen::Container(ContainerKind::Lectern)
                | Screen::Title
                | Screen::Pause
                | Screen::Multiplayer
                | Screen::ManageServer
                | Screen::DirectConnect
                | Screen::Accounts
                | Screen::EditProfile
                | Screen::Credits
                | Screen::Disconnected
                | Screen::Death
                | Screen::SignedChatPrompt
                | Screen::Dialog
                | Screen::Advancements
        )
    }
}

/// The graphics API the renderer starts on. Read once at startup, so a change
/// applies on the next launch.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RenderBackend {
    /// wgpu's own pick (or `WGPU_BACKEND`), as before the setting existed.
    #[default]
    Auto,
    Vulkan,
}

impl RenderBackend {
    pub const ALL: [RenderBackend; 2] = [RenderBackend::Auto, RenderBackend::Vulkan];

    pub fn caption(self) -> &'static str {
        match self {
            RenderBackend::Auto => "Auto",
            RenderBackend::Vulkan => "Vulkan",
        }
    }

    pub fn serialized_name(self) -> &'static str {
        match self {
            RenderBackend::Auto => "auto",
            RenderBackend::Vulkan => "vulkan",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<RenderBackend> {
        RenderBackend::ALL
            .into_iter()
            .find(|b| b.serialized_name() == name)
    }

    pub fn next(self) -> RenderBackend {
        let i = RenderBackend::ALL
            .iter()
            .position(|b| *b == self)
            .unwrap_or(0);
        RenderBackend::ALL[(i + 1) % RenderBackend::ALL.len()]
    }

    /// The setting as saved on disk, without building the other options.
    pub fn stored() -> RenderBackend {
        load_options()
            .as_ref()
            .and_then(|json| json.get("renderBackend"))
            .and_then(|v| v.as_str())
            .and_then(RenderBackend::from_serialized_name)
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TouchMovement {
    #[default]
    Buttons,
    Joystick,
    PadStick,
}

impl TouchMovement {
    pub const ALL: [TouchMovement; 3] = [
        TouchMovement::Buttons,
        TouchMovement::Joystick,
        TouchMovement::PadStick,
    ];

    pub fn caption(self) -> &'static str {
        match self {
            TouchMovement::Buttons => "Buttons",
            TouchMovement::Joystick => "Joystick",
            TouchMovement::PadStick => "Pad Joystick",
        }
    }

    pub fn serialized_name(self) -> &'static str {
        match self {
            TouchMovement::Buttons => "buttons",
            TouchMovement::Joystick => "joystick",
            TouchMovement::PadStick => "pad_joystick",
        }
    }

    pub fn from_serialized_name(name: &str) -> Option<TouchMovement> {
        TouchMovement::ALL
            .into_iter()
            .find(|m| m.serialized_name() == name)
    }

    pub fn next(self) -> TouchMovement {
        let i = TouchMovement::ALL
            .iter()
            .position(|m| *m == self)
            .unwrap_or(0);
        TouchMovement::ALL[(i + 1) % TouchMovement::ALL.len()]
    }

    pub fn has_stick(self) -> bool {
        !matches!(self, TouchMovement::Buttons)
    }
}

#[derive(Clone, Debug)]
pub struct GuiOptions {
    pub gui_scale: u32,
    pub fov: u32,
    pub fov_effects: u32,
    pub clouds: crate::renderer::clouds::CloudStatus,
    pub particles: crate::util::particles::ParticleStatus,
    pub render_distance: u32,
    pub gamma: u32,
    pub antialiasing: crate::renderer::antialias::Antialiasing,
    pub smooth_lighting: bool,
    pub lighting_enabled: bool,
    pub max_fps: u32,
    pub vsync: bool,
    pub render_backend: RenderBackend,
    pub auto_jump: bool,
    pub view_bobbing: bool,
    /// Vanilla `mouseSensitivity` as a percent: 0..=200, 100 is the default.
    pub mouse_sensitivity: u32,
    pub invert_x: bool,
    pub invert_y: bool,
    /// Vanilla `mouseWheelSensitivity` in hundredths: 1..=1000.
    pub scroll_sensitivity: u32,
    pub discrete_scrolling: bool,
    pub toggle_crouch: bool,
    pub toggle_sprint: bool,
    pub main_hand_left: bool,
    #[cfg(feature = "skins")]
    pub skin_parts: u8,
    pub fps_counter: bool,
    pub shader_quality: crate::renderer::terrain::ShaderQuality,
    pub shaders_enabled: bool,
    pub player_shadows: bool,
    pub touch_movement: TouchMovement,
    pub allow_signed_chat: bool,
    pub toast_advancement: bool,
    pub toast_recipe: bool,
    pub toast_system: bool,
    pub toast_time: u32,
    #[cfg(audio)]
    pub volumes: [u32; crate::audio::category::SoundCategory::COUNT],
    #[cfg(voice)]
    pub voice: VoiceOptions,
    /// The block information panel's settings.
    pub block_info: block_info::BlockInfoOptions,
    /// Vanilla's `soundDevice`: the speaker by name, `None` for the default.
    #[cfg(audio)]
    pub sound_device: Option<String>,
}

#[cfg(audio)]
pub const VOLUME_DEFAULT: u32 = 100;

/// Simple Voice Chat's client settings this client keeps, with its defaults.
#[cfg(voice)]
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceOptions {
    /// Voice chat turned off (the mod's disable toggle).
    pub disabled: bool,
    pub muted: bool,
    /// Voice activation instead of push-to-talk.
    pub voice_activation: bool,
    /// `voice_activation_threshold`, dB.
    pub threshold: i32,
    /// `voice_chat_volume`, as a percentage (0 to 300).
    pub volume: u32,
    /// The microphone by name, `None` for the default.
    pub input_device: Option<String>,
    /// How the HUD shows voice chat's state.
    pub status: VoiceStatusDisplay,
    /// Each player's voice volume as a percentage (0 to 400), with the name
    /// they had, so a player offline still shows in the list.
    pub player_volumes: std::collections::BTreeMap<uuid::Uuid, (String, u32)>,
}

/// How the HUD shows voice chat's state.
#[cfg(voice)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceStatusDisplay {
    Off,
    /// A line of text, bottom left.
    Text,
    /// A small square in the corner: green talking, grey idle, red muted.
    Icon,
}

#[cfg(voice)]
impl VoiceStatusDisplay {
    pub fn caption(self) -> &'static str {
        match self {
            VoiceStatusDisplay::Off => "Off",
            VoiceStatusDisplay::Text => "Text",
            VoiceStatusDisplay::Icon => "Icon",
        }
    }

    pub fn next(self) -> VoiceStatusDisplay {
        match self {
            VoiceStatusDisplay::Off => VoiceStatusDisplay::Text,
            VoiceStatusDisplay::Text => VoiceStatusDisplay::Icon,
            VoiceStatusDisplay::Icon => VoiceStatusDisplay::Off,
        }
    }

    fn key(self) -> &'static str {
        match self {
            VoiceStatusDisplay::Off => "off",
            VoiceStatusDisplay::Text => "text",
            VoiceStatusDisplay::Icon => "icon",
        }
    }

    fn from_key(key: &str) -> VoiceStatusDisplay {
        match key {
            "off" => VoiceStatusDisplay::Off,
            "text" => VoiceStatusDisplay::Text,
            _ => VoiceStatusDisplay::Icon,
        }
    }
}

#[cfg(voice)]
impl VoiceOptions {
    /// Hands the settings to the voice module.
    pub fn apply(&self) {
        crate::voice::set_disabled(self.disabled);
        crate::voice::set_muted(self.muted);
        crate::voice::set_activation(if self.voice_activation {
            crate::voice::Activation::Voice
        } else {
            crate::voice::Activation::PushToTalk
        });
        crate::voice::set_threshold(self.threshold as f32);
        crate::voice::set_volume(self.volume as f32 / 100.0);
        crate::voice::set_input_device(self.input_device.clone());
        crate::voice::set_player_volumes(
            self.player_volumes
                .iter()
                .map(|(id, (_, percent))| (*id, *percent as f32 / 100.0))
                .collect(),
        );
    }
}

pub const TOAST_TIME_DEFAULT: u32 = 25;

pub const TOAST_TIME_MIN: i32 = 5;
pub const TOAST_TIME_MAX: i32 = 100;

pub const AUTO_JUMP_DEFAULT: bool = cfg!(feature = "mobile_ui");

pub const FPS_COUNTER_DEFAULT: bool = true;

pub const MOUSE_SENSITIVITY_DEFAULT: u32 = 100;
pub const MOUSE_SENSITIVITY_MAX: u32 = 200;
pub const SCROLL_SENSITIVITY_DEFAULT: u32 = 100;
pub const SCROLL_SENSITIVITY_MIN: u32 = 1;
pub const SCROLL_SENSITIVITY_MAX: u32 = 1000;

impl GuiOptions {
    /// Degrees of turn per pixel of mouse motion: vanilla's
    /// `(s * 0.6 + 0.2)^3 * 8 * 0.15`, signed by the invert options.
    pub fn look_scale(&self) -> Vec2 {
        let s = self.mouse_sensitivity as f32 / MOUSE_SENSITIVITY_MAX as f32;
        let f = s * 0.6 + 0.2;
        let k = f * f * f * 8.0 * crate::renderer::input::LOOK_SCALE;
        let sign = |invert: bool| if invert { -1.0 } else { 1.0 };
        Vec2::new(k * sign(self.invert_x), k * sign(self.invert_y))
    }

    /// Wheel notches after scroll sensitivity, or one per event when
    /// scrolling is discrete.
    pub fn scroll(&self, notches: Vec2) -> Vec2 {
        if self.discrete_scrolling {
            notches.signum() * notches.abs().ceil().min(Vec2::ONE)
        } else {
            notches * (self.scroll_sensitivity as f32 / 100.0)
        }
    }
}

pub const MAX_FPS_MIN: u32 = 10;
pub const MAX_FPS_UNLIMITED: u32 = 260;
pub const MAX_FPS_DEFAULT: u32 = if cfg!(feature = "mobile_ui") { 30 } else { 60 };

pub const VSYNC_DEFAULT: bool = !cfg!(target_os = "android");

pub const RENDER_DISTANCE_DEFAULT: u32 = if cfg!(feature = "mobile_ui") { 2 } else { 8 };

pub const SMOOTH_LIGHTING_DEFAULT: bool = !cfg!(feature = "mobile_ui");

pub const LIGHTING_ENABLED_DEFAULT: bool = !cfg!(feature = "mobile_ui");

pub(crate) const GAMMA_DEFAULT: u32 = 50;

pub const GAMMA_MAX: u32 = 150;

pub const FOV_EFFECTS_DEFAULT: u32 = 100;

#[cfg(feature = "skins")]
pub const MODEL_PART_KEYS: [(&str, u8); 7] = [
    ("modelPart_cape", 0),
    ("modelPart_jacket", 1),
    ("modelPart_left_sleeve", 2),
    ("modelPart_right_sleeve", 3),
    ("modelPart_left_pants_leg", 4),
    ("modelPart_right_pants_leg", 5),
    ("modelPart_hat", 6),
];

impl Default for GuiOptions {
    fn default() -> Self {
        let stored = load_options();
        let value = |key: &str| stored.as_ref().and_then(|json| json.get(key).cloned());
        GuiOptions {
            gui_scale: value("guiScale")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(0),
            fov: value("fov")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(crate::renderer::systems::VANILLA_FOV_DEGREES as u32),
            fov_effects: value("fovEffectScale")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(FOV_EFFECTS_DEFAULT)
                .min(FOV_EFFECTS_DEFAULT),
            clouds: value("renderClouds")
                .and_then(|v| v.as_str().and_then(CloudStatus::from_serialized_name))
                .unwrap_or(if cfg!(feature = "mobile_ui") {
                    CloudStatus::Fast
                } else {
                    CloudStatus::default()
                }),
            particles: value("particles")
                .and_then(|v| {
                    v.as_str()
                        .and_then(crate::util::particles::ParticleStatus::from_serialized_name)
                })
                .unwrap_or(if cfg!(feature = "mobile_ui") {
                    crate::util::particles::ParticleStatus::Decreased
                } else {
                    crate::util::particles::ParticleStatus::All
                }),
            render_distance: value("renderDistance")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(RENDER_DISTANCE_DEFAULT),
            gamma: value("gamma")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(GAMMA_DEFAULT)
                .min(GAMMA_MAX),
            antialiasing: crate::renderer::antialias::Antialiasing::read(value),
            smooth_lighting: value("ao")
                .and_then(|v| v.as_bool())
                .unwrap_or(SMOOTH_LIGHTING_DEFAULT),
            lighting_enabled: value("lightingEnabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(LIGHTING_ENABLED_DEFAULT),
            max_fps: value("maxFps")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .unwrap_or(MAX_FPS_DEFAULT)
                .clamp(MAX_FPS_MIN, MAX_FPS_UNLIMITED),
            vsync: value("vsync")
                .and_then(|v| v.as_bool())
                .unwrap_or(VSYNC_DEFAULT),
            render_backend: value("renderBackend")
                .and_then(|v| v.as_str().and_then(RenderBackend::from_serialized_name))
                .unwrap_or_default(),
            view_bobbing: value("bobView").and_then(|v| v.as_bool()).unwrap_or(true),
            auto_jump: value("autoJump")
                .and_then(|v| v.as_bool())
                .unwrap_or(AUTO_JUMP_DEFAULT),
            mouse_sensitivity: value("mouseSensitivity")
                .and_then(|v| v.as_f64())
                .map(|v| (v.clamp(0.0, 1.0) * MOUSE_SENSITIVITY_MAX as f64).round() as u32)
                .unwrap_or(MOUSE_SENSITIVITY_DEFAULT),
            invert_x: value("invertXMouse")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            invert_y: value("invertYMouse")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            scroll_sensitivity: value("mouseWheelSensitivity")
                .and_then(|v| v.as_f64())
                .map(|v| (v * 100.0).round() as u32)
                .unwrap_or(SCROLL_SENSITIVITY_DEFAULT)
                .clamp(SCROLL_SENSITIVITY_MIN, SCROLL_SENSITIVITY_MAX),
            discrete_scrolling: value("discrete_mouse_scroll")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            toggle_crouch: value("toggleCrouch")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            toggle_sprint: value("toggleSprint")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            main_hand_left: value("mainHand").and_then(|v| v.as_str().map(|s| s == "left"))
                == Some(true),
            #[cfg(feature = "skins")]
            skin_parts: {
                let mut parts = 0u8;
                for (key, bit) in MODEL_PART_KEYS {
                    if value(key).and_then(|v| v.as_bool()).unwrap_or(true) {
                        parts |= 1 << bit;
                    }
                }
                parts
            },
            fps_counter: value("fpsCounter")
                .and_then(|v| v.as_bool())
                .unwrap_or(FPS_COUNTER_DEFAULT),
            shader_quality: value("shaderQuality")
                .and_then(|v| {
                    v.as_str()
                        .and_then(crate::renderer::terrain::ShaderQuality::from_serialized_name)
                })
                .unwrap_or_default(),
            shaders_enabled: value("shaders").and_then(|v| v.as_bool()).unwrap_or(false),
            player_shadows: value("playerShadows")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            touch_movement: value("touchMovement")
                .and_then(|v| v.as_str().and_then(TouchMovement::from_serialized_name))
                .unwrap_or_default(),
            allow_signed_chat: value("allowSignedChat")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            toast_advancement: value("toastAdvancement")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            toast_recipe: value("toastRecipe")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            toast_system: value("toastSystem")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            toast_time: value("toastDisplayTime")
                .and_then(|v| v.as_f64())
                .map(|v| (v * 10.0).round() as u32)
                .unwrap_or(TOAST_TIME_DEFAULT)
                .clamp(TOAST_TIME_MIN as u32, TOAST_TIME_MAX as u32),
            #[cfg(voice)]
            voice: VoiceOptions {
                disabled: value("voiceDisabled").and_then(|v| v.as_bool()).unwrap_or(false),
                muted: value("voiceMuted").and_then(|v| v.as_bool()).unwrap_or(false),
                voice_activation: value("voiceActivation")
                    .and_then(|v| v.as_str().map(|s| s == "voice"))
                    .unwrap_or(false),
                threshold: value("voiceThreshold")
                    .and_then(|v| v.as_i64())
                    .map_or(-50, |v| v.clamp(-127, 0) as i32),
                volume: value("voiceVolume")
                    .and_then(|v| v.as_u64())
                    .map_or(100, |v| v.min(300) as u32),
                input_device: value("voiceInputDevice")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .filter(|s| !s.is_empty()),
                status: VoiceStatusDisplay::from_key(
                    value("voiceStatus").and_then(|v| v.as_str().map(str::to_owned)).as_deref().unwrap_or("icon"),
                ),
                player_volumes: value("voicePlayerVolumes")
                    .and_then(|v| {
                        let map = v.as_object()?;
                        Some(
                            map.iter()
                                .filter_map(|(id, entry)| {
                                    let id = uuid::Uuid::parse_str(id).ok()?;
                                    let name = entry.get("name")?.as_str()?.to_owned();
                                    let volume = entry.get("volume")?.as_u64()?.min(400) as u32;
                                    Some((id, (name, volume)))
                                })
                                .collect(),
                        )
                    })
                    .unwrap_or_default(),
            },
            block_info: block_info::BlockInfoOptions::load(value("blockInfo").as_ref()),
            #[cfg(audio)]
            sound_device: value("soundDevice")
                .and_then(|v| v.as_str().map(str::to_owned))
                .filter(|s| !s.is_empty()),
            #[cfg(audio)]
            volumes: {
                let mut volumes = [VOLUME_DEFAULT; crate::audio::category::SoundCategory::COUNT];
                for category in crate::audio::category::SoundCategory::ALL {
                    volumes[category.index()] = value(&format!("soundCategory_{}", category.key()))
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32)
                        .unwrap_or(VOLUME_DEFAULT)
                        .min(100);
                }
                volumes
            },
        }
    }
}

fn load_options() -> Option<serde_json::Value> {
    let text = crate::platform::storage::OPTIONS.load()?;
    serde_json::from_str(&text).ok()
}

pub fn save_options(options: &GuiOptions) {
    let mut json = serde_json::json!({
        "guiScale": options.gui_scale,
        "fov": options.fov,
        "fovEffectScale": options.fov_effects,
        "renderClouds": options.clouds.serialized_name(),
        "particles": options.particles.serialized_name(),
        "renderDistance": options.render_distance,
        "gamma": options.gamma,
        "ao": options.smooth_lighting,
        "lightingEnabled": options.lighting_enabled,
        "maxFps": options.max_fps,
        "vsync": options.vsync,
        "renderBackend": options.render_backend.serialized_name(),
        "autoJump": options.auto_jump,
        "bobView": options.view_bobbing,
        "mouseSensitivity": options.mouse_sensitivity as f64 / MOUSE_SENSITIVITY_MAX as f64,
        "invertXMouse": options.invert_x,
        "invertYMouse": options.invert_y,
        "mouseWheelSensitivity": options.scroll_sensitivity as f64 / 100.0,
        "discrete_mouse_scroll": options.discrete_scrolling,
        "toggleCrouch": options.toggle_crouch,
        "toggleSprint": options.toggle_sprint,
        "mainHand": if options.main_hand_left { "left" } else { "right" },
        "fpsCounter": options.fps_counter,
        "shaders": options.shaders_enabled,
        "shaderQuality": options.shader_quality.serialized_name(),
        "playerShadows": options.player_shadows,
        "touchMovement": options.touch_movement.serialized_name(),
        "allowSignedChat": options.allow_signed_chat,
        "toastAdvancement": options.toast_advancement,
        "toastRecipe": options.toast_recipe,
        "toastSystem": options.toast_system,
        "toastDisplayTime": options.toast_time as f64 / 10.0,
    });
    #[cfg(feature = "skins")]
    {
        let map = json.as_object_mut().expect("json! object literal");
        for (key, bit) in MODEL_PART_KEYS {
            map.insert(
                key.to_string(),
                serde_json::json!(options.skin_parts & (1 << bit) != 0),
            );
        }
    }
    #[cfg(voice)]
    {
        let map = json.as_object_mut().expect("json! object literal");
        let voice = &options.voice;
        map.insert("voiceDisabled".into(), serde_json::json!(voice.disabled));
        map.insert("voiceMuted".into(), serde_json::json!(voice.muted));
        map.insert(
            "voiceActivation".into(),
            serde_json::json!(if voice.voice_activation { "voice" } else { "ptt" }),
        );
        map.insert("voiceThreshold".into(), serde_json::json!(voice.threshold));
        map.insert("voiceVolume".into(), serde_json::json!(voice.volume));
        map.insert(
            "voiceInputDevice".into(),
            serde_json::json!(voice.input_device.as_deref().unwrap_or("")),
        );
        map.insert("voiceStatus".into(), serde_json::json!(voice.status.key()));
        let volumes: serde_json::Map<String, serde_json::Value> = voice
            .player_volumes
            .iter()
            .map(|(id, (name, volume))| {
                (
                    id.to_string(),
                    serde_json::json!({ "name": name, "volume": volume }),
                )
            })
            .collect();
        map.insert("voicePlayerVolumes".into(), serde_json::Value::Object(volumes));
    }
    options
        .antialiasing
        .write(json.as_object_mut().expect("json! object literal"));
    json.as_object_mut()
        .expect("json! object literal")
        .insert("blockInfo".into(), options.block_info.save());
    #[cfg(audio)]
    {
        let map = json.as_object_mut().expect("json! object literal");
        map.insert(
            "soundDevice".into(),
            serde_json::json!(options.sound_device.as_deref().unwrap_or("")),
        );
    }
    #[cfg(audio)]
    {
        let map = json.as_object_mut().expect("json! object literal");
        for category in crate::audio::category::SoundCategory::ALL {
            map.insert(
                format!("soundCategory_{}", category.key()),
                serde_json::json!(options.volumes[category.index()]),
            );
        }
    }
    crate::platform::storage::OPTIONS.store(&json.to_string());
}

#[derive(Resource, Default)]
pub struct GuiState {
    pub screen: Screen,
    pub creative: creative::CreativeState,
    pub chat: chat::ChatState,
    pub sign_edit: sign_edit::SignEditState,
    pub command_block: command_block::CommandBlockState,
    pub book: book::BookState,
    pub dialog: dialog::DialogState,
    pub advanced_tooltips: bool,
    pub tab_list_held: bool,
    pub tablist: tablist::TabListState,
    pub hearts: health::HeartAnim,
    pub chunk_borders: bool,
    pub hide_gui: bool,
    pub slots: slots::SlotState,
    pub container: container::ContainerUi,
    #[cfg(feature = "click_gui")]
    pub clickgui: clickgui::ClickGuiState,
    #[cfg(feature = "click_gui")]
    pub clickgui_parent: Screen,
    pub hud_state: hud_layout::HudState,
    pub options: GuiOptions,
    pub keybinds: keybinds::KeyBinds,
    pub controls_scroll: f32,
    pub controls_scroll_drag: Option<f32>,
    pub nav: Option<Screen>,
    pub suppress_click: bool,
    pub death: death::DeathState,
    pub advancements: advancements::AdvancementsUi,
    pub quit: bool,
    pub disconnect: bool,
    pub menu: multiplayer::MenuState,
    pub profile: profile::ProfileState,
    pub video_scroll: f32,
    pub video_scroll_drag: Option<f32>,
    pub game_scroll: f32,
    pub game_scroll_drag: Option<f32>,
    #[cfg(voice)]
    pub voice_scroll: f32,
    #[cfg(voice)]
    pub voice_scroll_drag: Option<f32>,
    #[cfg(voice)]
    pub voice_groups: voice_chat::GroupsState,
    #[cfg(all(target_arch = "wasm32", feature = "asset_download"))]
    pub confirm_clear_assets: bool,
    #[cfg(feature = "asset_download")]
    pub storage_nothing_missing: bool,
    #[cfg(feature = "shader_support")]
    pub shaderpacks: shaderpacks::State,
    #[cfg(resource_packs)]
    pub resourcepacks: resourcepacks::State,
    #[cfg(explore_modrinth)]
    pub explore_packs: explore_packs::State,
    pub options_parent: Screen,
    pub disconnect_reason: Option<Vec<crate::text::Span>>,
    pub connect: Option<String>,
    pub hud: hud::HudInfo,
    pub hud_overlays: hud::HudOverlays,
    pub toasts: toast::Toasts,
    pub spectator_menu: spectator_menu::SpectatorMenuState,
    #[cfg(feature = "mobile_ui")]
    pub mobile: crate::mobile::MobileUi,
}

impl GuiState {
    pub fn anvil_typing(&self) -> bool {
        self.screen == Screen::Container(ContainerKind::Anvil) && self.container.anvil.editable()
    }

    pub fn in_menu(&self) -> bool {
        if self.screen.is_menu() || (self.screen.is_options() && self.options_parent.is_menu()) {
            return true;
        }
        if self.screen == Screen::Dialog && self.dialog.parent().is_menu() {
            return true;
        }
        #[cfg(feature = "hud_editor")]
        if self.screen == Screen::HudEditor && self.hud_state.over_menu {
            return true;
        }
        #[cfg(feature = "click_gui")]
        {
            self.screen == Screen::ClickGui && self.clickgui_parent.is_menu()
        }
        #[cfg(not(feature = "click_gui"))]
        {
            false
        }
    }
}

#[derive(Default)]
pub struct Snapshot {
    pub action_bar: Option<Vec<crate::text::Span>>,
    pub title_text: Option<Vec<crate::text::Span>>,
    pub subtitle_text: Option<Vec<crate::text::Span>>,
    pub title_times: Option<(Option<u32>, Option<u32>, Option<u32>)>,
    pub titles_clear: Option<bool>,
    pub menu_slots: Vec<SlotStack>,
    pub attack_strength: f32,
    pub attack_delay: f32,
    pub targeted_entity: bool,
    pub hotbar: std::sync::Arc<[SlotStack]>,
    pub carried: SlotStack,
    pub selected: u8,
    pub gamemode: Gamemode,
    pub health: f32,
    pub food: u32,
    pub saturation: f32,
    pub health_display: crate::session::HealthDisplay,
    pub hardcore: bool,
    pub health_epoch: u32,
    pub jump_charge: Option<f32>,
    pub air_supply: i32,
    pub eye_in_water: bool,
    pub chat_incoming: Vec<crate::session::ChatEntry>,
    pub toast_incoming: Vec<toast::ToastEvent>,
    pub chat_error: Option<String>,
    pub enforces_secure_chat: bool,
    pub chat_signing: bool,
    pub container_title: std::sync::Arc<[crate::text::Span]>,
    pub container_id: i32,
    pub container_data: crate::session::ContainerData,
    pub enchant_clues: [Option<Box<str>>; 3],
    pub merchant: Option<std::sync::Arc<crate::session::MerchantOffers>>,
    pub merchant_future_xp: u32,
    pub stonecutter_recipes: std::sync::Arc<[crate::session::StonecutterRecipe]>,
    pub advancements: Option<std::sync::Arc<crate::client::advancements::AdvancementTree>>,
    pub advancement_tab: Option<u32>,
    pub active_effects: Vec<crate::play::mob_effects::MobEffectInstance>,
    pub xp_progress: f32,
    pub xp_level: u32,
    pub xp_recent_gain: bool,
    pub waypoints: std::sync::Arc<Vec<crate::session::WaypointInfo>>,
    pub eye_pos: [f32; 3],
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    pub sleep_timer: u32,
    pub tab_list: std::sync::Arc<crate::client::tablist::TabList>,
    pub sidebar: std::sync::Arc<crate::client::tablist::Sidebar>,
    pub boss_bars: std::sync::Arc<Vec<crate::client::bossbar::BossBar>>,
    #[allow(
        dead_code,
        reason = "published session state the sleep screen does not read yet"
    )]
    pub sleeping: bool,
}

impl Snapshot {
    pub fn is_creative(&self) -> bool {
        self.gamemode == Gamemode::Creative
    }

    pub fn hotbar(&self, i: usize) -> &SlotStack {
        static EMPTY: std::sync::LazyLock<SlotStack> = std::sync::LazyLock::new(SlotStack::default);
        self.hotbar.get(i).unwrap_or(&EMPTY)
    }

    pub fn offhand(&self) -> &SlotStack {
        static EMPTY: std::sync::LazyLock<SlotStack> = std::sync::LazyLock::new(SlotStack::default);
        self.hotbar.get(9).unwrap_or(&EMPTY)
    }

    pub fn slot(&self, i: usize) -> &SlotStack {
        static EMPTY: std::sync::LazyLock<SlotStack> = std::sync::LazyLock::new(SlotStack::default);
        self.menu_slots.get(i).unwrap_or(&EMPTY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(item: &'static str) -> SlotStack {
        SlotStack {
            item,
            count: 1,
            ..Default::default()
        }
    }

    #[test]
    fn hotbar_and_offhand_ignore_menu_slots_shape() {
        let mut menu_slots = vec![SlotStack::default(); 46];
        menu_slots[45] = stack("stone");
        let mut hotbar = vec![SlotStack::default(); 10];
        hotbar[3] = stack("diamond_pickaxe");
        hotbar[9] = stack("shield");

        let snap = Snapshot {
            menu_slots,
            hotbar: hotbar.into(),
            ..Default::default()
        };

        assert_eq!(snap.hotbar(3).item, "diamond_pickaxe");
        assert_eq!(snap.offhand().item, "shield");
        assert!(snap.hotbar(0).is_empty());
    }
}

pub struct ScreenCtx<'a> {
    pub input: &'a GuiInput,
    pub vw: f32,
    pub vh: f32,
}

impl ScreenCtx<'_> {
    pub fn mouse(&self) -> Option<Vec2> {
        self.input.mouse
    }

    pub fn hovering(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        match self.input.mouse {
            Some(m) => m.x >= x && m.x < x + w && m.y >= y && m.y < y + h,
            None => false,
        }
    }
}
