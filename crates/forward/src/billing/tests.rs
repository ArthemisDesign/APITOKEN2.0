use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn pg_command_metrics_buckets_are_cumulative_and_per_op() {
    let metrics = PgCommandMetrics::default();
    metrics.observe(PgCommandOp::Reserve, Duration::from_millis(5));
    metrics.observe(PgCommandOp::Reserve, Duration::from_millis(600));
    metrics.observe(PgCommandOp::Settle, Duration::from_millis(1));
    let stats = metrics.snapshot();
    let reserve = PgCommandOp::Reserve as usize;
    let settle = PgCommandOp::Settle as usize;
    let capacity = PgCommandOp::AcquireCapacity as usize;
    let bucket = |op: usize, upper_ms: u64| {
        let bucket_index = PG_COMMAND_LATENCY_BUCKETS_MS
            .iter()
            .position(|candidate| *candidate == upper_ms)
            .expect("bucket boundary exists");
        stats.buckets[op * PG_COMMAND_LATENCY_BUCKETS_MS.len() + bucket_index]
    };
    assert_eq!(stats.count[reserve], 2);
    assert_eq!(stats.count[settle], 1);
    assert_eq!(stats.count[capacity], 0);
    // The 5 ms observation fits every bucket from 5 ms up; the 600 ms one fits only 1000 ms.
    assert_eq!(bucket(reserve, 5), 1);
    assert_eq!(bucket(reserve, 10), 1);
    assert_eq!(bucket(reserve, 500), 1);
    assert_eq!(bucket(reserve, 1_000), 2);
    assert_eq!(bucket(settle, 1), 1);
    assert_eq!(bucket(capacity, 1_000), 0);
    assert_eq!(
        stats.sum_micros[reserve],
        5_000 + 600_000,
        "sum must collect exact microseconds for the histogram _sum series"
    );
    assert_eq!(stats.sum_micros[settle], 1_000);
}

#[test]
fn pg_command_timer_observes_on_drop() {
    let metrics = PgCommandMetrics::default();
    {
        let _timer = metrics.timer(PgCommandOp::AcquireCapacity);
        std::thread::sleep(Duration::from_millis(2));
    }
    let stats = metrics.snapshot();
    assert_eq!(stats.count[PgCommandOp::AcquireCapacity as usize], 1);
    assert!(stats.sum_micros[PgCommandOp::AcquireCapacity as usize] >= 2_000);
}

#[test]
fn channel_queue_depth_counts_occupied_slots() {
    let (sender, mut receiver) = mpsc::channel::<u8>(4);
    assert_eq!(channel_queue_depth(&sender), 0);
    for value in 0..3 {
        sender.try_send(value).expect("capacity available");
    }
    assert_eq!(channel_queue_depth(&sender), 3);
    receiver.try_recv().expect("queued value");
    assert_eq!(channel_queue_depth(&sender), 2);
}

fn anthropic_event(
    request_id: &str,
    api_total_nanousd: i64,
    completed_at: i64,
) -> ProviderTurnCalibrationEvent {
    ProviderTurnCalibrationEvent {
        provider: registry::PROVIDER_ANTHROPIC.to_owned(),
        request_id: request_id.to_owned(),
        subject_id: "operator@example.test".to_owned(),
        model_id: "claude-sonnet-4-5".to_owned(),
        service_tier: "standard".to_owned(),
        inference_geo: "global".to_owned(),
        tariff_schedule_id: "anthropic/test/v1".to_owned(),
        priced_ts: completed_at,
        completed_at,
        input_tokens: 1,
        audio_input_tokens: 0,
        cache_read_tokens: 0,
        cached_audio_input_tokens: 0,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: 0,
        thinking_output_tokens: 0,
        image_output_tokens: 0,
        tool_prompt_tokens: 0,
        search_queries: 0,
        grounded_search_prompts: 0,
        api_input_nanousd: api_total_nanousd,
        api_audio_input_nanousd: 0,
        api_cache_read_nanousd: 0,
        api_cached_audio_input_nanousd: 0,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd: 0,
        api_image_output_nanousd: 0,
        api_search_nanousd: 0,
        api_total_nanousd,
    }
}

fn gemini_event(
    request_id: &str,
    api_total_nanousd: i64,
    completed_at: i64,
) -> ProviderTurnCalibrationEvent {
    ProviderTurnCalibrationEvent {
        provider: registry::PROVIDER_GOOGLE.to_owned(),
        request_id: request_id.to_owned(),
        subject_id: "profile-a".to_owned(),
        model_id: "gemini-2.5-flash".to_owned(),
        service_tier: "standard".to_owned(),
        inference_geo: "global".to_owned(),
        tariff_schedule_id: "google/test/v1".to_owned(),
        priced_ts: completed_at,
        completed_at,
        input_tokens: 1,
        audio_input_tokens: 0,
        cache_read_tokens: 0,
        cached_audio_input_tokens: 0,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: 0,
        thinking_output_tokens: 0,
        image_output_tokens: 0,
        tool_prompt_tokens: 0,
        search_queries: 0,
        grounded_search_prompts: 0,
        api_input_nanousd: api_total_nanousd,
        api_audio_input_nanousd: 0,
        api_cache_read_nanousd: 0,
        api_cached_audio_input_nanousd: 0,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd: 0,
        api_image_output_nanousd: 0,
        api_search_nanousd: 0,
        api_total_nanousd,
    }
}

fn anthropic_snapshot(
    window_kind: &str,
    used_fraction_units: i64,
    observed_at: i64,
) -> AnthropicQuotaSnapshot {
    AnthropicQuotaSnapshot {
        window_kind: window_kind.to_owned(),
        window_duration_mins: if window_kind == "5h" { 300 } else { 10_080 },
        resets_at: if window_kind == "5h" {
            2_000_000_000
        } else {
            2_000_500_000
        },
        used_fraction_units,
        measurement_resolution_fraction_units: 100_000,
        observed_at,
    }
}

fn gemini_snapshot(
    bucket_id: &str,
    window_kind: &str,
    used_fraction_units: i64,
    observed_at: i64,
) -> GeminiQuotaSnapshot {
    GeminiQuotaSnapshot {
        bucket_id: bucket_id.to_owned(),
        window_kind: window_kind.to_owned(),
        window_duration_mins: if window_kind == "5h" { 300 } else { 10_080 },
        resets_at: if window_kind == "5h" {
            2_000_000_000
        } else {
            2_000_500_000
        },
        used_fraction_units,
        measurement_resolution_fraction_units: 100_000,
        observed_at,
    }
}

fn kimi_event(
    request_id: &str,
    api_total_nanousd: i64,
    completed_at: i64,
) -> KimiTurnCalibrationEvent {
    KimiTurnCalibrationEvent {
        request_id: request_id.into(),
        subject_id: "kimi-subject-a".into(),
        plan: "Moderato".into(),
        requested_model: "kimi-for-coding".into(),
        served_model: "kimi-k2.7-code".into(),
        context_mode: "256k".into(),
        reasoning_effort: "high".into(),
        tariff_schedule_id: "moonshot/test/v1".into(),
        priced_ts: completed_at,
        completed_at,
        input_tokens: 1,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        api_input_nanousd: api_total_nanousd,
        api_cache_read_nanousd: 0,
        api_cache_write_nanousd: 0,
        api_output_nanousd: 0,
        api_total_nanousd,
    }
}

