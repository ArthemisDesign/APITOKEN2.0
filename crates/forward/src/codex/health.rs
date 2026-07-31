//! Pure health and admission policy for one Codex home.
//!
//! Every verdict here is a deterministic function of the observed signals and `now`, exactly like
//! the Claude pool's `apply_probe`: no clock, no socket, no subscription and no I/O. That is what
//! makes the policy testable as policy, instead of only being observable in production.
//!
//! # Why two independent axes
//!
//! A home can fail in two ways that look identical from the outside and require opposite responses:
//!
//! * The **account** is rate limited, unauthenticated or banned. That is a property of the
//!   subscription. It survives replacing the transport, and the only cure is time or an operator.
//! * The **transport** stopped answering while the account is perfectly healthy. That is a property
//!   of one app-server generation, and replacing that generation is exactly the cure.
//!
//! Collapsing both into a single "is this home ok" boolean is what allowed a silent home to stay
//! routable in production: a missed deadline is not an authentication verdict, so the previous
//! policy treated it as no verdict at all and the home never left rotation.
//!
//! # Why a deadline may stop admission but must not recycle the transport
//!
//! Turns are multiplexed over one shared app-server child, so recycling has a blast radius: it
//! kills every sibling turn on that home. Refusing to admit *new* turns has no blast radius at all
//! — in-flight turns keep running to their own completion. Those are different actions with
//! different costs, and the policy keeps them separate: a short deadline streak closes admission,
//! and only a longer, time-corroborated streak escalates to recycling.

/// Consecutive missed deadlines that close admission for new turns.
///
/// Two rather than one: a single deadline can be an unlucky slow turn, and closing admission on
/// every such blip would make the pool flap. Two consecutive misses with nothing succeeding in
/// between is already a home that is not serving.
pub(crate) const DEADLINE_DEGRADE_STREAK: i64 = 2;
/// Consecutive missed deadlines before the transport generation is declared unusable…
pub(crate) const DEADLINE_WEDGE_STREAK: i64 = 4;
/// …and only if the streak has also lasted this long. Recycling kills sibling turns, so it is
/// corroborated in both count and time, the same shape as the account verdict below.
pub(crate) const DEADLINE_WEDGE_MIN_SECS: i64 = 60;
/// Clean authentication failures needed before a subscription is declared dead, mirroring the
/// Claude pool's `DEAD_STREAK`. One 401/403 is never a verdict: it may belong to the request.
pub(crate) const AUTH_DEAD_STREAK: i64 = 2;
/// …stretched over at least this long, mirroring `DEAD_MIN_SECS`, so a momentary provider blip
/// cannot reach the terminal state while a real revoke still resolves within a few probe cycles.
pub(crate) const AUTH_DEAD_MIN_SECS: i64 = 300;
/// A rate-limit snapshot older than this many probe intervals is no longer evidence about now.
///
/// Three rather than one: the sweep is not perfectly periodic, and a snapshot that is merely one
/// interval late is still the best information available. Three consecutive missed refreshes mean
/// the refresh path itself is broken, which is precisely when the value must stop being trusted.
pub(crate) const SNAPSHOT_STALE_AFTER_PROBES: i64 = 3;
/// Cooling applied to a home whose transport was just declared wedged, so a crash-looping
/// generation cannot be re-selected in a hot loop while its replacement starts. Deliberately short:
/// the child is the fault, not the subscription, and its capacity must return as soon as it can.
pub(crate) const WEDGED_COOL_SECS: i64 = 10;
/// Quarantine for a home the provider actively rejected. Hammering a rejected profile is useless
/// and is itself a ban signal; the health sweep still probes it, so repair is automatic.
pub(crate) const AUTH_QUARANTINE_SECS: i64 = 900;

/// Liveness of the subscription behind a home. Durable in spirit: it must survive a transport
/// restart, because the account does not care which app-server generation talked to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum AccountState {
    #[default]
    Healthy,
    /// Authentication is failing but is not yet corroborated. Still routable — a single rejection
    /// may belong to the request rather than to the token — but forced to re-probe.
    Suspect,
    /// Corroborated dead or banned. Out of rotation until a probe proves otherwise.
    Dead,
}

