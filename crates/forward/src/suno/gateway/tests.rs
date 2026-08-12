use super::super::config::SunoPlaneConfig;
use super::super::roster::SunoProfile;
use super::super::transport::{SunoHosts, SunoTransportConfig};
use super::*;
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc;
use suno_credential::{
    encode_envelope, CredentialKeyring, SunoCredential, SunoCredentialKind, SunoPlan,
};

// ── fixtures ────────────────────────────────────────────────────────────────

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let root = std::env::temp_dir().join(format!("suno-gateway-{suffix}"));
        fs::create_dir_all(root.join("credentials")).unwrap();
        fs::create_dir_all(root.join("artifacts")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self { root }
    }

    /// Seal one profile directly (the roster loader path is covered by roster.rs tests; the
    /// gateway tests construct runtime profiles in memory to pin the loopback hosts).
    fn profile(&self, id: &str, session_id: &str) -> SunoProfile {
        let credential = SunoCredential {
            version: 1,
            kind: SunoCredentialKind::SessionCookie,
            cookie: format!("__client=clerk-{session_id}; ajs_id=x"),
            session_id: Some(session_id.into()),
            plan: SunoPlan::Pro,
            proxy_url: String::new(),
        };
        // The session manager re-seals rotations into the roster's credential file, which must
        // exist for the atomic replace to succeed.
        let sealed = keyring().seal("a1", id, &credential).unwrap();
        let path = self.root.join("credentials").join(format!("{id}.json"));
        fs::write(&path, encode_envelope(&sealed).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        SunoProfile {
            id: id.into(),
            subject_id: super::super::roster::subject_id_of(session_id),
            plan: SunoPlan::Pro,
            credential_key_id: "a1".into(),
            credential,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn keyring() -> CredentialKeyring {
    CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
}

fn config(fixture: &Fixture) -> SunoPlaneConfig {
    SunoPlaneConfig {
        roster_dir: fixture.root.clone(),
        keyring: keyring(),
        transport: SunoTransportConfig {
            request_timeout: Duration::from_secs(5),
        },
        readiness_probe: super::super::transport::ProbeRoute::BillingInfo,
        quota_poll_interval: Duration::from_secs(300),
        artifact_dir: fixture.root.join("artifacts"),
    }
}

/// A gateway over in-memory profiles pinned at the loopback mock. Production builds profiles
/// exclusively with `SunoHosts::official()`; this seam is crate-internal test wiring.
fn gateway_with(
    fixture: &Fixture,
    ids: &[&str],
    origin: &str,
    billing: Option<Arc<AsyncBilling>>,
) -> Arc<SunoGateway> {
    let config = config(fixture);
    let hosts = SunoHosts::loopback(origin.to_string(), origin.to_string());
    let profiles = ids
        .iter()
        .map(|id| {
            RuntimeProfile::from_roster_on(
                fixture.profile(id, &format!("sess_{id}")),
                &config,
                hosts.clone(),
            )
            .unwrap()
        })
        .collect();
    Arc::new(SunoGateway::from_profiles(config, billing, profiles))
}

// ── path-routed mock upstream (Clerk auth host + business host in one) ──────

type RouteTable = Arc<Mutex<HashMap<String, VecDeque<(u16, String)>>>>;

fn mock_upstream(routes: &[(&str, &[(u16, String)])]) -> (String, RouteTable, mpsc::Receiver<String>) {
    let table: HashMap<String, VecDeque<(u16, String)>> = routes
        .iter()
        .map(|(path, responses)| {
            (
                (*path).to_string(),
                responses
                    .iter()
                    .map(|(status, body)| (*status, body.clone()))
                    .collect(),
            )
        })
        .collect();
    let routes = Arc::new(Mutex::new(table));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://{address}");
    let (sender, receiver) = mpsc::channel::<String>();
    let shared = routes.clone();
    std::thread::spawn(move || loop {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let shared = shared.clone();
        let sender = sender.clone();
        let origin = origin.clone();
        std::thread::spawn(move || {
            let request = read_request(&mut stream);
            let line = request.lines().next().unwrap_or("").to_string();
            sender.send(request).unwrap();
            let (method, path) = line
                .split_once(' ')
                .map(|(m, rest)| (m, rest.split(' ').next().unwrap_or("")))
                .unwrap_or(("", ""));
            let route = format!("{method} {path}");
            let response = {
                let mut table = shared.lock().unwrap();
                if let Some(queue) = table.get_mut(&route) {
                    queue.pop_front()
                } else {
                    // Path-prefix fallback for mints (`POST /v1/client/sessions/<sid>/tokens?…`),
                    // polls (`GET /api/feed/v2?ids=…`) and CDN artifact fetches.
                    let prefix = table
                        .keys()
                        .filter(|route| route.ends_with('*'))
                        .find(|route| {
                            route[..route.len() - 1]
                                .split_once(' ')
                                .is_some_and(|(m, p)| method == m && path.starts_with(p))
                        })
                        .cloned();
                    prefix.and_then(|route| table.get_mut(&route).and_then(VecDeque::pop_front))
                }
            };
            let (status, body) =
                response.unwrap_or((404, r#"{"error":"unrouted"}"#.to_string()));
            let body = body.replace("__BASE__", &origin).into_bytes();
            let status_line = match status {
                200 => "200 OK",
                400 => "400 Bad Request",
                401 => "401 Unauthorized",
                403 => "403 Forbidden",
                404 => "404 Not Found",
                429 => "429 Too Many Requests",
                500 => "500 Internal Server Error",
                503 => "503 Service Unavailable",
                _ => "502 Bad Gateway",
            };
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes()
            .into_iter()
            .chain(body)
            .collect::<Vec<u8>>();
            stream.write_all(&response).unwrap();
        });
    });
    (format!("http://{address}"), routes, receiver)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        buffer.push(byte[0]);
        if buffer.ends_with(b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer).to_string();
            let content_length = head
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0u8; content_length];
            stream.read_exact(&mut body).unwrap_or_default();
            buffer.extend_from_slice(&body);
            break;
        }
        if buffer.len() > 128 * 1024 * 1024 {
            break;
        }
    }
    String::from_utf8_lossy(&buffer).to_string()
}

fn mint() -> (u16, String) {
    (200, r#"{"jwt":"mock-jwt"}"#.to_string())
}

fn no_captcha() -> (u16, String) {
    (200, r#"{"required":false}"#.to_string())
}

fn quota(left: i64, usage: i64) -> (u16, String) {
    (
        200,
        format!(
            r#"{{"total_credits_left":{left},"period":"2026-08","monthly_limit":2500,"monthly_usage":{usage}}}"#
        ),
    )
}

fn generation_body(operation: &str, extra: Value) -> Value {
    let mut body = json!({"operation": operation});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    body
}

async fn billing_with_funded_account(
    fixture: &Fixture,
    balance_nano: i64,
) -> (Arc<AsyncBilling>, SunoBillingInput) {
    let path = fixture.root.join("billing.sqlite");
    let billing = Arc::new(AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap());
    billing.create_account("acct-1", None, 10_000).await.unwrap();
    billing
        .issue_key("sk-test", "acct-1", None, None, None)
        .await
        .unwrap();
    billing.topup("acct-1", balance_nano, None).await.unwrap();
    let input = SunoBillingInput {
        account_id: "acct-1".into(),
        key: "sk-test".into(),
        mult_bp: 10_000,
        available_nano: balance_nano,
    };
    (billing, input)
}

async fn wait_for_final(gateway: &SunoGateway, generation_id: &str) -> SunoGenerationView {
    for _ in 0..800 {
        if let Some(view) = gateway.generation_view(generation_id, None) {
            if ["complete", "error", "expired"].contains(&view.status.as_str()) {
                return view;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("generation {generation_id} never finalized");
}

async fn created_generation_id(response: Response) -> String {
    let rt_body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    serde_json::from_slice::<Value>(&rt_body).unwrap()["generation_id"]
        .as_str()
        .unwrap()
        .to_string()
}

// ── admission validation matrix ─────────────────────────────────────────────

#[test]
fn admission_matrix_covers_every_operation_and_every_fail_closed_rule() {
    // Unknown operations (incl. the tariff-priced but wire-less MIDI) name the admitted set.
    for op in ["midi", "covers", "remaster", "video", ""] {
        let error = admit_generation(
            serde_json::from_value(generation_body(op, json!({}))).unwrap(),
        )
        .unwrap_err();
        assert!(
            matches!(error, GatewayFailure::BadRequest("suno_operation_unknown")),
            "{op}"
        );
    }

    // Song: published 5-credit reserve, model from the reviewed paid list only.
    let admitted = admit_generation(
        serde_json::from_value(generation_body(
            "song",
            json!({"model": "v5.5", "prompt": "a song about the sea"}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.kind, OperationKind::Song);
    assert_eq!(admitted.reserve_credits, 5);
    assert_eq!(
        admitted.upstream_body["gpt_description_prompt"],
        json!("a song about the sea")
    );
    assert_eq!(admitted.upstream_body["mv"], json!("v5.5"));
    assert_eq!(admitted.upstream_body["make_instrumental"], json!(false));

    // Custom mode: tags/title switch the wire shape; negative_tags ride along.
    let admitted = admit_generation(
        serde_json::from_value(generation_body(
            "song",
            json!({"model": "v4.5", "prompt": "verse one", "tags": "lofi", "title": "t",
                   "negative_tags": "drums", "make_instrumental": true}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.upstream_body["tags"], json!("lofi"));
    assert_eq!(admitted.upstream_body["negative_tags"], json!("drums"));
    assert!(admitted.upstream_body.get("gpt_description_prompt").is_none());

    // Unknown / free-tier / deprecated / malformed model ids fail closed.
    for model in ["v4.5-all", "v3.5", "v6", "V5", ""] {
        let error = admit_generation(
            serde_json::from_value(generation_body(
                "song",
                json!({"model": model, "prompt": "x"}),
            ))
            .unwrap(),
        )
        .unwrap_err();
        assert!(
            matches!(error, GatewayFailure::BadRequest("suno_model_unknown")),
            "{model}"
        );
    }
    // A song without a model or without a description prompt fails closed.
    for body in [
        json!({"prompt": "x"}),
        json!({"model": "v5"}),
    ] {
        let error = admit_generation(
            serde_json::from_value(generation_body("song", body)).unwrap(),
        )
        .unwrap_err();
        assert!(matches!(error, GatewayFailure::BadRequest(_)));
    }

    // Extend: the documented conservative reserve (50 credits — highest published per-op
    // price), concat wire carries only the clip reference.
    let admitted = admit_generation(
        serde_json::from_value(generation_body(
            "extend",
            json!({"continue_clip_id": "clip-9", "continue_at": 62.5}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.kind, OperationKind::Extend);
    assert_eq!(admitted.reserve_credits, 50);
    assert_eq!(admitted.upstream_body["continue_clip_id"], json!("clip-9"));
    assert_eq!(admitted.upstream_body["continue_at"], json!(62.5));
    let error = admit_generation(
        serde_json::from_value(generation_body("extend", json!({"continue_at": 10.0}))).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        GatewayFailure::BadRequest("suno_continue_clip_required")
    ));
    // A negative/NaN position is not a playback position.
    let error = admit_generation(
        serde_json::from_value(generation_body(
            "extend",
            json!({"continue_clip_id": "c", "continue_at": -1.0}),
        ))
        .unwrap(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        GatewayFailure::BadRequest("suno_continue_at_invalid")
    ));
    // Non-applicable controls must not ride the concat wire.
    let error = admit_generation(
        serde_json::from_value(generation_body(
            "extend",
            json!({"continue_clip_id": "c", "tags": "x"}),
        ))
        .unwrap(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        GatewayFailure::BadRequest("suno_option_not_applicable")
    ));

    // Lyrics: conservative reserve, prompt required.
    let admitted = admit_generation(
        serde_json::from_value(generation_body("lyrics", json!({"prompt": "ode to a router"})))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.kind, OperationKind::Lyrics);
    assert_eq!(admitted.reserve_credits, 50);
    assert_eq!(admitted.upstream_body["prompt"], json!("ode to a router"));

    // Stems: conservative reserve (no documented split-kind selector), song id in the URL path.
    let admitted = admit_generation(
        serde_json::from_value(generation_body("stems", json!({"song_id": "song-7"}))).unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.kind, OperationKind::Stems);
    assert_eq!(admitted.reserve_credits, 50);
    assert_eq!(admitted.song_id.as_deref(), Some("song-7"));
    let error = admit_generation(
        serde_json::from_value(generation_body("stems", json!({}))).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        GatewayFailure::BadRequest("suno_song_id_required")
    ));

    // Attachments on ANY operation fail closed: no upstream upload endpoint is documented
    // (manifest §4/§6), and this plane never invents one.
    for op in ["song", "extend", "lyrics", "stems"] {
        let error = admit_generation(
            serde_json::from_value(generation_body(
                op,
                json!({"model": "v5", "prompt": "x", "continue_clip_id": "c", "song_id": "s",
                       "attachments": ["suo-1"]}),
            ))
            .unwrap(),
        )
        .unwrap_err();
        assert!(
            matches!(error, GatewayFailure::Unsupported("suno_attachment_upstream_unknown")),
            "{op}"
        );
    }

    // An unknown field is rejected rather than silently dropped.
    assert!(serde_json::from_value::<GenerationBody>(generation_body(
        "song",
        json!({"model": "v5", "prompt": "x", "surprise": true}),
    ))
    .is_err());
}

// ── lifecycle: reserve → pre-check → create → poll → download → settle ──────

#[tokio::test]
async fn happy_path_reserves_creates_downloads_and_settles_the_attributed_delta() {
    let fixture = Fixture::new();
    let (origin, routes, requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert(
            "POST /v1/client/sessions*".into(),
            vec![mint(), mint(), mint(), mint()].into_iter().collect(),
        );
        table.insert("POST /api/c/check".into(), vec![no_captcha()].into_iter().collect());
        // Baseline read (2500 left), then the post-turn read (2495 left: the 5-credit song).
        table.insert(
            "GET /api/billing/info/".into(),
            vec![quota(2_500, 0), quota(2_495, 5)].into_iter().collect(),
        );
        table.insert(
            "POST /api/generate/v2/".into(),
            vec![(200, r#"[{"id":"clip-1","status":"queued"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
        table.insert(
            "GET /api/feed/v2*".into(),
            vec![(
                200,
                r#"[{"id":"clip-1","status":"complete","model_name":"chirp-v5-5","audio_url":"__BASE__/cdn/clip-1.mp3?sig=1"}]"#
                    .to_string(),
            )]
            .into_iter()
            .collect(),
        );
        table.insert(
            "GET /cdn/*".into(),
            vec![(200, "fake-mp3-bytes".to_string())]
                .into_iter()
                .collect(),
        );
    }
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000).await;
    let gateway = gateway_with(&fixture, &["suno-01"], &origin, Some(billing.clone()));

    let response = gateway
        .handle_create(
            generation_body("song", json!({"model": "v5.5", "prompt": "sea shanty"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let generation_id = created_generation_id(response).await;

    // The detached drain completes without the client: poll → download → exact settle.
    let view = wait_for_final(&gateway, &generation_id).await;
    assert_eq!(view.status, "complete");
    assert_eq!(view.artifacts, vec!["audio_url.mp3".to_string()]);
    let artifact = gateway
        .generation_artifact_path(&generation_id, "audio_url.mp3", None)
        .unwrap();
    assert_eq!(fs::read(&artifact).unwrap(), b"fake-mp3-bytes");

    // Attributed exactly: baseline 2500 → post 2495 ⇒ 5 credits = $0.02 at mult 10000.
    let account = billing.account("acct-1").await.unwrap().unwrap();
    assert_eq!(account.balance_nano, 1_000_000_000 - 20_000_000);

    // The upstream conversation: mint, pre-check, baseline, create, mint, feed, mint?, post…
    // — exact ordering is the single-flight's business; what matters is the wire shape.
    let log: Vec<String> = requests.try_iter().collect();
    assert!(log.iter().any(|r| r.contains("POST /v1/client/sessions/sess_suno-01/tokens")));
    assert!(log.iter().any(|r| r.contains("POST /api/c/check")));
    assert!(log.iter().any(|r| r.contains("POST /api/generate/v2/")));
    assert!(log.iter().any(|r| r.contains("GET /api/feed/v2?ids=clip-1")));
    // The customer never sees an upstream URL: only our generation id and artifact names.
    let status = gateway.generation_view(&generation_id, None).unwrap();
    let rendered = format!("{status:?}");
    assert!(!rendered.contains("clip-1"));
    assert!(!rendered.contains("cdn"));

    // The FIFO write fails without PostgreSQL: the head is retained and quota polling is
    // gated — evidence delivery honestly reports degraded, never a false green.
    let status = gateway.operational_status();
    assert!(!status.delivery.persistence_ok);
    assert_eq!(status.delivery.pending_events, 1);
}

#[tokio::test]
async fn captcha_required_soft_cools_and_rotates_without_solving() {
    let fixture = Fixture::new();
    let (origin, routes, requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert(
            "POST /v1/client/sessions*".into(),
            vec![mint(), mint(), mint(), mint(), mint()].into_iter().collect(),
        );
        // First pre-check (whichever profile the cursor picks) requires a CAPTCHA; the second
        // does not. No token is ever sent upstream — nothing is solved.
        table.insert(
            "POST /api/c/check".into(),
            vec![(200, r#"{"required":true}"#.to_string()), no_captcha()]
                .into_iter()
                .collect(),
        );
        table.insert(
            "GET /api/billing/info/".into(),
            vec![quota(2_500, 0), quota(2_490, 10), quota(2_480, 20)]
                .into_iter()
                .collect(),
        );
        table.insert(
            "POST /api/generate/concat/v2/".into(),
            vec![(200, r#"[{"id":"clip-x","status":"queued"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
        table.insert(
            "GET /api/feed/v2*".into(),
            vec![(200, r#"[{"id":"clip-x","status":"complete"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
    }
    let gateway = gateway_with(&fixture, &["suno-01", "suno-02"], &origin, None);
    let response = gateway
        .handle_create(
            generation_body("extend", json!({"continue_clip_id": "clip-0"})),
            ExecutionAttempt::direct(),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let generation_id = created_generation_id(response).await;
    let view = wait_for_final(&gateway, &generation_id).await;
    assert_eq!(view.status, "complete");

    let log: Vec<String> = requests.try_iter().collect();
    let create_calls = log
        .iter()
        .filter(|r| r.contains("POST /api/generate/concat/v2/"))
        .count();
    assert_eq!(create_calls, 1, "exactly one upstream creation");
    // The gated profile was soft-cooled, not hard-walled.
    let status = gateway.operational_status();
    assert_eq!(status.quota_walled_profiles, 0);
}

#[tokio::test]
async fn an_ambiguous_delta_settles_at_the_reserve_and_records_unattributed() {
    let fixture = Fixture::new();
    let (origin, routes, _requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert(
            "POST /v1/client/sessions*".into(),
            vec![mint(), mint(), mint(), mint(), mint(), mint()]
                .into_iter()
                .collect(),
        );
        table.insert("POST /api/c/check".into(), vec![no_captcha()].into_iter().collect());
        // The baseline read fails (500), so the turn carries no attribution baseline: the
        // post-turn read alone cannot attribute the movement.
        table.insert(
            "GET /api/billing/info/".into(),
            vec![(500, "down".to_string()), quota(2_450, 50)]
                .into_iter()
                .collect(),
        );
        table.insert(
            "POST /api/generate/concat/v2/".into(),
            vec![(200, r#"[{"id":"clip-x","status":"queued"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
        table.insert(
            "GET /api/feed/v2*".into(),
            vec![(200, r#"[{"id":"clip-x","status":"complete"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
    }
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000).await;
    let gateway = gateway_with(&fixture, &["suno-01"], &origin, Some(billing.clone()));
    let response = gateway
        .handle_create(
            generation_body("extend", json!({"continue_clip_id": "clip-0"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let generation_id = created_generation_id(response).await;
    let view = wait_for_final(&gateway, &generation_id).await;
    assert_eq!(view.status, "complete");

    // Reserve settlement: the documented conservative 50 credits = $0.20 at mult 10000.
    let account = billing.account("acct-1").await.unwrap().unwrap();
    assert_eq!(account.balance_nano, 1_000_000_000 - 200_000_000);
    assert_eq!(
        gateway.operational_status().unattributed_settlements,
        1,
        "the reserve settlement is recorded as unattributed"
    );
}

#[tokio::test]
async fn a_failed_generation_with_zero_movement_refunds_the_hold() {
    let fixture = Fixture::new();
    let (origin, routes, _requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert(
            "POST /v1/client/sessions*".into(),
            vec![mint(), mint(), mint()].into_iter().collect(),
        );
        table.insert("POST /api/c/check".into(), vec![no_captcha()].into_iter().collect());
        // No credit movement: 2500 → 2500.
        table.insert(
            "GET /api/billing/info/".into(),
            vec![quota(2_500, 0), quota(2_500, 0)].into_iter().collect(),
        );
        table.insert(
            "POST /api/generate/v2/".into(),
            vec![(200, r#"[{"id":"clip-1","status":"queued"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
        table.insert(
            "GET /api/feed/v2*".into(),
            vec![(200, r#"[{"id":"clip-1","status":"error"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
    }
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000).await;
    let gateway = gateway_with(&fixture, &["suno-01"], &origin, Some(billing.clone()));
    let response = gateway
        .handle_create(
            generation_body("song", json!({"model": "v5", "prompt": "x"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let generation_id = created_generation_id(response).await;
    let view = wait_for_final(&gateway, &generation_id).await;
    assert_eq!(view.status, "error");
    // The documented refund (manifest §4.1): the customer keeps the whole hold.
    let account = billing.account("acct-1").await.unwrap().unwrap();
    assert_eq!(account.balance_nano, 1_000_000_000);
}

#[tokio::test]
async fn a_zero_quota_probe_hard_walls_the_profile_and_the_fleet_429s() {
    let fixture = Fixture::new();
    let (origin, routes, _requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert("POST /v1/client/sessions*".into(), vec![mint()].into_iter().collect());
        table.insert(
            "GET /api/billing/info/".into(),
            vec![quota(0, 2_500)].into_iter().collect(),
        );
    }
    let gateway = gateway_with(&fixture, &["suno-01"], &origin, None);
    // Preflight publishes the zeroed quota: explicit exhaustion is the HARD verdict.
    gateway.preflight().await;
    let response = gateway
        .handle_create(
            generation_body("song", json!({"model": "v5", "prompt": "x"})),
            ExecutionAttempt::direct(),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key("retry-after"));
    assert_eq!(gateway.operational_status().quota_walled_profiles, 1);
}

#[tokio::test]
async fn transport_faults_rotate_before_create_and_never_double_create() {
    let fixture = Fixture::new();
    let (origin, routes, requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert(
            "POST /v1/client/sessions*".into(),
            vec![mint(), mint(), mint(), mint()].into_iter().collect(),
        );
        table.insert(
            "POST /api/c/check".into(),
            vec![no_captcha(), no_captcha()].into_iter().collect(),
        );
        table.insert(
            "GET /api/billing/info/".into(),
            vec![quota(2_500, 0), quota(2_495, 5), quota(2_495, 5)]
                .into_iter()
                .collect(),
        );
        // First creation attempt dies transport-side; nothing was created, so rotation is
        // legal. The second profile creates exactly once.
        table.insert(
            "POST /api/generate/v2/".into(),
            vec![
                (500, "boom".to_string()),
                (200, r#"[{"id":"clip-1","status":"queued"}]"#.to_string()),
            ]
            .into_iter()
            .collect(),
        );
        table.insert(
            "GET /api/feed/v2*".into(),
            vec![(200, r#"[{"id":"clip-1","status":"complete"}]"#.to_string())]
                .into_iter()
                .collect(),
        );
    }
    let gateway = gateway_with(&fixture, &["suno-01", "suno-02"], &origin, None);
    let response = gateway
        .handle_create(
            generation_body("song", json!({"model": "v5", "prompt": "x"})),
            ExecutionAttempt::direct(),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let log: Vec<String> = requests.try_iter().collect();
    let creates = log
        .iter()
        .filter(|r| r.contains("POST /api/generate/v2/"))
        .count();
    assert_eq!(creates, 2, "one failed attempt + exactly one creation");
}

#[tokio::test]
async fn a_corrupted_roster_reload_keeps_last_good_capacity() {
    let fixture = Fixture::new();
    let (origin, routes, _requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert("POST /v1/client/sessions*".into(), vec![mint()].into_iter().collect());
        table.insert(
            "GET /api/billing/info/".into(),
            vec![quota(2_500, 0)].into_iter().collect(),
        );
    }
    // Publish a real roster on disk so the gateway has a reload source.
    let profile = fixture.profile("suno-01", "sess_reload");
    let roster = json!({"profiles": [{"id": "suno-01",
        "credential_file": fixture.root.join("credentials").join("suno-01.json").to_string_lossy()}]});
    fs::write(fixture.root.join("profiles.json"), serde_json::to_vec(&roster).unwrap()).unwrap();
    fs::set_permissions(
        fixture.root.join("profiles.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let config = config(&fixture);
    let runtime = RuntimeProfile::from_roster_on(
        profile,
        &config,
        SunoHosts::loopback(origin.clone(), origin),
    )
    .unwrap();
    let gateway = Arc::new(SunoGateway::from_profiles(config, None, vec![runtime]));
    gateway.preflight().await;
    assert_eq!(gateway.operational_status().live_profiles, 1);

    // A corrupted roster must never empty a working pool.
    fs::write(fixture.root.join("profiles.json"), b"not json").unwrap();
    assert!(!gateway.refresh_profiles().await);
    assert_eq!(gateway.operational_status().total_profiles, 1);
    assert_eq!(gateway.operational_status().live_profiles, 1);
}

#[tokio::test]
async fn the_admin_projection_is_privacy_safe_by_construction() {
    let fixture = Fixture::new();
    let (origin, routes, _requests) = mock_upstream(&[]);
    {
        let mut table = routes.lock().unwrap();
        table.insert("POST /v1/client/sessions*".into(), vec![mint()].into_iter().collect());
        table.insert(
            "GET /api/billing/info/".into(),
            vec![quota(2_400, 100)].into_iter().collect(),
        );
    }
    let gateway = gateway_with(&fixture, &["suno-01"], &origin, None);
    gateway.preflight().await;
    let status = gateway.operational_status();
    assert_eq!(status.live_profiles, 1);
    let profile = &status.profiles[0];
    assert_eq!(profile.plan, "Pro");
    assert!(profile.live && profile.routable);
    assert_eq!(profile.total_credits_left, Some(2_400));
    assert_eq!(profile.monthly_limit, Some(2_500));
    assert_eq!(profile.monthly_usage, Some(100));
    // The projection carries no cookie, session id, proxy or subject material.
    let rendered = format!("{status:?}");
    assert!(!rendered.contains("clerk-sess_suno-01"));
    assert!(!rendered.contains("sess_suno-01"));
    assert!(!rendered.contains("ajs_id"));
    assert!(!rendered.contains("cookie"));
}

#[tokio::test]
async fn upload_intake_persists_bounded_audio_and_rejects_the_rest() {
    let fixture = Fixture::new();
    let gateway = gateway_with(&fixture, &[], "http://127.0.0.1:1", None);
    let response = gateway
        .handle_audio_upload(Bytes::from_static(b"ID3 fake mp3 payload"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert!(parsed["upload_id"].as_str().unwrap().starts_with("suo-"));
    assert_eq!(parsed["format"], json!("mp3"));
    // Opaque content is refused with a bounded 400.
    let response = gateway
        .handle_audio_upload(Bytes::from_static(b"%PDF-1.7 not audio"))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn the_credit_delta_reads_drawdown_or_usage_never_a_refill() {
    let baseline = |left, usage| BillingSnapshot {
        total_credits_left: left,
        monthly_limit: Some(2_500),
        monthly_usage: usage,
        period_raw: None,
    };
    // Drawdown: 2500 → 2495 = 5 credits.
    assert_eq!(credit_delta(&baseline(Some(2_500), Some(0)), &baseline(Some(2_495), Some(5))), Some(5));
    // A refill (left grew) is not consumption evidence.
    assert_eq!(credit_delta(&baseline(Some(2_000), Some(500)), &baseline(Some(2_500), Some(0))), None);
    // Usage-only fallback.
    assert_eq!(credit_delta(&baseline(None, Some(10)), &baseline(None, Some(15))), Some(5));
    // Both halves missing: unattributable.
    assert_eq!(credit_delta(&baseline(None, None), &baseline(None, None)), None);
}

#[test]
fn upstream_clip_ids_are_audit_metadata_never_serving_identity() {
    let finalized = Finalized::Clips {
        clips: vec![ClipState {
            id: "clip-audit".into(),
            ..ClipState::default()
        }],
        succeeded: true,
    };
    assert_eq!(upstream_clip_id_of(&finalized), "clip-audit");
}
