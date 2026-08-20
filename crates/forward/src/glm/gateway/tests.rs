use super::*;
use glm_credential::{encode_envelope, CredentialKeyring, GlmCredentialKind, GlmPlan};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tokio::sync::mpsc as async_mpsc;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let root = std::env::temp_dir().join(format!("glm-gateway-{suffix}"));
        fs::create_dir_all(root.join("credentials")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(root.join("credentials"), fs::Permissions::from_mode(0o700)).unwrap();
        Self { root }
    }

    fn publish_profile(&self, api_key: &str, base_url: &str) {
        self.publish_profile_as("glm-01", api_key, base_url);
    }

    fn publish_profile_as(&self, id: &str, api_key: &str, base_url: &str) {
        self.publish_profiles(&[(id, api_key)], base_url);
    }

    fn publish_profiles(&self, profiles: &[(&str, &str)], base_url: &str) {
        let ring = keyring();
        let mut entries = Vec::with_capacity(profiles.len());
        for (id, api_key) in profiles {
            let credential = GlmCredential {
                version: 1,
                kind: GlmCredentialKind::PlanKey,
                api_key: (*api_key).into(),
                plan: GlmPlan::Pro,
                base_url: base_url.into(),
                proxy_url: String::new(),
            };
            let envelope = ring.seal("a1", id, &credential).unwrap();
            let credential_path = self.root.join("credentials").join(format!("{id}.json"));
            write_private(&credential_path, &encode_envelope(&envelope).unwrap());
            entries.push(json!({
                "id": id,
                "credential_file": credential_path.to_string_lossy(),
            }));
        }
        write_private(
            &self.root.join("profiles.json"),
            &serde_json::to_vec(&json!({"profiles": entries})).unwrap(),
        );
    }

    fn publish_empty_roster(&self) {
        write_private(
            &self.root.join("profiles.json"),
            &serde_json::to_vec(&json!({"profiles": []})).unwrap(),
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn keyring() -> CredentialKeyring {
    CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
}

fn config(root: &Path, base_url: &str) -> GlmPlaneConfig {
    let _ = base_url; // GLM has no fleet-wide base url: the console origin is per-profile.
    GlmPlaneConfig {
        roster_dir: root.to_path_buf(),
        keyring: keyring(),
        transport: super::super::transport::GlmTransportConfig {
            auth_scheme: super::super::transport::AuthScheme::Bearer,
            request_timeout: Duration::from_secs(5),
            identity: super::super::transport::GlmIdentityHeaders::default(),
        },
        readiness_probe: ProbeRoute::Quota,
        quota_poll_interval: Duration::from_secs(300),
    }
}

fn http_status_response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes()
    .into_iter()
    .chain(body.iter().copied())
    .collect()
}

fn http_response(content_type: &str, body: &[u8]) -> Vec<u8> {
    http_status_response("200 OK", content_type, body)
}

fn mock_server(responses: Vec<Vec<u8>>) -> (String, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            sender.send(request).unwrap();
            stream.write_all(&response).unwrap();
        }
    });
    (format!("http://{address}"), receiver)
}

fn controlled_mock_server(
    requests: usize,
) -> (
    String,
    async_mpsc::UnboundedReceiver<Vec<u8>>,
    mpsc::Sender<Vec<u8>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sender, request_receiver) = async_mpsc::unbounded_channel();
    let (response_sender, response_receiver) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for _ in 0..requests {
            let (mut stream, _) = listener.accept().unwrap();
            request_sender.send(read_request(&mut stream)).unwrap();
            let Ok(response) = response_receiver.recv() else {
                return;
            };
            stream.write_all(&response).unwrap();
        }
    });
    (
        format!("http://{address}"),
        request_receiver,
        response_sender,
    )
}

/// SSE server that proves incremental passthrough: the first frame is flushed to the
/// client and the rest of the stream is held until the test confirms the frame arrived.
fn gated_sse_server() -> (String, mpsc::Receiver<Vec<u8>>, mpsc::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sender, request_receiver) = mpsc::channel();
    let (gate_sender, gate_receiver) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request_sender.send(read_request(&mut stream)).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
            )
            .unwrap();
        let write_chunk = |stream: &mut TcpStream, data: &[u8]| {
            stream
                .write_all(format!("{:x}\r\n", data.len()).as_bytes())
                .unwrap();
            stream.write_all(data).unwrap();
            stream.write_all(b"\r\n").unwrap();
            stream.flush().unwrap();
        };
        write_chunk(
            &mut stream,
            br#"data: {"type":"message_start","message":{"model":"glm-5.2","usage":{"input_tokens":10}}}"#
                .as_slice(),
        );
        write_chunk(&mut stream, b"\n");
        // Hold the terminal frames until the client proves the first one arrived.
        gate_receiver.recv().unwrap();
        write_chunk(
            &mut stream,
            br#"data: {"type":"message_delta","usage":{"output_tokens":4}}"#.as_slice(),
        );
        write_chunk(&mut stream, b"\n\n");
        write_chunk(&mut stream, br#"data: {"type":"message_stop"}"#.as_slice());
        write_chunk(&mut stream, b"\n\n");
        stream.write_all(b"0\r\n\r\n").unwrap();
        stream.flush().unwrap();
    });
    (format!("http://{address}"), request_receiver, gate_sender)
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    let mut scratch = [0u8; 4096];
    let mut expected = None;
    loop {
        let read = stream.read(&mut scratch).unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&scratch[..read]);
        if expected.is_none() {
            if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                expected = Some(end + 4 + length);
            }
        }
        if expected.is_some_and(|expected| request.len() >= expected) {
            break;
        }
    }
    request
}

fn affinity() -> Arc<AffinityStore> {
    Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap())
}

fn request_body(model: &str) -> Value {
    json!({
        "model": model,
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "hello"}],
    })
}

fn glm_request(model: &str, body: Value, billing: Option<GlmBillingInput>) -> GlmRequest {
    GlmRequest {
        raw_body_len: serde_json::to_vec(&body).unwrap().len(),
        body,
        model: model.into(),
        execution: ExecutionAttempt::direct(),
        billing,
        affinity: None,
        affinity_store: affinity(),
    }
}

fn turn_event(request_id: &str) -> GlmTurnCalibrationEvent {
    GlmTurnCalibrationEvent {
        request_id: request_id.into(),
        subject_id: "subject-1".into(),
        plan: "Pro".into(),
        requested_model: "glm-5.2".into(),
        served_model: "glm-5.2".into(),
        context_mode: "200k".into(),
        reasoning_effort: Some("high".into()),
        api_tariff_schedule_id: GLM_TARIFF_SCHEDULE_ID.into(),
        credit_schedule_id: GLM_CREDIT_SCHEDULE_ID.into(),
        priced_ts: 1_800_000_000,
        completed_at: 1_800_000_001,
        fresh_input_tokens: 1,
        cached_input_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 1,
        reasoning_tokens: 0,
        api_fresh_input_nanousd: 1_400,
        api_cached_input_nanousd: 0,
        api_output_nanousd: 4_400,
        api_total_nanousd: 5_800,
        native_fresh_input_microcredits: 69,
        native_cached_input_microcredits: 0,
        native_output_microcredits: 240,
        native_total_microcredits: 309,
        off_peak: false,
    }
}

/// A valid quota envelope: the documented credits form with both independent windows.
fn quota_body(used_5h: i64, limit_5h: i64, used_week: i64, limit_week: i64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "code": 200, "msg": "success", "success": true,
        "data": {
            "limits": [
                {"type":"TIME_LIMIT","unit":5,"number":limit_5h,"usage":used_5h,
                 "currentValue":limit_5h-used_5h,"remaining":limit_5h-used_5h,
                 "percentage":100.0*used_5h as f64/limit_5h as f64,
                 "nextResetTime":4_102_444_800_000i64},
                {"type":"TIME_LIMIT","unit":7,"number":limit_week,"usage":used_week,
                 "currentValue":limit_week-used_week,"remaining":limit_week-used_week,
                 "percentage":100.0*used_week as f64/limit_week as f64,
                 "nextResetTime":4_102_500_000_000i64}
            ],
            "usageDetails": [{"modelCode":"glm-5.2"}]
        }
    }))
    .unwrap()
}

