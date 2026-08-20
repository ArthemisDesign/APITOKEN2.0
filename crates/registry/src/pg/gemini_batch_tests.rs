use super::*;

fn blob(kind: &str, created: i64) -> crate::GeminiBatchEncryptedBlob {
    crate::GeminiBatchEncryptedBlob {
        kind: kind.into(),
        key_id: "kid".into(),
        nonce: vec![1; 24],
        ciphertext: vec![2; 20],
        plaintext_len: 4,
        plaintext_digest: [3; 32],
        retention_ts: created + 10_000,
    }
}
fn create(
    job: &str,
    account: &str,
    key_id: &str,
    request: &str,
    digest: u8,
    idem: u8,
    ts: i64,
) -> crate::GeminiBatchCreate {
    create_many(job, account, key_id, request, digest, idem, ts, 1)
}

fn create_many(
    job: &str,
    account: &str,
    key_id: &str,
    request_prefix: &str,
    digest: u8,
    idem: u8,
    ts: i64,
    item_count: usize,
) -> crate::GeminiBatchCreate {
    crate::GeminiBatchCreate {
        job_id: job.into(),
        account_id: account.into(),
        creator_key_id: key_id.into(),
        public_model: "gemini-2.5-flash".into(),
        display_name: "matrix".into(),
        canonical_request_digest: [digest; 32],
        idempotency_digest: Some([idem; 32]),
        priority: 0,
        input_kind: crate::GeminiBatchInputKind::Inline,
        input_file_id: None,
        schema_version: 1,
        encryption_policy_version: 1,
        create_ts: ts,
        deadline_ts: ts + 10_000,
        items: (0..item_count)
            .map(|index| {
                let request_id = if item_count == 1 {
                    request_prefix.to_owned()
                } else {
                    format!("{request_prefix}-{index:06}")
                };
                crate::GeminiBatchCreateItem {
                    item_index: index as i64,
                    request_id: request_id.clone(),
                    logical_request_id: format!("logical-{request_id}"),
                    execution_group_id: format!("group-{request_id}"),
                    client_key: None,
                    request_digest: [4; 32],
                    input_file_id: None,
                    referenced_file_ids: vec![],
                    hold_nano: 100,
                    payable_multiplier_bp: 5000,
                    priced_ts: ts,
                    tariff_family: "google/gemini/gemini-2.5-flash".into(),
                    tariff_version: 1,
                    tariff_schedule_id: "google/gemini/gemini-2.5-flash/v1".into(),
                    request_blob: blob("request", ts),
                    metadata_blob: None,
                }
            })
            .collect(),
    }
}

#[test]
fn settlement_validation_rejects_noncanonical_shapes() {
    let ts = 10;
    let usage = crate::GeminiBatchUsage {
        input_tokens: 2,
        tool_prompt_tokens: 0,
        audio_input_tokens: 0,
        cached_input_tokens: 0,
        cached_audio_input_tokens: 0,
        output_tokens: 3,
        thinking_output_tokens: 0,
        image_output_tokens: 0,
        search_queries: 0,
        grounded_search_prompts: 0,
    };
    let calibration = crate::ProviderTurnCalibrationEvent {
        provider: "google".into(),
        request_id: "request".into(),
        subject_id: "profile".into(),
        model_id: "gemini-2.5-flash".into(),
        service_tier: "standard".into(),
        inference_geo: "global".into(),
        tariff_schedule_id: "schedule".into(),
        priced_ts: ts,
        completed_at: ts,
        input_tokens: 2,
        audio_input_tokens: 0,
        cache_read_tokens: 0,
        cached_audio_input_tokens: 0,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: 3,
        thinking_output_tokens: 0,
        image_output_tokens: 0,
        tool_prompt_tokens: 0,
        search_queries: 0,
        grounded_search_prompts: 0,
        api_input_nanousd: 20,
        api_audio_input_nanousd: 0,
        api_cache_read_nanousd: 0,
        api_cached_audio_input_nanousd: 0,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd: 30,
        api_image_output_nanousd: 0,
        api_search_nanousd: 0,
        api_total_nanousd: 50,
    };
    let valid = crate::GeminiBatchSettlementIntent {
        job_id: "job".into(),
        item_index: 0,
        request_id: "request".into(),
        claim_generation: 1,
        disposition: crate::GeminiBatchSettlementDisposition::Settle,
        actual_nano: 50,
        charge_basis_nano: 100,
        real_nano: 50,
        usage: Some(usage),
        result_blob: blob("result", ts),
        terminal_state: crate::GeminiBatchItemState::Succeeded,
        terminal_class: crate::GeminiBatchTerminalClass::Success,
        calibration: Some(calibration),
        completed_ts: ts,
    };
    assert!(valid.validate().is_ok());
    let mut bad_class = valid.clone();
    bad_class.terminal_class = crate::GeminiBatchTerminalClass::ProtocolError;
    assert!(bad_class.validate().is_err());
    let mut bad_total = valid.clone();
    bad_total.calibration.as_mut().unwrap().api_total_nanousd += 1;
    assert!(bad_total.validate().is_err());
    let mut bad_blob = valid;
    bad_blob.result_blob.kind = "error".into();
    assert!(bad_blob.validate().is_err());
}