fn kimi_snapshot(duration_secs: i64, used: i64, limit: i64, observed_at: i64) -> KimiQuotaSnapshot {
    let fraction = registry::kimi_fraction_from_native(used, limit).unwrap();
    KimiQuotaSnapshot {
        window_duration_secs: duration_secs,
        window_name: Some(
            if duration_secs == registry::KIMI_ROLLING_WINDOW_SECS {
                "rate"
            } else {
                "weekly"
            }
            .into(),
        ),
        resets_at: if duration_secs == registry::KIMI_ROLLING_WINDOW_SECS {
            2_000_000_000
        } else {
            2_000_500_000
        },
        observed_at,
        native_used_units: used,
        native_limit_units: limit,
        used_fraction_units: fraction.used_fraction_units,
        measurement_resolution_fraction_units: fraction.measurement_resolution_fraction_units,
    }
}

fn codex_event(
    request_id: &str,
    api_total_nanousd: i64,
    chatgpt_total_nanocredits: i64,
    completed_at: i64,
) -> CodexTurnCalibrationEvent {
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
}

#[tokio::test]
async fn detached_dispatch_tracker_waits_for_a_backpressured_enqueue() {
    let tracker = Arc::new(DetachedDispatchTracker::default());
    let (writer, mut receiver) = mpsc::channel(1);
    let (first_reply, _first_result) = oneshot::channel();
    assert!(writer.try_send(WriteCmd::Flush(first_reply)).is_ok());
    let (second_reply, _second_result) = oneshot::channel();
    dispatch_detached(&writer, &tracker, WriteCmd::Flush(second_reply));

    let wait_tracker = tracker.clone();
    let waiter = tokio::spawn(async move { wait_tracker.wait_idle().await });
    tokio::task::yield_now().await;
    assert!(!waiter.is_finished());

    assert!(matches!(receiver.recv().await, Some(WriteCmd::Flush(_))));
    tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("backpressured detached command never entered the FIFO")
        .unwrap();
    assert!(matches!(receiver.recv().await, Some(WriteCmd::Flush(_))));
}

