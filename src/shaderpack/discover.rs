use crate::util::pack::{Pack, Source};

pub(crate) fn dir() -> std::path::PathBuf {
    match crate::platform::env::var_os("MC_SHADERS_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => crate::platform::storage::dir().join("shaders"),
    }
}

/// Packs from the shaders folder: PC only. Android runs the built-in pack
/// alone (`docs/ANDROID_VS_PC.md`).
pub(crate) const EXTERNAL: bool = cfg!(not(target_os = "android"));

/// Whether this build may load the pack called `name`.
pub(crate) fn allowed(name: &str) -> bool {
    EXTERNAL || name == super::builtin::NAME
}

/// The built-in pack first, then the ones in the shaders folder.
pub(crate) fn list() -> Vec<Pack> {
    let mut packs = vec![Pack {
        name: super::builtin::NAME.to_owned(),
    }];
    if !EXTERNAL {
        return packs;
    }
    packs.extend(
        crate::util::pack::scan(&dir())
            .into_iter()
            .filter(|p| p.name != super::builtin::NAME),
    );
    packs
}

/// The pack called `name`: the built-in one, or one in the shaders folder.
pub(crate) fn open(name: &str) -> Result<Source, String> {
    if name == super::builtin::NAME {
        return Ok(Source::Embedded {
            files: super::builtin::FILES,
        });
    }
    if !EXTERNAL {
        return Err("only the built-in pack runs on Android".to_owned());
    }
    Source::open(&dir().join(name), super::ROOT)
}

#[cfg(test)]
pub(crate) fn installed() -> impl Iterator<Item = (String, Source)> {
    list()
        .into_iter()
        .filter_map(|pack| open(&pack.name).ok().map(|s| (pack.name, s)))
}
