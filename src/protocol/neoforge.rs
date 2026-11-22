//! NeoForge's handshake payloads, as NeoForge 26.3.x's `network/payload/`
//! writes them: the channel query and answer, the negotiated setup, the
//! failure reasons, the shared `c:` channels and the configuration tasks'
//! data. Codecs only; `client::neoforge` runs the handshake.

use std::io::Cursor;

use azalea_buf::{AzBuf, AzBufVar};
use azalea_chat::FormattedText;
use azalea_registry::identifier::Identifier;

use super::wire::write_varint;

/// `ConnectionProtocol` ordinals.
pub(crate) const PLAY: i32 = 1;
pub(crate) const CONFIGURATION: i32 = 4;

/// `PacketFlow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    Serverbound,
    Clientbound,
}

impl Flow {
    fn ordinal(self) -> i32 {
        match self {
            Flow::Serverbound => 0,
            Flow::Clientbound => 1,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Flow::Serverbound => "serverbound",
            Flow::Clientbound => "clientbound",
        }
    }

    /// From a failure reason (`SERVERBOUND`) or a saved name.
    pub(crate) fn parse(name: &str) -> Option<Flow> {
        match name.to_ascii_lowercase().as_str() {
            "serverbound" => Some(Flow::Serverbound),
            "clientbound" => Some(Flow::Clientbound),
            _ => None,
        }
    }
}

/// One channel this client offers (`ModdedNetworkQueryComponent`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) flow: Option<Flow>,
    pub(crate) optional: bool,
}

/// `neoforge:register`: the offered channels by protocol.
pub(crate) fn query(protocols: &[(i32, Vec<Offer>)]) -> Vec<u8> {
    let mut out = Vec::new();
    write_count(&mut out, protocols.len());
    for (protocol, offers) in protocols {
        write_varint(&mut out, *protocol);
        write_count(&mut out, offers.len());
        for offer in offers {
            write_str(&mut out, &offer.id);
            write_str(&mut out, &offer.version);
            match offer.flow {
                None => out.push(0),
                Some(flow) => {
                    out.push(1);
                    write_varint(&mut out, flow.ordinal());
                }
            }
            out.push(offer.optional as u8);
        }
    }
    out
}

/// `neoforge:network`: the negotiated channels as `(protocol, id)`.
pub(crate) fn network_setup(data: &[u8]) -> Option<Vec<(i32, String)>> {
    let buf = &mut Cursor::new(data);
    let mut channels = Vec::new();
    for _ in 0..count(buf)? {
        let protocol = varint(buf)?;
        for _ in 0..count(buf)? {
            let _key = id(buf)?;
            let channel = id(buf)?;
            let _version = string(buf)?;
            channels.push((protocol, channel.to_string()));
        }
    }
    done(buf, channels)
}

/// `neoforge:modded_network_setup_failed`: why, by channel.
pub(crate) fn setup_failed(data: &[u8]) -> Option<Vec<(String, FormattedText)>> {
    let buf = &mut Cursor::new(data);
    let mut reasons = Vec::new();
    for _ in 0..count(buf)? {
        let channel = id(buf)?;
        let reason = FormattedText::azalea_read(buf).ok()?;
        reasons.push((channel.to_string(), reason));
    }
    done(buf, reasons)
}

/// `minecraft:register` and `unregister`: names, each ended by a NUL.
pub(crate) fn channels(data: &[u8]) -> Vec<String> {
    data.split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect()
}

pub(crate) fn write_channels<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<u8> {
    let mut out = Vec::new();
    for name in names {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
    }
    out
}

/// `c:version`: the shared networking versions.
pub(crate) fn common_version(data: &[u8]) -> Option<Vec<i32>> {
    let buf = &mut Cursor::new(data);
    let versions = (0..count(buf)?)
        .map(|_| varint(buf))
        .collect::<Option<_>>()?;
    done(buf, versions)
}

pub(crate) fn write_common_version(versions: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    write_count(&mut out, versions.len());
    for version in versions {
        write_varint(&mut out, *version);
    }
    out
}

/// `c:register`: version, protocol name (`play`) and channels.
pub(crate) fn write_common_register(version: i32, protocol: &str, channels: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, version);
    write_str(&mut out, protocol);
    write_count(&mut out, channels.len());
    for channel in channels {
        write_str(&mut out, channel);
    }
    out
}