#[tokio::test]
async fn codex_health_survives_billing_actor_restart() {
    let unique = std::process::id();
    let path = std::env::temp_dir().join(format!("claude-api-codex-health-{unique}.sqlite"));
    let _ = std::fs::remove_file(&path);
    let db = path.to_str().unwrap().to_string();

    {
        let billing = AsyncBilling::start(db.clone(), 1).unwrap();
        // Unknown home reads back healthy: absence of evidence is not evidence of a fault.
        let fresh = billing.load_codex_health("home-a").await.unwrap();
        assert_eq!(fresh.account_state, "healthy");

        billing
            .save_codex_health(
                "home-a",
                registry::CodexHomeHealthRow {
                    account_state: "dead".to_string(),
                    auth_fail_streak: 2,
                    first_auth_fail_ts: 1_000,
                    cooling_until: 1_900,
                },
                2_000,
            )
            .await
            .unwrap();
    }

    // A new actor over the same authority is what a blue-green handoff looks like to the pool.
    let billing = AsyncBilling::start(db, 1).unwrap();
    let restored = billing.load_codex_health("home-a").await.unwrap();
    assert_eq!(restored.account_state, "dead");
    assert_eq!(restored.auth_fail_streak, 2);
    assert_eq!(restored.cooling_until, 1_900);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn codex_calibration_survives_billing_actor_restart() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-codex-calibration-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();

    let first = AsyncBilling::start(path_string.clone(), 1).unwrap();
    let totals = first
        .record_codex_turn(codex_event(
            "request-1",
            100_000_000_000,
            10_000_000_000,
            100,
        ))
        .await
        .unwrap();
    assert_eq!(totals.spent_nano, 100_000_000_000);
    assert_eq!(totals.spent_nanocredits, Some(10_000_000_000));
    let (_, anchor) = first
        .observe_codex_window("home-a", 300, 2_000_000_000, 10, 10_000_000, 100)
        .await
        .unwrap();
    assert!(anchor.current_capacity_nano.is_none());
    first
        .record_codex_turn(codex_event("request-2", 40_000_000_000, 4_000_000_000, 101))
        .await
        .unwrap();
    let (_, measured) = first
        .observe_codex_window("home-a", 300, 2_000_000_000, 12, 12_000_000, 101)
        .await
        .unwrap();
    assert_eq!(measured.current_capacity_nano, Some(2_000_000_000_000));
    assert_eq!(measured.current_capacity_nanocredits, Some(200_000_000_000));
    assert_eq!(measured.samples, 1);
    assert!(measured.anchor_ready);
    first
        .record_codex_turn(codex_event("request-3", 40_000_000_000, 4_000_000_000, 102))
        .await
        .unwrap();
    let (_, measured) = first
        .observe_codex_window("home-a", 300, 2_000_000_000, 14, 14_000_000, 102)
        .await
        .unwrap();
    assert_eq!(measured.current_capacity_nano, Some(2_000_000_000_000));
    assert_eq!(measured.current_capacity_nanocredits, Some(200_000_000_000));
    assert_eq!(measured.samples, 2);
    let (_, duplicate) = first
        .observe_codex_window("home-a", 300, 2_000_000_000, 14, 14_000_000, 102)
        .await
        .unwrap();
    assert_eq!(duplicate.version, measured.version);
    first.flush().await.unwrap();
    drop(first);

    // Simulate the exact production upgrade case: raw observations are intact while the
    // derived v2 row contains a transient one-interval estimate. The restarted actor must
    // replay raw evidence before returning capacity.
    let connection = registry::open(&path_string).unwrap();
    connection
        .execute(
            "UPDATE codex_window_calibrations SET estimator_version=2, \
               current_capacity_nano=187994100000,sum_used_sq=1,\
               sum_used_spend_nano=1879941000,samples=1,observed_points=1,anchor_ready=0 \
             WHERE home_id='home-a' AND window_duration_mins=300",
            [],
        )
        .unwrap();
    // Migration-first blue-green overlap once allowed an old runtime to append an API-only
    // observation after native-credit tracking had started. The immutable residue must remain
    // auditable, but it must not permanently block a v9 history rebuild.
    connection
        .execute(
            "INSERT INTO codex_window_observations(\
               home_id,window_duration_mins,resets_at,observed_at,used_percent,\
               used_fraction_units,gateway_spend_nano,gateway_spend_nanocredits) \
             VALUES('home-a',300,2000000000,103,14,14000000,180000000000,NULL)",
            [],
        )
        .unwrap();
    drop(connection);

    let restarted = AsyncBilling::start(path_string, 1).unwrap();
    let (spend, restored) = restarted
        .observe_codex_window("home-a", 300, 2_000_000_000, 14, 14_000_000, 103)
        .await
        .unwrap();
    assert_eq!(spend.spent_nano, 180_000_000_000);
    assert_eq!(spend.spent_nanocredits, Some(18_000_000_000));
    assert_eq!(restored.estimator_version, crate::codex::ESTIMATOR_VERSION);
    assert_eq!(restored.current_capacity_nano, Some(2_000_000_000_000));
    assert_eq!(restored.current_capacity_nanocredits, Some(200_000_000_000));
    assert_eq!(restored.observed_at, 103);
    let report = restarted.codex_calibration_report().await.unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].turns, 3);
    assert_eq!(report[0].api_total_nanousd, 180_000_000_000);
    assert!(restored.version > measured.version);
    restarted.flush().await.unwrap();
    drop(restarted);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn anthropic_admin_turns_are_exact_idempotent_and_calibrate_both_windows() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-anthropic-calibration-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let first = AsyncBilling::start(path_string.clone(), 1).unwrap();

    // No account, key reservation or customer usage_event exists: provider capacity evidence
    // is deliberately independent and therefore includes successful operator/admin traffic.
    let anchor_event = anthropic_event("request-1", 1_000_000_000, 100);
    let (anchor_spend, anchors) = first
        .record_anthropic_turn(
            anchor_event.clone(),
            "max20",
            vec![
                anthropic_snapshot("5h", 10_000_000, 100),
                anthropic_snapshot("7d", 20_000_000, 100),
            ],
        )
        .await
        .unwrap();
    assert!(anchor_spend.inserted);
    assert_eq!(anchor_spend.spent_nano, 1_000_000_000);
    assert_eq!(anchors.len(), 2);
    assert!(anchors
        .iter()
        .all(|row| row.current_capacity_nano.is_none()));

    let (replayed_spend, replayed) = first
        .record_anthropic_turn(
            anchor_event,
            "max20",
            vec![
                anthropic_snapshot("5h", 10_000_000, 100),
                anthropic_snapshot("7d", 20_000_000, 100),
            ],
        )
        .await
        .unwrap();
    assert!(!replayed_spend.inserted);
    assert_eq!(replayed_spend.spent_nano, 1_000_000_000);
    assert_eq!(
        replayed.iter().map(|row| row.version).collect::<Vec<_>>(),
        anchors.iter().map(|row| row.version).collect::<Vec<_>>(),
    );

    let measured_event = anthropic_event("request-2", 2_000_000_000, 101);
    let (measured_spend, measured) = first
        .record_anthropic_turn(
            measured_event.clone(),
            "max20",
            vec![
                anthropic_snapshot("5h", 14_000_000, 101),
                anthropic_snapshot("7d", 21_000_000, 101),
            ],
        )
        .await
        .unwrap();
    assert_eq!(measured_spend.spent_nano, 3_000_000_000);
    assert_eq!(measured[0].window_kind, "5h");
    assert_eq!(measured[0].current_capacity_nano, Some(50_000_000_000));
    assert_eq!(measured[1].window_kind, "7d");
    assert_eq!(measured[1].current_capacity_nano, Some(200_000_000_000));

    let mut conflict = measured_event;
    conflict.input_tokens += 1;
    let error = first
        .record_anthropic_turn(conflict, "max20", Vec::new())
        .await
        .unwrap_err();
    assert!(registry::is_provider_turn_calibration_replay_conflict(
        &error
    ));
    let conflicted_status = first.anthropic_calibration_delivery_status();
    assert_eq!(conflicted_status.pending_events, 0);
    assert_eq!(conflicted_status.dropped_events, 1);
    assert!(!conflicted_status.persistence_ok);

    // A poll can move quota but never advances spend. The first unmatched movement is held for
    // one snapshot; seeing the same point again excludes it as unattributed instead of
    // manufacturing a larger dollar capacity.
    let (_, lagged) = first
        .observe_anthropic_window(
            "operator@example.test",
            "max20",
            "5h",
            300,
            2_000_000_000,
            15_000_000,
            100_000,
            102,
        )
        .await
        .unwrap();
    assert_eq!(lagged.unattributed_fraction_units, 0);
    let (spend, excluded) = first
        .observe_anthropic_window(
            "operator@example.test",
            "max20",
            "5h",
            300,
            2_000_000_000,
            15_000_000,
            100_000,
            103,
        )
        .await
        .unwrap();
    assert_eq!(spend, 3_000_000_000);
    assert_eq!(excluded.unattributed_fraction_units, 1_000_000);
    assert_eq!(excluded.current_capacity_nano, Some(50_000_000_000));

    // A poisoned request id is quarantined; it cannot pin later valid evidence behind the
    // immutable conflict forever.
    let (post_conflict, _) = first
        .record_anthropic_turn(
            anthropic_event("request-after-conflict", 1, 104),
            "max20",
            Vec::new(),
        )
        .await
        .unwrap();
    assert!(post_conflict.inserted);
    assert_eq!(post_conflict.spent_nano, 3_000_000_001);
    assert_eq!(
        first.anthropic_calibration_delivery_status().pending_events,
        0
    );

    first.flush().await.unwrap();
    drop(first);

    let restarted = AsyncBilling::start(path_string, 1).unwrap();
    let (rows, evidence, recent_turns) = restarted.anthropic_calibration_report().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].turns, 3);
    assert_eq!(evidence[0].api_total_nanousd, 3_000_000_001);
    assert_eq!(recent_turns.len(), 3);

    restarted.flush().await.unwrap();
    drop(restarted);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn backend_post_turn_poll_calibrates_without_a_second_customer_request() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-anthropic-post-turn-poll-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let billing = AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();

    for (kind, duration, used, reset) in [
        ("5h", 300, 10_000_000, 2_000_000_000),
        ("7d", 10_080, 20_000_000, 2_000_500_000),
    ] {
        let (spend, anchor) = billing
            .observe_anthropic_window(
                "operator@example.test",
                "max20",
                kind,
                duration,
                reset,
                used,
                100_000,
                100,
            )
            .await
            .unwrap();
        assert_eq!(spend, 0);
        assert!(anchor.current_capacity_nano.is_none());
    }

    // Response headers and the fast post-turn poll can share one wall-clock second. The headers
    // still carry the old fraction; FIFO ordering plus the later changed fraction must be
    // sufficient to finish both intervals without waiting for another customer request.
    let (spend, response_rows) = billing
        .record_anthropic_turn(
            anthropic_event("post-turn-only", 2_000_000_000, 101),
            "max20",
            vec![
                AnthropicQuotaSnapshot {
                    window_kind: "5h".to_owned(),
                    window_duration_mins: 300,
                    resets_at: 2_000_000_000,
                    used_fraction_units: 10_000_000,
                    measurement_resolution_fraction_units: 100_000,
                    observed_at: 101,
                },
                AnthropicQuotaSnapshot {
                    window_kind: "7d".to_owned(),
                    window_duration_mins: 10_080,
                    resets_at: 2_000_500_000,
                    used_fraction_units: 20_000_000,
                    measurement_resolution_fraction_units: 100_000,
                    observed_at: 101,
                },
            ],
        )
        .await
        .unwrap();
    assert_eq!(spend.spent_nano, 2_000_000_000);
    assert_eq!(response_rows.len(), 2);

    let (_, five_hour) = billing
        .observe_anthropic_window(
            "operator@example.test",
            "max20",
            "5h",
            300,
            2_000_000_000,
            14_000_000,
            100_000,
            101,
        )
        .await
        .unwrap();
    let (_, weekly) = billing
        .observe_anthropic_window(
            "operator@example.test",
            "max20",
            "7d",
            10_080,
            2_000_500_000,
            21_000_000,
            100_000,
            101,
        )
        .await
        .unwrap();
    assert_eq!(five_hour.current_capacity_nano, Some(50_000_000_000));
    assert_eq!(weekly.current_capacity_nano, Some(200_000_000_000));

    let (rows, evidence, recent_turns) = billing.anthropic_calibration_report().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].turns, 1);
    assert_eq!(recent_turns[0].request_id, "post-turn-only");
    billing.flush().await.unwrap();
    drop(billing);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn anthropic_outage_recovery_replays_turns_fifo_before_poll_snapshot() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-anthropic-fifo-recovery-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
    let control = registry::open(&path_string).unwrap();
    control
        .execute_batch(
            "CREATE TRIGGER reject_anthropic_calibration_turn \
             BEFORE INSERT ON provider_turn_calibration_events \
             BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
        )
        .unwrap();

    let first = billing
        .record_anthropic_turn(
            anthropic_event("fifo-first", 1_000_000_000, 100),
            "max20",
            vec![anthropic_snapshot("5h", 10_000_000, 100)],
        )
        .await;
    assert!(first.is_err());
    let second = billing
        .record_anthropic_turn(
            anthropic_event("fifo-second", 2_000_000_000, 101),
            "max20",
            vec![anthropic_snapshot("5h", 14_000_000, 101)],
        )
        .await;
    assert!(second.is_err());
    assert_eq!(
        billing.anthropic_calibration_delivery_status(),
        AnthropicCalibrationDeliveryStatus {
            pending_events: 2,
            dropped_events: 0,
            persistence_ok: false,
            queue_limit: MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS,
        }
    );

    control
        .execute_batch("DROP TRIGGER reject_anthropic_calibration_turn;")
        .unwrap();
    let (spend, row) = billing
        .observe_anthropic_window(
            "operator@example.test",
            "max20",
            "5h",
            300,
            2_000_000_000,
            14_000_000,
            100_000,
            102,
        )
        .await
        .unwrap();
    assert_eq!(spend, 3_000_000_000);
    assert_eq!(row.current_capacity_nano, Some(50_000_000_000));
    assert_eq!(
        billing.anthropic_calibration_delivery_status(),
        AnthropicCalibrationDeliveryStatus {
            pending_events: 0,
            dropped_events: 0,
            persistence_ok: true,
            queue_limit: MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS,
        }
    );

    let ids = {
        let mut statement = control
            .prepare("SELECT request_id FROM provider_turn_calibration_events ORDER BY rowid")
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(ids, ["fifo-first", "fifo-second"]);

    billing.flush().await.unwrap();
    drop(billing);
    drop(control);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn anthropic_flush_retries_detached_pending_turn_before_shutdown() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-anthropic-shutdown-drain-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
    let control = registry::open(&path_string).unwrap();
    control
        .execute_batch(
            "CREATE TRIGGER reject_anthropic_calibration_turn \
             BEFORE INSERT ON provider_turn_calibration_events \
             BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
        )
        .unwrap();

    billing.record_anthropic_turn_detached(
        anthropic_event("shutdown-pending", 1_000_000_000, 100),
        "max20",
        vec![anthropic_snapshot("5h", 10_000_000, 100)],
    );
    assert!(billing.flush_once().await.is_err());
    assert_eq!(
        billing
            .anthropic_calibration_delivery_status()
            .pending_events,
        1
    );

    control
        .execute_batch("DROP TRIGGER reject_anthropic_calibration_turn;")
        .unwrap();
    billing.flush().await.unwrap();
    assert_eq!(
        billing.anthropic_calibration_delivery_status(),
        AnthropicCalibrationDeliveryStatus {
            pending_events: 0,
            dropped_events: 0,
            persistence_ok: true,
            queue_limit: MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS,
        }
    );
    let (_, evidence, recent_turns) = billing.anthropic_calibration_report().await.unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].turns, 1);
    assert_eq!(recent_turns.len(), 1);

    drop(billing);
    drop(control);
    let _ = std::fs::remove_file(path);
}

