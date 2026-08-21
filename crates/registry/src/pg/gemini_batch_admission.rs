//! Schema-60 staged Gemini Batch admission.
//!
//! Staging never owns money and never makes rows visible to dispatch. Small ciphertext-only pages
//! are persisted before the final transaction locks the account/key and promotes the complete set.

use super::PgStore;
use crate::{
    GeminiBatchAdmissionBegin, GeminiBatchAdmissionBeginOutcome, GeminiBatchAdmissionItem,
    GeminiBatchCreateOutcome, GeminiBatchIdempotencyConflict, ACCOUNT_OVERDRAFT_NANO,
    MAX_BATCH_ADMISSION_PAGE_SIZE, MAX_BATCH_ITEMS, MAX_BATCH_NONTERMINAL_JOBS,
    MAX_BATCH_REFERENCED_FILE_BYTES,
};
use anyhow::{bail, Context, Result};
use postgres::Transaction;

pub const GEMINI_BATCH_ADMISSION_PAGE_SIZE: usize = MAX_BATCH_ADMISSION_PAGE_SIZE;
pub const GEMINI_BATCH_MAX_ITEMS: i64 = MAX_BATCH_ITEMS;

fn bytes32(value: Vec<u8>, field: &str) -> Result<[u8; 32]> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid {field} length"))
}

fn lock_account(tx: &mut Transaction<'_>, account_id: &str) -> Result<()> {
    tx.query_opt(
        "SELECT 1 FROM accounts WHERE id=$1 FOR UPDATE",
        &[&account_id],
    )?
    .context("Gemini Batch account does not exist")?;
    Ok(())
}

