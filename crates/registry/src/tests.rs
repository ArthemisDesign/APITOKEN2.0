use super::*;

fn db() -> Connection {
    open(":memory:").unwrap()
}

/// The roster-backed plane is a closed set. Anthropic is the one that matters: Claude
/// subscriptions already carry `active|paused|disabled`, so letting them through here would give
/// a single subscription two switches that can disagree.
#[test]
fn only_roster_backed_fleets_accept_an_operator_disable() {
    for provider in ROSTER_BACKED_PROVIDERS {
        require_roster_backed_provider(provider).unwrap();
    }
    assert!(require_roster_backed_provider(PROVIDER_ANTHROPIC).is_err());
    assert!(require_roster_backed_provider("").is_err());
    assert!(require_roster_backed_provider("gemini").is_err());
    assert!(require_roster_backed_provider("codex").is_err());
}

/// Персист состояния пула: save→load переносит cooling/калибровку (upsert по email).
#[test]
fn pool_state_save_load_roundtrip() {
    let c = db();
    let rows = vec![PoolStateRow {
        email: "a@x.io".into(),
        cooling_until: 123456,
        cap5h_usd: 50.0,
        cap7d_usd: 1500.0,
        spent_total_usd: 12.5,
        util5h: 0.3,
        util7d: 0.1,
        reset5h: 999,
        reset7d: 888,
        calib_n: 4,
        version: 0,
        spent_delta_usd: 0.0,
    }];
    save_pool_state(&c, &rows).unwrap();
    // повторный save (upsert) не дублирует и обновляет
    let mut r2 = rows.clone();
    r2[0].cooling_until = 222222;
    save_pool_state(&c, &r2).unwrap();
    let got = load_pool_state(&c).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].email, "a@x.io");
    assert_eq!(got[0].cooling_until, 222222);
    assert!((got[0].cap5h_usd - 50.0).abs() < 1e-9);
    assert_eq!(got[0].calib_n, 4);
}

#[test]
fn anthropic_calibration_schema_is_exact_plan_scoped_and_prior_free() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'provider_turn_calibration_events','provider_calibration_subject_spend', \
               'anthropic_window_calibrations','anthropic_window_observations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 4);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('anthropic_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| {
        let name = name.to_ascii_lowercase();
        name.contains("prior") || name.contains("ema") || name.contains("nominal")
    }));
    for name in [
        "plan",
        "window_kind",
        "anchor_used_fraction_units",
        "anchor_resolution_fraction_units",
        "observed_fraction_units",
        "observed_spend_nano",
        "unattributed_fraction_units",
        "current_capacity_nano",
        "current_low_nano",
        "current_high_nano",
        "current_confidence_bp",
    ] {
        assert!(columns.contains(&name.to_owned()), "missing {name}");
    }

    let turn_columns = c
        .prepare("SELECT name FROM pragma_table_info('provider_turn_calibration_events')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for name in [
        "subject_id",
        "model_id",
        "service_tier",
        "inference_geo",
        "cache_read_tokens",
        "cache_write_5m_tokens",
        "cache_write_1h_tokens",
        "thinking_output_tokens",
        "search_queries",
        "api_total_nanousd",
    ] {
        assert!(turn_columns.contains(&name.to_owned()), "missing {name}");
    }

    let insert_state = "INSERT INTO anthropic_window_calibrations( \
        subject_id,plan,window_kind,window_duration_mins,resets_at, \
        anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano, \
        used_fraction_units,measurement_resolution_fraction_units,observed_at,updated_ts) \
        VALUES(?1,?2,?3,?4,2000000000,?5,?6,0,?5,?6,100,100)";
    c.execute(
        insert_state,
        rusqlite::params!["sub-a", "max20", "5h", 300, 12_345_000, 1_000],
    )
    .unwrap();
    c.execute(
        insert_state,
        rusqlite::params!["sub-a", "max20", "7d", 10_080, 40_000_000, 1_000_000],
    )
    .unwrap();
    // Correcting the durable plan creates a separate cold identity instead of mutating the
    // Max cohort's evidence.
    c.execute(
        insert_state,
        rusqlite::params!["sub-a", "pro", "5h", 300, 12_345_000, 1_000],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_state,
            rusqlite::params!["sub-b", "max20", "7d", 300, 1, 1],
        )
        .is_err());

    let insert_observation = "INSERT INTO anthropic_window_observations( \
        subject_id,plan,window_kind,window_duration_mins,resets_at,observed_at, \
        used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano, \
        observation_source,source_request_id) \
        VALUES(?1,'max20','5h',300,2000000000,?2,?3,?4,?5,?6,?7)";
    c.execute(
        insert_observation,
        rusqlite::params![
            "sub-a",
            101,
            12_345_000,
            1_000,
            42_000_000,
            "response",
            "cal-request-a"
        ],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params![
                "sub-a",
                102,
                13_000_000,
                1_000_000,
                43_000_000,
                "response",
                "cal-request-a"
            ],
        )
        .is_err());
    c.execute(
        insert_observation,
        rusqlite::params![
            "sub-a",
            103,
            13_000_000,
            1_000_000,
            43_000_000,
            "poll",
            Option::<String>::None
        ],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params!["sub-b", 104, 1, 1, 0, "response", Option::<String>::None],
        )
        .is_err());
}

#[test]
fn codex_calibration_schema_has_no_capacity_prior() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'codex_home_spend','codex_window_calibrations','codex_window_observations', \
               'codex_turn_calibration_events')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 4);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('codex_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| name.contains("prior")));
    assert!(columns.contains(&"window_duration_mins".to_owned()));
    assert!(columns.contains(&"resets_at".to_owned()));
    assert!(columns.contains(&"anchor_ready".to_owned()));
    for name in [
        "anchor_spend_nanocredits",
        "observed_spend_nanocredits",
        "current_capacity_nanocredits",
        "credit_samples",
        "unattributed_fraction_units",
    ] {
        assert!(columns.contains(&name.to_owned()), "missing {name}");
    }

    let turn_columns = c
        .prepare("SELECT name FROM pragma_table_info('codex_turn_calibration_events')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for name in [
        "home_id",
        "model_id",
        "service_tier",
        "cached_input_tokens",
        "cache_write_input_tokens",
        "reasoning_output_tokens",
        "api_total_nanousd",
        "chatgpt_total_nanocredits",
    ] {
        assert!(turn_columns.contains(&name.to_owned()), "missing {name}");
    }
}

#[test]
fn gemini_calibration_schema_has_exact_two_window_contract_and_no_prior() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'gemini_profile_spend','gemini_window_calibrations',\
               'gemini_window_observations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 3);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('gemini_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| name.contains("prior")));
    assert!(columns.contains(&"bucket_id".to_owned()));
    assert!(columns.contains(&"window_kind".to_owned()));
    assert!(columns.contains(&"anchor_used_fraction_units".to_owned()));
    assert!(columns.contains(&"sum_used_sq".to_owned()));
    assert!(columns.contains(&"observed_spend_nano".to_owned()));

    let insert = "INSERT INTO gemini_window_calibrations( \
        profile_id,bucket_id,window_kind,window_duration_mins,resets_at, \
        anchor_used_fraction_units,anchor_spend_nano,used_fraction_units,observed_at, \
        updated_ts) VALUES(?1,?2,?3,?4,200,?5,0,?5,100,100)";
    c.execute(
        insert,
        rusqlite::params!["profile-a", "gemini-5h", "5h", 300, 12_345],
    )
    .unwrap();
    c.execute(
        insert,
        rusqlite::params!["profile-a", "gemini-weekly", "weekly", 10_080, 67_890],
    )
    .unwrap();
    assert!(c
        .execute(
            insert,
            rusqlite::params!["profile-b", "gemini-daily", "5h", 300, 1],
        )
        .is_err());
    assert!(c
        .execute(
            insert,
            rusqlite::params!["profile-b", "gemini-5h", "5h", 300, 100_000_001],
        )
        .is_err());
}

#[test]
fn gemini_exact_calibration_schema_is_plan_scoped_and_replayable() {
    let c = db();
    let tables: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ( \
               'gemini_exact_window_calibrations','gemini_exact_window_observations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 2);

    let columns = c
        .prepare("SELECT name FROM pragma_table_info('gemini_exact_window_calibrations')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(!columns.iter().any(|name| {
        let name = name.to_ascii_lowercase();
        name.contains("prior") || name.contains("ema") || name.contains("nominal")
    }));
    for name in [
        "plan",
        "bucket_id",
        "anchor_resolution_fraction_units",
        "measurement_resolution_fraction_units",
        "observed_fraction_units",
        "observed_spend_nano",
        "unattributed_fraction_units",
        "current_capacity_nano",
        "current_low_nano",
        "current_high_nano",
    ] {
        assert!(columns.contains(&name.to_owned()), "missing {name}");
    }

    let insert_state = "INSERT INTO gemini_exact_window_calibrations( \
        profile_id,plan,bucket_id,window_kind,window_duration_mins,resets_at, \
        anchor_used_fraction_units,anchor_resolution_fraction_units,anchor_spend_nano, \
        used_fraction_units,measurement_resolution_fraction_units,observed_at,updated_ts) \
        VALUES(?1,?2,?3,?4,?5,2000000000,?6,?7,0,?6,?7,100,100)";
    c.execute(
        insert_state,
        rusqlite::params!["profile-a", "google_ai_pro", "gemini-5h", "5h", 300, 10, 1],
    )
    .unwrap();
    c.execute(
        insert_state,
        rusqlite::params![
            "profile-a",
            "google_ai_ultra",
            "gemini-5h",
            "5h",
            300,
            10,
            1
        ],
    )
    .unwrap();
    c.execute(
        insert_state,
        rusqlite::params![
            "profile-a",
            "google_ai_pro",
            "gemini-weekly",
            "weekly",
            10_080,
            20,
            1_000_000
        ],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_state,
            rusqlite::params![
                "profile-b",
                "google_ai_pro",
                "gemini-weekly",
                "weekly",
                300,
                1,
                1
            ],
        )
        .is_err());

    let insert_observation = "INSERT INTO gemini_exact_window_observations( \
        profile_id,plan,bucket_id,window_kind,window_duration_mins,resets_at,observed_at, \
        used_fraction_units,measurement_resolution_fraction_units,gateway_spend_nano, \
        observation_source,source_request_id) \
        VALUES('profile-a','google_ai_pro','gemini-5h','5h',300,2000000000,?1, \
            ?2,?3,?4,?5,?6)";
    c.execute(
        insert_observation,
        rusqlite::params![101, 10, 1, 1_000, "poll", Option::<String>::None],
    )
    .unwrap();
    c.execute(
        insert_observation,
        rusqlite::params![102, 20, 10, 2_000, "response", "gemini-cal-a"],
    )
    .unwrap();
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params![103, 30, 10, 3_000, "response", "gemini-cal-a"],
        )
        .is_err());
    assert!(c
        .execute(
            insert_observation,
            rusqlite::params![104, 40, 10, 4_000, "response", Option::<String>::None],
        )
        .is_err());
}

