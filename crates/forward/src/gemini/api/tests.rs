use super::*;
use crate::gemini::{
    GeminiBatchAuthority, GeminiBatchBlobIdentity, GeminiBatchDataKeyring, GeminiBatchPublicFacade,
};
use crate::{AffinityStore, AsyncBilling, Breaker, Clients, ProxyConfig};
use axum::http::Uri;
use axum::routing::any;
use futures_util::stream;
use gemini_credential::{encode_envelope, CredentialKeyring, GeminiCredential};
use pool::{Pool, Reserve};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CUSTOMER_KEY: &str = "sk-pool-gemini-integration";
const PROFILE_A_KEY: &str = "gemini-profile-a-key-that-is-secret";
const PROFILE_B_KEY: &str = "gemini-profile-b-key-that-is-secret";
const ACCOUNT_ID: &str = "gemini-integration-account";

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

fn crashing_node_helper() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gemini-api-crashing-helper-{}-{unique}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let helper = directory.join("node");
    let spawns = directory.join("spawns");
    let attempts = directory.join("attempts");
    let script = format!(
        "#!/bin/sh\nprintf 'spawn\\n' >> '{}'\nIFS= read -r _configure || exit 1\nprintf '%s\\n' '{{\"type\":\"ready\",\"protocol\":1,\"node\":\"v24.18.0\",\"platform\":\"linux\",\"arch\":\"x64\",\"undici\":\"node-internal\"}}'\nIFS= read -r _request || exit 1\nprintf 'attempt\\n' >> '{}'\nprintf '%s\\n' '{{\"type\":\"error\",\"id\":1,\"kind\":\"protocol\"}}'\nexit 1\n",
        spawns.display(),
        attempts.display()
    );
    fs::write(&helper, script).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    (directory, helper, spawns, attempts)
}

fn pcm16_wav_base64(sample_rate: u32, frames: u32, channels: u16) -> String {
    let data_len = frames
        .checked_mul(u32::from(channels))
        .and_then(|samples| samples.checked_mul(2))
        .unwrap();
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize(44 + data_len as usize, 0);
    base64::engine::general_purpose::STANDARD.encode(wav)
}

#[derive(Clone)]
enum MockChunk {
    Data(Bytes),
    Error,
}

#[derive(Clone)]
enum MockReply {
    Json {
        status: StatusCode,
        body: Value,
        retry_after: Option<&'static str>,
    },
    DelayedJson {
        status: StatusCode,
        body: Value,
        delay: Duration,
    },
    Stream {
        chunks: Vec<MockChunk>,
        inter_chunk_delay: Duration,
        drained: Arc<AtomicBool>,
    },
    Stalled {
        first: Bytes,
    },
}

impl MockReply {
    fn json(status: StatusCode, body: Value) -> Self {
        let body = if status.is_success()
            && body.get("response").is_none()
            && body.get("totalTokens").is_none()
        {
            json!({
                "response": body,
                "traceId": "private-trace-id",
                "consumedCredits": [{"creditType": "G1", "creditAmount": "9"}],
                "remainingCredits": [{"creditType": "G1", "creditAmount": "91"}]
            })
        } else {
            body
        };
        Self::Json {
            status,
            body,
            retry_after: None,
        }
    }

