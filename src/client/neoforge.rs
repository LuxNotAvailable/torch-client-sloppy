//! The NeoForge handshake, as a NeoForge 26.3.x client runs it
//! (`ClientConfigurationPacketListenerImpl`, `ClientNetworkRegistry`): say
//! this client runs NeoForge, offer the channels it handles, apply the
//! server's frozen registry snapshots and acknowledge its configuration
//! checks. Answers go out from an observer, before the next packet is
//! read, so the channel answer precedes the ping-0 pong as NeoForge needs.
//!
//! A server mod's required channel fails the negotiation. The failure
//! reasons name the channel, then its flow, then its version; each is
//! remembered per server (`neoforge_channels.json`) and claimed on the
//! next try, which reconnects by itself (`take_retry`). A claimed
//! channel's payloads are skipped. Mod-only data inside vanilla packets is
//! still dropped, and mod screens and logic don't run.

use std::collections::HashSet;
use std::sync::Mutex;

use azalea::ecs::prelude::*;
use azalea_chat::FormattedText;
use azalea_chat::translatable_component::PrimitiveOrComponent;
use azalea_registry::identifier::Identifier;
use serde_json::{Map, Value as Json, json};

use crate::protocol::neoforge::{self as wire, CONFIGURATION, Flow, Offer, PLAY};
use crate::protocol::registries::RegistryId;
use crate::{SHARED, log_info, log_warn};

/// Payloads NeoForge reads before any negotiation (`BUILTIN_PAYLOADS`).
const BUILTIN: [&str; 7] = [
    "minecraft:register",
    "minecraft:unregister",
    "neoforge:register",
    "neoforge:network",
    "neoforge:modded_network_setup_failed",
    "c:version",
    "c:register",
];

/// NeoForge's own configuration channels this client handles, all
/// version `1` and optional (`NetworkInitialization`). Config files,
/// split packets and the play channels are left out: nothing here reads
/// them, so the server doesn't send them.
const OWN: [(&str, Option<Flow>); 9] = [
    (
        "neoforge:frozen_registry_sync_start",
        Some(Flow::Clientbound),
    ),
    ("neoforge:frozen_registry", Some(Flow::Clientbound)),
    ("neoforge:frozen_registry_sync_completed", None),
    ("neoforge:known_registry_data_maps", Some(Flow::Clientbound)),
    (
        "neoforge:known_registry_data_maps_reply",
        Some(Flow::Serverbound),
    ),
    ("neoforge:extensible_enum_data", Some(Flow::Clientbound)),
    ("neoforge:extensible_enum_ack", Some(Flow::Serverbound)),
    ("neoforge:feature_flags", Some(Flow::Clientbound)),
    ("neoforge:feature_flags_ack", Some(Flow::Serverbound)),
];

const OWN_VERSION: &str = "1";

/// Channels mods send on without registering them with NeoForge, which
/// kicks a client that didn't list them (as ad hoc channels, in
/// `minecraft:register`). Their payloads are skipped.
const AD_HOC: [&str; 2] = [
    // Iron's Lib, on join ("Invalid player data").
    "irons_lib:sync_patreon_data",
    "irons_lib:sync_attribute_remaps",
];

/// Tries in a row that may each learn more before the client gives up:
/// a channel takes up to three (missing, flow, version), per phase.
const MAX_RETRIES: u32 = 6;

const FAILURE: &str = "neoforge.network.negotiation.failure.";

/// A channel claimed for one server, learned from its failure reasons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Claim {
    id: String,
    version: String,
    flow: Option<Flow>,
}

impl Claim {
    /// A channel the reasons named, as first guessed.
    fn guess(id: &str) -> Claim {
        let own = OWN.iter().find(|(own, _)| *own == id);
        Claim {
            id: id.to_string(),
            version: OWN_VERSION.to_string(),
            flow: own.and_then(|(_, flow)| *flow),
        }
    }
}

