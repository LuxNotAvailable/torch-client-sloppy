//! Simple Voice Chat's wire formats, as its 26.3 source writes them
//! (`de.maxhenkel.voicechat`, compatibility version 20): Minecraft's
//! `FriendlyByteBuf` encoding for the plugin-channel packets, and for the UDP
//! packets an AES-GCM payload under a 16-byte secret.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes128Gcm, Nonce};
use uuid::Uuid;

/// `Voicechat.COMPATIBILITY_VERSION`: the server refuses another.
pub(crate) const COMPATIBILITY_VERSION: i32 = 20;

pub(crate) const REQUEST_SECRET: &str = "voicechat:request_secret";
pub(crate) const UPDATE_STATE: &str = "voicechat:update_state";
pub(crate) const SECRET: &str = "voicechat:secret";
pub(crate) const STATE: &str = "voicechat:state";
pub(crate) const STATES: &str = "voicechat:states";
pub(crate) const REMOVE_STATE: &str = "voicechat:remove_state";
pub(crate) const ADD_GROUP: &str = "voicechat:add_group";
pub(crate) const REMOVE_GROUP: &str = "voicechat:remove_group";
pub(crate) const JOINED_GROUP: &str = "voicechat:joined_group";
/// To the server: join a group (`JoinGroupPacket`'s channel is `set_group`).
pub(crate) const SET_GROUP: &str = "voicechat:set_group";
pub(crate) const CREATE_GROUP: &str = "voicechat:create_group";
pub(crate) const LEAVE_GROUP: &str = "voicechat:leave_group";
/// `Voicechat.MAX_GROUP_NAME_LENGTH`.
pub(crate) const MAX_GROUP_NAME: usize = 24;

/// Every channel the server sends on, registered so a Bukkit server (which
/// sends only on channels a client registered) sends them.
pub(crate) const CLIENT_BOUND: [&str; 9] = [
    SECRET,
    STATE,
    STATES,
    REMOVE_STATE,
    ADD_GROUP,
    REMOVE_GROUP,
    JOINED_GROUP,
    "voicechat:add_category",
    "voicechat:remove_category",
];

/// `NetworkMessage.MAGIC_BYTE`.
const MAGIC: u8 = 0xFF;
/// `Secret.IV_SIZE_BYTES`.
const IV_LEN: usize = 12;
pub(crate) const SECRET_LEN: usize = 16;
/// `Utils.MAX_VOICE_CHAT_PACKET_SIZE`.
pub(crate) const MAX_PACKET: usize = 2048;
/// `AudioUtils.MAX_OPUS_PAYLOAD_SIZE`.
const MAX_OPUS: usize = 1275;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Short;

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Short> {
        if self.bytes.len() < n {
            return Err(Short);
        }
        let (head, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Short> {
        Ok(self.take(N)?.try_into().expect("taken to length"))
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Short> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn bool(&mut self) -> Result<bool, Short> {
        Ok(self.u8()? != 0)
    }

    pub(crate) fn i32(&mut self) -> Result<i32, Short> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    pub(crate) fn i64(&mut self) -> Result<i64, Short> {
        Ok(i64::from_be_bytes(self.array()?))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, Short> {
        Ok(f32::from_be_bytes(self.array()?))
    }

    pub(crate) fn f64(&mut self) -> Result<f64, Short> {
        Ok(f64::from_be_bytes(self.array()?))
    }

    pub(crate) fn uuid(&mut self) -> Result<Uuid, Short> {
        Ok(Uuid::from_bytes(self.array()?))
    }

    pub(crate) fn var_int(&mut self) -> Result<i32, Short> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = self.u8()?;
            value |= u32::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Ok(value as i32);
            }
        }
        Err(Short)
    }

    /// `readByteArray(max)`: a VarInt length, then the bytes.
    pub(crate) fn byte_array(&mut self, max: usize) -> Result<&'a [u8], Short> {
        let len = usize::try_from(self.var_int()?).map_err(|_| Short)?;
        if len > max {
            return Err(Short);
        }
        self.take(len)
    }

    /// `readUtf(max)`: a VarInt byte length, then UTF-8.
    pub(crate) fn utf(&mut self, max_chars: usize) -> Result<String, Short> {
        let bytes = self.byte_array(max_chars * 3)?;
        let text = std::str::from_utf8(bytes).map_err(|_| Short)?;
        if text.chars().count() > max_chars {
            return Err(Short);
        }
        Ok(text.to_owned())
    }
}

