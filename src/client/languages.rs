//! Vanilla's languages: the list (from the asset index's `pack.mcmeta`) and
//! each language's strings, fetched from Mojang's object store on demand and
//! kept in the config folder. The chosen language is laid over English for
//! every translated string: chat, items, entities, death messages.

use std::collections::HashMap;
use std::sync::Mutex;

/// One language: its code, name and region.
#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    pub code: String,
    pub name: String,
    pub region: String,
}

#[derive(Clone, Debug, Default)]
pub enum ListState {
    #[default]
    Idle,
    Fetching,
    Ready(Vec<Language>),
    Failed(String),
}

struct Index {
    languages: Vec<Language>,
    /// Each language file's object hash.
    hashes: HashMap<String, String>,
}

static LIST: Mutex<ListState> = Mutex::new(ListState::Idle);
static INDEX: Mutex<Option<Index>> = Mutex::new(None);
/// A language being fetched, and one that failed.
static DOWNLOADING: Mutex<Option<String>> = Mutex::new(None);

fn dir() -> std::path::PathBuf {
    crate::platform::storage::dir().join("lang")
}

pub fn list() -> ListState {
    LIST.lock().unwrap().clone()
}

pub fn downloading() -> Option<String> {
    DOWNLOADING.lock().unwrap().clone()
}

/// Whether a language's strings are on disk.
pub fn installed(code: &str) -> bool {
    code == "en_us" || dir().join(format!("{code}.json")).exists()
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    let bytes = crate::platform::http::get_bytes_blocking(url)?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn object_url(hash: &str) -> String {
    format!("https://resources.download.minecraft.net/{}/{hash}", &hash[..2.min(hash.len())])
}

/// Reads the version's asset index for the languages, once.
pub fn refresh() {
    {
        let mut list = LIST.lock().unwrap();
        if matches!(*list, ListState::Fetching | ListState::Ready(_)) {
            return;
        }
        *list = ListState::Fetching;
    }
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(|| {
        let result = (|| -> Result<Index, String> {
            let manifest = fetch_json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?;
            let version = crate::ASSET_VERSION;
            let url = manifest["versions"]
                .as_array()
                .and_then(|v| v.iter().find(|e| e["id"] == version))
                .and_then(|e| e["url"].as_str())
                .ok_or_else(|| format!("{version} is not in Mojang's list"))?
                .to_owned();
            let meta = fetch_json(&url)?;
            let index_url = meta["assetIndex"]["url"].as_str().ok_or("no asset index")?.to_owned();
            let index = fetch_json(&index_url)?;
            let objects = index["objects"].as_object().ok_or("no objects")?;
            let mut hashes = HashMap::new();
            for (key, value) in objects {
                if let Some(code) = key.strip_prefix("minecraft/lang/").and_then(|k| k.strip_suffix(".json"))
                    && let Some(hash) = value["hash"].as_str()
                {
                    hashes.insert(code.to_owned(), hash.to_owned());
                }
            }
            let mcmeta_hash = objects
                .get("pack.mcmeta")
                .and_then(|v| v["hash"].as_str())
                .ok_or("no pack.mcmeta")?;
            let mcmeta = fetch_json(&object_url(mcmeta_hash))?;
            let mut languages: Vec<Language> = mcmeta["language"]
                .as_object()
                .map(|langs| {
                    langs
                        .iter()
                        .map(|(code, info)| Language {
                            code: code.clone(),
                            name: info["name"].as_str().unwrap_or(code).to_owned(),
                            region: info["region"].as_str().unwrap_or("").to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            languages.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            Ok(Index { languages, hashes })
        })();
        match result {
            Ok(index) => {
                *LIST.lock().unwrap() = ListState::Ready(index.languages.clone());
                *INDEX.lock().unwrap() = Some(index);
            }
            Err(e) => *LIST.lock().unwrap() = ListState::Failed(e),
        }
    });
    #[cfg(target_arch = "wasm32")]
    {
        *LIST.lock().unwrap() = ListState::Failed("not available in the browser".into());
    }
}

/// Fetches a language if needed, then switches to it.
pub fn choose(code: &str) {
    if installed(code) {
        apply(code);
        return;
    }
    let hash = INDEX
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|i| i.hashes.get(code).cloned());
    let Some(hash) = hash else { return };
    *DOWNLOADING.lock().unwrap() = Some(code.to_owned());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let code = code.to_owned();
        std::thread::spawn(move || {
            let ok = crate::platform::http::get_bytes_blocking(&object_url(&hash)).and_then(|bytes| {
                std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
                std::fs::write(dir().join(format!("{code}.json")), bytes).map_err(|e| e.to_string())
            });
            *DOWNLOADING.lock().unwrap() = None;
            if ok.is_ok() {
                apply(&code);
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    let _ = hash;
}

/// Lays `code`'s strings over English (or clears them, for English).
pub fn apply(code: &str) {
    let strings: Option<HashMap<String, String>> = (code != "en_us")
        .then(|| std::fs::read_to_string(dir().join(format!("{code}.json"))).ok())
        .flatten()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| {
            v.as_object().map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                    .collect()
            })
        });
    crate::gui::tooltip::set_language(strings.clone());
    azalea_language::set_override(strings);
}
