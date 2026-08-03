//! Bounded delivery queue between the KIMI stream finalizer and the calibration authority.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §10.2. Pure state machine — the caller performs
//! the database write and reports the outcome back.
//!
//! The invariant this exists to protect: **a quota observation must never be paired with a spend
//! total that an earlier turn has not yet reached.** If a turn's event is stuck and a later free
//! quota poll went ahead anyway, the estimator would see movement it cannot attribute and would
//! either inflate capacity or record our own spend as somebody else's. So a failed head stays at
//! the front and blocks both later turns and quota polls until it drains.

use std::collections::VecDeque;

use registry::KimiTurnCalibrationEvent;

/// What the authority reported for one attempted write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Stored, or an exact replay of an identical payload. Either way the head is done.
    Durable,
    /// Transient failure: connection, timeout, or an ambiguous reply. Safe to retry, because the
    /// write is idempotent by the immutable request id.
    Transient,
    /// A different payload was already stored under this request id. Retrying can never succeed.
    Conflict,
}

/// Health the plane publishes so capacity is not sold while evidence is undelivered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeliveryHealth {
    pub pending_events: usize,
    pub dropped_events: u64,
    pub persistence_ok: bool,
}

/// Bounded FIFO of priced turns awaiting durable storage.
pub struct TurnQueue {
    queue: VecDeque<KimiTurnCalibrationEvent>,
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

    /// Enqueue a priced turn. Returns false when the queue is full.
    ///
    /// Overflow drops the NEWEST event, never the head. The head is what blocks quota polling, so
    /// discarding it to make room would release the block and let a poll pair quota with a spend
    /// total that never arrived — exactly the corruption this queue prevents.
    pub fn push(&mut self, event: KimiTurnCalibrationEvent) -> bool {
        if self.queue.len() >= self.capacity {
            self.dropped = self.dropped.saturating_add(1);
            self.healthy = false;
            return false;
        }
        self.queue.push_back(event);
        true
    }

    /// The event a caller should attempt next, if any.
    pub fn head(&self) -> Option<&KimiTurnCalibrationEvent> {
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
                // Keep the head. A later turn or a free poll must not proceed past it.
                self.healthy = false;
            }
            WriteOutcome::Conflict => {
                // Quarantine exactly this row. Retrying can never succeed, and blocking the whole
                // tail on one poisoned event would stall every subsequent turn indefinitely.
                self.queue.pop_front();
                self.dropped = self.dropped.saturating_add(1);
                self.healthy = self.queue.is_empty();
            }
        }
    }

    /// Whether a free quota poll may read cumulative spend right now.
    ///
    /// Reads never create evidence, but a read taken while a turn is undelivered would be paired
    /// with a stale total, so polling waits for the queue to drain.
    pub fn may_poll_quota(&self) -> bool {
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

    fn event(id: &str) -> KimiTurnCalibrationEvent {
        KimiTurnCalibrationEvent {
            request_id: id.into(),
            subject_id: "u_1".into(),
            plan: "Moderato".into(),
            requested_model: "k3".into(),
            served_model: "kimi-k3".into(),
            context_mode: "1m".into(),
            reasoning_effort: "high".into(),
            tariff_schedule_id: "moonshot/kimi-open-platform/2026-08-03".into(),
            priced_ts: 1_800_000_000,
            completed_at: 1_800_000_001,
            input_tokens: 100,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 20,
            reasoning_output_tokens: 0,
            api_input_nanousd: 300_000,
            api_cache_read_nanousd: 0,
            api_cache_write_nanousd: 0,
            api_output_nanousd: 300_000,
            api_total_nanousd: 600_000,
        }
    }

    #[test]
    fn a_drained_queue_is_healthy_and_lets_quota_be_polled() {
        let mut queue = TurnQueue::new(8);
        assert!(queue.may_poll_quota());
        queue.push(event("a"));
        assert!(!queue.may_poll_quota());
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.may_poll_quota());
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
    fn a_transient_failure_keeps_the_head_and_blocks_quota_polling() {
        // This is the whole point: polling past an undelivered turn would pair quota movement
        // with a spend total that turn never reached.
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        queue.resolve_head(WriteOutcome::Transient);
        assert_eq!(queue.head().unwrap().request_id, "a");
        assert!(!queue.may_poll_quota());
        assert!(!queue.health().persistence_ok);
        assert_eq!(queue.health().pending_events, 1);
        // Retry succeeds and the block lifts.
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.may_poll_quota());
        assert!(queue.health().persistence_ok);
    }

    #[test]
    fn a_stuck_head_holds_back_later_turns_too() {
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        queue.push(event("b"));
        queue.resolve_head(WriteOutcome::Transient);
        // Order is preserved: "b" cannot overtake a turn that has not landed.
        assert_eq!(queue.head().unwrap().request_id, "a");
        queue.resolve_head(WriteOutcome::Durable);
        assert_eq!(queue.head().unwrap().request_id, "b");
    }

    #[test]
    fn a_conflict_quarantines_one_row_without_stalling_the_tail() {
        // Retrying a semantic conflict can never succeed, so blocking every later turn on it
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
        // Discarding the head would release the quota-poll block and let a poll pair quota with a
        // spend total that never arrived.
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
    fn degraded_delivery_is_visible_while_evidence_is_undelivered() {
        // Capacity must not read as fresh while turns are still queued.
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        assert!(!queue.health().persistence_ok);
        assert_eq!(queue.health().pending_events, 1);
    }

    #[test]
    fn an_ambiguous_reply_is_safe_to_retry_because_the_write_is_idempotent() {
        // The immutable request id makes an exact replay a no-op, so a transient outcome can be
        // retried without risking a double charge.
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        for _ in 0..5 {
            queue.resolve_head(WriteOutcome::Transient);
            assert_eq!(queue.head().unwrap().request_id, "a");
        }
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_queue_of_capacity_zero_still_accepts_one_event() {
        // A misconfigured zero capacity must not silently discard every turn.
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
}
