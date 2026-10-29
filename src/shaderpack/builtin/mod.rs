//! Torch Vibrant, the pack built into the client: Bedrock's Vibrant Visuals
//! as a lightweight native pack. Its programs are WGSL written against the
//! engine (see `renderer::packdraw::compile::native`), so nothing is
//! translated, and its terrain is shaded per block-face texel through a
//! cache instead of per pixel.

pub(crate) const NAME: &str = "Torch Vibrant (built-in)";

macro_rules! files {
    ($($path:literal),* $(,)?) => {
        &[$((concat!("/", $path), include_str!(concat!("pack/", $path)))),*]
    };
}

pub(crate) const FILES: &[(&str, &str)] = files![
    "shaders.properties",
    "block.properties",
    "lib/common.wgsl",
    "lib/shadow.wgsl",
    "lib/forward.wgsl",
    "lib/cache.wgsl",
    "lib/resolve.wgsl",
    "lang/en_us.lang",
    "shadow.wgsl",
    "gbuffers_terrain.wgsl",
    "gbuffers_water.wgsl",
    "gbuffers_textured.wgsl",
    "gbuffers_block.wgsl",
    "gbuffers_basic.wgsl",
    "gbuffers_damagedblock.wgsl",
    "gbuffers_skybasic.wgsl",
    "gbuffers_skytextured.wgsl",
    "prepare.wgsl",
    "deferred_a.wgsl",
    "deferred_b.wgsl",
    "deferred.wgsl",
    "composite.wgsl",
    "composite1.wgsl",
    "composite2.wgsl",
    "final.wgsl",
];

#[cfg(test)]
mod tests {
    use crate::renderer::dimension::Dimension;
    use crate::renderer::packdraw::compile::compile_frame;

    #[test]
    fn every_file_on_disk_is_embedded() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shaderpack/builtin/pack");
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
                assert_eq!(frame.constants.shadow_map_resolution, 2048);
                assert!(frame.buffers.iter().any(|b| b.index == 0));
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
        assert_eq!(defaults.targets.settings[4].resolve_size([1200, 600]), [600, 300]);
        assert_eq!(defaults.targets.settings[5].resolve_size([1200, 600]), [600, 300]);
        let scaled = at("REFLECTION_RES=4
VOLUMETRIC_RES=1
SCALED_RESOLVE=1
");
        assert_eq!(scaled.targets.settings[5].resolve_size([1200, 600]), [300, 150]);
        assert_eq!(scaled.targets.settings[4].resolve_size([1200, 600]), [1200, 600]);
    }
}
