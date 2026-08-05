use super::*;
use kimi_credential::{encode_envelope, CredentialKeyring, KimiCredentialKind, KIMI_STATUS_NORMAL};
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
        let root = std::env::temp_dir().join(format!("kimi-gateway-{suffix}"));
        fs::create_dir_all(root.join("credentials")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(root.join("credentials"), fs::Permissions::from_mode(0o700)).unwrap();
        Self { root }
    }

    fn publish_console_profile(&self) {
        self.publish_console_profile_with("console-secret");
    }

    fn publish_console_profile_with(&self, access_token: &str) {
        let credential = KimiCredential {
            version: 1,
            kind: KimiCredentialKind::ConsoleKey,
            access_token: access_token.into(),
            refresh_token: String::new(),
            expires_at: 0,
            scope: "coding".into(),
            subject_id: "subject-1".into(),
            plan_name: "unreviewed-base-plan".into(),
            plan_level: 1,
            status: KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
            proxy_url: String::new(),
        };
        let ring = keyring();
        let envelope = ring.seal("a1", "kimi-01", &credential).unwrap();
        let credential_path = self.root.join("credentials/kimi-01.json");
        write_private(&credential_path, &encode_envelope(&envelope).unwrap());
        let roster = json!({
            "profiles": [{
                "id": "kimi-01",
                "credential_file": credential_path.to_string_lossy(),
            }]
        });
        write_private(
            &self.root.join("profiles.json"),
            &serde_json::to_vec(&roster).unwrap(),
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

fn config(root: &Path, base_url: String) -> KimiPlaneConfig {
    KimiPlaneConfig {
        roster_dir: root.to_path_buf(),
        keyring: keyring(),
        transport: super::super::transport::KimiTransportConfig {
            base_url,
            auth_scheme: super::super::transport::AuthScheme::Bearer,
            request_timeout: Duration::from_secs(5),
            refresh_lead: Duration::from_secs(120),
        },
        readiness_probe: ProbeRoute::Identity,
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

fn calibration_event(request_id: &str) -> KimiTurnCalibrationEvent {
    KimiTurnCalibrationEvent {
        request_id: request_id.into(),
        subject_id: "subject-1".into(),
        plan: "unreviewed-base-plan".into(),
        requested_model: "kimi-for-coding".into(),
        served_model: "kimi-k2.7-code".into(),
        context_mode: "256k".into(),
        reasoning_effort: "high".into(),
        tariff_schedule_id: KIMI_TARIFF_SCHEDULE_ID.into(),
        priced_ts: 1_800_000_000,
        completed_at: 1_800_000_001,
        input_tokens: 10,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 2,
        reasoning_output_tokens: 0,
        api_input_nanousd: 600_000,
        api_cache_read_nanousd: 0,
        api_cache_write_nanousd: 0,
        api_output_nanousd: 600_000,
        api_total_nanousd: 1_200_000,
    }
}

fn quota_body(used: i64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "usage": {
            "used": used.to_string(),
            "limit": "1000",
            "resetTime": "2099-01-07T00:00:00Z"
        },
        "limits": [{
            "name": "rate",
            "window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
            "detail": {
                "used": used.to_string(),
                "limit": "100",
                "resetTime": "2099-01-01T00:00:00Z"
            }
        }]
    }))
    .unwrap()
}

#[tokio::test]
async fn a_pending_turn_blocks_the_provider_quota_read() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let (base_url, mut requests, _responses) = controlled_mock_server(1);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
    gateway
        .turn_queue
        .lock()
        .unwrap()
        .push(calibration_event("pending-before-poll"));

    assert_eq!(gateway.poll_quotas().await, 0);
    assert_eq!(gateway.operational_status().delivery.pending_events, 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), requests.recv())
            .await
            .is_err(),
        "/usages must not run past an undelivered spend head"
    );
}

#[tokio::test]
async fn customer_generation_start_invalidates_a_concurrent_quota_snapshot_without_waiting() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let (base_url, mut requests, responses) = controlled_mock_server(1);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    let profile = gateway.profiles_snapshot()[0].clone();

    let poll = {
        let gateway = gateway.clone();
        tokio::spawn(async move { gateway.poll_quotas().await })
    };
    let request = requests.recv().await.unwrap();
    assert!(request.starts_with(b"GET /usages "));

    // No semaphore or maintenance wait: a customer lease starts immediately while the GET is
    // outstanding. Its epoch makes the returned snapshot unusable for calibration.
    let lease = ProfileLease::new(profile.clone());
    assert_eq!(profile.inflight.load(Ordering::Acquire), 1);
    responses
        .send(http_response("application/json", &quota_body(10)))
        .unwrap();
    assert_eq!(poll.await.unwrap(), 0);
    drop(lease);
    assert_eq!(
        profile
            .candidate("kimi-for-coding", now_unix())
            .used_fraction_units,
        None
    );
}

#[tokio::test]
async fn transient_observation_failure_keeps_the_previous_quota_generation() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let (base_url, requests) =
        mock_server(vec![http_response("application/json", &quota_body(10))]);
    let sqlite = fixture.root.join("billing.sqlite");
    let billing = Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
    let gateway =
        KimiGateway::new_with_calibration(config(&fixture.root, base_url), Some(billing)).unwrap();
    let profile = gateway.profiles_snapshot()[0].clone();

    // SQLite deliberately refuses KIMI calibration. A successful provider GET is not enough
    // to publish steering before the durable PostgreSQL observation/CAS succeeds.
    assert_eq!(gateway.poll_quotas().await, 0);
    assert!(requests.recv().unwrap().starts_with(b"GET /usages "));
    let candidate = profile.candidate("kimi-for-coding", now_unix());
    assert_eq!(candidate.used_fraction_units, None);
    assert_eq!(candidate.quota_age_secs, None);
}