/// A list of names: `frozen_registry_sync_start`, `feature_flags`.
pub(crate) fn names(data: &[u8]) -> Option<Vec<Identifier>> {
    let buf = &mut Cursor::new(data);
    let names = (0..count(buf)?).map(|_| id(buf)).collect::<Option<_>>()?;
    done(buf, names)
}

/// `frozen_registry`: the registry and its `(id, name)` entries. Aliases
/// are read past; they rename, they don't number.
pub(crate) fn frozen_registry(data: &[u8]) -> Option<(Identifier, Vec<(u32, Identifier)>)> {
    let buf = &mut Cursor::new(data);
    let registry = id(buf)?;
    let mut entries = Vec::new();
    for _ in 0..count(buf)? {
        let raw = u32::try_from(varint(buf)?).ok()?;
        entries.push((raw, id(buf)?));
    }
    for _ in 0..count(buf)? {
        id(buf)?;
        id(buf)?;
    }
    done(buf, (registry, entries))
}

/// One `extensible_enum_data` entry.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EnumEntry {
    pub(crate) class: String,
    pub(crate) check: String,
    /// The entries mods added, if any.
    pub(crate) added: Option<Vec<String>>,
}

pub(crate) fn enum_entries(data: &[u8]) -> Option<Vec<EnumEntry>> {
    let buf = &mut Cursor::new(data);
    let mut entries = Vec::new();
    for _ in 0..count(buf)? {
        let class = string(buf)?;
        let check = string(buf)?;
        let added = match bool::azalea_read(buf).ok()? {
            false => None,
            true => {
                let _vanilla = varint(buf)?;
                let _total = varint(buf)?;
                Some(
                    (0..count(buf)?)
                        .map(|_| string(buf))
                        .collect::<Option<_>>()?,
                )
            }
        };
        entries.push(EnumEntry {
            class,
            check,
            added,
        });
    }
    done(buf, entries)
}

/// `known_registry_data_maps`: by registry, each map and whether it is
/// mandatory.
pub(crate) fn known_data_maps(data: &[u8]) -> Option<Vec<(Identifier, Vec<(Identifier, bool)>)>> {
    let buf = &mut Cursor::new(data);
    let mut registries = Vec::new();
    for _ in 0..count(buf)? {
        let registry = id(buf)?;
        let maps = (0..count(buf)?)
            .map(|_| Some((id(buf)?, bool::azalea_read(buf).ok()?)))
            .collect::<Option<_>>()?;
        registries.push((registry, maps));
    }
    done(buf, registries)
}

/// An empty map: `known_registry_data_maps_reply` from a client that
/// knows none.
pub(crate) fn empty_map() -> Vec<u8> {
    vec![0]
}

fn varint(buf: &mut Cursor<&[u8]>) -> Option<i32> {
    i32::azalea_read_var(buf).ok()
}

/// A length, at most the bytes left (every element takes one or more).
fn count(buf: &mut Cursor<&[u8]>) -> Option<usize> {
    let n = usize::try_from(varint(buf)?).ok()?;
    let left = buf.get_ref().len() - buf.position() as usize;
    (n <= left).then_some(n)
}

fn string(buf: &mut Cursor<&[u8]>) -> Option<String> {
    String::azalea_read(buf).ok()
}

fn id(buf: &mut Cursor<&[u8]>) -> Option<Identifier> {
    Identifier::azalea_read(buf).ok()
}

/// `value` if the payload was read to its end.
fn done<T>(buf: &Cursor<&[u8]>, value: T) -> Option<T> {
    (buf.position() as usize == buf.get_ref().len()).then_some(value)
}

