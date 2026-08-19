//! PostgreSQL-only Gemini Batch scheduler and worker claim lifecycle.

use super::{now, Owner, PgStore};
use crate::gemini_batch::{GeminiBatchClaim, GEMINI_BATCH_DISPATCH_LEADER};
use anyhow::{bail, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeminiBatchReconcileReport {
    pub requeued_before_dispatch: usize,
    pub indeterminate_after_dispatch: usize,
}

impl PgStore {
    /// Acquire or renew the one fleet-wide Gemini Batch dispatch leader lease.
    pub fn acquire_gemini_batch_leader(&mut self, owner: &Owner, ttl_secs: i64) -> Result<bool> {
        self.acquire_leader(owner, GEMINI_BATCH_DISPATCH_LEADER, ttl_secs)
    }

    /// Claim one queued item and one profile in the same transaction.
    ///
    /// The supplied profile is opaque to registry. An expired profile lease is reusable only when
    /// its exact previous owner generation no longer has a live `engine_instances` heartbeat.
    pub fn claim_gemini_batch_item(
        &mut self,
        owner: &Owner,
        profile_id: &str,
        lease_secs: i64,
    ) -> Result<Option<GeminiBatchClaim>> {
        if profile_id.is_empty() {
            bail!("Gemini Batch profile ID is empty");
        }
        let ts = now();
        let lease_until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        Self::assert_owner(&mut tx, owner, ts)?;

        // Serialize contenders for this profile without a separate coordination authority.
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 552966749))",
            &[&profile_id],
        )?;
        Self::assert_owner_locked(&mut tx, owner, now())?;

        let profile_available = tx
            .query_opt(
                "SELECT 1
                   FROM gemini_batch_profile_leases lease
              LEFT JOIN engine_instances instance
                     ON instance.instance_id=lease.worker_instance
                    AND instance.owner_epoch=lease.worker_epoch
                  WHERE lease.profile_id=$1
                    AND NOT (
                        lease.worker_instance=$2
                        AND lease.worker_epoch=$3
                        AND lease.job_id IN (
                            SELECT item.job_id
                              FROM gemini_batch_items item
                             WHERE item.job_id=lease.job_id
                               AND item.item_index=lease.item_index
                               AND item.worker_instance=$2
                               AND item.worker_epoch=$3
                               AND item.claim_generation=lease.claim_generation
                               AND item.state IN ('claimed','dispatching','settlement_pending')
                        )
                    )
                    AND (instance.instance_id IS NULL OR instance.lease_until < $4)
                  FOR UPDATE OF lease",
                &[&profile_id, &owner.instance_id, &owner.epoch, &ts],
            )?
            .is_some();
        let profile_exists = tx
            .query_opt(
                "SELECT 1 FROM gemini_batch_profile_leases WHERE profile_id=$1",
                &[&profile_id],
            )?
            .is_some();
        if profile_exists && !profile_available {
            tx.rollback()?;
            return Ok(None);
        }

        if profile_exists {
            tx.execute(
                "DELETE FROM gemini_batch_profile_leases WHERE profile_id=$1",
                &[&profile_id],
            )?;
        }

        // The row lock and SKIP LOCKED make claims concurrent; the account/job round-robin key
        // prevents one large old batch from monopolizing every claim.
        let Some(row) = tx.query_opt(
            "SELECT item.job_id,job.account_id,item.item_index,item.request_id,
                    item.claim_generation+1
               FROM gemini_batch_items item
               JOIN gemini_batch_jobs job ON job.job_id=item.job_id
              WHERE item.state='queued'
                AND item.next_attempt_ts <= $1
                AND job.cancel_requested_ts IS NULL
                AND job.completed_ts IS NULL
                AND job.delete_ts IS NULL
                AND job.deadline_ts > $1
                AND NOT EXISTS (
                    SELECT 1 FROM gemini_batch_items active
                     WHERE active.job_id=item.job_id
                       AND active.state IN ('claimed','dispatching','settlement_pending')
                )
              ORDER BY job.priority DESC,
                       COALESCE((
                           SELECT MAX(active.updated_ts)
                             FROM gemini_batch_items active
                            WHERE active.job_id=item.job_id
                              AND active.state IN ('claimed','dispatching','settlement_pending')
                       ),0),
                       job.create_ts,item.item_index
              FOR UPDATE OF item SKIP LOCKED
              LIMIT 1",
            &[&ts],
        )?
        else {
            tx.rollback()?;
            return Ok(None);
        };
        let job_id: String = row.get(0);
        let account_id: String = row.get(1);
        let item_index: i64 = row.get(2);
        let request_id: String = row.get(3);
        let claim_generation: i64 = row.get(4);

        Self::assert_owner_locked(&mut tx, owner, now())?;
        let changed = tx.execute(
            "UPDATE gemini_batch_items
                SET state='claimed',worker_instance=$3,worker_epoch=$4,
                    claim_generation=$5,lease_until=$6,selected_profile_id=$7,
                    updated_ts=$8
              WHERE job_id=$1 AND item_index=$2 AND state='queued'",
            &[
                &job_id,
                &item_index,
                &owner.instance_id,
                &owner.epoch,
                &claim_generation,
                &lease_until,
                &profile_id,
                &ts,
            ],
        )?;
        if changed != 1 {
            bail!("Gemini Batch item claim lost its locked row");
        }
        tx.execute(
            "INSERT INTO gemini_batch_profile_leases(
                 profile_id,job_id,item_index,worker_instance,worker_epoch,
                 claim_generation,lease_until,created_ts,updated_ts)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$8)",
            &[
                &profile_id,
                &job_id,
                &item_index,
                &owner.instance_id,
                &owner.epoch,
                &claim_generation,
                &lease_until,
                &ts,
            ],
        )?;
        Self::assert_owner_locked(&mut tx, owner, now())?;
        tx.commit()?;
        Ok(Some(GeminiBatchClaim {
            job_id,
            account_id,
            item_index,
            request_id,
            claim_generation,
            lease_until,
            profile_id: profile_id.to_owned(),
        }))
    }

    /// Durably record dispatch intent before transport is allowed to start.
    pub fn mark_gemini_batch_dispatching(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchClaim,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let lease_until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        Self::assert_owner_locked(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "UPDATE gemini_batch_items item
                SET state='dispatching',dispatch_intent_ts=COALESCE(dispatch_intent_ts,$7),
                    actual_send_evidence=COALESCE(actual_send_evidence,'not_sent'),
                    attempt_count=attempt_count+CASE WHEN dispatch_intent_ts IS NULL THEN 1 ELSE 0 END,
                    lease_until=$8,updated_ts=$7
              WHERE item.job_id=$1 AND item.item_index=$2 AND item.request_id=$3
                AND item.worker_instance=$4 AND item.worker_epoch=$5
                AND item.claim_generation=$6 AND item.selected_profile_id=$9
                AND item.state IN ('claimed','dispatching')
                AND EXISTS (
                    SELECT 1 FROM gemini_batch_profile_leases lease
                     WHERE lease.profile_id=$9 AND lease.job_id=$1 AND lease.item_index=$2
                       AND lease.worker_instance=$4 AND lease.worker_epoch=$5
                       AND lease.claim_generation=$6
                )",
            &[
                &claim.job_id,
                &claim.item_index,
                &claim.request_id,
                &owner.instance_id,
                &owner.epoch,
                &claim.claim_generation,
                &ts,
                &lease_until,
                &claim.profile_id,
            ],
        )?;
        if changed == 1 {
            tx.execute(
                "UPDATE gemini_batch_profile_leases
                    SET lease_until=$7,updated_ts=$8
                  WHERE profile_id=$1 AND job_id=$2 AND item_index=$3
                    AND worker_instance=$4 AND worker_epoch=$5 AND claim_generation=$6",
                &[
                    &claim.profile_id,
                    &claim.job_id,
                    &claim.item_index,
                    &owner.instance_id,
                    &owner.epoch,
                    &claim.claim_generation,
                    &lease_until,
                    &ts,
                ],
            )?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Persist the irreversible actual-send boundary. A reconciler will never replay this claim.
    pub fn mark_gemini_batch_actual_send(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchClaim,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let lease_until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        Self::assert_owner_locked(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "UPDATE gemini_batch_items item
                SET actual_send_ts=COALESCE(actual_send_ts,$7),actual_send_evidence='sent',
                    lease_until=$8,updated_ts=$7
              WHERE item.job_id=$1 AND item.item_index=$2 AND item.request_id=$3
                AND item.worker_instance=$4 AND item.worker_epoch=$5
                AND item.claim_generation=$6 AND item.selected_profile_id=$9
                AND item.state='dispatching' AND item.dispatch_intent_ts IS NOT NULL
                AND EXISTS (
                    SELECT 1 FROM gemini_batch_profile_leases lease
                     WHERE lease.profile_id=$9 AND lease.job_id=$1 AND lease.item_index=$2
                       AND lease.worker_instance=$4 AND lease.worker_epoch=$5
                       AND lease.claim_generation=$6
                )",
            &[
                &claim.job_id,
                &claim.item_index,
                &claim.request_id,
                &owner.instance_id,
                &owner.epoch,
                &claim.claim_generation,
                &ts,
                &lease_until,
                &claim.profile_id,
            ],
        )?;
        if changed == 1 {
            tx.execute(
                "UPDATE gemini_batch_profile_leases
                    SET lease_until=$7,updated_ts=$8
                  WHERE profile_id=$1 AND job_id=$2 AND item_index=$3
                    AND worker_instance=$4 AND worker_epoch=$5 AND claim_generation=$6",
                &[
                    &claim.profile_id,
                    &claim.job_id,
                    &claim.item_index,
                    &owner.instance_id,
                    &owner.epoch,
                    &claim.claim_generation,
                    &lease_until,
                    &ts,
                ],
            )?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Renew the item and profile leases under the complete owner and claim-generation fence.
    pub fn renew_gemini_batch_claim(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchClaim,
        lease_secs: i64,
    ) -> Result<bool> {
        let ts = now();
        let lease_until = ts.saturating_add(lease_secs.max(1));
        let mut tx = self.client.transaction()?;
        Self::assert_owner_locked(&mut tx, owner, ts)?;
        let profile_changed = tx.execute(
            "UPDATE gemini_batch_profile_leases
                SET lease_until=$7,updated_ts=$8
              WHERE profile_id=$1 AND job_id=$2 AND item_index=$3
                AND worker_instance=$4 AND worker_epoch=$5 AND claim_generation=$6",
            &[
                &claim.profile_id,
                &claim.job_id,
                &claim.item_index,
                &owner.instance_id,
                &owner.epoch,
                &claim.claim_generation,
                &lease_until,
                &ts,
            ],
        )?;
        if profile_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        let item_changed = tx.execute(
            "UPDATE gemini_batch_items
                SET lease_until=$7,updated_ts=$8
              WHERE job_id=$1 AND item_index=$2 AND request_id=$3
                AND worker_instance=$4 AND worker_epoch=$5 AND claim_generation=$6
                AND selected_profile_id=$9
                AND state IN ('claimed','dispatching','settlement_pending')",
            &[
                &claim.job_id,
                &claim.item_index,
                &claim.request_id,
                &owner.instance_id,
                &owner.epoch,
                &claim.claim_generation,
                &lease_until,
                &ts,
                &claim.profile_id,
            ],
        )?;
        if item_changed != 1 {
            tx.rollback()?;
            return Ok(false);
        }
        tx.commit()?;
        Ok(true)
    }

    /// Release a claim that provably never reached dispatch intent, optionally delaying retry.
    pub fn requeue_gemini_batch_claim(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchClaim,
        next_attempt_ts: i64,
    ) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner_locked(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "UPDATE gemini_batch_items
                SET state='queued',next_attempt_ts=GREATEST($7,0),worker_instance=NULL,
                    worker_epoch=NULL,lease_until=NULL,selected_profile_id=NULL,updated_ts=$8
              WHERE job_id=$1 AND item_index=$2 AND request_id=$3
                AND worker_instance=$4 AND worker_epoch=$5 AND claim_generation=$6
                AND selected_profile_id=$9 AND state='claimed'
                AND dispatch_intent_ts IS NULL AND actual_send_ts IS NULL
                AND actual_send_evidence IS NULL",
            &[
                &claim.job_id,
                &claim.item_index,
                &claim.request_id,
                &owner.instance_id,
                &owner.epoch,
                &claim.claim_generation,
                &next_attempt_ts,
                &ts,
                &claim.profile_id,
            ],
        )?;
        if changed == 1 {
            tx.execute(
                "DELETE FROM gemini_batch_profile_leases
                  WHERE profile_id=$1 AND job_id=$2 AND item_index=$3
                    AND worker_instance=$4 AND worker_epoch=$5 AND claim_generation=$6",
                &[
                    &claim.profile_id,
                    &claim.job_id,
                    &claim.item_index,
                    &owner.instance_id,
                    &owner.epoch,
                    &claim.claim_generation,
                ],
            )?;
        }
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Release only the profile lease after the item has moved beyond worker dispatch ownership.
    pub fn release_gemini_batch_profile(
        &mut self,
        owner: &Owner,
        claim: &GeminiBatchClaim,
    ) -> Result<bool> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        Self::assert_owner_locked(&mut tx, owner, ts)?;
        let changed = tx.execute(
            "DELETE FROM gemini_batch_profile_leases lease
              WHERE lease.profile_id=$1 AND lease.job_id=$2 AND lease.item_index=$3
                AND lease.worker_instance=$4 AND lease.worker_epoch=$5
                AND lease.claim_generation=$6
                AND EXISTS (
                    SELECT 1 FROM gemini_batch_items item
                     WHERE item.job_id=$2 AND item.item_index=$3 AND item.request_id=$7
                       AND item.worker_instance=$4 AND item.worker_epoch=$5
                       AND item.claim_generation=$6 AND item.selected_profile_id=$1
                       AND item.state IN ('settlement_pending','succeeded','failed','indeterminate','canceled')
                )",
            &[
                &claim.profile_id,
                &claim.job_id,
                &claim.item_index,
                &owner.instance_id,
                &owner.epoch,
                &claim.claim_generation,
                &claim.request_id,
            ],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    /// Reconcile expired claims only after the exact owner heartbeat is dead.
    ///
    /// Pre-dispatch claims are safe to replay. Every dispatch-intent claim is terminalized as
    /// indeterminate, regardless of whether the actual-send observer fired, because intent creates
    /// an ambiguity window and ordinary Code Assist generation has no replay key.
    pub fn reconcile_expired_gemini_batch_claims(
        &mut self,
        limit: usize,
    ) -> Result<GeminiBatchReconcileReport> {
        let ts = now();
        let mut tx = self.client.transaction()?;
        let rows = tx.query(
            "SELECT item.job_id,item.item_index,item.state,item.dispatch_intent_ts,
                    item.actual_send_ts,item.actual_send_evidence
               FROM gemini_batch_items item
          LEFT JOIN engine_instances instance
                 ON instance.instance_id=item.worker_instance
                AND instance.owner_epoch=item.worker_epoch
              WHERE item.state IN ('claimed','dispatching')
                AND item.lease_until < $1
                AND (instance.instance_id IS NULL OR instance.lease_until < $1)
              ORDER BY item.updated_ts,item.job_id,item.item_index
              FOR UPDATE OF item SKIP LOCKED
              LIMIT $2",
            &[&ts, &(limit.clamp(1, 10_000) as i64)],
        )?;
        let mut report = GeminiBatchReconcileReport::default();
        for row in rows {
            let job_id: String = row.get(0);
            let item_index: i64 = row.get(1);
            let state: String = row.get(2);
            let dispatch_intent_ts: Option<i64> = row.get(3);
            let actual_send_ts: Option<i64> = row.get(4);
            let actual_send_evidence: Option<String> = row.get(5);
            let safe_before_dispatch = state == "claimed"
                && dispatch_intent_ts.is_none()
                && actual_send_ts.is_none()
                && actual_send_evidence.is_none();
            if safe_before_dispatch {
                tx.execute(
                    "UPDATE gemini_batch_items
                        SET state='queued',worker_instance=NULL,worker_epoch=NULL,lease_until=NULL,
                            selected_profile_id=NULL,updated_ts=$3
                      WHERE job_id=$1 AND item_index=$2",
                    &[&job_id, &item_index, &ts],
                )?;
                report.requeued_before_dispatch += 1;
            } else {
                tx.execute(
                    "UPDATE gemini_batch_items
                        SET state='indeterminate',terminal_class='indeterminate',
                            actual_send_evidence=CASE
                                WHEN actual_send_ts IS NOT NULL THEN 'sent'
                                ELSE 'ambiguous'
                            END,
                            lease_until=NULL,terminal_ts=$3,updated_ts=$3
                      WHERE job_id=$1 AND item_index=$2",
                    &[&job_id, &item_index, &ts],
                )?;
                report.indeterminate_after_dispatch += 1;
            }
            tx.execute(
                "DELETE FROM gemini_batch_profile_leases
                  WHERE job_id=$1 AND item_index=$2",
                &[&job_id, &item_index],
            )?;
        }
        tx.commit()?;
        Ok(report)
    }
}
