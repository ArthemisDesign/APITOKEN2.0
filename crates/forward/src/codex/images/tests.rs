use super::*;
use crate::codex::{CodexConfig, CodexGateway};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant as StdInstant, SystemTime, UNIX_EPOCH};

const CREATED: u64 = 1_800_000_000;

fn stable_turn_id() -> ImageTurnId {
    ImageTurnId::new("stable-image-turn-123").unwrap()
}

fn png_fixture(width: u32, height: u32, color: png::ColorType) -> Vec<u8> {
    let channels = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => panic!("indexed fixture unsupported"),
    };
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&vec![0; width as usize * height as usize * channels])
        .unwrap();
    drop(writer);
    output
}

fn noisy_png(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = vec![0; width as usize * height as usize];
    let mut state = 0x1234_5678_u32;
    for pixel in &mut pixels {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *pixel = state as u8;
    }
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels).unwrap();
    drop(writer);
    output
}

fn valid_png() -> &'static [u8] {
    static PNG: OnceLock<Vec<u8>> = OnceLock::new();
    PNG.get_or_init(|| png_fixture(32, 24, png::ColorType::Rgba))
}

fn animated_png() -> Vec<u8> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, 8, 8);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(2, 0).unwrap();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[0; 64]).unwrap();
    writer.write_image_data(&[0; 64]).unwrap();
    drop(writer);
    output
}