#[test]
fn anthropic_pending_queue_is_bounded_and_counts_dropped_evidence() {
    let state = AnthropicCalibrationDeliveryState::default();
    for index in 0..MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS {
        enqueue_anthropic_calibration_turn(
            &state,
            anthropic_event(&format!("bounded-{index}"), 1, 100),
            "max20".to_owned(),
            Vec::new(),
        )
        .unwrap();
    }
    assert!(enqueue_anthropic_calibration_turn(
        &state,
        anthropic_event("bounded-overflow", 1, 100),
        "max20".to_owned(),
        Vec::new(),
    )
    .is_err());
    assert_eq!(
        state
            .queue
            .lock()
            .expect("Anthropic calibration delivery queue lock")
            .pending
            .len(),
        MAX_PENDING_ANTHROPIC_CALIBRATION_EVENTS
    );
    assert_eq!(state.dropped_events.load(Ordering::Relaxed), 1);
    assert!(!state.persistence_ok.load(Ordering::Relaxed));
}

#[tokio::test]
async fn identical_plan_credits_converge_while_api_usd_remains_workload_dependent() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-codex-like-for-like-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let billing = AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap();
    let mut native_capacities = Vec::new();
    let mut api_capacities = Vec::new();
    for (index, interval_api_nano) in [
        40_000_000_000,
        20_000_000_000,
        80_000_000_000,
        10_000_000_000,
    ]
    .into_iter()
    .enumerate()
    {
        let home_id = format!("pro-home-{index}");
        let mut anchor = codex_event(
            &format!("anchor-{index}"),
            10_000_000_000,
            1_000_000_000,
            100,
        );
        anchor.home_id = home_id.clone();
        billing.record_codex_turn(anchor).await.unwrap();
        billing
            .observe_codex_window(&home_id, 300, 2_000_000_000, 10, 10_000_000, 100)
            .await
            .unwrap();

        let mut measured = codex_event(
            &format!("measured-{index}"),
            interval_api_nano,
            4_000_000_000,
            101,
        );
        measured.home_id = home_id.clone();
        billing.record_codex_turn(measured).await.unwrap();
        let (_, row) = billing
            .observe_codex_window(&home_id, 300, 2_000_000_000, 12, 12_000_000, 101)
            .await
            .unwrap();
        native_capacities.push(row.current_capacity_nanocredits.unwrap());
        api_capacities.push(row.current_capacity_nano.unwrap());
    }

    assert_eq!(native_capacities, vec![200_000_000_000; 4]);
    assert_eq!(
        api_capacities,
        vec![
            2_000_000_000_000,
            1_000_000_000_000,
            4_000_000_000_000,
            500_000_000_000,
        ]
    );
    assert_eq!(
        billing
            .codex_calibration_report()
            .await
            .unwrap()
            .iter()
            .map(|row| row.turns)
            .sum::<i64>(),
        8
    );
    billing.flush().await.unwrap();
    drop(billing);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn gemini_exact_turns_calibrate_first_interval_and_keep_windows_independent() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-gemini-calibration-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let first = AsyncBilling::start(path_string.clone(), 1).unwrap();

    first
        .record_gemini_turn(
            gemini_event("gemini-1", 10_000, 100),
            "google_ai_pro",
            vec![],
        )
        .await
        .unwrap();
    let (_, anchor) = first
        .observe_gemini_window(
            "profile-a",
            "google_ai_pro",
            "gemini-5h",
            "5h",
            300,
            2_000_000_000,
            1_000,
            1,
            100,
        )
        .await
        .unwrap();
    assert!(anchor.current_capacity_nano.is_none());

    first
        .record_gemini_turn(
            gemini_event("gemini-2", 20_000, 101),
            "google_ai_pro",
            vec![],
        )
        .await
        .unwrap();
    let (_, measured) = first
        .observe_gemini_window(
            "profile-a",
            "google_ai_pro",
            "gemini-5h",
            "5h",
            300,
            2_000_000_000,
            2_000,
            1,
            101,
        )
        .await
        .unwrap();
    assert_eq!(measured.current_capacity_nano, Some(2_000_000_000));
    assert_eq!(measured.observed_spend_nano, 20_000);

    let (_, weekly) = first
        .observe_gemini_window(
            "profile-a",
            "google_ai_pro",
            "gemini-weekly",
            "weekly",
            10_080,
            2_000_500_000,
            500,
            1,
            102,
        )
        .await
        .unwrap();
    assert!(weekly.current_capacity_nano.is_none());
    drop(first);

    let second = AsyncBilling::start(path_string, 1).unwrap();
    let (_, restored) = second
        .observe_gemini_window(
            "profile-a",
            "google_ai_pro",
            "gemini-5h",
            "5h",
            300,
            2_000_000_000,
            2_000,
            1,
            103,
        )
        .await
        .unwrap();
    assert_eq!(restored.current_capacity_nano, Some(2_000_000_000));
    assert_eq!(restored.observed_spend_nano, 20_000);
    assert_eq!(restored.samples, 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn gemini_equal_second_settlement_catch_up_is_not_filtered_as_a_duplicate() {
    let anchor = gemini_observation(
        "profile-a",
        "google_ai_pro",
        &gemini_snapshot("gemini-5h", "5h", 10_000_000, 100),
        1_000,
        "response",
        Some("gemini-anchor"),
    );
    let pending = gemini_observation(
        "profile-a",
        "google_ai_pro",
        &gemini_snapshot("gemini-5h", "5h", 20_000_000, 101),
        1_000,
        "response",
        Some("gemini-pending"),
    );
    let anchor_row = crate::gemini::apply_observation_with_history(None, &[], &anchor).unwrap();
    let pending_row =
        crate::gemini::apply_observation_with_history(Some(anchor_row), &[], &pending).unwrap();
    assert!(gemini_observation_is_stale_or_duplicate(
        &pending_row,
        &pending
    ));

    let catch_up = GeminiExactWindowObservation {
        gateway_spend_nano: 2_001_000,
        observation_source: "poll".to_owned(),
        source_request_id: None,
        ..pending
    };
    assert!(!gemini_observation_is_stale_or_duplicate(
        &pending_row,
        &catch_up
    ));
    let settled =
        crate::gemini::apply_observation_with_history(Some(pending_row), &[], &catch_up).unwrap();
    assert_eq!(settled.samples, 1);
    assert_eq!(settled.current_capacity_nano, Some(20_000_000));
}

#[tokio::test]
async fn gemini_outage_recovery_replays_fifo_before_poll_snapshot() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-gemini-fifo-recovery-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
    let control = registry::open(&path_string).unwrap();
    control
        .execute_batch(
            "CREATE TRIGGER reject_gemini_calibration_turn \
             BEFORE INSERT ON provider_turn_calibration_events \
             WHEN NEW.provider='google' \
             BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
        )
        .unwrap();

    let first = billing
        .record_gemini_turn(
            gemini_event("gemini-fifo-first", 1_000_000_000, 100),
            "google_ai_pro",
            vec![gemini_snapshot("gemini-5h", "5h", 10_000_000, 100)],
        )
        .await;
    assert!(first.is_err());
    let second = billing
        .record_gemini_turn(
            gemini_event("gemini-fifo-second", 2_000_000_000, 101),
            "google_ai_pro",
            vec![gemini_snapshot("gemini-5h", "5h", 14_000_000, 101)],
        )
        .await;
    assert!(second.is_err());
    assert_eq!(
        billing.gemini_calibration_delivery_status(),
        GeminiCalibrationDeliveryStatus {
            pending_events: 2,
            dropped_events: 0,
            persistence_ok: false,
            queue_limit: MAX_PENDING_GEMINI_CALIBRATION_EVENTS,
        }
    );

    let blocked_poll = billing
        .observe_gemini_window(
            "profile-a",
            "google_ai_pro",
            "gemini-5h",
            "5h",
            300,
            2_000_000_000,
            14_000_000,
            100_000,
            102,
        )
        .await;
    assert!(blocked_poll.is_err());
    assert_eq!(
        control
            .query_row(
                "SELECT COUNT(*) FROM gemini_exact_window_observations",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0,
        "a free quota poll must not overtake pending paid-turn evidence"
    );

    control
        .execute_batch("DROP TRIGGER reject_gemini_calibration_turn;")
        .unwrap();
    let (spend, row) = billing
        .observe_gemini_window(
            "profile-a",
            "google_ai_pro",
            "gemini-5h",
            "5h",
            300,
            2_000_000_000,
            14_000_000,
            100_000,
            103,
        )
        .await
        .unwrap();
    assert_eq!(spend, 3_000_000_000);
    assert_eq!(row.current_capacity_nano, Some(50_000_000_000));
    assert_eq!(
        billing.gemini_calibration_delivery_status(),
        GeminiCalibrationDeliveryStatus {
            pending_events: 0,
            dropped_events: 0,
            persistence_ok: true,
            queue_limit: MAX_PENDING_GEMINI_CALIBRATION_EVENTS,
        }
    );
    let ids = {
        let mut statement = control
            .prepare(
                "SELECT request_id FROM provider_turn_calibration_events \
                 WHERE provider='google' ORDER BY rowid",
            )
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(ids, ["gemini-fifo-first", "gemini-fifo-second"]);

    billing.flush().await.unwrap();
    drop(billing);
    drop(control);
    let _ = std::fs::remove_file(path);
}

#[test]
fn gemini_replay_conflict_quarantines_only_the_corrupt_event() {
    let connection = registry::open(":memory:").unwrap();
    registry::record_provider_turn_calibration_event(
        &connection,
        &gemini_event("gemini-conflict", 1_000, 100),
    )
    .unwrap();
    let state = GeminiCalibrationDeliveryState::default();
    enqueue_gemini_calibration_turn(
        &state,
        gemini_event("gemini-conflict", 2_000, 100),
        "google_ai_pro".to_owned(),
        Vec::new(),
    )
    .unwrap();
    enqueue_gemini_calibration_turn(
        &state,
        gemini_event("gemini-after-conflict", 3_000, 101),
        "google_ai_pro".to_owned(),
        Vec::new(),
    )
    .unwrap();

    flush_pending_gemini_calibration_turns(&state, None, |turn| {
        let spend = registry::record_provider_turn_calibration_event(&connection, &turn.event)?;
        Ok((spend, Vec::new()))
    })
    .unwrap();

    assert_eq!(
        state
            .queue
            .lock()
            .expect("Gemini calibration delivery queue lock")
            .pending
            .len(),
        0
    );
    assert_eq!(state.dropped_events.load(Ordering::Relaxed), 1);
    assert!(!state.persistence_ok.load(Ordering::Relaxed));
    assert_eq!(
        registry::provider_calibration_subject_spend(
            &connection,
            registry::PROVIDER_GOOGLE,
            "profile-a",
        )
        .unwrap()
        .spent_nano,
        4_000
    );
    let report =
        registry::provider_turn_calibration_report(&connection, registry::PROVIDER_GOOGLE).unwrap();
    assert_eq!(report.iter().map(|row| row.turns).sum::<i64>(), 2);
}

#[tokio::test]
async fn gemini_flush_retries_detached_pending_turn_before_shutdown() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-gemini-shutdown-drain-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let billing = AsyncBilling::start(path_string.clone(), 1).unwrap();
    let control = registry::open(&path_string).unwrap();
    control
        .execute_batch(
            "CREATE TRIGGER reject_gemini_calibration_turn \
             BEFORE INSERT ON provider_turn_calibration_events \
             WHEN NEW.provider='google' \
             BEGIN SELECT RAISE(FAIL, 'simulated authority outage'); END;",
        )
        .unwrap();

    billing.record_gemini_turn_detached(
        gemini_event("gemini-shutdown-pending", 1_000_000_000, 100),
        "google_ai_pro",
        vec![gemini_snapshot("gemini-5h", "5h", 10_000_000, 100)],
    );
    assert!(billing.flush_once().await.is_err());
    assert_eq!(
        billing.gemini_calibration_delivery_status().pending_events,
        1
    );

    control
        .execute_batch("DROP TRIGGER reject_gemini_calibration_turn;")
        .unwrap();
    billing.flush().await.unwrap();
    assert_eq!(
        billing.gemini_calibration_delivery_status(),
        GeminiCalibrationDeliveryStatus {
            pending_events: 0,
            dropped_events: 0,
            persistence_ok: true,
            queue_limit: MAX_PENDING_GEMINI_CALIBRATION_EVENTS,
        }
    );
    let (_, evidence, recent_turns) = billing.gemini_calibration_report().await.unwrap();
    assert_eq!(evidence.iter().map(|row| row.turns).sum::<i64>(), 1);
    assert_eq!(recent_turns.len(), 1);

    drop(billing);
    drop(control);
    let _ = std::fs::remove_file(path);
}

#[test]
fn gemini_pending_queue_is_bounded_and_counts_dropped_evidence() {
    let state = GeminiCalibrationDeliveryState::default();
    for index in 0..MAX_PENDING_GEMINI_CALIBRATION_EVENTS {
        enqueue_gemini_calibration_turn(
            &state,
            gemini_event(&format!("gemini-bounded-{index}"), 1, 100),
            "google_ai_pro".to_owned(),
            Vec::new(),
        )
        .unwrap();
    }
    assert!(enqueue_gemini_calibration_turn(
        &state,
        gemini_event("gemini-bounded-overflow", 1, 100),
        "google_ai_pro".to_owned(),
        Vec::new(),
    )
    .is_err());
    assert_eq!(
        state
            .queue
            .lock()
            .expect("Gemini calibration delivery queue lock")
            .pending
            .len(),
        MAX_PENDING_GEMINI_CALIBRATION_EVENTS
    );
    assert_eq!(state.dropped_events.load(Ordering::Relaxed), 1);
    assert!(!state.persistence_ok.load(Ordering::Relaxed));
}

#[tokio::test]
async fn canceled_sqlite_reserve_handoff_releases_key_allowance() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "claude-api-billing-handoff-{}-{unique}.sqlite",
        std::process::id(),
    ));
    let path_string = path.to_string_lossy().into_owned();
    let billing = AsyncBilling::start(path_string, 1).unwrap();
    billing.create_account("acct", None, 10_000).await.unwrap();
    assert_eq!(
        billing.topup("acct", 1_000, Some("seed")).await.unwrap(),
        Some(1_000)
    );
    billing
        .issue_key("limited", "acct", None, Some(700), None)
        .await
        .unwrap();

    let handoff = Arc::new(AtomicU8::new(RESERVE_HANDOFF_CANCELED));
    let (reply, response) = oneshot::channel();
    billing
        .writer
        .send(WriteCmd::Reserve {
            request_id: "canceled-before-handoff".into(),
            account_id: "acct".into(),
            key: "limited".into(),
            hold: 500,
            execution: registry::ExecutionAttempt::direct(),
            pricing: None,
            handoff: Arc::clone(&handoff),
            reply,
        })
        .await
        .unwrap();
    assert!(response.await.is_err());
    billing.flush().await.unwrap();

    let account = billing.account("acct").await.unwrap().unwrap();
    let key = billing.get("limited").await.unwrap().unwrap();
    assert_eq!((account.balance_nano, account.reserved_nano), (1_000, 0));
    assert_eq!(key.reserved_nano, 0);
    assert_eq!(handoff.load(Ordering::Acquire), RESERVE_HANDOFF_REFUNDED);

    drop(billing);
    let _ = std::fs::remove_file(path);
}

