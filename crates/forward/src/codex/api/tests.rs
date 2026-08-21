use super::*;
use crate::affinity::AffinityStore;
use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::codex::{CodexConfig, CodexPrices};
use crate::config::ProxyConfig;
use crate::metrics::Metrics;
use crate::state::ProviderMode;
use crate::upstream::Clients;
use axum::body::to_bytes;
use pool::{Pool, Reserve};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

#[test]
fn strip_reasoning_items_keeps_portable_items_only() {
    let items = vec![
        json!({"type": "message", "role": "user", "content": []}),
        json!({"type": "reasoning", "encrypted_content": "secret", "summary": []}),
        json!({"type": "function_call", "name": "f", "arguments": "{}"}),
        json!({"type": "function_call_output", "output": "ok"}),
    ];
    let kept = strip_reasoning_items(items);
    let types: Vec<_> = kept
        .iter()
        .map(|item| item["type"].as_str().unwrap())
        .collect();
    // Reasoning (model-bound) is dropped on a cross-model replay; messages and tool items stay.
    assert_eq!(
        types,
        vec!["message", "function_call", "function_call_output"]
    );
}

#[test]
fn drop_unencrypted_reasoning_keeps_only_continuation_capable_items() {
    let items = vec![
        json!({"type": "message", "role": "user", "content": []}),
        // The echo-output-to-input shape every SDK produces without `include`: unresolvable
        // upstream, fails the whole turn (live probe 2026-08-18).
        json!({"type": "reasoning", "id": "rs_1", "summary": []}),
        json!({"type": "reasoning", "id": "rs_2", "summary": [], "encrypted_content": "key"}),
        json!({"type": "function_call", "id": "fc_1", "call_id": "c1", "name": "f", "arguments": "{}"}),
        json!({"type": "function_call_output", "call_id": "c1", "output": "ok"}),
        json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "done"}]}),
    ];
    let (kept, dropped) = drop_unencrypted_reasoning(items);
    assert_eq!(dropped, 1);
    let types: Vec<_> = kept
        .iter()
        .map(|item| item["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        vec![
            "message",
            "reasoning",
            "function_call",
            "function_call_output",
            "message"
        ]
    );
    // The surviving reasoning item is the one carrying its continuation key.
    assert_eq!(kept[1]["id"], "rs_2");
    assert_eq!(kept[1]["encrypted_content"], "key");
}

#[test]
fn drop_unencrypted_reasoning_is_a_noop_without_reasoning_items() {
    let items = vec![
        json!({"type": "message", "role": "user", "content": []}),
        json!({"type": "function_call", "id": "fc_1", "call_id": "c1", "name": "f", "arguments": "{}"}),
    ];
    let (kept, dropped) = drop_unencrypted_reasoning(items.clone());
    assert_eq!(dropped, 0);
    assert_eq!(kept, items);
}

/// Every public error the OpenAI-compatible surface can produce.
fn all_public_errors() -> Vec<ApiError> {
    let mut errors = vec![
        ApiError::invalid("test", None::<String>),
        ApiError::not_found("test", None::<String>),
        ApiError::unavailable(),
        ApiError::rate_limited(),
        ApiError::rate_limited_for(Some(42)),
    ];
    for admission in [
        AdmissionError::Unauthorized,
        AdmissionError::Unavailable,
        AdmissionError::LowBalance,
    ] {
        errors.push(ApiError::from(admission));
    }
    for process in [
        ProcessError::Disabled,
        ProcessError::InvalidConfig("credential file unreadable".to_string()),
        ProcessError::Closed,
        ProcessError::Timeout("turn completion"),
        ProcessError::Protocol("upstream served an unexpected model".to_string()),
        ProcessError::ContextWindowExceeded,
        ProcessError::UsageLimitExceeded {
            retry_after: Some(60),
        },
        ProcessError::BadRequest,
        ProcessError::AuthenticationRequired,
        ProcessError::SubscriptionRequired,
    ] {
        errors.push(ApiError::from(process));
    }
    errors
}

#[test]
fn public_errors_never_leak_internal_architecture() {
    // The client believes it is talking to an OpenAI-compatible endpoint. No public field may
    // reveal how the provider is built: the home pool, the app-server child, the pinned binary,
    // the ChatGPT profile behind it, or any upstream diagnostic text. This is the Codex twin of
    // `proxy::tests::local_err_never_leaks_internal_architecture`.
    let forbidden = [
        "codex",
        "app-server",
        "app server",
        "chatgpt",
        "subscription",
        "home",
        "pool",
        "upstream",
        "authority",
        "cooling",
        "rotat",
        "binary",
        "digest",
        "sha256",
        "profile",
        "device",
        "/srv/",
        "sensitive upstream diagnostic",
    ];
    for error in all_public_errors() {
        let haystack = format!("{} {}", error.kind, error.message).to_lowercase();
        for term in forbidden {
            assert!(
                !haystack.contains(term),
                "public error leaks internal term {term:?}: {haystack:?}"
            );
        }
    }
}

#[test]
fn failed_external_fallback_keeps_local_status_but_removes_not_started_proof() {
    let response = ApiError::from(ProcessError::ExternalFallbackFailed {
        local: Box::new(ProcessError::UsageLimitExceeded {
            retry_after: Some(42),
        }),
    })
    .into_response();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get("retry-after").unwrap(), "42");
    assert!(response
        .headers()
        .get("x-apitoken-execution-state")
        .is_none());
    assert_eq!(
        response.extensions().get::<TerminalErrorReason>(),
        Some(&TerminalErrorReason("claudestore_fallback_failed"))
    );
}

#[test]
fn public_errors_keep_openai_shaped_status_and_type_pairs() {
    // A client's retry logic keys on these pairs; an internal fault must always be retryable
    // rather than surfacing as a client error it would never retry.
    for error in all_public_errors() {
        let expected_kind = match error.status {
            StatusCode::BAD_REQUEST => "invalid_request_error",
            StatusCode::UNAUTHORIZED => "invalid_request_error",
            StatusCode::NOT_FOUND => "invalid_request_error",
            StatusCode::PAYMENT_REQUIRED => "insufficient_quota",
            StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
            StatusCode::SERVICE_UNAVAILABLE => "server_error",
            other => panic!("unexpected public status {other}"),
        };
        assert_eq!(error.kind, expected_kind, "status {}", error.status);
    }
}

#[test]
fn every_public_error_marks_the_execution_not_started() {
    // Каждый публичный отказ OpenAI-конверта — не-2xx до границы доставки: reserve (если
    // успели взять) возвращает дроп CodexAdmission → HoldGuard, ни байта клиенту не ушло.
    // Значит все они обязаны нести x-apitoken-execution-state: not_started, а успешный
    // json_response (2xx) — не нести никогда.
    for error in all_public_errors() {
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
    let ok = json_response(StatusCode::OK, json!({"id": "resp_1"}), "req_1");
    assert!(ok
        .headers()
        .get(crate::proxy::EXECUTION_STATE_HEADER)
        .is_none());
}

#[test]
fn a_subscription_limit_is_advertised_with_a_wait_a_client_can_honour() {
    let limited = ApiError::from(ProcessError::UsageLimitExceeded {
        retry_after: Some(123),
    });
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.retry_after, Some(123));
    // A limit with no published reset must still carry a usable wait, never none.
    let unknown = ApiError::from(ProcessError::UsageLimitExceeded { retry_after: None });
    assert!(unknown.retry_after.is_some_and(|seconds| seconds > 0));
}

#[test]
fn codex_catalog_clients_get_their_native_models_envelope() {
    for (header, value) in [
        ("originator", "codex_exec"),
        ("originator", "codex_cli_rs"),
        ("user-agent", "codex_cli_rs/0.146.0"),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(header, HeaderValue::from_static(value));
        assert!(
            requests_codex_models_envelope(&headers),
            "{header}: {value}"
        );
    }

    let mut openai_headers = HeaderMap::new();
    openai_headers.insert("user-agent", HeaderValue::from_static("OpenAI/Python 2.0"));
    assert!(!requests_codex_models_envelope(&openai_headers));
    let mut near_match = HeaderMap::new();
    near_match.insert("originator", HeaderValue::from_static("my-codex-proxy"));
    assert!(!requests_codex_models_envelope(&near_match));
}

#[test]
fn standard_model_list_falls_back_to_the_configured_catalog() {
    let gateway = gateway();
    let data = public_model_objects(&gateway, None);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], "gpt-5.6");
    assert_eq!(data[0]["apitoken"]["limits"], json!({"output": 128000}));
    assert_eq!(
        data[0]["apitoken"]["capabilities"]["service_tiers"],
        json!(["standard", "priority"])
    );
    assert_eq!(
        data[0]["apitoken"]["capabilities"]["reasoning_efforts"],
        json!(["none", "low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(
        data[0]["apitoken"]["capabilities"],
        json!({
            "reasoning_efforts": ["none", "low", "medium", "high", "xhigh", "max"],
            "service_tiers": ["standard", "priority"],
            "input_modalities": ["text", "image"],
            "output_modalities": ["text"],
            "tool_calling": true,
            "structured_outputs": true,
            "streaming": true
        })
    );
    assert!(data[0].get("name").is_none());
}

#[test]
fn standard_model_list_uses_the_last_good_upstream_intersection() {
    let gateway = gateway();
    let available = crate::codex::CodexModelCatalog {
        models: HashSet::from(["different-upstream-model".to_string()]),
        ..Default::default()
    };
    assert!(public_model_objects(&gateway, Some(&available)).is_empty());

    let available = crate::codex::CodexModelCatalog {
        models: HashSet::from(["gpt-5.6-sol".to_string()]),
        input_token_limits: HashMap::from([("gpt-5.6-sol".to_string(), 272_000)]),
        display_names: HashMap::from([("gpt-5.6-sol".to_string(), "GPT 5.6 Thinking".to_string())]),
        ..Default::default()
    };
    let data = public_model_objects(&gateway, Some(&available));
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], "gpt-5.6");
    assert_eq!(
        data[0]["apitoken"]["limits"],
        json!({"context": 400000, "input": 272000, "output": 128000})
    );
    assert_eq!(data[0]["name"], "GPT 5.6 Thinking");
}

/// Discovery must name the image models, because a client that cannot see them in `/v1/models`
/// concludes the pool has no image model at all — even though the image routes accept them.
#[test]
fn image_models_are_published_with_their_real_capabilities() {
    let data = public_image_model_objects();
    assert_eq!(
        data.iter().map(|model| &model["id"]).collect::<Vec<_>>(),
        vec!["gpt-image-2", "gpt-image-2-2026-04-21"]
    );
    for model in &data {
        // Both ids are exactly what the paid image routes admit.
        assert!(metering::openai_image_tariff(model["id"].as_str().unwrap()).is_ok());
        assert_eq!(model["object"], "model");
        assert_eq!(model["created"], metering::GPT_IMAGE_2_CREATED);
        assert_eq!(
            model["apitoken"]["capabilities"],
            json!({
                "reasoning_efforts": [],
                "service_tiers": ["standard"],
                "input_modalities": ["text", "image"],
                "output_modalities": ["image"],
                "tool_calling": false,
                "structured_outputs": false,
                "reasoning": false,
                "streaming": false
            })
        );
        assert_eq!(
            model["apitoken"]["endpoints"],
            json!(["/v1/images/generations", "/v1/images/edits"])
        );
        // No invented token limits: the image wire publishes none.
        assert!(model["apitoken"].get("limits").is_none());
    }
}

