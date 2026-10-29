pub mod accountlist;
pub mod advancements;
pub mod appleskin;
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
pub mod form;
pub mod gamemode_switcher;
pub mod health;
pub mod hud;
#[cfg(feature = "hud_editor")]
pub mod hud_editor;
pub mod hud_layout;
pub mod keybinds;
pub mod menu;
pub mod more_options;
pub mod multiplayer;
pub mod op_blocks;
pub mod option_codec;
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
pub mod scroll_list;
pub mod serverlist;
#[cfg(feature = "shader_support")]
pub mod shaderpacks;
pub mod sign_edit;
pub mod signedchat;
pub mod sleep;
pub mod slots;
pub mod social;
pub mod spectator_menu;
pub mod stats;
pub mod suggestions;
pub mod tablist;
pub mod title;
pub mod toast;
pub mod tooltip;
#[cfg(voice)]
pub mod voice_chat;
pub mod widgets;
pub mod win;

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
    /// Torch's own features: zoom, block info, inventory sorting.
    Features,
    /// The zoom key's settings, under Features.
    ZoomSettings,
    /// Inventory sorting's settings, under Features.
    InventorySortSettings,
    /// The end poem and credits, after leaving the End.
    EndPoem,
    /// F3+F4's game mode switcher.
    GameModeSwitcher,
    /// The player's statistics.
    Statistics,
    /// Players on the server, to hide their chat.
    SocialInteractions,
    /// A structure, jigsaw, test or test instance block's editor.
    OpBlock,
    Language,
    FontSettings,
    AccessibilitySettings,
    OnlineSettings,
    DebugOptions,
    /// AppleSkin's settings, under Features.
    AppleSkinSettings,
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