#[test]
fn kimi_postgres_actor_pairs_spend_before_independent_window_cas() {
    const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping KIMI PostgreSQL actor matrix: CLAUDE_API_TEST_DATABASE_URL is unset");
        return;
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let instance_id = format!("kimi-actor-{}-{unique}", std::process::id());
    let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock_holder
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    let owner = pg.claim_instance(&instance_id, 60).unwrap();
    drop(pg);

    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::Postgres { url: url.clone() },
        Some(owner),
        1,
        0,
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let first = vec![
            kimi_snapshot(registry::KIMI_ROLLING_WINDOW_SECS, 100, 1_000, 100),
            kimi_snapshot(registry::KIMI_WEEKLY_WINDOW_SECS, 200, 1_000, 100),
        ];
        let anchored = billing
            .observe_kimi_windows("kimi-subject-a", "Moderato", first)
            .await
            .unwrap();
        assert_eq!(anchored.len(), 2);
        assert!(anchored
            .iter()
            .all(|row| row.samples == 0 && row.version == 1));

        assert!(billing
            .record_kimi_turn(kimi_event("kimi-actor-turn", 1_000_000_000, 101))
            .await
            .unwrap());
        let second = vec![
            kimi_snapshot(registry::KIMI_ROLLING_WINDOW_SECS, 110, 1_000, 102),
            kimi_snapshot(registry::KIMI_WEEKLY_WINDOW_SECS, 230, 1_000, 102),
        ];
        let measured = billing
            .observe_kimi_windows("kimi-subject-a", "Moderato", second.clone())
            .await
            .unwrap();
        assert_eq!(measured.len(), 2);
        assert!(measured.iter().all(|row| {
            row.samples == 1
                && row.observed_spend_nano == 1_000_000_000
                && row.current_capacity_nano.is_some()
                && row.version == 2
        }));

        // Exact replay is idempotent: no extra immutable row, sample or CAS version.
        let replay = billing
            .observe_kimi_windows("kimi-subject-a", "Moderato", second)
            .await
            .unwrap();
        assert!(replay
            .iter()
            .all(|row| row.samples == 1 && row.version == 2));
        billing.flush().await.unwrap();
    });
    drop(billing);

    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    assert_eq!(
        pg.kimi_subject_spend("kimi-subject-a").unwrap(),
        1_000_000_000
    );
    for duration in [
        registry::KIMI_ROLLING_WINDOW_SECS,
        registry::KIMI_WEEKLY_WINDOW_SECS,
    ] {
        let history = pg
            .load_kimi_window_observations("kimi-subject-a", "Moderato", duration)
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].cumulative_api_spend_nano, 0);
        assert_eq!(history[1].cumulative_api_spend_nano, 1_000_000_000);
    }
    lock_holder
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