#[derive(Default)]
struct Handshake {
    address: String,
    claims: Vec<Claim>,
    /// Answered the server's first `minecraft:register`.
    registered: bool,
    /// Registries the sync named that haven't come yet.
    expected: HashSet<String>,
    snapshots: Vec<(RegistryId, Vec<(u32, Identifier)>)>,
    /// The failure reasons, for the disconnect screen.
    failures: Vec<(String, FormattedText)>,
    /// The reasons taught something new: try again.
    learned: bool,
    /// Every channel the server registered, by protocol, from its query.
    server: Vec<(i32, Vec<Offer>)>,
}

/// What one payload asks of the connection.
#[derive(Default)]
struct Outcome {
    replies: Vec<(&'static str, Vec<u8>)>,
    synced: Vec<(RegistryId, Vec<(u32, Identifier)>)>,
}

#[derive(Default)]
struct Retry {
    address: Option<String>,
    attempts: u32,
}

static HANDSHAKE: Mutex<Option<Handshake>> = Mutex::new(None);
static RETRY: Mutex<Retry> = Mutex::new(Retry {
    address: None,
    attempts: 0,
});

/// A connection to `address` starts: claim what it taught before.
pub(crate) fn begin(address: &str) {
    let claims = load(address);
    if !claims.is_empty() {
        log_info!(
            "neoforge",
            "claiming {} mod channel(s) learned from {address}",
            claims.len()
        );
    }
    *HANDSHAKE.lock().unwrap() = Some(Handshake {
        address: address.to_string(),
        claims,
        ..Handshake::default()
    });
}

/// The address to reconnect to, once a session ends after a failed
/// negotiation taught new channels.
pub(crate) fn take_retry() -> Option<String> {
    let learned = HANDSHAKE
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|h| std::mem::take(&mut h.learned).then(|| h.address.clone()));
    let mut retry = RETRY.lock().unwrap();
    match learned {
        Some(address) if retry.attempts < MAX_RETRIES => {
            retry.attempts += 1;
            log_info!(
                "neoforge",
                "reconnecting to {address} with the learned channels (try {})",
                retry.attempts
            );
            retry.address = Some(address.clone());
            Some(address)
        }
        _ => {
            *retry = Retry::default();
            None
        }
    }
}

/// The failure reasons, one line per channel, for the disconnect screen.
pub(crate) fn take_failure_lines() -> Vec<String> {
    let mut handshake = HANDSHAKE.lock().unwrap();
    let Some(h) = handshake.as_mut() else {
        return Vec::new();
    };
    std::mem::take(&mut h.failures)
        .into_iter()
        .map(|(channel, reason)| format!("{channel}: {reason}"))
        .collect()
}

pub(crate) struct NeoForgePlugin;

impl azalea::app::Plugin for NeoForgePlugin {
    fn build(&self, app: &mut azalea::app::App) {
        app.add_observer(on_config_payload);
    }
}

fn on_config_payload(
    event: On<azalea::packet::config::ConfigCustomPayloadEvent>,
    mut commands: Commands,
) {
    let channel = event.packet.identifier.to_string();
    let outcome = {
        let mut handshake = HANDSHAKE.lock().unwrap();
        let Some(h) = handshake.as_mut() else {
            return;
        };
        h.receive(&channel, &event.packet.data)
    };
    if !outcome.synced.is_empty()
        && let Some(shared) = SHARED.get()
    {
        let mut s = shared.lock().unwrap();
        for (registry, entries) in outcome.synced {
            // Over a hop, ids are renumbered on the way in; the names must be too.
            let entries = crate::protocol::native_snapshot(registry, entries);
            s.session.registries.replace_ids(registry, entries);
        }
    }
    for (channel, data) in outcome.replies {
        use azalea_protocol::packets::config::s_custom_payload::ServerboundCustomPayload;
        commands.trigger(azalea::packet::config::SendConfigPacketEvent::new(
            event.entity,
            ServerboundCustomPayload {
                identifier: Identifier::new(channel),
                data: data.into(),
            },
        ));
    }
}