fn quota_snapshot(
    duration_secs: i64,
    used: i64,
    limit: i64,
    resets_at: i64,
    observed_at: i64,
) -> GlmQuotaSnapshot {
    let derived = registry::glm_fraction_from_native(used, limit).unwrap();
    GlmQuotaSnapshot {
        window_duration_secs: duration_secs,
        resets_at: Some(resets_at),
        observed_at,
        native_used_units: Some(used),
        native_limit_units: Some(limit),
        native_remaining_units: Some(limit - used),
        percentage_raw: None,
        used_fraction_units: Some(derived.used_fraction_units),
        measurement_resolution_fraction_units: Some(derived.measurement_resolution_fraction_units),
    }
}

async fn wait_for_pending(gateway: &GlmGateway, expected: usize) {
    for _ in 0..100 {
        if gateway.operational_status().delivery.pending_events == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("turn FIFO never reached {expected} pending events");
}

#[tokio::test]
async fn a_pending_turn_blocks_the_provider_quota_read() {
    let fixture = Fixture::new();
    let (base_url, mut requests, _responses) = controlled_mock_server(1);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
    gateway
        .turn_queue
        .lock()
        .unwrap()
        .push(turn_event("pending-before-poll"));

    assert_eq!(gateway.poll_quotas().await, 0);
    assert_eq!(gateway.operational_status().delivery.pending_events, 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), requests.recv())
            .await
            .is_err(),
        "the quota GET must not run past an undelivered spend head"
    );
}

#[tokio::test]
async fn customer_generation_start_invalidates_a_concurrent_quota_snapshot_without_waiting() {
    let fixture = Fixture::new();
    let (base_url, mut requests, responses) = controlled_mock_server(1);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    let profile = gateway.profiles_snapshot()[0].clone();

    let poll = {
        let gateway = gateway.clone();
        tokio::spawn(async move { gateway.poll_quotas().await })
    };
    let request = requests.recv().await.unwrap();
    assert!(request.starts_with(b"GET /api/monitor/usage/quota/limit "));

    // No semaphore or maintenance wait: a customer lease starts immediately while the GET is
    // outstanding. Its epoch makes the returned snapshot unusable for calibration.
    let lease = ProfileLease::new(profile.clone());
    assert_eq!(profile.inflight.load(Ordering::Acquire), 1);
    responses
        .send(http_response(
            "application/json",
            &quota_body(10, 12_000, 0, 60_000),
        ))
        .unwrap();
    assert_eq!(poll.await.unwrap(), 0);
    drop(lease);
    assert_eq!(
        profile.candidate("glm-5.2", now_unix()).window_5h,
        None
    );
}

#[tokio::test]
async fn transient_observation_failure_keeps_the_previous_quota_generation() {
    let fixture = Fixture::new();
    let (base_url, requests) = mock_server(vec![http_response(
        "application/json",
        &quota_body(10, 12_000, 0, 60_000),
    )]);
    fixture.publish_profile("zai-key-1", &base_url);
    let sqlite = fixture.root.join("billing.sqlite");
    let billing = Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
    let gateway =
        GlmGateway::new_with_calibration(config(&fixture.root, &base_url), Some(billing)).unwrap();
    let profile = gateway.profiles_snapshot()[0].clone();

    // SQLite deliberately refuses GLM calibration. A successful provider GET is not enough
    // to publish steering before the durable PostgreSQL observation/CAS succeeds.
    assert_eq!(gateway.poll_quotas().await, 0);
    assert!(requests
        .recv()
        .unwrap()
        .starts_with(b"GET /api/monitor/usage/quota/limit "));
    let candidate = profile.candidate("glm-5.2", now_unix());
    assert_eq!(candidate.window_5h, None);
    assert_eq!(candidate.window_weekly, None);
    assert_eq!(candidate.quota_age_secs, None);
}

#[test]
fn a_durable_snapshot_publishes_the_tightest_window_and_exact_full_reset() {
    let fixture = Fixture::new();
    fixture.publish_profile("zai-key-1", "http://127.0.0.1:1");
    let gateway =
        GlmGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:1"), None)
            .unwrap();
    let profile = gateway.profiles_snapshot()[0].clone();
    let observed_at = now_unix();
    let snapshots = vec![
        quota_snapshot(
            registry::GLM_5H_WINDOW_SECS,
            250,
            1_000,
            observed_at + 300,
            observed_at,
        ),
        quota_snapshot(
            registry::GLM_WEEKLY_WINDOW_SECS,
            1_000,
            1_000,
            observed_at + 600,
            observed_at,
        ),
    ];
    profile.publish_quota(&snapshots, observed_at);
    let candidate = profile.candidate("glm-5.2", observed_at);
    // R7: оба окна доходят до селектора раздельно — 5h на 25% и weekly на 100%, каждое со
    // своим reset (в секундах до него), вместо прежней свёртки в одно max-число.
    assert_eq!(
        candidate.window_5h,
        Some(WindowEvidence {
            used_fraction_units: Some(registry::GLM_FRACTION_SCALE / 4),
            reset_in_secs: Some(300),
        })
    );
    assert_eq!(
        candidate.window_weekly,
        Some(WindowEvidence {
            used_fraction_units: Some(registry::GLM_FRACTION_SCALE),
            reset_in_secs: Some(600),
        })
    );
    assert_eq!(candidate.quota_age_secs, Some(0));
    assert_eq!(candidate.ineligible, Some(Ineligible::QuotaWall));
    assert_eq!(
        profile.health.lock().unwrap().quota_cool_until,
        observed_at + 600
    );
}

#[tokio::test]
async fn quota_poll_failures_stay_profile_local_and_classify_on_the_business_code() {
    let code_401 =
        br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#.as_slice();
    let quota_wall = br#"{"error":{"code":"1308","message":"5-hour quota exhausted"}}"#.as_slice();
    let fair_use = br#"{"error":{"code":"1313","message":"fair use violation"}}"#.as_slice();
    let empty = br#"{}"#.as_slice();
    for (status, body, expected) in [
        ("401 Unauthorized", empty, Some(Ineligible::AccountDead)),
        // The trap of this endpoint: HTTP 200 carrying code:401 is a dead key.
        ("200 OK", code_401, Some(Ineligible::AccountDead)),
        (
            "429 Too Many Requests",
            quota_wall,
            Some(Ineligible::QuotaWall),
        ),
        (
            "429 Too Many Requests",
            fair_use,
            Some(Ineligible::AccountSuspect),
        ),
        (
            "503 Service Unavailable",
            empty,
            Some(Ineligible::TransportWedged),
        ),
    ] {
        let fixture = Fixture::new();
        let (base_url, requests) =
            mock_server(vec![http_status_response(status, "application/json", body)]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
        let profile = gateway.profiles_snapshot()[0].clone();
        profile.mark_probe_healthy();
        gateway.live_profiles.store(1, Ordering::Release);

        assert_eq!(gateway.poll_quotas().await, 0, "status {status}");
        assert_eq!(gateway.profiles_snapshot().len(), 1, "status {status}");
        assert_eq!(
            profile.candidate("glm-5.2", now_unix()).ineligible,
            expected,
            "status {status}"
        );
        assert!(requests
            .recv()
            .unwrap()
            .starts_with(b"GET /api/monitor/usage/quota/limit "));
        // No same-profile retry: a refused static key is terminal, not a refresh prompt.
        assert!(requests.try_recv().is_err(), "status {status}");
    }
}

#[tokio::test]
async fn a_profile_removed_during_quota_io_is_never_reintroduced() {
    let fixture = Fixture::new();
    let (base_url, mut requests, responses) = controlled_mock_server(1);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    let poll = {
        let gateway = gateway.clone();
        tokio::spawn(async move { gateway.poll_quotas().await })
    };
    assert!(requests
        .recv()
        .await
        .unwrap()
        .starts_with(b"GET /api/monitor/usage/quota/limit "));

    fixture.publish_empty_roster();
    assert!(gateway.refresh_profiles().await);
    assert!(gateway.profiles_snapshot().is_empty());
    responses
        .send(http_response(
            "application/json",
            &quota_body(10, 12_000, 0, 60_000),
        ))
        .unwrap();
    assert_eq!(poll.await.unwrap(), 0);
    assert!(gateway.profiles_snapshot().is_empty());
    assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));
}

