//! Bodyless credential and startup probes for universal request surfaces.
//!
//! Provider runtimes own customer-key authority. The router races all three fixed loopback planes
//! so a dead first origin cannot serialize three two-second deadlines. A conclusive exact success
//! or terminal 401 wins; malformed, mixed-version and transport outcomes remain inconclusive.

use std::time::Duration;

use axum::http::HeaderMap;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;

use crate::bounded;
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartupErrorEnvelope {
    error: StartupError,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartupError {
    code: String,
    message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeOutcome {
    Success,
    Unauthorized,
    Inconclusive,
}

/// Authenticate once before materializing a universal request body. Requests are concurrent and
/// cancelled when the first conclusive authority response arrives.
pub async fn preflight(
    client: &reqwest::Client,
    origins: &PlaneOrigins<'_>,
    auth: &HeaderMap,
) -> Result<(), AuthError> {
    let mut probes = FuturesUnordered::new();
    for origin in [origins.anthropic, origins.openai, origins.gemini] {
        probes.push(probe_auth(client, origin, auth));
    }
    while let Some(outcome) = probes.next().await {
        match outcome {
            ProbeOutcome::Success => return Ok(()),
            ProbeOutcome::Unauthorized => return Err(AuthError::Unauthorized),
            ProbeOutcome::Inconclusive => {}
        }
    }
    Err(AuthError::Unavailable)
}

async fn probe_auth(client: &reqwest::Client, origin: &str, auth: &HeaderMap) -> ProbeOutcome {
    let response = match client
        .post(format!("{origin}{AUTH_PATH}"))
        .headers(auth.clone())
        .timeout(AUTH_TIMEOUT)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return ProbeOutcome::Inconclusive,
    };
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return ProbeOutcome::Unauthorized;
    }
    if !response.status().is_success() {
        return ProbeOutcome::Inconclusive;
    }
    let Ok(bytes) = bounded::response_bytes(response, MAX_BODY_BYTES).await else {
        return ProbeOutcome::Inconclusive;
    };
    match serde_json::from_slice::<AuthResponse>(&bytes) {
        Ok(response) if response.schema_version == 1 && response.authenticated => {
            ProbeOutcome::Success
        }
        _ => ProbeOutcome::Inconclusive,
    }
}

/// Deployment data-path probe. With no credential, a current provider runtime must return the
/// exact closed unauthenticated contract. A Caddy-generated 503, old 404, permissive 200 or
/// malformed body cannot admit a router slot for promotion.
pub async fn startup_probe(client: &reqwest::Client, origins: &PlaneOrigins<'_>) -> bool {
    let mut probes = FuturesUnordered::new();
    for origin in [origins.anthropic, origins.openai, origins.gemini] {
        probes.push(probe_unauthenticated_contract(client, origin));
    }
    while let Some(matches) = probes.next().await {
        if matches {
            return true;
        }
    }
    false
}

async fn probe_unauthenticated_contract(client: &reqwest::Client, origin: &str) -> bool {
    let response = match client
        .post(format!("{origin}{AUTH_PATH}"))
        .timeout(AUTH_TIMEOUT)
        .send()
        .await
    {
        Ok(response) if response.status() == reqwest::StatusCode::UNAUTHORIZED => response,
        _ => return false,
    };
    let Ok(bytes) = bounded::response_bytes(response, MAX_BODY_BYTES).await else {
        return false;
    };
    matches!(
        serde_json::from_slice::<StartupErrorEnvelope>(&bytes),
        Ok(StartupErrorEnvelope { error: StartupError { code, message } })
            if code == "unauthorized" && message == "Invalid API credential."
    )
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
    fn response_schemas_are_closed_and_require_exact_values() {
        assert!(serde_json::from_str::<AuthResponse>(
            r#"{"schema_version":1,"authenticated":true}"#
        )
        .is_ok());
        assert!(serde_json::from_str::<AuthResponse>(
            r#"{"schema_version":1,"authenticated":true,"account":"leak"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<StartupErrorEnvelope>(
            r#"{"error":{"code":"unauthorized","message":"Invalid API credential.","extra":1}}"#
        )
        .is_err());
    }

    async fn origin(
        status: StatusCode,
        body: &'static str,
        delay: Duration,
        calls: Arc<AtomicUsize>,
    ) -> String {
        let app = Router::new().fallback(any(move || {
            let calls = calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
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
    async fn healthy_plane_wins_without_waiting_for_hung_first_origin() {
        let slow_calls = Arc::new(AtomicUsize::new(0));
        let fast_calls = Arc::new(AtomicUsize::new(0));
        let slow = origin(
            StatusCode::INTERNAL_SERVER_ERROR,
            "{}",
            Duration::from_secs(5),
            slow_calls.clone(),
        )
        .await;
        let fast = origin(
            StatusCode::OK,
            r#"{"schema_version":1,"authenticated":true}"#,
            Duration::ZERO,
            fast_calls.clone(),
        )
        .await;
        let origins = PlaneOrigins {
            anthropic: &slow,
            openai: &fast,
            gemini: "http://127.0.0.1:0",
        };
        let started = tokio::time::Instant::now();
        assert_eq!(
            preflight(&reqwest::Client::new(), &origins, &HeaderMap::new()).await,
            Ok(())
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(slow_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fast_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn terminal_401_wins_when_it_arrives_before_success() {
        let terminal = origin(
            StatusCode::UNAUTHORIZED,
            "{}",
            Duration::ZERO,
            Arc::new(AtomicUsize::new(0)),
        )
        .await;
        let delayed_success = origin(
            StatusCode::OK,
            r#"{"schema_version":1,"authenticated":true}"#,
            Duration::from_millis(100),
            Arc::new(AtomicUsize::new(0)),
        )
        .await;
        let origins = PlaneOrigins {
            anthropic: &terminal,
            openai: &delayed_success,
            gemini: "http://127.0.0.1:0",
        };
        assert_eq!(
            preflight(&reqwest::Client::new(), &origins, &HeaderMap::new()).await,
            Err(AuthError::Unauthorized)
        );
    }

    #[tokio::test]
    async fn startup_requires_exact_missing_credential_contract() {
        let valid = origin(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"code":"unauthorized","message":"Invalid API credential."}}"#,
            Duration::ZERO,
            Arc::new(AtomicUsize::new(0)),
        )
        .await;
        let origins = PlaneOrigins {
            anthropic: "http://127.0.0.1:0",
            openai: &valid,
            gemini: "http://127.0.0.1:0",
        };
        assert!(startup_probe(&reqwest::Client::new(), &origins).await);

        let malformed = origin(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"code":"unauthorized","message":"different"}}"#,
            Duration::ZERO,
            Arc::new(AtomicUsize::new(0)),
        )
        .await;
        let origins = PlaneOrigins {
            anthropic: &malformed,
            openai: "http://127.0.0.1:0",
            gemini: "http://127.0.0.1:0",
        };
        assert!(!startup_probe(&reqwest::Client::new(), &origins).await);
    }
}