#[derive(Default)]
pub(crate) struct Writer {
    pub(crate) bytes: Vec<u8>,
}

impl Writer {
    pub(crate) fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }

    pub(crate) fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }

    /// `writeUtf`: a VarInt byte length, then UTF-8.
    pub(crate) fn utf(&mut self, v: &str) -> &mut Self {
        self.byte_array(v.as_bytes())
    }

    pub(crate) fn i16(&mut self, v: i16) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub(crate) fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub(crate) fn i64(&mut self, v: i64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub(crate) fn uuid(&mut self, v: Uuid) -> &mut Self {
        self.bytes.extend_from_slice(v.as_bytes());
        self
    }

    pub(crate) fn var_int(&mut self, v: i32) -> &mut Self {
        let mut v = v as u32;
        loop {
            if v & !0x7F == 0 {
                self.bytes.push(v as u8);
                return self;
            }
            self.bytes.push((v as u8 & 0x7F) | 0x80);
            v >>= 7;
        }
    }

    pub(crate) fn byte_array(&mut self, v: &[u8]) -> &mut Self {
        self.var_int(v.len() as i32);
        self.bytes.extend_from_slice(v);
        self
    }
}

/// `SecretPacket`: what the server answers a secret request with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Secret {
    pub(crate) secret: [u8; SECRET_LEN],
    pub(crate) port: i32,
    pub(crate) player: Uuid,
    pub(crate) codec: Codec,
    pub(crate) mtu: i32,
    pub(crate) distance: f64,
    /// Milliseconds between keep-alives; ten missed is a timeout.
    pub(crate) keep_alive: i32,
    pub(crate) groups: bool,
    /// Where to reach the voice server if not the game server's address:
    /// empty, a port, or `host[:port]` (`InitializationData.parseAddress`).
    pub(crate) voice_host: String,
    pub(crate) allow_recording: bool,
}

/// `ServerConfig.Codec`, by ordinal: the Opus application the server wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Codec {
    Voip,
    Audio,
    RestrictedLowDelay,
}

impl Secret {
    pub(crate) fn read(bytes: &[u8]) -> Result<Secret, Short> {
        let mut r = Reader::new(bytes);
        Ok(Secret {
            secret: r.array()?,
            port: r.i32()?,
            player: r.uuid()?,
            codec: match r.u8()? {
                0 => Codec::Voip,
                1 => Codec::Audio,
                2 => Codec::RestrictedLowDelay,
                _ => return Err(Short),
            },
            mtu: r.i32()?,
            distance: r.f64()?,
            keep_alive: r.i32()?,
            groups: r.bool()?,
            voice_host: r.utf(32767)?,
            allow_recording: r.bool()?,
        })
    }

    /// The voice server's host and port, from the game server's host.
    pub(crate) fn address(&self, game_host: &str) -> (String, u16) {
        let port = u16::try_from(self.port).unwrap_or(0);
        let host = self.voice_host.trim();
        if host.is_empty() {
            return (game_host.to_owned(), port);
        }
        if let Ok(only) = host.parse::<i32>() {
            return match u16::try_from(only) {
                Ok(p) if p > 0 => (game_host.to_owned(), p),
                _ => (game_host.to_owned(), port),
            };
        }
        let (name, at) = split_host(host);
        (
            if name.is_empty() { game_host.to_owned() } else { name.to_owned() },
            at.filter(|p| *p > 0).unwrap_or(port),
        )
    }
}

