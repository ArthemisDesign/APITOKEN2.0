//! Schema-60 Gemini Batch file-output construction and lifecycle maintenance.

use super::{now, Owner, PgStore};
use crate::{
    GeminiBatchEncryptedBlob, GeminiBatchFileChunk, GeminiBatchFileCompletion,
    GeminiBatchItemState, GeminiBatchMaintenanceReport, GeminiBatchOutputClaim,
    GeminiBatchOutputItem, GeminiBatchOutputItemPage, GeminiBatchTerminalClass,
    BATCH_QUEUED_EXPIRY_SECS, BATCH_RESULT_RETENTION_SECS, MAX_BATCH_OUTPUT_PAGE_SIZE,
    MAX_BATCH_PRUNE_LIMIT,
};
use anyhow::{bail, Context, Result};
use postgres::{Row, Transaction};
use sha2::{Digest, Sha256};

const FILE_CHUNK_MANIFEST_DOMAIN: &[u8] = b"apitoken:gemini-batch-file-chunks:v1\0";

fn bytes32(value: Vec<u8>, field: &str) -> Result<[u8; 32]> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid {field} length"))
}

fn blob_from_row(row: &Row, offset: usize) -> Result<Option<GeminiBatchEncryptedBlob>> {
    let Some(kind) = row.get::<_, Option<String>>(offset) else {
        return Ok(None);
    };
    Ok(Some(GeminiBatchEncryptedBlob {
        kind,
        key_id: row.get(offset + 1),
        nonce: row.get(offset + 2),
        ciphertext: row.get(offset + 3),
        plaintext_len: row.get(offset + 4),
        plaintext_digest: bytes32(row.get(offset + 5), "output blob digest")?,
        retention_ts: row.get(offset + 6),
    }))
}

fn assert_output_fence(
    tx: &mut Transaction<'_>,
    owner: &Owner,
    claim: &GeminiBatchOutputClaim,
    ts: i64,
) -> Result<bool> {
    PgStore::assert_owner_locked(tx, owner, ts)?;
    Ok(tx
        .query_opt(
            "SELECT 1 FROM gemini_batch_output_builds b JOIN gemini_batch_jobs j USING(job_id) \
         WHERE b.job_id=$1 AND b.file_id=$2 AND b.generation=$3 AND b.owner_instance=$4 \
         AND b.owner_epoch=$5 AND b.state='building' AND b.lease_until >= $6 \
         AND j.account_id=$7 AND j.completed_ts IS NULL AND j.terminal_items_ts IS NOT NULL \
         AND j.output_state='building' FOR UPDATE OF b,j",
            &[
                &claim.job_id,
                &claim.file_id,
                &claim.generation,
                &owner.instance_id,
                &owner.epoch,
                &ts,
                &claim.account_id,
            ],
        )?
        .is_some())
}