#[test]
fn recovery_candidate_preserves_settlement_policy() {
    let candidate = crate::GeminiBatchRecoveryCandidate {
        job_id: "job".into(),
        account_id: "account".into(),
        item_index: 1,
        request_id: "request".into(),
        claim_generation: 2,
        profile_id: "profile".into(),
        hold_nano: 100,
        disposition: crate::GeminiBatchSettlementDisposition::Indeterminate,
        terminal_state: crate::GeminiBatchItemState::Indeterminate,
        terminal_class: crate::GeminiBatchTerminalClass::Indeterminate,
        actual_send_evidence: Some("ambiguous".into()),
    };
    assert_eq!(candidate.hold_nano, 100);
    assert_eq!(
        candidate.disposition,
        crate::GeminiBatchSettlementDisposition::Indeterminate
    );
    assert_eq!(
        candidate.terminal_state,
        crate::GeminiBatchItemState::Indeterminate
    );
}

fn calibration_event(
    request_id: &str,
    subject_id: &str,
    completed_at: i64,
    total: i64,
) -> crate::ProviderTurnCalibrationEvent {
    crate::ProviderTurnCalibrationEvent {
        provider: "google".into(),
        request_id: request_id.into(),
        subject_id: subject_id.into(),
        model_id: "gemini-2.5-flash".into(),
        service_tier: "standard".into(),
        inference_geo: "global".into(),
        tariff_schedule_id: "google/gemini/gemini-2.5-flash/v1".into(),
        priced_ts: completed_at,
        completed_at,
        input_tokens: 2,
        audio_input_tokens: 0,
        cache_read_tokens: 0,
        cached_audio_input_tokens: 0,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: 3,
        thinking_output_tokens: 0,
        image_output_tokens: 0,
        tool_prompt_tokens: 0,
        search_queries: 0,
        grounded_search_prompts: 0,
        api_input_nanousd: total,
        api_audio_input_nanousd: 0,
        api_cache_read_nanousd: 0,
        api_cached_audio_input_nanousd: 0,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd: 0,
        api_image_output_nanousd: 0,
        api_search_nanousd: 0,
        api_total_nanousd: total,
    }
}