fn success_body(extra: Value) -> Vec<u8> {
    let mut value = json!({
        "created": CREATED,
        "background": "auto",
        "data": [{"b64_json": STANDARD_BASE64.encode(valid_png())}],
        "quality": "auto",
        "size": "auto"
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::to_vec(&value).unwrap()
}

fn http_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("Connection: close\r\n\r\n");
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .ok();
    let deadline = StdInstant::now() + Duration::from_secs(3);
    let mut request = Vec::new();
    let mut expected = None;
    while StdInstant::now() < deadline {
        let mut buffer = [0; 8192];
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => request.extend_from_slice(&buffer[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
        if expected.is_none() {
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let length = std::str::from_utf8(&request[..end])
                    .ok()
                    .and_then(|headers| {
                        headers.lines().find_map(|line| {
                            let (name, value) = line.split_once(':')?;
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

fn request_parts(request: &[u8]) -> (&str, &[u8]) {
    let end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    (
        std::str::from_utf8(&request[..end]).unwrap(),
        &request[end + 4..],
    )
}

struct TestGateway {
    gateway: Arc<CodexGateway>,
    root: std::path::PathBuf,
}

impl Drop for TestGateway {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn test_gateway(base_url: &str, home_count: usize, token_uri: Option<&str>) -> TestGateway {
    test_gateway_with_timeout(base_url, home_count, token_uri, 2_000)
}

fn test_gateway_with_timeout(
    base_url: &str,
    home_count: usize,
    token_uri: Option<&str>,
    turn_timeout_ms: u64,
) -> TestGateway {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "forward-codex-images-{}-{unique}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let credentials = root.join("credentials");
    std::fs::create_dir_all(&credentials).unwrap();
    let keyring =
        codex_credential::CredentialKeyring::parse(&format!("current:{}", "ab".repeat(32)))
            .unwrap();
    let mut profiles = Vec::new();
    for index in 0..home_count {
        let id = format!("home-{index}");
        let credential = codex_credential::CodexCredential {
            version: 1,
            access_token: format!("access-secret-{index}"),
            refresh_token: format!("refresh-secret-{index}"),
            expires_at: i64::MAX / 2,
            oauth_client_id: codex_credential::CODEX_OFFICIAL_OAUTH_CLIENT_ID.to_string(),
            token_uri: token_uri
                .unwrap_or(codex_credential::CODEX_OFFICIAL_TOKEN_URI)
                .to_string(),
            account_id: format!("account-secret-{index}"),
            email: format!("owner{index}@example.test"),
            plan: "chatgpt_pro".to_string(),
            proxy: String::new(),
            proxy_order_id: 0,
            issued_at: 1,
        };
        let envelope = keyring.seal("current", &id, &credential).unwrap();
        let path = credentials.join(format!("{id}.json"));
        std::fs::write(&path, codex_credential::encode_envelope(&envelope).unwrap()).unwrap();
        profiles.push(json!({"id": id, "credential_file": path.to_str().unwrap()}));
    }
    let roster = root.join("profiles.json");
    std::fs::write(
        &roster,
        serde_json::to_vec(&json!({"profiles": profiles})).unwrap(),
    )
    .unwrap();
    let config = CodexConfig {
        enabled: true,
        base_url: format!("{base_url}/codex"),
        profiles_file: roster.to_string_lossy().into_owned(),
        credential_keys: keyring,
        cli_version: codex_credential::CODEX_CLI_VERSION.to_string(),
        request_timeout_ms: 2_000,
        turn_timeout_ms,
        turn_silence_timeout_ms: 30_000,
        health_probe_interval_secs: 300,
        reserve_5h: 0.0,
        reserve_7d: 0.0,
        reserve_jitter: 0.0,
        reserve_overhead_tokens: 0,
        history_ttl_secs: 60,
        history_local_cap: 8,
        history_redis_url: None,
        history_secret: None,
        history_redis_timeout_ms: 10,
        default_proxy_env: BTreeMap::new(),
        models: Vec::new(),
    };
    TestGateway {
        gateway: Arc::new(CodexGateway::new(config).unwrap()),
        root,
    }
}

#[test]
fn constructors_enforce_prompt_png_and_aggregate_bounds_with_redaction() {
    assert!(ImageGenerationRequest::new("").is_err());
    assert!(ImageGenerationRequest::new("x".repeat(MAX_PROMPT_CHARS + 1)).is_err());
    let prompt = "private prompt material";
    let request = ImageGenerationRequest::new(prompt).unwrap();
    assert_eq!(request.model(), GptImage2);
    assert_eq!(request.background(), ImageBackground::Auto);
    assert_eq!(request.quality(), ImageQuality::Auto);
    assert_eq!(request.size(), ImageSize::Auto);
    assert!(!format!("{request:?}").contains(prompt));
    assert!(ImageSize::exact(1024, 1024).is_ok());
    assert!(ImageSize::exact(3840, 2160).is_ok());
    for invalid in [
        (0, 1024),
        (1000, 1024),
        (3840, 1264),
        (3840, 2176),
        (512, 512),
    ] {
        assert!(ImageSize::exact(invalid.0, invalid.1).is_err());
    }
    assert!(ImageTurnId::new("").is_err());
    assert!(ImageTurnId::new("bad turn id").is_err());
    assert!(ImageTurnId::new("x".repeat(129)).is_err());
    assert_eq!(stable_turn_id().as_str(), "stable-image-turn-123");

    assert!(ImageReference::new(Vec::new()).is_err());
    assert!(ImageReference::new(b"not png".to_vec()).is_err());
    let mut corrupt = valid_png().to_vec();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(ImageReference::new(corrupt).is_err());
    let mut truncated = valid_png().to_vec();
    truncated.truncate(truncated.len() - 5);
    assert!(ImageReference::new(truncated).is_err());
    assert!(ImageReference::new(animated_png()).is_err());
    let mut trailing = valid_png().to_vec();
    trailing.extend_from_slice(b"trailing");
    assert!(ImageReference::new(trailing).is_err());
    assert!(ImageReference::new(vec![0; MAX_IMAGE_STORAGE_BYTES + 1]).is_err());
    assert!(ImageReference::new(png_fixture(4097, 1, png::ColorType::Grayscale)).is_err());
    assert!(ImageReference::new(png_fixture(4096, 4096, png::ColorType::Rgba)).is_err());

    let image = ImageReference::new(valid_png().to_vec()).unwrap();
    assert!(!format!("{image:?}").contains("IHDR"));
    assert!(ImageEditRequest::new("edit", vec![]).is_err());
    assert!(ImageEditRequest::new("edit", vec![image.clone(); 5]).is_ok());
    assert!(ImageEditRequest::new("edit", vec![image.clone(); 6]).is_err());

    let storage_large = ImageReference::new(noisy_png(4096, 3072)).unwrap();
    assert!(storage_large.bytes.len() < MAX_IMAGE_STORAGE_BYTES);
    assert!(
        ImageEditRequest::new("edit", vec![storage_large.clone(), storage_large.clone()]).is_ok()
    );
    let mut storage_over = vec![storage_large.clone(), storage_large.clone()];
    storage_over[0].bytes.resize(MAX_IMAGE_STORAGE_BYTES, 0);
    storage_over[1].bytes.resize(MAX_IMAGE_STORAGE_BYTES, 0);
    storage_over.push(image.clone());
    assert!(ImageEditRequest::new("edit", storage_over).is_err());
    let decoded_large =
        ImageReference::new(png_fixture(4096, 4096, png::ColorType::Grayscale)).unwrap();
    assert!(ImageEditRequest::new("edit", vec![decoded_large.clone(); 4]).is_ok());
    assert!(ImageEditRequest::new("edit", vec![decoded_large; 5]).is_err());
}

#[test]
fn canonical_base64_and_response_schema_are_strict_but_additive() {
    assert_eq!(canonical_base64_decoded_len(b"AAA="), Some(2));
    assert_eq!(canonical_base64_decoded_len(b"AAAA"), Some(3));
    for invalid in [b"AB==".as_slice(), b"AAA", b"AA-_", b"AA\n=", b""] {
        assert_eq!(canonical_base64_decoded_len(invalid), None);
    }
    let context = ImageErrorContext {
        status: 200,
        request_id: Some("req_schema".to_string()),
    };
    let image_turn_id = stable_turn_id();
    let parsed = parse_success(
        &success_body(json!({
            "output_format": "png",
            "usage": {"private_token_detail": 7},
            "future_field": true
        })),
        context.clone(),
        "home-opaque".to_string(),
        image_turn_id.clone(),
    )
    .unwrap();
    assert_eq!(parsed.png(), valid_png());
    assert_eq!(parsed.image_turn_id(), &image_turn_id);
    assert_eq!((parsed.width(), parsed.height()), (32, 24));
    assert_eq!(parsed.output_format(), Some("png"));
    assert!(parsed.usage().is_some());
    let rendered = format!("{parsed:?}");
    assert!(rendered.contains("REDACTED"));
    assert!(!rendered.contains("private_token_detail"));

    for mutation in [
        json!({"background": null}),
        json!({"background": ""}),
        json!({"quality": null}),
        json!({"quality": "bad\nmetadata"}),
        json!({"size": null}),
        json!({"size": "x".repeat(129)}),
        json!({"output_format": null}),
        json!({"output_format": "jpeg"}),
        json!({"usage": null}),
        json!({"created": MIN_CREATED_UNIX_SECONDS - 1}),
        json!({"data": []}),
    ] {
        assert!(matches!(
            parse_success(
                &success_body(mutation),
                context.clone(),
                "home".to_string(),
                stable_turn_id(),
            ),
            Err(CodexImageError::InvalidResponse(_))
        ));
    }
    for missing in ["created", "background", "data", "quality", "size"] {
        let mut body: Value = serde_json::from_slice(&success_body(json!({}))).unwrap();
        body.as_object_mut().unwrap().remove(missing);
        assert!(parse_success(
            &serde_json::to_vec(&body).unwrap(),
            context.clone(),
            "home".to_string(),
            stable_turn_id(),
        )
        .is_err());
    }
    for data in [
        json!([{}]),
        json!([
            {"b64_json": STANDARD_BASE64.encode(valid_png())},
            {"b64_json": STANDARD_BASE64.encode(valid_png())}
        ]),
    ] {
        assert!(parse_success(
            &success_body(json!({"data": data})),
            context.clone(),
            "home".to_string(),
            stable_turn_id(),
        )
        .is_err());
    }

    for b64 in [
        "AB==".to_string(),
        STANDARD_BASE64.encode(b"not png"),
        STANDARD_BASE64.encode(animated_png()),
        STANDARD_BASE64.encode(png_fixture(4097, 1, png::ColorType::Grayscale)),
        STANDARD_BASE64.encode(png_fixture(4096, 4096, png::ColorType::Rgba)),
    ] {
        assert!(parse_success(
            &success_body(json!({"data": [{"b64_json": b64}]})),
            context.clone(),
            "home".to_string(),
            stable_turn_id(),
        )
        .is_err());
    }
}

#[tokio::test]
async fn generation_and_edit_wire_are_exact_and_authenticated() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_server = requests.clone();
    let response = http_response(
        "200 OK",
        &[("x-request-id", "req_wire")],
        &success_body(json!({"output_format": "png"})),
    );
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            requests_server
                .lock()
                .unwrap()
                .push(read_request(&mut stream));
            stream.write_all(&response).unwrap();
        }
    });
    let test = test_gateway(&base_url, 1, None);
    assert_eq!(
        test.gateway.select_image_canary_home().await.unwrap(),
        "home-0"
    );
    let generation = ImageGenerationRequest::new("draw private subject")
        .unwrap()
        .with_controls(
            ImageBackground::Opaque,
            ImageQuality::Low,
            ImageSize::exact(1024, 1024).unwrap(),
        );
    let result = test.gateway.generate_image(&generation).await.unwrap();
    assert_eq!(result.home_id(), "home-0");
    assert_eq!(result.request_id(), Some("req_wire"));
    let edit = ImageEditRequest::new(
        "edit private subject",
        vec![
            ImageReference::new(valid_png().to_vec()).unwrap(),
            ImageReference::new(png_fixture(7, 9, png::ColorType::Grayscale)).unwrap(),
        ],
    )
    .unwrap()
    .with_controls(
        ImageBackground::Opaque,
        ImageQuality::Low,
        ImageSize::exact(1024, 1024).unwrap(),
    );
    let exact_turn_id = stable_turn_id();
    let edit_result = test
        .gateway
        .edit_image_on_home("home-0", &exact_turn_id, &edit)
        .await
        .unwrap();
    assert_eq!(edit_result.image_turn_id(), &exact_turn_id);
    server.join().unwrap();

    let requests = requests.lock().unwrap();
    let mut turn_ids = Vec::new();
    for (index, raw) in requests.iter().enumerate() {
        let (headers, body) = request_parts(raw);
        let lower = headers.to_ascii_lowercase();
        let expected_path = if index == 0 {
            "/codex/images/generations"
        } else {
            "/codex/images/edits"
        };
        assert!(headers.starts_with(&format!("POST {expected_path} HTTP/1.1")));
        for expected in [
            "authorization: bearer access-secret-0",
            "chatgpt-account-id: account-secret-0",
            "originator: codex_cli_rs",
            "accept: application/json",
            "content-type: application/json",
            "version: ",
            "user-agent: codex_cli_rs/",
        ] {
            assert!(lower.contains(expected), "missing {expected} in {lower}");
        }
        if index == 0 {
            assert!(lower.contains("x-codex-image-turn-id: image_turn_"));
        } else {
            assert!(lower.contains("x-codex-image-turn-id: stable-image-turn-123"));
        }
        turn_ids.push(
            lower
                .lines()
                .find_map(|line| line.strip_prefix("x-codex-image-turn-id: "))
                .unwrap()
                .to_string(),
        );
        let wire: Value = serde_json::from_slice(body).unwrap();
        if index == 0 {
            assert_eq!(
                wire,
                json!({
                    "prompt": "draw private subject",
                    "background": "opaque",
                    "model": GPT_IMAGE_2,
                    "quality": "low",
                    "size": "1024x1024"
                })
            );
        } else {
            assert_eq!(wire["prompt"], "edit private subject");
            assert_eq!(wire["background"], "opaque");
            assert_eq!(wire["model"], GPT_IMAGE_2);
            assert_eq!(wire["quality"], "low");
            assert_eq!(wire["size"], "1024x1024");
            let images = wire["images"].as_array().unwrap();
            assert_eq!(images.len(), 2);
            for image in images {
                assert!(image["image_url"]
                    .as_str()
                    .unwrap()
                    .starts_with("data:image/png;base64,"));
            }
        }
    }
    assert!(turn_ids[0].starts_with("image_turn_"));
    assert_eq!(turn_ids[1], exact_turn_id.as_str());
}

#[tokio::test]
async fn received_401_refreshes_once_and_replays_same_home() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let token_uri = format!("{base_url}/oauth/token");
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_server = requests.clone();
    let success = http_response("200 OK", &[], &success_body(json!({})));
    let server = thread::spawn(move || {
        for index in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            requests_server.lock().unwrap().push(request);
            let response = match index {
                0 => http_response(
                    "401 Unauthorized",
                    &[("x-request-id", "req_401")],
                    b"private raw body",
                ),
                1 => http_response(
                    "200 OK",
                    &[],
                    &serde_json::to_vec(&json!({
                        "access_token": "refreshed-access-secret",
                        "refresh_token": "refreshed-refresh-secret",
                        "expires_in": 3600
                    }))
                    .unwrap(),
                ),
                _ => success.clone(),
            };
            stream.write_all(&response).unwrap();
        }
    });
    let test = test_gateway(&base_url, 1, Some(&token_uri));
    let result = test
        .gateway
        .generate_image(&ImageGenerationRequest::new("secret prompt").unwrap())
        .await
        .unwrap();
    assert_eq!(result.home_id(), "home-0");
    server.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let (first_headers, first_body) = request_parts(&requests[0]);
    let (refresh_headers, _) = request_parts(&requests[1]);
    let (second_headers, second_body) = request_parts(&requests[2]);
    assert!(first_headers.starts_with("POST /codex/images/generations"));
    assert!(refresh_headers.starts_with("POST /oauth/token"));
    assert!(second_headers.starts_with("POST /codex/images/generations"));
    assert!(first_headers
        .to_ascii_lowercase()
        .contains("authorization: bearer access-secret-0"));
    assert!(second_headers
        .to_ascii_lowercase()
        .contains("authorization: bearer refreshed-access-secret"));
    assert_eq!(first_body, second_body);
    let first_turn = first_headers
        .to_ascii_lowercase()
        .lines()
        .find_map(|line| line.strip_prefix("x-codex-image-turn-id: "))
        .unwrap()
        .to_string();
    let second_turn = second_headers
        .to_ascii_lowercase()
        .lines()
        .find_map(|line| line.strip_prefix("x-codex-image-turn-id: "))
        .unwrap()
        .to_string();
    assert_eq!(first_turn, second_turn);
}

#[tokio::test]
async fn first_401_body_drain_failure_prevents_refresh_and_replay() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let token_uri = format!("{base_url}/oauth/token");
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_server = accepted.clone();
    let server = thread::spawn(move || {
        let deadline = StdInstant::now() + Duration::from_millis(500);
        while StdInstant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted_server.fetch_add(1, Ordering::SeqCst);
                    let _ = read_request(&mut stream);
                    stream
                        .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 10\r\nx-request-id: req_401_drain\r\nConnection: close\r\n\r\nshort")
                        .unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    let test = test_gateway(&base_url, 1, Some(&token_uri));
    let error = test
        .gateway
        .generate_image(&ImageGenerationRequest::new("no replay").unwrap())
        .await
        .unwrap_err();
    server.join().unwrap();
    assert!(matches!(error, CodexImageError::OutcomeUnknown(Some(_))));
    assert_eq!(error.request_id(), Some("req_401_drain"));
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn final_401_stops_after_one_refresh_and_redacts_every_secret() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let token_uri = format!("{base_url}/oauth/token");
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_server = accepted.clone();
    let server = thread::spawn(move || {
        for index in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            accepted_server.fetch_add(1, Ordering::SeqCst);
            let _ = read_request(&mut stream);
            let response = if index == 1 {
                http_response(
                    "200 OK",
                    &[],
                    &serde_json::to_vec(&json!({
                        "access_token": "refreshed-access-secret",
                        "refresh_token": "refreshed-refresh-secret",
                        "expires_in": 3600
                    }))
                    .unwrap(),
                )
            } else {
                http_response(
                    "401 Unauthorized",
                    &[((
                        "x-request-id",
                        if index == 2 {
                            "refreshed-refresh-secret"
                        } else {
                            "req secret id"
                        },
                    ))],
                    b"raw-provider-secret",
                )
            };
            stream.write_all(&response).unwrap();
        }
    });
    let test = test_gateway(&base_url, 1, Some(&token_uri));
    let prompt = "prompt-secret";
    let error = test
        .gateway
        .generate_image(&ImageGenerationRequest::new(prompt).unwrap())
        .await
        .unwrap_err();
    server.join().unwrap();
    assert_eq!(accepted.load(Ordering::SeqCst), 3);
    assert!(matches!(error, CodexImageError::AuthenticationRequired(_)));
    assert_eq!(error.request_id(), None);
    let rendered = format!("{error:?} {error}");
    for secret in [
        prompt,
        "access-secret-0",
        "refresh-secret-0",
        "refreshed-access-secret",
        "account-secret-0",
        "raw-provider-secret",
    ] {
        assert!(!rendered.contains(secret), "leaked {secret}");
    }
}

#[tokio::test]
async fn automatic_generation_rotates_after_final_auth_rejection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let token_uri = format!("{base_url}/oauth/token");
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_server = requests.clone();
    let success = http_response("200 OK", &[], &success_body(json!({})));
    let server = thread::spawn(move || {
        for index in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            requests_server
                .lock()
                .unwrap()
                .push(read_request(&mut stream));
            let response = match index {
                0 => http_response("401 Unauthorized", &[], b"rejected"),
                1 => http_response(
                    "200 OK",
                    &[],
                    &serde_json::to_vec(&json!({
                        "access_token": "refreshed-access-secret",
                        "refresh_token": "refreshed-refresh-secret",
                        "expires_in": 3600
                    }))
                    .unwrap(),
                ),
                2 => http_response("401 Unauthorized", &[], b"rejected"),
                _ => success.clone(),
            };
            stream.write_all(&response).unwrap();
        }
    });
    let test = test_gateway(&base_url, 2, Some(&token_uri));
    let result = test
        .gateway
        .generate_image(&ImageGenerationRequest::new("rotate auth").unwrap())
        .await
        .unwrap();
    assert_eq!(result.home_id(), "home-1");
    server.join().unwrap();

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    let (first_headers, first_body) = request_parts(&requests[0]);
    let (refresh_headers, _) = request_parts(&requests[1]);
    let (retry_headers, retry_body) = request_parts(&requests[2]);
    let (second_home_headers, second_home_body) = request_parts(&requests[3]);
    assert!(first_headers
        .to_ascii_lowercase()
        .contains("authorization: bearer access-secret-0"));
    assert!(refresh_headers.starts_with("POST /oauth/token"));
    assert!(retry_headers
        .to_ascii_lowercase()
        .contains("authorization: bearer refreshed-access-secret"));
    assert!(second_home_headers
        .to_ascii_lowercase()
        .contains("authorization: bearer access-secret-1"));
    assert_eq!(first_body, retry_body);
    assert_eq!(first_body, second_home_body);
    let first_turn = first_headers
        .to_ascii_lowercase()
        .lines()
        .find_map(|line| line.strip_prefix("x-codex-image-turn-id: "))
        .unwrap()
        .to_string();
    let retry_turn = retry_headers
        .to_ascii_lowercase()
        .lines()
        .find_map(|line| line.strip_prefix("x-codex-image-turn-id: "))
        .unwrap()
        .to_string();
    let second_home_turn = second_home_headers
        .to_ascii_lowercase()
        .lines()
        .find_map(|line| line.strip_prefix("x-codex-image-turn-id: "))
        .unwrap()
        .to_string();
    assert_eq!(first_turn, retry_turn);
    assert_eq!(first_turn, second_home_turn);
}

#[tokio::test]
async fn automatic_edit_rotates_after_usage_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_server = requests.clone();
    let success = http_response("200 OK", &[], &success_body(json!({})));
    let server = thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            requests_server
                .lock()
                .unwrap()
                .push(read_request(&mut stream));
            let response = if index == 0 {
                http_response("429 Too Many Requests", &[], b"limited")
            } else {
                success.clone()
            };
            stream.write_all(&response).unwrap();
        }
    });
    let test = test_gateway(&base_url, 2, None);
    let request = ImageEditRequest::new(
        "rotate limit",
        vec![ImageReference::new(valid_png().to_vec()).unwrap()],
    )
    .unwrap();
    let result = test.gateway.edit_image(&request).await.unwrap();
    assert_eq!(result.home_id(), "home-1");
    server.join().unwrap();

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let (first_headers, first_body) = request_parts(&requests[0]);
    let (second_headers, second_body) = request_parts(&requests[1]);
    assert!(first_headers.starts_with("POST /codex/images/edits"));
    assert!(second_headers.starts_with("POST /codex/images/edits"));
    assert!(first_headers
        .to_ascii_lowercase()
        .contains("authorization: bearer access-secret-0"));
    assert!(second_headers
        .to_ascii_lowercase()
        .contains("authorization: bearer access-secret-1"));
    assert_eq!(first_body, second_body);
    let first_turn = first_headers
        .to_ascii_lowercase()
        .lines()
        .find_map(|line| {
            line.strip_prefix("x-codex-image-turn-id: ")
                .map(str::to_string)
        })
        .unwrap();
    let second_turn = second_headers
        .to_ascii_lowercase()
        .lines()
        .find_map(|line| {
            line.strip_prefix("x-codex-image-turn-id: ")
                .map(str::to_string)
        })
        .unwrap();
    assert_eq!(first_turn, second_turn);
}

