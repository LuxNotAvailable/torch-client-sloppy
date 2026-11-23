//! Rewrites a jar's block models that need a NeoForge loader into plain
//! ones the client reads: `neoforge:obj` becomes `torch_quads` (free quads
//! baked from the OBJ), `neoforge:composite` becomes one model with every
//! child's elements. Others with a loader are left as they are.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

/// The jar's assets by key, rewritten in place.
/// A jar's assets by key.
pub struct Lookup<'a> {
    assets: &'a [(String, Vec<u8>)],
    index: &'a HashMap<String, usize>,
}

impl<'a> Lookup<'a> {
    fn get(&self, key: &str) -> Option<&'a [u8]> {
        self.index.get(key).map(|&i| self.assets[i].1.as_slice())
    }
}

/// The jar's assets, rewritten in place. How many OBJ and composite models
/// became plain.
pub fn rewrite(assets: &mut [(String, Vec<u8>)]) -> (usize, usize) {
    let index: HashMap<String, usize> = assets
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.clone(), i))
        .collect();
    let mut changes = Vec::new();
    {
        let lookup = Lookup {
            assets,
            index: &index,
        };
        for (key, bytes) in assets.iter() {
            if !key.ends_with(".json") || !key.contains("/models/") {
                continue;
            }
            let Ok(Value::Object(model)) = serde_json::from_slice::<Value>(bytes) else {
                continue;
            };
            let new = match model.get("loader").and_then(Value::as_str) {
                Some("neoforge:obj" | "forge:obj") => obj_model(&model, &lookup).map(|m| (m, 0)),
                Some("neoforge:composite" | "forge:composite") => {
                    composite_model(&model, &lookup).map(|m| (m, 1))
                }
                _ => None,
            };
            if let Some((model, kind)) = new {
                let bytes = serde_json::to_vec(&Value::Object(model)).unwrap_or_default();
                changes.push((key.clone(), bytes, kind));
            }
        }
    }
    let mut counts = (0, 0);
    for (key, bytes, kind) in changes {
        if let Some(&i) = index.get(&key) {
            assets[i].1 = bytes;
            if kind == 0 {
                counts.0 += 1;
            } else {
                counts.1 += 1;
            }
        }
    }
    counts
}

/// `ns:path` (or `path` in `minecraft`) as the asset key under `folder`.
fn asset_key(id: &str, folder: &str, ext: &str) -> String {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    if folder.is_empty() {
        format!("assets/{ns}/{path}{ext}")
    } else {
        format!("assets/{ns}/{folder}/{path}{ext}")
    }
}