#[test]
fn a_durable_snapshot_publishes_the_tightest_window_and_exact_full_reset() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let gateway =
        KimiGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:1".into()), None)
            .unwrap();
    let profile = gateway.profiles_snapshot()[0].clone();
    let observed_at = now_unix();
    let snapshots = vec![
        KimiQuotaSnapshot {
            window_duration_secs: registry::KIMI_ROLLING_WINDOW_SECS,
            window_name: Some("rate".into()),
            resets_at: observed_at + 300,
            observed_at,
            native_used_units: 60,
            native_limit_units: 100,
            used_fraction_units: 60_000_000,
            measurement_resolution_fraction_units: 1_000_000,
        },
        KimiQuotaSnapshot {
            window_duration_secs: registry::KIMI_WEEKLY_WINDOW_SECS,
            window_name: None,
            resets_at: observed_at + 600,
            observed_at,
            native_used_units: 1_000,
            native_limit_units: 1_000,
            used_fraction_units: registry::KIMI_FRACTION_SCALE,
            measurement_resolution_fraction_units: 100_000,
        },
    ];
    profile.publish_quota(&snapshots, observed_at);
    let candidate = profile.candidate("kimi-for-coding", observed_at);
    assert_eq!(
        candidate.used_fraction_units,
        Some(registry::KIMI_FRACTION_SCALE)
    );
    assert_eq!(candidate.quota_age_secs, Some(0));
    assert_eq!(candidate.ineligible, Some(Ineligible::QuotaWall));
    assert_eq!(
        profile.health.lock().unwrap().quota_cool_until,
        observed_at + 600
    );
}

#[tokio::test]
async fn quota_auth_capacity_and_transport_failures_stay_profile_local() {
    for (status, responses, expected) in [
        ("401 Unauthorized", 2, Some(Ineligible::AuthQuarantined)),
        ("403 Forbidden", 1, Some(Ineligible::QuotaWall)),
        (
            "429 Too Many Requests",
            1,
            Some(Ineligible::TransportWedged),
        ),
        (
            "503 Service Unavailable",
            1,
            Some(Ineligible::TransportWedged),
        ),
    ] {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let response = http_status_response(status, "application/json", br#"{}"#);
        let (base_url, requests) = mock_server(vec![response; responses]);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
        let profile = gateway.profiles_snapshot()[0].clone();
        profile.mark_healthy();
        gateway.live_profiles.store(1, Ordering::Release);

        assert_eq!(gateway.poll_quotas().await, 0, "status {status}");
        assert_eq!(gateway.profiles_snapshot().len(), 1, "status {status}");
        assert_eq!(
            profile.candidate("kimi-for-coding", now_unix()).ineligible,
            expected,
            "status {status}"
        );
        for _ in 0..responses {
            assert!(requests.recv().unwrap().starts_with(b"GET /usages "));
        }
    }
}

#[tokio::test]
async fn a_profile_removed_during_quota_io_is_never_reintroduced() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let (base_url, mut requests, responses) = controlled_mock_server(1);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    let poll = {
        let gateway = gateway.clone();
        tokio::spawn(async move { gateway.poll_quotas().await })
    };
    assert!(requests.recv().await.unwrap().starts_with(b"GET /usages "));

    fixture.publish_empty_roster();
    assert!(gateway.refresh_profiles().await);
    assert!(gateway.profiles_snapshot().is_empty());
    responses
        .send(http_response("application/json", &quota_body(10)))
        .unwrap();
    assert_eq!(poll.await.unwrap(), 0);
    assert!(gateway.profiles_snapshot().is_empty());
    assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));
}

#[tokio::test]
async fn shutdown_cancels_the_steady_poll_and_bounds_its_final_quota_read() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let (base_url, mut requests, responses) = controlled_mock_server(2);
    let mut test_config = config(&fixture.root, base_url);
    test_config.transport.request_timeout = Duration::from_secs(30);
    let gateway = Arc::new(KimiGateway::new_with_calibration(test_config, None).unwrap());
    let steady = {
        let gateway = gateway.clone();
        tokio::spawn(async move { gateway.poll_quotas().await })
    };
    assert!(requests.recv().await.unwrap().starts_with(b"GET /usages "));

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
    assert!(requests.recv().await.unwrap().starts_with(b"GET /usages "));
    stopping.await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "provider I/O must not extend shutdown past the bounded deadline"
    );
}

#[tokio::test]
async fn exact_base_alias_uses_identity_readiness_and_transparent_messages_bytes() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", identity),
        http_response("application/json", generation),
    ]);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    assert_eq!(gateway.readiness(), Ok(()));

    let body = json!({
        "model": "kimi-for-coding",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "hello"}],
    });
    let response = gateway
        .handle(KimiRequest {
            headers: HeaderMap::new(),
            raw_body_len: serde_json::to_vec(&body).unwrap().len(),
            body,
            model: "kimi-for-coding".into(),
            execution: ExecutionAttempt::direct(),
            billing: None,
            affinity: None,
            affinity_store: affinity(),
            calibration: None,
        })
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let returned = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap();
    assert_eq!(returned.as_ref(), generation);

    let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(probe.starts_with("GET /me "));
    assert!(probe
        .to_ascii_lowercase()
        .contains("authorization: bearer console-secret"));
    let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(turn.starts_with("POST /messages "));
    assert!(turn.contains("\"model\":\"kimi-for-coding\""));
    // No calibration authority in this test: evidence remains visible in the bounded FIFO
    // instead of being silently discarded.
    assert_eq!(gateway.operational_status().delivery.pending_events, 1);
}