#[tokio::test]
async fn ambiguous_abort_is_not_replayed_or_fanned_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_server = accepted.clone();
    let server = thread::spawn(move || {
        let deadline = StdInstant::now() + Duration::from_millis(600);
        while StdInstant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted_server.fetch_add(1, Ordering::SeqCst);
                    let _ = read_request(&mut stream);
                    drop(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    let test = test_gateway(&base_url, 2, None);
    let error = test
        .gateway
        .generate_image(&ImageGenerationRequest::new("secret").unwrap())
        .await
        .unwrap_err();
    server.join().unwrap();
    assert!(matches!(error, CodexImageError::OutcomeUnknown(_)));
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn response_content_length_and_stream_caps_are_invalid_response() {
    for streamed in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            if streamed {
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nx-request-id: req_stream_cap\r\nConnection: close\r\n\r\n1000001\r\n")
                    .unwrap();
                let chunk = vec![b'x'; 64 * 1024];
                for _ in 0..=(MAX_RESPONSE_BYTES / chunk.len()) {
                    if stream.write_all(&chunk).is_err() {
                        break;
                    }
                }
                let _ = stream.write_all(b"\r\n0\r\n\r\n");
            } else {
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nx-request-id: req_length_cap\r\nConnection: close\r\n\r\n",
                            MAX_RESPONSE_BYTES + 1
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
        });
        let test = test_gateway(&base_url, 1, None);
        let error = test
            .gateway
            .generate_image(&ImageGenerationRequest::new("bounded").unwrap())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            CodexImageError::InvalidResponse(_) | CodexImageError::ResponseBodyClosed(_)
        ));
        assert_eq!(
            error.request_id(),
            Some(if streamed {
                "req_stream_cap"
            } else {
                "req_length_cap"
            })
        );
        server.join().unwrap();
    }
}

