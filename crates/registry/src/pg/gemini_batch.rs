use super::PgStore;
use crate::gemini_batch::{
    GeminiBatchCreate, GeminiBatchCreateOutcome, GeminiBatchEncryptedBlob, GeminiBatchFile,
    GeminiBatchFileChunk, GeminiBatchFileChunkPage, GeminiBatchFileCompletion,
    GeminiBatchFileCreate, GeminiBatchFileCreateOutcome, GeminiBatchIdempotencyConflict,
    GeminiBatchInputKind, GeminiBatchItem, GeminiBatchItemState, GeminiBatchJob,
    GeminiBatchJobDetail, GeminiBatchJobPage, GeminiBatchJobState, GeminiBatchPageCursor,
    GeminiBatchStats, GeminiBatchTerminalClass, MAX_BATCH_ACCOUNT_FILE_BYTES, MAX_BATCH_FILE_BYTES,
    MAX_BATCH_FILE_CHUNK_BYTES, MAX_BATCH_FILE_CHUNK_PAGE_SIZE, MAX_BATCH_NONTERMINAL_JOBS,
    MAX_BATCH_PAGE_SIZE, MAX_BATCH_REFERENCED_FILE_BYTES,
};
use crate::ACCOUNT_OVERDRAFT_NANO;
use anyhow::{bail, Context, Result};
use postgres::{IsolationLevel, Row, Transaction};
use sha2::{Digest, Sha256};

const FILE_CHUNK_MANIFEST_DOMAIN: &[u8] = b"apitoken:gemini-batch-file-chunks:v1\0";

/// Digest the exact ordered chunk authority without reading/decrypting customer bytes.
#[allow(dead_code)]
pub fn gemini_batch_file_chunk_manifest_digest(
    chunks: &[GeminiBatchFileChunk],
) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(FILE_CHUNK_MANIFEST_DOMAIN);
    hasher.update(
        u64::try_from(chunks.len())
            .context("Gemini Batch chunk count overflow")?
            .to_be_bytes(),
    );
    for (expected, chunk) in chunks.iter().enumerate() {
        chunk.validate()?;
        if chunk.chunk_index
            != i64::try_from(expected).context("Gemini Batch chunk index overflow")?
        {
            bail!("Gemini Batch file chunks are not contiguous")
        }
        hasher.update(chunk.chunk_index.to_be_bytes());
        hasher.update(chunk.plaintext_len.to_be_bytes());
        hasher.update(chunk.plaintext_digest);
    }
    Ok(hasher.finalize().into())
}

const JOB_READ_COLUMNS: &str = "j.job_id,j.account_id,j.creator_key_id,j.public_model,j.display_name,\
 j.priority,j.input_kind,j.input_file_id,j.output_file_id,j.cancel_requested_ts,j.create_ts,j.update_ts,\
 j.deadline_ts,j.terminal_items_ts,j.output_state,j.completed_ts,j.delete_ts,j.result_expiration_ts,\
 COUNT(i.*)::bigint,COUNT(i.*) FILTER (WHERE i.state='succeeded')::bigint,\
 COUNT(i.*) FILTER (WHERE i.state IN ('failed','indeterminate','canceled'))::bigint,\
 COUNT(i.*) FILTER (WHERE i.state NOT IN ('succeeded','failed','indeterminate','canceled'))::bigint";

fn bytes32(value: Vec<u8>, field: &str) -> Result<[u8; 32]> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid {field} length"))
}

fn job_from_row(row: &Row) -> Result<GeminiBatchJob> {
    let request_count: i64 = row.get(18);
    let successful_request_count: i64 = row.get(19);
    let failed_request_count: i64 = row.get(20);
    let pending_request_count: i64 = row.get(21);
    let completed_ts: Option<i64> = row.get(15);
    let delete_ts: Option<i64> = row.get(16);
    let result_expiration_ts: Option<i64> = row.get(17);
    let state = if delete_ts.is_some() || result_expiration_ts.is_some_and(|ts| ts <= super::now())
    {
        GeminiBatchJobState::Expired
    } else if completed_ts.is_none() {
        if successful_request_count == 0 && failed_request_count == 0 {
            GeminiBatchJobState::Pending
        } else {
            GeminiBatchJobState::Running
        }
    } else if row.get::<_, Option<i64>>(9).is_some() {
        GeminiBatchJobState::Cancelled
    } else {
        // Per-item errors remain a successfully processed operation whose output carries those
        // errors. FAILED is reserved for an explicit job-level failure, not inferred from stats.
        GeminiBatchJobState::Succeeded
    };
    Ok(GeminiBatchJob {
        job_id: row.get(0),
        account_id: row.get(1),
        creator_key_id: row.get(2),
        public_model: row.get(3),
        display_name: row.get(4),
        priority: row.get(5),
        input_kind: GeminiBatchInputKind::parse(row.get::<_, String>(6).as_str())?,
        input_file_id: row.get(7),
        output_file_id: row.get(8),
        cancel_requested_ts: row.get(9),
        create_ts: row.get(10),
        update_ts: row.get(11),
        deadline_ts: row.get(12),
        terminal_items_ts: row.get(13),
        output_state: row.get(14),
        completed_ts,
        delete_ts,
        result_expiration_ts,
        state,
        stats: GeminiBatchStats {
            request_count,
            successful_request_count,
            failed_request_count,
            pending_request_count,
        },
    })
}

fn item_from_row(row: &Row) -> Result<GeminiBatchItem> {
    Ok(GeminiBatchItem {
        job_id: row.get(0),
        item_index: row.get(1),
        request_id: row.get(2),
        logical_request_id: row.get(3),
        execution_group_id: row.get(4),
        creator_key_id: row.get(5),
        client_key: row.get(6),
        state: GeminiBatchItemState::parse(row.get::<_, String>(7).as_str())?,
        terminal_class: row
            .get::<_, Option<String>>(8)
            .map(|value| GeminiBatchTerminalClass::parse(&value))
            .transpose()?,
        claim_generation: row.get(9),
        worker_instance: row.get(10),
        worker_epoch: row.get(11),
        lease_until: row.get(12),
        selected_profile_id: row.get(13),
    })
}

