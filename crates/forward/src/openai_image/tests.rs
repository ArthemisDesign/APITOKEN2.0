use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant as StdInstant};

use serde_json::{json, Value};

use super::*;

const TEST_KEY: &str = "test-key-0123456789-abcdefghijklmnop";
const CREATED: u64 = 1_800_000_000;

fn png_fixture_with_color(width: u32, height: u32, color: png::ColorType) -> Vec<u8> {
    let channels = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => panic!("indexed fixture is not supported"),
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

fn png_fixture(width: u32, height: u32) -> Vec<u8> {
    png_fixture_with_color(width, height, png::ColorType::Grayscale)
}

fn alpha_png_fixture(width: u32, height: u32) -> Vec<u8> {
    png_fixture_with_color(width, height, png::ColorType::GrayscaleAlpha)
}

fn noisy_png_fixture_with_color(width: u32, height: u32, color: png::ColorType) -> Vec<u8> {
    let channels = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => panic!("indexed fixture is not supported"),
    };
    let mut pixels = vec![0; width as usize * height as usize * channels];
    let mut state = 0x1234_5678_u32;
    for pixel in &mut pixels {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *pixel = state as u8;
    }
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels).unwrap();
    drop(writer);
    output
}

fn noisy_png_fixture(width: u32, height: u32) -> Vec<u8> {
    noisy_png_fixture_with_color(width, height, png::ColorType::Grayscale)
}

fn valid_png() -> &'static [u8] {
    static PNG: OnceLock<Vec<u8>> = OnceLock::new();
    PNG.get_or_init(|| png_fixture(1024, 1024))
}

fn animated_png() -> Vec<u8> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, 32, 32);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_animated(2, 0).unwrap();
    let mut writer = encoder.write_header().unwrap();
    let frame = vec![0; 32 * 32];
    writer.write_image_data(&frame).unwrap();
    writer.write_image_data(&frame).unwrap();
    drop(writer);
    output
}

fn success_json_with_usage(usage: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "created": CREATED,
        "data": [{"b64_json": STANDARD_BASE64.encode(valid_png())}],
        "usage": usage,
    }))
    .unwrap()
}

fn valid_usage() -> Value {
    json!({
        "input_tokens": 30,
        "input_tokens_details": {"text_tokens": 10, "image_tokens": 20},
        "output_tokens": 40,
        "total_tokens": 70,
        "output_tokens_details": {"text_tokens": 0, "image_tokens": 40}
    })
}

fn http_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("Connection: close\r\n\r\n");
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    let deadline = StdInstant::now() + Duration::from_secs(3);
    let mut request = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut expected = None;
    while StdInstant::now() < deadline {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => return request,
        }
        if expected.is_none() {
            if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let content_length = std::str::from_utf8(&request[..header_end])
                    .ok()
                    .and_then(|headers| {
                        headers.lines().find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                    })
                    .unwrap_or(0);
                expected = header_end
                    .checked_add(4)
                    .and_then(|value| value.checked_add(content_length));
            }
        }
        if expected.is_some_and(|expected| request.len() >= expected) {
            break;
        }
    }
    request
}

fn spawn_server(response: Vec<u8>) -> (String, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        let _ = stream.write_all(&response);
        let _ = sender.send(request);
    });
    (format!("http://{address}"), receiver)
}

fn test_gateway_with_timeout(
    base_url: &str,
    max_response_body: usize,
    turn_timeout: Duration,
) -> OpenAiImageGateway {
    let config = ApiYiImageConfig::loopback(
        base_url,
        TEST_KEY,
        Duration::from_secs(2),
        turn_timeout,
        max_response_body,
    )
    .unwrap();
    OpenAiImageGateway::new(config).unwrap()
}

fn test_gateway(base_url: &str, max_response_body: usize) -> OpenAiImageGateway {
    test_gateway_with_timeout(base_url, max_response_body, Duration::from_secs(5))
}

fn request_parts(request: &[u8]) -> (&str, &[u8]) {
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    (
        std::str::from_utf8(&request[..header_end]).unwrap(),
        &request[header_end + 4..],
    )
}

fn request() -> GenerationRequest {
    GenerationRequest::new(GPT_IMAGE_2_ALIAS, "prompt").unwrap()
}