#[tokio::test]
async fn identity_auth_rejection_forces_exactly_one_refresh_retry() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, requests) = mock_server(vec![
        http_status_response("401 Unauthorized", "application/json", br#"{}"#),
        http_response("application/json", identity),
    ]);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();

    assert_eq!(gateway.preflight().await, 1);
    assert_eq!(gateway.readiness(), Ok(()));
    for _ in 0..2 {
        let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(probe.starts_with("GET /me "));
        assert!(probe
            .to_ascii_lowercase()
            .contains("authorization: bearer console-secret"));
    }
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn a_cold_degraded_gateway_adopts_a_new_profile_only_after_identity_probe() {
    let fixture = Fixture::new();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, requests) = mock_server(vec![http_response("application/json", identity)]);
    let gateway = KimiGateway::new_degraded(config(&fixture.root, base_url), None);
    assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

    fixture.publish_console_profile();
    assert!(gateway.refresh_profiles().await);
    assert_eq!(gateway.readiness(), Ok(()));
    assert_eq!(gateway.operational_status().total_profiles, 1);
    let request = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(request.starts_with("GET /me "));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer console-secret"));
}

#[tokio::test]
async fn an_unchanged_roster_reuses_the_exact_profile_without_another_probe() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, requests) = mock_server(vec![http_response("application/json", identity)]);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();

    assert!(!gateway.refresh_profiles().await);
    assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
    assert_eq!(gateway.readiness(), Ok(()));
    assert!(requests.recv().unwrap().starts_with(b"GET /me "));
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn broken_or_disappeared_rosters_retain_the_last_good_ready_profile() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, _requests) = mock_server(vec![http_response("application/json", identity)]);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
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
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, _requests) = mock_server(vec![http_response("application/json", identity)]);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
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
async fn a_changed_credential_is_published_only_after_a_successful_identity_probe() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", identity),
        http_response("application/json", identity),
    ]);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();

    fixture.publish_console_profile_with("replacement-secret");
    assert!(gateway.refresh_profiles().await);
    let replacement = gateway.profiles_snapshot()[0].clone();
    assert!(!Arc::ptr_eq(&original, &replacement));
    assert_eq!(gateway.readiness(), Ok(()));

    let first = String::from_utf8(requests.recv().unwrap()).unwrap();
    let second = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(first
        .to_ascii_lowercase()
        .contains("authorization: bearer console-secret"));
    assert!(second
        .to_ascii_lowercase()
        .contains("authorization: bearer replacement-secret"));
}

#[tokio::test]
async fn a_failed_probe_for_a_changed_credential_keeps_the_old_ready_snapshot() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let rejected = http_status_response("403 Forbidden", "application/json", br#"{}"#);
    let (base_url, requests) =
        mock_server(vec![http_response("application/json", identity), rejected]);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let original = gateway.profiles_snapshot()[0].clone();

    fixture.publish_console_profile_with("rejected-secret");
    assert!(!gateway.refresh_profiles().await);
    assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
    assert_eq!(gateway.readiness(), Ok(()));
    for expected in ["console-secret", "rejected-secret"] {
        let request = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {expected}")));
    }
}

#[tokio::test]
async fn final_verification_never_publishes_a_credential_rotated_during_probe() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, mut requests, responses) = controlled_mock_server(2);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    let original = gateway.profiles_snapshot()[0].clone();
    original.mark_healthy();
    gateway.live_profiles.store(1, Ordering::Release);

    fixture.publish_console_profile_with("candidate-secret");
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
        .contains("authorization: bearer candidate-secret"));

    // Simulate the other blue-green generation atomically rotating the shared envelope after
    // this generation loaded it but before its candidate probe completed.
    fixture.publish_console_profile_with("peer-rotated-secret");
    responses
        .send(http_response("application/json", identity))
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
        .contains("authorization: bearer peer-rotated-secret"));
    responses
        .send(http_response("application/json", identity))
        .unwrap();

    assert!(reload.await.unwrap());
    let published = gateway.profiles_snapshot()[0].clone();
    let state = published.credential.lock().await;
    assert_eq!(state.credential.access_token, "peer-rotated-secret");
    assert!(!Arc::ptr_eq(&original, &published));
    assert_eq!(gateway.readiness(), Ok(()));
}

#[tokio::test]
async fn degraded_gateway_keeps_exact_aliases_on_a_zero_capacity_kimi_path() {
    let fixture = Fixture::new();
    let gateway = Arc::new(KimiGateway::new_degraded(
        config(&fixture.root, "https://example.invalid".into()),
        None,
    ));
    assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

    let body = json!({
        "model": "kimi-for-coding",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "hello"}],
    });
    let response = gateway
        .handle(KimiRequest {
            headers: HeaderMap::new(),
            raw_body_len: serde_json::to_vec(&body).unwrap().len(),
            body,
            model: "kimi-for-coding".into(),
            execution: ExecutionAttempt::direct(),
            billing: None,
            affinity: None,
            affinity_store: affinity(),
            calibration: None,
        })
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response
            .extensions()
            .get::<crate::proxy::TerminalErrorReason>()
            .map(|reason| reason.0),
        Some("kimi_capacity_exhausted")
    );
    let body = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["type"], "rate_limit_error");
    assert!(!String::from_utf8_lossy(&body)
        .to_ascii_lowercase()
        .contains("kimi"));
}

#[tokio::test]
async fn every_synthetic_failure_uses_the_shared_anthropic_sanitizer() {
    for failure in [
        GatewayFailure::Auth,
        GatewayFailure::Transport,
        GatewayFailure::Protocol,
        GatewayFailure::Capacity,
        GatewayFailure::LowBalance,
        GatewayFailure::BadRequest("kimi_private_request_reason"),
        GatewayFailure::Unsupported("kimi_private_capability_reason"),
        GatewayFailure::Unavailable("kimi_private_runtime_reason"),
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
        for private in ["kimi", "subscription", "roster", "upstream", "provider"] {
            assert!(
                !body.contains(private),
                "{failure:?} leaked {private}: {body}"
            );
        }
    }
}

