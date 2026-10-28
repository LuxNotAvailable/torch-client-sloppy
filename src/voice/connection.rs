//! The UDP side, as `ClientVoicechatConnection` runs it: authenticate once a
//! second until the server acknowledges, then send connection checks the same
//! way until it acknowledges those; after that answer keep-alives and pings,
//! and give up after ten keep-alive periods of silence.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::Status;
use super::wire::{self, Packet, Secret};

/// What arrived that the game plays: a voice, still Opus. The connection
/// hands each one here as it comes, on its own thread.
pub(crate) type OnVoice = Box<dyn FnMut(Packet) + Send>;

pub(crate) struct Connection {
    stop: Arc<AtomicBool>,
}

impl Connection {
    /// Starts the connection thread for `secret`, reaching the voice server
    /// beside `game_host`; once connected it starts the microphone (unless
    /// voice chat is off) and hands voices to `on_voice`.
    pub(crate) fn start(
        secret: Secret,
        game_host: String,
        status: Arc<Mutex<Status>>,
        on_voice: OnVoice,
    ) -> Connection {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let spawned = std::thread::Builder::new()
            .name("voicechat".into())
            .spawn(move || {
                let (outgoing, queued) = std::sync::mpsc::channel();
                let mut on_voice = on_voice;
                let result = run(
                    &secret,
                    &game_host,
                    &status,
                    &thread_stop,
                    (&outgoing, &queued),
                    &mut on_voice,
                );
                let end = match result {
                    Ok(()) => Status::Off,
                    Err(reason) => {
                        crate::log_warn!("voice", "voice chat disconnected: {reason}");
                        Status::Failed(reason)
                    }
                };
                if let Ok(mut s) = status.lock() {
                    *s = end;
                }
            });
        if let Err(e) = spawned {
            crate::log_warn!("voice", "cannot start the voice chat thread: {e}");
        }
        Connection { stop }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

const RETRY: Duration = Duration::from_secs(1);

fn resolve(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host} has no address"))
}

fn run(
    secret: &Secret,
    game_host: &str,
    status: &Mutex<Status>,
    stop: &AtomicBool,
    (outgoing, queued): (&Sender<Packet>, &Receiver<Packet>),
    on_voice: &mut OnVoice,
) -> Result<(), String> {
    let set = |s: Status| {
        if let Ok(mut status) = status.lock() {
            *status = s;
        }
    };
    let (host, port) = secret.address(game_host);
    let server = resolve(&host, port)?;
    let bind: SocketAddr = if server.is_ipv4() {
        "0.0.0.0:0".parse().expect("literal")
    } else {
        "[::]:0".parse().expect("literal")
    };
    let socket = UdpSocket::bind(bind).map_err(|e| format!("cannot open a socket: {e}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    crate::log_info!("voice", "voice chat server at {server}");

    let send = |packet: &Packet| {
        let datagram = wire::seal(secret.player, &secret.secret, packet);
        socket.send_to(&datagram, server).map(|_| ())
    };

    let (mut authenticated, mut connected) = (false, false);
    let mut last_try: Option<Instant> = None;
    let mut last_keep_alive: Option<Instant> = None;
    let timeout = Duration::from_millis(u64::try_from(secret.keep_alive).unwrap_or(1000) * 10);
    let mut buffer = [0u8; wire::MAX_PACKET + 64];
    let mut mic: Option<super::mic::Mic> = None;
    set(Status::Authenticating);

    while !stop.load(Ordering::Relaxed) {
        // `MicThread` runs while connected and voice chat is not off.
        let wanted = connected && !super::disabled();
        let stale = mic
            .as_ref()
            .is_some_and(|m| m.generation != super::input_generation());
        if wanted != mic.is_some() || stale {
            // The old microphone closes first, freeing its device.
            drop(mic.take());
            mic = wanted.then(|| super::mic::Mic::start(secret.codec, outgoing.clone()));
        }
        if !connected && last_try.is_none_or(|at| at.elapsed() >= RETRY) {
            last_try = Some(Instant::now());
            let packet = if authenticated {
                Packet::ConnectionCheck
            } else {
                Packet::Authenticate {
                    player: secret.player,
                    secret: secret.secret,
                }
            };
            send(&packet).map_err(|e| format!("cannot send: {e}"))?;
        }
        if let Some(at) = last_keep_alive
            && at.elapsed() > timeout
        {
            return Err("timed out".into());
        }
        loop {
            match queued.try_recv() {
                Ok(packet) if connected => {
                    send(&packet).map_err(|e| format!("cannot send: {e}"))?;
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }

        let (len, from) = match socket.recv_from(&mut buffer) {
            Ok(got) => got,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            // A port the server closed answers with an ICMP error some
            // systems report on the next read; keep trying until the timeout.
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => continue,
            Err(e) => return Err(format!("cannot read: {e}")),
        };
        if from.ip() != server.ip() {
            continue;
        }
        let Some(packet) = wire::open(&secret.secret, &buffer[..len]) else {
            continue;
        };
        match packet {
            Packet::AuthenticateAck if !authenticated => {
                crate::log_info!("voice", "the voice chat server accepted the secret");
                authenticated = true;
                last_try = None;
            }
            Packet::ConnectionCheckAck if authenticated && !connected => {
                crate::log_info!("voice", "voice chat connected");
                connected = true;
                last_keep_alive = Some(Instant::now());
                set(Status::Connected);
            }
            Packet::KeepAlive => {
                last_keep_alive = Some(Instant::now());
                send(&Packet::KeepAlive).map_err(|e| format!("cannot send: {e}"))?;
            }
            ping @ Packet::Ping { .. } => {
                send(&ping).map_err(|e| format!("cannot send: {e}"))?;
            }
            voice @ (Packet::PlayerSound { .. }
            | Packet::GroupSound { .. }
            | Packet::LocationSound { .. }) => on_voice(voice),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A loopback stand-in for the voice server: acknowledges, then sends a
    /// keep-alive and a voice, as the real one would.
    #[test]
    fn the_handshake_runs_against_a_loopback_server() {
        let _serial = super::super::TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        use super::super::wire::{Codec, Writer};
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        server.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let port = server.local_addr().unwrap().port();
        let key = *b"fedcba9876543210";
        let player = uuid::Uuid::from_u128(99);
        let secret = Secret {
            secret: key,
            port: i32::from(port),
            player,
            codec: Codec::Voip,
            mtu: 1024,
            distance: 48.0,
            keep_alive: 1000,
            groups: false,
            voice_host: String::new(),
            allow_recording: false,
        };
        let status = Arc::new(Mutex::new(Status::Requested));
        let (tx, rx) = std::sync::mpsc::channel();
        // Voice chat off, so no microphone opens on the test machine.
        super::super::set_disabled(true);
        let connection = Connection::start(
            secret,
            "127.0.0.1".into(),
            status.clone(),
            Box::new(move |packet| {
                let _ = tx.send(packet);
            }),
        );

        // What the server's `writeServer` sends: magic, then the encrypted
        // body as a byte array, with no player id.
        let reply = |to: SocketAddr, body: &[u8]| {
            let mut w = Writer::default();
            w.u8(0xFF).byte_array(&wire::encrypt(&key, body));
            server.send_to(&w.bytes, to).unwrap();
        };
        let mut buf = [0u8; 4096];
        let read = |buf: &mut [u8]| -> (Packet, SocketAddr) {
            let (len, from) = server.recv_from(buf).unwrap();
            let mut r = wire::Reader::new(&buf[..len]);
            assert_eq!(r.u8().unwrap(), 0xFF);
            assert_eq!(r.uuid().unwrap(), player);
            let payload = r.byte_array(wire::MAX_PACKET).unwrap();
            let body = wire::decrypt(&key, payload).unwrap();
            let packet = match body[0] {
                0x5 => Packet::Authenticate {
                    player: uuid::Uuid::from_slice(&body[1..17]).unwrap(),
                    secret: body[17..33].try_into().unwrap(),
                },
                0x8 => Packet::KeepAlive,
                0x9 => Packet::ConnectionCheck,
                other => panic!("unexpected packet {other}"),
            };
            (packet, from)
        };

        let (first, from) = read(&mut buf);
        assert_eq!(first, Packet::Authenticate { player, secret: key });
        reply(from, &[0x6]);
        let (check, from) = read(&mut buf);
        assert_eq!(check, Packet::ConnectionCheck);
        reply(from, &[0xA]);
        reply(from, &[0x8]);
        let (answer, _) = read(&mut buf);
        assert_eq!(answer, Packet::KeepAlive);
        assert_eq!(*status.lock().unwrap(), Status::Connected);

        let mut voice = Writer::default();
        voice
            .u8(0x3)
            .uuid(uuid::Uuid::from_u128(1))
            .uuid(uuid::Uuid::from_u128(2))
            .byte_array(&[7, 7])
            .i64(5)
            .u8(0);
        reply(from, &voice.bytes);
        let Packet::GroupSound { sound } = rx.recv_timeout(Duration::from_secs(5)).unwrap() else {
            panic!("no group sound");
        };
        assert_eq!(sound.opus, [7, 7]);
        drop(connection);
    }
}
