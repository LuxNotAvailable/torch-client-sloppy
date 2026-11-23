use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ProtocolVersion {
    pub name: &'static str,
    pub protocol: i32,
}

const fn v(name: &'static str, protocol: i32) -> ProtocolVersion {
    ProtocolVersion { name, protocol }
}

pub(crate) const VERSIONS: &[ProtocolVersion] = &[
    v("26.3", 777),
    v("26.2", 776),
    v("26.1.2", 775),
    v("26.1.1", 775),
    v("26.1", 775),
    v("1.21.11", 774),
    v("1.21.10", 773),
    v("1.21.9", 773),
    v("1.21.1", 767),
    v("1.21", 767),
];

pub(crate) const NATIVE: ProtocolVersion = v("26.3", 777);

/// Newest first, which is the order the version picker lists them in.
const TRANSLATED: &[i32] = &[775, 774];

/// Translated in part: tables and a hop exist, the packet rewrites don't
/// yet (`docs/MULTIVERSION_PLAN.md`). Joinable only while the Online
/// Options toggle is on.
const EXPERIMENTAL: &[i32] = &[767];

static EXPERIMENTAL_ON: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_experimental(on: bool) {
    EXPERIMENTAL_ON.store(on, Ordering::Relaxed);
}

pub(crate) fn experimental_on() -> bool {
    EXPERIMENTAL_ON.load(Ordering::Relaxed)
}

pub(crate) fn is_experimental(protocol: i32) -> bool {
    EXPERIMENTAL.contains(&protocol)
}

/// Whether entity metadata indices for players and mannequins match the native
/// layout. They last moved between 1.21.11 and 26.1. The 767 hop writes
/// native indices itself (`v767::entity_data`).
pub(crate) fn metadata_matches_native(protocol: i32) -> bool {
    protocol >= 775 || protocol == 767
}

pub(crate) fn joinable(protocol: i32) -> bool {
    joinable_with(protocol, experimental_on())
}

fn joinable_with(protocol: i32, experimental: bool) -> bool {
    protocol == NATIVE.protocol
        || TRANSLATED.contains(&protocol)
        || (experimental && is_experimental(protocol))
}

pub(crate) fn selectable() -> impl Iterator<Item = ProtocolVersion> {
    selectable_with(experimental_on())
}

fn selectable_with(experimental: bool) -> impl Iterator<Item = ProtocolVersion> {
    let extra: &[i32] = if experimental { EXPERIMENTAL } else { &[] };
    std::iter::once(NATIVE).chain(
        TRANSLATED
            .iter()
            .chain(extra)
            .filter_map(|protocol| ProtocolVersion::from_protocol(*protocol)),
    )
}

impl ProtocolVersion {
    pub(crate) fn from_protocol(protocol: i32) -> Option<Self> {
        VERSIONS.iter().copied().find(|v| v.protocol == protocol)
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        VERSIONS.iter().copied().find(|v| v.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_is_azaleas_own_version() {
        assert_eq!(NATIVE.protocol, azalea_protocol::packets::PROTOCOL_VERSION);
        assert_eq!(NATIVE.name, crate::ASSET_VERSION);
        assert!(VERSIONS.contains(&NATIVE));
    }

    #[test]
    fn selectable_is_the_joinable_set() {
        let picked: Vec<_> = selectable_with(false).collect();
        assert_eq!(picked[0], NATIVE);
        assert_eq!(picked.len(), 1 + TRANSLATED.len());
        assert!(picked.iter().all(|v| joinable_with(v.protocol, false)));
        assert_eq!(picked[1].name, "26.1.2");
        assert_eq!(picked[2].name, "1.21.11");
    }

    #[test]
    fn experimental_versions_are_listed_and_joinable_only_when_on() {
        assert!(!joinable_with(767, false));
        assert!(joinable_with(767, true));
        let on: Vec<_> = selectable_with(true).collect();
        assert_eq!(on.len(), 1 + TRANSLATED.len() + EXPERIMENTAL.len());
        assert_eq!(on.last().unwrap().name, "1.21.1");
        assert!(on.iter().all(|v| joinable_with(v.protocol, true)));
        assert!(selectable_with(false).all(|v| !is_experimental(v.protocol)));
    }

    #[test]
    fn every_translated_version_has_a_hop_and_tables() {
        for &protocol in TRANSLATED.iter().chain(EXPERIMENTAL) {
            assert!(
                crate::protocol::packets::PacketTable::for_protocol(protocol).is_some(),
                "{protocol} is joinable with no packet table"
            );
            assert!(
                crate::protocol::hop::Translator::for_protocol(protocol).is_some(),
                "{protocol} is joinable with no hop"
            );
        }
        assert!(crate::protocol::hop::Translator::for_protocol(NATIVE.protocol).is_none());
    }

    #[test]
    fn lookups() {
        assert_eq!(ProtocolVersion::from_protocol(777).unwrap().name, "26.3");
        assert_eq!(ProtocolVersion::from_protocol(775).unwrap().name, "26.1.2");
        assert_eq!(ProtocolVersion::from_protocol(773).unwrap().name, "1.21.10");
        assert_eq!(ProtocolVersion::from_name("1.21.11").unwrap().protocol, 774);
        assert_eq!(ProtocolVersion::from_protocol(767).unwrap().name, "1.21.1");
        assert!(ProtocolVersion::from_protocol(47).is_none());
        assert!(ProtocolVersion::from_name("1.8.9").is_none());
    }

    #[test]
    fn a_translated_version_is_joinable_and_an_unknown_one_is_not() {
        for experimental in [false, true] {
            let joinable = |p| joinable_with(p, experimental);
            assert!(joinable(NATIVE.protocol));
            assert!(joinable(775));
            assert!(joinable(774));
            assert!(!joinable(776), "26.2 has no tables and no hop");
            assert!(!joinable(773), "1.21.10 has no tables and no hop");
            assert!(!joinable(47));
        }
    }
}