#[test]
fn settlement_safety_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping settlement safety PostgreSQL matrix");
        return;
    };
    let mut lock = PgStore::connect(&url).unwrap();
    lock.client
        .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
        .unwrap();
    lock.client
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();

    pg.client
        .batch_execute(
            "DELETE FROM provider_turn_calibration_events WHERE request_id LIKE 'safety-%';
             DELETE FROM provider_calibration_subject_spend WHERE subject_id='safety-subject';
             DELETE FROM accounts WHERE id LIKE 'safety-collection-%';",
        )
        .unwrap();
    for (suffix, balance, hold, actual, expected) in [
        ("funded", 900i64, 100i64, 50i64, (950i64, 50i64, 0i64)),
        ("over-hold", 900, 100, 150, (850, 150, 0)),
        ("floor", -999_999_950, 100, 150, (-1_000_000_000, 150, 0)),
        (
            "shortfall",
            -1_000_000_000,
            100,
            150,
            (-1_000_000_000, 100, 50),
        ),
        (
            "deep-debt",
            -1_000_000_050,
            100,
            150,
            (-1_000_000_050, 100, 50),
        ),
    ] {
        let account_id = format!("safety-collection-{suffix}");
        pg.client
            .execute(
                "INSERT INTO accounts(id,balance_nano,spent_nano,reserved_nano,uncollected_nano,mult_bp,status,created_ts,created) \
                 VALUES($1,$2,0,$3,0,5000,'active',1,'x')",
                &[&account_id, &balance, &hold],
            )
            .unwrap();
        let mut tx = pg.client.transaction().unwrap();
        let result =
            super::collect_account_settlement_tx(&mut tx, &account_id, hold, actual).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            (
                result.balance_nano,
                result.collected_nano,
                result.uncollected_nano
            ),
            expected,
            "collection case {suffix}"
        );
    }

    let newest = calibration_event("safety-newest", "safety-subject", 300, 30);
    let oldest = calibration_event("safety-oldest", "safety-subject", 100, 10);
    let middle = calibration_event("safety-middle", "safety-subject", 200, 20);
    assert!(
        pg.record_provider_turn_calibration_event(&newest)
            .unwrap()
            .inserted
    );
    assert!(
        pg.record_provider_turn_calibration_event(&oldest)
            .unwrap()
            .inserted
    );
    let replay = pg.record_provider_turn_calibration_event(&oldest).unwrap();
    assert!(!replay.inserted);
    assert_eq!(replay.spent_nano, 40);
    let conflict_total = replay.spent_nano;
    let mut conflict = oldest.clone();
    conflict.model_id = "gemini-conflict".into();
    assert!(crate::is_provider_turn_calibration_replay_conflict(
        &pg.record_provider_turn_calibration_event(&conflict)
            .unwrap_err()
    ));
    assert_eq!(
        pg.provider_calibration_subject_spend("google", "safety-subject")
            .unwrap()
            .spent_nano,
        conflict_total
    );
    let final_spend = pg.record_provider_turn_calibration_event(&middle).unwrap();
    assert_eq!(final_spend.spent_nano, 60);
    assert_eq!(final_spend.tracking_started_ts, Some(100));
    assert_eq!(final_spend.updated_ts, Some(300));

    assert_eq!(super::retry_backoff_seconds(0), 1);
    assert_eq!(super::retry_backoff_seconds(3), 8);
    assert_eq!(super::retry_backoff_seconds(100), 256);
}