#[test]
fn gemini_spend_and_calibration_are_exact_durable_and_cas_versioned() {
    let c = db();
    assert_eq!(gemini_profile_spend(&c, "profile-a").unwrap(), 0);
    assert_eq!(
        credit_gemini_profile_spend(&c, "profile-a", 19_404_000, 100).unwrap(),
        19_404_000
    );
    assert_eq!(
        credit_gemini_profile_spend(&c, "profile-a", 1, 101).unwrap(),
        19_404_001
    );

    let mut state = GeminiCalibrationRow {
        profile_id: "profile-a".to_string(),
        bucket_id: "gemini-5h".to_string(),
        window_kind: "5h".to_string(),
        window_duration_mins: 300,
        resets_at: 2_000_000_000,
        anchor_used_fraction_units: 1_970,
        anchor_spend_nano: 0,
        anchor_ready: false,
        used_fraction_units: 1_970,
        observed_at: 100,
        sum_used_sq: "170141183460469231731687303715884105727".to_string(),
        sum_used_spend_nano: "0".to_string(),
        observed_fraction_units: 0,
        observed_spend_nano: 12_345,
        samples: 0,
        current_capacity_nano: None,
        current_low_nano: None,
        current_high_nano: None,
        current_confidence_bp: 0,
        last_measured_at: None,
        estimator_version: 1,
        version: 0,
        updated_ts: 100,
    };
    let observation = GeminiWindowObservation {
        profile_id: state.profile_id.clone(),
        bucket_id: state.bucket_id.clone(),
        window_kind: state.window_kind.clone(),
        window_duration_mins: state.window_duration_mins,
        resets_at: state.resets_at,
        observed_at: state.observed_at,
        used_fraction_units: state.used_fraction_units,
        gateway_spend_nano: 19_404_001,
    };
    assert_eq!(
        save_gemini_calibration(&c, &state, &observation).unwrap(),
        Some(1)
    );
    assert_eq!(
        save_gemini_calibration(&c, &state, &observation).unwrap(),
        None
    );
    state = load_gemini_calibration(&c, "profile-a", "gemini-5h")
        .unwrap()
        .unwrap();
    assert_eq!(state.version, 1);
    assert_eq!(state.sum_used_sq, i128::MAX.to_string());
    assert_eq!(state.observed_spend_nano, 12_345);
    assert_eq!(
        load_gemini_window_observations(&c, "profile-a", "gemini-5h").unwrap(),
        vec![observation]
    );

    let mismatched = GeminiWindowObservation {
        profile_id: "profile-b".to_string(),
        observed_at: 101,
        ..load_gemini_window_observations(&c, "profile-a", "gemini-5h")
            .unwrap()
            .pop()
            .unwrap()
    };
    assert!(save_gemini_calibration(&c, &state, &mismatched).is_err());

    state.sum_used_sq = "01".to_string();
    assert!(save_gemini_calibration(
        &c,
        &state,
        &GeminiWindowObservation {
            observed_at: 101,
            ..load_gemini_window_observations(&c, "profile-a", "gemini-5h")
                .unwrap()
                .pop()
                .unwrap()
        }
    )
    .is_err());
}

#[test]
fn codex_home_health_defaults_to_healthy_and_round_trips() {
    let c = db();
    // Absence of evidence is not evidence of a fault: an unknown home starts routable.
    assert_eq!(
        load_codex_home_health(&c, "home-new").unwrap(),
        CodexHomeHealthRow::default()
    );

    let dead = CodexHomeHealthRow {
        account_state: "dead".to_string(),
        auth_fail_streak: 2,
        first_auth_fail_ts: 1_000,
        cooling_until: 1_900,
    };
    save_codex_home_health(&c, "home-a", &dead, 2_000).unwrap();
    // The verdict a restart used to discard now survives it, which is the whole point: a
    // corroborated dead subscription must not be re-admitted by every blue-green handoff.
    assert_eq!(load_codex_home_health(&c, "home-a").unwrap(), dead);

    let repaired = CodexHomeHealthRow::default();
    save_codex_home_health(&c, "home-a", &repaired, 2_100).unwrap();
    assert_eq!(load_codex_home_health(&c, "home-a").unwrap(), repaired);
    // Homes are independent: one dead subscription never taints its neighbours.
    assert_eq!(
        load_codex_home_health(&c, "home-b").unwrap(),
        CodexHomeHealthRow::default()
    );
}

#[test]
fn codex_spend_and_calibration_are_durable_and_cas_versioned() {
    let c = db();
    assert_eq!(
        codex_home_calibration_spend(&c, "home-a").unwrap(),
        CodexHomeCalibrationSpend::default()
    );
    let event = |request_id: &str,
                 api_total_nanousd: i64,
                 chatgpt_total_nanocredits: i64,
                 completed_at: i64| {
        CodexTurnCalibrationEvent {
            request_id: request_id.into(),
            home_id: "home-a".into(),
            model_id: "gpt-5.6-terra".into(),
            service_tier: "standard".into(),
            provider_reported_tier: Some("default".into()),
            api_tariff_schedule_id: "openai/test/v1".into(),
            credit_schedule_id: "chatgpt/test/v1".into(),
            completed_at,
            input_tokens: 1,
            cached_input_tokens: 0,
            cache_write_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            api_input_nanousd: api_total_nanousd,
            api_cached_input_nanousd: 0,
            api_cache_write_nanousd: 0,
            api_output_nanousd: 0,
            api_total_nanousd,
            chatgpt_input_nanocredits: chatgpt_total_nanocredits,
            chatgpt_cached_input_nanocredits: 0,
            chatgpt_output_nanocredits: 0,
            chatgpt_total_nanocredits,
        }
    };
    let first = event("request-1", 40_000_000_000, 4_000_000_000, 100);
    let totals = record_codex_turn_calibration_event(&c, &first).unwrap();
    assert!(totals.inserted);
    assert_eq!(totals.spent_nano, 40_000_000_000);
    assert_eq!(totals.spent_nanocredits, Some(4_000_000_000));
    assert_eq!(totals.credit_tracking_started_ts, Some(100));
    assert!(
        !record_codex_turn_calibration_event(&c, &first)
            .unwrap()
            .inserted
    );
    let mut conflict = first.clone();
    conflict.api_input_nanousd += 1;
    conflict.api_total_nanousd += 1;
    let conflict_error = record_codex_turn_calibration_event(&c, &conflict).unwrap_err();
    assert!(is_codex_turn_calibration_replay_conflict(&conflict_error));
    let totals = record_codex_turn_calibration_event(
        &c,
        &event("request-2", 60_000_000_000, 6_000_000_000, 101),
    )
    .unwrap();
    assert_eq!(totals.spent_nano, 100_000_000_000);
    assert_eq!(totals.spent_nanocredits, Some(10_000_000_000));
    assert_eq!(
        totals.credit_tracking_started_ts,
        Some(100),
        "tracking start stays pinned to the first immutable event"
    );
    let report = codex_turn_calibration_report(&c).unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].turns, 2);
    assert_eq!(report[0].api_total_nanousd, 100_000_000_000);
    assert_eq!(report[0].chatgpt_total_nanocredits, 10_000_000_000);

    let mut state = CodexCalibrationRow {
        home_id: "home-a".into(),
        window_duration_mins: 300,
        resets_at: 2_000_000_000,
        anchor_used_percent: 10,
        anchor_used_fraction_units: 10_000_000,
        anchor_spend_nano: 100_000_000_000,
        used_percent: 10,
        used_fraction_units: 10_000_000,
        observed_at: 101,
        sum_used_sq: 0,
        sum_used_spend_nano: 0,
        observed_points: 0,
        observed_fraction_units: 0,
        observed_spend_nano: 0,
        anchor_spend_nanocredits: Some(10_000_000_000),
        observed_spend_nanocredits: Some(0),
        current_capacity_nanocredits: None,
        current_low_nanocredits: None,
        current_high_nanocredits: None,
        last_capacity_nanocredits: None,
        last_low_nanocredits: None,
        last_high_nanocredits: None,
        credit_samples: Some(0),
        credit_estimator_version: Some(1),
        unattributed_fraction_units: Some(0),
        samples: 0,
        current_capacity_nano: None,
        current_low_nano: None,
        current_high_nano: None,
        current_confidence_bp: 0,
        last_capacity_nano: None,
        last_low_nano: None,
        last_high_nano: None,
        last_confidence_bp: 0,
        last_measured_at: None,
        anchor_ready: false,
        estimator_version: 1,
        version: 0,
        updated_ts: 101,
    };
    let observation = CodexWindowObservation {
        home_id: "home-a".into(),
        window_duration_mins: 300,
        resets_at: 2_000_000_000,
        observed_at: 101,
        used_percent: 10,
        used_fraction_units: 10_000_000,
        gateway_spend_nano: 100_000_000_000,
        gateway_spend_nanocredits: Some(10_000_000_000),
    };
    assert_eq!(
        save_codex_calibration(&c, &state, &observation).unwrap(),
        Some(1)
    );
    assert_eq!(
        save_codex_calibration(&c, &state, &observation).unwrap(),
        None,
        "a second absent-row derivation must lose CAS"
    );

    state = load_codex_calibration(&c, "home-a", 300).unwrap().unwrap();
    assert_eq!(state.version, 1);
    assert!(!state.anchor_ready);
    state.used_percent = 11;
    state.used_fraction_units = 11_000_000;
    state.observed_at = 102;
    state.updated_ts = 102;
    let mut second = observation.clone();
    second.used_percent = 11;
    second.used_fraction_units = 11_000_000;
    second.observed_at = 102;
    assert_eq!(
        save_codex_calibration(&c, &state, &second).unwrap(),
        Some(2)
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM codex_window_observations WHERE home_id='home-a'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        2
    );
    let observations = load_codex_window_observations(&c, "home-a", 300).unwrap();
    assert_eq!(
        observations
            .iter()
            .map(|row| row.observed_at)
            .collect::<Vec<_>>(),
        vec![101, 102]
    );
}

// хелпер: аккаунт с балансом + ключ под ним (ref=None — админ-сид, не платёж, без дедупа)
fn acct_with_key(c: &Connection, acct: &str, key: &str, usd_nano: i64, mult: i64) {
    account_create(c, acct, None, mult).unwrap();
    account_topup(c, acct, usd_nano, None).unwrap();
    key_issue(c, key, acct, None).unwrap();
}

#[test]
fn authoritative_database_uses_full_synchronous_durability() {
    let c = db();
    let synchronous: i64 = c.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
    assert_eq!(synchronous, 2); // SQLite FULL
}

#[test]
fn open_fails_closed_when_legacy_topup_references_are_duplicated() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "registry-duplicate-ref-{}-{unique}.db",
        std::process::id()
    ));
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE ledger(id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, \
             key TEXT, kind TEXT NOT NULL, amount_nano INTEGER NOT NULL, ref TEXT, \
             balance_after_nano INTEGER, ts INTEGER, model TEXT); \
             INSERT INTO ledger(account_id,kind,amount_nano,ref) VALUES('a','topup',1,'dup'); \
             INSERT INTO ledger(account_id,kind,amount_nano,ref) VALUES('a','topup',1,'dup');",
        )
        .unwrap();
    }
    assert!(open(path.to_str().unwrap()).is_err());
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
}

#[test]
fn legacy_keys_with_same_suffix_migrate_to_distinct_accounts() {
    let c = db();
    c.execute(
        "INSERT INTO api_keys(key,key_id,balance_nano,spent_nano,mult_bp,status,reserved_nano) \
         VALUES(?1,?2,?3,?4,?5,'active',0)",
        rusqlite::params!["sk-user-a-123456789abc", "legacy_a", 100, 10, 2000],
    )
    .unwrap();
    c.execute(
        "INSERT INTO api_keys(key,key_id,balance_nano,spent_nano,mult_bp,status,reserved_nano) \
         VALUES(?1,?2,?3,?4,?5,'active',0)",
        rusqlite::params!["sk-user-b-123456789abc", "legacy_b", 200, 20, 3000],
    )
    .unwrap();
    migrate_legacy_keys(&c).unwrap();
    let a = key_get(&c, "sk-user-a-123456789abc")
        .unwrap()
        .unwrap()
        .account_id
        .unwrap();
    let b = key_get(&c, "sk-user-b-123456789abc")
        .unwrap()
        .unwrap()
        .account_id
        .unwrap();
    assert_ne!(a, b);
    assert_eq!(account_get(&c, &a).unwrap().unwrap().balance_nano, 100);
    assert_eq!(account_get(&c, &b).unwrap().unwrap().balance_nano, 200);
}

/// Агрегаты трат (для /metrics): суммы по аккаунтам + число активных.
#[test]
fn billing_totals_aggregates_across_accounts() {
    let c = db();
    acct_with_key(&c, "acct_1", "sk-1", 5_000_000_000, 10000); // $5
    acct_with_key(&c, "acct_2", "sk-2", 3_000_000_000, 10000); // $3
    account_reserve(&c, "acct_1", 1_000_000_000).unwrap();
    account_settle(&c, "acct_1", "sk-1", 1_000_000_000, 400_000_000, None, None).unwrap(); // spent $0.4
    account_reserve(&c, "acct_2", 500_000_000).unwrap(); // висящий резерв $0.5
    account_set_status(&c, "acct_2", "disabled").unwrap();
    let t = billing_totals(&c).unwrap();
    assert_eq!(t.balance_nano, 4_600_000_000 + 2_500_000_000); // $4.6 + $2.5
    assert_eq!(t.spent_nano, 400_000_000);
    assert_eq!(t.reserved_nano, 500_000_000);
    assert_eq!(t.active_accounts, 1);
}