#[tokio::test]
async fn kimi_calibration_report_is_empty_on_a_sqlite_authority() {
    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
        None,
        1,
        0,
    )
    .unwrap();
    // KIMI calibration authority is PostgreSQL-only: the report is empty, not an error,
    // while the evidence commands themselves keep refusing the SQLite authority.
    assert_eq!(billing.kimi_calibration_report().await.unwrap(), Vec::new());
    assert!(billing
        .record_kimi_turn(kimi_event("kimi-sqlite-turn", 1, 1))
        .await
        .is_err());
}

fn glm_event(
    request_id: &str,
    api_total_nanousd: i64,
    native_total_microcredits: i64,
    completed_at: i64,
) -> GlmTurnCalibrationEvent {
    GlmTurnCalibrationEvent {
        request_id: request_id.into(),
        subject_id: "glm-subject-a".into(),
        plan: "Pro".into(),
        requested_model: "glm-5.2".into(),
        served_model: "glm-5.2".into(),
        context_mode: "200k".into(),
        reasoning_effort: Some("high".into()),
        api_tariff_schedule_id: "zhipu/zai-open-platform/2026-08-03".into(),
        credit_schedule_id: "zhipu/glm-coding-plan-credits/2026-08-03".into(),
        priced_ts: completed_at,
        completed_at,
        fresh_input_tokens: 1,
        cached_input_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 1,
        reasoning_tokens: 0,
        api_fresh_input_nanousd: api_total_nanousd / 2,
        api_cached_input_nanousd: 0,
        api_output_nanousd: api_total_nanousd - api_total_nanousd / 2,
        api_total_nanousd,
        native_fresh_input_microcredits: native_total_microcredits / 2,
        native_cached_input_microcredits: 0,
        native_output_microcredits: native_total_microcredits - native_total_microcredits / 2,
        native_total_microcredits,
        off_peak: false,
    }
}

