//! Bodyless credential and startup probes for universal request surfaces.
//!
//! Provider runtimes own customer-key authority. Customer preflight probes fixed loopback planes
//! in hedged order so the usual fast first authority does not amplify work, while a dead origin
//! cannot impose its full timeout. A conclusive exact success or terminal 401 wins; malformed,
//! mixed-version and transport outcomes remain inconclusive.

use std::future::Future;
use std::time::Duration;

use axum::http::HeaderMap;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;

use crate::bounded;
use crate::catalog::PlaneOrigins;

const AUTH_PATH: &str = "/internal/router/auth/preflight";
const AUTH_TIMEOUT: Duration = Duration::from_secs(2);
const AUTH_HEDGE_DELAY: Duration = Duration::from_millis(50);
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

/// Authenticate once before materializing a universal request body. Anthropic starts first;
/// OpenAI and Gemini are hedged in fixed order only while no conclusive result has arrived.
pub async fn preflight(
    client: &reqwest::Client,
    origins: &PlaneOrigins<'_>,
    auth: &HeaderMap,
) -> Result<(), AuthError> {
    let origins = [origins.anthropic, origins.openai, origins.gemini];
    preflight_with_hedge(
        |index| probe_auth(client, origins[index], auth),
        AUTH_HEDGE_DELAY,
    )
    .await
}