#[tokio::test]
async fn shutdown_cancels_the_steady_poll_and_bounds_its_final_quota_read() {
    let fixture = Fixture::new();
    let (base_url, mut requests, responses) = controlled_mock_server(2);
    fixture.publish_profile("zai-key-1", &base_url);
    let mut test_config = config(&fixture.root, &base_url);
    test_config.transport.request_timeout = Duration::from_secs(30);
    let gateway = Arc::new(GlmGateway::new_with_calibration(test_config, None).unwrap());
    let steady = {
        let gateway = gateway.clone();
        tokio::spawn(async move { gateway.poll_quotas().await })
    };
    assert!(requests
        .recv()
        .await
        .unwrap()
        .starts_with(b"GET /api/monitor/usage/quota/limit "));

    let started = tokio::time::Instant::now();
    let stopping = {
        let gateway = gateway.clone();
        tokio::spawn(async move {
            gateway
                .shutdown_until(Some(started + Duration::from_millis(300)))
                .await
        })
    };
    // The test server handles connections serially. Let it retire the cancelled steady-state
    // socket so the final bounded shutdown attempt can be observed on the next connection.
    tokio::time::sleep(Duration::from_millis(20)).await;
    responses
        .send(http_status_response(
            "503 Service Unavailable",
            "application/json",
            br#"{}"#,
        ))
        .unwrap();
    assert_eq!(steady.await.unwrap(), 0);
    // The regular request was cancelled by shutdown; one final ordered attempt was permitted
    // inside the existing process deadline and cancelled at that boundary.
    assert!(requests
        .recv()
        .await
        .unwrap()
        .starts_with(b"GET /api/monitor/usage/quota/limit "));
    stopping.await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "provider I/O must not extend shutdown past the bounded deadline"
    );
}

#[tokio::test]
async fn exact_alias_uses_quota_readiness_and_transparent_messages_bytes() {
    let fixture = Fixture::new();
    let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
        http_response("application/json", generation),
    ]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    assert_eq!(gateway.readiness(), Ok(()));

    let body = request_body("glm-5.2");
    let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let returned = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap();
    // Anthropic-compatible upstream: public bytes pass through without a translation layer.
    assert_eq!(returned.as_ref(), generation);

    let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(probe.starts_with("GET /api/monitor/usage/quota/limit "));
    // The quota endpoint takes the raw key WITHOUT the Bearer prefix (wire contract §4).
    assert!(probe
        .to_ascii_lowercase()
        .contains("authorization: zai-key-1"));
    assert!(!probe.to_ascii_lowercase().contains("bearer"));
    let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(turn.starts_with("POST /api/anthropic/v1/messages "));
    assert!(turn
        .to_ascii_lowercase()
        .contains("authorization: bearer zai-key-1"));
    // Generation traffic carries the full Claude-Code-compatible identity set: Z.ai
    // risk-control bans SDK-like clients, so these headers keep the subscription alive.
    let turn_lower = turn.to_ascii_lowercase();
    assert!(turn_lower.contains("user-agent: claude-cli/"));
    assert!(turn_lower.contains("anthropic-version: 2023-06-01"));
    assert!(turn_lower.contains("x-client-request-id:"));
    assert!(turn.contains("\"model\":\"glm-5.2\""));
    // No calibration authority in this test: evidence remains visible in the bounded FIFO
    // instead of being silently discarded.
    assert_eq!(gateway.operational_status().delivery.pending_events, 1);
}

/// The body of a captured raw HTTP/1.1 request as parsed JSON.
fn captured_request_json(request: &str) -> Value {
    let body = request
        .split("\r\n\r\n")
        .nth(1)
        .expect("a captured request always carries a body");
    serde_json::from_str(body).expect("the generation body is JSON")
}

#[tokio::test]
async fn generation_carries_the_full_fleet_fingerprint_and_nothing_from_the_client() {
    let fixture = Fixture::new();
    let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
        http_response("application/json", generation),
    ]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    let response = gateway
        .handle(glm_request("glm-5.2", request_body("glm-5.2"), None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let _probe = requests.recv().unwrap();
    let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
    let turn = turn.to_ascii_lowercase();
    // The exact reviewed 2.1.195 capture, header by header.
    assert!(turn.contains("user-agent: claude-cli/2.1.195 (external, sdk-cli)"));
    assert!(turn.contains("anthropic-version: 2023-06-01"));
    let beta_line = turn
        .lines()
        .find(|line| line.starts_with("anthropic-beta: "))
        .expect("anthropic-beta is sent");
    for beta in [
        "oauth-2025-04-20",
        "interleaved-thinking-2025-05-14",
        "thinking-token-count-2026-05-13",
        "context-management-2025-06-27",
        "prompt-caching-scope-2026-01-05",
        "claude-code-20250219",
        "advisor-tool-2026-03-01",
        "advanced-tool-use-2025-11-20",
        "extended-cache-ttl-2025-04-11",
        "cache-diagnosis-2026-04-07",
    ] {
        assert!(beta_line.contains(beta), "missing beta {beta}");
    }
    assert!(turn.contains("x-app: cli"));
    assert!(turn.contains("x-stainless-lang: js"));
    assert!(turn.contains("x-stainless-runtime: node"));
    assert!(turn.contains("x-stainless-runtime-version: v26.3.0"));
    assert!(turn.contains("x-stainless-package-version: 0.94.0"));
    assert!(turn.contains("x-stainless-os: linux"));
    assert!(turn.contains("x-stainless-arch: x64"));
    assert!(turn.contains("accept: application/json"));
    assert!(turn.contains("anthropic-dangerous-direct-browser-access: true"));
    // Client identity headers structurally never enter the gateway: the wire shows the
    // synthesized persona and nothing a foreign SDK could have sent (no python, no curl).
    assert!(!turn.contains("python"));
    assert!(!turn.contains("curl"));
}

#[tokio::test]
async fn generation_body_carries_identity_first_and_a_per_profile_billing_block() {
    let fixture = Fixture::new();
    let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
        http_response("application/json", generation),
    ]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    let response = gateway
        .handle(glm_request("glm-5.2", request_body("glm-5.2"), None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let _probe = requests.recv().unwrap();
    let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
    let body = captured_request_json(&turn);
    let system = body["system"].as_array().expect("system is an array");
    // Exactly the real client's order: billing header first, identity second, no
    // cache_control on either (a cache-controlled identity is not what Claude Code sends).
    assert_eq!(system.len(), 2);
    let billing = system[0]["text"].as_str().unwrap();
    assert!(
        billing.starts_with("x-anthropic-billing-header: cc_version=2.1.195.d"),
        "billing block: {billing}"
    );
    assert!(billing.contains("; cc_entrypoint=sdk-cli; cch="));
    assert!(billing.ends_with(';'));
    assert_eq!(system.len(), 2, "billing and identity only");
    // Deterministic per profile: the fixture profile is "glm-01", so the gateway must emit
    // exactly the derivation the persona function produces for that id.
    let expected =
        super::super::transport::GlmIdentityHeaders::default().billing_header_for("glm-01");
    assert_eq!(billing, expected);
    assert_eq!(
        system[1]["text"].as_str().unwrap(),
        "You are a Claude agent, built on Anthropic's Claude Agent SDK."
    );
    assert!(system[0].get("cache_control").is_none());
    assert!(system[1].get("cache_control").is_none());
}

#[tokio::test]
async fn a_genuine_claude_code_body_is_never_doubled() {
    let fixture = Fixture::new();
    let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
        http_response("application/json", generation),
    ]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    // The customer IS Claude Code: its own identity block is already first. The gateway
    // must not stack a second identity; the per-profile billing block still goes first
    // (replacing nothing here — the client's billing marker would be replaced in place).
    let mut body = request_body("glm-5.2");
    body["system"] = json!([{
        "type": "text",
        "text": "You are a Claude agent, built on Anthropic's Claude Agent SDK."
    }]);
    let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let _probe = requests.recv().unwrap();
    let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
    let body = captured_request_json(&turn);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 2, "no stacked identity: {system:?}");
    assert!(system[0]["text"]
        .as_str()
        .unwrap()
        .starts_with("x-anthropic-billing-header:"));
    assert_eq!(
        system[1]["text"].as_str().unwrap(),
        "You are a Claude agent, built on Anthropic's Claude Agent SDK."
    );
}

