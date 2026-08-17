use super::*;

fn credential(subject: &str) -> GeminiCredential {
    GeminiCredential {
        version: 1,
        access_token: "access-token-value".into(),
        refresh_token: "refresh-token-value".into(),
        expires_at: 1_000,
        oauth_client_id: ANTIGRAVITY_CLIENT_ID.into(),
        oauth_client_secret: ANTIGRAVITY_CLIENT_SECRET.into(),
        token_uri: TOKEN_URL.into(),
        subject: subject.into(),
        email: "owner@example.com".into(),
        project_id: "managed-project".into(),
        tier_id: "g1-pro-tier".into(),
        tier_name: "Google AI Pro".into(),
        plan: "google_ai_pro".into(),
        proxy: "http://user:pass@127.0.0.1:8080".into(),
        proxy_order_id: 42,
        issued_at: 100,
    }
}

fn fixture() -> (PathBuf, CredentialKeyring) {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let root = std::env::temp_dir().join(format!(
        "gemini-oauth-publish-{}-{}",
        std::process::id(),
        URL_SAFE_NO_PAD.encode(random)
    ));
    let ring = CredentialKeyring::parse(&format!("current:{}", "55".repeat(32))).unwrap();
    (root, ring)
}

/// A new handoff is one Antigravity consent, and a legacy-bootstrap session sealed by an older
/// binary can still reach its second phase across a deploy.
#[test]
fn one_consent_uses_the_pinned_antigravity_identity_and_legacy_sessions_still_advance() {
    let (root, ring) = fixture();
    let database = root.join("state").join("authbot.db");
    let store = Store::open(database.to_str().unwrap()).unwrap();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.join("gemini").to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let proxy = "http://user:pass@127.0.0.1:8080";
    let links = begin(&store, &config, 1, proxy, 0).unwrap();
    let authorize = reqwest::Url::parse(&links.authorize_url).unwrap();
    assert!(authorize
        .query_pairs()
        .any(|(name, value)| name == "client_id" && value == ANTIGRAVITY_CLIENT_ID));
    assert!(authorize
        .query_pairs()
        .any(|(name, value)| { name == "redirect_uri" && value == ANTIGRAVITY_REDIRECT_URI }));
    assert!(links
        .submit_url
        .starts_with("https://gemini.example/oauth/callback?state="));
    let state = authorize
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .unwrap();
    let session = store.claim_gemini_oauth(&state).unwrap().unwrap();
    assert_eq!(
        open_pending_secret(&config, &session).unwrap().phase,
        OAuthPhase::DirectAntigravity,
        "no new handoff may create the retired Gemini CLI bootstrap phase"
    );

    // Deploy overlap: a session an older binary sealed in the legacy phase still transitions.
    let legacy_prepared = prepare_oauth(
        &config,
        OAuthPhase::LegacyBootstrap,
        "http://user:pass@127.0.0.1:8080/",
        0,
        "",
    )
    .unwrap();
    store
        .start_gemini_oauth(
            2,
            &legacy_prepared.state,
            &legacy_prepared.sealed_payload,
            now() + 600,
            0,
        )
        .unwrap();
    let legacy = store
        .claim_gemini_oauth(&legacy_prepared.state)
        .unwrap()
        .unwrap();
    let final_links = begin_antigravity_phase(
        &store,
        &config,
        &legacy,
        "http://user:pass@127.0.0.1:8080/",
        0,
        "google-subject",
    )
    .unwrap();
    let authorize = reqwest::Url::parse(&final_links.authorize_url).unwrap();
    assert!(authorize
        .query_pairs()
        .any(|(name, value)| name == "client_id" && value == ANTIGRAVITY_CLIENT_ID));
    assert!(authorize
        .query_pairs()
        .any(|(name, value)| { name == "redirect_uri" && value == ANTIGRAVITY_REDIRECT_URI }));
    let final_state = authorize
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .unwrap();
    let final_session = store.claim_gemini_oauth(&final_state).unwrap().unwrap();
    let final_pending = open_pending_secret(&config, &final_session).unwrap();
    assert_eq!(final_pending.phase, OAuthPhase::AntigravityFinal);
    assert_eq!(final_pending.bootstrap_subject, "google-subject");
    for path in [
        database.clone(),
        PathBuf::from(format!("{}-wal", database.display())),
    ] {
        if let Ok(bytes) = fs::read(path) {
            for private in ["google-subject", "user:pass", "managed-project"] {
                assert!(!bytes
                    .windows(private.len())
                    .any(|window| window == private.as_bytes()));
            }
        }
    }
    drop(store);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn onboarding_wire_identity_matches_antigravity() {
    assert_eq!(
        antigravity_user_agent(),
        "antigravity/hub/2.2.1 darwin/arm64"
    );
    assert_eq!(
        antigravity_control_user_agent(),
        "antigravity/hub/2.2.1 darwin/arm64 google-api-nodejs-client/10.3.0"
    );
    assert_eq!(
        load_code_assist_request_body(),
        json!({"metadata": {"ideType": "ANTIGRAVITY"}})
    );
    assert_eq!(
        antigravity_control_metadata(),
        json!({
            "ide_type": "ANTIGRAVITY",
            "ide_version": "2.2.1",
            "ide_name": "antigravity"
        })
    );
    assert_eq!(
        onboard_request_body("paid-tier"),
        json!({
            "tier_id": "paid-tier",
            "metadata": {
                "ide_type": "ANTIGRAVITY",
                "ide_version": "2.2.1",
                "ide_name": "antigravity"
            }
        })
    );
}

#[test]
fn legacy_identity_bootstrap_defers_subscription_admission_to_antigravity() {
    assert_eq!(
        post_identity_action(OAuthPhase::LegacyBootstrap),
        PostIdentityAction::StartAntigravityConsent
    );
    for phase in [OAuthPhase::AntigravityFinal, OAuthPhase::DirectAntigravity] {
        assert_eq!(
            post_identity_action(phase),
            PostIdentityAction::ResolveAntigravitySubscription
        );
    }
}

#[test]
fn oauth_token_form_order_matches_each_pinned_client() {
    let form = token_exchange_form(
        OAuthPhase::AntigravityFinal,
        "client id",
        "verifier/value",
        "code+value",
        ANTIGRAVITY_REDIRECT_URI,
        "client-secret",
    )
    .unwrap();
    assert_eq!(
        form.as_str(),
        "client_id=client+id&client_secret=client-secret&code=code%2Bvalue&code_verifier=verifier%2Fvalue&grant_type=authorization_code&redirect_uri=http%3A%2F%2Flocalhost%3A51121%2Foauth-callback"
    );
    let legacy = token_exchange_form(
        OAuthPhase::LegacyBootstrap,
        "client id",
        "verifier/value",
        "code+value",
        LEGACY_REDIRECT_URI,
        "client-secret",
    )
    .unwrap();
    assert_eq!(
        legacy.as_str(),
        "client_id=client+id&code_verifier=verifier%2Fvalue&code=code%2Bvalue&grant_type=authorization_code&redirect_uri=https%3A%2F%2Fcodeassist.google.com%2Fauthcode&client_secret=client-secret"
    );
}

#[test]
fn userinfo_supplies_only_the_official_fetch_authorization_header() {
    assert_eq!(
        official_userinfo_headers("Bearer redacted"),
        [("Authorization", "Bearer redacted")]
    );
}

#[test]
fn oauth_start_persists_only_a_state_bound_encrypted_payload() {
    let (root, ring) = fixture();
    let state_dir = root.join("state");
    let database = state_dir.join("authbot.db");
    let store = Store::open(database.to_str().unwrap()).unwrap();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.join("gemini").to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let proxy = "http://user:pass@127.0.0.1:8080";
    let links = begin(&store, &config, 42, proxy, 777).unwrap();
    assert!(!links.authorize_url.contains("user:pass"));
    assert!(!links.authorize_url.contains(ANTIGRAVITY_CLIENT_SECRET));
    assert!(!links.submit_url.contains("user:pass"));
    let url = reqwest::Url::parse(&links.authorize_url).unwrap();
    assert!(url
        .query_pairs()
        .any(|(name, value)| { name == "client_id" && value == ANTIGRAVITY_CLIENT_ID }));
    let state = url
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .unwrap();
    assert!(url
        .query_pairs()
        .any(|(name, value)| { name == "code_challenge_method" && value == "S256" }));
    let session = store.claim_gemini_oauth(&state).unwrap().unwrap();
    assert_eq!(
        active_egress(&store, &config, 42),
        Some((format!("{proxy}/"), 777))
    );
    assert_eq!(store.interrupted_gemini_chats().unwrap(), vec![42]);
    assert!(!session.sealed_payload.contains("user:pass"));
    assert!(!session.sealed_payload.contains(LEGACY_CLIENT_SECRET));
    let envelope: SealedCredential = serde_json::from_str(&session.sealed_payload).unwrap();
    let decrypted = config
        .keyring
        .open_secret(&session.state, &envelope)
        .unwrap();
    let pending: PendingOAuthSecret = serde_json::from_str(decrypted.as_str()).unwrap();
    assert_eq!(pending.proxy, format!("{proxy}/"));
    assert_eq!(pending.proxy_order_id, 777);
    assert_eq!(pending.client_id, ANTIGRAVITY_CLIENT_ID);
    assert_eq!(pending.client_secret, ANTIGRAVITY_CLIENT_SECRET);
    assert_eq!(pending.redirect_uri, ANTIGRAVITY_REDIRECT_URI);
    assert_eq!(pending.phase, OAuthPhase::DirectAntigravity);
    assert!(pending.bootstrap_subject.is_empty());
    assert!(valid_oauth_value(&pending.verifier, 256));
    assert!(!session.sealed_payload.contains(&pending.verifier));
    for path in [
        database.clone(),
        PathBuf::from(format!("{}-wal", database.display())),
    ] {
        if let Ok(bytes) = fs::read(path) {
            assert!(!bytes
                .windows(proxy.len())
                .any(|window| window == proxy.as_bytes()));
            assert!(!bytes
                .windows(pending.verifier.len())
                .any(|window| window == pending.verifier.as_bytes()));
        }
    }
    drop(store);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cancel_fence_restarts_with_fresh_pkce_and_rejects_the_old_generation() {
    let (root, ring) = fixture();
    let database = root.join("state").join("authbot.db");
    let store = Store::open(database.to_str().unwrap()).unwrap();
    store.register_user(42, 42, "seller").unwrap();
    let offer = store
        .create_offer_with_proxy("Google AI Pro", "$20", 999, 42, "seller", "")
        .unwrap();
    store.set_response(offer, 42, "accepted").unwrap();
    assert!(store.claim_offer_payment(offer, 42).unwrap());
    assert!(store.mark_offer_paid(offer, 42).unwrap());
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.join("gemini").to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let links = begin(&store, &config, 42, "http://user:pass@127.0.0.1:8080", 777).unwrap();
    let state = reqwest::Url::parse(&links.authorize_url)
        .unwrap()
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .unwrap();
    let session = store.claim_gemini_oauth(&state).unwrap().unwrap();
    assert!(oauth_session_handoff_is_current(&store, &session));
    assert_eq!(
        active_egress(&store, &config, 42),
        Some(("http://user:pass@127.0.0.1:8080/".into(), 777))
    );
    let job = links.job.unwrap();
    let fresh_job = store
        .rewind_handoff_step(42, &job, "gm_wait", "gm_gproxy", Some(("", 777)))
        .unwrap()
        .expect("/cancel rotates the exact seller generation");
    assert!(!oauth_session_handoff_is_current(&store, &session));
    assert!(store.active_gemini_session(42).unwrap().is_none());
    assert_ne!(fresh_job.token, job.token);

    let restarted = begin(&store, &config, 42, "http://user:pass@127.0.0.1:8080/", 777).unwrap();
    let restarted_state = reqwest::Url::parse(&restarted.authorize_url)
        .unwrap()
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .unwrap();
    assert_ne!(restarted_state, state);
    assert!(store.claim_gemini_oauth(&state).unwrap().is_none());
    let restarted_session = store
        .pending_gemini_session_by_state(&restarted_state)
        .unwrap()
        .expect("fresh PKCE generation is immediately pending");
    assert!(oauth_session_handoff_is_current(&store, &restarted_session));
    assert_ne!(restarted.job.unwrap().token, fresh_job.token);
    // A late old worker cannot move the seller back onto a retry step after the restart.
    assert!(!store
        .set_handoff_state_for_seller_job(
            42,
            session.job.as_ref().unwrap(),
            "gm_gproxy",
            "http://attacker.invalid:8080",
            0,
        )
        .unwrap());
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn claimed_invalid_callback_finishes_outside_the_http_future() {
    let (root, ring) = fixture();
    let store = Arc::new(Store::open(root.join("state/authbot.db").to_str().unwrap()).unwrap());
    let oauth = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.join("gemini").to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let links = begin(&store, &oauth, 42, "http://user:pass@127.0.0.1:8080", 777).unwrap();
    let oauth_state = reqwest::Url::parse(&links.authorize_url)
        .unwrap()
        .query_pairs()
        .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
        .unwrap();
    let bot_config = Arc::new(BotConfig {
        kimi_roster: None,
        glm_roster: None,
        tripo3d_roster: None,
        suno_roster: None,
        admins_id: HashSet::new(),
        admins_name: HashSet::new(),
        claude_bin: String::new(),
        claude_config_dir: String::new(),
        database_url: String::new(),
        fleet: String::new(),
        bsc_python: String::new(),
        bsc_script: String::new(),
        iproyal_key: String::new(),
        codex_bin: String::new(),
        codex_homes_dir: String::new(),
        codex_roster: None,
        gemini_dir: root.join("gemini").to_string_lossy().into_owned(),
        gemini_oauth: Some(oauth.clone()),
    });
    let callback = CallbackState {
        bot: Bot::new("unused-test-token"),
        store: store.clone(),
        config: bot_config,
    };

    let response = finish_oauth(&callback, Some(&oauth_state), Some(""), None).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    tokio::time::timeout(Duration::from_secs(1), async {
        while store.active_gemini_session(42).unwrap().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("detached terminal task removes the claimed session");
    assert!(!oauth.abort_inflight(42));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn pending_payload_without_redirect_keeps_inflight_hosted_callback_compatible() {
    let pending: PendingOAuthSecret = serde_json::from_value(json!({
        "verifier": "legacy-verifier",
        "proxy": "http://proxy.example:8080/",
        "proxy_order_id": 0,
        "client_id": "legacy.apps.googleusercontent.com",
        "client_secret": "legacy-secret"
    }))
    .unwrap();
    assert!(pending.redirect_uri.is_empty());
    assert_eq!(pending.phase, OAuthPhase::DirectAntigravity);
    assert!(pending.bootstrap_subject.is_empty());
}

#[test]
fn callback_page_is_non_cacheable_and_cannot_load_or_refer() {
    let response = status_page(StatusPage::CheckingSubscription);
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers()["cache-control"], "no-store, max-age=0");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(
        response.headers()["cross-origin-opener-policy"],
        "same-origin"
    );
    assert_eq!(
        response.headers()["cross-origin-resource-policy"],
        "same-origin"
    );
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("default-src 'none'"));
    assert!(csp.contains("style-src 'sha256-"));
    assert!(!csp.contains("'unsafe-inline'"));
    let page = page_shell(
        "wait",
        "02",
        "Callback принят",
        "Проверяем подписку",
        "Вкладку можно закрыть.",
        "Результат придёт в Telegram; /cancel начнёт всё заново.",
        2,
        "",
    );
    assert!(page.contains("viewport-fit=cover"));
    assert!(page.contains("/cancel начнёт всё заново"));
    assert!(page.contains("prefers-reduced-motion"));
}

#[tokio::test]
async fn inflight_completion_is_aborted_exactly_once() {
    let (root, ring) = fixture();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.join("gemini").to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let task = tokio::spawn(std::future::pending::<()>());
    config.register_inflight(42, "A".repeat(43), task.abort_handle());
    assert!(config.abort_inflight(42));
    assert!(!config.abort_inflight(42));
    assert!(task.await.unwrap_err().is_cancelled());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn code_form_accepts_only_generated_state_and_posts_without_query_secrets() {
    let state = "A".repeat(43);
    assert!(valid_oauth_state(&state));
    assert!(!valid_oauth_state("too-short"));
    assert!(!valid_oauth_state(&format!("{}\"", "A".repeat(42))));
    let response = code_form(&state, OAuthPhase::AntigravityFinal);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store, max-age=0");
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("form-action 'self'"));
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let legacy = code_form(&state, OAuthPhase::LegacyBootstrap);
    assert_eq!(legacy.status(), StatusCode::OK);
}

#[test]
fn localhost_callback_submission_is_state_bound() {
    let state = "A".repeat(43);
    let callback =
        format!("http://localhost:51121/oauth-callback?state={state}&code=4%2Fsecret-code&scope=x");
    assert_eq!(
        submitted_authorization_code(&callback, &state, false).as_deref(),
        Some("4/secret-code")
    );
    assert!(submitted_authorization_code(&callback, &"B".repeat(43), false).is_none());
    assert!(submitted_authorization_code(
        &format!("https://localhost:51121/oauth-callback?state={state}&code=x"),
        &state,
        false,
    )
    .is_none());
    assert_eq!(
        submitted_authorization_code("4/direct-code", &state, true).as_deref(),
        Some("4/direct-code")
    );
    assert!(submitted_authorization_code("4/direct-code", &state, false).is_none());
    for ambiguous in [
        format!("http://localhost:51121/oauth-callback?state={state}&state={state}&code=x"),
        format!("http://localhost:51121/oauth-callback?state={state}&code=x&code=y"),
        format!("http://localhost:51121/oauth-callback?state={state}&error=x&error=y"),
    ] {
        assert!(submitted_authorization_code(&ambiguous, &state, false).is_none());
    }
    assert!(submitted_authorization_code("state=x&code=y", &state, true).is_none());
}

#[test]
fn empty_caddy_transport_headers_do_not_hide_present_values_or_create_fake_values() {
    assert_eq!(first_nonempty(None, Some("state")), Some("state"));
    assert_eq!(first_nonempty(Some(""), Some("state")), Some("state"));
    assert_eq!(first_nonempty(Some("query"), Some("header")), Some("query"));
    assert_eq!(first_nonempty(None, Some("")), None);
    assert_eq!(first_nonempty(Some(""), Some("")), None);
}

#[test]
fn pending_proxy_is_restricted_to_a_credential_safe_http_origin() {
    assert!(normalize_proxy_url("http://user:pass@127.0.0.1:8080").is_ok());
    assert!(normalize_proxy_url("https://proxy.example:8443").is_ok());
    for invalid in [
        "socks5://user:pass@127.0.0.1:1080",
        "http://proxy.example/path",
        "http://proxy.example?token=secret",
        "http://proxy.example/#fragment",
        "http://proxy.example/\nheader",
    ] {
        assert!(
            normalize_proxy_url(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
}

#[test]
fn transport_recovery_reaches_the_observed_gateway_recovery_window_without_bursting() {
    assert_eq!(
        TRANSPORT_RECOVERY_DELAYS,
        [
            Duration::from_secs(2),
            Duration::from_secs(5),
            Duration::from_secs(10),
            Duration::from_secs(20),
        ]
    );
    assert_eq!(
        TRANSPORT_RECOVERY_DELAYS.iter().sum::<Duration>(),
        Duration::from_secs(37)
    );
}

#[test]
fn a_held_account_is_parked_sealed_and_reopens_only_for_its_own_chat() {
    let (root, ring) = fixture();
    let database = root.join("state").join("authbot.db");
    let store = Store::open(database.to_str().unwrap()).unwrap();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.join("gemini").to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let session = GeminiOAuthSession {
        state: "state-value".into(),
        chat_id: 4242,
        sealed_payload: String::new(),
        expires_ts: now() + 600,
        job: None,
    };
    let credential = credential("subject-parked");
    park_verification(
        &store,
        &config,
        session.chat_id,
        session.job.as_ref(),
        &credential,
    );

    let parked = store
        .claim_gemini_verification(4242, now() + VERIFICATION_PROBE_INTERVAL_SECS)
        .unwrap()
        .unwrap();
    // Automatic acceptance owns the record from here: the first retry is one interval away and
    // the window closes a day after consent, while the envelope itself outlives both.
    assert!(parked.probe_deadline_ts > now() + VERIFICATION_PROBE_WINDOW_SECS - 60);
    assert!(parked.expires_ts > parked.probe_deadline_ts);
    assert!(!parked.deadline_notified);
    // Nothing identifying may sit in the clear next to the record.
    for secret in [
        credential.refresh_token.as_str(),
        credential.access_token.as_str(),
        credential.email.as_str(),
        credential.subject.as_str(),
        credential.proxy.as_str(),
    ] {
        assert!(!parked.sealed_payload.contains(secret));
    }
    let opened = open_parked_credential(&config, &parked).unwrap();
    assert_eq!(opened.refresh_token, credential.refresh_token);
    assert_eq!(opened.project_id, credential.project_id);
    assert_eq!(opened.proxy, credential.proxy);

    // The envelope is bound to its chat, so another seller's record cannot be opened with it.
    let foreign = GeminiPendingVerification {
        chat_id: 4243,
        ..parked
    };
    assert!(open_parked_credential(&config, &foreign).is_none());
}

/// The contract of the automatic acceptance window, in one place: how often it retries, how
/// long it retries for, how long the credential outlives that, and which verdicts are allowed
/// to end it. Yesterday's dead retry button came from treating a throttled CONNECT as a verdict
/// about the subscription — that class must stay retryable here and in `generation_request`.
#[test]
fn automatic_acceptance_retries_everything_except_settled_verdicts() {
    assert_eq!(VERIFICATION_PROBE_INTERVAL_SECS, 300);
    assert_eq!(VERIFICATION_PROBE_WINDOW_SECS, 24 * 3600);
    assert!(
        VERIFICATION_PARK_SECS > VERIFICATION_PROBE_WINDOW_SECS,
        "the credential must stay on record after automatic probing stops"
    );

    for settled in [
        Failure::Authorization,
        Failure::AccountMismatch,
        Failure::Duplicate,
        Failure::DuplicateProxy,
        Failure::MigrationProxyMismatch,
        Failure::StaleHandoff,
    ] {
        assert!(
            settled.stops_automatic_probing(),
            "{} cannot change on a retry",
            settled.code()
        );
    }
    for retryable in [
        // The account is held by Google, or its tier is not provisioned yet: both clear on
        // their own timescale, which is exactly what the 24-hour window is for.
        Failure::AccountValidationRequired,
        Failure::UnsupportedPlan,
        // Surface, transport and egress states — never verdicts about the subscription.
        Failure::GenerationUnavailable,
        Failure::TransportUnavailable,
        Failure::Temporary,
        Failure::CodeAssistApiDisabled,
        Failure::Storage,
    ] {
        assert!(
            !retryable.stops_automatic_probing(),
            "{} must keep the automatic window open",
            retryable.code()
        );
    }

    // A CONNECT-stage refusal never reached Google, so the probe may safely resend it; anything
    // ambiguous after the request left us must not be replayed.
    for pre_target in [
        crate::gemini_transport::RequestFailureKind::ProxyThrottle,
        crate::gemini_transport::RequestFailureKind::ProxyTimeout,
        crate::gemini_transport::RequestFailureKind::ProxyConnect,
    ] {
        assert!(pre_target.safe_to_retry_before_target());
    }
    for ambiguous in [
        crate::gemini_transport::RequestFailureKind::Timeout,
        crate::gemini_transport::RequestFailureKind::Network,
    ] {
        assert!(!ambiguous.safe_to_retry_before_target());
    }
}

#[test]
fn generation_acceptance_surfaces_are_ordered_and_access_failures_stay_actionable() {
    assert_eq!(
        GENERATION_PROBE_SURFACES.map(|(_, host)| host),
        [
            CODE_ASSIST_SANDBOX_URL,
            CODE_ASSIST_DAILY_URL,
            CODE_ASSIST_PROD_URL
        ],
        "acceptance must first ask the origin the engine actually serves customer traffic from,          which is CLAUDE_API_GEMINI_UPSTREAM (the sandbox origin), then the origin the first-party          Antigravity client uses, and only then the legacy production host"
    );
    let disabled = json!({
        "error": {
            "code": 403,
            "status": "PERMISSION_DENIED",
            "message": "Cloud Code Private API has not been used in project 123 before or it is disabled. Enable cloudcode-pa.googleapis.com then retry.",
        }
    });
    assert_eq!(
        classify_generation_failure(&serde_json::to_vec(&disabled).unwrap()),
        Some(Failure::CodeAssistApiDisabled)
    );
    let plain = json!({"error": {"code": 403, "status": "PERMISSION_DENIED"}});
    assert_eq!(
        classify_generation_failure(&serde_json::to_vec(&plain).unwrap()),
        None,
        "a generic access rejection must not claim the private API is disabled"
    );
    // Observed in production on 2026-08-03: an account with a live g1-pro-tier subscription is
    // refused generation on every surface until Google's own account verification is done.
    let validation = json!({
        "error": {
            "code": 403,
            "status": "PERMISSION_DENIED",
            "message": "Verify your account to continue.",
            "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "VALIDATION_REQUIRED"}],
        }
    });
    assert_eq!(
        classify_generation_failure(&serde_json::to_vec(&validation).unwrap()),
        Some(Failure::AccountValidationRequired)
    );
    let message_only = json!({
        "error": {"code": 403, "status": "PERMISSION_DENIED", "message": "Verify your account to continue."}
    });
    assert_eq!(
        classify_generation_failure(&serde_json::to_vec(&message_only).unwrap()),
        Some(Failure::AccountValidationRequired),
        "the reason field is not guaranteed; the exact message alone is enough evidence"
    );
    // Google carries the account's own verification link in the rejection metadata; forwarding
    // it is the only actionable instruction, since a normal Gemini session never sees this check.
    let with_link = json!({
        "error": {
            "code": 403,
            "status": "PERMISSION_DENIED",
            "message": "Verify your account to continue.",
            "details": [{
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": "VALIDATION_REQUIRED",
                "metadata": {
                    "validation_url": "https://accounts.google.com/signin/continue?sarp=1&scc=1&plt=token",
                    "validation_url_link_text": "Verify your account",
                },
            }],
        }
    });
    assert_eq!(
        verification_url_from_body(&serde_json::to_vec(&with_link).unwrap()).as_deref(),
        Some("https://accounts.google.com/signin/continue?sarp=1&scc=1&plt=token")
    );
    assert_eq!(
        verification_url_from_body(&serde_json::to_vec(&validation).unwrap()),
        None,
        "a rejection without metadata must not invent a link"
    );
    // The link is upstream input that we forward to a human, so anything but a real Google
    // sign-in URL is refused rather than turned into a phishing vector in our own message.
    for hostile in [
        "http://accounts.google.com/signin",
        "https://accounts.google.com.example.net/signin",
        "https://evil.example/accounts.google.com/",
        "https://accounts.google.com/signin\"><script>",
        "https://accounts.google.com/sign in",
    ] {
        assert!(!valid_verification_url(hostile), "must reject {hostile}");
    }
    assert!(valid_verification_url(
        "https://accounts.google.com/signin/continue?sarp=1&scc=1"
    ));
    for message in [
        Failure::AccountValidationRequired.public_message(),
        Failure::AccountValidationRequired.fixed_proxy_message(),
    ] {
        assert!(
            message.contains("повторить"),
            "the seller still needs the exact command to resume after verifying"
        );
        assert!(
            !message.contains("Подожди немного"),
            "waiting never clears an account verification requirement"
        );
    }
    assert_eq!(
        Failure::AccountValidationRequired.code(),
        "account_validation_required"
    );
    assert_eq!(bounded_label(None), "<none>");
    assert_eq!(bounded_label(Some("")), "<empty>");
    assert_eq!(
        bounded_label(Some("PERMISSION\u{7}_DENIED")),
        "PERMISSION_DENIED"
    );
    assert_eq!(bounded_label(Some(&"a".repeat(200))).len(), 96);
}

#[test]
fn generation_acceptance_requires_a_wrapped_candidate_and_authoritative_usage() {
    let body = generation_probe_body(
        "managed-project",
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    );
    assert_eq!(body["model"], GENERATION_PROBE_MODEL);
    assert_eq!(body["project"], "managed-project");
    assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 8);
    assert_eq!(body["requestType"], "agent");
    assert!(body["requestId"].as_str().unwrap().starts_with("agent-"));

    let accepted = json!({
        "response": {
            "candidates": [{"content": {"parts": [{"text": "OK"}]}}],
            "usageMetadata": {
                "promptTokenCount": 4,
                "candidatesTokenCount": 1,
                "totalTokenCount": 5
            }
        }
    });
    assert!(
        validate_generation_probe_response(200, &serde_json::to_vec(&accepted).unwrap()).is_ok()
    );
    for (status, rejected) in [
        (503, accepted.clone()),
        (200, json!({"response": {"candidates": []}})),
        (
            200,
            json!({"response": {"candidates": [{}], "usageMetadata": {}}}),
        ),
        (200, json!({"notResponse": {}})),
    ] {
        assert_eq!(
            validate_generation_probe_response(status, &serde_json::to_vec(&rejected).unwrap()),
            Err(Failure::GenerationUnavailable)
        );
    }
    assert_eq!(
        validate_generation_probe_response(200, b"not-json"),
        Err(Failure::GenerationUnavailable)
    );
    assert!(
        validate_final_subject(OAuthPhase::AntigravityFinal, "same-subject", "same-subject")
            .is_ok()
    );
    assert_eq!(
        validate_final_subject(
            OAuthPhase::AntigravityFinal,
            "other-subject",
            "same-subject"
        ),
        Err(Failure::AccountMismatch)
    );
}

#[test]
fn plan_detection_distinguishes_supported_subscriptions() {
    assert_eq!(
        classify_plan(
            "g1-pro-tier",
            "Gemini Code Assist in Google One AI Pro",
            true
        ),
        "google_ai_pro"
    );
    assert_eq!(classify_plan("", "Google AI Pro", true), "google_ai_pro");
    assert_eq!(
        classify_plan("", "Google AI Ultra", true),
        "google_ai_ultra"
    );
    assert_eq!(
        classify_plan("standard-tier", "Code Assist Standard", false),
        "code_assist_standard"
    );
    assert_eq!(
        classify_plan("free-tier", "Individual", false),
        "individual_free"
    );
    assert_eq!(
        classify_plan("", "Google AI Plus", true),
        "google_ai_plus_unsupported"
    );
    assert_eq!(
        classify_plan("future-paid", "Future Paid", true),
        "unknown_paid_unsupported"
    );
    assert_eq!(
        classify_plan("future-pro", "Future Pro Trial", true),
        "unknown_paid_unsupported"
    );
    // An unreviewed id no longer suppresses an exact reviewed display name: Google introduces
    // new tier ids for the same product, and rejecting the pair blocked live subscriptions.
    assert_eq!(
        classify_plan(
            "future-pro-tier",
            "Gemini Code Assist in Google One AI Pro",
            true
        ),
        "google_ai_pro"
    );
    assert!(!supported_paid_plan("google_ai_plus_unsupported"));
    assert!(!supported_paid_plan("unknown_paid_unsupported"));
}

#[test]
fn reported_tier_prefers_reviewed_ids_and_the_paid_entitlement() {
    let pro = Tier {
        id: Some("g1-pro-tier".into()),
        name: Some("Gemini Code Assist in Google One AI Pro".into()),
        is_default: false,
    };
    let ultra = Tier {
        id: Some("g1-ultra-tier".into()),
        name: Some("Gemini Code Assist in Google One AI Ultra".into()),
        is_default: false,
    };
    let drifted_paid = Tier {
        id: Some("new-paid-shape".into()),
        name: Some("New Paid Shape".into()),
        is_default: false,
    };
    let renamed_pro = Tier {
        id: Some("g1-pro-tier".into()),
        name: Some("Google AI Pro renamed after purchase".into()),
        is_default: false,
    };
    let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
        paid_tier: Some(renamed_pro),
        ..LoadCodeAssistResponse::default()
    })
    .unwrap();
    assert_eq!(resolved.0, "g1-pro-tier");
    assert_eq!(resolved.2, "google_ai_pro");

    let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
        current_tier: Some(pro.clone()),
        paid_tier: Some(drifted_paid.clone()),
        ..LoadCodeAssistResponse::default()
    })
    .unwrap();
    assert_eq!(resolved.0, "g1-pro-tier");
    assert_eq!(resolved.2, "google_ai_pro");

    // A reviewed id survives a display name that maps to another reviewed product: the name is
    // marketing copy, the id is the stable contract.
    let conflicting_name = Tier {
        id: Some("g1-pro-tier".into()),
        name: Some("Google AI Ultra".into()),
        is_default: false,
    };
    let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
        paid_tier: Some(conflicting_name),
        ..LoadCodeAssistResponse::default()
    })
    .unwrap();
    assert_eq!(resolved.2, "google_ai_pro");

    // Antigravity onboarding can leave `currentTier` on another product while the account
    // really carries the paid one; the purchased entitlement decides.
    let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
        current_tier: Some(ultra),
        paid_tier: Some(pro),
        ..LoadCodeAssistResponse::default()
    })
    .unwrap();
    assert_eq!(resolved.0, "g1-pro-tier");
    assert_eq!(resolved.2, "google_ai_pro");

    // Nothing reviewed anywhere still fails closed.
    let unsupported = resolve_reported_tier(&LoadCodeAssistResponse {
        paid_tier: Some(drifted_paid),
        ..LoadCodeAssistResponse::default()
    })
    .unwrap();
    assert_eq!(unsupported.2, "unknown_paid_unsupported");
    assert!(!supported_paid_plan(&unsupported.2));
}