/// `host`, `host:port`, `[v6]` or `[v6]:port`.
pub(crate) fn split_host(text: &str) -> (&str, Option<u16>) {
    if let Some(rest) = text.strip_prefix('[')
        && let Some((host, tail)) = rest.split_once(']')
    {
        return (host, tail.strip_prefix(':').and_then(|p| p.parse().ok()));
    }
    match text.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, port.parse().ok()),
        _ => (text, None),
    }
}

/// `PlayerState`: who has voice chat, and whether it is off.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlayerState {
    pub(crate) uuid: Uuid,
    pub(crate) name: String,
    pub(crate) disabled: bool,
    pub(crate) disconnected: bool,
    pub(crate) group: Option<Uuid>,
}

impl PlayerState {
    fn read(r: &mut Reader) -> Result<PlayerState, Short> {
        let disabled = r.bool()?;
        let disconnected = r.bool()?;
        let uuid = r.uuid()?;
        let name = r.utf(32767)?;
        let group = if r.bool()? { Some(r.uuid()?) } else { None };
        Ok(PlayerState {
            uuid,
            name,
            disabled,
            disconnected,
            group,
        })
    }

    /// `voicechat:state`.
    pub(crate) fn read_one(bytes: &[u8]) -> Result<PlayerState, Short> {
        PlayerState::read(&mut Reader::new(bytes))
    }

    /// `voicechat:states`: an int count, then each.
    pub(crate) fn read_all(bytes: &[u8]) -> Result<Vec<PlayerState>, Short> {
        let mut r = Reader::new(bytes);
        let count = usize::try_from(r.i32()?).map_err(|_| Short)?;
        (0..count.min(4096)).map(|_| PlayerState::read(&mut r)).collect()
    }
}

/// `Group.Type`, as `GroupImpl.TypeImpl` numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupType {
    /// Members hear each other, and nobody outside.
    Normal,
    /// Members also hear, and are heard by, players near them.
    Open,
    /// Members hear only each other, even those nearby in other groups.
    Isolated,
}

impl GroupType {
    fn from_i16(v: i16) -> GroupType {
        match v {
            1 => GroupType::Open,
            2 => GroupType::Isolated,
            _ => GroupType::Normal,
        }
    }

    fn to_i16(self) -> i16 {
        match self {
            GroupType::Normal => 0,
            GroupType::Open => 1,
            GroupType::Isolated => 2,
        }
    }
}

/// `ClientGroup`: a group the server lists.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Group {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) has_password: bool,
    pub(crate) persistent: bool,
    pub(crate) hidden: bool,
    pub(crate) kind: GroupType,
}

impl Group {
    /// `voicechat:add_group`.
    pub(crate) fn read(bytes: &[u8]) -> Result<Group, Short> {
        let mut r = Reader::new(bytes);
        Ok(Group {
            id: r.uuid()?,
            name: r.utf(512)?,
            has_password: r.bool()?,
            persistent: r.bool()?,
            hidden: r.bool()?,
            kind: GroupType::from_i16(i16::from_be_bytes([r.u8()?, r.u8()?])),
        })
    }
}

/// `voicechat:remove_group` and `voicechat:remove_state`: one id.
pub(crate) fn read_id(bytes: &[u8]) -> Result<Uuid, Short> {
    Reader::new(bytes).uuid()
}

/// `voicechat:joined_group`: the group now in (none), and whether a join
/// failed on its password.
pub(crate) fn read_joined(bytes: &[u8]) -> Result<(Option<Uuid>, bool), Short> {
    let mut r = Reader::new(bytes);
    let group = if r.bool()? { Some(r.uuid()?) } else { None };
    Ok((group, r.bool()?))
}

/// `voicechat:set_group`.
pub(crate) fn join_group(group: Uuid, password: Option<&str>) -> Vec<u8> {
    let mut w = Writer::default();
    w.uuid(group).bool(password.is_some());
    if let Some(password) = password {
        w.utf(password);
    }
    w.bytes
}