/// Без per-request identity старт не может доказать, что резерв осиротел: fail-closed оставляет hold.
#[test]
fn reconcile_does_not_refund_unowned_aggregate_reservations() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 2000);
    account_reserve(&c, "a", 600_000_000).unwrap();
    assert_eq!(reconcile_reservations(&c).unwrap(), 0);
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, 400_000_000);
    assert_eq!(acc.reserved_nano, 600_000_000);
}

/// reserve атомарно гейтит по общему account floor; settle сводит пару к −actual; per-key spent + ledger.
#[test]
fn reserve_gates_and_settle_nets_to_actual() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 2000); // $1.00
    assert_eq!(
        account_reserve(&c, "a", 600_000_000).unwrap(),
        Some(400_000_000)
    );
    assert_eq!(account_reserve(&c, "a", 1_500_000_000).unwrap(), None); // post-balance −$1.10 → отказ
    assert_eq!(
        account_settle(&c, "a", "k", 600_000_000, 100_000_000, Some("req1"), None).unwrap(),
        Some(900_000_000)
    );
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, 900_000_000);
    assert_eq!(acc.spent_nano, 100_000_000);
    // per-key атрибуция: spent по ключу тоже $0.10
    assert_eq!(key_get(&c, "k").unwrap().unwrap().spent_nano, 100_000_000);
    // ledger: строка topup ($1) + строка charge ($0.10)
    let cnt: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM ledger WHERE account_id='a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cnt, 2);
}

/// Exact provider usage is never silently clamped to the estimate held before delivery.
#[test]
fn settle_records_exact_actual_above_hold() {
    let c = db();
    acct_with_key(&c, "a", "k", 100, 2000);
    assert_eq!(account_reserve(&c, "a", 100).unwrap(), Some(0));
    assert_eq!(
        account_settle(&c, "a", "k", 100, 150, Some("req"), None).unwrap(),
        Some(-50)
    );
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, -50);
    assert_eq!(acc.spent_nano, 150);
    assert_eq!(acc.reserved_nano, 0);
    assert_eq!(key_get(&c, "k").unwrap().unwrap().spent_nano, 150);
    let ledger = ledger_recent(&c, "a", 10).unwrap();
    assert_eq!(ledger[0].amount_nano, 150);
    assert_eq!(ledger[0].uncollected_nano, 0);
    c.execute(
        "UPDATE ledger SET uncollected_nano=7 WHERE kind='charge' AND account_id='a'",
        [],
    )
    .unwrap();
    let ledger = ledger_recent(&c, "a", 10).unwrap();
    assert_eq!(ledger[0].amount_nano, 150, "billed amount stays unchanged");
    assert_eq!(ledger[0].uncollected_nano, 7);
}

#[test]
fn sqlite_settlements_share_one_floor_and_preserve_full_billed_usage() {
    let c = db();
    acct_with_key(&c, "floor-account", "floor-key", 700_000_000, 5_000);
    let pricing = ReservationPricing::new(PROVIDER_OPENAI, 5_000).unwrap();
    let execution = ExecutionAttempt::direct();
    for (request_id, hold, expected_balance) in [
        ("floor-1", 200_000_000, 500_000_000),
        ("floor-2", 200_000_000, 300_000_000),
        ("floor-3", 300_000_000, 0),
    ] {
        assert_eq!(
            sqlite_reserve_priced_request_for_execution(
                &c,
                request_id,
                "floor-account",
                "floor-key",
                hold,
                60,
                &execution,
                &pricing,
            )
            .unwrap(),
            Some(expected_balance),
        );
    }

    let usages = [
        ("floor-1", 200_000_000, 550_000_000),
        ("floor-2", 200_000_000, 600_000_000),
        ("floor-3", 300_000_000, 700_000_000),
    ]
    .map(|(request_id, hold, actual)| {
        (
            request_id,
            hold,
            actual,
            UsageEventInput {
                model: "gpt-floor-test".into(),
                provider: PROVIDER_OPENAI.into(),
                real_nano: actual * 2,
                charge_basis_nano: actual * 2,
                ..Default::default()
            },
        )
    });
    assert_eq!(
        sqlite_settle_request(
            &c,
            usages[0].0,
            "floor-account",
            "floor-key",
            usages[0].1,
            usages[0].2,
            Some("floor-ref-1"),
            Some(&usages[0].3),
        )
        .unwrap(),
        Some(-350_000_000),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            usages[1].0,
            "floor-account",
            "floor-key",
            usages[1].1,
            usages[1].2,
            Some("floor-ref-2"),
            Some(&usages[1].3),
        )
        .unwrap(),
        Some(-750_000_000),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            usages[2].0,
            "floor-account",
            "floor-key",
            usages[2].1,
            usages[2].2,
            Some("floor-ref-3"),
            Some(&usages[2].3),
        )
        .unwrap(),
        Some(-1_000_000_000),
    );

    let account = c
        .query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano \
             FROM accounts WHERE id='floor-account'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(account, (-1_000_000_000, 1_850_000_000, 0, 150_000_000));
    assert_eq!(
        account.0 + account.1 + account.2 - account.3,
        700_000_000,
        "funding must equal balance + full spend + holds - pool-funded shortfall",
    );
    assert_eq!(
        key_get(&c, "floor-key").unwrap().unwrap().spent_nano,
        1_850_000_000,
    );
    assert_eq!(
        c.query_row(
            "SELECT COALESCE(SUM(actual_nano),0),COALESCE(SUM(collected_nano),0), \
                    COALESCE(SUM(uncollected_nano),0),COUNT(DISTINCT provider), \
                    MIN(payable_multiplier_bp),MAX(payable_multiplier_bp) \
             FROM billing_reservations WHERE request_id LIKE 'floor-%'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            )),
        )
        .unwrap(),
        (1_850_000_000, 1_700_000_000, 150_000_000, 1, 5_000, 5_000),
    );
    assert_eq!(
        c.query_row(
            "SELECT COALESCE(SUM(amount_nano),0),COALESCE(SUM(uncollected_nano),0), \
                    COALESCE(SUM(official_nano),0),COUNT(*) \
             FROM ledger WHERE kind='charge' AND request_id LIKE 'floor-%'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            )),
        )
        .unwrap(),
        (1_850_000_000, 150_000_000, 3_700_000_000, 3),
    );
    assert_eq!(
        c.query_row(
            "SELECT COALESCE(SUM(charge_nano),0),COALESCE(SUM(uncollected_nano),0), \
                    COALESCE(SUM(charge_basis_nano),0),COUNT(*) \
             FROM usage_events WHERE request_id LIKE 'floor-%'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            )),
        )
        .unwrap(),
        (1_850_000_000, 150_000_000, 3_700_000_000, 3),
    );

    // Exact terminal replay returns the durable result without duplicating either the customer
    // charge or the explicit shortfall.
    assert_eq!(
        sqlite_settle_request(
            &c,
            usages[2].0,
            "floor-account",
            "floor-key",
            usages[2].1,
            usages[2].2,
            Some("floor-ref-3"),
            Some(&usages[2].3),
        )
        .unwrap(),
        Some(-1_000_000_000),
    );
    assert_eq!(
        c.query_row(
            "SELECT uncollected_nano FROM accounts WHERE id='floor-account'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        150_000_000,
    );
    assert!(c
        .execute(
            "UPDATE billing_reservations SET collected_nano=NULL WHERE request_id='floor-3'",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "UPDATE accounts SET uncollected_nano=-1 WHERE id='floor-account'",
            [],
        )
        .is_err());
    assert!(c
        .execute(
            "UPDATE usage_events SET uncollected_nano=charge_nano+1 \
             WHERE request_id='floor-3'",
            [],
        )
        .is_err());
}

#[test]
fn sqlite_settlement_preserves_preexisting_adjustment_debt_without_forgiving_the_hold() {
    let c = db();
    acct_with_key(&c, "debt-account", "debt-key", 1_000_000_000, 5_000);
    let pricing = ReservationPricing::new(PROVIDER_OPENAI, 5_000).unwrap();
    assert_eq!(
        sqlite_reserve_priced_request_for_execution(
            &c,
            "debt-request",
            "debt-account",
            "debt-key",
            500_000_000,
            60,
            &ExecutionAttempt::direct(),
            &pricing,
        )
        .unwrap(),
        Some(500_000_000),
    );
    assert_eq!(
        account_topup(&c, "debt-account", -2_000_000_000, Some("debt-adjustment"),).unwrap(),
        Some(-1_500_000_000),
    );
    let usage = UsageEventInput {
        model: "gpt-debt-test".into(),
        provider: PROVIDER_OPENAI.into(),
        real_nano: 1_400_000_000,
        charge_basis_nano: 1_400_000_000,
        ..Default::default()
    };
    assert_eq!(
        sqlite_settle_request(
            &c,
            "debt-request",
            "debt-account",
            "debt-key",
            500_000_000,
            700_000_000,
            Some("debt-provider-ref"),
            Some(&usage),
        )
        .unwrap(),
        Some(-1_500_000_000),
    );
    assert_eq!(
        c.query_row(
            "SELECT a.balance_nano,a.spent_nano,a.reserved_nano,a.uncollected_nano, \
                    r.collected_nano,r.uncollected_nano,l.amount_nano,l.uncollected_nano \
             FROM accounts a JOIN billing_reservations r ON r.account_id=a.id \
             JOIN ledger l ON l.account_id=a.id AND l.request_id=r.request_id \
             WHERE a.id='debt-account'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            )),
        )
        .unwrap(),
        (
            -1_500_000_000,
            700_000_000,
            0,
            200_000_000,
            500_000_000,
            200_000_000,
            700_000_000,
            200_000_000,
        ),
    );
}

#[test]
fn sqlite_zero_multiplier_preserves_usage_without_a_charge_row() {
    let c = db();
    acct_with_key(&c, "meter-only-account", "meter-only-key", 0, 0);
    let pricing = ReservationPricing::new(PROVIDER_OPENAI, 0).unwrap();
    assert_eq!(
        sqlite_reserve_priced_request_for_execution(
            &c,
            "meter-only-request",
            "meter-only-account",
            "meter-only-key",
            0,
            60,
            &ExecutionAttempt::direct(),
            &pricing,
        )
        .unwrap(),
        Some(0),
    );
    let usage = UsageEventInput {
        model: "gpt-meter-only".into(),
        provider: PROVIDER_OPENAI.into(),
        input_tokens: 7,
        output_tokens: 11,
        real_nano: 123,
        charge_basis_nano: 123,
        ..Default::default()
    };
    assert_eq!(
        sqlite_settle_request(
            &c,
            "meter-only-request",
            "meter-only-account",
            "meter-only-key",
            0,
            0,
            Some("meter-only-ref"),
            Some(&usage),
        )
        .unwrap(),
        Some(0),
    );
    assert_eq!(
        c.query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='meter-only-account'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            )),
        )
        .unwrap(),
        (0, 0, 0, 0),
    );
    assert_eq!(
        c.query_row(
            "SELECT real_nano,charge_nano,provider,payable_multiplier_bp,
                    charge_basis_nano,uncollected_nano
               FROM usage_events WHERE request_id='meter-only-request'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, i64>(5)?,
            )),
        )
        .unwrap(),
        (123, 0, PROVIDER_OPENAI.into(), Some(0), Some(123), 0),
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM ledger
              WHERE kind='charge' AND request_id='meter-only-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
    );

    assert_eq!(
        sqlite_settle_request(
            &c,
            "meter-only-request",
            "meter-only-account",
            "meter-only-key",
            0,
            0,
            Some("meter-only-ref"),
            Some(&usage),
        )
        .unwrap(),
        Some(0),
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM usage_events WHERE request_id='meter-only-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );
}

