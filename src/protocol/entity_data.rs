//! Entity data layouts per version (`data/entity-data-<ver>.json`, from
//! `tools/gen_entity_data.py`), and the index map between two of them.
//!
//! A field is known by its name along the entity's class chain, so a field
//! that moved class (`DATA_PLAYER_MAIN_HAND` into `Avatar`) still matches.

use std::collections::HashMap;

use serde_json::Value;

pub(crate) struct Layouts {
    /// Serializer names by network id.
    pub(crate) serializers: Vec<String>,
    /// Entity type name to `(network id, fields in index order)`; each field
    /// is `(name, serializer)`.
    types: HashMap<String, (i32, Vec<(String, String)>)>,
}

impl Layouts {
    pub(crate) fn parse(json: &str) -> Result<Layouts, String> {
        let file: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let serializers = file["serializers"]
            .as_array()
            .ok_or("no serializers")?
            .iter()
            .map(|s| s.as_str().unwrap_or_default().to_owned())
            .collect();
        let classes = file["classes"].as_object().ok_or("no classes")?;
        let fields_of = |class: &str| -> Vec<(String, String)> {
            let mut chain = Vec::new();
            let mut at = Some(class);
            while let Some(class) = at {
                chain.push(class);
                at = classes.get(class).and_then(|c| c["parent"].as_str());
            }
            chain
                .iter()
                .rev()
                .filter_map(|class| classes.get(*class)?["fields"].as_array())
                .flatten()
                .filter_map(|f| Some((f[0].as_str()?.to_owned(), f[1].as_str()?.to_owned())))
                .collect()
        };
        let mut types = HashMap::new();
        for (name, info) in file["types"].as_object().ok_or("no types")? {
            let id = info["id"].as_i64().ok_or("a type without an id")? as i32;
            let class = info["class"].as_str().ok_or("a type without a class")?;
            types.insert(name.clone(), (id, fields_of(class)));
        }
        Ok(Layouts { serializers, types })
    }

    pub(crate) fn type_id(&self, name: &str) -> Option<i32> {
        self.types.get(name).map(|(id, _)| *id)
    }

    pub(crate) fn serializer_id(&self, name: &str) -> Option<i32> {
        self.serializers
            .iter()
            .position(|s| s == name)
            .map(|i| i as i32)
    }
}

/// Where one old field goes: its new index and serializer name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Moved {
    pub(crate) index: u8,
    pub(crate) serializer: String,
}

/// Per old entity type id, per old index: where that field goes, or `None`
/// when it was dropped. The index comes from `target`'s layout and the
/// serializer from `shape`'s, so a hop can write the native layout while
/// later hops still convert the values' shapes. `renames` maps an old type
/// name to its new one where the type was renamed or split.
pub(crate) fn map(
    old: &Layouts,
    shape: &Layouts,
    target: &Layouts,
    renames: &[(&str, &str)],
) -> HashMap<i32, Vec<Option<Moved>>> {
    let mut out = HashMap::new();
    for (name, (id, fields)) in &old.types {
        let to = renames
            .iter()
            .find(|(from, _)| from == name)
            .map_or(name.as_str(), |(_, to)| to);
        let (Some((_, shape_fields)), Some((_, target_fields))) =
            (shape.types.get(to), target.types.get(to))
        else {
            continue;
        };
        let serializer: HashMap<&str, &str> = shape_fields
            .iter()
            .map(|(f, s)| (f.as_str(), s.as_str()))
            .collect();
        let index: HashMap<&str, usize> = target_fields
            .iter()
            .enumerate()
            .map(|(i, (f, _))| (f.as_str(), i))
            .collect();
        let moved = fields
            .iter()
            .map(|(field, _)| {
                Some(Moved {
                    index: u8::try_from(*index.get(field.as_str())?).ok()?,
                    serializer: (*serializer.get(field.as_str())?).to_owned(),
                })
            })
            .collect();
        out.insert(*id, moved);
    }
    out
}
