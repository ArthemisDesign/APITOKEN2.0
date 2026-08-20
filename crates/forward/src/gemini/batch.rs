//! Default-off durable Gemini Batch scheduler core.
//!
//! Public HTTP admission is deliberately absent. Starting this runtime is an explicit composition
//! decision; otherwise it owns no task, route, discovery method, or interactive resource.

use super::{
    prepare_nonstream_generate_request, ActualSendObserver, GeminiBatchAuthority,
    GeminiBatchBlobIdentity, GeminiBatchDataKeyring, GeminiGateway,
};
use anyhow::{bail, Context, Result};
use registry::{
    GeminiBatchClaimedItem, GeminiBatchItemState, GeminiBatchOperationalReport,
    GeminiBatchSettlementDisposition, GeminiBatchSettlementIntent, GeminiBatchTerminalClass,
    GeminiBatchUsage,
};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{sync::Notify, time::Instant};

#[derive(Clone, Debug)]
pub struct GeminiBatchRuntimeConfig {
    pub enabled: bool,
    pub leader_ttl_secs: i64,
    pub claim_lease_secs: i64,
    pub idle_backoff: Duration,
    pub retry_backoff: Duration,
}

impl Default for GeminiBatchRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            leader_ttl_secs: 30,
            claim_lease_secs: 120,
            idle_backoff: Duration::from_secs(1),
            retry_backoff: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeminiBatchOperationalWindowSnapshot {
    pub window: String,
    pub jobs_created: u64,
    pub items_created: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub canceled: u64,
    pub indeterminate: u64,
    pub settled_nano: u64,
    pub avg_queue_wait_seconds: Option<f64>,
    pub avg_execution_seconds: Option<f64>,
    pub throughput_items_per_hour: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeminiBatchOperationalSnapshot {
    pub authority_available: bool,
    pub queued_jobs: u64,
    pub running_jobs: u64,
    pub queued_items: u64,
    pub claimed_items: u64,
    pub dispatching_items: u64,
    pub settlement_pending_items: u64,
    pub succeeded_items: u64,
    pub failed_items: u64,
    pub canceled_items: u64,
    pub indeterminate_items: u64,
    pub oldest_queued_age_seconds: u64,
    pub reserved_hold_nano: u64,
    pub leader_held: bool,
    pub leader_expires_at: Option<i64>,
    pub headroom_stops: u64,
    pub settlement_pending: u64,
    pub settlement_failed: u64,
    pub settlement_oldest_age_seconds: u64,
    pub settlement_retries: u64,
    pub active_file_bytes: u64,
    pub active_file_chunks: u64,
    pub windows: Vec<GeminiBatchOperationalWindowSnapshot>,
}

impl GeminiBatchOperationalSnapshot {
    fn from_report(report: GeminiBatchOperationalReport, headroom_stops: u64) -> Self {
        let nonnegative = |value: i64| u64::try_from(value.max(0)).unwrap_or(u64::MAX);
        let windows = report
            .windows
            .into_iter()
            .map(|window| GeminiBatchOperationalWindowSnapshot {
                window: window.window,
                jobs_created: nonnegative(window.jobs_created),
                items_created: nonnegative(window.items_created),
                succeeded: nonnegative(window.succeeded),
                failed: nonnegative(window.failed),
                canceled: nonnegative(window.canceled),
                indeterminate: nonnegative(window.indeterminate),
                settled_nano: nonnegative(window.settled_nano),
                avg_queue_wait_seconds: window.avg_queue_wait_seconds,
                avg_execution_seconds: window.avg_execution_seconds,
                throughput_items_per_hour: window.throughput_items_per_hour.max(0.0),
            })
            .collect();
        Self {
            authority_available: true,
            queued_jobs: nonnegative(report.queued_jobs),
            running_jobs: nonnegative(report.running_jobs),
            queued_items: nonnegative(report.queued_items),
            claimed_items: nonnegative(report.claimed_items),
            dispatching_items: nonnegative(report.dispatching_items),
            settlement_pending_items: nonnegative(report.settlement_pending_items),
            succeeded_items: nonnegative(report.succeeded_items),
            failed_items: nonnegative(report.failed_items),
            canceled_items: nonnegative(report.canceled_items),
            indeterminate_items: nonnegative(report.indeterminate_items),
            oldest_queued_age_seconds: nonnegative(report.oldest_queued_age_seconds),
            reserved_hold_nano: nonnegative(report.reserved_hold_nano),
            leader_held: report.leader_held,
            leader_expires_at: report.leader_expires_at,
            headroom_stops,
            settlement_pending: nonnegative(report.settlement_pending),
            settlement_failed: nonnegative(report.settlement_failed),
            settlement_oldest_age_seconds: nonnegative(report.settlement_oldest_age_seconds),
            settlement_retries: nonnegative(report.settlement_retries),
            active_file_bytes: nonnegative(report.active_file_bytes),
            active_file_chunks: nonnegative(report.active_file_chunks),
            windows,
        }
    }
}

pub struct GeminiBatchRuntime {
    config: GeminiBatchRuntimeConfig,
    authority: GeminiBatchAuthority,
    gateway: Arc<GeminiGateway>,
    keys: Arc<GeminiBatchDataKeyring>,
    accepting: AtomicBool,
    active: AtomicUsize,
    headroom_stops: AtomicU64,
    notify: Notify,
}

impl std::fmt::Debug for GeminiBatchRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiBatchRuntime")
            .field("enabled", &self.config.enabled)
            .field("active", &self.active.load(Ordering::Acquire))
            .field("secrets", &"REDACTED")
            .finish()
    }
}