#[test]
fn sqlite_priced_terminal_replay_requires_collection_evidence() {
    let c = db();
    acct_with_key(
        &c,
        "priced-terminal-account",
        "priced-terminal-key",
        1_000,
        5_000,
    );
    let pricing = ReservationPricing::new(PROVIDER_OPENAI, 5_000).unwrap();
    assert_eq!(
        sqlite_reserve_priced_request_for_execution(
            &c,
            "priced-terminal-request",
            "priced-terminal-account",
            "priced-terminal-key",
            100,
            60,
            &ExecutionAttempt::direct(),
            &pricing,
        )
        .unwrap(),
        Some(900),
    );

    // Simulate a pre-guard/corrupt audit snapshot. Fresh SQLite databases reject this UPDATE at
    // the trigger, but the runtime must also fail closed while reading an already-bad terminal row.
    c.execute_batch("DROP TRIGGER billing_reservations_settlement_evidence_update")
        .unwrap();
    c.execute(
        "UPDATE billing_reservations
            SET state='settled',actual_nano=50,balance_after_settle_nano=950,
                reference='priced-terminal-ref',settled_ts=1
          WHERE request_id='priced-terminal-request'",
        [],
    )
    .unwrap();
    let usage = UsageEventInput {
        model: "gpt-priced-terminal".into(),
        provider: PROVIDER_OPENAI.into(),
        real_nano: 100,
        charge_basis_nano: 100,
        ..Default::default()
    };
    let replay_error = sqlite_settle_request(
        &c,
        "priced-terminal-request",
        "priced-terminal-account",
        "priced-terminal-key",
        100,
        50,
        Some("priced-terminal-ref"),
        Some(&usage),
    )
    .unwrap_err()
    .to_string();
    assert!(
        replay_error.contains("terminal settlement collection evidence is inconsistent"),
        "unexpected replay error: {replay_error}",
    );

    let usage_json = serde_json::to_string(&usage).unwrap();
    c.execute(
        "INSERT INTO billing_settlement_outbox(
             request_id,actual_nano,reference,usage_json,charge_basis_nano,disposition,state,
             attempts,next_attempt_ts,created_ts,updated_ts
         ) VALUES(?1,50,'priced-terminal-ref',?2,100,'settle','pending',0,0,1,1)",
        rusqlite::params!["priced-terminal-request", usage_json],
    )
    .unwrap();
    let process_error = sqlite_process_settlement(&c, "priced-terminal-request")
        .unwrap_err()
        .to_string();
    assert!(
        process_error.contains("terminal settlement collection evidence is inconsistent"),
        "unexpected process error: {process_error}",
    );
}

#[test]
fn sqlite_corrupt_key_reserve_rolls_back_the_whole_settlement() {
    let c = db();
    acct_with_key(&c, "key-fence-account", "key-fence-key", 1_000, 10_000);
    assert_eq!(
        sqlite_reserve_request(
            &c,
            "key-fence-request",
            "key-fence-account",
            "key-fence-key",
            400,
            60,
        )
        .unwrap(),
        Some(600),
    );
    c.execute(
        "UPDATE api_keys SET reserved_nano=0 WHERE key='key-fence-key'",
        [],
    )
    .unwrap();

    let error = sqlite_settle_request(
        &c,
        "key-fence-request",
        "key-fence-account",
        "key-fence-key",
        400,
        300,
        Some("key-fence-ref"),
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("reservation/key aggregate invariant failed"),
        "unexpected key-fence error: {error}",
    );
    assert_eq!(
        c.query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='key-fence-account'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?
            )),
        )
        .unwrap(),
        (600, 0, 400, 0),
        "the account update before the key check must be rolled back",
    );
    assert_eq!(
        c.query_row(
            "SELECT state,actual_nano,collected_nano,uncollected_nano
               FROM billing_reservations WHERE request_id='key-fence-request'",
            [],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?
            )),
        )
        .unwrap(),
        ("reserved".into(), None, None, None),
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM ledger WHERE kind='charge' AND request_id='key-fence-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
    );

    c.execute(
        "UPDATE api_keys SET reserved_nano=400 WHERE key='key-fence-key'",
        [],
    )
    .unwrap();
    assert_eq!(
        sqlite_settle_request(
            &c,
            "key-fence-request",
            "key-fence-account",
            "key-fence-key",
            400,
            300,
            Some("key-fence-ref"),
            None,
        )
        .unwrap(),
        Some(700),
    );
}

#[test]
fn sqlite_conflicting_evidence_rows_cannot_commit_partial_aggregates() {
    let c = db();
    acct_with_key(&c, "evidence-account", "evidence-key", 1_000, 5_000);
    let pricing = ReservationPricing::new(PROVIDER_OPENAI, 5_000).unwrap();
    assert_eq!(
        sqlite_reserve_priced_request_for_execution(
            &c,
            "evidence-request",
            "evidence-account",
            "evidence-key",
            400,
            60,
            &ExecutionAttempt::direct(),
            &pricing,
        )
        .unwrap(),
        Some(600),
    );
    let usage = UsageEventInput {
        model: "gpt-evidence".into(),
        provider: PROVIDER_OPENAI.into(),
        real_nano: 600,
        charge_basis_nano: 600,
        ..Default::default()
    };

    c.execute(
        "INSERT INTO ledger(
             account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,
             model,provider,official_nano,payable_multiplier_bp,uncollected_nano
         ) VALUES(
             'evidence-account','evidence-key','charge','evidence-request',1,
             'foreign-ledger-row',999,1,'foreign','openai',2,5000,0
         )",
        [],
    )
    .unwrap();
    assert!(sqlite_settle_request(
        &c,
        "evidence-request",
        "evidence-account",
        "evidence-key",
        400,
        300,
        Some("evidence-ref"),
        Some(&usage),
    )
    .is_err());
    assert_eq!(
        c.query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='evidence-account'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?
            )),
        )
        .unwrap(),
        (600, 0, 400, 0),
    );
    c.execute(
        "DELETE FROM ledger WHERE kind='charge' AND request_id='evidence-request'",
        [],
    )
    .unwrap();

    c.execute(
        "INSERT INTO usage_events(request_id,account_id,key,model,provider)
         VALUES('evidence-request','evidence-account','evidence-key','foreign','openai')",
        [],
    )
    .unwrap();
    assert!(sqlite_settle_request(
        &c,
        "evidence-request",
        "evidence-account",
        "evidence-key",
        400,
        300,
        Some("evidence-ref"),
        Some(&usage),
    )
    .is_err());
    assert_eq!(
        c.query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='evidence-account'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?
            )),
        )
        .unwrap(),
        (600, 0, 400, 0),
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM ledger WHERE kind='charge' AND request_id='evidence-request'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
        "the ledger insert preceding the usage conflict must roll back",
    );
    c.execute(
        "DELETE FROM usage_events WHERE request_id='evidence-request'",
        [],
    )
    .unwrap();

    assert_eq!(
        sqlite_settle_request(
            &c,
            "evidence-request",
            "evidence-account",
            "evidence-key",
            400,
            300,
            Some("evidence-ref"),
            Some(&usage),
        )
        .unwrap(),
        Some(700),
    );
    assert_eq!(
        c.query_row(
            "SELECT balance_nano,spent_nano,reserved_nano,uncollected_nano
               FROM accounts WHERE id='evidence-account'",
            [],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?
            )),
        )
        .unwrap(),
        (700, 300, 0, 0),
    );
}

#[test]
fn sqlite_execution_group_charges_only_the_first_nonzero_settlement() {
    const GROUP: &str = "018f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
    const ZERO_GROUP: &str = "128f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";

    let c = db();
    acct_with_key(&c, "group-account", "group-key", 1_000, 2_000);
    let first = ExecutionAttempt::grouped(GROUP, 1).unwrap();
    let second = ExecutionAttempt::grouped(GROUP, 2).unwrap();
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            60,
            &first,
        )
        .unwrap(),
        Some(600),
    );
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            60,
            &first,
        )
        .unwrap(),
        Some(600),
    );
    assert!(sqlite_reserve_request_for_execution(
        &c,
        "group-request-1",
        "group-account",
        "group-key",
        400,
        60,
        &second,
    )
    .is_err());
    assert!(
        sqlite_reserve_request(&c, "group-request-1", "group-account", "group-key", 400, 60,)
            .is_err()
    );
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "group-request-2",
            "group-account",
            "group-key",
            300,
            60,
            &second,
        )
        .unwrap(),
        Some(300),
    );

    // Settlement order, not attempt number, chooses the durable winner.
    assert_eq!(
        sqlite_settle_request(
            &c,
            "group-request-2",
            "group-account",
            "group-key",
            300,
            200,
            Some("provider:second"),
            None,
        )
        .unwrap(),
        Some(400),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            150,
            Some("provider:first"),
            None,
        )
        .unwrap(),
        Some(800),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "group-request-1",
            "group-account",
            "group-key",
            400,
            150,
            Some("provider:first"),
            None,
        )
        .unwrap(),
        Some(800),
    );
    let account = account_get(&c, "group-account").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (800, 200, 0),
    );
    assert_eq!(
        c.query_row(
            "SELECT
               (SELECT winner_request_id FROM execution_group_winner WHERE group_id=?1),
               (SELECT COUNT(*) FROM ledger WHERE kind='charge'
                 AND ref IN ('provider:first','provider:second')),
               (SELECT actual_nano FROM billing_reservations
                 WHERE request_id='group-request-1'),
               (SELECT state FROM billing_reservations
                 WHERE request_id='group-request-1'),
               (SELECT actual_nano FROM billing_settlement_outbox
                 WHERE request_id='group-request-1')",
            rusqlite::params![GROUP],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            )),
        )
        .unwrap(),
        ("group-request-2".into(), 1, 0, "canceled".into(), 150),
    );

    // A zero settlement does not consume the group winner slot.
    let zero = ExecutionAttempt::grouped(ZERO_GROUP, 1).unwrap();
    let positive = ExecutionAttempt::grouped(ZERO_GROUP, 2).unwrap();
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "zero-request",
            "group-account",
            "group-key",
            100,
            60,
            &zero,
        )
        .unwrap(),
        Some(700),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "zero-request",
            "group-account",
            "group-key",
            100,
            0,
            None,
            None,
        )
        .unwrap(),
        Some(800),
    );
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![ZERO_GROUP],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
    );
    assert_eq!(
        sqlite_reserve_request_for_execution(
            &c,
            "positive-request",
            "group-account",
            "group-key",
            100,
            60,
            &positive,
        )
        .unwrap(),
        Some(700),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "positive-request",
            "group-account",
            "group-key",
            100,
            50,
            Some("provider:positive"),
            None,
        )
        .unwrap(),
        Some(750),
    );
    assert_eq!(
        c.query_row(
            "SELECT winner_request_id FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![ZERO_GROUP],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "positive-request",
    );
}

#[test]
fn sqlite_execution_group_winner_is_pruned_only_after_all_group_replays_expire() {
    const GROUP: &str = "228f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";

    let c = db();
    acct_with_key(
        &c,
        "retained-group-account",
        "retained-group-key",
        1_000,
        2_000,
    );
    for (request_id, attempt) in [("retained-winner", 1), ("retained-loser", 2)] {
        assert!(sqlite_reserve_request_for_execution(
            &c,
            request_id,
            "retained-group-account",
            "retained-group-key",
            100,
            60,
            &ExecutionAttempt::grouped(GROUP, attempt).unwrap(),
        )
        .unwrap()
        .is_some());
    }
    assert_eq!(
        sqlite_settle_request(
            &c,
            "retained-winner",
            "retained-group-account",
            "retained-group-key",
            100,
            50,
            Some("retained:winner"),
            None,
        )
        .unwrap(),
        Some(850),
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "retained-loser",
            "retained-group-account",
            "retained-group-key",
            100,
            60,
            Some("retained:loser"),
            None,
        )
        .unwrap(),
        Some(950),
    );
    c.execute(
        "UPDATE billing_reservations SET settled_ts=1 WHERE request_id='retained-winner'",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE billing_settlement_outbox SET committed_ts=1 WHERE request_id='retained-winner'",
        [],
    )
    .unwrap();
    let first_prune = sqlite_maintenance_prune(&c, 2).unwrap();
    assert_eq!((first_prune.outbox, first_prune.reservations), (1, 1));
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![GROUP],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );
    assert_eq!(
        sqlite_settle_request(
            &c,
            "retained-loser",
            "retained-group-account",
            "retained-group-key",
            100,
            60,
            Some("retained:loser"),
            None,
        )
        .unwrap(),
        Some(950),
    );

    c.execute(
        "UPDATE billing_reservations SET settled_ts=1 WHERE request_id='retained-loser'",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE billing_settlement_outbox SET committed_ts=1 WHERE request_id='retained-loser'",
        [],
    )
    .unwrap();
    let second_prune = sqlite_maintenance_prune(&c, 2).unwrap();
    assert_eq!((second_prune.outbox, second_prune.reservations), (1, 1));
    assert_eq!(
        c.query_row(
            "SELECT COUNT(*) FROM execution_group_winner WHERE group_id=?1",
            rusqlite::params![GROUP],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
    );
}