/// `voicechat:create_group`.
pub(crate) fn create_group(name: &str, password: Option<&str>, kind: GroupType) -> Vec<u8> {
    let mut w = Writer::default();
    w.utf(name).bool(password.is_some());
    if let Some(password) = password {
        w.utf(password);
    }
    w.i16(kind.to_i16());
    w.bytes
}

/// `voicechat:request_secret`.
pub(crate) fn request_secret() -> Vec<u8> {
    let mut w = Writer::default();
    w.i32(COMPATIBILITY_VERSION);
    w.bytes
}

/// `voicechat:update_state`: whether this player has voice chat off.
pub(crate) fn update_state(disabled: bool) -> Vec<u8> {
    let mut w = Writer::default();
    w.bool(disabled);
    w.bytes
}

/// `minecraft:register`: channel names, each ended by a NUL.
pub(crate) fn register(channels: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for channel in channels {
        out.extend_from_slice(channel.as_bytes());
        out.push(0);
    }
    out
}

/// The channels a `minecraft:register` payload names.
pub(crate) fn registered(payload: &[u8]) -> impl Iterator<Item = &str> {
    payload
        .split(|b| *b == 0)
        .filter_map(|name| std::str::from_utf8(name).ok())
        .filter(|name| !name.is_empty())
}

/// A packet over UDP, by its `NetworkMessage` id.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Packet {
    /// 0x1, to the server: one Opus frame from the microphone.
    #[allow(dead_code, reason = "the microphone is not captured yet")]
    Mic {
        opus: Vec<u8>,
        sequence: i64,
        whispering: bool,
    },
    /// 0x2: a player's voice, heard at that player.
    PlayerSound {
        sound: Sound,
        distance: f32,
        whispering: bool,
    },
    /// 0x3: a voice from this player's group, heard everywhere.
    GroupSound { sound: Sound },
    /// 0x4: a voice at a fixed point (a plugin's speaker).
    LocationSound {
        sound: Sound,
        at: [f64; 3],
        distance: f32,
    },
    /// 0x5, to the server.
    Authenticate {
        player: Uuid,
        secret: [u8; SECRET_LEN],
    },
    /// 0x6.
    AuthenticateAck,
    /// 0x7: echoed back as it came.
    Ping { id: Uuid, timestamp: i64 },
    /// 0x8: answered in kind.
    KeepAlive,
    /// 0x9, to the server.
    ConnectionCheck,
    /// 0xA.
    ConnectionCheckAck,
}

/// What every voice packet carries (`SoundPacket`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sound {
    /// The stream it belongs to; a sender's frames share one.
    pub(crate) channel: Uuid,
    pub(crate) sender: Uuid,
    pub(crate) opus: Vec<u8>,
    /// Below 0 for audio a plugin sent rather than a microphone.
    pub(crate) sequence: i64,
    /// A plugin's volume category, if any.
    pub(crate) category: Option<String>,
}

/// `SoundPacket`'s flag byte.
const WHISPER: u8 = 0b1;
const HAS_CATEGORY: u8 = 0b10;

impl Packet {
    fn id(&self) -> u8 {
        match self {
            Packet::Mic { .. } => 0x1,
            Packet::PlayerSound { .. } => 0x2,
            Packet::GroupSound { .. } => 0x3,
            Packet::LocationSound { .. } => 0x4,
            Packet::Authenticate { .. } => 0x5,
            Packet::AuthenticateAck => 0x6,
            Packet::Ping { .. } => 0x7,
            Packet::KeepAlive => 0x8,
            Packet::ConnectionCheck => 0x9,
            Packet::ConnectionCheckAck => 0xA,
        }
    }