fn write_count(out: &mut Vec<u8>, n: usize) {
    write_varint(out, n as i32);
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    write_count(out, s.len());
    out.extend_from_slice(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_offers(data: &[u8]) -> Vec<(i32, Vec<Offer>)> {
        let buf = &mut Cursor::new(data);
        let mut protocols = Vec::new();
        for _ in 0..count(buf).unwrap() {
            let protocol = varint(buf).unwrap();
            let offers = (0..count(buf).unwrap())
                .map(|_| {
                    let id = id(buf).unwrap().to_string();
                    let version = string(buf).unwrap();
                    let flow =
                        bool::azalea_read(buf)
                            .unwrap()
                            .then(|| match varint(buf).unwrap() {
                                0 => Flow::Serverbound,
                                _ => Flow::Clientbound,
                            });
                    let optional = bool::azalea_read(buf).unwrap();
                    Offer {
                        id,
                        version,
                        flow,
                        optional,
                    }
                })
                .collect();
            protocols.push((protocol, offers));
        }
        assert_eq!(buf.position() as usize, data.len());
        protocols
    }

    #[test]
    fn a_query_reads_back_as_neoforge_writes_it() {
        let offers = vec![
            (
                CONFIGURATION,
                vec![Offer {
                    id: "neoforge:frozen_registry".into(),
                    version: "1".into(),
                    flow: Some(Flow::Clientbound),
                    optional: true,
                }],
            ),
            (
                PLAY,
                vec![Offer {
                    id: "examplemod:sync".into(),
                    version: "2.1".into(),
                    flow: None,
                    optional: true,
                }],
            ),
        ];
        assert_eq!(read_offers(&query(&offers)), offers);
    }

    #[test]
    fn a_network_setup_names_its_channels() {
        let mut data = Vec::new();
        write_count(&mut data, 1);
        write_varint(&mut data, CONFIGURATION);
        write_count(&mut data, 1);
        write_str(&mut data, "neoforge:frozen_registry");
        write_str(&mut data, "neoforge:frozen_registry");
        write_str(&mut data, "1");
        assert_eq!(
            network_setup(&data),
            Some(vec![(CONFIGURATION, "neoforge:frozen_registry".into())])
        );
        data.push(0);
        assert_eq!(network_setup(&data), None, "trailing bytes");
    }

    #[test]
    fn a_frozen_registry_reads_ids_and_skips_aliases() {
        let mut data = Vec::new();
        write_str(&mut data, "minecraft:item");
        write_count(&mut data, 2);
        write_varint(&mut data, 0);
        write_str(&mut data, "minecraft:air");
        write_varint(&mut data, 1400);
        write_str(&mut data, "examplemod:gear");
        write_count(&mut data, 1);
        write_str(&mut data, "examplemod:old_gear");
        write_str(&mut data, "examplemod:gear");
        let (registry, entries) = frozen_registry(&data).unwrap();
        assert_eq!(registry.to_string(), "minecraft:item");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].0, 1400);
        assert_eq!(entries[1].1.to_string(), "examplemod:gear");
    }

    #[test]
    fn channel_lists_round_trip_and_skip_empty_names() {
        let data = write_channels(["c:version", "c:register"]);
        assert_eq!(channels(&data), ["c:version", "c:register"]);
        assert_eq!(channels(b"a:b\0\0c:d"), ["a:b", "c:d"]);
    }

    #[test]
    fn enum_entries_and_data_maps_read() {
        let mut data = Vec::new();
        write_count(&mut data, 2);
        write_str(&mut data, "net.minecraft.world.item.Rarity");
        write_str(&mut data, "BIDIRECTIONAL");
        data.push(1);
        write_varint(&mut data, 4);
        write_varint(&mut data, 5);
        write_count(&mut data, 1);
        write_str(&mut data, "EXAMPLEMOD_MYTHIC");
        write_str(&mut data, "net.minecraft.Other");
        write_str(&mut data, "CLIENTBOUND");
        data.push(0);
        let entries = enum_entries(&data).unwrap();
        assert_eq!(
            entries[0].added.as_deref(),
            Some(&["EXAMPLEMOD_MYTHIC".to_string()][..])
        );
        assert_eq!(entries[1].added, None);

        let mut data = Vec::new();
        write_count(&mut data, 1);
        write_str(&mut data, "minecraft:item");
        write_count(&mut data, 1);
        write_str(&mut data, "neoforge:compostables");
        data.push(1);
        let maps = known_data_maps(&data).unwrap();
        assert_eq!(maps[0].1[0].0.to_string(), "neoforge:compostables");
        assert!(maps[0].1[0].1);
    }

    #[test]
    fn a_huge_count_is_refused_not_allocated() {
        let mut data = Vec::new();
        write_varint(&mut data, i32::MAX);
        assert_eq!(names(&data), None);
        assert_eq!(common_version(&data), None);
    }
}