impl Handshake {
    fn receive(&mut self, channel: &str, data: &[u8]) -> Outcome {
        let mut out = Outcome::default();
        match channel {
            // NeoForge answers only the first, with what it listens on.
            "minecraft:register" if !self.registered => {
                self.registered = true;
                let listening = OWN
                    .iter()
                    .filter(|(_, flow)| *flow != Some(Flow::Serverbound))
                    .map(|(id, _)| *id);
                out.replies.push((
                    "minecraft:register",
                    wire::write_channels(BUILTIN.into_iter().chain(listening).chain(AD_HOC)),
                ));
            }
            "neoforge:register" => {
                self.server = wire::read_query(data).unwrap_or_default();
                out.replies
                    .push(("neoforge:register", wire::query(&self.offers())));
            }
            "neoforge:network" => {
                let Some(setup) = wire::network_setup(data) else {
                    log_warn!("neoforge", "the negotiated channels did not read");
                    return out;
                };
                log_info!(
                    "neoforge",
                    "the server negotiated {} channel(s)",
                    setup.len()
                );
                *RETRY.lock().unwrap() = Retry::default();
                let config = setup
                    .iter()
                    .filter(|(protocol, _)| *protocol == CONFIGURATION)
                    .map(|(_, id)| id.as_str());
                out.replies.push((
                    "minecraft:register",
                    wire::write_channels(BUILTIN.into_iter().chain(config)),
                ));
            }
            "neoforge:modded_network_setup_failed" => {
                let Some(reasons) = wire::setup_failed(data) else {
                    log_warn!("neoforge", "the negotiation failure reasons did not read");
                    return out;
                };
                for (channel, reason) in &reasons {
                    match failure(reason) {
                        Some((key, args)) => {
                            log_warn!("neoforge", "channel {channel} failed: {key} {args:?}")
                        }
                        None => log_warn!("neoforge", "channel {channel} failed: {reason}"),
                    }
                    self.learned |= learn(&mut self.claims, channel, reason);
                }
                if self.learned {
                    save(&self.address, &self.claims);
                }
                self.failures = reasons;
            }
            "c:version" => {
                if wire::common_version(data).is_some_and(|v| !v.contains(&1)) {
                    log_warn!("neoforge", "the server offers no shared network version 1");
                }
                out.replies
                    .push(("c:version", wire::write_common_version(&[1])));
            }
            "c:register" => {
                out.replies
                    .push(("c:register", wire::write_common_register(1, "play", &[])));
            }
            "neoforge:frozen_registry_sync_start" => {
                self.expected = wire::names(data)
                    .unwrap_or_default()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                self.snapshots.clear();
            }
            "neoforge:frozen_registry" => {
                let Some((registry, entries)) = wire::frozen_registry(data) else {
                    log_warn!("neoforge", "a frozen registry snapshot did not read");
                    return out;
                };
                self.expected.remove(&registry.to_string());
                if let Some(id) = RegistryId::from_key(&registry) {
                    self.snapshots.push((id, entries));
                }
            }
            "neoforge:frozen_registry_sync_completed" => {
                if !self.expected.is_empty() {
                    log_warn!(
                        "neoforge",
                        "registries named but never sent: {:?}",
                        self.expected
                    );
                }
                out.synced = std::mem::take(&mut self.snapshots);
                log_info!(
                    "neoforge",
                    "took the server's numbering for {} registr(ies)",
                    out.synced.len()
                );
                out.replies
                    .push(("neoforge:frozen_registry_sync_completed", Vec::new()));
            }
            // The checks below compare against the client's own mods. This
            // client has none, so it acknowledges and notes what won't run.
            "neoforge:feature_flags" => {
                let flags = wire::names(data).unwrap_or_default();
                if !flags.is_empty() {
                    log_info!("neoforge", "mod feature flags: {flags:?}");
                }
                out.replies.push(("neoforge:feature_flags_ack", Vec::new()));
            }
            "neoforge:extensible_enum_data" => {
                for entry in wire::enum_entries(data).unwrap_or_default() {
                    if let Some(added) = entry.added {
                        log_warn!(
                            "neoforge",
                            "{} gained {added:?} ({}): packets naming them may not read",
                            entry.class,
                            entry.check
                        );
                    }
                }
                out.replies
                    .push(("neoforge:extensible_enum_ack", Vec::new()));
            }
            "neoforge:known_registry_data_maps" => {
                for (registry, maps) in wire::known_data_maps(data).unwrap_or_default() {
                    for (map, mandatory) in maps {
                        if mandatory {
                            log_info!("neoforge", "data map {map} ({registry}) not synced");
                        }
                    }
                }
                out.replies
                    .push(("neoforge:known_registry_data_maps_reply", wire::empty_map()));
            }
            _ => {}
        }
        out
    }

