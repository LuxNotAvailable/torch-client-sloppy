use crate::util::pack::{Pack, Source};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn dir() -> std::path::PathBuf {
    crate::platform::storage::dir().join("shaders")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn dir() -> std::path::PathBuf {
    std::path::PathBuf::from("shaders")
}

/// The built-in pack first, then the ones in the shaders folder.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn list() -> Vec<Pack> {
    let mut packs = vec![Pack {
        name: super::builtin::NAME.to_owned(),
    }];
    packs.extend(
        crate::util::pack::scan(&dir())
            .into_iter()
            .filter(|p| p.name != super::builtin::NAME),
    );
    packs
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn list() -> Vec<Pack> {
    vec![Pack {
        name: super::builtin::NAME.to_owned(),
    }]
}

/// The pack called `name`: the built-in one, or one in the shaders folder.
pub(crate) fn open(name: &str) -> Result<Source, String> {
    if name == super::builtin::NAME {
        return Ok(Source::Embedded {
            files: super::builtin::FILES,
        });
    }
    Source::open(&dir().join(name), super::ROOT)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn installed() -> impl Iterator<Item = (String, Source)> {
    list()
        .into_iter()
        .filter_map(|pack| open(&pack.name).ok().map(|s| (pack.name, s)))
}