#[tokio::test]
async fn billing_injection_can_be_switched_off_without_losing_the_identity() {
    let fixture = Fixture::new();
    let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
        http_response("application/json", generation),
    ]);
    fixture.publish_profile("zai-key-1", &base_url);
    let mut config = config(&fixture.root, &base_url);
    config.transport.identity.inject_billing = false;
    let gateway = Arc::new(GlmGateway::new_with_calibration(config, None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    let response = gateway
        .handle(glm_request("glm-5.2", request_body("glm-5.2"), None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let _probe = requests.recv().unwrap();
    let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(!turn.contains("x-anthropic-billing-header"));
    let body = captured_request_json(&turn);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 1);
    assert_eq!(
        system[0]["text"].as_str().unwrap(),
        "You are a Claude agent, built on Anthropic's Claude Agent SDK."
    );
}

#[tokio::test]
async fn the_quota_probe_carries_no_identity_set() {
    let fixture = Fixture::new();
    let (base_url, requests) = mock_server(vec![http_response(
        "application/json",
        &quota_body(0, 12_000, 0, 60_000),
    )]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);

    let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(probe.starts_with("GET /api/monitor/usage/quota/limit "));
    // The quota endpoint is a monitor surface, not generation: none of the Claude Code
    // fingerprint may ride on it.
    let probe = probe.to_ascii_lowercase();
    for absent in [
        "anthropic-version",
        "anthropic-beta",
        "x-stainless",
        "x-app",
        "x-anthropic-billing-header",
        "anthropic-dangerous-direct-browser-access",
        "claude-cli",
    ] {
        assert!(!probe.contains(absent), "probe must not carry {absent}");
    }
}

#[tokio::test]
async fn a_rejected_key_quarantines_its_profile_without_taking_down_the_fleet() {
    let fixture = Fixture::new();
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
        http_response(
            "application/json",
            br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#,
        ),
    ]);
    fixture.publish_profiles(
        &[("glm-01", "zai-key-good"), ("glm-02", "zai-key-dead")],
        &base_url,
    );
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();

    // One dead key quarantines exactly its own profile (manifest §4.2): the business code
    // decides, and the rest of the fleet — let alone the whole gateway — stays up.
    assert_eq!(gateway.preflight().await, 1);
    assert_eq!(gateway.readiness(), Ok(()));
    let status = gateway.operational_status();
    assert_eq!(status.live_profiles, 1);
    assert_eq!(status.account_dead_profiles, 1);
    let dead = status
        .profiles
        .iter()
        .find(|profile| profile.id == "glm-02")
        .unwrap();
    assert!(dead.account_dead);
    assert!(!dead.live);
    assert!(status
        .profiles
        .iter()
        .any(|profile| profile.id == "glm-01" && profile.live));
    // Exactly one probe per profile: a static key is never retried in place.
    assert!(requests.recv().is_ok());
    assert!(requests.recv().is_ok());
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn a_cold_degraded_gateway_adopts_a_new_profile_only_after_a_quota_probe() {
    let fixture = Fixture::new();
    let (base_url, requests) = mock_server(vec![http_response(
        "application/json",
        &quota_body(0, 12_000, 0, 60_000),
    )]);
    let gateway = GlmGateway::new_degraded(config(&fixture.root, &base_url), None);
    assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

    fixture.publish_profile("zai-key-1", &base_url);
    assert!(gateway.refresh_profiles().await);
    assert_eq!(gateway.readiness(), Ok(()));
    assert_eq!(gateway.operational_status().total_profiles, 1);
    let request = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(request.starts_with("GET /api/monitor/usage/quota/limit "));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: zai-key-1"));
}

#[tokio::test]
async fn an_unchanged_roster_reuses_the_exact_profile_without_another_probe() {
    let fixture = Fixture::new();
    let (base_url, requests) = mock_server(vec![http_response(
        "application/json",
        &quota_body(0, 12_000, 0, 60_000),
    )]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();

    assert!(!gateway.refresh_profiles().await);
    assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
    assert_eq!(gateway.readiness(), Ok(()));
    assert!(requests
        .recv()
        .unwrap()
        .starts_with(b"GET /api/monitor/usage/quota/limit "));
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn broken_or_disappeared_rosters_retain_the_last_good_ready_profile() {
    let fixture = Fixture::new();
    let (base_url, _requests) = mock_server(vec![http_response(
        "application/json",
        &quota_body(0, 12_000, 0, 60_000),
    )]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();

    write_private(&fixture.root.join("profiles.json"), br#"{"profiles":["#);
    assert!(!gateway.refresh_profiles().await);
    assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
    assert_eq!(gateway.readiness(), Ok(()));

    fs::remove_file(fixture.root.join("profiles.json")).unwrap();
    assert!(!gateway.refresh_profiles().await);
    assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
    assert_eq!(gateway.readiness(), Ok(()));
}

#[tokio::test]
async fn an_explicit_empty_roster_removes_new_admission_but_not_an_existing_lease() {
    let fixture = Fixture::new();
    let (base_url, _requests) = mock_server(vec![http_response(
        "application/json",
        &quota_body(0, 12_000, 0, 60_000),
    )]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();
    let lease = ProfileLease::new(original.clone());
    assert_eq!(original.inflight.load(Ordering::Acquire), 1);

    fixture.publish_empty_roster();
    assert!(gateway.refresh_profiles().await);
    assert!(gateway.profiles_snapshot().is_empty());
    assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));
    assert_eq!(original.inflight.load(Ordering::Acquire), 1);
    drop(lease);
    assert_eq!(original.inflight.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn a_changed_credential_is_published_only_after_a_successful_quota_probe() {
    let fixture = Fixture::new();
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
        http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
    ]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();

    fixture.publish_profile("zai-key-2", &base_url);
    assert!(gateway.refresh_profiles().await);
    let replacement = gateway.profiles_snapshot()[0].clone();
    assert!(!Arc::ptr_eq(&original, &replacement));
    assert_eq!(gateway.readiness(), Ok(()));

    let first = String::from_utf8(requests.recv().unwrap()).unwrap();
    let second = String::from_utf8(requests.recv().unwrap()).unwrap();
    // Replacement arrives as an atomic republication with a new static key; the probe
    // authenticates with the raw key, never a Bearer prefix.
    assert!(first
        .to_ascii_lowercase()
        .contains("authorization: zai-key-1"));
    assert!(!first.to_ascii_lowercase().contains("bearer"));
    assert!(second
        .to_ascii_lowercase()
        .contains("authorization: zai-key-2"));
}

#[tokio::test]
async fn a_failed_probe_for_a_changed_credential_keeps_the_old_ready_snapshot() {
    let fixture = Fixture::new();
    let rejected = http_response(
        "application/json",
        br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#,
    );
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
        rejected,
    ]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();

    fixture.publish_profile("zai-key-rejected", &base_url);
    assert!(!gateway.refresh_profiles().await);
    assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
    assert_eq!(gateway.readiness(), Ok(()));
    for expected in ["zai-key-1", "zai-key-rejected"] {
        let request = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(request
            .to_ascii_lowercase()
            .contains(&format!("authorization: {expected}")));
    }
}

#[tokio::test]
async fn final_verification_never_publishes_a_credential_rotated_during_probe() {
    let fixture = Fixture::new();
    let (base_url, mut requests, responses) = controlled_mock_server(2);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    let original = gateway.profiles_snapshot()[0].clone();
    original.mark_probe_healthy();
    gateway.live_profiles.store(1, Ordering::Release);

    fixture.publish_profile("candidate-secret", &base_url);
    let reload = {
        let gateway = gateway.clone();
        tokio::spawn(async move { gateway.refresh_profiles().await })
    };
    let first = String::from_utf8(
        tokio::time::timeout(Duration::from_secs(5), requests.recv())
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert!(first
        .to_ascii_lowercase()
        .contains("authorization: candidate-secret"));

    // Simulate the other blue-green generation atomically republishing the shared roster
    // after this generation loaded it but before its candidate probe completed.
    fixture.publish_profile("peer-rotated-secret", &base_url);
    responses
        .send(http_response(
            "application/json",
            &quota_body(0, 12_000, 0, 60_000),
        ))
        .unwrap();

    let second = String::from_utf8(
        tokio::time::timeout(Duration::from_secs(5), requests.recv())
            .await
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert!(second
        .to_ascii_lowercase()
        .contains("authorization: peer-rotated-secret"));
    responses
        .send(http_response(
            "application/json",
            &quota_body(0, 12_000, 0, 60_000),
        ))
        .unwrap();

    assert!(reload.await.unwrap());
    let published = gateway.profiles_snapshot()[0].clone();
    assert_eq!(published.credential.api_key, "peer-rotated-secret");
    assert!(!Arc::ptr_eq(&original, &published));
    assert_eq!(gateway.readiness(), Ok(()));
}

#[tokio::test]
async fn degraded_gateway_keeps_exact_aliases_on_a_zero_capacity_glm_path() {
    let fixture = Fixture::new();
    let gateway = Arc::new(GlmGateway::new_degraded(
        config(&fixture.root, "https://example.invalid"),
        None,
    ));
    assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

    let body = request_body("glm-5.2");
    let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response
            .extensions()
            .get::<crate::proxy::TerminalErrorReason>()
            .map(|reason| reason.0),
        Some("glm_capacity_exhausted")
    );
    let body = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["type"], "rate_limit_error");
    let lowered = String::from_utf8_lossy(&body).to_ascii_lowercase();
    for private in ["glm", "zhipu", "z.ai"] {
        assert!(!lowered.contains(private), "leaked {private}: {lowered}");
    }
}

#[tokio::test]
async fn every_synthetic_failure_uses_the_shared_anthropic_sanitizer() {
    for failure in [
        GatewayFailure::Auth,
        GatewayFailure::Suspect,
        GatewayFailure::Transport,
        GatewayFailure::Protocol,
        GatewayFailure::Capacity,
        GatewayFailure::LowBalance,
        GatewayFailure::BadRequest("glm_private_request_reason"),
        GatewayFailure::Unsupported("glm_private_capability_reason"),
        GatewayFailure::Unavailable("glm_private_runtime_reason"),
        GatewayFailure::Upstream(400),
        GatewayFailure::Upstream(404),
        GatewayFailure::Upstream(429),
        GatewayFailure::Upstream(503),
    ] {
        let response = error_response(failure);
        assert!(!response.status().is_success());
        assert!(crate::proxy::is_exact_not_started_response(&response));
        let body = axum::body::to_bytes(response.into_body(), ERROR_BODY_LIMIT)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&body).to_ascii_lowercase();
        for private in [
            "glm",
            "zhipu",
            "z.ai",
            "subscription",
            "roster",
            "upstream",
            "provider",
        ] {
            assert!(
                !body.contains(private),
                "{failure:?} leaked {private}: {body}"
            );
        }
    }
}

#[test]
fn sse_accounting_survives_split_frames_and_requires_a_terminal_event() {
    let mut accounting = SseAccounting::default();
    accounting
        .push(br#"data: {"type":"message_start","message":{"model":"glm-5.2","usage":{"input_tokens":10}}}"#)
        .unwrap();
    assert!(!accounting.terminal);
    accounting
        .push(b"\n\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":4}}\n")
        .unwrap();
    accounting
        .push(b"\ndata: {\"type\":\"message_stop\"}\n\n")
        .unwrap();
    assert!(accounting.terminal);
    assert!(accounting.usage_seen);
    assert_eq!(accounting.usage.input_tokens, 10);
    assert_eq!(accounting.usage.output_tokens, 4);
    assert_eq!(accounting.served_model.as_deref(), Some("glm-5.2"));
}

#[tokio::test]
async fn the_turn_fifo_holds_order_and_a_transient_head_blocks_the_tail() {
    let fixture = Fixture::new();
    fixture.publish_profile("zai-key-1", "http://127.0.0.1:1");
    let sqlite = fixture.root.join("billing.sqlite");
    let billing = Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
    let gateway = GlmGateway::new_with_calibration(
        config(&fixture.root, "http://127.0.0.1:1"),
        Some(billing),
    )
    .unwrap();
    // Readiness needs a live profile first; only then can degraded delivery block it.
    gateway.profiles_snapshot()[0].mark_probe_healthy();
    gateway.live_profiles.store(1, Ordering::Release);

    // SQLite refuses GLM calibration, so every write is transient: the head must stay in
    // place and hold the tail — a later turn may never overtake an undelivered one.
    gateway.enqueue_turn(turn_event("glm-fifo-a")).await;
    gateway.enqueue_turn(turn_event("glm-fifo-b")).await;
    let queue = gateway.turn_queue.lock().unwrap();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.head().unwrap().request_id, "glm-fifo-a");
    drop(queue);
    let delivery = gateway.operational_status().delivery;
    assert_eq!(delivery.pending_events, 2);
    assert!(!delivery.persistence_ok);
    assert_eq!(gateway.readiness(), Err(NotReady::DeliveryDegraded));
}

#[tokio::test]
async fn a_stream_passthrough_is_incremental_and_never_replays_after_the_first_byte() {
    let fixture = Fixture::new();
    let (base_url, requests, gate) = gated_sse_server();
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    let profile = gateway.profiles_snapshot()[0].clone();
    profile.mark_probe_healthy();
    gateway.live_profiles.store(1, Ordering::Release);

    let mut body = request_body("glm-5.2");
    body["stream"] = json!(true);
    let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.into_body().into_data_stream();
    let first = futures_util::StreamExt::next(&mut stream)
        .await
        .unwrap()
        .unwrap();
    // The first public byte arrived while the upstream still held the rest of the stream:
    // passthrough is genuinely incremental, not a buffered frame.
    assert!(String::from_utf8_lossy(&first).contains("message_start"));
    gate.send(()).unwrap();

    let mut rest = Vec::new();
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        rest.extend_from_slice(&chunk.unwrap());
    }
    let rest = String::from_utf8_lossy(&rest);
    assert!(rest.contains("message_delta"));
    assert!(rest.contains("message_stop"));

    // One upstream request, no replay: retry after the first public byte is forbidden.
    assert!(requests.recv().is_ok());
    assert!(requests.try_recv().is_err());
    wait_for_pending(&gateway, 1).await;
    let queue = gateway.turn_queue.lock().unwrap();
    let event = queue.head().unwrap();
    assert_eq!(event.served_model, "glm-5.2");
    assert_eq!(event.fresh_input_tokens, 10);
    assert_eq!(event.output_tokens, 4);
}

#[tokio::test]
async fn a_client_disconnect_still_drains_upstream_to_terminal_usage() {
    let fixture = Fixture::new();
    let sse = br#"data: {"type":"message_start","message":{"model":"glm-5.2","usage":{"input_tokens":10}}}

data: {"type":"message_delta","usage":{"output_tokens":4}}

data: {"type":"message_stop"}

"#
    .as_slice();
    let (base_url, _requests) = mock_server(vec![http_response("text/event-stream", sse)]);
    fixture.publish_profile("zai-key-1", &base_url);
    let gateway =
        Arc::new(GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap());
    let profile = gateway.profiles_snapshot()[0].clone();
    profile.mark_probe_healthy();
    gateway.live_profiles.store(1, Ordering::Release);

    let mut body = request_body("glm-5.2");
    body["stream"] = json!(true);
    let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.into_body().into_data_stream();
    let first = futures_util::StreamExt::next(&mut stream)
        .await
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&first).contains("message_start"));
    // The client goes away mid-stream. Delivery stops, but the bounded task keeps draining
    // the provider to terminal usage so settlement stays exact.
    drop(stream);

    wait_for_pending(&gateway, 1).await;
    let queue = gateway.turn_queue.lock().unwrap();
    let event = queue.head().unwrap();
    assert_eq!(event.served_model, "glm-5.2");
    assert_eq!(event.fresh_input_tokens, 10);
    assert_eq!(event.output_tokens, 4);
    assert!(event.native_total_microcredits > 0);
}

