//! Bounded delivery queue between the Tripo3D (VAST / Holymolly) task finalizer and the
//! calibration authority.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §10.2. Pure state machine — the caller
//! performs the database write and reports the outcome back.
//!
//! The invariant this exists to protect: **a balance observation must never be paired with a
//! spend total that an earlier task has not yet reached.** Tripo3D turn events carry the
//! authoritative `consumed_credit` as two exact legs (native millicredits AND its fixed-rate
//! API nanoUSD image, `docs/engine/TRIPO3D_PROVIDER.md` §5.3), and both cumulative ledgers
//! advance per task: if a task's event is stuck and a later free balance poll went ahead
//! anyway, the estimator would see balance drawdown it cannot attribute to settled spend and
//! would either misprice capacity or record our own consumption as somebody else's. So a
//! failed head stays at the front and blocks both later turns and balance polls until it
//! drains.

use std::collections::VecDeque;

use registry::Tripo3dTurnCalibrationEvent;

/// What the authority reported for one attempted write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Stored, or an exact replay of an identical payload. Either way the head is done.
    Durable,
    /// Transient failure: connection, timeout, or an ambiguous reply. Safe to retry, because
    /// the write is idempotent by the immutable request id.
    Transient,
    /// A different payload was already stored under this request id. Retrying can never
    /// succeed.
    Conflict,
}

/// Health the plane publishes so capacity is not sold while evidence is undelivered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeliveryHealth {
    pub pending_events: usize,
    pub dropped_events: u64,
    pub persistence_ok: bool,
}

/// Bounded FIFO of settled tasks awaiting durable storage.
pub struct TurnQueue {
    queue: VecDeque<Tripo3dTurnCalibrationEvent>,
    capacity: usize,
    dropped: u64,
    /// False once a write failed transiently, until the head drains.
    healthy: bool,
}

