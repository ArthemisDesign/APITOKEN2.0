use super::PgStore;
use crate::gemini_batch::{
    GeminiBatchCreate, GeminiBatchCreateOutcome, GeminiBatchFile, GeminiBatchFileChunk,
    GeminiBatchFileCreate, GeminiBatchIdempotencyConflict, GeminiBatchInputKind, GeminiBatchItem,
    GeminiBatchItemState, GeminiBatchJob, GeminiBatchJobDetail, GeminiBatchJobPage,
    GeminiBatchJobState, GeminiBatchPageCursor, GeminiBatchStats, GeminiBatchTerminalClass,
    MAX_BATCH_PAGE_SIZE,
};
use crate::ACCOUNT_OVERDRAFT_NANO;
use anyhow::{bail, Context, Result};
use postgres::{IsolationLevel, Row, Transaction};

const JOB_READ_COLUMNS: &str = "j.job_id,j.account_id,j.creator_key_id,j.public_model,j.display_name,\
 j.priority,j.input_kind,j.cancel_requested_ts,j.create_ts,j.update_ts,j.deadline_ts,j.completed_ts,\
 j.delete_ts,j.result_expiration_ts,COUNT(i.*)::bigint,\
 COUNT(i.*) FILTER (WHERE i.state='succeeded')::bigint,\
 COUNT(i.*) FILTER (WHERE i.state IN ('failed','indeterminate','canceled'))::bigint,\
 COUNT(i.*) FILTER (WHERE i.state NOT IN ('succeeded','failed','indeterminate','canceled'))::bigint";

fn bytes32(value: Vec<u8>, field: &str) -> Result<[u8; 32]> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid {field} length"))
}

fn job_from_row(row: &Row) -> Result<GeminiBatchJob> {
    let request_count: i64 = row.get(14);
    let successful_request_count: i64 = row.get(15);
    let failed_request_count: i64 = row.get(16);
    let pending_request_count: i64 = row.get(17);
    let completed_ts: Option<i64> = row.get(11);
    let delete_ts: Option<i64> = row.get(12);
    let result_expiration_ts: Option<i64> = row.get(13);
    let state = if delete_ts.is_some() || result_expiration_ts.is_some_and(|ts| ts <= super::now())
    {
        GeminiBatchJobState::Expired
    } else if completed_ts.is_none() {
        if pending_request_count == request_count {
            GeminiBatchJobState::Pending
        } else {
            GeminiBatchJobState::Running
        }
    } else if row.get::<_, Option<i64>>(7).is_some() {
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
        cancel_requested_ts: row.get(7),
        create_ts: row.get(8),
        update_ts: row.get(9),
        deadline_ts: row.get(10),
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
        state: GeminiBatchItemState::parse(row.get::<_, String>(6).as_str())?,
        terminal_class: row
            .get::<_, Option<String>>(7)
            .map(|value| GeminiBatchTerminalClass::parse(&value))
            .transpose()?,
        claim_generation: row.get(8),
        worker_instance: row.get(9),
        worker_epoch: row.get(10),
        lease_until: row.get(11),
        selected_profile_id: row.get(12),
    })
}

fn file_from_row(row: &Row) -> GeminiBatchFile {
    GeminiBatchFile {
        file_id: row.get(0),
        account_id: row.get(1),
        display_name: row.get(2),
        mime_type: row.get(3),
        size_bytes: row.get(4),
        source_kind: row.get(5),
        state: row.get(6),
        create_ts: row.get(7),
        expiration_ts: row.get(8),
    }
}