#[test]
fn config_is_redacted_pinned_and_strict() {
    let config = ApiYiImageConfig::production(
        TEST_KEY,
        Duration::from_secs(30),
        Duration::from_secs(600),
        DEFAULT_MAX_RESPONSE_BODY,
    )
    .unwrap();
    let rendered = format!("{config:?}");
    assert!(rendered.contains(PRODUCTION_BASE_URL));
    assert!(rendered.contains("REDACTED"));
    assert!(!rendered.contains(TEST_KEY));
    assert_eq!(config.max_response_body(), 16 * 1024 * 1024);

    assert!(ApiYiImageConfig::production(
        TEST_KEY,
        Duration::from_secs(1),
        Duration::from_secs(1),
        MAX_RESPONSE_BODY + 1,
    )
    .is_err());
    for origin in [
        "http://localhost:8000",
        "https://127.0.0.1:8000",
        "http://127.0.0.2:8000",
        "http://127.0.0.1:8000/path",
    ] {
        assert!(ApiYiImageConfig::loopback(
            origin,
            TEST_KEY,
            Duration::from_secs(1),
            Duration::from_secs(1),
            1024,
        )
        .is_err());
    }
}

#[test]
fn request_and_png_constructors_are_strict_and_redacted() {
    assert!(GenerationRequest::new("other", "prompt").is_err());
    assert!(GenerationRequest::new(GPT_IMAGE_2_ALIAS, "").is_err());
    let secret_prompt = "draw SECRET-PROMPT";
    let generation = GenerationRequest::new(GPT_IMAGE_2_ALIAS, secret_prompt).unwrap();
    assert!(!format!("{generation:?}").contains(secret_prompt));

    assert!(ReferenceImage::new(Vec::new()).is_err());
    assert!(ReferenceImage::new(b"not-png".to_vec()).is_err());
    let mut corrupt = png_fixture(12, 34);
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(ReferenceImage::new(corrupt).is_err());
    let mut truncated = png_fixture(12, 34);
    truncated.truncate(truncated.len() - 5);
    assert!(ReferenceImage::new(truncated).is_err());
    assert!(ReferenceImage::new(animated_png()).is_err());

    let reference = ReferenceImage::new(png_fixture(12, 34)).unwrap();
    assert!(!format!("{reference:?}").contains("IHDR"));
    assert!(ImageMask::new(png_fixture(12, 34)).is_err());
    let mut corrupt_mask = alpha_png_fixture(12, 34);
    *corrupt_mask.last_mut().unwrap() ^= 1;
    assert!(ImageMask::new(corrupt_mask).is_err());
    let mut truncated_mask = alpha_png_fixture(12, 34);
    truncated_mask.truncate(truncated_mask.len() - 5);
    assert!(ImageMask::new(truncated_mask).is_err());
    assert!(ImageMask::new(alpha_png_fixture(13, 34)).is_ok());
    let oversized_mask = noisy_png_fixture_with_color(1_500, 1_500, png::ColorType::GrayscaleAlpha);
    assert!(oversized_mask.len() >= MAX_MASK_BYTES_EXCLUSIVE);
    assert!(ImageMask::new(oversized_mask).is_err());
    let mask = ImageMask::new(alpha_png_fixture(11, 34)).unwrap();
    assert!(EditRequest::new(
        GPT_IMAGE_2_ALIAS,
        "edit",
        vec![reference.clone()],
        Some(mask)
    )
    .is_err());
    assert!(EditRequest::new(GPT_IMAGE_2_ALIAS, "edit", vec![], None).is_err());

    let differently_sized = ReferenceImage::new(png_fixture(99, 7)).unwrap();
    assert!(EditRequest::new(
        GPT_IMAGE_2_ALIAS,
        "edit",
        vec![reference.clone(), differently_sized],
        None,
    )
    .is_ok());
    let matching_mask = ImageMask::new(alpha_png_fixture(12, 34)).unwrap();
    assert!(EditRequest::new(
        GPT_IMAGE_2_ALIAS,
        "edit",
        vec![reference],
        Some(matching_mask),
    )
    .is_ok());

    let large = ReferenceImage::new(noisy_png_fixture(4096, 3072)).unwrap();
    assert!(EditRequest::new(GPT_IMAGE_2_ALIAS, "edit", vec![large.clone()], None,).is_ok());
    assert!(EditRequest::new(GPT_IMAGE_2_ALIAS, "edit", vec![large.clone(), large], None).is_err());
}