    fn stream(chunks: Vec<MockChunk>) -> (Self, Arc<AtomicBool>) {
        let drained = Arc::new(AtomicBool::new(false));
        (
            Self::Stream {
                chunks,
                inter_chunk_delay: Duration::from_millis(40),
                drained: drained.clone(),
            },
            drained,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SeenRequest {
    credential: String,
    uri: String,
    body: Bytes,
    user_agent: String,
    google_api_client: String,
    client_metadata: String,
    has_private_client_headers: bool,
}

#[derive(Default)]
struct MockState {
    replies: Mutex<HashMap<String, VecDeque<MockReply>>>,
    seen: Mutex<Vec<SeenRequest>>,
}

impl MockState {
    fn with_replies(entries: impl IntoIterator<Item = (&'static str, Vec<MockReply>)>) -> Self {
        Self {
            replies: Mutex::new(
                entries
                    .into_iter()
                    .map(|(key, replies)| (key.to_string(), replies.into()))
                    .collect(),
            ),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn seen(&self) -> Vec<SeenRequest> {
        self.seen.lock().unwrap().clone()
    }
}

struct MockServer {
    upstream: String,
    state: Arc<MockState>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock_upstream(
    State(state): State<Arc<MockState>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let credential = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string();
    state.seen.lock().unwrap().push(SeenRequest {
        credential: credential.clone(),
        uri: uri.to_string(),
        body,
        user_agent: headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        google_api_client: headers
            .get("x-goog-api-client")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        client_metadata: headers
            .get("client-metadata")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        has_private_client_headers: [
            "x-goog-user-project",
            "x-goog-request-params",
            "x-forwarded-for",
            "forwarded",
            "x-real-ip",
        ]
        .iter()
        .any(|name| headers.contains_key(*name)),
    });
    let reply = state
        .replies
        .lock()
        .unwrap()
        .get_mut(&credential)
        .and_then(VecDeque::pop_front)
        .unwrap_or_else(|| {
            MockReply::json(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": {"message": "unexpected mock request"}}),
            )
        });
    match reply {
        MockReply::Json {
            status,
            body,
            retry_after,
        } => {
            let mut response = Response::builder()
                .status(status)
                .header("content-type", "application/json");
            if let Some(retry_after) = retry_after {
                response = response.header("retry-after", retry_after);
            }
            response
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap()
        }
        MockReply::DelayedJson {
            status,
            body,
            delay,
        } => {
            tokio::time::sleep(delay).await;
            Response::builder()
                .status(status)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap()
        }
        MockReply::Stream {
            chunks,
            inter_chunk_delay,
            drained,
        } => {
            let body = stream::unfold(
                (chunks.into_iter(), true, drained),
                move |(mut chunks, first, drained)| async move {
                    let Some(chunk) = chunks.next() else {
                        drained.store(true, Ordering::Release);
                        return None;
                    };
                    if !first {
                        tokio::time::sleep(inter_chunk_delay).await;
                    }
                    let chunk = match chunk {
                        MockChunk::Data(bytes) => Ok(bytes),
                        MockChunk::Error => Err(std::io::Error::other("mock stream failure")),
                    };
                    Some((chunk, (chunks, false, drained)))
                },
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(body))
                .unwrap()
        }
        MockReply::Stalled { first } => {
            let body = stream::once(async move { Ok::<_, std::io::Error>(first) })
                .chain(stream::pending());
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(body))
                .unwrap()
        }
    }
}

async fn start_mock(state: MockState) -> MockServer {
    let state = Arc::new(state);
    let router = axum::Router::new()
        .fallback(any(mock_upstream))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    MockServer {
        upstream: format!("http://{address}"),
        state,
        task,
    }
}

struct GatewayFixture {
    gateway: Arc<GeminiGateway>,
    directory: PathBuf,
}

impl Drop for GatewayFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn gateway_fixture(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
) -> GatewayFixture {
    gateway_fixture_with_oauth_kind(
        upstream,
        proxies,
        max_transport_retries,
        None,
        OAuthKind::LegacyGeminiCli,
    )
}

fn gateway_fixture_with_token_uri(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
    token_uri: Option<&str>,
) -> GatewayFixture {
    gateway_fixture_with_oauth_kind(
        upstream,
        proxies,
        max_transport_retries,
        token_uri,
        OAuthKind::LegacyGeminiCli,
    )
}

fn gateway_fixture_with_oauth_kind(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
    token_uri: Option<&str>,
    oauth_kind: OAuthKind,
) -> GatewayFixture {
    gateway_fixture_with_oauth_kind_and_output_limit(
        upstream,
        proxies,
        max_transport_retries,
        token_uri,
        oauth_kind,
        64,
    )
}

fn gateway_fixture_with_oauth_kind_and_output_limit(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
    token_uri: Option<&str>,
    oauth_kind: OAuthKind,
    output_token_limit: u64,
) -> GatewayFixture {
    gateway_fixture_with_models(
        upstream,
        proxies,
        max_transport_retries,
        token_uri,
        oauth_kind,
        output_token_limit,
        &["gemini-integration-model"],
    )
}

fn gateway_fixture_with_models(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
    token_uri: Option<&str>,
    oauth_kind: OAuthKind,
    output_token_limit: u64,
    model_ids: &[&str],
) -> GatewayFixture {
    gateway_fixture_with_models_and_calibration(
        upstream,
        proxies,
        max_transport_retries,
        token_uri,
        oauth_kind,
        output_token_limit,
        model_ids,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn gateway_fixture_with_models_and_expiry(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
    token_uri: Option<&str>,
    oauth_kind: OAuthKind,
    output_token_limit: u64,
    model_ids: &[&str],
    credential_expires_at: i64,
) -> GatewayFixture {
    gateway_fixture_with_models_calibration_and_node(
        upstream,
        proxies,
        max_transport_retries,
        token_uri,
        oauth_kind,
        output_token_limit,
        model_ids,
        None,
        None,
        Some(credential_expires_at),
    )
}

#[allow(clippy::too_many_arguments)]
fn gateway_fixture_with_models_and_calibration(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
    token_uri: Option<&str>,
    oauth_kind: OAuthKind,
    output_token_limit: u64,
    model_ids: &[&str],
    calibration_store: Option<Arc<AsyncBilling>>,
) -> GatewayFixture {
    gateway_fixture_with_models_calibration_and_node(
        upstream,
        proxies,
        max_transport_retries,
        token_uri,
        oauth_kind,
        output_token_limit,
        model_ids,
        calibration_store,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn gateway_fixture_with_models_calibration_and_node(
    upstream: &str,
    proxies: &[Option<&str>],
    max_transport_retries: usize,
    token_uri: Option<&str>,
    oauth_kind: OAuthKind,
    output_token_limit: u64,
    model_ids: &[&str],
    calibration_store: Option<Arc<AsyncBilling>>,
    node_runtime: Option<(&str, &str, &str)>,
    credential_expires_at: Option<i64>,
) -> GatewayFixture {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "gemini-api-integration-{}-{unique}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let credential_directory = directory.join("credentials");
    fs::create_dir_all(&credential_directory).unwrap();
    fs::set_permissions(&credential_directory, fs::Permissions::from_mode(0o700)).unwrap();
    let keys = [PROFILE_A_KEY, PROFILE_B_KEY];
    let ring = CredentialKeyring::parse(&format!("test:{}", "42".repeat(32))).unwrap();
    let mut profiles = Vec::new();
    for (index, proxy) in proxies.iter().enumerate() {
        let profile_id = format!("profile_{}", (b'a' + index as u8) as char);
        let credential_file = credential_directory.join(format!("{profile_id}.json"));
        let (oauth_client_id, oauth_client_secret) = match oauth_kind {
            OAuthKind::Antigravity => (
                gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_ID,
                gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_SECRET,
            ),
            OAuthKind::LegacyGeminiCli => (
                gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID,
                gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET,
            ),
        };
        let credential = GeminiCredential {
            version: 1,
            access_token: keys[index].to_string(),
            refresh_token: format!("refresh-token-value-{index}"),
            expires_at: credential_expires_at.unwrap_or_else(|| pool::now() + 3_600),
            oauth_client_id: oauth_client_id.to_string(),
            oauth_client_secret: oauth_client_secret.to_string(),
            token_uri: token_uri
                .unwrap_or("https://oauth2.googleapis.com/token")
                .to_string(),
            subject: format!("google-subject-{index}"),
            email: format!("owner-{index}@example.invalid"),
            project_id: format!("paid-project-{:02}", index + 1),
            tier_id: "paid-tier".to_string(),
            tier_name: "Google AI Pro".to_string(),
            plan: "google_ai_pro".to_string(),
            proxy: proxy.unwrap_or_default().to_string(),
            proxy_order_id: 0,
            issued_at: pool::now(),
        };
        let envelope = ring.seal("test", &profile_id, &credential).unwrap();
        fs::write(&credential_file, encode_envelope(&envelope).unwrap()).unwrap();
        fs::set_permissions(&credential_file, fs::Permissions::from_mode(0o600)).unwrap();
        profiles.push(json!({
            "id": profile_id,
            "credential_file": credential_file,
        }));
    }
    let profiles_file = directory.join("profiles.json");
    fs::write(
        &profiles_file,
        serde_json::to_vec(&json!({"profiles": profiles})).unwrap(),
    )
    .unwrap();
    fs::set_permissions(&profiles_file, fs::Permissions::from_mode(0o600)).unwrap();
    let models = model_ids
        .iter()
        .map(|model_id| GeminiModel {
            id: (*model_id).to_string(),
            display_name: format!("Gemini Integration Model {model_id}"),
            created: 1,
            input_token_limit: 1_000_000,
            output_token_limit,
            prices: metering::GeminiPrices {
                input: 1,
                audio_input: 1,
                cached_input: 1,
                cached_audio_input: 1,
                output: 1,
                image_output: 0,
                long_context_threshold: u64::MAX,
                long_input: 1,
                long_audio_input: 1,
                long_cached_input: 1,
                long_cached_audio_input: 1,
                long_output: 1,
                search: metering::GeminiSearchBilling::PerGroundedPrompt { nano: 1 },
            },
        })
        .collect();
    // A scripted Node helper must first be scheduled, read its configure frame and publish the
    // ready handshake. One second is too tight under a parallel Rust test/build load and can kill
    // the child before its first shell instruction (and therefore before the spawn marker). Keep
    // ordinary loopback mocks fast while giving this bounded process fixture deterministic room.
    let connect_timeout_secs = if node_runtime.is_some() { 5 } else { 1 };
    let (node_binary, node_version, node_sha256) =
        node_runtime.unwrap_or(("/usr/bin/node", "v24.18.0", ""));
    let node_sha256 = if node_sha256.is_empty() {
        "0".repeat(64)
    } else {
        node_sha256.to_string()
    };
    let gateway = GeminiGateway::new_with_calibration(
        super::super::config::GeminiConfig {
            enabled: true,
            upstream: upstream.to_string(),
            profiles_file: profiles_file.to_string_lossy().into_owned(),
            credential_layout: super::super::config::GeminiCredentialLayout::SealedRoster,
            credential_keys: ring,
            models,
            connect_timeout_secs,
            read_timeout_secs: 5,
            generation_idle_timeout_secs: 5,
            max_transport_retries,
            auth_quarantine_secs: 900,
            auth_blocked_cool_secs: 15,
            min_probe_interval_secs: 15,
            transport_cool_secs: 30,
            model_failure_cool_secs: 15,
            model_failure_max_cool_secs: 900,
            default_rate_limit_cool_secs: 60,
            rate_limit_rpm_cool_secs: 2,
            rate_limit_unknown_cool_secs: 60,
            cooldown_state_file: String::new(),
            quota_reserve_fraction: 0.05,
            quota_reserve_jitter: 0.01,
            batch_5h_headroom_percent: 20,
            health_probe_interval_secs: 60,
            reserve_overhead_tokens: 10,
            antigravity_version: gemini_credential::ANTIGRAVITY_VERSION.to_string(),
            node_binary: node_binary.to_string(),
            node_version: node_version.to_string(),
            node_sha256,
        },
        calibration_store,
    )
    .unwrap();
    GatewayFixture {
        gateway: Arc::new(gateway),
        directory,
    }
}

fn proxy_config(admin_key: bool) -> Arc<ProxyConfig> {
    Arc::new(ProxyConfig {
        api_keys: if admin_key {
            vec![CUSTOMER_KEY.to_string()]
        } else {
            Vec::new()
        },
        control_keys: Vec::new(),
        panel_keys: Vec::new(),
        default_mult_bp: 10_000,
        trust_loopback: false,
        upstream: "http://127.0.0.1:1".to_string(),
        claudestore_fallback: None,
        max_tries: 2,
        util_cap: 1.0,
        cool_secs: 60,
        smooth_wait_ms: 0,
        poll: false,
        inject_identity: false,
        identity: String::new(),
        inject_billing: false,
        cc_version: String::new(),
        cc_entrypoint: String::new(),
        default_beta: String::new(),
        user_agent: "gemini-integration-test".to_string(),
        user_agents: Vec::new(),
        ua_spread: 0,
        anthropic_version: String::new(),
        connect_timeout: 1,
        read_timeout: 120,
        nonstream_read_timeout: 1800,
        x_app: String::new(),
        stainless_lang: String::new(),
        stainless_runtime: String::new(),
        stainless_runtime_version: String::new(),
        stainless_package_version: String::new(),
        stainless_os: String::new(),
        stainless_arch: String::new(),
    })
}

fn app_state(gateway: Arc<GeminiGateway>, billing: Option<Arc<AsyncBilling>>) -> AppState {
    let cfg = proxy_config(billing.is_none());
    AppState {
        provider: crate::ProviderMode::Gemini,
        authority: Arc::new(registry::authority::AuthorityConfig::new(
            ":memory:".to_string(),
            None,
        )),
        data_db_path: Arc::new(":memory:".to_string()),
        pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
        affinity: Arc::new(AffinityStore::new(None, None, 3600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: None,
        codex: None,
        gemini: Some(gateway),
        gemini_batch: None,
        gemini_batch_runtime: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing,
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(1)),
        metrics: Arc::new(Metrics::new()),
        probe_poke: None,
        admin_changes: tokio::sync::broadcast::channel(16).0,
        cfg,
    }
}

async fn invoke(app: AppState, body: Value, streaming: bool) -> Response {
    invoke_with_identity(app, body, streaming, CUSTOMER_KEY, None).await
}

async fn invoke_with_identity(
    app: AppState,
    body: Value,
    streaming: bool,
    key: &str,
    session_id: Option<&str>,
) -> Response {
    // Existing streaming tests assert the SSE downstream shape, so request alt=sse explicitly;
    // the JSON-array default (no alt) has its own dedicated coverage.
    let uri = if streaming {
        "/v1beta/models/gemini-integration-model:streamGenerateContent?alt=sse".to_string()
    } else {
        "/v1beta/models/gemini-integration-model:generateContent".to_string()
    };
    let mut builder = axum::extract::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-goog-api-key", key);
    if let Some(session_id) = session_id {
        builder = builder.header("x-session-id", session_id);
    }
    let request = builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let mut response = match api_inner(app, "198.51.100.10:12345".parse().unwrap(), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    apply_native_response_headers(&mut response);
    response
}

async fn invoke_exact_calibration(
    app: AppState,
    body: Value,
    profile_id: &str,
    request_id: &str,
) -> Response {
    let request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .header("x-apitoken-calibration-profile", profile_id)
        .header("x-apitoken-calibration-request-id", request_id)
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    match api_inner(app, "198.51.100.10:12345".parse().unwrap(), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn invoke_exact_count_tokens(
    app: AppState,
    model: &str,
    profile_id: &str,
    request_id: &str,
) -> Response {
    invoke_exact_uri(
        app,
        &format!("/v1beta/models/{model}:countTokens"),
        json!({"contents": [{"role": "user", "parts": [{"text": "count me"}]}]}),
        profile_id,
        request_id,
    )
    .await
}

async fn invoke_uri(app: AppState, uri: &str, body: Value) -> Response {
    let request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    match api_inner(app, "198.51.100.10:12345".parse().unwrap(), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn invoke_exact_uri(
    app: AppState,
    uri: &str,
    body: Value,
    profile_id: &str,
    request_id: &str,
) -> Response {
    let not_after = uri
        .contains("/gemini-3.7-flash:")
        .then(|| (pool::now() + 60) as u64);
    invoke_exact_uri_with_deadline(app, uri, body, profile_id, request_id, not_after).await
}

async fn invoke_exact_uri_with_deadline(
    app: AppState,
    uri: &str,
    body: Value,
    profile_id: &str,
    request_id: &str,
    not_after: Option<u64>,
) -> Response {
    let mut builder = axum::extract::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .header("x-apitoken-calibration-profile", profile_id)
        .header("x-apitoken-calibration-request-id", request_id);
    if let Some(not_after) = not_after {
        builder = builder.header("x-apitoken-calibration-not-after", not_after.to_string());
    }
    let request = builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    match api_inner(app, "198.51.100.10:12345".parse().unwrap(), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn response_json(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap(),
    )
    .unwrap()
}

fn calibration_dispatch_ms(response: &Response) -> u64 {
    let mut values = response
        .headers()
        .get_all(CALIBRATION_DISPATCH_HEADER)
        .iter();
    let value = values.next().expect("deadline-bound response attestation");
    assert!(values.next().is_none(), "dispatch attestation is singular");
    let value = value.to_str().expect("ASCII dispatch attestation");
    assert!(!value.is_empty() && !value.starts_with('0'));
    assert!(value.bytes().all(|byte| byte.is_ascii_digit()));
    value
        .parse::<u64>()
        .expect("canonical positive milliseconds")
}

fn catalog_model(id: &str) -> GeminiModel {
    let spec = metering::gemini_catalog_at(0)
        .into_iter()
        .find(|spec| spec.id == id)
        .expect("catalog model");
    GeminiModel {
        id: spec.id.to_string(),
        display_name: spec.display_name.to_string(),
        created: spec.created,
        input_token_limit: spec.input_token_limit,
        output_token_limit: spec.output_token_limit,
        prices: spec.prices,
    }
}

#[test]
fn canonicalize_promotes_snake_case_and_normalizes_tools() {
    let mut value = json!({
        "contents": [],
        "system_instruction": {"parts": [{"text": "be terse"}]},
        "safety_settings": [],
        "generation_config": {
            "max_output_tokens": 10,
            "top_p": 0.8,
            "thinking_config": {"thinking_level": "high", "include_thoughts": true}
        },
        "tools": [{"google_search": {}}]
    });
    canonicalize_native_request(&mut value);
    assert!(value.get("systemInstruction").is_some());
    assert!(value.get("system_instruction").is_none());
    assert!(value.get("safetySettings").is_some());
    assert!(value.get("generationConfig").is_some());
    assert_eq!(value["generationConfig"]["maxOutputTokens"], 10);
    assert_eq!(value["generationConfig"]["topP"], 0.8);
    assert_eq!(
        value["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "high"
    );
    assert_eq!(
        value["generationConfig"]["thinkingConfig"]["includeThoughts"],
        true
    );
    let tool = &value["tools"][0];
    assert!(tool.get("googleSearch").is_some());
    assert!(tool.get("google_search").is_none());
    // The normalized tool must pass validation just like its camelCase form.
    assert!(
        validate_generation_request(&value, &catalog_model("gemini-2.5-flash-lite"), false).is_ok()
    );
}

#[test]
fn omitted_generation_limit_uses_the_model_output_ceiling() {
    let model = catalog_model("gemini-3.6-flash");
    let uncapped = json!({"contents": [{"parts": [{"text": "hello"}]}]});
    let (_, output, _, _) = generation_controls(&uncapped, &model, 0, AudioUsageHint::default());
    assert_eq!(output, model.output_token_limit);

    let capped = json!({
        "contents": [{"parts": [{"text": "hello"}]}],
        "generationConfig": {"maxOutputTokens": 5_000}
    });
    let (_, output, _, _) = generation_controls(&capped, &model, 0, AudioUsageHint::default());
    assert_eq!(output, 5_000);
}

#[test]
fn camel_case_wins_over_snake_case_duplicate() {
    let mut value = json!({
        "systemInstruction": {"parts": [{"text": "camel"}]},
        "system_instruction": {"parts": [{"text": "snake"}]}
    });
    canonicalize_native_request(&mut value);
    assert_eq!(value["systemInstruction"]["parts"][0]["text"], "camel");
    assert!(value.get("system_instruction").is_none());
}

#[test]
fn public_thinking_levels_select_closed_wire_candidates() {
    let flash_37 = catalog_model("gemini-3.7-flash");
    for level in [
        None,
        Some("low"),
        Some("medium"),
        Some("high"),
        Some("HIGH"),
    ] {
        assert_eq!(flash_37.wire_model_id(level), Ok("gemini-3.7-flash-tiered"));
    }
    assert!(flash_37.wire_model_id(Some("minimal")).is_err());
    assert!(flash_37.wire_model_id(Some("future")).is_err());
    assert_eq!(flash_37.quota_model_ids(), vec!["gemini-3.7-flash-tiered"]);

    let flash_preview = catalog_model("gemini-3-flash-preview");
    for level in [
        None,
        Some("minimal"),
        Some("low"),
        Some("medium"),
        Some("high"),
    ] {
        assert_eq!(flash_preview.wire_model_id(level), Ok("gemini-3-flash"));
    }
    assert!(flash_preview.wire_model_id(Some("future")).is_err());
    assert_eq!(
        flash_preview.quota_model_ids(),
        vec!["gemini-3-flash", "gemini-3-flash-agent"]
    );

    let flash = catalog_model("gemini-3.6-flash");
    for (level, expected) in [
        (None, "gemini-3.6-flash-medium"),
        (Some("minimal"), "gemini-3.6-flash-low"),
        (Some("low"), "gemini-3.6-flash-low"),
        (Some("medium"), "gemini-3.6-flash-medium"),
        (Some("high"), "gemini-3.6-flash-high"),
        (Some("HIGH"), "gemini-3.6-flash-high"),
    ] {
        assert_eq!(flash.wire_model_id(level), Ok(expected));
    }
    assert!(flash.wire_model_id(Some("future")).is_err());

    let flash_35 = catalog_model("gemini-3.5-flash");
    for (level, expected) in [
        (None, "gemini-3.5-flash-low"),
        (Some("minimal"), "gemini-3.5-flash-extra-low"),
        (Some("low"), "gemini-3.5-flash-low"),
        (Some("medium"), "gemini-3.5-flash-low"),
        (Some("high"), "gemini-3.5-flash-low"),
    ] {
        assert_eq!(flash_35.wire_model_id(level), Ok(expected));
    }
    assert!(flash_35.wire_model_id(Some("future")).is_err());

    let pro = catalog_model("gemini-3.1-pro-preview");
    for (level, expected) in [
        (None, "gemini-pro-agent"),
        (Some("low"), "gemini-3.1-pro-low"),
        (Some("medium"), "gemini-pro-agent"),
        (Some("high"), "gemini-pro-agent"),
    ] {
        assert_eq!(pro.wire_model_id(level), Ok(expected));
    }
    assert!(pro.wire_model_id(Some("minimal")).is_err());

    let direct = catalog_model("gemini-2.5-flash");
    assert_eq!(direct.wire_model_id(Some("future")), Ok("gemini-2.5-flash"));
}

#[test]
fn nested_count_tokens_request_controls_the_wire_tier() {
    let model = catalog_model("gemini-3.6-flash");
    let body = json!({
        "generateContentRequest": {
            "model": "models/caller-controlled",
            "contents": [],
            "generationConfig": {"thinkingConfig": {"thinkingLevel": "low"}}
        }
    });
    assert_eq!(
        wire_model_for_request(Operation::CountTokens, &model, &body).unwrap(),
        "gemini-3.6-flash-low"
    );
}

#[test]
fn flash_preview_audio_hint_uses_exact_duration_and_ignores_channel_count() {
    let mono = pcm16_wav_base64(8_000, 2_000, 1);
    let stereo = pcm16_wav_base64(8_000, 2_000, 2);
    let body = json!({
        "contents": [{
            "parts": [
                {"inlineData": {"mimeType": "audio/wav", "data": mono}},
                {"inlineData": {"mimeType": "Audio/WAV", "data": stereo}},
                {"text": "two quarter-second clips"}
            ]
        }]
    });
    let hint = flash_preview_audio_usage_hint(&body).unwrap();
    assert_eq!(hint.tokens, 16);
    assert_eq!(
        hint.encoded_data_bytes,
        body.pointer("/contents/0/parts/0/inlineData/data")
            .and_then(Value::as_str)
            .unwrap()
            .len() as u64
            + body
                .pointer("/contents/0/parts/1/inlineData/data")
                .and_then(Value::as_str)
                .unwrap()
                .len() as u64
    );

    let model = catalog_model("gemini-3-flash-preview");
    let (estimated_input, _, _, _) = generation_controls(&body, &model, 10, hint);
    assert_eq!(
        estimated_input,
        body.to_string().len() as u64 - hint.encoded_data_bytes + hint.tokens + 10
    );
}

#[test]
fn flash_preview_audio_hint_rejects_unprovable_duration_format_and_location() {
    let fractional = json!({
        "contents": [{"parts": [{
            "inlineData": {
                "mimeType": "audio/wav",
                "data": pcm16_wav_base64(8_000, 2_001, 1)
            }
        }]}]
    });
    assert!(flash_preview_audio_usage_hint(&fractional).is_err());

    let compressed = json!({
        "contents": [{"parts": [{
            "inlineData": {"mimeType": "audio/mp3", "data": "AA=="}
        }]}]
    });
    assert!(flash_preview_audio_usage_hint(&compressed).is_err());

    let remote = json!({
        "contents": [{"parts": [{
            "fileData": {"mimeType": "audio/wav", "fileUri": "files/private"}
        }]}]
    });
    assert!(flash_preview_audio_usage_hint(&remote).is_err());

    let malformed = json!({
        "contents": [{"parts": [{
            "inlineData": {"mimeType": "audio/wav", "data": "bm90LXdhdg=="}
        }]}]
    });
    assert!(flash_preview_audio_usage_hint(&malformed).is_err());
}

#[test]
fn flash_preview_audio_fallback_reconstructs_only_provable_cache_splits() {
    let hint = AudioUsageHint {
        tokens: 8,
        encoded_data_bytes: 0,
    };
    let mut fresh = json!({
        "usageMetadata": {
            "promptTokenCount": 55,
            "candidatesTokenCount": 1,
            "thoughtsTokenCount": 118
        }
    });
    apply_audio_usage_fallback(&mut fresh, hint).unwrap();
    assert_eq!(
        fresh["usageMetadata"]["promptTokensDetails"],
        json!([{"modality": "AUDIO", "tokenCount": 8}])
    );
    let usage = metering::gemini::usage_from_response_value(&fresh).unwrap();
    assert_eq!(usage.input_tokens, 47);
    assert_eq!(usage.audio_input_tokens, 8);

    let mut fully_cached = json!({
        "usageMetadata": {
            "promptTokenCount": 55,
            "cachedContentTokenCount": 55,
            "candidatesTokenCount": 1
        }
    });
    apply_audio_usage_fallback(&mut fully_cached, hint).unwrap();
    let usage = metering::gemini::usage_from_response_value(&fully_cached).unwrap();
    assert_eq!(usage.cached_input_tokens, 47);
    assert_eq!(usage.cached_audio_input_tokens, 8);

    let mut explicit_partial_cache = json!({
        "usageMetadata": {
            "promptTokenCount": 55,
            "cachedContentTokenCount": 20,
            "cacheTokensDetails": [
                {"modality": "TEXT", "tokenCount": 17},
                {"modality": "AUDIO", "tokenCount": 3}
            ],
            "candidatesTokenCount": 1
        }
    });
    apply_audio_usage_fallback(&mut explicit_partial_cache, hint).unwrap();
    let usage = metering::gemini::usage_from_response_value(&explicit_partial_cache).unwrap();
    assert_eq!(usage.input_tokens, 30);
    assert_eq!(usage.audio_input_tokens, 5);
    assert_eq!(usage.cached_input_tokens, 17);
    assert_eq!(usage.cached_audio_input_tokens, 3);

    let mut ambiguous_partial_cache = json!({
        "usageMetadata": {
            "promptTokenCount": 55,
            "cachedContentTokenCount": 20,
            "candidatesTokenCount": 1
        }
    });
    assert_eq!(
        apply_audio_usage_fallback(&mut ambiguous_partial_cache, hint),
        Err(AudioUsageFallbackError::AmbiguousCache)
    );

    let mut impossible_cache_total = json!({
        "usageMetadata": {
            "promptTokenCount": 55,
            "cachedContentTokenCount": 56,
            "candidatesTokenCount": 1
        }
    });
    assert_eq!(
        apply_audio_usage_fallback(&mut impossible_cache_total, hint),
        Err(AudioUsageFallbackError::InvalidMetadata)
    );
}

#[test]
fn provider_audio_usage_wins_and_reconstructed_usage_is_public_in_json_and_sse() {
    let hint = AudioUsageHint {
        tokens: 8,
        encoded_data_bytes: 0,
    };
    let mut authoritative = json!({
        "usageMetadata": {
            "promptTokenCount": 55,
            "cachedContentTokenCount": 20,
            "promptTokensDetails": [{"modality": "audio", "tokenCount": 0}]
        }
    });
    apply_audio_usage_fallback(&mut authoritative, hint).unwrap();
    assert_eq!(
        authoritative["usageMetadata"]["promptTokensDetails"],
        json!([{"modality": "audio", "tokenCount": 0}])
    );

    let response = serde_json::to_vec(&json!({
        "response": {
            "candidates": [],
            "usageMetadata": {"promptTokenCount": 55, "candidatesTokenCount": 1},
            "modelVersion": "gemini-3-flash"
        }
    }))
    .unwrap();
    let native = unwrap_code_assist_response(
        Operation::Generate,
        &response,
        "gemini-3-flash-preview",
        hint,
        false,
    )
    .unwrap();
    let native: Value = serde_json::from_slice(&native).unwrap();
    assert_eq!(
        native["usageMetadata"]["promptTokensDetails"],
        json!([{"modality": "AUDIO", "tokenCount": 8}])
    );

    let mut stream =
        SseTranslator::new_with_image_usage(StreamFraming::Sse, "gemini-3-flash-preview", 0, hint);
    let chunks = stream
        .push(
            b"data: {\"response\":{\"candidates\":[],\"usageMetadata\":{\"promptTokenCount\":55,\"candidatesTokenCount\":1},\"modelVersion\":\"gemini-3-flash\"}}\n\n",
        )
        .unwrap();
    let streamed: Value = serde_json::from_slice(
        chunks[0]
            .strip_prefix(b"data: ")
            .unwrap()
            .strip_suffix(b"\n\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        streamed["usageMetadata"]["promptTokensDetails"],
        json!([{"modality": "AUDIO", "tokenCount": 8}])
    );
    assert_eq!(stream.usage.audio_input_tokens, 8);
}

#[test]
fn private_model_versions_are_rewritten_to_the_public_model() {
    let response = serde_json::to_vec(&json!({
        "response": {
            "candidates": [],
            "modelVersion": "gemini-3.6-flash-high"
        },
        "traceId": "private-trace"
    }))
    .unwrap();
    let native = unwrap_code_assist_response(
        Operation::Generate,
        &response,
        "gemini-3.6-flash",
        AudioUsageHint::default(),
        false,
    )
    .unwrap();
    let native: Value = serde_json::from_slice(&native).unwrap();
    assert_eq!(native["modelVersion"], "gemini-3.6-flash");
    assert!(native.get("traceId").is_none());

    let mut stream = SseTranslator::new_with_image_usage(
        StreamFraming::Sse,
        "gemini-3.6-flash",
        0,
        AudioUsageHint::default(),
    );
    let chunks = stream
        .push(
            b"data: {\"response\":{\"candidates\":[],\"modelVersion\":\"gemini-3.6-flash-high\"}}\n\n",
        )
        .unwrap();
    let value: Value = serde_json::from_slice(
        chunks[0]
            .strip_prefix(b"data: ")
            .unwrap()
            .strip_suffix(b"\n\n")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["modelVersion"], "gemini-3.6-flash");
}

#[test]
fn private_content_roles_are_inferred_without_changing_explicit_values() {
    let mut request = json!({
        "contents": [
            {"parts": [{"text": "first"}]},
            {"role": "model", "parts": [{"text": "second"}]},
            {"role": "", "parts": [{"text": "third"}]},
            {"role": "user", "parts": [{"text": "fourth"}]},
            {"role": null, "parts": [{"text": "fifth"}]},
            {"role": "invalid", "parts": [{"text": "sixth"}]}
        ]
    });
    normalize_private_content_roles(request.as_object_mut().unwrap());
    let roles = request["contents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|content| content["role"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(roles, ["user", "model", "user", "user", "model", "invalid"]);
}

#[test]
fn antigravity_wire_clamps_only_the_rejected_output_boundary() {
    let wrap = |oauth_kind, max_output_tokens| {
        let native = json!({
            "contents": [{"parts": [{"text": "hello"}]}],
            "generationConfig": {"maxOutputTokens": max_output_tokens}
        });
        let bytes = wrap_code_assist_request(
            Operation::StreamGenerate,
            oauth_kind,
            "gemini-2.5-flash",
            "paid-project",
            &native,
            "prompt-id",
            Some("session-id"),
            Some("request-id"),
        )
        .unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    };

    let boundary = wrap(OAuthKind::Antigravity, 65_536);
    assert_eq!(
        boundary["request"]["generationConfig"]["maxOutputTokens"],
        ANTIGRAVITY_WIRE_OUTPUT_TOKEN_LIMIT
    );
    assert_eq!(boundary["request"]["contents"][0]["role"], "user");

    let lower = wrap(OAuthKind::Antigravity, 8_192);
    assert_eq!(
        lower["request"]["generationConfig"]["maxOutputTokens"],
        8_192
    );

    let legacy = wrap(OAuthKind::LegacyGeminiCli, 65_536);
    assert_eq!(
        legacy["request"]["generationConfig"]["maxOutputTokens"],
        65_536
    );
    assert!(legacy["request"]["contents"][0].get("role").is_none());
}

#[test]
fn native_tool_replay_adds_private_marker_only_when_signature_is_missing() {
    let native = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "Use both tools."}]},
            {"role": "model", "parts": [
                {"functionCall": {"name": "unsigned", "args": {}}},
                {
                    "functionCall": {"name": "signed", "args": {"x": 1}},
                    "thoughtSignature": "opaque-client-signature"
                }
            ]},
            {"role": "user", "parts": [
                {"functionResponse": {"name": "unsigned", "response": {"output": "a"}}},
                {"functionResponse": {"name": "signed", "response": {"output": "b"}}}
            ]}
        ]
    });

    for operation in [Operation::Generate, Operation::CountTokens] {
        let wrapped = wrap_code_assist_request(
            operation,
            OAuthKind::Antigravity,
            "gemini-3.1-flash-lite",
            "paid-project",
            &native,
            "prompt-id",
            Some("session-id"),
            Some("request-id"),
        )
        .unwrap();
        let wrapped: Value = serde_json::from_slice(&wrapped).unwrap();
        let request = &wrapped["request"];
        assert_eq!(
            request["contents"][1]["parts"][0]["thoughtSignature"],
            REPLAYED_FUNCTION_CALL_THOUGHT_SIGNATURE
        );
        assert_eq!(
            request["contents"][1]["parts"][1]["thoughtSignature"],
            "opaque-client-signature"
        );
    }

    assert!(native["contents"][1]["parts"][0]
        .get("thoughtSignature")
        .is_none());
}

#[test]
fn nano_banana_request_is_validated_and_wrapped_as_image_generation() {
    let model = catalog_model("gemini-3.1-flash-image");
    let mut native = json!({
        "contents": [{
            "parts": [
                {"text": "Draw a tiny banana astronaut"},
                {"inline_data": {"mime_type": "image/png", "data": "aGVsbG8="}}
            ]
        }],
        "generation_config": {
            "candidate_count": 1,
            "response_modalities": ["IMAGE", "TEXT"],
            "image_config": {"aspectRatio": "16:9", "imageSize": "4K"}
        }
    });
    canonicalize_native_request(&mut native);
    validate_native_request(Operation::Generate, &native, &model, false).unwrap();
    assert_eq!(image_output_tokens(&native), 2_520);
    let (estimated_input, _, image_output, _) =
        generation_controls(&native, &model, 0, AudioUsageHint::default());
    assert_eq!(image_output, 2_520);
    assert!(estimated_input < native.to_string().len() as u64 + 2_240);

    let wrapped = wrap_code_assist_request(
        Operation::Generate,
        OAuthKind::Antigravity,
        &model.id,
        "paid-project",
        &native,
        "prompt-id",
        Some("session-id"),
        Some("image_gen/1/request-uuid/12"),
    )
    .unwrap();
    let wrapped: Value = serde_json::from_slice(&wrapped).unwrap();
    assert_eq!(wrapped["requestType"], "image_gen");
    assert_eq!(wrapped["requestId"], "image_gen/1/request-uuid/12");
    assert_eq!(wrapped["model"], "gemini-3.1-flash-image");
    assert_eq!(wrapped["request"]["contents"][0]["role"], "user");
    assert!(wrapped["request"].get("sessionId").is_none());
    assert_eq!(
        wrapped["request"]["generationConfig"]["responseModalities"],
        json!(["TEXT", "IMAGE"])
    );
    assert_eq!(
        wrapped["request"]["generationConfig"]["imageConfig"]["imageSize"],
        "4K"
    );
    assert_eq!(wrapped["request"]["generationConfig"]["candidateCount"], 1);
}

#[test]
fn nano_banana_defaults_are_explicit_and_unsupported_controls_fail_closed() {
    let model = catalog_model("gemini-3.1-flash-image");
    let native = json!({"contents": [{"parts": [{"text": "Draw a banana"}]}]});
    validate_native_request(Operation::Generate, &native, &model, false).unwrap();
    let wrapped = wrap_code_assist_request(
        Operation::Generate,
        OAuthKind::Antigravity,
        &model.id,
        "paid-project",
        &native,
        "prompt-id",
        Some("session-id"),
        Some("request-id"),
    )
    .unwrap();
    let wrapped: Value = serde_json::from_slice(&wrapped).unwrap();
    assert_eq!(
        wrapped["request"]["generationConfig"]["imageConfig"],
        json!({"aspectRatio": "1:1", "imageSize": "1K"})
    );
    assert_eq!(
        wrapped["request"]["generationConfig"]["responseModalities"],
        json!(["TEXT", "IMAGE"])
    );

    for invalid in [
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"candidateCount": 2}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"responseModalities": ["IMAGE"]}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"responseModalities": "IMAGE"}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"maxOutputTokens": 0}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"maxOutputTokens": 32_769}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"maxOutputTokens": "100"}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"imageConfig": {"imageSize": "0.5K"}}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"imageConfig": {"imageSize": "8K"}}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"thinkingConfig": {}}}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "tools": [{"googleSearch": {}}]}),
        json!({"contents": [{"parts": [{"text": "x"}]}], "tools": {}}),
        json!({"contents": [{"parts": [{"text": "x"}, {"inlineData": {"mimeType": "image/png", "data": "not base64"}}]}]}),
        json!({"contents": [{"parts": [{"text": "x"}, {"inlineData": {"data": "aGVsbG8="}}]}]}),
        json!({"contents": [{"parts": [{"text": "x"}, {"inlineData": {"mimeType": "image/png"}}]}]}),
        json!({"contents": [{"parts": [{"text": "x"}, {"inlineData": "aGVsbG8="}]}]}),
    ] {
        assert!(validate_native_request(Operation::Generate, &invalid, &model, false).is_err());
    }

    let references = (0..15)
        .map(|_| json!({"inlineData": {"mimeType": "image/png", "data": "aGVsbG8="}}))
        .collect::<Vec<_>>();
    let too_many_references = json!({
        "contents": [{"parts": [{"text": "x"}]} , {"parts": references}]
    });
    assert!(
        validate_native_request(Operation::Generate, &too_many_references, &model, false).is_err()
    );
}

#[test]
fn antigravity_image_request_id_uses_the_first_party_lineage() {
    let image = fresh_antigravity_request_id(true);
    let parts = image.split('/').collect::<Vec<_>>();
    assert_eq!(parts.len(), 4);
    assert_eq!(parts[0], "image_gen");
    assert!(parts[1].parse::<u128>().is_ok());
    assert!(!parts[2].is_empty());
    assert_eq!(parts[3], "12");

    let agent = fresh_antigravity_request_id(false);
    assert!(agent.starts_with("agent-"));
    assert!(!agent.contains('/'));
}

#[test]
fn model_value_is_native_shaped() {
    let mut model = GeminiModel {
        id: "gemini-2.5-flash".to_string(),
        display_name: "Gemini 2.5 Flash".to_string(),
        created: 1_750_118_400,
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        prices: metering::GeminiPrices {
            input: 1,
            audio_input: 1,
            cached_input: 1,
            cached_audio_input: 1,
            output: 1,
            image_output: 0,
            long_context_threshold: u64::MAX,
            long_input: 1,
            long_audio_input: 1,
            long_cached_input: 1,
            long_cached_audio_input: 1,
            long_output: 1,
            search: metering::GeminiSearchBilling::PerGroundedPrompt { nano: 1 },
        },
    };
    let value = model_value(&model, false);
    assert_eq!(value["name"], "models/gemini-2.5-flash");
    assert_eq!(value["version"], "2.5");
    assert_eq!(value["created"], 1_750_118_400);
    assert_eq!(
        value["supportedGenerationMethods"],
        json!(["generateContent", "streamGenerateContent", "countTokens"])
    );
    assert_eq!(
        model_value(&model, true)["supportedGenerationMethods"],
        json!([
            "generateContent",
            "streamGenerateContent",
            "countTokens",
            "batchGenerateContent"
        ])
    );
    assert_ne!(value["description"], value["displayName"]);
    assert!(value["temperature"].is_number());
    assert!(value["topP"].is_number());
    assert!(value["topK"].is_number());
    assert!(value["maxTemperature"].is_number());
    assert_eq!(
        value["apitoken"]["limits"],
        json!({"context": 1_048_576, "input": 1_048_576, "output": 65_536})
    );
    assert_eq!(
        value["apitoken"]["capabilities"],
        json!({
            "reasoning_efforts": ["minimal", "low", "medium", "high"],
            "service_tiers": ["standard"],
            "input_modalities": ["text", "image", "audio", "video"],
            "output_modalities": ["text"],
            "tool_calling": true,
            "structured_outputs": true,
            "streaming": true
        })
    );

    model.id = "gemini-3.1-flash-image".to_string();
    model.display_name = "Gemini 3.1 Flash Image".to_string();
    model.created = 1_779_926_400;
    let image = model_value(&model, true);
    assert_eq!(image["created"], 1_779_926_400);
    assert_eq!(
        image["supportedGenerationMethods"],
        json!(["generateContent", "streamGenerateContent", "countTokens"])
    );
    assert_eq!(
        image["apitoken"]["capabilities"],
        json!({
            "reasoning_efforts": [],
            "service_tiers": ["standard"],
            "input_modalities": ["text", "image", "pdf"],
            "output_modalities": ["text", "image"],
            "tool_calling": false,
            "structured_outputs": false,
            "streaming": true
        })
    );

    model.id = "gemini-3-flash-preview".to_string();
    model.created = 1_765_929_600;
    let audio = model_value(&model, false);
    assert_eq!(audio["created"], 1_765_929_600);
    assert_eq!(
        audio["apitoken"]["capabilities"]["input_modalities"],
        json!(["text", "image", "audio", "video", "pdf"])
    );

    model.id = "gemini-3.7-flash".to_string();
    model.created = 1_786_579_200;
    let dormant = model_value(&model, false);
    assert_eq!(dormant["created"], 1_786_579_200);
    for removed in ["temperature", "topP", "topK", "maxTemperature"] {
        assert!(dormant.get(removed).is_none());
    }
}

#[test]
fn parse_list_models_query_supports_pagination() {
    let page = parse_list_models_query(Some("pageSize=2&pageToken=3&irrelevant=x")).unwrap();
    assert_eq!(page.size, 2);
    assert_eq!(page.start, 3);
    // Default when absent, and clamped upper bound.
    assert_eq!(parse_list_models_query(None).unwrap().size, 50);
    assert_eq!(
        parse_list_models_query(Some("pageSize=999999"))
            .unwrap()
            .size,
        1000
    );
    // Query-string API keys stay rejected.
    assert!(parse_list_models_query(Some("key=leak")).is_err());
}

#[test]
fn native_stream_error_value_is_sanitized() {
    let wrapper = json!({
        "error": {
            "code": 429,
            "status": "RESOURCE_EXHAUSTED",
            "message": "project paid-project-99 for owner@example.invalid exceeded quota"
        }
    });
    let value = native_stream_error_value(&wrapper).expect("error value");
    assert_eq!(value["error"]["status"], "RESOURCE_EXHAUSTED");
    assert_eq!(value["error"]["code"], 429);
    // Upstream private detail must never survive into the public element.
    let text = value.to_string();
    assert!(!text.contains("paid-project-99"));
    assert!(!text.contains("owner@example.invalid"));
    // A credit/accounting-only frame with no error has no public representation.
    assert!(native_stream_error_value(&json!({"consumedCredits": 3})).is_none());
}

#[test]
fn settlement_usage_survives_every_envelope_google_reports_it_in() {
    // Three real stream shapes used to leave the turn unmetered. Each one settles the customer at
    // the conservative preflight hold — a double-digit multiple of the measured cost — so each is
    // pinned here with the counters that let the journal name which one occurred.
    fn drive(frames: &[&str]) -> SseTranslator {
        let mut translator = SseTranslator::new_with_image_usage(
            StreamFraming::Sse,
            "gemini-3.1-pro-preview",
            0,
            AudioUsageHint::default(),
        );
        for frame in frames {
            translator.push(frame.as_bytes()).unwrap();
        }
        translator
    }

    // Usage reported beside the response envelope rather than inside it.
    let beside = drive(&[concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]},",
        "\"finishReason\":\"STOP\"}]},",
        "\"usageMetadata\":{\"promptTokenCount\":100,\"candidatesTokenCount\":5,\"totalTokenCount\":105}}\n\n"
    )]);
    assert!(!beside.usage.is_zero());
    assert_eq!(beside.usage.input_tokens, 100);
    assert_eq!(beside.shape.last_finish_reason.as_deref(), Some("STOP"));

    // Usage reported in a trailing envelope that carries no response at all.
    let trailing = drive(&[
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}]}}\n\n",
        "data: {\"usageMetadata\":{\"promptTokenCount\":100,\"candidatesTokenCount\":5,\"totalTokenCount\":105}}\n\n",
    ]);
    assert!(!trailing.usage.is_zero());
    assert_eq!(trailing.usage.output_tokens, 5);
    assert_eq!(trailing.shape.envelope_only_frames, 1);

    // A terminal frame whose usageMetadata carries no counts must not erase the reported ones.
    let annotated = drive(&[
        concat!(
            "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}],",
            "\"usageMetadata\":{\"promptTokenCount\":100,\"candidatesTokenCount\":5,\"totalTokenCount\":105}}}\n\n"
        ),
        concat!(
            "data: {\"response\":{\"candidates\":[{\"finishReason\":\"STOP\"}],",
            "\"usageMetadata\":{\"trafficType\":\"PROVISIONED_THROUGHPUT\"}}}\n\n"
        ),
    ]);
    assert!(!annotated.usage.is_zero());
    assert_eq!(annotated.usage.input_tokens, 100);
    assert_eq!(annotated.shape.usage_frames, 2);
    assert_eq!(annotated.shape.countless_usage_frames, 1);

    // A turn Google genuinely reports no usage for stays unmetered — and says so in its shape.
    let silent = drive(&[concat!(
        "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[",
        "{\"functionCall\":{\"name\":\"f\",\"args\":{}}}]},\"finishReason\":\"STOP\"}]}}\n\n"
    )]);
    assert!(silent.usage.is_zero());
    assert_eq!(silent.shape.usage_frames, 0);
    assert_eq!(silent.shape.frames, 1);
}

