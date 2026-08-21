//! Gemini Batch cancellation, durable settlement, and bounded pruning.

use super::{now, Owner, PgStore};
use crate::gemini_batch::{
    GeminiBatchCancelResult, GeminiBatchPruneReport, GeminiBatchSettlementIntent, GeminiBatchUsage,
    BATCH_RESULT_RETENTION_SECS, MAX_BATCH_PRUNE_LIMIT,
};
use crate::{ProviderTurnCalibrationEvent, PROVIDER_GOOGLE};
use anyhow::{bail, Context, Result};
use postgres::{Row, Transaction};

const SETTLEMENT_REPLAY_SELECT: &str = "SELECT job_id,item_index,disposition,actual_nano,charge_basis_nano,real_nano,usage_input_tokens,usage_tool_prompt_tokens,usage_audio_input_tokens,usage_cached_input_tokens,usage_cached_audio_input_tokens,usage_output_tokens,usage_thinking_output_tokens,usage_image_output_tokens,usage_search_queries,usage_grounded_search_prompts,result_kind,terminal_state,created_ts,calibration_profile_id,calibration_model_id,calibration_service_tier,calibration_inference_geo,calibration_tariff_schedule_id,calibration_priced_ts,calibration_completed_at,calibration_input_tokens,calibration_audio_input_tokens,calibration_cache_read_tokens,calibration_cached_audio_input_tokens,calibration_cache_write_5m_tokens,calibration_cache_write_1h_tokens,calibration_output_tokens,calibration_thinking_output_tokens,calibration_image_output_tokens,calibration_tool_prompt_tokens,calibration_search_queries,calibration_grounded_search_prompts,calibration_api_input_nanousd,calibration_api_audio_input_nanousd,calibration_api_cache_read_nanousd,calibration_api_cached_audio_input_nanousd,calibration_api_cache_write_5m_nanousd,calibration_api_cache_write_1h_nanousd,calibration_api_output_nanousd,calibration_api_image_output_nanousd,calibration_api_search_nanousd,calibration_api_total_nanousd FROM gemini_batch_settlement_outbox WHERE request_id=$1";

fn usage_from_row(row: &Row, offset: usize) -> Option<GeminiBatchUsage> {
    row.get::<_, Option<i64>>(offset)
        .map(|input_tokens| GeminiBatchUsage {
            input_tokens,
            tool_prompt_tokens: row.get(offset + 1),
            audio_input_tokens: row.get(offset + 2),
            cached_input_tokens: row.get(offset + 3),
            cached_audio_input_tokens: row.get(offset + 4),
            output_tokens: row.get(offset + 5),
            thinking_output_tokens: row.get(offset + 6),
            image_output_tokens: row.get(offset + 7),
            search_queries: row.get(offset + 8),
            grounded_search_prompts: row.get(offset + 9),
        })
}

fn calibration_from_row(
    row: &Row,
    offset: usize,
    request_id: &str,
) -> Option<ProviderTurnCalibrationEvent> {
    row.get::<_, Option<String>>(offset)
        .map(|subject_id| ProviderTurnCalibrationEvent {
            provider: PROVIDER_GOOGLE.to_owned(),
            request_id: request_id.to_owned(),
            subject_id,
            model_id: row.get(offset + 1),
            service_tier: row.get(offset + 2),
            inference_geo: row.get(offset + 3),
            tariff_schedule_id: row.get(offset + 4),
            priced_ts: row.get(offset + 5),
            completed_at: row.get(offset + 6),
            input_tokens: row.get(offset + 7),
            audio_input_tokens: row.get(offset + 8),
            cache_read_tokens: row.get(offset + 9),
            cached_audio_input_tokens: row.get(offset + 10),
            cache_write_5m_tokens: row.get(offset + 11),
            cache_write_1h_tokens: row.get(offset + 12),
            output_tokens: row.get(offset + 13),
            thinking_output_tokens: row.get(offset + 14),
            image_output_tokens: row.get(offset + 15),
            tool_prompt_tokens: row.get(offset + 16),
            search_queries: row.get(offset + 17),
            grounded_search_prompts: row.get(offset + 18),
            api_input_nanousd: row.get(offset + 19),
            api_audio_input_nanousd: row.get(offset + 20),
            api_cache_read_nanousd: row.get(offset + 21),
            api_cached_audio_input_nanousd: row.get(offset + 22),
            api_cache_write_5m_nanousd: row.get(offset + 23),
            api_cache_write_1h_nanousd: row.get(offset + 24),
            api_output_nanousd: row.get(offset + 25),
            api_image_output_nanousd: row.get(offset + 26),
            api_search_nanousd: row.get(offset + 27),
            api_total_nanousd: row.get(offset + 28),
        })
}