#[test]
fn stage2_authority_postgres_matrix() {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Stage 2 batch authority matrix");
        return;
    };
    let mut lock = PgStore::connect(&url).unwrap();
    lock.client
        .batch_execute("SET statement_timeout=0; SET lock_timeout=0")
        .unwrap();
    lock.client
        .query_one(
            "SELECT pg_advisory_lock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
    let mut pg = PgStore::connect(&url).unwrap();
    pg.migrate().unwrap();
    pg.client.batch_execute("DELETE FROM gemini_batch_profile_leases_slot2 WHERE job_id='stage2-job';DELETE FROM gemini_batch_profile_leases WHERE job_id='stage2-job';DELETE FROM gemini_batch_settlement_outbox WHERE job_id='stage2-job';DELETE FROM gemini_batch_blobs WHERE job_id='stage2-job';DELETE FROM gemini_batch_items WHERE job_id='stage2-job';DELETE FROM gemini_batch_jobs WHERE job_id='stage2-job';DELETE FROM gemini_batch_file_chunks WHERE file_id='stage2-file';DELETE FROM gemini_batch_files WHERE file_id='stage2-file';DELETE FROM provider_turn_calibration_events WHERE request_id='stage2-request';DELETE FROM provider_calibration_subject_spend WHERE subject_id='stage2-profile';DELETE FROM usage_events WHERE request_id='stage2-request';DELETE FROM ledger WHERE request_id='stage2-request';DELETE FROM api_keys WHERE key='stage2-key';DELETE FROM accounts WHERE id='stage2-account';").unwrap();
    pg.client.execute("INSERT INTO accounts(id,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created)VALUES('stage2-account',1000,0,0,5000,'active',1,'x')",&[]).unwrap();
    pg.client.execute("INSERT INTO api_keys(key,key_id,account_id,spent_nano,reserved_nano,status,created_ts,created)VALUES('stage2-key','stage2-key-id','stage2-account',0,0,'active',1,'x')",&[]).unwrap();
    let ts = now();
    let c = create(
        "stage2-job",
        "stage2-account",
        "stage2-key-id",
        "stage2-request",
        1,
        2,
        ts,
    );
    assert!(matches!(
        pg.gemini_batch_create(&c, "stage2-key").unwrap(),
        crate::GeminiBatchCreateOutcome::Created { balance_nano: 900 }
    ));
    assert!(matches!(
        pg.gemini_batch_create(&c, "stage2-key").unwrap(),
        crate::GeminiBatchCreateOutcome::Replay { .. }
    ));
    let mut bad = c.clone();
    bad.job_id = "bad-job".into();
    bad.canonical_request_digest = [9; 32];
    assert!(crate::is_gemini_batch_idempotency_conflict(
        &pg.gemini_batch_create(&bad, "stage2-key").unwrap_err()
    ));
    assert!(pg
        .gemini_batch_get("foreign", "stage2-job")
        .unwrap()
        .is_none());
    let detail = pg
        .gemini_batch_get("stage2-account", "stage2-job")
        .unwrap()
        .unwrap();
    // Nonterminal operation reads are summary-only so 100k jobs never materialize every item.
    assert!(detail.items.is_empty());
    let owner = pg.claim_instance("stage2-owner", 600).unwrap();
    assert!(pg.acquire_gemini_batch_leader(&owner, 60).unwrap());
    let claimed = pg
        .claim_gemini_batch_item(
            &owner,
            "stage2-profile",
            "gemini-2.5-flash",
            crate::GEMINI_BATCH_STANDARD_PROFILE_CAPACITY,
            60,
        )
        .unwrap()
        .unwrap();
    assert_eq!(claimed.public_model, "gemini-2.5-flash");
    assert_eq!(claimed.request_blob.kind, "request");
    assert_eq!(claimed.hold_nano, 100);
    assert_eq!(claimed.payable_multiplier_bp, 5000);
    assert_eq!(claimed.tariff_version, 1);
    assert!(format!("{claimed:?}").contains("[REDACTED]"));
    assert!(!format!("{claimed:?}").contains("[2, 2, 2"));
    assert_eq!(
        pg.gemini_batch_blob_get("stage2-account", "stage2-job", 0, "request")
            .unwrap()
            .unwrap(),
        claimed.request_blob
    );
    assert!(pg
        .gemini_batch_blob_get("foreign", "stage2-job", 0, "request")
        .unwrap()
        .is_none());
    let claim = claimed.claim;
    assert!(pg
        .mark_gemini_batch_dispatching(&owner, &claim, 60)
        .unwrap());
    assert!(pg
        .mark_gemini_batch_actual_send(&owner, &claim, 60)
        .unwrap());
    pg.client
        .execute("DELETE FROM api_keys WHERE key='stage2-key'", &[])
        .unwrap();
    let cal = crate::ProviderTurnCalibrationEvent {
        provider: "google".into(),
        request_id: "stage2-request".into(),
        subject_id: "stage2-profile".into(),
        model_id: "gemini-2.5-flash".into(),
        service_tier: "standard".into(),
        inference_geo: "global".into(),
        tariff_schedule_id: "google/gemini/gemini-2.5-flash/v1".into(),
        priced_ts: ts,
        completed_at: ts,
        input_tokens: 2,
        audio_input_tokens: 0,
        cache_read_tokens: 0,
        cached_audio_input_tokens: 0,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: 3,
        thinking_output_tokens: 0,
        image_output_tokens: 0,
        tool_prompt_tokens: 0,
        search_queries: 0,
        grounded_search_prompts: 0,
        api_input_nanousd: 20,
        api_audio_input_nanousd: 0,
        api_cache_read_nanousd: 0,
        api_cached_audio_input_nanousd: 0,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd: 30,
        api_image_output_nanousd: 0,
        api_search_nanousd: 0,
        api_total_nanousd: 50,
    };
    let intent = crate::GeminiBatchSettlementIntent {
        job_id: "stage2-job".into(),
        item_index: 0,
        request_id: "stage2-request".into(),
        claim_generation: claim.claim_generation,
        disposition: crate::GeminiBatchSettlementDisposition::Settle,
        actual_nano: 50,
        charge_basis_nano: 100,
        real_nano: 50,
        usage: Some(crate::GeminiBatchUsage {
            input_tokens: 2,
            tool_prompt_tokens: 0,
            audio_input_tokens: 0,
            cached_input_tokens: 0,
            cached_audio_input_tokens: 0,
            output_tokens: 3,
            thinking_output_tokens: 0,
            image_output_tokens: 0,
            search_queries: 0,
            grounded_search_prompts: 0,
        }),
        result_blob: blob("result", ts),
        terminal_state: crate::GeminiBatchItemState::Succeeded,
        terminal_class: crate::GeminiBatchTerminalClass::Success,
        calibration: Some(cal),
        completed_ts: ts,
    };
    pg.enqueue_gemini_batch_settlement(&owner, &claim, &intent)
        .unwrap();
    assert_eq!(
        pg.process_gemini_batch_settlement("stage2-request")
            .unwrap(),
        Some(950)
    );
    assert_eq!(
        pg.process_gemini_batch_settlement("stage2-request")
            .unwrap(),
        Some(950)
    );
    let a = pg
        .client
        .query_one(
            "SELECT balance_nano,spent_nano,reserved_nano FROM accounts WHERE id='stage2-account'",
            &[],
        )
        .unwrap();
    assert_eq!(
        (a.get::<_, i64>(0), a.get::<_, i64>(1), a.get::<_, i64>(2)),
        (950, 50, 0)
    );
    let e=pg.client.query_one("SELECT l.key_id,u.key_id,(SELECT spent_nano FROM provider_calibration_subject_spend WHERE provider='google' AND subject_id='stage2-profile') FROM ledger l JOIN usage_events u USING(request_id) WHERE l.request_id='stage2-request'",&[]).unwrap();
    assert_eq!(
        e.get::<_, Option<String>>(0).as_deref(),
        Some("stage2-key-id")
    );
    assert_eq!(
        e.get::<_, Option<String>>(1).as_deref(),
        Some("stage2-key-id")
    );
    assert_eq!(e.get::<_, i64>(2), 50);
    let f = crate::GeminiBatchFileCreate {
        file_id: "stage2-file".into(),
        account_id: "stage2-account".into(),
        display_name: "f".into(),
        mime_type: "application/jsonl".into(),
        size_bytes: 4,
        sha256_digest: [4; 32],
        source_kind: "client_upload".into(),
        create_ts: ts,
        expiration_ts: ts + 100,
    };
    assert_eq!(
        pg.gemini_batch_file_create(&f).unwrap(),
        crate::GeminiBatchFileCreateOutcome::Created
    );
    assert_eq!(
        pg.gemini_batch_file_create(&f).unwrap(),
        crate::GeminiBatchFileCreateOutcome::Replay
    );
    let mut conflict = f.clone();
    conflict.display_name = "other".into();
    assert_eq!(
        pg.gemini_batch_file_create(&conflict).unwrap(),
        crate::GeminiBatchFileCreateOutcome::Unavailable
    );
    let ch = crate::GeminiBatchFileChunk {
        chunk_index: 0,
        key_id: "k".into(),
        nonce: vec![1; 24],
        ciphertext: vec![2; 20],
        plaintext_len: 4,
        plaintext_digest: [4; 32],
        created_ts: ts,
    };
    assert!(pg
        .gemini_batch_file_append_chunk("stage2-account", "stage2-file", &ch)
        .unwrap());
    let mut replay = ch.clone();
    replay.created_ts = ts + 10;
    assert!(pg
        .gemini_batch_file_append_chunk("stage2-account", "stage2-file", &replay)
        .unwrap());
    assert_eq!(
        pg.client
            .query_one(
                "SELECT update_ts FROM gemini_batch_files WHERE file_id='stage2-file'",
                &[]
            )
            .unwrap()
            .get::<_, i64>(0),
        ts
    );
    assert!(pg
        .gemini_batch_file_complete(
            "stage2-account",
            "stage2-file",
            &crate::GeminiBatchFileCompletion {
                completed_ts: ts + 1,
                whole_file_sha256_digest: [9; 32],
                chunk_manifest_digest: [
                    205, 73, 180, 247, 182, 157, 101, 1, 58, 197, 142, 66, 96, 155, 64, 95, 235,
                    253, 134, 122, 184, 144, 157, 181, 52, 188, 252, 121, 94, 73, 233, 136
                ]
            }
        )
        .is_err());
    assert!(pg
        .gemini_batch_file_complete(
            "stage2-account",
            "stage2-file",
            &crate::GeminiBatchFileCompletion {
                completed_ts: ts + 1,
                whole_file_sha256_digest: [4; 32],
                chunk_manifest_digest: [
                    205, 73, 180, 247, 182, 157, 101, 1, 58, 197, 142, 66, 96, 155, 64, 95, 235,
                    253, 134, 122, 184, 144, 157, 181, 52, 188, 252, 121, 94, 73, 233, 136
                ]
            }
        )
        .unwrap());
    let chunk_page = pg
        .gemini_batch_file_chunk_page("stage2-account", "stage2-file", None, 1)
        .unwrap();
    assert_eq!(chunk_page.chunks, vec![ch]);
    assert_eq!(chunk_page.next_chunk_index, None);
    assert!(pg
        .gemini_batch_file_chunk_page("foreign", "stage2-file", None, 1)
        .unwrap()
        .chunks
        .is_empty());
    let descriptor = pg
        .gemini_batch_file_get("stage2-account", "stage2-file")
        .unwrap()
        .unwrap();
    assert_eq!(descriptor.sha256_digest, [4; 32]);
    assert_eq!(descriptor.storage_kind, "chunked");
    assert!(pg
        .gemini_batch_file_delete("stage2-account", "stage2-file")
        .unwrap());
    let zero = crate::GeminiBatchFileCreate {
        file_id: "stage2-zero-file".into(),
        size_bytes: 0,
        sha256_digest: [0; 32],
        ..f
    };
    assert_eq!(
        pg.gemini_batch_file_create(&zero).unwrap(),
        crate::GeminiBatchFileCreateOutcome::Created
    );
    assert!(pg
        .gemini_batch_file_complete(
            "stage2-account",
            "stage2-zero-file",
            &crate::GeminiBatchFileCompletion {
                completed_ts: ts + 1,
                whole_file_sha256_digest: [0; 32],
                chunk_manifest_digest: [
                    160, 21, 227, 48, 235, 217, 253, 190, 242, 13, 102, 210, 42, 9, 140, 207, 75,
                    199, 12, 236, 163, 222, 104, 168, 110, 208, 96, 24, 193, 96, 120, 238
                ]
            }
        )
        .unwrap());
    assert!(pg
        .gemini_batch_file_delete("stage2-account", "stage2-zero-file")
        .unwrap());
    let mut sqlite = crate::authority::Authority::Sqlite(crate::open(":memory:").unwrap());
    assert!(crate::is_gemini_batch_unsupported(
        &sqlite.gemini_batch_get("a", "b").unwrap_err()
    ));
    assert!(crate::is_gemini_batch_unsupported(
        &sqlite
            .gemini_batch_blob_get("a", "b", 0, "request")
            .unwrap_err()
    ));
    assert!(crate::is_gemini_batch_unsupported(
        &sqlite
            .gemini_batch_file_chunk_page("a", "f", None, 1)
            .unwrap_err()
    ));
    assert!(crate::is_gemini_batch_unsupported(
        &sqlite
            .gemini_batch_link_output_file("a", "b", "f")
            .unwrap_err()
    ));
    lock.client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}