impl PgStore {
    pub fn gemini_batch_admission_begin(
        &mut self,
        begin: &GeminiBatchAdmissionBegin,
    ) -> Result<GeminiBatchAdmissionBeginOutcome> {
        begin.validate()?;
        let mut tx = self.client.transaction()?;
        if let Some(digest) = begin.idempotency_digest {
            tx.query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1,hashtextextended(encode($2::bytea,'hex'),741925)))",
                &[&begin.account_id, &&digest[..]],
            )?;
            if let Some(row) = tx.query_opt(
                "SELECT job_id,canonical_request_digest FROM gemini_batch_jobs WHERE account_id=$1 AND idempotency_digest=$2",
                &[&begin.account_id, &&digest[..]],
            )? {
                let job_id = row.get(0);
                let canonical_request_digest = bytes32(row.get(1), "canonical request digest")?;
                tx.commit()?;
                return Ok(GeminiBatchAdmissionBeginOutcome::Replay {
                    job_id,
                    canonical_request_digest,
                });
            }
            if let Some(row) = tx.query_opt(
                "SELECT admission_id,job_id,creator_key_id,public_model,display_name,priority,input_kind,input_file_id,\
                 schema_version,encryption_policy_version,state,next_item_index,create_ts,deadline_ts,expires_ts FROM gemini_batch_admissions \
                 WHERE account_id=$1 AND idempotency_digest=$2 FOR UPDATE",
                &[&begin.account_id, &&digest[..]],
            )? {
                let exact = row.get::<_, String>(2) == begin.creator_key_id
                    && row.get::<_, String>(3) == begin.public_model
                    && row.get::<_, String>(4) == begin.display_name
                    && row.get::<_, i64>(5) == begin.priority
                    && row.get::<_, String>(6) == begin.input_kind.as_str()
                    && row.get::<_, Option<String>>(7) == begin.input_file_id
                    && row.get::<_, i32>(8) == begin.schema_version
                    && row.get::<_, i32>(9) == begin.encryption_policy_version;
                if !exact {
                    return Err(GeminiBatchIdempotencyConflict.into());
                }
                let state: String = row.get(10);
                if state == "committed" {
                    let job_id = row.get(1);
                    let stored = tx.query_one(
                        "SELECT canonical_request_digest FROM gemini_batch_jobs WHERE job_id=$1",
                        &[&job_id],
                    )?;
                    let canonical_request_digest =
                        bytes32(stored.get(0), "canonical request digest")?;
                    tx.commit()?;
                    return Ok(GeminiBatchAdmissionBeginOutcome::Replay {
                        job_id,
                        canonical_request_digest,
                    });
                }
                if state != "staging" {
                    return Err(GeminiBatchIdempotencyConflict.into());
                }
                let admission_id: String = row.get(0);
                // Restart incomplete staging from zero under the persisted identity. A retry can
                // never publish a hybrid of old and new customer prompts or file references.
                tx.execute("DELETE FROM gemini_batch_admission_items WHERE admission_id=$1", &[&admission_id])?;
                tx.execute("UPDATE gemini_batch_admissions SET next_item_index=0,aggregate_hold_nano=0,aggregate_output_tokens=0,update_ts=$2 WHERE admission_id=$1 AND state='staging'", &[&admission_id,&super::now()])?;
                let outcome = GeminiBatchAdmissionBeginOutcome::Started {
                    admission_id,
                    job_id: row.get(1),
                    next_item_index: 0,
                    create_ts: row.get(12), deadline_ts: row.get(13), expires_ts: row.get(14),
                };
                tx.commit()?;
                return Ok(outcome);
            }
        }
        tx.execute(
            "INSERT INTO gemini_batch_admissions(admission_id,job_id,account_id,creator_key_id,public_model,display_name,\
             idempotency_digest,priority,input_kind,input_file_id,schema_version,encryption_policy_version,state,\
             next_item_index,aggregate_hold_nano,aggregate_output_tokens,create_ts,deadline_ts,expires_ts,update_ts)\
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'staging',0,0,0,$13,$14,$15,$13)",
            &[
                &begin.admission_id,
                &begin.job_id,
                &begin.account_id,
                &begin.creator_key_id,
                &begin.public_model,
                &begin.display_name,
                &begin.idempotency_digest.as_ref().map(|value| &value[..]),
                &begin.priority,
                &begin.input_kind.as_str(),
                &begin.input_file_id,
                &begin.schema_version,
                &begin.encryption_policy_version,
                &begin.create_ts,
                &begin.deadline_ts,
                &begin.expires_ts,
            ],
        )?;
        tx.commit()?;
        Ok(GeminiBatchAdmissionBeginOutcome::Started {
            admission_id: begin.admission_id.clone(),
            job_id: begin.job_id.clone(),
            create_ts: begin.create_ts,
            deadline_ts: begin.deadline_ts,
            expires_ts: begin.expires_ts,
            next_item_index: 0,
        })
    }

    /// Append one bounded ciphertext page. This transaction never locks account or key money rows.
    pub fn gemini_batch_admission_append(
        &mut self,
        admission_id: &str,
        expected_start: i64,
        items: &[GeminiBatchAdmissionItem],
    ) -> Result<i64> {
        if admission_id.is_empty()
            || expected_start < 0
            || items.is_empty()
            || items.len() > GEMINI_BATCH_ADMISSION_PAGE_SIZE
        {
            bail!("invalid Gemini Batch admission page")
        }
        let mut tx = self.client.transaction()?;
        let row = tx
            .query_opt(
                "SELECT state,next_item_index,create_ts FROM gemini_batch_admissions WHERE admission_id=$1 FOR UPDATE",
                &[&admission_id],
            )?
            .context("Gemini Batch admission does not exist")?;
        if row.get::<_, String>(0) != "staging" || row.get::<_, i64>(1) != expected_start {
            bail!("Gemini Batch admission page is stale")
        }
        let create_ts: i64 = row.get(2);
        let item_count = i64::try_from(items.len()).context("Gemini Batch page length overflow")?;
        let next = expected_start
            .checked_add(item_count)
            .context("Gemini Batch item count overflow")?;
        if next > GEMINI_BATCH_MAX_ITEMS {
            bail!("Gemini Batch contains too many requests")
        }
        let mut page_hold = 0i64;
        let mut page_output = 0i64;
        let item_statement = tx.prepare(
            "INSERT INTO gemini_batch_admission_items(admission_id,item_index,request_id,logical_request_id,\
             execution_group_id,client_key,request_digest,input_file_id,hold_nano,requested_output_tokens,\
             payable_multiplier_bp,priced_ts,tariff_family,tariff_version,tariff_schedule_id,request_key_id,\
             request_nonce,request_ciphertext,request_plaintext_len,request_plaintext_digest,metadata_key_id,\
             metadata_nonce,metadata_ciphertext,metadata_plaintext_len,metadata_plaintext_digest,retention_ts,created_ts)\
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27)",
        )?;
        let file_statement = tx.prepare(
            "INSERT INTO gemini_batch_admission_item_files(admission_id,item_index,ordinal,file_id) VALUES($1,$2,$3,$4)",
        )?;
        for (offset, staged) in items.iter().enumerate() {
            staged.validate(create_ts)?;
            let expected = expected_start
                .checked_add(i64::try_from(offset).context("Gemini Batch page offset overflow")?)
                .context("Gemini Batch item index overflow")?;
            if staged.item.item_index != expected {
                bail!("Gemini Batch admission items are not contiguous")
            }
            page_hold = page_hold
                .checked_add(staged.item.hold_nano)
                .context("Gemini Batch aggregate hold overflow")?;
            page_output = page_output
                .checked_add(staged.requested_output_tokens)
                .context("Gemini Batch aggregate output overflow")?;
            let metadata = staged.item.metadata_blob.as_ref();
            tx.execute(
                &item_statement,
                &[
                    &admission_id,
                    &staged.item.item_index,
                    &staged.item.request_id,
                    &staged.item.logical_request_id,
                    &staged.item.execution_group_id,
                    &staged.item.client_key,
                    &&staged.item.request_digest[..],
                    &staged.item.input_file_id,
                    &staged.item.hold_nano,
                    &staged.requested_output_tokens,
                    &staged.item.payable_multiplier_bp,
                    &staged.item.priced_ts,
                    &staged.item.tariff_family,
                    &staged.item.tariff_version,
                    &staged.item.tariff_schedule_id,
                    &staged.item.request_blob.key_id,
                    &staged.item.request_blob.nonce,
                    &staged.item.request_blob.ciphertext,
                    &staged.item.request_blob.plaintext_len,
                    &&staged.item.request_blob.plaintext_digest[..],
                    &metadata.map(|blob| blob.key_id.as_str()),
                    &metadata.map(|blob| blob.nonce.as_slice()),
                    &metadata.map(|blob| blob.ciphertext.as_slice()),
                    &metadata.map(|blob| blob.plaintext_len),
                    &metadata.map(|blob| &blob.plaintext_digest[..]),
                    &staged.item.request_blob.retention_ts,
                    &create_ts,
                ],
            )?;
            for (ordinal, file_id) in staged.item.referenced_file_ids.iter().enumerate() {
                let ordinal =
                    i32::try_from(ordinal).context("Gemini Batch file ordinal overflow")?;
                tx.execute(
                    &file_statement,
                    &[&admission_id, &staged.item.item_index, &ordinal, file_id],
                )?;
            }
        }
        let changed = tx.execute(
            "UPDATE gemini_batch_admissions SET next_item_index=$2,aggregate_hold_nano=aggregate_hold_nano+$3,\
             aggregate_output_tokens=aggregate_output_tokens+$4,update_ts=GREATEST(update_ts,$5)\
             WHERE admission_id=$1 AND state='staging' AND next_item_index=$6",
            &[&admission_id, &next, &page_hold, &page_output, &super::now(), &expected_start],
        )?;
        if changed != 1 {
            bail!("Gemini Batch admission page lost its fence")
        }
        tx.commit()?;
        Ok(next)
    }

    /// Atomically validate and publish a complete staged admission, taking money only here.
    pub fn gemini_batch_admission_publish(
        &mut self,
        admission_id: &str,
        expected_items: i64,
        canonical_request_digest: [u8; 32],
        creator_key: &str,
    ) -> Result<GeminiBatchCreateOutcome> {
        if admission_id.is_empty()
            || creator_key.is_empty()
            || !(1..=GEMINI_BATCH_MAX_ITEMS).contains(&expected_items)
        {
            bail!("invalid Gemini Batch admission publish")
        }
        let mut tx = self.client.transaction()?;
        let admission = tx
            .query_opt(
                "SELECT job_id,account_id,creator_key_id,public_model,display_name,idempotency_digest,priority,\
                 input_kind,input_file_id,schema_version,encryption_policy_version,state,next_item_index,\
                 aggregate_hold_nano,aggregate_output_tokens,create_ts,deadline_ts,expires_ts,canonical_request_digest \
                 FROM gemini_batch_admissions WHERE admission_id=$1 FOR UPDATE",
                &[&admission_id],
            )?
            .context("Gemini Batch admission does not exist")?;
        let job_id: String = admission.get(0);
        let account_id: String = admission.get(1);
        let creator_key_id: String = admission.get(2);
        let state: String = admission.get(11);
        if state == "committed" {
            let row = tx.query_one(
                "SELECT canonical_request_digest FROM gemini_batch_jobs WHERE account_id=$1 AND job_id=$2",
                &[&account_id, &job_id],
            )?;
            if bytes32(row.get(0), "canonical request digest")? != canonical_request_digest {
                return Err(GeminiBatchIdempotencyConflict.into());
            }
            tx.commit()?;
            return Ok(GeminiBatchCreateOutcome::Replay { job_id });
        }
        if state != "staging" || admission.get::<_, i64>(12) != expected_items {
            bail!("Gemini Batch admission is incomplete")
        }
        let output_bound = admission
            .get::<_, i64>(14)
            .checked_mul(crate::MAX_BATCH_OUTPUT_BYTES_PER_TOKEN)
            .and_then(|value| {
                value.checked_add(
                    expected_items.saturating_mul(crate::MAX_BATCH_OUTPUT_ITEM_OVERHEAD_BYTES),
                )
            })
            .context("Gemini Batch aggregate output bound overflow")?;
        if admission.get::<_, String>(7) == "file" && output_bound > crate::MAX_BATCH_FILE_BYTES {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedLimit);
        }
        let totals = tx.query_one(
            "SELECT COUNT(*)::bigint,COALESCE(SUM(hold_nano),0)::bigint,\
             COALESCE(SUM(requested_output_tokens),0)::bigint,MIN(item_index),MAX(item_index)\
             FROM gemini_batch_admission_items WHERE admission_id=$1",
            &[&admission_id],
        )?;
        if totals.get::<_, i64>(0) != expected_items
            || totals.get::<_, i64>(1) != admission.get::<_, i64>(13)
            || totals.get::<_, i64>(2) != admission.get::<_, i64>(14)
            || totals.get::<_, Option<i64>>(3) != Some(0)
            || totals.get::<_, Option<i64>>(4) != Some(expected_items - 1)
        {
            bail!("Gemini Batch staged aggregate is invalid")
        }
        let idempotency_digest: Option<Vec<u8>> = admission.get(5);
        if let Some(ref digest) = idempotency_digest {
            tx.query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1,hashtextextended(encode($2::bytea,'hex'),741925)))",
                &[&account_id, &digest],
            )?;
            if let Some(row) = tx.query_opt(
                "SELECT job_id,canonical_request_digest FROM gemini_batch_jobs WHERE account_id=$1 AND idempotency_digest=$2",
                &[&account_id, &digest],
            )? {
                let replay_job: String = row.get(0);
                if bytes32(row.get(1), "canonical request digest")? != canonical_request_digest {
                    return Err(GeminiBatchIdempotencyConflict.into());
                }
                tx.execute(
                    "UPDATE gemini_batch_admissions SET state='committed',canonical_request_digest=$2,update_ts=$3 WHERE admission_id=$1",
                    &[&admission_id, &&canonical_request_digest[..], &super::now()],
                )?;
                tx.execute("DELETE FROM gemini_batch_admission_items WHERE admission_id=$1", &[&admission_id])?;
                tx.commit()?;
                return Ok(GeminiBatchCreateOutcome::Replay { job_id: replay_job });
            }
        }
        if tx
            .query_opt(
                "SELECT 1 FROM gemini_batch_jobs WHERE job_id=$1",
                &[&job_id],
            )?
            .is_some()
        {
            return Err(GeminiBatchIdempotencyConflict.into());
        }
        lock_account(&mut tx, &account_id)?;
        let key = tx.query_opt(
            "SELECT key_id,status,expires_ts,spend_limit_nano,spent_nano,reserved_nano FROM api_keys \
             WHERE key=$1 AND account_id=$2 FOR UPDATE",
            &[&creator_key, &account_id],
        )?;
        let Some(key) = key else {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedFunds);
        };
        if key.get::<_, String>(0) != creator_key_id {
            bail!("Gemini Batch creator key identity changed")
        }
        let now = super::now();
        let aggregate_hold: i64 = admission.get(13);
        let active_key = key.get::<_, String>(1) == "active"
            && key
                .get::<_, Option<i64>>(2)
                .is_none_or(|expires| expires > now);
        let within_key_limit = key.get::<_, Option<i64>>(3).is_none_or(|limit| {
            key.get::<_, i64>(4)
                .checked_add(key.get::<_, i64>(5))
                .and_then(|value| value.checked_add(aggregate_hold))
                .is_some_and(|value| value <= limit)
        });
        if !active_key || !within_key_limit {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedFunds);
        }
        let active_jobs: i64 = tx.query_one(
            "SELECT COUNT(*)::bigint FROM gemini_batch_jobs WHERE account_id=$1 AND completed_ts IS NULL AND delete_ts IS NULL",
            &[&account_id],
        )?.get(0);
        if active_jobs >= MAX_BATCH_NONTERMINAL_JOBS {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedLimit);
        }
        let file_check = tx.query_one(
            "WITH refs AS (
               SELECT input_file_id AS file_id FROM gemini_batch_admissions WHERE admission_id=$1 AND input_file_id IS NOT NULL
               UNION SELECT input_file_id FROM gemini_batch_admission_items WHERE admission_id=$1 AND input_file_id IS NOT NULL
               UNION SELECT file_id FROM gemini_batch_admission_item_files WHERE admission_id=$1),
             checked AS (SELECT r.file_id,f.size_bytes FROM refs r LEFT JOIN gemini_batch_files f ON f.file_id=r.file_id
               AND f.account_id=$2 AND f.state='active' AND f.expiration_ts>$3)
             SELECT COUNT(*)::bigint,COUNT(size_bytes)::bigint,COALESCE(SUM(size_bytes),0)::bigint FROM checked",
            &[&admission_id, &account_id, &now],
        )?;
        if file_check.get::<_, i64>(0) != file_check.get::<_, i64>(1)
            || file_check.get::<_, i64>(2) > MAX_BATCH_REFERENCED_FILE_BYTES
        {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedLimit);
        }
        let Some(account) = tx.query_opt(
            "UPDATE accounts SET balance_nano=balance_nano-$1,reserved_nano=reserved_nano+$1 \
             WHERE id=$2 AND status='active' AND balance_nano >= $1::bigint-$3::bigint RETURNING balance_nano",
            &[&aggregate_hold, &account_id, &ACCOUNT_OVERDRAFT_NANO],
        )? else {
            tx.rollback()?;
            return Ok(GeminiBatchCreateOutcome::RejectedFunds);
        };
        let balance_nano: i64 = account.get(0);
        if tx.execute(
            "UPDATE api_keys SET reserved_nano=reserved_nano+$1 WHERE key=$2 AND account_id=$3",
            &[&aggregate_hold, &creator_key, &account_id],
        )? != 1
        {
            bail!("Gemini Batch creator key disappeared while locked")
        }
        tx.execute(
            "INSERT INTO gemini_batch_jobs(job_id,account_id,creator_key_id,public_model,display_name,\
             canonical_request_digest,idempotency_digest,priority,input_kind,input_file_id,schema_version,\
             encryption_policy_version,create_ts,update_ts,deadline_ts,result_expiration_ts) \
             SELECT job_id,account_id,creator_key_id,public_model,display_name,$2,idempotency_digest,priority,\
             input_kind,input_file_id,schema_version,encryption_policy_version,create_ts,create_ts,deadline_ts,NULL \
             FROM gemini_batch_admissions WHERE admission_id=$1",
            &[&admission_id, &&canonical_request_digest[..]],
        )?;
        tx.execute(
            "INSERT INTO gemini_batch_items(job_id,item_index,request_id,logical_request_id,execution_group_id,\
             client_key,request_digest,input_file_id,hold_nano,payable_multiplier_bp,priced_ts,tariff_family,\
             tariff_version,tariff_schedule_id,state,creator_key_id,created_ts,updated_ts)\
             SELECT $2,item_index,request_id,logical_request_id,execution_group_id,client_key,request_digest,\
             input_file_id,hold_nano,payable_multiplier_bp,priced_ts,tariff_family,tariff_version,\
             tariff_schedule_id,'queued',$3,created_ts,created_ts FROM gemini_batch_admission_items \
             WHERE admission_id=$1 ORDER BY item_index",
            &[&admission_id, &job_id, &creator_key_id],
        )?;
        tx.execute(
            "INSERT INTO gemini_batch_blobs(job_id,item_index,kind,key_id,nonce,ciphertext,plaintext_len,\
             plaintext_digest,retention_ts,created_ts) \
             SELECT $2,item_index,'request',request_key_id,request_nonce,request_ciphertext,request_plaintext_len,\
             request_plaintext_digest,retention_ts,created_ts FROM gemini_batch_admission_items WHERE admission_id=$1 \
             UNION ALL SELECT $2,item_index,'metadata',metadata_key_id,metadata_nonce,metadata_ciphertext,\
             metadata_plaintext_len,metadata_plaintext_digest,retention_ts,created_ts \
             FROM gemini_batch_admission_items WHERE admission_id=$1 AND metadata_key_id IS NOT NULL",
            &[&admission_id, &job_id],
        )?;
        tx.execute(
            "INSERT INTO gemini_batch_item_files(job_id,item_index,ordinal,file_id) \
             SELECT $2,item_index,ordinal,file_id FROM gemini_batch_admission_item_files WHERE admission_id=$1",
            &[&admission_id, &job_id],
        )?;
        tx.execute(
            "UPDATE gemini_batch_admissions SET state='committed',canonical_request_digest=$2,update_ts=$3 WHERE admission_id=$1",
            &[&admission_id, &&canonical_request_digest[..], &now],
        )?;
        tx.execute(
            "DELETE FROM gemini_batch_admission_items WHERE admission_id=$1",
            &[&admission_id],
        )?;
        tx.commit()?;
        Ok(GeminiBatchCreateOutcome::Created { balance_nano })
    }

    pub fn gemini_batch_admission_abort(&mut self, admission_id: &str) -> Result<bool> {
        Ok(self.client.execute(
            "UPDATE gemini_batch_admissions SET state='aborted',update_ts=$2 WHERE admission_id=$1 AND state='staging'",
            &[&admission_id, &super::now()],
        )? == 1)
    }

    /// Delete a bounded set of abandoned ciphertext-only admissions. Committed rows are retained as
    /// the idempotency replay marker; live jobs and money are never touched by this maintenance path.
    pub fn prune_expired_gemini_batch_admissions(
        &mut self,
        older_than: i64,
        limit: usize,
    ) -> Result<usize> {
        let limit = i64::try_from(limit.clamp(1, 5_000))
            .context("Gemini Batch admission prune limit overflow")?;
        Ok(self.client.execute(
            "DELETE FROM gemini_batch_admissions WHERE admission_id IN (\
             SELECT admission_id FROM gemini_batch_admissions WHERE expires_ts<$1 \
             AND state IN ('staging','sealed','aborted') ORDER BY expires_ts,admission_id LIMIT $2)",
            &[&older_than, &limit],
        )? as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_and_item_limits_are_exact() {
        assert_eq!(GEMINI_BATCH_ADMISSION_PAGE_SIZE, 128);
        assert_eq!(GEMINI_BATCH_MAX_ITEMS, 100_000);
    }

    #[test]
    fn staged_publish_exactly_100k_postgres() {
        let Ok(url) = std::env::var("CLAUDE_API_TEST_DATABASE_URL") else {
            eprintln!("skipping 100k staged publish matrix");
            return;
        };
        let mut lock = PgStore::connect(&url).unwrap();
        // The complete workspace test binary runs destructive real-PG matrices in parallel. This
        // proof may legitimately wait behind another holder; a connection-level inherited
        // lock_timeout would otherwise turn correct serialization into a flaky RED candidate.
        lock.client.batch_execute("SET lock_timeout=0").unwrap();
        lock.client
            .query_one(
                "SELECT pg_advisory_lock($1)",
                &[&super::super::POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
        let mut pg = PgStore::connect(&url).unwrap();
        pg.migrate().unwrap();
        let id = "stage100k-admission";
        let job = "stage100k-job";
        let account = "stage100k-account";
        pg.client.batch_execute("DELETE FROM gemini_batch_admissions WHERE admission_id='stage100k-admission';DELETE FROM gemini_batch_blobs WHERE job_id='stage100k-job';DELETE FROM gemini_batch_items WHERE job_id='stage100k-job';DELETE FROM gemini_batch_jobs WHERE job_id='stage100k-job';DELETE FROM api_keys WHERE key='stage100k-key';DELETE FROM accounts WHERE id='stage100k-account';").unwrap();
        pg.client.execute("INSERT INTO accounts(id,balance_nano,spent_nano,reserved_nano,mult_bp,status,created_ts,created) VALUES($1,20000000,0,0,5000,'active',1,'x')", &[&account]).unwrap();
        pg.client.execute("INSERT INTO api_keys(key,key_id,account_id,spent_nano,reserved_nano,status,created_ts,created) VALUES('stage100k-key','stage100k-key-id',$1,0,0,'active',1,'x')", &[&account]).unwrap();
        let begin = GeminiBatchAdmissionBegin {
            admission_id: id.into(),
            job_id: job.into(),
            account_id: account.into(),
            creator_key_id: "stage100k-key-id".into(),
            public_model: "gemini-2.5-flash".into(),
            display_name: "100k".into(),
            idempotency_digest: None,
            priority: 0,
            input_kind: crate::GeminiBatchInputKind::Inline,
            input_file_id: None,
            schema_version: 1,
            encryption_policy_version: 1,
            create_ts: 10,
            deadline_ts: 100000,
            expires_ts: 100000,
        };
        pg.gemini_batch_admission_begin(&begin).unwrap();
        // Generate the full staged cardinality in one provider-free PostgreSQL statement so the
        // exact lifecycle proof stays below the trusted per-test watchdog. Page-size and parser
        // boundedness are covered independently; this test targets atomic set-based publication.
        pg.client.execute(
            "INSERT INTO gemini_batch_admission_items(admission_id,item_index,request_id,logical_request_id,execution_group_id,request_digest,hold_nano,requested_output_tokens,payable_multiplier_bp,priced_ts,tariff_family,tariff_version,tariff_schedule_id,request_key_id,request_nonce,request_ciphertext,request_plaintext_len,request_plaintext_digest,retention_ts,created_ts) SELECT $1,i,'stage100k-r-'||i,'l-stage100k-r-'||i,'g-stage100k-r-'||i,decode(repeat('04',32),'hex'),100,1,5000,10,'google/gemini/gemini-2.5-flash',1,'v1','kid',decode(repeat('01',24),'hex'),decode(repeat('02',18),'hex'),2,decode(repeat('03',32),'hex'),100000,10 FROM generate_series(0,99999) AS i",
            &[&id],
        ).unwrap();
        pg.client.execute("UPDATE gemini_batch_admissions SET next_item_index=100000,aggregate_hold_nano=10000000,aggregate_output_tokens=100000 WHERE admission_id=$1", &[&id]).unwrap();
        assert!(matches!(
            pg.gemini_batch_admission_publish(id, 100000, [9; 32], "stage100k-key")
                .unwrap(),
            GeminiBatchCreateOutcome::Created { .. }
        ));
        let row=pg.client.query_one("SELECT (SELECT COUNT(*) FROM gemini_batch_items WHERE job_id=$1),(SELECT COUNT(*) FROM gemini_batch_blobs WHERE job_id=$1),(SELECT reserved_nano FROM accounts WHERE id=$2),(SELECT COUNT(*) FROM gemini_batch_admission_items WHERE admission_id=$3)",&[&job,&account,&id]).unwrap();
        assert_eq!(row.get::<_, i64>(0), 100000);
        assert_eq!(row.get::<_, i64>(1), 100000);
        assert_eq!(row.get::<_, i64>(2), 10000000);
        assert_eq!(row.get::<_, i64>(3), 0);
        pg.client.batch_execute("DELETE FROM gemini_batch_blobs WHERE job_id='stage100k-job';DELETE FROM gemini_batch_items WHERE job_id='stage100k-job';DELETE FROM gemini_batch_jobs WHERE job_id='stage100k-job';DELETE FROM gemini_batch_admissions WHERE admission_id='stage100k-admission';DELETE FROM api_keys WHERE key='stage100k-key';DELETE FROM accounts WHERE id='stage100k-account';").unwrap();
        lock.client
            .query_one(
                "SELECT pg_advisory_unlock($1)",
                &[&super::super::POSTGRES_DESTRUCTIVE_TEST_LOCK],
            )
            .unwrap();
    }
}