#[test]
fn reservation_cap_is_an_integer_upper_bound_and_never_crosses_the_overdraft_floor() {
    let prices = glm_prices_for_served_model("glm-5.2", 1).unwrap();
    let balance = 1_000_000i128;
    let (tokens, hold) = cap_to_balance(balance, 100, prices, 10_000, 1_000_000).unwrap();
    assert!(tokens > 0);
    assert!(i128::from(hold) <= balance + metering::OVERDRAFT_NANO);
    let raw = 100 * prices.input + i128::from(tokens) * prices.output;
    assert_eq!(i128::from(hold), metering::apply_multiplier(raw, 10_000));
    assert!(cap_to_balance(-metering::OVERDRAFT_NANO, 1, prices, 10_000, 1).is_none());
    assert_eq!(cap_to_balance(0, 1, prices, 0, 777), Some((777, 0)));
}

#[test]
fn settlement_actual_is_not_capped_by_the_reservation_hold() {
    let hold = 17i64;
    let actual = customer_actual(2 * metering::OVERDRAFT_NANO, 10_000);
    assert_eq!(i128::from(actual), 2 * metering::OVERDRAFT_NANO);
    assert!(i128::from(actual) > i128::from(hold) + metering::OVERDRAFT_NANO);
}

#[test]
fn output_reserve_bound_is_also_enforced_on_the_forwarded_body() {
    let mut body = json!({"max_tokens": u64::MAX});
    assert_eq!(
        bounded_requested_output(&mut body),
        MAX_REQUESTED_OUTPUT_TOKENS
    );
    assert_eq!(body["max_tokens"], MAX_REQUESTED_OUTPUT_TOKENS);

    let mut defaulted = json!({});
    assert_eq!(bounded_requested_output(&mut defaulted), 4_096);
    assert!(defaulted.get("max_tokens").is_none());
}

