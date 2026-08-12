use super::super::config::Tripo3dPlaneConfig;
use super::super::transport::{AuthScheme, ProbeRoute, Tripo3dTransportConfig};
use super::*;
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tripo3d_credential::{encode_envelope, CredentialKeyring, Tripo3dCredentialKind};

// ── fixtures ────────────────────────────────────────────────────────────────

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let suffix = random.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let root = std::env::temp_dir().join(format!("tripo3d-gateway-{suffix}"));
        fs::create_dir_all(root.join("credentials")).unwrap();
        fs::create_dir_all(root.join("artifacts")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self { root }
    }

    fn seal(&self, id: &str, api_key: &str, base_url: &str) {
        let credential = Tripo3dCredential {
            version: 1,
            kind: Tripo3dCredentialKind::ApiKey,
            api_key: api_key.into(),
            cohort: "tripo3d-api-50".into(),
            base_url: base_url.into(),
            proxy_url: String::new(),
        };
        let envelope = keyring().seal("a1", id, &credential).unwrap();
        let credential_path = self.root.join("credentials").join(format!("{id}.json"));
        write_private(&credential_path, &encode_envelope(&envelope).unwrap());
    }

    fn publish(&self, profiles: &[(&str, &str, &str)]) {
        for (id, key, base_url) in profiles {
            self.seal(id, key, base_url);
        }
        let entries = profiles
            .iter()
            .map(|(id, _, _)| {
                json!({"id": id, "credential_file": self.root.join("credentials").join(format!("{id}.json")).to_string_lossy()})
            })
            .collect::<Vec<_>>();
        write_private(
            &self.root.join("profiles.json"),
            &serde_json::to_vec(&json!({"profiles": entries})).unwrap(),
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

fn config(fixture: &Fixture) -> Tripo3dPlaneConfig {
    Tripo3dPlaneConfig {
        roster_dir: fixture.root.clone(),
        keyring: keyring(),
        transport: Tripo3dTransportConfig {
            auth_scheme: AuthScheme::Bearer,
            request_timeout: Duration::from_secs(5),
        },
        readiness_probe: ProbeRoute::Balance,
        balance_poll_interval: Duration::from_secs(300),
        artifact_dir: fixture.root.join("artifacts"),
    }
}

fn gateway_with(fixture: &Fixture, billing: Option<Arc<AsyncBilling>>) -> Arc<Tripo3dGateway> {
    Tripo3dGateway::new_with_calibration(config(fixture), billing).unwrap()
}

// ── path-routed mock upstream ───────────────────────────────────────────────

/// A mock Tripo3D platform: requests are matched by `METHOD PATH` (a trailing `*` marks a path
/// prefix), answered from a per-route FIFO, and logged. Bodies may contain the `__BASE__`
/// marker, replaced at serve time with the mock's own origin. Responses are raw HTTP/1.1 with
/// `connection: close`.
type RouteTable = Arc<Mutex<HashMap<String, VecDeque<(u16, Vec<u8>)>>>>;

fn mock_upstream(routes: &[(&str, &[(u16, String)])]) -> (String, RouteTable, mpsc::Receiver<String>) {
    let table: HashMap<String, VecDeque<(u16, Vec<u8>)>> = routes
        .iter()
        .map(|(path, responses)| {
            (
                (*path).to_string(),
                responses
                    .iter()
                    .map(|(status, body)| (*status, body.as_bytes().to_vec()))
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
                    // Path-prefix fallback for task polls (`GET /v2/openapi/task/<id>`) and
                    // CDN artifact fetches: routes registered with a trailing `*`.
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
                response.unwrap_or((404, b"{\"code\":1404,\"message\":\"unrouted\"}".to_vec()));
            let body = String::from_utf8_lossy(&body).replace("__BASE__", &origin).into_bytes();
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

/// Read the request head plus a content-length body (upload bodies are asserted by tests).
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

fn ok_balance(balance: &str) -> (u16, String) {
    (
        200,
        format!("{{\"code\":0,\"data\":{{\"balance\":{balance},\"frozen\":0}}}}"),
    )
}

fn generation_body(task_type: &str, extra: Value) -> Value {
    let mut body = json!({"type": task_type});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    body
}

async fn billing_with_funded_account(
    fixture: &Fixture,
    balance_nano: i64,
) -> (Arc<AsyncBilling>, Tripo3dBillingInput) {
    let path = fixture.root.join("billing.sqlite");
    let billing = Arc::new(AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap());
    billing.create_account("acct-1", None, 10_000).await.unwrap();
    billing
        .issue_key("sk-test", "acct-1", None, None, None)
        .await
        .unwrap();
    billing.topup("acct-1", balance_nano, None).await.unwrap();
    let input = Tripo3dBillingInput {
        account_id: "acct-1".into(),
        key: "sk-test".into(),
        mult_bp: 10_000,
        available_nano: balance_nano,
    };
    (billing, input)
}

async fn wait_for_task_final(gateway: &Tripo3dGateway, task_id: &str) -> Tripo3dTaskView {
    for _ in 0..800 {
        if let Some(view) = gateway.task_view(task_id, None) {
            if ["success", "failed", "expired", "banned", "cancelled", "unknown"]
                .contains(&view.status.as_str())
            {
                return view;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("task {task_id} never finalized");
}

async fn created_task_id(response: Response) -> String {
    let rt_body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    serde_json::from_slice::<Value>(&rt_body).unwrap()["task_id"]
        .as_str()
        .unwrap()
        .to_string()
}

// ── admission validation matrix ─────────────────────────────────────────────

#[test]
fn admission_matrix_covers_the_full_catalog_and_every_fail_closed_rule() {
    let fixture = Fixture::new();
    let gateway = gateway_with(&fixture, None);

    // Unknown type names the admitted set, never a guess.
    let error = admit_task(
        &gateway,
        serde_json::from_value(generation_body("text_to_3d", json!({}))).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, GatewayFailure::BadRequest("tripo3d_task_type_unknown")));

    // The conflicted highpoly task stays closed on both spellings.
    for version in ["P-v2.0-20251225", "P-v2.0-20251226"] {
        let error = admit_task(
            &gateway,
            serde_json::from_value(generation_body(
                "highpoly_to_lowpoly",
                json!({"original_model_task_id": "t", "model_version": version}),
            ))
            .unwrap(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            GatewayFailure::BadRequest("tripo3d_highpoly_version_conflict")
        ));
    }

    // An unlisted model_version fails closed — the conservative fallback must not launder it.
    let error = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "text_to_model",
            json!({"prompt": "a cat", "model_version": "v9.9-20990101"}),
        ))
        .unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, GatewayFailure::Unsupported("tripo3d_task_unpriced")));

    // text_to_model happy admission, exact price (v2.5 default, no texture).
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body("text_to_model", json!({"prompt": "a cat"})))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.kind, Tripo3dTaskKind::TextToModel);
    assert_eq!(admitted.reserve_credits, 10);
    assert!(!admitted.conservative);
    assert_eq!(admitted.upstream_body["prompt"], json!("a cat"));

    // Conservative family-max reserve: P1 with a surcharge the card does not price on P1.
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "text_to_model",
            json!({"prompt": "a cat", "model_version": "P1-20260311", "smart_low_poly": true}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 100);
    assert!(admitted.conservative);
    assert_eq!(admitted.upstream_body["smart_low_poly"], json!(true));

    // The style surcharge has no proven wire field on *_to_model: named limitation.
    let error = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "text_to_model",
            json!({"prompt": "a cat", "style": true}),
        ))
        .unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, GatewayFailure::Unsupported("tripo3d_style_wire_unproven")));

    // image_to_model requires exactly one image input form.
    let error = admit_task(
        &gateway,
        serde_json::from_value(generation_body("image_to_model", json!({}))).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, GatewayFailure::BadRequest("tripo3d_image_input_invalid")));
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "image_to_model",
            json!({"image_token": "tok-1", "image_type": "png", "texture": true, "texture_quality": "detailed"}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 40); // 20 base + detailed 20
    assert_eq!(
        admitted.upstream_body["file"],
        json!({"type": "png", "file_token": "tok-1"})
    );
    assert_eq!(admitted.upstream_body["texture_quality"], json!("detailed"));

    // multiview: exactly 4 slots, front mandatory; sparse slots ride as empty objects.
    let error = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "multiview_to_model",
            json!({"files": [null, {"image_token": "t"}, null, null]}),
        ))
        .unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, GatewayFailure::BadRequest("tripo3d_multiview_slots_invalid")));
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "multiview_to_model",
            json!({"files": [
                {"image_token": "front"}, {"image_url": "https://example.com/left.png"}, null, null
            ], "model_version": "v3.0-20250812"}),
        ))
        .unwrap(),
    )
    .unwrap();
    let files = admitted.upstream_body["files"].as_array().unwrap();
    assert_eq!(files.len(), 4);
    assert_eq!(files[0], json!({"type": "jpeg", "file_token": "front"}));
    assert_eq!(files[1], json!({"type": "jpeg", "url": "https://example.com/left.png"}));
    assert_eq!(files[2], json!({}));
    assert_eq!(admitted.reserve_credits, 20);

    // texture_model: original task required; style rides texture_prompt.style_image; quality
    // is the price selector; the wire always textures.
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "texture_model",
            json!({"original_model_task_id": "task-1", "texture_quality": "extreme",
                   "style": true, "style_image_token": "style-tok"}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 35); // extreme 30 + style ref 5
    assert_eq!(admitted.upstream_body["texture"], json!(true));
    assert_eq!(
        admitted.upstream_body["texture_prompt"]["style_image"],
        json!({"type": "jpeg", "file_token": "style-tok"})
    );

    // animate_retarget prices by animation count.
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "animate_retarget",
            json!({"original_model_task_id": "t", "animations": ["idle", "walk"]}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 20);
    assert_eq!(admitted.upstream_body["animations"], json!(["idle", "walk"]));

    // convert_model: advanced reserves the published advanced price conservatively (the wire
    // selector is unproven, manifest §6 — settle stays exact from consumed_credit).
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "convert_model",
            json!({"original_model_task_id": "t", "format": "GLTF", "mode": "advanced"}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 10);
    assert!(admitted.conservative);

    // Free tasks reserve an exact zero.
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "animate_prerigcheck",
            json!({"original_model_task_id": "t"}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 0);

    // edit_multiview_image prices per edited view.
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "edit_multiview_image",
            json!({"original_task_id": "t", "prompts": [
                {"view": "front", "prompt": "make it red"},
                {"view": "left", "prompt": "make it blue"}
            ]}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 10);

    // refine_model is the legacy flat 30.
    let admitted = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "refine_model",
            json!({"draft_model_task_id": "draft-1"}),
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(admitted.reserve_credits, 30);

    // Options do not ride onto kinds they do not price.
    let error = admit_task(
        &gateway,
        serde_json::from_value(generation_body(
            "text_to_image",
            json!({"prompt": "a cat", "quad": true}),
        ))
        .unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, GatewayFailure::BadRequest("tripo3d_option_not_applicable")));

    // Unknown fields never pass silently.
    assert!(serde_json::from_value::<GenerationBody>(generation_body(
        "text_to_model",
        json!({"prompt": "a", "surprise": 1})
    ))
    .is_err());
}