impl PgStore {
    pub fn claim_gemini_batch_output(
        &mut self,
        owner: &Owner,
        lease_secs: i64,
    ) -> Result<Option<GeminiBatchOutputClaim>> {
        let ts = now();
        let lease_until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        Self::assert_owner_locked(&mut tx, owner, ts)?;
        let Some(row) = tx.query_opt(
            "SELECT b.job_id,j.account_id,b.file_id,b.generation,b.next_item_index,b.next_chunk_index,\
                    b.plaintext_bytes,COALESCE(j.result_expiration_ts,j.terminal_items_ts+$1) \
             FROM gemini_batch_output_builds b JOIN gemini_batch_jobs j USING(job_id) \
             WHERE b.state IN ('pending','failed','building') AND (b.lease_until IS NULL OR b.lease_until<$2) \
               AND j.input_kind='file' AND j.terminal_items_ts IS NOT NULL AND j.completed_ts IS NULL \
               AND j.delete_ts IS NULL AND j.output_state IN ('pending','failed','building') \
             ORDER BY b.updated_ts,b.job_id FOR UPDATE OF b,j SKIP LOCKED LIMIT 1",
            &[&BATCH_RESULT_RETENTION_SECS,&ts],
        )? else { tx.rollback()?; return Ok(None) };
        let job_id: String = row.get(0);
        let account_id: String = row.get(1);
        let file_id: String = row.get(2);
        // Rebuild abandoned generations only from durable terminal blobs. No provider execution or
        // money mutation participates in output recovery.
        tx.execute(
            "DELETE FROM gemini_batch_file_chunks WHERE file_id=$1",
            &[&file_id],
        )?;
        tx.execute("UPDATE gemini_batch_files SET size_bytes=0,received_bytes=0,next_chunk_index=0,chunk_count=0,chunk_manifest_digest=NULL,completed_ts=NULL,state='processing',update_ts=$2 WHERE file_id=$1 AND source_kind='batch_output'", &[&file_id,&ts])?;
        tx.execute("UPDATE gemini_batch_output_builds SET next_item_index=0,next_chunk_index=0,plaintext_bytes=0 WHERE job_id=$1", &[&job_id])?;
        let generation: i64 = row
            .get::<_, i64>(3)
            .checked_add(1)
            .context("output generation overflow")?;
        if tx.execute(
            "UPDATE gemini_batch_output_builds SET generation=$2,state='building',owner_instance=$3,\
             owner_epoch=$4,lease_until=$5,updated_ts=$6 WHERE job_id=$1",
            &[&job_id,&generation,&owner.instance_id,&owner.epoch,&lease_until,&ts],
        )? != 1 { bail!("Gemini Batch output claim lost its row") }
        tx.execute("UPDATE gemini_batch_jobs SET output_state='building',update_ts=GREATEST(update_ts,$2) WHERE job_id=$1", &[&job_id,&ts])?;
        let claim = GeminiBatchOutputClaim {
            job_id,
            account_id,
            file_id,
            generation,
            next_item_index: 0,
            next_chunk_index: 0,
            plaintext_bytes: 0,
            result_expiration_ts: row.get(7),
        };
        tx.commit()?;
        Ok(Some(claim))
    }

    pub fn renew_gemini_batch_output(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchOutputClaim,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        if !assert_output_fence(&mut tx, owner, claim, ts)? {
            tx.rollback()?;
            return Ok(false);
        }
        let changed=tx.execute("UPDATE gemini_batch_output_builds SET lease_until=$6,updated_ts=$7 WHERE job_id=$1 AND file_id=$2 AND generation=$3 AND owner_instance=$4 AND owner_epoch=$5 AND state='building'", &[&claim.job_id,&claim.file_id,&claim.generation,&owner.instance_id,&owner.epoch,&until,&ts])?;
        tx.commit()?;
        Ok(changed == 1)
    }