    /// The `neoforge:register` answer: NeoForge's channels this client
    /// handles, then the learned ones. Learned channels are offered in
    /// both phases as optional, so the phase without them drops them.
    fn offers(&self) -> Vec<(i32, Vec<Offer>)> {
        let learned = self.claims.iter().map(|claim| Offer {
            id: claim.id.clone(),
            version: claim.version.clone(),
            flow: claim.flow,
            optional: true,
        });
        let own = OWN
            .iter()
            .filter(|(id, _)| !self.claims.iter().any(|c| c.id == *id))
            .map(|(id, flow)| Offer {
                id: id.to_string(),
                version: OWN_VERSION.to_string(),
                flow: *flow,
                optional: true,
            });
        let mut protocols = vec![
            (
                CONFIGURATION,
                own.chain(learned.clone()).collect::<Vec<_>>(),
            ),
            (PLAY, learned.collect()),
        ];
        // The server's other channels, echoed as it registered them: some
        // mods send their payloads without checking the client agreed
        // (Iron's Lib on join), which NeoForge turns into a kick. Their
        // payloads are skipped like a claimed channel's.
        for (protocol, offers) in &mut protocols {
            let theirs = self.server.iter().filter(|(p, _)| p == protocol);
            for offer in theirs.flat_map(|(_, o)| o) {
                if !offers.iter().any(|o| o.id == offer.id) {
                    offers.push(Offer {
                        optional: true,
                        ..offer.clone()
                    });
                }
            }
        }
        protocols
    }
}

/// Updates the claims from one failure reason (`NetworkComponentNegotiator`);
/// true if they changed.
fn learn(claims: &mut Vec<Claim>, channel: &str, reason: &FormattedText) -> bool {
    let Some((key, args)) = failure(reason) else {
        return false;
    };
    let arg = |i: usize| args.get(i).map(String::as_str);
    let at = claims.iter().position(|c| c.id == channel);
    match key.as_str() {
        "missing.server.client" => {
            if at.is_none() {
                claims.push(Claim::guess(channel));
                return true;
            }
            false
        }
        "missing.client.server" => at.map(|i| claims.remove(i)).is_some(),
        _ => {
            // The server's side of a mismatch: its flow, or its version.
            let (flow, version) = match key.as_str() {
                "flow.client.missing" | "flow.client.mismatch" => {
                    (Some(arg(0).and_then(Flow::parse)), None)
                }
                "flow.server.mismatch" => (Some(arg(1).and_then(Flow::parse)), None),
                "flow.server.missing" => (Some(None), None),
                // The server's version is the one that isn't ours: which
                // argument it is depends on the side that ran the check
                // (1.21.1 names it first).
                "version.mismatch" => {
                    let ours = at.map(|i| claims[i].version.as_str());
                    let theirs = [arg(1), arg(0)]
                        .into_iter()
                        .flatten()
                        .find(|v| Some(*v) != ours);
                    (None, theirs)
                }
                _ => return false,
            };
            let i = at.unwrap_or_else(|| {
                claims.push(Claim::guess(channel));
                claims.len() - 1
            });
            let before = claims[i].clone();
            if let Some(flow) = flow {
                claims[i].flow = flow;
            }
            if let Some(version) = version {
                claims[i].version = version.to_string();
            }
            at.is_none() || claims[i] != before
        }
    }
}

