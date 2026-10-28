//! Native Simple Voice Chat: when a server runs the mod or plugin, this
//! client asks for its secret over the plugin channels, as the mod's
//! `ClientManager` does, keeps the encrypted UDP connection the voice server
//! expects (`connection`), plays the voices it sends at the players speaking
//! (`playback`), and sends the microphone (`mic`). Servers without it never
//! answer the request, and nothing else happens.
//!
//! The formats are in `wire`, from the mod's 26.3 source; the codec is
//! `codec`.

mod codec;
mod connection;
mod mic;
mod playback;
pub(crate) mod wire;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use uuid::Uuid;

use connection::Connection;
pub(crate) use mic::Activation;
use playback::{Listening, Playback};
pub(crate) use wire::{Group, GroupType};
use wire::PlayerState;

/// Where voice chat stands on this server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// Not asked yet, or this server has no voice chat (it never answers).
    Off,
    /// The secret was asked for.
    Requested,
    /// Talking to the voice server.
    Authenticating,
    Connected,
    Failed(String),
}

struct Session {
    status: Arc<Mutex<Status>>,
    /// The server named voice chat's channels in its `minecraft:register`.
    advertised: bool,
    connection: Option<Connection>,
    players: HashMap<Uuid, PlayerState>,
    /// Whether the server allows groups (`SecretPacket.groupsEnabled`).
    groups_enabled: bool,
    groups: HashMap<Uuid, Group>,
    /// The group this player is in.
    group: Option<Uuid>,
    /// The last join failed on its password.
    wrong_password: bool,
}

impl Session {
    fn new() -> Session {
        Session {
            status: Arc::new(Mutex::new(Status::Off)),
            advertised: false,
            connection: None,
            players: HashMap::new(),
            groups_enabled: false,
            groups: HashMap::new(),
            group: None,
            wrong_password: false,
        }
    }