    pub fn gemini_batch_output_item_page(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchOutputClaim,
        after: Option<i64>,
        limit: i64,
    ) -> Result<GeminiBatchOutputItemPage> {
        if after.is_some_and(|v| v < 0) {
            bail!("invalid output item cursor")
        }
        let ts = now();
        let mut tx = self.client.transaction()?;
        if !assert_output_fence(&mut tx, owner, claim, ts)? {
            bail!("Gemini Batch output fence is stale")
        }
        let page = limit.clamp(1, MAX_BATCH_OUTPUT_PAGE_SIZE);
        let rows=tx.query("SELECT i.item_index,i.client_key,i.state,i.terminal_class,b.kind,b.key_id,b.nonce,b.ciphertext,b.plaintext_len,b.plaintext_digest,b.retention_ts FROM gemini_batch_items i LEFT JOIN gemini_batch_blobs b ON b.job_id=i.job_id AND b.item_index=i.item_index AND b.kind=CASE WHEN i.state='succeeded' THEN 'result' ELSE 'error' END WHERE i.job_id=$1 AND ($2::bigint IS NULL OR i.item_index>$2) ORDER BY i.item_index LIMIT $3", &[&claim.job_id,&after,&(page+1)])?;
        let more = rows.len() as i64 > page;
        let items = rows
            .iter()
            .take(page as usize)
            .map(|row| {
                Ok(GeminiBatchOutputItem {
                    item_index: row.get(0),
                    client_key: row.get(1),
                    state: GeminiBatchItemState::parse(row.get::<_, String>(2).as_str())?,
                    terminal_class: GeminiBatchTerminalClass::parse(
                        row.get::<_, String>(3).as_str(),
                    )?,
                    blob: blob_from_row(row, 4)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if items.iter().any(|item| !item.state.is_terminal()) {
            bail!("output page contains nonterminal item")
        }
        let next_item_index = more.then(|| items.last().expect("nonempty page").item_index);
        tx.commit()?;
        Ok(GeminiBatchOutputItemPage {
            items,
            next_item_index,
        })
    }

    pub fn append_gemini_batch_output_chunk(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchOutputClaim,
        next_item_index: i64,
        chunk: &GeminiBatchFileChunk,
    ) -> Result<bool> {
        chunk.validate()?;
        let ts = now();
        let mut tx = self.client.transaction()?;
        if !assert_output_fence(&mut tx, owner, claim, ts)? {
            tx.rollback()?;
            return Ok(false);
        }
        let build=tx.query_one("SELECT next_item_index,next_chunk_index,plaintext_bytes FROM gemini_batch_output_builds WHERE job_id=$1 FOR UPDATE", &[&claim.job_id])?;
        let expected_item: i64 = build.get(0);
        let expected_chunk: i64 = build.get(1);
        let bytes: i64 = build.get(2);
        if chunk.chunk_index < expected_chunk {
            let exact=tx.query_opt("SELECT 1 FROM gemini_batch_file_chunks WHERE file_id=$1 AND chunk_index=$2 AND key_id=$3 AND nonce=$4 AND ciphertext=$5 AND plaintext_len=$6 AND plaintext_digest=$7", &[&claim.file_id,&chunk.chunk_index,&chunk.key_id,&chunk.nonce,&chunk.ciphertext,&chunk.plaintext_len,&&chunk.plaintext_digest[..]])?.is_some();
            tx.commit()?;
            return Ok(exact && next_item_index <= expected_item);
        }
        if chunk.chunk_index != expected_chunk || next_item_index < expected_item {
            bail!("non-contiguous Gemini Batch output checkpoint")
        }
        let new_bytes = bytes
            .checked_add(chunk.plaintext_len)
            .context("output size overflow")?;
        if new_bytes > crate::MAX_BATCH_FILE_BYTES {
            bail!("Gemini Batch output exceeds the 2 GiB file limit")
        }
        // Serialize storage accounting on the owning account row; PostgreSQL forbids FOR UPDATE
        // on aggregate queries, and the row lock composes with upload/file creation quota.
        tx.query_one(
            "SELECT 1 FROM accounts WHERE id=$1 FOR UPDATE",
            &[&claim.account_id],
        )?;
        let stored: i64 = tx.query_one(
            "SELECT COALESCE(SUM(size_bytes),0)::bigint FROM gemini_batch_files WHERE account_id=$1 AND expiration_ts>$2 AND file_id<>$3",
            &[&claim.account_id,&ts,&claim.file_id],
        )?.get(0);
        if stored
            .checked_add(new_bytes)
            .is_none_or(|total| total > crate::MAX_BATCH_ACCOUNT_FILE_BYTES)
        {
            bail!("Gemini Batch account output storage quota exceeded")
        }
        tx.execute("INSERT INTO gemini_batch_file_chunks(file_id,chunk_index,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,created_ts) VALUES($1,$2,$3,$4,$5,$6,$7,$8)", &[&claim.file_id,&chunk.chunk_index,&chunk.key_id,&chunk.nonce,&chunk.ciphertext,&chunk.plaintext_len,&&chunk.plaintext_digest[..],&chunk.created_ts])?;
        tx.execute("UPDATE gemini_batch_files SET size_bytes=$2,received_bytes=$2,next_chunk_index=$3,chunk_count=$3,update_ts=GREATEST(update_ts,$4) WHERE file_id=$1 AND state='processing' AND source_kind='batch_output'", &[&claim.file_id,&new_bytes,&(expected_chunk+1),&ts])?;
        tx.execute("UPDATE gemini_batch_output_builds SET next_item_index=$2,next_chunk_index=$3,plaintext_bytes=$4,updated_ts=$5 WHERE job_id=$1", &[&claim.job_id,&next_item_index,&(expected_chunk+1),&new_bytes,&ts])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn fail_gemini_batch_output(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchOutputClaim,
        class: &str,
    ) -> Result<bool> {
        if class.is_empty() || class.len() > 128 {
            bail!("invalid output failure class")
        }
        let ts = now();
        let mut tx = self.client.transaction()?;
        if !assert_output_fence(&mut tx, owner, claim, ts)? {
            tx.rollback()?;
            return Ok(false);
        }
        tx.execute("UPDATE gemini_batch_output_builds SET state='failed',owner_instance=NULL,owner_epoch=NULL,lease_until=NULL,last_error_class=$2,updated_ts=$3 WHERE job_id=$1", &[&claim.job_id,&class,&ts])?;
        tx.execute("UPDATE gemini_batch_jobs SET output_state='failed',update_ts=GREATEST(update_ts,$2) WHERE job_id=$1", &[&claim.job_id,&ts])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn finalize_gemini_batch_output(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchOutputClaim,
        completion: &GeminiBatchFileCompletion,
    ) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        if !assert_output_fence(&mut tx, owner, claim, ts)? {
            tx.rollback()?;
            return Ok(false);
        }
        let build=tx.query_one("SELECT next_item_index,next_chunk_index,plaintext_bytes FROM gemini_batch_output_builds WHERE job_id=$1 FOR UPDATE", &[&claim.job_id])?;
        let item_count: i64 = tx
            .query_one(
                "SELECT COUNT(*)::bigint FROM gemini_batch_items WHERE job_id=$1",
                &[&claim.job_id],
            )?
            .get(0);
        if build.get::<_, i64>(0) != item_count {
            bail!("Gemini Batch output does not cover all items")
        }
        let chunks=tx.query("SELECT chunk_index,plaintext_len,plaintext_digest FROM gemini_batch_file_chunks WHERE file_id=$1 ORDER BY chunk_index", &[&claim.file_id])?;
        if chunks.len() as i64 != build.get::<_, i64>(1) {
            bail!("Gemini Batch output chunk count mismatch")
        }
        let mut manifest = Sha256::new();
        manifest.update(FILE_CHUNK_MANIFEST_DOMAIN);
        manifest.update((chunks.len() as u64).to_be_bytes());
        let mut total = 0i64;
        for (expected, row) in chunks.iter().enumerate() {
            let idx: i64 = row.get(0);
            if idx != expected as i64 {
                bail!("output chunks are non-contiguous")
            };
            let len: i64 = row.get(1);
            let digest: [u8; 32] = bytes32(row.get(2), "output chunk digest")?;
            manifest.update(idx.to_be_bytes());
            manifest.update(len.to_be_bytes());
            manifest.update(digest);
            total = total.checked_add(len).context("output size overflow")?;
        }
        if total != build.get::<_, i64>(2)
            || <[u8; 32]>::from(manifest.finalize()) != completion.chunk_manifest_digest
        {
            bail!("output manifest mismatch")
        }
        let completed = completion.completed_ts.max(ts);
        let expiry = completed.saturating_add(BATCH_RESULT_RETENTION_SECS);
        tx.execute("UPDATE gemini_batch_files SET state='active',sha256_digest=$2,chunk_manifest_digest=$3,completed_ts=$4,expiration_ts=GREATEST(expiration_ts,$5),update_ts=$4 WHERE file_id=$1 AND source_kind='batch_output' AND state='processing'", &[&claim.file_id,&&completion.whole_file_sha256_digest[..],&&completion.chunk_manifest_digest[..],&completed,&expiry])?;
        tx.execute("UPDATE gemini_batch_jobs SET output_file_id=$2,output_state='ready',completed_ts=$3,result_expiration_ts=$4,update_ts=$3 WHERE job_id=$1 AND completed_ts IS NULL AND output_file_id IS NULL", &[&claim.job_id,&claim.file_id,&completed,&expiry])?;
        tx.execute("UPDATE gemini_batch_output_builds SET state='ready',owner_instance=NULL,owner_epoch=NULL,lease_until=NULL,updated_ts=$2,last_error_class=NULL WHERE job_id=$1", &[&claim.job_id,&completed])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn expire_queued_gemini_batch(&mut self, limit: usize) -> Result<usize> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let limit = limit.clamp(1, MAX_BATCH_PRUNE_LIMIT) as i64;
        let jobs=tx.query("SELECT j.job_id,j.account_id,j.creator_key_id FROM gemini_batch_jobs j WHERE j.completed_ts IS NULL AND j.create_ts+$1<=$2 AND EXISTS(SELECT 1 FROM gemini_batch_items i WHERE i.job_id=j.job_id AND i.state='queued') ORDER BY j.create_ts,j.job_id FOR UPDATE OF j SKIP LOCKED LIMIT $3", &[&BATCH_QUEUED_EXPIRY_SECS,&ts,&limit])?;
        let mut count = 0usize;
        for job in jobs {
            let job_id: String = job.get(0);
            let account: String = job.get(1);
            let key: String = job.get(2);
            tx.query_one("SELECT 1 FROM accounts WHERE id=$1 FOR UPDATE", &[&account])?;
            let rows=tx.query("UPDATE gemini_batch_items SET state='canceled',terminal_class='expired',terminal_ts=$2,updated_ts=$2 WHERE job_id=$1 AND state='queued' AND dispatch_intent_ts IS NULL RETURNING hold_nano", &[&job_id,&ts])?;
            let hold = rows.iter().try_fold(0i64, |sum, row| {
                sum.checked_add(row.get::<_, i64>(0))
                    .context("expiry hold overflow")
            })?;
            if hold > 0 {
                if tx.execute("UPDATE accounts SET balance_nano=(balance_nano::numeric+$1::bigint::numeric)::bigint,reserved_nano=reserved_nano-$1 WHERE id=$2 AND reserved_nano>=$1", &[&hold,&account])?!=1{bail!("expiry account hold mismatch")};
                let changed=tx.execute("UPDATE api_keys SET reserved_nano=reserved_nano-$1 WHERE account_id=$2 AND key_id=$3 AND reserved_nano>=$1", &[&hold,&account,&key])?;
                if changed == 0
                    && tx
                        .query_opt(
                            "SELECT 1 FROM api_keys WHERE account_id=$1 AND key_id=$2",
                            &[&account, &key],
                        )?
                        .is_some()
                {
                    bail!("expiry key hold mismatch")
                }
            }
            tx.execute("UPDATE gemini_batch_jobs SET cancel_requested_ts=COALESCE(cancel_requested_ts,$2),update_ts=$2 WHERE job_id=$1", &[&job_id,&ts])?;
            super::gemini_batch_settlement::complete_job_if_terminal(&mut tx, &job_id, ts)?;
            count += rows.len();
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn maintain_gemini_batch(
        &mut self,
        older_than: i64,
        limit: usize,
    ) -> Result<GeminiBatchMaintenanceReport> {
        let bounded = limit.clamp(1, MAX_BATCH_PRUNE_LIMIT) as i64;
        let expired_queued_items = self.expire_queued_gemini_batch(limit)?;
        let expired_processing_files=self.client.execute("DELETE FROM gemini_batch_files WHERE file_id IN (SELECT file_id FROM gemini_batch_files WHERE state='processing' AND source_kind='client_upload' AND expiration_ts<=$1 AND NOT EXISTS(SELECT 1 FROM gemini_batch_file_chunks c WHERE c.file_id=gemini_batch_files.file_id) ORDER BY expiration_ts,file_id LIMIT $2)", &[&older_than,&bounded])? as usize;
        self.client.execute("DELETE FROM gemini_batch_admissions WHERE admission_id IN (SELECT admission_id FROM gemini_batch_admissions WHERE state IN ('staging','sealed','aborted') AND expires_ts<=$1 ORDER BY expires_ts,admission_id LIMIT $2)", &[&older_than,&bounded])?;
        let pruned = self.prune_gemini_batch(older_than, limit)?;
        Ok(GeminiBatchMaintenanceReport {
            expired_queued_items,
            expired_processing_files,
            pruned,
        })
    }
}
