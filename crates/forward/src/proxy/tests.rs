use super::*;
use crate::affinity::AffinityStore;
use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::config::ProxyConfig;
use crate::upstream::Clients;
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use pool::{Pool, Reserve};
use registry::Sub;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use tokio::sync::mpsc;

fn lim(u5: f64, u7: f64, claim: Option<&str>, r5: i64, r7: i64) -> Limits {
    Limits {
        util5h: Some(u5),
        util7d: Some(u7),
        quota5h: None,
        quota7d: None,
        status: None,
        reset5h: Some(r5),
        reset7d: Some(r7),
        claim: claim.map(|s| s.to_string()),
    }
}

fn proxy_test_config() -> Arc<ProxyConfig> {
    Arc::new(ProxyConfig {
        api_keys: Vec::new(),
        control_keys: Vec::new(),
        panel_keys: Vec::new(),
        default_mult_bp: 10_000,
        trust_loopback: false,
        upstream: "http://127.0.0.1:1".to_string(),
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
        user_agent: "proxy-auth-test".to_string(),
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

fn proxy_test_app(billing: Arc<AsyncBilling>, path: &str) -> AppState {
    let cfg = proxy_test_config();
    AppState {
        provider: crate::ProviderMode::Anthropic,
        authority: Arc::new(registry::authority::AuthorityConfig::new(
            path.to_string(),
            None,
        )),
        data_db_path: Arc::new(path.to_string()),
        pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: None,
        codex: None,
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
    }
}

const COUNT_FACT_LOGICAL_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const COUNT_FACT_EXECUTION_GROUP: &str = "bbbbbbbb-bbbb-4bbb-9bbb-bbbbbbbbbbbb";
const COUNT_FACT_RAW_KEY: &str = "sk-anthropic-count-fact-secret";
const COUNT_FACT_ACCOUNT_ID: &str = "anthropic-count-fact-account";
const COUNT_FACT_KEY_ID: &str = "key_anthropic_count_nonsecret";
// Every real-PostgreSQL test that truncates shared engine tables must serialize with registry's
// canonical destructive-test advisory lock. A distinct lock races the workspace's concurrent PG
// suites and can interleave TRUNCATE/setup, producing false duplicate-key failures.
const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

fn native_count_tokens_request(
    body: serde_json::Value,
    with_context: bool,
    client: Option<&str>,
) -> axum::extract::Request {
    let mut request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages/count_tokens")
        .header("x-api-key", COUNT_FACT_RAW_KEY)
        .header("anthropic-version", "2023-06-01")
        .header(
            crate::execution::EXECUTION_GROUP_HEADER,
            COUNT_FACT_EXECUTION_GROUP,
        )
        .header(crate::execution::EXECUTION_ATTEMPT_HEADER, 2)
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    if with_context {
        let mut headers = HeaderMap::new();
        headers.insert(
            crate::execution::LOGICAL_REQUEST_ID_HEADER,
            COUNT_FACT_LOGICAL_ID.parse().unwrap(),
        );
        request
            .extensions_mut()
            .insert(crate::execution::admit_logical_request_id(&mut headers).unwrap());
        request
            .extensions_mut()
            .insert(crate::execution::RequestLifecycleClock::default());
    }
    if let Some(client) = client {
        let mut headers = HeaderMap::new();
        headers.insert(
            crate::execution::CLIENT_ATTRIBUTION_HEADER,
            client.parse().unwrap(),
        );
        request
            .extensions_mut()
            .insert(crate::execution::admit_client_attribution(&mut headers));
    }
    request
}

fn universal_request(uri: &str, body: serde_json::Value) -> axum::extract::Request {
    let mut request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("x-api-key", COUNT_FACT_RAW_KEY)
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        crate::execution::LOGICAL_REQUEST_ID_HEADER,
        COUNT_FACT_LOGICAL_ID.parse().unwrap(),
    );
    request
        .extensions_mut()
        .insert(crate::execution::admit_logical_request_id(&mut headers).unwrap());
    request
        .extensions_mut()
        .insert(crate::execution::RequestLifecycleClock::default());
    let mut headers = HeaderMap::new();
    headers.insert(
        crate::execution::CLIENT_ATTRIBUTION_HEADER,
        "opencode/1.2.3".parse().unwrap(),
    );
    request
        .extensions_mut()
        .insert(crate::execution::admit_client_attribution(&mut headers));
    request
}

async fn count_fact_test_app(
    upstream: &str,
    subs: usize,
    fact_sender: Option<mpsc::Sender<TerminalRequestFact>>,
) -> (AppState, std::path::PathBuf) {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-anthropic-count-fact-{}-{unique}.sqlite",
        std::process::id()
    ));
    let path_string = path.to_string_lossy().into_owned();
    let connection = registry::open(&path_string).unwrap();
    registry::account_create(&connection, COUNT_FACT_ACCOUNT_ID, None, 10_000).unwrap();
    registry::account_topup(&connection, COUNT_FACT_ACCOUNT_ID, 1_000, None).unwrap();
    registry::key_issue(&connection, COUNT_FACT_RAW_KEY, COUNT_FACT_ACCOUNT_ID, None).unwrap();
    connection
        .execute(
            "UPDATE api_keys SET key_id=?1 WHERE key=?2",
            (COUNT_FACT_KEY_ID, COUNT_FACT_RAW_KEY),
        )
        .unwrap();
    drop(connection);
    let mut billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
    if let Some(sender) = fact_sender {
        billing.replace_request_fact_inbox_for_test(sender);
    }
    let billing = Arc::new(billing);
    let mut cfg = (*proxy_test_config()).clone();
    cfg.upstream = upstream.to_owned();
    cfg.max_tries = subs.max(1);
    let cfg = Arc::new(cfg);
    let pool = Pool::new(
        (0..subs)
            .map(|index| Sub {
                email: format!("count-fact-{index}@example.test"),
                token: "subscription-token".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            })
            .collect(),
        Reserve::FULL,
        1.0,
        1.0,
    );
    let app = AppState {
        provider: crate::ProviderMode::Anthropic,
        authority: Arc::new(registry::authority::AuthorityConfig::new(
            path_string.clone(),
            None,
        )),
        data_db_path: Arc::new(path_string),
        pool: Arc::new(pool),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: None,
        codex: None,
        gemini: None,
        gemini_batch: None,
            gemini_batch_runtime: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing: Some(billing),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(100)),
        metrics: Arc::new(Metrics::new()),
        probe_poke: None,
        admin_changes: tokio::sync::broadcast::channel(16).0,
        cfg,
    };
    (app, path)
}

fn spawn_count_upstream(
    responses: Vec<(u16, Vec<(&'static str, &'static str)>, &'static [u8])>,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        for (status, headers, body) in responses {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = socket.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let header_end = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4);
                let Some(header_end) = header_end else {
                    continue;
                };
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .or_else(|| line.strip_prefix("Content-Length: "))
                    })
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            assert!(request.starts_with(b"POST /v1/messages/count_tokens HTTP/1.1"));
            write!(
                socket,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n",
                body.len()
            )
            .unwrap();
            for (name, value) in headers {
                write!(socket, "{name}: {value}\r\n").unwrap();
            }
            socket.write_all(b"\r\n").unwrap();
            socket.write_all(body).unwrap();
        }
    });
    (format!("http://{address}"), handle)
}

fn spawn_messages_upstream(
    transient_failures: usize,
    response_headers: Vec<(&'static str, &'static str)>,
    body: &'static [u8],
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        for attempt in 0..=transient_failures {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = socket.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4)
                else {
                    continue;
                };
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .or_else(|| line.strip_prefix("Content-Length: "))
                    })
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            assert!(request.starts_with(b"POST /v1/messages?beta=true HTTP/1.1"));
            if attempt < transient_failures {
                socket
                    .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 2\r\n\r\n{}")
                    .unwrap();
            } else {
                write!(
                    socket,
                    "HTTP/1.1 201 Created\r\nContent-Length: {}\r\n",
                    body.len()
                )
                .unwrap();
                for (name, value) in &response_headers {
                    write!(socket, "{name}: {value}\r\n").unwrap();
                }
                socket.write_all(b"\r\n").unwrap();
                socket.write_all(body).unwrap();
            }
        }
    });
    (format!("http://{address}"), handle)
}

fn spawn_universal_messages_upstream(
    responses: Vec<(&'static str, &'static str, &'static [u8])>,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        for (request_id, content_type, body) in responses {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = socket.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4)
                else {
                    continue;
                };
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .or_else(|| line.strip_prefix("Content-Length: "))
                    })
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= header_end + content_length {
                    break;
                }
            }
            assert!(request.starts_with(b"POST /v1/messages?beta=true HTTP/1.1"));
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nrequest-id: {request_id}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            socket.write_all(body).unwrap();
        }
    });
    (format!("http://{address}"), handle)
}

async fn body_snapshot(response: Response) -> (StatusCode, HeaderMap, bytes::Bytes) {
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, headers, body)
}