#[test]
fn json_array_framing_wraps_elements_and_closes() {
    let mut translator = SseTranslator::new_with_image_usage(
        StreamFraming::JsonArray,
        "gemini-integration-model",
        0,
        AudioUsageHint::default(),
    );
    let first = translator.frame(&json!({"a": 1})).unwrap();
    let second = translator.frame(&json!({"b": 2})).unwrap();
    let close = translator.finish_stream().unwrap();
    let whole = [first.as_ref(), second.as_ref(), close.as_ref()].concat();
    let parsed: Value = serde_json::from_slice(&whole).unwrap();
    assert_eq!(parsed, json!([{"a": 1}, {"b": 2}]));
    // An empty JSON-array stream still closes as a valid empty array.
    let mut empty = SseTranslator::new_with_image_usage(
        StreamFraming::JsonArray,
        "gemini-integration-model",
        0,
        AudioUsageHint::default(),
    );
    assert_eq!(empty.finish_stream().unwrap().as_ref(), b"[]");
    // SSE framing has no terminator and keeps the data: envelope.
    let mut sse = SseTranslator::new_with_image_usage(
        StreamFraming::Sse,
        "gemini-integration-model",
        0,
        AudioUsageHint::default(),
    );
    let frame = sse.frame(&json!({"a": 1})).unwrap();
    assert!(frame.starts_with(b"data: "));
    assert!(sse.finish_stream().is_none());
}

#[test]
fn private_stream_prelude_has_independent_byte_and_chunk_bounds() {
    let mut bytes = 0usize;
    let mut chunks = 0usize;
    account_stream_start_chunk(
        &mut bytes,
        &mut chunks,
        STREAM_START_MAX_BYTES,
        STREAM_START_MAX_BYTES,
        STREAM_START_MAX_CHUNKS,
    )
    .unwrap();
    assert!(account_stream_start_chunk(
        &mut bytes,
        &mut chunks,
        1,
        STREAM_START_MAX_BYTES,
        STREAM_START_MAX_CHUNKS,
    )
    .is_err());

    let mut bytes = 0usize;
    let mut chunks = 0usize;
    for _ in 0..STREAM_START_MAX_CHUNKS {
        account_stream_start_chunk(
            &mut bytes,
            &mut chunks,
            0,
            STREAM_START_MAX_BYTES,
            STREAM_START_MAX_CHUNKS,
        )
        .unwrap();
    }
    assert!(account_stream_start_chunk(
        &mut bytes,
        &mut chunks,
        0,
        STREAM_START_MAX_BYTES,
        STREAM_START_MAX_CHUNKS,
    )
    .is_err());

    let mut image_bytes = 0usize;
    let mut image_chunks = 0usize;
    account_stream_start_chunk(
        &mut image_bytes,
        &mut image_chunks,
        STREAM_START_MAX_BYTES + 1,
        GEMINI_BODY_LIMIT,
        IMAGE_STREAM_START_MAX_CHUNKS,
    )
    .unwrap();
}

#[tokio::test]
async fn streaming_without_alt_returns_a_native_json_array() {
    let first = Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"one\"}]}}]},\"traceId\":\"t\"}\n\n",
    );
    let usage = Bytes::from_static(
        b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":4}}}\n\n",
    );
    let (reply, _drained) = MockReply::stream(vec![MockChunk::Data(first), MockChunk::Data(usage)]);
    let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let response = invoke_uri(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-integration-model:streamGenerateContent",
        json!({"contents": []}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    // The native default streams a JSON array, not Server-Sent Events.
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    let bytes = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).expect("valid JSON array body");
    let array = parsed.as_array().expect("top-level array");
    assert_eq!(array.len(), 2);
    assert!(array[0]["candidates"].is_array());
    assert_eq!(array[1]["usageMetadata"]["promptTokenCount"], 10);
    // Private wrapper fields never surface, and each element carries the same responseId.
    assert!(!parsed.to_string().contains("traceId"));
    assert_eq!(array[0]["responseId"], array[1]["responseId"]);
    assert!(array[0]["responseId"]
        .as_str()
        .is_some_and(|id| !id.is_empty()));
}

#[tokio::test]
async fn tiered_public_model_uses_one_wire_id_for_generate_stream_and_count_tokens() {
    let non_stream = MockReply::json(
        StatusCode::OK,
        json!({
            "response": {
                "candidates": [{"content": {"role": "model", "parts": [{"text": "ok"}]}}],
                "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 1},
                "modelVersion": "gemini-3.6-flash-high"
            }
        }),
    );
    let (stream, _drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"ok\"}]}}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":1},\"modelVersion\":\"gemini-3.6-flash-high\"}}\n\n",
    ))]);
    let count = MockReply::json(StatusCode::OK, json!({"totalTokens": 2}));
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![non_stream, stream, count],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        0,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.6-flash"],
    );
    let app = app_state(fixture.gateway.clone(), None);
    let generation = json!({
        "contents": [{"role": "user", "parts": [{"text": "reply ok"}]}],
        "generationConfig": {"thinkingConfig": {"thinkingLevel": "high"}}
    });

    let response = invoke_uri(
        app.clone(),
        "/v1beta/models/gemini-3.6-flash:generateContent",
        generation.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(CALIBRATION_DISPATCH_HEADER));
    let response = response_json(response).await;
    assert_eq!(response["modelVersion"], "gemini-3.6-flash");

    let response = invoke_uri(
        app.clone(),
        "/v1beta/models/gemini-3.6-flash:streamGenerateContent?alt=sse",
        generation,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(CALIBRATION_DISPATCH_HEADER));
    let stream_body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    let stream_text = std::str::from_utf8(&stream_body).unwrap();
    assert!(stream_text.contains("\"modelVersion\":\"gemini-3.6-flash\""));
    assert!(!stream_text.contains("gemini-3.6-flash-high"));

    let response = invoke_uri(
        app,
        "/v1beta/models/gemini-3.6-flash:countTokens",
        json!({
            "generateContentRequest": {
                "model": "models/caller-model",
                "contents": [{"role": "user", "parts": [{"text": "reply ok"}]}],
                "generationConfig": {"thinkingConfig": {"thinkingLevel": "high"}}
            }
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(CALIBRATION_DISPATCH_HEADER));
    assert_eq!(response_json(response).await["totalTokens"], 2);

    let seen = server.state.seen();
    assert_eq!(seen.len(), 3);
    for request in &seen {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let wire = body
            .get("model")
            .and_then(Value::as_str)
            .or_else(|| body.pointer("/request/model").and_then(Value::as_str))
            .unwrap();
        assert!(wire.ends_with("gemini-3.6-flash-high"));
    }
}

#[tokio::test]
async fn published_37_uses_observed_tiered_wire_for_ordinary_and_exact_requests() {
    let ordinary_non_stream = MockReply::json(
        StatusCode::OK,
        json!({
            "response": {
                "candidates": [{"content": {"role": "model", "parts": [{"text": "ok"}]}}],
                "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 1},
                "modelVersion": "gemini-3.7-flash-upstream-canary"
            }
        }),
    );
    let (ordinary_stream, _ordinary_drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"ok\"}]}}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":1},\"modelVersion\":\"gemini-3.7-flash-upstream-canary\"}}\n\n",
    ))]);
    let ordinary_count = MockReply::json(StatusCode::OK, json!({"totalTokens": 2}));
    let exact_non_stream = MockReply::json(
        StatusCode::OK,
        json!({
            "response": {
                "candidates": [{"content": {"role": "model", "parts": [{"text": "ok"}]}}],
                "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 1},
                "modelVersion": "gemini-3.7-flash-upstream-canary"
            }
        }),
    );
    let (exact_stream, _exact_drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"ok\"}]}}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":1},\"modelVersion\":\"gemini-3.7-flash-upstream-canary\"}}\n\n",
    ))]);
    let exact_count = MockReply::json(StatusCode::OK, json!({"totalTokens": 2}));
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            ordinary_non_stream,
            ordinary_stream,
            ordinary_count,
            exact_non_stream,
            exact_stream,
            exact_count,
        ],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        0,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
    );
    let app = app_state(fixture.gateway.clone(), None);
    let ordinary_generation = json!({
        "contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]
    });
    for (uri, body) in [
        (
            "/v1beta/models/gemini-3.7-flash:generateContent",
            ordinary_generation.clone(),
        ),
        (
            "/v1beta/models/gemini-3.7-flash:streamGenerateContent?alt=sse",
            ordinary_generation,
        ),
        (
            "/v1beta/models/gemini-3.7-flash:countTokens",
            json!({
                "generateContentRequest": {
                    "contents": [{"role": "user", "parts": [{"text": "hello"}]}]
                }
            }),
        ),
    ] {
        let response = invoke_uri(app.clone(), uri, body).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(server.state.seen().len(), 3);

    let missing_deadline = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-3.7-flash:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .header("x-apitoken-calibration-profile", "profile_a")
        .header(
            "x-apitoken-calibration-request-id",
            "123e4567-e89b-42d3-a456-426614174000",
        )
        .body(Body::from(
            serde_json::to_vec(&json!({
                "contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let missing_deadline = api_inner(
        app.clone(),
        "198.51.100.10:12345".parse().unwrap(),
        missing_deadline,
    )
    .await
    .unwrap_err()
    .into_response();
    assert_eq!(missing_deadline.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        missing_deadline
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("not_started")
    );
    assert_eq!(server.state.seen().len(), 3);

    let expired = invoke_exact_uri_with_deadline(
        app.clone(),
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({"contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]}),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174009",
        Some(pool::now() as u64),
    )
    .await;
    assert_eq!(expired.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        expired
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("not_started")
    );
    assert!(!expired.headers().contains_key(CALIBRATION_DISPATCH_HEADER));
    assert_eq!(server.state.seen().len(), 3);

    let generation = json!({
        "contents": [{"role": "user", "parts": [{"text": "reply ok"}]}],
        "generationConfig": {"thinkingConfig": {"thinkingLevel": "high"}}
    });

    for (uri, request_id) in [
        (
            "/v1beta/models/gemini-3.7-flash:generateContent",
            "123e4567-e89b-42d3-a456-426614174001",
        ),
        (
            "/v1beta/models/gemini-3.7-flash:streamGenerateContent?alt=sse",
            "123e4567-e89b-42d3-a456-426614174002",
        ),
    ] {
        let response = invoke_exact_uri(
            app.clone(),
            uri,
            generation.clone(),
            "profile_a",
            request_id,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let dispatch_ms = calibration_dispatch_ms(&response);
        assert!(dispatch_ms < (pool::now() as u64 + 60) * 1_000);
        let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("gemini-3.7-flash-upstream-canary"));
        assert!(!body.contains("\"modelVersion\":\"gemini-3.7-flash\""));
    }

    let response = invoke_exact_uri(
        app,
        "/v1beta/models/gemini-3.7-flash:countTokens",
        json!({
            "generateContentRequest": {
                "model": "models/caller-model",
                "contents": [{"role": "user", "parts": [{"text": "reply ok"}]}],
                "generationConfig": {"thinkingConfig": {"thinkingLevel": "high"}}
            }
        }),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174003",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let dispatch_ms = calibration_dispatch_ms(&response);
    assert!(dispatch_ms < (pool::now() as u64 + 60) * 1_000);
    assert_eq!(response_json(response).await["totalTokens"], 2);

    let seen = server.state.seen();
    assert_eq!(seen.len(), 6);
    for request in &seen {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let wire = body
            .get("model")
            .and_then(Value::as_str)
            .or_else(|| body.pointer("/request/model").and_then(Value::as_str))
            .unwrap();
        assert!(wire.ends_with("gemini-3.7-flash-tiered"));
    }
}

#[tokio::test]
async fn published_37_customer_uses_tiered_wire_and_public_response_identity() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({
                "response": {
                    "candidates": [{"content": {"role": "model", "parts": [{"text": "ok"}]}}],
                    "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 1},
                    "modelVersion": "gemini-3.7-flash-tiered"
                }
            }),
        )],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
    );
    let app = app_state(fixture.gateway.clone(), None);

    let response = invoke_uri(
        app,
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({"contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]}),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await["modelVersion"],
        "gemini-3.7-flash"
    );
    let seen = server.state.seen();
    assert_eq!(seen.len(), 1);
    let wire: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert!(wire["model"]
        .as_str()
        .is_some_and(|model| model.ends_with("gemini-3.7-flash-tiered")));
}

#[tokio::test]
async fn dormant_37_rejects_removed_controls_and_prefill_before_upstream() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            MockReply::json(StatusCode::OK, json!({"totalTokens": 2})),
            MockReply::json(StatusCode::OK, json!({"totalTokens": 2})),
        ],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        0,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash", "gemini-3.6-flash"],
    );
    let app = app_state(fixture.gateway.clone(), None);

    for generation_config in [
        json!({"temperature": 0.5}),
        json!({"topP": 0.9}),
        json!({"top_k": 20}),
        json!({"candidate_count": 1}),
        json!({"thinkingConfig": {"thinkingBudget": 64}}),
        json!({"thinking_config": {"thinking_budget": 64}}),
    ] {
        let response = invoke_exact_uri(
            app.clone(),
            "/v1beta/models/gemini-3.7-flash:generateContent",
            json!({
                "contents": [{"role": "user", "parts": [{"text": "hello"}]}],
                "generationConfig": generation_config
            }),
            "profile_a",
            "123e4567-e89b-42d3-a456-426614174010",
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    let response = invoke_exact_uri(
        app.clone(),
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({
            "contents": [
                {"role": "user", "parts": [{"text": "hello"}]},
                {"role": "model", "parts": [{"text": "prefilled"}]}
            ]
        }),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174011",
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Role inference must match the Antigravity wire normalizer: after an explicit user turn an
    // omitted role becomes `model`, so this is a forbidden prefill rather than a second user turn.
    let response = invoke_exact_uri(
        app.clone(),
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({
            "contents": [
                {"role": "user", "parts": [{"text": "hello"}]},
                {"parts": [{"text": "prefilled through omitted role"}]}
            ]
        }),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174012",
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(server.state.seen().is_empty());

    for final_turn in [
        json!({"role": "user", "parts": []}),
        json!({"role": "user", "parts": [{"text": "   "}]}),
        json!({"role": "user", "parts": [{"inlineData": {"mimeType": "image/png", "data": "AA=="}}]}),
    ] {
        let response = invoke_exact_uri(
            app.clone(),
            "/v1beta/models/gemini-3.7-flash:generateContent",
            json!({
                "contents": [
                    {"role": "user", "parts": [{"text": "hello"}]},
                    {"role": "model", "parts": [{"text": "prefilled"}]},
                    final_turn
                ]
            }),
            "profile_a",
            "123e4567-e89b-42d3-a456-426614174013",
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // A final user turn of only functionResponse parts is exactly the shape the closed live
    // admission lane proved (run gemini-cal-1787152582-af5e9cfb), so it passes local validation
    // on every lane; the mock has no generation reply queued, so dispatch fails closed with 503
    // rather than a local 400.
    let response = invoke_exact_uri(
        app.clone(),
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({
            "contents": [
                {"role": "user", "parts": [{"text": "hello"}]},
                {"role": "model", "parts": [{"functionCall": {"name": "lookup", "args": {}}}]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "lookup", "response": {"ok": true}}}
                ]}
            ]
        }),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174015",
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    // The admitted tool-result shape spends exactly one one-shot dispatch against the mock
    // upstream (the exact lane forbids retry/rotation), receives no usable reply and terminates.
    let seen = server.state.seen();
    let generations = seen
        .iter()
        .filter(|request| request.uri.contains(":generateContent"))
        .count();
    assert_eq!(generations, 1, "exactly one one-shot generation dispatch");

    let response = invoke_exact_uri(
        app.clone(),
        "/v1beta/models/gemini-3.7-flash:countTokens",
        json!({
            "generate_content_request": {
                "contents": [{"role": "user", "parts": [{"text": "hello"}]}],
                "generation_config": {"top_p": 0.9}
            }
        }),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174014",
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    // Only the admitted tool-result one-shot dispatch may have reached the upstream so far.
    let generations_so_far = server
        .state
        .seen()
        .iter()
        .filter(|request| request.uri.contains(":generateContent"))
        .count();
    assert_eq!(generations_so_far, 1);

    // The removal is model-specific: the existing 3.6 route retains its current sampling and
    // historical-model-turn behavior.
    let response = invoke_uri(
        app,
        "/v1beta/models/gemini-3.6-flash:countTokens",
        json!({
            "generateContentRequest": {
                "contents": [
                    {"role": "user", "parts": [{"text": "hello"}]},
                    {"role": "model", "parts": [{"text": "prefilled"}]}
                ],
                "generationConfig": {"topP": 0.9}
            }
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["totalTokens"], 2);
    // The 3.6 count consumed its queued mock reply on top of the admitted one-shot dispatch.
    assert_eq!(server.state.seen().len(), 2);
}

#[test]
fn gate_37_admits_tool_result_final_turn_for_all_traffic() {
    let model = catalog_model("gemini-3.7-flash");
    let tool_loop = || {
        json!({
            "contents": [
                {"role": "user", "parts": [{"text": "look it up"}]},
                {"role": "model", "parts": [{"functionCall": {"name": "lookup", "args": {}}}]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "lookup", "response": {"ok": true}}}
                ]}
            ]
        })
    };
    // The exact-SHA live gate proved the wire contract (run
    // gemini-cal-1787152582-af5e9cfb: upstream returned incremental SSE with visible text and
    // terminal usage), so ordinary traffic admits the tool-result-only final turn too.
    assert!(validate_generation_request(&tool_loop(), &model, false).is_ok());
    // The one-shot exact-profile calibration lane admits exactly this shape.
    assert!(validate_generation_request(&tool_loop(), &model, true).is_ok());

    // A mixed final turn (text plus a tool result) stays admitted for ordinary traffic too.
    let mixed = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "look it up"}]},
            {"role": "model", "parts": [{"functionCall": {"name": "lookup", "args": {}}}]},
            {"role": "user", "parts": [
                {"functionResponse": {"name": "lookup", "response": {"ok": true}}},
                {"text": "thanks"}
            ]}
        ]
    });
    assert!(validate_generation_request(&mixed, &model, false).is_ok());
    assert!(validate_generation_request(&mixed, &model, true).is_ok());

    // The gate must not become a blanket bypass: empty parts, whitespace-only text, image-only
    // finals and prefilled model finals stay rejected on both lanes.
    for final_turn in [
        json!({"role": "user", "parts": []}),
        json!({"role": "user", "parts": [{"text": "   "}]}),
        json!({"role": "user", "parts": [{"inlineData": {"mimeType": "image/png", "data": "AA=="}}]}),
        json!({"role": "user", "parts": [{"functionResponse": {"name": "lookup", "response": {"ok": true}}}, {"inlineData": {"mimeType": "image/png", "data": "AA=="}}]}),
        json!({"role": "model", "parts": [{"functionResponse": {"name": "lookup", "response": {"ok": true}}}]}),
    ] {
        let body = json!({
            "contents": [
                {"role": "user", "parts": [{"text": "look it up"}]},
                {"role": "model", "parts": [{"functionCall": {"name": "lookup", "args": {}}}]},
                final_turn
            ]
        });
        assert!(
            validate_generation_request(&body, &model, true).is_err(),
            "exact lane must stay closed for {final_turn}"
        );
        assert!(
            validate_generation_request(&body, &model, false).is_err(),
            "ordinary traffic must stay closed for {final_turn}"
        );
    }

    // The same admission applies to the free countTokens fence on both lanes.
    let nested = json!({
        "generateContentRequest": tool_loop()
    });
    assert!(validate_native_request(Operation::CountTokens, &nested, &model, true).is_ok());
    assert!(validate_native_request(Operation::CountTokens, &nested, &model, false).is_ok());
}

#[test]
fn published_37_accepts_standard_output_bounds() {
    let request = |max_output_tokens| {
        json!({
            "contents": [{"role": "user", "parts": [{"text": "reply ok"}]}],
            "generationConfig": {"maxOutputTokens": max_output_tokens}
        })
    };
    let model = catalog_model("gemini-3.7-flash");

    assert!(validate_generation_request(&request(256), &model, false).is_ok());
    assert!(validate_generation_request(&request(512), &model, false).is_ok());
    assert!(validate_generation_request(&request(513), &model, false).is_ok());
}

#[tokio::test]
async fn dormant_37_exact_generation_requires_an_already_fresh_cached_bearer() {
    let server = start_mock(MockState::default()).await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_models_and_expiry(
        &server.upstream,
        &[None],
        2,
        Some(&token_uri),
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
        pool::now(),
    );

    let response = invoke_exact_uri(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({"contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]}),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174015",
    )
    .await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some(crate::proxy::EXECUTION_STATE_NOT_STARTED)
    );
    assert!(response
        .headers()
        .get(CALIBRATION_DISPATCH_HEADER)
        .is_none());
    assert!(
        server.state.seen().is_empty(),
        "paid generation must not refresh or dispatch with a stale bearer"
    );
}

#[tokio::test]
async fn dormant_37_exact_count_refreshes_once_before_dispatch() {
    let refreshed = "gemini-profile-a-pre-dispatch-count-token";
    let server = start_mock(MockState::with_replies([
        (
            "",
            vec![MockReply::Json {
                status: StatusCode::OK,
                body: json!({"access_token": refreshed, "expires_in": 3600}),
                retry_after: None,
            }],
        ),
        (
            refreshed,
            vec![MockReply::json(StatusCode::OK, json!({"totalTokens": 7}))],
        ),
    ]))
    .await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_models_and_expiry(
        &server.upstream,
        &[None],
        2,
        Some(&token_uri),
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
        pool::now(),
    );

    let response = invoke_exact_count_tokens(
        app_state(fixture.gateway.clone(), None),
        "gemini-3.7-flash",
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174016",
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(calibration_dispatch_ms(&response) > 0);
    assert_eq!(response_json(response).await, json!({"totalTokens": 7}));
    let seen = server.state.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].uri, "/token");
    assert_eq!(seen[1].uri, "/v1internal:countTokens");
    assert_eq!(seen[1].credential, refreshed);
}

#[tokio::test]
async fn dormant_37_exact_count_rechecks_deadline_after_refresh() {
    let refreshed = "gemini-profile-a-too-late-count-token";
    let server = start_mock(MockState::with_replies([(
        "",
        vec![MockReply::DelayedJson {
            status: StatusCode::OK,
            body: json!({"access_token": refreshed, "expires_in": 3600}),
            delay: Duration::from_millis(1_200),
        }],
    )]))
    .await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_models_and_expiry(
        &server.upstream,
        &[None],
        2,
        Some(&token_uri),
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
        pool::now(),
    );
    let not_after = (pool::now() + 1) as u64;

    let response = invoke_exact_uri_with_deadline(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-3.7-flash:countTokens",
        json!({"contents": [{"role": "user", "parts": [{"text": "count me"}]}]}),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174017",
        Some(not_after),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some(crate::proxy::EXECUTION_STATE_NOT_STARTED)
    );
    assert!(response
        .headers()
        .get(CALIBRATION_DISPATCH_HEADER)
        .is_none());
    let seen = server.state.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].uri, "/token");
}

#[tokio::test]
async fn dormant_37_exact_generation_does_not_refresh_or_replay_after_401() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            MockReply::json(
                StatusCode::UNAUTHORIZED,
                json!({"error": {"message": "rejected exact attempt"}}),
            ),
            MockReply::json(
                StatusCode::OK,
                json!({
                    "candidates": [{"content": {"role": "model", "parts": [{"text": "must not be used"}]}}],
                    "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
                }),
            ),
        ],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        2,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
    );

    let response = invoke_exact_uri(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({"contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]}),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174020",
    )
    .await;

    assert!(!response.status().is_success());
    assert!(response
        .headers()
        .get(crate::proxy::EXECUTION_STATE_HEADER)
        .is_none());
    assert_eq!(server.state.seen().len(), 1);
}

#[tokio::test]
async fn dormant_37_exact_count_tokens_does_not_refresh_or_replay_after_401() {
    let refreshed = "gemini-profile-a-count-refreshed-access-token";
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::UNAUTHORIZED,
                json!({"error": {"message": "rejected exact count"}}),
            )],
        ),
        (
            "",
            vec![MockReply::Json {
                status: StatusCode::OK,
                body: json!({"access_token": refreshed, "expires_in": 3600}),
                retry_after: None,
            }],
        ),
        (
            refreshed,
            vec![MockReply::json(StatusCode::OK, json!({"totalTokens": 9}))],
        ),
    ]))
    .await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        2,
        Some(&token_uri),
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
    );

    let response = invoke_exact_count_tokens(
        app_state(fixture.gateway.clone(), None),
        "gemini-3.7-flash",
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174021",
    )
    .await;

    assert!(!response.status().is_success());
    assert!(response
        .headers()
        .get(crate::proxy::EXECUTION_STATE_HEADER)
        .is_none());
    let seen = server.state.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].uri, "/v1internal:countTokens");
    assert!(seen.iter().all(|request| request.uri != "/token"));
}

#[tokio::test]
async fn ordinary_count_tokens_retains_401_refresh_and_same_profile_retry() {
    let refreshed = "gemini-profile-a-ordinary-count-refreshed-access-token";
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::UNAUTHORIZED,
                json!({"error": {"message": "expired ordinary count token"}}),
            )],
        ),
        (
            "",
            vec![MockReply::Json {
                status: StatusCode::OK,
                body: json!({"access_token": refreshed, "expires_in": 3600}),
                retry_after: None,
            }],
        ),
        (
            refreshed,
            vec![MockReply::json(StatusCode::OK, json!({"totalTokens": 11}))],
        ),
    ]))
    .await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_token_uri(&server.upstream, &[None], 2, Some(&token_uri));

    let response = invoke_uri(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-integration-model:countTokens",
        json!({"contents": [{"role": "user", "parts": [{"text": "count me"}]}]}),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, json!({"totalTokens": 11}));
    let seen = server.state.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(
        seen.iter()
            .filter(|request| request.uri == "/v1internal:countTokens")
            .count(),
        2
    );
    assert_eq!(
        seen.iter()
            .filter(|request| request.uri == "/token")
            .count(),
        1
    );
}

