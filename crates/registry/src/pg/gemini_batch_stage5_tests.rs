use super::*;

fn stage5_blob(kind: &str, created: i64) -> crate::GeminiBatchEncryptedBlob {
    crate::GeminiBatchEncryptedBlob {
        kind: kind.into(),
        key_id: "stage5-kid".into(),
        nonce: vec![1; 24],
        ciphertext: vec![2; 20],
        plaintext_len: 4,
        plaintext_digest: [3; 32],
        retention_ts: created + 10_000,
    }
}

fn stage5_create(
    job: &str,
    account: &str,
    key_id: &str,
    request_prefix: &str,
    ts: i64,
    item_count: usize,
) -> crate::GeminiBatchCreate {
    crate::GeminiBatchCreate {
        job_id: job.into(),
        account_id: account.into(),
        creator_key_id: key_id.into(),
        public_model: "gemini-2.5-flash".into(),
        display_name: "stage5".into(),
        canonical_request_digest: [job.as_bytes()[job.len() - 1]; 32],
        idempotency_digest: None,
        priority: 0,
        input_kind: crate::GeminiBatchInputKind::Inline,
        input_file_id: None,
        schema_version: 1,
        encryption_policy_version: 1,
        create_ts: ts,
        deadline_ts: ts + 10_000,
        items: (0..item_count)
            .map(|index| {
                let request_id = format!("{request_prefix}-{index:06}");
                crate::GeminiBatchCreateItem {
                    item_index: index as i64,
                    request_id: request_id.clone(),
                    logical_request_id: format!("logical-{request_id}"),
                    execution_group_id: format!("group-{request_id}"),
                    client_key: Some(format!("key-{index:06}")),
                    request_digest: [4; 32],
                    input_file_id: None,
                    referenced_file_ids: vec![],
                    hold_nano: 100,
                    payable_multiplier_bp: 5000,
                    priced_ts: ts,
                    tariff_family: "google/gemini/gemini-2.5-flash".into(),
                    tariff_version: 1,
                    tariff_schedule_id: "google/gemini/gemini-2.5-flash/v1".into(),
                    request_blob: stage5_blob("request", ts),
                    metadata_blob: None,
                }
            })
            .collect(),
    }
}

fn stage5_settlement(
    claim: &crate::GeminiBatchClaim,
    ts: i64,
) -> crate::GeminiBatchSettlementIntent {
    crate::GeminiBatchSettlementIntent {
        job_id: claim.job_id.clone(),
        item_index: claim.item_index,
        request_id: claim.request_id.clone(),
        claim_generation: claim.claim_generation,
        disposition: crate::GeminiBatchSettlementDisposition::Cancel,
        actual_nano: 0,
        charge_basis_nano: 0,
        real_nano: 0,
        usage: None,
        result_blob: stage5_blob("error", ts),
        terminal_state: crate::GeminiBatchItemState::Canceled,
        terminal_class: crate::GeminiBatchTerminalClass::Canceled,
        calibration: None,
        completed_ts: ts,
    }
}

fn stage5_lock_and_store() -> Option<(PgStore, PgStore)> {
    let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
        eprintln!("skipping Stage 5 real-PostgreSQL matrix");
        return None;
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
    Some((lock, pg))
}

fn stage5_cleanup(pg: &mut PgStore) {
    pg.client
        .batch_execute(
            "DELETE FROM gemini_batch_profile_leases WHERE job_id LIKE 'stage5-%';
             DELETE FROM gemini_batch_settlement_outbox WHERE job_id LIKE 'stage5-%';
             DELETE FROM gemini_batch_blobs WHERE job_id LIKE 'stage5-%';
             DELETE FROM gemini_batch_items WHERE job_id LIKE 'stage5-%';
             DELETE FROM gemini_batch_jobs WHERE job_id LIKE 'stage5-%';
             DELETE FROM api_keys WHERE account_id LIKE 'stage5-%';
             DELETE FROM accounts WHERE id LIKE 'stage5-%';
             DELETE FROM leader_leases WHERE name='gemini_batch_dispatch';
             DELETE FROM engine_instances WHERE instance_id LIKE 'stage5-%';",
        )
        .unwrap();
}