async fn preflight_with_hedge<F, Fut>(mut probe: F, hedge_delay: Duration) -> Result<(), AuthError>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = ProbeOutcome>,
{
    let mut probes = FuturesUnordered::new();
    let mut next_origin = 1;
    probes.push(probe(0));

    let hedge = tokio::time::sleep(hedge_delay);
    tokio::pin!(hedge);

    loop {
        let outcome = if next_origin < 3 {
            tokio::select! {
                biased;
                outcome = probes.next() => outcome.expect("at least one auth probe is active"),
                _ = &mut hedge => {
                    probes.push(probe(next_origin));
                    next_origin += 1;
                    if next_origin < 3 {
                        hedge.as_mut().reset(tokio::time::Instant::now() + hedge_delay);
                    }
                    continue;
                }
            }
        } else {
            match probes.next().await {
                Some(outcome) => outcome,
                None => return Err(AuthError::Unavailable),
            }
        };

        match outcome {
            ProbeOutcome::Success => return Ok(()),
            ProbeOutcome::Unauthorized => return Err(AuthError::Unauthorized),
            ProbeOutcome::Inconclusive if probes.is_empty() && next_origin < 3 => {
                probes.push(probe(next_origin));
                next_origin += 1;
                if next_origin < 3 {
                    hedge
                        .as_mut()
                        .reset(tokio::time::Instant::now() + hedge_delay);
                }
            }
            ProbeOutcome::Inconclusive => {}
        }
    }
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

    struct ControlledPreflight {
        starts: tokio::sync::mpsc::UnboundedReceiver<usize>,
        outcomes: [Option<tokio::sync::oneshot::Sender<ProbeOutcome>>; 3],
        task: tokio::task::JoinHandle<Result<(), AuthError>>,
    }

    impl ControlledPreflight {
        fn spawn() -> Self {
            let (start_tx, starts) = tokio::sync::mpsc::unbounded_channel();
            let (outcome_0, receive_0) = tokio::sync::oneshot::channel();
            let (outcome_1, receive_1) = tokio::sync::oneshot::channel();
            let (outcome_2, receive_2) = tokio::sync::oneshot::channel();
            let mut receivers = [Some(receive_0), Some(receive_1), Some(receive_2)];
            let task = tokio::spawn(preflight_with_hedge(
                move |index| {
                    let start_tx = start_tx.clone();
                    let receiver = receivers[index]
                        .take()
                        .expect("each controlled probe starts once");
                    async move {
                        start_tx.send(index).expect("test observes probe starts");
                        receiver.await.expect("test supplies every active outcome")
                    }
                },
                AUTH_HEDGE_DELAY,
            ));
            Self {
                starts,
                outcomes: [Some(outcome_0), Some(outcome_1), Some(outcome_2)],
                task,
            }
        }

        async fn next_start(&mut self) -> usize {
            self.starts.recv().await.expect("scheduler starts a probe")
        }

        fn assert_no_start(&mut self) {
            assert!(matches!(
                self.starts.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
        }

        fn finish(&mut self, index: usize, outcome: ProbeOutcome) {
            self.outcomes[index]
                .take()
                .expect("controlled probe finishes once")
                .send(outcome)
                .expect("scheduler still awaits the probe");
        }

        async fn result(self) -> Result<(), AuthError> {
            self.task
                .await
                .expect("controlled scheduler task completes")
        }
    }

    #[tokio::test(start_paused = true)]
    async fn fast_first_authority_does_not_contact_secondary_origins() {
        let mut preflight = ControlledPreflight::spawn();
        assert_eq!(preflight.next_start().await, 0);
        preflight.assert_no_start();

        preflight.finish(0, ProbeOutcome::Success);
        assert_eq!(preflight.result().await, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn hung_first_authority_reaches_healthy_second_after_one_hedge() {
        let mut preflight = ControlledPreflight::spawn();
        assert_eq!(preflight.next_start().await, 0);

        tokio::time::advance(AUTH_HEDGE_DELAY - Duration::from_millis(1)).await;
        preflight.assert_no_start();
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(preflight.next_start().await, 1);

        preflight.finish(1, ProbeOutcome::Success);
        assert_eq!(preflight.result().await, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn hedges_follow_fixed_order_at_fifty_millisecond_intervals() {
        let mut preflight = ControlledPreflight::spawn();
        assert_eq!(preflight.next_start().await, 0);

        tokio::time::advance(AUTH_HEDGE_DELAY).await;
        assert_eq!(preflight.next_start().await, 1);
        tokio::time::advance(AUTH_HEDGE_DELAY - Duration::from_millis(1)).await;
        preflight.assert_no_start();
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(preflight.next_start().await, 2);

        preflight.finish(2, ProbeOutcome::Success);
        assert_eq!(preflight.result().await, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn immediate_inconclusive_advances_without_waiting_for_hedge() {
        let started = tokio::time::Instant::now();
        let mut preflight = ControlledPreflight::spawn();
        assert_eq!(preflight.next_start().await, 0);

        preflight.finish(0, ProbeOutcome::Inconclusive);
        assert_eq!(preflight.next_start().await, 1);
        assert_eq!(tokio::time::Instant::now(), started);

        preflight.finish(1, ProbeOutcome::Success);
        assert_eq!(preflight.result().await, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn inconclusive_probe_does_not_bypass_active_probes_hedge() {
        let mut preflight = ControlledPreflight::spawn();
        assert_eq!(preflight.next_start().await, 0);
        tokio::time::advance(AUTH_HEDGE_DELAY).await;
        assert_eq!(preflight.next_start().await, 1);

        preflight.finish(0, ProbeOutcome::Inconclusive);
        tokio::task::yield_now().await;
        preflight.assert_no_start();
        tokio::time::advance(AUTH_HEDGE_DELAY - Duration::from_millis(1)).await;
        preflight.assert_no_start();
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(preflight.next_start().await, 2);

        preflight.finish(1, ProbeOutcome::Success);
        assert_eq!(preflight.result().await, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_401_from_hedged_probe_stays_terminal() {
        let mut preflight = ControlledPreflight::spawn();
        assert_eq!(preflight.next_start().await, 0);
        tokio::time::advance(AUTH_HEDGE_DELAY).await;
        assert_eq!(preflight.next_start().await, 1);

        preflight.finish(1, ProbeOutcome::Unauthorized);
        assert_eq!(preflight.result().await, Err(AuthError::Unauthorized));
    }

    #[tokio::test(start_paused = true)]
    async fn all_inconclusive_authorities_stay_unavailable() {
        let mut preflight = ControlledPreflight::spawn();
        for index in 0..3 {
            assert_eq!(preflight.next_start().await, index);
            preflight.finish(index, ProbeOutcome::Inconclusive);
        }
        assert_eq!(preflight.result().await, Err(AuthError::Unavailable));
    }

    #[tokio::test]
    async fn production_preflight_reaches_healthy_second_origin_before_first_timeout() {
        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let first = origin(
            StatusCode::INTERNAL_SERVER_ERROR,
            "{}",
            Duration::from_secs(5),
            first_calls.clone(),
        )
        .await;
        let second = origin(
            StatusCode::OK,
            r#"{"schema_version":1,"authenticated":true}"#,
            Duration::ZERO,
            second_calls.clone(),
        )
        .await;
        let origins = PlaneOrigins {
            anthropic: &first,
            openai: &second,
            gemini: "http://127.0.0.1:0",
        };

        let result = tokio::time::timeout(
            Duration::from_millis(1500),
            preflight(&reqwest::Client::new(), &origins, &HeaderMap::new()),
        )
        .await;

        assert_eq!(result, Ok(Ok(())));
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);
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