// ── lifecycle over the mock upstream ────────────────────────────────────────

#[tokio::test]
async fn happy_path_reserves_creates_downloads_and_settles_exactly() {
    let fixture = Fixture::new();
    let (base_url, _routes, requests) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
        ("POST /v2/openapi/task", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-1\"}}".into(),
        )]),
        ("GET /v2/openapi/task/*", &[
            (200, "{\"code\":0,\"data\":{\"task_id\":\"up-1\",\"type\":\"text_to_model\",\"status\":\"running\",\"progress\":10}}".into()),
            (200, "{\"code\":0,\"data\":{\"task_id\":\"up-1\",\"type\":\"text_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":10,\"input\":{\"model_version\":\"v2.5-20250123\"},\"output\":{\"model\":\"__BASE__/model.glb?sig=1\",\"rendered_image\":\"__BASE__/r.jpg?sig=2\"}}}".into()),
        ]),
        ("GET /model.glb*", &[(200, "GLB-BYTES".into())]),
        ("GET /r.jpg*", &[(200, "JPG-BYTES".into())]),
    ]);
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing.clone()));
    assert_eq!(gateway.preflight().await, 1);

    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a dachshund"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let task_id = created_task_id(response).await;

    let view = wait_for_task_final(&gateway, &task_id).await;
    assert_eq!(view.status, "success");
    // Both documented artifact fields downloaded into OUR store and are named for serving.
    assert_eq!(view.artifacts, ["model.glb", "rendered_image.jpg"]);
    let artifact = fixture.root.join("artifacts").join(&task_id).join("model.glb");
    assert_eq!(fs::read(&artifact).unwrap(), b"GLB-BYTES");
    let mode = fs::metadata(&artifact).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "artifacts are private by construction");

    // Settlement is exact from consumed_credit: 10 credits = $0.10 = 100_000_000 nano.
    let account = billing.account("acct-1").await.unwrap().unwrap();
    assert_eq!(account.balance_nano, 1_000_000_000_000 - 100_000_000);
    assert_eq!(account.reserved_nano, 0, "the hold is fully released");

    // The immutable event carries both exact legs at the fixed rate; the FIFO head is the
    // paired turn (SQLite authority keeps it pending — the calibration writer is PG-only).
    let head = gateway
        .turn_queue
        .lock()
        .expect("queue")
        .head()
        .cloned()
        .expect("the turn is queued");
    assert_eq!(head.event.request_id, task_id);
    assert_eq!(head.event.task_type, "text_to_model");
    assert_eq!(head.event.native_total_millicredits, 10_000);
    assert_eq!(head.event.api_total_nanousd, 100_000_000);
    assert_eq!(head.event.upstream_task_id, "up-1");
    assert_eq!(
        head.event.resolved_model_version.as_deref(),
        Some("v2.5-20250123")
    );
    // Codex/Gemini pairing: the post-turn balance read rode the same FIFO entry.
    let balance = head.balance.expect("the post-turn balance read is paired");
    assert_eq!(balance.balance_raw, "50");

    // The request log proves lifecycle order and auth: create precedes the first poll, and
    // every platform call authenticates with the profile's key.
    let mut log = Vec::new();
    while let Ok(request) = requests.recv_timeout(Duration::from_secs(5)) {
        let platform = request.lines().next().unwrap_or("").contains("/v2/openapi/");
        if platform {
            log.push(request);
        }
        if log
            .iter()
            .filter(|r| r.starts_with("GET /v2/openapi/task/up-1"))
            .count()
            >= 2
            && log.iter().any(|r| r.starts_with("POST /v2/openapi/task "))
        {
            break;
        }
    }
    let create_at = log
        .iter()
        .position(|r| r.starts_with("POST /v2/openapi/task "))
        .unwrap();
    let first_poll = log
        .iter()
        .position(|r| r.starts_with("GET /v2/openapi/task/up-1"))
        .unwrap();
    assert!(create_at < first_poll);
    assert!(log
        .iter()
        .all(|r| r.contains("authorization: Bearer tsk_key-1")));
}