fn publish_second_console_profile(fixture: &Fixture) {
    let credential_path = fixture.root.join("credentials/kimi-02.json");
    // The second envelope must decrypt to its own subject: seal it on its own profile id.
    let ring = keyring();
    let credential = KimiCredential {
        version: 1,
        kind: KimiCredentialKind::ConsoleKey,
        access_token: "console-secret-2".into(),
        refresh_token: String::new(),
        expires_at: 0,
        scope: "coding".into(),
        subject_id: "subject-2".into(),
        plan_name: "unreviewed-base-plan".into(),
        plan_level: 1,
        status: KIMI_STATUS_NORMAL.into(),
        region: "REGION_CN".into(),
        proxy_url: String::new(),
    };
    let envelope = ring.seal("a1", "kimi-02", &credential).unwrap();
    write_private(
        &credential_path,
        &kimi_credential::encode_envelope(&envelope).unwrap(),
    );
    let roster = json!({
        "profiles": [
            {
                "id": "kimi-01",
                "credential_file": fixture.root.join("credentials/kimi-01.json").to_string_lossy(),
            },
            {
                "id": "kimi-02",
                "credential_file": credential_path.to_string_lossy(),
            },
        ]
    });
    write_private(
        &fixture.root.join("profiles.json"),
        &serde_json::to_vec(&roster).unwrap(),
    );
}

fn kimi_request_body(stream: bool) -> Value {
    json!({
        "model": "kimi-for-coding",
        "max_tokens": 8,
        "stream": stream,
        "messages": [{"role": "user", "content": "hello"}],
    })
}

fn kimi_request(body: Value) -> KimiRequest {
    KimiRequest {
        headers: HeaderMap::new(),
        raw_body_len: serde_json::to_vec(&body).unwrap().len(),
        body,
        model: "kimi-for-coding".into(),
        execution: ExecutionAttempt::direct(),
        calibration: None,
        billing: None,
        affinity: None,
        affinity_store: affinity(),
    }
}

#[tokio::test]
async fn every_upstream_call_carries_the_pinned_cli_user_agent() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, requests) = mock_server(vec![http_response("application/json", identity)]);
    let gateway = KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
    assert_eq!(gateway.preflight().await, 1);
    let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(
        probe
            .to_ascii_lowercase()
            .contains("user-agent: kimi-code-cli/0.31.1"),
        "the subscription endpoint must see the pinned official CLI identity: {probe}"
    );
}

#[tokio::test]
async fn a_stalled_stream_start_rotates_pre_byte_and_marks_the_model() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    publish_second_console_profile(&fixture);
    let identity_1 = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let identity_2 = br#"{"user_id":"subject-2","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    // First generation attempt: 2xx SSE whose stream ends without a single byte — a wedged
    // stream start. Second: a complete valid stream.
    let stalled = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec();
    let stream_body = br#"data: {"type":"message_start","message":{"model":"kimi-k2.7-code","usage":{"input_tokens":10}}}

data: {"type":"message_delta","usage":{"output_tokens":4}}

data: {"type":"message_stop"}

"#.to_vec();
    let good = http_response("text/event-stream", &stream_body);
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", identity_1),
        http_response("application/json", identity_2),
        stalled,
        good,
    ]);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 2);

    let response = gateway.handle(kimi_request(kimi_request_body(true))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let served = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&served).contains("message_stop"));

    let posts: Vec<String> = (0..4)
        .map(|_| String::from_utf8(requests.recv().unwrap()).unwrap())
        .filter(|request| request.starts_with("POST /messages "))
        .collect();
    assert_eq!(posts.len(), 2, "the wedged profile must be rotated past");
    // A 2xx that never became a byte is the model path, not the egress: exactly one profile
    // carries the first model-failure streak and NO profile-level cooling.
    let now = now_unix();
    let streaked = gateway
        .profiles_snapshot()
        .iter()
        .filter(|profile| {
            let health = profile.health.lock().unwrap();
            health
                .model_failures
                .get("kimi-for-coding")
                .is_some_and(|failure| failure.streak == 1 && failure.cool_until == 0)
                && health.transport_cool_until <= now
                && health.quota_cool_until <= now
                && health.auth_quarantined_until <= now
        })
        .count();
    assert_eq!(streaked, 1);
}

#[tokio::test]
async fn a_failed_non_stream_body_read_rotates_pre_byte() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    publish_second_console_profile(&fixture);
    let identity_1 = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let identity_2 = br#"{"user_id":"subject-2","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    // Declares more bytes than it delivers, then closes: the body read fails before the
    // customer sees anything, so rotation is still legal.
    let mut truncated = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 4096\r\nconnection: close\r\n\r\n".to_vec();
    truncated.extend_from_slice(br#"{"partial":"#);
    let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", identity_1),
        http_response("application/json", identity_2),
        truncated,
        http_response("application/json", generation),
    ]);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 2);

    let response = gateway.handle(kimi_request(kimi_request_body(false))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let served = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
        .await
        .unwrap();
    assert_eq!(served.as_ref(), generation);

    let mut posts = 0;
    for _ in 0..4 {
        let request = String::from_utf8(requests.recv().unwrap()).unwrap();
        if request.starts_with("POST /messages ") {
            posts += 1;
        }
    }
    assert_eq!(posts, 2);
    // Same model-axis semantics as the stalled stream start: a streak on one profile, no
    // profile-level cooling.
    let now = now_unix();
    let streaked = gateway
        .profiles_snapshot()
        .iter()
        .filter(|profile| {
            let health = profile.health.lock().unwrap();
            health
                .model_failures
                .get("kimi-for-coding")
                .is_some_and(|failure| failure.streak == 1 && failure.cool_until == 0)
                && health.transport_cool_until <= now
                && health.quota_cool_until <= now
                && health.auth_quarantined_until <= now
        })
        .count();
    assert_eq!(streaked, 1);
}