#[tokio::test]
async fn exact_count_tokens_refresh_helper_crash_is_attempted_once() {
    let (helper_directory, helper, spawns, attempts) = crashing_node_helper();
    let fixture = gateway_fixture_with_models_calibration_and_node(
        // A non-loopback, non-production test scheme selects the Node helper without weakening
        // the production binary/host attestation contract or opening a real network connection.
        "node-test://cloudcode-pa.googleapis.com",
        &[Some("http://127.0.0.1:18080")],
        2,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
        None,
        Some((
            helper.to_str().unwrap(),
            gemini_credential::GEMINI_NODE_VERSION,
            gemini_credential::GEMINI_NODE_SHA256,
        )),
        Some(pool::now()),
    );

    let response = invoke_exact_count_tokens(
        app_state(fixture.gateway.clone(), None),
        "gemini-3.7-flash",
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174022",
    )
    .await;

    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    assert!(!status.is_success());
    assert_eq!(
        headers
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some(crate::proxy::EXECUTION_STATE_NOT_STARTED)
    );
    assert_eq!(
        fs::read_to_string(&spawns)
            .unwrap_or_else(|error| panic!(
                "helper did not spawn: status={status} headers={headers:?} body={} error={error}",
                String::from_utf8_lossy(&body)
            ))
            .lines()
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(&attempts).unwrap_or_else(|error| panic!(
            "helper did not receive request: status={status} headers={headers:?} body={} error={error}",
            String::from_utf8_lossy(&body)
        ))
        .lines()
        .count(),
        1
    );
    drop(fixture);
    let _ = fs::remove_dir_all(helper_directory);
}

#[tokio::test]
async fn every_exact_target_generation_is_one_shot_after_provider_dispatch() {
    for (index, reply) in [
        MockReply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error": {"status": "UNAVAILABLE"}}),
        ),
        MockReply::json(StatusCode::OK, json!({"response": []})),
    ]
    .into_iter()
    .enumerate()
    {
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![
                reply,
                MockReply::json(
                    StatusCode::OK,
                    json!({
                        "candidates": [{"content": {"role": "model", "parts": [{"text": "must not be used"}]}}],
                        "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
                    }),
                ),
            ],
        )]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None], 2);
        let request_id = format!("123e4567-e89b-42d3-a456-4266141741{index:02}");

        let response = invoke_exact_calibration(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]}),
            "profile_a",
            &request_id,
        )
        .await;

        assert!(!response.status().is_success());
        assert!(response
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .is_none());
        assert_eq!(server.state.seen().len(), 1);
    }
}

#[tokio::test]
async fn exact_target_429_records_bounded_diagnostic_and_cooling_without_replay() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            MockReply::Json {
                status: StatusCode::TOO_MANY_REQUESTS,
                body: json!({
                    "error": {
                        "code": 429,
                        "status": "RESOURCE_EXHAUSTED",
                        "message": "private project customer@example.test hit quota",
                        "details": [
                            {
                                "@type": "type.googleapis.com/google.rpc.RetryInfo",
                                "retryDelay": "7s"
                            },
                            {
                                "@type": "type.googleapis.com/google.rpc.QuotaFailure",
                                "violations": [{
                                    "subject": "projects/private-project/locations/global",
                                    "description": "private quota description"
                                }]
                            }
                        ]
                    }
                }),
                retry_after: Some("7"),
            },
            MockReply::json(
                StatusCode::OK,
                json!({
                    "candidates": [{
                        "content": {"role": "model", "parts": [{"text": "must not be used"}]}
                    }],
                    "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
                }),
            ),
        ],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        2,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash"],
    );
    let app = app_state(fixture.gateway.clone(), None);

    let response = invoke_exact_uri(
        app.clone(),
        "/v1beta/models/gemini-3.7-flash:generateContent",
        json!({"contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]}),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174030",
    )
    .await;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("7")
    );
    assert!(response
        .headers()
        .get(crate::proxy::EXECUTION_STATE_HEADER)
        .is_none());
    let body = response_json(response).await;
    assert_eq!(body["error"]["code"], 429);
    assert_eq!(body["error"]["status"], "RESOURCE_EXHAUSTED");
    let public_body = body.to_string();
    assert!(!public_body.contains("customer@example.test"));
    assert!(!public_body.contains("private-project"));
    assert!(!public_body.contains("private quota description"));
    assert_eq!(server.state.seen().len(), 1);
    assert_eq!(Metrics::get(&app.metrics.upstream_429), 1);

    let now = pool::now();
    let status = fixture.gateway.operational_status().await;
    let profile = status
        .profiles
        .iter()
        .find(|profile| profile.id == "profile_a")
        .expect("exact target profile remains visible in the private operator projection");
    assert!(profile.model_cooling.iter().any(|cooling| {
        cooling.model_id == "gemini-3.7-flash" && cooling.cooling_until >= now + 6
    }));
}

#[tokio::test]
async fn exact_target_stream_start_provider_error_is_terminal_and_execution_ambiguous() {
    let (provider_error, _drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
        b"data: {\"error\":{\"code\":429,\"message\":\"quota\"}}\n\n",
    ))]);
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            provider_error,
            MockReply::json(StatusCode::OK, json!({"must": "not be used"})),
        ],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 2);

    let response = invoke_exact_uri(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-integration-model:streamGenerateContent?alt=sse",
        json!({"contents": [{"role": "user", "parts": [{"text": "reply ok"}]}]}),
        "profile_a",
        "123e4567-e89b-42d3-a456-426614174120",
    )
    .await;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response
        .headers()
        .get(crate::proxy::EXECUTION_STATE_HEADER)
        .is_none());
    assert_eq!(server.state.seen().len(), 1);
}

#[tokio::test]
async fn published_37_is_listed_and_resolves_by_exact_public_id() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        Vec::<MockReply>::new(),
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        0,
        None,
        OAuthKind::Antigravity,
        65_536,
        &["gemini-3.7-flash", "gemini-3.6-flash"],
    );
    let app = app_state(fixture.gateway.clone(), None);

    let list = axum::extract::Request::builder()
        .method(Method::GET)
        .uri("/v1beta/models")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::empty())
        .unwrap();
    let response = api_inner(app.clone(), "198.51.100.10:12345".parse().unwrap(), list)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["models"].as_array().unwrap().len(), 2);
    assert_eq!(body["models"][0]["name"], "models/gemini-3.7-flash");
    assert_eq!(body["models"][1]["name"], "models/gemini-3.6-flash");

    let get = axum::extract::Request::builder()
        .method(Method::GET)
        .uri("/v1beta/models/gemini-3.7-flash")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::empty())
        .unwrap();
    let response = api_inner(app, "198.51.100.10:12345".parse().unwrap(), get)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await["name"],
        "models/gemini-3.7-flash"
    );
}

#[tokio::test]
async fn invalid_key_maps_to_native_400_api_key_invalid() {
    let response = ApiError::from(AdmissionError::Unauthorized).into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["error"]["code"], 400);
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    let details = body["error"]["details"].as_array().expect("details");
    assert!(details.iter().any(|detail| {
        detail["@type"]
            .as_str()
            .is_some_and(|kind| kind.ends_with("google.rpc.ErrorInfo"))
            && detail["reason"] == "API_KEY_INVALID"
    }));
}

#[tokio::test]
async fn rate_limited_error_carries_retry_info_detail() {
    let response = ApiError::rate_limited(Some(7)).into_response();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("7")
    );
    let body = response_json(response).await;
    assert_eq!(body["error"]["status"], "RESOURCE_EXHAUSTED");
    let details = body["error"]["details"].as_array().expect("details");
    assert!(details.iter().any(|detail| {
        detail["@type"]
            .as_str()
            .is_some_and(|kind| kind.ends_with("google.rpc.RetryInfo"))
            && detail["retryDelay"] == "7s"
    }));
}

#[tokio::test]
async fn every_public_error_marks_the_execution_not_started() {
    // Все конструкторы публичных ошибок плоскости — не-2xx отказы до первого публичного
    // байта: admission ещё в Option и при дропе возвращает reserve через HoldGuard
    // (refund). Каждый такой ответ обязан нести not_started.
    for error in [
        ApiError::invalid("bad request"),
        ApiError::not_found(),
        ApiError::unavailable("test_unavailable"),
        ApiError::rate_limited(Some(3)),
        ApiError::provider_rejected(StatusCode::PAYLOAD_TOO_LARGE),
        ApiError::from(AdmissionError::Unauthorized),
        ApiError::from(AdmissionError::Unavailable),
        ApiError::from(AdmissionError::LowBalance),
    ] {
        let response = error.into_response();
        assert!(!response.status().is_success());
        assert_eq!(
            response
                .headers()
                .get(crate::proxy::EXECUTION_STATE_HEADER)
                .unwrap(),
            crate::proxy::EXECUTION_STATE_NOT_STARTED
        );
    }
}

#[tokio::test]
async fn inline_audio_is_rejected_before_provider_dispatch() {
    let server = start_mock(MockState::default()).await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::LegacyGeminiCli,
        64,
        &["gemini-3.1-flash-image"],
    );
    let body = json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "text": "describe this sound",
                "inlineData": {
                    "mimeType": "audio/wav",
                    "data": "UklGRg=="
                }
            }]
        }]
    });
    let request = axum::extract::Request::builder()
        .method(Method::POST)
        // Every published text model now admits inline audio; the image-generation
        // surface is the remaining fail-closed route.
        .uri("/v1beta/models/gemini-3.1-flash-image:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = match api_inner(
        app_state(fixture.gateway.clone(), None),
        "198.51.100.10:12345".parse().unwrap(),
        request,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .unwrap(),
        crate::proxy::EXECUTION_STATE_NOT_STARTED
    );
    assert!(
        server.state.seen().is_empty(),
        "inline audio must fail before any provider request"
    );
}

#[tokio::test]
async fn existing_profile_load_never_delays_a_new_request() {
    let success = MockReply::json(
        StatusCode::OK,
        json!({"candidates": [], "usageMetadata": {}}),
    );
    let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![success])])).await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let mut leases = Vec::new();
    for _ in 0..1_000 {
        leases.push(
            fixture
                .gateway
                .select("gemini-integration-model", &HashSet::new(), None, true)
                .unwrap(),
        );
    }
    let app = app_state(fixture.gateway.clone(), None);
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        invoke(
            app,
            json!({"contents": [{"parts": [{"text": "queued turn"}]}]}),
            false,
        ),
    )
    .await
    .expect("existing load must not gate upstream dispatch");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(server.state.seen().len(), 1);
    drop(leases);
}

#[tokio::test]
async fn large_fanout_reaches_upstream_without_waiting_for_existing_leases() {
    const FANOUT: usize = 32;
    let success = MockReply::json(
        StatusCode::OK,
        json!({"candidates": [], "usageMetadata": {}}),
    );
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![success; FANOUT],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let leases = (0..1_000)
        .map(|_| {
            fixture
                .gateway
                .select("gemini-integration-model", &HashSet::new(), None, true)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let app = app_state(fixture.gateway.clone(), None);
    let requests = (0..FANOUT)
        .map(|index| {
            tokio::spawn(invoke(
                app.clone(),
                json!({"contents": [{"parts": [{"text": format!("turn {index}")}]}]}),
                false,
            ))
        })
        .collect::<Vec<_>>();
    for request in requests {
        let response = tokio::time::timeout(Duration::from_secs(5), request)
            .await
            .expect("fanout dispatch is independent of existing leases")
            .expect("request task");
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(server.state.seen().len(), FANOUT);
    drop(leases);
}

#[test]
fn provider_rejected_never_emits_an_impossible_status_pair() {
    // A 413 upstream rejection must collapse to the native INVALID_ARGUMENT/400 pair, never
    // 413/FAILED_PRECONDITION.
    let error = ApiError::provider_rejected(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.google_status, "INVALID_ARGUMENT");
    let forbidden = ApiError::provider_rejected(StatusCode::FORBIDDEN);
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.google_status, "PERMISSION_DENIED");
}

async fn billed_app_with_balance(
    gateway: Arc<GeminiGateway>,
    balance_nano: i64,
) -> (AppState, Arc<AsyncBilling>, PathBuf) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "gemini-api-billing-{}-{unique}-{}.sqlite",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    let billing = Arc::new(
        AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
            .expect("start Gemini integration billing"),
    );
    billing
        .create_account(ACCOUNT_ID, None, 10_000)
        .await
        .unwrap();
    billing
        .topup(ACCOUNT_ID, balance_nano, Some("seed"))
        .await
        .unwrap();
    billing
        .issue_key(CUSTOMER_KEY, ACCOUNT_ID, None, None, None)
        .await
        .unwrap();
    (app_state(gateway, Some(billing.clone())), billing, path)
}

async fn billed_app(gateway: Arc<GeminiGateway>) -> (AppState, Arc<AsyncBilling>, PathBuf) {
    billed_app_with_balance(gateway, 1_000_000_000).await
}

#[tokio::test]
async fn immediate_transport_failure_releases_customer_reservation() {
    let fixture = gateway_fixture("http://127.0.0.1:1", &[None], 1);
    let leases = (0..1_000)
        .map(|_| {
            fixture
                .gateway
                .select("gemini-integration-model", &HashSet::new(), None, true)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let (app, billing, path) = billed_app(fixture.gateway.clone()).await;
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        invoke_with_identity(
            app,
            json!({"contents": [{"parts": [{"text": "fail without leaking money"}]}]}),
            false,
            CUSTOMER_KEY,
            None,
        ),
    )
    .await
    .expect("local load must not delay the transport attempt");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(leases);
    billing.flush().await.unwrap();
    assert_eq!(billing.totals().await.unwrap().reserved_nano, 0);
    drop(billing);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn admin_generation_persists_exact_provider_spend_without_customer_usage_event() {
    const CALIBRATION_REQUEST_ID: &str = "123e4567-e89b-42d3-a456-426614174000";
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({
                "candidates": [{
                    "content": {"role": "model", "parts": [{"text": "ok"}]}
                }],
                "usageMetadata": {
                    "promptTokenCount": 3,
                    "candidatesTokenCount": 1
                }
            }),
        )],
    )]))
    .await;
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "gemini-admin-capacity-{}-{unique}-{}.sqlite",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    let path_string = path.to_string_lossy().into_owned();
    let billing = Arc::new(AsyncBilling::start(path_string.clone(), 1).unwrap());
    let fixture = gateway_fixture_with_models_and_calibration(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::LegacyGeminiCli,
        64,
        &["gemini-integration-model"],
        Some(billing.clone()),
    );

    let response = invoke_exact_calibration(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]}),
        "profile_a",
        CALIBRATION_REQUEST_ID,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::timeout(
        Duration::from_millis(100),
        fixture.gateway.probe_requested(),
    )
    .await
    .expect("an exact settled turn must wake the free quota probe");
    billing.flush().await.unwrap();

    let connection = registry::open(&path_string).unwrap();
    assert_eq!(
        registry::provider_calibration_subject_spend(
            &connection,
            registry::PROVIDER_GOOGLE,
            "profile_a",
        )
        .unwrap()
        .spent_nano,
        4
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM provider_turn_calibration_events \
                 WHERE provider='google' AND subject_id='profile_a' AND request_id=?1",
                [CALIBRATION_REQUEST_ID],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM usage_events", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(connection);
    drop(fixture);
    drop(billing);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn quota_rotates_to_the_next_project_and_client_credential_never_reaches_google() {
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::Json {
                status: StatusCode::TOO_MANY_REQUESTS,
                body: json!({"error": {"status": "RESOURCE_EXHAUSTED"}}),
                retry_after: Some("12"),
            }],
        ),
        (
            PROFILE_B_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({
                    "candidates": [],
                    "usageMetadata": {"promptTokenCount": 3},
                    "internalIdentity": "paid-project-02 owner@example.invalid"
                }),
            )],
        ),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json; charset=UTF-8")
    );
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("*")
    );
    let public = response_json(response).await;
    assert!(public.get("response").is_none());
    assert!(public.get("consumedCredits").is_none());
    assert!(public.get("remainingCredits").is_none());
    // A native-shaped responseId is synthesized locally; it must be present but must not be the
    // correlatable upstream wrapper trace id.
    let response_id = public
        .get("responseId")
        .and_then(Value::as_str)
        .expect("native responseId is present");
    assert!(!response_id.is_empty());
    assert_ne!(response_id, "private-trace-id");
    assert!(!public.to_string().contains("private-trace-id"));
    assert!(public.get("internalIdentity").is_none());
    let seen = server.state.seen();
    assert_eq!(
        seen.iter()
            .map(|request| request.credential.as_str())
            .collect::<Vec<_>>(),
        [PROFILE_A_KEY, PROFILE_B_KEY]
    );
    assert!(seen
        .iter()
        .all(|request| request.credential != CUSTOMER_KEY));
}

#[tokio::test]
async fn concurrent_401s_refresh_once_and_retry_with_the_new_bearer() {
    let refreshed = "gemini-profile-a-refreshed-access-token";
    let unauthorized = MockReply::json(
        StatusCode::UNAUTHORIZED,
        json!({"error": {"message": "private rejected token"}}),
    );
    let success = MockReply::json(
        StatusCode::OK,
        json!({"candidates": [], "usageMetadata": {}}),
    );
    let token_reply = MockReply::Json {
        status: StatusCode::OK,
        body: json!({"access_token": refreshed, "expires_in": 3600}),
        retry_after: None,
    };
    let server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![unauthorized.clone(), unauthorized]),
        (refreshed, vec![success.clone(), success]),
        ("", vec![token_reply]),
    ]))
    .await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_token_uri(&server.upstream, &[None], 1, Some(&token_uri));
    let app = app_state(fixture.gateway.clone(), None);
    let body = json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]});
    let (first, second) = tokio::join!(
        invoke(app.clone(), body.clone(), false),
        invoke(app, body, false)
    );
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    let seen = server.state.seen();
    assert_eq!(
        seen.iter()
            .filter(|request| request.uri == "/token")
            .count(),
        1
    );
    assert_eq!(
        seen.iter()
            .filter(|request| request.uri == "/v1internal:generateContent")
            .filter(|request| request.credential == refreshed)
            .count(),
        2
    );
}

#[tokio::test]
async fn client_identity_headers_are_stripped_and_runtime_identity_is_truthful() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({
                "candidates": [],
                "usageMetadata": {
                    "promptTokenCount": 25,
                    "candidatesTokenCount": 7
                }
            }),
        )],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .header("authorization", "Bearer customer-secret")
        .header("user-agent", "GeminiCLI/forged")
        .header("client-metadata", "customer-identity")
        .header("x-goog-user-project", "customer-project")
        .header("x-goog-api-client", "forged-client")
        .header("x-forwarded-for", "203.0.113.9")
        .body(Body::from(
            br#"{"contents":[{"role":"user","parts":[{"text":"hello"}]}]}"#.as_slice(),
        ))
        .unwrap();
    let response = api_inner(
        app_state(fixture.gateway.clone(), None),
        "198.51.100.10:12345".parse().unwrap(),
        request,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let seen = server.state.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].credential, PROFILE_A_KEY);
    assert_eq!(
        seen[0].user_agent,
        "GeminiCLI/0.53.0/gemini-integration-model (linux; x64; cli) google-api-nodejs-client/10.9.0"
    );
    assert!(seen[0].client_metadata.is_empty());
    assert_eq!(seen[0].google_api_client, "gl-node/24.18.0");
    assert!(!seen[0].has_private_client_headers);
}

#[tokio::test]
async fn antigravity_generation_uses_agent_wrapper_and_keeps_ids_across_rotation() {
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error": {"message": "private quota"}}),
            )],
        ),
        (
            PROFILE_B_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [], "usageMetadata": {}}),
            )],
        ),
    ]))
    .await;
    let fixture = gateway_fixture_with_oauth_kind(
        &server.upstream,
        &[None, None],
        1,
        None,
        OAuthKind::Antigravity,
    );
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let seen = server.state.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].credential, PROFILE_A_KEY);
    assert_eq!(seen[1].credential, PROFILE_B_KEY);
    let bodies = seen
        .iter()
        .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap())
        .collect::<Vec<_>>();
    for (index, body) in bodies.iter().enumerate() {
        assert_eq!(body["userAgent"], "antigravity");
        assert_eq!(body["requestType"], "agent");
        assert_eq!(body["project"], format!("paid-project-{:02}", index + 1));
        assert!(body.get("user_prompt_id").is_none());
        assert!(body["request"].get("session_id").is_none());
        assert!(body["request"]["sessionId"].as_str().is_some());
        assert!(body["requestId"]
            .as_str()
            .is_some_and(|value| value.starts_with("agent-") && value.len() > 16));
    }
    assert_eq!(bodies[0]["requestId"], bodies[1]["requestId"]);
    assert_eq!(
        bodies[0]["request"]["sessionId"],
        bodies[1]["request"]["sessionId"]
    );
    assert!(seen.iter().all(|request| {
        request.user_agent == "antigravity/hub/2.2.1 darwin/arm64"
            && request.google_api_client == "google-cloud-sdk vscode_cloudshelleditor/0.1"
            && request.client_metadata
                == r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#
    }));
}

#[tokio::test]
async fn flash_preview_uses_private_wire_for_generate_stream_and_count_tokens() {
    let non_stream = MockReply::json(
        StatusCode::OK,
        json!({
            "response": {
                "candidates": [{"content": {"role": "model", "parts": [{"text": "ok"}]}}],
                "usageMetadata": {
                    "promptTokenCount": 2,
                    "candidatesTokenCount": 1,
                    "thoughtsTokenCount": 3
                },
                "modelVersion": "gemini-3-flash"
            }
        }),
    );
    let (stream, _drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"ok\"}]}}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":1,\"thoughtsTokenCount\":3},\"modelVersion\":\"gemini-3-flash\"}}\n\n",
    ))]);
    let count = MockReply::json(StatusCode::OK, json!({"totalTokens": 2}));
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![non_stream, stream, count],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::Antigravity,
        64,
        &["gemini-3-flash-preview"],
    );
    let app = app_state(fixture.gateway.clone(), None);
    let generation = json!({
        "contents": [{"role": "user", "parts": [{"text": "hello"}]}],
        "generationConfig": {"thinkingConfig": {"thinkingLevel": "high"}}
    });
    let response = invoke_uri(
        app.clone(),
        "/v1beta/models/gemini-3-flash-preview:generateContent",
        generation.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await["modelVersion"],
        "gemini-3-flash-preview"
    );

    let response = invoke_uri(
        app.clone(),
        "/v1beta/models/gemini-3-flash-preview:streamGenerateContent?alt=sse",
        generation,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let stream_body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    let stream_text = std::str::from_utf8(&stream_body).unwrap();
    assert!(stream_text.contains("\"modelVersion\":\"gemini-3-flash-preview\""));
    assert!(!stream_text.contains("\"modelVersion\":\"gemini-3-flash\"}"));

    let response = invoke_uri(
        app,
        "/v1beta/models/gemini-3-flash-preview:countTokens",
        json!({
            "generateContentRequest": {
                "model": "models/caller-controlled",
                "contents": [{"role": "user", "parts": [{"text": "hello"}]}]
            }
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["totalTokens"], 2);

    let seen = server.state.seen();
    assert_eq!(seen.len(), 3);
    for request in &seen {
        assert_eq!(request.credential, PROFILE_A_KEY);
        assert_eq!(request.user_agent, "antigravity/hub/2.2.1 darwin/arm64");
        assert!(request.google_api_client.is_empty());
        assert!(request.client_metadata.is_empty());
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let wire = body
            .get("model")
            .and_then(Value::as_str)
            .or_else(|| body.pointer("/request/model").and_then(Value::as_str))
            .unwrap();
        assert_eq!(wire.trim_start_matches("models/"), "gemini-3-flash");
    }
}

#[tokio::test]
async fn flash_preview_pcm_wav_usage_is_reconstructed_before_public_delivery() {
    let (stream, _drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"silent\"}]}}],\"usageMetadata\":{\"promptTokenCount\":55,\"candidatesTokenCount\":1,\"thoughtsTokenCount\":7},\"modelVersion\":\"gemini-3-flash\"}}\n\n",
    ))]);
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            MockReply::json(
                StatusCode::OK,
                json!({
                    "response": {
                        "candidates": [{"content": {"parts": [{"text": "silent"}]}}],
                        "usageMetadata": {
                            "promptTokenCount": 55,
                            "candidatesTokenCount": 1,
                            "thoughtsTokenCount": 7
                        },
                        "modelVersion": "gemini-3-flash"
                    }
                }),
            ),
            stream,
        ],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::Antigravity,
        128,
        &["gemini-3-flash-preview"],
    );
    let audio = pcm16_wav_base64(8_000, 2_000, 1);
    let app = app_state(fixture.gateway.clone(), None);
    let response = invoke_uri(
        app.clone(),
        "/v1beta/models/gemini-3-flash-preview:generateContent",
        json!({
            "contents": [{"parts": [
                {"inlineData": {"mimeType": "audio/wav", "data": audio.clone()}},
                {"text": "Is this silent?"}
            ]}],
            "generationConfig": {"maxOutputTokens": 64}
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = response_json(response).await;
    assert_eq!(
        response["usageMetadata"]["promptTokensDetails"],
        json!([{"modality": "AUDIO", "tokenCount": 8}])
    );
    assert_eq!(response["modelVersion"], "gemini-3-flash-preview");

    let stream_response = invoke_uri(
        app,
        "/v1beta/models/gemini-3-flash-preview:streamGenerateContent?alt=sse",
        json!({
            "contents": [{"parts": [
                {"inlineData": {"mimeType": "audio/wav", "data": audio.clone()}},
                {"text": "Is this silent?"}
            ]}],
            "generationConfig": {"maxOutputTokens": 64}
        }),
    )
    .await;
    assert_eq!(stream_response.status(), StatusCode::OK);
    let stream_body = to_bytes(stream_response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    let stream_text = std::str::from_utf8(&stream_body).unwrap();
    assert!(
        stream_text.contains("\"promptTokensDetails\":[{\"modality\":\"AUDIO\",\"tokenCount\":8}]")
    );
    assert!(stream_text.contains("\"modelVersion\":\"gemini-3-flash-preview\""));

    let seen = server.state.seen();
    assert_eq!(seen.len(), 2);
    for request in seen {
        let wire: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(wire["model"], "gemini-3-flash");
        assert_eq!(
            wire["request"]["contents"][0]["parts"][0]["inlineData"]["data"],
            audio
        );
    }
}

#[tokio::test]
async fn flash_preview_ambiguous_audio_cache_usage_is_not_delivered() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({
                "response": {
                    "candidates": [{"content": {"parts": [{"text": "silent"}]}}],
                    "usageMetadata": {
                        "promptTokenCount": 55,
                        "cachedContentTokenCount": 20,
                        "candidatesTokenCount": 1
                    },
                    "modelVersion": "gemini-3-flash"
                }
            }),
        )],
    )]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::Antigravity,
        128,
        &["gemini-3-flash-preview"],
    );
    let response = invoke_uri(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-3-flash-preview:generateContent",
        json!({
            "contents": [{"parts": [{
                "inlineData": {
                    "mimeType": "audio/wav",
                    "data": pcm16_wav_base64(8_000, 2_000, 1)
                }
            }]}]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(server.state.seen().len(), 1);
}

#[tokio::test]
async fn flash_preview_unprovable_audio_is_rejected_before_upstream_and_reserve() {
    let server = start_mock(MockState::default()).await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::Antigravity,
        128,
        &["gemini-3-flash-preview"],
    );
    let response = invoke_uri(
        app_state(fixture.gateway.clone(), None),
        "/v1beta/models/gemini-3-flash-preview:generateContent",
        json!({
            "contents": [{"parts": [{
                "inlineData": {"mimeType": "audio/mp3", "data": "AA=="}
            }]}]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(server.state.seen().is_empty());
}

#[tokio::test]
async fn minimal_public_stream_request_is_adapted_to_antigravity_wire_contract() {
    let first = Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"one\"}]}}]}}\n\n",
    );
    let usage = Bytes::from_static(
        b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":4}}}\n\n",
    );
    let (reply, _drained) = MockReply::stream(vec![MockChunk::Data(first), MockChunk::Data(usage)]);
    let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
    let fixture = gateway_fixture_with_oauth_kind_and_output_limit(
        &server.upstream,
        &[None],
        1,
        None,
        OAuthKind::Antigravity,
        65_536,
    );

    // This is the original valid public request that the private stream endpoint rejected.
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": [{"parts": [{"text": "Count from 1 to 5 as words, one per line."}]}]}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let downstream = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&downstream).unwrap().contains("data:"));

    let seen = server.state.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].uri, "/v1internal:streamGenerateContent?alt=sse");
    let private: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(private["request"]["contents"][0]["role"], "user");
    assert_eq!(
        private["request"]["generationConfig"]["maxOutputTokens"],
        ANTIGRAVITY_WIRE_OUTPUT_TOKEN_LIMIT
    );
}

#[tokio::test]
async fn antigravity_refresh_uses_go_identity_without_legacy_google_header() {
    let refreshed = "antigravity-refreshed-access-token";
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::UNAUTHORIZED,
                json!({"error": {"message": "expired"}}),
            )],
        ),
        (
            "",
            vec![MockReply::Json {
                status: StatusCode::OK,
                body: json!({"access_token": refreshed, "expires_in": 3600}),
                retry_after: None,
            }],
        ),
        (
            refreshed,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [], "usageMetadata": {}}),
            )],
        ),
    ]))
    .await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_oauth_kind(
        &server.upstream,
        &[None],
        1,
        Some(&token_uri),
        OAuthKind::Antigravity,
    );
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let seen = server.state.seen();
    let refresh = seen
        .iter()
        .find(|request| request.uri == "/token")
        .expect("one token refresh request");
    assert_eq!(refresh.user_agent, "Go-http-client/2.0");
    assert!(refresh.google_api_client.is_empty());
}