impl AccountState {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "dead" => AccountState::Dead,
            "suspect" => AccountState::Suspect,
            // An unrecognised verdict is treated as healthy on purpose: a schema the running binary
            // does not understand must not silently quarantine a working subscription.
            _ => AccountState::Healthy,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AccountState::Healthy => "healthy",
            AccountState::Suspect => "suspect",
            AccountState::Dead => "dead",
        }
    }
}

/// Responsiveness of the current app-server generation serving a home.
///
/// Deliberately distinct from "the child process exists". A live process attached to a socket whose
/// server was replaced is a process that will never answer again — production hit exactly that, and
/// a liveness check based on the process handle reported it healthy the whole time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum TransportState {
    #[default]
    Responsive,
    /// Missing deadlines. New turns are not admitted; in-flight turns are left alone.
    Degraded,
    /// Corroborated unusable. The generation must be replaced before this home serves again.
    Wedged,
}

impl TransportState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TransportState::Responsive => "responsive",
            TransportState::Degraded => "degraded",
            TransportState::Wedged => "wedged",
        }
    }
}

/// How much the cached rate-limit snapshot can be trusted right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Freshness {
    /// Refreshed within the expected cadence.
    Fresh,
    /// Older than the cadence allows: usable as a hint, never as proof.
    Stale,
    /// Never observed.
    Unknown,
}

/// Everything the policy learns about a home, normalised so `health.rs` never sees a protocol type.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LimitView {
    /// The provider stated explicitly that the window is exhausted.
    pub(crate) reached: bool,
    /// Highest reported window utilisation, whole percent as published.
    pub(crate) max_used_percent: Option<i64>,
    /// When this snapshot was observed.
    pub(crate) observed_at: i64,
    /// Soonest moment a blocked window is expected to reopen.
    pub(crate) soonest_reset_at: Option<i64>,
}

/// One observation about a home. The caller classifies the event; the policy decides what it means.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HealthSignal {
    /// A probe completed: the account answered and the transport carried the answer.
    ProbeOk,
    /// A customer turn completed. Proof of the same two facts, earned without extra traffic.
    TurnOk,
    /// A probe or a turn missed its deadline. Says nothing about which layer is at fault, which is
    /// why it only ever closes admission until corroborated.
    Deadline,
    /// The transport is provably gone: EOF, a protocol violation, or a closed generation.
    TransportClosed,
    /// The provider rejected authentication. `permanent` marks a verdict that needs an operator
    /// (subscription missing) rather than a possibly transient token problem.
    AuthRejected { permanent: bool },
    /// The provider refused on quota.
    UsageLimited { retry_after_secs: Option<i64> },
}

/// Why a home is not admitting turns right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RejectReason {
    AccountDead,
    TransportWedged,
    TransportDegraded,
    Cooling,
    ProviderLimit,
}

impl RejectReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RejectReason::AccountDead => "account_dead",
            RejectReason::TransportWedged => "transport_wedged",
            RejectReason::TransportDegraded => "transport_degraded",
            RejectReason::Cooling => "cooling",
            RejectReason::ProviderLimit => "provider_limit",
        }
    }
}

/// The admission verdict for one home at one instant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Admission {
    /// Routable. `snapshot_stale` down-ranks the home against equally loaded peers whose quota
    /// evidence is current, so a frozen reading can never win a tie by looking optimistic.
    Admit { snapshot_stale: bool },
    Reject {
        reason: RejectReason,
        /// When this home is expected back, when that is knowable.
        ready_at: Option<i64>,
    },
}

impl Admission {
    pub(crate) fn is_admitted(self) -> bool {
        matches!(self, Admission::Admit { .. })
    }
}

/// The part of a home's health that outlives the process.
///
/// Only the account axis. Transport health belongs to one app-server generation, so carrying it
/// across a restart would assert something about a bridge that no longer exists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DurableAccountHealth {
    pub(crate) account: AccountState,
    pub(crate) auth_fail_streak: i64,
    pub(crate) first_auth_fail_ts: i64,
    pub(crate) cooling_until: i64,
}