    /// The packet's id and body, before encryption. Only what a client
    /// sends is written.
    fn write(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u8(self.id());
        match self {
            Packet::Mic {
                opus,
                sequence,
                whispering,
            } => {
                w.byte_array(opus).i64(*sequence).bool(*whispering);
            }
            Packet::Authenticate { player, secret } => {
                w.uuid(*player);
                w.bytes.extend_from_slice(secret);
            }
            Packet::Ping { id, timestamp } => {
                w.uuid(*id).i64(*timestamp);
            }
            Packet::KeepAlive | Packet::ConnectionCheck => {}
            Packet::PlayerSound { .. }
            | Packet::GroupSound { .. }
            | Packet::LocationSound { .. }
            | Packet::AuthenticateAck
            | Packet::ConnectionCheckAck => {
                debug_assert!(false, "a client does not send {self:?}");
            }
        }
        w.bytes
    }

    /// A decrypted packet from the server.
    fn read(bytes: &[u8]) -> Result<Packet, Short> {
        let mut r = Reader::new(bytes);
        let sound = |r: &mut Reader| -> Result<(Uuid, Uuid), Short> { Ok((r.uuid()?, r.uuid()?)) };
        let flags = |r: &mut Reader| -> Result<(bool, Option<String>), Short> {
            let flags = r.u8()?;
            let category = if flags & HAS_CATEGORY != 0 {
                Some(r.utf(16)?)
            } else {
                None
            };
            Ok((flags & WHISPER != 0, category))
        };
        Ok(match r.u8()? {
            0x2 => {
                let (channel, sender) = sound(&mut r)?;
                let opus = r.byte_array(MAX_OPUS)?.to_vec();
                let sequence = r.i64()?;
                let distance = r.f32()?;
                let (whispering, category) = flags(&mut r)?;
                Packet::PlayerSound {
                    sound: Sound {
                        channel,
                        sender,
                        opus,
                        sequence,
                        category,
                    },
                    distance,
                    whispering,
                }
            }
            0x3 => {
                let (channel, sender) = sound(&mut r)?;
                let opus = r.byte_array(MAX_OPUS)?.to_vec();
                let sequence = r.i64()?;
                let (_, category) = flags(&mut r)?;
                Packet::GroupSound {
                    sound: Sound {
                        channel,
                        sender,
                        opus,
                        sequence,
                        category,
                    },
                }
            }
            0x4 => {
                let (channel, sender) = sound(&mut r)?;
                let at = [r.f64()?, r.f64()?, r.f64()?];
                let opus = r.byte_array(MAX_OPUS)?.to_vec();
                let sequence = r.i64()?;
                let distance = r.f32()?;
                let (_, category) = flags(&mut r)?;
                Packet::LocationSound {
                    sound: Sound {
                        channel,
                        sender,
                        opus,
                        sequence,
                        category,
                    },
                    at,
                    distance,
                }
            }
            0x6 => Packet::AuthenticateAck,
            0x7 => Packet::Ping {
                id: r.uuid()?,
                timestamp: r.i64()?,
            },
            0x8 => Packet::KeepAlive,
            0xA => Packet::ConnectionCheckAck,
            _ => return Err(Short),
        })
    }
}

/// `Secret.encrypt`: a random 12-byte IV, then AES-GCM (128-bit tag).
pub(super) fn encrypt(secret: &[u8; SECRET_LEN], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes128Gcm::new(secret.into());
    let iv = Aes128Gcm::generate_nonce(&mut OsRng);
    let sealed = cipher
        .encrypt(&iv, plain)
        .expect("AES-GCM encrypts any length a datagram holds");
    let mut out = Vec::with_capacity(IV_LEN + sealed.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&sealed);
    out
}

pub(super) fn decrypt(secret: &[u8; SECRET_LEN], payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < IV_LEN {
        return None;
    }
    let (iv, sealed) = payload.split_at(IV_LEN);
    Aes128Gcm::new(secret.into())
        .decrypt(Nonce::from_slice(iv), sealed)
        .ok()
}

/// `ClientNetworkMessage.writeClient`: magic, this player's id, then the
/// encrypted packet as a byte array.
pub(crate) fn seal(player: Uuid, secret: &[u8; SECRET_LEN], packet: &Packet) -> Vec<u8> {
    let payload = encrypt(secret, &packet.write());
    let mut w = Writer::default();
    w.u8(MAGIC).uuid(player).byte_array(&payload);
    w.bytes
}