#[tokio::test]
async fn native_count_tokens_submits_one_success_fact_after_rotation_without_polling_body() {
    let success_body = br#"{"input_tokens":7}"#;
    let (upstream, server) = spawn_count_upstream(vec![
        (500, vec![("request-id", "req-discarded")], b"retry"),
        (
            200,
            vec![("request-id", "req-terminal"), ("x-test", "kept")],
            success_body,
        ),
    ]);
    let (sender, mut facts) = mpsc::channel(4);
    let (app, path) = count_fact_test_app(&upstream, 2, Some(sender)).await;
    let body = serde_json::json!({
        "model": "claude-test",
        "messages": [{"role":"user","content":[{"type":"text","text":"PRIVATE"},{"type":"image","source":{"type":"base64","data":"SECRET"}}]}],
        "tools": [{"name":"never-store-me","description":"PRIVATE","input_schema":{"secret":"schema"}}],
        "tool_choice": {"type":"any", "disable_parallel_tool_use":false},
        "output_config": {"format":{"type":"json_schema","schema":{"secret":true}}},
        "thinking": {"type":"enabled", "budget_tokens":1}
    });
    let response = forward(
        State(app.clone()),
        ConnectInfo("192.0.2.1:443".parse().unwrap()),
        native_count_tokens_request(body, true, Some("opencode/1.2.3")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("x-test").unwrap(), "kept");
    // Terminal fact exists before the response body is ever polled.
    let fact = facts
        .try_recv()
        .expect("one terminal fact at returned headers");
    assert!(
        facts.try_recv().is_err(),
        "one fact across subscription retries"
    );
    assert_eq!(fact.logical_request_id, COUNT_FACT_LOGICAL_ID);
    assert_eq!(fact.billing_request_id, None);
    assert_eq!(
        fact.execution_group_id.as_deref(),
        Some(COUNT_FACT_EXECUTION_GROUP)
    );
    assert_eq!(fact.attempt, 2);
    assert_eq!(fact.account_id, COUNT_FACT_ACCOUNT_ID);
    assert_ne!(fact.key_id, COUNT_FACT_RAW_KEY);
    assert_eq!(fact.provider_plane, "anthropic");
    assert_eq!(fact.route_class, "native");
    assert_eq!(fact.request_class, "count_tokens");
    assert_eq!(fact.requested_model.as_deref(), Some("claude-test"));
    assert_eq!(fact.executable_model, None);
    assert!(!fact.stream_flag);
    assert_eq!(fact.tools_declared_count, Some(1));
    assert_eq!(
        fact.tool_classes,
        Some(registry::request_facts::TOOL_CLASS_CUSTOM_FUNCTION)
    );
    assert_eq!(
        fact.tool_choice_mode,
        Some(registry::request_facts::ToolChoiceMode::Required)
    );
    assert_eq!(fact.parallel_tools_requested, Some(true));
    assert_eq!(fact.structured_output_flag, Some(true));
    assert_eq!(fact.reasoning_flag, Some(true));
    assert_eq!(
        fact.input_modalities,
        Some(registry::request_facts::MODALITY_TEXT | registry::request_facts::MODALITY_IMAGE)
    );
    assert_eq!(fact.terminal.http_status_code, Some(200));
    assert_eq!(
        fact.terminal.provider_terminal_class,
        ProviderTerminalClass::Success
    );
    assert_eq!(fact.terminal.delivery_state, DeliveryState::Started);
    assert_eq!(fact.terminal.internal_attempt_count, Some(2));
    assert_eq!(
        fact.terminal.upstream_request_id.as_deref(),
        Some("req-terminal")
    );
    assert_eq!(fact.terminal.first_public_byte_at, None);
    assert_eq!(fact.terminal.downstream_disconnect, None);
    assert_eq!(fact.terminal.tool_calls_in_output, None);
    assert_eq!(format!("{:?}", fact.client_kind), "OpenCode");
    assert_eq!(fact.client_version.as_deref(), Some("1.2.3"));
    let serialized = format!("{fact:?}");
    for private in ["PRIVATE", "SECRET", "never-store-me", "schema"] {
        assert!(!serialized.contains(private));
    }
    let observed = body_snapshot(response).await;
    assert_eq!(observed.0, StatusCode::OK);
    assert_eq!(observed.2.as_ref(), success_body);
    assert!(facts.try_recv().is_err());
    server.join().unwrap();
    drop(app);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn native_count_tokens_upstream_errors_keep_exact_response_and_discard_classifier_candidate()
{
    for (status, expected_class) in [
        (400, ProviderTerminalClass::ClientError),
        (401, ProviderTerminalClass::Auth),
        (429, ProviderTerminalClass::Quota),
        (500, ProviderTerminalClass::UpstreamError),
    ] {
        let body = format!("terminal-{status}").into_bytes();
        let static_body: &'static [u8] = Box::leak(body.clone().into_boxed_slice());
        let (upstream, server) = spawn_count_upstream(vec![(
            status,
            vec![("request-id", "req-error"), ("x-exact", "yes")],
            static_body,
        )]);
        let (sender, mut facts) = mpsc::channel(2);
        let (app, path) = count_fact_test_app(&upstream, 1, Some(sender)).await;
        let response = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            native_count_tokens_request(
                serde_json::json!({
                    "model":"claude-test",
                    "messages":[{"role":"user","content":"PRIVATE"}],
                    "tools":[]
                }),
                true,
                None,
            ),
        )
        .await;
        let fact = facts.try_recv().unwrap();
        assert_eq!(fact.terminal.http_status_code, Some(status as i32));
        assert_eq!(fact.terminal.provider_terminal_class, expected_class);
        assert_eq!(fact.terminal.delivery_state, DeliveryState::Started);
        assert_eq!(fact.terminal.downstream_disconnect, None);
        assert_eq!(fact.terminal.first_public_byte_at, None);
        assert_eq!(fact.terminal.internal_attempt_count, Some(1));
        assert_eq!(
            fact.requested_model, None,
            "a rejected upstream shape does not validate the requested model"
        );
        assert_eq!(fact.tools_declared_count, None);
        assert_eq!(fact.tool_classes, None);
        assert_eq!(fact.input_modalities, None);
        assert_eq!(
            fact.client_kind,
            registry::request_facts::ClientKind::Unknown
        );
        assert_eq!(
            fact.client_source,
            registry::request_facts::ClientSource::Unknown
        );
        let response = body_snapshot(response).await;
        assert_eq!(response.0.as_u16(), status);
        assert_eq!(response.1.get("x-exact").unwrap(), "yes");
        assert_eq!(response.2.as_ref(), body.as_slice());
        assert!(facts.try_recv().is_err());
        server.join().unwrap();
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn native_count_tokens_local_pool_and_transport_terminals_are_honest() {
    for (upstream, subs, expected_attempts, expected_delivery) in [
        (
            "http://127.0.0.1:1".to_owned(),
            0,
            Some(0),
            DeliveryState::NotStarted,
        ),
        (
            "http://127.0.0.1:1".to_owned(),
            1,
            Some(1),
            DeliveryState::Unknown,
        ),
    ] {
        let (sender, mut facts) = mpsc::channel(2);
        let (app, path) = count_fact_test_app(&upstream, subs, Some(sender)).await;
        let response = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            native_count_tokens_request(
                serde_json::json!({"model":"claude-test","messages":[]}),
                true,
                None,
            ),
        )
        .await;
        let response = body_snapshot(response).await;
        assert!(!response.0.is_success());
        let fact = facts.try_recv().unwrap();
        assert_eq!(
            fact.terminal.http_status_code,
            Some(response.0.as_u16() as i32)
        );
        assert_eq!(
            fact.terminal.provider_terminal_class,
            ProviderTerminalClass::Unknown
        );
        assert_eq!(fact.terminal.delivery_state, expected_delivery);
        assert_eq!(fact.terminal.internal_attempt_count, expected_attempts);
        assert_eq!(fact.terminal.upstream_request_id, None);
        assert!(facts.try_recv().is_err());
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn native_count_tokens_omits_wrong_targets_auth_and_typed_context() {
    for (method, uri) in [(Method::POST, "/v1/messages"), (Method::GET, "/v1/models")] {
        let (sender, mut facts) = mpsc::channel(2);
        let (app, path) = count_fact_test_app("http://127.0.0.1:1", 0, Some(sender)).await;
        let mut request = native_count_tokens_request(
            serde_json::json!({"model":"claude-test","messages":[]}),
            true,
            None,
        );
        *request.method_mut() = method;
        *request.uri_mut() = uri.parse().unwrap();
        let _ = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            request,
        )
        .await;
        assert!(facts.try_recv().is_err());
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    for missing_lifecycle in [false, true] {
        let (sender, mut facts) = mpsc::channel(2);
        let (app, path) = count_fact_test_app("http://127.0.0.1:1", 0, Some(sender)).await;
        let mut request = native_count_tokens_request(
            serde_json::json!({"model":"claude-test","messages":[]}),
            true,
            None,
        );
        if missing_lifecycle {
            request
                .extensions_mut()
                .remove::<crate::execution::RequestLifecycleClock>();
        } else {
            request
                .extensions_mut()
                .remove::<crate::execution::LogicalRequestId>();
        }
        let _ = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            request,
        )
        .await;
        assert!(facts.try_recv().is_err());
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    for provider in [
        crate::ProviderMode::OpenAi,
        crate::ProviderMode::Gemini,
        crate::ProviderMode::Kimi,
        crate::ProviderMode::Tripo3d,
        crate::ProviderMode::Suno,
    ] {
        let (sender, mut facts) = mpsc::channel(2);
        let (mut app, path) = count_fact_test_app("http://127.0.0.1:1", 0, Some(sender)).await;
        app.provider = provider;
        let _ = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            native_count_tokens_request(
                serde_json::json!({"model":"claude-test","messages":[]}),
                true,
                None,
            ),
        )
        .await;
        assert!(facts.try_recv().is_err());
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    for auth in ["unauthorized", "admin"] {
        let (sender, mut facts) = mpsc::channel(2);
        let (mut app, path) = count_fact_test_app("http://127.0.0.1:1", 0, Some(sender)).await;
        let mut request = native_count_tokens_request(
            serde_json::json!({"model":"claude-test","messages":[]}),
            true,
            None,
        );
        if auth == "unauthorized" {
            request
                .headers_mut()
                .insert("x-api-key", "invalid".parse().unwrap());
        } else {
            let mut cfg = (*app.cfg).clone();
            cfg.api_keys = vec![COUNT_FACT_RAW_KEY.to_string()];
            app.cfg = Arc::new(cfg);
        }
        let _ = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            request,
        )
        .await;
        assert!(facts.try_recv().is_err());
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn native_count_tokens_combined_mode_uses_anthropic_leaf_fact() {
    let (upstream, server) = spawn_count_upstream(vec![(200, vec![], br#"{"input_tokens":1}"#)]);
    let (sender, mut facts) = mpsc::channel(2);
    let (mut app, path) = count_fact_test_app(&upstream, 1, Some(sender)).await;
    app.provider = crate::ProviderMode::Combined;
    let response = forward(
        State(app.clone()),
        ConnectInfo("192.0.2.1:443".parse().unwrap()),
        native_count_tokens_request(
            serde_json::json!({"model":"claude-test","messages":[]}),
            true,
            None,
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(facts.try_recv().unwrap().provider_plane, "anthropic");
    assert!(facts.try_recv().is_err());
    server.join().unwrap();
    drop(app);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn native_count_tokens_missing_and_overlong_models_keep_success_fact_without_model() {
    let overlong = "m".repeat(MAX_REQUEST_FACT_MODEL_LEN + 1);
    for model in [None, Some(overlong.as_str())] {
        let (upstream, server) =
            spawn_count_upstream(vec![(200, vec![], br#"{"input_tokens":1}"#)]);
        let (sender, mut facts) = mpsc::channel(2);
        let (app, path) = count_fact_test_app(&upstream, 1, Some(sender)).await;
        let mut body = serde_json::json!({"messages": [{"role":"user","content":"PRIVATE"}]});
        if let Some(model) = model {
            body["model"] = serde_json::Value::String(model.to_string());
        }
        let response = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            native_count_tokens_request(body, true, None),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let fact = facts.try_recv().unwrap();
        assert_eq!(fact.requested_model, None);
        assert_eq!(
            fact.input_modalities,
            Some(registry::request_facts::MODALITY_TEXT)
        );
        assert!(facts.try_recv().is_err());
        server.join().unwrap();
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn native_count_tokens_internal_aliases_discard_structural_candidate_on_local_rejection() {
    for (model, expected_attempts, expected_delivery) in [
        ("kimi-for-coding", Some(0), DeliveryState::NotStarted),
        ("glm-4.7", Some(1), DeliveryState::Unknown),
    ] {
        let (sender, mut facts) = mpsc::channel(2);
        let (app, path) = count_fact_test_app("http://127.0.0.1:1", 1, Some(sender)).await;
        let response = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            native_count_tokens_request(
                serde_json::json!({
                    "model": model,
                    "messages": [{"role":"user","content":"PRIVATE"}],
                    "tools": [{"name":"PRIVATE TOOL","input_schema":{"type":"object"}}]
                }),
                true,
                None,
            ),
        )
        .await;
        assert!(!response.status().is_success());
        let fact = facts.try_recv().unwrap();
        assert_eq!(fact.requested_model, None);
        assert_eq!(fact.input_modalities, None);
        assert_eq!(fact.tools_declared_count, None);
        assert_eq!(fact.terminal.internal_attempt_count, expected_attempts);
        assert_eq!(fact.terminal.delivery_state, expected_delivery);
        assert!(facts.try_recv().is_err());
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn native_count_tokens_unsupported_sqlite_fact_authority_preserves_response() {
    let success_body = br#"{"input_tokens":17}"#;
    let (upstream, server) = spawn_count_upstream(vec![(
        200,
        vec![("x-exact", "sqlite-unchanged")],
        success_body,
    )]);
    let (app, path) = count_fact_test_app(&upstream, 1, None).await;
    let response = forward(
        State(app.clone()),
        ConnectInfo("192.0.2.1:443".parse().unwrap()),
        native_count_tokens_request(
            serde_json::json!({"model":"claude-test","messages":[]}),
            true,
            None,
        ),
    )
    .await;
    let response = body_snapshot(response).await;
    assert_eq!(response.0, StatusCode::OK);
    assert_eq!(response.1.get("x-exact").unwrap(), "sqlite-unchanged");
    assert_eq!(response.2.as_ref(), success_body);
    server.join().unwrap();
    drop(app);
    let _ = std::fs::remove_file(path);
}

#[test]
fn native_count_tokens_terminal_seal_keeps_prior_observation_and_blocks_later_one() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-anthropic-count-lifecycle-{}-{unique}.sqlite",
        std::process::id()
    ));
    let mut billing = AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();
    let (sender, mut receiver) = mpsc::channel(2);
    billing.replace_request_fact_inbox_for_test(sender);
    let billing = Arc::new(billing);

    let observed_clock = crate::execution::RequestLifecycleClock::default();
    observed_clock.observe_first_public_byte();
    let observed_at = observed_clock.first_public_byte_at().unwrap();
    let admitted_at = observed_at;
    let seed = AnthropicCountTokensFactSeed {
        logical_request_id: COUNT_FACT_LOGICAL_ID.into(),
        client_attribution: {
            let mut headers = HeaderMap::new();
            crate::execution::admit_client_attribution(&mut headers)
        },
        execution: registry::ExecutionAttempt::direct(),
        account_id: COUNT_FACT_ACCOUNT_ID.into(),
        key_id: "key-lifecycle".into(),
        requested_model_candidate: None,
        classification_candidate: Some(classify_anthropic_messages(&serde_json::json!({}))),
        admitted_at,
        lifecycle_clock: observed_clock,
    };
    let response = AnthropicCountTokensFactGuard::new(Arc::clone(&billing), seed)
        .finish_local(local_err(LocalErr::BadRequest, None));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        receiver.try_recv().unwrap().terminal.first_public_byte_at,
        Some(observed_at)
    );

    let sealed_clock = crate::execution::RequestLifecycleClock::default();
    let seed = AnthropicCountTokensFactSeed {
        logical_request_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(),
        client_attribution: {
            let mut headers = HeaderMap::new();
            crate::execution::admit_client_attribution(&mut headers)
        },
        execution: registry::ExecutionAttempt::direct(),
        account_id: COUNT_FACT_ACCOUNT_ID.into(),
        key_id: "key-lifecycle".into(),
        requested_model_candidate: None,
        classification_candidate: Some(classify_anthropic_messages(&serde_json::json!({}))),
        admitted_at: pool::now(),
        lifecycle_clock: sealed_clock.clone(),
    };
    let _ = AnthropicCountTokensFactGuard::new(Arc::clone(&billing), seed)
        .finish_local(local_err(LocalErr::BadRequest, None));
    let fact = receiver.try_recv().unwrap();
    assert_eq!(fact.terminal.first_public_byte_at, None);
    sealed_clock.observe_first_public_byte();
    assert_eq!(sealed_clock.first_public_byte_at(), None);
    drop(billing);
    drop(runtime);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn native_count_tokens_cancellation_submits_unknown_without_fabricated_status_or_count() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 4096];
        let _ = socket.read(&mut buffer);
        accepted_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    });
    let (sender, mut facts) = mpsc::channel(2);
    let (app, path) = count_fact_test_app(&format!("http://{address}"), 1, Some(sender)).await;
    let task = tokio::spawn(forward(
        State(app.clone()),
        ConnectInfo("192.0.2.1:443".parse().unwrap()),
        native_count_tokens_request(
            serde_json::json!({"model":"claude-test","messages":[]}),
            true,
            None,
        ),
    ));
    tokio::task::spawn_blocking(move || {
        accepted_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let fact = tokio::time::timeout(std::time::Duration::from_secs(1), facts.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fact.terminal.http_status_code, None);
    assert_eq!(
        fact.terminal.provider_terminal_class,
        ProviderTerminalClass::Unknown
    );
    assert_eq!(fact.terminal.delivery_state, DeliveryState::Unknown);
    assert_eq!(fact.terminal.internal_attempt_count, None);
    assert_eq!(fact.terminal.first_public_byte_at, None);
    assert!(facts.try_recv().is_err());
    server.join().unwrap();
    drop(app);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn native_count_tokens_fact_inbox_closed_or_full_never_changes_response() {
    let success_body = br#"{"input_tokens":11}"#;
    for full in [false, true] {
        let (upstream, server) =
            spawn_count_upstream(vec![(200, vec![("x-exact", "unchanged")], success_body)]);
        let (sender, receiver) = mpsc::channel(1);
        if full {
            sender
                .try_send(TerminalRequestFact {
                    logical_request_id: COUNT_FACT_LOGICAL_ID.into(),
                    billing_request_id: None,
                    execution_group_id: None,
                    attempt: 1,
                    account_id: "filler-account".into(),
                    key_id: "filler-key".into(),
                    client_kind: registry::request_facts::ClientKind::Unknown,
                    client_source: registry::request_facts::ClientSource::Unknown,
                    client_version: None,
                    provider_plane: "anthropic".into(),
                    route_class: "native".into(),
                    request_class: "count_tokens".into(),
                    requested_model: None,
                    executable_model: None,
                    stream_flag: false,
                    tools_declared_count: None,
                    tool_classes: None,
                    tool_choice_mode: None,
                    parallel_tools_requested: None,
                    tool_results_in_input: None,
                    structured_output_flag: None,
                    reasoning_flag: None,
                    service_tier: None,
                    input_modalities: None,
                    output_modalities: None,
                    admitted_at: pool::now(),
                    terminal: RequestFactTerminalEvidence {
                        terminal_at: pool::now(),
                        http_status_code: Some(200),
                        provider_terminal_class: ProviderTerminalClass::Success,
                        delivery_state: DeliveryState::Started,
                        downstream_disconnect: None,
                        upstream_request_id: None,
                        first_public_byte_at: None,
                        internal_attempt_count: Some(1),
                        failure_class: None,
                        tool_calls_in_output: None,
                    },
                })
                .unwrap();
        } else {
            drop(receiver);
        }
        let (app, path) = count_fact_test_app(&upstream, 1, Some(sender)).await;
        let response = forward(
            State(app.clone()),
            ConnectInfo("192.0.2.1:443".parse().unwrap()),
            native_count_tokens_request(
                serde_json::json!({"model":"claude-test","messages":[]}),
                true,
                None,
            ),
        )
        .await;
        let response = body_snapshot(response).await;
        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.get("x-exact").unwrap(), "unchanged");
        assert_eq!(response.2.as_ref(), success_body);
        server.join().unwrap();
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn strip_own_namespace_rewrites_prefixed_model_in_body() {
    // Universal dispatch проксирует тело байт-идентично: namespaced id доезжает
    // до native lane как есть. Strip снимает собственный префикс и в возвращаемом
    // значении, и в теле, которое уйдёт upstream.
    let mut body =
        serde_json::json!({"model": "anthropic/claude-haiku-4-5-20251001", "max_tokens": 16});
    let model = strip_own_namespace(&mut body);
    assert_eq!(model, "claude-haiku-4-5-20251001");
    assert_eq!(
        body["model"],
        serde_json::json!("claude-haiku-4-5-20251001")
    );
    // Остальное тело не тронуто.
    assert_eq!(body["max_tokens"], serde_json::json!(16));
}

#[test]
fn strip_own_namespace_keeps_native_and_absent_model() {
    // Native id — без изменений (байт-идентичность native контракта).
    let mut body = serde_json::json!({"model": "claude-opus-4-8"});
    let model = strip_own_namespace(&mut body);
    assert_eq!(model, "claude-opus-4-8");
    assert_eq!(body["model"], serde_json::json!("claude-opus-4-8"));
    // Нет поля model / не строка — пустая строка, тело не мутирует.
    let mut body = serde_json::json!({"max_tokens": 16});
    let model = strip_own_namespace(&mut body);
    assert_eq!(model, "");
    assert!(body.get("model").is_none());
    // Голый префикс → пустой id (admission отклонит позже, как и пустой model).
    let mut body = serde_json::json!({"model": "anthropic/"});
    let model = strip_own_namespace(&mut body);
    assert_eq!(model, "");
    assert_eq!(body["model"], serde_json::json!(""));
}

#[test]
fn smooth_step_bounds() {
    use std::time::Duration;
    assert_eq!(smooth_step(0, 0), None); // бюджет исчерпан
    assert_eq!(smooth_step(100, 0), None); // исчерпан даже при большом hint
    assert_eq!(smooth_step(10, 10_000), Some(Duration::from_millis(2000))); // hint велик → кап 2с
    assert_eq!(smooth_step(0, 10_000), Some(Duration::from_millis(250))); // hint 0 → пол 250мс
    assert_eq!(smooth_step(5, 300), Some(Duration::from_millis(300))); // остаток < шага → остаток
    assert_eq!(smooth_step(1, 10_000), Some(Duration::from_millis(1000))); // hint 1с в диапазоне
}

#[test]
fn window_cool_prefers_authoritative_claim() {
    let now = 1_000_000;
    let (r5, r7) = (now + 3600, now + 100_000);
    // claim=seven_day + 7d у потолка → студим до reset7d (не до 5h, хотя 5h тоже высок)
    assert_eq!(
        window_cool(&lim(0.97, 0.96, Some("seven_day"), r5, r7), now),
        Some(100_000)
    );
    // claim=five_hour → до reset5h
    assert_eq!(
        window_cool(&lim(0.97, 0.96, Some("five_hour"), r5, r7), now),
        Some(3600)
    );
    // claim есть, но окно НЕ у потолка (0.5) → burst-429 (rate), не quota → None (короткий дефолт)
    assert_eq!(
        window_cool(&lim(0.5, 0.5, Some("five_hour"), r5, r7), now),
        None
    );
    // нет claim → фолбэк-эвристика (7d≥0.95 → reset7d)
    assert_eq!(
        window_cool(&lim(0.1, 0.96, None, r5, r7), now),
        Some(100_000)
    );
}

#[test]
fn billing_block_is_idempotent_and_first() {
    // identity уже стоит первым; billing должен встать ПЕРЕД ним и НЕ дублироваться на «ротации».
    let mut v = serde_json::json!({
        "messages": [],
        "system": [{"type":"text","text":"You are a Claude agent, built on Anthropic's Claude Agent SDK."}]
    });
    set_billing_block(
        &mut v,
        "x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=abcde;",
    );
    // вторая подписка (ротация) — другой cch: заменяем, не добавляем второй блок
    set_billing_block(
        &mut v,
        "x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=99999;",
    );
    let sys = v["system"].as_array().unwrap();
    assert_eq!(sys.len(), 2, "billing не должен дублироваться на ротации");
    assert_eq!(
        sys[0]["text"].as_str().unwrap(),
        "x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=99999;"
    );
    assert!(
        sys[0].get("cache_control").is_none(),
        "billing-блок БЕЗ cache_control (как у CC)"
    );
    assert!(sys[1]["text"]
        .as_str()
        .unwrap()
        .starts_with("You are a Claude agent"));
    // per-подписка cch/ccbuild стабильны и различаются между подписками (анти-кластер)
    assert_eq!(
        crate::upstream::persona_cch("a@x.io"),
        crate::upstream::persona_cch("a@x.io")
    );
    assert_ne!(
        crate::upstream::persona_cch("a@x.io"),
        crate::upstream::persona_cch("b@x.io")
    );
    let cb = crate::upstream::persona_ccbuild("a@x.io");
    assert_eq!(cb, crate::upstream::persona_ccbuild("a@x.io")); // стабилен
    assert!(
        cb.starts_with('d')
            && cb[1..]
                .parse::<u32>()
                .map(|n| (10..100).contains(&n))
                .unwrap_or(false),
        "формат dNN (10..99): {cb}"
    );
}

#[test]
fn endpoint_allowlist() {
    use super::Method;
    assert!(is_supported_endpoint(&Method::POST, "/v1/messages"));
    assert!(is_supported_endpoint(
        &Method::POST,
        "/v1/messages/count_tokens"
    ));
    assert!(is_supported_endpoint(&Method::GET, "/v1/models"));
    assert!(is_supported_endpoint(
        &Method::GET,
        "/v1/models/claude-haiku-4-5"
    ));
    // недоступное на подписке — отклоняем
    assert!(!is_supported_endpoint(
        &Method::POST,
        "/v1/messages/batches"
    ));
    assert!(!is_supported_endpoint(&Method::GET, "/v1/messages/batches"));
    assert!(!is_supported_endpoint(&Method::POST, "/v1/files"));
    assert!(!is_supported_endpoint(&Method::GET, "/v1/files"));
    assert!(!is_supported_endpoint(&Method::POST, "/v1/agents"));
    assert!(!is_supported_endpoint(&Method::POST, "/v1/complete")); // легаси
    assert!(!is_supported_endpoint(&Method::GET, "/v1/messages")); // messages только POST
    assert!(!is_supported_endpoint(&Method::DELETE, "/v1/models/x"));
    // C4: только один raw model-id сегмент; URL-normalized traversal/separators не проходят.
    assert!(!is_supported_endpoint(&Method::GET, "/v1/models/a/b"));
    assert!(!is_supported_endpoint(
        &Method::GET,
        "/v1/models/%2e%2e/%2e%2e/api/oauth/profile"
    ));
    assert!(!is_supported_endpoint(
        &Method::GET,
        "/v1/models/%2Fapi%2Foauth%2Fprofile"
    ));
    assert!(!is_supported_endpoint(
        &Method::GET,
        "/v1/models/..\\api\\oauth\\profile"
    ));
}

#[test]
fn beta_merge_preserves_client_capabilities_and_adds_only_identity() {
    let mut headers = HeaderMap::new();
    headers.append(
        "anthropic-beta",
        "task-budgets-2026-03-13,oauth-2025-04-20".parse().unwrap(),
    );
    headers.append(
        "anthropic-beta",
        "server-side-fallback-2026-06-01".parse().unwrap(),
    );
    let configured = "oauth-2025-04-20,claude-code-20250219,advisor-tool-2026-03-01";
    assert_eq!(merged_beta(&headers, configured).unwrap(),
        "task-budgets-2026-03-13,oauth-2025-04-20,server-side-fallback-2026-06-01,claude-code-20250219");
}

#[test]
fn persona_metadata_never_overwrites_or_panics_on_client_shape() {
    let mut supplied = serde_json::json!({"metadata":{"user_id":"hashed-customer-42"}});
    set_persona_user_id_if_absent(&mut supplied, "persona".into());
    assert_eq!(
        supplied["metadata"]["user_id"].as_str(),
        Some("hashed-customer-42")
    );

    let mut absent = serde_json::json!({"messages":[]});
    set_persona_user_id_if_absent(&mut absent, "persona".into());
    assert_eq!(absent["metadata"]["user_id"].as_str(), Some("persona"));

    let mut malformed = serde_json::json!({"metadata":"x"});
    set_persona_user_id_if_absent(&mut malformed, "persona".into());
    assert_eq!(malformed["metadata"].as_str(), Some("x"));
}

#[test]
fn ct_eq_is_correct() {
    assert!(ct_eq(b"secret-key", b"secret-key"));
    assert!(!ct_eq(b"secret-key", b"secret-keX"));
    assert!(!ct_eq(b"short", b"longer-key")); // разная длина
    assert!(ct_eq(b"", b""));
}

#[test]
fn every_client_credential_participates_without_header_priority() {
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", "stale-x-key".parse().unwrap());
    headers.insert("authorization", "bEaReR valid-bearer-key".parse().unwrap());
    headers.insert("x-goog-api-key", "stale-google-key".parse().unwrap());

    assert_eq!(
        client_keys(&headers),
        vec![
            "stale-google-key".to_string(),
            "stale-x-key".to_string(),
            "valid-bearer-key".to_string(),
        ]
    );
    assert_eq!(
        matching_key(&headers, &["valid-bearer-key".to_string()]),
        Some("valid-bearer-key".to_string())
    );

    headers.insert("x-api-key", "valid-x-key".parse().unwrap());
    headers.insert("authorization", "Bearer stale-bearer-key".parse().unwrap());
    assert_eq!(
        matching_key(&headers, &["valid-x-key".to_string()]),
        Some("valid-x-key".to_string())
    );

    headers.insert("x-goog-api-key", "valid-x-key".parse().unwrap());
    assert_eq!(
        client_keys(&headers)
            .iter()
            .filter(|key| key.as_str() == "valid-x-key")
            .count(),
        1,
        "the same credential in two headers must be checked only once"
    );
}

#[test]
fn calibration_target_is_admin_only_bounded_and_never_forwarded() {
    let mut headers = HeaderMap::new();
    headers.insert(
        CALIBRATION_PROFILE_HEADER,
        "besp".parse().expect("valid bounded profile hint"),
    );
    let admin = Authz::Admin {
        affinity_scope: "operator".to_string(),
    };
    assert_eq!(operator_calibration_target(&admin, &headers), Some("besp"));
    assert_eq!(
        operator_calibration_target(&Authz::Unauthorized, &headers),
        None,
        "a customer-controlled header cannot select a subscription"
    );
    assert!(skip_req_header(CALIBRATION_PROFILE_HEADER));

    headers.insert(
        CALIBRATION_PROFILE_HEADER,
        "too-long".parse().expect("syntactically valid header"),
    );
    assert_eq!(operator_calibration_target(&admin, &headers), None);
    assert_eq!(calibration_profile_hint("bespoke@example.com"), "besp");
}

#[tokio::test]
async fn metered_auth_accepts_any_valid_credential_deterministically() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-any-valid-auth-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let billing =
        crate::billing::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();
    billing
        .create_account("acct-a", None, 10_000)
        .await
        .unwrap();
    billing
        .create_account("acct-z", None, 10_000)
        .await
        .unwrap();
    billing
        .issue_key("a-valid", "acct-a", None, None, None)
        .await
        .unwrap();
    billing
        .issue_key("z-valid", "acct-z", None, None, None)
        .await
        .unwrap();

    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", "stale".parse().unwrap());
    headers.insert("authorization", "Bearer z-valid".parse().unwrap());
    let (key, auth) = resolve_client_key(&billing, &headers)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (key.as_str(), auth.account_id.as_str()),
        ("z-valid", "acct-z")
    );

    headers.insert("x-api-key", "z-valid".parse().unwrap());
    headers.insert("authorization", "Bearer stale".parse().unwrap());
    let (key, auth) = resolve_client_key(&billing, &headers)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (key.as_str(), auth.account_id.as_str()),
        ("z-valid", "acct-z")
    );

    // Если валидны оба, выбор зависит от канонического набора значений, а не от типа заголовка.
    headers.insert("x-api-key", "z-valid".parse().unwrap());
    headers.insert("authorization", "Bearer a-valid".parse().unwrap());
    let first = resolve_client_key(&billing, &headers)
        .await
        .unwrap()
        .unwrap();
    headers.insert("x-api-key", "a-valid".parse().unwrap());
    headers.insert("authorization", "Bearer z-valid".parse().unwrap());
    let second = resolve_client_key(&billing, &headers)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.0, "a-valid");
    assert_eq!(second.0, "a-valid");
    assert_eq!(first.1.account_id, second.1.account_id);

    drop(billing);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn authorize_keeps_nonsecret_key_id_separate_from_raw_billing_key() {
    use std::time::{SystemTime, UNIX_EPOCH};

    const RAW_SECRET_KEY: &str = "sk-pool-forward-secret-used-for-money-only";
    const NONSECRET_KEY_ID: &str = "key_forward_nonsecret_identity_d42c";
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-key-identity-auth-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    {
        let connection = registry::open(&path_string).unwrap();
        registry::account_create(&connection, "key-identity-account", None, 10_000).unwrap();
        registry::account_topup(&connection, "key-identity-account", 5_000, None).unwrap();
        registry::key_issue(&connection, RAW_SECRET_KEY, "key-identity-account", None).unwrap();
        connection
            .execute(
                "UPDATE api_keys SET key_id=?1 WHERE key=?2",
                (NONSECRET_KEY_ID, RAW_SECRET_KEY),
            )
            .unwrap();
    }
    let billing = Arc::new(AsyncBilling::start(path_string.clone(), 1).unwrap());
    let app = proxy_test_app(Arc::clone(&billing), &path_string);
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", RAW_SECRET_KEY.parse().unwrap());
    let peer = "192.0.2.1:443".parse().unwrap();

    let authz = authorize(&app, &headers, &peer).await;
    let Authz::Metered {
        account_id,
        key,
        key_id,
        available_nano,
        ..
    } = authz
    else {
        panic!("raw key should authorize as a metered credential");
    };
    assert_eq!(account_id, "key-identity-account");
    assert_eq!(key, RAW_SECRET_KEY);
    assert_eq!(key_id, NONSECRET_KEY_ID);
    assert_ne!(key, key_id);
    assert_eq!(available_nano, 5_000);

    assert_eq!(
        billing
            .reserve_request("raw-key-reserve", &account_id, &key, 400)
            .await
            .unwrap(),
        Some(4_600),
        "existing billing flows must continue to receive the raw credential",
    );
    assert_eq!(
        billing
            .reserve_request("key-id-must-not-reserve", &account_id, &key_id, 1)
            .await
            .unwrap(),
        None,
        "the non-secret identity must never be substituted into raw-key billing calls",
    );
    billing
        .settle_request("raw-key-reserve", &account_id, &key, 400, 300, None)
        .await
        .unwrap();
    let key_row = billing.get(RAW_SECRET_KEY).await.unwrap().unwrap();
    assert_eq!(key_row.key_id, NONSECRET_KEY_ID);
    assert_eq!(key_row.spent_nano, 300);
    assert_eq!(key_row.reserved_nano, 0);

    billing.flush().await.unwrap();
    drop(app);
    drop(billing);
    let _ = std::fs::remove_file(path);
}

#[test]
fn cap_to_balance_enforces_budget() {
    let p = metering::model_prices("claude-haiku-4-5"); // input 1000, output 5000, cw1h 2000
    let od = metering::OVERDRAFT_NANO;
    // ИНВАРИАНТ с овердрафт-буфером: hold ≤ bal+$1 (funded не роняем; резерв держит пол −$1),
    // charge(worst usage) ≤ hold, и +1 output-токен пробил бы bal+$1 (точность отруба «ни на токен больше»).
    for &m in &[10000i64, 2000, 900, 33333] {
        // ×1.0, ×0.2 (прод), ×0.09, ×3.33
        for &bal in &[500_000i128, 2_000_000, 50_000_000, 10_000_000_000] {
            let ib = 137i128; // байты входа
            if let Some((eff, hold)) = cap_to_balance(bal, ib, 0, &p, m, 100_000) {
                assert!(
                    (hold as i128) <= bal + od,
                    "hold {hold} > bal+$1 ({}) (m={m})",
                    bal + od
                );
                let real = ib * p.cache_write_1h + (eff as i128) * p.output; // worst-case usage
                assert!(
                    metering::apply_multiplier(real, m) <= hold as i128,
                    "charge > hold (m={m}, bal={bal}, eff={eff})"
                );
                // если урезали (eff < запрошенного) — +1 токен обязан пробить bal+$1
                if eff < 100_000 {
                    let over = ib * p.cache_write_1h + ((eff + 1) as i128) * p.output;
                    assert!(
                        metering::apply_multiplier(over, m) > bal + od,
                        "eff+1 должен пробить bal+$1 (m={m}, bal={bal}, eff={eff})"
                    );
                }
            }
        }
    }
    // большой баланс + большой запрос → НЕ режем (eff == запрошенное)
    let (eff, _) = cap_to_balance(1_000_000_000, 100, 0, &p, 2000, 50).unwrap();
    assert_eq!(eff, 50);
    // бесплатный ключ (наценка 0) → не лимитируем, hold 0
    assert_eq!(
        cap_to_balance(0, 999_999, 0, &p, 0, 12345),
        Some((12345, 0))
    );
    // funded (bal>0) НЕ роняем: овердрафт-буфер $1 покрывает — прежние балансовые «None» теперь Some
    assert!(cap_to_balance(100, 100_000, 0, &p, 2000, 10).is_some());
    assert!(cap_to_balance(0, 10, 0, &p, 2000, 10).is_some());
    // отказ ТОЛЬКО когда вход worst-case не влезает даже в bal+$1, либо аккаунт уже на полу −$1
    assert!(cap_to_balance(100, 600_000, 0, &p, 10000, 10).is_none());
    assert!(cap_to_balance(-od, 10, 0, &p, 2000, 10).is_none());
    // Переполнения нет: огромный баланс и max_tokens.
    let (_, h) = cap_to_balance(i64::MAX as i128, 100, 0, &p, 2000, u64::MAX).unwrap();
    assert!(h >= 0);
}

/// Все синтетические причины перебираем в одном месте (гарантия, что тест покрывает КАЖДУЮ).
const ALL_LOCAL_ERRS: [LocalErr; 9] = [
    LocalErr::Overloaded,
    LocalErr::RateLimited,
    LocalErr::InvalidKey,
    LocalErr::LowBalance,
    LocalErr::NotFound,
    LocalErr::BodyTooLarge,
    LocalErr::BadRequest,
    LocalErr::BadBeta,
    LocalErr::Internal,
];

#[test]
fn local_err_never_leaks_internal_architecture() {
    // Клиент считает, что говорит с api.anthropic.com. НИ ОДНО публичное поле (тип+сообщение)
    // синтетической ошибки не должно раскрывать наши внутренности: подписки, пул, upstream,
    // authority/fencing, cooling/ротацию, персоны/флот, oauth-инжект. Регрессия-гард: если кто-то
    // добавит вариант с текстом «no subscriptions…» — тест упадёт.
    let forbidden = [
        "subscription",
        "pool",
        "upstream",
        "authority",
        "cooling",
        "rotat",
        "persona",
        "fleet",
        "oauth",
        "in-house",
        "in house",
        "quota",
    ];
    for reason in ALL_LOCAL_ERRS {
        let (_code, kind, msg) = reason.parts();
        let hay = format!("{kind} {msg}").to_lowercase();
        for term in forbidden {
            assert!(
                !hay.contains(term),
                "{reason:?} leaks internal term {term:?}: {hay:?}"
            );
        }
    }
}

#[test]
fn local_err_maps_to_authentic_anthropic_triples() {
    // Статус+тип каждой причины совпадают с настоящим Anthropic (иначе ответ отличим от API).
    let cases = [
        (LocalErr::Overloaded, 529u16, "overloaded_error"),
        (LocalErr::RateLimited, 429, "rate_limit_error"),
        (LocalErr::InvalidKey, 401, "authentication_error"),
        (LocalErr::LowBalance, 402, "invalid_request_error"),
        (LocalErr::NotFound, 404, "not_found_error"),
        (LocalErr::BodyTooLarge, 413, "request_too_large"),
        (LocalErr::BadRequest, 400, "invalid_request_error"),
        (LocalErr::BadBeta, 400, "invalid_request_error"),
        (LocalErr::Internal, 500, "api_error"),
    ];
    for (reason, want_code, want_type) in cases {
        let (code, kind, _msg) = reason.parts();
        assert_eq!(code.as_u16(), want_code, "{reason:?} wrong status");
        assert_eq!(kind, want_type, "{reason:?} wrong error.type");
    }
    // overloaded=529 достижим (вне именованных констант http) и валиден.
    assert_eq!(http_overloaded().as_u16(), 529);
}

#[test]
fn local_err_body_is_anthropic_error_envelope() {
    // Тело — ровно Anthropic-конверт {"type":"error","error":{"type":...,"message":...}},
    // а Retry-After ставится только у retryable-причин.
    for reason in ALL_LOCAL_ERRS {
        let (_c, kind, msg) = reason.parts();
        let body = serde_json::json!({"type":"error","error":{"type":kind,"message":msg}});
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], kind);
        assert!(body["error"]["message"]
            .as_str()
            .map(|m| !m.is_empty())
            .unwrap_or(false));
    }
}

#[test]
fn local_err_carries_only_static_terminal_reason() {
    for reason in ALL_LOCAL_ERRS {
        let response = local_err(reason, None);
        assert_eq!(
            response
                .extensions()
                .get::<TerminalErrorReason>()
                .map(|value| value.0),
            Some(reason.reason())
        );
    }
    let response = local_err_for(LocalErr::LowBalance, "key_spend_limit", None);
    assert_eq!(
        response
            .extensions()
            .get::<TerminalErrorReason>()
            .map(|value| value.0),
        Some("key_spend_limit")
    );
}

#[test]
fn local_err_marks_every_synthetic_refusal_not_started() {
    // Каждый синтетический отказ local_err — не-2xx до границы доставки → обязан нести
    // x-apitoken-execution-state: not_started (с retry-after и без).
    for reason in ALL_LOCAL_ERRS {
        for retry_after in [None, Some(2)] {
            let response = local_err(reason, retry_after);
            assert!(!response.status().is_success());
            assert_eq!(
                response.headers().get(EXECUTION_STATE_HEADER).unwrap(),
                EXECUTION_STATE_NOT_STARTED,
                "{reason:?} обязан нести not_started"
            );
        }
    }
    // Страховка для веток после границы доставки: заголовок снимается.
    let response = without_not_started(local_err(LocalErr::Internal, None));
    assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
}

#[test]
fn exact_not_started_metric_predicate_matches_the_router_proof() {
    let response = with_not_started(local_err(LocalErr::Internal, None));
    assert!(is_exact_not_started_response(&response));

    let mut duplicate = with_not_started(local_err(LocalErr::Internal, None));
    duplicate.headers_mut().append(
        EXECUTION_STATE_HEADER,
        HeaderValue::from_static(EXECUTION_STATE_NOT_STARTED),
    );
    assert!(!is_exact_not_started_response(&duplicate));

    let mut wrong = local_err(LocalErr::Internal, None);
    wrong.headers_mut().insert(
        EXECUTION_STATE_HEADER,
        HeaderValue::from_static("NOT_STARTED"),
    );
    assert!(!is_exact_not_started_response(&wrong));

    let success = Response::builder()
        .status(StatusCode::OK)
        .header(EXECUTION_STATE_HEADER, EXECUTION_STATE_NOT_STARTED)
        .body(Body::empty())
        .unwrap();
    assert!(!is_exact_not_started_response(&success));
}

#[test]
fn anthropic_universal_chat_and_responses_persist_exactly_one_postgres_fact_each() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Anthropic universal fact rows: test URL is unset");
        return;
    };
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut connection = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    connection
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    connection
        .batch_execute(
            "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations, \
             capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts, \
             pool_state,subs RESTART IDENTITY CASCADE",
        )
        .unwrap();
    pg.account_create(COUNT_FACT_ACCOUNT_ID, None, 10_000)
        .unwrap();
    pg.account_topup(
        COUNT_FACT_ACCOUNT_ID,
        1_000_000_000,
        Some("anthropic-universal-facts"),
    )
    .unwrap();
    pg.key_issue(COUNT_FACT_RAW_KEY, COUNT_FACT_ACCOUNT_ID, None)
        .unwrap();
    let expected_key_id = pg.key_get(COUNT_FACT_RAW_KEY).unwrap().unwrap().key_id;
    pg.add(
        "pg-universal@example.test",
        "subscription-token",
        "",
        "test",
    )
    .unwrap();
    let owner = pg
        .claim_instance(
            &format!("anthropic-universal-facts-{}-{unique}", std::process::id()),
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
    let success_body = br#"{"type":"message","id":"msg_private","model":"claude-opus-4-7","content":[{"type":"text","text":"PUBLIC RESULT"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":3}}"#;
    let success_sse = br#"event: message_start
data: {"type":"message_start","message":{"id":"msg_private_stream","model":"claude-opus-4-7","usage":{"input_tokens":2,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"PUBLIC STREAM RESULT"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}

event: message_stop
data: {"type":"message_stop"}

"#;
    let (upstream, server) = spawn_universal_messages_upstream(vec![
        ("req-universal-chat", "application/json", success_body),
        ("req-universal-responses", "text/event-stream", success_sse),
        (
            "req-universal-missing-context",
            "application/json",
            success_body,
        ),
    ]);
    let mut cfg = (*proxy_test_config()).clone();
    cfg.upstream = upstream;
    let cfg = Arc::new(cfg);
    let app = AppState {
        provider: crate::ProviderMode::Anthropic,
        authority: Arc::new(registry::authority::AuthorityConfig::Postgres { url: url.clone() }),
        data_db_path: Arc::new(format!("/tmp/anthropic-universal-facts-{unique}")),
        pool: Arc::new(Pool::new(
            vec![Sub {
                email: "pg-universal@example.test".into(),
                token: "subscription-token".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            }],
            Reserve::FULL,
            1.0,
            1.0,
        )),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: None,
        codex: None,
        gemini: None,
        gemini_batch: None,
            gemini_batch_runtime: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing: Some(Arc::clone(&billing)),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(100)),
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
            let malformed_chat = crate::anthropic::anthropic_chat_completions(
                State(app.clone()),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                universal_request(
                    "/v1/chat/completions",
                    serde_json::json!({
                        "model":"anthropic/claude-opus-4-7",
                        "messages":[],
                        "stream":true,
                        "tools":[{"type":"function","function":{"name":"MALFORMED PRIVATE TOOL"}}]
                    }),
                ),
            )
            .await;
            assert_eq!(malformed_chat.status(), StatusCode::BAD_REQUEST);
            let malformed_responses = crate::anthropic_responses::anthropic_responses(
                State(app.clone()),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                universal_request(
                    "/v1/responses",
                    serde_json::json!({
                        "model":"anthropic/claude-opus-4-7",
                        "stream":"not-a-boolean",
                        "input":"MALFORMED PRIVATE INPUT"
                    }),
                ),
            )
            .await;
            assert_eq!(malformed_responses.status(), StatusCode::BAD_REQUEST);

            let chat = crate::anthropic::anthropic_chat_completions(
                State(app.clone()),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                universal_request(
                    "/v1/chat/completions",
                    serde_json::json!({
                        "model":"anthropic/claude-opus-4-7",
                        "messages":[{"role":"user","content":"PRIVATE CHAT PROMPT"}],
                        "max_completion_tokens":8,
                        "stream":false,
                        "tools":[{"type":"function","function":{"name":"PRIVATE CHAT TOOL","parameters":{"type":"object","properties":{"secret":{"type":"string"}}}}}],
                        "tool_choice":"required",
                        "parallel_tool_calls":false,
                        "response_format":{"type":"json_schema","json_schema":{"name":"PRIVATE CHAT FORMAT","schema":{"type":"object"}}},
                        "reasoning_effort":"high"
                    }),
                ),
            )
            .await;
            assert_eq!(chat.status(), StatusCode::OK);
            let _ = to_bytes(chat.into_body(), 1024 * 1024).await.unwrap();

            let responses = crate::anthropic_responses::anthropic_responses(
                State(app.clone()),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                universal_request(
                    "/v1/responses",
                    serde_json::json!({
                        "model":"anthropic/claude-opus-4-7",
                        "input":"PRIVATE RESPONSES PROMPT",
                        "max_output_tokens":8,
                        "stream":true,
                        "tools":[{"type":"function","name":"PRIVATE RESPONSES TOOL","parameters":{"type":"object","properties":{"secret":{"type":"string"}}}}],
                        "tool_choice":"required",
                        "parallel_tool_calls":false,
                        "text":{"format":{"type":"json_schema","name":"PRIVATE RESPONSES FORMAT","schema":{"type":"object"}}},
                        "reasoning":{"effort":"high"}
                    }),
                ),
            )
            .await;
            assert_eq!(responses.status(), StatusCode::OK);
            // Never poll the outer public stream. Dropping it must keep the existing TeeMeter
            // detach-and-drain settlement path and must not create a late or duplicate fact.
            drop(responses);

            let mut missing_context = universal_request(
                "/v1/chat/completions",
                serde_json::json!({
                    "model":"anthropic/claude-opus-4-7",
                    "messages":[{"role":"user","content":"PRIVATE MISSING CONTEXT"}],
                    "max_completion_tokens":8
                }),
            );
            missing_context
                .extensions_mut()
                .remove::<crate::execution::RequestLifecycleClock>();
            let response = crate::anthropic::anthropic_chat_completions(
                State(app.clone()),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                missing_context,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let _ = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();

            let unauthorized = universal_request(
                "/v1/chat/completions",
                serde_json::json!({
                    "model":"anthropic/claude-opus-4-7",
                    "messages":[{"role":"user","content":"PRIVATE UNAUTHORIZED"}]
                }),
            );
            let mut unauthorized = unauthorized;
            unauthorized
                .headers_mut()
                .insert("x-api-key", "invalid".parse().unwrap());
            let response = crate::anthropic::anthropic_chat_completions(
                State(app.clone()),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                unauthorized,
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            billing.flush().await.unwrap();
        });
    server.join().unwrap();

    let rows = connection
        .query(
            "SELECT f.account_id,f.key_id,f.provider_plane,f.route_class,f.request_class, \
                    f.requested_model,f.executable_model,f.stream_flag,f.tools_declared_count, \
                    f.tool_classes,f.tool_choice_mode,f.parallel_tools_requested, \
                    f.structured_output_flag,f.reasoning_flag,f.delivery_started_at, \
                    f.provider_terminal_class,f.billing_outcome,o.state,f.http_status_code, \
                    f.delivery_state,f.downstream_disconnect,f.upstream_request_id, \
                    f.internal_attempt_count,f.tool_calls_in_output,f.terminal_at, \
                    f.first_public_byte_at,row_to_json(f)::text \
               FROM request_facts f \
               JOIN settlement_outbox o ON o.request_id=f.billing_request_id \
              WHERE f.logical_request_id=$1 ORDER BY f.request_class",
            &[&COUNT_FACT_LOGICAL_ID],
        )
        .unwrap();
    assert_eq!(
        rows.len(),
        2,
        "one leaf fact for Chat and one for Responses"
    );
    for (row, (request_class, stream_flag)) in
        rows.iter().zip([("chat", false), ("responses", true)])
    {
        assert_eq!(row.get::<_, String>(0), COUNT_FACT_ACCOUNT_ID);
        assert_eq!(row.get::<_, String>(1), expected_key_id);
        assert_eq!(row.get::<_, String>(2), "anthropic");
        assert_eq!(row.get::<_, String>(3), "universal");
        assert_eq!(row.get::<_, String>(4), request_class);
        assert_eq!(
            row.get::<_, Option<String>>(5).as_deref(),
            Some("anthropic/claude-opus-4-7")
        );
        assert_eq!(row.get::<_, Option<String>>(6), None);
        assert_eq!(row.get::<_, bool>(7), stream_flag);
        assert_eq!(row.get::<_, Option<i32>>(8), Some(1));
        assert_eq!(
            row.get::<_, Option<i32>>(9),
            Some(registry::request_facts::TOOL_CLASS_CUSTOM_FUNCTION)
        );
        assert_eq!(row.get::<_, String>(10), "required");
        assert_eq!(row.get::<_, Option<bool>>(11), Some(false));
        assert_eq!(row.get::<_, Option<bool>>(12), Some(true));
        assert_eq!(row.get::<_, Option<bool>>(13), Some(true));
        assert!(row.get::<_, Option<i64>>(14).is_some());
        assert_eq!(row.get::<_, String>(15), "success");
        assert_eq!(row.get::<_, String>(16), "winner");
        assert_eq!(row.get::<_, String>(17), "done");
        assert_eq!(row.get::<_, Option<i32>>(18), Some(200));
        assert_eq!(row.get::<_, String>(19), "completed");
        assert_eq!(
            row.get::<_, Option<bool>>(20),
            Some(request_class == "responses")
        );
        assert_eq!(
            row.get::<_, Option<String>>(21).as_deref(),
            Some(if request_class == "chat" {
                "req-universal-chat"
            } else {
                "req-universal-responses"
            })
        );
        assert_eq!(row.get::<_, Option<i32>>(22), Some(1));
        assert_eq!(row.get::<_, Option<bool>>(23), Some(false));
        assert!(row.get::<_, Option<i64>>(24).is_some());
        // This test invokes the leaf adapters directly, outside the server's final public body
        // observer, so no first-byte time may be invented for either lifecycle.
        assert_eq!(row.get::<_, Option<i64>>(25), None);
        let row_json = row.get::<_, String>(26);
        for private in [
            COUNT_FACT_RAW_KEY,
            "PRIVATE CHAT PROMPT",
            "PRIVATE CHAT TOOL",
            "PRIVATE CHAT FORMAT",
            "PRIVATE RESPONSES PROMPT",
            "PRIVATE RESPONSES TOOL",
            "PRIVATE RESPONSES FORMAT",
            "MALFORMED PRIVATE TOOL",
            "MALFORMED PRIVATE INPUT",
            "PRIVATE MISSING CONTEXT",
            "PRIVATE UNAUTHORIZED",
            "secret",
        ] {
            assert!(!row_json.contains(private));
        }
    }
    connection
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    drop(billing);
}

#[test]
fn native_billable_messages_admission_delivery_and_terminal_share_postgres_money_lifecycle() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Anthropic billable Messages fact row: test URL is unset");
        return;
    };
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut connection = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    connection
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    connection
        .batch_execute(
            "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations, \
             capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts, \
             pool_state,subs RESTART IDENTITY CASCADE",
        )
        .unwrap();
    pg.account_create(COUNT_FACT_ACCOUNT_ID, None, 10_000)
        .unwrap();
    pg.account_topup(
        COUNT_FACT_ACCOUNT_ID,
        1_000_000_000,
        Some("anthropic-billable-fact"),
    )
    .unwrap();
    pg.key_issue(COUNT_FACT_RAW_KEY, COUNT_FACT_ACCOUNT_ID, None)
        .unwrap();
    let expected_key_id = pg.key_get(COUNT_FACT_RAW_KEY).unwrap().unwrap().key_id;
    pg.add("pg-message@example.test", "subscription-token", "", "test")
        .unwrap();
    pg.add(
        "pg-message-rotation@example.test",
        "subscription-token-rotation",
        "",
        "test",
    )
    .unwrap();
    let owner = pg
        .claim_instance(
            &format!("anthropic-billable-fact-{}-{unique}", std::process::id()),
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
    let success_body = br#"{"type":"message","id":"msg_private","model":"claude-test-served","content":[{"type":"tool_use","id":"toolu_private","name":"PRIVATE TOOL","input":{"secret":true}}],"stop_reason":"tool_use","usage":{"input_tokens":2,"output_tokens":3}}"#;
    let (upstream, server) = spawn_messages_upstream(
        1,
        vec![
            ("content-type", "application/json"),
            ("request-id", "req-pg-billable"),
        ],
        success_body,
    );
    let mut cfg = (*proxy_test_config()).clone();
    cfg.upstream = upstream;
    cfg.max_tries = 2;
    let cfg = Arc::new(cfg);
    let app = AppState {
        provider: crate::ProviderMode::Anthropic,
        authority: Arc::new(registry::authority::AuthorityConfig::Postgres { url: url.clone() }),
        data_db_path: Arc::new(format!("/tmp/anthropic-billable-fact-{unique}")),
        pool: Arc::new(Pool::new(
            vec![
                Sub {
                    email: "pg-message@example.test".into(),
                    token: "subscription-token".into(),
                    proxy: String::new(),
                    fleet: "test".into(),
                    plan: "max20".into(),
                },
                Sub {
                    email: "pg-message-rotation@example.test".into(),
                    token: "subscription-token-rotation".into(),
                    proxy: String::new(),
                    fleet: "test".into(),
                    plan: "max20".into(),
                },
            ],
            Reserve::FULL,
            1.0,
            1.0,
        )),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: None,
        codex: None,
        gemini: None,
        gemini_batch: None,
            gemini_batch_runtime: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing: Some(Arc::clone(&billing)),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(100)),
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
            let mut request = native_count_tokens_request(
                serde_json::json!({
                    "model":"claude-test",
                    "max_tokens":8,
                    "messages":[{"role":"user","content":"PRIVATE PG PROMPT"}],
                    "tools":[{"name":"PRIVATE PG TOOL","input_schema":{"type":"object"}}]
                }),
                true,
                Some("claude_code/2.1.220"),
            );
            *request.uri_mut() = "/v1/messages".parse().unwrap();
            let response = forward(
                State(app),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                request,
            )
            .await;
            assert_eq!(response.status(), StatusCode::CREATED);
            // Fully exhaust the body: TeeMeter remains authoritative for usage and terminal fact.
            assert_eq!(
                to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap()
                    .as_ref(),
                success_body
            );
            billing.flush().await.unwrap();
        });
    server.join().unwrap();

    let row = connection
        .query_one(
            "SELECT account_id,key_id,provider_plane,route_class,request_class,requested_model, \
                    executable_model,stream_flag,tools_declared_count,delivery_started_at, \
                    http_status_code,provider_terminal_class,delivery_state,billing_outcome, \
                    downstream_disconnect,upstream_request_id,internal_attempt_count,tool_calls_in_output, \
                    terminal_at \
               FROM request_facts WHERE logical_request_id=$1",
            &[&COUNT_FACT_LOGICAL_ID],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), COUNT_FACT_ACCOUNT_ID);
    assert_eq!(row.get::<_, String>(1), expected_key_id);
    assert_eq!(row.get::<_, String>(2), "anthropic");
    assert_eq!(row.get::<_, String>(3), "native");
    assert_eq!(row.get::<_, String>(4), "messages");
    assert_eq!(
        row.get::<_, Option<String>>(5).as_deref(),
        Some("claude-test")
    );
    assert_eq!(row.get::<_, Option<String>>(6), None);
    assert!(!row.get::<_, bool>(7));
    assert_eq!(row.get::<_, Option<i32>>(8), Some(1));
    assert!(row.get::<_, Option<i64>>(9).is_some());
    assert_eq!(row.get::<_, Option<i32>>(10), Some(201));
    assert_eq!(row.get::<_, String>(11), "success");
    assert_eq!(row.get::<_, String>(12), "completed");
    assert_eq!(row.get::<_, String>(13), "winner");
    assert_eq!(row.get::<_, Option<bool>>(14), Some(false));
    assert_eq!(
        row.get::<_, Option<String>>(15).as_deref(),
        Some("req-pg-billable")
    );
    assert_eq!(row.get::<_, Option<i32>>(16), Some(2));
    assert_eq!(row.get::<_, Option<bool>>(17), Some(true));
    assert!(row.get::<_, Option<i64>>(18).is_some());
    let row_json = connection
        .query_one(
            "SELECT row_to_json(request_facts)::text FROM request_facts WHERE logical_request_id=$1",
            &[&COUNT_FACT_LOGICAL_ID],
        )
        .unwrap()
        .get::<_, String>(0);
    for private in [
        COUNT_FACT_RAW_KEY,
        "PRIVATE PG PROMPT",
        "PRIVATE PG TOOL",
        "secret",
    ] {
        assert!(!row_json.contains(private));
    }
    connection
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    drop(billing);
}

#[test]
fn native_count_tokens_terminal_fact_persists_privacy_bounded_postgres_row() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Anthropic count_tokens fact row: test URL is unset");
        return;
    };
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut connection = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    connection
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    connection
        .batch_execute(
            "TRUNCATE request_facts,execution_group_winner,settlement_outbox,reservations, \
             capacity_leases,leader_leases,engine_instances,usage_events,ledger,api_keys,accounts, \
             pool_state,subs RESTART IDENTITY CASCADE",
        )
        .unwrap();
    pg.account_create(COUNT_FACT_ACCOUNT_ID, None, 10_000)
        .unwrap();
    pg.account_topup(COUNT_FACT_ACCOUNT_ID, 1_000, Some("anthropic-count-fact"))
        .unwrap();
    pg.key_issue(COUNT_FACT_RAW_KEY, COUNT_FACT_ACCOUNT_ID, None)
        .unwrap();
    let expected_key_id = pg.key_get(COUNT_FACT_RAW_KEY).unwrap().unwrap().key_id;
    pg.add("pg-count@example.test", "subscription-token", "", "test")
        .unwrap();
    let owner = pg
        .claim_instance(
            &format!("anthropic-count-fact-{}-{unique}", std::process::id()),
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
    let success_body = br#"{"input_tokens":13}"#;
    let (upstream, server) = spawn_count_upstream(vec![(
        200,
        vec![("request-id", "req-pg-terminal")],
        success_body,
    )]);
    let mut cfg = (*proxy_test_config()).clone();
    cfg.upstream = upstream;
    let cfg = Arc::new(cfg);
    let app = AppState {
        provider: crate::ProviderMode::Anthropic,
        authority: Arc::new(registry::authority::AuthorityConfig::Postgres { url: url.clone() }),
        data_db_path: Arc::new(format!("/tmp/anthropic-count-fact-{unique}")),
        pool: Arc::new(Pool::new(
            vec![Sub {
                email: "pg-count@example.test".into(),
                token: "subscription-token".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            }],
            Reserve::FULL,
            1.0,
            1.0,
        )),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        body_storage: None,
        codex: None,
        gemini: None,
        gemini_batch: None,
            gemini_batch_runtime: None,
        kimi: None,
        glm: None,
        tripo3d: None,
        suno: None,
        billing: Some(Arc::clone(&billing)),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(100)),
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
            let response = forward(
                State(app),
                ConnectInfo("192.0.2.1:443".parse().unwrap()),
                native_count_tokens_request(
                    serde_json::json!({
                        "model":"claude-pg-test",
                        "messages":[{"role":"user","content":"PRIVATE PG PROMPT"}],
                        "tools":[{"name":"PRIVATE PG TOOL","input_schema":{"type":"object"}}]
                    }),
                    true,
                    Some("claude_code/2.1.220"),
                ),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        });
    server.join().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let row = loop {
        if let Some(row) = connection
            .query_opt(
                "SELECT account_id,key_id,client_kind,client_version,provider_plane,route_class, \
                        request_class,requested_model,executable_model,tools_declared_count,tool_classes, \
                        http_status_code,provider_terminal_class,delivery_state,billing_outcome, \
                        upstream_request_id,internal_attempt_count \
                   FROM request_facts WHERE logical_request_id=$1",
                &[&COUNT_FACT_LOGICAL_ID],
            )
            .unwrap()
        {
            break row;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fact was not persisted"
        );
        std::thread::yield_now();
    };
    assert_eq!(row.get::<_, String>(0), COUNT_FACT_ACCOUNT_ID);
    assert_eq!(row.get::<_, String>(1), expected_key_id);
    assert_ne!(row.get::<_, String>(1), COUNT_FACT_RAW_KEY);
    assert_eq!(row.get::<_, String>(2), "claude_code");
    assert_eq!(row.get::<_, Option<String>>(3).as_deref(), Some("2.1.220"));
    assert_eq!(row.get::<_, String>(4), "anthropic");
    assert_eq!(row.get::<_, String>(5), "native");
    assert_eq!(row.get::<_, String>(6), "count_tokens");
    assert_eq!(
        row.get::<_, Option<String>>(7).as_deref(),
        Some("claude-pg-test")
    );
    assert_eq!(row.get::<_, Option<String>>(8), None);
    assert_eq!(row.get::<_, Option<i32>>(9), Some(1));
    assert_eq!(
        row.get::<_, Option<i32>>(10),
        Some(registry::request_facts::TOOL_CLASS_CUSTOM_FUNCTION)
    );
    assert_eq!(row.get::<_, Option<i32>>(11), Some(200));
    assert_eq!(row.get::<_, String>(12), "success");
    assert_eq!(row.get::<_, String>(13), "started");
    assert_eq!(row.get::<_, String>(14), "not_applicable");
    assert_eq!(
        row.get::<_, Option<String>>(15).as_deref(),
        Some("req-pg-terminal")
    );
    assert_eq!(row.get::<_, Option<i32>>(16), Some(1));
    let row_json = connection
        .query_one(
            "SELECT row_to_json(request_facts)::text FROM request_facts WHERE logical_request_id=$1",
            &[&COUNT_FACT_LOGICAL_ID],
        )
        .unwrap()
        .get::<_, String>(0);
    for private in [COUNT_FACT_RAW_KEY, "PRIVATE PG PROMPT", "PRIVATE PG TOOL"] {
        assert!(!row_json.contains(private));
    }
    connection
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    drop(billing);
}