#[tokio::test]
async fn a_second_failure_of_the_same_model_cools_only_that_model() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let stalled = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec();
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", identity),
        stalled.clone(),
        stalled,
    ]);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    assert!(requests.recv().unwrap().starts_with(b"GET /me "));

    // Two consecutive failures of the same model on the only profile.
    for _ in 0..2 {
        let response = gateway.handle(kimi_request(kimi_request_body(true))).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
    // The model axis cooled; every profile-level axis stayed clean.
    let now = now_unix();
    let profile = gateway.profiles_snapshot()[0].clone();
    assert!(matches!(
        profile.candidate("kimi-for-coding", now).ineligible,
        Some(Ineligible::ModelCooling)
    ));
    {
        let health = profile.health.lock().unwrap();
        assert!(health.transport_cool_until <= now);
        assert!(health.quota_cool_until <= now);
        assert!(health.auth_quarantined_until <= now);
    }
    // The next request for the cooled model never reaches the upstream; the profile's other
    // models would still be eligible had the plan granted them.
    let response = gateway.handle(kimi_request(kimi_request_body(true))).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let mut posts = 0;
    while let Ok(request) = requests.try_recv() {
        if request.starts_with(b"POST /messages ") {
            posts += 1;
        }
    }
    assert_eq!(posts, 2, "the cooled model must not be attempted again");
}

#[test]
fn retry_after_hint_is_bounded_and_optional() {
    let mut headers = wreq::header::HeaderMap::new();
    assert_eq!(retry_after_seconds(&headers), None);
    headers.insert("retry-after", "17".parse().unwrap());
    assert_eq!(retry_after_seconds(&headers), Some(17));
    for bad in ["0", "-3", "7201", "not-a-number", "1.5"] {
        headers.insert("retry-after", bad.parse().unwrap());
        assert_eq!(retry_after_seconds(&headers), None, "{bad}");
    }
}

#[tokio::test]
async fn a_quota_wall_honors_retry_after_exactly_and_rotates() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    publish_second_console_profile(&fixture);
    let identity_1 = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let identity_2 = br#"{"user_id":"subject-2","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let wall = b"HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\nretry-after: 42\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}".to_vec();
    let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", identity_1),
        http_response("application/json", identity_2),
        wall,
        http_response("application/json", generation),
    ]);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 2);

    let before = now_unix();
    let response = gateway.handle(kimi_request(kimi_request_body(false))).await;
    assert_eq!(response.status(), StatusCode::OK);
    for _ in 0..4 {
        let _ = requests.recv().unwrap();
    }
    // The walled profile cools on the quota axis until exactly now + 42 — not the flat
    // fallback — and the request still completes on the sibling profile.
    let now = now_unix();
    let walls: Vec<i64> = gateway
        .profiles_snapshot()
        .iter()
        .filter_map(
            |profile| match profile.candidate("kimi-for-coding", now).ineligible {
                Some(Ineligible::QuotaWall) => {
                    Some(profile.health.lock().unwrap().quota_cool_until)
                }
                _ => None,
            },
        )
        .collect();
    assert_eq!(walls.len(), 1);
    let expected = before + 42;
    assert!(
        walls[0] >= expected && walls[0] <= expected + 2,
        "quota wall must honor the provider hint exactly: {} vs {expected}",
        walls[0]
    );
}

#[test]
fn calibration_headers_parse_only_for_a_valid_admin_pair() {
    let profile = "kimi-01";
    let uuid = "123e4567-e89b-42d3-a456-426614174000";
    let mut headers = HeaderMap::new();
    assert!(parse_kimi_calibration_headers(&headers, true)
        .unwrap()
        .is_none());

    headers.insert("x-apitoken-calibration-profile", profile.parse().unwrap());
    headers.insert("x-apitoken-calibration-request-id", uuid.parse().unwrap());
    let target = parse_kimi_calibration_headers(&headers, true)
        .unwrap()
        .expect("a valid admin pair parses");
    assert_eq!(target.profile_id, profile);
    assert_eq!(target.request_id, uuid);

    // A non-admin caller carrying the pair is refused outright.
    assert!(parse_kimi_calibration_headers(&headers, false).is_err());

    // Half a pair is never meaningful, admin or not.
    let mut half = HeaderMap::new();
    half.insert("x-apitoken-calibration-profile", profile.parse().unwrap());
    assert!(parse_kimi_calibration_headers(&half, true).is_err());
    assert!(parse_kimi_calibration_headers(&half, false).is_err());

    // A profile id with a path escape and a non-v4 request id both fail closed.
    let mut bad_profile = headers.clone();
    bad_profile.insert(
        "x-apitoken-calibration-profile",
        "../escape".parse().unwrap(),
    );
    assert!(parse_kimi_calibration_headers(&bad_profile, true).is_err());
    let mut not_v4 = headers.clone();
    not_v4.insert(
        "x-apitoken-calibration-request-id",
        "123e4567-e89b-12d3-a456-426614174000".parse().unwrap(),
    );
    assert!(parse_kimi_calibration_headers(&not_v4, true).is_err());
}