#[test]
fn sqlite_pending_settlement_survives_until_recovery() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000, 2000);
    sqlite_reserve_request(&c, "req", "a", "k", 500, 60).unwrap();
    sqlite_mark_delivering(&c, "req", 60).unwrap();
    // Simulate a process crash after durable intent commit but before the balance transaction.
    assert_eq!(
        sqlite_enqueue_settlement(
            &c,
            "req",
            "a",
            "k",
            500,
            175,
            Some("provider:req"),
            None,
            "settle",
        )
        .unwrap(),
        None,
    );
    let before = account_get(&c, "a").unwrap().unwrap();
    assert_eq!((before.balance_nano, before.reserved_nano), (500, 500));
    let report = sqlite_reconcile_expired(&c, 100, false).unwrap();
    assert_eq!(report.processed_outbox, 1);
    let after = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(
        (after.balance_nano, after.spent_nano, after.reserved_nano),
        (825, 175, 0)
    );
}

#[test]
fn sqlite_expired_reservations_follow_delivery_state() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000, 2000);
    sqlite_reserve_request(&c, "pre", "a", "k", 200, 60).unwrap();
    sqlite_reserve_request(&c, "delivered", "a", "k", 300, 60).unwrap();
    sqlite_mark_delivering(&c, "delivered", 60).unwrap();
    c.execute("UPDATE billing_reservations SET lease_until=0", [])
        .unwrap();
    // Default policy: a turn that died before reporting any usage is released, not billed at the
    // admission ceiling. The pre-delivery reservation is cancelled either way.
    let report = sqlite_reconcile_expired(&c, 100, false).unwrap();
    assert_eq!(report.canceled_before_delivery, 1);
    assert_eq!(report.charged_after_delivery, 1);
    let account = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (1_000, 0, 0)
    );
}

/// The conservative fallback is a switch, not a deletion: an operator facing a provider that stops
/// reporting usage must be able to restore full-hold recovery without a deploy.
#[test]
fn sqlite_expired_delivery_bills_the_hold_only_when_the_fallback_is_re_armed() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000, 2000);
    sqlite_reserve_request(&c, "delivered", "a", "k", 300, 60).unwrap();
    sqlite_mark_delivering(&c, "delivered", 60).unwrap();
    c.execute("UPDATE billing_reservations SET lease_until=0", [])
        .unwrap();
    let report = sqlite_reconcile_expired(&c, 100, true).unwrap();
    assert_eq!(report.charged_after_delivery, 1);
    let account = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(
        (
            account.balance_nano,
            account.spent_nano,
            account.reserved_nano
        ),
        (700, 300, 0)
    );
}

#[test]
fn token_source_switch_resets_only_stale_health() {
    let c = db();
    add(&c, "sub@example.com", "token-a", "", "prod").unwrap();
    let dead = SubHealth {
        email: "sub@example.com".into(),
        auth_state: "dead".into(),
        auth_fail_streak: 3,
        first_auth_fail_ts: 1,
        last_auth_fail_ts: 2,
        last_auth_http: 401,
        dead_since_ts: 2,
        dead_reason: "authentication_error".into(),
        auth_token_fp: "old-fingerprint".into(),
    };
    save_sub_health(&c, &dead).unwrap();

    add(&c, "sub@example.com", "token-a", "proxy", "prod").unwrap();
    assert_eq!(load_sub_health(&c, None).unwrap()[0].auth_state, "dead");

    add(&c, "sub@example.com", "token-b", "proxy", "prod").unwrap();
    let changed = &load_sub_health(&c, None).unwrap()[0];
    assert_eq!(
        (changed.auth_state.as_str(), changed.auth_fail_streak),
        ("healthy", 0)
    );
    assert!(changed.auth_token_fp.is_empty());
    let sources: (Option<String>, Option<String>) = c
        .query_row(
            "SELECT token,token_file FROM subs WHERE email='sub@example.com'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(sources, (Some("token-b".into()), None));

    add_file(&c, "sub@example.com", "/tmp/token", "proxy", "prod").unwrap();
    let sources: (Option<String>, Option<String>) = c
        .query_row(
            "SELECT token,token_file FROM subs WHERE email='sub@example.com'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(sources, (None, Some("/tmp/token".into())));
}

/// Двойной settle (перекрытие деплоя: reconcile уже вернул резерв, затем settle старого инстанса)
/// НЕ переначисляет и НЕ уводит reserved в минус — кламп MIN(hold,reserved)/MAX(0,…).
#[test]
fn double_settle_no_overcredit() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 10000);
    account_reserve(&c, "a", 400_000_000).unwrap();
    // Эмулируем внешний/исторический возврат hold до прихода старого settle.
    c.execute(
        "UPDATE accounts SET balance_nano=balance_nano+reserved_nano, reserved_nano=0 WHERE id='a'",
        [],
    )
    .unwrap();
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        1_000_000_000
    );
    // теперь прилетает settle СТАРОГО инстанса на тот же hold (actual $0.1)
    account_settle(&c, "a", "k", 400_000_000, 100_000_000, None, None).unwrap();
    let acc = account_get(&c, "a").unwrap().unwrap();
    // без клампа было бы: +$0.4 (второй раз!) − $0.1 = $1.3 (over-credit) и reserved=−$0.4.
    // с клампом: MIN(0.4, reserved=0)=0 → баланс += 0 − $0.1 = $0.9; reserved MAX(0,−0.4)=0.
    assert_eq!(
        acc.balance_nano, 900_000_000,
        "нет over-credit: списан только actual"
    );
    assert_eq!(acc.reserved_nano, 0, "reserved не ушёл в минус");
}

/// release (settle с actual=0) возвращает резерв полностью, ledger-charge НЕ пишется.
#[test]
fn reserve_release_refunds_fully() {
    let c = db();
    acct_with_key(&c, "a", "k", 500_000_000, 2000);
    account_reserve(&c, "a", 200_000_000).unwrap();
    account_settle(&c, "a", "k", 200_000_000, 0, None, None).unwrap();
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        500_000_000
    );
    let charges: i64 = c
        .query_row("SELECT COUNT(*) FROM ledger WHERE kind='charge'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(charges, 0);
}

/// usage_events: запись по корзинам и агрегат по модели (суммы + real/charge nano + requests).
#[test]
fn usage_events_aggregate_by_model() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let opus = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 200,
        cache_write_5m_tokens: 100,
        cache_write_1h_tokens: 50,
        web_search_requests: 2,
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req2")).unwrap();
    let sonnet = UsageEventInput {
        model: "claude-sonnet-5".into(),
        input_tokens: 300,
        output_tokens: 100,
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &sonnet, 2_000_000, Some("req3")).unwrap();

    let aggs = usage_by_model(&c, "a", 0).unwrap();
    assert_eq!(aggs.len(), 2);
    // сортировка по SUM(real_nano) DESC → opus первый (2×20M > 5M)
    let o = &aggs[0];
    assert_eq!(o.model, "claude-opus-4-8");
    assert_eq!(o.requests, 2);
    assert_eq!(o.input_tokens, 2000); // 2×1000
    assert_eq!(o.output_tokens, 1000);
    assert_eq!(o.cache_read_tokens, 400);
    assert_eq!(o.cache_write_5m_tokens, 200);
    assert_eq!(o.cache_write_1h_tokens, 100);
    assert_eq!(o.web_search_requests, 4);
    assert_eq!(o.real_nano, 40_000_000);
    assert_eq!(o.charge_nano, 16_000_000);
    assert_eq!(aggs[1].model, "claude-sonnet-5");
    assert_eq!(aggs[1].requests, 1);
    // окно отсекает по ts: since в будущем → пусто
    assert!(usage_by_model(&c, "a", now() + 10_000).unwrap().is_empty());
    // prune всего → таблица пуста
    assert!(usage_prune(&c, now() + 10_000).unwrap() >= 3);
    assert!(usage_by_model(&c, "a", 0).unwrap().is_empty());
}

#[test]
fn usage_report_uses_one_exact_window_for_daily_and_key_totals() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let day_one = 20_000 * 86_400;
    let day_two = day_one + 86_400;
    let first = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 10,
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        input_nano: 20_000_000,
        ..Default::default()
    };
    let second = UsageEventInput {
        model: "claude-opus-4-8".into(),
        provider: PROVIDER_OPENAI.into(),
        output_tokens: 10,
        real_nano: 30_000_000,
        charge_basis_nano: 30_000_000,
        output_nano: 30_000_000,
        ..Default::default()
    };
    let third = UsageEventInput {
        model: "claude-sonnet-5".into(),
        cache_read_tokens: 10,
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        cache_read_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &first, 8_000_000, Some("r1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &second, 12_000_000, Some("r2")).unwrap();
    usage_event_add(&c, "a", Some("k-other"), &third, 2_000_000, Some("r3")).unwrap();
    c.execute(
        "UPDATE usage_events SET ts=CASE ref \
         WHEN 'r1' THEN ?1 WHEN 'r2' THEN ?2 ELSE ?3 END",
        rusqlite::params![day_one + 100, day_one + 200, day_two + 10],
    )
    .unwrap();

    let report = usage_report(&c, "a", day_one + 150, day_two + 100).unwrap();
    assert_eq!(report.models.len(), 2);
    assert_eq!(
        report.daily,
        vec![
            UsageDailyAgg {
                day_ts: day_one,
                requests: 1,
                real_nano: 30_000_000,
                charge_nano: 12_000_000,
            },
            UsageDailyAgg {
                day_ts: day_two,
                requests: 1,
                real_nano: 5_000_000,
                charge_nano: 2_000_000,
            },
        ]
    );
    assert_eq!(
        report.daily_providers,
        vec![
            UsageDailyProviderAgg {
                day_ts: day_one,
                provider: PROVIDER_OPENAI.into(),
                requests: 1,
                real_nano: 30_000_000,
                charge_nano: 12_000_000,
            },
            UsageDailyProviderAgg {
                day_ts: day_two,
                provider: PROVIDER_ANTHROPIC.into(),
                requests: 1,
                real_nano: 5_000_000,
                charge_nano: 2_000_000,
            },
        ]
    );
    assert_eq!(
        report.keys,
        vec![
            UsageKeyAgg {
                key: Some("k".into()),
                requests: 1,
                real_nano: 30_000_000,
                charge_nano: 12_000_000,
            },
            UsageKeyAgg {
                key: Some("k-other".into()),
                requests: 1,
                real_nano: 5_000_000,
                charge_nano: 2_000_000,
            },
        ]
    );
    assert_eq!(
        report.daily.iter().map(|row| row.real_nano).sum::<i64>(),
        report.models.iter().map(|row| row.real_nano).sum::<i64>(),
    );
    assert_eq!(
        report.keys.iter().map(|row| row.charge_nano).sum::<i64>(),
        report.models.iter().map(|row| row.charge_nano).sum::<i64>(),
    );
    assert_eq!(
        usage_report(&c, "a", day_two, day_two).unwrap(),
        UsageReport::default()
    );
}

/// Оба апстрима сеттлятся в одни и те же денежные таблицы, поэтому «кто заработал» должно
/// читаться из явной колонки, а не угадываться по имени модели.
#[test]
fn spend_is_attributed_to_the_serving_provider() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let claude = UsageEventInput {
        model: "claude-opus-5".into(),
        provider: PROVIDER_ANTHROPIC.into(),
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        ..Default::default()
    };
    let codex = UsageEventInput {
        model: "gpt-5.6".into(),
        provider: PROVIDER_OPENAI.into(),
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &claude, 8_000_000, Some("req1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &codex, 2_000_000, Some("req2")).unwrap();
    usage_event_add(&c, "a", Some("k"), &codex, 3_000_000, Some("req3")).unwrap();

    let rows = spend_by_provider(&c, 0).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);
    assert_eq!(rows[0].requests, 1);
    assert_eq!(rows[0].charge_nano, 8_000_000);
    assert_eq!(rows[1].provider, PROVIDER_OPENAI);
    assert_eq!(rows[1].requests, 2);
    assert_eq!(rows[1].charge_nano, 5_000_000);
    assert_eq!(rows[1].real_nano, 10_000_000);
    // Окно отсекает по ts, как и остальные агрегаты панели.
    assert!(spend_by_provider(&c, now() + 10_000).unwrap().is_empty());
}

/// Строка, записанная релизом без атрибуции, должна читаться как Claude, а не выпадать из
/// разбивки: blue-green оставляет предыдущий слот пишущим во время промоушена.
#[test]
fn usage_written_before_attribution_reads_as_the_claude_fleet() {
    let c = db();
    acct_with_key(&c, "a", "k", 10_000_000_000, 4000);
    let legacy = UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 1_000_000,
        charge_basis_nano: 1_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &legacy, 1_000_000, Some("req1")).unwrap();
    c.execute("UPDATE usage_events SET provider=''", [])
        .unwrap();
    let rows = spend_by_provider(&c, 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);

    // The queued settlement payload is JSON: a row serialized by the previous release carries
    // every field except this one, and must still decode instead of poisoning the outbox.
    let mut payload: serde_json::Value = serde_json::to_value(&legacy).unwrap();
    payload.as_object_mut().unwrap().remove("provider");
    let decoded: UsageEventInput = serde_json::from_value(payload).unwrap();
    assert_eq!(decoded.provider, PROVIDER_ANTHROPIC);
    assert_eq!(decoded.model, "claude-opus-5");
}