/// A published model that a text lane silently 404s as "does not exist" is worse than an unlisted
/// one: the client cannot tell a typo from a wrong endpoint.
#[test]
fn text_lanes_reject_image_models_by_pointing_at_the_image_routes() {
    let gateway = gateway();
    for requested in ["gpt-image-2", "openai/gpt-image-2-2026-04-21"] {
        let error =
            parse_responses_request(&gateway, json!({"model": requested, "input": "draw a cat"}))
                .expect_err("image model on a text lane");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(
            error.message.contains("/v1/images/generations"),
            "{}",
            error.message
        );
    }

    let unknown = parse_responses_request(&gateway, json!({"model": "gpt-nope", "input": "hi"}))
        .expect_err("unknown model");
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

fn model() -> CodexModel {
    CodexModel {
        id: "gpt-5.6".to_string(),
        upstream: "gpt-5.6-sol".to_string(),
        created: 0,
        owned_by: "test".to_string(),
        max_output_tokens: 128_000,
        reasoning_efforts: ["none", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        input_modalities: vec!["text".to_string(), "image".to_string()],
        output_modalities: vec!["text".to_string()],
        tool_calling: true,
        structured_outputs: true,
        fast_multiplier_basis_points: Some(25_000),
        prices: CodexPrices {
            input: 5_000,
            cached_input: 500,
            cache_write_input: 6_250,
            output: 30_000,
            api_fast_multiplier_basis_points: 25_000,
            long_context_threshold: 272_000,
            long_input_basis_points: 20_000,
            long_output_basis_points: 15_000,
        },
    }
}

fn gateway() -> CodexGateway {
    gateway_at(codex_credential::CODEX_DEFAULT_BASE_URL)
}

fn gateway_at(base_url: &str) -> CodexGateway {
    let root = std::env::temp_dir().join(format!("claude-api-codex-api-test-{}", new_id("roster")));
    let credentials = root.join("credentials");
    std::fs::create_dir_all(&credentials).unwrap();
    let keyring =
        codex_credential::CredentialKeyring::parse(&format!("current:{}", "ab".repeat(32)))
            .unwrap();
    let credential = codex_credential::CodexCredential {
        version: 1,
        access_token: "test-access-token".to_string(),
        refresh_token: "test-refresh-token".to_string(),
        expires_at: i64::MAX / 2,
        oauth_client_id: codex_credential::CODEX_OFFICIAL_OAUTH_CLIENT_ID.to_string(),
        token_uri: codex_credential::CODEX_OFFICIAL_TOKEN_URI.to_string(),
        account_id: "acct_test_1234".to_string(),
        email: "owner@example.com".to_string(),
        plan: "chatgpt_plus".to_string(),
        proxy: String::new(),
        proxy_order_id: 0,
        issued_at: 0,
    };
    let envelope = keyring.seal("current", "alpha", &credential).unwrap();
    std::fs::write(
        credentials.join("alpha.json"),
        codex_credential::encode_envelope(&envelope).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("profiles.json"),
        serde_json::to_vec(&serde_json::json!({
            "profiles": [{
                "id": "alpha",
                "credential_file": credentials.join("alpha.json").to_str().unwrap(),
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    CodexGateway::new(CodexConfig {
        smooth_wait_ms: 0,
        enabled: true,
        base_url: base_url.to_string(),
        profiles_file: root.join("profiles.json").to_str().unwrap().to_string(),
        credential_keys: keyring,
        cli_version: codex_credential::CODEX_CLI_VERSION.to_string(),
        request_timeout_ms: 1_000,
        turn_timeout_ms: 1_000,
        turn_silence_timeout_ms: 1_000,
        health_probe_interval_secs: 300,
        reserve_5h: 0.10,
        reserve_7d: 0.03,
        reserve_jitter: 0.0,
        reserve_overhead_tokens: 0,
        history_ttl_secs: 600,
        history_local_cap: 32,
        history_redis_url: None,
        history_secret: Some("test".to_string()),
        history_redis_timeout_ms: 10,
        default_proxy_env: BTreeMap::new(),
        models: vec![model()],
    })
    .unwrap()
}

const INPUT_TOKENS_RAW_KEY: &str = "sk-pool-native-input-secret-never-a-fact";
const INPUT_TOKENS_ACCOUNT_ID: &str = "native-input-fact-account";
const INPUT_TOKENS_KEY_ID: &str = "key_native_input_nonsecret";
const INPUT_TOKENS_LOGICAL_ID: &str = "33333333-3333-4333-8333-333333333333";
const INPUT_TOKENS_EXECUTION_GROUP: &str = "44444444-4444-4444-8444-444444444444";

struct InputTokensTestApp {
    app: AppState,
    path: PathBuf,
}

impl Drop for InputTokensTestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(format!("{}-wal", self.path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", self.path.display()));
    }
}

fn input_tokens_unique_path(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "claude-api-native-input-{label}-{}-{unique}-{sequence}.sqlite",
        std::process::id()
    ))
}

fn input_tokens_proxy_config() -> Arc<ProxyConfig> {
    Arc::new(ProxyConfig {
        api_keys: Vec::new(),
        control_keys: Vec::new(),
        panel_keys: Vec::new(),
        default_mult_bp: 10_000,
        trust_loopback: false,
        upstream: "http://127.0.0.1:1".into(),
        claudestore_fallback: None,
        max_tries: 1,
        util_cap: 1.0,
        cool_secs: 1,
        smooth_wait_ms: 0,
        poll: false,
        inject_identity: false,
        identity: String::new(),
        inject_billing: false,
        cc_version: String::new(),
        cc_entrypoint: String::new(),
        default_beta: String::new(),
        user_agent: "native-input-test".into(),
        user_agents: Vec::new(),
        ua_spread: 0,
        anthropic_version: String::new(),
        connect_timeout: 1,
        read_timeout: 1,
        nonstream_read_timeout: 1,
        x_app: String::new(),
        stainless_lang: String::new(),
        stainless_runtime: String::new(),
        stainless_runtime_version: String::new(),
        stainless_package_version: String::new(),
        stainless_os: String::new(),
        stainless_arch: String::new(),
    })
}

fn codex_test_body_storage(label: &str) -> Arc<crate::BodyStorage> {
    use std::os::unix::fs::PermissionsExt;
    static BODY_STORAGE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "codex-body-storage-{}-{}-{}-{label}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        BODY_STORAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    Arc::new(crate::BodyStorage::new(api_limits::current::PROVIDER, root).unwrap())
}

async fn input_tokens_test_app(
    metered: bool,
    fact_sender: Option<mpsc::Sender<registry::request_facts::TerminalRequestFact>>,
    provider: ProviderMode,
) -> InputTokensTestApp {
    let path = input_tokens_unique_path("facts");
    if metered {
        let connection = registry::open(path.to_str().unwrap()).unwrap();
        registry::account_create(&connection, INPUT_TOKENS_ACCOUNT_ID, None, 10_000).unwrap();
        registry::account_topup(&connection, INPUT_TOKENS_ACCOUNT_ID, 1_000, None).unwrap();
        registry::key_issue(
            &connection,
            INPUT_TOKENS_RAW_KEY,
            INPUT_TOKENS_ACCOUNT_ID,
            None,
        )
        .unwrap();
        connection
            .execute(
                "UPDATE api_keys SET key_id=?1 WHERE key=?2",
                (INPUT_TOKENS_KEY_ID, INPUT_TOKENS_RAW_KEY),
            )
            .unwrap();
    }
    let mut billing = AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();
    if let Some(sender) = fact_sender {
        billing.replace_request_fact_inbox_for_test(sender);
    }
    let billing = Arc::new(billing);
    let cfg = input_tokens_proxy_config();
    let app = AppState {
        provider,
        authority: Arc::new(registry::authority::AuthorityConfig::new(
            path.to_string_lossy().into_owned(),
            None,
        )),
        data_db_path: Arc::new(path.to_string_lossy().into_owned()),
        pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: Some(codex_test_body_storage("responses")),
        codex: Some(Arc::new(gateway())),
        gemini: None,
        gemini_batch: None,
        gemini_batch_runtime: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing: Some(billing),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(1)),
        metrics: Arc::new(Metrics::new()),
        probe_poke: None,
        admin_changes: tokio::sync::broadcast::channel(16).0,
        cfg,
    };
    InputTokensTestApp { app, path }
}

fn input_tokens_request(
    body: impl Into<Body>,
    key: Option<&str>,
    logical_id: Option<&str>,
    client: Option<&str>,
    attempt: Option<i32>,
    lifecycle_clock: Option<crate::execution::RequestLifecycleClock>,
) -> axum::extract::Request {
    let mut builder = axum::extract::Request::builder();
    if let Some(key) = key {
        builder = builder.header("x-api-key", key);
    }
    if let Some(attempt) = attempt {
        builder = builder
            .header(
                crate::execution::EXECUTION_GROUP_HEADER,
                INPUT_TOKENS_EXECUTION_GROUP,
            )
            .header(crate::execution::EXECUTION_ATTEMPT_HEADER, attempt);
    }
    let mut request = builder.body(body.into()).unwrap();
    if let Some(logical_id) = logical_id {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            crate::execution::LOGICAL_REQUEST_ID_HEADER,
            logical_id.parse().unwrap(),
        );
        request
            .extensions_mut()
            .insert(crate::execution::admit_logical_request_id(&mut headers).unwrap());
    }
    if let Some(client) = client {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            crate::execution::CLIENT_ATTRIBUTION_HEADER,
            client.parse().unwrap(),
        );
        request
            .extensions_mut()
            .insert(crate::execution::admit_client_attribution(&mut headers));
    }
    if let Some(lifecycle_clock) = lifecycle_clock {
        request.extensions_mut().insert(lifecycle_clock);
    }
    request
}

fn input_tokens_json_request(
    body: &Value,
    key: Option<&str>,
    logical_id: Option<&str>,
    client: Option<&str>,
    attempt: Option<i32>,
    lifecycle_clock: Option<crate::execution::RequestLifecycleClock>,
) -> axum::extract::Request {
    input_tokens_request(
        serde_json::to_vec(body).unwrap(),
        key,
        logical_id,
        client,
        attempt,
        lifecycle_clock,
    )
}

fn valid_input_tokens_body() -> Value {
    json!({
        "model": "openai/gpt-5.6",
        "input": "private prompt marker",
        "tools": [{
            "type": "function",
            "name": "private_tool_name",
            "parameters": {"type": "object", "properties": {"secret": {"type": "string"}}}
        }],
        "tool_choice": "required",
        "parallel_tool_calls": false,
        "text": {
            "format": {
                "type": "json_schema",
                "name": "private_schema_name",
                "schema": {"type": "object"}
            }
        },
        "reasoning": {"effort": "high"},
        "service_tier": "priority"
    })
}

async fn input_tokens_response_snapshot(
    response: Response,
) -> (StatusCode, axum::http::HeaderMap, Bytes) {
    let status = response.status();
    let mut headers = response.headers().clone();
    headers.remove("x-request-id");
    let bytes = to_bytes(response.into_body(), OPENAI_BODY_LIMIT)
        .await
        .unwrap();
    (status, headers, bytes)
}

async fn call_input_tokens(app: AppState, request: axum::extract::Request) -> Response {
    input_tokens(
        State(app),
        ConnectInfo("192.0.2.1:443".parse().unwrap()),
        request,
    )
    .await
}

fn assert_native_input_terminal(
    fact: &registry::request_facts::TerminalRequestFact,
    expected_status: StatusCode,
) {
    assert_eq!(fact.logical_request_id, INPUT_TOKENS_LOGICAL_ID);
    assert_eq!(fact.billing_request_id, None);
    assert_eq!(
        fact.execution_group_id.as_deref(),
        Some(INPUT_TOKENS_EXECUTION_GROUP)
    );
    assert_eq!(fact.attempt, 3);
    assert_eq!(fact.account_id, INPUT_TOKENS_ACCOUNT_ID);
    assert_eq!(fact.key_id, INPUT_TOKENS_KEY_ID);
    assert_ne!(fact.key_id, INPUT_TOKENS_RAW_KEY);
    assert_eq!(fact.provider_plane, "openai");
    assert_eq!(fact.route_class, "native");
    assert_eq!(fact.request_class, "input_tokens");
    assert!(!fact.stream_flag);
    assert_eq!(
        fact.terminal.http_status_code,
        Some(i32::from(expected_status.as_u16()))
    );
    assert_eq!(fact.terminal.internal_attempt_count, Some(0));
    assert_eq!(fact.terminal.upstream_request_id, None);
    assert_eq!(fact.terminal.downstream_disconnect, None);
    assert_eq!(fact.terminal.failure_class, None);
    assert_eq!(fact.terminal.tool_calls_in_output, None);
    assert_eq!(
        fact.terminal.provider_terminal_class,
        if expected_status.is_success() {
            registry::request_facts::ProviderTerminalClass::Success
        } else {
            registry::request_facts::ProviderTerminalClass::ClientError
        }
    );
    assert_eq!(
        fact.terminal.delivery_state,
        if expected_status.is_success() {
            registry::request_facts::DeliveryState::Completed
        } else {
            registry::request_facts::DeliveryState::NotStarted
        }
    );
}

#[tokio::test]
async fn input_tokens_submits_one_content_free_fact_after_owning_parse() {
    let (sender, mut receiver) = mpsc::channel(2);
    let test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
    let clock = crate::execution::RequestLifecycleClock::default();
    clock.observe_first_public_byte();
    let expected_first_public_byte_at = clock.first_public_byte_at();
    let response = call_input_tokens(
        test.app.clone(),
        input_tokens_json_request(
            &valid_input_tokens_body(),
            Some(INPUT_TOKENS_RAW_KEY),
            Some(INPUT_TOKENS_LOGICAL_ID),
            Some("opencode/1.2.3"),
            Some(3),
            Some(clock),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let fact = receiver.try_recv().expect("one terminal request fact");
    assert!(receiver.try_recv().is_err(), "exactly one submission");
    assert_native_input_terminal(&fact, StatusCode::OK);
    assert_eq!(
        fact.client_kind,
        registry::request_facts::ClientKind::OpenCode
    );
    assert_eq!(
        fact.client_source,
        registry::request_facts::ClientSource::Explicit
    );
    assert_eq!(fact.client_version.as_deref(), Some("1.2.3"));
    assert_eq!(fact.requested_model.as_deref(), Some("openai/gpt-5.6"));
    assert_eq!(fact.executable_model.as_deref(), Some("gpt-5.6"));
    assert_eq!(fact.tools_declared_count, Some(1));
    assert_eq!(
        fact.tool_classes,
        Some(registry::request_facts::TOOL_CLASS_CUSTOM_FUNCTION)
    );
    assert_eq!(
        fact.tool_choice_mode,
        Some(registry::request_facts::ToolChoiceMode::Required)
    );
    assert_eq!(fact.parallel_tools_requested, Some(false));
    assert_eq!(fact.tool_results_in_input, Some(false));
    assert_eq!(fact.structured_output_flag, Some(true));
    assert_eq!(fact.reasoning_flag, Some(true));
    assert_eq!(fact.service_tier.as_deref(), Some("priority"));
    assert_eq!(
        fact.input_modalities,
        Some(registry::request_facts::MODALITY_TEXT)
    );
    assert_eq!(fact.output_modalities, None);
    assert_eq!(
        fact.terminal.first_public_byte_at,
        expected_first_public_byte_at
    );
    let debug = format!("{fact:?}");
    for private in [
        "private prompt marker",
        "private_tool_name",
        "private_schema_name",
        "secret-never-a-fact",
    ] {
        assert!(!debug.contains(private), "fact Debug leaked {private:?}");
    }
}

#[tokio::test]
async fn input_tokens_parser_gate_discards_models_and_classifier_on_rejection() {
    let (sender, mut receiver) = mpsc::channel(1);
    let test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
    let mut oversized = input_tokens_request(
        b"x".to_vec(),
        Some(INPUT_TOKENS_RAW_KEY),
        Some(INPUT_TOKENS_LOGICAL_ID),
        None,
        Some(3),
        Some(crate::execution::RequestLifecycleClock::default()),
    );
    oversized.headers_mut().insert(
        axum::http::header::CONTENT_LENGTH,
        (OPENAI_BODY_LIMIT as u64 + 1)
            .to_string()
            .parse()
            .unwrap(),
    );
    let response = call_input_tokens(test.app.clone(), oversized).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let fact = receiver.try_recv().expect("post-admission error fact");
    assert_native_input_terminal(&fact, StatusCode::BAD_REQUEST);
    assert_eq!(fact.requested_model, None);
    assert_eq!(fact.executable_model, None);

    let peer_cases = [
        (b"{".to_vec(), StatusCode::BAD_REQUEST),
        (
            serde_json::to_vec(&json!({
                "model": "openai/gpt-5.6",
                "input": "private rejected content",
                "tools": [{"type": "function", "name": "private_rejected_tool"}],
                "parallel_tool_calls": "not-a-boolean"
            }))
            .unwrap(),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (body, expected_status) in peer_cases {
        let (sender, mut receiver) = mpsc::channel(1);
        let test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
        let response = call_input_tokens(
            test.app.clone(),
            input_tokens_request(
                body,
                Some(INPUT_TOKENS_RAW_KEY),
                Some(INPUT_TOKENS_LOGICAL_ID),
                None,
                Some(3),
                Some(crate::execution::RequestLifecycleClock::default()),
            ),
        )
        .await;
        assert_eq!(response.status(), expected_status);
        let fact = receiver.try_recv().expect("post-admission error fact");
        assert_native_input_terminal(&fact, expected_status);
        assert_eq!(fact.requested_model, None);
        assert_eq!(fact.executable_model, None);
        assert_eq!(fact.tools_declared_count, None);
        assert_eq!(fact.tool_classes, None);
        assert_eq!(fact.tool_choice_mode, None);
        assert_eq!(fact.parallel_tools_requested, None);
        assert_eq!(fact.tool_results_in_input, None);
        assert_eq!(fact.structured_output_flag, None);
        assert_eq!(fact.reasoning_flag, None);
        assert_eq!(fact.service_tier, None);
        assert_eq!(fact.input_modalities, None);
        assert_eq!(fact.output_modalities, None);
        assert!(receiver.try_recv().is_err(), "exactly one submission");
    }
}

#[test]
fn input_tokens_fact_model_bound_omits_overlong_or_unsafe_values() {
    let maximum = "m".repeat(registry::request_facts::MAX_REQUEST_FACT_MODEL_LEN);
    assert_eq!(
        bounded_request_fact_model(&maximum).as_deref(),
        Some(maximum.as_str())
    );
    assert_eq!(
        bounded_request_fact_model(&format!("{maximum}x")),
        None,
        "overlong requested model must stay unknown"
    );
    assert_eq!(bounded_request_fact_model("unsafe\nmodel"), None);
    assert_eq!(bounded_request_fact_model(""), None);
}

#[tokio::test]
async fn input_tokens_prepare_failure_retains_parser_accepted_evidence() {
    let (sender, mut receiver) = mpsc::channel(1);
    let test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
    let mut body = valid_input_tokens_body();
    body["previous_response_id"] = json!("resp_missing");
    let response = call_input_tokens(
        test.app.clone(),
        input_tokens_json_request(
            &body,
            Some(INPUT_TOKENS_RAW_KEY),
            Some(INPUT_TOKENS_LOGICAL_ID),
            Some("claude_code/2.1.220"),
            Some(3),
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let fact = receiver.try_recv().expect("prepare failure fact");
    assert_native_input_terminal(&fact, StatusCode::BAD_REQUEST);
    assert_eq!(fact.requested_model.as_deref(), Some("openai/gpt-5.6"));
    assert_eq!(fact.executable_model.as_deref(), Some("gpt-5.6"));
    assert_eq!(fact.tools_declared_count, Some(1));
    assert_eq!(fact.structured_output_flag, Some(true));
    assert_eq!(fact.reasoning_flag, Some(true));
    assert_eq!(fact.service_tier.as_deref(), Some("priority"));
    assert_eq!(
        fact.client_kind,
        registry::request_facts::ClientKind::ClaudeCode
    );
}

#[tokio::test]
async fn input_tokens_omits_unowned_context_and_normalizes_missing_client() {
    for (logical_id, lifecycle_clock) in [
        (
            None,
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
        (Some(INPUT_TOKENS_LOGICAL_ID), None),
    ] {
        let (sender, mut receiver) = mpsc::channel(1);
        let test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
        let response = call_input_tokens(
            test.app.clone(),
            input_tokens_json_request(
                &valid_input_tokens_body(),
                Some(INPUT_TOKENS_RAW_KEY),
                logical_id,
                None,
                None,
                lifecycle_clock,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(receiver.try_recv().is_err(), "missing context emitted fact");
    }

    let (sender, mut receiver) = mpsc::channel(1);
    let test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
    let response = call_input_tokens(
        test.app.clone(),
        input_tokens_json_request(
            &valid_input_tokens_body(),
            Some(INPUT_TOKENS_RAW_KEY),
            Some(INPUT_TOKENS_LOGICAL_ID),
            None,
            None,
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let fact = receiver.try_recv().expect("missing client stays unknown");
    assert_eq!(
        fact.client_kind,
        registry::request_facts::ClientKind::Unknown
    );
    assert_eq!(
        fact.client_source,
        registry::request_facts::ClientSource::Unknown
    );
    assert_eq!(fact.client_version, None);
}

#[tokio::test]
async fn input_tokens_unauthorized_and_admin_traffic_emit_no_fact() {
    let (sender, mut receiver) = mpsc::channel(2);
    let mut test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
    Arc::get_mut(&mut test.app.cfg)
        .expect("test owns config")
        .api_keys
        .push("native-input-admin".into());
    for (key, expected_status) in [
        ("unknown", StatusCode::UNAUTHORIZED),
        ("native-input-admin", StatusCode::OK),
    ] {
        let response = call_input_tokens(
            test.app.clone(),
            input_tokens_json_request(
                &valid_input_tokens_body(),
                Some(key),
                Some(INPUT_TOKENS_LOGICAL_ID),
                None,
                None,
                Some(crate::execution::RequestLifecycleClock::default()),
            ),
        )
        .await;
        assert_eq!(response.status(), expected_status);
    }
    assert!(receiver.try_recv().is_err(), "excluded auth emitted fact");
}

#[tokio::test]
async fn input_tokens_fixed_and_combined_leaf_each_emit_exactly_one_fact() {
    for provider in [ProviderMode::OpenAi, ProviderMode::Combined] {
        let (sender, mut receiver) = mpsc::channel(2);
        let test = input_tokens_test_app(true, Some(sender), provider).await;
        let response = call_input_tokens(
            test.app.clone(),
            input_tokens_json_request(
                &valid_input_tokens_body(),
                Some(INPUT_TOKENS_RAW_KEY),
                Some(INPUT_TOKENS_LOGICAL_ID),
                None,
                None,
                Some(crate::execution::RequestLifecycleClock::default()),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(receiver.try_recv().is_ok(), "leaf omitted fact");
        assert!(receiver.try_recv().is_err(), "leaf emitted duplicate fact");
    }
}

#[tokio::test]
async fn input_tokens_fail_open_delivery_paths_preserve_exact_response() {
    let body = valid_input_tokens_body();
    let baseline = input_tokens_test_app(true, None, ProviderMode::OpenAi).await;
    let baseline = call_input_tokens(
        baseline.app.clone(),
        input_tokens_json_request(
            &body,
            Some(INPUT_TOKENS_RAW_KEY),
            None,
            None,
            None,
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
    )
    .await;
    let baseline = input_tokens_response_snapshot(baseline).await;

    let (full_sender, _full_receiver) = mpsc::channel(1);
    full_sender
        .try_send(
            super::super::billing::CodexRequestFactSeed::for_test(
                INPUT_TOKENS_LOGICAL_ID,
                crate::execution::ClientAttribution::unknown_for_internal_use(),
                registry::ExecutionAttempt::direct(),
                INPUT_TOKENS_ACCOUNT_ID,
                INPUT_TOKENS_KEY_ID,
                pool::now(),
                crate::execution::RequestLifecycleClock::default(),
            )
            .terminal_input_tokens_fact(StatusCode::OK, None, None, None),
        )
        .unwrap();
    let full = input_tokens_test_app(true, Some(full_sender), ProviderMode::OpenAi).await;
    let full = call_input_tokens(
        full.app.clone(),
        input_tokens_json_request(
            &body,
            Some(INPUT_TOKENS_RAW_KEY),
            Some(INPUT_TOKENS_LOGICAL_ID),
            None,
            None,
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
    )
    .await;
    let full = input_tokens_response_snapshot(full).await;

    let (closed_sender, closed_receiver) = mpsc::channel(1);
    drop(closed_receiver);
    let closed = input_tokens_test_app(true, Some(closed_sender), ProviderMode::OpenAi).await;
    let closed = call_input_tokens(
        closed.app.clone(),
        input_tokens_json_request(
            &body,
            Some(INPUT_TOKENS_RAW_KEY),
            Some(INPUT_TOKENS_LOGICAL_ID),
            None,
            None,
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
    )
    .await;
    let closed = input_tokens_response_snapshot(closed).await;

    assert_eq!(baseline, full, "queue-full changed public response");
    assert_eq!(baseline, closed, "writer-closed changed public response");
}

#[tokio::test]
async fn input_tokens_sqlite_unsupported_preserves_response() {
    let body = valid_input_tokens_body();
    let baseline = input_tokens_test_app(true, None, ProviderMode::OpenAi).await;
    let baseline = call_input_tokens(
        baseline.app.clone(),
        input_tokens_json_request(
            &body,
            Some(INPUT_TOKENS_RAW_KEY),
            None,
            None,
            None,
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
    )
    .await;
    let baseline = input_tokens_response_snapshot(baseline).await;

    let observed = input_tokens_test_app(true, None, ProviderMode::OpenAi).await;
    let billing = Arc::clone(observed.app.billing.as_ref().unwrap());
    let response = call_input_tokens(
        observed.app.clone(),
        input_tokens_json_request(
            &body,
            Some(INPUT_TOKENS_RAW_KEY),
            Some(INPUT_TOKENS_LOGICAL_ID),
            None,
            None,
            Some(crate::execution::RequestLifecycleClock::default()),
        ),
    )
    .await;
    let response = input_tokens_response_snapshot(response).await;
    assert_eq!(baseline, response);
    assert_eq!(
        billing.request_fact_delivery_snapshot().dropped_unsupported,
        1
    );
}

#[test]
fn input_tokens_terminal_fact_persists_privacy_bounded_postgres_row() {
    const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping native input_tokens fact row: test URL is unset");
        return;
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let instance_id = format!("native-input-fact-{}-{unique}", std::process::id());
    let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock_holder
        .batch_execute(
            "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations, \
             capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    pg.account_create(INPUT_TOKENS_ACCOUNT_ID, None, 10_000)
        .unwrap();
    pg.account_topup(
        INPUT_TOKENS_ACCOUNT_ID,
        1_000,
        Some("native-input-fact-seed"),
    )
    .unwrap();
    pg.key_issue(INPUT_TOKENS_RAW_KEY, INPUT_TOKENS_ACCOUNT_ID, None)
        .unwrap();
    let key_id = pg.key_get(INPUT_TOKENS_RAW_KEY).unwrap().unwrap().key_id;
    let owner = pg.claim_instance(&instance_id, 600).unwrap();
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
    let cfg = input_tokens_proxy_config();
    let gateway_path = input_tokens_unique_path("pg-gateway");
    let app = AppState {
        provider: ProviderMode::OpenAi,
        authority: Arc::new(registry::authority::AuthorityConfig::Postgres { url: url.clone() }),
        data_db_path: Arc::new(gateway_path.to_string_lossy().into_owned()),
        pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: Some(codex_test_body_storage("responses")),
        codex: Some(Arc::new(gateway())),
        gemini: None,
        gemini_batch: None,
        gemini_batch_runtime: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing: Some(Arc::clone(&billing)),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(1)),
        metrics: Arc::new(Metrics::new()),
        probe_poke: None,
        admin_changes: tokio::sync::broadcast::channel(16).0,
        cfg,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let response = call_input_tokens(
                app,
                input_tokens_json_request(
                    &valid_input_tokens_body(),
                    Some(INPUT_TOKENS_RAW_KEY),
                    Some(INPUT_TOKENS_LOGICAL_ID),
                    Some("claude_code/2.1.220"),
                    Some(3),
                    Some(crate::execution::RequestLifecycleClock::default()),
                ),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let row = loop {
        if let Some(row) = lock_holder
            .query_opt(
                "SELECT logical_request_id,billing_request_id,execution_group_id,attempt, \
                        account_id,key_id,client_kind,client_source,client_version,provider_plane, \
                        route_class,request_class,requested_model,executable_model,stream_flag, \
                        tools_declared_count,tool_classes,tool_choice_mode,parallel_tools_requested, \
                        tool_results_in_input,structured_output_flag,reasoning_flag,service_tier, \
                        input_modalities,output_modalities,http_status_code,provider_terminal_class, \
                        delivery_state,billing_outcome,downstream_disconnect,upstream_request_id, \
                        first_public_byte_at,internal_attempt_count,failure_class,tool_calls_in_output \
                   FROM request_facts WHERE logical_request_id=$1",
                &[&INPUT_TOKENS_LOGICAL_ID],
            )
            .unwrap()
        {
            break row;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native input_tokens fact was not persisted"
        );
        std::thread::yield_now();
    };
    assert_eq!(row.get::<_, String>(0), INPUT_TOKENS_LOGICAL_ID);
    assert_eq!(row.get::<_, Option<String>>(1), None);
    assert_eq!(
        row.get::<_, Option<String>>(2).as_deref(),
        Some(INPUT_TOKENS_EXECUTION_GROUP)
    );
    assert_eq!(row.get::<_, i32>(3), 3);
    assert_eq!(row.get::<_, String>(4), INPUT_TOKENS_ACCOUNT_ID);
    assert_eq!(row.get::<_, String>(5), key_id);
    assert_ne!(row.get::<_, String>(5), INPUT_TOKENS_RAW_KEY);
    assert_eq!(row.get::<_, String>(6), "claude_code");
    assert_eq!(row.get::<_, String>(7), "explicit");
    assert_eq!(row.get::<_, Option<String>>(8).as_deref(), Some("2.1.220"));
    assert_eq!(row.get::<_, String>(9), "openai");
    assert_eq!(row.get::<_, String>(10), "native");
    assert_eq!(row.get::<_, String>(11), "input_tokens");
    assert_eq!(
        row.get::<_, Option<String>>(12).as_deref(),
        Some("openai/gpt-5.6")
    );
    assert_eq!(row.get::<_, Option<String>>(13).as_deref(), Some("gpt-5.6"));
    assert!(!row.get::<_, bool>(14));
    assert_eq!(row.get::<_, Option<i32>>(15), Some(1));
    assert_eq!(
        row.get::<_, Option<i32>>(16),
        Some(registry::request_facts::TOOL_CLASS_CUSTOM_FUNCTION)
    );
    assert_eq!(
        row.get::<_, Option<String>>(17).as_deref(),
        Some("required")
    );
    assert_eq!(row.get::<_, Option<bool>>(18), Some(false));
    assert_eq!(row.get::<_, Option<bool>>(19), Some(false));
    assert_eq!(row.get::<_, Option<bool>>(20), Some(true));
    assert_eq!(row.get::<_, Option<bool>>(21), Some(true));
    assert_eq!(
        row.get::<_, Option<String>>(22).as_deref(),
        Some("priority")
    );
    assert_eq!(
        row.get::<_, Option<i32>>(23),
        Some(registry::request_facts::MODALITY_TEXT)
    );
    assert_eq!(row.get::<_, Option<i32>>(24), None);
    assert_eq!(row.get::<_, Option<i32>>(25), Some(200));
    assert_eq!(row.get::<_, String>(26), "success");
    assert_eq!(row.get::<_, String>(27), "completed");
    assert_eq!(row.get::<_, String>(28), "not_applicable");
    assert_eq!(row.get::<_, Option<bool>>(29), None);
    assert_eq!(row.get::<_, Option<String>>(30), None);
    assert_eq!(row.get::<_, Option<i64>>(31), None);
    assert_eq!(row.get::<_, Option<i32>>(32), Some(0));
    assert_eq!(row.get::<_, Option<String>>(33), None);
    assert_eq!(row.get::<_, Option<bool>>(34), None);
    let row_debug = format!("{row:?}");
    for private in [
        "private prompt marker",
        "private_tool_name",
        "private_schema_name",
        "secret-never-a-fact",
    ] {
        assert!(
            !row_debug.contains(private),
            "PostgreSQL row leaked {private:?}"
        );
    }
    lock_holder
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    drop(billing);
    let _ = std::fs::remove_file(gateway_path);
}

#[tokio::test]
async fn input_tokens_terminal_seal_rejects_late_outer_observation() {
    let (sender, mut receiver) = mpsc::channel(1);
    let test = input_tokens_test_app(true, Some(sender), ProviderMode::OpenAi).await;
    let clock = crate::execution::RequestLifecycleClock::default();
    let response = call_input_tokens(
        test.app.clone(),
        input_tokens_json_request(
            &valid_input_tokens_body(),
            Some(INPUT_TOKENS_RAW_KEY),
            Some(INPUT_TOKENS_LOGICAL_ID),
            None,
            None,
            Some(clock.clone()),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let fact = receiver.try_recv().expect("one fact");
    assert_eq!(fact.terminal.first_public_byte_at, None);
    clock.observe_first_public_byte();
    assert_eq!(
        clock.first_public_byte_at(),
        None,
        "late observation won seal"
    );
}

#[test]
fn string_input_becomes_one_user_turn_without_duplicate_injection() {
    let normalized = normalize_responses_input(&json!("hello")).unwrap();
    assert_eq!(normalized.canonical_items.len(), 1);
    assert!(normalized.prior_items.is_empty());
    assert_eq!(
        normalized.turn_input,
        vec![json!({"type": "text", "text": "hello"})]
    );
}

#[test]
fn full_history_injects_prefix_and_sends_only_final_user_message() {
    let normalized = normalize_responses_input(&json!([
        {"role": "user", "content": "one"},
        {"role": "assistant", "content": "two"},
        {"role": "user", "content": [{"type": "input_text", "text": "three"}]}
    ]))
    .unwrap();
    assert_eq!(normalized.canonical_items.len(), 3);
    assert_eq!(normalized.prior_items.len(), 2);
    assert_eq!(
        normalized.turn_input,
        vec![json!({"type": "text", "text": "three"})]
    );
}

#[test]
fn responses_system_history_is_preserved_as_backend_supported_developer_history() {
    let normalized = normalize_responses_input(&json!([
        {"role": "system", "content": "follow this policy"},
        {"role": "user", "content": "hello"}
    ]))
    .unwrap();
    assert_eq!(normalized.prior_items[0]["role"], "developer");
    assert_eq!(
        normalized.prior_items[0]["content"][0]["text"],
        "follow this policy"
    );
    assert_eq!(normalized.turn_input[0]["text"], "hello");
}

/// SDK-style echo `output → input` replays reasoning items without `encrypted_content` (the
/// gateway does not publish the key unless `include` asks for it). The backend cannot resolve such
/// an item and fails the whole turn (live probe 2026-08-18), so `prepare_turn` must keep it out of
/// the upstream body while leaving the canonical history untouched.
#[tokio::test]
async fn replayed_reasoning_without_encrypted_content_never_reaches_upstream() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {"role": "user", "content": "one"},
                {"type": "reasoning", "id": "rs_bare", "summary": [{"type": "summary_text", "text": "thought"}]},
                {"role": "assistant", "content": "two"},
                {"role": "user", "content": "three"}
            ]
        }),
    )
    .unwrap();
    let prepared = prepare_turn(&gateway(), "tenant", parsed).await.unwrap();
    // The upstream body carries no bare reasoning item; portable items stay in order.
    let injected_types: Vec<_> = prepared
        .turn
        .injected_items
        .iter()
        .map(|item| item["type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(injected_types, vec!["message", "message"]);
    assert!(prepared
        .turn
        .injected_items
        .iter()
        .all(|item| item.get("encrypted_content").is_none()));
    // The canonical history (what a later store=true chain would persist) still holds the item.
    assert!(prepared
        .full_history_prefix
        .iter()
        .any(|item| item["id"] == "rs_bare"));
}

/// A reasoning item that does carry its encrypted continuation key is replayable and must stay.
#[tokio::test]
async fn replayed_reasoning_with_encrypted_content_is_kept_upstream() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {"role": "user", "content": "one"},
                {"type": "reasoning", "id": "rs_keyed", "summary": [], "encrypted_content": "key"},
                {"role": "user", "content": "two"}
            ]
        }),
    )
    .unwrap();
    let prepared = prepare_turn(&gateway(), "tenant", parsed).await.unwrap();
    let reasoning = prepared
        .turn
        .injected_items
        .iter()
        .find(|item| item["type"] == "reasoning")
        .expect("keyed reasoning item must reach upstream");
    assert_eq!(reasoning["id"], "rs_keyed");
    assert_eq!(reasoning["encrypted_content"], "key");
}

/// Codex multi-agent collaboration (spawn_agent) replays inter-agent messages as
/// `agent_message` items on the next turn; the upstream Responses backend has no such type, so
/// the gateway must translate them into plain messages instead of hard-rejecting the turn.
#[tokio::test]
async fn codex_agent_message_history_roundtrips_as_plain_messages() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {"role": "user", "content": "Audit the relay fallback."},
                {
                    "type": "agent_message",
                    "id": "amsg_1",
                    "author": "/root/audit_llmsrelay_fallback",
                    "recipient": "/root",
                    "content": [{"type": "input_text", "text": "Message Type: FINAL_ANSWER\nPayload: done"}]
                },
                {
                    "type": "agent_message",
                    "id": "amsg_2",
                    "author": "/root",
                    "recipient": "/root/audit_llmsrelay_fallback",
                    "content": [{"type": "input_text", "text": "Retry the fallback probe."}]
                },
                {"role": "user", "content": "Continue."}
            ]
        }),
    )
    .unwrap();
    let prepared = prepare_turn(&gateway(), "tenant", parsed).await.unwrap();
    let inbound = &prepared.turn.injected_items[1];
    assert_eq!(inbound["type"], "message");
    assert_eq!(inbound["role"], "user");
    // The client-owned amsg_* id must not reach upstream: the backend resolves it as a replay
    // reference and fails the whole turn in-stream when it cannot (verified live 2026-08-14).
    assert!(inbound.get("id").is_none());
    let text = inbound["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("/root/audit_llmsrelay_fallback"));
    assert!(text.contains("FINAL_ANSWER"));
    let outbound = &prepared.turn.injected_items[2];
    assert_eq!(outbound["type"], "message");
    assert_eq!(outbound["role"], "assistant");
    assert!(outbound["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Retry the fallback probe."));
    // The new turn input stays exactly the final user message.
    assert_eq!(
        prepared.turn.turn_input,
        vec![json!({"type": "text", "text": "Continue."})]
    );
}

#[test]
fn agent_message_without_text_content_is_rejected() {
    let error = normalize_responses_input(&json!([
        {"type": "agent_message", "author": "/root/a", "recipient": "/root", "content": "not-an-array"},
        {"role": "user", "content": "hi"}
    ]))
    .unwrap_err();
    assert_eq!(error.param.as_deref(), Some("input.0.content"));
}

#[tokio::test]
async fn responses_instructions_replace_the_upstream_base_prompt() {
    let gateway = gateway();
    let parsed = parse_responses_request(
        &gateway,
        json!({
            "model": "gpt-5.6",
            "instructions": "Only the client's instruction.",
            "input": "hello"
        }),
    )
    .unwrap();
    let prepared = prepare_turn(&gateway, "tenant", parsed).await.unwrap();
    assert_eq!(
        prepared.turn.base_instructions.as_deref(),
        Some("Only the client's instruction.")
    );
    assert!(prepared.turn.developer_instructions.is_none());
}

#[test]
fn parser_ignores_fields_the_backend_cannot_honor() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "temperature": 0.2,
            "top_p": 0.5,
            "max_output_tokens": 512,
            "truncation": "auto",
            "user": "end-user",
            "background": true,
            "max_tool_calls": 3,
            "top_logprobs": 2,
            "service_tier": "flex",
            "stream_options": {"include_obfuscation": true},
            "some_future_field": {"anything": true}
        }),
    )
    .expect("parameters the transport cannot honor must be ignored, not rejected");
    assert_eq!(parsed.input.turn_input.len(), 1);
    assert!(parsed.service_tier.is_none());
    assert_eq!(parsed.max_output_tokens, Some(512));
    let response = response_object(&parsed, "resp_with_cap", 0, "in_progress", Vec::new(), None);
    assert_eq!(response["max_output_tokens"], 512);
}

#[test]
fn responses_output_limit_is_strict_but_null_remains_absent() {
    for value in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("512"),
        json!({}),
        serde_json::from_str("18446744073709551616").unwrap(),
    ] {
        let error = parse_responses_request(
            &gateway(),
            json!({"model": "gpt-5.6", "input": "hi", "max_output_tokens": value}),
        )
        .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.param.as_deref(), Some("max_output_tokens"));
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    let parsed = parse_responses_request(
        &gateway(),
        json!({"model": "gpt-5.6", "input": "hi", "max_output_tokens": null}),
    )
    .unwrap();
    assert_eq!(parsed.max_output_tokens, None);
}

#[test]
fn parser_normalizes_codex_fast_and_openai_priority_service_tiers() {
    for requested in ["fast", "priority"] {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "hi",
                "service_tier": requested
            }),
        )
        .unwrap();
        assert_eq!(parsed.service_tier.as_deref(), Some("priority"));
        let response = response_object(&parsed, "resp_fast", 0, "in_progress", Vec::new(), None);
        assert_eq!(response["service_tier"], "priority");
        let missing_effective_tier = build_completed_response(
            &parsed,
            &CodexTurnResult {
                output: Vec::new(),
                usage: CodexUsage::default(),
                effective_service_tier: None,
                provider_reported_service_tier: None,
            },
            "resp_fast",
            0,
        );
        assert_eq!(missing_effective_tier["service_tier"], "default");
        let accepted_fast_with_default_provider_report = build_completed_response(
            &parsed,
            &CodexTurnResult {
                output: Vec::new(),
                usage: CodexUsage::default(),
                effective_service_tier: Some("priority".to_string()),
                provider_reported_service_tier: Some("default".to_string()),
            },
            "resp_fast",
            0,
        );
        assert_eq!(
            accepted_fast_with_default_provider_report["service_tier"],
            "priority"
        );
    }
    for requested in ["default", "auto", "flex", "future-tier"] {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "hi",
                "service_tier": requested
            }),
        )
        .unwrap();
        assert_eq!(parsed.service_tier, None, "{requested}");
    }
}

#[test]
fn responses_accept_namespaced_openai_catalog_ids() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "openai/gpt-5.6",
            "input": "hi",
            "service_tier": "priority"
        }),
    )
    .expect("the OpenAI plane must resolve the namespace published by the router catalog");

    assert_eq!(parsed.public_model.id, "gpt-5.6");
    assert_eq!(parsed.service_tier.as_deref(), Some("priority"));
}