#[tokio::test]
async fn a_calibration_turn_uses_the_preselected_request_id_and_never_rebinds() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = mock_server(vec![
        http_response("application/json", identity),
        http_response("application/json", generation),
    ]);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);

    let uuid = "123e4567-e89b-42d3-a456-426614174000";
    let body = json!({
        "model": "kimi-for-coding",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "hello"}],
    });
    let response = gateway
        .handle(KimiRequest {
            headers: HeaderMap::new(),
            raw_body_len: serde_json::to_vec(&body).unwrap().len(),
            body,
            model: "kimi-for-coding".into(),
            execution: ExecutionAttempt::direct(),
            billing: None,
            affinity: None,
            affinity_store: affinity(),
            calibration: Some(KimiCalibrationTarget {
                profile_id: "kimi-01".into(),
                request_id: uuid.into(),
            }),
        })
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(probe.starts_with("GET /me "));
    let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
    assert!(turn.starts_with("POST /messages "));
    assert!(
        turn.to_ascii_lowercase()
            .contains(&format!("x-client-request-id: {uuid}")),
        "the upstream attempt must carry the preselected immutable id: {turn}"
    );
    // The queued durable turn carries exactly the preselected id — that is what the runner
    // diffs against when it attributes the spend.
    let event = gateway
        .turn_queue
        .lock()
        .unwrap()
        .head()
        .expect("the turn is queued for durable delivery")
        .clone();
    assert_eq!(event.request_id, uuid);
}

#[tokio::test]
async fn a_pinned_calibration_turn_fails_closed_instead_of_rebinding() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let (base_url, requests) = mock_server(vec![http_response("application/json", identity)]);
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    let _probe = requests.recv().unwrap();

    // Cool the only profile: a pinned turn has nowhere to go and must not hunt for one.
    let profile = gateway.profiles_snapshot()[0].clone();
    profile.apply_effect(ProfileEffect::TransportFault, now_unix());
    for pinned_id in ["kimi-01", "kimi-99"] {
        let body = json!({
            "model": "kimi-for-coding",
            "max_tokens": 8,
            "messages": [{"role": "user", "content": "hello"}],
        });
        let response = gateway
            .handle(KimiRequest {
                headers: HeaderMap::new(),
                raw_body_len: serde_json::to_vec(&body).unwrap().len(),
                body,
                model: "kimi-for-coding".into(),
                execution: ExecutionAttempt::direct(),
                billing: None,
                affinity: None,
                affinity_store: affinity(),
                calibration: Some(KimiCalibrationTarget {
                    profile_id: pinned_id.into(),
                    request_id: "123e4567-e89b-42d3-a456-426614174000".into(),
                }),
            })
            .await;
        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "{pinned_id}: a cooled or unknown pinned target is capacity, not a rebind"
        );
    }
    assert!(
        requests.try_recv().is_err(),
        "no upstream attempt may happen for a pinned target that cannot serve"
    );
}

/// Barriered mock: identity probes are answered immediately so preflight completes; every
/// burst attempt is then accepted and fully read BEFORE any of them is answered. If admission
/// had a semaphore or queue, the read loop would deadlock — the test would time out.
fn burst_mock_server(
    identities: Vec<Vec<u8>>,
    generation: Vec<u8>,
    burst: usize,
) -> (String, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for identity in identities {
            let (mut stream, _) = listener.accept().unwrap();
            sender.send(read_request(&mut stream)).unwrap();
            stream.write_all(&identity).unwrap();
        }
        let mut pending = Vec::new();
        for _ in 0..burst {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            sender.send(request).unwrap();
            pending.push(stream);
        }
        for mut stream in pending {
            stream.write_all(&generation).unwrap();
        }
    });
    (format!("http://{address}"), receiver)
}

/// Bounded receive for test assertions that yields to the executor: on the single-threaded
/// test runtime a blocking std-channel wait would starve the very tasks being awaited.
async fn recv_bounded(receiver: &mpsc::Receiver<Vec<u8>>, what: &str) -> Vec<u8> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(request) => return request,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for {what}"
                );
                tokio::task::yield_now().await;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("request channel closed while waiting for {what}")
            }
        }
    }
}

#[tokio::test]
async fn burst_admission_starts_every_attempt_before_any_response() {
    const BURST: usize = 8;
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = burst_mock_server(
        vec![http_response("application/json", identity)],
        http_response("application/json", generation),
        BURST,
    );
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 1);
    assert!(recv_bounded(&requests, "the /me probe")
        .await
        .starts_with(b"GET /me "));
    let profile = gateway.profiles_snapshot()[0].clone();

    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..BURST {
        let gateway = gateway.clone();
        tasks.spawn(async move { gateway.handle(kimi_request(kimi_request_body(false))).await });
    }
    // All eight upstream attempts start before any answer exists: inflight is a placement
    // signal, never a ceiling.
    for _ in 0..BURST {
        let request = recv_bounded(&requests, "a burst attempt").await;
        assert!(request.starts_with(b"POST /messages "));
    }
    assert_eq!(profile.inflight.load(Ordering::Acquire), BURST as u32);
    while let Some(outcome) = tasks.join_next().await {
        assert_eq!(outcome.unwrap().status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn concurrent_first_turns_of_one_conversation_cannot_double_home() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    publish_second_console_profile(&fixture);
    let identity_1 = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let identity_2 = br#"{"user_id":"subject-2","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = burst_mock_server(
        vec![
            http_response("application/json", identity_1),
            http_response("application/json", identity_2),
        ],
        http_response("application/json", generation),
        2,
    );
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 2);
    assert!(recv_bounded(&requests, "the first /me probe")
        .await
        .starts_with(b"GET /me "));
    assert!(recv_bounded(&requests, "the second /me probe")
        .await
        .starts_with(b"GET /me "));

    let store = affinity();
    let mut headers = HeaderMap::new();
    headers.insert("x-session-id", "session-0042".parse().unwrap());
    let body = kimi_request_body(false);
    let input = store
        .infer("account-1", &headers, &body)
        .expect("session alias");

    // Turn A claims the conversation's home before its attempt; turn B starts only after
    // A's attempt is on the wire, so it must resolve to A's home rather than the cursor's
    // next profile.
    let first = {
        let gateway = gateway.clone();
        let store = store.clone();
        let input = input.clone();
        let body = body.clone();
        tokio::spawn(async move {
            let mut request = kimi_request(body);
            request.affinity = Some(input);
            request.affinity_store = store;
            gateway.handle(request).await
        })
    };
    let post_a = String::from_utf8(recv_bounded(&requests, "turn A's attempt").await).unwrap();
    assert!(post_a.starts_with("POST /messages "));
    let second = {
        let gateway = gateway.clone();
        tokio::spawn(async move {
            let mut request = kimi_request(body);
            request.affinity = Some(input);
            request.affinity_store = store;
            gateway.handle(request).await
        })
    };
    let post_b = String::from_utf8(recv_bounded(&requests, "turn B's attempt").await).unwrap();
    assert!(post_b.starts_with("POST /messages "));
    assert_eq!(first.await.unwrap().status(), StatusCode::OK);
    assert_eq!(second.await.unwrap().status(), StatusCode::OK);

    let a_second = post_a.contains("console-secret-2");
    let b_second = post_b.contains("console-secret-2");
    assert_eq!(
        a_second, b_second,
        "concurrent first turns of one conversation must land on one home"
    );
}

