//! Torch Vibrant, the pack built into the client: Bedrock's Vibrant Visuals
//! as a lightweight native pack. Its programs are Slang written against the
//! engine (`iris/`, see `shaderpack::compile::native`), compiled to SPIR-V
//! by build.rs, so nothing is translated at run time, and its terrain is
//! shaded per block-face texel through a cache instead of per pixel. The
//! sources are embedded too: the engine reads their directives, options
//! and the resources they name.

pub(crate) const NAME: &str = "Torch Vibrant (built-in)";

macro_rules! files {
    ($($path:literal),* $(,)?) => {
        &[$((concat!("/", $path), include_str!(concat!("pack/", $path)))),*]
    };
}

pub(crate) const FILES: &[(&str, &str)] = files![
    "shaders.properties",
    "block.properties",
    "lib/common.slang",
    "lib/shadow.slang",
    "lib/forward.slang",
    "lib/cache.slang",
    "lib/resolve.slang",
    "lib/water.slang",
    "lang/en_us.lang",
    "shadow.slang",
    "gbuffers_terrain.slang",
    "gbuffers_water.slang",
    "gbuffers_textured.slang",
    "gbuffers_clouds.slang",
    "gbuffers_block.slang",
    "gbuffers_basic.slang",
    "gbuffers_damagedblock.slang",
    "gbuffers_weather.slang",
    "gbuffers_skybasic.slang",
    "gbuffers_skytextured.slang",
    "prepare.slang",
    "deferred_a.slang",
    "deferred_b.slang",
    "deferred.slang",
    "composite_a.slang",
    "composite.slang",
    "composite1_a.slang",
    "composite1.slang",
    "composite2.slang",
    "final.slang",
];

#[cfg(test)]
mod tests {
    use crate::client::dimension::Dimension;
    use crate::shaderpack::compile::compile_frame;

