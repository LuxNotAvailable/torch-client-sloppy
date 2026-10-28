fn main() {
    println!("cargo::rustc-check-cfg=cfg(packed_assets)");

    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if arch == "wasm32" || os == "android" {
        println!("cargo::rustc-cfg=packed_assets");
    }

    // Sound is desktop-only: the sounds are 332 MB in an object store a
    // browser cannot reach, and rodio's backend is not built for these
    // targets. The `audio` feature is on by default and simply off there.
    println!("cargo::rustc-check-cfg=cfg(audio)");
    if std::env::var_os("CARGO_FEATURE_AUDIO").is_some() && arch != "wasm32" && os != "android" {
        println!("cargo::rustc-cfg=audio");
    }

    println!("cargo::rustc-check-cfg=cfg(voice)");
    if std::env::var_os("CARGO_FEATURE_VOICE").is_some() && arch != "wasm32" && os != "android" {
        println!("cargo::rustc-cfg=voice");
    }

    println!("cargo::rustc-check-cfg=cfg(resource_packs)");
    println!("cargo::rustc-check-cfg=cfg(explore_modrinth)");
    if std::env::var_os("CARGO_FEATURE_RESOURCEPACKS").is_some()
        && arch != "wasm32"
        && os != "android"
    {
        println!("cargo::rustc-cfg=resource_packs");
        if std::env::var_os("CARGO_FEATURE_EXPLORE_MODRINTH").is_some() {
            println!("cargo::rustc-cfg=explore_modrinth");
        }
    }
}