#[test]
fn decoded_image_limit_is_inclusive_and_precomputed() {
    let canonical = vec![b'A'; (MAX_DECODED_IMAGE_BYTES / 3) * 4];
    assert_eq!(
        canonical_base64_decoded_len(&canonical),
        Some(MAX_DECODED_IMAGE_BYTES)
    );
    let mut over = canonical;
    over.extend_from_slice(b"AAAA");
    assert_eq!(
        canonical_base64_decoded_len(&over),
        Some(MAX_DECODED_IMAGE_BYTES + 3)
    );
}

#[test]
fn canonical_standard_base64_is_checked_without_decoding() {
    assert_eq!(canonical_base64_decoded_len(b"AAA="), Some(2));
    assert_eq!(canonical_base64_decoded_len(b"AAAA"), Some(3));
    for malformed in [
        b"AB==".as_slice(),
        b"AA=A",
        b"AAA",
        b"AAA==",
        b"AA-_",
        b"AA\n=",
        b"",
    ] {
        assert_eq!(
            canonical_base64_decoded_len(malformed),
            None,
            "{malformed:?}"
        );
    }
}

#[tokio::test]
async fn generation_wire_is_exact_and_success_is_strictly_accounted() {
    let body = success_json_with_usage(valid_usage());
    let response = http_response("200 OK", &[("x-request-id", "req_abc-123")], &body);
    let (base_url, received) = spawn_server(response);
    let result = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
        .generate(&GenerationRequest::new(GPT_IMAGE_2_ALIAS, "draw a black square").unwrap())
        .await
        .unwrap();

    let raw = received.recv_timeout(Duration::from_secs(2)).unwrap();
    let (headers, request_body) = request_parts(&raw);
    let lower = headers.to_ascii_lowercase();
    assert!(headers.starts_with("POST /v1/images/generations HTTP/1.1"));
    assert!(lower.contains(&format!("authorization: bearer {TEST_KEY}")));
    assert!(lower.contains("content-type: application/json"));
    let wire: Value = serde_json::from_slice(request_body).unwrap();
    assert_eq!(
        wire,
        json!({
            "model": GPT_IMAGE_2_ALIAS,
            "prompt": "draw a black square",
            "quality": "low",
            "size": "1024x1024",
            "n": 1,
            "output_format": "png",
            "background": "opaque",
            "moderation": "auto",
            "stream": false
        })
    );

    assert_eq!(result.image(), valid_png());
    assert_eq!(result.request_id(), Some("req_abc-123"));
    assert_eq!(result.requested_model_id(), GPT_IMAGE_2_ALIAS);
    assert_eq!(result.canonical_model_id(), GPT_IMAGE_2_SNAPSHOT);
    assert_eq!(result.usage().input_tokens(), 30);
    assert_eq!(result.usage().output_details().image_tokens(), 40);
    assert_eq!(result.usage().output_details().text_tokens(), 0);
    assert_eq!(result.cost_nanodollars(), 1_410_000);
    let rendered = format!("{result:?}");
    assert!(rendered.contains("REDACTED"));
    assert!(!rendered.contains("IHDR"));
}

#[tokio::test]
async fn required_created_and_output_details_reject_with_success_context() {
    let cases = [
        json!({
            "data": [{"b64_json": STANDARD_BASE64.encode(valid_png())}],
            "usage": valid_usage()
        }),
        json!({
            "created": CREATED,
            "data": [{"b64_json": STANDARD_BASE64.encode(valid_png())}],
            "usage": {
                "input_tokens": 30,
                "input_tokens_details": {"text_tokens": 10, "image_tokens": 20},
                "output_tokens": 40,
                "total_tokens": 70
            }
        }),
    ];
    for body in cases {
        let response = http_response(
            "200 OK",
            &[("x-request-id", "req_required")],
            &serde_json::to_vec(&body).unwrap(),
        );
        let (base_url, _) = spawn_server(response);
        let error = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
            .generate(&request())
            .await
            .unwrap_err();
        assert!(matches!(error, ImageTransportError::InvalidResponse(_)));
        assert_eq!(error.status(), Some(200));
        assert_eq!(error.request_id(), Some("req_required"));
    }
}