#[test]
fn unenforceable_tool_controls_degrade_instead_of_failing() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "parameters": {"type": "object"}
            }],
            "tool_choice": {"type": "function", "name": "get_weather"},
            "parallel_tool_calls": false
        }),
    )
    .expect("forced tool choice and parallel=false must degrade, not fail");
    assert_eq!(parsed.dynamic_tools.len(), 1);
    assert_eq!(parsed.tool_choice, json!("auto"));

    let required = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "parameters": {"type": "object"}
            }],
            "tool_choice": "required"
        }),
    )
    .expect("tool_choice=required must degrade to auto");
    assert_eq!(required.tool_choice, json!("auto"));
    assert_eq!(required.dynamic_tools.len(), 1);
}

#[test]
fn unsupported_reasoning_effort_degrades_to_model_default() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "reasoning": {"effort": "minimal", "summary": "verbose", "context": "last_turn"}
        }),
    )
    .expect("unsupported effort/summary must degrade, not fail");
    assert_eq!(parsed.reasoning_effort, None);
    assert_eq!(parsed.reasoning_summary, None);
}

#[test]
fn responses_input_image_parts_translate_to_turn_image_inputs() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "first"},
                        {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo=", "detail": "low"}
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "input_image", "image_url": "https://example.com/x.png"},
                        {"type": "input_text", "text": "second"}
                    ]
                }
            ]
        }),
    )
    .expect("input_image parts must translate");
    // First user message is history and keeps canonical Responses image parts.
    let history = &parsed.input.prior_items[0];
    assert_eq!(history["content"][1]["type"], "input_image");
    assert_eq!(history["content"][1]["detail"], "low");
    // Final user message becomes upstream image turn inputs.
    assert_eq!(parsed.input.turn_input[0]["type"], "image");
    assert_eq!(
        parsed.input.turn_input[0]["url"],
        "https://example.com/x.png"
    );
    assert_eq!(
        parsed.input.turn_input[1],
        json!({"type": "text", "text": "second"})
    );
}