fn glm_snapshot(duration_secs: i64, used: i64, limit: i64, observed_at: i64) -> GlmQuotaSnapshot {
    let fraction = registry::glm_fraction_from_native(used, limit).unwrap();
    GlmQuotaSnapshot {
        window_duration_secs: duration_secs,
        resets_at: Some(observed_at + duration_secs),
        observed_at,
        native_used_units: Some(used),
        native_limit_units: Some(limit),
        native_remaining_units: Some(limit - used),
        percentage_raw: None,
        used_fraction_units: Some(fraction.used_fraction_units),
        measurement_resolution_fraction_units: Some(fraction.measurement_resolution_fraction_units),
    }
}

#[tokio::test]
async fn glm_calibration_commands_refuse_a_sqlite_authority() {
    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
        None,
        1,
        0,
    )
    .unwrap();
    // GLM calibration is PostgreSQL-only, like KIMI: evidence commands refuse the SQLite
    // authority rather than writing provider evidence somewhere it cannot be paired.
    assert!(billing
        .record_glm_turn(glm_event("glm-sqlite-turn", 2, 2, 1))
        .await
        .is_err());
    assert!(billing
        .observe_glm_windows(
            "glm-subject-a",
            "Pro",
            vec![glm_snapshot(registry::GLM_5H_WINDOW_SECS, 100, 1_000, 100)],
        )
        .await
        .is_err());
}

#[tokio::test]
async fn glm_calibration_report_is_empty_on_a_sqlite_authority() {
    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
        None,
        1,
        0,
    )
    .unwrap();
    // GLM calibration authority is PostgreSQL-only: the report is empty, not an error,
    // while the evidence commands themselves keep refusing the SQLite authority.
    assert_eq!(billing.glm_calibration_report().await.unwrap(), Vec::new());
    assert!(billing
        .record_glm_turn(glm_event("glm-sqlite-report-turn", 1, 1, 1))
        .await
        .is_err());
}