#[tokio::test]
async fn a_new_conversation_prefers_the_warm_home_for_its_first_attempt() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    publish_second_console_profile(&fixture);
    let identity_1 = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let identity_2 = br#"{"user_id":"subject-2","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
    let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
    let (base_url, requests) = burst_mock_server(
        vec![
            http_response("application/json", identity_1),
            http_response("application/json", identity_2),
        ],
        http_response("application/json", generation),
        1,
    );
    let gateway =
        Arc::new(KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap());
    assert_eq!(gateway.preflight().await, 2);
    assert!(recv_bounded(&requests, "the first /me probe")
        .await
        .starts_with(b"GET /me "));
    assert!(recv_bounded(&requests, "the second /me probe")
        .await
        .starts_with(b"GET /me "));

    let store = affinity();
    // A cacheable system prompt creates the cache root; mark kimi-02's home warm for it.
    let body = json!({
        "model": "kimi-for-coding",
        "max_tokens": 8,
        "system": "x".repeat(8192),
        "messages": [{"role": "user", "content": "hello"}],
    });
    let input = store
        .infer("account-1", &HeaderMap::new(), &body)
        .expect("cache root");
    let warm_home = store.home_id("kimi-02");
    store.mark_cache_warm(&input, &warm_home);

    let mut request = kimi_request(body);
    request.affinity = Some(input);
    request.affinity_store = store.clone();
    let response = gateway.handle(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let post = String::from_utf8(recv_bounded(&requests, "the generation attempt").await).unwrap();
    assert!(post.starts_with("POST /messages "));
    assert!(
        post.to_ascii_lowercase()
            .contains("authorization: bearer console-secret-2"),
        "the first attempt must prefer the warm home: {post}"
    );
}

#[test]
fn sse_accounting_survives_split_frames_and_requires_a_terminal_event() {
    let mut accounting = SseAccounting::default();
    accounting
        .push(br#"data: {"type":"message_start","message":{"model":"kimi-k2.7-code","usage":{"input_tokens":10}}}"#)
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
    assert_eq!(accounting.served_model.as_deref(), Some("kimi-k2.7-code"));
}

#[test]
fn reservation_cap_is_an_integer_upper_bound_and_never_crosses_the_overdraft_floor() {
    let prices = kimi_prices_for_served_model("kimi-k2.7-code", 1).unwrap();
    let balance = 1_000_000i128;
    let (tokens, hold) = cap_to_balance(balance, 100, prices, 10_000, 1_000_000).unwrap();
    assert!(tokens > 0);
    assert!(i128::from(hold) <= balance + metering::OVERDRAFT_NANO);
    let raw = 100 * prices.input + i128::from(tokens) * prices.output;
    assert_eq!(i128::from(hold), metering::apply_multiplier(raw, 10_000));
    assert!(cap_to_balance(-metering::OVERDRAFT_NANO, 1, prices, 10_000, 1).is_none());
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
    assert!(matches!(
        validate_priced_surface(&json!({"tools": [{"name": "search"}]})),
        Err(GatewayFailure::Unsupported("kimi_tools_unpriced"))
    ));
    for body in [
        json!({"tools": "provider-default"}),
        json!({"tool_choice": {"type": "auto"}}),
        json!({"messages": [{"content": [{"type": "tool_result"}]}]}),
        json!({"messages": [{"content": [{"type": "web_search_tool_result"}]}]}),
    ] {
        assert!(matches!(
            validate_priced_surface(&body),
            Err(GatewayFailure::Unsupported("kimi_tools_unpriced"))
        ));
    }
    assert!(matches!(
        validate_priced_surface(&json!({
            "messages": [{"content": [{"type": "image", "source": {}}]}]
        })),
        Err(GatewayFailure::Unsupported("kimi_media_unpriced"))
    ));
    assert_eq!(reasoning_effort(&json!({})).unwrap(), "high");
    assert_eq!(
        reasoning_effort(&json!({"reasoning_effort": "xhigh"})).unwrap(),
        "max"
    );
    assert!(reasoning_effort(&json!({"reasoning_effort": "invented"})).is_err());
}

fn quota_snapshot(used: i64, limit: i64, resets_at: i64, observed_at: i64) -> KimiQuotaSnapshot {
    let derived = registry::kimi_fraction_from_native(used, limit).unwrap();
    KimiQuotaSnapshot {
        window_duration_secs: 18_000,
        window_name: None,
        resets_at,
        observed_at,
        native_used_units: used,
        native_limit_units: limit,
        used_fraction_units: derived.used_fraction_units,
        measurement_resolution_fraction_units: derived.measurement_resolution_fraction_units,
    }
}

#[tokio::test]
async fn the_status_projection_reports_cooling_axes_availability_and_inflight() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let gateway =
        KimiGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9".into()), None)
            .unwrap();
    let now = now_unix();
    let profile = gateway.profiles.read().unwrap()[0].clone();

    let healthy = gateway.operational_status();
    assert_eq!(healthy.total_profiles, 1);
    assert_eq!(healthy.available_profiles, 1);
    assert_eq!(healthy.auth_quarantined_profiles, 0);
    assert_eq!(healthy.transport_cooling_profiles, 0);
    assert_eq!(healthy.quota_cooling_profiles, 0);
    assert_eq!(healthy.inflight_requests, 0);
    assert_eq!(healthy.profiles[0].id, "kimi-01");
    assert_eq!(healthy.profiles[0].auth_quarantined_until, None);
    assert_eq!(healthy.profiles[0].transport_cool_until, None);
    assert_eq!(healthy.profiles[0].quota_cool_until, None);
    assert_eq!(healthy.profiles[0].quota_observed_at, None);
    assert_eq!(healthy.profiles[0].quota_windows, Vec::new());
    assert!(!healthy.profiles[0].live);

    profile.inflight.store(3, Ordering::Release);
    profile.apply_effect(ProfileEffect::AuthQuarantine, now);
    let quarantined = gateway.operational_status();
    assert_eq!(quarantined.available_profiles, 0);
    assert_eq!(quarantined.auth_quarantined_profiles, 1);
    assert_eq!(quarantined.inflight_requests, 3);
    assert_eq!(quarantined.profiles[0].inflight, 3);
    assert_eq!(
        quarantined.profiles[0].auth_quarantined_until,
        Some(now + AUTH_QUARANTINE_SECS)
    );
    assert!(!quarantined.profiles[0].live);

    profile.apply_effect(ProfileEffect::TransportFault, now);
    let wedged = gateway.operational_status();
    assert_eq!(wedged.transport_cooling_profiles, 1);
    assert_eq!(
        wedged.profiles[0].transport_cool_until,
        Some(now + TRANSPORT_COOL_SECS)
    );

    // An expired or cleared deadline is "not cooling", never a timestamp in the past.
    profile.mark_healthy();
    let recovered = gateway.operational_status();
    assert_eq!(recovered.available_profiles, 1);
    assert_eq!(recovered.auth_quarantined_profiles, 0);
    assert_eq!(recovered.transport_cooling_profiles, 0);
    assert_eq!(recovered.profiles[0].auth_quarantined_until, None);
    assert_eq!(recovered.profiles[0].transport_cool_until, None);
    assert!(recovered.profiles[0].live);
}