#[test]
fn data_url_images_do_not_inflate_the_billing_estimate() {
    let mut value = json!({
        "input": [
            {"type": "image", "url": format!("data:image/png;base64,{}", "A".repeat(1_000_000))},
            {"type": "text", "text": "describe"}
        ]
    });
    sanitize_estimate_images(&mut value);
    let bytes = serde_json::to_vec(&value).unwrap().len();
    assert!(
        bytes < 16_000,
        "estimate must not carry raw base64: {bytes}"
    );
}

#[tokio::test]
async fn injected_history_keeps_data_url_images_verbatim() {
    let data_url = format!("data:image/png;base64,{}", "A".repeat(200_000));
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "first"},
                        {"type": "input_image", "image_url": data_url}
                    ]
                },
                {"role": "user", "content": [{"type": "input_text", "text": "second"}]}
            ]
        }),
    )
    .expect("data-url history image must parse");
    let prepared = prepare_turn(&gateway(), "tenant", parsed).await.unwrap();
    // The backend cannot decode the fixed-size estimate placeholder; injecting it would
    // surface codex's "image content omitted" text instead of the screenshot.
    let injected_image = prepared.turn.injected_items[0]["content"][1]["image_url"]
        .as_str()
        .expect("history image part must survive");
    assert_eq!(injected_image, data_url);
    // The billing reserve still sees the fixed-size placeholder, not the raw payload.
    assert!(
        prepared.estimated_input_tokens < 100_000,
        "estimate must not carry raw base64: {}",
        prepared.estimated_input_tokens
    );
}