/// `ClientNetworkMessage.readPacketClient`: magic, then the encrypted packet
/// as a byte array. `None` for anything else, or a packet this secret did
/// not seal.
pub(crate) fn open(secret: &[u8; SECRET_LEN], datagram: &[u8]) -> Option<Packet> {
    let mut r = Reader::new(datagram);
    if r.u8().ok()? != MAGIC {
        return None;
    }
    let payload = r.byte_array(MAX_PACKET).ok()?;
    Packet::read(&decrypt(secret, payload)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; SECRET_LEN] = *b"0123456789abcdef";

    /// What the server's `writeServer` makes of a packet: magic, then the
    /// encrypted body as a byte array (no player id).
    fn from_server(body: &[u8]) -> Vec<u8> {
        let mut w = Writer::default();
        w.u8(MAGIC).byte_array(&encrypt(&KEY, body));
        w.bytes
    }

    #[test]
    fn a_secret_packet_reads_field_by_field() {
        let player = Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        let mut w = Writer::default();
        w.bytes.extend_from_slice(&KEY);
        w.i32(24454).uuid(player).u8(1).i32(1024);
        w.bytes.extend_from_slice(&48.0f64.to_be_bytes());
        w.i32(1000).bool(true).byte_array(b"voice.example.net:25000").bool(false);
        let secret = Secret::read(&w.bytes).unwrap();
        assert_eq!(secret.secret, KEY);
        assert_eq!(secret.port, 24454);
        assert_eq!(secret.player, player);
        assert_eq!(secret.codec, Codec::Audio);
        assert_eq!(secret.distance, 48.0);
        assert!(secret.groups && !secret.allow_recording);
        assert_eq!(
            secret.address("play.example.net"),
            ("voice.example.net".to_owned(), 25000)
        );
    }

    #[test]
    fn the_voice_host_falls_back_as_the_mod_does() {
        let mut secret = Secret {
            secret: KEY,
            port: 24454,
            player: Uuid::nil(),
            codec: Codec::Voip,
            mtu: 1024,
            distance: 48.0,
            keep_alive: 1000,
            groups: false,
            voice_host: String::new(),
            allow_recording: false,
        };
        assert_eq!(secret.address("mc.net"), ("mc.net".to_owned(), 24454));
        secret.voice_host = "25565".to_owned();
        assert_eq!(secret.address("mc.net"), ("mc.net".to_owned(), 25565));
        secret.voice_host = "[::1]:7000".to_owned();
        assert_eq!(secret.address("mc.net"), ("::1".to_owned(), 7000));
        secret.voice_host = "relay.net".to_owned();
        assert_eq!(secret.address("mc.net"), ("relay.net".to_owned(), 24454));
    }

    #[test]
    fn player_states_read_with_and_without_a_group() {
        let a = Uuid::from_u128(1);
        let g = Uuid::from_u128(2);
        let mut w = Writer::default();
        w.i32(2);
        w.bool(false).bool(false).uuid(a).byte_array(b"Lux").bool(true).uuid(g);
        w.bool(true).bool(true).uuid(a).byte_array(b"Jet").bool(false);
        let states = PlayerState::read_all(&w.bytes).unwrap();
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].group, Some(g));
        assert_eq!(states[1].name, "Jet");
        assert!(states[1].disabled && states[1].disconnected);
    }

    #[test]
    fn a_client_packet_seals_as_write_client_does() {
        let player = Uuid::from_u128(7);
        let sealed = seal(player, &KEY, &Packet::Authenticate { player, secret: KEY });
        let mut r = Reader::new(&sealed);
        assert_eq!(r.u8().unwrap(), MAGIC);
        assert_eq!(r.uuid().unwrap(), player);
        let payload = r.byte_array(MAX_PACKET).unwrap();
        let body = decrypt(&KEY, payload).unwrap();
        // id 5, the player's id, then the 16 secret bytes.
        assert_eq!(body[0], 0x5);
        assert_eq!(&body[1..17], player.as_bytes());
        assert_eq!(&body[17..], &KEY);
    }

    #[test]
    fn server_packets_open_and_a_wrong_secret_does_not() {
        assert_eq!(open(&KEY, &from_server(&[0x6])), Some(Packet::AuthenticateAck));
        assert_eq!(open(&KEY, &from_server(&[0xA])), Some(Packet::ConnectionCheckAck));
        assert_eq!(open(&[0; SECRET_LEN], &from_server(&[0x6])), None);
        assert_eq!(open(&KEY, b"\x00junk"), None);
    }

    #[test]
    fn a_player_sound_reads_its_flags_and_category() {
        let mut w = Writer::default();
        w.u8(0x2)
            .uuid(Uuid::from_u128(3))
            .uuid(Uuid::from_u128(4))
            .byte_array(&[1, 2, 3])
            .i64(42);
        w.bytes.extend_from_slice(&16.0f32.to_be_bytes());
        w.u8(WHISPER | HAS_CATEGORY).byte_array(b"music");
        let Some(Packet::PlayerSound {
            sound,
            distance,
            whispering,
        }) = open(&KEY, &from_server(&w.bytes))
        else {
            panic!("not a player sound");
        };
        assert_eq!(sound.opus, [1, 2, 3]);
        assert_eq!(sound.sequence, 42);
        assert_eq!(sound.category.as_deref(), Some("music"));
        assert_eq!(distance, 16.0);
        assert!(whispering);
    }

    #[test]
    fn a_location_sound_reads_its_point() {
        let mut w = Writer::default();
        w.u8(0x4).uuid(Uuid::from_u128(3)).uuid(Uuid::from_u128(4));
        for v in [1.5f64, 64.0, -3.0] {
            w.bytes.extend_from_slice(&v.to_be_bytes());
        }
        w.byte_array(&[9]).i64(-1);
        w.bytes.extend_from_slice(&8.0f32.to_be_bytes());
        w.u8(0);
        let Some(Packet::LocationSound { sound, at, distance }) = open(&KEY, &from_server(&w.bytes))
        else {
            panic!("not a location sound");
        };
        assert_eq!(at, [1.5, 64.0, -3.0]);
        assert_eq!(distance, 8.0);
        assert_eq!(sound.sequence, -1);
        assert_eq!(sound.category, None);
    }

    #[test]
    fn a_register_payload_round_trips() {
        let payload = register(&[SECRET, STATE]);
        assert_eq!(payload, b"voicechat:secret\0voicechat:state\0");
        assert_eq!(registered(&payload).collect::<Vec<_>>(), [SECRET, STATE]);
    }

    #[test]
    fn group_packets_read_and_write_as_the_mod_does() {
        let id = Uuid::from_u128(5);
        let mut w = Writer::default();
        w.uuid(id).utf("Builders").bool(true).bool(false).bool(false).i16(2);
        let group = Group::read(&w.bytes).unwrap();
        assert_eq!(group.name, "Builders");
        assert!(group.has_password);
        assert_eq!(group.kind, GroupType::Isolated);

        let mut w = Writer::default();
        w.bool(true).uuid(id).bool(false);
        assert_eq!(read_joined(&w.bytes).unwrap(), (Some(id), false));

        let join = join_group(id, Some("pw"));
        assert_eq!(&join[..16], id.as_bytes());
        assert_eq!(&join[16..], &[1, 2, b'p', b'w']);
        let create = create_group("A", None, GroupType::Open);
        assert_eq!(create, [1, b'A', 0, 0, 1]);
    }

    #[test]
    fn var_ints_round_trip_at_their_edges() {
        for v in [0, 1, 127, 128, 255, 2097151, i32::MAX, -1] {
            let mut w = Writer::default();
            w.var_int(v);
            assert_eq!(Reader::new(&w.bytes).var_int().unwrap(), v, "{v}");
        }
    }
}
