//! Converted mods' entity types (`entities.json` in `.torchmod` packages):
//! their sizes, so a modded entity's placeholder box, name tag and hitbox
//! match the real thing until something draws it.

use std::collections::HashMap;

use parking_lot::RwLock;
use serde_json::Value;

/// A modded entity type's `EntityDimensions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
}

/// What a type no package describes is drawn as: a player's box.
pub const FALLBACK: Size = Size {
    width: 0.6,
    height: 1.8,
    eye_height: 1.62,
};

static TYPES: RwLock<Option<HashMap<String, Size>>> = RwLock::new(None);

/// The packages' `entities.json` files, as `(package, text)`. A type two
/// packages both have keeps the first.
pub fn load_packages(packages: &[(String, String)]) {
    let mut out = HashMap::new();
    for (package, text) in packages {
        let Ok(Value::Array(types)) = serde_json::from_str::<Value>(text) else {
            crate::log_warn!("mods", "{package}: its entities.json doesn't read");
            continue;
        };
        for t in &types {
            let Some(name) = t["name"].as_str() else {
                continue;
            };
            let f = |k: &str, d: f32| t[k].as_f64().map_or(d, |v| v as f32);
            let height = f("height", FALLBACK.height);
            out.entry(name.to_owned()).or_insert(Size {
                width: f("width", FALLBACK.width),
                height,
                eye_height: f("eye_height", height * 0.85),
            });
        }
    }
    *TYPES.write() = Some(out);
}

/// A modded entity type's size by its id (`create:contraption`).
pub fn size(name: &str) -> Option<Size> {
    TYPES.read().as_ref()?.get(name).copied()
}

/// A modded entity's size by its raw (native) type id, through the
/// server's synced registry; [`FALLBACK`] when nothing names it.
pub fn size_of_raw(raw: u32) -> Size {
    crate::protocol::modded_entity(raw)
        .and_then(size)
        .unwrap_or(FALLBACK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_s_entity_sizes_read() {
        load_packages(&[(
            "ia.torchmod".into(),
            r#"[{"name": "ia:biplane", "width": 3.5, "height": 2.0, "eye_height": 1.2}]"#.into(),
        )]);
        assert_eq!(
            size("ia:biplane"),
            Some(Size {
                width: 3.5,
                height: 2.0,
                eye_height: 1.2
            })
        );
        assert!(size("ia:nothing").is_none());
    }
}