/// Each settings screen's parent, as one table: where Escape and Done go
/// back to. Options' own parent changes (the title screen or the pause
/// menu) and is kept in `GuiState::options_parent`.
macro_rules! settings_tree {
    ($( $(#[$m:meta])* $($from:ident)|+ => $to:ident ),* $(,)?) => {
        impl Screen {
            /// The screen this settings screen backs out to.
            pub fn settings_parent(self) -> Option<Screen> {
                match self {
                    $( $(#[$m])* $(Screen::$from)|+ => Some(Screen::$to), )*
                    _ => None,
                }
            }
        }
    };
}

settings_tree! {
    VideoSettings | Controls | ChatSettings | SkinCustomisation | ManageStorage | Features => Options,
    Language | AccessibilitySettings | OnlineSettings | DebugOptions => Options,
    MouseSettings => Controls,
    ZoomSettings | BlockInfoSettings | InventorySortSettings | AppleSkinSettings => Features,
    FontSettings => Language,
    #[cfg(audio)]
    AudioSettings => Options,
    #[cfg(resource_packs)]
    ResourcePacks => Options,
    #[cfg(explore_modrinth)]
    ExplorePacks => ResourcePacks,
    #[cfg(feature = "shader_support")]
    ShaderPacks => VideoSettings,
    #[cfg(feature = "shader_support")]
    ShaderOptions => ShaderPacks,
    #[cfg(voice)]
    VoiceChat => Options,
    #[cfg(voice)]
    VoicePlayers | VoiceGroups => VoiceChat,
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

    /// An options screen: Options itself, or anything under it.
    pub fn is_options(self) -> bool {
        self == Screen::Options || self.settings_parent().is_some()
    }

    /// Whether the chat box is up: in chat, and in bed (vanilla's
    /// `InBedChatScreen` is the chat screen with a Leave Bed button).
    pub fn chat_open(self) -> bool {
        matches!(self, Screen::Chat | Screen::Sleep)
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

crate::serialized_option!(
    crate::renderer::clouds::CloudStatus,
    crate::util::particles::ParticleStatus,
    RenderBackend,
    TouchMovement,
);

crate::options! {
    #[derive(Clone, Debug)]
    pub struct GuiOptions {
        pub gui_scale: u32 = 0 => key("guiScale"),
        pub fov: u32 = crate::renderer::systems::VANILLA_FOV_DEGREES as u32 => key("fov"),
        pub fov_effects: u32 = FOV_EFFECTS_DEFAULT => key("fovEffectScale").clamp(0, FOV_EFFECTS_DEFAULT),
        pub clouds: crate::renderer::clouds::CloudStatus = if cfg!(feature = "mobile_ui") {
            crate::renderer::clouds::CloudStatus::Fast
        } else {
            crate::renderer::clouds::CloudStatus::default()
        } => key("renderClouds"),
        pub particles: crate::util::particles::ParticleStatus = if cfg!(feature = "mobile_ui") {
            crate::util::particles::ParticleStatus::Decreased
        } else {
            crate::util::particles::ParticleStatus::All
        } => key("particles"),
        pub render_distance: u32 = RENDER_DISTANCE_DEFAULT => key("renderDistance"),
        pub gamma: u32 = GAMMA_DEFAULT => key("gamma").clamp(0, GAMMA_MAX),
        pub antialiasing: crate::renderer::antialias::Antialiasing = Default::default()
            => custom(|v| crate::renderer::antialias::Antialiasing::read(v), |m, a| a.write(m)),
        pub smooth_lighting: bool = SMOOTH_LIGHTING_DEFAULT => key("ao"),
        pub lighting_enabled: bool = LIGHTING_ENABLED_DEFAULT => key("lightingEnabled"),
        pub max_fps: u32 = MAX_FPS_DEFAULT => key("maxFps").clamp(MAX_FPS_MIN, MAX_FPS_UNLIMITED),
        pub vsync: bool = VSYNC_DEFAULT => key("vsync"),
        pub render_backend: RenderBackend = RenderBackend::default() => key("renderBackend"),
        pub auto_jump: bool = AUTO_JUMP_DEFAULT => key("autoJump"),
        pub view_bobbing: bool = true => key("bobView"),
        pub zoom: ZoomOptions = ZoomOptions::defaults() => group(),
        pub chat: ChatOptions = ChatOptions::defaults() => group(),
        pub appleskin: appleskin::AppleSkinOptions = appleskin::AppleSkinOptions::defaults() => group(),
        pub language: String = "en_us".to_owned() => key("lang"),
        pub accessibility: more_options::AccessibilityOptions = more_options::AccessibilityOptions::defaults() => group(),
        pub force_unicode: bool = false => key("forceUnicodeFont"),
        pub allow_listing: bool = true => key("allowServerListing"),
        pub pause_on_lost_focus: bool = true => key("pauseOnLostFocus"),
        /// The sort and transfer buttons on inventories.
        pub sort_buttons: bool = true => key("sortButtons"),
        /// Whether sorting and storing take in the hotbar.
        pub sort_hotbar: bool = false => key("sortHotbar"),
        /// Vanilla `mouseSensitivity` as a percent: 0..=200, 100 is the default.
        pub mouse_sensitivity: u32 = MOUSE_SENSITIVITY_DEFAULT
            => key("mouseSensitivity").scaled(MOUSE_SENSITIVITY_MAX as f64).clamp(0, MOUSE_SENSITIVITY_MAX),
        pub invert_x: bool = false => key("invertXMouse"),
        pub invert_y: bool = false => key("invertYMouse"),
        /// Vanilla `mouseWheelSensitivity` in hundredths: 1..=1000.
        pub scroll_sensitivity: u32 = SCROLL_SENSITIVITY_DEFAULT
            => key("mouseWheelSensitivity").scaled(100.0).clamp(SCROLL_SENSITIVITY_MIN, SCROLL_SENSITIVITY_MAX),
        pub discrete_scrolling: bool = false => key("discrete_mouse_scroll"),
        pub toggle_crouch: bool = false => key("toggleCrouch"),
        pub toggle_sprint: bool = false => key("toggleSprint"),
        pub main_hand_left: bool = false => custom(
            |v| v("mainHand").and_then(|v| v.as_str().map(|s| s == "left")) == Some(true),
            |m, left| {
                m.insert("mainHand".into(), serde_json::Value::from(if *left { "left" } else { "right" }));
            },
        ),
        #[cfg(feature = "skins")]
        pub skin_parts: u8 = 0x7F => custom(read_skin_parts, write_skin_parts),
        pub fps_counter: bool = FPS_COUNTER_DEFAULT => key("fpsCounter"),
        /// Round shadows under entities (`entityShadows`), and how dark they
        /// are, in percent.
        pub entity_shadows: bool = true => key("entityShadows"),
        pub shadow_opacity: u32 = 50 => key("shadowOpacity").clamp(0, 100),
        pub touch_movement: TouchMovement = TouchMovement::default() => key("touchMovement"),
        pub allow_signed_chat: bool = false => key("allowSignedChat"),
        pub toast_advancement: bool = true => key("toastAdvancement"),
        pub toast_recipe: bool = true => key("toastRecipe"),
        pub toast_system: bool = true => key("toastSystem"),
        pub toast_time: u32 = TOAST_TIME_DEFAULT
            => key("toastDisplayTime").scaled(10.0).clamp(TOAST_TIME_MIN as u32, TOAST_TIME_MAX as u32),
        #[cfg(audio)]
        pub volumes: [u32; crate::audio::category::SoundCategory::COUNT] = [VOLUME_DEFAULT; crate::audio::category::SoundCategory::COUNT]
            => custom(read_volumes, write_volumes),
        #[cfg(voice)]
        pub voice: VoiceOptions = VoiceOptions::read(&|_| None) => custom(|v| VoiceOptions::read(v), |m, voice| voice.write(m)),
        /// The block information panel's settings.
        pub block_info: block_info::BlockInfoOptions = block_info::BlockInfoOptions::load(None) => custom(
            |v| block_info::BlockInfoOptions::load(v("blockInfo").as_ref()),
            |m, b| {
                m.insert("blockInfo".into(), b.save());
            },
        ),
        /// Vanilla's `soundDevice`: the speaker by name, `None` for the default.
        #[cfg(audio)]
        pub sound_device: Option<String> = None => key("soundDevice"),
    }
}

impl Default for GuiOptions {
    /// The options as stored.
    fn default() -> Self {
        let stored = load_options();
        let value = |key: &str| stored.as_ref().and_then(|json| json.get(key).cloned());
        <GuiOptions as option_codec::OptionGroup>::read_from(&value)
    }
}

#[cfg(feature = "skins")]
fn read_skin_parts(value: option_codec::Lookup) -> u8 {
    let mut parts = 0u8;
    for (key, bit) in MODEL_PART_KEYS {
        if value(key).and_then(|v| v.as_bool()).unwrap_or(true) {
            parts |= 1 << bit;
        }
    }
    parts
}

#[cfg(feature = "skins")]
fn write_skin_parts(map: &mut serde_json::Map<String, serde_json::Value>, parts: &u8) {
    for (key, bit) in MODEL_PART_KEYS {
        map.insert(key.to_string(), serde_json::json!(parts & (1 << bit) != 0));
    }
}

#[cfg(audio)]
fn read_volumes(
    value: option_codec::Lookup,
) -> [u32; crate::audio::category::SoundCategory::COUNT] {
    let mut volumes = [VOLUME_DEFAULT; crate::audio::category::SoundCategory::COUNT];
    for category in crate::audio::category::SoundCategory::ALL {
        volumes[category.index()] = value(&format!("soundCategory_{}", category.key()))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(VOLUME_DEFAULT)
            .min(100);
    }
    volumes
}

#[cfg(audio)]
fn write_volumes(
    map: &mut serde_json::Map<String, serde_json::Value>,
    volumes: &[u32; crate::audio::category::SoundCategory::COUNT],
) {
    for category in crate::audio::category::SoundCategory::ALL {
        map.insert(
            format!("soundCategory_{}", category.key()),
            serde_json::json!(volumes[category.index()]),
        );
    }
}

#[cfg(voice)]
impl VoiceOptions {
    fn read(value: option_codec::Lookup) -> VoiceOptions {
        VoiceOptions {
            disabled: value("voiceDisabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            muted: value("voiceMuted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
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
                value("voiceStatus")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .as_deref()
                    .unwrap_or("icon"),
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
        }
    }

    fn write(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        let voice = self;
        map.insert("voiceDisabled".into(), serde_json::json!(voice.disabled));
        map.insert("voiceMuted".into(), serde_json::json!(voice.muted));
        map.insert(
            "voiceActivation".into(),
            serde_json::json!(if voice.voice_activation {
                "voice"
            } else {
                "ptt"
            }),
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
        map.insert(
            "voicePlayerVolumes".into(),
            serde_json::Value::Object(volumes),
        );
    }
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

/// Who the chat shows: `chatVisibility`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChatShown {
    #[default]
    Shown,
    CommandsOnly,
    Hidden,
}

impl ChatShown {
    pub fn caption(self) -> &'static str {
        match self {
            ChatShown::Shown => "Shown",
            ChatShown::CommandsOnly => "Commands Only",
            ChatShown::Hidden => "Hidden",
        }
    }

    pub fn next(self) -> ChatShown {
        match self {
            ChatShown::Shown => ChatShown::CommandsOnly,
            ChatShown::CommandsOnly => ChatShown::Hidden,
            ChatShown::Hidden => ChatShown::Shown,
        }
    }

    /// The id sent in `ClientInformation`.
    pub fn id(self) -> u8 {
        self as u8
    }
}

pub const CHAT_DELAY_MAX_TENTHS: u32 = 60;

/// `chatVisibility` is stored by its id.
impl option_codec::OptionValue for ChatShown {
    fn from_json(value: &serde_json::Value) -> Option<Self> {
        Some(match value.as_u64()? {
            1 => ChatShown::CommandsOnly,
            2 => ChatShown::Hidden,
            _ => ChatShown::Shown,
        })
    }
    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::from(self.id())
    }
}

crate::options! {
    /// Vanilla's chat options, as percentages and pixels where vanilla keeps
    /// fractions.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct ChatOptions {
        pub shown: ChatShown = ChatShown::Shown => key("chatVisibility"),
        pub colors: bool = true => key("chatColors"),
        pub links: bool = true => key("chatLinks"),
        pub links_prompt: bool = true => key("chatLinksPrompt"),
        /// `chatOpacity` and `textBackgroundOpacity`, percent.
        pub opacity: u32 = 100 => key("chatOpacity").clamp(0, 100),
        pub background: u32 = 50 => key("textBackgroundOpacity").clamp(0, 100),
        /// `chatScale` and `chatLineSpacing`, percent.
        pub scale: u32 = 100 => key("chatScale").clamp(0, 100),
        pub line_spacing: u32 = 0 => key("chatLineSpacing").clamp(0, 100),
        /// `chatDelay`, in tenths of a second.
        pub delay_tenths: u32 = 0 => key("chatDelay").clamp(0, CHAT_DELAY_MAX_TENTHS),
        /// Pixels: width 40 to 320, heights 20 to 180.
        pub width: u32 = 320 => key("chatWidth").clamp(40, 320),
        pub focused_height: u32 = 180 => key("chatHeightFocused").clamp(20, 180),
        pub unfocused_height: u32 = 90 => key("chatHeightUnfocused").clamp(20, 180),
        pub auto_suggestions: bool = true => key("autoSuggestions"),
        pub only_secure: bool = false => key("onlyShowSecureChat"),
    }
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self::defaults()
    }
}

pub const ZOOM_TENTHS_MIN: u32 = 15;
pub const ZOOM_TENTHS_MAX: u32 = 1000;
pub const ZOOM_SMOOTHING_MAX_MS: u32 = 500;

crate::options! {
    /// The zoom key's settings. Zoom levels are how many times narrower than
    /// the FOV the view gets, in tenths.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct ZoomOptions {
        pub default_tenths: u32 = 40 => key("zoomDefault").clamp(ZOOM_TENTHS_MIN, ZOOM_TENTHS_MAX),
        pub max_tenths: u32 = 500 => key("zoomMax").clamp(ZOOM_TENTHS_MIN, ZOOM_TENTHS_MAX),
        /// How long the view takes to settle on a new zoom; 0 snaps.
        pub smoothing_ms: u32 = 100 => key("zoomSmoothingMs").clamp(0, ZOOM_SMOOTHING_MAX_MS),
        /// How much the mouse slows with the zoom, in percent: 100 slows it as
        /// much as the view narrows, 0 not at all.
        pub sensitivity: u32 = 50 => key("zoomSensitivity").clamp(0, 100),
    }
}

impl Default for ZoomOptions {
    fn default() -> Self {
        Self::defaults()
    }
}

impl ZoomOptions {
    pub fn default_zoom(&self) -> f32 {
        self.default_tenths.min(self.max_tenths) as f32 / 10.0
    }

    pub fn max_zoom(&self) -> f32 {
        self.max_tenths as f32 / 10.0
    }
}

/// Effects drawn over the game for a moment: the item activation
/// (`ItemActivation`) and the elder guardian's face.
#[derive(Default)]
pub struct ScreenEffects {
    /// When the totem started, and its sideways drift.
    pub activation: Option<(crate::platform::time::Instant, f32, f32, &'static str)>,
    pub elder: Option<crate::platform::time::Instant>,
}

impl ScreenEffects {
    pub fn totem(&mut self) {
        let seed = crate::platform::time::Instant::now().elapsed().as_nanos() as i64 ^ 0x5DEECE66D;
        let mut rng = crate::util::javarandom::JavaRandom::new(seed);
        let (x, y) = (rng.next_f32() * 2.0 - 1.0, rng.next_f32() * 2.0 - 1.0);
        self.activation = Some((
            crate::platform::time::Instant::now(),
            x,
            y,
            "totem_of_undying",
        ));
    }

    /// How far through the activation (0 to 1), its drift and item.
    pub fn activation(&mut self) -> Option<(f32, f32, f32, &'static str)> {
        let (start, x, y, item) = self.activation?;
        let t = start.elapsed().as_secs_f32() / 2.0;
        if t >= 1.0 {
            self.activation = None;
            return None;
        }
        Some((t, x, y, item))
    }

    /// How far through the elder guardian's appearance (0 to 1).
    pub fn elder(&mut self) -> Option<f32> {
        let t = self.elder?.elapsed().as_secs_f32() / 1.5;
        if t >= 1.0 {
            self.elder = None;
            return None;
        }
        Some(t)
    }
}

/// The options as last saved, for code outside the GUI (the renderer,
/// items, chat drawing): one shared copy, swapped when options are saved,
/// instead of each part keeping its own.
static CURRENT_OPTIONS: std::sync::RwLock<Option<std::sync::Arc<GuiOptions>>> =
    std::sync::RwLock::new(None);

pub fn current_options() -> std::sync::Arc<GuiOptions> {
    if let Some(current) = CURRENT_OPTIONS.read().unwrap().as_ref() {
        return current.clone();
    }
    let loaded = std::sync::Arc::new(GuiOptions::default());
    publish_options(&loaded);
    loaded
}

fn publish_options(options: &std::sync::Arc<GuiOptions>) {
    crate::text::FORCE_UNICODE.store(options.force_unicode, std::sync::atomic::Ordering::Relaxed);
    *CURRENT_OPTIONS.write().unwrap() = Some(options.clone());
}

fn load_options() -> Option<serde_json::Value> {
    let text = crate::platform::storage::OPTIONS.load()?;
    serde_json::from_str(&text).ok()
}

pub fn save_options(options: &GuiOptions) {
    publish_options(&std::sync::Arc::new(options.clone()));
    let mut map = serde_json::Map::new();
    option_codec::OptionGroup::write_to(options, &mut map);
    let json = serde_json::Value::Object(map);
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
    /// F3+B: entities' hitboxes.
    pub hitboxes: bool,
    pub stats: stats::StatsState,
    pub social: social::SocialState,
    pub op_block: op_blocks::OpBlockState,
    pub language_list: scroll_list::ScrollList,
    /// Screen effects the game set off.
    pub effects: ScreenEffects,
    pub switcher: gamemode_switcher::SwitcherState,
    pub hide_gui: bool,
    /// The zoom key is held: the HUD hides, as with F1.
    pub zooming: bool,
    pub win: win::WinState,
    pub slots: slots::SlotState,
    pub container: container::ContainerUi,
    #[cfg(feature = "click_gui")]
    pub clickgui: clickgui::ClickGuiState,
    #[cfg(feature = "click_gui")]
    pub clickgui_parent: Screen,
    pub hud_state: hud_layout::HudState,
    pub options: GuiOptions,
    pub keybinds: keybinds::KeyBinds,
    pub controls_list: scroll_list::ScrollList,
    pub nav: Option<Screen>,
    pub suppress_click: bool,
    pub death: death::DeathState,
    pub advancements: advancements::AdvancementsUi,
    pub quit: bool,
    pub disconnect: bool,
    pub menu: multiplayer::MenuState,
    pub profile: profile::ProfileState,
    pub video_list: scroll_list::ScrollList,
    pub game_list: scroll_list::ScrollList,
    #[cfg(voice)]
    pub voice_list: scroll_list::ScrollList,
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
    /// F1's hidden GUI, or the zoom key held: no HUD, no hand.
    pub fn gui_hidden(&self) -> bool {
        self.hide_gui || self.zooming
    }

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
    pub menu_slots: std::sync::Arc<[SlotStack]>,
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
            menu_slots: menu_slots.into(),
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
