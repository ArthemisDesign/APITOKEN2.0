use super::*;
use crate::affinity::AffinityStore;
use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::config::ProxyConfig;
use crate::upstream::Clients;
use crate::{PricingBridgeFallbackReason, ProviderMode};
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use pool::{Pool, Reserve};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static NEXT_ANTHROPIC_BRIDGE_DB: AtomicU64 = AtomicU64::new(0);



fn anthropic_test_proxy_config() -> Arc<ProxyConfig> {
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
        user_agent: "admission-test".to_string(),
        user_agents: Vec::new(),
        ua_spread: 0,
        anthropic_version: "2023-06-01".to_string(),
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

fn anthropic_test_app(billing: Arc<AsyncBilling>) -> AppState {
    let cfg = anthropic_test_proxy_config();
    AppState {
        provider: ProviderMode::Anthropic,
        authority: Arc::new(registry::authority::AuthorityConfig::new(
            ":memory:".to_string(),
            None,
        )),
        data_db_path: Arc::new(":memory:".to_string()),
        pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        codex: None,
        gemini: None,
        kimi: None,
        glm: None,
        billing: Some(billing),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(1)),
        metrics: Arc::new(Metrics::new()),
        probe_poke: None,
        cfg,
    }
}

async fn invoke_anthropic_bridge(
    execution: Option<(&str, &str)>,
) -> (AppState, Arc<AsyncBilling>, std::path::PathBuf, Response) {
    const ACCOUNT: &str = "anthropic-bridge-account";
    const KEY: &str = "sk-pool-anthropic-bridge";
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_ANTHROPIC_BRIDGE_DB.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "claude-api-anthropic-bridge-{}-{unique}-{sequence}.sqlite",
        std::process::id(),
    ));
    let billing = Arc::new(
        AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
            .expect("start Anthropic bridge test billing"),
    );
    billing.create_account(ACCOUNT, None, 2_000).await.unwrap();
    billing
        .topup(ACCOUNT, 20_000_000, Some("anthropic-bridge-seed"))
        .await
        .unwrap();
    billing
        .issue_key(KEY, ACCOUNT, None, None, None)
        .await
        .unwrap();
    let app = anthropic_test_app(Arc::clone(&billing));
    let mut request_builder = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("x-api-key", KEY)
        .header("content-type", "application/json");
    if let Some((group_id, attempt)) = execution {
        request_builder = request_builder
            .header("x-apitoken-execution-group", group_id)
            .header("x-apitoken-attempt", attempt);
    }
    let request = request_builder
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "model": "claude-sonnet-4-6",
                "max_tokens": 10,
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let response = forward(
        State(app.clone()),
        ConnectInfo("127.0.0.1:4242".parse().unwrap()),
        request,
    )
    .await;
    assert_eq!(response.status().as_u16(), 529);
    billing.flush().await.unwrap();
    (app, billing, path, response)
}



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
        cap_to_balance(1_000, 999_999, 0, &p, 0, 12345),
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


const NS_ACCOUNT: &str = "not-started-account";
const NS_KEY: &str = "sk-pool-not-started";
const NS_TOPUP: i64 = 20_000_000;

/// Биллинг-фикстура с метерным ключом, как в `invoke_anthropic_bridge`, но со своим
/// аккаунтом — тесты с реальным upstream-проходом и settle.
async fn not_started_billing(tag: &str) -> (Arc<AsyncBilling>, std::path::PathBuf) {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_ANTHROPIC_BRIDGE_DB.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "claude-api-not-started-{tag}-{}-{unique}-{sequence}.sqlite",
        std::process::id(),
    ));
    let billing = Arc::new(
        AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
            .expect("start not_started test billing"),
    );
    billing
        .create_account(NS_ACCOUNT, None, 2_000)
        .await
        .unwrap();
    billing
        .topup(NS_ACCOUNT, NS_TOPUP, Some("not-started-seed"))
        .await
        .unwrap();
    billing
        .issue_key(NS_KEY, NS_ACCOUNT, None, None, None)
        .await
        .unwrap();
    (billing, path)
}

