use std::error::Error;
use std::fmt;

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::state::{AuthMode, BodyFormat, HttpMethod, QueryParamToken, RequestState};
use crate::util::url_parser::rebuild_url_with_params;

#[derive(Debug)]
pub enum RequestBuildError {
    EmptyUrl,
    InvalidUrl(String),
    InvalidHeaderName(String),
    InvalidHeaderValue(String),
    Build(reqwest::Error),
}

impl fmt::Display for RequestBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyUrl => write!(f, "request URL is empty"),
            Self::InvalidUrl(url) => write!(f, "invalid request URL: {url}"),
            Self::InvalidHeaderName(name) => write!(f, "invalid header name: {name}"),
            Self::InvalidHeaderValue(name) => write!(f, "invalid header value for: {name}"),
            Self::Build(error) => write!(f, "failed to build request: {error}"),
        }
    }
}

impl Error for RequestBuildError {}

pub fn build_request(
    client: &reqwest::Client,
    request_state: &RequestState,
) -> Result<reqwest::Request, RequestBuildError> {
    let url = build_effective_url(request_state)?;
    let method = map_method(request_state.method);

    let mut headers = build_headers(request_state)?;
    apply_auth_header(&mut headers, request_state)?;

    let mut builder = client.request(method, url).headers(headers);
    builder = apply_body(builder, request_state);

    builder.build().map_err(RequestBuildError::Build)
}

fn build_effective_url(request_state: &RequestState) -> Result<String, RequestBuildError> {
    let base_url = request_state.url.trim();
    if base_url.is_empty() {
        return Err(RequestBuildError::EmptyUrl);
    }

    let query_tokens = if request_state.query_param_tokens.is_empty() {
        vec![QueryParamToken::KeyValue; request_state.query_params.len()]
    } else {
        request_state.query_param_tokens.clone()
    };

    let effective = rebuild_url_with_params(base_url, &request_state.query_params, &query_tokens);
    if reqwest::Url::parse(&effective).is_err() {
        return Err(RequestBuildError::InvalidUrl(effective));
    }
    Ok(effective)
}

fn map_method(method: HttpMethod) -> reqwest::Method {
    match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Head => reqwest::Method::HEAD,
        HttpMethod::Options => reqwest::Method::OPTIONS,
    }
}

fn build_headers(request_state: &RequestState) -> Result<HeaderMap, RequestBuildError> {
    let mut headers = HeaderMap::new();

    for header in request_state.headers.iter().filter(|row| row.enabled) {
        let key = header.key.trim();
        if key.is_empty() {
            continue;
        }

        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| RequestBuildError::InvalidHeaderName(key.to_string()))?;
        let value = HeaderValue::from_str(header.value.trim())
            .map_err(|_| RequestBuildError::InvalidHeaderValue(key.to_string()))?;
        headers.insert(name, value);
    }

    Ok(headers)
}

fn apply_auth_header(
    headers: &mut HeaderMap,
    request_state: &RequestState,
) -> Result<(), RequestBuildError> {
    let value = match request_state.auth_mode {
        AuthMode::None => return Ok(()),
        AuthMode::Bearer => {
            let token = request_state.auth_token.trim();
            if token.is_empty() {
                return Ok(());
            }
            format!("Bearer {token}")
        }
        AuthMode::Basic => {
            if request_state.auth_username.trim().is_empty() {
                return Ok(());
            }

            let raw = format!(
                "{}:{}",
                request_state.auth_username, request_state.auth_password
            );
            let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
            format!("Basic {encoded}")
        }
    };

    let auth_value = HeaderValue::from_str(&value)
        .map_err(|_| RequestBuildError::InvalidHeaderValue(String::from("Authorization")))?;
    headers.insert(reqwest::header::AUTHORIZATION, auth_value);
    Ok(())
}

fn apply_body(
    builder: reqwest::RequestBuilder,
    request_state: &RequestState,
) -> reqwest::RequestBuilder {
    match request_state.body_format {
        BodyFormat::Json => {
            if request_state.body_json.trim().is_empty() {
                builder
            } else {
                builder.body(request_state.body_json.clone())
            }
        }
        BodyFormat::Form => {
            let form_pairs: Vec<(String, String)> = request_state
                .body_form
                .iter()
                .filter(|row| row.enabled && !row.key.is_empty())
                .map(|row| (row.key.clone(), row.value.clone()))
                .collect();

            if form_pairs.is_empty() {
                builder
            } else {
                builder.form(&form_pairs)
            }
        }
    }
}