fn obj_model(model: &Map<String, Value>, lookup: &Lookup) -> Option<Map<String, Value>> {
    let obj_id = model.get("model")?.as_str()?;
    // `create:models/block/x.obj`: a path under `assets/<ns>/`.
    let obj_key = asset_key(obj_id, "", "");
    let obj = String::from_utf8_lossy(lookup.get(&obj_key)?).into_owned();
    let flip_v = model
        .get("flip_v")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let dir = obj_key.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
    let mut materials: HashMap<String, String> = HashMap::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut material = String::from("#missingno");
    let mut quads = Vec::new();
    for line in obj.lines() {
        let mut parts = line.split_whitespace();
        let Some(tag) = parts.next() else { continue };
        let nums = |parts: std::str::SplitWhitespace| -> Vec<f32> {
            parts.filter_map(|p| p.parse().ok()).collect()
        };
        match tag {
            "mtllib" => {
                for name in parts {
                    if let Some(bytes) = lookup.get(&format!("{dir}/{name}")) {
                        read_mtl(&String::from_utf8_lossy(bytes), &mut materials);
                    }
                }
            }
            "usemtl" => {
                let name = parts.next().unwrap_or_default();
                material = materials
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| format!("#{name}"));
            }
            "v" => {
                let n = nums(parts);
                if n.len() >= 3 {
                    positions.push([n[0], n[1], n[2]]);
                }
            }
            "vt" => {
                let n = nums(parts);
                if n.len() >= 2 {
                    uvs.push([n[0], if flip_v { 1.0 - n[1] } else { n[1] }]);
                }
            }
            "f" => {
                let corners: Vec<([f32; 3], [f32; 2])> = parts
                    .filter_map(|c| {
                        let mut idx = c.split('/');
                        let at = |i: Option<&str>, len: usize| -> Option<usize> {
                            let i: i64 = i?.parse().ok()?;
                            let i = if i < 0 { len as i64 + i } else { i - 1 };
                            usize::try_from(i).ok().filter(|i| *i < len)
                        };
                        let p = positions[at(idx.next(), positions.len())?];
                        let t = at(idx.next(), uvs.len()).map_or([0.0, 0.0], |i| uvs[i]);
                        Some((p, t))
                    })
                    .collect();
                // Quads as they are; anything else as a fan of triangles,
                // each a quad with its last corner doubled.
                let mut emit = |c: [&([f32; 3], [f32; 2]); 4]| {
                    quads.push(json!({
                        "vertices": c.iter().map(|(p, t)| json!([p[0] * 16.0, p[1] * 16.0, p[2] * 16.0, t[0] * 16.0, t[1] * 16.0])).collect::<Vec<_>>(),
                        "texture": material,
                    }));
                };
                if corners.len() == 4 {
                    emit([&corners[0], &corners[1], &corners[2], &corners[3]]);
                } else {
                    for i in 1..corners.len().saturating_sub(1) {
                        emit([&corners[0], &corners[i], &corners[i + 1], &corners[i + 1]]);
                    }
                }
            }
            _ => {}
        }
    }
    if quads.is_empty() {
        return None;
    }
    let mut out = plain(model);
    out.insert("torch_quads".into(), Value::Array(quads));
    Some(out)
}

fn read_mtl(text: &str, out: &mut HashMap<String, String>) {
    let mut current = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("newmtl") => current = parts.next().map(str::to_owned),
            Some("map_Kd") => {
                if let (Some(name), Some(tex)) = (&current, parts.next()) {
                    out.insert(name.clone(), tex.to_owned());
                }
            }
            _ => {}
        }
    }
}

/// The model without the loader's own keys.
fn plain(model: &Map<String, Value>) -> Map<String, Value> {
    let mut out = model.clone();
    for key in [
        "loader",
        "model",
        "flip_v",
        "children",
        "item_render_order",
        "emissive_ambient",
        "automatic_culling",
        "shade_quads",
        "render_type",
        "visibility",
    ] {
        out.remove(key);
    }
    out
}