#[tokio::test]
async fn antigravity_health_fetches_model_quotas_and_cools_explicit_zero() {
    let quota_document = json!({
        "models": {
            "gemini-integration-model": {
                "displayName": "Gemini Integration Model",
                "quotaInfo": {
                    "remainingFraction": 0.0,
                    "resetTime": "2099-01-01T00:00:00Z"
                }
            }
        },
        "groups": [{
            "displayName": "Gemini Models",
            "buckets": [
                {
                    "bucketId": "gemini-5h",
                    "remainingFraction": 0.75,
                    "resetTime": "2099-01-01T00:00:00Z"
                },
                {
                    "bucketId": "gemini-weekly",
                    "remainingFraction": 0.60,
                    "resetTime": "2099-01-07T00:00:00Z"
                },
                {
                    "bucketId": "3p-5h",
                    "remainingFraction": 0.10,
                    "resetTime": "2099-01-01T00:00:00Z"
                },
                {
                    "bucketId": "3p-weekly",
                    "remainingFraction": 0.20,
                    "resetTime": "2099-01-07T00:00:00Z"
                }
            ]
        }]
    });
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            MockReply::Json {
                status: StatusCode::OK,
                body: json!({"cloudaicompanionProject": "paid-project-01"}),
                retry_after: None,
            },
            MockReply::Json {
                status: StatusCode::OK,
                body: quota_document.clone(),
                retry_after: None,
            },
            MockReply::Json {
                status: StatusCode::OK,
                body: quota_document,
                retry_after: None,
            },
        ],
    )]))
    .await;
    let fixture =
        gateway_fixture_with_oauth_kind(&server.upstream, &[None], 1, None, OAuthKind::Antigravity);
    fixture.gateway.probe_health().await;

    let seen = server.state.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].uri, "/v1internal:loadCodeAssist");
    assert_eq!(
        seen.iter()
            .filter(|request| request.uri == "/v1internal:fetchAvailableModels")
            .count(),
        1
    );
    assert_eq!(
        seen.iter()
            .filter(|request| request.uri == "/v1internal:retrieveUserQuotaSummary")
            .count(),
        1
    );
    let load_body: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(load_body, json!({"metadata": {"ideType": "ANTIGRAVITY"}}));
    for request in &seen[1..] {
        let quota_body: Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(quota_body, json!({"project": "paid-project-01"}));
    }
    assert!(seen.iter().all(|request| {
        request.user_agent == "antigravity/hub/2.2.1 darwin/arm64"
            && request.google_api_client == "google-cloud-sdk vscode_cloudshelleditor/0.1"
            && request.client_metadata
                == r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#
    }));

    let status = fixture.gateway.operational_status().await;
    assert_eq!(status.models[0].available, 0);
    assert_eq!(status.profiles[0].quotas.len(), 1);
    let quota = &status.profiles[0].quotas[0];
    assert_eq!(quota.model_id, "gemini-integration-model");
    assert_eq!(quota.remaining_fraction, Some(0.0));
    assert_eq!(quota.reset_time.as_deref(), Some("2099-01-01T00:00:00Z"));
    assert_eq!(quota.token_type.as_deref(), Some("antigravity_model"));
    assert_eq!(status.profiles[0].capacities.len(), 2);
    assert_eq!(status.profiles[0].capacities[0].bucket_id, "gemini-5h");
    assert_eq!(status.profiles[0].capacities[1].bucket_id, "gemini-weekly");
    assert!(status.profiles[0]
        .capacities
        .iter()
        .all(|capacity| capacity.cap_usd.is_none()));
}

#[tokio::test]
async fn probe_429_applies_existing_global_cooling_before_diagnostic_drain() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::Json {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: json!({
                "error": {
                    "code": 429,
                    "status": "RESOURCE_EXHAUSTED",
                    "message": "private backend temporarily unavailable"
                }
            }),
            retry_after: Some("2"),
        }],
    )]))
    .await;
    let fixture =
        gateway_fixture_with_oauth_kind(&server.upstream, &[None], 1, None, OAuthKind::Antigravity);
    fixture.gateway.probe_health().await;

    let status = fixture.gateway.operational_status().await;
    assert!(status.profiles[0].cooling_until > pool::now());
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if fixture.gateway.active_background_tasks() == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("detached diagnostic drain must finish");
}

#[tokio::test]
async fn affinity_keeps_a_growing_conversation_on_the_same_subscription() {
    let success = MockReply::json(
        StatusCode::OK,
        json!({"candidates": [], "usageMetadata": {}}),
    );
    let server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![success.clone(), success.clone()]),
        (PROFILE_B_KEY, vec![success]),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let app = app_state(fixture.gateway.clone(), None);
    let first = json!({
        "contents": [{"role": "user", "parts": [{"text": "turn one"}]}]
    });
    let second = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "turn one"}]},
            {"role": "model", "parts": [{"text": "answer"}]},
            {"role": "user", "parts": [{"text": "turn two"}]}
        ]
    });
    assert_eq!(
        invoke(app.clone(), first, false).await.status(),
        StatusCode::OK
    );
    assert_eq!(invoke(app, second, false).await.status(), StatusCode::OK);
    let seen = server.state.seen();
    let credentials = seen
        .iter()
        .map(|request| request.credential.as_str())
        .collect::<Vec<_>>();
    assert_eq!(credentials, [PROFILE_A_KEY, PROFILE_A_KEY]);
    let wire_bodies = seen
        .iter()
        .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap())
        .collect::<Vec<_>>();
    let sessions = wire_bodies
        .iter()
        .map(|body| body["request"]["session_id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let prompts = wire_bodies
        .iter()
        .map(|body| body["user_prompt_id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(sessions[0], sessions[1]);
    assert_eq!(sessions[0].len(), 36);
    assert_eq!(prompts[0], format!("{}########1", sessions[0]));
    assert_eq!(prompts[1], format!("{}########2", sessions[1]));
    assert!(!sessions[0].contains("turn one"));
}

#[tokio::test]
async fn affinity_remains_sticky_under_arbitrary_parallel_load() {
    let success = MockReply::json(
        StatusCode::OK,
        json!({"candidates": [], "usageMetadata": {}}),
    );
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![success.clone(), success.clone(), success.clone()],
        ),
        (PROFILE_B_KEY, vec![]),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let app = app_state(fixture.gateway.clone(), None);
    let affinity = app.affinity.clone();
    let body = json!({
        "contents": [{"role": "user", "parts": [{"text": "sticky turn"}]}]
    });

    assert_eq!(
        invoke_with_identity(
            app.clone(),
            body.clone(),
            false,
            CUSTOMER_KEY,
            Some("sticky-session"),
        )
        .await
        .status(),
        StatusCode::OK
    );

    // Existing work on the bound home is only an observability/balancing signal. A resolved
    // conversation remains on its home and starts immediately, with no local cap or wait.
    let mut occupied = Vec::new();
    for _ in 0..1_000 {
        let lease = fixture
            .gateway
            .select(
                "gemini-integration-model",
                &HashSet::new(),
                Some("profile_a"),
                true,
            )
            .unwrap();
        assert_eq!(lease.profile().id(), "profile_a");
        occupied.push(lease);
    }
    assert_eq!(
        invoke_with_identity(
            app.clone(),
            body.clone(),
            false,
            CUSTOMER_KEY,
            Some("sticky-session"),
        )
        .await
        .status(),
        StatusCode::OK
    );
    drop(occupied);
    assert_eq!(
        invoke_with_identity(app, body, false, CUSTOMER_KEY, Some("sticky-session"),)
            .await
            .status(),
        StatusCode::OK
    );

    let credentials = server
        .state
        .seen()
        .into_iter()
        .map(|request| request.credential)
        .collect::<Vec<_>>();
    assert_eq!(credentials, [PROFILE_A_KEY, PROFILE_A_KEY, PROFILE_A_KEY]);
    assert_eq!(affinity.stats().rebinds, 0);
}

#[tokio::test]
async fn shared_cache_root_warms_two_subscriptions_then_prefers_a_warm_copy() {
    let success = MockReply::json(
        StatusCode::OK,
        json!({"candidates": [], "usageMetadata": {}}),
    );
    let server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![success.clone(), success.clone()]),
        (PROFILE_B_KEY, vec![success]),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let app = app_state(fixture.gateway.clone(), None);
    let affinity = app.affinity.clone();
    let shared_instruction = "shared-system-root-".repeat(300);
    let body = json!({
        "systemInstruction": {"parts": [{"text": shared_instruction}]},
        "contents": [{"role": "user", "parts": [{"text": "same first turn"}]}]
    });

    for session in ["independent-one", "independent-two", "independent-three"] {
        assert_eq!(
            invoke_with_identity(
                app.clone(),
                body.clone(),
                false,
                CUSTOMER_KEY,
                Some(session),
            )
            .await
            .status(),
            StatusCode::OK
        );
    }

    let credentials = server
        .state
        .seen()
        .into_iter()
        .map(|request| request.credential)
        .collect::<Vec<_>>();
    assert_eq!(credentials, [PROFILE_A_KEY, PROFILE_B_KEY, PROFILE_A_KEY]);
    let stats = affinity.stats();
    assert_eq!(stats.cache_root_cold_placements, 2);
    assert_eq!(stats.cache_root_warm_placements, 1);
    assert_eq!(stats.cache_root_hits, 2);
}

#[tokio::test]
async fn upstream_session_is_stable_but_isolated_by_explicit_session_and_tenant() {
    const OTHER_ACCOUNT: &str = "gemini-integration-account-other";
    const OTHER_KEY: &str = "sk-gemini-customer-other";
    let success = MockReply::json(
        StatusCode::OK,
        json!({
            "candidates": [],
            "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 1}
        }),
    );
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![success.clone(), success.clone(), success.clone(), success],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
    billing
        .create_account(OTHER_ACCOUNT, None, 10_000)
        .await
        .unwrap();
    billing
        .topup(OTHER_ACCOUNT, 1_000_000_000, Some("seed-other"))
        .await
        .unwrap();
    billing
        .issue_key(OTHER_KEY, OTHER_ACCOUNT, None, None, None)
        .await
        .unwrap();
    let body = json!({
        "contents": [{"role": "user", "parts": [{"text": "raw prompt secret"}]}]
    });
    for (key, session) in [
        (CUSTOMER_KEY, "raw-client-session-a"),
        (CUSTOMER_KEY, "raw-client-session-a"),
        (CUSTOMER_KEY, "raw-client-session-b"),
        (OTHER_KEY, "raw-client-session-a"),
    ] {
        let response =
            invoke_with_identity(app.clone(), body.clone(), false, key, Some(session)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let seen = server.state.seen();
    let sessions = seen
        .iter()
        .map(|request| {
            let value: Value = serde_json::from_slice(&request.body).unwrap();
            value["request"]["session_id"].as_str().unwrap().to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(sessions[0], sessions[1]);
    assert_ne!(sessions[0], sessions[2]);
    assert_ne!(sessions[0], sessions[3]);
    assert!(sessions.iter().all(|session| session.len() == 36));
    for request in &seen {
        let wire = String::from_utf8_lossy(&request.body);
        assert!(!wire.contains("raw-client-session"));
        assert!(!wire.contains(CUSTOMER_KEY));
        assert!(!wire.contains(OTHER_KEY));
    }
    billing.flush().await.unwrap();
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[tokio::test]
async fn count_tokens_uses_the_private_shape_and_returns_only_native_json() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({
                "totalTokens": 17,
                "privateProject": "paid-project-01",
                "traceId": "private-count-trace"
            }),
        )],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:countTokens")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(
            br#"{"contents":[{"role":"user","parts":[{"text":"hello"}]}]}"#.as_slice(),
        ))
        .unwrap();
    let response = api_inner(
        app_state(fixture.gateway.clone(), None),
        "198.51.100.10:12345".parse().unwrap(),
        request,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, json!({"totalTokens": 17}));
    let seen = server.state.seen();
    assert_eq!(seen[0].uri, "/v1internal:countTokens");
    let private: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(
        private["request"]["model"],
        "models/gemini-integration-model"
    );
    assert!(private.get("project").is_none());
    assert!(private.get("user_prompt_id").is_none());
}

#[tokio::test]
async fn count_tokens_honors_the_official_nested_generate_content_request() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(StatusCode::OK, json!({"totalTokens": 23}))],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:countTokens")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(
            json!({
                "generate_content_request": {
                    "model": "models/caller-controlled-model",
                    "contents": [{"role":"user","parts":[{"text":"hello"}]}],
                    "system_instruction": {"parts":[{"text":"be concise"}]},
                    "tools": [{"function_declarations": [{"name":"lookup"}]}]
                }
            })
            .to_string(),
        ))
        .unwrap();
    let response = api_inner(
        app_state(fixture.gateway.clone(), None),
        "198.51.100.10:12345".parse().unwrap(),
        request,
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, json!({"totalTokens": 23}));
    let private: Value = serde_json::from_slice(&server.state.seen()[0].body).unwrap();
    assert_eq!(
        private["request"]["model"],
        "models/gemini-integration-model"
    );
    assert_eq!(
        private["request"]["systemInstruction"]["parts"][0]["text"],
        "be concise"
    );
    assert!(private["request"]["tools"][0]
        .get("functionDeclarations")
        .is_some());
    assert!(private["request"].get("generateContentRequest").is_none());
}

#[test]
fn count_tokens_rejects_ambiguous_input_and_unsupported_semantic_controls() {
    assert!(validate_native_request(
        Operation::CountTokens,
        &json!({
            "contents": [],
            "generateContentRequest": {"contents": []}
        }),
        &catalog_model("gemini-2.5-flash"),
        false
    )
    .is_err());
    assert!(validate_native_request(
        Operation::CountTokens,
        &json!({"generateContentRequest": []}),
        &catalog_model("gemini-2.5-flash"),
        false
    )
    .is_err());
    for body in [
        json!({"contents": [], "serviceTier": "priority"}),
        json!({"contents": [], "store": false}),
        json!({"generateContentRequest": {"contents": [], "store": true}}),
    ] {
        assert!(validate_native_request(
            Operation::CountTokens,
            &body,
            &catalog_model("gemini-2.5-flash"),
            false,
        )
        .is_err());
    }
    assert!(validate_native_request(
        Operation::Generate,
        &json!({"contents": [], "serviceTier": "standard"}),
        &catalog_model("gemini-2.5-flash"),
        false
    )
    .is_err());
}

#[tokio::test]
async fn malformed_private_success_is_never_exposed_and_rotates() {
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::Json {
                status: StatusCode::OK,
                body: json!(["cloudcode-pa", "owner@example.invalid", "secret-token"]),
                retry_after: None,
            }],
        ),
        (
            PROFILE_B_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [], "usageMetadata": {}}),
            )],
        ),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let public = response_json(response).await.to_string();
    for forbidden in ["cloudcode-pa", "owner@example.invalid", "secret-token"] {
        assert!(!public.contains(forbidden));
    }
    assert_eq!(server.state.seen().len(), 2);
}

#[tokio::test]
async fn low_balance_caps_max_output_tokens_before_the_google_request() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({
                "candidates": [],
                "usageMetadata": {
                    "promptTokenCount": 25,
                    "candidatesTokenCount": 7
                }
            }),
        )],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    // Compact request length 15 + fixture overhead 10 + seven output tokens at 1 nano each.
    let (app, billing, db_path) = billed_app_with_balance(fixture.gateway.clone(), 32).await;
    let response = invoke(app.clone(), json!({"contents": []}), false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response_json(response).await;
    let seen = server.state.seen();
    assert_eq!(seen.len(), 1);
    let upstream_body: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(
        upstream_body["request"]["generationConfig"]["maxOutputTokens"],
        7
    );
    assert_eq!(upstream_body["project"], "paid-project-01");
    assert!(upstream_body.get("user_prompt_id").is_some());
    billing.flush().await.unwrap();
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[tokio::test]
async fn deterministic_client_400_is_returned_without_pool_rotation() {
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::BAD_REQUEST,
                json!({
                    "error": {
                        "status": "INVALID_ARGUMENT",
                        "message": "cloudcode-pa Code Assist paid-project-01 owner@example.invalid refresh-token"
                    }
                }),
            )],
        ),
        (
            PROFILE_B_KEY,
            vec![MockReply::json(StatusCode::OK, json!({"unexpected": true}))],
        ),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.headers().get("retry-after").is_none());
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json; charset=UTF-8")
    );
    let public_value = response_json(response).await;
    assert_eq!(public_value["error"]["code"], 400);
    assert_eq!(public_value["error"]["status"], "INVALID_ARGUMENT");
    assert_eq!(
        public_value["error"]["message"],
        "The model service rejected this request."
    );
    let public = public_value.to_string();
    for secret in [
        "cloudcode-pa",
        "Code Assist",
        "paid-project-01",
        "owner@example.invalid",
        "refresh-token",
    ] {
        assert!(!public.contains(secret), "leaked {secret}: {public}");
    }
    assert_eq!(server.state.seen().len(), 1);
    assert_eq!(server.state.seen()[0].credential, PROFILE_A_KEY);
}

#[tokio::test]
async fn auth_failure_cools_only_the_failed_project_and_rotates_without_losing_capacity() {
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::FORBIDDEN,
                json!({"error": {"status": "UNAUTHENTICATED"}}),
            )],
        ),
        (
            PROFILE_B_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [], "usageMetadata": {}}),
            )],
        ),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let status = fixture.gateway.operational_status().await;
    let failed = status
        .profiles
        .iter()
        .find(|profile| profile.id == "profile_a")
        .unwrap();
    let healthy = status
        .profiles
        .iter()
        .find(|profile| profile.id == "profile_b")
        .unwrap();
    // Rotation away from the rejected project is the point; declaring its credential dead is not.
    // A 401/403 on the generation surface is environment, and only a refresh answering
    // `invalid_grant` may take a paid subscription out of the authenticated set.
    assert!(failed.cooling_until > pool::now());
    assert!(failed.authenticated);
    assert!(healthy.authenticated);
    assert_eq!(status.authenticated, status.profiles.len());
}

#[tokio::test]
async fn exhausted_auth_and_transport_faults_return_one_native_503() {
    let auth = MockReply::json(
        StatusCode::FORBIDDEN,
        json!({"error": {"status": "UNAUTHENTICATED"}}),
    );
    let auth_server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![auth.clone()]),
        (PROFILE_B_KEY, vec![auth]),
    ]))
    .await;
    let auth_fixture = gateway_fixture(&auth_server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(auth_fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response_json(response).await;
    assert_eq!(body["error"]["code"], 503);
    assert_eq!(body["error"]["status"], "UNAVAILABLE");
    assert_eq!(auth_server.state.seen().len(), 2);

    let transport = MockReply::json(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error": {"status": "UNAVAILABLE"}}),
    );
    let transport_server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![transport.clone()]),
        (PROFILE_B_KEY, vec![transport]),
    ]))
    .await;
    let transport_fixture = gateway_fixture(&transport_server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(transport_fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response_json(response).await;
    assert_eq!(body["error"]["code"], 503);
    assert_eq!(body["error"]["status"], "UNAVAILABLE");
    assert_eq!(transport_server.state.seen().len(), 2);
}

#[tokio::test]
async fn transport_failure_and_backend_5xx_each_rotate_within_the_transport_budget() {
    // Profile A cannot reach its configured proxy; profile B reaches the mock directly.
    let network_server = start_mock(MockState::with_replies([(
        PROFILE_B_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({"candidates": [], "usageMetadata": {}}),
        )],
    )]))
    .await;
    let network_fixture = gateway_fixture(
        &network_server.upstream,
        &[Some("http://127.0.0.1:9"), None],
        1,
    );
    let response = invoke(
        app_state(network_fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(network_server.state.seen()[0].credential, PROFILE_B_KEY);

    let backend_server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![
                MockReply::json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error": {"status": "UNAVAILABLE"}}),
                ),
                MockReply::json(
                    StatusCode::OK,
                    json!({"candidates": [], "usageMetadata": {}}),
                ),
            ],
        ),
        (
            PROFILE_B_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [], "usageMetadata": {}}),
            )],
        ),
    ]))
    .await;
    let backend_fixture = gateway_fixture_with_models(
        &backend_server.upstream,
        &[None, None],
        1,
        None,
        OAuthKind::LegacyGeminiCli,
        64,
        &["gemini-integration-model", "gemini-other-model"],
    );
    let app = app_state(backend_fixture.gateway.clone(), None);
    let response = invoke(app.clone(), json!({"contents": []}), false).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(backend_server.state.seen().len(), 2);
    assert_eq!(Metrics::get(&app.metrics.gemini_backend_failures), 1);
    assert_eq!(Metrics::get(&app.metrics.gemini_transport_failures), 0);

    // The backend fault belongs only to gemini-integration-model on profile A. Its other model
    // remains immediately routable and the round-robin cursor selects that same profile again.
    let response = invoke_uri(
        app.clone(),
        "/v1beta/models/gemini-other-model:generateContent",
        json!({"contents": []}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let seen = backend_server.state.seen();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[2].credential, PROFILE_A_KEY);
    let status = backend_fixture.gateway.operational_status().await;
    let profile_a = status
        .profiles
        .iter()
        .find(|profile| profile.id == "profile_a")
        .unwrap();
    let failed = profile_a
        .model_cooling
        .iter()
        .find(|health| health.model_id == "gemini-integration-model")
        .unwrap();
    let healthy = profile_a
        .model_cooling
        .iter()
        .find(|health| health.model_id == "gemini-other-model")
        .unwrap();
    assert_eq!(failed.failure_streak, 1);
    assert_eq!(failed.last_failure_class.as_deref(), Some("backend"));
    assert_eq!(healthy.failure_streak, 0);
    assert!(healthy.last_success_at > 0);
}

#[tokio::test]
async fn retry_info_cools_the_exact_project_and_all_quota_returns_one_native_429() {
    let quota = MockReply::Json {
        status: StatusCode::TOO_MANY_REQUESTS,
        body: json!({
            "error": {
                "status": "RESOURCE_EXHAUSTED",
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.RetryInfo",
                    "retryDelay": "2.25s"
                }]
            }
        }),
        retry_after: None,
    };
    let server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![quota.clone()]),
        (PROFILE_B_KEY, vec![quota]),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key("retry-after"));
    let body = response_json(response).await;
    assert_eq!(body["error"]["code"], 429);
    assert_eq!(body["error"]["status"], "RESOURCE_EXHAUSTED");
    assert_eq!(server.state.seen().len(), 2);
    let now = pool::now();
    let status = fixture.gateway.operational_status().await;
    assert!(status
        .profiles
        .iter()
        .all(|profile| profile.model_cooling.iter().any(|cooling| {
            cooling.model_id == "gemini-integration-model" && cooling.cooling_until >= now + 2
        })));
}