#[test]
fn function_tools_translate_to_dynamic_tool_schema() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                },
                "strict": false
            }]
        }),
    )
    .unwrap();
    assert_eq!(parsed.dynamic_tools.len(), 1);
    assert_eq!(parsed.dynamic_tools[0]["type"], "function");
    assert_eq!(parsed.dynamic_tools[0]["name"], "get_weather");
    assert_eq!(
        parsed.dynamic_tools[0]["inputSchema"]["required"],
        json!(["city"])
    );
}

#[test]
fn codex_0146_top_level_tools_translate_current_client_forms() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "Reply briefly.",
            "tools": [
                {
                    "type": "function",
                    "name": "list_mcp_resources",
                    "description": "List resources",
                    "parameters": {"type": "object", "properties": {}},
                    "strict": false
                },
                {
                    "type": "function",
                    "name": "read_mcp_resource",
                    "description": "Read a resource",
                    "parameters": {
                        "type": "object",
                        "properties": {"uri": {"type": "string"}},
                        "required": ["uri"]
                    },
                    "strict": false
                },
                {
                    "type": "custom",
                    "name": "apply_patch",
                    "description": "Apply a patch",
                    "format": {
                        "type": "grammar",
                        "syntax": "lark",
                        "definition": "start: /[\\s\\S]+/"
                    }
                },
                {
                    "type": "function",
                    "name": "view_image",
                    "description": "View an image",
                    "parameters": {
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    },
                    "strict": false
                },
                {
                    "type": "tool_search",
                    "execution": "client",
                    "description": "Search available tools",
                    "parameters": {
                        "type": "object",
                        "properties": {"query": {"type": "string"}},
                        "required": ["query"]
                    }
                }
            ]
        }),
    )
    .expect("Codex 0.146 top-level client tools must parse");

    assert_eq!(parsed.original_tools.len(), 5);
    assert_eq!(parsed.dynamic_tools.len(), 5);
    assert_eq!(parsed.dynamic_tools[0]["type"], "function");
    assert_eq!(parsed.dynamic_tools[0]["name"], "list_mcp_resources");
    assert_eq!(parsed.dynamic_tools[2]["type"], "custom");
    assert_eq!(parsed.dynamic_tools[2]["name"], "apply_patch");
    assert_eq!(
        parsed.dynamic_tools[2]["format"]["definition"],
        "start: /[\\s\\S]+/"
    );
    assert_eq!(parsed.dynamic_tools[4]["type"], "function");
    assert_eq!(parsed.dynamic_tools[4]["name"], TOOL_SEARCH_DYNAMIC_NAME);
    assert_eq!(
        parsed.dynamic_tools[4]["inputSchema"]["required"],
        json!(["query"])
    );
}

#[test]
fn official_codex_cli_request_shape_translates_all_additional_tool_kinds() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {
                    "type": "additional_tools",
                    "role": "developer",
                    "tools": [
                        {
                            "type": "custom",
                            "name": "exec",
                            "description": "Run source",
                            "format": {
                                "type": "grammar",
                                "syntax": "lark",
                                "definition": "start: /[\\s\\S]+/"
                            }
                        },
                        {
                            "type": "function",
                            "name": "wait",
                            "description": "Wait for a task",
                            "parameters": {
                                "type": "object",
                                "properties": {"id": {"type": "string"}},
                                "required": ["id"]
                            },
                            "strict": false,
                            "defer_loading": true
                        },
                        {
                            "type": "namespace",
                            "name": "collaboration",
                            "description": "Agent coordination",
                            "tools": [{
                                "type": "function",
                                "name": "list_agents",
                                "description": "List agents",
                                "parameters": {
                                    "type": "object",
                                    "properties": {}
                                },
                                "strict": false
                            }]
                        },
                        {
                            "type": "tool_search",
                            "execution": "client",
                            "description": "Search available tools",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "query": {"type": "string"},
                                    "limit": {"type": "integer"}
                                },
                                "required": ["query"]
                            }
                        }
                    ]
                },
                {
                    "role": "developer",
                    "content": "Follow the caller's policy."
                },
                {
                    "role": "user",
                    "content": "Reply briefly."
                }
            ],
            "include": ["reasoning.encrypted_content"],
            "parallel_tool_calls": false,
            "tool_choice": "auto",
            "reasoning": {"effort": "low", "context": "all_turns"},
            "text": {"verbosity": "low"},
            "prompt_cache_key": "session-123",
            "client_metadata": {
                "session_id": "session-123",
                "turn_id": "turn-456",
                "x-codex-turn-metadata": "opaque"
            },
            "store": false,
            "stream": true
        }),
    )
    .unwrap();

    assert_eq!(parsed.prompt_cache_key.as_deref(), Some("session-123"));
    assert_eq!(parsed.verbosity.as_deref(), Some("low"));
    assert_eq!(parsed.reasoning_effort.as_deref(), Some("low"));
    assert!(!parsed.parallel_tool_calls);
    assert!(parsed.stream);
    assert_eq!(parsed.original_tools.len(), 4);
    assert_eq!(parsed.dynamic_tools.len(), 4);
    assert_eq!(parsed.dynamic_tools[0]["type"], "custom");
    assert_eq!(parsed.dynamic_tools[0]["name"], "exec");
    assert_eq!(
        parsed.dynamic_tools[0]["format"]["definition"],
        "start: /[\\s\\S]+/"
    );
    assert_eq!(
        parsed.dynamic_tools[1]["inputSchema"]["required"],
        json!(["id"])
    );
    assert_eq!(parsed.dynamic_tools[1]["deferLoading"], true);
    assert_eq!(parsed.dynamic_tools[2]["type"], "namespace");
    assert_eq!(parsed.dynamic_tools[2]["name"], "collaboration");
    assert_eq!(parsed.dynamic_tools[2]["tools"][0]["name"], "list_agents");
    assert_eq!(parsed.dynamic_tools[3]["type"], "function");
    assert_eq!(parsed.dynamic_tools[3]["name"], TOOL_SEARCH_DYNAMIC_NAME);
    assert_eq!(
        parsed.dynamic_tools[3]["inputSchema"]["required"],
        json!(["query"])
    );
    assert_eq!(parsed.input.canonical_items.len(), 2);
    assert_eq!(parsed.input.prior_items.len(), 1);
    assert_eq!(
        parsed.input.turn_input,
        vec![json!({"type": "text", "text": "Reply briefly."})]
    );
}

#[test]
fn codex_cli_0_147_namespaced_custom_tool_is_accepted() {
    // Captured from Codex CLI 0.147.0 against a gpt-5.6 model: the Lark `exec` tool that 0.146
    // sent as a sibling now lives INSIDE the `functions` namespace. Rejecting a non-function
    // namespace child failed every 0.147 turn before generation.
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {
                    "type": "additional_tools",
                    "role": "developer",
                    "tools": [
                        {
                            "type": "namespace",
                            "name": "functions",
                            "description": "Local execution tools",
                            "tools": [
                                {
                                    "type": "custom",
                                    "name": "exec",
                                    "description": "Run source",
                                    "format": {
                                        "type": "grammar",
                                        "syntax": "lark",
                                        "definition": "start: /[\\s\\S]+/"
                                    }
                                },
                                {
                                    "type": "function",
                                    "name": "wait",
                                    "description": "Wait for a task",
                                    "parameters": {
                                        "type": "object",
                                        "properties": {"id": {"type": "string"}},
                                        "required": ["id"]
                                    },
                                    "strict": false
                                }
                            ]
                        },
                        {
                            "type": "namespace",
                            "name": "collaboration",
                            "description": "Agent coordination",
                            "tools": [{
                                "type": "function",
                                "name": "list_agents",
                                "description": "List agents",
                                "parameters": {"type": "object", "properties": {}},
                                "strict": false
                            }]
                        }
                    ]
                },
                {"role": "user", "content": "Reply briefly."}
            ],
            "store": false,
            "stream": true
        }),
    )
    .unwrap();

    assert_eq!(parsed.dynamic_tools.len(), 2);
    assert_eq!(parsed.dynamic_tools[0]["type"], "namespace");
    assert_eq!(parsed.dynamic_tools[0]["name"], "functions");
    assert_eq!(parsed.dynamic_tools[0]["tools"][0]["type"], "custom");
    assert_eq!(parsed.dynamic_tools[0]["tools"][0]["name"], "exec");
    assert_eq!(
        parsed.dynamic_tools[0]["tools"][0]["format"]["definition"],
        "start: /[\\s\\S]+/"
    );
    assert_eq!(parsed.dynamic_tools[0]["tools"][1]["type"], "function");
    assert_eq!(
        parsed.dynamic_tools[0]["tools"][1]["inputSchema"]["required"],
        json!(["id"])
    );
    assert_eq!(parsed.dynamic_tools[1]["tools"][0]["name"], "list_agents");
}

