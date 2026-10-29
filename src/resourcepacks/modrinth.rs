use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use serde_json::Value;
use sha1::Digest;

use crate::platform::{http, storage};

pub(crate) const USER_AGENT: &str =
    "torch-client/torch-client/0.1.0 (github.com/torch-client/torch-client)";

const API: &str = "https://api.modrinth.com/v2";

pub(crate) const PAGE: usize = 20;

pub(crate) const OLDEST: &str = "1.21.11";

pub(crate) struct Hit {
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub follows: u64,
    pub date_modified: String,
    pub versions: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Sort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl Sort {
    pub const ALL: [Sort; 5] = [
        Sort::Relevance,
        Sort::Downloads,
        Sort::Follows,
        Sort::Newest,
        Sort::Updated,
    ];

    fn index(self) -> &'static str {
        match self {
            Sort::Relevance => "relevance",
            Sort::Downloads => "downloads",
            Sort::Follows => "follows",
            Sort::Newest => "newest",
            Sort::Updated => "updated",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Relevance => "Relevance",
            Sort::Downloads => "Downloads",
            Sort::Follows => "Follows",
            Sort::Newest => "Newest",
            Sort::Updated => "Updated",
        }
    }
}

#[derive(Clone, PartialEq, Default)]
pub(crate) struct Query {
    pub text: String,
    pub sort: Sort,
    pub versions: Vec<String>,
    pub resolution: Option<String>,
}

#[derive(Default)]
pub(crate) struct Tags {
    pub recent: Vec<String>,
    pub resolutions: Vec<String>,
}

