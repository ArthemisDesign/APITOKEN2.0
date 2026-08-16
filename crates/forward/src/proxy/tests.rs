use super::*;
use crate::affinity::AffinityStore;
use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::config::ProxyConfig;
use crate::upstream::Clients;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use pool::{Pool, Reserve};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
        codex: None,
        gemini: None,
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