/// Разбивка расхода по моделям: top-N по charge, группировка по (model, provider) — один
/// model ID, обслуженный разными апстримами, не смешивается в одну строку.
#[test]
fn spend_is_broken_down_by_served_model() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
    let opus = UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 20_000_000,
        charge_basis_nano: 20_000_000,
        ..Default::default()
    };
    let gpt = UsageEventInput {
        model: "gpt-5.6".into(),
        provider: PROVIDER_OPENAI.into(),
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req1")).unwrap();
    usage_event_add(&c, "a", Some("k"), &gpt, 2_000_000, Some("req2")).unwrap();
    usage_event_add(&c, "a", Some("k"), &gpt, 3_000_000, Some("req3")).unwrap();

    let rows = spend_by_model(&c, 0, 20).unwrap();
    assert_eq!(rows.len(), 2);
    // сортировка по SUM(charge_nano) DESC → opus первый (8M > 2+3M)
    assert_eq!(rows[0].model, "claude-opus-5");
    assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);
    assert_eq!(rows[0].requests, 1);
    assert_eq!(rows[0].charge_nano, 8_000_000);
    assert_eq!(rows[0].real_nano, 20_000_000);
    assert_eq!(rows[1].model, "gpt-5.6");
    assert_eq!(rows[1].provider, PROVIDER_OPENAI);
    assert_eq!(rows[1].requests, 2);
    assert_eq!(rows[1].charge_nano, 5_000_000);
    // limit обрезает выдачу, окно — по ts, как у остальных spend-агрегатов
    assert_eq!(spend_by_model(&c, 0, 1).unwrap().len(), 1);
    assert!(spend_by_model(&c, now() + 10_000, 20).unwrap().is_empty());
}

/// Верхняя граница range-вариантов spend-агрегатов: полуоткрытое окно [since, until) —
/// событие ровно на `until` не попадает (стыкующиеся диапазоны не задваиваются), а open-ended
/// обёртки эквивалентны until=i64::MAX.
#[test]
fn spend_range_honors_upper_bound() {
    let c = db();
    acct_with_key(&c, "a", "k", 100_000_000_000, 2000);
    let usage = UsageEventInput {
        model: "claude-opus-5".into(),
        real_nano: 10_000_000,
        charge_basis_nano: 10_000_000,
        ..Default::default()
    };
    for (i, ts) in [1_000i64, 2_000, 3_000].iter().enumerate() {
        usage_event_add(
            &c,
            "a",
            Some("k"),
            &usage,
            1_000_000,
            Some(&format!("req{i}")),
        )
        .unwrap();
        c.execute(
            "UPDATE usage_events SET ts=?1 WHERE ref=?2",
            rusqlite::params![ts, format!("req{i}")],
        )
        .unwrap();
    }
    // [1000, 3000): события 1000 и 2000 внутри, 3000 — ровно на границе, исключено.
    let accounts = spend_by_account_range(&c, 1_000, 3_000, 50).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].requests, 2);
    assert_eq!(accounts[0].charge_nano, 2_000_000);
    assert_eq!(accounts[0].real_nano, 20_000_000);
    assert_eq!(accounts[0].last_ts, 2_000);
    let providers = spend_by_provider_range(&c, 1_000, 3_000).unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].requests, 2);
    let models = spend_by_model_range(&c, 1_000, 3_000, 20).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].requests, 2);
    // Нижняя граница включительна, пустой хвост за последним событием — пуст.
    assert_eq!(
        spend_by_account_range(&c, 2_000, 3_000, 50).unwrap()[0].requests,
        1
    );
    assert!(spend_by_provider_range(&c, 3_001, 9_999)
        .unwrap()
        .is_empty());
    // Open-ended обёртки видят всё, как раньше.
    assert_eq!(spend_by_account(&c, 0, 50).unwrap()[0].requests, 3);
    assert_eq!(spend_by_provider(&c, 0).unwrap()[0].requests, 3);
    assert_eq!(spend_by_model(&c, 0, 20).unwrap()[0].requests, 3);
}

/// Сводка settlement pipeline: counts по state, failed за 24ч, backlog старых несеттленых,
/// ≤10 failed с урезанным до 200 символов last_error и лаг pricing-консьюмера ledger'а.
#[test]
fn settlement_health_reports_outbox_and_consumer_lag() {
    let c = db();
    // Пустая БД: везде нули, oldest_* = 0, consumer lag без watermark'ов.
    let empty = settlement_health(&c, 300, "pricing").unwrap();
    assert_eq!(empty.pending + empty.done + empty.failed + empty.backlog, 0);
    assert_eq!(empty.oldest_unsettled_ts, 0);
    assert!(empty.recent_failed.is_empty());
    assert_eq!(empty.ledger_consumer.ledger_max_id, 0);
    assert_eq!(empty.ledger_consumer.checkpoints, 0);

    acct_with_key(&c, "a", "k", 100_000_000_000, 2000);
    let ts = now();
    let seed_outbox = |request_id: &str,
                       state: &str,
                       attempts: i64,
                       error: Option<&str>,
                       created: i64,
                       updated: i64| {
        c.execute(
            "INSERT INTO billing_settlement_outbox(request_id,actual_nano,state,attempts, \
             next_attempt_ts,last_error,created_ts,updated_ts) \
             VALUES(?1,1000,?2,?3,0,?4,?5,?6)",
            rusqlite::params![request_id, state, attempts, error, created, updated],
        )
        .unwrap();
    };
    seed_outbox("r-done", "done", 1, None, ts - 100, ts - 90);
    seed_outbox("r-pending-fresh", "pending", 0, None, ts - 10, ts - 10);
    seed_outbox(
        "r-pending-old",
        "pending",
        3,
        Some("transient pg error"),
        ts - 3600,
        ts - 60,
    );
    seed_outbox(
        "r-failed-new",
        "failed",
        5,
        Some(&"x".repeat(500)),
        ts - 7200,
        ts - 30,
    );
    seed_outbox(
        "r-failed-old",
        "failed",
        5,
        Some("invariant violated"),
        ts - 200_000,
        ts - 100_000,
    );

    let h = settlement_health(&c, 300, "pricing").unwrap();
    assert_eq!(h.pending, 2);
    assert_eq!(h.processing, 0);
    assert_eq!(h.done, 1);
    assert_eq!(h.failed, 2);
    assert_eq!(h.failed_24h, 1, "старый failed за пределами 24ч-окна");
    assert_eq!(h.pending_with_error, 1);
    assert_eq!(h.backlog, 1, "только r-pending-old старше 300с");
    assert_eq!(h.oldest_unsettled_ts, ts - 3600);
    assert_eq!(h.recent_failed.len(), 2);
    assert_eq!(
        h.recent_failed[0].request_id, "r-failed-new",
        "свежий failed первым"
    );
    assert_eq!(
        h.recent_failed[0]
            .last_error
            .as_deref()
            .unwrap()
            .chars()
            .count(),
        200,
        "last_error урезан до 200 символов"
    );
    assert_eq!(
        h.recent_failed[1].last_error.as_deref(),
        Some("invariant violated")
    );

    // Лаг консьюмера: первая topup-строка подтверждена (ack), вторая — ещё нет.
    let first: i64 = c
        .query_row("SELECT MIN(id) FROM ledger", [], |r| r.get(0))
        .unwrap();
    ledger_ack(&c, "pricing", "a", first).unwrap();
    account_topup(&c, "a", 1_000_000, None).unwrap();
    let h = settlement_health(&c, 300, "pricing").unwrap();
    let lag = &h.ledger_consumer;
    assert_eq!(lag.consumer, "pricing");
    assert!(lag.ledger_max_id > first);
    assert_eq!(lag.checkpoints, 1);
    assert_eq!(lag.checkpoint_min, first);
    assert_eq!(lag.unacked, 1, "вторая topup-строка выше watermark'а");
    assert!(lag.oldest_unacked_ts > 0);
    // Consumer без watermark'ов не считается отставшим (та же семантика, что у ledger_prune).
    let h = settlement_health(&c, 300, "unknown").unwrap();
    assert_eq!(h.ledger_consumer.checkpoints, 0);
    assert_eq!(h.ledger_consumer.unacked, 0);
    assert_eq!(h.ledger_consumer.oldest_unacked_ts, 0);
}

/// settle пишет usage_event В ТОЙ ЖЕ операции (один коммит); при actual=0 usage НЕ пишется.
#[test]
fn settle_writes_usage_event_in_same_tx() {
    let c = db();
    acct_with_key(&c, "a", "k", 10_000_000_000, 4000);
    account_reserve(&c, "a", 1_000_000_000).unwrap();
    let u = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 100,
        output_tokens: 50,
        real_nano: 5_000_000,
        charge_basis_nano: 5_000_000,
        ..Default::default()
    };
    account_settle(
        &c,
        "a",
        "k",
        1_000_000_000,
        400_000_000,
        Some("req1"),
        Some(&u),
    )
    .unwrap();
    let charges: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM ledger WHERE kind='charge' AND account_id='a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(charges, 1, "charge записан");
    // charge-строка несёт модель (для точного per-model графика); topup/adjust — NULL.
    assert_eq!(
        ledger_recent(&c, "a", 10).unwrap()[0].model.as_deref(),
        Some("claude-opus-4-8"),
        "модель проставлена в ledger-charge"
    );
    let agg = usage_by_model(&c, "a", 0).unwrap();
    assert_eq!(agg.len(), 1);
    assert_eq!(agg[0].model, "claude-opus-4-8");
    assert_eq!(agg[0].input_tokens, 100);
    assert_eq!(agg[0].charge_nano, 400_000_000);
    // actual=0 (release/refund) → usage НЕ добавляется (charge не было)
    account_reserve(&c, "a", 500_000_000).unwrap();
    account_settle(&c, "a", "k", 500_000_000, 0, None, Some(&u)).unwrap();
    assert_eq!(
        usage_by_model(&c, "a", 0).unwrap()[0].requests,
        1,
        "usage не прибавился при actual=0"
    );
}

/// Group commit sees previous reserves in the same transaction: four contenders can consume the
/// shared buffer down to exactly −$1, while the fifth is refused instead of receiving its own floor.
/// Settles retain result order and write usage.
#[test]
fn hot_batch_sequential_and_atomic() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 4000);
    // Five nominally concurrent 500M reserves: only the first four fit the one account-wide floor.
    let ops = vec![
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
        },
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
        },
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
        },
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
        },
        HotOp::Reserve {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
        },
    ];
    let r = apply_hot_batch(&c, &ops).unwrap();
    assert_eq!(
        r,
        vec![
            Some(500_000_000),
            Some(0),
            Some(-500_000_000),
            Some(-ACCOUNT_OVERDRAFT_NANO),
            None,
        ],
        "the floor belongs to the account, not to each request"
    );
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, -ACCOUNT_OVERDRAFT_NANO);
    assert_eq!(acc.reserved_nano, 2_000_000_000);
    // settle в пачке: возвращает hold − actual, пишет usage; release (actual=0) возвращает hold.
    let u = UsageEventInput {
        model: "claude-opus-4-8".into(),
        input_tokens: 10,
        real_nano: 1000,
        charge_basis_nano: 1000,
        ..Default::default()
    };
    let ops2 = vec![
        HotOp::Settle {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
            actual: 100_000_000,
            reference: Some("r1"),
            usage: Some(&u),
        },
        HotOp::Settle {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
            actual: 0,
            reference: None,
            usage: None,
        },
        HotOp::Settle {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
            actual: 0,
            reference: None,
            usage: None,
        },
        HotOp::Settle {
            account_id: "a",
            key: "k",
            hold: 500_000_000,
            actual: 0,
            reference: None,
            usage: None,
        },
    ];
    apply_hot_batch(&c, &ops2).unwrap();
    let acc = account_get(&c, "a").unwrap().unwrap();
    assert_eq!(acc.balance_nano, 900_000_000); // −1000 +400 + three 500M releases
    assert_eq!(acc.reserved_nano, 0);
    assert_eq!(acc.spent_nano, 100_000_000);
    assert_eq!(
        usage_by_model(&c, "a", 0).unwrap().len(),
        1,
        "usage записан из батча"
    );
}