impl GeminiBatchRuntime {
    pub fn new(
        config: GeminiBatchRuntimeConfig,
        authority: GeminiBatchAuthority,
        gateway: Arc<GeminiGateway>,
        keys: Arc<GeminiBatchDataKeyring>,
    ) -> Result<Arc<Self>> {
        if !config.enabled {
            bail!("Gemini Batch runtime is disabled")
        }
        if config.leader_ttl_secs <= 0 || config.claim_lease_secs <= 0 {
            bail!("invalid Gemini Batch runtime bounds")
        }
        Ok(Arc::new(Self {
            config,
            authority,
            gateway,
            keys,
            accepting: AtomicBool::new(true),
            active: AtomicUsize::new(0),
            headroom_stops: AtomicU64::new(0),
            notify: Notify::new(),
        }))
    }

    pub fn spawn(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let this = Arc::clone(self);
        tokio::spawn(async move { this.run().await })
    }

    pub async fn operational_snapshot(&self) -> GeminiBatchOperationalSnapshot {
        let headroom_stops = self.headroom_stops.load(Ordering::Relaxed);
        self.authority
            .operational_report()
            .await
            .map(|report| GeminiBatchOperationalSnapshot::from_report(report, headroom_stops))
            .unwrap_or(GeminiBatchOperationalSnapshot {
                headroom_stops,
                ..GeminiBatchOperationalSnapshot::default()
            })
    }

