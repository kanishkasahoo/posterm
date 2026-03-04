pub mod client;
pub mod request_builder;
pub mod response_processor;

use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};

use crate::action::Action;
use crate::state::RequestState;

use self::client::HttpClientPool;
use self::client::InsecureTlsGuardError;
use self::request_builder::{RequestBuildError, build_request};
use self::response_processor::extract_response_metadata;
use crate::util::terminal_sanitize::sanitize_terminal_text;

#[derive(Debug)]
pub enum ExecuteRequestError {
    Build(RequestBuildError),
    InsecureTlsGuard(InsecureTlsGuardError),
    Request(reqwest::Error),
    ActionChannelClosed,
}

impl fmt::Display for ExecuteRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build(error) => write!(f, "request build failed: {error}"),
            Self::InsecureTlsGuard(error) => write!(f, "request execution blocked: {error}"),
            Self::Request(error) => write!(f, "request execution failed: {error}"),
            Self::ActionChannelClosed => write!(f, "action channel closed"),
        }
    }
}

impl Error for ExecuteRequestError {}

pub async fn execute_request(
    pool: &HttpClientPool,
    request_state: &RequestState,
    request_id: u64,
    permissive_tls: bool,
    cancel_receiver: &mut watch::Receiver<bool>,
    action_sender: &mpsc::Sender<Action>,
) -> Result<(), ExecuteRequestError> {
    let start_action = Action::RequestStarted {
        request_id,
        method: request_state.method,
        url: request_state.url.clone(),
    };
    send_action(action_sender, start_action).await?;

    let client = match pool.client(permissive_tls) {
        Ok(client) => client,
        Err(error) => {
            let failed = Action::RequestFailed {
                request_id,
                error: user_safe_insecure_tls_error(error),
            };
            send_action(action_sender, failed).await?;
            return Err(ExecuteRequestError::InsecureTlsGuard(error));
        }
    };
    let request = match build_request(client, request_state) {
        Ok(request) => request,
        Err(error) => {
            let failed = Action::RequestFailed {
                request_id,
                error: user_safe_build_error(&error),
            };
            send_action(action_sender, failed).await?;
            return Err(ExecuteRequestError::Build(error));
        }
    };

    let started_at = Instant::now();
    let execute_future = client.execute(request);
    tokio::pin!(execute_future);
    let response_result = tokio::select! {
        changed = cancel_receiver.changed() => {
            if changed.is_ok() && *cancel_receiver.borrow() {
                let cancelled = Action::RequestCancelled { request_id };
                send_action(action_sender, cancelled).await?;
                return Ok(());
            }

            execute_future.await
        }
        response = &mut execute_future => response,
    };

    let response = match response_result {
        Ok(response) => response,
        Err(error) => {
            let failed = Action::RequestFailed {
                request_id,
                error: user_safe_transport_error(&error),
            };
            send_action(action_sender, failed).await?;
            return Err(ExecuteRequestError::Request(error));
        }
    };

    let mut metadata = extract_response_metadata(&response, 0, Duration::ZERO);
    let mut stream = response.bytes_stream();
    let mut total_bytes = 0usize;
    while let Some(next) = tokio::select! {
        changed = cancel_receiver.changed() => {
            if changed.is_ok() && *cancel_receiver.borrow() {
                let cancelled = Action::RequestCancelled { request_id };
                send_action(action_sender, cancelled).await?;
                return Ok(());
            }
            stream.next().await
        }
        chunk = stream.next() => chunk,
    } {
        match next {
            Ok(chunk) => {
                total_bytes = total_bytes.saturating_add(chunk.len());
                let chunk_action = Action::ResponseChunk {
                    request_id,
                    chunk: chunk.to_vec(),
                };
                send_action(action_sender, chunk_action).await?;
            }
            Err(error) => {
                let failed = Action::RequestFailed {
                    request_id,
                    error: user_safe_transport_error(&error),
                };
                send_action(action_sender, failed).await?;
                return Err(ExecuteRequestError::Request(error));
            }
        }
    }

    metadata.total_bytes = total_bytes;
    metadata.duration_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let completed = Action::RequestCompleted {
        request_id,
        metadata,
    };
    send_action(action_sender, completed).await?;

    Ok(())
}

fn user_safe_build_error(error: &RequestBuildError) -> String {
    let message = match error {
        RequestBuildError::EmptyUrl => "Request URL is empty.".to_string(),
        RequestBuildError::InvalidUrl(_) => "Request URL is invalid.".to_string(),
        RequestBuildError::InvalidHeaderName(_) => "A request header name is invalid.".to_string(),
        RequestBuildError::InvalidHeaderValue(name) => {
            format!("A request header value is invalid for: {name}.")
        }
        RequestBuildError::Build(_) => "Failed to build the request.".to_string(),
    };

    sanitize_terminal_text(&message)
}

fn user_safe_transport_error(error: &reqwest::Error) -> String {
    let message = if error.is_timeout() {
        "Request timed out before the server responded."
    } else if error.is_connect() {
        "Could not connect to the server."
    } else if error.is_decode() {
        "Failed to decode the server response."
    } else if error.is_body() {
        "Failed while reading the response body."
    } else if error.is_redirect() {
        "Too many redirects while sending the request."
    } else if error.is_builder() {
        "Request configuration is invalid."
    } else if error.is_request() {
        "Failed to send the request."
    } else {
        "Network request failed."
    };

    sanitize_terminal_text(message)
}

fn user_safe_insecure_tls_error(_error: InsecureTlsGuardError) -> String {
    sanitize_terminal_text(
        "Insecure TLS is disabled. Set POSTERM_ALLOW_INSECURE_TLS=1 to enable it intentionally.",
    )
}

async fn send_action(
    action_sender: &mpsc::Sender<Action>,
    action: Action,
) -> Result<(), ExecuteRequestError> {
    action_sender
        .send(action)
        .await
        .map_err(|_| ExecuteRequestError::ActionChannelClosed)
}