/// Health of one home. Cheap to copy, so callers can snapshot it under a short lock.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HomeHealth {
    pub(crate) account: AccountState,
    pub(crate) transport: TransportState,
    /// Consecutive missed deadlines with no success in between.
    pub(crate) deadline_streak: i64,
    first_deadline_ts: i64,
    /// Consecutive clean authentication rejections.
    pub(crate) auth_fail_streak: i64,
    first_auth_fail_ts: i64,
    /// Nothing is admitted before this instant.
    pub(crate) cooling_until: i64,
    /// Last moment this home provably served something.
    pub(crate) last_ok_ts: i64,
    /// Set when a signal asks the sweep to re-probe this home ahead of its cadence.
    pub(crate) probe_requested: bool,
}

impl HomeHealth {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Fold one observation into the state.
    pub(crate) fn apply(&mut self, signal: HealthSignal, now: i64) {
        match signal {
            // Success is the universal solvent: it proves the account answered *and* the transport
            // carried the answer, so both axes reset. Anything less would leave a repaired home
            // permanently down-ranked by a failure it has since disproved.
            HealthSignal::ProbeOk | HealthSignal::TurnOk => {
                self.account = AccountState::Healthy;
                self.transport = TransportState::Responsive;
                self.deadline_streak = 0;
                self.first_deadline_ts = 0;
                self.auth_fail_streak = 0;
                self.first_auth_fail_ts = 0;
                self.cooling_until = 0;
                self.last_ok_ts = now;
                self.probe_requested = false;
            }
            HealthSignal::Deadline => {
                self.deadline_streak += 1;
                if self.first_deadline_ts == 0 {
                    self.first_deadline_ts = now;
                }
                // A deadline is the one signal that used to mean nothing. It now closes admission
                // quickly and escalates to a recycle only once corroborated over time, because the
                // two actions have very different costs for sibling turns.
                if self.deadline_streak >= DEADLINE_WEDGE_STREAK
                    && now - self.first_deadline_ts >= DEADLINE_WEDGE_MIN_SECS
                {
                    self.transport = TransportState::Wedged;
                    self.cool_until(now.saturating_add(WEDGED_COOL_SECS));
                } else if self.deadline_streak >= DEADLINE_DEGRADE_STREAK {
                    self.transport = TransportState::Degraded;
                }
                // Ask the sweep to look at this home now rather than at its next scheduled turn.
                self.probe_requested = true;
            }
            // A closed generation is not a suspicion, it is a fact, and it is cheap to act on: the
            // turns it could have killed are already dead.
            HealthSignal::TransportClosed => {
                self.transport = TransportState::Wedged;
                self.cool_until(now.saturating_add(WEDGED_COOL_SECS));
                self.probe_requested = true;
            }
            HealthSignal::AuthRejected { permanent } => {
                self.auth_fail_streak += 1;
                if self.first_auth_fail_ts == 0 {
                    self.first_auth_fail_ts = now;
                }
                if self.account == AccountState::Healthy {
                    self.account = AccountState::Suspect;
                }
                // A subscription the provider says is absent needs an operator, not corroboration.
                // A token rejection might still belong to the request, so it is corroborated in
                // both count and elapsed time before reaching the terminal state.
                let corroborated = self.auth_fail_streak >= AUTH_DEAD_STREAK
                    && now - self.first_auth_fail_ts >= AUTH_DEAD_MIN_SECS;
                if permanent || corroborated {
                    self.account = AccountState::Dead;
                }
                self.cool_until(now.saturating_add(AUTH_QUARANTINE_SECS));
                self.probe_requested = true;
            }
            // Quota is not a fault of either layer: the home is healthy and simply has nothing left
            // to sell until its window turns over. Cool it, but never stain its health.
            HealthSignal::UsageLimited { retry_after_secs } => {
                let wait = retry_after_secs.unwrap_or(0).max(1);
                self.cool_until(now.saturating_add(wait));
            }
        }
    }

    /// Cooling is only ever extended. A weaker later signal must not shorten a quarantine, or a
    /// long provider ban would be silently downgraded to a ten-second pause.
    fn cool_until(&mut self, until: i64) {
        self.cooling_until = self.cooling_until.max(until);
    }