fn stage5_account(pg: &mut PgStore, suffix: &str, balance: i64) -> (String, String, String) {
    let account = format!("stage5-account-{suffix}");
    let raw_key = format!("stage5-key-{suffix}");
    let key_id = format!("stage5-key-id-{suffix}");
    pg.client
        .execute(
            "INSERT INTO accounts(id,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created)
             VALUES($1,$2,0,0,5000,'active',1,'x')",
            &[&account, &balance],
        )
        .unwrap();
    pg.client
        .execute(
            "INSERT INTO api_keys(key,key_id,account_id,spent_nano,reserved_nano,status,created_ts,created)
             VALUES($1,$2,$3,0,0,'active',1,'x')",
            &[&raw_key, &key_id, &account],
        )
        .unwrap();
    (account, raw_key, key_id)
}

#[test]
fn stage5_resilience_postgres_matrix() {
    let Some((mut lock, mut pg)) = stage5_lock_and_store() else {
        return;
    };
    stage5_cleanup(&mut pg);
    let ts = now();
    let (account, raw_key, key_id) = stage5_account(&mut pg, "fault", 100_000);

    for boundary in ["predispatch", "actual-send", "result", "outbox", "apply"] {
        let job = format!("stage5-job-{boundary}");
        let owner_name = format!("stage5-owner-{boundary}");
        let replacement_name = format!("stage5-replacement-{boundary}");
        let profile = format!("stage5-profile-{boundary}");
        let create = stage5_create(&job, &account, &key_id, &job, ts, 1);
        assert!(matches!(
            pg.gemini_batch_create(&create, &raw_key).unwrap(),
            crate::GeminiBatchCreateOutcome::Created { .. }
        ));
        let owner = pg.claim_instance(&owner_name, 600).unwrap();
        pg.client.execute("DELETE FROM leader_leases WHERE name=$1", &[&crate::GEMINI_BATCH_DISPATCH_LEADER]).unwrap();
        assert!(pg.acquire_gemini_batch_leader(&owner, 600).unwrap());
        let claimed = pg
            .claim_gemini_batch_item(&owner, &profile, "gemini-2.5-flash", 600)
            .unwrap()
            .unwrap();
        let claim = claimed.claim;

        if boundary != "predispatch" {
            assert!(pg
                .mark_gemini_batch_dispatching(&owner, &claim, 600)
                .unwrap());
        }
        if matches!(boundary, "actual-send" | "result" | "outbox" | "apply") {
            assert!(pg
                .mark_gemini_batch_actual_send(&owner, &claim, 600)
                .unwrap());
        }
        if matches!(boundary, "result" | "outbox" | "apply") {
            let result = stage5_blob("error", ts);
            pg.client
                .execute(
                    "INSERT INTO gemini_batch_blobs(job_id,item_index,kind,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,retention_ts,created_ts)
                     VALUES($1,$2,'error',$3,$4,$5,$6,$7,$8,$9)
                     ON CONFLICT(job_id,item_index,kind) DO NOTHING",
                    &[
                        &claim.job_id,
                        &claim.item_index,
                        &result.key_id,
                        &result.nonce,
                        &result.ciphertext,
                        &result.plaintext_len,
                        &&result.plaintext_digest[..],
                        &result.retention_ts,
                        &ts,
                    ],
                )
                .unwrap();
        }
        if matches!(boundary, "outbox" | "apply") {
            pg.enqueue_gemini_batch_settlement(&owner, &claim, &stage5_settlement(&claim, ts))
                .unwrap();
            if boundary == "outbox" {
                let canceled = pg
                    .gemini_batch_cancel(&account, &claim.job_id)
                    .unwrap()
                    .unwrap();
                assert_eq!(canceled.canceled_items, 0);
                assert_eq!(
                    pg.client
                        .query_one(
                            "SELECT state FROM gemini_batch_items WHERE job_id=$1 AND item_index=$2",
                            &[&claim.job_id, &claim.item_index],
                        )
                        .unwrap()
                        .get::<_, String>(0),
                    "settlement_pending"
                );
            }
        }
        if boundary == "apply" {
            assert!(pg
                .process_gemini_batch_settlement(&claim.request_id)
                .unwrap()
                .is_some());
        }

        if boundary != "apply" {
            pg.client
                .execute(
                    "UPDATE gemini_batch_items SET lease_until=0 WHERE job_id=$1 AND item_index=$2",
                    &[&claim.job_id, &claim.item_index],
                )
                .unwrap();
        }
        pg.client
            .execute(
                "UPDATE gemini_batch_profile_leases SET lease_until=0 WHERE profile_id=$1",
                &[&claim.profile_id],
            )
            .unwrap();
        pg.client
            .execute(
                "UPDATE engine_instances SET lease_until=0 WHERE instance_id=$1",
                &[&owner.instance_id],
            )
            .unwrap();

        let report = pg.reconcile_expired_gemini_batch_claims(64).unwrap();
        let replacement = pg.claim_instance(&owner_name, 600).unwrap();
        assert_ne!(replacement.epoch, owner.epoch);
        assert!(pg
            .mark_gemini_batch_actual_send(&owner, &claim, 60)
            .is_err());
        assert!(pg.renew_gemini_batch_claim(&owner, &claim, 60).is_err());
        match boundary {
            "predispatch" => {
                assert_eq!(report.requeued_before_dispatch, 1);
                assert!(report.recovery_candidates.is_empty());
                let restart_owner = pg.claim_instance(&replacement_name, 600).unwrap();
                pg.client.execute("UPDATE leader_leases SET lease_until=0 WHERE name=$1", &[&crate::GEMINI_BATCH_DISPATCH_LEADER]).unwrap();
                assert!(pg.acquire_gemini_batch_leader(&restart_owner, 600).unwrap());
                let restarted = pg
                    .claim_gemini_batch_item(
                        &restart_owner,
                        &format!("{profile}-restart"),
                        "gemini-2.5-flash",
                        600,
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(restarted.claim.claim_generation, claim.claim_generation + 1);
            }
            "actual-send" | "result" => {
                assert_eq!(report.recovery_candidates.len(), 1);
                let recovery = report.recovery_candidates.into_iter().next().unwrap();
                assert_eq!(
                    recovery.disposition,
                    crate::GeminiBatchSettlementDisposition::Indeterminate
                );
                assert_eq!(recovery.actual_send_evidence.as_deref(), Some("sent"));
                let recovery_intent = crate::GeminiBatchSettlementIntent {
                    disposition: crate::GeminiBatchSettlementDisposition::Indeterminate,
                    terminal_state: crate::GeminiBatchItemState::Indeterminate,
                    terminal_class: crate::GeminiBatchTerminalClass::Indeterminate,
                    result_blob: stage5_blob("error", ts),
                    ..stage5_settlement(&claim, ts)
                };
                pg.enqueue_gemini_batch_recovery_settlement(&recovery, &recovery_intent)
                    .unwrap();
                assert!(pg
                    .process_gemini_batch_settlement(&claim.request_id)
                    .unwrap()
                    .is_some());
            }
            "outbox" => {
                assert!(report.recovery_candidates.is_empty());
                assert_eq!(pg.drain_gemini_batch_settlements(64).unwrap(), 1);
            }
            "apply" => {
                assert!(report.recovery_candidates.is_empty());
                assert_eq!(pg.drain_gemini_batch_settlements(64).unwrap(), 0);
                assert!(pg
                    .process_gemini_batch_settlement(&claim.request_id)
                    .unwrap()
                    .is_some());
            }
            _ => unreachable!(),
        }
    }

    let cancel_job = "stage5-job-cancel";
    let cancel_create = stage5_create(cancel_job, &account, &key_id, cancel_job, ts, 2);
    pg.gemini_batch_create(&cancel_create, &raw_key).unwrap();
    let owner = pg.claim_instance("stage5-owner-cancel", 600).unwrap();
    pg.client.execute("DELETE FROM leader_leases WHERE name=$1", &[&crate::GEMINI_BATCH_DISPATCH_LEADER]).unwrap();
    assert!(pg.acquire_gemini_batch_leader(&owner, 600).unwrap());
    let claimed = pg
        .claim_gemini_batch_item(&owner, "stage5-profile-cancel", "gemini-2.5-flash", 600)
        .unwrap()
        .unwrap();
    assert!(pg
        .mark_gemini_batch_dispatching(&owner, &claimed.claim, 600)
        .unwrap());
    let canceled = pg
        .gemini_batch_cancel(&account, cancel_job)
        .unwrap()
        .unwrap();
    assert_eq!(canceled.canceled_items, 1);
    let states = pg
        .client
        .query(
            "SELECT state FROM gemini_batch_items WHERE job_id=$1 ORDER BY item_index",
            &[&cancel_job],
        )
        .unwrap()
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>();
    assert_eq!(states, vec!["dispatching", "canceled"]);

    stage5_cleanup(&mut pg);
    lock.client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}

#[test]
fn stage5_postgres_load_and_fairness() {
    let Some((mut lock, mut pg)) = stage5_lock_and_store() else {
        return;
    };
    stage5_cleanup(&mut pg);
    let item_count = std::env::var("GEMINI_BATCH_STAGE5_LOAD_ITEMS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1_000)
        .clamp(200, 5_000);
    let ts = now();
    let total_hold = i64::try_from(item_count).unwrap() * 100;
    let (account_a, key_a, key_id_a) = stage5_account(&mut pg, "load-a", total_hold + 100_000);
    let (account_b, key_b, key_id_b) = stage5_account(&mut pg, "load-b", total_hold + 100_000);
    let create_a = stage5_create(
        "stage5-load-job-a",
        &account_a,
        &key_id_a,
        "stage5-load-request-a",
        ts,
        item_count,
    );
    let create_b = stage5_create(
        "stage5-load-job-b",
        &account_b,
        &key_id_b,
        "stage5-load-request-b",
        ts + 1,
        item_count,
    );
    pg.gemini_batch_create(&create_a, &key_a).unwrap();
    pg.gemini_batch_create(&create_b, &key_b).unwrap();

    let owner = pg.claim_instance("stage5-owner-load", 600).unwrap();
    assert!(pg.acquire_gemini_batch_leader(&owner, 600).unwrap());
    let mut claims = Vec::new();
    for index in 0..32 {
        let profile = if index % 2 == 0 {
            "stage5-profile-a"
        } else {
            "stage5-profile-b"
        };
        let claimed = pg
            .claim_gemini_batch_item(&owner, profile, "gemini-2.5-flash", 600)
            .unwrap()
            .unwrap();
        claims.push(claimed.claim);
        // Fairness is measured over the bounded claim window rather than every adjacent pair:
        // an account may legitimately win two ties while neither account may starve.
        let claim = claims.last().unwrap();
        assert!(pg.requeue_gemini_batch_claim(&owner, claim, 0).unwrap());
    }
    assert!(claims.iter().any(|claim| claim.account_id == account_a));
    assert!(claims
        .iter()
        .any(|claim| claim.profile_id == "stage5-profile-a"));
    assert!(claims
        .iter()
        .any(|claim| claim.profile_id == "stage5-profile-b"));

    let counts = pg
        .client
        .query(
            "SELECT job_id,COUNT(*)::bigint,MIN(item_index)::bigint,MAX(item_index)::bigint
             FROM gemini_batch_items WHERE job_id LIKE 'stage5-load-job-%'
             GROUP BY job_id ORDER BY job_id",
            &[],
        )
        .unwrap();
    assert_eq!(counts.len(), 2);
    for row in counts {
        assert_eq!(row.get::<_, i64>(1), item_count as i64);
        assert_eq!(row.get::<_, i64>(2), 0);
        assert_eq!(row.get::<_, i64>(3), item_count as i64 - 1);
    }
    let report = pg.gemini_batch_operational_report().unwrap();
    assert_eq!(report.queued_items, (item_count * 2) as i64);
    assert_eq!(report.active_items, 0);

    stage5_cleanup(&mut pg);
    lock.client
        .query_one(
            "SELECT pg_advisory_unlock($1)",
            &[&POSTGRES_DESTRUCTIVE_TEST_LOCK],
        )
        .unwrap();
}
