//! Bounded delivery queue between the Suno (suno.com) generation finalizer and the
//! calibration authority.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §10.2, following the Codex/Gemini pairing
//! discipline (NOT the split KIMI/GLM one): the FIFO head is the immutable turn event PLUS the
//! post-turn billing read taken in the generation's wake, so one writer command persists the
//! event and an observation whose cumulative ledgers already include that turn — a settlement
//! is never paired with a quota snapshot that its own generation has not reached, and the
//! post-turn credit delta is what attributes (or fails to attribute) the spend.
//!
//! Pure state machine — the caller performs the database write and reports the outcome back.
//!
//! The invariant this exists to protect: **a quota observation must never be paired with a
//! spend total that an earlier generation has not yet reached.** Suno turn events carry the
//! native credit consumption as two exact legs (native millicredits AND its fixed-rate API
//! nanoUSD image, `docs/engine/SUNO_PROVIDER.md` §5.3), and both cumulative ledgers advance per
//! generation: if a generation's event is stuck and a later free billing poll went ahead
//! anyway, the estimator would see credit drawdown it cannot attribute to settled spend. So a
//! failed head stays at the front and blocks both later turns and billing polls until it
//! drains.

use std::collections::VecDeque;

use registry::SunoTurnCalibrationEvent;

use super::client::BillingSnapshot;

/// One FIFO entry: the immutable turn event plus the post-turn billing read taken in its wake
/// (Codex/Gemini pairing). A missing snapshot means the post-turn read failed; the periodic
/// poll path covers quota freshness independently, and the turn event still persists alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingTurn {
    pub event: SunoTurnCalibrationEvent,
    pub billing: Option<BillingSnapshot>,
}

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

/// Bounded FIFO of settled generations awaiting durable storage.
pub(crate) struct TurnQueue {
    queue: VecDeque<PendingTurn>,
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

    /// Enqueue a settled generation. Returns false when the queue is full.
    ///
    /// Overflow drops the NEWEST event, never the head. The head is what blocks billing
    /// polling, so discarding it to make room would release the block and let a poll pair
    /// quota with a spend total that never arrived — exactly the corruption this queue
    /// prevents.
    pub(crate) fn push(&mut self, turn: PendingTurn) -> bool {
        if self.queue.len() >= self.capacity {
            self.dropped = self.dropped.saturating_add(1);
            self.healthy = false;
            return false;
        }
        self.queue.push_back(turn);
        true
    }

    /// The event a caller should attempt next, if any.
    pub(crate) fn head(&self) -> Option<&PendingTurn> {
        self.queue.front()
    }

    /// Report the result of writing the head.
    pub(crate) fn resolve_head(&mut self, outcome: WriteOutcome) {
        match outcome {
            WriteOutcome::Durable => {
                self.queue.pop_front();
                self.healthy = self.queue.is_empty();
            }
            WriteOutcome::Transient => {
                // Keep the head. A later generation or a free poll must not proceed past it.
                self.healthy = false;
            }
            WriteOutcome::Conflict => {
                // Quarantine exactly this row. Retrying can never succeed, and blocking the
                // whole tail on one poisoned event would stall every subsequent generation
                // indefinitely.
                self.queue.pop_front();
                self.dropped = self.dropped.saturating_add(1);
                self.healthy = self.queue.is_empty();
            }
        }
    }

    /// Whether a free billing poll may read cumulative spend right now.
    ///
    /// Reads never create evidence, but a read taken while a generation is undelivered would
    /// be paired with a stale total, so polling waits for the queue to drain.
    pub(crate) fn may_poll_quota(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn health(&self) -> DeliveryHealth {
        DeliveryHealth {
            pending_events: self.queue.len(),
            dropped_events: self.dropped,
            persistence_ok: self.healthy && self.queue.is_empty(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }
}

/// Default queue depth, matching the other planes.
pub const DEFAULT_QUEUE_CAPACITY: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str) -> PendingTurn {
        PendingTurn {
            event: SunoTurnCalibrationEvent {
                request_id: id.into(),
                subject_id: "u_1".into(),
                plan: "Pro".into(),
                requested_model: "v5.5".into(),
                served_model: None,
                tariff_schedule_id: "suno/derived-subscription/2026-08-12".into(),
                priced_ts: 1_800_000_000,
                completed_at: 1_800_000_001,
                upstream_clip_id: "clip_1".into(),
                native_total_millicredits: 5_000,
                api_total_nanousd: 20_000_000,
                native_schedule_derived: true,
            },
            billing: None,
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
        // This is the whole point: polling past an undelivered generation would pair quota
        // movement with a spend total that generation never reached.
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        queue.resolve_head(WriteOutcome::Transient);
        assert_eq!(queue.head().unwrap().event.request_id, "a");
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
        // Order is preserved: "b" cannot overtake a generation that has not landed.
        assert_eq!(queue.head().unwrap().event.request_id, "a");
        queue.resolve_head(WriteOutcome::Durable);
        assert_eq!(queue.head().unwrap().event.request_id, "b");
    }

    #[test]
    fn a_conflict_quarantines_one_row_without_stalling_the_tail() {
        // Retrying a semantic conflict can never succeed, so blocking every later generation
        // on it would stall the plane indefinitely.
        let mut queue = TurnQueue::new(8);
        queue.push(event("a"));
        queue.push(event("b"));
        queue.resolve_head(WriteOutcome::Conflict);
        assert_eq!(queue.head().unwrap().event.request_id, "b");
        assert_eq!(queue.health().dropped_events, 1);
        queue.resolve_head(WriteOutcome::Durable);
        assert!(queue.health().persistence_ok);
        // The drop is remembered even after recovery, so it stays visible to operators.
        assert_eq!(queue.health().dropped_events, 1);
    }

    #[test]
    fn overflow_drops_the_newest_and_never_the_head() {
        // Discarding the head would release the quota-poll block and let a poll pair quota
        // with a spend total that never arrived.
        let mut queue = TurnQueue::new(2);
        assert!(queue.push(event("a")));
        assert!(queue.push(event("b")));
        assert!(!queue.push(event("c")));
        assert_eq!(queue.head().unwrap().event.request_id, "a");
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.health().dropped_events, 1);
        assert!(!queue.health().persistence_ok);
    }

    #[test]
    fn a_queue_of_capacity_zero_still_accepts_one_event() {
        // A misconfigured zero capacity must not silently discard every generation.
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