/// заблокированный аккаунт не резервируется; резолв ключа отражает активность обоих.
#[test]
fn reserve_rejects_disabled_account() {
    let c = db();
    acct_with_key(&c, "a", "k", 1_000_000_000, 2000);
    assert!(key_account(&c, "k").unwrap().unwrap().active);
    account_set_status(&c, "a", "disabled").unwrap();
    assert_eq!(account_reserve(&c, "a", 1).unwrap(), None);
    assert!(!key_account(&c, "k").unwrap().unwrap().active); // аккаунт неактивен → ключ тоже
}

/// Идемпотентный topup: повтор вебхука с тем же payment-ref НЕ начисляет дважды.
#[test]
fn topup_is_idempotent_by_ref() {
    let c = db();
    account_create(&c, "a", None, 2000).unwrap();
    // первый вебхук: +$10, ref=tx_ABC
    assert_eq!(
        account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
        Some(10_000_000_000)
    );
    // ПОВТОР того же вебхука (ретрай) — баланс НЕ должен вырасти
    assert_eq!(
        account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
        Some(10_000_000_000)
    );
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        10_000_000_000
    ); // ровно $10
       // ДРУГОЙ ref начисляет нормально
    assert_eq!(
        account_topup(&c, "a", 5_000_000_000, Some("tx_XYZ")).unwrap(),
        Some(15_000_000_000)
    );
    // без ref (админ-коррекция) — не дедупится, всегда применяется
    account_topup(&c, "a", 1_000_000_000, None).unwrap();
    account_topup(&c, "a", 1_000_000_000, None).unwrap();
    assert_eq!(
        account_get(&c, "a").unwrap().unwrap().balance_nano,
        17_000_000_000
    );
    // в ledger ровно один topup на каждый уникальный ref (+ 2 без ref)
    let topups: i64 = c
        .query_row("SELECT COUNT(*) FROM ledger WHERE kind='topup'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(topups, 4); // tx_ABC, tx_XYZ, и 2 без ref
                           // Поздний точный replay возвращает сохранённый исходный результат, не текущий баланс.
    assert_eq!(
        account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
        Some(10_000_000_000)
    );
}

/// A duplicate monetary reference succeeds only for the exact original operation.
#[test]
fn monetary_reference_rejects_parameter_mismatch_and_deduplicates_adjustments() {
    let c = db();
    account_create(&c, "a", None, 2000).unwrap();
    account_create(&c, "b", None, 2000).unwrap();
    assert_eq!(
        account_topup(&c, "a", 100, Some("payment:1")).unwrap(),
        Some(100)
    );
    assert!(account_topup(&c, "a", 200, Some("payment:1")).is_err());
    assert!(account_topup(&c, "b", 100, Some("payment:1")).is_err());
    assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 100);
    assert_eq!(account_get(&c, "b").unwrap().unwrap().balance_nano, 0);
    assert_eq!(
        account_topup(&c, "a", -25, Some("adjust:1")).unwrap(),
        Some(75)
    );
    assert_eq!(
        account_topup(&c, "a", -25, Some("adjust:1")).unwrap(),
        Some(75)
    );
    assert!(account_topup(&c, "a", -30, Some("adjust:1")).is_err());
    assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 75);
    assert!(account_topup(&c, "a", 1, Some("   ")).is_err());
}

/// Без consumer acknowledgement watermark charge-строки нельзя безопасно удалять.
#[test]
fn ledger_prune_is_disabled_without_consumer_watermarks() {
    let c = db();
    acct_with_key(&c, "a", "k", 5_000_000_000, 10000);
    account_reserve(&c, "a", 1_000_000_000).unwrap();
    account_settle(&c, "a", "k", 1_000_000_000, 400_000_000, Some("old"), None).unwrap();
    c.execute("UPDATE ledger SET ts = 1000", []).unwrap();
    assert_eq!(ledger_prune(&c, 2000).unwrap(), 0);
    let rows = ledger_after(&c, "a", 0, 10).unwrap();
    assert_eq!(
        rows.len(),
        2,
        "topup and unacknowledged charge remain cursor-visible"
    );
    assert!(rows.iter().any(|row| row.kind == "charge"));
}

/// N ключей под ОДНИМ аккаунтом тратят из ОБЩЕГО баланса (ключевая модель).
#[test]
fn multiple_keys_share_one_account_balance() {
    let c = db();
    account_create(&c, "team", Some("tg:123"), 2000).unwrap();
    account_topup(&c, "team", 1_000_000_000, None).unwrap(); // $1 на команду
    key_issue(&c, "k-alice", "team", Some("alice")).unwrap();
    key_issue(&c, "k-bob", "team", Some("bob")).unwrap();
    // оба ключа резолвятся в тот же аккаунт
    assert_eq!(
        key_account(&c, "k-alice").unwrap().unwrap().account_id,
        "team"
    );
    assert_eq!(
        key_account(&c, "k-bob").unwrap().unwrap().account_id,
        "team"
    );
    // alice тратит $0.30, bob $0.20 — из общего баланса
    account_reserve(&c, "team", 300_000_000).unwrap();
    account_settle(&c, "team", "k-alice", 300_000_000, 300_000_000, None, None).unwrap();
    account_reserve(&c, "team", 200_000_000).unwrap();
    account_settle(&c, "team", "k-bob", 200_000_000, 200_000_000, None, None).unwrap();
    assert_eq!(
        account_get(&c, "team").unwrap().unwrap().balance_nano,
        500_000_000
    ); // $0.50 осталось
       // атрибуция по ключам раздельная
    assert_eq!(
        key_get(&c, "k-alice").unwrap().unwrap().spent_nano,
        300_000_000
    );
    assert_eq!(
        key_get(&c, "k-bob").unwrap().unwrap().spent_nano,
        200_000_000
    );
    // вход по handle
    assert_eq!(account_by_handle(&c, "tg:123").unwrap().unwrap().id, "team");
}

/// Control-plane management uses a stable public ID and never needs to persist the raw key.
#[test]
fn key_can_be_disabled_by_non_secret_id() {
    let c = db();
    account_create(&c, "acct", None, 2000).unwrap();
    key_issue(&c, "sk-pool-super-secret", "acct", Some("prod")).unwrap();
    let issued = key_get(&c, "sk-pool-super-secret").unwrap().unwrap();
    assert!(issued.key_id.starts_with("key_"));
    assert_eq!(
        key_set_status_by_id(&c, &issued.key_id, "disabled").unwrap(),
        1
    );
    assert_eq!(
        key_set_label_by_id(&c, &issued.key_id, "renamed").unwrap(),
        1
    );
    let updated = key_get(&c, "sk-pool-super-secret").unwrap().unwrap();
    assert_eq!(updated.status, "disabled");
    assert_eq!(updated.label.as_deref(), Some("renamed"));
    assert_eq!(key_set_label_by_id(&c, "key_missing", "unused").unwrap(), 0);
}

#[test]
fn per_key_policy_gates_reservations_and_releases_allowance() {
    let c = db();
    account_create(&c, "acct", None, 10_000).unwrap();
    account_topup(&c, "acct", 1_000, None).unwrap();
    key_issue_with_policy(
        &c,
        "limited",
        "acct",
        Some("limited"),
        Some(700),
        Some(now() + 60),
    )
    .unwrap();

    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 500).unwrap(),
        Some(500)
    );
    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 300).unwrap(),
        None
    );
    let account = account_get(&c, "acct").unwrap().unwrap();
    assert_eq!((account.balance_nano, account.reserved_nano), (500, 500));

    account_settle(&c, "acct", "limited", 500, 400, None, None).unwrap();
    let key = key_get(&c, "limited").unwrap().unwrap();
    assert_eq!(
        (key.spent_nano, key.reserved_nano, key.spend_limit_nano),
        (400, 0, Some(700))
    );

    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 300).unwrap(),
        Some(300)
    );
    account_settle(&c, "acct", "limited", 300, 0, None, None).unwrap();
    assert_eq!(key_get(&c, "limited").unwrap().unwrap().reserved_nano, 0);

    key_issue_with_policy(&c, "expired", "acct", None, None, Some(now())).unwrap();
    assert_eq!(
        account_reserve_for_key(&c, "acct", "expired", 1).unwrap(),
        None
    );
    assert_eq!(account_get(&c, "acct").unwrap().unwrap().reserved_nano, 0);
    let expired_auth = key_account(&c, "expired").unwrap().unwrap();
    assert!(expired_auth.active);
    assert!(
        !expired_auth.active_at(now()),
        "expiry is exclusive at the exact second"
    );

    key_set_status(&c, "limited", "disabled").unwrap();
    assert_eq!(
        account_reserve_for_key(&c, "acct", "limited", 1).unwrap(),
        None
    );
    assert!(!key_account(&c, "limited")
        .unwrap()
        .unwrap()
        .active_at(now()));
}

#[test]
fn key_policy_can_be_replaced_without_undercutting_live_usage() {
    let c = db();
    account_create(&c, "acct", None, 10_000).unwrap();
    account_topup(&c, "acct", 2_000, None).unwrap();
    key_issue_with_policy(&c, "mutable", "acct", None, Some(1_000), Some(now() + 60)).unwrap();
    let key_id = key_get(&c, "mutable").unwrap().unwrap().key_id;

    assert_eq!(
        account_reserve_for_key(&c, "acct", "mutable", 600).unwrap(),
        Some(1_400)
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, Some(599), None).unwrap(),
        KeyPolicyUpdate::LimitBelowUsage,
    );
    assert_eq!(
        key_get(&c, "mutable").unwrap().unwrap().spend_limit_nano,
        Some(1_000)
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, Some(600), None).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    account_settle(&c, "acct", "mutable", 600, 500, None, None).unwrap();
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, Some(499), None).unwrap(),
        KeyPolicyUpdate::LimitBelowUsage,
    );

    let future = now() + 3_600;
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, None, Some(future)).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    let updated = key_get(&c, "mutable").unwrap().unwrap();
    assert_eq!(
        (updated.spend_limit_nano, updated.expires_ts),
        (None, Some(future))
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, None, None).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    key_set_status_by_id(&c, &key_id, "disabled").unwrap();
    assert_eq!(
        key_set_policy_by_id(&c, "acct", &key_id, None, Some(now() + 7_200)).unwrap(),
        KeyPolicyUpdate::Updated,
    );
    assert!(!key_account(&c, "mutable")
        .unwrap()
        .unwrap()
        .active_at(now()));
    assert_eq!(
        key_set_policy_by_id(&c, "other-account", &key_id, None, None).unwrap(),
        KeyPolicyUpdate::NotFound,
    );
    assert_eq!(
        key_set_policy_by_id(&c, "acct", "key_missing", None, None).unwrap(),
        KeyPolicyUpdate::NotFound,
    );
}

#[test]
fn ledger_reads_recover_exact_provider_from_matching_usage() {
    let c = db();
    account_create(&c, "provider-account", None, 10_000).unwrap();
    c.execute_batch(
        "INSERT INTO ledger(
             account_id,kind,request_id,amount_nano,balance_after_nano,ts,model,provider
         ) VALUES(
             'provider-account','charge','provider-request',250,9750,100,'gpt-test',NULL
         );
         INSERT INTO usage_events(
             request_id,account_id,model,real_nano,charge_nano,ts,provider
         ) VALUES(
             'provider-request','provider-account','gpt-test',500,250,100,'openai'
         );",
    )
    .unwrap();

    let rows = ledger_after(&c, "provider-account", 0, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider.as_deref(), Some(PROVIDER_OPENAI));

    c.execute(
        "UPDATE ledger SET provider='anthropic' WHERE request_id='provider-request'",
        [],
    )
    .unwrap();
    assert!(ledger_after(&c, "provider-account", 0, 10).is_err());
}