    /// The durable slice, for persistence and for change detection.
    pub(crate) fn durable(&self) -> DurableAccountHealth {
        DurableAccountHealth {
            account: self.account,
            auth_fail_streak: self.auth_fail_streak,
            first_auth_fail_ts: self.first_auth_fail_ts,
            cooling_until: self.cooling_until,
        }
    }

    /// Restore a verdict recovered from the authority. The transport axis is deliberately left at
    /// its default: this process holds a new app-server bridge and must earn its own verdict.
    pub(crate) fn restore(&mut self, durable: DurableAccountHealth) {
        self.account = durable.account;
        self.auth_fail_streak = durable.auth_fail_streak;
        self.first_auth_fail_ts = durable.first_auth_fail_ts;
        self.cooling_until = self.cooling_until.max(durable.cooling_until);
    }

    pub(crate) fn is_cooling(&self, now: i64) -> bool {
        self.cooling_until > now
    }

    /// Whether the transport generation should be replaced. Separate from admission on purpose:
    /// the caller owns the replacement, and replacement is the expensive half of the policy.
    pub(crate) fn needs_recycle(&self) -> bool {
        self.transport == TransportState::Wedged
    }

    /// Whether the sweep should probe this home ahead of its cadence.
    ///
    /// This is the Codex counterpart of the Claude pool's `request_probe`: a bad outcome on the
    /// data path immediately queues a control-plane check, instead of waiting a full interval for
    /// the pool to notice on its own.
    pub(crate) fn wants_probe(&self) -> bool {
        self.probe_requested
    }

    /// Consume the forced-probe request. Called by the sweep as it picks the home up.
    pub(crate) fn take_probe_request(&mut self) -> bool {
        std::mem::replace(&mut self.probe_requested, false)
    }

    /// How fresh the quota evidence is, expressed against the sweep cadence rather than a wall
    /// clock constant, so tuning the cadence cannot silently invalidate the staleness rule.
    pub(crate) fn freshness(
        limits: Option<&LimitView>,
        now: i64,
        probe_interval_secs: i64,
    ) -> Freshness {
        let Some(limits) = limits else {
            return Freshness::Unknown;
        };
        let ttl = probe_interval_secs
            .max(1)
            .saturating_mul(SNAPSHOT_STALE_AFTER_PROBES);
        if now.saturating_sub(limits.observed_at) > ttl {
            Freshness::Stale
        } else {
            Freshness::Fresh
        }
    }

    /// The admission verdict, as a pure function of health, quota evidence and time.
    ///
    /// Order matters and encodes the cost of being wrong. A dead account and a wedged transport are
    /// certainties, so they reject first. Quota rejects next, because serving past a full window
    /// burns a customer request on a subscription that cannot answer. A degraded transport rejects
    /// last among the certainties, since it is the most likely to be disproved by the next probe.
    pub(crate) fn admission(
        &self,
        limits: Option<&LimitView>,
        now: i64,
        probe_interval_secs: i64,
    ) -> Admission {
        if self.account == AccountState::Dead {
            return Admission::Reject {
                reason: RejectReason::AccountDead,
                ready_at: (self.cooling_until > now).then_some(self.cooling_until),
            };
        }
        if self.transport == TransportState::Wedged {
            return Admission::Reject {
                reason: RejectReason::TransportWedged,
                ready_at: (self.cooling_until > now).then_some(self.cooling_until),
            };
        }
        if self.is_cooling(now) {
            return Admission::Reject {
                reason: RejectReason::Cooling,
                ready_at: Some(self.cooling_until),
            };
        }
        let freshness = Self::freshness(limits, now, probe_interval_secs);
        // A frozen snapshot is not evidence about now. Trusting one is what kept a home that had
        // stopped answering at the head of the rotation for as long as it stayed frozen, and what
        // would have kept a recovered home out of rotation for as long as its last reading said
        // "full". Stale quota therefore never rejects and never wins a tie: it only down-ranks.
        if freshness == Freshness::Fresh {
            if let Some(limits) = limits {
                if Self::is_provider_limited(limits) {
                    return Admission::Reject {
                        reason: RejectReason::ProviderLimit,
                        ready_at: limits.soonest_reset_at,
                    };
                }
            }
        }
        if self.transport == TransportState::Degraded {
            return Admission::Reject {
                reason: RejectReason::TransportDegraded,
                ready_at: None,
            };
        }
        Admission::Admit {
            // Only evidence that went *stale* is down-ranked. A home that has simply never reported
            // yet must rank equal to a fresh one, or the first home to receive a snapshot becomes a
            // permanent magnet and the rotation cursor can never break the tie — the exact herding
            // the cursor exists to prevent. Unknown quota is not bad quota, it is no evidence, and
            // the forced probe is what resolves it.
            snapshot_stale: freshness == Freshness::Stale,
        }
    }