#[tokio::test]
async fn optional_metadata_missing_is_allowed_but_null_or_mismatch_is_rejected() {
    let (base_url, _) = spawn_server(http_response(
        "200 OK",
        &[],
        &success_json_with_usage(valid_usage()),
    ));
    test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
        .generate(&request())
        .await
        .unwrap();

    let complete_metadata = json!({
        "created": CREATED,
        "background": "opaque",
        "output_format": "png",
        "quality": "low",
        "size": "1024x1024",
        "data": [{"b64_json": STANDARD_BASE64.encode(valid_png())}],
        "usage": valid_usage()
    });
    let (base_url, _) = spawn_server(http_response(
        "200 OK",
        &[],
        &serde_json::to_vec(&complete_metadata).unwrap(),
    ));
    test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
        .generate(&request())
        .await
        .unwrap();

    for metadata in [
        json!({"background": null}),
        json!({"background": "transparent"}),
        json!({"output_format": "jpeg"}),
        json!({"quality": "medium"}),
        json!({"size": "512x512"}),
        json!({"created": null}),
        json!({"created": MIN_CREATED_UNIX_SECONDS - 1}),
        json!({"created": MAX_CREATED_UNIX_SECONDS + 1}),
        json!({"unknown": true}),
    ] {
        let mut body = json!({
            "created": CREATED,
            "data": [{"b64_json": STANDARD_BASE64.encode(valid_png())}],
            "usage": valid_usage()
        });
        body.as_object_mut()
            .unwrap()
            .extend(metadata.as_object().unwrap().clone());
        let (base_url, _) = spawn_server(http_response(
            "200 OK",
            &[],
            &serde_json::to_vec(&body).unwrap(),
        ));
        assert!(matches!(
            test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
                .generate(&request())
                .await
                .unwrap_err(),
            ImageTransportError::InvalidResponse(_)
        ));
    }
}

#[tokio::test]
async fn strict_success_structure_rejects_unknown_fields_and_multiple_items() {
    let encoded = STANDARD_BASE64.encode(valid_png());
    for body in [
        json!({
            "created": CREATED,
            "data": [{"b64_json": encoded, "url": "https://example.invalid/image.png"}],
            "usage": valid_usage()
        }),
        json!({
            "created": CREATED,
            "data": [{"b64_json": encoded}, {"b64_json": encoded}],
            "usage": valid_usage()
        }),
    ] {
        let (base_url, _) = spawn_server(http_response(
            "200 OK",
            &[],
            &serde_json::to_vec(&body).unwrap(),
        ));
        assert!(matches!(
            test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
                .generate(&request())
                .await
                .unwrap_err(),
            ImageTransportError::InvalidResponse(_)
        ));
    }
}

#[tokio::test]
async fn malformed_base64_png_dimensions_crc_and_truncation_are_rejected() {
    let mut corrupt_crc = valid_png().to_vec();
    *corrupt_crc.last_mut().unwrap() ^= 1;
    let mut truncated = valid_png().to_vec();
    truncated.truncate(truncated.len() - 6);
    for b64_json in [
        "AB==".to_string(),
        "not base64".to_string(),
        STANDARD_BASE64.encode(b"not a png"),
        STANDARD_BASE64.encode(png_fixture(512, 1024)),
        STANDARD_BASE64.encode(animated_png()),
        STANDARD_BASE64.encode(corrupt_crc),
        STANDARD_BASE64.encode(truncated),
    ] {
        let body = serde_json::to_vec(&json!({
            "created": CREATED,
            "data": [{"b64_json": b64_json}],
            "usage": valid_usage()
        }))
        .unwrap();
        let (base_url, _) = spawn_server(http_response("200 OK", &[], &body));
        let error = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
            .generate(&request())
            .await
            .unwrap_err();
        assert!(matches!(error, ImageTransportError::InvalidResponse(_)));
        assert_eq!(error.status(), Some(200));
    }
}