#[tokio::test]
async fn publish_quota_retains_the_exact_per_window_snapshot() {
    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let gateway =
        KimiGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9".into()), None)
            .unwrap();
    let profile = gateway.profiles.read().unwrap()[0].clone();
    let observed_at = now_unix();
    let five_hour = quota_snapshot(250, 1000, 4_102_444_800, observed_at);
    let mut weekly = quota_snapshot(100, 100, 4_102_500_000, observed_at);
    weekly.window_duration_secs = 604_800;
    profile.publish_quota(&[five_hour, weekly], observed_at);

    let status = gateway.operational_status();
    let projected = &status.profiles[0];
    assert_eq!(projected.quota_observed_at, Some(observed_at));
    assert_eq!(projected.quota_windows.len(), 2);
    let window = &projected.quota_windows[0];
    assert_eq!(window.duration_secs, 18_000);
    assert_eq!(window.used_units, 250);
    assert_eq!(window.limit_units, 1000);
    // Exact fraction semantics: 250/1000 is 25% in 10^-8 units, and the real measurement
    // resolution of a limit-1000 counter is one 0.1% step, not one fixed-point unit.
    assert_eq!(window.used_fraction_units, 25_000_000);
    assert_eq!(window.measurement_resolution_fraction_units, 100_000);
    assert_eq!(window.resets_at, 4_102_444_800);
    assert_eq!(window.observed_at, observed_at);
    assert_eq!(projected.quota_windows[1].duration_secs, 604_800);
    // The full weekly window walls the profile until the exact provider reset instant.
    assert_eq!(status.quota_cooling_profiles, 1);
    assert_eq!(projected.quota_cool_until, Some(4_102_500_000));
    assert_eq!(status.available_profiles, 0);
    // A successful poll authenticates the profile on this runtime generation.
    assert!(projected.live);
}

#[tokio::test]
async fn the_projection_bounds_plan_labels_and_cannot_carry_the_subject() {
    // The reviewed list is empty today, so every provider-controlled string collapses to the
    // bounded placeholder; a raw plan name must never reach logs, metrics or admin output.
    assert_eq!(bounded_plan_label("unreviewed-base-plan"), "unreviewed");
    assert_eq!(bounded_plan_label("Moderato"), "unreviewed");
    for entry in kimi_credential::KIMI_REVIEWED_PLANS {
        assert_eq!(bounded_plan_label(entry.plan_name), entry.plan_name);
    }

    let fixture = Fixture::new();
    fixture.publish_console_profile();
    let gateway =
        KimiGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9".into()), None)
            .unwrap();
    let status = gateway.operational_status();
    assert_eq!(status.profiles[0].plan, "unreviewed");
    // The durable-calibration join resolves only through the opaque roster id; an unknown
    // subject resolves to nothing and its rows are dropped rather than serialized.
    assert_eq!(
        gateway.profile_id_for_subject("subject-1").as_deref(),
        Some("kimi-01")
    );
    assert_eq!(gateway.profile_id_for_subject("subject-unknown"), None);
    let rendered = format!("{status:?}");
    assert!(!rendered.contains("subject-1"));
    assert!(!rendered.contains("unreviewed-base-plan"));
}