    /// An explicit provider verdict, or a window the provider reports as full.
    ///
    /// `usedPercent` is quantised to whole percent, so `100` can arrive slightly before the true
    /// wall. That remainder is not worth selling: selection is fail-closed and an excluded home
    /// becomes a real `429 + Retry-After`, which beats spending a customer request on a
    /// subscription that has stopped answering.
    fn is_provider_limited(limits: &LimitView) -> bool {
        limits.reached || limits.max_used_percent.is_some_and(|used| used >= 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits_at(observed_at: i64, used: i64) -> LimitView {
        LimitView {
            reached: false,
            max_used_percent: Some(used),
            observed_at,
            soonest_reset_at: None,
        }
    }

    const INTERVAL: i64 = 10;

    #[test]
    fn a_single_deadline_does_not_close_admission() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::Deadline, 100);
        assert_eq!(health.transport, TransportState::Responsive);
        assert!(health
            .admission(Some(&limits_at(100, 10)), 100, INTERVAL)
            .is_admitted());
        // …but it does ask for an immediate re-probe rather than waiting a full cadence.
        assert!(health.wants_probe());
    }

    #[test]
    fn a_deadline_streak_closes_admission_without_recycling_the_transport() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::Deadline, 100);
        health.apply(HealthSignal::Deadline, 110);
        assert_eq!(health.transport, TransportState::Degraded);
        // This is the production failure: the home must stop taking new turns…
        assert!(matches!(
            health.admission(Some(&limits_at(110, 10)), 110, INTERVAL),
            Admission::Reject {
                reason: RejectReason::TransportDegraded,
                ..
            }
        ));
        // …while the shared child keeps serving the turns already multiplexed over it.
        assert!(!health.needs_recycle());
    }

    #[test]
    fn a_corroborated_deadline_streak_escalates_to_a_recycle() {
        let mut health = HomeHealth::new();
        for tick in 0..DEADLINE_WEDGE_STREAK {
            health.apply(HealthSignal::Deadline, 100 + tick * 30);
        }
        assert_eq!(health.transport, TransportState::Wedged);
        assert!(health.needs_recycle());
    }

    #[test]
    fn a_fast_deadline_burst_stops_short_of_recycling() {
        // Four misses inside a few seconds are one bad moment, not a dead generation: escalating
        // here would kill sibling turns on evidence that has not been corroborated over time.
        let mut health = HomeHealth::new();
        for tick in 0..DEADLINE_WEDGE_STREAK {
            health.apply(HealthSignal::Deadline, 100 + tick);
        }
        assert_eq!(health.transport, TransportState::Degraded);
        assert!(!health.needs_recycle());
    }

    #[test]
    fn success_clears_every_axis_so_a_repaired_home_returns_immediately() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::Deadline, 100);
        health.apply(HealthSignal::Deadline, 130);
        health.apply(HealthSignal::AuthRejected { permanent: false }, 160);
        health.apply(HealthSignal::TurnOk, 200);
        assert_eq!(health.account, AccountState::Healthy);
        assert_eq!(health.transport, TransportState::Responsive);
        assert_eq!(health.deadline_streak, 0);
        assert!(!health.is_cooling(200));
        assert!(health
            .admission(Some(&limits_at(200, 10)), 200, INTERVAL)
            .is_admitted());
    }

    #[test]
    fn a_closed_transport_is_a_fact_and_needs_no_corroboration() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::TransportClosed, 100);
        assert_eq!(health.transport, TransportState::Wedged);
        assert!(health.needs_recycle());
    }

    #[test]
    fn one_auth_rejection_suspects_but_does_not_kill() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::AuthRejected { permanent: false }, 100);
        assert_eq!(health.account, AccountState::Suspect);
        // Suspect is quarantined by cooling, but it is not the terminal verdict.
        assert_ne!(health.account, AccountState::Dead);
    }

    #[test]
    fn auth_death_requires_both_a_streak_and_elapsed_time() {
        // Two rejections inside the corroboration window are still not a verdict…
        let mut fast = HomeHealth::new();
        fast.apply(HealthSignal::AuthRejected { permanent: false }, 100);
        fast.apply(HealthSignal::AuthRejected { permanent: false }, 120);
        assert_eq!(fast.account, AccountState::Suspect);
        // …the same two, stretched past the window, are.
        let mut slow = HomeHealth::new();
        slow.apply(HealthSignal::AuthRejected { permanent: false }, 100);
        slow.apply(
            HealthSignal::AuthRejected { permanent: false },
            100 + AUTH_DEAD_MIN_SECS,
        );
        assert_eq!(slow.account, AccountState::Dead);
    }

    #[test]
    fn a_missing_subscription_is_terminal_on_the_first_verdict() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::AuthRejected { permanent: true }, 100);
        assert_eq!(health.account, AccountState::Dead);
    }

    #[test]
    fn quota_cools_the_home_without_staining_its_health() {
        let mut health = HomeHealth::new();
        health.apply(
            HealthSignal::UsageLimited {
                retry_after_secs: Some(300),
            },
            100,
        );
        assert_eq!(health.account, AccountState::Healthy);
        assert_eq!(health.transport, TransportState::Responsive);
        assert!(health.is_cooling(399));
        assert!(!health.is_cooling(400));
    }

    #[test]
    fn cooling_is_extended_never_shortened() {
        let mut health = HomeHealth::new();
        health.apply(
            HealthSignal::UsageLimited {
                retry_after_secs: Some(3_600),
            },
            100,
        );
        health.apply(HealthSignal::Deadline, 100);
        health.apply(HealthSignal::Deadline, 130);
        health.apply(HealthSignal::Deadline, 160);
        health.apply(HealthSignal::Deadline, 190);
        // The wedge cooling is far shorter than the quota window it must not overwrite.
        assert_eq!(health.cooling_until, 3_700);
    }

    #[test]
    fn a_stale_snapshot_never_rejects_and_is_reported_as_stale() {
        let health = HomeHealth::new();
        let stale_full = LimitView {
            reached: false,
            max_used_percent: Some(100),
            observed_at: 100,
            soonest_reset_at: None,
        };
        let now = 100 + INTERVAL * SNAPSHOT_STALE_AFTER_PROBES + 1;
        // A frozen "full" reading must not keep a home out forever: the very probe that would
        // refresh it is the thing that is failing.
        assert_eq!(
            health.admission(Some(&stale_full), now, INTERVAL),
            Admission::Admit {
                snapshot_stale: true
            }
        );
    }

    #[test]
    fn a_fresh_full_window_rejects_with_its_reset_time() {
        let health = HomeHealth::new();
        let full = LimitView {
            reached: false,
            max_used_percent: Some(100),
            observed_at: 100,
            soonest_reset_at: Some(9_000),
        };
        assert_eq!(
            health.admission(Some(&full), 105, INTERVAL),
            Admission::Reject {
                reason: RejectReason::ProviderLimit,
                ready_at: Some(9_000)
            }
        );
    }

    #[test]
    fn an_explicit_reached_verdict_rejects_below_a_hundred_percent() {
        let health = HomeHealth::new();
        let reached = LimitView {
            reached: true,
            max_used_percent: Some(12),
            observed_at: 100,
            soonest_reset_at: None,
        };
        assert!(!health
            .admission(Some(&reached), 105, INTERVAL)
            .is_admitted());
    }

    #[test]
    fn a_missing_snapshot_stays_fail_open() {
        // Quota evidence is observational. Making it a hard dependency would turn one unavailable
        // provider endpoint into a total pool outage.
        let health = HomeHealth::new();
        assert_eq!(
            health.admission(None, 100, INTERVAL),
            Admission::Admit {
                snapshot_stale: false
            }
        );
        assert_eq!(
            HomeHealth::freshness(None, 100, INTERVAL),
            Freshness::Unknown
        );
    }

    #[test]
    fn a_home_that_has_never_reported_is_not_down_ranked() {
        // Regression: ranking "no evidence yet" below "fresh evidence" made the first home to
        // receive a snapshot win every subsequent tie, collapsing the whole pool onto it and
        // defeating the rotation cursor. Unknown must tie with Fresh.
        let health = HomeHealth::new();
        let fresh = health.admission(Some(&limits_at(100, 10)), 105, INTERVAL);
        let never_reported = health.admission(None, 105, INTERVAL);
        assert_eq!(fresh, never_reported);
    }

    #[test]
    fn freshness_tracks_the_configured_cadence() {
        let limits = limits_at(100, 10);
        let edge = 100 + INTERVAL * SNAPSHOT_STALE_AFTER_PROBES;
        assert_eq!(
            HomeHealth::freshness(Some(&limits), edge, INTERVAL),
            Freshness::Fresh
        );
        assert_eq!(
            HomeHealth::freshness(Some(&limits), edge + 1, INTERVAL),
            Freshness::Stale
        );
    }

    #[test]
    fn a_dead_account_outranks_every_other_rejection() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::AuthRejected { permanent: true }, 100);
        health.apply(HealthSignal::TransportClosed, 100);
        assert!(matches!(
            health.admission(None, 105, INTERVAL),
            Admission::Reject {
                reason: RejectReason::AccountDead,
                ..
            }
        ));
    }

    #[test]
    fn a_restored_verdict_keeps_the_account_but_not_the_transport() {
        let mut source = HomeHealth::new();
        source.apply(HealthSignal::AuthRejected { permanent: true }, 100);
        source.apply(HealthSignal::TransportClosed, 100);
        assert_eq!(source.transport, TransportState::Wedged);

        // A restart gives the home a brand new app-server bridge, so the transport verdict must not
        // be inherited — but the dead subscription must be, or every blue-green handoff would
        // re-admit it and rediscover the same failure with customer traffic.
        let mut restored = HomeHealth::new();
        restored.restore(source.durable());
        assert_eq!(restored.account, AccountState::Dead);
        assert_eq!(restored.transport, TransportState::Responsive);
        assert!(restored.is_cooling(105));
    }

    #[test]
    fn an_unknown_persisted_verdict_is_treated_as_healthy() {
        // A row written by a newer binary must never quarantine a working subscription.
        assert_eq!(
            AccountState::from_str("something-new"),
            AccountState::Healthy
        );
        assert_eq!(AccountState::from_str("dead"), AccountState::Dead);
        assert_eq!(AccountState::from_str("suspect"), AccountState::Suspect);
    }

    #[test]
    fn losing_a_race_for_the_home_is_not_a_verdict_about_the_account() {
        // Ownership contention resolves the moment the other generation exits — it happens on every
        // blue-green handoff. Treating it as an account verdict was wrong twice: the subscription is
        // fine, and the verdict is durable, so seconds of ordinary contention would have outlived
        // the restart that cleared it and kept a healthy subscription out of the pool for good.
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::TransportClosed, 100);
        assert_eq!(health.account, AccountState::Healthy);
        assert_eq!(health.transport, TransportState::Wedged);
        // And it clears itself as soon as the home serves again.
        health.apply(HealthSignal::ProbeOk, 200);
        assert_eq!(health.transport, TransportState::Responsive);
        assert!(!health.is_cooling(200));
    }

    #[test]
    fn a_forced_probe_request_is_consumed_once() {
        let mut health = HomeHealth::new();
        health.apply(HealthSignal::Deadline, 100);
        assert!(health.wants_probe());
        assert!(health.take_probe_request());
        assert!(!health.wants_probe());
        assert!(!health.take_probe_request());
    }
}
