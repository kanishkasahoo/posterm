use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::atomic_write::atomic_write;

/// Returns the posterm configuration directory.
///
/// Prefers `dirs::config_dir()/posterm`; falls back to `~/.config/posterm`.
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        })
        .join("posterm")
}

/// Application-wide configuration stored in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    /// HTTP request timeout in seconds.
    pub default_timeout_secs: u64,
    /// Maximum number of history entries to retain.
    pub history_limit: usize,
    /// Whether to follow HTTP redirects automatically.
    pub follow_redirects: bool,
    /// Whether to persist sensitive headers (Authorization, Cookie, etc.) in
    /// history snapshots.
    pub persist_sensitive_headers: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 30,
            history_limit: 200,
            follow_redirects: true,
            persist_sensitive_headers: false,
        }
    }
}

/// Reads `config.toml` from the config directory.  Returns `AppConfig::default()`
/// on any error (missing file, parse failure, etc.).
pub fn load_config() -> AppConfig {
    let path = config_dir().join("config.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return AppConfig::default(),
    };
    toml::from_str::<AppConfig>(&content).unwrap_or_default()
}

/// Serialises `config` and writes it to `config.toml` atomically.
pub fn save_config(config: &AppConfig) -> std::io::Result<()> {
    let path = config_dir().join("config.toml");
    let serialized = toml::to_string_pretty(config).unwrap_or_default();
    atomic_write(&path, serialized.as_bytes())
}

/// Loads the configuration, creating the config file with defaults if it does
/// not exist yet.  Also ensures that the `collections/` subdirectory exists so
/// that `save_collection()` never fails on a missing parent directory.
pub fn ensure_config_exists() -> AppConfig {
    let dir = config_dir();
    // Create the config dir and collections subdir upfront so that collection
    // saves never fail with a missing-parent error.
    let _ = std::fs::create_dir_all(dir.join("collections"));

    let path = dir.join("config.toml");
    if !path.exists() {
        let defaults = AppConfig::default();
        let _ = save_config(&defaults);
        return defaults;
    }
    load_config()
}