impl TurnQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            capacity: capacity.max(1),
            dropped: 0,
            healthy: true,
        }
    }

    /// Enqueue a settled task. Returns false when the queue is full.
    ///
    /// Overflow drops the NEWEST event, never the head. The head is what blocks balance
    /// polling, so discarding it to make room would release the block and let a poll pair
    /// balance with a spend total that never arrived — exactly the corruption this queue
    /// prevents.
    pub fn push(&mut self, event: Tripo3dTurnCalibrationEvent) -> bool {
        if self.queue.len() >= self.capacity {
            self.dropped = self.dropped.saturating_add(1);
            self.healthy = false;
            return false;
        }
        self.queue.push_back(event);
        true
    }

    /// The event a caller should attempt next, if any.
    pub fn head(&self) -> Option<&Tripo3dTurnCalibrationEvent> {
        self.queue.front()
    }

    /// Report the result of writing the head.
    pub fn resolve_head(&mut self, outcome: WriteOutcome) {
        match outcome {
            WriteOutcome::Durable => {
                self.queue.pop_front();
                self.healthy = self.queue.is_empty();
            }
            WriteOutcome::Transient => {
                // Keep the head. A later task or a free poll must not proceed past it.
                self.healthy = false;
            }
            WriteOutcome::Conflict => {
                // Quarantine exactly this row. Retrying can never succeed, and blocking the
                // whole tail on one poisoned event would stall every subsequent task
                // indefinitely.
                self.queue.pop_front();
                self.dropped = self.dropped.saturating_add(1);
                self.healthy = self.queue.is_empty();
            }
        }
    }

    /// Whether a free balance poll may read cumulative spend right now.
    ///
    /// Reads never create evidence, but a read taken while a task is undelivered would be
    /// paired with a stale total, so polling waits for the queue to drain.
    pub fn may_poll_balance(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn health(&self) -> DeliveryHealth {
        DeliveryHealth {
            pending_events: self.queue.len(),
            dropped_events: self.dropped,
            persistence_ok: self.healthy && self.queue.is_empty(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }
}

/// Default queue depth, matching the other planes.
pub const DEFAULT_QUEUE_CAPACITY: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str) -> Tripo3dTurnCalibrationEvent {
        Tripo3dTurnCalibrationEvent {
            request_id: id.into(),
            subject_id: "u_1".into(),
            cohort: "tripo3d-api-50".into(),
            task_type: "image_to_model".into(),
            requested_model_version: Some("v2.5-20250123".into()),
            resolved_model_version: Some("v2.5-20250123".into()),
            tariff_schedule_id: "tripo3d/openapi-billing/2026-08-12".into(),
            priced_ts: 1_800_000_000,
            completed_at: 1_800_000_001,
            upstream_task_id: "task_1".into(),
            native_total_millicredits: 20_000,
            api_total_nanousd: 200_000_000,
        }
    }

    #[test]
    fn a_drained_queue_is_healthy_and_lets_balance_be_polled() {
        let mut queue = TurnQueue::new(8);
        assert!(queue.may_poll_balance());
        queue.push(event("a"));
        assert!(!queue.may_poll_balance());
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.may_poll_balance());
        assert_eq!(
            queue.health(),
            DeliveryHealth {
                pending_events: 0,
                dropped_events: 0,
                persistence_ok: true
            }
        );
    }

    #[test]
    fn a_transient_failure_keeps_the_head_and_blocks_balance_polling() {
        // This is the whole point: polling past an undelivered task would pair balance
        // movement with a spend total that task never reached.
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        queue.resolve_head(WriteOutcome::Transient);
        assert_eq!(queue.head().unwrap().request_id, "a");
        assert!(!queue.may_poll_balance());
        assert!(!queue.health().persistence_ok);
        assert_eq!(queue.health().pending_events, 1);
        // Retry succeeds and the block lifts.
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.may_poll_balance());
        assert!(queue.health().persistence_ok);
    }

    #[test]
    fn a_stuck_head_holds_back_later_turns_too() {
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        queue.push(event("b"));
        queue.resolve_head(WriteOutcome::Transient);
        // Order is preserved: "b" cannot overtake a task that has not landed.
        assert_eq!(queue.head().unwrap().request_id, "a");
        queue.resolve_head(WriteOutcome::Durable);
        assert_eq!(queue.head().unwrap().request_id, "b");
    }

    #[test]
    fn a_conflict_quarantines_one_row_without_stalling_the_tail() {
        // Retrying a semantic conflict can never succeed, so blocking every later task on it
        // would stall the plane indefinitely.
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        queue.push(event("b"));
        queue.resolve_head(WriteOutcome::Conflict);
        assert_eq!(queue.head().unwrap().request_id, "b");
        assert_eq!(queue.health().dropped_events, 1);
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.health().persistence_ok);
        // The drop is remembered even after recovery, so it stays visible to operators.
        assert_eq!(queue.health().dropped_events, 1);
    }

    #[test]
    fn overflow_drops_the_newest_and_never_the_head() {
        // Discarding the head would release the balance-poll block and let a poll pair balance
        // with a spend total that never arrived.
        let mut queue = TurnQueue::new(2);
        assert!(queue.push(event("a")));
        assert!(queue.push(event("b")));
        assert!(!queue.push(event("c")));
        assert_eq!(queue.head().unwrap().request_id, "a");
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.health().dropped_events, 1);
        assert!(!queue.health().persistence_ok);
    }

    #[test]
    fn a_queue_of_capacity_zero_still_accepts_one_event() {
        // A misconfigured zero capacity must not silently discard every task.
        let mut queue = TurnQueue::new(0);
        assert!(queue.push(event("a")));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn draining_in_order_reports_health_only_when_fully_empty() {
        let mut queue = TurnQueue::new(8);
        for id in ["a", "b", "c"] {
            queue.push(event(id));
        }
        queue.resolve_head(WriteOutcome::Durable);
        assert!(!queue.health().persistence_ok, "still two pending");
        queue.resolve_head(WriteOutcome::Durable);
        assert!(!queue.health().persistence_ok, "still one pending");
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.health().persistence_ok);
    }

    #[test]
    fn the_default_capacity_matches_the_other_planes() {
        assert_eq!(DEFAULT_QUEUE_CAPACITY, 4096);
    }
}