fn file_from_row(row: &Row) -> Result<GeminiBatchFile> {
    Ok(GeminiBatchFile {
        file_id: row.get(0),
        account_id: row.get(1),
        display_name: row.get(2),
        mime_type: row.get(3),
        size_bytes: row.get(4),
        sha256_digest: bytes32(row.get(5), "file digest")?,
        source_kind: row.get(6),
        state: row.get(7),
        storage_kind: row.get(8),
        create_ts: row.get(9),
        update_ts: row.get(10),
        expiration_ts: row.get(11),
        received_bytes: row.get(12),
        next_chunk_index: row.get(13),
        chunk_count: row.get(14),
        chunk_manifest_digest: row
            .get::<_, Option<Vec<u8>>>(15)
            .map(|value| bytes32(value, "file chunk manifest digest"))
            .transpose()?,
        completed_ts: row.get(16),
    })
}

fn encrypted_blob_from_row(row: &Row) -> Result<GeminiBatchEncryptedBlob> {
    Ok(GeminiBatchEncryptedBlob {
        kind: row.get(0),
        key_id: row.get(1),
        nonce: row.get(2),
        ciphertext: row.get(3),
        plaintext_len: row.get(4),
        plaintext_digest: bytes32(row.get(5), "encrypted blob digest")?,
        retention_ts: row.get(6),
    })
}

fn validate_file_create(create: &GeminiBatchFileCreate) -> Result<()> {
    if create.file_id.is_empty()
        || create.account_id.is_empty()
        || create.display_name.len() > 512
        || create.mime_type.is_empty()
        || create.mime_type.len() > 255
        || !(0..=MAX_BATCH_FILE_BYTES).contains(&create.size_bytes)
        || !matches!(
            create.source_kind.as_str(),
            "client_upload" | "batch_output"
        )
        || create.create_ts <= 0
        || create.expiration_ts < create.create_ts
    {
        bail!("invalid Gemini Batch file create")
    }
    Ok(())
}

fn lock_account<'a>(tx: &mut Transaction<'a>, account_id: &str) -> Result<()> {
    tx.query_opt(
        "SELECT 1 FROM accounts WHERE id=$1 FOR UPDATE",
        &[&account_id],
    )?
    .context("Gemini Batch account does not exist")?;
    Ok(())
}