    fn set(&self, status: Status) {
        if let Ok(mut s) = self.status.lock() {
            *s = status;
        }
    }
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Plugin messages for the server, drained by the game tick.
static OUTGOING: Mutex<Vec<(&'static str, Vec<u8>)>> = Mutex::new(Vec::new());

/// Voice chat turned off by this player: no microphone, no voices, and the
/// server tells everyone (the crossed-out icon).
static DISABLED: AtomicBool = AtomicBool::new(false);

/// Where voices are heard from, kept by the game tick.
struct Heard {
    me: Option<Uuid>,
    listener: [f32; 3],
    eyes: HashMap<Uuid, [f32; 3]>,
    volume: f32,
    player_volumes: HashMap<Uuid, f32>,
}

static HEARD: std::sync::LazyLock<Mutex<Heard>> = std::sync::LazyLock::new(|| {
    Mutex::new(Heard {
        me: None,
        listener: [0.0; 3],
        eyes: HashMap::new(),
        volume: 1.0,
        player_volumes: HashMap::new(),
    })
});

fn queue(channel: &'static str, payload: Vec<u8>) {
    if let Ok(mut out) = OUTGOING.lock() {
        out.push((channel, payload));
    }
}

fn with<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    let mut session = SESSION.lock().ok()?;
    Some(f(session.get_or_insert_with(Session::new)))
}

/// On joining a world (`ClientboundLogin`), as `ClientManager.onJoinWorld`:
/// drop any earlier connection, register the channels the server sends on,
/// and ask for a secret. A server without voice chat ignores both.
pub(crate) fn on_join() {
    with(|session| {
        *session = Session::new();
        session.set(Status::Requested);
    });
    queue("minecraft:register", wire::register(&wire::CLIENT_BOUND));
    queue(wire::REQUEST_SECRET, wire::request_secret());
    if disabled() {
        queue(wire::UPDATE_STATE, wire::update_state(true));
    }
}

/// On leaving the server.
pub(crate) fn on_leave() {
    if let Ok(mut session) = SESSION.lock() {
        *session = None;
    }
    if let Ok(mut out) = OUTGOING.lock() {
        out.clear();
    }
}

/// Plays each voice as it arrives, on the connection's thread.
fn player() -> connection::OnVoice {
    let mut playback = Playback::default();
    let mut swept = std::time::Instant::now();
    Box::new(move |packet| {
        if disabled() {
            return;
        }
        let Ok(heard) = HEARD.lock() else { return };
        let listening = Listening {
            me: heard.me,
            listener: heard.listener,
            eyes: &heard.eyes,
            volume: heard.volume,
            player_volumes: &heard.player_volumes,
        };
        playback.receive(packet, &listening);
        drop(heard);
        if swept.elapsed().as_secs() >= 5 {
            swept = std::time::Instant::now();
            playback.sweep();
        }
    })
}

/// A plugin message from the server. `game_host` is the game server's host,
/// which the voice server shares unless the secret names another.
pub(crate) fn on_payload(channel: &str, payload: &[u8], game_host: Option<&str>) {
    match channel {
        "minecraft:register" => {
            if wire::registered(payload).any(|c| c == wire::REQUEST_SECRET) {
                with(|session| session.advertised = true);
            }
        }
        wire::SECRET => {
            let secret = match wire::Secret::read(payload) {
                Ok(secret) => secret,
                Err(_) => {
                    crate::log_warn!("voice", "the voice chat secret did not read");
                    with(|session| session.set(Status::Failed("unreadable secret".into())));
                    return;
                }
            };
            let Some(host) = game_host else {
                crate::log_warn!("voice", "a voice chat secret, but no server address");
                return;
            };
            let host = wire::split_host(host).0.to_owned();
            crate::log_info!(
                "voice",
                "received the voice chat secret ({:?}, {} block range, groups {})",
                secret.codec,
                secret.distance,
                if secret.groups { "on" } else { "off" }
            );
            let groups = secret.groups;
            with(|session| {
                session.groups_enabled = groups;
                session.connection = None;
                session.connection = Some(Connection::start(
                    secret,
                    host,
                    session.status.clone(),
                    player(),
                ));
            });
        }
        wire::STATE => {
            if let Ok(state) = PlayerState::read_one(payload) {
                with(|session| session.players.insert(state.uuid, state));
            }
        }
        wire::STATES => {
            if let Ok(states) = PlayerState::read_all(payload) {
                with(|session| {
                    session.players = states.into_iter().map(|s| (s.uuid, s)).collect();
                });
            }
        }
        wire::REMOVE_STATE => {
            if let Ok(id) = wire::read_id(payload) {
                with(|session| session.players.remove(&id));
            }
        }
        wire::ADD_GROUP => {
            if let Ok(group) = Group::read(payload) {
                with(|session| session.groups.insert(group.id, group));
            }
        }
        wire::REMOVE_GROUP => {
            if let Ok(id) = wire::read_id(payload) {
                with(|session| session.groups.remove(&id));
            }
        }
        wire::JOINED_GROUP => {
            if let Ok((group, wrong_password)) = wire::read_joined(payload) {
                with(|session| {
                    session.group = group;
                    session.wrong_password = wrong_password;
                });
            }
        }
        _ => {}
    }
}

/// The plugin messages waiting to go to the server.
pub(crate) fn take_outgoing() -> Vec<(&'static str, Vec<u8>)> {
    OUTGOING
        .lock()
        .map(|mut out| std::mem::take(&mut *out))
        .unwrap_or_default()
}

/// Each game tick: who this player is, where the camera is, and every
/// player's eyes, for placing voices.
pub(crate) fn tick(me: Option<Uuid>, listener: [f32; 3], eyes: HashMap<Uuid, [f32; 3]>) {
    if let Ok(mut heard) = HEARD.lock() {
        heard.me = me;
        heard.listener = listener;
        heard.eyes = eyes;
    }
}

pub(crate) fn status() -> Status {
    with(|session| session.status.lock().map_or(Status::Off, |s| s.clone())).unwrap_or(Status::Off)
}

/// Whether the server named voice chat's channels when it registered its own.
#[allow(dead_code, reason = "for a voice chat screen, not built yet")]
pub(crate) fn advertised() -> bool {
    with(|session| session.advertised).unwrap_or(false)
}

/// What the server last said about `player`'s voice chat.
#[allow(dead_code, reason = "for the speaking and muted icons over players, not drawn yet")]
pub(crate) fn player_state(player: Uuid) -> Option<PlayerState> {
    with(|session| session.players.get(&player).cloned()).flatten()
}

/// The microphone chosen by name, `None` for the default.
static INPUT_DEVICE: Mutex<Option<String>> = Mutex::new(None);
/// Counts microphone choices, so an open one knows it was replaced.
static INPUT_GENERATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn input_device() -> Option<String> {
    INPUT_DEVICE.lock().ok().and_then(|d| d.clone())
}

pub(crate) fn input_generation() -> u32 {
    INPUT_GENERATION.load(Ordering::Relaxed)
}

/// Chooses the microphone; an open one is replaced.
pub(crate) fn set_input_device(name: Option<String>) {
    let changed = INPUT_DEVICE
        .lock()
        .is_ok_and(|mut d| std::mem::replace(&mut *d, name.clone()) != name);
    if changed {
        INPUT_GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn disabled() -> bool {
    DISABLED.load(Ordering::Relaxed)
}

/// Turns voice chat off or on, as the mod's disable toggle does, and tells
/// the server.
pub(crate) fn set_disabled(off: bool) {
    if DISABLED.swap(off, Ordering::Relaxed) != off {
        queue(wire::UPDATE_STATE, wire::update_state(off));
    }
}

pub(crate) fn muted() -> bool {
    mic::CONTROLS.muted.load(Ordering::Relaxed)
}

pub(crate) fn set_muted(muted: bool) {
    mic::CONTROLS.muted.store(muted, Ordering::Relaxed);
}

/// Push-to-talk held (the game's key state).
pub(crate) fn set_push_to_talk(held: bool) {
    mic::CONTROLS.push_to_talk.store(held, Ordering::Relaxed);
}

pub(crate) fn set_whisper(whisper: bool) {
    mic::CONTROLS.whisper.store(whisper, Ordering::Relaxed);
}

pub(crate) fn set_activation(activation: Activation) {
    mic::CONTROLS.set_activation(activation);
}

pub(crate) fn activation() -> Activation {
    mic::CONTROLS.activation()
}

/// The voice activation threshold in dB (default -50).
pub(crate) fn set_threshold(db: f32) {
    mic::CONTROLS
        .threshold
        .store(db.clamp(-127.0, 0.0).to_bits(), Ordering::Relaxed);
}

/// `voice_chat_volume`, 0 to 3; applies to voices that start after.
pub(crate) fn set_volume(volume: f32) {
    if let Ok(mut heard) = HEARD.lock() {
        heard.volume = volume.clamp(0.0, 3.0);
    }
}

/// Each speaker's own volume (`PlayerVolumeConfig`), 0 to 4; one not set
/// is 1. Applies to voices as they play.
pub(crate) fn set_player_volumes(volumes: HashMap<Uuid, f32>) {
    if let Ok(mut heard) = HEARD.lock() {
        heard.player_volumes = volumes
            .into_iter()
            .map(|(id, v)| (id, v.clamp(0.0, 4.0)))
            .collect();
    }
}

/// The players the server says have voice chat, but not this one, by name
/// (`ClientPlayerStateManager.getPlayerStates(false)`).
pub(crate) fn voice_players() -> Vec<(Uuid, String)> {
    let me = HEARD.lock().ok().and_then(|h| h.me);
    let mut players: Vec<(Uuid, String)> = with(|session| {
        session
            .players
            .values()
            .filter(|s| Some(s.uuid) != me)
            .map(|s| (s.uuid, s.name.clone()))
            .collect()
    })
    .unwrap_or_default();
    players.sort_by_key(|(_, name)| name.to_lowercase());
    players
}

/// What a groups screen shows: whether groups are on, the groups the server
/// lists (hidden ones left out), the one this player is in, and whether the
/// last join had the wrong password.
pub(crate) struct Groups {
    pub(crate) enabled: bool,
    pub(crate) list: Vec<Group>,
    pub(crate) current: Option<Uuid>,
    pub(crate) wrong_password: bool,
}

pub(crate) fn groups() -> Groups {
    with(|session| {
        let mut list: Vec<Group> = session
            .groups
            .values()
            .filter(|g| !g.hidden || Some(g.id) == session.group)
            .cloned()
            .collect();
        list.sort_by_key(|g| g.name.to_lowercase());
        Groups {
            enabled: session.groups_enabled,
            list,
            current: session.group,
            wrong_password: session.wrong_password,
        }
    })
    .unwrap_or(Groups {
        enabled: false,
        list: Vec::new(),
        current: None,
        wrong_password: false,
    })
}

/// The names of the players in `group`.
pub(crate) fn group_members(group: Uuid) -> Vec<String> {
    let mut names: Vec<String> = with(|session| {
        session
            .players
            .values()
            .filter(|s| s.group == Some(group))
            .map(|s| s.name.clone())
            .collect()
    })
    .unwrap_or_default();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

pub(crate) fn join_group(group: Uuid, password: Option<&str>) {
    with(|session| session.wrong_password = false);
    queue(wire::SET_GROUP, wire::join_group(group, password));
}

pub(crate) fn create_group(name: &str, password: Option<&str>, kind: GroupType) {
    let name: String = name.chars().take(wire::MAX_GROUP_NAME).collect();
    let password: Option<String> = password.map(|p| p.chars().take(wire::MAX_GROUP_NAME).collect());
    queue(wire::CREATE_GROUP, wire::create_group(&name, password.as_deref(), kind));
}

pub(crate) fn leave_group() {
    queue(wire::LEAVE_GROUP, Vec::new());
}

/// Whether the microphone is sending now.
pub(crate) fn talking() -> bool {
    mic::CONTROLS.talking.load(Ordering::Relaxed)
}

/// Tests that touch the shared voice state take this first.
#[cfg(test)]
pub(crate) static TEST_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joining_registers_the_channels_then_asks_for_the_secret() {
        let _serial = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        set_disabled(false);
        let _ = take_outgoing();
        on_join();
        let out = take_outgoing();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "minecraft:register");
        assert!(wire::registered(&out[0].1).any(|c| c == wire::SECRET));
        assert_eq!(out[1], (wire::REQUEST_SECRET, 20i32.to_be_bytes().to_vec()));
        assert_eq!(status(), Status::Requested);
        on_payload(
            "minecraft:register",
            &wire::register(&[wire::REQUEST_SECRET]),
            None,
        );
        assert!(advertised());
        on_leave();
        assert_eq!(status(), Status::Off);
    }
}