#[tokio::test]
async fn additive_error_without_message_maps_allowlisted_code_and_context_without_leaks() {
    let raw_secret = format!("provider says {TEST_KEY}");
    let body = serde_json::to_vec(&json!({
        "status_code": 400,
        "unknown_top": true,
        "error": {
            "code": "invalid_image_file",
            "param": raw_secret,
            "moderation": {"reason": "private"},
            "new_field": "also private"
        }
    }))
    .unwrap();
    let (base_url, _) = spawn_server(http_response(
        "400 Bad Request",
        &[("x-request-id", "req_error")],
        &body,
    ));
    let error = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
        .generate(&request())
        .await
        .unwrap_err();
    let ImageTransportError::Upstream(upstream) = &error else {
        panic!("unexpected error: {error:?}");
    };
    assert_eq!(upstream.code(), UpstreamErrorCode::InvalidImageFile);
    assert_eq!(upstream.context().status(), 400);
    assert_eq!(upstream.context().request_id(), Some("req_error"));
    assert_eq!(error.status(), Some(400));
    assert_eq!(error.request_id(), Some("req_error"));
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(TEST_KEY));
    assert!(!rendered.contains("provider"));
    assert!(!rendered.contains("private"));
}

#[tokio::test]
async fn wrong_nonstring_and_unknown_error_codes_map_unknown() {
    for code in [
        json!(123),
        json!(null),
        json!("INVALID_VALUE"),
        json!("new_code"),
    ] {
        let body = serde_json::to_vec(&json!({"error": {"code": code}})).unwrap();
        let (base_url, _) = spawn_server(http_response("400 Bad Request", &[], &body));
        let error = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
            .generate(&request())
            .await
            .unwrap_err();
        let ImageTransportError::Upstream(upstream) = error else {
            panic!("unexpected error");
        };
        assert_eq!(upstream.code(), UpstreamErrorCode::Unknown);
    }
}

#[tokio::test]
async fn every_allowlisted_error_code_maps_exactly() {
    for (raw, expected) in [
        ("moderation_blocked", UpstreamErrorCode::ModerationBlocked),
        ("invalid_image_file", UpstreamErrorCode::InvalidImageFile),
        ("invalid_value", UpstreamErrorCode::InvalidValue),
        ("rate_limit_exceeded", UpstreamErrorCode::RateLimitExceeded),
        (
            "insufficient_balance",
            UpstreamErrorCode::InsufficientBalance,
        ),
        ("server_error", UpstreamErrorCode::ServerError),
    ] {
        let body = serde_json::to_vec(&json!({"error": {"code": raw}})).unwrap();
        let (base_url, _) = spawn_server(http_response("400 Bad Request", &[], &body));
        let error = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
            .generate(&request())
            .await
            .unwrap_err();
        let ImageTransportError::Upstream(upstream) = error else {
            panic!("unexpected error");
        };
        assert_eq!(upstream.code(), expected);
    }
}

#[tokio::test]
async fn malformed_and_empty_non_success_are_unparseable_with_context() {
    for (status, body) in [
        ("429 Too Many Requests", b"{".as_slice()),
        ("502 Bad Gateway", b""),
    ] {
        let (base_url, _) = spawn_server(http_response(
            status,
            &[("request-id", "req_unparseable")],
            body,
        ));
        let error = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
            .generate(&request())
            .await
            .unwrap_err();
        assert!(matches!(error, ImageTransportError::UnparseableUpstream(_)));
        assert_eq!(error.request_id(), Some("req_unparseable"));
    }
}

#[tokio::test]
async fn oversized_content_length_is_classified_by_status() {
    for (status, expected_unparseable) in [("200 OK", false), ("429 Too Many Requests", true)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: 1025\r\nx-request-id: req_cap\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
        });
        let error = test_gateway(&base_url, 1024)
            .generate(&request())
            .await
            .unwrap_err();
        if expected_unparseable {
            assert!(matches!(error, ImageTransportError::UnparseableUpstream(_)));
        } else {
            assert!(matches!(error, ImageTransportError::InvalidResponse(_)));
        }
        assert_eq!(error.request_id(), Some("req_cap"));
    }
}

