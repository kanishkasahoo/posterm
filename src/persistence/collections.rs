use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::atomic_write::atomic_write;
use super::config::config_dir;

/// Returns the directory that holds one TOML file per collection.
pub fn collections_dir() -> PathBuf {
    config_dir().join("collections")
}

/// A single key-value row that is serialised into TOML.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SerializedKeyValueRow {
    pub key: String,
    pub value: String,
    pub enabled: bool,
}

/// A saved HTTP request stored inside a [`Collection`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedRequest {
    pub id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub query_params: Vec<SerializedKeyValueRow>,
    #[serde(default)]
    pub headers: Vec<SerializedKeyValueRow>,
    pub auth_mode: String,
    pub auth_token: String,
    pub auth_username: String,
    pub auth_password: String,
    pub body_format: String,
    pub body_json: String,
    #[serde(default)]
    pub body_form: Vec<SerializedKeyValueRow>,
}

impl Default for SavedRequest {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            method: String::from("GET"),
            url: String::new(),
            query_params: Vec::new(),
            headers: Vec::new(),
            auth_mode: String::from("None"),
            auth_token: String::new(),
            auth_username: String::new(),
            auth_password: String::new(),
            body_format: String::from("Json"),
            body_json: String::new(),
            body_form: Vec::new(),
        }
    }
}

/// A named collection of [`SavedRequest`]s.
///
/// The `expanded` field is UI-only state and is intentionally excluded from
/// serialisation with `#[serde(skip)]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Collection {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub expanded: bool,
    #[serde(default)]
    pub requests: Vec<SavedRequest>,
}

/// Loads all collections from `collections_dir()`.
///
/// Each `*.toml` file in the directory represents one collection.  Files that
/// fail to parse are silently skipped.  The returned list is sorted by
/// collection name.
pub fn load_all_collections() -> Vec<Collection> {
    let dir = collections_dir();
    let read_dir = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut collections = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        match toml::from_str::<Collection>(&content) {
            Ok(c) => collections.push(c),
            Err(_) => continue,
        }
    }

    collections.sort_by(|a, b| a.name.cmp(&b.name));
    collections
}

/// Saves a single collection to `collections_dir/{collection.id}.toml`.
pub fn save_collection(collection: &Collection) -> std::io::Result<()> {
    let path = collections_dir().join(format!("{}.toml", collection.id));
    let serialized = toml::to_string_pretty(collection).unwrap_or_default();
    atomic_write(&path, serialized.as_bytes())
}

/// Removes the TOML file for the collection identified by `id`.
pub fn delete_collection_file(id: &str) -> std::io::Result<()> {
    let path = collections_dir().join(format!("{id}.toml"));
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