#[tokio::test]
async fn a_failed_task_refunds_the_hold_and_records_the_zero_pair() {
    let fixture = Fixture::new();
    let (base_url, _r, _requests) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-f\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-f\",\"type\":\"text_to_model\",\"status\":\"failed\",\"progress\":0,\"consumed_credit\":0,\"error_code\":3001}}".into(),
        )]),
    ]);
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing.clone()));
    gateway.preflight().await;

    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let task_id = created_task_id(response).await;
    let view = wait_for_task_final(&gateway, &task_id).await;
    assert_eq!(view.status, "failed");

    // Documented refund: the hold is returned in full (consumed_credit = 0, manifest §4.1).
    let account = billing.account("acct-1").await.unwrap().unwrap();
    assert_eq!(account.balance_nano, 1_000_000_000_000);
    assert_eq!(account.reserved_nano, 0);
    let head = gateway.turn_queue.lock().expect("queue").head().cloned().unwrap();
    assert_eq!(head.event.native_total_millicredits, 0);
    assert_eq!(head.event.api_total_nanousd, 0);
}

#[tokio::test]
async fn rotation_is_legal_only_before_a_successful_create() {
    let fixture = Fixture::new();
    // Profile A answers the documented hard wall (429 + code 2000); profile B creates. After
    // creation, mid-poll 500s on B never rotate — B owns the task (per-key isolation).
    let (base_a, _r, _ra) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50")]),
        ("POST /v2/openapi/task", &[(429, "{\"code\":2000,\"message\":\"concurrency\"}".into())]),
    ]);
    let (base_b, _r, _rb) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-b\"}}".into())]),
        ("GET /v2/openapi/task/*", &[
            (500, "boom".into()),
            (500, "boom".into()),
            (200, "{\"code\":0,\"data\":{\"task_id\":\"up-b\",\"type\":\"text_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":10,\"output\":{}}}".into()),
        ]),
    ]);
    fixture.publish(&[("tripo3d-a", "tsk_key-a", &base_a), ("tripo3d-b", "tsk_key-b", &base_b)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing.clone()));
    assert_eq!(gateway.preflight().await, 2);

    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the wall rotates to the healthy profile"
    );
    let task_id = created_task_id(response).await;
    let view = wait_for_task_final(&gateway, &task_id).await;
    assert_eq!(view.status, "success");

    // The 429+2000 wall cooled profile A on the HARD axis.
    let status = gateway.operational_status();
    let a = status.profiles.iter().find(|p| p.id == "tripo3d-a").unwrap();
    assert!(a.rate_limit_cool_until.is_some());
    // The task was created exactly once, on B, and drained there despite mid-poll 500s.
    let head = gateway.turn_queue.lock().expect("queue").head().cloned().unwrap();
    assert_eq!(head.event.upstream_task_id, "up-b");
    assert_eq!(head.event.native_total_millicredits, 10_000);
    let account = billing.account("acct-1").await.unwrap().unwrap();
    assert_eq!(account.balance_nano, 1_000_000_000_000 - 100_000_000);
}