fn blob_matches(tx: &mut Transaction<'_>, intent: &GeminiBatchSettlementIntent) -> Result<bool> {
    let Some(row) = tx.query_opt(
        "SELECT key_id,nonce,ciphertext,plaintext_len,plaintext_digest,retention_ts,created_ts FROM gemini_batch_blobs WHERE job_id=$1 AND item_index=$2 AND kind=$3",
        &[&intent.job_id, &intent.item_index, &intent.result_blob.kind],
    )? else {
        return Ok(false);
    };
    Ok(row.get::<_, String>(0) == intent.result_blob.key_id
        && row.get::<_, Vec<u8>>(1) == intent.result_blob.nonce
        && row.get::<_, Vec<u8>>(2) == intent.result_blob.ciphertext
        && row.get::<_, i64>(3) == intent.result_blob.plaintext_len
        && row.get::<_, Vec<u8>>(4).as_slice() == intent.result_blob.plaintext_digest
        && row.get::<_, i64>(5) == intent.result_blob.retention_ts
        && row.get::<_, i64>(6) == intent.completed_ts)
}

pub(super) fn complete_job_if_terminal(
    tx: &mut Transaction<'_>,
    job_id: &str,
    ts: i64,
) -> Result<()> {
    let Some(job) = tx.query_opt(
        "SELECT account_id,input_kind,terminal_items_ts,completed_ts FROM gemini_batch_jobs j \
         WHERE job_id=$1 AND NOT EXISTS(SELECT 1 FROM gemini_batch_items i WHERE i.job_id=j.job_id \
         AND i.state NOT IN ('succeeded','failed','indeterminate','canceled')) FOR UPDATE",
        &[&job_id],
    )?
    else {
        return Ok(());
    };
    let input_kind: String = job.get(1);
    if input_kind == "inline" {
        tx.execute(
            "UPDATE gemini_batch_jobs SET terminal_items_ts=COALESCE(terminal_items_ts,$2),\
             completed_ts=COALESCE(completed_ts,$2),result_expiration_ts=COALESCE(result_expiration_ts,$2::bigint+$3::bigint),\
             update_ts=GREATEST(update_ts,$2) WHERE job_id=$1",
            &[&job_id, &ts, &BATCH_RESULT_RETENTION_SECS],
        )?;
        return Ok(());
    }
    if job.get::<_, Option<i64>>(2).is_some() || job.get::<_, Option<i64>>(3).is_some() {
        return Ok(());
    }
    let account_id: String = job.get(0);
    let file_id = format!("batch-output-{job_id}");
    tx.execute(
        "INSERT INTO gemini_batch_files(file_id,account_id,display_name,mime_type,size_bytes,sha256_digest,\
         source_kind,state,storage_kind,create_ts,update_ts,expiration_ts,received_bytes,next_chunk_index,chunk_count)\
         VALUES($1,$2,'Gemini Batch output','application/jsonl',0,decode(repeat('00',32),'hex'),\
         'batch_output','processing','chunked',$3,$3,$3::bigint+$4::bigint,0,0,0) ON CONFLICT(file_id) DO NOTHING",
        &[&file_id, &account_id, &ts, &BATCH_RESULT_RETENTION_SECS],
    )?;
    tx.execute(
        "INSERT INTO gemini_batch_output_builds(job_id,file_id,generation,state,next_item_index,next_chunk_index,\
         plaintext_bytes,created_ts,updated_ts) VALUES($1,$2,1,'pending',0,0,0,$3,$3)\
         ON CONFLICT(job_id) DO UPDATE SET state=CASE WHEN gemini_batch_output_builds.state='ready' THEN 'ready' ELSE 'pending' END,\
         updated_ts=GREATEST(gemini_batch_output_builds.updated_ts,EXCLUDED.updated_ts)",
        &[&job_id, &file_id, &ts],
    )?;
    tx.execute(
        "UPDATE gemini_batch_jobs SET terminal_items_ts=$2,output_state='pending',update_ts=GREATEST(update_ts,$2)\
         WHERE job_id=$1 AND completed_ts IS NULL",
        &[&job_id, &ts],
    )?;
    Ok(())
}

