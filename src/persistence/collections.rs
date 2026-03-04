use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::atomic_write::atomic_write;
use super::config::config_dir;

/// Returns the directory that holds one TOML file per collection.
pub fn collections_dir() -> PathBuf {
    config_dir().join("collections")
}

fn is_safe_collection_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

fn resolve_collection_path(base_dir: &Path, id: &str) -> Option<PathBuf> {
    if !is_safe_collection_id(id) {
        return None;
    }

    let path = base_dir.join(format!("{id}.toml"));
    let relative = path.strip_prefix(base_dir).ok()?;

    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }

    Some(path)
}

fn load_all_collections_from_dir(dir: &Path) -> Vec<Collection> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut collections = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_safe_collection_id(stem) {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let collection = match toml::from_str::<Collection>(&content) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if !is_safe_collection_id(&collection.id) {
            continue;
        }

        if collection.id != stem {
            continue;
        }

        collections.push(collection);
    }

    collections.sort_by(|a, b| a.name.cmp(&b.name));
    collections
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
    load_all_collections_from_dir(&collections_dir())
}

/// Saves a single collection to `collections_dir/{collection.id}.toml`.
pub fn save_collection(collection: &Collection) -> std::io::Result<()> {
    let dir = collections_dir();
    let path = resolve_collection_path(&dir, &collection.id)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid collection id"))?;
    let serialized = toml::to_string_pretty(collection).unwrap_or_default();
    atomic_write(&path, serialized.as_bytes())
}

/// Removes the TOML file for the collection identified by `id`.
pub fn delete_collection_file(id: &str) -> std::io::Result<()> {
    let dir = collections_dir();
    let path = resolve_collection_path(&dir, id)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid collection id"))?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        delete_collection_file, load_all_collections_from_dir, save_collection, Collection,
        SavedRequest,
    };

    fn temp_collections_dir() -> std::path::PathBuf {
        let base =
            std::env::temp_dir().join(format!("posterm-collections-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).expect("failed to create temp dir");
        base
    }

    fn write_collection_file(path: &std::path::Path, id: &str, name: &str) {
        let collection = Collection {
            id: id.to_string(),
            name: name.to_string(),
            expanded: false,
            requests: vec![SavedRequest::default()],
        };
        let text = toml::to_string_pretty(&collection).expect("failed to serialize collection");
        std::fs::write(path, text).expect("failed to write collection file");
    }

    #[test]
    fn load_skips_invalid_collection_ids() {
        let dir = temp_collections_dir();
        let valid_id = uuid::Uuid::new_v4().to_string();
        let other_valid_id = uuid::Uuid::new_v4().to_string();

        write_collection_file(&dir.join(format!("{valid_id}.toml")), &valid_id, "valid");
        write_collection_file(
            &dir.join(format!("{other_valid_id}.toml")),
            "../../escape",
            "invalid-id",
        );
        write_collection_file(
            &dir.join("not-a-uuid.toml"),
            "not-a-uuid",
            "invalid-file-stem",
        );

        let loaded = load_all_collections_from_dir(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, valid_id);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn save_rejects_non_uuid_collection_id() {
        let collection = Collection {
            id: String::from("../outside"),
            name: String::from("bad"),
            expanded: false,
            requests: Vec::new(),
        };

        let err = save_collection(&collection).expect_err("save should reject invalid id");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn delete_rejects_non_uuid_collection_id() {
        let err =
            delete_collection_file("../outside").expect_err("delete should reject invalid id");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
