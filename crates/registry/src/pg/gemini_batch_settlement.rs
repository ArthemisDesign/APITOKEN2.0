//! Gemini Batch cancellation, durable settlement, and bounded pruning.

use super::{now, PgStore};
use crate::gemini_batch::{
    BATCH_RESULT_RETENTION_SECS, GeminiBatchCancelResult, GeminiBatchPruneReport,
    GeminiBatchSettlementIntent, MAX_BATCH_PRUNE_LIMIT,
};
use crate::ACCOUNT_OVERDRAFT_NANO;
use anyhow::{bail, Context, Result};

impl PgStore {
    /// Cancel every not-started item and release its independent hold in one account transaction.
    pub fn gemini_batch_cancel(&mut self, account_id: &str, job_id: &str) -> Result<Option<GeminiBatchCancelResult>> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let Some(job) = tx.query_opt(
            "SELECT creator_key_id FROM gemini_batch_jobs WHERE account_id=$1 AND job_id=$2 AND delete_ts IS NULL FOR UPDATE",
            &[&account_id, &job_id],
        )? else { tx.rollback()?; return Ok(None) };
        let creator_key_id: String = job.get(0);
        let holds: i64 = tx.query_one(
            "SELECT COALESCE(SUM(hold_nano),0)::bigint FROM gemini_batch_items WHERE job_id=$1 AND state IN ('queued','claimed') AND dispatch_intent_ts IS NULL FOR UPDATE",
            &[&job_id],
        )?.get(0);
        let changed = tx.execute(
            "UPDATE gemini_batch_items SET state='canceled',terminal_class='canceled',terminal_ts=$2,updated_ts=$2,worker_instance=NULL,worker_epoch=NULL,lease_until=NULL,selected_profile_id=NULL WHERE job_id=$1 AND state IN ('queued','claimed') AND dispatch_intent_ts IS NULL",
            &[&job_id, &ts],
        )? as usize;
        tx.execute("DELETE FROM gemini_batch_profile_leases WHERE job_id=$1 AND NOT EXISTS (SELECT 1 FROM gemini_batch_items i WHERE i.job_id=$1 AND i.item_index=gemini_batch_profile_leases.item_index AND i.state IN ('dispatching','settlement_pending'))", &[&job_id])?;
        if holds > 0 {
            tx.execute("UPDATE accounts SET balance_nano=balance_nano+$1,reserved_nano=reserved_nano-$1 WHERE id=$2 AND reserved_nano >= $1", &[&holds, &account_id])?;
            tx.execute("UPDATE api_keys SET reserved_nano=reserved_nano-$1 WHERE key_id=$2 AND account_id=$3 AND reserved_nano >= $1", &[&holds, &creator_key_id, &account_id])?;
        }
        let requested = tx.execute("UPDATE gemini_batch_jobs SET cancel_requested_ts=COALESCE(cancel_requested_ts,$3),update_ts=$3 WHERE account_id=$1 AND job_id=$2", &[&account_id,&job_id,&ts])? == 1;
        tx.commit()?;
        Ok(Some(GeminiBatchCancelResult { cancel_requested: requested, canceled_items: changed }))
    }

    /// Store encrypted terminal output and an immutable settlement intent under the live claim fence.
    pub fn enqueue_gemini_batch_settlement(&mut self, intent: &GeminiBatchSettlementIntent) -> Result<()> {
        intent.validate()?;
        let mut tx = self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1, 8485412))", &[&intent.request_id])?;
        let Some(item) = tx.query_opt("SELECT state,claim_generation FROM gemini_batch_items WHERE job_id=$1 AND item_index=$2 AND request_id=$3 FOR UPDATE", &[&intent.job_id,&intent.item_index,&intent.request_id])? else { bail!("Gemini Batch settlement item does not exist") };
        let state: String = item.get(0);
        if state != "settlement_pending" && (!matches!(state.as_str(), "dispatching"|"claimed") || item.get::<_,i64>(1) != intent.claim_generation) { bail!("Gemini Batch settlement fence is stale") }
        let blob = &intent.result_blob;
        tx.execute("INSERT INTO gemini_batch_blobs(job_id,item_index,kind,key_id,nonce,ciphertext,plaintext_len,plaintext_digest,retention_ts,created_ts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(job_id,item_index,kind) DO NOTHING", &[&intent.job_id,&intent.item_index,&blob.kind,&blob.key_id,&blob.nonce,&blob.ciphertext,&blob.plaintext_len,&&blob.plaintext_digest[..],&blob.retention_ts,&intent.completed_ts])?;
        let usage = intent.usage.as_ref();
        let calibration = intent.calibration.as_ref();
        let inserted = tx.execute("INSERT INTO gemini_batch_settlement_outbox(request_id,job_id,item_index,disposition,actual_nano,charge_basis_nano,real_nano,usage_input_tokens,usage_tool_prompt_tokens,usage_audio_input_tokens,usage_cached_input_tokens,usage_cached_audio_input_tokens,usage_output_tokens,usage_thinking_output_tokens,usage_image_output_tokens,usage_search_queries,usage_grounded_search_prompts,result_kind,terminal_state,state,created_ts,updated_ts,calibration_profile_id,calibration_model_id,calibration_service_tier,calibration_inference_geo,calibration_tariff_schedule_id,calibration_priced_ts,calibration_completed_at,calibration_input_tokens,calibration_audio_input_tokens,calibration_cache_read_tokens,calibration_cached_audio_input_tokens,calibration_cache_write_5m_tokens,calibration_cache_write_1h_tokens,calibration_output_tokens,calibration_thinking_output_tokens,calibration_image_output_tokens,calibration_tool_prompt_tokens,calibration_search_queries,calibration_grounded_search_prompts,calibration_api_input_nanousd,calibration_api_audio_input_nanousd,calibration_api_cache_read_nanousd,calibration_api_cached_audio_input_nanousd,calibration_api_cache_write_5m_nanousd,calibration_api_cache_write_1h_nanousd,calibration_api_output_nanousd,calibration_api_image_output_nanousd,calibration_api_search_nanousd,calibration_api_total_nanousd) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,'pending',$20,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,$39,$40,$41,$42,$43,$44,$45,$46,$47,$48,$49) ON CONFLICT(request_id) DO NOTHING", &[
            &intent.request_id,&intent.job_id,&intent.item_index,&intent.disposition.as_str(),&intent.actual_nano,&intent.charge_basis_nano,&intent.real_nano,
            &usage.map(|v|v.input_tokens),&usage.map(|v|v.tool_prompt_tokens),&usage.map(|v|v.audio_input_tokens),&usage.map(|v|v.cached_input_tokens),&usage.map(|v|v.cached_audio_input_tokens),&usage.map(|v|v.output_tokens),&usage.map(|v|v.thinking_output_tokens),&usage.map(|v|v.image_output_tokens),&usage.map(|v|v.search_queries),&usage.map(|v|v.grounded_search_prompts),
            &if blob.kind=="result"{"response"}else{"error"},&intent.terminal_state.as_str(),&intent.completed_ts,
            &calibration.map(|v|v.subject_id.as_str()),&calibration.map(|v|v.model_id.as_str()),&calibration.map(|v|v.service_tier.as_str()),&calibration.map(|v|v.inference_geo.as_str()),&calibration.map(|v|v.tariff_schedule_id.as_str()),&calibration.map(|v|v.priced_ts),&calibration.map(|v|v.completed_at),&calibration.map(|v|v.input_tokens),&calibration.map(|v|v.audio_input_tokens),&calibration.map(|v|v.cache_read_tokens),&calibration.map(|v|v.cached_audio_input_tokens),&calibration.map(|v|v.cache_write_5m_tokens),&calibration.map(|v|v.cache_write_1h_tokens),&calibration.map(|v|v.output_tokens),&calibration.map(|v|v.thinking_output_tokens),&calibration.map(|v|v.image_output_tokens),&calibration.map(|v|v.tool_prompt_tokens),&calibration.map(|v|v.search_queries),&calibration.map(|v|v.grounded_search_prompts),&calibration.map(|v|v.api_input_nanousd),&calibration.map(|v|v.api_audio_input_nanousd),&calibration.map(|v|v.api_cache_read_nanousd),&calibration.map(|v|v.api_cached_audio_input_nanousd),&calibration.map(|v|v.api_cache_write_5m_nanousd),&calibration.map(|v|v.api_cache_write_1h_nanousd),&calibration.map(|v|v.api_output_nanousd),&calibration.map(|v|v.api_image_output_nanousd),&calibration.map(|v|v.api_search_nanousd),&calibration.map(|v|v.api_total_nanousd)
        ])?;
        if inserted == 0 {
            let row = tx.query_one("SELECT job_id,item_index,actual_nano,charge_basis_nano,real_nano,disposition,terminal_state FROM gemini_batch_settlement_outbox WHERE request_id=$1", &[&intent.request_id])?;
            if row.get::<_,String>(0)!=intent.job_id || row.get::<_,i64>(1)!=intent.item_index || row.get::<_,i64>(2)!=intent.actual_nano || row.get::<_,i64>(3)!=intent.charge_basis_nano || row.get::<_,i64>(4)!=intent.real_nano || row.get::<_,String>(5)!=intent.disposition.as_str() || row.get::<_,String>(6)!=intent.terminal_state.as_str() { bail!("Gemini Batch settlement replay conflict") }
        }
        tx.execute("UPDATE gemini_batch_items SET state='settlement_pending',settlement_id=$3,updated_ts=$4 WHERE job_id=$1 AND item_index=$2", &[&intent.job_id,&intent.item_index,&intent.request_id,&intent.completed_ts])?;
        tx.commit()?;
        Ok(())
    }

    /// Apply one batch settlement atomically. This uses the same account-floor equation as the
    /// interactive outbox; the source row differs, but no approximate/clamped money is invented.
    pub fn process_gemini_batch_settlement(&mut self, request_id: &str) -> Result<Option<i64>> {
        let ts=now(); let mut tx=self.client.transaction()?;
        tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1, 8485412))", &[&request_id])?;
        let Some(r)=tx.query_opt("SELECT o.job_id,o.item_index,o.actual_nano,o.charge_basis_nano,o.real_nano,o.disposition,o.terminal_state,o.state,i.hold_nano,j.account_id,i.creator_key_id,i.payable_multiplier_bp,j.public_model,i.selected_profile_id,o.usage_input_tokens,o.usage_output_tokens,o.usage_cached_input_tokens,o.calibration_api_total_nanousd FROM gemini_batch_settlement_outbox o JOIN gemini_batch_items i ON(i.job_id=o.job_id AND i.item_index=o.item_index) JOIN gemini_batch_jobs j ON j.job_id=o.job_id WHERE o.request_id=$1 FOR UPDATE OF o,i,j", &[&request_id])? else{return Ok(None)};
        if r.get::<_,String>(7)=="done" { let b=tx.query_one("SELECT balance_nano FROM accounts WHERE id=$1", &[&r.get::<_,String>(9)])?.get(0);tx.commit()?;return Ok(Some(b)) }
        let job_id:String=r.get(0);let item_index:i64=r.get(1);let actual:i64=r.get(2);let hold:i64=r.get(8);let account_id:String=r.get(9);let key_id:String=r.get(10);let mult:i64=r.get(11);let model:String=r.get(12);
        let floor=-ACCOUNT_OVERDRAFT_NANO;
        let a=tx.query_one("WITH c AS MATERIALIZED(SELECT id,LEAST($2::bigint::numeric,GREATEST(0::numeric,balance_nano::numeric+$1::bigint::numeric-LEAST(balance_nano::numeric,$4::bigint::numeric)))::bigint collected FROM accounts WHERE id=$3 AND reserved_nano >= $1 FOR UPDATE) UPDATE accounts a SET balance_nano=(a.balance_nano::numeric+$1-current.collected::numeric)::bigint,spent_nano=(a.spent_nano::numeric+$2)::bigint,reserved_nano=a.reserved_nano-$1,uncollected_nano=(a.uncollected_nano::numeric+$2-current.collected::numeric)::bigint FROM c current WHERE a.id=current.id RETURNING a.balance_nano,current.collected", &[&hold,&actual,&account_id,&floor])?;
        let balance:i64=a.get(0);let collected:i64=a.get(1);let uncollected=actual.checked_sub(collected).context("batch collection exceeds actual")?;
        tx.execute("UPDATE api_keys SET spent_nano=spent_nano+$1,reserved_nano=reserved_nano-$2 WHERE key_id=$3 AND account_id=$4 AND reserved_nano >= $2", &[&actual,&hold,&key_id,&account_id])?;
        if actual>0 {tx.execute("INSERT INTO ledger(account_id,key_id,kind,request_id,amount_nano,balance_after_nano,ts,model,provider,official_nano,payable_multiplier_bp,uncollected_nano) VALUES($1,$2,'charge',$3,$4,$5,$6,$7,'google',$8,$9,$10) ON CONFLICT DO NOTHING", &[&account_id,&key_id,&request_id,&actual,&balance,&ts,&model,&r.get::<_,i64>(3),&mult,&uncollected])?;}
        if r.get::<_,Option<i64>>(14).is_some(){tx.execute("INSERT INTO usage_events(request_id,account_id,key_id,model,input_tokens,output_tokens,cache_read_tokens,real_nano,charge_nano,ts,provider,payable_multiplier_bp,uncollected_nano,charge_basis_nano) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'google',$11,$12,$13) ON CONFLICT(request_id) DO NOTHING", &[&request_id,&account_id,&key_id,&model,&r.get::<_,Option<i64>>(14).unwrap_or(0),&r.get::<_,Option<i64>>(15).unwrap_or(0),&r.get::<_,Option<i64>>(16).unwrap_or(0),&r.get::<_,i64>(4),&actual,&ts,&mult,&uncollected,&r.get::<_,i64>(3)])?;}
        // Calibration event is copied from the durable outbox by a single INSERT...SELECT. The
        // cumulative subject ledger advances only when that immutable insert wins.
        let inserted=tx.execute("INSERT INTO provider_turn_calibration_events(provider,request_id,subject_id,model_id,service_tier,inference_geo,tariff_schedule_id,priced_ts,completed_at,input_tokens,audio_input_tokens,cache_read_tokens,cached_audio_input_tokens,cache_write_5m_tokens,cache_write_1h_tokens,output_tokens,thinking_output_tokens,image_output_tokens,tool_prompt_tokens,search_queries,grounded_search_prompts,api_input_nanousd,api_audio_input_nanousd,api_cache_read_nanousd,api_cached_audio_input_nanousd,api_cache_write_5m_nanousd,api_cache_write_1h_nanousd,api_output_nanousd,api_image_output_nanousd,api_search_nanousd,api_total_nanousd) SELECT 'google',request_id,calibration_profile_id,calibration_model_id,calibration_service_tier,calibration_inference_geo,calibration_tariff_schedule_id,calibration_priced_ts,calibration_completed_at,calibration_input_tokens,calibration_audio_input_tokens,calibration_cache_read_tokens,calibration_cached_audio_input_tokens,calibration_cache_write_5m_tokens,calibration_cache_write_1h_tokens,calibration_output_tokens,calibration_thinking_output_tokens,calibration_image_output_tokens,calibration_tool_prompt_tokens,calibration_search_queries,calibration_grounded_search_prompts,calibration_api_input_nanousd,calibration_api_audio_input_nanousd,calibration_api_cache_read_nanousd,calibration_api_cached_audio_input_nanousd,calibration_api_cache_write_5m_nanousd,calibration_api_cache_write_1h_nanousd,calibration_api_output_nanousd,calibration_api_image_output_nanousd,calibration_api_search_nanousd,calibration_api_total_nanousd FROM gemini_batch_settlement_outbox WHERE request_id=$1 AND calibration_profile_id IS NOT NULL ON CONFLICT(provider,request_id) DO NOTHING", &[&request_id])?;
        if inserted==1 {tx.execute("INSERT INTO provider_calibration_subject_spend(provider,subject_id,spent_nano,tracking_started_ts,updated_ts) SELECT 'google',calibration_profile_id,calibration_api_total_nanousd,calibration_completed_at,calibration_completed_at FROM gemini_batch_settlement_outbox WHERE request_id=$1 ON CONFLICT(provider,subject_id) DO UPDATE SET spent_nano=provider_calibration_subject_spend.spent_nano+EXCLUDED.spent_nano,updated_ts=GREATEST(provider_calibration_subject_spend.updated_ts,EXCLUDED.updated_ts)", &[&request_id])?;}
        let terminal:String=r.get(6);let class=match terminal.as_str(){"succeeded"=>"success","indeterminate"=>"indeterminate","canceled"=>"canceled",_=>"protocol_error"};
        tx.execute("UPDATE gemini_batch_items SET state=$3,terminal_class=$4,terminal_ts=$5,updated_ts=$5,worker_instance=NULL,worker_epoch=NULL,lease_until=NULL,selected_profile_id=NULL WHERE job_id=$1 AND item_index=$2", &[&job_id,&item_index,&terminal,&class,&ts])?;
        tx.execute("DELETE FROM gemini_batch_profile_leases WHERE job_id=$1 AND item_index=$2", &[&job_id,&item_index])?;
        tx.execute("UPDATE gemini_batch_settlement_outbox SET state='done',committed_ts=$2,updated_ts=$2 WHERE request_id=$1", &[&request_id,&ts])?;
        let pending:i64=tx.query_one("SELECT COUNT(*)::bigint FROM gemini_batch_items WHERE job_id=$1 AND state NOT IN ('succeeded','failed','indeterminate','canceled')", &[&job_id])?.get(0);
        if pending==0{tx.execute("UPDATE gemini_batch_jobs SET completed_ts=COALESCE(completed_ts,$2),result_expiration_ts=COALESCE(result_expiration_ts,$2+$3),update_ts=$2 WHERE job_id=$1", &[&job_id,&ts,&BATCH_RESULT_RETENTION_SECS])?;}
        tx.commit()?;Ok(Some(balance))
    }

    pub fn drain_gemini_batch_settlements(&mut self,limit:usize)->Result<usize>{let ids:Vec<String>=self.client.query("SELECT request_id FROM gemini_batch_settlement_outbox WHERE state='pending' AND next_attempt_ts <= $1 ORDER BY created_ts LIMIT $2", &[&now(),&(limit.clamp(1,10000)as i64)])?.into_iter().map(|r|r.get(0)).collect();let mut n=0;for id in ids{if self.process_gemini_batch_settlement(&id)?.is_some(){n+=1}}Ok(n)}

    pub fn gemini_batch_delete(&mut self,account_id:&str,job_id:&str)->Result<bool>{let ts=now();Ok(self.client.execute("UPDATE gemini_batch_jobs j SET delete_ts=$3,update_ts=$3 WHERE account_id=$1 AND job_id=$2 AND completed_ts IS NOT NULL AND NOT EXISTS(SELECT 1 FROM gemini_batch_items i WHERE i.job_id=j.job_id AND i.state NOT IN ('succeeded','failed','indeterminate','canceled')) AND NOT EXISTS(SELECT 1 FROM gemini_batch_settlement_outbox o WHERE o.job_id=j.job_id AND o.state<>'done')", &[&account_id,&job_id,&ts])?==1)}

    pub fn prune_gemini_batch(&mut self,older_than:i64,limit:usize)->Result<GeminiBatchPruneReport>{let lim=limit.clamp(1,MAX_BATCH_PRUNE_LIMIT)as i64;let mut tx=self.client.transaction()?;let blobs=tx.execute("DELETE FROM gemini_batch_blobs WHERE (job_id,item_index,kind) IN (SELECT b.job_id,b.item_index,b.kind FROM gemini_batch_blobs b JOIN gemini_batch_jobs j USING(job_id) WHERE b.retention_ts<$1 AND j.completed_ts IS NOT NULL ORDER BY b.retention_ts LIMIT $2)", &[&older_than,&lim])? as usize;let chunks=tx.execute("DELETE FROM gemini_batch_file_chunks WHERE file_id IN(SELECT f.file_id FROM gemini_batch_files f WHERE f.expiration_ts<$1 AND NOT EXISTS(SELECT 1 FROM gemini_batch_item_files r WHERE r.file_id=f.file_id) LIMIT $2)", &[&older_than,&lim])? as usize;let files=tx.execute("DELETE FROM gemini_batch_files f WHERE f.expiration_ts<$1 AND NOT EXISTS(SELECT 1 FROM gemini_batch_file_chunks c WHERE c.file_id=f.file_id) AND NOT EXISTS(SELECT 1 FROM gemini_batch_item_files r WHERE r.file_id=f.file_id) AND NOT EXISTS(SELECT 1 FROM gemini_batch_jobs j WHERE j.input_file_id=f.file_id OR j.output_file_id=f.file_id)", &[&older_than])? as usize;tx.commit()?;Ok(GeminiBatchPruneReport{blobs,chunks,files,items:0,jobs:0})}
}