fn get_json(url: &str) -> Result<Value, String> {
    let bytes = http::get_as_blocking(url, USER_AGENT)?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn json_param(value: Value) -> String {
    encode(&value.to_string())
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn str_of(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}

fn strings_of(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn search(query: &Query, offset: usize) -> Result<(Vec<Hit>, usize), String> {
    let mut facets = vec![vec!["project_type:resourcepack".to_owned()]];
    if !query.versions.is_empty() {
        facets.push(
            query
                .versions
                .iter()
                .map(|v| format!("versions:{v}"))
                .collect(),
        );
    }
    if let Some(resolution) = &query.resolution {
        facets.push(vec![format!("categories:{resolution}")]);
    }
    let url = format!(
        "{API}/search?query={}&facets={}&index={}&offset={offset}&limit={PAGE}",
        encode(&query.text),
        json_param(Value::from(facets)),
        query.sort.index(),
    );
    let root = get_json(&url)?;
    let hits = root["hits"]
        .as_array()
        .map(|hits| {
            hits.iter()
                .map(|h| Hit {
                    project_id: str_of(h, "project_id"),
                    title: str_of(h, "title"),
                    description: str_of(h, "description"),
                    author: str_of(h, "author"),
                    icon_url: h["icon_url"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                    downloads: h["downloads"].as_u64().unwrap_or(0),
                    follows: h["follows"].as_u64().unwrap_or(0),
                    date_modified: str_of(h, "date_modified"),
                    versions: strings_of(h, "versions"),
                    categories: strings_of(h, "display_categories"),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((hits, root["total_hits"].as_u64().unwrap_or(0) as usize))
}

pub(crate) fn tags() -> Result<Tags, String> {
    let versions = get_json(&format!("{API}/tag/game_version"))?;
    let mut recent = Vec::new();
    for v in versions.as_array().into_iter().flatten() {
        if v["version_type"].as_str() != Some("release") {
            continue;
        }
        let name = str_of(v, "version");
        let last = name == OLDEST;
        recent.push(name);
        if last {
            break;
        }
    }

    let categories = get_json(&format!("{API}/tag/category"))?;
    let mut resolutions: Vec<String> = categories
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| {
            c["project_type"].as_str() == Some("resourcepack")
                && c["header"].as_str() == Some("resolutions")
        })
        .map(|c| str_of(c, "name"))
        .collect();
    resolutions.sort_by_key(|name| {
        name.bytes()
            .take_while(u8::is_ascii_digit)
            .fold(0u32, |n, d| n * 10 + u32::from(d - b'0'))
    });
    Ok(Tags {
        recent,
        resolutions,
    })
}

pub(crate) fn body(project_id: &str) -> Result<String, String> {
    Ok(str_of(
        &get_json(&format!("{API}/project/{}", encode(project_id)))?,
        "body",
    ))
}

pub(crate) fn icon(url: &str) -> Option<Vec<u8>> {
    super::icon_pixels(&http::get_as_blocking(url, USER_AGENT).ok()?)
}

pub(crate) fn download(project_id: &str, versions: &[String]) -> Result<String, String> {
    let mut url = format!(
        "{API}/project/{}/version?loaders={}&include_changelog=false",
        encode(project_id),
        json_param(Value::from(vec!["minecraft"])),
    );
    if !versions.is_empty() {
        url.push_str(&format!(
            "&game_versions={}",
            json_param(Value::from(versions.to_vec()))
        ));
    }
    let list = get_json(&url)?;
    let newest = list
        .as_array()
        .into_iter()
        .flatten()
        .max_by(|a, b| {
            a["date_published"]
                .as_str()
                .cmp(&b["date_published"].as_str())
        })
        .ok_or("no version for these versions")?;
    let files = newest["files"].as_array().ok_or("version has no files")?;
    let file = files
        .iter()
        .find(|f| f["primary"].as_bool() == Some(true))
        .or(files.first())
        .ok_or("version has no files")?;

    let name = str_of(file, "filename");
    if Path::new(&name).file_name().and_then(|n| n.to_str()) != Some(name.as_str())
        || !name.ends_with(".zip")
    {
        return Err(format!("not a pack zip: {name}"));
    }
    let dir = super::dir();
    let dest = dir.join(&name);
    if dest.exists() {
        return Ok(name);
    }

    let bytes = http::get_as_blocking(&str_of(file, "url"), USER_AGENT)?;
    let expected = file["hashes"]["sha1"].as_str().unwrap_or_default();
    if !sha1_hex(&bytes).eq_ignore_ascii_case(expected) {
        return Err("download does not match its hash".into());
    }
    let part = dir.join(format!("{name}.part"));
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&part, &bytes))
        .and_then(|()| std::fs::rename(&part, &dest))
        .map_err(|e| e.to_string())?;
    edit_downloaded(|record| {
        record.insert(name.clone(), project_id.to_owned());
    });
    Ok(name)
}

pub(crate) fn downloaded() -> HashMap<String, String> {
    storage::MODRINTH_PACKS
        .load()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn delete(name: &str) -> Result<String, String> {
    let project = downloaded()
        .remove(name)
        .ok_or("not downloaded from Modrinth")?;
    std::fs::remove_file(super::dir().join(name)).map_err(|e| e.to_string())?;
    edit_downloaded(|record| {
        record.remove(name);
    });
    Ok(project)
}

fn edit_downloaded(edit: impl FnOnce(&mut HashMap<String, String>)) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut record = downloaded();
    edit(&mut record);
    match serde_json::to_string(&record) {
        Ok(text) => storage::MODRINTH_PACKS.store(&text),
        Err(e) => crate::log_warn!("modrinth", "could not encode the download record: {e}"),
    }
}

pub(crate) fn installed_projects() -> Vec<String> {
    let dir = super::dir();
    super::list()
        .into_iter()
        .filter(|pack| {
            Path::new(&pack.name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        })
        .filter_map(|pack| {
            let hash = sha1_hex(&std::fs::read(dir.join(&pack.name)).ok()?);
            let version = get_json(&format!("{API}/version_file/{hash}?algorithm=sha1")).ok()?;
            Some(str_of(&version, "project_id")).filter(|id| !id.is_empty())
        })
        .collect()
}

fn sha1_hex(bytes: &[u8]) -> String {
    sha1::Sha1::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