#[tokio::test]
async fn a_full_soft_fleet_still_serves_and_a_full_hard_fleet_answers_429() {
    let fixture = Fixture::new();
    let (base_a, _r, _ra) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50")]),
        ("POST /v2/openapi/task", &[(401, "{\"code\":401,\"message\":\"invalid key\"}".into())]),
    ]);
    let (base_b, _r, _rb) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-soft\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-soft\",\"type\":\"text_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":10,\"output\":{}}}".into(),
        )]),
    ]);
    fixture.publish(&[("tripo3d-a", "tsk_key-a", &base_a), ("tripo3d-b", "tsk_key-b", &base_b)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing));
    gateway.preflight().await;
    // Force both profiles onto the soft axis.
    for profile in gateway.profiles_snapshot() {
        profile.apply_effect(ProfileEffect::SoftAuthFault, now_unix(), None);
    }
    assert_eq!(
        gateway.operational_status().available_profiles,
        0,
        "strict pass is empty"
    );
    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input.clone()),
        )
        .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "THE pool-must-not-empty invariant: soft axes never empty the pool"
    );

    // A full-hard fleet answers an honest 429 with Retry-After, never an invented 503.
    for profile in gateway.profiles_snapshot() {
        profile.apply_effect(ProfileEffect::RestForBalance, now_unix(), None);
    }
    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().get("retry-after").is_some());
    assert_eq!(
        response
            .extensions()
            .get::<crate::proxy::TerminalErrorReason>()
            .map(|reason| reason.0),
        Some("tripo3d_capacity_exhausted")
    );
}