#[tokio::test]
async fn unhinted_429_with_positive_quota_catalog_cools_briefly_not_for_the_default_window() {
    // A generation 429 with no RetryInfo/Retry-After while the profile's own fresh quota catalogue
    // still reports a positive remainder is an RPM/concurrency stall, not exhaustion. The model
    // must be parked only for the short RPM cool, never for the full default exhaustion window,
    // so one momentary throttle cannot freeze the model across the fleet for a minute.
    let quota_document = json!({
        "models": {
            "gemini-integration-model": {
                "displayName": "Gemini Integration Model",
                "quotaInfo": {
                    "remainingFraction": 0.99,
                    "resetTime": "2099-01-01T00:00:00Z"
                }
            }
        },
        "groups": [{
            "displayName": "Gemini Models",
            "buckets": [{
                "bucketId": "gemini-5h",
                "remainingFraction": 0.99,
                "resetTime": "2099-01-01T00:00:00Z"
            }]
        }]
    });
    let stall = MockReply::Json {
        status: StatusCode::TOO_MANY_REQUESTS,
        body: json!({
            "error": {"code": 429, "status": "RESOURCE_EXHAUSTED"}
        }),
        retry_after: None,
    };
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![
            // probe_health: loadCodeAssist, fetchAvailableModels, retrieveUserQuotaSummary.
            MockReply::Json {
                status: StatusCode::OK,
                body: json!({"cloudaicompanionProject": "paid-project-01"}),
                retry_after: None,
            },
            MockReply::Json {
                status: StatusCode::OK,
                body: quota_document.clone(),
                retry_after: None,
            },
            MockReply::Json {
                status: StatusCode::OK,
                body: quota_document,
                retry_after: None,
            },
            // generation attempt: RPM stall.
            stall,
        ],
    )]))
    .await;
    let fixture =
        gateway_fixture_with_oauth_kind(&server.upstream, &[None], 1, None, OAuthKind::Antigravity);
    fixture.gateway.probe_health().await;
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let now = pool::now();
    let status = fixture.gateway.operational_status().await;
    let cooling = status.profiles[0]
        .model_cooling
        .iter()
        .find(|cooling| cooling.model_id == "gemini-integration-model")
        .unwrap();
    // Short RPM cool: present but far below the 60s default exhaustion window.
    assert!(cooling.cooling_until >= now + 1, "cooling should be set");
    assert!(
        cooling.cooling_until < now + 60,
        "cooling_until={} must not reach the default exhaustion window",
        cooling.cooling_until
    );
}

#[tokio::test]
async fn unhinted_429_without_fresh_positive_catalog_keeps_the_default_exhaustion_cool() {
    // Fail-closed guard: with no positive fresh catalogue evidence the unhinted 429 must keep the
    // long default exhaustion cool. A missing catalogue can never be read as an RPM stall.
    let stall = MockReply::Json {
        status: StatusCode::TOO_MANY_REQUESTS,
        body: json!({
            "error": {"code": 429, "status": "RESOURCE_EXHAUSTED"}
        }),
        retry_after: None,
    };
    let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![stall])])).await;
    let fixture =
        gateway_fixture_with_oauth_kind(&server.upstream, &[None], 1, None, OAuthKind::Antigravity);
    // No probe_health(): the quota catalogue is missing, so quota_reports_remaining is false.
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let now = pool::now();
    let status = fixture.gateway.operational_status().await;
    let cooling = status.profiles[0]
        .model_cooling
        .iter()
        .find(|cooling| cooling.model_id == "gemini-integration-model")
        .unwrap();
    // Default exhaustion cool (60s in this fixture) is preserved.
    assert!(
        cooling.cooling_until >= now + 59,
        "cooling_until={} must keep the default exhaustion window",
        cooling.cooling_until
    );
}

#[tokio::test]
async fn quota_error_before_first_stream_byte_rotates_without_false_health() {
    let (quota, _quota_drained) = MockReply::stream(vec![MockChunk::Data(
        Bytes::from_static(
            b"data: {\"error\":{\"code\":429,\"status\":\"RESOURCE_EXHAUSTED\",\"message\":\"private\",\"details\":[{\"@type\":\"type.googleapis.com/google.rpc.RetryInfo\",\"retryDelay\":\"2.25s\"}]}}\n\n",
        ),
    )]);
    let (healthy, _healthy_drained) = MockReply::stream(vec![MockChunk::Data(
        Bytes::from_static(
            b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]}}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":1}}}\n\n",
        ),
    )]);
    let server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![quota]),
        (PROFILE_B_KEY, vec![healthy]),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&body).unwrap().contains("ok"));
    assert_eq!(server.state.seen().len(), 2);
    let now = pool::now();
    let status = fixture.gateway.operational_status().await;
    assert!(status.profiles[0].model_cooling.iter().any(|cooling| {
        cooling.model_id == "gemini-integration-model" && cooling.cooling_until >= now + 2
    }));
}

#[tokio::test]
async fn sse_is_forwarded_across_upstream_chunk_boundaries_without_retry_after_first_byte() {
    let first = Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"one\"}]}}]},\"traceId\":\"stream-trace\",\"remainingCredits\":[{\"creditAmount\":\"91\"}]}\n\n",
    );
    let final_usage = Bytes::from_static(
        b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":4}},\"consumedCredits\":[{\"creditAmount\":\"9\"}]}\n\n",
    );
    let split = first.len() / 2;
    let (reply, drained) = MockReply::stream(vec![
        MockChunk::Data(first.slice(..split)),
        MockChunk::Data(first.slice(split..)),
        MockChunk::Data(final_usage.clone()),
    ]);
    let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(!text.contains("stream-trace"));
    assert!(text.contains("promptTokenCount"));
    assert!(!text.contains("remainingCredits"));
    assert!(!text.contains("consumedCredits"));
    assert!(!text.contains("\"response\""));
    assert!(drained.load(Ordering::Acquire));
    assert_eq!(
        server.state.seen()[0].uri,
        "/v1internal:streamGenerateContent?alt=sse"
    );

    let (broken, _) = MockReply::stream(vec![
        MockChunk::Data(Bytes::from_static(
            b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"first\"}]}}]}}\n\n",
        )),
        MockChunk::Error,
    ]);
    let broken_server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![broken]),
        (
            PROFILE_B_KEY,
            vec![MockReply::json(StatusCode::OK, json!({"mustNot": "retry"}))],
        ),
    ]))
    .await;
    let broken_fixture = gateway_fixture(&broken_server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(broken_fixture.gateway.clone(), None),
        json!({"contents": []}),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&body).unwrap().contains("first"));
    assert_eq!(broken_server.state.seen().len(), 1);
    assert_eq!(broken_server.state.seen()[0].credential, PROFILE_A_KEY);
}

#[tokio::test]
async fn downstream_disconnect_still_drains_final_usage_and_settles_google_ledger() {
    let first = Bytes::from_static(b"data: {\"response\":{\"candidates\":[{\"content\":{}}]}}\n\n");
    let final_usage = Bytes::from_static(
        b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":5}}}\n\n",
    );
    let (reply, drained) =
        MockReply::stream(vec![MockChunk::Data(first), MockChunk::Data(final_usage)]);
    let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
    let response = invoke(app.clone(), json!({"contents": []}), true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut downstream = response.into_body().into_data_stream();
    assert!(downstream.next().await.unwrap().is_ok());
    drop(downstream);

    fixture.gateway.shutdown_until(None).await;
    billing.flush().await.unwrap();
    assert!(drained.load(Ordering::Acquire));
    let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
    assert_eq!(account.reserved_nano, 0);
    assert_eq!(account.spent_nano, 15);
    let usage = billing.usage_by_model(ACCOUNT_ID, 0).await.unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].model, "gemini-integration-model");
    assert_eq!(usage[0].input_tokens, 10);
    assert_eq!(usage[0].output_tokens, 5);
    let providers = billing.spend_by_provider(0).await.unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].provider, registry::PROVIDER_GOOGLE);
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[tokio::test]
async fn metered_non_stream_success_without_usage_is_withheld_and_refunded() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({"candidates": [{"content": {"parts": [{"text": "private success"}]}}]}),
        )],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
    let response = invoke(app.clone(), json!({"contents": []}), false).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let public = response_json(response).await;
    assert_eq!(public["error"]["status"], "UNAVAILABLE");
    assert!(!public.to_string().contains("private success"));

    billing.flush().await.unwrap();
    let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
    assert_eq!(account.reserved_nano, 0);
    assert_eq!(account.spent_nano, 0);
    assert!(billing
        .usage_by_model(ACCOUNT_ID, 0)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(Metrics::get(&app.metrics.gemini_usage_missing), 1);
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[tokio::test]
async fn not_started_refusal_before_first_byte_refunds_the_whole_reservation() {
    // Upstream «успешно» ответил без usageMetadata → плоскость удерживает ответ ДО первого
    // публичного байта и возвращает reserve целиком (admission дропается до mark_delivering).
    // Ответ не-2xx → контракт not_started: заголовок есть, списания нет.
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({"candidates": [{"content": {"parts": [{"text": "withheld"}]}}]}),
        )],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
    let response = invoke(app.clone(), json!({"contents": []}), false).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .unwrap(),
        crate::proxy::EXECUTION_STATE_NOT_STARTED
    );

    billing.flush().await.unwrap();
    let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
    assert_eq!(account.balance_nano, 1_000_000_000);
    assert_eq!(account.reserved_nano, 0);
    assert_eq!(account.spent_nano, 0);
    assert!(billing
        .ledger(ACCOUNT_ID, 10)
        .await
        .unwrap()
        .iter()
        .all(|row| row.kind != "charge"));
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[tokio::test]
async fn successful_delivery_never_carries_not_started_and_charges_usage() {
    // Успешный 200: заголовка нет НИКОГДА; резерв закрывается фактической стоимостью usage.
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({
                "candidates": [{"content": {"parts": [{"text": "delivered"}]}}],
                "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 1}
            }),
        )],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
    let response = invoke(app.clone(), json!({"contents": []}), false).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get(crate::proxy::EXECUTION_STATE_HEADER)
        .is_none());

    billing.flush().await.unwrap();
    let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
    assert_eq!(account.reserved_nano, 0);
    assert!(account.spent_nano > 0, "usage must be charged");
    assert!(billing
        .ledger(ACCOUNT_ID, 10)
        .await
        .unwrap()
        .iter()
        .any(|row| row.kind == "charge"));
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[tokio::test]
async fn a_metered_stream_without_final_usage_bills_nothing_and_invents_no_usage() {
    let (reply, drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
        b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"delivered\"}]}}]}}\n\n",
    ))]);
    let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
    let response = invoke(app.clone(), json!({"contents": []}), true).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&body).unwrap().contains("delivered"));

    fixture.gateway.shutdown_until(None).await;
    billing.flush().await.unwrap();
    assert!(drained.load(Ordering::Acquire));
    let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
    assert_eq!(account.reserved_nano, 0);
    // The answer was delivered but its cost was never reported, and the preflight hold is an
    // admission device rather than a price: billing it charged a double-digit multiple of the turn.
    // The reservation is released instead, and no usage event is invented either way.
    assert_eq!(
        account.spent_nano, 0,
        "an unmeasured turn must not be billed at the admission ceiling"
    );
    assert!(billing
        .usage_by_model(ACCOUNT_ID, 0)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(Metrics::get(&app.metrics.gemini_usage_missing), 1);
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[tokio::test]
async fn shutdown_deadline_aborts_stalled_stream_then_settles_last_known_usage_before_returning() {
    let first = Bytes::from_static(
        b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":7,\"candidatesTokenCount\":2}}}\n\n",
    );
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::Stalled { first }],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
    let response = invoke(app.clone(), json!({"contents": []}), true).await;
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);

    tokio::time::timeout(
        Duration::from_secs(1),
        fixture.gateway.shutdown_until(Some(
            tokio::time::Instant::now() + Duration::from_millis(25),
        )),
    )
    .await
    .expect("shutdown barrier did not abort the stalled Gemini stream");
    billing.flush().await.unwrap();
    let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
    assert_eq!(account.reserved_nano, 0);
    assert_eq!(account.spent_nano, 9);
    drop(app);
    drop(billing);
    let _ = fs::remove_file(db_path);
}

#[test]
fn route_allowlist_is_native_and_closed() {
    assert_eq!(
        parse_route(&Method::GET, "/v1beta/models")
            .unwrap()
            .operation,
        Operation::Models
    );
    assert_eq!(
        parse_route(
            &Method::POST,
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent"
        )
        .unwrap()
        .operation,
        Operation::StreamGenerate
    );
    assert!(parse_route(&Method::POST, "/v1beta/files").is_err());
    assert!(parse_route(&Method::GET, "/v1beta/models/x:generateContent").is_err());
}

#[test]
fn query_credentials_are_rejected_and_streaming_framing_is_native() {
    assert!(parse_stream_query(Some("key=secret"), false).is_err());
    assert!(parse_stream_query(Some("%6bey=secret"), false).is_err());
    assert!(parse_stream_query(Some("API%5fKEY=secret"), false).is_err());
    assert!(parse_stream_query(Some("%zz=broken"), false).is_err());
    assert!(parse_stream_query(Some("foo=bar"), true).is_err());
    // An unknown alt value is rejected; sse and json are the two native framings.
    assert!(parse_stream_query(Some("alt=media"), true).is_err());
    // alt on a non-streaming operation is rejected.
    assert!(parse_stream_query(Some("alt=sse"), false).is_err());
    // Upstream is always alt=sse for streaming; the downstream framing follows the client.
    let (upstream, framing) = parse_stream_query(Some("alt=sse"), true).unwrap();
    assert_eq!(upstream, "alt=sse");
    assert_eq!(framing, StreamFraming::Sse);
    let (upstream, framing) = parse_stream_query(Some("alt=json"), true).unwrap();
    assert_eq!(upstream, "alt=sse");
    assert_eq!(framing, StreamFraming::JsonArray);
    // No alt on a streaming call yields the native JSON array, not SSE.
    let (upstream, framing) = parse_stream_query(None, true).unwrap();
    assert_eq!(upstream, "alt=sse");
    assert_eq!(framing, StreamFraming::JsonArray);
    // Non-streaming carries no upstream query.
    assert_eq!(parse_stream_query(None, false).unwrap().0, "");
}

#[test]
fn independently_billed_or_unknown_server_tools_fail_closed() {
    let model = catalog_model("gemini-2.5-flash-lite");
    for body in [
        json!({"tools": [{"googleMaps": {}}]}),
        json!({"tools": [{"fileSearch": {"fileSearchStoreNames": ["stores/a"]}}]}),
        json!({"tools": [{"futurePaidTool": {}}]}),
        json!({"cachedContent": "cachedContents/customer-selected-resource"}),
    ] {
        assert!(validate_generation_request(&body, &model, false).is_err());
    }
    for body in [
        json!({"tools": [{"googleSearch": {}}]}),
        json!({"tools": [{"urlContext": {}}]}),
        json!({"tools": [{"codeExecution": {}}]}),
        json!({"tools": [{"functionDeclarations": []}]}),
    ] {
        validate_generation_request(&body, &model, false).unwrap();
    }
}

#[test]
fn audio_input_fails_closed_until_usage_reports_authoritative_modality_tokens() {
    let model = catalog_model("gemini-3.1-flash-image");
    let audio = json!({
        "contents": [{
            "parts": [{
                "inlineData": {
                    "mimeType": "audio/wav; codecs=pcm",
                    "data": "UklGRg=="
                }
            }]
        }]
    });
    assert!(validate_native_request(Operation::Generate, &audio, &model, false).is_err());
    assert!(validate_native_request(Operation::CountTokens, &audio, &model, false).is_err());
    assert!(validate_native_request(
        Operation::CountTokens,
        &json!({"generateContentRequest": audio}),
        &model,
        false,
    )
    .is_err());

    let mut snake_case_audio = json!({
        "contents": [{
            "parts": [{
                "inline_data": {"mime_type": "Audio/MP3", "data": "SUQz"}
            }]
        }]
    });
    canonicalize_native_request(&mut snake_case_audio);
    assert!(
        validate_native_request(Operation::Generate, &snake_case_audio, &model, false).is_err()
    );

    assert!(validate_native_request(
        Operation::Generate,
        &json!({
            "systemInstruction": {
                "parts": [{
                    "inlineData": {"mimeType": "audio/wav", "data": "UklGRg=="}
                }]
            },
            "contents": []
        }),
        &model,
        false,
    )
    .is_err());

    validate_native_request(
        Operation::Generate,
        &json!({
            "contents": [{
                "parts": [
                    {"text": "generate a blue circle"},
                    {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}}
                ]
            }]
        }),
        &model,
        false,
    )
    .expect("same-priced inline image input remains available");

    // Published text models accept inline audio: the fleet media matrix admits each one with a
    // mandatory perception marker, and the usage fallback accounts the exact WAV duration.
    let text_model = catalog_model("gemini-2.5-flash-lite");
    validate_native_request(Operation::Generate, &audio, &text_model, false)
        .expect("published text models admit inline audio after the fleet media matrix");
}

#[test]
fn retry_info_and_headers_are_parsed_without_exposing_body() {
    let headers = HeaderMap::new();
    let body = br#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"2.25s"}]}}"#;
    let hint = rate_limit::retry_after_header_delay(Some(&headers)).or_else(|| {
        serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|value| rate_limit::retry_info_delay(&value))
    });
    assert_eq!(hint, Some(3));
}

#[test]
fn incremental_sse_tracker_keeps_last_usage_across_chunk_boundaries() {
    let mut tracker = SseTranslator::new_with_image_usage(
        StreamFraming::Sse,
        "gemini-integration-model",
        0,
        AudioUsageHint::default(),
    );
    tracker
        .push(b"data: {\"response\":{\"usageMetadata\":{\"promptToken")
        .unwrap();
    tracker
        .push(b"Count\":10}}}\n\ndata: {\"response\":{\"candidates\":[{\"groundingMetadata\":{")
        .unwrap();
    tracker
        .push(b"\"webSearchQueries\":[\"one\",\"two\"]}}]}}\n\ndata: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":20,")
        .unwrap();
    tracker.push(b"\"candidatesTokenCount\":5}}}\n\n").unwrap();
    assert_eq!(
        tracker.usage,
        metering::GeminiUsage {
            input_tokens: 20,
            output_tokens: 5,
            search_queries: 2,
            grounded_search_prompts: 1,
            ..metering::GeminiUsage::default()
        }
    );
}

#[test]
fn image_usage_without_private_modality_details_uses_the_official_size_sku() {
    let response = json!({
        "candidates": [{
            "content": {"parts": [{
                "inlineData": {"mimeType": "image/jpeg", "data": "AQID"}
            }]}
        }],
        "usageMetadata": {
            "promptTokenCount": 21,
            "candidatesTokenCount": 1_387
        }
    });
    let usage = settlement_usage_from_response(&response, 1_120).unwrap();
    assert_eq!(
        usage,
        metering::GeminiUsage {
            input_tokens: 21,
            output_tokens: 267,
            image_output_tokens: 1_120,
            ..metering::GeminiUsage::default()
        }
    );

    let no_image = json!({
        "candidates": [{"content": {"parts": [{"text": "refused"}]}}],
        "usageMetadata": {"candidatesTokenCount": 1_387}
    });
    assert_eq!(
        settlement_usage_from_response(&no_image, 1_120)
            .unwrap()
            .output_tokens,
        1_387
    );

    let detailed = json!({
        "candidates": [{
            "content": {"parts": [{
                "inlineData": {"mimeType": "image/jpeg", "data": "AQID"}
            }]}
        }],
        "usageMetadata": {
            "candidatesTokenCount": 901,
            "candidatesTokensDetails": [
                {"modality": "TEXT", "tokenCount": 1},
                {"modality": "IMAGE", "tokenCount": 900}
            ]
        }
    });
    let usage = settlement_usage_from_response(&detailed, 1_120).unwrap();
    assert_eq!(usage.output_tokens, 1);
    assert_eq!(usage.image_output_tokens, 900);
}

#[test]
fn image_stream_fallback_survives_separate_media_and_usage_frames() {
    let mut tracker = SseTranslator::new_with_image_usage(
        StreamFraming::Sse,
        "image-model",
        747,
        AudioUsageHint::default(),
    );
    tracker
        .push(
            b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"inlineData\":{\"mimeType\":\"image/jpeg\",\"data\":\"AQID\"}}]}}]}}\n\n",
        )
        .unwrap();
    tracker
        .push(
            b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":800}}}\n\n",
        )
        .unwrap();
    assert_eq!(tracker.usage.input_tokens, 10);
    assert_eq!(tracker.usage.output_tokens, 53);
    assert_eq!(tracker.usage.image_output_tokens, 747);
}

#[test]
fn synthetic_errors_never_leak_internal_architecture() {
    let errors = [
        ApiError::not_found(),
        ApiError::unavailable("test"),
        ApiError::rate_limited(Some(1)),
        ApiError::from(AdmissionError::Unauthorized),
        ApiError::from(AdmissionError::LowBalance),
    ];
    for error in errors {
        let body = error.into_response();
        let debug = format!("{body:?}").to_ascii_lowercase();
        for forbidden in [
            "profile",
            "project_id",
            "api_key_file",
            "credential pool",
            "upstream",
            "cooling",
            "billing authority",
        ] {
            assert!(!debug.contains(forbidden), "leaked {forbidden}: {debug}");
        }
    }
}

/// A request Google denies on every profile is the request's fault, not the fleet's.
///
/// Production hit exactly this: Google answered `403 PERMISSION_DENIED` to one identical request on
/// all seven profiles, twice, while every OAuth token stayed valid and the profiles kept serving
/// afterwards. Treating it as an auth fault cooled the whole fleet on an exponential streak — so one
/// caller's request degraded Gemini for everyone — and the customer received a retryable
/// `503 UNAVAILABLE`, which is why their client retried the denial in a loop instead of stopping.
#[tokio::test]
async fn request_scoped_permission_denied_returns_googles_verdict_and_spares_the_fleet() {
    let denied = MockReply::json(
        StatusCode::FORBIDDEN,
        json!({"error": {"status": "PERMISSION_DENIED"}}),
    );
    let server = start_mock(MockState::with_replies([
        (PROFILE_A_KEY, vec![denied.clone()]),
        (PROFILE_B_KEY, vec![denied]),
    ]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
    let response = invoke(
        app_state(fixture.gateway.clone(), None),
        json!({"contents": []}),
        false,
    )
    .await;

    // Google's own verdict reaches the caller, so a client can stop instead of retrying forever.
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response_json(response).await;
    assert_eq!(body["error"]["code"], 403);
    assert_eq!(body["error"]["status"], "PERMISSION_DENIED");
    // Every profile was tried once for this request and none was punished for answering.
    assert_eq!(server.state.seen().len(), 2);
    let status = fixture.gateway.operational_status().await;
    assert_eq!(status.authenticated, status.profiles.len());
    for profile in &status.profiles {
        assert!(
            profile.cooling_until <= pool::now(),
            "profile {} was cooled by a request-scoped denial",
            profile.id
        );
    }
}

/// A Files API reference is refused before dispatch, with a reason the caller can act on.
///
/// A `files/…` resource belongs to the customer's own Google project, so the pooled subscription
/// this gateway calls under cannot read it and every profile returns `PERMISSION_DENIED`. That
/// used to reach the customer as a synthetic `503 UNAVAILABLE` with a retry delay, so their SDK
/// retried an input that can never succeed. Refusing locally keeps the fleet out of it entirely
/// and tells them what to send instead.
#[tokio::test]
async fn files_api_reference_is_refused_locally_with_a_machine_reason() {
    let server = start_mock(MockState::with_replies([(
        PROFILE_A_KEY,
        vec![MockReply::json(
            StatusCode::OK,
            json!({"candidates": [], "usageMetadata": {}}),
        )],
    )]))
    .await;
    let fixture = gateway_fixture(&server.upstream, &[None], 1);
    for field in ["fileData", "file_data"] {
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": [{"role": "user", "parts": [
                {"text": "describe"},
                {field: {"mime_type": "image/png", "file_uri": "https://generativelanguage.googleapis.com/v1beta/files/x"}}
            ]}]}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
        let body = response_json(response).await;
        assert_eq!(body["error"]["status"], "INVALID_ARGUMENT", "{field}");
        assert_eq!(
            body["error"]["details"][0]["reason"], "FILE_URI_UNSUPPORTED",
            "{field}"
        );
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("inlineData")),
            "the refusal must name the supported alternative: {body}"
        );
    }
    // Never dispatched: the pool must not spend a profile on an input we already know is refused.
    assert!(server.state.seen().is_empty());
}

fn cool_test_config() -> super::super::config::GeminiConfig {
    super::super::config::GeminiConfig {
        enabled: true,
        upstream: "http://127.0.0.1".to_string(),
        profiles_file: String::new(),
        credential_layout: super::super::config::GeminiCredentialLayout::SealedRoster,
        credential_keys: CredentialKeyring::parse(
            "00:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
        models: Vec::new(),
        batch_5h_headroom_percent: 15,
        connect_timeout_secs: 5,
        read_timeout_secs: 5,
        generation_idle_timeout_secs: 5,
        max_transport_retries: 1,
        auth_quarantine_secs: 900,
        auth_blocked_cool_secs: 15,
        min_probe_interval_secs: 15,
        transport_cool_secs: 5,
        model_failure_cool_secs: 15,
        model_failure_max_cool_secs: 900,
        default_rate_limit_cool_secs: 60,
        rate_limit_rpm_cool_secs: 2,
        rate_limit_unknown_cool_secs: 60,
        cooldown_state_file: String::new(),
        quota_reserve_fraction: 0.05,
        quota_reserve_jitter: 0.01,
        health_probe_interval_secs: 60,
        reserve_overhead_tokens: 10,
        antigravity_version: gemini_credential::ANTIGRAVITY_VERSION.to_string(),
        node_binary: "/usr/bin/node".to_string(),
        node_version: "v24.18.0".to_string(),
        node_sha256: String::new(),
    }
}

fn reason_diagnostic(reason: &str) -> RateLimitDiagnostic {
    let value = serde_json::json!({
        "error": {
            "code": 429,
            "status": "RESOURCE_EXHAUSTED",
            "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": reason}]
        }
    });
    RateLimitDiagnostic::from_value(None, Some(&value))
}