#[tokio::test]
async fn oversized_streamed_non_success_is_unparseable_with_context() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        let _ = stream.write_all(
            b"HTTP/1.1 500 Internal Server Error\r\nTransfer-Encoding: chunked\r\nx-request-id: req_stream_cap\r\nConnection: close\r\n\r\n401\r\n",
        );
        let _ = stream.write_all(&vec![b'x'; 1025]);
        let _ = stream.write_all(b"\r\n0\r\n\r\n");
    });
    let error = test_gateway(&base_url, 1024)
        .generate(&request())
        .await
        .unwrap_err();
    assert!(matches!(error, ImageTransportError::UnparseableUpstream(_)));
    assert_eq!(error.status(), Some(500));
    assert_eq!(error.request_id(), Some("req_stream_cap"));
}

#[tokio::test]
async fn post_header_timeout_carries_response_context() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nx-request-id: req_timeout\r\nConnection: close\r\n\r\n",
        );
        thread::sleep(Duration::from_millis(250));
    });
    let error = test_gateway_with_timeout(
        &base_url,
        DEFAULT_MAX_RESPONSE_BODY,
        Duration::from_millis(50),
    )
    .generate(&request())
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        ImageTransportError::OutcomeUnknown(Some(_))
    ));
    assert_eq!(error.status(), Some(200));
    assert_eq!(error.request_id(), Some("req_timeout"));
    assert!(!format!("{error}").contains("dispatch"));
}

#[tokio::test]
async fn semaphore_wait_is_inside_deadline_and_does_not_dispatch() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let gateway = test_gateway_with_timeout(
        &base_url,
        DEFAULT_MAX_RESPONSE_BODY,
        Duration::from_millis(25),
    );
    let permit = gateway.single_turn.acquire().await.unwrap();
    let error = gateway.generate(&request()).await.unwrap_err();
    drop(permit);
    assert!(matches!(error, ImageTransportError::Timeout(None)));
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}

#[tokio::test]
async fn connection_abort_causes_exactly_one_request_and_no_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_in_thread = Arc::clone(&accepted);
    let server = thread::spawn(move || {
        let deadline = StdInstant::now() + Duration::from_millis(500);
        while StdInstant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted_in_thread.fetch_add(1, Ordering::SeqCst);
                    let _ = read_request(&mut stream);
                    drop(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        }
    });
    let error = test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
        .generate(&request())
        .await
        .unwrap_err();
    assert!(matches!(error, ImageTransportError::OutcomeUnknown(None)));
    server.join().unwrap();
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn edit_multipart_uses_neutral_png_filenames_and_single_media_copies() {
    let response = http_response("200 OK", &[], &success_json_with_usage(valid_usage()));
    let (base_url, received) = spawn_server(response);
    let first_bytes = png_fixture(12, 34);
    let second_bytes = png_fixture(9, 7);
    let mask_bytes = alpha_png_fixture(12, 34);
    let request = EditRequest::new(
        GPT_IMAGE_2_SNAPSHOT,
        "edit the image",
        vec![
            ReferenceImage::new(first_bytes.clone()).unwrap(),
            ReferenceImage::new(second_bytes.clone()).unwrap(),
        ],
        Some(ImageMask::new(mask_bytes.clone()).unwrap()),
    )
    .unwrap();

    test_gateway(&base_url, DEFAULT_MAX_RESPONSE_BODY)
        .edit(&request)
        .await
        .unwrap();
    let raw = received.recv_timeout(Duration::from_secs(2)).unwrap();
    let (headers, body) = request_parts(&raw);
    let text = String::from_utf8_lossy(body);
    assert!(headers.starts_with("POST /v1/images/edits HTTP/1.1"));
    for expected in [
        "name=\"image[]\"; filename=\"image-01.png\"",
        "name=\"image[]\"; filename=\"image-02.png\"",
        "name=\"mask\"; filename=\"mask.png\"",
        "Content-Type: image/png",
    ] {
        assert!(
            text.contains(expected),
            "missing multipart fragment: {expected}"
        );
    }
    assert!(!text.contains("customer"));
    for bytes in [&first_bytes, &second_bytes, &mask_bytes] {
        assert_eq!(
            body.windows(bytes.len())
                .filter(|window| *window == bytes.as_slice())
                .count(),
            1
        );
    }
}
