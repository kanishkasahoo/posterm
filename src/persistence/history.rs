use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::atomic_write::atomic_write;
use super::collections::SavedRequest;
use super::config::config_dir;

/// Returns the path to `history.toml`.
pub fn history_path() -> PathBuf {
    config_dir().join("history.toml")
}

/// A single entry in the request history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: String,
    /// Unix timestamp (seconds since UNIX_EPOCH).
    pub timestamp_secs: u64,
    pub method: String,
    pub url: String,
    pub status_code: Option<u16>,
    pub elapsed_ms: Option<u64>,
    /// Full request snapshot captured at send time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<SavedRequest>,
}

/// TOML wrapper that serialises the history as `[[entries]]`.
#[derive(Debug, Serialize, Deserialize, Default)]
struct HistoryFile {
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

/// Loads history entries from `history.toml`, most-recent-first.
///
/// Returns an empty list on any I/O or parse error.
pub fn load_history() -> Vec<HistoryEntry> {
    let path = history_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let file: HistoryFile = toml::from_str(&content).unwrap_or_default();
    file.entries
}

/// Sensitive HTTP header names that should be redacted when
/// `persist_sensitive_headers` is `false`.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

/// Returns `true` if `name` is a sensitive header that should be redacted.
pub fn is_sensitive_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SENSITIVE_HEADERS.contains(&lower.as_str())
}

/// Saves `entries` to `history.toml`.
///
/// - Keeps only the first `limit` entries (FIFO: oldest entries are at the
///   tail of the slice and are truncated).
/// - When `persist_sensitive` is `false`, redacts sensitive headers in the
///   stored request snapshots.
pub fn save_history(
    entries: &[HistoryEntry],
    limit: usize,
    persist_sensitive: bool,
) -> std::io::Result<()> {
    let truncated: Vec<HistoryEntry> = entries
        .iter()
        .take(limit)
        .map(|e| {
            if persist_sensitive {
                e.clone()
            } else {
                redact_entry(e)
            }
        })
        .collect();

    let file = HistoryFile { entries: truncated };
    let serialized = toml::to_string_pretty(&file).unwrap_or_default();
    atomic_write(&history_path(), serialized.as_bytes())
}

/// Returns a clone of `entry` with sensitive header values replaced by
/// `"[REDACTED]"` and auth credentials cleared.
fn redact_entry(entry: &HistoryEntry) -> HistoryEntry {
    let mut cloned = entry.clone();
    if let Some(req) = cloned.request.as_mut() {
        for header in &mut req.headers {
            if is_sensitive_header(&header.key) {
                header.value = String::from("[REDACTED]");
            }
        }
        req.auth_token.clear();
        req.auth_username.clear();
        req.auth_password.clear();
    }
    cloned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::collections::{SavedRequest, SerializedKeyValueRow};

    fn make_entry_with_headers(headers: Vec<(&str, &str)>) -> HistoryEntry {
        let saved = SavedRequest {
            headers: headers
                .iter()
                .map(|(k, v)| SerializedKeyValueRow {
                    key: k.to_string(),
                    value: v.to_string(),
                    enabled: true,
                })
                .collect(),
            auth_token: String::from("secret-token"),
            auth_username: String::from("user"),
            auth_password: String::from("pass"),
            ..SavedRequest::default()
        };
        HistoryEntry {
            id: String::from("test-id"),
            timestamp_secs: 1_700_000_000,
            method: String::from("GET"),
            url: String::from("https://example.com"),
            status_code: Some(200),
            elapsed_ms: Some(42),
            request: Some(saved),
        }
    }

    #[test]
    fn is_sensitive_header_recognizes_authorization() {
        assert!(is_sensitive_header("Authorization"));
        assert!(is_sensitive_header("authorization"));
        assert!(is_sensitive_header("AUTHORIZATION"));
    }

    #[test]
    fn is_sensitive_header_recognizes_cookie_variants() {
        assert!(is_sensitive_header("cookie"));
        assert!(is_sensitive_header("Cookie"));
        assert!(is_sensitive_header("set-cookie"));
        assert!(is_sensitive_header("Set-Cookie"));
        assert!(is_sensitive_header("proxy-authorization"));
    }

    #[test]
    fn is_sensitive_header_does_not_flag_safe_headers() {
        assert!(!is_sensitive_header("content-type"));
        assert!(!is_sensitive_header("accept"));
        assert!(!is_sensitive_header("x-request-id"));
    }

    #[test]
    fn redact_entry_replaces_sensitive_header_values() {
        let entry = make_entry_with_headers(vec![
            ("Authorization", "Bearer super-secret"),
            ("Content-Type", "application/json"),
        ]);
        let redacted = redact_entry(&entry);
        let req = redacted.request.as_ref().unwrap();
        let auth_header = req
            .headers
            .iter()
            .find(|h| h.key == "Authorization")
            .unwrap();
        let ct_header = req
            .headers
            .iter()
            .find(|h| h.key == "Content-Type")
            .unwrap();
        assert_eq!(auth_header.value, "[REDACTED]");
        assert_eq!(ct_header.value, "application/json");
    }

    #[test]
    fn redact_entry_clears_auth_credentials() {
        let entry = make_entry_with_headers(vec![]);
        let redacted = redact_entry(&entry);
        let req = redacted.request.as_ref().unwrap();
        assert!(req.auth_token.is_empty());
        assert!(req.auth_username.is_empty());
        assert!(req.auth_password.is_empty());
    }

    #[test]
    fn redact_entry_leaves_non_sensitive_headers_intact() {
        let entry = make_entry_with_headers(vec![
            ("X-Trace-Id", "abc123"),
            ("Accept", "application/json"),
        ]);
        let redacted = redact_entry(&entry);
        let req = redacted.request.as_ref().unwrap();
        for h in &req.headers {
            assert_ne!(h.value, "[REDACTED]");
        }
    }

    #[test]
    fn history_file_round_trips_through_toml() {
        let entry = make_entry_with_headers(vec![("Accept", "application/json")]);
        let file = HistoryFile {
            entries: vec![entry.clone()],
        };
        let toml_str = toml::to_string_pretty(&file).expect("serialization should succeed");
        let parsed: HistoryFile =
            toml::from_str(&toml_str).expect("deserialization should succeed");
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].id, entry.id);
        assert_eq!(parsed.entries[0].url, entry.url);
        assert_eq!(parsed.entries[0].status_code, entry.status_code);
    }
}