#[test]
fn ledger_reads_recover_legacy_provider_only_from_strict_settlement_fingerprint() {
    let c = db();
    account_create(&c, "legacy-provider-account", None, 10_000).unwrap();
    account_create(&c, "other-provider-account", None, 10_000).unwrap();
    c.execute_batch(
        "INSERT INTO ledger(
             account_id,key,kind,amount_nano,ref,balance_after_nano,ts,model,provider
         ) VALUES
             ('legacy-provider-account','legacy-key','charge',250,'legacy:exact',9750,100,
              'gpt-legacy',NULL),
             ('legacy-provider-account','legacy-key','charge',125,'legacy:claude',9625,200,
              'claude-legacy',NULL),
             ('legacy-provider-account','legacy-key','charge',75,'legacy:conflict',9550,300,
              'ambiguous-model',NULL),
             ('legacy-provider-account','legacy-key','charge',60,'legacy:model-only',9490,400,
              'gpt-5',NULL);

         INSERT INTO usage_events(
             account_id,key,model,real_nano,charge_nano,ref,ts,provider
         ) VALUES
             ('legacy-provider-account','legacy-key','gpt-legacy',500,250,'legacy:exact',101,
              'openai');
         INSERT INTO usage_events(
             account_id,key,model,real_nano,charge_nano,ref,ts
         ) VALUES
             ('legacy-provider-account','legacy-key','claude-legacy',250,125,'legacy:claude',200);
         INSERT INTO usage_events(
             account_id,key,model,real_nano,charge_nano,ref,ts,provider
         ) VALUES
             ('legacy-provider-account','legacy-key','ambiguous-model',150,75,
              'legacy:conflict',300,'openai'),
             ('legacy-provider-account','legacy-key','ambiguous-model',150,75,
              'legacy:conflict',300,'google'),
             ('legacy-provider-account','wrong-key','gpt-5',120,60,'legacy:model-only',400,
              'openai'),
             ('legacy-provider-account','legacy-key','gpt-5',122,61,'legacy:model-only',400,
              'openai'),
             ('legacy-provider-account','legacy-key','gpt-5',120,60,'wrong-ref',400,'openai'),
             ('legacy-provider-account','legacy-key','wrong-model',120,60,
              'legacy:model-only',400,'openai'),
             ('legacy-provider-account','legacy-key','gpt-5',120,60,'legacy:model-only',402,
              'openai'),
             ('other-provider-account','legacy-key','gpt-5',120,60,'legacy:model-only',400,
              'openai');
         INSERT INTO usage_events(
             request_id,account_id,key,model,real_nano,charge_nano,ref,ts,provider
         ) VALUES(
             'unrelated-request','legacy-provider-account','legacy-key','gpt-5',120,60,
             'legacy:model-only',400,'openai'
         );",
    )
    .unwrap();

    let rows = ledger_after(&c, "legacy-provider-account", 0, 10).unwrap();
    assert_eq!(rows.len(), 4);
    let provider_for = |reference: &str| {
        rows.iter()
            .find(|row| row.reference.as_deref() == Some(reference))
            .unwrap()
            .provider
            .as_deref()
    };
    assert_eq!(provider_for("legacy:exact"), Some(PROVIDER_OPENAI));
    assert_eq!(provider_for("legacy:claude"), Some(PROVIDER_ANTHROPIC));
    assert_eq!(provider_for("legacy:conflict"), None);
    assert_eq!(provider_for("legacy:model-only"), None);
}

/// The whole pricing policy: one default discount on the account, optionally overridden per
/// provider. A provider without a row must keep the default — that fallback is what lets a B2C
/// account stay a single number while a B2B account holds different terms per provider.
#[test]
fn provider_discount_overrides_the_account_default_and_falls_back_without_a_row() {
    let c = db();
    account_create(&c, "acct-discount", None, 5_000).unwrap();
    account_topup(&c, "acct-discount", 9_000, None).unwrap();
    key_issue_with_policy(
        &c,
        "sk-pool-secret-that-must-remain-the-billing-credential",
        "acct-discount",
        None,
        Some(8_500),
        Some(now() + 3_600),
    )
    .unwrap();
    const NONSECRET_KEY_ID: &str = "key_nonsecret_authoritative_identity_7f3a";
    c.execute(
        "UPDATE api_keys SET key_id=?1 WHERE key=?2",
        rusqlite::params![
            NONSECRET_KEY_ID,
            "sk-pool-secret-that-must-remain-the-billing-credential"
        ],
    )
    .unwrap();
    assert_eq!(
        account_reserve_for_key(
            &c,
            "acct-discount",
            "sk-pool-secret-that-must-remain-the-billing-credential",
            500,
        )
        .unwrap(),
        Some(8_500),
    );
    account_settle(
        &c,
        "acct-discount",
        "sk-pool-secret-that-must-remain-the-billing-credential",
        500,
        300,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        account_reserve_for_key(
            &c,
            "acct-discount",
            "sk-pool-secret-that-must-remain-the-billing-credential",
            200,
        )
        .unwrap(),
        Some(8_500),
    );

    let auth = key_account(&c, "sk-pool-secret-that-must-remain-the-billing-credential")
        .unwrap()
        .unwrap();
    assert_eq!(auth.account_id, "acct-discount");
    assert_eq!(auth.key_id, NONSECRET_KEY_ID);
    assert_eq!(auth.mult_bp, 5_000);
    assert_eq!(auth.balance_nano, 8_500);
    assert_eq!(auth.spent_nano, 300);
    assert_eq!(auth.reserved_nano, 200);
    assert_eq!(auth.spend_limit_nano, Some(8_500));
    assert!(auth.expires_ts.is_some_and(|expires| expires > now()));
    assert!(auth.active);
    assert!(auth.provider_mult_bp.is_empty());
    for provider in DISCOUNT_PROVIDER_IDS {
        assert_eq!(auth.mult_for(provider), 5_000, "{provider}");
    }

    set_account_provider_discount(&c, "acct-discount", PROVIDER_OPENAI, 2_500, 100).unwrap();
    set_account_provider_discount(&c, "acct-discount", PROVIDER_GOOGLE, 10_000, 100).unwrap();
    let auth = key_account(&c, "sk-pool-secret-that-must-remain-the-billing-credential")
        .unwrap()
        .unwrap();
    assert_eq!(auth.mult_for(PROVIDER_OPENAI), 2_500);
    assert_eq!(auth.mult_for(PROVIDER_GOOGLE), 10_000);
    assert_eq!(auth.mult_for(PROVIDER_ANTHROPIC), 5_000);
    assert_eq!(auth.mult_for(PROVIDER_KIMI), 5_000);

    // A rewrite replaces the row rather than accumulating versions, and a cleared override
    // returns the provider to the account default in the same read.
    set_account_provider_discount(&c, "acct-discount", PROVIDER_OPENAI, 1_000, 200).unwrap();
    assert_eq!(
        key_account(&c, "sk-pool-secret-that-must-remain-the-billing-credential")
            .unwrap()
            .unwrap()
            .mult_for(PROVIDER_OPENAI),
        1_000
    );
    assert!(clear_account_provider_discount(&c, "acct-discount", PROVIDER_OPENAI).unwrap());
    assert!(!clear_account_provider_discount(&c, "acct-discount", PROVIDER_OPENAI).unwrap());
    let auth = key_account(&c, "sk-pool-secret-that-must-remain-the-billing-credential")
        .unwrap()
        .unwrap();
    assert_eq!(auth.mult_for(PROVIDER_OPENAI), 5_000);
    assert_eq!(
        account_provider_discounts(&c, "acct-discount").unwrap(),
        vec![(PROVIDER_GOOGLE.to_string(), 10_000)]
    );
}

/// A discount is money, so the writer refuses anything it cannot price: an unknown provider id
/// would silently never match a request, and a multiplier outside 0..=10000 would either give the
/// customer free inference or charge above list price.
#[test]
fn provider_discount_writes_are_bounded_and_require_a_known_account() {
    let c = db();
    account_create(&c, "acct-bounds", None, 5_000).unwrap();

    assert!(
        set_account_provider_discount(&c, "acct-bounds", "openai-compatible", 5_000, 1).is_err()
    );
    assert!(set_account_provider_discount(&c, "acct-bounds", PROVIDER_OPENAI, -1, 1).is_err());
    assert!(set_account_provider_discount(&c, "acct-bounds", PROVIDER_OPENAI, 10_001, 1).is_err());
    assert!(
        set_account_provider_discount(&c, "missing-account", PROVIDER_OPENAI, 5_000, 1).is_err()
    );
    assert!(account_provider_discounts(&c, "acct-bounds")
        .unwrap()
        .is_empty());

    // The bounds are inclusive: a free key (0) and list price (10000) are both legitimate.
    set_account_provider_discount(&c, "acct-bounds", PROVIDER_OPENAI, 0, 1).unwrap();
    set_account_provider_discount(&c, "acct-bounds", PROVIDER_ANTHROPIC, 10_000, 1).unwrap();
    assert_eq!(
        account_provider_discounts(&c, "acct-bounds").unwrap(),
        vec![
            (PROVIDER_ANTHROPIC.to_string(), 10_000),
            (PROVIDER_OPENAI.to_string(), 0),
        ]
    );
}

/// Fresh SQLite audit databases carry the same table constraints as PostgreSQL. Runtime writer
/// validation remains necessary for old snapshots whose table already predates these CREATE-time
/// checks, but a direct import into a new snapshot must not bypass the money contract.
#[test]
fn sqlite_provider_discount_table_rejects_invalid_direct_rows() {
    let c = db();
    account_create(&c, "acct-table-bounds", None, 5_000).unwrap();

    assert!(c
        .execute(
            "INSERT INTO account_provider_discounts(account_id,provider_id,mult_bp,updated_ts) \
             VALUES(?1,'zhipu',5000,1)",
            rusqlite::params!["acct-table-bounds"],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_provider_discounts(account_id,provider_id,mult_bp,updated_ts) \
             VALUES(?1,'openai',-1,1)",
            rusqlite::params!["acct-table-bounds"],
        )
        .is_err());
    assert!(c
        .execute(
            "INSERT INTO account_provider_discounts(account_id,provider_id,mult_bp,updated_ts) \
             VALUES(?1,'openai',10001,1)",
            rusqlite::params!["acct-table-bounds"],
        )
        .is_err());

    c.execute(
        "INSERT INTO account_provider_discounts(account_id,provider_id,mult_bp,updated_ts) \
         VALUES(?1,'openai',5000,1)",
        rusqlite::params!["acct-table-bounds"],
    )
    .unwrap();
    assert_eq!(
        account_provider_discounts(&c, "acct-table-bounds").unwrap(),
        vec![(PROVIDER_OPENAI.to_string(), 5_000)],
    );
}

/// Discounts belong to the account, not the key: every key of an account is priced identically,
/// and one account's override never reaches another account's key.
#[test]
fn provider_discounts_are_shared_by_the_keys_of_one_account_only() {
    let c = db();
    account_create(&c, "acct-one", None, 5_000).unwrap();
    account_create(&c, "acct-two", None, 7_000).unwrap();
    key_issue(&c, "sk-pool-one-a", "acct-one", None).unwrap();
    key_issue(&c, "sk-pool-one-b", "acct-one", None).unwrap();
    key_issue(&c, "sk-pool-two", "acct-two", None).unwrap();
    set_account_provider_discount(&c, "acct-one", PROVIDER_ANTHROPIC, 1_500, 1).unwrap();

    for key in ["sk-pool-one-a", "sk-pool-one-b"] {
        let auth = key_account(&c, key).unwrap().unwrap();
        assert_eq!(auth.mult_for(PROVIDER_ANTHROPIC), 1_500, "{key}");
    }
    let other = key_account(&c, "sk-pool-two").unwrap().unwrap();
    assert!(other.provider_mult_bp.is_empty());
    assert_eq!(other.mult_for(PROVIDER_ANTHROPIC), 7_000);
}