#[tokio::test]
async fn response_body_timeout_has_distinct_class() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nx-request-id: req_body_timeout\r\nConnection: close\r\n\r\n")
            .unwrap();
        thread::sleep(Duration::from_millis(300));
    });
    let test = test_gateway_with_timeout(&base_url, 1, None, 100);
    let error = test
        .gateway
        .generate_image(&ImageGenerationRequest::new("timeout").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(error, CodexImageError::ResponseTimeout(Some(_))));
    assert_eq!(error.request_id(), Some("req_body_timeout"));
    server.join().unwrap();
}

#[tokio::test]
async fn parallel_requests_have_no_image_semaphore() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_server = accepted.clone();
    let (both_sender, both_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let response = http_response("200 OK", &[], &success_body(json!({})));
    let server = thread::spawn(move || {
        let mut streams = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            accepted_server.fetch_add(1, Ordering::SeqCst);
            streams.push(stream);
        }
        both_sender.send(()).unwrap();
        release_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        for mut stream in streams {
            stream.write_all(&response).unwrap();
        }
    });
    let test = test_gateway(&base_url, 1, None);
    let request = Arc::new(ImageGenerationRequest::new("parallel").unwrap());
    let first = test.gateway.generate_image(&request);
    let second = test.gateway.generate_image(&request);
    let release = async {
        tokio::task::spawn_blocking(move || {
            both_receiver.recv_timeout(Duration::from_secs(1)).unwrap();
            release_sender.send(()).unwrap();
        })
        .await
        .unwrap();
    };
    let (first, second, ()) = tokio::join!(first, second, release);
    assert_eq!(accepted.load(Ordering::SeqCst), 2);
    first.unwrap();
    second.unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn exact_target_rejects_unknown_and_never_spills() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_server = accepted.clone();
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests_server = requests.clone();
    let server = thread::spawn(move || {
        let deadline = StdInstant::now() + Duration::from_millis(600);
        while StdInstant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted_server.fetch_add(1, Ordering::SeqCst);
                    requests_server
                        .lock()
                        .unwrap()
                        .push(read_request(&mut stream));
                    stream
                        .write_all(&http_response("429 Too Many Requests", &[], b"limited"))
                        .unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    let test = test_gateway(&base_url, 2, None);
    let request = ImageGenerationRequest::new("target").unwrap();
    let image_turn_id = stable_turn_id();
    assert!(matches!(
        test.gateway
            .generate_image_on_home("unknown", &image_turn_id, &request)
            .await,
        Err(CodexImageError::Unavailable)
    ));
    let error = test
        .gateway
        .generate_image_on_home("home-1", &image_turn_id, &request)
        .await
        .unwrap_err();
    assert!(matches!(error, CodexImageError::UsageLimit(_)));
    server.join().unwrap();
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    let requests = requests.lock().unwrap();
    let (headers, _) = request_parts(&requests[0]);
    let lower = headers.to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer access-secret-1"));
    assert!(lower.contains("chatgpt-account-id: account-secret-1"));
    assert!(!lower.contains("access-secret-0"));
}

#[tokio::test]
async fn exact_image_preflight_probes_only_requested_home() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_server = accepted.clone();
    let request = Arc::new(std::sync::Mutex::new(Vec::new()));
    let request_server = request.clone();
    let server = thread::spawn(move || {
        let deadline = StdInstant::now() + Duration::from_millis(600);
        while StdInstant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted_server.fetch_add(1, Ordering::SeqCst);
                    *request_server.lock().unwrap() = read_request(&mut stream);
                    let body = serde_json::to_vec(&json!({
                        "rate_limit": {
                            "allowed": true,
                            "limit_reached": false,
                            "primary_window": {
                                "used_percent": 1,
                                "limit_window_seconds": 604800,
                                "reset_at": 4_102_444_800i64
                            }
                        }
                    }))
                    .unwrap();
                    stream
                        .write_all(&http_response("200 OK", &[], &body))
                        .unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    let test = test_gateway(&base_url, 2, None);
    test.gateway.preflight_image_home("home-1").await.unwrap();
    server.join().unwrap();
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
    let request = request.lock().unwrap();
    let (headers, body) = request_parts(&request);
    let lower = headers.to_ascii_lowercase();
    assert!(headers.starts_with("GET /wham/usage HTTP/1.1"));
    assert!(lower.contains("authorization: bearer access-secret-1"));
    assert!(lower.contains("chatgpt-account-id: account-secret-1"));
    assert!(!lower.contains("access-secret-0"));
    assert!(body.is_empty());
}

#[tokio::test]
async fn retry_after_controls_usage_limit_cooling() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        stream
            .write_all(&http_response(
                "429 Too Many Requests",
                &[("Retry-After", "321")],
                b"limited",
            ))
            .unwrap();
    });
    let test = test_gateway(&base_url, 1, None);
    let before = pool::now();
    let error = test
        .gateway
        .generate_image(&ImageGenerationRequest::new("retry after").unwrap())
        .await
        .unwrap_err();
    assert!(matches!(error, CodexImageError::UsageLimit(_)));
    server.join().unwrap();
    let status = test.gateway.operational_status().await;
    assert!(!status.homes[0].admitted);
    assert!(status.homes[0].cooling_until >= before.saturating_add(321));
}