#[test]
fn unsupported_plan_diagnostic_is_structural_and_secret_free() {
    let raw_project = "private-project-123";
    let raw_tier_id = "private-future-tier";
    let raw_tier_name = "Private Future Plan";
    let loaded = LoadCodeAssistResponse {
        current_tier: Some(Tier {
            id: Some(raw_tier_id.into()),
            name: Some(raw_tier_name.into()),
            is_default: false,
        }),
        paid_tier: Some(Tier {
            id: Some("g1-pro-tier".into()),
            name: Some("Renamed paid display".into()),
            is_default: false,
        }),
        allowed_tiers: vec![Tier::default(), Tier::default()],
        cloudaicompanion_project: Some(json!(raw_project)),
    };
    let diagnostic = CodeAssistDiagnostic::from_response(&loaded).sanitized();
    assert_eq!(
        diagnostic,
        "project=present paid=known_id_name_drift current=unknown allowed_tiers=2"
    );
    for private_value in [
        raw_project,
        raw_tier_id,
        raw_tier_name,
        "Renamed paid display",
    ] {
        assert!(!diagnostic.contains(private_value));
    }
}

#[test]
fn disabled_cloud_code_api_is_actionable_instead_of_generic_auth_failure() {
    let detail = "Cloud Code Private API has not been used in project 123 before or it is disabled. Enable cloudcode-pa.googleapis.com then retry.";
    assert_eq!(
        classify_google_http_failure(403, detail),
        Failure::CodeAssistApiDisabled
    );
    assert!(Failure::CodeAssistApiDisabled
        .public_message()
        .contains("администратор проверит причину"));
    assert!(Failure::Temporary
        .fixed_proxy_message()
        .contains("менять его не нужно"));
    assert!(!Failure::Temporary
        .fixed_proxy_message()
        .contains("пришли прокси"));
    assert!(Failure::TransportUnavailable
        .fixed_proxy_message()
        .contains("CONNECT/TLS"));
    assert_ne!(
        Failure::TransportUnavailable.code(),
        Failure::Temporary.code()
    );
    for failure in [
        Failure::Authorization,
        Failure::CodeAssistApiDisabled,
        Failure::TransportUnavailable,
        Failure::Temporary,
        Failure::UnsupportedPlan,
        Failure::AccountMismatch,
        Failure::GenerationUnavailable,
        Failure::Duplicate,
        Failure::DuplicateProxy,
        Failure::MigrationProxyMismatch,
        Failure::Storage,
    ] {
        for internal_term in [
            "OAuth-клиент",
            "Cloud API",
            "consumer project",
            "managed project",
            "roster",
            "Client ID",
            "Client secret",
        ] {
            assert!(
                !failure.public_message().contains(internal_term),
                "seller error contains internal term {internal_term}"
            );
        }
    }
    assert_eq!(
        classify_google_http_failure(403, "permission denied"),
        Failure::Authorization
    );
    assert_eq!(
        classify_google_http_failure(500, detail),
        Failure::Temporary
    );
}