#[tokio::test]
async fn the_transport_budget_bounds_rotation_and_surfaces_the_upstream_error() {
    let fixture = Fixture::new();
    let (base, _routes, requests) = mock_upstream(&[
        (
            "GET /v2/openapi/user/balance",
            &[ok_balance("50"), ok_balance("50"), ok_balance("50")],
        ),
        ("POST /v2/openapi/task", &[
            (503, "down".into()),
            (503, "down".into()),
            (503, "down".into()),
            (503, "down".into()),
        ]),
    ]);
    fixture.publish(&[
        ("tripo3d-1", "tsk_k1", &base),
        ("tripo3d-2", "tsk_k2", &base),
        ("tripo3d-3", "tsk_k3", &base),
    ]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing));
    gateway.preflight().await;

    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    // Exactly the budget: three create attempts, no more (the 4th mocked answer is unconsumed).
    let creates = (0..10)
        .filter_map(|_| requests.recv_timeout(Duration::from_millis(300)).ok())
        .filter(|request| request.starts_with("POST /v2/openapi/task "))
        .count();
    assert_eq!(creates, AttemptPolicy::default().transport_budget as usize);
}

#[tokio::test]
async fn consumed_credit_above_the_reserve_bound_is_a_quarantined_anomaly() {
    let fixture = Fixture::new();
    let (base_url, _r, _requests) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-x\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-x\",\"type\":\"text_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":999,\"output\":{}}}".into(),
        )]),
    ]);
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing.clone()));
    gateway.preflight().await;

    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    let task_id = created_task_id(response).await;
    let view = wait_for_task_final(&gateway, &task_id).await;
    assert_eq!(view.status, "success");

    // 999 credits exceed the admitted shape's reserve (10): typed anomaly, the conservative
    // hold settles, and NO immutable event is created — never silent acceptance.
    assert_eq!(gateway.tariff_anomaly.load(Ordering::Relaxed), 1);
    let account = billing.account("acct-1").await.unwrap().unwrap();
    assert_eq!(
        account.balance_nano,
        1_000_000_000_000 - 100_000_000,
        "the conservative hold (the reserve) is what settles"
    );
    assert!(gateway.turn_queue.lock().expect("queue").is_empty());
}