    #[test]
    fn every_file_on_disk_is_embedded() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaderpack/builtin/pack");
        let mut on_disk = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("readable") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(&root).expect("under root");
                    on_disk.push(format!("/{}", rel.to_string_lossy().replace('\\', "/")));
                }
            }
        }
        for file in on_disk {
            assert!(
                super::FILES.iter().any(|(at, _)| *at == file),
                "{file} is not in FILES"
            );
        }
    }

    #[test]
    fn the_pack_compiles_in_every_dimension_and_mode() {
        for dimension in [Dimension::Overworld, Dimension::Nether, Dimension::End] {
            for direct in [false, true] {
                let frame = compile_frame(super::NAME, direct, None, dimension)
                    .unwrap_or_else(|e| panic!("{dimension:?} direct={direct}: {e}"));
                let names: Vec<&str> = frame.programs.iter().map(|p| p.name.as_str()).collect();
                for wanted in [
                    "gbuffers_terrain",
                    "gbuffers_water",
                    "deferred_a",
                    "deferred_b",
                    "deferred",
                    "composite1",
                    "final",
                    "shadow",
                ] {
                    assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
                }
                assert!(
                    frame.notes.iter().all(|n| !n.contains("did not compile")),
                    "{:?}",
                    frame.notes
                );
                assert_eq!(frame.constants.shadow_map_resolution, 1024);
                assert!(frame.buffers.iter().any(|b| b.index == 0));
                // The texel target's 8-byte words.
                assert_eq!(
                    frame.targets.settings[2].format,
                    crate::shaderpack::directives::TargetFormat::Rgba16Unorm
                );
            }
        }
    }

    #[test]
    fn the_shadow_map_size_and_reach_are_options_the_presets_can_set() {
        let source = crate::shaderpack::discover::open(super::NAME).expect("embedded");
        let texts = crate::shaderpack::include::option_texts(&source);
        let declared = crate::shaderpack::options::discover(texts.iter().map(String::as_str));
        for (name, values) in [
            ("shadowMapResolution", ["1024", "2048", "4096"]),
            ("shadowDistance", ["64.0", "96.0", "192.0"]),
        ] {
            let option = declared
                .get(name)
                .unwrap_or_else(|| panic!("{name} is not an option"));
            for value in values {
                assert!(option.accepts(value), "{name} refuses {value}");
            }
        }
    }

    #[test]
    fn the_scale_options_size_the_traced_targets() {
        let source = crate::shaderpack::discover::open(super::NAME).expect("embedded");
        let texts = crate::shaderpack::include::option_texts(&source);
        let declared = crate::shaderpack::options::discover(texts.iter().map(String::as_str));
        let at = |text: &str| {
            let values = crate::shaderpack::options::Values::load(text, &declared);
            compile_frame(super::NAME, false, Some(values), Dimension::Overworld)
                .unwrap_or_else(|e| panic!("{text}: {e}"))
        };
        let defaults = at("");
        assert_eq!(
            defaults.targets.settings[4].resolve_size([1200, 600]),
            [600, 300]
        );
        let scaled = at("VOLUMETRIC_RES=1
SCALED_RESOLVE=1
");
        assert_eq!(
            scaled.targets.settings[4].resolve_size([1200, 600]),
            [1200, 600]
        );
    }

    #[test]
    fn the_voxel_method_asks_for_voxels() {
        let source = crate::shaderpack::discover::open(super::NAME).expect("embedded");
        let texts = crate::shaderpack::include::option_texts(&source);
        let declared = crate::shaderpack::options::discover(texts.iter().map(String::as_str));
        let at = |text: &str| {
            let values = crate::shaderpack::options::Values::load(text, &declared);
            compile_frame(super::NAME, false, Some(values), Dimension::Overworld)
                .unwrap_or_else(|e| panic!("{text}: {e}"))
                .constants
        };
        assert!(at("").voxels, "World Traced is the default");
        assert!(
            !at("REFLECTION_METHOD=0
")
            .voxels
        );
        assert!(at("REFLECTION_METHOD=1\n").voxels);
        assert!(!at("REFLECTION_METHOD=1\nWATER_REFLECTIONS=0\n").voxels);
        assert_eq!(at("REFLECTION_METHOD=1\n").voxel_reach, 128);
        assert_eq!(
            at("REFLECTION_METHOD=1\nREFLECTION_DISTANCE=512\n").voxel_reach,
            512
        );
    }

    #[test]
    fn a_bubble_column_shades_as_water() {
        use azalea::block::BlockState;
        use azalea_registry::builtin::BlockKind;
        let frame =
            compile_frame(super::NAME, false, None, Dimension::Overworld).expect("built-in");
        let ids = frame.block_ids.expect("the pack draws terrain");
        let water = ids.of(BlockState::from(BlockKind::Water).id() as u16);
        assert_ne!(water, crate::shaderpack::packvertex::UNMAPPED);
        assert_eq!(
            ids.of(BlockState::from(BlockKind::BubbleColumn).id() as u16),
            water
        );
    }

    #[test]
    fn every_foliage_shading_mode_compiles() {
        let source = crate::shaderpack::discover::open(super::NAME).expect("embedded");
        let texts = crate::shaderpack::include::option_texts(&source);
        let declared = crate::shaderpack::options::discover(texts.iter().map(String::as_str));
        for mode in ["0", "1", "2"] {
            assert!(
                declared
                    .get("FOLIAGE_SHADING")
                    .expect("declared")
                    .accepts(mode)
            );
            let values = crate::shaderpack::options::Values::load(
                &format!(
                    "FOLIAGE_SHADING={mode}
"
                ),
                &declared,
            );
            let frame = compile_frame(super::NAME, false, Some(values), Dimension::Overworld)
                .unwrap_or_else(|e| panic!("mode {mode}: {e}"));
            assert!(
                frame.notes.iter().all(|n| !n.contains("did not compile")),
                "mode {mode}: {:?}",
                frame.notes
            );
        }
    }

    #[test]
    fn on_android_the_pack_drops_the_shadow_cache_and_still_compiles() {
        crate::shaderpack::features::OS_OVERRIDE.set(Some("MC_OS_ANDROID"));
        let frame = compile_frame(super::NAME, false, None, Dimension::Overworld);
        crate::shaderpack::features::OS_OVERRIDE.set(None);
        let frame = frame.unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<&str> = frame.programs.iter().map(|p| p.name.as_str()).collect();
        for cache_pass in ["prepare", "deferred_a", "deferred_b"] {
            assert!(!names.contains(&cache_pass), "{cache_pass} in {names:?}");
        }
        for wanted in ["deferred", "composite1", "final", "shadow"] {
            assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
        }
        assert!(frame.buffers.iter().all(|b| b.index != 0));
        // RGBA16 may fall back to a float format there, too narrow for the words.
        assert_eq!(
            frame.targets.settings[2].format,
            crate::shaderpack::directives::TargetFormat::Rgba32Float
        );
        assert!(
            frame.notes.iter().all(|n| !n.contains("did not compile")),
            "{:?}",
            frame.notes
        );
    }
}