#[test]
fn namespaced_hosted_tools_still_fail_closed() {
    let error = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "tools": [{
                "type": "namespace",
                "name": "functions",
                "tools": [{"type": "web_search"}]
            }]
        }),
    )
    .unwrap_err();
    assert_eq!(error.param.as_deref(), Some("tools.0.tools.0.type"));
}

#[test]
fn hosted_web_search_is_accepted_and_never_forwarded() {
    // Codex CLI ships web search in mode `cached` by default, so the descriptor is present in
    // every stock config. Rejecting the list made the models that carry it unusable; the tool is
    // accepted as undeliverable and dropped, never proxied as an unmetered hosted call.
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "weather?",
            "tools": [
                {
                    "type": "function",
                    "name": "get_weather",
                    "parameters": {"type": "object", "properties": {}}
                },
                {
                    "type": "web_search",
                    "external_web_access": false,
                    "search_content_types": ["text", "image"]
                }
            ]
        }),
    )
    .unwrap();

    // The client's declaration is echoed back verbatim, but only the callable tool is dispatched.
    assert_eq!(parsed.original_tools.len(), 2);
    assert_eq!(parsed.dynamic_tools.len(), 1);
    assert_eq!(parsed.dynamic_tools[0]["name"], "get_weather");
    assert!(!parsed
        .dynamic_tools
        .iter()
        .any(|tool| tool["type"] == "web_search"));
}

/// The caller's `client_metadata` never reaches upstream: the transport overwrites it with the
/// gateway's own wire identity. So no shape of it may fail a turn — an oversized
/// `x-codex-turn-metadata` (shipped by newer Codex CLI builds), embedded control characters, and
/// non-string values all parse and are simply ignored.
#[test]
fn codex_diagnostic_metadata_is_ignored() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "client_metadata": {
                "turn_id": 42,
                "x-codex-turn-metadata": "x".repeat(64 * 1024),
                "x-codex-window-id": "line\nbreak",
            }
        }),
    )
    .unwrap();
    assert_eq!(parsed.public_model.id, "gpt-5.6");
}

/// `safety_identifier` is pinned to null in the public response and never forwarded, and an
/// unusual `prompt_cache_key` is normalized by `bounded_cache_key` on the way upstream. Neither
/// may fail a turn on its shape.
#[test]
fn discarded_caller_identity_fields_never_fail_a_turn() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "safety_identifier": "x".repeat(4096),
            "prompt_cache_key": "session\n".repeat(64)
        }),
    )
    .unwrap();
    assert_eq!(
        parsed.prompt_cache_key.as_deref(),
        Some(&*"session\n".repeat(64))
    );

    let empty = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "prompt_cache_key": "   "
        }),
    )
    .unwrap();
    assert_eq!(empty.prompt_cache_key, None);
}

/// The `additional_tools` list (the gpt-5.6 family's channel for client tools) must reach the same
/// verdict as the top-level list for the same descriptor: unknown fields ignored, `strict` degraded
/// rather than rejected, dotted MCP-style names accepted.
#[test]
fn additional_tools_match_top_level_tool_leniency() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [{
                "type": "additional_tools",
                "role": "developer",
                "origin": "codex-cli-future-field",
                "tools": [{
                    "type": "function",
                    "name": "docs.search",
                    "strict": true,
                    "cache_control": {"type": "ephemeral"},
                    "parameters": {"type": "object", "properties": {}}
                }]
            }, {
                "role": "user",
                "content": "find the release notes"
            }]
        }),
    )
    .unwrap();
    assert!(parsed
        .dynamic_tools
        .iter()
        .any(|tool| tool["name"] == "docs.search"));
}

/// An unknown tool type is dropped like the hosted `web_search` descriptor instead of failing the
/// turn: it is never forwarded, so it can neither run nor bill, and the tools we do understand
/// still reach the model. Namespace children keep failing closed (asserted separately).
#[test]
fn unknown_top_level_tool_types_are_dropped_not_rejected() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "weather?",
            "tools": [
                {"type": "future_hosted_tool", "config": {"mode": "auto"}},
                {
                    "type": "function",
                    "name": "get_weather",
                    "parameters": {"type": "object", "properties": {}}
                }
            ]
        }),
    )
    .unwrap();
    assert_eq!(parsed.dynamic_tools.len(), 1);
    assert_eq!(parsed.dynamic_tools[0]["name"], "get_weather");
}

/// A history item type the gateway has no translation for is forwarded verbatim instead of failing
/// the turn — the Responses backend is the authority on what it accepts. Structural requirements
/// (an object carrying a type or a role) still hold, so genuinely malformed input fails locally.
#[test]
fn unknown_input_item_types_pass_through_verbatim() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {"type": "future_history_item", "id": "fh_1", "payload": {"note": "kept"}},
                {"role": "user", "content": "continue"}
            ]
        }),
    )
    .unwrap();
    assert_eq!(parsed.input.canonical_items.len(), 2);
    assert_eq!(
        parsed.input.canonical_items[0],
        json!({"type": "future_history_item", "id": "fh_1", "payload": {"note": "kept"}})
    );

    let untyped = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [{"id": "fh_2"}]
        }),
    )
    .unwrap_err();
    assert_eq!(untyped.param.as_deref(), Some("input.0.type"));
}

#[test]
fn strict_function_tools_are_silently_downgraded() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "parameters": {"type": "object"},
                "strict": true
            }]
        }),
    )
    .expect("strict=true must downgrade to a non-strict dynamic tool");
    assert_eq!(parsed.dynamic_tools.len(), 1);
    assert_eq!(parsed.dynamic_tools[0]["name"], "get_weather");
    assert!(parsed.dynamic_tools[0].get("strict").is_none());
}

#[test]
fn response_id_validation_matches_history_write_format() {
    assert!(valid_response_id("resp_abc123_XYZ"));
    assert!(!valid_response_id("chatcmpl_123"));
    assert!(!valid_response_id("resp_a b"));
    assert!(!valid_response_id(&format!("resp_{}", "a".repeat(200))));
}

#[tokio::test]
async fn stream_failure_emits_error_event_then_failed_response() {
    let (sender, mut receiver) = mpsc::channel(8);
    let gateway = gateway();
    let parsed =
        parse_responses_request(&gateway, json!({"model": "gpt-5.6", "input": "hi"})).unwrap();
    let prepared = prepare_turn(&gateway, "tenant", parsed).await.unwrap();
    emit_stream_failure(
        &sender,
        &prepared,
        "resp_x",
        42,
        7,
        Some("server_error"),
        "boom",
    )
    .await;
    drop(sender);
    let frames = std::iter::from_fn(|| receiver.try_recv().ok())
        .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 2);
    assert!(frames[0].starts_with("event: error\n"));
    assert!(frames[0].contains("\"code\":\"server_error\""));
    assert!(frames[1].starts_with("event: response.failed\n"));
    assert!(frames[1].contains("\"status\":\"failed\""));
    assert!(frames[1].contains("\"message\":\"boom\""));
}

#[test]
fn public_usage_preserves_cached_write_and_reasoning_details() {
    let usage = public_usage(&CodexUsage {
        input_tokens: 100,
        cached_input_tokens: 40,
        cache_write_input_tokens: 10,
        output_tokens: 20,
        reasoning_output_tokens: 12,
        total_tokens: 120,
    });
    assert_eq!(usage["input_tokens_details"]["cached_tokens"], 40);
    assert_eq!(usage["input_tokens_details"]["cache_write_tokens"], 10);
    assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], 12);
}

#[test]
fn structured_provider_limits_map_to_openai_style_errors() {
    let usage_error = ApiError::from(ProcessError::UsageLimitExceeded {
        retry_after: Some(123),
    });
    assert_eq!(usage_error.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(usage_error.code, Some("rate_limit_exceeded"));
    assert_eq!(usage_error.retry_after, Some(123));

    let context_error = ApiError::from(ProcessError::ContextWindowExceeded);
    assert_eq!(context_error.status, StatusCode::BAD_REQUEST);
    assert_eq!(context_error.code, Some("context_length_exceeded"));
    assert_eq!(context_error.param.as_deref(), Some("input"));
}

#[test]
fn output_normalization_strips_internal_passthrough_metadata() {
    let item = json!({
        "type": "message",
        "id": "msg_1",
        "role": "assistant",
        "content": [{"type": "output_text", "text": "hello"}],
        "internal_chat_message_metadata_passthrough": {"secret": true}
    });
    let output = normalize_output_item(&item).unwrap();
    assert_eq!(output["status"], "completed");
    assert_eq!(output["content"][0]["annotations"], json!([]));
    assert!(output
        .get("internal_chat_message_metadata_passthrough")
        .is_none());
}

#[test]
fn output_normalization_drops_raw_input_and_empty_message_items() {
    assert!(normalize_output_item(&json!({
        "type": "message",
        "id": "msg_user",
        "role": "user",
        "content": [{"type": "input_text", "text": "private request"}]
    }))
    .is_none());
    assert!(normalize_output_item(&json!({
        "type": "message",
        "id": "msg_empty",
        "role": "assistant",
        "content": []
    }))
    .is_none());
    assert!(normalize_output_item(&json!({
        "type": "message",
        "id": "msg_empty_text",
        "role": "assistant",
        "content": [{"type": "output_text", "text": ""}]
    }))
    .is_none());
    assert!(normalize_output_item(&json!({
        "type": "message",
        "id": "msg_non_output",
        "role": "assistant",
        "content": [{"type": "input_text", "text": "not model output"}]
    }))
    .is_none());
}

#[test]
fn public_reasoning_hides_raw_chain_of_thought_and_gates_encrypted_content() {
    let item = json!({
        "type": "reasoning",
        "id": "rs_1",
        "summary": [{
            "type": "summary_text",
            "text": "Checked the inputs.",
            "internal_provider_metadata": "must not escape"
        }],
        "content": [{"type": "reasoning_text", "text": "private chain of thought"}],
        "encrypted_content": "ciphertext"
    });
    let default_output = normalize_output_item(&item).unwrap();
    assert_eq!(default_output["summary"][0]["text"], "Checked the inputs.");
    assert!(default_output["summary"][0]
        .get("internal_provider_metadata")
        .is_none());
    assert!(default_output.get("content").is_none());
    assert!(default_output.get("encrypted_content").is_none());

    let included_output = normalize_output_item_with_options(&item, true).unwrap();
    assert_eq!(included_output["encrypted_content"], "ciphertext");
    assert!(included_output.get("content").is_none());
}

#[test]
fn function_call_normalization_always_returns_public_string_fields() {
    let output = normalize_output_item(&json!({
        "type": "function_call",
        "id": "fc_1",
        "call_id": "call_1",
        "name": "lookup",
        "arguments": {"query": "safe"},
        "internal_provider_metadata": {"must": "not escape"}
    }))
    .unwrap();
    assert_eq!(output["arguments"], r#"{"query":"safe"}"#);
    assert!(output.get("internal_provider_metadata").is_none());
}

#[test]
fn internal_tool_search_function_normalizes_to_codex_0146_wire_item() {
    let output = normalize_output_item(&json!({
        "type": "function_call",
        "id": "fc_search",
        "call_id": "call_search",
        "name": TOOL_SEARCH_DYNAMIC_NAME,
        "arguments": "{\"query\":\"calendar create\",\"limit\":2}"
    }))
    .unwrap();
    assert_eq!(output["type"], "tool_search_call");
    assert_eq!(output["execution"], "client");
    assert_eq!(output["call_id"], "call_search");
    assert_eq!(output["arguments"]["query"], "calendar create");
    assert_eq!(output["arguments"]["limit"], 2);
    assert!(output.get("name").is_none());
}

#[tokio::test]
async fn codex_tool_search_history_roundtrips_through_pinned_client() {
    let parsed = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": [
                {
                    "type": "tool_search_call",
                    "id": "tsc_1",
                    "call_id": "call_search",
                    "status": "completed",
                    "execution": "client",
                    "arguments": {"query": "calendar create", "limit": 2}
                },
                {
                    "type": "tool_search_output",
                    "call_id": "call_search",
                    "status": "completed",
                    "execution": "client",
                    "tools": [{
                        "type": "function",
                        "name": "create_event",
                        "description": "Create a calendar event",
                        "parameters": {"type": "object"}
                    }]
                },
                {
                    "role": "user",
                    "content": "Continue."
                }
            ]
        }),
    )
    .unwrap();
    assert_eq!(parsed.input.canonical_items[0]["type"], "tool_search_call");
    assert_eq!(
        parsed.input.canonical_items[1]["type"],
        "tool_search_output"
    );

    let prepared = prepare_turn(&gateway(), "tenant", parsed).await.unwrap();
    assert_eq!(prepared.turn.injected_items[0]["type"], "function_call");
    assert_eq!(
        prepared.turn.injected_items[0]["name"],
        TOOL_SEARCH_DYNAMIC_NAME
    );
    assert_eq!(
        prepared.turn.injected_items[0]["arguments"],
        r#"{"limit":2,"query":"calendar create"}"#
    );
    assert_eq!(
        prepared.turn.injected_items[1]["type"],
        "function_call_output"
    );
    assert_eq!(prepared.turn.injected_items[1]["call_id"], "call_search");
    let output: Value =
        serde_json::from_str(prepared.turn.injected_items[1]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["execution"], "client");
    assert_eq!(output["tools"][0]["name"], "create_event");
    assert_eq!(prepared.full_history_prefix[0]["type"], "tool_search_call");
    assert_eq!(
        prepared.full_history_prefix[1]["type"],
        "tool_search_output"
    );
}