    async fn run(self: Arc<Self>) {
        while self.accepting.load(Ordering::Acquire) {
            if !self
                .authority
                .acquire_leader(self.config.leader_ttl_secs)
                .await
                .unwrap_or(false)
            {
                tokio::time::sleep(self.config.idle_backoff).await;
                continue;
            }
            let _ = self.authority.drain_settlements(64).await;
            if let Ok(report) = self.authority.reconcile(64).await {
                for recovery in report.recovery_candidates {
                    let completed = pool::now();
                    let identity = GeminiBatchBlobIdentity {
                        account_id: &recovery.account_id,
                        job_id: &recovery.job_id,
                        item_index: recovery.item_index,
                        kind: "error",
                        schema_version: 1,
                    };
                    if let Ok(blob) = self.keys.encrypt_blob(
                        &identity,
                        b"{\"error\":{\"code\":500,\"message\":\"execution outcome indeterminate\"}}",
                        completed + 42 * 24 * 3600,
                    ) {
                        let intent = GeminiBatchSettlementIntent {
                            job_id: recovery.job_id.clone(),
                            item_index: recovery.item_index,
                            request_id: recovery.request_id.clone(),
                            claim_generation: recovery.claim_generation,
                            disposition: recovery.disposition,
                            actual_nano: 0,
                            charge_basis_nano: 0,
                            real_nano: 0,
                            usage: None,
                            result_blob: blob,
                            terminal_state: recovery.terminal_state,
                            terminal_class: recovery.terminal_class,
                            calibration: None,
                            completed_ts: completed,
                        };
                        let request_id = recovery.request_id.clone();
                        if self
                            .authority
                            .enqueue_recovery_settlement(recovery, intent)
                            .await
                            .is_ok()
                        {
                            let _ = self.authority.process_settlement(request_id).await;
                        }
                    }
                }
            }
            let mut started = false;
            for model_id in self
                .gateway
                .config()
                .models
                .iter()
                .filter(|model| !model.is_image_generation())
                .map(|model| model.id.clone())
            {
                if !self.accepting.load(Ordering::Acquire) {
                    break;
                }
                let selection = self.gateway.select_batch(&model_id, &HashSet::new());
                let Some(lease) = selection.lease else {
                    if selection.reason() == Some("batch_5h_headroom_stop") {
                        self.headroom_stops.fetch_add(1, Ordering::Relaxed);
                    }
                    continue;
                };
                let profile = lease.profile_id().to_owned();
                let profile_capacity = lease.batch_profile_capacity();
                drop(lease);
                let Ok(Some(item)) = self
                    .authority
                    .claim(
                        profile,
                        model_id,
                        profile_capacity,
                        self.config.claim_lease_secs,
                    )
                    .await
                else {
                    continue;
                };
                started = true;
                let this = Arc::clone(&self);
                tokio::spawn(async move {
                    this.active.fetch_add(1, Ordering::AcqRel);
                    if let Err(error) = this.execute_item(item).await {
                        elog::warn(
                            "gemini-batch",
                            format!("batch worker failed before terminal apply: {error:#}"),
                        );
                    }
                    if this.active.fetch_sub(1, Ordering::AcqRel) == 1 {
                        this.notify.notify_waiters();
                    }
                });
            }
            if !started {
                tokio::time::sleep(self.config.idle_backoff).await
            }
        }
    }