/// A model's elements and textures, following `parent`s inside the jar.
fn resolve(model: &Map<String, Value>, lookup: &Lookup) -> (Vec<Value>, Map<String, Value>) {
    let mut elements = Vec::new();
    let mut textures = Map::new();
    let mut current = Some(model.clone());
    for _ in 0..16 {
        let Some(node) = current.take() else { break };
        if let Some(Value::Object(t)) = node.get("textures") {
            for (k, v) in t {
                textures.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        if elements.is_empty()
            && let Some(Value::Array(e)) = node.get("elements")
        {
            elements = e.clone();
        }
        current = node
            .get("parent")
            .and_then(Value::as_str)
            .and_then(|p| lookup.get(&asset_key(p, "models", ".json")))
            .and_then(|b| serde_json::from_slice(b).ok());
    }
    (elements, textures)
}

/// Follows `#name` through `maps` (first map wins) to a texture id.
fn texture_of(reference: &str, maps: &[&Map<String, Value>]) -> Option<String> {
    let mut name = reference.strip_prefix('#')?.to_owned();
    for _ in 0..8 {
        let value = maps.iter().find_map(|m| m.get(&name))?;
        let value = value.as_str().or_else(|| value.get("sprite")?.as_str())?;
        match value.strip_prefix('#') {
            Some(next) => name = next.to_owned(),
            None => return Some(value.to_owned()),
        }
    }
    None
}

fn composite_model(model: &Map<String, Value>, lookup: &Lookup) -> Option<Map<String, Value>> {
    let children = model.get("children")?.as_object()?;
    let own_textures = model
        .get("textures")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut textures = own_textures.clone();
    let mut elements = Vec::new();
    for (n, child) in children.values().enumerate() {
        let child = match child {
            Value::Object(c) => c.clone(),
            Value::String(parent) => {
                let mut c = Map::new();
                c.insert("parent".into(), json!(parent));
                c
            }
            _ => continue,
        };
        let (child_elements, child_textures) = resolve(&child, lookup);
        for mut element in child_elements {
            if let Some(Value::Object(faces)) = element.get_mut("faces") {
                for face in faces.values_mut() {
                    let Some(reference) = face.get("texture").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(resolved) = texture_of(reference, &[&child_textures, &own_textures])
                    else {
                        continue;
                    };
                    let key = format!("c{n}_{}", reference.trim_start_matches('#'));
                    textures.insert(key.clone(), json!(resolved));
                    face["texture"] = json!(format!("#{key}"));
                }
            }
            elements.push(element);
        }
    }
    if elements.is_empty() {
        return None;
    }
    let mut out = plain(model);
    out.insert("textures".into(), Value::Object(textures));
    out.insert("elements".into(), Value::Array(elements));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_obj_model_becomes_free_quads_in_sixteenths() {
        let mut assets = vec![
            (
                "assets/m/models/block/w.json".to_owned(),
                br##"{"parent": "m:block/w_tex", "loader": "neoforge:obj", "flip_v": true, "model": "m:models/block/w.obj"}"##.to_vec(),
            ),
            (
                "assets/m/models/block/w.obj".to_owned(),
                b"mtllib w.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl a\nf 1/1 2/2 3/3 4/4\nf 1/1 2/2 3/3\n".to_vec(),
            ),
            ("assets/m/models/block/w.mtl".to_owned(), b"newmtl a\nmap_Kd #side\n".to_vec()),
        ];
        assert_eq!(rewrite(&mut assets), (1, 0));
        let model: Value = serde_json::from_slice(&assets[0].1).unwrap();
        assert!(model.get("loader").is_none());
        assert_eq!(model["parent"], "m:block/w_tex");
        let quads = model["torch_quads"].as_array().unwrap();
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0]["texture"], "#side");
        assert_eq!(quads[0]["vertices"][2], json!([16.0, 16.0, 0.0, 16.0, 0.0]));
        // A triangle doubles its last corner.
        assert_eq!(quads[1]["vertices"][2], quads[1]["vertices"][3]);
    }

    #[test]
    fn a_composite_model_becomes_one_with_every_childs_elements() {
        let mut assets = vec![(
            "assets/m/models/block/c.json".to_owned(),
            br##"{"loader": "neoforge:composite", "parent": "block/block", "textures": {"particle": "m:block/p", "shared": "m:block/s"},
                "children": {
                  "a": {"textures": {"0": "m:block/a"}, "elements": [{"from": [0,0,0], "to": [16,1,16], "faces": {"up": {"texture": "#0"}}}]},
                  "b": {"textures": {"0": "m:block/b"}, "elements": [{"from": [0,1,0], "to": [16,2,16], "faces": {"up": {"texture": "#0"}, "down": {"texture": "#shared"}}}]}
                }}"##
                .to_vec(),
        )];
        assert_eq!(rewrite(&mut assets), (0, 1));
        let model: Value = serde_json::from_slice(&assets[0].1).unwrap();
        assert!(model.get("children").is_none());
        let elements = model["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        let up = |i: usize| {
            elements[i]["faces"]["up"]["texture"]
                .as_str()
                .unwrap()
                .trim_start_matches('#')
                .to_owned()
        };
        assert_eq!(model["textures"][up(0)], "m:block/a");
        assert_eq!(model["textures"][up(1)], "m:block/b");
        let down = elements[1]["faces"]["down"]["texture"]
            .as_str()
            .unwrap()
            .trim_start_matches('#');
        assert_eq!(model["textures"][down], "m:block/s");
        assert_eq!(model["textures"]["particle"], "m:block/p");
    }
}