#[tokio::test]
async fn a_pending_fifo_head_blocks_the_balance_poll() {
    let fixture = Fixture::new();
    let (base_url, _r, _requests) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50")]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-q\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-q\",\"type\":\"text_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":10,\"output\":{}}}".into(),
        )]),
    ]);
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    // billing None: the calibration write can never land, so the head stays pending.
    let gateway = gateway_with(&fixture, None);
    gateway.preflight().await;

    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let task_id = created_task_id(response).await;
    wait_for_task_final(&gateway, &task_id).await;

    assert_eq!(gateway.operational_status().delivery.pending_events, 1);
    // The poll path refuses to read balance while evidence is undelivered.
    assert_eq!(gateway.poll_balances().await, 0);
    assert_eq!(gateway.operational_status().delivery.pending_events, 1);
    assert!(gateway.readiness().is_err());
}

#[tokio::test]
async fn roster_reload_preserves_last_good_and_probes_before_publication() {
    let fixture = Fixture::new();
    let (base_a, _r, _ra) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
    ]);
    let (base_b, _r, _rb) = mock_upstream(&[
        // The new profile's key is rejected: it must not join the serving generation.
        ("GET /v2/openapi/user/balance", &[(401, "{\"code\":401}".into())]),
    ]);
    fixture.publish(&[("tripo3d-a", "tsk_key-a", &base_a)]);
    let gateway = gateway_with(&fixture, None);
    assert_eq!(gateway.preflight().await, 1);
    let generation = gateway.profiles_snapshot();

    // A broken roster (the credential file vanishes) keeps last-good capacity.
    fs::remove_file(fixture.root.join("credentials/tripo3d-a.json")).unwrap();
    assert!(!gateway.refresh_profiles().await);
    let after = gateway.profiles_snapshot();
    assert!(Arc::ptr_eq(&generation[0], &after[0]), "an unchanged profile keeps its Arc");

    // A new profile whose probe fails must not be published.
    fixture.publish(&[
        ("tripo3d-a", "tsk_key-a", &base_a),
        ("tripo3d-b", "tsk_key-b", &base_b),
    ]);
    assert!(!gateway.refresh_profiles().await, "the whole reload fails closed");
    assert_eq!(gateway.profiles_snapshot().len(), 1);
}