#[test]
fn glm_postgres_actor_pairs_dual_spend_before_independent_window_cas() {
    const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping GLM PostgreSQL actor matrix: CLAUDE_API_TEST_DATABASE_URL is unset");
        return;
    };
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let instance_id = format!("glm-actor-{}-{unique}", std::process::id());
    let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock_holder
        .batch_execute(
            "TRUNCATE glm_window_calibrations,glm_window_observations,\
             glm_calibration_subject_spend,glm_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    let owner = pg.claim_instance(&instance_id, 60).unwrap();
    drop(pg);

    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::Postgres { url: url.clone() },
        Some(owner),
        1,
        0,
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let first = vec![
            glm_snapshot(registry::GLM_5H_WINDOW_SECS, 100, 1_000, 100),
            glm_snapshot(registry::GLM_WEEKLY_WINDOW_SECS, 200, 1_000, 100),
        ];
        let anchored = billing
            .observe_glm_windows("glm-subject-a", "Pro", first)
            .await
            .unwrap();
        assert_eq!(anchored.len(), 2);
        assert!(anchored
            .iter()
            .all(|row| row.samples == 0 && row.version == 1));

        assert!(billing
            .record_glm_turn(glm_event("glm-actor-turn", 1_000_000_000, 500_000_000, 101))
            .await
            .unwrap());
        let second = vec![
            glm_snapshot(registry::GLM_5H_WINDOW_SECS, 110, 1_000, 102),
            glm_snapshot(registry::GLM_WEEKLY_WINDOW_SECS, 230, 1_000, 102),
        ];
        let measured = billing
            .observe_glm_windows("glm-subject-a", "Pro", second.clone())
            .await
            .unwrap();
        assert_eq!(measured.len(), 2);
        assert!(measured.iter().all(|row| {
            row.samples == 1
                && row.observed_spend_api_nanousd == 1_000_000_000
                && row.observed_spend_native_microcredits == 500_000_000
                && row.current_capacity_nanousd.is_some()
                && row.version == 2
        }));

        // Exact replay is idempotent: no extra immutable row, sample or CAS version.
        let replay = billing
            .observe_glm_windows("glm-subject-a", "Pro", second)
            .await
            .unwrap();
        assert!(replay
            .iter()
            .all(|row| row.samples == 1 && row.version == 2));
        billing.flush().await.unwrap();
    });
    drop(billing);

    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    // The two ledgers advanced independently and exactly; one is never the other rescaled.
    assert_eq!(
        pg.glm_subject_spend("glm-subject-a").unwrap(),
        GlmSubjectSpend {
            spent_api_nanousd: 1_000_000_000,
            spent_native_microcredits: 500_000_000,
        }
    );
    for duration in [
        registry::GLM_5H_WINDOW_SECS,
        registry::GLM_WEEKLY_WINDOW_SECS,
    ] {
        let history = pg
            .load_glm_window_observations("glm-subject-a", "Pro", duration)
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].cumulative_api_nanousd, 0);
        assert_eq!(history[0].cumulative_native_microcredits, 0);
        assert_eq!(history[1].cumulative_api_nanousd, 1_000_000_000);
        assert_eq!(history[1].cumulative_native_microcredits, 500_000_000);
    }
    lock_holder
        .batch_execute(
            "TRUNCATE glm_window_calibrations,glm_window_observations,\
             glm_calibration_subject_spend,glm_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

#[test]
fn glm_postgres_calibration_report_lists_every_subject_window() {
    const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping GLM PostgreSQL report matrix: CLAUDE_API_TEST_DATABASE_URL is unset");
        return;
    };
    let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock_holder
        .batch_execute(
            "TRUNCATE glm_window_calibrations,glm_window_observations,\
             glm_calibration_subject_spend,glm_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let instance_id = format!("glm-report-{}-{unique}", std::process::id());
    let owner = pg.claim_instance(&instance_id, 60).unwrap();
    drop(pg);

    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::Postgres { url: url.clone() },
        Some(owner),
        1,
        0,
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        billing
            .observe_glm_windows(
                "glm-subject-a",
                "Pro",
                vec![
                    glm_snapshot(registry::GLM_5H_WINDOW_SECS, 100, 1_000, 100),
                    glm_snapshot(registry::GLM_WEEKLY_WINDOW_SECS, 200, 1_000, 100),
                ],
            )
            .await
            .unwrap();
        billing
            .observe_glm_windows(
                "glm-subject-b",
                "Max",
                vec![glm_snapshot(registry::GLM_5H_WINDOW_SECS, 10, 1_000, 101)],
            )
            .await
            .unwrap();

        let report = billing.glm_calibration_report().await.unwrap();
        // Every durable row is reported, across subjects, plans and independent windows.
        assert_eq!(report.len(), 3);
        assert!(report.iter().all(|row| row.samples == 0));
        let subject_a: Vec<_> = report
            .iter()
            .filter(|row| row.subject_id == "glm-subject-a")
            .collect();
        assert_eq!(subject_a.len(), 2);
        assert!(subject_a.iter().any(|row| row.window_duration_secs
            == registry::GLM_WEEKLY_WINDOW_SECS
            && row.native_used_microcredits == Some(200_000_000)));
        let subject_b: Vec<_> = report
            .iter()
            .filter(|row| row.subject_id == "glm-subject-b")
            .collect();
        assert_eq!(subject_b.len(), 1);
        assert_eq!(subject_b[0].plan, "Max");
        assert_eq!(subject_b[0].native_used_microcredits, Some(10_000_000));
        billing.flush().await.unwrap();
    });
    drop(billing);

    lock_holder
        .batch_execute(
            "TRUNCATE glm_window_calibrations,glm_window_observations,\
             glm_calibration_subject_spend,glm_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

#[test]
fn kimi_postgres_calibration_report_lists_every_subject_window() {
    const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping KIMI PostgreSQL report matrix: CLAUDE_API_TEST_DATABASE_URL is unset");
        return;
    };
    let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock_holder
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let instance_id = format!("kimi-report-{}-{unique}", std::process::id());
    let owner = pg.claim_instance(&instance_id, 60).unwrap();
    drop(pg);

    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::Postgres { url: url.clone() },
        Some(owner),
        1,
        0,
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        billing
            .observe_kimi_windows(
                "kimi-subject-a",
                "Moderato",
                vec![
                    kimi_snapshot(registry::KIMI_ROLLING_WINDOW_SECS, 100, 1_000, 100),
                    kimi_snapshot(registry::KIMI_WEEKLY_WINDOW_SECS, 200, 1_000, 100),
                ],
            )
            .await
            .unwrap();
        billing
            .observe_kimi_windows(
                "kimi-subject-b",
                "unreviewed-base-plan",
                vec![kimi_snapshot(
                    registry::KIMI_ROLLING_WINDOW_SECS,
                    10,
                    1_000,
                    101,
                )],
            )
            .await
            .unwrap();

        let report = billing.kimi_calibration_report().await.unwrap();
        // Every durable row is reported, across subjects, plans and independent windows.
        assert_eq!(report.len(), 3);
        assert!(report.iter().all(|row| row.samples == 0));
        let subject_a: Vec<_> = report
            .iter()
            .filter(|row| row.subject_id == "kimi-subject-a")
            .collect();
        assert_eq!(subject_a.len(), 2);
        assert!(subject_a.iter().any(|row| row.window_duration_secs
            == registry::KIMI_WEEKLY_WINDOW_SECS
            && row.native_used_units == 200));
        let subject_b: Vec<_> = report
            .iter()
            .filter(|row| row.subject_id == "kimi-subject-b")
            .collect();
        assert_eq!(subject_b.len(), 1);
        assert_eq!(subject_b[0].plan, "unreviewed-base-plan");
        assert_eq!(subject_b[0].native_used_units, 10);
        billing.flush().await.unwrap();
    });
    drop(billing);

    lock_holder
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

#[tokio::test]
async fn kimi_recent_turns_is_empty_on_a_sqlite_authority() {
    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::new(":memory:".to_owned(), None),
        None,
        1,
        0,
    )
    .unwrap();
    // KIMI calibration is PostgreSQL-only: the read is empty, not an error.
    assert_eq!(billing.kimi_recent_turns(512).await.unwrap(), Vec::new());
}

#[test]
fn kimi_postgres_recent_turns_read_is_bounded_newest_first_and_exact() {
    const POSTGRES_DESTRUCTIVE_TEST_LOCK: i64 = 831_572_908_441;

    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!(
            "skipping KIMI PostgreSQL recent-turns matrix: CLAUDE_API_TEST_DATABASE_URL is unset"
        );
        return;
    };
    let mut lock_holder = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = registry::pg::PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    lock_holder
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let instance_id = format!("kimi-turns-{}-{unique}", std::process::id());
    let owner = pg.claim_instance(&instance_id, 60).unwrap();
    drop(pg);

    let billing = AsyncBilling::start_authority(
        registry::authority::AuthorityConfig::Postgres { url: url.clone() },
        Some(owner),
        1,
        0,
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for (id, total, completed_at) in [
            ("kimi-turn-older", 11_600, 1_800_000_100),
            ("kimi-turn-newer", 22_400, 1_800_000_200),
            ("kimi-turn-middle", 5_000, 1_800_000_150),
        ] {
            billing
                .record_kimi_turn(kimi_event(id, total, completed_at))
                .await
                .unwrap();
        }
        billing.flush().await.unwrap();

        let turns = billing.kimi_recent_turns(512).await.unwrap();
        let ids: Vec<&str> = turns.iter().map(|turn| turn.request_id.as_str()).collect();
        assert_eq!(
            ids,
            ["kimi-turn-newer", "kimi-turn-middle", "kimi-turn-older"],
            "newest first by completed_at"
        );
        // Exact roundtrip: the full usage and money vector survives the read.
        let newer = &turns[0];
        assert_eq!(newer.served_model, "kimi-k2.7-code");
        assert_eq!(newer.api_total_nanousd, 22_400);
        assert_eq!(newer.completed_at, 1_800_000_200);
        assert_eq!(newer.tariff_schedule_id, "moonshot/test/v1");
        // The bound is honored.
        let limited = billing.kimi_recent_turns(2).await.unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].request_id, "kimi-turn-newer");
        assert_eq!(limited[1].request_id, "kimi-turn-middle");
    });
    drop(billing);

    lock_holder
        .batch_execute(
            "TRUNCATE kimi_window_calibrations,kimi_window_observations,\
             kimi_calibration_subject_spend,kimi_turn_calibration_events \
             RESTART IDENTITY CASCADE",
        )
        .unwrap();
    lock_holder
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}