#[test]
fn hinted_real_quota_exhaustion_is_honoured_in_full() {
    let cfg = cool_test_config();
    let diagnostic = reason_diagnostic("QUOTA_EXHAUSTED");
    assert_eq!(
        generation_429_cool_secs(Some(13_437), &diagnostic, true, &cfg),
        13_437
    );
}

#[test]
fn hinted_transient_stall_is_capped_not_honoured_verbatim() {
    let cfg = cool_test_config();
    let diagnostic = reason_diagnostic("SOME_TRANSIENT_STALL");
    // A 1376s hint on a non-exhaustion reason must be capped to the short window.
    assert_eq!(
        generation_429_cool_secs(Some(1_376), &diagnostic, true, &cfg),
        60
    );
}

#[test]
fn hinted_short_hint_below_cap_is_kept() {
    let cfg = cool_test_config();
    let diagnostic = reason_diagnostic("RATE_LIMIT_EXCEEDED");
    assert_eq!(
        generation_429_cool_secs(Some(2), &diagnostic, true, &cfg),
        2
    );
}

#[test]
fn unhinted_with_quota_remaining_cools_briefly() {
    let cfg = cool_test_config();
    let diagnostic = RateLimitDiagnostic::from_value(None, None);
    assert_eq!(generation_429_cool_secs(None, &diagnostic, true, &cfg), 2);
}

#[test]
fn unhinted_without_quota_remaining_uses_default_cool() {
    let cfg = cool_test_config();
    let diagnostic = RateLimitDiagnostic::from_value(None, None);
    assert_eq!(generation_429_cool_secs(None, &diagnostic, false, &cfg), 60);
}

#[tokio::test]
async fn count_tokens_fact_handoff_is_exactly_once_privacy_bounded_and_parser_gated() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let mut billing = AsyncBilling::start(":memory:".to_string(), 1).unwrap();
    billing.replace_request_fact_inbox_for_test(sender);
    let billing = Arc::new(billing);
    let clock = crate::execution::RequestLifecycleClock::default();
    let seed = GeminiRequestFactSeed {
        logical_request_id: "123e4567-e89b-42d3-a456-426614174099".into(),
        client_attribution: crate::execution::ClientAttribution::unknown_for_internal_use(),
        execution: registry::ExecutionAttempt::direct(),
        account_id: ACCOUNT_ID.into(),
        key_id: "key_123e4567e89b42d3a456426614174099".into(),
        admitted_at: pool::now(),
        lifecycle_clock: clock.clone(),
    };
    let intent = UniversalCountTokensIntent {
        requested_model: Some("google/gemini-integration-model".into()),
        classification: classify_gemini_generate_content(&json!({"tools": []})),
    };
    let guard = GeminiCountTokensFactGuard::new(billing, seed, Some(intent));
    let observer = guard.actual_send_observer();
    observer.record().await;
    observer.record().await;
    let mut response = (StatusCode::OK, "native").into_response();
    guard.terminal_response(&mut response);
    let handoff = response
        .extensions_mut()
        .remove::<GeminiCountTokensFactHandoff>()
        .unwrap();
    handoff.finish(StatusCode::INTERNAL_SERVER_ERROR, true);
    let fact = receiver.try_recv().unwrap();
    assert_eq!(fact.provider_plane, "gemini");
    assert_eq!(fact.route_class, "universal");
    assert_eq!(fact.request_class, "count_tokens");
    assert_eq!(
        fact.requested_model.as_deref(),
        Some("google/gemini-integration-model")
    );
    assert_eq!(fact.executable_model, None);
    assert_eq!(fact.terminal.http_status_code, Some(500));
    assert_eq!(fact.terminal.internal_attempt_count, Some(2));
    assert_eq!(
        fact.terminal.provider_terminal_class,
        ProviderTerminalClass::ProtocolError
    );
    assert_eq!(fact.terminal.delivery_state, DeliveryState::NotStarted);
    assert_eq!(clock.first_public_byte_at(), None);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn count_tokens_fact_native_acceptance_and_drop_are_conservative() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let mut billing = AsyncBilling::start(":memory:".to_string(), 1).unwrap();
    billing.replace_request_fact_inbox_for_test(sender);
    let seed = GeminiRequestFactSeed {
        logical_request_id: "123e4567-e89b-42d3-a456-426614174098".into(),
        client_attribution: crate::execution::ClientAttribution::unknown_for_internal_use(),
        execution: registry::ExecutionAttempt::grouped("123e4567-e89b-42d3-a456-426614174097", 2)
            .unwrap(),
        account_id: ACCOUNT_ID.into(),
        key_id: "key_123e4567e89b42d3a456426614174098".into(),
        admitted_at: pool::now(),
        lifecycle_clock: crate::execution::RequestLifecycleClock::default(),
    };
    let mut guard = GeminiCountTokensFactGuard::new(Arc::new(billing), seed, None);
    guard.update_after_native_accept(
        "gemini-integration-model",
        &json!({"contents": [], "tools": [{"googleSearch": {}}]}),
    );
    guard.resolve_executable_model("gemini-integration-model");
    drop(guard);
    let fact = receiver.try_recv().unwrap();
    assert_eq!(fact.route_class, "native");
    assert_eq!(
        fact.requested_model.as_deref(),
        Some("gemini-integration-model")
    );
    assert_eq!(
        fact.executable_model.as_deref(),
        Some("gemini-integration-model")
    );
    assert_eq!(fact.terminal.http_status_code, None);
    assert_eq!(fact.terminal.internal_attempt_count, None);
    assert_eq!(
        fact.terminal.provider_terminal_class,
        ProviderTerminalClass::Unknown
    );
    assert_eq!(fact.terminal.delivery_state, DeliveryState::Unknown);
}

#[tokio::test]
async fn native_count_tokens_emits_one_fact_with_exact_401_resend_attempts() {
    let refreshed = "gemini-profile-a-fact-refreshed-token";
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::UNAUTHORIZED,
                json!({"error": {"message": "refresh me"}}),
            )],
        ),
        (
            "",
            vec![MockReply::Json {
                status: StatusCode::OK,
                body: json!({"access_token": refreshed, "expires_in": 3600}),
                retry_after: None,
            }],
        ),
        (
            refreshed,
            vec![MockReply::json(StatusCode::OK, json!({"totalTokens": 19}))],
        ),
    ]))
    .await;
    let token_uri = format!("{}/token", server.upstream);
    let fixture = gateway_fixture_with_token_uri(&server.upstream, &[None], 2, Some(&token_uri));
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "gemini-count-fact-{}-{unique}.sqlite",
        std::process::id()
    ));
    let mut billing_owned = AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();
    billing_owned
        .create_account(ACCOUNT_ID, None, 10_000)
        .await
        .unwrap();
    billing_owned
        .topup(ACCOUNT_ID, 1_000_000_000, None)
        .await
        .unwrap();
    billing_owned
        .issue_key(CUSTOMER_KEY, ACCOUNT_ID, None, None, None)
        .await
        .unwrap();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    billing_owned.replace_request_fact_inbox_for_test(sender);
    let billing_owned = Arc::new(billing_owned);
    let app = app_state(fixture.gateway.clone(), Some(billing_owned.clone()));
    let mut request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:countTokens")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(
            json!({"contents": [{"role":"user","parts":[{"text":"private prompt"}]}]}).to_string(),
        ))
        .unwrap();
    request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174090".parse().unwrap(),
    );
    let logical = crate::execution::admit_logical_request_id(request.headers_mut()).unwrap();
    request.extensions_mut().insert(logical);
    request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        request,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let fact = receiver.recv().await.unwrap();
    assert_eq!(fact.route_class, "native");
    assert_eq!(fact.request_class, "count_tokens");
    assert_eq!(
        fact.requested_model.as_deref(),
        Some("gemini-integration-model")
    );
    assert_eq!(
        fact.executable_model.as_deref(),
        Some("gemini-integration-model")
    );
    assert_eq!(fact.terminal.http_status_code, Some(200));
    assert_eq!(fact.terminal.internal_attempt_count, Some(2));
    assert_eq!(
        fact.terminal.provider_terminal_class,
        ProviderTerminalClass::Success
    );
    assert_eq!(fact.terminal.delivery_state, DeliveryState::Completed);
    let debug = format!("{fact:?}");
    assert!(!debug.contains("private prompt"));
    assert!(!debug.contains(CUSTOMER_KEY));
    drop(billing_owned);
    let _ = fs::remove_file(path);
}

async fn invoke_batch_request(
    app: AppState,
    method: Method,
    uri: &str,
    key: &str,
    headers: &[(&str, &str)],
    body: impl Into<Body>,
) -> Response {
    let mut builder = axum::extract::Request::builder()
        .method(method)
        .uri(uri)
        .header("x-goog-api-key", key);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    api(
        State(app),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        builder.body(body.into()).unwrap(),
    )
    .await
}