#[test]
fn unknown_money_surfaces_fail_closed_before_transport() {
    // Tools, web search and MCP surfaces are `unavailable` in v1 (manifest §3/§5.1): their
    // per-request ceiling is unproven, so the plane must not spend budget on them.
    assert!(matches!(
        validate_priced_surface(&json!({"tools": [{"name": "search"}]})),
        Err(GatewayFailure::Unsupported("glm_tools_unavailable"))
    ));
    for body in [
        json!({"tools": "provider-default"}),
        json!({"tool_choice": {"type": "auto"}}),
        json!({"mcp_servers": [{"name": "zread"}]}),
        json!({"messages": [{"content": [{"type": "tool_result"}]}]}),
        json!({"messages": [{"content": [{"type": "web_search_tool_result"}]}]}),
    ] {
        assert!(matches!(
            validate_priced_surface(&body),
            Err(GatewayFailure::Unsupported("glm_tools_unavailable"))
        ));
    }
    // Vision is outside the reviewed trio and fails closed the same way.
    assert!(matches!(
        validate_priced_surface(&json!({
            "messages": [{"content": [{"type": "image", "source": {}}]}]
        })),
        Err(GatewayFailure::Unsupported("glm_media_unavailable"))
    ));
}

#[test]
fn reasoning_effort_maps_only_for_glm_5_2_and_follows_the_provider_mapping() {
    assert_eq!(
        reasoning_effort("glm-5.2", &json!({})).unwrap(),
        Some("high".into())
    );
    assert_eq!(
        reasoning_effort("glm-5.2[1m]", &json!({})).unwrap(),
        Some("high".into())
    );
    for (raw, mapped) in [
        ("xhigh", "max"),
        ("max", "max"),
        ("medium", "high"),
        ("low", "high"),
        ("minimal", "off"),
        ("none", "off"),
    ] {
        assert_eq!(
            reasoning_effort("glm-5.2", &json!({"reasoning_effort": raw})).unwrap(),
            Some(mapped.into()),
            "{raw}"
        );
    }
    assert_eq!(
        reasoning_effort("glm-5.2", &json!({"thinking": {"type": "disabled"}})).unwrap(),
        Some("off".into())
    );
    assert!(reasoning_effort("glm-5.2", &json!({"reasoning_effort": "invented"})).is_err());
    // Models without a reasoning effort carry none at all (manifest §3).
    assert_eq!(reasoning_effort("glm-4.7", &json!({})).unwrap(), None);
    assert_eq!(reasoning_effort("glm-5-turbo", &json!({})).unwrap(), None);
}

#[test]
fn context_mode_marks_only_the_1m_selector_spelling() {
    assert_eq!(context_mode("glm-5.2[1m]"), "1m");
    assert_eq!(context_mode("glm-5.2"), "200k");
    assert_eq!(context_mode("glm-5-turbo"), "200k");
    assert_eq!(context_mode("glm-4.7"), "200k");
}

#[test]
fn model_is_glm_accepts_only_the_exact_reviewed_aliases() {
    for alias in [
        "glm-5.2",
        "glm-5.2[1m]",
        "glm-5-turbo",
        "glm-4.7",
        "GLM-5.2",
    ] {
        assert!(GlmGateway::model_is_glm(alias), "{alias}");
    }
    // Historical/echoed ids are NOT admission aliases: they never dispatch here.
    for other in [
        "glm-5",
        "glm-5.1",
        "glm-4.5",
        "glm-4.6v",
        "glm-5.2-highspeed",
        "claude-sonnet-4-6",
        "kimi-for-coding",
        "",
    ] {
        assert!(!GlmGateway::model_is_glm(other), "{other}");
    }
}