#[tokio::test]
async fn task_views_are_isolated_per_account_and_never_leak_upstream_identity() {
    let fixture = Fixture::new();
    let (base_url, _r, _requests) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-priv\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-priv\",\"type\":\"text_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":10,\"output\":{}}}".into(),
        )]),
    ]);
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing));
    gateway.preflight().await;

    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    let task_id = created_task_id(response).await;
    wait_for_task_final(&gateway, &task_id).await;

    // The owner and an admin read; a foreign account gets the same nothing as an unknown id.
    assert!(gateway.task_view(&task_id, Some("acct-1")).is_some());
    assert!(gateway.task_view(&task_id, None).is_some());
    assert!(gateway.task_view(&task_id, Some("acct-2")).is_none());
    assert!(gateway.task_view("unknown-task", None).is_none());

    // The view never carries the upstream task id or the signed URLs.
    let view = gateway.task_view(&task_id, None).unwrap();
    let rendered = format!("{view:?}");
    assert!(!rendered.contains("up-priv"));
    assert!(!rendered.contains("sig="));
}

#[tokio::test]
async fn the_operational_projection_is_privacy_safe() {
    let fixture = Fixture::new();
    let (base_url, _r, _requests) =
        mock_upstream(&[("GET /v2/openapi/user/balance", &[ok_balance("50")])]);
    fixture.publish(&[("tripo3d-01", "tsk_secret-key-9f8c", &base_url)]);
    let gateway = gateway_with(&fixture, None);
    gateway.preflight().await;
    let status = gateway.operational_status();
    let rendered = format!("{status:?}");
    assert!(!rendered.contains("tsk_secret-key-9f8c"));
    assert!(!rendered.contains(&gateway.profiles_snapshot()[0].subject_id));
    assert!(rendered.contains("tripo3d-01"));
    assert!(rendered.contains("tripo3d-api-50"));
    // Balance halves are raw text evidence; parsed halves stay None while the unit is unproven.
    let profile = &status.profiles[0];
    assert_eq!(profile.balance_raw.as_deref(), Some("50"));
    assert_eq!(profile.balance_micro_units, None);
    assert!(profile.live);
}