    async fn execute_item(&self, item: GeminiBatchClaimedItem) -> Result<()> {
        let identity = GeminiBatchBlobIdentity {
            account_id: &item.claim.account_id,
            job_id: &item.claim.job_id,
            item_index: item.claim.item_index,
            kind: "request",
            schema_version: 1,
        };
        let plain = self.keys.decrypt_blob(&identity, &item.request_blob)?;
        let value: serde_json::Value =
            serde_json::from_slice(&plain).context("decode encrypted batch request")?;
        let Some(lease) = self
            .gateway
            .select_batch_profile(&item.public_model, &item.claim.profile_id)
        else {
            self.authority
                .requeue(
                    item.claim,
                    pool::now() + self.config.retry_backoff.as_secs() as i64,
                )
                .await?;
            return Ok(());
        };
        if !self
            .authority
            .mark_dispatching(item.claim.clone(), self.config.claim_lease_secs)
            .await?
        {
            return Ok(());
        }
        let renewing = Arc::new(AtomicBool::new(true));
        let renewal = {
            let authority = self.authority.clone();
            let claim = item.claim.clone();
            let renewing = Arc::clone(&renewing);
            let lease = self.config.claim_lease_secs;
            tokio::spawn(async move {
                while renewing.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_secs((lease / 3).max(1) as u64)).await;
                    if renewing.load(Ordering::Acquire) {
                        let _ = authority.renew(claim.clone(), lease).await;
                    }
                }
            })
        };
        loop {
            let mut random = [0u8; 4];
            getrandom::fill(&mut random).context("Gemini Batch dispatch CSPRNG unavailable")?;
            let span = (registry::GEMINI_BATCH_DISPATCH_DELAY_MAX_MS
                - registry::GEMINI_BATCH_DISPATCH_DELAY_MIN_MS
                + 1) as u32;
            let delay = registry::GEMINI_BATCH_DISPATCH_DELAY_MIN_MS
                + i64::from(u32::from_be_bytes(random) % span);
            match self
                .authority
                .reserve_dispatch(item.claim.clone(), delay)
                .await?
            {
                registry::GeminiBatchDispatchReservation::Granted { .. } => break,
                registry::GeminiBatchDispatchReservation::WaitUntil { not_before_ms } => {
                    let now_ms = pool::now().saturating_mul(1_000);
                    let wait_ms = not_before_ms.saturating_sub(now_ms).max(1) as u64;
                    tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                }
                registry::GeminiBatchDispatchReservation::Stale => return Ok(()),
            }
        }
        let send_observer = ActualSendObserver::acknowledged();
        let model = self
            .gateway
            .config()
            .model(&item.public_model)
            .context("batch model unavailable")?
            .clone();
        let request = prepare_nonstream_generate_request(
            &model,
            item.public_model.clone(),
            value,
            None,
            item.claim.request_id.clone(),
            item.claim.request_id.clone(),
        );
        let execution_future = super::api::execute_nonstream_generate_observed(
            &self.gateway,
            &lease,
            &model,
            &request,
            Some(send_observer.clone()),
        );
        tokio::pin!(execution_future);
        let mut fence_error = None;
        let execution = tokio::select! {
            result = &mut execution_future => Some(result),
            _ = send_observer.observed() => {
                let marked = self
                    .authority
                    .mark_actual_send(item.claim.clone(), self.config.claim_lease_secs)
                    .await;
                // The shared helper reader is blocked on this acknowledgement. Release it on every
                // authority outcome before returning, otherwise one Batch fence failure can wedge
                // ordinary Gemini traffic on the same profile.
                send_observer.acknowledge();
                match marked {
                    Ok(true) => Some(execution_future.await),
                    Ok(false) => {
                        fence_error = Some(anyhow::anyhow!("Gemini Batch actual-send claim became stale"));
                        None
                    }
                    Err(error) => {
                        fence_error = Some(error.context("persist Gemini Batch actual-send boundary"));
                        None
                    }
                }
            }
        };
        renewing.store(false, Ordering::Release);
        renewal.abort();
        if let Some(error) = fence_error {
            return Err(error);
        }
        let execution = execution.context("Gemini Batch execution result is missing")?;
        match execution {
            Ok(raw) if raw.status.is_success() && raw.usage.is_some() => {
                let usage = raw.usage.unwrap();
                let completed = pool::now();
                let result_identity = GeminiBatchBlobIdentity {
                    kind: "result",
                    ..identity
                };
                let result = self.keys.encrypt_blob(
                    &result_identity,
                    &raw.body,
                    completed + 42 * 24 * 3600,
                )?;
                let prices = crate::pricing::tariff_book::snapshot()
                    .version_payload(&item.tariff_family, item.tariff_version)
                    .as_ref()
                    .and_then(crate::pricing::tariff_book::as_gemini)
                    .or_else(|| {
                        (item.tariff_version == 1).then(|| {
                            metering::gemini_prices_at(&model.id, item.priced_ts)
                                .unwrap_or(model.prices)
                        })
                    })
                    .context("pinned Gemini Batch tariff unavailable")?;
                let (actual, event) = super::billing::settled_charge_with_prices(
                    &model,
                    &usage,
                    item.hold_nano,
                    item.payable_multiplier_bp,
                    item.priced_ts,
                    prices,
                );
                let calibration = super::billing::gemini_calibration_event_with_prices(
                    &item.claim.request_id,
                    &item.claim.profile_id,
                    &model,
                    &usage,
                    item.priced_ts,
                    completed,
                    prices,
                    Some(item.tariff_schedule_id.clone()),
                )
                .context("batch calibration event missing")?;
                let intent = GeminiBatchSettlementIntent {
                    job_id: item.claim.job_id.clone(),
                    item_index: item.claim.item_index,
                    request_id: item.claim.request_id.clone(),
                    claim_generation: item.claim.claim_generation,
                    disposition: GeminiBatchSettlementDisposition::Settle,
                    actual_nano: actual,
                    charge_basis_nano: event.as_ref().map_or(0, |event| event.charge_basis_nano),
                    real_nano: event.as_ref().map_or(0, |event| event.real_nano),
                    usage: Some(GeminiBatchUsage {
                        input_tokens: usage.input_tokens as i64,
                        tool_prompt_tokens: usage.tool_prompt_tokens as i64,
                        audio_input_tokens: usage.audio_input_tokens as i64,
                        cached_input_tokens: usage.cached_input_tokens as i64,
                        cached_audio_input_tokens: usage.cached_audio_input_tokens as i64,
                        output_tokens: usage.output_tokens as i64,
                        thinking_output_tokens: usage.thinking_output_tokens as i64,
                        image_output_tokens: usage.image_output_tokens as i64,
                        search_queries: usage.search_queries as i64,
                        grounded_search_prompts: usage.grounded_search_prompts as i64,
                    }),
                    result_blob: result,
                    terminal_state: GeminiBatchItemState::Succeeded,
                    terminal_class: GeminiBatchTerminalClass::Success,
                    calibration: Some(calibration),
                    completed_ts: completed,
                };
                self.authority
                    .enqueue_live_settlement(item.claim.clone(), intent)
                    .await?;
                let _ = self
                    .authority
                    .process_settlement(item.claim.request_id)
                    .await?;
            }
            Ok(raw) => {
                self.settle_indeterminate(&item, identity, &raw.body)
                    .await?;
            }
            Err(_) => {
                self.settle_indeterminate(
                    &item,
                    identity,
                    b"{\"error\":{\"code\":500,\"message\":\"execution outcome indeterminate\"}}",
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn settle_indeterminate(
        &self,
        item: &GeminiBatchClaimedItem,
        identity: GeminiBatchBlobIdentity<'_>,
        body: &[u8],
    ) -> Result<()> {
        let completed = pool::now();
        let error_identity = GeminiBatchBlobIdentity {
            kind: "error",
            ..identity
        };
        let blob = self
            .keys
            .encrypt_blob(&error_identity, body, completed + 42 * 24 * 3600)?;
        let intent = GeminiBatchSettlementIntent {
            job_id: item.claim.job_id.clone(),
            item_index: item.claim.item_index,
            request_id: item.claim.request_id.clone(),
            claim_generation: item.claim.claim_generation,
            disposition: GeminiBatchSettlementDisposition::Indeterminate,
            actual_nano: crate::settlement_policy::unknown_usage_charge(item.hold_nano),
            charge_basis_nano: 0,
            real_nano: 0,
            usage: None,
            result_blob: blob,
            terminal_state: GeminiBatchItemState::Indeterminate,
            terminal_class: GeminiBatchTerminalClass::Indeterminate,
            calibration: None,
            completed_ts: completed,
        };
        self.authority
            .enqueue_live_settlement(item.claim.clone(), intent)
            .await?;
        let _ = self
            .authority
            .process_settlement(item.claim.request_id.clone())
            .await?;
        Ok(())
    }

    pub async fn shutdown(&self, deadline: Instant) -> Result<()> {
        self.accepting.store(false, Ordering::Release);
        while self.active.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now());
            let _ = tokio::time::timeout(remain, self.notify.notified()).await;
        }
        let _ = self.authority.drain_settlements(1024).await?;
        self.authority.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_snapshot_clamps_authority_values_and_keeps_fixed_shape() {
        let snapshot = GeminiBatchOperationalSnapshot::from_report(
            GeminiBatchOperationalReport {
                queued_jobs: 1,
                running_jobs: 2,
                queued_items: 3,
                claimed_items: 1,
                dispatching_items: 1,
                settlement_pending_items: 0,
                succeeded_items: 7,
                failed_items: 1,
                canceled_items: 2,
                indeterminate_items: 4,
                oldest_queued_age_seconds: -1,
                reserved_hold_nano: 99,
                leader_held: true,
                leader_expires_at: Some(123),
                settlement_pending: 5,
                settlement_failed: 1,
                settlement_oldest_age_seconds: 11,
                settlement_retries: 6,
                active_file_bytes: 1024,
                active_file_chunks: 8,
                windows: vec![registry::GeminiBatchOperationalWindow {
                    window: "1h".into(),
                    jobs_created: 1,
                    items_created: 2,
                    succeeded: 1,
                    failed: -1,
                    canceled: 0,
                    indeterminate: 0,
                    settled_nano: 50,
                    avg_queue_wait_seconds: Some(2.5),
                    avg_execution_seconds: Some(3.5),
                    throughput_items_per_hour: 1.0,
                }],
            },
            9,
        );
        assert!(snapshot.authority_available);
        assert_eq!(snapshot.queued_items, 3);
        assert_eq!(snapshot.oldest_queued_age_seconds, 0);
        assert_eq!(snapshot.headroom_stops, 9);
        assert_eq!(snapshot.active_file_chunks, 8);
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].failed, 0);
        assert_eq!(snapshot.reserved_hold_nano, 99);
    }
}