#[test]
fn price_turn_computes_both_ledgers_from_the_served_model() {
    // Monday 15:00 SGT is peak; Monday 12:00 SGT is off-peak (official schedule, UTC+8).
    const PEAK: i64 = 4 * 86_400 + 15 * 3_600 - 8 * 3_600;
    const OFF_PEAK: i64 = 4 * 86_400 + 12 * 3_600 - 8 * 3_600;
    let usage = GlmUsage {
        input_tokens: 1_000,
        cache_read_tokens: 500,
        cache_write_tokens: 50,
        output_tokens: 100,
        reasoning_output_tokens: 40,
    };
    let peak = price_turn(&usage, "glm-5.2", 1, PEAK).unwrap();
    assert!(!peak.off_peak);
    // API ledger (glm-5.2: 260/1_400/4_400 nanoUSD): the cache write carries the miss rate.
    assert_eq!(peak.input, 1_000 * 1_400);
    assert_eq!(peak.cache_read, 500 * 260);
    assert_eq!(peak.cache_write, 50 * 1_400);
    assert_eq!(peak.output, 100 * 4_400);
    assert_eq!(peak.total, 2_040_000);
    // Native ledger (6.9/1.7/24 tenths): exact, no rounding, no cache-write leg.
    assert_eq!(peak.native_input, 1_000 * 69 * 10);
    assert_eq!(peak.native_cache_read, 500 * 17 * 10);
    assert_eq!(peak.native_output, 100 * 240 * 10);
    assert_eq!(peak.native_total, 1_015_000);

    let off_peak = price_turn(&usage, "glm-5.2", 1, OFF_PEAK).unwrap();
    assert!(off_peak.off_peak);
    // Off-peak is exactly half on the native ledger; the dollar ledger never changes.
    assert_eq!(off_peak.total, peak.total);
    assert_eq!(off_peak.native_total, peak.native_total / 2);

    // The immutable event folds cache-write cost into the fresh-input leg so the three
    // disjoint legs still sum to the total.
    let fixture = Fixture::new();
    fixture.publish_profile("zai-key-1", "http://127.0.0.1:1");
    let gateway =
        GlmGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:1"), None)
            .unwrap();
    let context = AccountingContext {
        request_id: "req-1".into(),
        requested_model: "glm-5.2".into(),
        context_mode: "200k".into(),
        reasoning_effort: Some("high".into()),
        priced_ts: 1,
        profile: gateway.profiles_snapshot()[0].clone(),
    };
    let event = peak
        .calibration_event(&context, &usage, "glm-5.2", PEAK)
        .unwrap();
    assert_eq!(event.api_fresh_input_nanousd, 1_470_000);
    assert_eq!(event.api_total_nanousd, 2_040_000);
    assert_eq!(event.native_total_microcredits, 1_015_000);
    assert!(!event.off_peak);

    // Fail closed: an echoed id without credit multipliers, an unknown id, a broken subset
    // invariant and an empty usage vector are never priced.
    assert!(price_turn(&usage, "glm-5", 1, PEAK).is_err());
    assert!(price_turn(&usage, "glm-9", 1, PEAK).is_err());
    let mut broken = usage.clone();
    broken.reasoning_output_tokens = broken.output_tokens + 1;
    assert!(price_turn(&broken, "glm-5.2", 1, PEAK).is_err());
    assert!(price_turn(&GlmUsage::default(), "glm-5.2", 1, PEAK).is_err());
}

#[tokio::test]
async fn the_status_projection_reports_cooling_axes_availability_inflight_and_counters() {
    let fixture = Fixture::new();
    fixture.publish_profile("zai-key-1", "http://127.0.0.1:9");
    let gateway =
        GlmGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9"), None)
            .unwrap();
    let now = now_unix();
    let profile = gateway.profiles_snapshot()[0].clone();

    let healthy = gateway.operational_status();
    assert_eq!(healthy.total_profiles, 1);
    assert_eq!(healthy.available_profiles, 1);
    assert_eq!(healthy.account_dead_profiles, 0);
    assert_eq!(healthy.account_suspect_profiles, 0);
    assert_eq!(healthy.transport_cooling_profiles, 0);
    assert_eq!(healthy.quota_cooling_profiles, 0);
    assert_eq!(healthy.inflight_requests, 0);
    assert_eq!(healthy.missing_terminal_usage, 0);
    assert_eq!(healthy.served_model_rejected, 0);
    assert_eq!(healthy.profiles[0].id, "glm-01");
    assert_eq!(healthy.profiles[0].plan, "Pro");
    assert!(!healthy.profiles[0].account_dead);
    assert!(!healthy.profiles[0].account_suspect);
    assert_eq!(healthy.profiles[0].transport_cool_until, None);
    assert_eq!(healthy.profiles[0].quota_cool_until, None);
    assert_eq!(healthy.profiles[0].quota_observed_at, None);
    assert_eq!(healthy.profiles[0].quota_windows, Vec::new());
    assert!(!healthy.profiles[0].live);

    profile.inflight.store(3, Ordering::Release);
    profile.apply_effect(ProfileEffect::AccountDead, now, None, None);
    let dead = gateway.operational_status();
    assert_eq!(dead.available_profiles, 0);
    assert_eq!(dead.account_dead_profiles, 1);
    assert_eq!(dead.inflight_requests, 3);
    assert!(dead.profiles[0].account_dead);
    assert!(!dead.profiles[0].live);

    profile.apply_effect(ProfileEffect::TransportFault, now, None, None);
    let wedged = gateway.operational_status();
    assert_eq!(wedged.transport_cooling_profiles, 1);
    assert_eq!(
        wedged.profiles[0].transport_cool_until,
        Some(now + TRANSPORT_COOL_SECS)
    );

    // Model scope stays model-scoped: 1311 on one alias blocks nothing else.
    profile.mark_probe_healthy();
    profile.apply_effect(ProfileEffect::ModelIneligible, now, Some("glm-5.2"), None);
    assert_eq!(
        profile.candidate("glm-5.2", now).ineligible,
        Some(Ineligible::ModelIneligible)
    );
    assert_eq!(profile.candidate("glm-4.7", now).ineligible, None);
    // A successful generation rehabilitates exactly the served model's block.
    profile.mark_healthy("glm-5.2");
    assert_eq!(profile.candidate("glm-5.2", now).ineligible, None);
    assert!(profile.authenticated());

    // A quota wall with provider reset evidence cools to the exact reset, never a guess.
    profile.apply_effect(ProfileEffect::CoolUntilReset, now, None, Some(now + 3_600));
    assert_eq!(profile.health.lock().unwrap().quota_cool_until, now + 3_600);
    // …and without one it falls back to the bounded cool the next poll will refine.
    profile.apply_effect(ProfileEffect::CoolUntilReset, now, None, None);
    assert_eq!(
        profile.health.lock().unwrap().quota_cool_until,
        now + QUOTA_WALL_FALLBACK_COOL_SECS
    );
    // A probe alone proves auth, not quota state: the wall axis survives it and clears
    // only on a real quota snapshot or at its deadline.
    profile.mark_probe_healthy();
    assert_eq!(
        profile.candidate("glm-4.7", now).ineligible,
        Some(Ineligible::QuotaWall)
    );

    // Typed operational counters surface without any private identity.
    gateway.missing_terminal_usage.store(2, Ordering::Release);
    gateway.served_model_rejected.store(1, Ordering::Release);
    let counted = gateway.operational_status();
    assert_eq!(counted.missing_terminal_usage, 2);
    assert_eq!(counted.served_model_rejected, 1);
}