#[test]
fn custom_tool_call_normalization_preserves_only_public_fields() {
    let output = normalize_output_item(&json!({
        "type": "custom_tool_call",
        "id": "ctc_1",
        "call_id": "call_1",
        "name": "exec",
        "input": "text('ok')",
        "internal_provider_metadata": {"must": "not escape"}
    }))
    .unwrap();
    assert_eq!(output["type"], "custom_tool_call");
    assert_eq!(output["input"], "text('ok')");
    assert!(output.get("internal_provider_metadata").is_none());
}

#[test]
fn reasoning_encrypted_content_requires_explicit_include() {
    let default_request =
        parse_responses_request(&gateway(), json!({"model": "gpt-5.6", "input": "hi"})).unwrap();
    assert!(!default_request.include_encrypted_reasoning);

    let included_request = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "include": ["reasoning.encrypted_content"]
        }),
    )
    .unwrap();
    assert!(included_request.include_encrypted_reasoning);
}

#[tokio::test]
async fn reasoning_completion_events_use_authoritative_summary_text() {
    let (sender, mut receiver) = mpsc::channel(8);
    let mut sequence = 0;
    emit_reasoning_item_added(&sender, &mut sequence, 0, "rs_1").await;
    emit_reasoning_summary_part_added(&sender, &mut sequence, 0, "rs_1", 0).await;
    let state = StreamReasoningState {
        output_index: 0,
        parts: BTreeMap::from([(0, "partial".to_string())]),
    };
    emit_completed_reasoning_item(
        &sender,
        &mut sequence,
        "rs_1",
        &state,
        &json!({
            "id": "rs_1",
            "type": "reasoning",
            "status": "completed",
            "summary": [{"type": "summary_text", "text": "final"}]
        }),
    )
    .await;
    drop(sender);

    let frames = std::iter::from_fn(|| receiver.try_recv().ok())
        .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 5);
    assert!(frames[0].starts_with("event: response.output_item.added\n"));
    assert!(frames[1].starts_with("event: response.reasoning_summary_part.added\n"));
    assert!(frames[2].contains("\"type\":\"response.reasoning_summary_text.done\""));
    assert!(frames[2].contains("\"text\":\"final\""));
    assert!(frames[3].contains("\"type\":\"response.reasoning_summary_part.done\""));
    assert!(frames[4].starts_with("event: response.output_item.done\n"));
}

#[tokio::test]
async fn custom_tool_call_emits_the_responses_stream_lifecycle() {
    let (sender, mut receiver) = mpsc::channel(8);
    let mut sequence = 0;
    assert!(
        emit_completed_item(
            &sender,
            &mut sequence,
            0,
            &json!({
                "id": "ctc_1",
                "type": "custom_tool_call",
                "status": "completed",
                "call_id": "call_1",
                "name": "exec",
                "input": "text('ok')"
            }),
        )
        .await
    );
    drop(sender);

    let frames = std::iter::from_fn(|| receiver.try_recv().ok())
        .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(frames.len(), 4);
    assert!(frames[0].starts_with("event: response.output_item.added\n"));
    assert!(frames[0].contains("\"input\":\"\""));
    assert!(frames[1].starts_with("event: response.custom_tool_call_input.delta\n"));
    assert!(frames[1].contains("\"delta\":\"text('ok')\""));
    assert!(frames[2].starts_with("event: response.custom_tool_call_input.done\n"));
    assert!(frames[2].contains("\"input\":\"text('ok')\""));
    assert!(frames[3].starts_with("event: response.output_item.done\n"));
}

#[tokio::test]
async fn sse_send_stops_immediately_after_downstream_disconnect() {
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    assert!(!send_sse(&sender, "response.test", json!({"type": "response.test"})).await);
}

async fn generation_mock_upstream() -> (String, Arc<AtomicU64>) {
    mock_upstream_serving(concat!(
        "event: response.output_item.done\n",
        "data: {\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",",
        "\"call_id\":\"call_1\",\"name\":\"private_tool_name\",",
        "\"arguments\":\"{\\\"private_argument\\\":\\\"secret\\\"}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"service_tier\":\"default\",\"usage\":{",
        "\"input_tokens\":100,\"input_tokens_details\":{\"cached_tokens\":20,",
        "\"cache_write_tokens\":10},\"output_tokens\":20,",
        "\"output_tokens_details\":{\"reasoning_tokens\":5},",
        "\"total_tokens\":120}}}\n\n"
    ))
    .await
}

/// One-shot Responses upstream that replays a fixed SSE turn. The body is a parameter so a test
/// can pin the provider's own item ordering, not just its content.
async fn mock_upstream_serving(body: &'static str) -> (String, Arc<AtomicU64>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let sends = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&sends);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let observed = Arc::clone(&observed);
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                let header_end = loop {
                    let read = socket.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    if let Some(offset) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        break offset + 4;
                    }
                };
                let head = String::from_utf8_lossy(&request[..header_end]);
                assert!(head.starts_with("POST /responses HTTP/1.1"));
                let content_length = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let read = socket.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..read]);
                }
                observed.fetch_add(1, Ordering::Relaxed);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    (format!("http://{address}"), sends)
}

fn generation_request(
    body: Value,
    key: &str,
    logical_id: Option<&str>,
    with_lifecycle: bool,
) -> axum::extract::Request {
    let mut request = axum::extract::Request::builder()
        .header("x-api-key", key)
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    if let Some(logical_id) = logical_id {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            crate::execution::LOGICAL_REQUEST_ID_HEADER,
            logical_id.parse().unwrap(),
        );
        request
            .extensions_mut()
            .insert(crate::execution::admit_logical_request_id(&mut headers).unwrap());
    }
    request.extensions_mut().insert({
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            crate::execution::CLIENT_ATTRIBUTION_HEADER,
            "opencode/1.2.3".parse().unwrap(),
        );
        crate::execution::admit_client_attribution(&mut headers)
    });
    if with_lifecycle {
        request
            .extensions_mut()
            .insert(crate::execution::RequestLifecycleClock::default());
    }
    request
}

async fn invoke_generation_handler(
    app: AppState,
    route: &str,
    body: Value,
    key: &str,
    logical_id: Option<&str>,
    with_lifecycle: bool,
) -> (StatusCode, Bytes) {
    let request = generation_request(body, key, logical_id, with_lifecycle);
    let peer = ConnectInfo("192.0.2.10:443".parse().unwrap());
    let response = match route {
        "responses" => responses(State(app), peer, request).await,
        "chat" => super::super::chat::completions(State(app), peer, request).await,
        "messages" => super::super::skin::messages(State(app), peer, request).await,
        _ => unreachable!(),
    };
    let status = response.status();
    let bytes = to_bytes(response.into_body(), OPENAI_BODY_LIMIT)
        .await
        .unwrap();
    (status, bytes)
}

fn generation_body(route: &str, stream: bool) -> Value {
    match route {
        "responses" => json!({
            "model": "openai/gpt-5.6",
            "input": "private prompt marker",
            "stream": stream,
            "tools": [{"type":"function","name":"private_tool_name","parameters":{"type":"object","properties":{"private_schema_name":{"type":"string"}}}}]
        }),
        "chat" => json!({
            "model": "openai/gpt-5.6",
            "messages": [{"role":"user","content":"private prompt marker"}],
            "stream": stream,
            "tools": [{"type":"function","function":{"name":"private_tool_name","parameters":{"type":"object","properties":{"private_schema_name":{"type":"string"}}}}}]
        }),
        "messages" => json!({
            "model": "openai/gpt-5.6",
            "max_tokens": 32,
            "messages": [{"role":"user","content":"private prompt marker"}],
            "stream": stream,
            "tools": [{"name":"private_tool_name","input_schema":{"type":"object","properties":{"private_schema_name":{"type":"string"}}}}]
        }),
        _ => unreachable!(),
    }
}

