use std::time::Duration;

use reqwest::Version;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderMap};

use crate::state::ResponseMetadata;
use crate::util::terminal_sanitize::sanitize_terminal_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentTypeInfo {
    pub raw: Option<String>,
    pub mime_type: Option<String>,
    pub charset: Option<String>,
    pub is_textual: bool,
}

pub fn detect_content_type(headers: &HeaderMap) -> ContentTypeInfo {
    let raw = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| sanitize_terminal_text(value.trim()))
        .filter(|value| !value.is_empty());

    let mut mime_type = None;
    let mut charset = None;
    let mut is_textual = false;

    if let Some(content_type) = &raw {
        let mut parts = content_type.split(';').map(str::trim);
        if let Some(primary) = parts.next() {
            let normalized = primary.to_ascii_lowercase();
            if !normalized.is_empty() {
                is_textual = normalized.starts_with("text/")
                    || normalized.contains("json")
                    || normalized.contains("xml")
                    || normalized.contains("javascript")
                    || normalized.contains("html")
                    || normalized.contains("x-www-form-urlencoded");
                mime_type = Some(normalized);
            }
        }

        for part in parts {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };

            if key.trim().eq_ignore_ascii_case("charset") {
                let normalized = value.trim().trim_matches('"');
                if !normalized.is_empty() {
                    charset = Some(normalized.to_string());
                }
            }
        }
    }

    ContentTypeInfo {
        raw,
        mime_type,
        charset,
        is_textual,
    }
}

pub fn extract_response_metadata(
    response: &reqwest::Response,
    total_bytes: usize,
    duration: Duration,
) -> ResponseMetadata {
    let headers = response.headers();
    let content_type = detect_content_type(headers);

    let header_pairs = headers
        .iter()
        .map(|(name, value)| {
            (
                sanitize_terminal_text(name.as_ref()),
                value
                    .to_str()
                    .map(sanitize_terminal_text)
                    .unwrap_or_else(|_| String::from("<non-utf8>")),
            )
        })
        .collect();

    let content_length = headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| response.content_length());

    let duration_ms = duration.as_millis().min(u64::MAX as u128) as u64;

    ResponseMetadata {
        status_code: Some(response.status().as_u16()),
        reason_phrase: response
            .status()
            .canonical_reason()
            .map(sanitize_terminal_text),
        http_version: sanitize_terminal_text(&version_to_string(response.version())),
        content_type: content_type.raw,
        charset: content_type
            .charset
            .map(|charset| sanitize_terminal_text(&charset)),
        is_textual: content_type.is_textual,
        content_length,
        headers: header_pairs,
        total_bytes,
        duration_ms,
        truncated: false,
    }
}

fn version_to_string(version: Version) -> String {
    match version {
        Version::HTTP_09 => String::from("HTTP/0.9"),
        Version::HTTP_10 => String::from("HTTP/1.0"),
        Version::HTTP_11 => String::from("HTTP/1.1"),
        Version::HTTP_2 => String::from("HTTP/2"),
        Version::HTTP_3 => String::from("HTTP/3"),
        _ => String::from("HTTP/?"),
    }
}