impl PgStore {
    /// Cancel every not-started item and release its independent hold in one account transaction.
    pub fn gemini_batch_cancel(
        &mut self,
        account_id: &str,
        job_id: &str,
    ) -> Result<Option<GeminiBatchCancelResult>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let Some(job) = tx.query_opt(
            "SELECT creator_key_id FROM gemini_batch_jobs WHERE account_id=$1 AND job_id=$2 AND delete_ts IS NULL FOR UPDATE",
            &[&account_id, &job_id],
        )? else {
            tx.rollback()?;
            return Ok(None);
        };
        let creator_key_id: String = job.get(0);
        let canceled = tx.query(
            "UPDATE gemini_batch_items SET state='canceled',terminal_class='canceled',terminal_ts=$2,updated_ts=$2,worker_instance=NULL,worker_epoch=NULL,lease_until=NULL,selected_profile_id=NULL WHERE job_id=$1 AND state IN ('queued','claimed') AND dispatch_intent_ts IS NULL RETURNING item_index,hold_nano",
            &[&job_id, &ts],
        )?;
        let holds = canceled.iter().try_fold(0i64, |sum, row| {
            sum.checked_add(row.get::<_, i64>(1))
                .context("Gemini Batch canceled hold overflow")
        })?;
        for row in &canceled {
            let item_index = row.get::<_, i64>(0);
            // Extras first, then slot 2: either legacy delete may promote a surviving extra row.
            let mut deleted = tx.execute(
                "DELETE FROM gemini_batch_profile_leases_extra WHERE job_id=$1 AND item_index=$2",
                &[&job_id, &item_index],
            )?;
            deleted += tx.execute(
                "DELETE FROM gemini_batch_profile_leases_slot2 WHERE job_id=$1 AND item_index=$2",
                &[&job_id, &item_index],
            )?;
            deleted += tx.execute(
                "DELETE FROM gemini_batch_profile_leases WHERE job_id=$1 AND item_index=$2",
                &[&job_id, &item_index],
            )?;
            if deleted > 1 {
                bail!("Gemini Batch cancel deleted multiple profile leases")
            }
        }
        if holds > 0 {
            let account_changed = tx.execute(
                "UPDATE accounts SET balance_nano=(balance_nano::numeric+$1::bigint::numeric)::bigint,reserved_nano=reserved_nano-$1 WHERE id=$2 AND reserved_nano >= $1",
                &[&holds, &account_id],
            )?;
            if account_changed != 1 {
                bail!("Gemini Batch cancel account reservation mismatch")
            }
            let key_changed = tx.execute(
                "UPDATE api_keys SET reserved_nano=reserved_nano-$1 WHERE key_id=$2 AND account_id=$3 AND reserved_nano >= $1",
                &[&holds, &creator_key_id, &account_id],
            )?;
            if key_changed != 1 {
                bail!("Gemini Batch cancel key reservation mismatch")
            }
        }
        if tx.execute(
            "UPDATE gemini_batch_jobs SET cancel_requested_ts=COALESCE(cancel_requested_ts,$3),update_ts=$3 WHERE account_id=$1 AND job_id=$2",
            &[&account_id, &job_id, &ts],
        )? != 1
        {
            bail!("Gemini Batch cancel lost its locked job")
        }
        complete_job_if_terminal(&mut tx, job_id, ts)?;
        tx.commit()?;
        Ok(Some(GeminiBatchCancelResult {
            cancel_requested: true,
            canceled_items: canceled.len(),
        }))
    }

    /// Store encrypted terminal output and an immutable settlement intent under the live claim fence.
    pub fn enqueue_gemini_batch_settlement(
        &mut self,
        owner: &Owner,
        claim: &crate::GeminiBatchClaim,
        intent: &GeminiBatchSettlementIntent,
    ) -> Result<()> {
        self.enqueue_gemini_batch_settlement_inner(Some(owner), claim, intent)
    }

    pub fn enqueue_gemini_batch_recovery_settlement(
        &mut self,
        recovery: &crate::GeminiBatchRecoveryCandidate,
        intent: &GeminiBatchSettlementIntent,
    ) -> Result<()> {
        if recovery.job_id != intent.job_id
            || recovery.item_index != intent.item_index
            || recovery.request_id != intent.request_id
            || recovery.claim_generation != intent.claim_generation
            || recovery.profile_id.is_empty()
            || recovery.disposition != intent.disposition
            || recovery.terminal_state != intent.terminal_state
            || recovery.terminal_class != intent.terminal_class
        {
            bail!("Gemini Batch recovery settlement mismatch")
        }
        let claim = crate::GeminiBatchClaim {
            job_id: recovery.job_id.clone(),
            account_id: recovery.account_id.clone(),
            item_index: recovery.item_index,
            request_id: recovery.request_id.clone(),
            claim_generation: recovery.claim_generation,
            lease_until: 0,
            profile_id: recovery.profile_id.clone(),
        };
        self.enqueue_gemini_batch_settlement_inner(None, &claim, intent)
    }

    fn enqueue_gemini_batch_settlement_inner(
        &mut self,
        owner: Option<&Owner>,
        claim: &crate::GeminiBatchClaim,
        intent: &GeminiBatchSettlementIntent,
    ) -> Result<()> {
        intent.validate()?;
        if claim.job_id != intent.job_id
            || claim.item_index != intent.item_index
            || claim.request_id != intent.request_id
            || claim.claim_generation != intent.claim_generation
        {
            bail!("Gemini Batch settlement claim mismatch")
        }
        let mut tx = self.client.transaction()?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 8485412))",
            &[&intent.request_id],
        )?;
        if let Some(owner) = owner {
            Self::assert_owner_locked(&mut tx, owner, now())?;
        }
        let Some(item) = tx.query_opt(
            "SELECT state,claim_generation,worker_instance,worker_epoch,selected_profile_id FROM gemini_batch_items WHERE job_id=$1 AND item_index=$2 AND request_id=$3 FOR UPDATE",
            &[&intent.job_id, &intent.item_index, &intent.request_id],
        )? else {
            bail!("Gemini Batch settlement item does not exist")
        };
        let worker_instance: String = item
            .get::<_, Option<String>>(2)
            .context("Gemini Batch settlement worker is missing")?;
        let worker_epoch: i64 = item
            .get::<_, Option<i64>>(3)
            .context("Gemini Batch settlement epoch is missing")?;
        if let Some(owner) = owner {
            if worker_instance != owner.instance_id || worker_epoch != owner.epoch { bail!("Gemini Batch settlement owner fence is stale") }
        } else if tx.query_opt("SELECT 1 FROM engine_instances WHERE instance_id=$1 AND owner_epoch=$2 AND lease_until >= $3", &[&worker_instance,&worker_epoch,&now()])?.is_some() {
            bail!("Gemini Batch recovery owner is still live")
        }
        if item.get::<_, i64>(1) != intent.claim_generation
            || item.get::<_, Option<String>>(4).as_deref() != Some(claim.profile_id.as_str())
            || !matches!(
                item.get::<_, String>(0).as_str(),
                "dispatching" | "settlement_pending"
            )
        {
            bail!("Gemini Batch settlement fence is stale")
        }
        let lease_count = super::gemini_batch_claims::lock_gemini_batch_profile_lease(
            &mut tx,
            claim,
            &worker_instance,
            worker_epoch,
        )?;
        if lease_count != 1 {
            bail!("Gemini Batch settlement profile lease is stale")
        }
        let blob = &intent.result_blob;
        let blob_inserted = tx.execute(
            "INSERT INTO gemini_batch_blobs(job_id,item_index,kind,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,retention_ts,created_ts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(job_id,item_index,kind) DO NOTHING",
            &[&intent.job_id, &intent.item_index, &blob.kind, &blob.key_id, &blob.nonce, &blob.ciphertext, &blob.plaintext_len, &&blob.plaintext_digest[..], &blob.retention_ts, &intent.completed_ts],
        )?;
        if blob_inserted == 0 && !blob_matches(&mut tx, intent)? {
            bail!("Gemini Batch settlement blob replay conflict")
        }
        let usage = intent.usage.as_ref();
        let calibration = intent.calibration.as_ref();
        let result_kind = if blob.kind == "result" {
            "response"
        } else {
            "error"
        };
        let inserted = tx.execute("INSERT INTO gemini_batch_settlement_outbox(request_id,job_id,item_index,disposition,actual_nano,charge_basis_nano,real_nano,usage_input_tokens,usage_tool_prompt_tokens,usage_audio_input_tokens,usage_cached_input_tokens,usage_cached_audio_input_tokens,usage_output_tokens,usage_thinking_output_tokens,usage_image_output_tokens,usage_search_queries,usage_grounded_search_prompts,result_kind,terminal_state,state,created_ts,updated_ts,calibration_profile_id,calibration_model_id,calibration_service_tier,calibration_inference_geo,calibration_tariff_schedule_id,calibration_priced_ts,calibration_completed_at,calibration_input_tokens,calibration_audio_input_tokens,calibration_cache_read_tokens,calibration_cached_audio_input_tokens,calibration_cache_write_5m_tokens,calibration_cache_write_1h_tokens,calibration_output_tokens,calibration_thinking_output_tokens,calibration_image_output_tokens,calibration_tool_prompt_tokens,calibration_search_queries,calibration_grounded_search_prompts,calibration_api_input_nanousd,calibration_api_audio_input_nanousd,calibration_api_cache_read_nanousd,calibration_api_cached_audio_input_nanousd,calibration_api_cache_write_5m_nanousd,calibration_api_cache_write_1h_nanousd,calibration_api_output_nanousd,calibration_api_image_output_nanousd,calibration_api_search_nanousd,calibration_api_total_nanousd) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,'pending',$20,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,$39,$40,$41,$42,$43,$44,$45,$46,$47,$48,$49) ON CONFLICT(request_id) DO NOTHING", &[
            &intent.request_id,&intent.job_id,&intent.item_index,&intent.disposition.as_str(),&intent.actual_nano,&intent.charge_basis_nano,&intent.real_nano,
            &usage.map(|v|v.input_tokens),&usage.map(|v|v.tool_prompt_tokens),&usage.map(|v|v.audio_input_tokens),&usage.map(|v|v.cached_input_tokens),&usage.map(|v|v.cached_audio_input_tokens),&usage.map(|v|v.output_tokens),&usage.map(|v|v.thinking_output_tokens),&usage.map(|v|v.image_output_tokens),&usage.map(|v|v.search_queries),&usage.map(|v|v.grounded_search_prompts),
            &result_kind,&intent.terminal_state.as_str(),&intent.completed_ts,
            &calibration.map(|v|v.subject_id.as_str()),&calibration.map(|v|v.model_id.as_str()),&calibration.map(|v|v.service_tier.as_str()),&calibration.map(|v|v.inference_geo.as_str()),&calibration.map(|v|v.tariff_schedule_id.as_str()),&calibration.map(|v|v.priced_ts),&calibration.map(|v|v.completed_at),&calibration.map(|v|v.input_tokens),&calibration.map(|v|v.audio_input_tokens),&calibration.map(|v|v.cache_read_tokens),&calibration.map(|v|v.cached_audio_input_tokens),&calibration.map(|v|v.cache_write_5m_tokens),&calibration.map(|v|v.cache_write_1h_tokens),&calibration.map(|v|v.output_tokens),&calibration.map(|v|v.thinking_output_tokens),&calibration.map(|v|v.image_output_tokens),&calibration.map(|v|v.tool_prompt_tokens),&calibration.map(|v|v.search_queries),&calibration.map(|v|v.grounded_search_prompts),&calibration.map(|v|v.api_input_nanousd),&calibration.map(|v|v.api_audio_input_nanousd),&calibration.map(|v|v.api_cache_read_nanousd),&calibration.map(|v|v.api_cached_audio_input_nanousd),&calibration.map(|v|v.api_cache_write_5m_nanousd),&calibration.map(|v|v.api_cache_write_1h_nanousd),&calibration.map(|v|v.api_output_nanousd),&calibration.map(|v|v.api_image_output_nanousd),&calibration.map(|v|v.api_search_nanousd),&calibration.map(|v|v.api_total_nanousd)
        ])?;
        if inserted == 0 {
            let row = tx.query_one(SETTLEMENT_REPLAY_SELECT, &[&intent.request_id])?;
            let exact = row.get::<_, String>(0) == intent.job_id
                && row.get::<_, i64>(1) == intent.item_index
                && row.get::<_, String>(2) == intent.disposition.as_str()
                && row.get::<_, i64>(3) == intent.actual_nano
                && row.get::<_, i64>(4) == intent.charge_basis_nano
                && row.get::<_, i64>(5) == intent.real_nano
                && usage_from_row(&row, 6) == intent.usage
                && row.get::<_, String>(16) == result_kind
                && row.get::<_, String>(17) == intent.terminal_state.as_str()
                && row.get::<_, i64>(18) == intent.completed_ts
                && calibration_from_row(&row, 19, &intent.request_id) == intent.calibration;
            if !exact {
                bail!("Gemini Batch settlement replay conflict")
            }
        }
        let item_changed = tx.execute(
            "UPDATE gemini_batch_items SET state='settlement_pending',settlement_id=$3,updated_ts=$4 WHERE job_id=$1 AND item_index=$2 AND request_id=$3 AND claim_generation=$5 AND worker_instance=$6 AND worker_epoch=$7 AND selected_profile_id=$8 AND state IN ('dispatching','settlement_pending')",
            &[&intent.job_id, &intent.item_index, &intent.request_id, &intent.completed_ts, &intent.claim_generation, &worker_instance, &worker_epoch, &claim.profile_id],
        )?;
        if item_changed != 1 {
            bail!("Gemini Batch settlement lost its fenced item")
        }
        tx.commit()?;
        Ok(())
    }

    /// Apply one batch settlement atomically.
    pub fn process_gemini_batch_settlement(&mut self, request_id: &str) -> Result<Option<i64>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 8485412))",
            &[&request_id],
        )?;
        let Some(r) = tx.query_opt("SELECT o.job_id,o.item_index,o.actual_nano,o.charge_basis_nano,o.real_nano,o.disposition,o.terminal_state,o.state,i.hold_nano,j.account_id,i.creator_key_id,i.payable_multiplier_bp,j.public_model,i.selected_profile_id,o.usage_input_tokens,o.usage_tool_prompt_tokens,o.usage_audio_input_tokens,o.usage_cached_input_tokens,o.usage_cached_audio_input_tokens,o.usage_output_tokens,o.usage_thinking_output_tokens,o.usage_image_output_tokens,o.usage_search_queries,o.usage_grounded_search_prompts,o.result_kind,o.created_ts,o.calibration_profile_id,o.calibration_model_id,o.calibration_service_tier,o.calibration_inference_geo,o.calibration_tariff_schedule_id,o.calibration_priced_ts,o.calibration_completed_at,o.calibration_input_tokens,o.calibration_audio_input_tokens,o.calibration_cache_read_tokens,o.calibration_cached_audio_input_tokens,o.calibration_cache_write_5m_tokens,o.calibration_cache_write_1h_tokens,o.calibration_output_tokens,o.calibration_thinking_output_tokens,o.calibration_image_output_tokens,o.calibration_tool_prompt_tokens,o.calibration_search_queries,o.calibration_grounded_search_prompts,o.calibration_api_input_nanousd,o.calibration_api_audio_input_nanousd,o.calibration_api_cache_read_nanousd,o.calibration_api_cached_audio_input_nanousd,o.calibration_api_cache_write_5m_nanousd,o.calibration_api_cache_write_1h_nanousd,o.calibration_api_output_nanousd,o.calibration_api_image_output_nanousd,o.calibration_api_search_nanousd,o.calibration_api_total_nanousd,i.worker_instance,i.worker_epoch,i.claim_generation FROM gemini_batch_settlement_outbox o JOIN gemini_batch_items i ON(i.job_id=o.job_id AND i.item_index=o.item_index AND i.request_id=o.request_id) JOIN gemini_batch_jobs j ON j.job_id=o.job_id WHERE o.request_id=$1 FOR UPDATE OF o,i,j", &[&request_id])? else {
            return Ok(None);
        };
        let account_id: String = r.get(9);
        if r.get::<_, String>(7) == "done" {
            let job_id: String = r.get(0);
            let item_index: i64 = r.get(1);
            let actual: i64 = r.get(2);
            let measured = r.get::<_, String>(5) == "settle";
            let terminal: String = r.get(6);
            let result_kind: String = r.get(24);
            let usage = usage_from_row(&r, 14);
            let calibration = calibration_from_row(&r, 26, request_id);
            let item = tx.query_one(
                "SELECT state,settlement_id,terminal_ts FROM gemini_batch_items \
                 WHERE job_id=$1 AND item_index=$2 AND request_id=$3",
                &[&job_id, &item_index, &request_id],
            )?;
            if item.get::<_, String>(0) != terminal
                || item.get::<_, Option<String>>(1).as_deref() != Some(request_id)
                || item.get::<_, Option<i64>>(2).is_none()
                || measured != (terminal == "succeeded" && result_kind == "response")
            {
                bail!("Gemini Batch done replay terminal integrity mismatch")
            }
            let ledger_count: i64 = tx
                .query_one(
                    "SELECT COUNT(*)::bigint FROM ledger WHERE kind='charge' AND request_id=$1",
                    &[&request_id],
                )?
                .get(0);
            if ledger_count != i64::from(actual > 0) {
                bail!("Gemini Batch done replay ledger integrity mismatch")
            }
            let usage_count: i64 = tx
                .query_one(
                    "SELECT COUNT(*)::bigint FROM usage_events WHERE request_id=$1",
                    &[&request_id],
                )?
                .get(0);
            let calibration_count: i64 = tx
                .query_one(
                    "SELECT COUNT(*)::bigint FROM provider_turn_calibration_events \
                     WHERE provider='google' AND request_id=$1",
                    &[&request_id],
                )?
                .get(0);
            if usage_count != i64::from(measured)
                || calibration_count != i64::from(measured)
                || measured != usage.is_some()
                || measured != calibration.is_some()
            {
                bail!("Gemini Batch done replay evidence integrity mismatch")
            }
            let balance = tx
                .query_one(
                    "SELECT balance_nano FROM accounts WHERE id=$1",
                    &[&account_id],
                )?
                .get(0);
            tx.commit()?;
            return Ok(Some(balance));
        }
        if r.get::<_, String>(7) != "pending" {
            bail!("Gemini Batch settlement outbox state is inconsistent")
        }
        let disposition: String = r.get(5);
        let terminal: String = r.get(6);
        let result_kind: String = r.get(24);
        let measured = disposition == "settle";
        if measured != (terminal == "succeeded" && result_kind == "response") {
            bail!("Gemini Batch settlement terminal shape is inconsistent")
        }
        let usage = usage_from_row(&r, 14);
        let calibration = calibration_from_row(&r, 26, request_id);
        if measured {
            let usage = usage
                .as_ref()
                .context("Gemini Batch settlement usage is missing")?;
            let calibration = calibration
                .as_ref()
                .context("Gemini Batch settlement calibration is missing")?;
            crate::validate_provider_turn_calibration_event(calibration)?;
            if calibration.subject_id
                != r.get::<_, Option<String>>(13)
                    .context("Gemini Batch settlement profile is missing")?
                || calibration.model_id != r.get::<_, String>(12)
                || calibration.tariff_schedule_id.is_empty()
                || calibration.completed_at != r.get::<_, i64>(25)
                || calibration.input_tokens != usage.input_tokens
                || calibration.tool_prompt_tokens != usage.tool_prompt_tokens
                || calibration.audio_input_tokens != usage.audio_input_tokens
                || calibration.cache_read_tokens != usage.cached_input_tokens
                || calibration.cached_audio_input_tokens != usage.cached_audio_input_tokens
                || calibration.output_tokens != usage.output_tokens
                || calibration.thinking_output_tokens != usage.thinking_output_tokens
                || calibration.image_output_tokens != usage.image_output_tokens
                || calibration.search_queries != usage.search_queries
                || calibration.grounded_search_prompts != usage.grounded_search_prompts
            {
                bail!("Gemini Batch settlement usage/calibration mismatch")
            }
        } else if usage.is_some() || calibration.is_some() {
            bail!("Gemini Batch unmeasured settlement carries measured evidence")
        }
        let job_id: String = r.get(0);
        let item_index: i64 = r.get(1);
        let actual: i64 = r.get(2);
        let charge_basis: i64 = r.get(3);
        let real: i64 = r.get(4);
        let hold: i64 = r.get(8);
        let key_id: String = r.get(10);
        let mult: i64 = r.get(11);
        let model: String = r.get(12);
        let collection = super::collect_account_settlement_tx(&mut tx, &account_id, hold, actual)
            .context("Gemini Batch settlement account reservation mismatch")?;
        let balance = collection.balance_nano;
        let uncollected = collection.uncollected_nano;
        let key_changed = tx.execute("UPDATE api_keys SET spent_nano=(spent_nano::numeric+$1::bigint::numeric)::bigint,reserved_nano=reserved_nano-$2 WHERE key_id=$3 AND account_id=$4 AND reserved_nano >= $2", &[&actual, &hold, &key_id, &account_id])?;
        if key_changed != 1
            && tx
                .query_opt("SELECT 1 FROM api_keys WHERE key_id=$1", &[&key_id])?
                .is_some()
        {
            bail!("Gemini Batch settlement key reservation mismatch")
        }
        if actual > 0 {
            let ledger_inserted = tx.execute("INSERT INTO ledger(account_id,key_id,kind,request_id,amount_nano,balance_after_nano,ts,model,provider,official_nano,payable_multiplier_bp,uncollected_nano) VALUES($1,$2,'charge',$3,$4,$5,$6,$7,'google',$8,$9,$10) ON CONFLICT DO NOTHING", &[&account_id, &key_id, &request_id, &actual, &balance, &ts, &model, &charge_basis, &mult, &uncollected])?;
            if ledger_inserted != 1 {
                bail!("Gemini Batch settlement ledger key survived with inconsistent replay")
            }
        } else if tx
            .query_opt(
                "SELECT 1 FROM ledger WHERE kind='charge' AND request_id=$1",
                &[&request_id],
            )?
            .is_some()
        {
            bail!("Gemini Batch zero settlement has a surviving ledger key")
        }
        if measured {
            let usage = usage.as_ref().expect("measured usage validated");
            let calibration = calibration
                .as_ref()
                .expect("measured calibration validated");
            let usage_input = usage
                .input_tokens
                .checked_add(usage.audio_input_tokens)
                .context("Gemini Batch usage input overflow")?;
            let usage_cache = usage
                .cached_input_tokens
                .checked_add(usage.cached_audio_input_tokens)
                .context("Gemini Batch usage cache overflow")?;
            let usage_output = usage
                .output_tokens
                .checked_add(usage.image_output_tokens)
                .context("Gemini Batch usage output overflow")?;
            let usage_search = usage.search_queries.max(usage.grounded_search_prompts);
            if tx.execute("INSERT INTO usage_events(request_id,account_id,key_id,model,input_tokens,output_tokens,cache_read_tokens,web_search_requests,real_nano,charge_nano,ts,provider,payable_multiplier_bp,uncollected_nano,charge_basis_nano) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'google',$12,$13,$14) ON CONFLICT(request_id) DO NOTHING", &[&request_id, &account_id, &key_id, &model, &usage_input, &usage_output, &usage_cache, &usage_search, &real, &actual, &ts, &mult, &uncollected, &charge_basis])? != 1 { bail!("Gemini Batch settlement usage key survived with inconsistent replay") }
            let calibration_spend =
                super::record_provider_turn_calibration_event_tx(&mut tx, calibration)?;
            if !calibration_spend.inserted {
                bail!("Gemini Batch settlement calibration already existed before APPLY")
            }
        }
        let terminal_class = match terminal.as_str() {
            "succeeded" => "success",
            "canceled" => "canceled",
            "indeterminate" => "indeterminate",
            _ => "expired",
        };
        if tx.execute("UPDATE gemini_batch_items SET state=$3,terminal_class=$4,terminal_ts=$5,updated_ts=$5,usage_input_tokens=$6,usage_tool_prompt_tokens=$7,usage_audio_input_tokens=$8,usage_cached_input_tokens=$9,usage_cached_audio_input_tokens=$10,usage_output_tokens=$11,usage_thinking_output_tokens=$12,usage_image_output_tokens=$13,usage_search_queries=$14,usage_grounded_search_prompts=$15,worker_instance=NULL,worker_epoch=NULL,lease_until=NULL,selected_profile_id=NULL WHERE job_id=$1 AND item_index=$2 AND request_id=$16 AND state='settlement_pending' AND settlement_id=$16", &[&job_id,&item_index,&terminal,&terminal_class,&ts,&usage.as_ref().map(|v|v.input_tokens),&usage.as_ref().map(|v|v.tool_prompt_tokens),&usage.as_ref().map(|v|v.audio_input_tokens),&usage.as_ref().map(|v|v.cached_input_tokens),&usage.as_ref().map(|v|v.cached_audio_input_tokens),&usage.as_ref().map(|v|v.output_tokens),&usage.as_ref().map(|v|v.thinking_output_tokens),&usage.as_ref().map(|v|v.image_output_tokens),&usage.as_ref().map(|v|v.search_queries),&usage.as_ref().map(|v|v.grounded_search_prompts),&request_id])? != 1 { bail!("Gemini Batch settlement item terminalization failed") }
        let claim = crate::GeminiBatchClaim {
            job_id: job_id.clone(),
            account_id: account_id.clone(),
            item_index,
            request_id: request_id.to_owned(),
            claim_generation: r
                .get::<_, Option<i64>>(57)
                .context("Gemini Batch settlement claim generation is missing")?,
            lease_until: 0,
            profile_id: r
                .get::<_, Option<String>>(13)
                .context("Gemini Batch settlement profile is missing")?,
        };
        let worker_instance: String = r
            .get::<_, Option<String>>(55)
            .context("Gemini Batch settlement worker is missing")?;
        let worker_epoch: i64 = r
            .get::<_, Option<i64>>(56)
            .context("Gemini Batch settlement epoch is missing")?;
        if super::gemini_batch_claims::delete_gemini_batch_profile_lease(
            &mut tx,
            &claim,
            &worker_instance,
            worker_epoch,
        )? != 1
        {
            bail!("Gemini Batch settlement profile lease is missing")
        }
        if tx.execute("UPDATE gemini_batch_settlement_outbox SET state='done',committed_ts=$2,updated_ts=$2 WHERE request_id=$1 AND state='pending'", &[&request_id, &ts])? != 1 {
            bail!("Gemini Batch settlement outbox completion failed")
        }
        complete_job_if_terminal(&mut tx, &job_id, ts)?;
        tx.commit()?;
        Ok(Some(balance))
    }

    pub fn drain_gemini_batch_settlements(&mut self, limit: usize) -> Result<usize> {
        let ids: Vec<String> = self
            .client
            .query(
                "SELECT request_id FROM gemini_batch_settlement_outbox WHERE state='pending' AND next_attempt_ts <= $1 ORDER BY created_ts LIMIT $2",
                &[&now(), &(limit.clamp(1, 10000) as i64)],
            )?
            .into_iter()
            .map(|row| row.get(0))
            .collect();
        let mut processed = 0;
        for id in ids {
            match self.process_gemini_batch_settlement(&id) {
                Ok(Some(_)) => processed += 1,
                Ok(None) => {}
                Err(error) => {
                    let ts = now();
                    let message = format!("{error:#}").chars().take(128).collect::<String>();
                    let permanent =
                        super::classify_failure(&error) == super::FailureClass::Permanent;
                    let state = if permanent { "failed" } else { "pending" };
                    let attempts_before_failure = self
                        .client
                        .query_opt(
                            "SELECT attempts FROM gemini_batch_settlement_outbox \
                             WHERE request_id=$1 AND state <> 'done'",
                            &[&id],
                        )?
                        .map(|row| row.get::<_, i64>(0))
                        .unwrap_or_default();
                    let next_attempt = if permanent {
                        0
                    } else {
                        ts + super::retry_backoff_seconds(attempts_before_failure)
                    };
                    let attempts = self
                        .client
                        .query_opt(
                            "UPDATE gemini_batch_settlement_outbox SET state=$2,\
                             attempts=attempts+1,last_error_class=$3,next_attempt_ts=$4,updated_ts=$5 \
                             WHERE request_id=$1 AND state <> 'done' RETURNING attempts",
                            &[&id, &state, &message, &next_attempt, &ts],
                        )?
                        .map(|row| row.get::<_, i64>(0));
                    if permanent {
                        elog::error(
                            "registry",
                            format!(
                                "Gemini Batch settlement {id} moved to failed after {} attempts: {message}",
                                attempts.unwrap_or_default()
                            ),
                        );
                    }
                }
            }
        }
        Ok(processed)
    }

    pub fn gemini_batch_delete(&mut self, account_id: &str, job_id: &str) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let deleted = tx.execute("UPDATE gemini_batch_jobs j SET delete_ts=COALESCE(delete_ts,$3),update_ts=GREATEST(update_ts,$3) WHERE account_id=$1 AND job_id=$2 AND completed_ts IS NOT NULL AND NOT EXISTS(SELECT 1 FROM gemini_batch_items i WHERE i.job_id=j.job_id AND i.state NOT IN ('succeeded','failed','indeterminate','canceled')) AND NOT EXISTS(SELECT 1 FROM gemini_batch_settlement_outbox o WHERE o.job_id=j.job_id AND o.state<>'done')", &[&account_id, &job_id, &ts])? == 1;
        if !deleted {
            tx.rollback()?;
            return Ok(false);
        }
        // A committed admission is only a pre-publish replay marker. The durable job row remains the
        // canonical idempotency authority, so retaining this staging row after job deletion merely
        // pins its input file through the expand-only foreign key forever.
        tx.execute(
            "DELETE FROM gemini_batch_admissions WHERE account_id=$1 AND job_id=$2 AND state='committed'",
            &[&account_id, &job_id],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn prune_gemini_batch(
        &mut self,
        older_than: i64,
        limit: usize,
    ) -> Result<GeminiBatchPruneReport> {
        let limit = limit.clamp(1, MAX_BATCH_PRUNE_LIMIT) as i64;
        let mut tx = self.client.transaction()?;
        let blobs = tx.execute("DELETE FROM gemini_batch_blobs WHERE (job_id,item_index,kind) IN (SELECT b.job_id,b.item_index,b.kind FROM gemini_batch_blobs b JOIN gemini_batch_jobs j USING(job_id) WHERE b.retention_ts<$1 AND j.completed_ts IS NOT NULL ORDER BY b.retention_ts LIMIT $2)", &[&older_than, &limit])? as usize;
        let chunks = tx.execute("DELETE FROM gemini_batch_file_chunks WHERE file_id IN(SELECT f.file_id FROM gemini_batch_files f WHERE f.expiration_ts<$1 AND NOT EXISTS(SELECT 1 FROM gemini_batch_item_files r WHERE r.file_id=f.file_id) LIMIT $2)", &[&older_than, &limit])? as usize;
        let files = tx.execute("DELETE FROM gemini_batch_files f WHERE f.file_id IN (SELECT candidate.file_id FROM gemini_batch_files candidate WHERE candidate.expiration_ts<$1 AND NOT EXISTS(SELECT 1 FROM gemini_batch_file_chunks c WHERE c.file_id=candidate.file_id) AND NOT EXISTS(SELECT 1 FROM gemini_batch_item_files r WHERE r.file_id=candidate.file_id) AND NOT EXISTS(SELECT 1 FROM gemini_batch_jobs j WHERE j.input_file_id=candidate.file_id OR j.output_file_id=candidate.file_id) ORDER BY candidate.expiration_ts,candidate.file_id LIMIT $2)", &[&older_than,&limit])? as usize;
        tx.commit()?;
        Ok(GeminiBatchPruneReport {
            blobs,
            chunks,
            files,
            items: 0,
            jobs: 0,
        })
    }
}