fn validate_file_create(create: &GeminiBatchFileCreate) -> Result<()> {
    if create.file_id.is_empty()
        || create.account_id.is_empty()
        || create.display_name.len() > 512
        || create.mime_type.is_empty()
        || create.mime_type.len() > 255
        || create.size_bytes < 0
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

        if tx
            .query_opt(
                "SELECT 1 FROM gemini_batch_jobs WHERE job_id=$1",
                &[&create.job_id],
            )?
            .is_some()
        {
            return Err(GeminiBatchIdempotencyConflict.into());
        }

        let mut referenced_files = create
            .input_file_id
            .iter()
            .chain(create.items.iter().filter_map(|item| item.input_file_id.as_ref()))
            .chain(create.items.iter().flat_map(|item| item.referenced_file_ids.iter()))
            .cloned()
            .collect::<Vec<_>>();
        referenced_files.sort();
        referenced_files.dedup();
        for file_id in &referenced_files {
            if tx.query_opt(
                "SELECT 1 FROM gemini_batch_files WHERE account_id=$1 AND file_id=$2 AND state='active' AND expiration_ts>$3 FOR KEY SHARE",
                &[&create.account_id, file_id, &super::now()],
            )?.is_none() {
                bail!("Gemini Batch referenced file is unavailable");
            }
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
             WHERE j.account_id=$1 AND j.job_id=$2 GROUP BY j.job_id"
        );
        let Some(row) = tx.query_opt(&sql, &[&account_id, &job_id])? else {
            tx.commit()?;
            return Ok(None);
        };
        let job = job_from_row(&row)?;
        let items = tx
            .query(
                "SELECT item.job_id,item.item_index,item.request_id,item.logical_request_id,item.execution_group_id,\
                  item.creator_key_id,item.state,item.terminal_class,item.claim_generation,item.worker_instance,item.worker_epoch,\
                  item.lease_until,item.selected_profile_id FROM gemini_batch_items item \
                  JOIN gemini_batch_jobs scoped ON scoped.job_id=item.job_id \
                  WHERE scoped.account_id=$1 AND item.job_id=$2 ORDER BY item.item_index",
                &[&account_id, &job_id],
            )?
            .iter()
            .map(item_from_row)
            .collect::<Result<Vec<_>>>()?;
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
             WHERE j.account_id=$1 AND ($2::bigint IS NULL OR (j.create_ts,j.job_id)<($2,$3)) \
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

    pub fn gemini_batch_file_create(&mut self, create: &GeminiBatchFileCreate) -> Result<bool> {
        validate_file_create(create)?;
        Ok(self.client.execute(
            "INSERT INTO gemini_batch_files(\
             file_id,account_id,display_name,mime_type,size_bytes,sha256_digest,source_kind,state,\
             storage_kind,create_ts,update_ts,expiration_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,'processing','chunked',$8,$8,$9) \
             ON CONFLICT(file_id) DO NOTHING",
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
        )? == 1)
    }

    pub fn gemini_batch_file_append_chunk(
        &mut self,
        account_id: &str,
        file_id: &str,
        chunk: &GeminiBatchFileChunk,
    ) -> Result<bool> {
        chunk.validate()?;
        let mut tx = self.client.transaction()?;
        let file = tx.query_opt(
            "SELECT state,storage_kind FROM gemini_batch_files \
             WHERE account_id=$1 AND file_id=$2 FOR UPDATE",
            &[&account_id, &file_id],
        )?;
        let Some(file) = file else {
            tx.rollback()?;
            return Ok(false);
        };
        if file.get::<_, String>(0) != "processing" || file.get::<_, String>(1) != "chunked" {
            bail!("Gemini Batch file is not appendable")
        }
        let expected_index: i64 = tx
            .query_one(
                "SELECT COUNT(*)::bigint FROM gemini_batch_file_chunks WHERE file_id=$1",
                &[&file_id],
            )?
            .get(0);
        if chunk.chunk_index != expected_index {
            if chunk.chunk_index < expected_index {
                let row = tx.query_one(
                    "SELECT key_id,nonce,ciphertext,plaintext_len,plaintext_digest,created_ts \
                     FROM gemini_batch_file_chunks WHERE file_id=$1 AND chunk_index=$2",
                    &[&file_id, &chunk.chunk_index],
                )?;
                let exact = row.get::<_, String>(0) == chunk.key_id
                    && row.get::<_, Vec<u8>>(1) == chunk.nonce
                    && row.get::<_, Vec<u8>>(2) == chunk.ciphertext
                    && row.get::<_, i64>(3) == chunk.plaintext_len
                    && bytes32(row.get(4), "file chunk digest")? == chunk.plaintext_digest
                    && row.get::<_, i64>(5) == chunk.created_ts;
                if exact {
                    tx.commit()?;
                    return Ok(true);
                }
            }
            bail!("Gemini Batch file chunk is non-contiguous or conflicts with stored data")
        }
        tx.execute(
            "INSERT INTO gemini_batch_file_chunks(\
             file_id,chunk_index,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,created_ts) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &file_id,
                &chunk.chunk_index,
                &chunk.key_id,
                &chunk.nonce,
                &chunk.ciphertext,
                &chunk.plaintext_len,
                &&chunk.plaintext_digest[..],
                &chunk.created_ts,
            ],
        )?;
        tx.execute(
            "UPDATE gemini_batch_files SET update_ts=GREATEST(update_ts,$3) \
             WHERE account_id=$1 AND file_id=$2",
            &[&account_id, &file_id, &chunk.created_ts],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn gemini_batch_file_complete(
        &mut self,
        account_id: &str,
        file_id: &str,
        completed_ts: i64,
    ) -> Result<bool> {
        if completed_ts <= 0 {
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
        if file.get::<_, String>(0) == "active" {
            tx.commit()?;
            return Ok(true);
        }
        if file.get::<_, String>(0) != "processing" || completed_ts < file.get::<_, i64>(3) {
            bail!("Gemini Batch file cannot be completed")
        }
        let chunks = tx.query(
            "SELECT chunk_index,plaintext_len,plaintext_digest FROM gemini_batch_file_chunks \
             WHERE file_id=$1 ORDER BY chunk_index",
            &[&file_id],
        )?;
        let mut total = 0i64;
        for (expected, chunk) in chunks.iter().enumerate() {
            if chunk.get::<_, i64>(0) != expected as i64 {
                bail!("Gemini Batch file chunks are not contiguous")
            }
            total = total
                .checked_add(chunk.get::<_, i64>(1))
                .context("Gemini Batch file size overflow")?;
        }
        if total != file.get::<_, i64>(1) {
            bail!("Gemini Batch file size does not match its chunks")
        }
        if chunks.len() == 1 {
            let digest = bytes32(chunks[0].get(2), "file chunk digest")?;
            let expected = bytes32(file.get(2), "file digest")?;
            if digest != expected {
                bail!("Gemini Batch single-chunk file digest mismatch")
            }
        }
        tx.execute(
            "UPDATE gemini_batch_files SET state='active',update_ts=$3 \
             WHERE account_id=$1 AND file_id=$2",
            &[&account_id, &file_id, &completed_ts],
        )?;
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
                "SELECT file_id,account_id,display_name,mime_type,size_bytes,source_kind,state,\
                 create_ts,expiration_ts FROM gemini_batch_files \
                 WHERE account_id=$1 AND file_id=$2",
                &[&account_id, &file_id],
            )?
            .as_ref()
            .map(file_from_row))
    }

    pub fn gemini_batch_file_list(
        &mut self,
        account_id: &str,
        limit: i64,
    ) -> Result<Vec<GeminiBatchFile>> {
        Ok(self
            .client
            .query(
                "SELECT file_id,account_id,display_name,mime_type,size_bytes,source_kind,state,\
                 create_ts,expiration_ts FROM gemini_batch_files WHERE account_id=$1 \
                 ORDER BY create_ts DESC,file_id DESC LIMIT $2",
                &[&account_id, &limit.clamp(1, MAX_BATCH_PAGE_SIZE)],
            )?
            .iter()
            .map(file_from_row)
            .collect())
    }

    pub fn gemini_batch_file_delete(&mut self, account_id: &str, file_id: &str) -> Result<bool> {
        let mut tx = self.client.transaction()?;
        let Some(file) = tx.query_opt(
            "SELECT 1 FROM gemini_batch_files WHERE account_id=$1 AND file_id=$2 FOR UPDATE",
            &[&account_id, &file_id],
        )?
        else {
            tx.rollback()?;
            return Ok(false);
        };
        let _ = file;
        let referenced: bool = tx
            .query_one(
                "SELECT EXISTS(\
                 SELECT 1 FROM gemini_batch_jobs WHERE input_file_id=$1 OR output_file_id=$1 \
                 UNION ALL SELECT 1 FROM gemini_batch_items WHERE input_file_id=$1 \
                 UNION ALL SELECT 1 FROM gemini_batch_item_files WHERE file_id=$1)",
                &[&file_id],
            )?
            .get(0);
        if referenced {
            bail!("Gemini Batch file is referenced")
        }
        tx.execute(
            "DELETE FROM gemini_batch_file_chunks WHERE file_id=$1",
            &[&file_id],
        )?;
        tx.execute(
            "DELETE FROM gemini_batch_files WHERE account_id=$1 AND file_id=$2",
            &[&account_id, &file_id],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