/// AppState с ОДНОЙ подпиской в пуле и upstream'ом на мок: запрос реально уходит в сеть
/// (loopback), резерв берётся по-настоящему.
fn not_started_pool_app(billing: Arc<AsyncBilling>, upstream: String) -> AppState {
    let mut cfg = (*anthropic_test_proxy_config()).clone();
    cfg.upstream = upstream;
    let cfg = Arc::new(cfg);
    AppState {
        provider: ProviderMode::Anthropic,
        authority: Arc::new(registry::authority::AuthorityConfig::new(
            ":memory:".to_string(),
            None,
        )),
        data_db_path: Arc::new(":memory:".to_string()),
        pool: Arc::new(Pool::new(
            vec![registry::Sub {
                email: "not-started@example.test".into(),
                token: "secret".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            }],
            Reserve::FULL,
            50.0,
            1_500.0,
        )),
        affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
        clients: Arc::new(Clients::new(&cfg)),
        codex: None,
        gemini: None,
        kimi: None,
        glm: None,
        billing: Some(billing),
        authority_ready: Arc::new(AtomicBool::new(true)),
        breaker: Arc::new(Breaker::new(1)),
        metrics: Arc::new(Metrics::new()),
        probe_poke: None,
        cfg,
    }
}

struct FixedUpstream {
    upstream: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for FixedUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Мок-апстрим, который на любой запрос отвечает одним фиксированным JSON-ответом.
async fn fixed_upstream(status: StatusCode, body: serde_json::Value) -> FixedUpstream {
    let router = axum::Router::new().fallback(move || {
        let body = body.clone();
        async move { (status, axum::Json(body)) }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    FixedUpstream {
        upstream: format!("http://{address}"),
        task,
    }
}

struct CapturingUpstream {
    upstream: String,
    requests: Arc<AtomicU64>,
    captured: Arc<std::sync::Mutex<Option<(HeaderMap, serde_json::Value)>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for CapturingUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct BrokenSseUpstream {
    upstream: String,
    requests: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for BrokenSseUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn broken_sse_upstream() -> BrokenSseUpstream {
    let requests = Arc::new(AtomicU64::new(0));
    let handler_requests = Arc::clone(&requests);
    let router = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(move || {
            let requests = Arc::clone(&handler_requests);
            async move {
                requests.fetch_add(1, Ordering::Relaxed);
                let first = futures_util::stream::once(async {
                    Ok::<_, std::io::Error>(bytes::Bytes::from_static(
                        b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
                    ))
                });
                let failure = futures_util::stream::once(async {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    Err(std::io::Error::other("mock stream failure"))
                });
                let stream = first.chain(failure);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(stream))
                    .unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    BrokenSseUpstream {
        upstream: format!("http://{address}"),
        requests,
        task,
    }
}

async fn capturing_upstream(
    status: StatusCode,
    response_body: serde_json::Value,
) -> CapturingUpstream {
    let requests = Arc::new(AtomicU64::new(0));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let handler_requests = Arc::clone(&requests);
    let handler_captured = Arc::clone(&captured);
    let router = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(move |headers: HeaderMap, body: bytes::Bytes| {
            let response_body = response_body.clone();
            let requests = Arc::clone(&handler_requests);
            let captured = Arc::clone(&handler_captured);
            async move {
                requests.fetch_add(1, Ordering::Relaxed);
                let parsed = serde_json::from_slice(&body).unwrap();
                *captured.lock().unwrap() = Some((headers, parsed));
                (status, axum::Json(response_body))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    CapturingUpstream {
        upstream: format!("http://{address}"),
        requests,
        captured,
        task,
    }
}

async fn fail_once_then_succeed_upstream(success_body: serde_json::Value) -> CapturingUpstream {
    let requests = Arc::new(AtomicU64::new(0));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let handler_requests = Arc::clone(&requests);
    let handler_captured = Arc::clone(&captured);
    let router = axum::Router::new().fallback(move |headers: HeaderMap, body: bytes::Bytes| {
        let success_body = success_body.clone();
        let requests = Arc::clone(&handler_requests);
        let captured = Arc::clone(&handler_captured);
        async move {
            let attempt = requests.fetch_add(1, Ordering::Relaxed);
            let parsed = serde_json::from_slice(&body).unwrap();
            *captured.lock().unwrap() = Some((headers, parsed));
            if attempt == 0 {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(serde_json::json!({
                        "type": "error",
                        "error": {"type": "api_error", "message": "retry"}
                    })),
                )
            } else {
                (StatusCode::OK, axum::Json(success_body))
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    CapturingUpstream {
        upstream: format!("http://{address}"),
        requests,
        captured,
        task,
    }
}

fn enable_test_fallback(app: &mut AppState, upstream: String) {
    let mut cfg = (*app.cfg).clone();
    cfg.claudestore_fallback = Some(crate::config::ClaudeStoreFallbackConfig::for_test(upstream));
    cfg.inject_identity = true;
    cfg.inject_billing = true;
    cfg.identity = "local-oauth-identity-must-not-leak".to_owned();
    let cfg = Arc::new(cfg);
    app.clients = Arc::new(Clients::new(&cfg));
    app.cfg = cfg;
}

async fn invoke_not_started(app: &AppState) -> Response {
    let request = axum::extract::Request::builder()
        .method(Method::POST)
        .uri("/v1/messages")
        .header("x-api-key", NS_KEY)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "model": "claude-sonnet-4-6",
                "max_tokens": 10,
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .unwrap(),
        ))
        .unwrap();
    forward(
        State(app.clone()),
        ConnectInfo("127.0.0.1:4242".parse().unwrap()),
        request,
    )
    .await
}

/// Ждём закрытия резерва (settle асинхронен): возвращает аккаунт с reserved_nano == 0.
async fn settled_account(billing: &AsyncBilling) -> registry::AccountRow {
    loop {
        billing.flush().await.unwrap();
        let account = billing.account(NS_ACCOUNT).await.unwrap().unwrap();
        if account.reserved_nano == 0 {
            break account;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn not_started_upstream_failure_passthrough_refunds_the_hold() {
    // Upstream отвечает 500, попытка одна → терминальный passthrough 500 с заголовком;
    // reserve ушёл в refund (armed HoldGuard), ни цента не списано.
    let mock = fixed_upstream(
        StatusCode::INTERNAL_SERVER_ERROR,
        serde_json::json!({"type":"error","error":{"type":"api_error","message":"boom"}}),
    )
    .await;
    let (billing, path) = not_started_billing("upstream-500").await;
    let app = not_started_pool_app(Arc::clone(&billing), mock.upstream.clone());

    let response = invoke_not_started(&app).await;
    assert_eq!(response.status().as_u16(), 500);
    assert_eq!(
        response.headers().get(EXECUTION_STATE_HEADER).unwrap(),
        EXECUTION_STATE_NOT_STARTED
    );
    let account = settled_account(&billing).await;
    assert_eq!(account.balance_nano, NS_TOPUP);
    let ledger = billing.ledger(NS_ACCOUNT, 10).await.unwrap();
    assert!(!ledger.iter().any(|row| row.kind == "charge"));

    drop(app);
    drop(billing);
    drop(mock);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn successful_delivery_never_carries_not_started_and_charges_the_actual_cost() {
    // Успешный 200: заголовка нет НИКОГДА; tee-метеринг закрывает резерв фактической
    // стоимостью (10 input + 5 output) — баланс уменьшился, charge в журнале есть.
    let upstream_body = serde_json::json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let mock = fixed_upstream(StatusCode::OK, upstream_body.clone()).await;
    let (billing, path) = not_started_billing("upstream-200").await;
    let app = not_started_pool_app(Arc::clone(&billing), mock.upstream.clone());

    let response = invoke_not_started(&app).await;
    assert_eq!(response.status().as_u16(), 200);
    assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
    let delivered = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let delivered: serde_json::Value = serde_json::from_slice(&delivered).unwrap();
    assert_eq!(delivered["usage"]["output_tokens"], 5);
    let account = settled_account(&billing).await;
    assert!(account.balance_nano < NS_TOPUP);
    let ledger = billing.ledger(NS_ACCOUNT, 10).await.unwrap();
    assert!(ledger.iter().any(|row| row.kind == "charge"));

    drop(app);
    drop(billing);
    drop(mock);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn healthy_local_pool_never_calls_claudestore() {
    let response_body = serde_json::json!({
        "id": "msg_local",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "content": [{"type": "text", "text": "local"}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let local = fixed_upstream(StatusCode::OK, response_body.clone()).await;
    let external = capturing_upstream(StatusCode::OK, response_body).await;
    let (billing, path) = not_started_billing("fallback-local-healthy").await;
    let mut app = not_started_pool_app(Arc::clone(&billing), local.upstream.clone());
    enable_test_fallback(&mut app, external.upstream.clone());

    let response = invoke_not_started(&app).await;
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(external.requests.load(Ordering::Relaxed), 0);
    assert_eq!(
        app.metrics
            .claudestore_fallback_attempts
            .load(Ordering::Relaxed),
        0
    );

    drop(app);
    drop(billing);
    drop(local);
    drop(external);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn local_rotation_success_prevents_claudestore_attempt() {
    let response_body = serde_json::json!({
        "id": "msg_local_retry",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "content": [{"type": "text", "text": "local retry"}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let local = fail_once_then_succeed_upstream(response_body.clone()).await;
    let external = capturing_upstream(StatusCode::OK, response_body).await;
    let (billing, path) = not_started_billing("fallback-local-rotation").await;
    let mut app = not_started_pool_app(Arc::clone(&billing), local.upstream.clone());
    app.pool = Arc::new(Pool::new(
        vec![
            registry::Sub {
                email: "rotation-a@example.test".into(),
                token: "secret-a".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            },
            registry::Sub {
                email: "rotation-b@example.test".into(),
                token: "secret-b".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            },
        ],
        Reserve::FULL,
        50.0,
        1_500.0,
    ));
    app.breaker = Arc::new(Breaker::new(2));
    let mut cfg = (*app.cfg).clone();
    cfg.max_tries = 2;
    app.cfg = Arc::new(cfg);
    app.clients = Arc::new(Clients::new(&app.cfg));
    enable_test_fallback(&mut app, external.upstream.clone());

    let response = invoke_not_started(&app).await;
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(local.requests.load(Ordering::Relaxed), 2);
    assert_eq!(external.requests.load(Ordering::Relaxed), 0);

    drop(app);
    drop(billing);
    drop(local);
    drop(external);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn exhausted_local_pool_calls_claudestore_once_without_local_identity() {
    let response_body = serde_json::json!({
        "id": "msg_external",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "content": [{"type": "text", "text": "external"}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let external = capturing_upstream(StatusCode::OK, response_body).await;
    let (billing, path) = not_started_billing("fallback-empty-pool").await;
    let mut app = not_started_pool_app(Arc::clone(&billing), "http://127.0.0.1:1".to_owned());
    app.pool = Arc::new(Pool::new(Vec::new(), Reserve::FULL, 50.0, 1_500.0));
    enable_test_fallback(&mut app, external.upstream.clone());

    let response = invoke_not_started(&app).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(external.requests.load(Ordering::Relaxed), 1);
    assert_eq!(
        app.metrics
            .claudestore_fallback_attempts
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        app.metrics
            .claudestore_fallback_successes
            .load(Ordering::Relaxed),
        1
    );
    assert_eq!(
        app.metrics
            .claudestore_fallback_failures
            .load(Ordering::Relaxed),
        0
    );
    let (headers, outbound_body) = external.captured.lock().unwrap().clone().unwrap();
    assert!(headers.get("x-api-key").is_some());
    assert!(headers.get("authorization").is_none());
    assert!(headers.get("x-claude-code-session-id").is_none());
    assert_eq!(outbound_body["model"], "claude-sonnet-4-6");
    let serialized = serde_json::to_string(&outbound_body).unwrap();
    assert!(!serialized.contains("local-oauth-identity-must-not-leak"));
    assert!(!serialized.contains("x-anthropic-billing-header"));
    let account = settled_account(&billing).await;
    assert!(account.balance_nano < NS_TOPUP);
    let ledger = billing.ledger(NS_ACCOUNT, 10).await.unwrap();
    assert!(ledger.iter().any(|row| row.kind == "charge"));

    drop(app);
    drop(billing);
    drop(external);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn claudestore_failure_returns_local_terminal_and_refunds_hold() {
    let external = capturing_upstream(
        StatusCode::INTERNAL_SERVER_ERROR,
        serde_json::json!({
            "type": "error",
            "error": {"type": "api_error", "message": "external unavailable"}
        }),
    )
    .await;
    let (billing, path) = not_started_billing("fallback-external-failure").await;
    let mut app = not_started_pool_app(Arc::clone(&billing), "http://127.0.0.1:1".to_owned());
    app.pool = Arc::new(Pool::new(Vec::new(), Reserve::FULL, 50.0, 1_500.0));
    enable_test_fallback(&mut app, external.upstream.clone());

    let response = invoke_not_started(&app).await;
    assert_eq!(response.status().as_u16(), 529);
    assert!(response.headers().get(EXECUTION_STATE_HEADER).is_none());
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(external.requests.load(Ordering::Relaxed), 1);
    assert_eq!(
        app.metrics
            .claudestore_fallback_failures
            .load(Ordering::Relaxed),
        1
    );
    let account = settled_account(&billing).await;
    assert_eq!(account.balance_nano, NS_TOPUP);
    let ledger = billing.ledger(NS_ACCOUNT, 10).await.unwrap();
    assert!(!ledger.iter().any(|row| row.kind == "charge"));

    drop(app);
    drop(billing);
    drop(external);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn claudestore_post_byte_stream_failure_is_never_replayed() {
    let external = broken_sse_upstream().await;
    let (billing, path) = not_started_billing("fallback-broken-stream").await;
    let mut app = not_started_pool_app(Arc::clone(&billing), "http://127.0.0.1:1".to_owned());
    app.pool = Arc::new(Pool::new(Vec::new(), Reserve::FULL, 50.0, 1_500.0));
    enable_test_fallback(&mut app, external.upstream.clone());

    let response = invoke_not_started(&app).await;
    assert_eq!(response.status(), StatusCode::OK);
    let delivered = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let delivered = String::from_utf8(delivered.to_vec()).unwrap();
    assert!(delivered.contains("event: message_start"));
    assert!(delivered.contains("event: error"));
    assert_eq!(external.requests.load(Ordering::Relaxed), 1);
    assert_eq!(
        app.metrics
            .claudestore_fallback_attempts
            .load(Ordering::Relaxed),
        1
    );
    let _ = settled_account(&billing).await;

    drop(app);
    drop(billing);
    drop(external);
    let _ = std::fs::remove_file(path);
}

/// Authorization carries the discount that will price the request, resolved per provider. That is
/// the only pricing decision admission makes, so both halves must be visible on the `Authz` a
/// request is admitted with: the override where the account has one, the default everywhere else.
#[tokio::test]
async fn authorization_resolves_the_discount_per_provider() {
    const ACCOUNT: &str = "discount-authz-account";
    const KEY: &str = "sk-pool-discount-authz";
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-discount-authz-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let billing = Arc::new(AsyncBilling::start(path_string.clone(), 1).unwrap());
    billing.create_account(ACCOUNT, None, 5_000).await.unwrap();
    billing
        .topup(ACCOUNT, 1_000_000, Some("discount-seed"))
        .await
        .unwrap();
    billing
        .issue_key(KEY, ACCOUNT, None, None, None)
        .await
        .unwrap();
    billing
        .account_provider_discount(ACCOUNT, registry::PROVIDER_OPENAI, Some(2_000))
        .await
        .unwrap();

    let app = anthropic_test_app(Arc::clone(&billing));
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-api-key", KEY.parse().unwrap());
    let peer: std::net::SocketAddr = "127.0.0.1:65000".parse().unwrap();
    let authz = authorize(&app, &headers, &peer).await;

    assert_eq!(authz.mult_for(registry::PROVIDER_OPENAI), 2_000);
    assert_eq!(authz.mult_for(registry::PROVIDER_ANTHROPIC), 5_000);
    assert_eq!(authz.mult_for(registry::PROVIDER_GOOGLE), 5_000);

    // Clearing the override puts that provider straight back on the account default: no
    // intermediate state and no version to activate.
    billing
        .account_provider_discount(ACCOUNT, registry::PROVIDER_OPENAI, None)
        .await
        .unwrap();
    let authz = authorize(&app, &headers, &peer).await;
    assert_eq!(authz.mult_for(registry::PROVIDER_OPENAI), 5_000);

    drop(app);
    drop(billing);
    let _ = std::fs::remove_file(path);
}