#[test]
fn legacy_preflight_blocks_antigravity_duplicates_before_the_second_consent() {
    let (root, ring) = fixture();
    publish(&root, &ring, "current", credential("existing-subject")).unwrap();
    assert_eq!(
        preflight_bootstrap_candidate(
            &root,
            &ring,
            "existing-subject",
            "http://user:pass@127.0.0.1:8080/",
            42,
        ),
        Err(Failure::Duplicate)
    );
    assert_eq!(
        preflight_bootstrap_candidate(
            &root,
            &ring,
            "different-subject",
            "http://user:pass@127.0.0.1:8080/",
            42,
        ),
        Err(Failure::DuplicateProxy)
    );
    let _ = fs::remove_dir_all(root);

    let (root, ring) = fixture();
    let mut legacy = credential("legacy-subject");
    legacy.oauth_client_id = LEGACY_CLIENT_ID.into();
    legacy.oauth_client_secret = LEGACY_CLIENT_SECRET.into();
    publish(&root, &ring, "current", legacy).unwrap();
    assert!(preflight_bootstrap_candidate(
        &root,
        &ring,
        "legacy-subject",
        "http://user:pass@127.0.0.1:8080/",
        42,
    )
    .is_ok());
    assert_eq!(
        preflight_bootstrap_candidate(
            &root,
            &ring,
            "legacy-subject",
            "http://user:pass@127.0.0.2:8080/",
            42,
        ),
        Err(Failure::MigrationProxyMismatch)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn publication_encrypts_identity_proxy_and_tokens_and_rejects_duplicates() {
    let (root, ring) = fixture();
    let first = publish(&root, &ring, "current", credential("subject-1")).unwrap();
    let roster = fs::read_to_string(root.join("profiles.json")).unwrap();
    assert!(!roster.contains("owner@example.com"));
    assert!(!roster.contains("user:pass"));
    assert!(!roster.contains("refresh-token"));
    let sealed =
        fs::read_to_string(root.join("credentials").join(format!("{}.json", first.id))).unwrap();
    assert!(!sealed.contains("owner@example.com"));
    assert!(!sealed.contains("refresh-token"));
    // Тот же subject через тот же прокси — переавторизация, а не дубликат: свежее согласие
    // Google уже аннулировало прежний refresh-токен, поэтому отказ оставил бы в roster
    // заведомо мёртвый credential.
    let reauthorized = publish(&root, &ring, "current", credential("subject-1")).unwrap();
    assert_eq!(reauthorized.id, first.id);
    assert!(reauthorized.reauthorized);
    assert!(!reauthorized.migrated);
    assert!(matches!(
        publish(&root, &ring, "current", credential("subject-2")),
        Err(Failure::DuplicateProxy)
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn publication_migrates_legacy_profile_in_place_to_antigravity() {
    let (root, ring) = fixture();
    let mut legacy = credential("migration-subject");
    legacy.oauth_client_id = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID.into();
    legacy.oauth_client_secret = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET.into();
    let published = publish(&root, &ring, "current", legacy).unwrap();
    assert!(!published.migrated);

    let roster_path = root.join("profiles.json");
    let roster_before = fs::read(&roster_path).unwrap();
    let credential_path = root
        .join("credentials")
        .join(format!("{}.json", published.id));

    let mut antigravity = credential("migration-subject");
    antigravity.proxy = "http://user:pass@127.0.0.1:8080/".into();
    antigravity.proxy_order_id = 0;
    antigravity.issued_at = 999;
    antigravity.access_token = "new-access-token-value".into();
    antigravity.refresh_token = "new-refresh-token-value".into();
    let migrated = publish(&root, &ring, "current", antigravity).unwrap();

    assert!(migrated.migrated);
    assert_eq!(migrated.id, published.id);
    assert_eq!(fs::read(&roster_path).unwrap(), roster_before);
    let envelope = decode_envelope(&fs::read(&credential_path).unwrap()).unwrap();
    let opened = ring.open(&migrated.id, &envelope).unwrap();
    assert_eq!(opened.oauth_kind().unwrap(), OAuthKind::Antigravity);
    assert_eq!(opened.proxy_order_id, 42);
    assert_eq!(opened.issued_at, 100);
    assert_eq!(opened.access_token, "new-access-token-value");
    assert_eq!(opened.refresh_token, "new-refresh-token-value");
    let _ = fs::remove_dir_all(root);
}

#[test]
/// Повторное согласие того же аккаунта заменяет материал на месте: id профиля, roster и
/// quota identity сохраняются, меняется только конверт. Отказ здесь оставлял бы подписку
/// мёртвой — Google аннулирует прежний refresh-токен ещё на экране согласия.
fn publication_reauthorizes_an_existing_antigravity_profile_in_place() {
    let (root, ring) = fixture();
    let published = publish(&root, &ring, "current", credential("antigravity-duplicate")).unwrap();
    let roster_path = root.join("profiles.json");
    let credential_path = root
        .join("credentials")
        .join(format!("{}.json", published.id));
    let roster_before = fs::read(&roster_path).unwrap();
    let credential_before = fs::read(&credential_path).unwrap();

    let mut duplicate = credential("antigravity-duplicate");
    duplicate.access_token = "replacement-access-token".into();
    duplicate.refresh_token = "replacement-refresh-token".into();
    duplicate.proxy_order_id = 0;
    duplicate.issued_at = 999;
    let reauthorized = publish(&root, &ring, "current", duplicate).unwrap();
    assert_eq!(reauthorized.id, published.id);
    assert!(reauthorized.reauthorized);
    // Roster не меняется: профиль тот же, подменён только запечатанный материал.
    assert_eq!(fs::read(&roster_path).unwrap(), roster_before);
    assert_ne!(fs::read(&credential_path).unwrap(), credential_before);
    let opened = ring
        .open(
            &reauthorized.id,
            &decode_envelope(&fs::read(&credential_path).unwrap()).unwrap(),
        )
        .unwrap();
    assert_eq!(opened.refresh_token, "replacement-refresh-token");
    assert_eq!(opened.proxy_order_id, 42);
    assert_eq!(opened.issued_at, 100);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn publication_rejects_legacy_migration_through_a_different_proxy() {
    let (root, ring) = fixture();
    let mut legacy = credential("proxy-mismatch-subject");
    legacy.oauth_client_id = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID.into();
    legacy.oauth_client_secret = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET.into();
    let published = publish(&root, &ring, "current", legacy).unwrap();
    let roster_path = root.join("profiles.json");
    let credential_path = root
        .join("credentials")
        .join(format!("{}.json", published.id));
    let roster_before = fs::read(&roster_path).unwrap();
    let credential_before = fs::read(&credential_path).unwrap();

    let mut migration = credential("proxy-mismatch-subject");
    migration.proxy = "http://user:pass@127.0.0.2:8080".into();
    assert!(matches!(
        publish(&root, &ring, "current", migration),
        Err(Failure::MigrationProxyMismatch)
    ));
    assert_eq!(fs::read(&roster_path).unwrap(), roster_before);
    assert_eq!(fs::read(&credential_path).unwrap(), credential_before);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn startup_rewrap_moves_existing_envelopes_to_the_active_key() {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let root = std::env::temp_dir().join(format!(
        "gemini-oauth-rewrap-{}-{}",
        std::process::id(),
        URL_SAFE_NO_PAD.encode(random)
    ));
    let ring = CredentialKeyring::parse(&format!(
        "current:{},old:{}",
        "77".repeat(32),
        "88".repeat(32)
    ))
    .unwrap();
    let profile = publish(&root, &ring, "old", credential("rotate-subject")).unwrap();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    config.rewrap_existing().unwrap();
    let path = root
        .join("credentials")
        .join(format!("{}.json", profile.id));
    let envelope = decode_envelope(&fs::read(path).unwrap()).unwrap();
    assert_eq!(envelope.key_id, "current");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn startup_prepares_a_private_empty_layout_for_the_runtime_mount() {
    let (root, ring) = fixture();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    config.rewrap_existing().unwrap();
    for directory in [&root, &root.join("credentials")] {
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    }
    assert!(!root.join("profiles.json").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lifecycle_profiles_include_external_and_same_order_different_ip_profiles() {
    let (root, ring) = fixture();
    let first = publish(&root, &ring, "current", credential("private-subject")).unwrap();

    let mut external = credential("external-subject");
    external.proxy = "http://user:pass@proxy.example:8080".into();
    external.proxy_order_id = 0;
    external.plan = "google_ai_ultra".into();
    external.tier_id = "g1-ultra-tier".into();
    external.tier_name = "Google AI Ultra".into();
    external.issued_at = 200;
    let external = publish(&root, &ring, "current", external).unwrap();

    let mut second = credential("second-managed-subject");
    second.proxy = "http://user:pass@[2001:db8::9]:8080".into();
    second.proxy_order_id = 43;
    second.issued_at = 300;
    let second = publish(&root, &ring, "current", second).unwrap();

    let mut managed_hostname = credential("managed-hostname-subject");
    managed_hostname.proxy = "http://user:pass@managed.example:8080".into();
    managed_hostname.proxy_order_id = 44;
    managed_hostname.issued_at = 400;
    let managed_hostname = publish(&root, &ring, "current", managed_hostname).unwrap();

    let second_path = root.join("credentials").join(format!("{}.json", second.id));
    let mut second_credential = ring
        .open(
            &second.id,
            &decode_envelope(&read_private(&second_path).unwrap()).unwrap(),
        )
        .unwrap();
    second_credential.proxy_order_id = 42;
    let second_envelope = ring
        .seal("current", &second.id, &second_credential)
        .unwrap();
    atomic_private_replace(&second_path, &encode_envelope(&second_envelope).unwrap()).unwrap();

    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let profiles = config.lifecycle_profiles().unwrap();
    assert_eq!(profiles.len(), 4);
    let first = profiles
        .iter()
        .find(|profile| profile.profile_id == first.id)
        .unwrap();
    assert_eq!(first.account_email, "owner@example.com");
    assert_eq!(first.order_id, 42);
    assert_eq!(first.issued_at, 100);
    assert_eq!(first.canonical_plan, "google_ai_pro");
    assert_eq!(first.canonical_ip, Some("127.0.0.1".parse().unwrap()));
    let external = profiles
        .iter()
        .find(|profile| profile.profile_id == external.id)
        .unwrap();
    assert_eq!(external.order_id, 0);
    assert_eq!(external.issued_at, 200);
    assert_eq!(external.canonical_plan, "google_ai_ultra");
    assert_eq!(external.canonical_ip, None);
    let second = profiles
        .iter()
        .find(|profile| profile.profile_id == second.id)
        .unwrap();
    assert_eq!(second.order_id, 42);
    assert_eq!(second.canonical_ip, Some("2001:db8::9".parse().unwrap()));
    let managed_hostname = profiles
        .iter()
        .find(|profile| profile.profile_id == managed_hostname.id)
        .unwrap();
    assert_eq!(managed_hostname.order_id, 44);
    assert_eq!(managed_hostname.canonical_ip, None);
    assert_eq!(config.iproyal_leases().unwrap().len(), 2);

    fn consume_without_formatting(
        profile: &LifecycleProfile,
    ) -> (&str, &str, i64, i64, &str, Option<std::net::IpAddr>) {
        let LifecycleProfile {
            profile_id,
            account_email,
            order_id,
            issued_at,
            canonical_plan,
            canonical_ip,
        } = profile;
        (
            profile_id,
            account_email,
            *order_id,
            *issued_at,
            canonical_plan,
            *canonical_ip,
        )
    }
    assert_eq!(consume_without_formatting(first).1, "owner@example.com");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lifecycle_reader_fails_closed_on_corruption_and_exact_binding_ambiguity() {
    let (root, ring) = fixture();
    let first = publish(&root, &ring, "current", credential("first-subject")).unwrap();
    let mut second = credential("second-subject");
    second.proxy = "http://user:pass@127.0.0.2:8080".into();
    second.proxy_order_id = 43;
    let second = publish(&root, &ring, "current", second).unwrap();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let roster_path = root.join("profiles.json");
    let original_roster = read_private(&roster_path).unwrap();
    let mut roster: serde_json::Value = serde_json::from_slice(&original_roster).unwrap();
    let duplicate = roster["profiles"][0].clone();
    roster["profiles"].as_array_mut().unwrap().push(duplicate);
    atomic_private_replace(&roster_path, &serde_json::to_vec(&roster).unwrap()).unwrap();
    assert!(config.lifecycle_profiles().is_err());
    atomic_private_replace(&roster_path, &original_roster).unwrap();

    let second_path = root.join("credentials").join(format!("{}.json", second.id));
    let mut ambiguous = config
        .keyring
        .open(
            &second.id,
            &decode_envelope(&read_private(&second_path).unwrap()).unwrap(),
        )
        .unwrap();
    ambiguous.proxy_order_id = 42;
    ambiguous.proxy = "http://other:secret@127.0.0.1:9000".into();
    let ambiguous = config
        .keyring
        .seal("current", &second.id, &ambiguous)
        .unwrap();
    atomic_private_replace(&second_path, &encode_envelope(&ambiguous).unwrap()).unwrap();
    assert!(config.lifecycle_profiles().is_err());
    assert!(root
        .join("credentials")
        .join(format!("{}.json", first.id))
        .exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn operator_proxy_replacement_is_atomic_secret_safe_and_reversible() {
    let (root, ring) = fixture();
    let published = publish(&root, &ring, "current", credential("replace-subject")).unwrap();
    let credential_path = root
        .join("credentials")
        .join(format!("{}.json", published.id));
    let rollback_path = proxy_rollback_path(&root.join("credentials"), &published.id);
    let original = fs::read(&credential_path).unwrap();
    let roster = fs::read(root.join("profiles.json")).unwrap();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();
    let replacement = "http://other:replacement-secret@127.0.0.2:9000";

    config
        .stage_proxy_replacement(&published.id, replacement)
        .unwrap();
    assert_eq!(fs::read(root.join("profiles.json")).unwrap(), roster);
    assert_eq!(fs::read(&rollback_path).unwrap(), original);
    assert_eq!(
        fs::symlink_metadata(&rollback_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let staged_bytes = fs::read(&credential_path).unwrap();
    let staged_text = String::from_utf8(staged_bytes.clone()).unwrap();
    assert!(!staged_text.contains("replacement-secret"));
    let staged = config
        .keyring
        .open(&published.id, &decode_envelope(&staged_bytes).unwrap())
        .unwrap();
    assert_eq!(
        staged.proxy,
        gemini_credential::normalize_proxy_url(replacement).unwrap()
    );
    assert_eq!(staged.proxy_order_id, 0);
    assert!(config
        .stage_proxy_replacement(&published.id, replacement)
        .is_err());

    config.rollback_proxy_replacement(&published.id).unwrap();
    assert_eq!(fs::read(&credential_path).unwrap(), original);
    assert!(!rollback_path.exists());

    config
        .stage_proxy_replacement(&published.id, replacement)
        .unwrap();
    let committed = fs::read(&credential_path).unwrap();
    config.commit_proxy_replacement(&published.id).unwrap();
    assert_eq!(fs::read(&credential_path).unwrap(), committed);
    assert!(!rollback_path.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn operator_proxy_replacement_rejects_another_profiles_egress() {
    let (root, ring) = fixture();
    let first = publish(&root, &ring, "current", credential("replace-first")).unwrap();
    let mut second_credential = credential("replace-second");
    second_credential.proxy = "http://second:secret@127.0.0.2:9000".into();
    second_credential.proxy_order_id = 43;
    let second = publish(&root, &ring, "current", second_credential).unwrap();
    let first_path = root.join("credentials").join(format!("{}.json", first.id));
    let before = fs::read(&first_path).unwrap();
    let config = Config::new(
        "https://gemini.example/oauth/callback".into(),
        "127.0.0.1:8796".parse().unwrap(),
        root.to_string_lossy().into_owned(),
        ring,
        "current".into(),
    )
    .unwrap();

    assert!(config
        .stage_proxy_replacement(&first.id, "http://second:secret@127.0.0.2:9000")
        .is_err());
    assert_eq!(fs::read(first_path).unwrap(), before);
    assert!(!proxy_rollback_path(&root.join("credentials"), &first.id).exists());
    assert!(root
        .join("credentials")
        .join(format!("{}.json", second.id))
        .exists());
    let _ = fs::remove_dir_all(root);
}
