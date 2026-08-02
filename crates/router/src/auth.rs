//! Bodyless credential preflight client for buffered universal request surfaces.
//!
//! Provider runtimes own customer-key authority. The router forwards only credential headers to
//! their loopback-only producer contract and accepts an exact closed schema before it reads the
//! public request body. Decisions are per request and never cached.

use std::time::Duration;

use axum::http::HeaderMap;
use serde::Deserialize;

use crate::catalog::PlaneOrigins;

const AUTH_PATH: &str = "/internal/router/auth/preflight";
const AUTH_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_BODY_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthError {
    Unauthorized,
    Unavailable,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthResponse {
    schema_version: u64,
    authenticated: bool,
}

/// Authenticate once before materializing a universal request body.
pub async fn preflight(
    client: &reqwest::Client,
    origins: &PlaneOrigins<'_>,
    auth: &HeaderMap,
) -> Result<(), AuthError> {
    for origin in [origins.anthropic, origins.openai, origins.gemini] {
        let response = match client
            .post(format!("{origin}{AUTH_PATH}"))
            .headers(auth.clone())
            .timeout(AUTH_TIMEOUT)
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => continue,
        };
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AuthError::Unauthorized);
        }
        if !response.status().is_success() {
            continue;
        }
        let Some(bytes) = read_bounded(response).await else {
            continue;
        };
        let Ok(response) = serde_json::from_slice::<AuthResponse>(&bytes) else {
            continue;
        };
        if response.schema_version == 1 && response.authenticated {
            return Ok(());
        }
    }
    Err(AuthError::Unavailable)
}

async fn read_bounded(mut response: reqwest::Response) -> Option<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY_BYTES as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Some(body),
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::response::Response;
    use axum::routing::any;
    use axum::Router;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn response_schema_is_closed_and_requires_exact_success() {
        assert!(serde_json::from_str::<AuthResponse>(
            r#"{"schema_version":1,"authenticated":true}"#
        )
        .is_ok());
        assert!(serde_json::from_str::<AuthResponse>(
            r#"{"schema_version":1,"authenticated":true,"account":"leak"}"#
        )
        .is_err());
    }

    async fn origin(status: StatusCode, body: &'static str, calls: Arc<AtomicUsize>) -> String {
        let app = Router::new().fallback(any(move || {
            let calls = calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap()
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn mixed_version_failover_accepts_only_exact_success_and_401_is_terminal() {
        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let first = origin(
            StatusCode::OK,
            r#"{"schema_version":1,"authenticated":true,"extra":1}"#,
            first_calls.clone(),
        )
        .await;
        let second = origin(
            StatusCode::OK,
            r#"{"schema_version":1,"authenticated":true}"#,
            second_calls.clone(),
        )
        .await;
        let origins = PlaneOrigins {
            anthropic: &first,
            openai: &second,
            gemini: "http://127.0.0.1:0",
        };
        assert_eq!(
            preflight(&reqwest::Client::new(), &origins, &HeaderMap::new()).await,
            Ok(())
        );
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);

        let terminal_calls = Arc::new(AtomicUsize::new(0));
        let unreachable_calls = Arc::new(AtomicUsize::new(0));
        let terminal = origin(StatusCode::UNAUTHORIZED, "{}", terminal_calls.clone()).await;
        let unreachable = origin(
            StatusCode::OK,
            r#"{"schema_version":1,"authenticated":true}"#,
            unreachable_calls.clone(),
        )
        .await;
        let origins = PlaneOrigins {
            anthropic: &terminal,
            openai: &unreachable,
            gemini: "http://127.0.0.1:0",
        };
        assert_eq!(
            preflight(&reqwest::Client::new(), &origins, &HeaderMap::new()).await,
            Err(AuthError::Unauthorized)
        );
        assert_eq!(terminal_calls.load(Ordering::SeqCst), 1);
        assert_eq!(unreachable_calls.load(Ordering::SeqCst), 0);
    }
}