#[tokio::test]
async fn publish_quota_retains_the_exact_per_window_snapshot_with_unknowns_absent() {
    let fixture = Fixture::new();
    fixture.publish_profile("zai-key-1", "http://127.0.0.1:9");
    let gateway =
        GlmGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9"), None)
            .unwrap();
    let profile = gateway.profiles_snapshot()[0].clone();
    let observed_at = now_unix();
    let five_hour = quota_snapshot(18_000, 250, 1_000, 4_102_444_800, observed_at);
    // A window whose units are unproven keeps every raw field absent — never zero.
    let unproven = GlmQuotaSnapshot {
        window_duration_secs: 604_800,
        resets_at: None,
        observed_at,
        native_used_units: None,
        native_limit_units: None,
        native_remaining_units: None,
        percentage_raw: None,
        used_fraction_units: None,
        measurement_resolution_fraction_units: None,
    };
    profile.publish_quota(&[five_hour, unproven], observed_at);

    let status = gateway.operational_status();
    let projected = &status.profiles[0];
    assert_eq!(projected.quota_observed_at, Some(observed_at));
    assert_eq!(projected.quota_windows.len(), 2);
    let window = &projected.quota_windows[0];
    assert_eq!(window.duration_secs, 18_000);
    assert_eq!(window.used_units, Some(250));
    assert_eq!(window.limit_units, Some(1_000));
    assert_eq!(window.remaining_units, Some(750));
    assert_eq!(window.used_fraction_units, Some(25_000_000));
    assert_eq!(window.measurement_resolution_fraction_units, Some(100_000));
    assert_eq!(window.resets_at, Some(4_102_444_800));
    assert_eq!(window.observed_at, observed_at);
    let unknown = &projected.quota_windows[1];
    assert_eq!(unknown.used_units, None);
    assert_eq!(unknown.used_fraction_units, None);
    assert_eq!(unknown.resets_at, None);
    // No window is full: no quota cooling, and a successful poll authenticates the profile.
    assert_eq!(status.quota_cooling_profiles, 0);
    assert!(projected.live);
}

#[tokio::test]
async fn the_projection_bounds_plan_labels_and_cannot_carry_the_subject() {
    assert_eq!(bounded_plan_label("lite"), "Lite");
    assert_eq!(bounded_plan_label("pro"), "Pro");
    assert_eq!(bounded_plan_label("max"), "Max");
    // The runtime stores the capitalized declared form; the label mapping accepts both.
    assert_eq!(bounded_plan_label("Pro"), "Pro");
    assert_eq!(bounded_plan_label("Max"), "Max");
    assert_eq!(bounded_plan_label("enterprise"), "unreviewed");

    let fixture = Fixture::new();
    fixture.publish_profile("zai-key-1", "http://127.0.0.1:9");
    let gateway =
        GlmGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9"), None)
            .unwrap();
    let status = gateway.operational_status();
    assert_eq!(status.profiles[0].plan, "Pro");
    // The durable-calibration join resolves only through the opaque roster id; an unknown
    // subject resolves to nothing and its rows are dropped rather than serialized.
    let profile = gateway.profiles_snapshot()[0].clone();
    assert_eq!(
        gateway
            .profile_id_for_subject(&profile.subject_id)
            .as_deref(),
        Some("glm-01")
    );
    assert_eq!(gateway.profile_id_for_subject("subject-unknown"), None);
    let rendered = format!("{status:?}");
    assert!(!rendered.contains(&profile.subject_id));
    assert!(!rendered.contains("zai-key-1"));
}

/// Settlement replays the exact override version pinned at admission on BOTH ledgers (API
/// nanoUSD and native microcredits), never the compiled cards.
#[tokio::test]
async fn settlement_replays_the_pinned_override_version_on_both_ledgers() {
    // Monday 15:00 SGT is peak (official schedule, UTC+8).
    const PEAK: i64 = 4 * 86_400 + 15 * 3_600 - 8 * 3_600;
    let _lock = crate::pricing::tariff_book::GLOBAL_BOOK_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    // effective_from = i64::MAX keeps the rows invisible to every timestamped resolve — and so to
    // every concurrently running test; only the exact pinned-version lookup sees them.
    crate::pricing::tariff_book::install_global_rows_for_test(vec![
        crate::pricing::tariff_book::test_row(
            "zhipu/glm/glm-5.2",
            2,
            i64::MAX,
            serde_json::json!({
                "cached_input": "520",
                "input": "2800",
                "cache_write": "2800",
                "output": "8800"
            }),
        ),
    ]);
    let usage = GlmUsage {
        input_tokens: 1_000,
        cache_read_tokens: 500,
        cache_write_tokens: 50,
        output_tokens: 100,
        reasoning_output_tokens: 40,
    };
    let pin = PinnedTariff {
        family: "zhipu/glm/glm-5.2".to_owned(),
        version: 2,
        schedule_id: "zhipu/glm/glm-5.2/v2".to_owned(),
    };
    let priced = price_turn_settlement(None, &usage, "glm-5.2", 1, PEAK, Some(&pin))
        .await
        .unwrap();
    crate::pricing::tariff_book::clear_global_book_for_test();
    // API ledger at the override card (2_800/520/2_800/8_800).
    assert_eq!(priced.input, 1_000 * 2_800);
    assert_eq!(priced.cache_read, 500 * 520);
    assert_eq!(priced.cache_write, 50 * 2_800);
    assert_eq!(priced.output, 100 * 8_800);
    assert_eq!(priced.total, 2_800_000 + 260_000 + 140_000 + 880_000);
    assert!(!priced.off_peak);
    assert_eq!(priced.api_schedule_id, "zhipu/glm/glm-5.2/v2");
}

/// The credit-rate settlement under an override card: the native microcredit legs reprice from
/// the override multipliers with the off-peak schedule still code-applied on top.
#[test]
fn the_credit_ledger_reprices_under_an_override_card() {
    const PEAK: i64 = 4 * 86_400 + 15 * 3_600 - 8 * 3_600;
    const OFF_PEAK: i64 = 4 * 86_400 + 12 * 3_600 - 8 * 3_600;
    let usage = GlmUsage {
        input_tokens: 1_000,
        cache_read_tokens: 500,
        cache_write_tokens: 50,
        output_tokens: 100,
        reasoning_output_tokens: 40,
    };
    let prices = glm_prices_for_served_model("glm-5.2", 1).unwrap();
    let override_rates = metering::glm::GlmCreditRates {
        input_tenths: 138,
        cached_input_tenths: 34,
        output_tenths: 480,
    };
    let peak = price_turn_with_rates(
        &usage,
        prices,
        override_rates,
        PEAK,
        GLM_TARIFF_SCHEDULE_ID.to_string(),
        "zhipu/glm-credits/glm-5.2/v2".to_string(),
    )
    .unwrap();
    assert_eq!(peak.native_input, 1_000 * 138 * 10);
    assert_eq!(peak.native_cache_read, 500 * 34 * 10);
    assert_eq!(peak.native_output, 100 * 480 * 10);
    assert_eq!(peak.native_total, 1_380_000 + 170_000 + 480_000);
    assert_eq!(peak.credit_schedule_id, "zhipu/glm-credits/glm-5.2/v2");
    // The dollar legs are untouched by the credit override, and off-peak still halves the
    // native ledger.
    assert_eq!(peak.total, 2_040_000);
    let off_peak = price_turn_with_rates(
        &usage,
        prices,
        override_rates,
        OFF_PEAK,
        GLM_TARIFF_SCHEDULE_ID.to_string(),
        "zhipu/glm-credits/glm-5.2/v2".to_string(),
    )
    .unwrap();
    assert_eq!(off_peak.native_total, peak.native_total / 2);
}

/// A pinned version the book cannot produce is an integrity error: the turn fails closed to the
/// documented conservative hold, never a silent compiled reprice.
#[tokio::test]
async fn a_missing_pinned_override_version_fails_closed() {
    const PEAK: i64 = 4 * 86_400 + 15 * 3_600 - 8 * 3_600;
    let _lock = crate::pricing::tariff_book::GLOBAL_BOOK_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    crate::pricing::tariff_book::clear_global_book_for_test();
    let usage = GlmUsage {
        input_tokens: 1_000,
        cache_read_tokens: 500,
        cache_write_tokens: 50,
        output_tokens: 100,
        reasoning_output_tokens: 40,
    };
    let pin = PinnedTariff {
        family: "zhipu/glm/glm-5.2".to_owned(),
        version: 9,
        schedule_id: "zhipu/glm/glm-5.2/v9".to_owned(),
    };
    let missing = price_turn_settlement(None, &usage, "glm-5.2", 1, PEAK, Some(&pin)).await;
    crate::pricing::tariff_book::clear_global_book_for_test();
    assert!(missing.is_err());
}