/// A negotiation failure's key (after `...failure.`) and arguments as text,
/// unwrapping the per-mod wrapper.
fn failure(reason: &FormattedText) -> Option<(String, Vec<String>)> {
    let FormattedText::Translatable(t) = reason else {
        return None;
    };
    let key = t.key.strip_prefix(FAILURE)?;
    if key == "mod" {
        return match t.args.get(1)? {
            PrimitiveOrComponent::FormattedText(inner) => failure(inner),
            _ => None,
        };
    }
    let args = t
        .args
        .iter()
        .map(|arg| match arg {
            PrimitiveOrComponent::String(s) => s.clone(),
            PrimitiveOrComponent::FormattedText(text) => text.to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    Some((key.to_string(), args))
}

fn server_key(address: &str) -> String {
    address.trim().to_ascii_lowercase()
}

fn load_all() -> Map<String, Json> {
    crate::platform::storage::NEOFORGE_CHANNELS
        .load()
        .and_then(|text| serde_json::from_str::<Json>(&text).ok())
        .and_then(|root| root.get("servers")?.as_object().cloned())
        .unwrap_or_default()
}

fn load(address: &str) -> Vec<Claim> {
    let all = load_all();
    let Some(Json::Array(saved)) = all.get(&server_key(address)) else {
        return Vec::new();
    };
    saved
        .iter()
        .filter_map(|c| {
            Some(Claim {
                id: c.get("id")?.as_str()?.to_string(),
                version: c.get("version")?.as_str()?.to_string(),
                flow: c.get("flow").and_then(Json::as_str).and_then(Flow::parse),
            })
        })
        .collect()
}

fn save(address: &str, claims: &[Claim]) {
    let mut all = load_all();
    let saved = claims
        .iter()
        .map(|c| json!({ "id": c.id, "version": c.version, "flow": c.flow.map(Flow::name) }))
        .collect();
    all.insert(server_key(address), Json::Array(saved));
    let root = json!({ "servers": all });
    crate::platform::storage::NEOFORGE_CHANNELS.store(&root.to_string());
}

#[cfg(test)]
mod tests {
    use azalea_chat::translatable_component::TranslatableComponent;

    use super::*;

    fn reason(key: &str, args: &[&str]) -> FormattedText {
        FormattedText::Translatable(TranslatableComponent::new(
            format!("{FAILURE}{key}"),
            args.iter()
                .map(|a| PrimitiveOrComponent::String(a.to_string()))
                .collect(),
        ))
    }

    fn wrapped(inner: FormattedText) -> FormattedText {
        FormattedText::Translatable(TranslatableComponent::new(
            format!("{FAILURE}mod"),
            vec![
                PrimitiveOrComponent::String("Example Mod".into()),
                PrimitiveOrComponent::FormattedText(inner),
            ],
        ))
    }

    #[test]
    fn a_required_channel_is_learned_then_its_flow_then_its_version() {
        let mut claims = Vec::new();
        let ch = "examplemod:sync";
        assert!(learn(
            &mut claims,
            ch,
            &wrapped(reason("missing.server.client", &[]))
        ));
        assert_eq!(claims, [Claim::guess(ch)]);
        assert!(!learn(
            &mut claims,
            ch,
            &reason("missing.server.client", &[])
        ));

        assert!(learn(
            &mut claims,
            ch,
            &reason("flow.client.missing", &["CLIENTBOUND"])
        ));
        assert_eq!(claims[0].flow, Some(Flow::Clientbound));

        let mismatch = wrapped(reason("version.mismatch", &["1", "3.2"]));
        assert!(learn(&mut claims, ch, &mismatch));
        assert_eq!(claims[0].version, "3.2");

        // NeoForge 1.21.1 names the server's version first.
        let mut first = vec![Claim::guess("create_connected:sync_config")];
        let reversed = wrapped(reason("version.mismatch", &["2.0.0", "1"]));
        assert!(learn(&mut first, "create_connected:sync_config", &reversed));
        assert_eq!(first[0].version, "2.0.0");

        assert!(learn(
            &mut claims,
            ch,
            &reason("missing.client.server", &[])
        ));
        assert!(claims.is_empty());
    }

    #[test]
    fn an_own_channel_keeps_its_flow_when_first_learned() {
        let mut claims = Vec::new();
        let ch = "neoforge:frozen_registry";
        assert!(learn(
            &mut claims,
            ch,
            &reason("version.mismatch", &["1", "2"])
        ));
        assert_eq!(claims[0].flow, Some(Flow::Clientbound));
        assert_eq!(claims[0].version, "2");
    }

    #[test]
    fn other_reasons_teach_nothing() {
        let mut claims = Vec::new();
        let plain = FormattedText::from("disconnected");
        assert!(!learn(&mut claims, "a:b", &plain));
        assert!(!learn(&mut claims, "a:b", &reason("something.else", &[])));
        assert!(claims.is_empty());
    }

    #[test]
    fn the_handshake_answers_register_once_and_the_query_with_claims() {
        let mut h = Handshake {
            claims: vec![Claim {
                id: "examplemod:sync".into(),
                version: "3".into(),
                flow: None,
            }],
            ..Handshake::default()
        };
        let first = h.receive("minecraft:register", b"");
        assert_eq!(first.replies.len(), 1);
        let listening = wire::channels(&first.replies[0].1);
        assert!(listening.contains(&"neoforge:register".to_string()));
        assert!(listening.contains(&"neoforge:frozen_registry".to_string()));
        assert!(!listening.contains(&"neoforge:feature_flags_ack".to_string()));
        assert!(listening.contains(&"irons_lib:sync_patreon_data".to_string()));
        assert!(h.receive("minecraft:register", b"").replies.is_empty());

        let offers = h.offers();
        assert_eq!(offers[0].0, CONFIGURATION);
        assert_eq!(offers[0].1.len(), OWN.len() + 1);
        assert_eq!(offers[1].0, PLAY);
        assert_eq!(offers[1].1[0].id, "examplemod:sync");
        assert!(offers.iter().all(|(_, o)| o.iter().all(|o| o.optional)));
        let answer = h.receive("neoforge:register", b"");
        assert_eq!(answer.replies[0].0, "neoforge:register");
    }

    #[test]
    fn the_answer_echoes_the_servers_own_channels() {
        let mut h = Handshake::default();
        let patreon = Offer {
            id: "irons_lib:sync_patreon_data".into(),
            version: "1.0.0".into(),
            flow: Some(Flow::Clientbound),
            optional: false,
        };
        let query = wire::query(&[(PLAY, vec![patreon.clone()])]);
        let answer = h.receive("neoforge:register", &query);
        let ours = wire::read_query(&answer.replies[0].1).unwrap();
        let play = &ours.iter().find(|(p, _)| *p == PLAY).unwrap().1;
        let echoed = play.iter().find(|o| o.id == patreon.id).unwrap();
        assert_eq!(echoed.version, "1.0.0");
        assert_eq!(echoed.flow, Some(Flow::Clientbound));
    }

    #[test]
    fn a_frozen_sync_is_applied_on_completion_and_acknowledged() {
        let mut h = Handshake::default();
        let mut start = Vec::new();
        crate::protocol::wire::write_varint(&mut start, 1);
        start.push(14);
        start.extend_from_slice(b"minecraft:item");
        h.receive("neoforge:frozen_registry_sync_start", &start);
        assert!(h.expected.contains("minecraft:item"));

        let mut snapshot = start.clone();
        snapshot.remove(0);
        snapshot.extend_from_slice(&[1, 0, 13]);
        snapshot.extend_from_slice(b"minecraft:air");
        snapshot.push(0);
        assert!(
            h.receive("neoforge:frozen_registry", &snapshot)
                .replies
                .is_empty()
        );
        assert!(h.expected.is_empty());

        let done = h.receive("neoforge:frozen_registry_sync_completed", b"");
        assert_eq!(done.synced.len(), 1);
        assert_eq!(done.synced[0].0, RegistryId::Item);
        assert_eq!(done.replies[0].0, "neoforge:frozen_registry_sync_completed");
    }

    #[test]
    fn every_config_check_is_answered() {
        let mut h = Handshake::default();
        for (channel, reply) in [
            ("neoforge:feature_flags", "neoforge:feature_flags_ack"),
            (
                "neoforge:extensible_enum_data",
                "neoforge:extensible_enum_ack",
            ),
            (
                "neoforge:known_registry_data_maps",
                "neoforge:known_registry_data_maps_reply",
            ),
            ("c:version", "c:version"),
            ("c:register", "c:register"),
        ] {
            let out = h.receive(channel, &[0]);
            assert_eq!(out.replies.len(), 1, "{channel}");
            assert_eq!(out.replies[0].0, reply);
        }
    }
}
