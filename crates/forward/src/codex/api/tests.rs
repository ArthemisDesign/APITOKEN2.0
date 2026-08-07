use super::*;
use crate::codex::{CodexConfig, CodexPrices};
use std::collections::BTreeMap;

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
        base_url: codex_credential::CODEX_DEFAULT_BASE_URL.to_string(),
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
fn codex_diagnostic_metadata_is_bounded() {
    let metadata_error = parse_responses_request(
        &gateway(),
        json!({
            "model": "gpt-5.6",
            "input": "hi",
            "client_metadata": {"turn_id": 42}
        }),
    )
    .unwrap_err();
    assert_eq!(
        metadata_error.param.as_deref(),
        Some("client_metadata.turn_id")
    );
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