#[test]
fn gemini_batch_public_handlers_postgres_lifecycle_files_and_account_isolation() {
    const LOCK: i64 = 831_572_908_441;
    const ACCOUNT_A: &str = "gemini-batch-http-account-a";
    const ACCOUNT_B: &str = "gemini-batch-http-account-b";
    const KEY_A: &str = "sk-pool-gemini-batch-http-a";
    const KEY_B: &str = "sk-pool-gemini-batch-http-b";
    if std::env::var("CLAUDE_API_GEMINI_BATCH_HTTP_LIFECYCLE").as_deref() != Ok("1") {
        eprintln!(
            "skipping Gemini Batch public handler lifecycle: explicit serial marker is unset"
        );
        return;
    }
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Gemini Batch public handler lifecycle: test URL is unset");
        return;
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut sql = postgres::Client::connect(&url, postgres::NoTls)
        .expect("CLAUDE_API_TEST_DATABASE_URL was supplied but PostgreSQL is unavailable");
    sql.batch_execute("SET statement_timeout=0; SET lock_timeout=0")
        .unwrap();
    sql.query_one("SELECT pg_advisory_lock($1)", &[&LOCK])
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    sql.batch_execute(
        "TRUNCATE gemini_batch_settlement_outbox,gemini_batch_profile_leases,gemini_batch_blobs,\
         gemini_batch_item_files,gemini_batch_items,gemini_batch_jobs,gemini_batch_file_chunks,\
         gemini_batch_files,request_facts,execution_group_winner,settlement_outbox,reservations,\
         capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts \
         RESTART IDENTITY CASCADE",
    )
    .unwrap();
    for (account, key, reference) in [
        (ACCOUNT_A, KEY_A, "gemini-batch-http-seed-a"),
        (ACCOUNT_B, KEY_B, "gemini-batch-http-seed-b"),
    ] {
        pg.account_create(account, None, 10_000).unwrap();
        pg.account_topup(account, 100_000_000_000, Some(reference))
            .unwrap();
        pg.key_issue(key, account, None).unwrap();
    }
    let owner = pg
        .claim_instance(
            &format!("gemini-batch-http-{}-{unique}", std::process::id()),
            600,
        )
        .unwrap();
    drop(pg);

    let billing = Arc::new(
        AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner.clone()),
            1,
            0,
        )
        .unwrap(),
    );
    let batch_authority = GeminiBatchAuthority::start(
        registry::authority::AuthorityConfig::Postgres { url: url.clone() },
        owner,
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let fixture = gateway_fixture("http://127.0.0.1:1", &[None], 0);
        let data_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0x5au8; 32]);
        let keyring = Arc::new(
            GeminiBatchDataKeyring::parse(&format!("test;test:{data_key}")).unwrap(),
        );
        let facade = GeminiBatchPublicFacade::new(
            batch_authority.clone(),
            fixture.gateway.clone(),
            Arc::clone(&keyring),
        );
        let mut app = app_state(fixture.gateway.clone(), Some(Arc::clone(&billing)));
        app.authority = Arc::new(registry::authority::AuthorityConfig::Postgres { url: url.clone() });
        app.gemini_batch = Some(facade);

        let upload = b"{\"key\":\"first\",\"request\":{\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":\"hello\"}]}]}}\n";
        let response = invoke_batch_request(
            app.clone(),
            Method::POST,
            "/upload/v1beta/files",
            KEY_A,
            &[
                ("x-goog-upload-protocol", "resumable"),
                ("x-goog-upload-command", "start"),
                ("x-goog-upload-header-content-length", "84"),
                ("x-goog-upload-file-name", "batch-input.jsonl"),
                ("x-goog-upload-header-content-type", "application/jsonl"),
            ],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let upload_url = response.headers()["x-goog-upload-url"].to_str().unwrap().to_owned();
        assert_eq!(response.headers()["x-goog-upload-status"], "active");
        let response = invoke_batch_request(
            app.clone(),
            Method::POST,
            &upload_url,
            KEY_A,
            &[
                ("content-type", "application/jsonl"),
                ("x-goog-upload-protocol", "resumable"),
                ("x-goog-upload-command", "upload, finalize"),
                ("x-goog-upload-offset", "0"),
            ],
            upload.as_slice(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-goog-upload-status"], "final");
        let uploaded = response_json(response).await;
        let file_name = uploaded["file"]["name"].as_str().unwrap().to_owned();
        let file_id = file_name.strip_prefix("files/").unwrap().to_owned();

        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            &format!("/v1beta/files/{file_id}"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["name"], file_name);
        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            "/v1beta/files",
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["files"][0]["name"], file_name);
        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            &format!("/v1beta/files/{file_id}:download"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), GEMINI_BODY_LIMIT).await.unwrap(),
            upload.as_slice()
        );

        for uri in [
            format!("/v1beta/files/{file_id}"),
            format!("/v1beta/files/{file_id}:download"),
        ] {
            let response = invoke_batch_request(
                app.clone(),
                Method::GET,
                &uri,
                KEY_B,
                &[],
                Body::empty(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            "/v1beta/files",
            KEY_B,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["files"], json!([]));

        let create_body = json!({"batch":{"displayName":"HTTP lifecycle","inputConfig":{"fileName":file_name}}});
        let response = invoke_batch_request(
            app.clone(),
            Method::POST,
            "/v1beta/models/gemini-integration-model:batchGenerateContent",
            KEY_A,
            &[
                ("content-type", "application/json"),
                ("idempotency-key", "gemini-batch-http-lifecycle"),
            ],
            serde_json::to_vec(&create_body).unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let operation = response_json(response).await;
        let batch_name = operation["name"].as_str().unwrap().to_owned();
        let batch_id = batch_name.strip_prefix("batches/").unwrap().to_owned();

        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            &format!("/v1beta/batches/{batch_id}"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["name"], batch_name);
        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            "/v1beta/batches?pageSize=1",
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["operations"][0]["name"], batch_name);

        let seeded_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let exact_response = json!({
            "candidates":[{"content":{"role":"model","parts":[{"text":"hello"}]},"finishReason":"STOP"}],
            "usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2},
            "modelVersion":"gemini-integration-model"
        });
        let metadata = json!({"trace":"client-metadata"});
        let result_blob = keyring
            .encrypt_blob(
                &GeminiBatchBlobIdentity {
                    account_id: ACCOUNT_A,
                    job_id: &batch_id,
                    item_index: 0,
                    kind: "result",
                    schema_version: 1,
                },
                &serde_json::to_vec(&exact_response).unwrap(),
                seeded_now + 42 * 24 * 3600,
            )
            .unwrap();
        let metadata_blob = keyring
            .encrypt_blob(
                &GeminiBatchBlobIdentity {
                    account_id: ACCOUNT_A,
                    job_id: &batch_id,
                    item_index: 0,
                    kind: "metadata",
                    schema_version: 1,
                },
                &serde_json::to_vec(&metadata).unwrap(),
                seeded_now + 42 * 24 * 3600,
            )
            .unwrap();
        let seed_url = url.clone();
        let seed_batch_id = batch_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut seeded = postgres::Client::connect(&seed_url, postgres::NoTls).unwrap();
            for blob in [&result_blob, &metadata_blob] {
                seeded.execute(
                    "INSERT INTO gemini_batch_blobs(job_id,item_index,kind,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,retention_ts,created_ts) VALUES($1,0,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(job_id,item_index,kind) DO UPDATE SET key_id=EXCLUDED.key_id,nonce=EXCLUDED.nonce,ciphertext=EXCLUDED.ciphertext,plaintext_len=EXCLUDED.plaintext_len,plaintext_digest=EXCLUDED.plaintext_digest,retention_ts=EXCLUDED.retention_ts",
                    &[&seed_batch_id,&blob.kind,&blob.key_id,&blob.nonce,&blob.ciphertext,&blob.plaintext_len,&blob.plaintext_digest.as_slice(),&blob.retention_ts,&seeded_now],
                ).unwrap();
            }
            seeded.execute(
                "UPDATE gemini_batch_items SET state='succeeded',terminal_class='success',terminal_ts=$2,settlement_id=$3,client_key='first',worker_instance=NULL,worker_epoch=NULL,lease_until=NULL WHERE job_id=$1 AND item_index=0",
                &[&seed_batch_id,&seeded_now,&format!("seeded-{seed_batch_id}")],
            ).unwrap();
            seeded.execute(
                "UPDATE gemini_batch_jobs SET completed_ts=$2,update_ts=$2,result_expiration_ts=$3 WHERE job_id=$1",
                &[&seed_batch_id,&seeded_now,&(seeded_now + 42 * 24 * 3600)],
            ).unwrap();
        }).await.unwrap();
        let response = invoke_batch_request(
            app.clone(), Method::GET, &format!("/v1beta/batches/{batch_id}"), KEY_A, &[], Body::empty(),
        ).await;
        assert_eq!(response.status(), StatusCode::OK);
        let terminal = response_json(response).await;
        assert_eq!(terminal["response"]["inlinedResponses"][0]["response"], exact_response);
        assert_eq!(
            terminal["response"]["inlinedResponses"][0]["metadata"],
            json!({"trace":"client-metadata","key":"first"})
        );
        assert!(terminal.to_string().find("requestId").is_none());

        let tamper_url = url.clone();
        let tamper_batch_id = batch_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut tamper = postgres::Client::connect(&tamper_url, postgres::NoTls).unwrap();
            tamper.execute(
                "UPDATE gemini_batch_blobs SET ciphertext=set_byte(ciphertext,0,get_byte(ciphertext,0)#1) WHERE job_id=$1 AND item_index=0 AND kind='result'",
                &[&tamper_batch_id],
            ).unwrap();
        }).await.unwrap();
        let response = invoke_batch_request(
            app.clone(), Method::GET, &format!("/v1beta/batches/{batch_id}"), KEY_A, &[], Body::empty(),
        ).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let data_loss = response_json(response).await;
        assert_eq!(data_loss["error"]["status"], "DATA_LOSS");

        let exact_status = json!({"error":{"code":429,"message":"quota exhausted","status":"RESOURCE_EXHAUSTED","details":[{"reason":"QUOTA"}]}});
        let error_blob = keyring
            .encrypt_blob(
                &GeminiBatchBlobIdentity {
                    account_id: ACCOUNT_A,
                    job_id: &batch_id,
                    item_index: 0,
                    kind: "error",
                    schema_version: 1,
                },
                &serde_json::to_vec(&exact_status).unwrap(),
                seeded_now + 42 * 24 * 3600,
            )
            .unwrap();
        let error_url = url.clone();
        let error_batch_id = batch_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut seeded = postgres::Client::connect(&error_url, postgres::NoTls).unwrap();
            seeded.execute(
                "INSERT INTO gemini_batch_blobs(job_id,item_index,kind,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,retention_ts,created_ts) VALUES($1,0,'error',$2,$3,$4,$5,$6,$7,$8)",
                &[&error_batch_id,&error_blob.key_id,&error_blob.nonce,&error_blob.ciphertext,&error_blob.plaintext_len,&error_blob.plaintext_digest.as_slice(),&error_blob.retention_ts,&seeded_now],
            ).unwrap();
            seeded.execute(
                "UPDATE gemini_batch_items SET state='failed',terminal_class='upstream_error' WHERE job_id=$1 AND item_index=0",
                &[&error_batch_id],
            ).unwrap();
        }).await.unwrap();
        let response = invoke_batch_request(
            app.clone(), Method::GET, &format!("/v1beta/batches/{batch_id}"), KEY_A, &[], Body::empty(),
        ).await;
        assert_eq!(response.status(), StatusCode::OK);
        let terminal_error = response_json(response).await;
        assert_eq!(
            terminal_error["response"]["inlinedResponses"][0]["error"],
            exact_status["error"]
        );
        assert_eq!(
            terminal_error["response"]["inlinedResponses"][0]["metadata"]["key"],
            "first"
        );

        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            &format!("/v1beta/batches/{batch_id}"),
            KEY_B,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            "/v1beta/batches",
            KEY_B,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["operations"], json!([]));
        for (method, suffix) in [
            (Method::POST, ":cancel"),
            (Method::DELETE, ""),
        ] {
            let response = invoke_batch_request(
                app.clone(),
                method,
                &format!("/v1beta/batches/{batch_id}{suffix}"),
                KEY_B,
                &[],
                Body::empty(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        let response = invoke_batch_request(
            app.clone(),
            Method::POST,
            &format!("/v1beta/batches/{batch_id}:cancel"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let response = invoke_batch_request(
            app.clone(),
            Method::DELETE,
            &format!("/v1beta/batches/{batch_id}"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let response = invoke_batch_request(
            app.clone(),
            Method::GET,
            &format!("/v1beta/batches/{batch_id}"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = invoke_batch_request(
            app.clone(),
            Method::DELETE,
            &format!("/v1beta/files/{file_id}"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FAILED_DEPENDENCY);
        let response = invoke_batch_request(
            app,
            Method::GET,
            &format!("/v1beta/files/{file_id}"),
            KEY_A,
            &[],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        batch_authority.shutdown().await.unwrap();
    });
    drop(billing);
    assert!(sql
        .query_one("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .unwrap()
        .get::<_, bool>(0));
}

#[test]
fn gemini_count_tokens_facts_persist_universal_and_native_postgres_rows() {
    const LOCK: i64 = 831_572_908_442;
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Gemini count-token fact rows: test URL is unset");
        return;
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut lock = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock.query_one("SELECT pg_advisory_lock($1)", &[&LOCK])
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock.batch_execute(
        "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations,          capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts          RESTART IDENTITY CASCADE",
    )
    .unwrap();
    pg.account_create(ACCOUNT_ID, None, 10_000).unwrap();
    pg.account_topup(ACCOUNT_ID, 1_000, Some("gemini-count-facts"))
        .unwrap();
    pg.key_issue(CUSTOMER_KEY, ACCOUNT_ID, None).unwrap();
    let key_id = pg.key_get(CUSTOMER_KEY).unwrap().unwrap().key_id;
    let owner = pg
        .claim_instance(
            &format!("gemini-count-facts-{}-{unique}", std::process::id()),
            600,
        )
        .unwrap();
    drop(pg);
    let billing = Arc::new(
        AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner),
            1,
            0,
        )
        .unwrap(),
    );
    for (logical, intent) in [
        (
            "123e4567-e89b-42d3-a456-426614174080",
            Some(UniversalCountTokensIntent {
                requested_model: Some("google/gemini-integration-model".into()),
                classification: classify_gemini_generate_content(&json!({"tools": []})),
            }),
        ),
        ("123e4567-e89b-42d3-a456-426614174081", None),
    ] {
        let seed = GeminiRequestFactSeed {
            logical_request_id: logical.into(),
            client_attribution: crate::execution::ClientAttribution::unknown_for_internal_use(),
            execution: registry::ExecutionAttempt::direct(),
            account_id: ACCOUNT_ID.into(),
            key_id: key_id.clone(),
            admitted_at: pool::now(),
            lifecycle_clock: crate::execution::RequestLifecycleClock::default(),
        };
        let mut guard = GeminiCountTokensFactGuard::new(Arc::clone(&billing), seed, intent);
        if logical.ends_with("81") {
            guard.update_after_native_accept(
                "gemini-integration-model",
                &json!({"contents": [], "tools": [{"googleSearch": {}}]}),
            );
            guard.resolve_executable_model("gemini-integration-model");
        }
        guard.observe(CountTokensTerminalEvidence::body(StatusCode::OK));
        let mut response = (StatusCode::OK, "mapped").into_response();
        guard.terminal_response(&mut response);
        if let Some(handoff) = response
            .extensions_mut()
            .remove::<GeminiCountTokensFactHandoff>()
        {
            handoff.finish(StatusCode::OK, false);
        }
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let rows = loop {
        let rows = lock
            .query(
                "SELECT logical_request_id,route_class,provider_plane,request_class,requested_model,                         executable_model,stream_flag,billing_request_id,http_status_code,                         provider_terminal_class,delivery_state,upstream_request_id,internal_attempt_count                    FROM request_facts ORDER BY logical_request_id",
                &[],
            )
            .unwrap();
        if rows.len() == 2 {
            break rows;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Gemini facts did not arrive"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(rows[0].get::<_, String>(1), "universal");
    assert_eq!(rows[1].get::<_, String>(1), "native");
    for row in &rows {
        assert_eq!(row.get::<_, String>(2), "gemini");
        assert_eq!(row.get::<_, String>(3), "count_tokens");
        assert!(!row.get::<_, bool>(6));
        assert_eq!(row.get::<_, Option<String>>(7), None);
        assert_eq!(row.get::<_, Option<i32>>(8), Some(200));
        assert_eq!(row.get::<_, String>(9), "success");
        assert_eq!(row.get::<_, String>(10), "completed");
        assert_eq!(row.get::<_, Option<String>>(11), None);
        assert_eq!(row.get::<_, Option<i32>>(12), Some(0));
        let debug = format!("{row:?}");
        assert!(!debug.contains(CUSTOMER_KEY));
        assert!(!debug.contains(PROFILE_A_KEY));
        assert!(!debug.contains("googleSearch"));
    }
    assert_eq!(
        rows[0].get::<_, Option<String>>(4).as_deref(),
        Some("google/gemini-integration-model")
    );
    assert_eq!(rows[0].get::<_, Option<String>>(5), None);
    assert_eq!(
        rows[1].get::<_, Option<String>>(4).as_deref(),
        Some("gemini-integration-model")
    );
    assert_eq!(
        rows[1].get::<_, Option<String>>(5).as_deref(),
        Some("gemini-integration-model")
    );
    drop(billing);
    lock.query_one("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .unwrap();
}

#[test]
fn native_generation_tool_output_evidence_is_closed_and_conservative() {
    assert_eq!(
        gemini_tool_calls_in_output(&json!({
            "candidates": [{"content": {"parts": [{"text": "safe"}]}}],
            "usageMetadata": {}
        })),
        Some(false)
    );
    assert_eq!(
        gemini_tool_calls_in_output(&json!({
            "candidates": [{"content": {"parts": [{"functionCall": {"name": "redacted", "args": {}}}]}}],
            "usageMetadata": {}
        })),
        Some(true)
    );
    assert_eq!(
        gemini_tool_calls_in_output(&json!({
            "candidates": [{"content": {"parts": [{"futurePart": {}}]}}],
            "usageMetadata": {}
        })),
        None
    );
    assert_eq!(
        gemini_tool_calls_in_output(&json!({"candidates": "bad"})),
        None
    );
}

#[test]
fn streaming_tool_output_evidence_merges_without_inventing_false() {
    assert_eq!(
        merge_tool_call_evidence(Some(false), Some(false)),
        Some(false)
    );
    assert_eq!(merge_tool_call_evidence(None, Some(false)), None);
    assert_eq!(merge_tool_call_evidence(None, Some(true)), Some(true));
    assert_eq!(merge_tool_call_evidence(Some(true), None), Some(true));
}

#[test]
fn native_generation_fact_is_owned_by_postgres_reservation_and_settlement() {
    const LOCK: i64 = 831_572_908_441;
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping native Gemini generation fact matrix: test URL is unset");
        return;
    };
    let mut lock = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock.query_one("SELECT pg_advisory_lock($1)", &[&LOCK])
        .unwrap();
    lock.query_one("SELECT pg_advisory_lock($1)", &[&831_572_908_442_i64])
        .unwrap();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock.batch_execute(
        "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations,         capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts         RESTART IDENTITY CASCADE",
    )
    .unwrap();
    pg.account_create(ACCOUNT_ID, None, 10_000).unwrap();
    pg.account_topup(ACCOUNT_ID, 1_000_000_000, Some("gemini-generation-facts"))
        .unwrap();
    pg.key_issue(CUSTOMER_KEY, ACCOUNT_ID, None).unwrap();
    let owner = pg
        .claim_instance(
            &format!("gemini-generation-facts-{}-{unique}", std::process::id()),
            600,
        )
        .unwrap();
    drop(pg);

    let logical_id = "123e4567-e89b-42d3-a456-426614174071";
    let billing = Arc::new(
        AsyncBilling::start_authority(
            registry::authority::AuthorityConfig::Postgres { url: url.clone() },
            Some(owner),
            1,
            0,
        )
        .unwrap(),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let seen_count = runtime.block_on(async {
    let response_body = json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{
                "functionCall": {"name": "redacted", "args": {"private": "never persisted"}}
            }]},
            "finishReason": "STOP",
            "index": 0
        }],
        "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 3, "totalTokenCount": 5}
    });
    let (stream_reply, stream_drained) = MockReply::stream(vec![MockChunk::Data(
        Bytes::from_static(
            b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"stream\"}]}}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":3,\"totalTokenCount\":5}}}\n\n",
        ),
    )]);
    let mut replies = vec![MockReply::json(StatusCode::OK, response_body.clone()); 4];
    replies.push(stream_reply);
    replies.push(MockReply::json(StatusCode::OK, response_body.clone()));
    replies.push(MockReply::json(StatusCode::OK, response_body.clone()));
    replies.push(MockReply::json(StatusCode::OK, response_body.clone()));
    replies.push(MockReply::json(StatusCode::OK, json!({"totalTokens": 0})));
    replies.push(MockReply::json(
        StatusCode::OK,
        json!({
            "candidates": [{
                "content": {"parts": [{
                    "inlineData": {"mimeType": "image/png", "data": "AQID"}
                }]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1}
        }),
    ));
    replies.push(MockReply::json(StatusCode::OK, response_body.clone()));
    replies.push(MockReply::json(
        StatusCode::BAD_REQUEST,
        json!({"error": {"message": "private provider error"}}),
    ));
    replies.push(MockReply::json(
        StatusCode::OK,
        json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "missing usage"}]},
                "finishReason": "STOP"
            }]
        }),
    ));
    let server = start_mock(MockState::with_replies([
        (
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": {"message": "rotate"}}),
            )],
        ),
        (PROFILE_B_KEY, replies),
    ]))
    .await;
    let fixture = gateway_fixture_with_models(
        &server.upstream,
        &[None, None],
        1,
        None,
        OAuthKind::LegacyGeminiCli,
        64,
        &["gemini-integration-model", "gemini-3.1-flash-image"],
    );
    let app = app_state(fixture.gateway.clone(), Some(Arc::clone(&billing)));
    let mut request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(
            json!({
                "contents": [{"role": "user", "parts": [{"text": "private prompt"}]}],
                "tools": [{"functionDeclarations": [{"name": "private_tool", "description": "secret"}]}]
            })
            .to_string(),
        ))
        .unwrap();
    request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        logical_id.parse().unwrap(),
    );
    let logical = crate::execution::admit_logical_request_id(request.headers_mut()).unwrap();
    request.extensions_mut().insert(logical);
    request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        request,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = axum::body::to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();

    async fn invoke_universal_generation(app: AppState, route: &str, body: Value) -> Response {
        let mut inner = axum::extract::Request::builder()
            .method(Method::POST)
            .uri(format!("/test-{route}"))
            .header("content-type", "application/json")
            .header("x-goog-api-key", CUSTOMER_KEY)
            .body(Body::from(body.to_string()))
            .unwrap();
        let logical_id = match route {
            "chat" => "123e4567-e89b-42d3-a456-426614174084",
            "responses" => "123e4567-e89b-42d3-a456-426614174085",
            "messages" => "123e4567-e89b-42d3-a456-426614174087",
            _ => unreachable!(),
        };
        let mut logical_headers = HeaderMap::new();
        logical_headers.insert(
            crate::execution::LOGICAL_REQUEST_ID_HEADER,
            logical_id.parse().unwrap(),
        );
        let logical = crate::execution::admit_logical_request_id(&mut logical_headers).unwrap();
        inner.extensions_mut().insert(logical);
        inner
            .extensions_mut()
            .insert(crate::execution::RequestLifecycleClock::default());
        let state = State(app);
        let peer = ConnectInfo("198.51.100.10:12345".parse().unwrap());
        match route {
            "chat" => crate::gemini::chat::gemini_chat_completions(state, peer, inner).await,
            "responses" => crate::gemini::responses::gemini_responses(state, peer, inner).await,
            "messages" => crate::gemini::skin::gemini_messages_skin(state, peer, inner).await,
            _ => unreachable!(),
        }
    }
    for (route, body) in [
        (
            "chat",
            json!({"model": "google/gemini-integration-model", "messages": [{"role": "user", "content": "chat secret"}]}),
        ),
        (
            "responses",
            json!({"model": "google/gemini-integration-model", "input": "responses secret"}),
        ),
        (
            "messages",
            json!({"model": "google/gemini-integration-model", "max_tokens": 8, "messages": [{"role": "user", "content": "messages secret"}]}),
        ),
    ] {
        let wrapped = invoke_universal_generation(app.clone(), route, body).await;
        assert_eq!(wrapped.status(), StatusCode::OK, "{route}");
        let _ = axum::body::to_bytes(wrapped.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap();
    }
    let stream_logical_id = "123e4567-e89b-42d3-a456-426614174073";
    let mut stream_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:streamGenerateContent?alt=sse")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(json!({"contents": []}).to_string()))
        .unwrap();
    stream_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        stream_logical_id.parse().unwrap(),
    );
    let stream_logical = crate::execution::admit_logical_request_id(stream_request.headers_mut()).unwrap();
    stream_request.extensions_mut().insert(stream_logical);
    stream_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let stream_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        stream_request,
    )
    .await;
    assert_eq!(stream_response.status(), StatusCode::OK);
    let _ = axum::body::to_bytes(stream_response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    assert!(stream_drained.load(Ordering::Acquire));

    super::super::billing::fail_next_gemini_delivery_marker_for_test();
    let marker_logical_id = "123e4567-e89b-42d3-a456-426614174074";
    let mut marker_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(json!({"contents": []}).to_string()))
        .unwrap();
    marker_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        marker_logical_id.parse().unwrap(),
    );
    let marker_logical = crate::execution::admit_logical_request_id(marker_request.headers_mut()).unwrap();
    marker_request.extensions_mut().insert(marker_logical);
    marker_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let marker_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        marker_request,
    )
    .await;
    assert_eq!(marker_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    for (case, include_logical, include_lifecycle) in [
        ("missing-logical", false, true),
        ("missing-lifecycle", true, false),
    ] {
        let mut excluded_request = axum::extract::Request::builder()
            .method(Method::POST)
            .uri("/v1beta/models/gemini-integration-model:generateContent")
            .header("content-type", "application/json")
            .header("x-goog-api-key", CUSTOMER_KEY)
            .body(Body::from(json!({"contents": []}).to_string()))
            .unwrap();
        if include_logical {
            excluded_request.headers_mut().insert(
                crate::execution::LOGICAL_REQUEST_ID_HEADER,
                "123e4567-e89b-42d3-a456-426614174076".parse().unwrap(),
            );
            let logical = crate::execution::admit_logical_request_id(excluded_request.headers_mut()).unwrap();
            excluded_request.extensions_mut().insert(logical);
        }
        if include_lifecycle {
            excluded_request
                .extensions_mut()
                .insert(crate::execution::RequestLifecycleClock::default());
        }
        let excluded_response = api(
            State(app.clone()),
            ConnectInfo("198.51.100.10:12345".parse().unwrap()),
            excluded_request,
        )
        .await;
        assert_eq!(excluded_response.status(), StatusCode::OK, "{case}");
    }
    let mut unauthorized = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", "sk-pool-invalid-gemini-key")
        .body(Body::from(json!({"contents": []}).to_string()))
        .unwrap();
    unauthorized.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174077".parse().unwrap(),
    );
    let unauthorized_logical = crate::execution::admit_logical_request_id(unauthorized.headers_mut()).unwrap();
    unauthorized.extensions_mut().insert(unauthorized_logical);
    unauthorized
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let unauthorized_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        unauthorized,
    )
    .await;
    assert_eq!(unauthorized_response.status(), StatusCode::BAD_REQUEST);
    let mut malformed = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from("not-json"))
        .unwrap();
    malformed.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174078".parse().unwrap(),
    );
    let malformed_logical = crate::execution::admit_logical_request_id(malformed.headers_mut()).unwrap();
    malformed.extensions_mut().insert(malformed_logical);
    malformed
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let malformed_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        malformed,
    )
    .await;
    assert_eq!(malformed_response.status(), StatusCode::BAD_REQUEST);

    let mut count_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:countTokens")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(json!({"contents": []}).to_string()))
        .unwrap();
    count_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174080".parse().unwrap(),
    );
    let count_logical =
        crate::execution::admit_logical_request_id(count_request.headers_mut()).unwrap();
    count_request.extensions_mut().insert(count_logical);
    count_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let count_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        count_request,
    )
    .await;
    assert_eq!(count_response.status(), StatusCode::OK);
    let _ = axum::body::to_bytes(count_response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();

    let mut image_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-3.1-flash-image:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(
            json!({
                "contents": [{"parts": [{"text": "private image prompt"}]}],
                "generationConfig": {
                    "responseModalities": ["TEXT", "IMAGE"],
                    "imageConfig": {"aspectRatio": "1:1", "imageSize": "1K"}
                }
            })
            .to_string(),
        ))
        .unwrap();
    image_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174081".parse().unwrap(),
    );
    let image_logical =
        crate::execution::admit_logical_request_id(image_request.headers_mut()).unwrap();
    image_request.extensions_mut().insert(image_logical);
    image_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let image_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        image_request,
    )
    .await;
    let image_status = image_response.status();
    let image_body = axum::body::to_bytes(image_response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();
    assert_eq!(
        image_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&image_body)
    );

    let admin_app = AppState {
        cfg: proxy_config(true),
        ..app.clone()
    };
    let mut admin_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(
            json!({"contents": [{"parts": [{"text": "admin private prompt"}]}]}).to_string(),
        ))
        .unwrap();
    admin_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174082".parse().unwrap(),
    );
    let admin_logical =
        crate::execution::admit_logical_request_id(admin_request.headers_mut()).unwrap();
    admin_request.extensions_mut().insert(admin_logical);
    admin_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let admin_response = api(
        State(admin_app),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        admin_request,
    )
    .await;
    assert_eq!(admin_response.status(), StatusCode::OK);
    let _ = axum::body::to_bytes(admin_response.into_body(), GEMINI_BODY_LIMIT)
        .await
        .unwrap();

    let mut batch_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/batches")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(json!({"requests": []}).to_string()))
        .unwrap();
    batch_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174083".parse().unwrap(),
    );
    let batch_logical =
        crate::execution::admit_logical_request_id(batch_request.headers_mut()).unwrap();
    batch_request.extensions_mut().insert(batch_logical);
    batch_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let batch_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        batch_request,
    )
    .await;
    assert_eq!(batch_response.status(), StatusCode::NOT_FOUND);

    let failure_logical_id = "123e4567-e89b-42d3-a456-426614174075";
    let mut failure_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(json!({"contents": []}).to_string()))
        .unwrap();
    failure_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        failure_logical_id.parse().unwrap(),
    );
    let failure_logical = crate::execution::admit_logical_request_id(failure_request.headers_mut()).unwrap();
    failure_request.extensions_mut().insert(failure_logical);
    failure_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let failure_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        failure_request,
    )
    .await;
    assert_eq!(failure_response.status(), StatusCode::BAD_REQUEST);

    let mut missing_usage_request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1beta/models/gemini-integration-model:generateContent")
        .header("content-type", "application/json")
        .header("x-goog-api-key", CUSTOMER_KEY)
        .body(Body::from(json!({"contents": []}).to_string()))
        .unwrap();
    missing_usage_request.headers_mut().insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        "123e4567-e89b-42d3-a456-426614174086".parse().unwrap(),
    );
    let missing_usage_logical =
        crate::execution::admit_logical_request_id(missing_usage_request.headers_mut()).unwrap();
    missing_usage_request
        .extensions_mut()
        .insert(missing_usage_logical);
    missing_usage_request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let missing_usage_response = api(
        State(app.clone()),
        ConnectInfo("198.51.100.10:12345".parse().unwrap()),
        missing_usage_request,
    )
    .await;
    assert_eq!(
        missing_usage_response.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );

    billing.flush().await.unwrap();
    server.state.seen().len()
    });

    let row = lock
        .query_one(
            "SELECT logical_request_id,billing_request_id,provider_plane,route_class,request_class,                    requested_model,executable_model,stream_flag,tools_declared_count,                    tool_calls_in_output,http_status_code,provider_terminal_class,delivery_state,                    upstream_request_id,internal_attempt_count,billing_outcome             FROM request_facts WHERE logical_request_id=$1",
            &[&logical_id],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), logical_id);
    assert!(row.get::<_, Option<String>>(1).is_some());
    assert_eq!(row.get::<_, String>(2), "gemini");
    assert_eq!(row.get::<_, String>(3), "native");
    assert_eq!(row.get::<_, String>(4), "generate");
    assert_eq!(
        row.get::<_, Option<String>>(5).as_deref(),
        Some("gemini-integration-model")
    );
    assert_eq!(
        row.get::<_, Option<String>>(6).as_deref(),
        Some("gemini-integration-model")
    );
    assert!(!row.get::<_, bool>(7));
    assert_eq!(row.get::<_, Option<i32>>(8), Some(1));
    assert_eq!(row.get::<_, Option<bool>>(9), Some(true));
    assert_eq!(row.get::<_, Option<i32>>(10), Some(200));
    assert_eq!(row.get::<_, String>(11), "success");
    assert_eq!(row.get::<_, String>(12), "completed");
    assert_eq!(row.get::<_, Option<String>>(13), None);
    assert_eq!(row.get::<_, Option<i32>>(14), Some(2));
    assert_eq!(row.get::<_, String>(15), "winner");
    let stream_row = lock
        .query_one(
            "SELECT request_class,stream_flag,tool_calls_in_output,http_status_code,provider_terminal_class,delivery_state,internal_attempt_count FROM request_facts WHERE logical_request_id=$1",
            &[&"123e4567-e89b-42d3-a456-426614174073"],
        )
        .unwrap();
    assert_eq!(stream_row.get::<_, String>(0), "stream_generate");
    assert!(stream_row.get::<_, bool>(1));
    assert_eq!(stream_row.get::<_, Option<bool>>(2), Some(false));
    assert_eq!(stream_row.get::<_, Option<i32>>(3), Some(200));
    assert_eq!(stream_row.get::<_, String>(4), "success");
    assert_eq!(stream_row.get::<_, String>(5), "completed");
    assert_eq!(stream_row.get::<_, Option<i32>>(6), Some(1));
    let fact_count = lock
        .query_one(
            "SELECT count(*) FROM request_facts WHERE billing_request_id IS NOT NULL",
            &[],
        )
        .unwrap()
        .get::<_, i64>(0);
    assert_eq!(
        fact_count, 8,
        "native and universal generation routes must each create exactly one leaf fact"
    );
    let universal_rows = lock
        .query(
            "SELECT logical_request_id,route_class,request_class,requested_model,executable_model,stream_flag,billing_outcome FROM request_facts WHERE logical_request_id IN ($1,$2,$3) ORDER BY request_class",
            &[
                &"123e4567-e89b-42d3-a456-426614174084",
                &"123e4567-e89b-42d3-a456-426614174085",
                &"123e4567-e89b-42d3-a456-426614174087",
            ],
        )
        .unwrap();
    assert_eq!(universal_rows.len(), 3);
    for row in &universal_rows {
        assert_eq!(row.get::<_, String>(1), "universal");
        assert_eq!(
            row.get::<_, Option<String>>(3).as_deref(),
            Some("google/gemini-integration-model")
        );
        assert_eq!(
            row.get::<_, Option<String>>(4).as_deref(),
            Some("gemini-integration-model")
        );
        assert!(!row.get::<_, bool>(5));
        assert_eq!(row.get::<_, String>(6), "winner");
    }
    assert_eq!(universal_rows[0].get::<_, String>(2), "chat");
    assert_eq!(universal_rows[1].get::<_, String>(2), "messages");
    assert_eq!(universal_rows[2].get::<_, String>(2), "responses");
    let excluded_ids = [
        "123e4567-e89b-42d3-a456-426614174076",
        "123e4567-e89b-42d3-a456-426614174077",
        "123e4567-e89b-42d3-a456-426614174078",
        "123e4567-e89b-42d3-a456-426614174080",
        "123e4567-e89b-42d3-a456-426614174081",
        "123e4567-e89b-42d3-a456-426614174082",
        "123e4567-e89b-42d3-a456-426614174083",
    ];
    for excluded_id in excluded_ids {
        let excluded_count = lock
            .query_one(
                "SELECT count(*) FROM request_facts WHERE logical_request_id=$1 AND request_class IN ('generate','stream_generate')",
                &[&excluded_id],
            )
            .unwrap()
            .get::<_, i64>(0);
        assert_eq!(excluded_count, 0, "excluded request {excluded_id}");
    }
    let count_deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let count_fact = lock
            .query_one(
                "SELECT count(*) FROM request_facts WHERE logical_request_id=$1 AND request_class='count_tokens'",
                &[&"123e4567-e89b-42d3-a456-426614174080"],
            )
            .unwrap()
            .get::<_, i64>(0);
        if count_fact == 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < count_deadline,
            "countTokens must remain owned only by its Stage-6 producer"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let ownership = lock
        .query(
            "SELECT f.logical_request_id,f.billing_request_id,f.client_kind,f.client_source,                    r.state,r.provider,o.state,o.disposition,                    (SELECT count(*) FROM request_facts d WHERE d.billing_request_id=f.billing_request_id),                    (SELECT count(*) FROM usage_events u WHERE u.request_id=f.billing_request_id AND u.provider='google'),                    (SELECT count(*) FROM ledger l WHERE l.request_id=f.billing_request_id)             FROM request_facts f             JOIN reservations r ON r.request_id=f.billing_request_id             JOIN settlement_outbox o ON o.request_id=f.billing_request_id             ORDER BY f.logical_request_id",
            &[],
        )
        .unwrap();
    assert_eq!(ownership.len(), 8);
    for row in &ownership {
        assert!(row.get::<_, Option<String>>(1).is_some());
        assert_eq!(row.get::<_, String>(2), "unknown");
        assert_eq!(row.get::<_, String>(3), "unknown");
        assert!(matches!(
            row.get::<_, String>(4).as_str(),
            "settled" | "canceled"
        ));
        assert_eq!(row.get::<_, String>(5), registry::PROVIDER_GOOGLE);
        assert_eq!(row.get::<_, String>(6), "done");
        assert_eq!(row.get::<_, i64>(8), 1, "one fact per billing reservation");
    }
    // Every provider-success turn, including all three universal adapters, owns one usage and
    // charge row. Provider rejection and missing terminal usage own applied cancellations only.
    for row in &ownership {
        let logical_id = row.get::<_, String>(0);
        let canceled = logical_id.ends_with("4075") || logical_id.ends_with("4086");
        assert_eq!(
            row.get::<_, String>(4),
            if canceled { "canceled" } else { "settled" }
        );
        assert_eq!(
            row.get::<_, String>(7),
            if canceled { "cancel" } else { "settle" }
        );
        assert_eq!(row.get::<_, i64>(9), if canceled { 0 } else { 1 });
        assert_eq!(row.get::<_, i64>(10), if canceled { 0 } else { 1 });
    }
    let missing_usage_row = lock
        .query_one(
            "SELECT http_status_code,provider_terminal_class,delivery_state,internal_attempt_count,billing_outcome FROM request_facts WHERE logical_request_id=$1",
            &[&"123e4567-e89b-42d3-a456-426614174086"],
        )
        .unwrap();
    assert_eq!(missing_usage_row.get::<_, Option<i32>>(0), Some(503));
    assert_eq!(missing_usage_row.get::<_, String>(1), "protocol_error");
    assert_eq!(missing_usage_row.get::<_, String>(2), "interrupted");
    assert_eq!(missing_usage_row.get::<_, Option<i32>>(3), Some(1));
    assert_eq!(missing_usage_row.get::<_, String>(4), "canceled");
    let failure_row = lock
        .query_one(
            "SELECT http_status_code,provider_terminal_class,delivery_state,internal_attempt_count,billing_outcome FROM request_facts WHERE logical_request_id=$1",
            &[&"123e4567-e89b-42d3-a456-426614174075"],
        )
        .unwrap();
    assert_eq!(failure_row.get::<_, Option<i32>>(0), Some(400));
    assert_eq!(failure_row.get::<_, String>(1), "client_error");
    assert_eq!(failure_row.get::<_, String>(2), "interrupted");
    assert_eq!(failure_row.get::<_, Option<i32>>(3), Some(1));
    assert_eq!(failure_row.get::<_, String>(4), "canceled");
    let marker_row = lock
        .query_one(
            "SELECT http_status_code,provider_terminal_class,delivery_state,internal_attempt_count,billing_outcome FROM request_facts WHERE logical_request_id=$1",
            &[&"123e4567-e89b-42d3-a456-426614174074"],
        )
        .unwrap();
    assert_eq!(marker_row.get::<_, Option<i32>>(0), Some(503));
    assert_eq!(marker_row.get::<_, String>(1), "success");
    assert_eq!(marker_row.get::<_, String>(2), "unknown");
    assert_eq!(marker_row.get::<_, Option<i32>>(3), Some(1));
    assert_eq!(marker_row.get::<_, String>(4), "winner");
    let persisted_json = lock
        .query_one(
            "SELECT json_build_object(                'facts',(SELECT coalesce(json_agg(row_to_json(f)),'[]'::json) FROM request_facts f),                'outbox',(SELECT coalesce(json_agg(row_to_json(o)),'[]'::json) FROM settlement_outbox o)             )::text",
            &[],
        )
        .unwrap()
        .get::<_, String>(0);
    for forbidden in [
        "private prompt",
        "private_tool",
        "secret",
        "private provider error",
        "private image prompt",
        "admin private prompt",
        CUSTOMER_KEY,
        PROFILE_A_KEY,
        PROFILE_B_KEY,
    ] {
        assert!(!persisted_json.contains(forbidden));
    }
    assert_eq!(seen_count, 15);
    drop(billing);
    lock.query_one("SELECT pg_advisory_unlock($1)", &[&831_572_908_442_i64])
        .unwrap();
    lock.query_one("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .unwrap();
}

#[test]
fn generation_fact_surface_excludes_non_stage7_routes() {
    assert!(!super::super::GeminiBatchRuntimeConfig::default().enabled);
    assert!(billable_generation_fact_eligible(
        Operation::Generate,
        false
    ));
    assert!(billable_generation_fact_eligible(
        Operation::StreamGenerate,
        false
    ));
    for operation in [Operation::CountTokens, Operation::Models, Operation::Model] {
        assert!(!billable_generation_fact_eligible(operation, false));
    }
    assert!(!billable_generation_fact_eligible(
        Operation::Generate,
        true
    ));
}
