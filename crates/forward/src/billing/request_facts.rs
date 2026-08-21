//! Dormant request-fact transport owned by the billing facade.
//!
//! Billable lifecycle facts stay in the main `WriteCmd` money FIFO. This module owns only the
//! fail-open PostgreSQL terminal-at-insert inbox used by a later producer stage.

use registry::request_facts::{TerminalRequestFact, MAX_REQUEST_FACT_BATCH};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

pub const TERMINAL_REQUEST_FACT_QUEUE_CAPACITY: usize = 4_096;
const REQUEST_FACT_HEALTH_HEALTHY: u8 = 1;
const REQUEST_FACT_HEALTH_FAILED: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalRequestFactSubmission {
    Queued,
    Invalid,
    QueueFull,
    WriterClosed,
    UnsupportedAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestFactPersistenceHealth {
    Unknown,
    Healthy,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestFactDeliverySnapshot {
    pub enabled: bool,
    pub queue_depth: usize,
    pub accepted: u64,
    pub persisted: u64,
    pub deduplicated: u64,
    pub dropped_invalid: u64,
    pub dropped_full: u64,
    pub dropped_closed: u64,
    pub dropped_unsupported: u64,
    pub persistence_failed: u64,
    pub persistence_health: RequestFactPersistenceHealth,
    pub process_started_at: Option<i64>,
}

#[derive(Default)]
pub(super) struct RequestFactDeliveryState {
    accepted: AtomicU64,
    persisted: AtomicU64,
    deduplicated: AtomicU64,
    dropped_invalid: AtomicU64,
    dropped_full: AtomicU64,
    dropped_closed: AtomicU64,
    dropped_unsupported: AtomicU64,
    persistence_failed: AtomicU64,
    persistence_health: AtomicU8,
    process_started_at: i64,
}

pub(super) struct TerminalRequestFactInbox {
    sender: Option<mpsc::Sender<TerminalRequestFact>>,
    state: Arc<RequestFactDeliveryState>,
}

impl TerminalRequestFactInbox {
    pub(super) fn disabled() -> Self {
        Self {
            sender: None,
            state: Arc::new(RequestFactDeliveryState::default()),
        }
    }

    pub(super) fn start_postgres(url: String, retry_deadline: Duration) -> Self {
        let (sender, receiver) = mpsc::channel(TERMINAL_REQUEST_FACT_QUEUE_CAPACITY);
        let process_started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .unwrap_or(0);
        let state = Arc::new(RequestFactDeliveryState {
            process_started_at,
            ..RequestFactDeliveryState::default()
        });
        let worker_state = Arc::clone(&state);
        if let Err(error) = std::thread::Builder::new()
            .name("request-facts-pg-writer".into())
            .spawn(move || {
                terminal_request_fact_worker(url, retry_deadline, receiver, worker_state)
            })
        {
            // Analytics must not make authority startup fail. Dropping the receiver makes every
            // future nonblocking submission report WriterClosed with a fixed counter.
            elog::error(
                "billing",
                format!("request-fact writer thread could not start: {error}"),
            );
        }
        Self {
            sender: Some(sender),
            state,
        }
    }

    pub(super) fn submit(&self, fact: TerminalRequestFact) -> TerminalRequestFactSubmission {
        let Some(sender) = self.sender.as_ref() else {
            self.state
                .dropped_unsupported
                .fetch_add(1, Ordering::Relaxed);
            return TerminalRequestFactSubmission::UnsupportedAuthority;
        };
        if fact.validate().is_err() {
            self.state.dropped_invalid.fetch_add(1, Ordering::Relaxed);
            return TerminalRequestFactSubmission::Invalid;
        }
        match sender.try_send(fact) {
            Ok(()) => {
                self.state.accepted.fetch_add(1, Ordering::Relaxed);
                TerminalRequestFactSubmission::Queued
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.state.dropped_full.fetch_add(1, Ordering::Relaxed);
                TerminalRequestFactSubmission::QueueFull
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.state.dropped_closed.fetch_add(1, Ordering::Relaxed);
                TerminalRequestFactSubmission::WriterClosed
            }
        }
    }

    pub(super) fn snapshot(&self) -> RequestFactDeliverySnapshot {
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        let persistence_health = match self.state.persistence_health.load(Ordering::Relaxed) {
            REQUEST_FACT_HEALTH_HEALTHY => RequestFactPersistenceHealth::Healthy,
            REQUEST_FACT_HEALTH_FAILED => RequestFactPersistenceHealth::Failed,
            _ => RequestFactPersistenceHealth::Unknown,
        };
        RequestFactDeliverySnapshot {
            enabled: self.sender.is_some(),
            queue_depth: self
                .sender
                .as_ref()
                .map(super::channel_queue_depth)
                .unwrap_or(0),
            accepted: load(&self.state.accepted),
            persisted: load(&self.state.persisted),
            deduplicated: load(&self.state.deduplicated),
            dropped_invalid: load(&self.state.dropped_invalid),
            dropped_full: load(&self.state.dropped_full),
            dropped_closed: load(&self.state.dropped_closed),
            dropped_unsupported: load(&self.state.dropped_unsupported),
            persistence_failed: load(&self.state.persistence_failed),
            persistence_health,
            process_started_at: self
                .sender
                .is_some()
                .then_some(self.state.process_started_at)
                .filter(|timestamp| *timestamp > 0),
        }
    }

    #[cfg(test)]
    pub(super) fn enabled_for_test(
        sender: mpsc::Sender<TerminalRequestFact>,
        state: Arc<RequestFactDeliveryState>,
    ) -> Self {
        Self {
            sender: Some(sender),
            state,
        }
    }
}

fn terminal_request_fact_worker(
    url: String,
    retry_deadline: Duration,
    mut receiver: mpsc::Receiver<TerminalRequestFact>,
    state: Arc<RequestFactDeliveryState>,
) {
    let mut pg = None;
    while let Some(first) = receiver.blocking_recv() {
        let mut batch = Vec::with_capacity(MAX_REQUEST_FACT_BATCH);
        batch.push(first);
        while batch.len() < MAX_REQUEST_FACT_BATCH {
            match receiver.try_recv() {
                Ok(fact) => batch.push(fact),
                Err(mpsc::error::TryRecvError::Empty | mpsc::error::TryRecvError::Disconnected) => {
                    break;
                }
            }
        }
        match persist_terminal_request_fact_batch(&url, retry_deadline, &mut pg, &batch) {
            Ok(inserted) => {
                state
                    .persisted
                    .fetch_add(inserted as u64, Ordering::Relaxed);
                state.deduplicated.fetch_add(
                    batch.len().saturating_sub(inserted) as u64,
                    Ordering::Relaxed,
                );
                state
                    .persistence_health
                    .store(REQUEST_FACT_HEALTH_HEALTHY, Ordering::Relaxed);
            }
            Err(()) => {
                state
                    .persistence_failed
                    .fetch_add(batch.len() as u64, Ordering::Relaxed);
                state
                    .persistence_health
                    .store(REQUEST_FACT_HEALTH_FAILED, Ordering::Relaxed);
                elog::error(
                    "billing",
                    format!(
                        "request-fact persistence failed; dropping {} terminal facts",
                        batch.len()
                    ),
                );
            }
        }
    }
}

fn terminal_request_fact_batch_is_replay_safe(batch: &[TerminalRequestFact]) -> bool {
    batch.iter().all(|fact| fact.billing_request_id.is_some())
}

fn persist_terminal_request_fact_batch(
    url: &str,
    retry_deadline: Duration,
    pg: &mut Option<registry::pg::PgStore>,
    batch: &[TerminalRequestFact],
) -> Result<usize, ()> {
    let deadline = Instant::now() + retry_deadline;
    let replay_safe = terminal_request_fact_batch_is_replay_safe(batch);
    loop {
        if pg.is_none() {
            match registry::pg::PgStore::connect(url) {
                Ok(connection) => *pg = Some(connection),
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(_) => return Err(()),
            }
        }
        match pg
            .as_mut()
            .expect("request-fact connection established")
            .insert_terminal_request_facts(batch)
        {
            Ok(inserted) => return Ok(inserted),
            Err(error) => {
                let failure_class = registry::pg::classify_failure(&error);
                // An insert may have committed before its acknowledgement was lost. Always discard
                // the uncertain connection, and replay only when S2's non-null billing identity
                // makes every row in the batch idempotent. Nullable rows are at-most-once here.
                *pg = None;
                if failure_class == registry::pg::FailureClass::Transient
                    && replay_safe
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                return Err(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::request_facts::{
        ClientKind, ClientSource, DeliveryState, ProviderTerminalClass, RequestFactTerminalEvidence,
    };

    fn terminal_fact(logical_request_id: &str) -> TerminalRequestFact {
        TerminalRequestFact {
            logical_request_id: logical_request_id.into(),
            billing_request_id: None,
            execution_group_id: None,
            attempt: 1,
            account_id: "account".into(),
            key_id: "key-id".into(),
            client_kind: ClientKind::Unknown,
            client_source: ClientSource::Unknown,
            client_version: None,
            provider_plane: "anthropic".into(),
            route_class: "direct".into(),
            request_class: "messages".into(),
            requested_model: None,
            executable_model: None,
            stream_flag: false,
            tools_declared_count: None,
            tool_classes: None,
            tool_choice_mode: None,
            parallel_tools_requested: None,
            tool_results_in_input: None,
            structured_output_flag: None,
            reasoning_flag: None,
            service_tier: None,
            input_modalities: None,
            output_modalities: None,
            admitted_at: 10,
            terminal: RequestFactTerminalEvidence {
                terminal_at: 11,
                http_status_code: None,
                provider_terminal_class: ProviderTerminalClass::Unknown,
                delivery_state: DeliveryState::NotStarted,
                downstream_disconnect: None,
                upstream_request_id: None,
                first_public_byte_at: None,
                internal_attempt_count: None,
                failure_class: None,
                tool_calls_in_output: None,
            },
        }
    }

    #[test]
    fn batch_replay_requires_non_null_billing_ids_for_every_fact() {
        let all_null = vec![
            terminal_fact("11111111-1111-4111-8111-111111111111"),
            terminal_fact("22222222-2222-4222-8222-222222222222"),
        ];
        assert!(!terminal_request_fact_batch_is_replay_safe(&all_null));

        let mut mixed = all_null.clone();
        mixed[0].billing_request_id = Some("33333333-3333-4333-8333-333333333333".into());
        assert!(!terminal_request_fact_batch_is_replay_safe(&mixed));

        let mut all_non_null = mixed;
        all_non_null[1].billing_request_id = Some("44444444-4444-4444-8444-444444444444".into());
        assert!(terminal_request_fact_batch_is_replay_safe(&all_non_null));
    }

    #[test]
    fn submission_outcomes_and_snapshot_counters_are_fixed_and_bounded() {
        let state = Arc::new(RequestFactDeliveryState::default());
        let (sender, mut receiver) = mpsc::channel(1);
        let inbox = TerminalRequestFactInbox::enabled_for_test(sender, Arc::clone(&state));
        assert_eq!(
            inbox.submit(terminal_fact("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")),
            TerminalRequestFactSubmission::Queued
        );
        assert_eq!(inbox.snapshot().queue_depth, 1);
        assert_eq!(
            inbox.submit(terminal_fact("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")),
            TerminalRequestFactSubmission::QueueFull
        );
        let mut invalid = receiver.try_recv().expect("queued request fact");
        invalid.logical_request_id = "not-a-uuid".into();
        assert_eq!(
            inbox.submit(invalid),
            TerminalRequestFactSubmission::Invalid
        );
        drop(receiver);
        assert_eq!(
            inbox.submit(terminal_fact("cccccccc-cccc-4ccc-8ccc-cccccccccccc")),
            TerminalRequestFactSubmission::WriterClosed
        );
        assert_eq!(
            inbox.snapshot(),
            RequestFactDeliverySnapshot {
                enabled: true,
                queue_depth: 0,
                accepted: 1,
                persisted: 0,
                deduplicated: 0,
                dropped_invalid: 1,
                dropped_full: 1,
                dropped_closed: 1,
                dropped_unsupported: 0,
                persistence_failed: 0,
                persistence_health: RequestFactPersistenceHealth::Unknown,
                process_started_at: None,
            }
        );
    }

    #[test]
    fn disabled_inbox_has_fixed_unsupported_outcome() {
        let inbox = TerminalRequestFactInbox::disabled();
        let mut invalid = terminal_fact("not-a-uuid");
        invalid.account_id.clear();
        assert_eq!(
            inbox.submit(invalid),
            TerminalRequestFactSubmission::UnsupportedAuthority
        );
        let snapshot = inbox.snapshot();
        assert!(!snapshot.enabled);
        assert_eq!(snapshot.dropped_unsupported, 1);
        assert_eq!(snapshot.dropped_invalid, 0);
        assert_eq!(
            snapshot.persistence_health,
            RequestFactPersistenceHealth::Unknown
        );
    }
}