#[tokio::test]
async fn invalid_proxy_config_does_not_poison_health() {
    let test = test_gateway("http://127.0.0.1:9", 1, None);
    // Construction rejects invalid proxy configuration before a home can enter the pool, and the
    // request-local guard must likewise leave a loaded home's health untouched.
    let loaded = test.gateway.homes().await.pop().unwrap();
    note_pre_dispatch_error(&loaded, &ProcessError::InvalidConfig("proxy".to_string()));
    let status = test.gateway.operational_status().await;
    assert_eq!(status.homes[0].account_state, "healthy");
    assert_eq!(status.homes[0].transport_state, "responsive");
}

#[tokio::test]
async fn bad_request_does_not_stain_home_but_usage_limit_cools_it() {
    for (status, expected, admitted_after) in [
        ("400 Bad Request", "bad_request", true),
        ("429 Too Many Requests", "usage_limit", false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let response = http_response(status, &[], b"private provider body");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            stream.write_all(&response).unwrap();
        });
        let test = test_gateway(&base_url, 1, None);
        let error = test
            .gateway
            .generate_image(&ImageGenerationRequest::new("client input").unwrap())
            .await
            .unwrap_err();
        server.join().unwrap();
        match expected {
            "bad_request" => assert!(matches!(error, CodexImageError::BadRequest(_))),
            "usage_limit" => assert!(matches!(error, CodexImageError::UsageLimit(_))),
            _ => unreachable!(),
        }
        let status = test.gateway.operational_status().await;
        assert_eq!(status.homes[0].admitted, admitted_after);
        assert_eq!(status.homes[0].account_state, "healthy");
        assert_eq!(status.homes[0].transport_state, "responsive");
    }
}