#[tokio::test]
async fn image_upload_passes_through_and_pins_the_token_to_its_profile() {
    let fixture = Fixture::new();
    let (base_url, _routes, requests) = mock_upstream(&[
        (
            "GET /v2/openapi/user/balance",
            &[ok_balance("50"), ok_balance("50"), ok_balance("50")],
        ),
        ("POST /v2/openapi/upload/sts", &[(200, "{\"code\":0,\"data\":{\"image_token\":\"imgtok-1\"}}".into())]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-img\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-img\",\"type\":\"image_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":20,\"output\":{}}}".into(),
        )]),
    ]);
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing));
    gateway.preflight().await;

    let response = gateway
        .handle_image_upload("cat.png", Bytes::from_static(b"\x89PNG\r\n\x1a\nminimal"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["image_token"],
        json!("imgtok-1")
    );
    // The upstream received a multipart body with the exact bytes.
    let upload = (0..4)
        .filter_map(|_| requests.recv_timeout(Duration::from_secs(5)).ok())
        .find(|request| request.starts_with("POST /v2/openapi/upload/sts "))
        .expect("the upload reached the mock");
    assert!(upload.contains("multipart/form-data; boundary="));
    assert!(upload.contains("Content-Type: image/png"));

    // The task created with that token lands on the SAME profile (account-scoped upload).
    let response = gateway
        .handle_create(
            generation_body("image_to_model", json!({"image_token": "imgtok-1"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let create = (0..4)
        .filter_map(|_| requests.recv_timeout(Duration::from_secs(5)).ok())
        .find(|request| request.starts_with("POST /v2/openapi/task "))
        .expect("the create reached the mock");
    assert!(create.contains("\"file_token\":\"imgtok-1\""));
}

#[tokio::test]
async fn model_upload_runs_the_sts_flow_and_import_references_the_object() {
    let fixture = Fixture::new();
    let (base_url, routes, requests) = mock_upstream(&[
        (
            "GET /v2/openapi/user/balance",
            &[ok_balance("50"), ok_balance("50"), ok_balance("50")],
        ),
        ("POST /v2/openapi/upload/sts/token", &[(
            200,
            "{\"code\":0,\"data\":{\"s3_host\":\"__BASE__\",\"sts_ak\":\"AK\",\"sts_sk\":\"SK\",\"session_token\":\"ST\",\"resource_bucket\":\"bucket\",\"resource_uri\":\"obj/key.glb\"}}".into(),
        )]),
        ("PUT /obj/key.glb*", &[(200, "{}".into())]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-imp\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-imp\",\"type\":\"import_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":0,\"output\":{}}}".into(),
        )]),
    ]);
    let _ = routes;
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing));
    gateway.preflight().await;

    let response = gateway
        .handle_model_upload("model.glb", Bytes::from_static(b"glTF-BINARY"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    let model_token = serde_json::from_slice::<Value>(&body).unwrap()["model_token"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(model_token.starts_with("t3m-"));
    // The S3 PUT carried the SigV4 headers.
    let put = (0..4)
        .filter_map(|_| requests.recv_timeout(Duration::from_secs(5)).ok())
        .find(|request| request.starts_with("PUT /obj/key.glb "))
        .expect("the S3 PUT reached the mock");
    assert!(put.contains("authorization: AWS4-HMAC-SHA256 Credential=AK/"));
    assert!(put.contains("x-amz-content-sha256"));

    // import_model references the stored object and runs pinned to the uploading profile.
    let response = gateway
        .handle_create(
            generation_body("import_model", json!({"model_token": model_token})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let create = (0..3)
        .filter_map(|_| requests.recv_timeout(Duration::from_secs(5)).ok())
        .find(|request| request.starts_with("POST /v2/openapi/task "))
        .expect("the create reached the mock");
    assert!(create.contains("\"bucket\":\"bucket\""));
    assert!(create.contains("\"key\":\"obj/key.glb\""));

    // An unknown token names nothing.
    let response = gateway
        .handle_create(
            generation_body("import_model", json!({"model_token": "t3m-unknown"})),
            ExecutionAttempt::direct(),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn shutdown_drains_detached_work_and_flushes_the_fifo() {
    let fixture = Fixture::new();
    let (base_url, _r, _requests) = mock_upstream(&[
        ("GET /v2/openapi/user/balance", &[ok_balance("50"), ok_balance("50")]),
        ("POST /v2/openapi/task", &[(200, "{\"code\":0,\"data\":{\"task_id\":\"up-s\"}}".into())]),
        ("GET /v2/openapi/task/*", &[(
            200,
            "{\"code\":0,\"data\":{\"task_id\":\"up-s\",\"type\":\"text_to_model\",\"status\":\"success\",\"progress\":100,\"consumed_credit\":10,\"output\":{}}}".into(),
        )]),
    ]);
    fixture.publish(&[("tripo3d-01", "tsk_key-1", &base_url)]);
    let (billing, input) = billing_with_funded_account(&fixture, 1_000_000_000_000).await;
    let gateway = gateway_with(&fixture, Some(billing));
    gateway.preflight().await;
    let response = gateway
        .handle_create(
            generation_body("text_to_model", json!({"prompt": "a cat"})),
            ExecutionAttempt::direct(),
            Some(input),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Shutdown waits for the detached drain to poll to final and settle, then the FIFO flush
    // runs inside the same deadline (SQLite keeps the calibration head pending — the writer is
    // PostgreSQL-only — but both the drain and the flush pass completed).
    gateway
        .shutdown_until(Some(tokio::time::Instant::now() + Duration::from_secs(20)))
        .await;
    assert_eq!(gateway.operational_status().inflight_requests, 0);
}