impl PgStore {
    /// Aggregate operational state for the bounded Gemini Batch metrics/admin surface.
    ///
    /// Every value is fleet-wide. No customer or workload identity leaves PostgreSQL, and one
    /// snapshot requires a fixed number of aggregate queries regardless of queue size.
    pub fn gemini_batch_operational_report(
        &mut self,
    ) -> Result<crate::GeminiBatchOperationalReport> {
        let ts = super::now();
        let mut tx = self
            .client
            .build_transaction()
            .isolation_level(postgres::IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let jobs = tx.query_one(
            "SELECT
                COUNT(*) FILTER (WHERE completed_ts IS NULL AND NOT EXISTS (
                    SELECT 1 FROM gemini_batch_items item WHERE item.job_id=job.job_id
                      AND item.state IN ('claimed','dispatching','settlement_pending')
                ))::bigint,
                COUNT(*) FILTER (WHERE completed_ts IS NULL AND EXISTS (
                    SELECT 1 FROM gemini_batch_items item WHERE item.job_id=job.job_id
                      AND item.state IN ('claimed','dispatching','settlement_pending')
                ))::bigint
             FROM gemini_batch_jobs job WHERE delete_ts IS NULL",
            &[],
        )?;
        let items = tx.query_one(
            "SELECT
                COUNT(*) FILTER (WHERE state='queued' AND next_attempt_ts <= $1)::bigint,
                COUNT(*) FILTER (WHERE state='claimed')::bigint,
                COUNT(*) FILTER (WHERE state='dispatching')::bigint,
                COUNT(*) FILTER (WHERE state='settlement_pending')::bigint,
                COUNT(*) FILTER (WHERE state='succeeded')::bigint,
                COUNT(*) FILTER (WHERE state='failed')::bigint,
                COUNT(*) FILTER (WHERE state='canceled')::bigint,
                COUNT(*) FILTER (WHERE state='indeterminate')::bigint,
                COALESCE($1 - MIN(created_ts) FILTER (WHERE state='queued' AND next_attempt_ts <= $1), 0)::bigint,
                COALESCE(SUM(hold_nano) FILTER (WHERE state IN ('queued','claimed','dispatching','settlement_pending')), 0)::bigint
             FROM gemini_batch_items",
            &[&ts],
        )?;
        let settlement = tx.query_one(
            "SELECT
                COUNT(*) FILTER (WHERE state='pending')::bigint,
                COUNT(*) FILTER (WHERE state='failed')::bigint,
                COALESCE($1 - MIN(created_ts) FILTER (WHERE state='pending'), 0)::bigint,
                COALESCE(SUM(attempts) FILTER (WHERE state='pending'), 0)::bigint
             FROM gemini_batch_settlement_outbox",
            &[&ts],
        )?;
        let leader = tx.query_opt(
            "SELECT lease_until FROM leader_leases WHERE name=$1 AND lease_until >= $2",
            &[&crate::GEMINI_BATCH_DISPATCH_LEADER, &ts],
        )?;
        let files = tx.query_one(
            "SELECT
                COALESCE(SUM(chunk.plaintext_len), 0)::bigint,
                COUNT(chunk.*)::bigint
             FROM gemini_batch_file_chunks chunk
             JOIN gemini_batch_files file ON file.file_id=chunk.file_id
             WHERE file.state='active' AND file.expiration_ts > $1 AND file.payload_deleted_ts IS NULL",
            &[&ts],
        )?;
        let window_rows = tx.query(
            "WITH windows(label,seconds) AS (VALUES ('1h'::text,3600::bigint),('24h',86400),('7d',604800))
             SELECT w.label,
                (SELECT COUNT(*) FROM gemini_batch_jobs j WHERE j.create_ts >= $1-w.seconds)::bigint,
                COUNT(*) FILTER (WHERE i.created_ts >= $1-w.seconds)::bigint,
                COUNT(*) FILTER (WHERE i.terminal_ts >= $1-w.seconds AND i.state='succeeded')::bigint,
                COUNT(*) FILTER (WHERE i.terminal_ts >= $1-w.seconds AND i.state='failed')::bigint,
                COUNT(*) FILTER (WHERE i.terminal_ts >= $1-w.seconds AND i.state='canceled')::bigint,
                COUNT(*) FILTER (WHERE i.terminal_ts >= $1-w.seconds AND i.state='indeterminate')::bigint,
                COALESCE((SELECT SUM(o.actual_nano) FROM gemini_batch_settlement_outbox o WHERE o.state='done' AND o.committed_ts >= $1-w.seconds),0)::bigint,
                AVG((i.dispatch_intent_ts-i.created_ts)::double precision) FILTER (WHERE i.dispatch_intent_ts >= $1-w.seconds AND i.dispatch_intent_ts >= i.created_ts),
                AVG((i.terminal_ts-i.dispatch_intent_ts)::double precision) FILTER (WHERE i.terminal_ts >= $1-w.seconds AND i.dispatch_intent_ts IS NOT NULL AND i.terminal_ts >= i.dispatch_intent_ts),
                (COUNT(*) FILTER (WHERE i.terminal_ts >= $1-w.seconds AND i.state IN ('succeeded','failed','canceled','indeterminate')))::double precision * 3600.0 / w.seconds::double precision
             FROM windows w LEFT JOIN gemini_batch_items i ON true
             GROUP BY w.label,w.seconds ORDER BY w.seconds",
            &[&ts],
        )?;
        let windows = window_rows
            .into_iter()
            .map(|row| crate::GeminiBatchOperationalWindow {
                window: row.get(0),
                jobs_created: row.get(1),
                items_created: row.get(2),
                succeeded: row.get(3),
                failed: row.get(4),
                canceled: row.get(5),
                indeterminate: row.get(6),
                settled_nano: row.get(7),
                avg_queue_wait_seconds: row.get(8),
                avg_execution_seconds: row.get(9),
                throughput_items_per_hour: row.get(10),
            })
            .collect();
        let report = crate::GeminiBatchOperationalReport {
            queued_jobs: jobs.get(0),
            running_jobs: jobs.get(1),
            queued_items: items.get(0),
            claimed_items: items.get(1),
            dispatching_items: items.get(2),
            settlement_pending_items: items.get(3),
            succeeded_items: items.get(4),
            failed_items: items.get(5),
            canceled_items: items.get(6),
            indeterminate_items: items.get(7),
            oldest_queued_age_seconds: items.get::<_, i64>(8).max(0),
            reserved_hold_nano: items.get(9),
            leader_held: leader.is_some(),
            leader_expires_at: leader.as_ref().map(|row| row.get(0)),
            settlement_pending: settlement.get(0),
            settlement_failed: settlement.get(1),
            settlement_oldest_age_seconds: settlement.get::<_, i64>(2).max(0),
            settlement_retries: settlement.get(3),
            active_file_bytes: files.get(0),
            active_file_chunks: files.get(1),
            windows,
        };
        tx.commit()?;
        Ok(report)
    }
    /// Create a complete batch and reserve its aggregate hold in one transaction.
    /// Locks are always acquired account first and raw access key second.
    pub fn gemini_batch_create(
        &mut self,
        create: &GeminiBatchCreate,
        creator_key: &str,
    ) -> Result<GeminiBatchCreateOutcome> {
        let aggregate_hold = create.validate()?;
        if creator_key.is_empty() {
            bail!("Gemini Batch creator key is empty")
        }
        let mut tx = self.client.transaction()?;
        if let Some(digest) = create.idempotency_digest {
            tx.query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, hashtextextended(encode($2::bytea,'hex'), 741925)))",
                &[&create.account_id, &&digest[..]],
            )?;
        }
        lock_account(&mut tx, &create.account_id)?;

        let key_row = tx.query_opt(
            "SELECT key_id,status,expires_ts,spend_limit_nano,spent_nano,reserved_nano \
             FROM api_keys WHERE key=$1 AND account_id=$2 FOR UPDATE",
            &[&creator_key, &create.account_id],
        )?;
        let Some(key_row) = key_row else {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedFunds);
        };
        let key_id: String = key_row.get(0);
        if key_id != create.creator_key_id {
            bail!("Gemini Batch creator_key_id does not match the raw key")
        }

        if let Some(digest) = create.idempotency_digest {
            if let Some(row) = tx.query_opt(
                "SELECT job_id,canonical_request_digest FROM gemini_batch_jobs \
                 WHERE account_id=$1 AND idempotency_digest=$2",
                &[&create.account_id, &&digest[..]],
            )? {
                let job_id: String = row.get(0);
                let stored = bytes32(row.get(1), "canonical request digest")?;
                if stored != create.canonical_request_digest {
                    return Err(GeminiBatchIdempotencyConflict.into());
                }
                tx.commit()?;
                return Ok(GeminiBatchCreateOutcome::Replay { job_id });
            }
        }

        if let Some(row) = tx.query_opt(
            "SELECT account_id,creator_key_id,canonical_request_digest,idempotency_digest \
             FROM gemini_batch_jobs WHERE job_id=$1",
            &[&create.job_id],
        )? {
            let exact = row.get::<_, String>(0) == create.account_id
                && row.get::<_, String>(1) == create.creator_key_id
                && bytes32(row.get(2), "canonical request digest")?
                    == create.canonical_request_digest
                && row
                    .get::<_, Option<Vec<u8>>>(3)
                    .map(|value| bytes32(value, "idempotency digest"))
                    .transpose()?
                    == create.idempotency_digest;
            if exact {
                tx.commit()?;
                return Ok(GeminiBatchCreateOutcome::Replay {
                    job_id: create.job_id.clone(),
                });
            }
            return Err(GeminiBatchIdempotencyConflict.into());
        }

        let active_jobs: i64 = tx
            .query_one(
                "SELECT COUNT(*)::bigint FROM gemini_batch_jobs \
                 WHERE account_id=$1 AND completed_ts IS NULL AND delete_ts IS NULL",
                &[&create.account_id],
            )?
            .get(0);
        if active_jobs >= MAX_BATCH_NONTERMINAL_JOBS {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedLimit);
        }

        let mut referenced_files = create
            .input_file_id
            .iter()
            .chain(
                create
                    .items
                    .iter()
                    .filter_map(|item| item.input_file_id.as_ref()),
            )
            .chain(
                create
                    .items
                    .iter()
                    .flat_map(|item| item.referenced_file_ids.iter()),
            )
            .cloned()
            .collect::<Vec<_>>();
        referenced_files.sort();
        referenced_files.dedup();
        let mut referenced_size = 0i64;
        for file_id in &referenced_files {
            let Some(row) = tx.query_opt(
                "SELECT size_bytes FROM gemini_batch_files \
                 WHERE account_id=$1 AND file_id=$2 AND state='active' AND expiration_ts>$3 \
                 FOR KEY SHARE",
                &[&create.account_id, file_id, &super::now()],
            )?
            else {
                bail!("Gemini Batch referenced file is unavailable");
            };
            referenced_size = referenced_size
                .checked_add(row.get::<_, i64>(0))
                .context("Gemini Batch referenced file size overflow")?;
        }
        if referenced_size > MAX_BATCH_REFERENCED_FILE_BYTES {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedLimit);
        }

        let now = super::now();
        let active_key = key_row.get::<_, String>(1) == "active"
            && key_row
                .get::<_, Option<i64>>(2)
                .is_none_or(|expires| expires > now);
        let within_limit = key_row.get::<_, Option<i64>>(3).is_none_or(|limit| {
            key_row
                .get::<_, i64>(4)
                .checked_add(key_row.get::<_, i64>(5))
                .and_then(|used| used.checked_add(aggregate_hold))
                .is_some_and(|used| used <= limit)
        });
        if !active_key || !within_limit {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedFunds);
        }

        let Some(account_row) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1 \
             WHERE id=$2 AND status='active' \
               AND balance_nano >= $1::bigint-$3::bigint RETURNING balance_nano",
            &[&aggregate_hold, &create.account_id, &ACCOUNT_OVERDRAFT_NANO],
        )?
        else {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedFunds);
        };
        let balance_nano: i64 = account_row.get(0);
        if tx.execute(
            "UPDATE api_keys SET reserved_nano=reserved_nano+$1 WHERE key=$2 AND account_id=$3",
            &[&aggregate_hold, &creator_key, &create.account_id],
        )? != 1
        {
            bail!("Gemini Batch creator key disappeared while locked")
        }

        tx.execute(
            "INSERT INTO gemini_batch_jobs(\
             job_id,account_id,creator_key_id,public_model,display_name,canonical_request_digest,\
             idempotency_digest,priority,input_kind,input_file_id,schema_version,\
             encryption_policy_version,create_ts,update_ts,deadline_ts,result_expiration_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,$14,NULL)",
            &[
                &create.job_id,
                &create.account_id,
                &create.creator_key_id,
                &create.public_model,
                &create.display_name,
                &&create.canonical_request_digest[..],
                &create.idempotency_digest.as_ref().map(|v| &v[..]),
                &create.priority,
                &create.input_kind.as_str(),
                &create.input_file_id,
                &create.schema_version,
                &create.encryption_policy_version,
                &create.create_ts,
                &create.deadline_ts,
            ],
        )?;

        for item in &create.items {
            tx.execute(
                "INSERT INTO gemini_batch_items(\
                 job_id,item_index,request_id,logical_request_id,execution_group_id,client_key,\
                 request_digest,input_file_id,hold_nano,payable_multiplier_bp,priced_ts,\
                 tariff_family,tariff_version,tariff_schedule_id,state,creator_key_id,created_ts,updated_ts) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,'queued',$15,$16,$16)",
                &[
                    &create.job_id,
                    &item.item_index,
                    &item.request_id,
                    &item.logical_request_id,
                    &item.execution_group_id,
                    &item.client_key,
                    &&item.request_digest[..],
                    &item.input_file_id,
                    &item.hold_nano,
                    &item.payable_multiplier_bp,
                    &item.priced_ts,
                    &item.tariff_family,
                    &item.tariff_version,
                    &item.tariff_schedule_id,
                    &create.creator_key_id,
                    &create.create_ts,
                ],
            )?;
            for (ordinal, file_id) in item.referenced_file_ids.iter().enumerate() {
                tx.execute(
                    "INSERT INTO gemini_batch_item_files(job_id,item_index,ordinal,file_id) \
                     VALUES($1,$2,$3,$4)",
                    &[&create.job_id, &item.item_index, &(ordinal as i32), file_id],
                )?;
            }
            for blob in std::iter::once(&item.request_blob).chain(item.metadata_blob.iter()) {
                tx.execute(
                    "INSERT INTO gemini_batch_blobs(\
                     job_id,item_index,kind,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,\
                     retention_ts,created_ts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                    &[
                        &create.job_id,
                        &item.item_index,
                        &blob.kind,
                        &blob.key_id,
                        &blob.nonce,
                        &blob.ciphertext,
                        &blob.plaintext_len,
                        &&blob.plaintext_digest[..],
                        &blob.retention_ts,
                        &create.create_ts,
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(GeminiBatchCreateOutcome::Created { balance_nano })
    }

    pub fn gemini_batch_get(
        &mut self,
        account_id: &str,
        job_id: &str,
    ) -> Result<Option<GeminiBatchJobDetail>> {
        let mut tx = self
            .client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let sql = format!(
            "SELECT {JOB_READ_COLUMNS} FROM gemini_batch_jobs j \
             LEFT JOIN gemini_batch_items i ON i.job_id=j.job_id \
             WHERE j.account_id=$1 AND j.job_id=$2 AND j.delete_ts IS NULL \
             GROUP BY j.job_id"
        );
        let Some(row) = tx.query_opt(&sql, &[&account_id, &job_id])? else {
            tx.commit()?;
            return Ok(None);
        };
        let job = job_from_row(&row)?;
        let items = if job.input_kind == GeminiBatchInputKind::Inline
            && job.completed_ts.is_some()
            && job.state != GeminiBatchJobState::Expired
        {
            tx.query(
                "SELECT item.job_id,item.item_index,item.request_id,item.logical_request_id,item.execution_group_id,\
                  item.creator_key_id,item.client_key,item.state,item.terminal_class,item.claim_generation,item.worker_instance,item.worker_epoch,\
                  item.lease_until,item.selected_profile_id FROM gemini_batch_items item \
                  JOIN gemini_batch_jobs scoped ON scoped.job_id=item.job_id \
                  WHERE scoped.account_id=$1 AND item.job_id=$2 ORDER BY item.item_index",
                &[&account_id, &job_id],
            )?.iter().map(item_from_row).collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        tx.commit()?;
        Ok(Some(GeminiBatchJobDetail { job, items }))
    }

    pub fn gemini_batch_list(
        &mut self,
        account_id: &str,
        cursor: Option<&GeminiBatchPageCursor>,
        limit: i64,
    ) -> Result<GeminiBatchJobPage> {
        let page_size = limit.clamp(1, MAX_BATCH_PAGE_SIZE);
        let query_limit = page_size + 1;
        let sql = format!(
            "SELECT {JOB_READ_COLUMNS} FROM gemini_batch_jobs j \
             LEFT JOIN gemini_batch_items i ON i.job_id=j.job_id \
             WHERE j.account_id=$1 AND j.delete_ts IS NULL \
               AND ($2::bigint IS NULL OR (j.create_ts,j.job_id)<($2,$3)) \
             GROUP BY j.job_id ORDER BY j.create_ts DESC,j.job_id DESC LIMIT $4"
        );
        let rows = self.client.query(
            &sql,
            &[
                &account_id,
                &cursor.map(|v| v.create_ts),
                &cursor.map(|v| v.job_id.as_str()),
                &query_limit,
            ],
        )?;
        let has_more = rows.len() as i64 > page_size;
        let mut jobs = rows
            .iter()
            .take(page_size as usize)
            .map(job_from_row)
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = if has_more {
            jobs.last().map(|job| GeminiBatchPageCursor {
                create_ts: job.create_ts,
                job_id: job.job_id.clone(),
            })
        } else {
            None
        };
        jobs.shrink_to_fit();
        Ok(GeminiBatchJobPage { jobs, next_cursor })
    }

    pub fn gemini_batch_file_create(
        &mut self,
        create: &GeminiBatchFileCreate,
    ) -> Result<GeminiBatchFileCreateOutcome> {
        validate_file_create(create)?;
        let mut tx = self.client.transaction()?;
        let Some(_) = tx.query_opt(
            "SELECT 1 FROM accounts WHERE id=$1 FOR UPDATE",
            &[&create.account_id],
        )?
        else {
            tx.rollback()?;
            return Ok(GeminiBatchFileCreateOutcome::Unavailable);
        };
        if let Some(row) = tx.query_opt(
            "SELECT account_id,display_name,mime_type,size_bytes,sha256_digest,source_kind,\
             create_ts,expiration_ts FROM gemini_batch_files WHERE file_id=$1 FOR UPDATE",
            &[&create.file_id],
        )? {
            if row.get::<_, String>(0) != create.account_id {
                tx.rollback()?;
                return Ok(GeminiBatchFileCreateOutcome::Unavailable);
            }
            let exact = row.get::<_, String>(1) == create.display_name
                && row.get::<_, String>(2) == create.mime_type
                && row.get::<_, i64>(3) == create.size_bytes
                && bytes32(row.get(4), "file digest")? == create.sha256_digest
                && row.get::<_, String>(5) == create.source_kind
                && row.get::<_, i64>(6) == create.create_ts
                && row.get::<_, i64>(7) == create.expiration_ts;
            tx.commit()?;
            return Ok(if exact {
                GeminiBatchFileCreateOutcome::Replay
            } else {
                GeminiBatchFileCreateOutcome::Unavailable
            });
        }
        let stored_bytes: i64 = tx
            .query_one(
                "SELECT COALESCE(SUM(size_bytes),0)::bigint FROM gemini_batch_files \
                 WHERE account_id=$1 AND expiration_ts>$2 AND payload_deleted_ts IS NULL",
                &[&create.account_id, &super::now()],
            )?
            .get(0);
        if stored_bytes
            .checked_add(create.size_bytes)
            .is_none_or(|total| total > MAX_BATCH_ACCOUNT_FILE_BYTES)
        {
            tx.rollback()?;
            return Ok(GeminiBatchFileCreateOutcome::RejectedQuota);
        }
        tx.execute(
            "INSERT INTO gemini_batch_files(\
             file_id,account_id,display_name,mime_type,size_bytes,sha256_digest,source_kind,state,\
             storage_kind,create_ts,update_ts,expiration_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,'processing','chunked',$8,$8,$9)",
            &[
                &create.file_id,
                &create.account_id,
                &create.display_name,
                &create.mime_type,
                &create.size_bytes,
                &&create.sha256_digest[..],
                &create.source_kind,
                &create.create_ts,
                &create.expiration_ts,
            ],
        )?;
        tx.commit()?;
        Ok(GeminiBatchFileCreateOutcome::Created)
    }

    pub fn gemini_batch_file_progress(
        &mut self,
        account_id: &str,
        file_id: &str,
    ) -> Result<Option<crate::GeminiBatchFileProgress>> {
        Ok(self.client.query_opt(
            "SELECT received_bytes,next_chunk_index,chunk_count,size_bytes,state='active' FROM gemini_batch_files WHERE account_id=$1 AND file_id=$2 AND expiration_ts>$3 AND payload_deleted_ts IS NULL",
            &[&account_id,&file_id,&super::now()],
        )?.map(|row| crate::GeminiBatchFileProgress {
            received_bytes: row.get(0), next_chunk_index: row.get(1), chunk_count: row.get(2),
            size_bytes: row.get(3), active: row.get(4),
        }))
    }

    pub fn gemini_batch_file_append_chunk_at(
        &mut self,
        account_id: &str,
        file_id: &str,
        expected_offset: i64,
        chunk: &GeminiBatchFileChunk,
    ) -> Result<crate::GeminiBatchFileAppendOutcome> {
        chunk.validate()?;
        let mut tx = self.client.transaction()?;
        let Some(file) = tx.query_opt(
            "SELECT state,storage_kind,size_bytes,create_ts,received_bytes,next_chunk_index,chunk_count
             FROM gemini_batch_files WHERE account_id=$1 AND file_id=$2 FOR UPDATE",
            &[&account_id, &file_id],
        )? else {
            tx.rollback()?;
            return Ok(crate::GeminiBatchFileAppendOutcome::Unavailable);
        };
        let progress = crate::GeminiBatchFileProgress {
            received_bytes: file.get(4),
            next_chunk_index: file.get(5),
            chunk_count: file.get(6),
            size_bytes: file.get(2),
            active: file.get::<_, String>(0) == "active",
        };
        if file.get::<_, String>(0) != "processing" || file.get::<_, String>(1) != "chunked" {
            tx.commit()?;
            return Ok(crate::GeminiBatchFileAppendOutcome::OffsetConflict(
                progress,
            ));
        }
        if expected_offset != progress.received_bytes
            || chunk.chunk_index != progress.next_chunk_index
        {
            tx.commit()?;
            return Ok(crate::GeminiBatchFileAppendOutcome::OffsetConflict(
                progress,
            ));
        }
        if chunk.created_ts < file.get::<_, i64>(3) {
            bail!("Gemini Batch file chunk predates its file")
        }
        let next_received = progress
            .received_bytes
            .checked_add(chunk.plaintext_len)
            .context("Gemini Batch file size overflow")?;
        if next_received > progress.size_bytes {
            bail!("Gemini Batch file chunks exceed declared size")
        }
        // Every non-final physical chunk is exactly 8 MiB; at most 256 rows can represent 2 GiB.
        if next_received < progress.size_bytes && chunk.plaintext_len != MAX_BATCH_FILE_CHUNK_BYTES
        {
            bail!("Gemini Batch non-final upload chunk must be exactly 8 MiB")
        }
        if progress.next_chunk_index >= 256 {
            bail!("Gemini Batch file has too many chunks")
        }
        tx.execute(
            "INSERT INTO gemini_batch_file_chunks(file_id,chunk_index,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,created_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            &[&file_id,&chunk.chunk_index,&chunk.key_id,&chunk.nonce,&chunk.ciphertext,
              &chunk.plaintext_len,&&chunk.plaintext_digest[..],&chunk.created_ts],
        )?;
        let next_index = progress.next_chunk_index + 1;
        if tx.execute(
            "UPDATE gemini_batch_files SET received_bytes=$3,next_chunk_index=$4,chunk_count=$4,
             update_ts=GREATEST(update_ts,$5) WHERE account_id=$1 AND file_id=$2
             AND state='processing' AND received_bytes=$6 AND next_chunk_index=$7",
            &[
                &account_id,
                &file_id,
                &next_received,
                &next_index,
                &chunk.created_ts,
                &progress.received_bytes,
                &progress.next_chunk_index,
            ],
        )? != 1
        {
            bail!("Gemini Batch upload progress CAS failed")
        }
        tx.commit()?;
        Ok(crate::GeminiBatchFileAppendOutcome::Appended(
            crate::GeminiBatchFileProgress {
                received_bytes: next_received,
                next_chunk_index: next_index,
                chunk_count: next_index,
                size_bytes: progress.size_bytes,
                active: false,
            },
        ))
    }

    /// Compatibility wrapper for existing internal callers.
    pub fn gemini_batch_file_append_chunk(
        &mut self,
        account_id: &str,
        file_id: &str,
        chunk: &GeminiBatchFileChunk,
    ) -> Result<bool> {
        let Some(progress) = self.gemini_batch_file_progress(account_id, file_id)? else {
            return Ok(false);
        };
        if chunk.chunk_index < progress.next_chunk_index {
            return Ok(self.client.query_opt(
                "SELECT 1 FROM gemini_batch_file_chunks c JOIN gemini_batch_files f USING(file_id) WHERE f.account_id=$1 AND c.file_id=$2 AND c.chunk_index=$3 AND c.key_id=$4 AND c.nonce=$5 AND c.ciphertext=$6 AND c.plaintext_len=$7 AND c.plaintext_digest=$8",
                &[&account_id,&file_id,&chunk.chunk_index,&chunk.key_id,&chunk.nonce,&chunk.ciphertext,&chunk.plaintext_len,&&chunk.plaintext_digest[..]],
            )?.is_some());
        }
        Ok(matches!(
            self.gemini_batch_file_append_chunk_at(
                account_id,
                file_id,
                progress.received_bytes,
                chunk
            )?,
            crate::GeminiBatchFileAppendOutcome::Appended(_)
        ))
    }

    pub fn gemini_batch_file_complete(
        &mut self,
        account_id: &str,
        file_id: &str,
        completion: &GeminiBatchFileCompletion,
    ) -> Result<bool> {
        if completion.completed_ts <= 0 {
            bail!("invalid Gemini Batch file completion timestamp")
        }
        let mut tx = self.client.transaction()?;
        let Some(file) = tx.query_opt(
            "SELECT state,size_bytes,sha256_digest,create_ts FROM gemini_batch_files \
             WHERE account_id=$1 AND file_id=$2 FOR UPDATE",
            &[&account_id, &file_id],
        )?
        else {
            tx.rollback()?;
            return Ok(false);
        };
        let declared_digest = bytes32(file.get(2), "file digest")?;
        // Resumable start persists a zero digest because the whole plaintext does not exist yet.
        // Completion supplies the authenticated streaming digest; nonzero declarations remain exact.
        if declared_digest != [0; 32] && declared_digest != completion.whole_file_sha256_digest {
            bail!("Gemini Batch whole-file digest mismatch")
        }
        if file.get::<_, String>(0) == "active" {
            tx.commit()?;
            return Ok(true);
        }
        if file.get::<_, String>(0) != "processing"
            || completion.completed_ts < file.get::<_, i64>(3)
        {
            bail!("Gemini Batch file cannot be completed")
        }
        let chunks = tx.query(
            "SELECT c.chunk_index,c.plaintext_len,c.plaintext_digest \
             FROM gemini_batch_file_chunks c JOIN gemini_batch_files f USING(file_id) \
             WHERE f.account_id=$1 AND c.file_id=$2 ORDER BY c.chunk_index",
            &[&account_id, &file_id],
        )?;
        let mut manifest = Sha256::new();
        manifest.update(FILE_CHUNK_MANIFEST_DOMAIN);
        manifest.update(
            u64::try_from(chunks.len())
                .context("Gemini Batch chunk count overflow")?
                .to_be_bytes(),
        );
        let mut total = 0i64;
        for (expected, chunk) in chunks.iter().enumerate() {
            let chunk_index: i64 = chunk.get(0);
            let plaintext_len: i64 = chunk.get(1);
            if chunk_index
                != i64::try_from(expected).context("Gemini Batch chunk index overflow")?
            {
                bail!("Gemini Batch file chunks are not contiguous")
            }
            let digest = bytes32(chunk.get(2), "file chunk digest")?;
            manifest.update(chunk_index.to_be_bytes());
            manifest.update(plaintext_len.to_be_bytes());
            manifest.update(digest);
            total = total
                .checked_add(plaintext_len)
                .context("Gemini Batch file size overflow")?;
        }
        let durable_manifest: [u8; 32] = manifest.finalize().into();
        if durable_manifest != completion.chunk_manifest_digest {
            bail!("Gemini Batch file chunk manifest mismatch")
        }
        if total != file.get::<_, i64>(1) {
            bail!("Gemini Batch file size does not match its chunks")
        }
        if declared_digest != [0; 32]
            && chunks.len() == 1
            && bytes32(chunks[0].get(2), "file chunk digest")? != declared_digest
        {
            bail!("Gemini Batch single-chunk file digest mismatch")
        }
        if tx.execute(
            "UPDATE gemini_batch_files SET state='active',sha256_digest=$3,chunk_manifest_digest=$4,completed_ts=$5,update_ts=$5 \
              WHERE account_id=$1 AND file_id=$2 AND state='processing' AND received_bytes=size_bytes \
                AND chunk_count=next_chunk_index",
            &[&account_id, &file_id, &&completion.whole_file_sha256_digest[..],
              &&completion.chunk_manifest_digest[..], &completion.completed_ts],
        )? != 1 { bail!("Gemini Batch file completion CAS failed") }
        tx.commit()?;
        Ok(true)
    }

    pub fn gemini_batch_file_get(
        &mut self,
        account_id: &str,
        file_id: &str,
    ) -> Result<Option<GeminiBatchFile>> {
        Ok(self
            .client
            .query_opt(
                "SELECT file_id,account_id,display_name,mime_type,size_bytes,sha256_digest,\
                 source_kind,state,storage_kind,create_ts,update_ts,expiration_ts,received_bytes,next_chunk_index,chunk_count,chunk_manifest_digest,completed_ts \
                 FROM gemini_batch_files \
                 WHERE account_id=$1 AND file_id=$2 AND expiration_ts>$3 AND payload_deleted_ts IS NULL",
                &[&account_id, &file_id, &super::now()],
            )?
            .as_ref()
            .map(file_from_row)
            .transpose()?)
    }

    pub fn gemini_batch_file_list(
        &mut self,
        account_id: &str,
        limit: i64,
    ) -> Result<Vec<GeminiBatchFile>> {
        Ok(self
            .client
            .query(
                "SELECT file_id,account_id,display_name,mime_type,size_bytes,sha256_digest,\
                 source_kind,state,storage_kind,create_ts,update_ts,expiration_ts,received_bytes,next_chunk_index,chunk_count,chunk_manifest_digest,completed_ts \
                 FROM gemini_batch_files WHERE account_id=$1 \
                 AND expiration_ts>$3 AND payload_deleted_ts IS NULL ORDER BY create_ts DESC,file_id DESC LIMIT $2",
                &[
                    &account_id,
                    &limit.clamp(1, MAX_BATCH_PAGE_SIZE),
                    &super::now(),
                ],
            )?
            .iter()
            .map(file_from_row)
            .collect::<Result<Vec<_>>>()?)
    }

    /// Read one encrypted item blob through its owning account boundary.
    pub fn gemini_batch_blob_get(
        &mut self,
        account_id: &str,
        job_id: &str,
        item_index: i64,
        kind: &str,
    ) -> Result<Option<GeminiBatchEncryptedBlob>> {
        if item_index < 0 || !matches!(kind, "request" | "metadata" | "result" | "error") {
            bail!("invalid Gemini Batch blob read")
        }
        let row = self.client.query_opt(
            "SELECT b.kind,b.key_id,b.nonce,b.ciphertext,b.plaintext_len,b.plaintext_digest,b.retention_ts \
             FROM gemini_batch_blobs b JOIN gemini_batch_jobs j USING(job_id) \
             WHERE j.account_id=$1 AND b.job_id=$2 AND b.item_index=$3 AND b.kind=$4 \
               AND j.delete_ts IS NULL AND ($4 IN ('request','metadata') \
                    OR (j.result_expiration_ts IS NOT NULL AND j.result_expiration_ts>$5))",
            &[&account_id, &job_id, &item_index, &kind, &super::now()],
        )?;
        row.as_ref().map(encrypted_blob_from_row).transpose()
    }

    /// Read a bounded ascending page of encrypted chunks; never materializes a logical file.
    pub fn gemini_batch_file_chunk_page(
        &mut self,
        account_id: &str,
        file_id: &str,
        after_chunk_index: Option<i64>,
        limit: i64,
    ) -> Result<GeminiBatchFileChunkPage> {
        if after_chunk_index.is_some_and(|index| index < 0) {
            bail!("invalid Gemini Batch file chunk cursor")
        }
        let page_size = limit.clamp(1, MAX_BATCH_FILE_CHUNK_PAGE_SIZE);
        let rows = self.client.query(
            "SELECT c.chunk_index,c.key_id,c.nonce,c.ciphertext,c.plaintext_len,\
                    c.plaintext_digest,c.created_ts \
             FROM gemini_batch_file_chunks c JOIN gemini_batch_files f USING(file_id) \
             WHERE f.account_id=$1 AND c.file_id=$2 AND f.state IN ('processing','active') \
               AND f.storage_kind='chunked' AND f.expiration_ts>$3 \
               AND ($4::bigint IS NULL OR c.chunk_index>$4) \
             ORDER BY c.chunk_index LIMIT $5",
            &[
                &account_id,
                &file_id,
                &super::now(),
                &after_chunk_index,
                &(page_size + 1),
            ],
        )?;
        let has_more = rows.len() as i64 > page_size;
        let chunks = rows
            .iter()
            .take(page_size as usize)
            .map(|row| {
                Ok(GeminiBatchFileChunk {
                    chunk_index: row.get(0),
                    key_id: row.get(1),
                    nonce: row.get(2),
                    ciphertext: row.get(3),
                    plaintext_len: row.get(4),
                    plaintext_digest: bytes32(row.get(5), "file chunk digest")?,
                    created_ts: row.get(6),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let next_chunk_index =
            has_more.then(|| chunks.last().expect("nonempty bounded page").chunk_index);
        Ok(GeminiBatchFileChunkPage {
            chunks,
            next_chunk_index,
        })
    }

    /// Link an active batch-output file and extend it through the job result lifetime.
    pub fn gemini_batch_link_output_file(
        &mut self,
        account_id: &str,
        job_id: &str,
        file_id: &str,
    ) -> Result<bool> {
        let mut tx = self.client.transaction()?;
        let Some(job) = tx.query_opt(
            "SELECT completed_ts,result_expiration_ts,output_file_id FROM gemini_batch_jobs \
             WHERE account_id=$1 AND job_id=$2 AND delete_ts IS NULL FOR UPDATE",
            &[&account_id, &job_id],
        )?
        else {
            tx.rollback()?;
            return Ok(false);
        };
        let completed_ts: Option<i64> = job.get(0);
        let expiration_ts: Option<i64> = job.get(1);
        let existing: Option<String> = job.get(2);
        if existing.as_deref() == Some(file_id) {
            tx.commit()?;
            return Ok(true);
        }
        if existing.is_some() {
            bail!("Gemini Batch output file linkage conflicts with stored data")
        }
        let completed_ts =
            completed_ts.context("Gemini Batch output file requires a completed job")?;
        let expiration_ts =
            expiration_ts.context("Gemini Batch completed job has no result expiration")?;
        if tx.execute(
            "UPDATE gemini_batch_files SET expiration_ts=GREATEST(expiration_ts,$3),\
                    update_ts=GREATEST(update_ts,$4) \
             WHERE account_id=$1 AND file_id=$2 AND source_kind='batch_output' \
               AND state='active' AND storage_kind='chunked' AND expiration_ts>$4",
            &[&account_id, &file_id, &expiration_ts, &super::now()],
        )? != 1
        {
            tx.rollback()?;
            return Ok(false);
        }
        if tx.execute(
            "UPDATE gemini_batch_jobs SET output_file_id=$3,update_ts=GREATEST(update_ts,$4) \
             WHERE account_id=$1 AND job_id=$2 AND output_file_id IS NULL",
            &[&account_id, &job_id, &file_id, &completed_ts],
        )? != 1
        {
            bail!("Gemini Batch output file linkage lost its locked job")
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn gemini_batch_file_delete(&mut self, account_id: &str, file_id: &str) -> Result<bool> {
        let ts = super::now();
        let mut tx = self.client.transaction()?;
        let Some(file) = tx.query_opt(
            "SELECT payload_deleted_ts FROM gemini_batch_files WHERE account_id=$1 AND file_id=$2 FOR UPDATE",
            &[&account_id, &file_id],
        )?
        else {
            tx.rollback()?;
            return Ok(false);
        };
        if file.get::<_, Option<i64>>(0).is_some() {
            tx.commit()?;
            return Ok(true);
        }
        let referenced: bool = tx
            .query_one(
                "SELECT EXISTS(\
                 SELECT 1 FROM gemini_batch_jobs j \
                  WHERE j.account_id=$1 AND j.delete_ts IS NULL \
                    AND (j.completed_ts IS NULL OR j.result_expiration_ts>$3) \
                    AND (j.input_file_id=$2 OR j.output_file_id=$2) \
                 UNION ALL SELECT 1 FROM gemini_batch_items i \
                  JOIN gemini_batch_jobs j USING(job_id) \
                  WHERE j.account_id=$1 AND j.delete_ts IS NULL \
                    AND (j.completed_ts IS NULL OR j.result_expiration_ts>$3) \
                    AND i.input_file_id=$2 \
                 UNION ALL SELECT 1 FROM gemini_batch_item_files r \
                  JOIN gemini_batch_jobs j USING(job_id) \
                  WHERE j.account_id=$1 AND j.delete_ts IS NULL \
                    AND (j.completed_ts IS NULL OR j.result_expiration_ts>$3) \
                    AND r.file_id=$2)",
                &[&account_id, &file_id, &ts],
            )?
            .get(0);
        if referenced {
            bail!("Gemini Batch file is referenced")
        }
        tx.execute(
            "DELETE FROM gemini_batch_file_chunks c USING gemini_batch_files f \
             WHERE f.account_id=$1 AND f.file_id=$2 AND c.file_id=f.file_id",
            &[&account_id, &file_id],
        )?;
        tx.execute(
            "UPDATE gemini_batch_files SET payload_deleted_ts=$3,update_ts=GREATEST(update_ts,$3),received_bytes=0,next_chunk_index=0,chunk_count=0 WHERE account_id=$1 AND file_id=$2 AND payload_deleted_ts IS NULL",
            &[&account_id, &file_id, &ts],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