#[test]
fn generation_handlers_persist_apply_facts_for_all_routes_and_stream_modes_on_postgres() {
    const LOCK: i64 = 831_572_908_441;
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Codex generation route PostgreSQL matrix: test URL is unset");
        return;
    };
    const ACCOUNT: &str = "codex-generation-route-account";
    const KEY: &str = "sk-pool-codex-generation-private-key";
    let mut sql = postgres::Client::connect(&url, postgres::NoTls)
        .expect("CLAUDE_API_TEST_DATABASE_URL was supplied but PostgreSQL is unavailable");
    sql.query_one("SELECT pg_advisory_lock($1)", &[&LOCK])
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    sql.batch_execute(
        "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations, \
         capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts \
         RESTART IDENTITY CASCADE",
    )
    .unwrap();
    pg.account_create(ACCOUNT, None, 10_000).unwrap();
    pg.account_topup(ACCOUNT, 100_000_000_000, Some("generation-route-seed"))
        .unwrap();
    pg.key_issue(KEY, ACCOUNT, None).unwrap();
    let key_id = pg.key_get(KEY).unwrap().unwrap().key_id;
    let owner = pg
        .claim_instance(
            &format!("codex-generation-route-{}", std::process::id()),
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let sends = runtime.block_on(async {
        let (upstream, sends) = generation_mock_upstream().await;
        let mut cfg = input_tokens_proxy_config();
        Arc::get_mut(&mut cfg)
            .unwrap()
            .api_keys
            .push("generation-admin-key".into());
        let app = AppState {
            provider: ProviderMode::OpenAi,
            authority: Arc::new(registry::authority::AuthorityConfig::Postgres {
                url: url.clone(),
            }),
            data_db_path: Arc::new("codex-generation-route-pg".into()),
            pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
            affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
            clients: Arc::new(Clients::new(&cfg)),
            body_storage: Some(codex_test_body_storage("generation")),
            codex: Some(Arc::new(gateway_at(&upstream))),
            gemini: None,
            gemini_batch: None,
            gemini_batch_runtime: None,
            kimi: None,
            glm: None,
            tripo3d: None,
            suno: None,
            billing: Some(Arc::clone(&billing)),
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(Breaker::new(1)),
            metrics: Arc::new(Metrics::new()),
            probe_poke: None,
            admin_changes: tokio::sync::broadcast::channel(16).0,
            cfg,
        };
        // Excluded paths exercise the owning handlers but must not admit a request fact.
        assert_eq!(
            invoke_generation_handler(
                app.clone(),
                "responses",
                generation_body("responses", false),
                "unknown-key",
                Some("cccccccc-cccc-4ccc-8ccc-cccccccccccc"),
                true,
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED,
        );
        assert_eq!(
            invoke_generation_handler(
                app.clone(),
                "responses",
                json!({"model": 7, "input": []}),
                KEY,
                Some("dddddddd-dddd-4ddd-8ddd-dddddddddddd"),
                true,
            )
            .await
            .0,
            StatusCode::BAD_REQUEST,
        );
        assert_eq!(
            invoke_generation_handler(
                app.clone(),
                "responses",
                generation_body("responses", false),
                KEY,
                Some("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee"),
                false,
            )
            .await
            .0,
            StatusCode::OK,
        );
        assert_eq!(
            invoke_generation_handler(
                app.clone(),
                "chat",
                generation_body("chat", false),
                KEY,
                None,
                true,
            )
            .await
            .0,
            StatusCode::OK,
        );
        assert_eq!(
            invoke_generation_handler(
                app.clone(),
                "messages",
                generation_body("messages", false),
                "generation-admin-key",
                Some("99999999-9999-4999-8999-999999999999"),
                true,
            )
            .await
            .0,
            StatusCode::OK,
        );

        super::super::billing::fail_next_codex_delivery_marker_for_test();
        let marker_failed = invoke_generation_handler(
            app.clone(),
            "responses",
            generation_body("responses", false),
            KEY,
            Some("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
            true,
        )
        .await;
        assert_eq!(marker_failed.0, StatusCode::SERVICE_UNAVAILABLE);

        for (index, (route, stream)) in [
            ("responses", false),
            ("responses", true),
            ("chat", false),
            ("chat", true),
            ("messages", false),
            ("messages", true),
        ]
        .into_iter()
        .enumerate()
        {
            let logical = format!("aaaaaaaa-aaaa-4aaa-8aaa-{index:012x}");
            let (status, body) = invoke_generation_handler(
                app.clone(),
                route,
                generation_body(route, stream),
                KEY,
                Some(&logical),
                true,
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "{route} stream={stream}: {}",
                String::from_utf8_lossy(&body)
            );
            assert!(!body.is_empty());
        }
        // Return a stream and drop it without ever polling the public body. The background turn
        // must still settle and the explicit closed receiver must be retained as disconnect=true.
        let never_polled = responses(
            State(app.clone()),
            ConnectInfo("192.0.2.10:443".parse().unwrap()),
            generation_request(
                generation_body("responses", true),
                KEY,
                Some("ffffffff-ffff-4fff-8fff-ffffffffffff"),
                true,
            ),
        )
        .await;
        assert_eq!(never_polled.status(), StatusCode::OK);
        drop(never_polled);
        app.codex.as_ref().unwrap().shutdown().await;
        billing.flush().await.unwrap();
        sends
    });
    assert_eq!(
        sends.load(Ordering::Relaxed),
        10,
        "only generation POSTs were expected"
    );

    let excluded_fact_count: i64 = sql.query_one(
        "SELECT COUNT(*)::bigint FROM request_facts WHERE logical_request_id IN          ('cccccccc-cccc-4ccc-8ccc-cccccccccccc','dddddddd-dddd-4ddd-8ddd-dddddddddddd',           'eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee','99999999-9999-4999-8999-999999999999')",
        &[],
    ).unwrap().get(0);
    assert_eq!(excluded_fact_count, 0);

    let marker_failed = sql.query_one(
        "SELECT f.http_status_code,f.provider_terminal_class,f.delivery_state,                 f.internal_attempt_count,f.tool_calls_in_output,f.billing_outcome,                 f.delivery_started_at,o.state,r.state,u.provider            FROM request_facts f JOIN settlement_outbox o ON o.request_id=f.billing_request_id            JOIN reservations r ON r.request_id=f.billing_request_id            JOIN usage_events u ON u.request_id=f.billing_request_id           WHERE f.logical_request_id=$1",
        &[&"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"],
    ).unwrap();
    assert_eq!(marker_failed.get::<_, Option<i32>>(0), Some(503));
    assert_eq!(marker_failed.get::<_, String>(1), "success");
    assert_eq!(marker_failed.get::<_, String>(2), "unknown");
    assert_eq!(marker_failed.get::<_, Option<i32>>(3), Some(1));
    assert_eq!(marker_failed.get::<_, Option<bool>>(4), Some(true));
    assert_eq!(marker_failed.get::<_, String>(5), "winner");
    assert_eq!(marker_failed.get::<_, Option<i64>>(6), None);
    assert_eq!(marker_failed.get::<_, String>(7), "done");
    assert_eq!(marker_failed.get::<_, String>(8), "settled");
    assert_eq!(marker_failed.get::<_, String>(9), "openai");

    let rows = sql.query(
        "SELECT logical_request_id,billing_request_id,provider_plane,route_class,request_class,stream_flag, \
                tools_declared_count,http_status_code,provider_terminal_class,delivery_state,billing_outcome, \
                downstream_disconnect,first_public_byte_at,internal_attempt_count,tool_calls_in_output, \
                delivery_started_at \
           FROM request_facts WHERE logical_request_id LIKE 'aaaaaaaa-%' ORDER BY logical_request_id",
        &[],
    ).unwrap();
    assert_eq!(rows.len(), 6);
    assert_eq!(
        sql.query_one(
            "SELECT COUNT(*)::bigint FROM request_facts WHERE key_id=$1 AND key_id<>$2              AND logical_request_id LIKE 'aaaaaaaa-%'",
            &[&key_id, &KEY],
        )
        .unwrap()
        .get::<_, i64>(0),
        6,
    );
    for (index, row) in rows.iter().enumerate() {
        let route = [
            "responses",
            "responses",
            "chat",
            "chat",
            "messages",
            "messages",
        ][index];
        let stream = index % 2 == 1;
        assert_eq!(
            row.get::<_, String>(0),
            format!("aaaaaaaa-aaaa-4aaa-8aaa-{index:012x}")
        );
        let request_id = row.get::<_, String>(1);
        assert_eq!(row.get::<_, String>(2), "openai");
        assert_eq!(
            row.get::<_, String>(3),
            if route == "messages" {
                "universal"
            } else {
                "native"
            }
        );
        assert_eq!(row.get::<_, String>(4), route);
        assert_eq!(row.get::<_, bool>(5), stream);
        assert_eq!(row.get::<_, Option<i32>>(6), Some(1));
        assert_eq!(row.get::<_, Option<i32>>(7), Some(200));
        assert_eq!(row.get::<_, String>(8), "success");
        assert_eq!(row.get::<_, String>(9), "completed");
        assert_eq!(row.get::<_, String>(10), "winner");
        assert_eq!(row.get::<_, Option<bool>>(11), None);
        assert_eq!(row.get::<_, Option<i64>>(12), None);
        assert_eq!(row.get::<_, Option<i32>>(13), Some(1));
        assert_eq!(row.get::<_, Option<bool>>(14), Some(true));
        assert!(row.get::<_, Option<i64>>(15).is_some());
        let linked = sql
            .query_one(
                "SELECT o.state,r.state,r.provider,u.provider, \
                    (SELECT COUNT(*)::bigint FROM request_facts WHERE billing_request_id=$1) \
               FROM settlement_outbox o JOIN reservations r USING(request_id) \
               JOIN usage_events u USING(request_id) WHERE o.request_id=$1",
                &[&request_id],
            )
            .unwrap();
        assert_eq!(linked.get::<_, String>(0), "done");
        assert_eq!(linked.get::<_, String>(1), "settled");
        assert_eq!(
            linked.get::<_, Option<String>>(2).as_deref(),
            Some("openai")
        );
        assert_eq!(linked.get::<_, String>(3), "openai");
        assert_eq!(linked.get::<_, i64>(4), 1);
    }
    let never_polled = sql.query_one(
        "SELECT provider_terminal_class,delivery_state,downstream_disconnect,internal_attempt_count,                 tool_calls_in_output,billing_outcome FROM request_facts WHERE logical_request_id=$1",
        &[&"ffffffff-ffff-4fff-8fff-ffffffffffff"],
    ).unwrap();
    assert_eq!(
        never_polled.get::<_, Option<String>>(0).as_deref(),
        Some("unknown")
    );
    assert_eq!(
        never_polled.get::<_, Option<String>>(1).as_deref(),
        Some("unknown")
    );
    assert_eq!(never_polled.get::<_, Option<bool>>(2), Some(true));
    assert_eq!(never_polled.get::<_, Option<i32>>(3), None);
    assert_eq!(never_polled.get::<_, Option<bool>>(4), None);
    assert_eq!(never_polled.get::<_, String>(5), "canceled");

    let private_scan: String = sql
        .query_one(
            "SELECT COALESCE(string_agg(row_to_json(f)::text || row_to_json(o)::text, ''), '') \
           FROM request_facts f JOIN settlement_outbox o ON o.request_id=f.billing_request_id",
            &[],
        )
        .unwrap()
        .get(0);
    for secret in [
        "private prompt marker",
        "private_tool_name",
        "private_schema_name",
        "private_argument",
        KEY,
    ] {
        assert!(
            !private_scan.contains(secret),
            "request fact/outbox leaked {secret:?}"
        );
    }
    assert!(sql
        .query_one("SELECT pg_advisory_unlock($1)", &[&LOCK])
        .unwrap()
        .get::<_, bool>(0));
    drop(billing);
}

#[tokio::test]
async fn frame_timeout_is_not_misclassified_as_a_downstream_disconnect() {
    let (sender, mut receiver) = mpsc::channel(1);
    sender.send(Bytes::from_static(b"occupied")).await.unwrap();
    assert!(
        !send_sse_bytes_with_timeout(
            &sender,
            Bytes::from_static(b"blocked"),
            std::time::Duration::from_millis(1),
        )
        .await
    );
    assert!(
        !sender.is_closed(),
        "full-but-open Responses receiver is not disconnected"
    );
    assert!(
        !super::super::chat::send_chat_bytes_with_timeout(
            &sender,
            Bytes::from_static(b"blocked-chat"),
            std::time::Duration::from_millis(1),
        )
        .await
    );
    assert!(
        !sender.is_closed(),
        "full-but-open Chat/Messages receiver is not disconnected"
    );
    assert_eq!(
        receiver.recv().await.unwrap(),
        Bytes::from_static(b"occupied")
    );
    drop(receiver);
    assert!(
        !send_sse_bytes_with_timeout(
            &sender,
            Bytes::from_static(b"closed"),
            std::time::Duration::from_millis(50),
        )
        .await
    );
    assert!(
        sender.is_closed(),
        "closed receiver is explicit disconnect evidence"
    );
}

/// A turn where the model narrates an answer and then calls a tool. The provider closes the
/// message before opening the tool item; a client renders the streamed answer as a live cell and
/// finalizes that cell the moment the next item opens. Closing the message afterwards therefore
/// arrives as a *second* assistant message and the answer is rendered twice.
const NARRATE_THEN_TOOL_CALL_SSE: &str = concat!(
    "event: response.output_item.added\n",
    "data: {\"type\":\"response.output_item.added\",\"sequence_number\":1,\"output_index\":0,",
    "\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",",
    "\"status\":\"in_progress\",\"content\":[]}}\n\n",
    "event: response.output_text.delta\n",
    "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":2,\"output_index\":0,",
    "\"item_id\":\"msg_1\",\"delta\":\"I will check the weather.\"}\n\n",
    "event: response.output_item.done\n",
    "data: {\"type\":\"response.output_item.done\",\"sequence_number\":3,\"output_index\":0,",
    "\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"status\":\"completed\",",
    "\"content\":[{\"type\":\"output_text\",\"text\":\"I will check the weather.\"}]}}\n\n",
    "event: response.output_item.added\n",
    "data: {\"type\":\"response.output_item.added\",\"sequence_number\":4,\"output_index\":1,",
    "\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",",
    "\"name\":\"private_tool_name\",\"arguments\":\"\"}}\n\n",
    "event: response.output_item.done\n",
    "data: {\"type\":\"response.output_item.done\",\"sequence_number\":5,\"output_index\":1,",
    "\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",",
    "\"name\":\"private_tool_name\",\"arguments\":\"{}\"}}\n\n",
    "event: response.completed\n",
    "data: {\"type\":\"response.completed\",\"sequence_number\":6,\"response\":{",
    "\"service_tier\":\"default\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5,",
    "\"total_tokens\":15}}}\n\n"
);

#[tokio::test]
async fn streamed_message_is_closed_before_the_next_output_item_opens() {
    let (upstream, _) = mock_upstream_serving(NARRATE_THEN_TOOL_CALL_SSE).await;
    let test = input_tokens_test_app(true, None, ProviderMode::OpenAi).await;
    let app = AppState {
        codex: Some(Arc::new(gateway_at(&upstream))),
        ..test.app.clone()
    };
    app.billing
        .as_ref()
        .unwrap()
        .topup(
            INPUT_TOKENS_ACCOUNT_ID,
            100_000_000_000,
            Some("stream-order-seed"),
        )
        .await
        .unwrap();
    let (status, bytes) = invoke_generation_handler(
        app,
        "responses",
        generation_body("responses", true),
        INPUT_TOKENS_RAW_KEY,
        Some(INPUT_TOKENS_LOGICAL_ID),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stream = String::from_utf8(bytes.to_vec()).unwrap();
    let frames = stream
        .split("\n\n")
        .filter(|frame| !frame.trim().is_empty())
        .collect::<Vec<_>>();

    let message_done = frames
        .iter()
        .position(|frame| {
            frame.starts_with("event: response.output_item.done\n") && frame.contains("\"msg_1\"")
        })
        .expect("the streamed message must be closed downstream");
    let tool_opened = frames
        .iter()
        .position(|frame| frame.contains("\"fc_1\""))
        .expect("the tool call must reach the client");
    assert!(
        message_done < tool_opened,
        "message must close before the tool item opens, got frames: {frames:#?}"
    );
    assert_eq!(
        frames
            .iter()
            .filter(
                |frame| frame.starts_with("event: response.output_item.done\n")
                    && frame.contains("\"msg_1\"")
            )
            .count(),
        1,
        "one provider message must never be closed twice"
    );
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.contains("I will check the weather.")
                && frame.starts_with("event: response.output_item.done\n"))
            .count(),
        1,
        "the answer text must be delivered as exactly one completed message"
    );
}

#[tokio::test]
async fn sqlite_generation_handlers_emit_no_request_fact() {
    let (upstream, sends) = generation_mock_upstream().await;
    let test = input_tokens_test_app(true, None, ProviderMode::OpenAi).await;
    let app = AppState {
        codex: Some(Arc::new(gateway_at(&upstream))),
        ..test.app.clone()
    };
    app.billing
        .as_ref()
        .unwrap()
        .topup(
            INPUT_TOKENS_ACCOUNT_ID,
            100_000_000_000,
            Some("sqlite-generation-seed"),
        )
        .await
        .unwrap();
    for route in ["responses", "chat", "messages"] {
        let (status, _) = invoke_generation_handler(
            app.clone(),
            route,
            generation_body(route, false),
            INPUT_TOKENS_RAW_KEY,
            Some(INPUT_TOKENS_LOGICAL_ID),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{route}");
    }
    app.billing.as_ref().unwrap().flush().await.unwrap();
    assert_eq!(sends.load(Ordering::Relaxed), 3);
    assert_eq!(
        app.billing
            .as_ref()
            .unwrap()
            .request_fact_delivery_snapshot()
            .dropped_unsupported,
        0,
        "billable SQLite omission must not use the terminal-only inbox",
    );
}
