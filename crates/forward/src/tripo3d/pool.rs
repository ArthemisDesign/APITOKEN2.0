//! Attempt loop for the Tripo3D (VAST / Holymolly) prepaid API plane: the piece that turns
//! known profiles into served tasks.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §8.1 and §8.4, adapted to a task-based media
//! API (`docs/engine/TRIPO3D_PROVIDER.md` §4). This module is a pure decision engine — it
//! decides what to do after an attempt, and the caller performs the I/O.
//!
//! Two rules everything else defers to:
//!
//! * **The money boundary is a successful task creation, not a byte.** Until `code: 0 +
//!   task_id` the provider created nothing, so retry/rotation across profiles is legal. After
//!   it, the task is owned by the creating profile (tasks are per-key isolated, manifest §2),
//!   the reservation is delivering, and no rotation can ever happen — the phase transition is
//!   one-way and encoded in the type system.
//! * **There is no auth retry and no auth verdict.** The credential is a static key with no
//!   refresh family: repeating a refused request with the same key is pointless, so the loop
//!   rotates away. But a 401 is SOFT (`docs/engine/TRIPO3D_PROVIDER.md` §4.1): it quarantines
//!   with exponential backoff and never, on its own, removes the profile from the fleet.

use super::transport::{may_rotate, spends_transport_budget, UpstreamVerdict};

/// Lifecycle phase of one request. The transition to `TaskCreated` is one-way: the upstream
/// task exists, is queryable only by the creating key, and from now on the drain (poll →
/// download → settle) is detached and never cancelled by a client disconnect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing was created upstream yet; rotation is still legal.
    BeforeCreate,
    /// The provider returned `code: 0 + task_id`; the reservation is committed to this profile.
    TaskCreated,
}

/// Budget for attempts that are the upstream's fault rather than an account's.
///
/// There is deliberately no `auth_retry_available` field (KIMI has one): a static key cannot
/// be refreshed, so there is nothing to retry on the same profile after a refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttemptPolicy {
    /// How many transport-class failures may be spent before giving up on the fleet.
    pub transport_budget: u32,
}

impl Default for AttemptPolicy {
    fn default() -> Self {
        Self {
            transport_budget: 3,
        }
    }
}

/// What the caller should do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextStep {
    /// Take the profile out of rotation for this request and try another.
    RotateToAnotherProfile,
    /// Stop and surface the upstream's own error to the client.
    SurfaceUpstreamError,
    /// Stop and surface a synthetic retryable error: capacity, not correctness. Carries the
    /// `Retry-After` the honest 429 must have (never an invented 503).
    SurfaceCapacityExhausted,
    /// The task was created; proceed to the detached drain.
    Created,
}

/// Consequence for the profile that just answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileEffect {
    None,
    /// HARD axis: cool until the provider's `Retry-After` (429 + code 2000).
    CoolRateLimited,
    /// HARD axis: the provider said the balance cannot create tasks (403 + code 2010). Rest
    /// until a free balance probe shows funds — no timer clears a money verdict.
    RestForBalance,
    /// SOFT axis: a 401 (or codeless 403). Exponential backoff from a small base, reset on any
    /// proven success. Never removes the profile from the fleet on its own.
    SoftAuthFault,
    /// SOFT axis: transport streak; the caller cools briefly.
    TransportFault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub next: NextStep,
    pub effect: ProfileEffect,
    /// Policy to carry into the next attempt.
    pub policy: AttemptPolicy,
}

/// Decide what happens after one creation attempt.
///
/// `phase` is the hard gate: in `TaskCreated` no branch can produce a retry or a rotation,
/// because the provider already owns a task for this request on this profile.
pub fn decide(
    verdict: UpstreamVerdict,
    phase: Phase,
    policy: AttemptPolicy,
    eligible_profiles_left: usize,
) -> Decision {
    if verdict == UpstreamVerdict::Ok {
        return Decision {
            next: NextStep::Created,
            effect: ProfileEffect::None,
            policy,
        };
    }

    // The money boundary. Everything below it is unreachable once the task exists: the only
    // honest option is to surface the failure — the detached drain owns the task's settlement.
    if phase == Phase::TaskCreated {
        return Decision {
            next: NextStep::SurfaceUpstreamError,
            effect: effect_for(verdict),
            policy,
        };
    }

    let effect = effect_for(verdict);

    match verdict {
        UpstreamVerdict::ClientError | UpstreamVerdict::Protocol => Decision {
            next: NextStep::SurfaceUpstreamError,
            effect: ProfileEffect::None,
            policy,
        },
        _ if !may_rotate(verdict) => Decision {
            next: NextStep::SurfaceUpstreamError,
            effect,
            policy,
        },
        _ => {
            let spends = spends_transport_budget(verdict);
            let remaining = if spends {
                policy.transport_budget.saturating_sub(1)
            } else {
                policy.transport_budget
            };
            // A transport budget exhausted means the upstream is out, not that this customer
            // did anything wrong, so the surfaced error is the upstream's own.
            if spends && remaining == 0 {
                return Decision {
                    next: NextStep::SurfaceUpstreamError,
                    effect,
                    policy: AttemptPolicy {
                        transport_budget: 0,
                    },
                };
            }
            // Provider verdicts and soft faults do not spend budget, but they still need
            // somewhere to go. With no eligible profile left this is exhausted capacity, which
            // is retryable for the client rather than an error about their request.
            if eligible_profiles_left == 0 {
                return Decision {
                    next: NextStep::SurfaceCapacityExhausted,
                    effect,
                    policy: AttemptPolicy {
                        transport_budget: remaining,
                    },
                };
            }
            Decision {
                next: NextStep::RotateToAnotherProfile,
                effect,
                policy: AttemptPolicy {
                    transport_budget: remaining,
                },
            }
        }
    }
}

fn effect_for(verdict: UpstreamVerdict) -> ProfileEffect {
    match verdict {
        UpstreamVerdict::RateLimitedHard => ProfileEffect::CoolRateLimited,
        UpstreamVerdict::InsufficientBalance => ProfileEffect::RestForBalance,
        UpstreamVerdict::AuthRefused => ProfileEffect::SoftAuthFault,
        UpstreamVerdict::Transport => ProfileEffect::TransportFault,
        UpstreamVerdict::Ok | UpstreamVerdict::ClientError | UpstreamVerdict::Protocol => {
            ProfileEffect::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> AttemptPolicy {
        AttemptPolicy::default()
    }

    #[test]
    fn nothing_retries_or_rotates_once_the_task_exists() {
        // The task is queryable only by the creating key: after creation no other profile can
        // even SEE it, so rotation is impossible by construction.
        for verdict in [
            UpstreamVerdict::RateLimitedHard,
            UpstreamVerdict::InsufficientBalance,
            UpstreamVerdict::AuthRefused,
            UpstreamVerdict::Transport,
            UpstreamVerdict::ClientError,
            UpstreamVerdict::Protocol,
        ] {
            let decision = decide(verdict, Phase::TaskCreated, policy(), 5);
            assert_eq!(
                decision.next,
                NextStep::SurfaceUpstreamError,
                "{verdict:?} must not retry after the money boundary"
            );
        }
    }

    #[test]
    fn a_post_create_failure_still_records_the_profile_effect() {
        // The request cannot be saved, but the fleet must still learn the profile hit a wall.
        let decision = decide(
            UpstreamVerdict::InsufficientBalance,
            Phase::TaskCreated,
            policy(),
            5,
        );
        assert_eq!(decision.effect, ProfileEffect::RestForBalance);
    }

    #[test]
    fn hard_verdicts_rotate_without_spending_the_transport_budget() {
        for (verdict, effect) in [
            (UpstreamVerdict::RateLimitedHard, ProfileEffect::CoolRateLimited),
            (
                UpstreamVerdict::InsufficientBalance,
                ProfileEffect::RestForBalance,
            ),
        ] {
            let decision = decide(verdict, Phase::BeforeCreate, policy(), 5);
            assert_eq!(decision.next, NextStep::RotateToAnotherProfile, "{verdict:?}");
            assert_eq!(decision.effect, effect, "{verdict:?}");
            assert_eq!(decision.policy.transport_budget, policy().transport_budget);
        }
    }

    #[test]
    fn a_first_auth_refusal_rotates_immediately_and_stays_soft() {
        // A static key cannot be refreshed: a repeated 401 on the same profile is pointless,
        // so unlike KIMI there is no RefreshAndRetrySameProfile step — and the axis stays
        // soft, never a fleet-removing verdict.
        let decision = decide(UpstreamVerdict::AuthRefused, Phase::BeforeCreate, policy(), 5);
        assert_eq!(decision.next, NextStep::RotateToAnotherProfile);
        assert_eq!(decision.effect, ProfileEffect::SoftAuthFault);
        assert_eq!(decision.policy.transport_budget, policy().transport_budget);
    }

    #[test]
    fn transport_faults_spend_the_budget_and_stop_at_zero() {
        let mut current = policy();
        for expected_remaining in [2, 1] {
            let decision = decide(UpstreamVerdict::Transport, Phase::BeforeCreate, current, 5);
            assert_eq!(decision.next, NextStep::RotateToAnotherProfile);
            assert_eq!(decision.effect, ProfileEffect::TransportFault);
            assert_eq!(decision.policy.transport_budget, expected_remaining);
            current = decision.policy;
        }
        let last = decide(UpstreamVerdict::Transport, Phase::BeforeCreate, current, 5);
        // Budget exhausted: the upstream is out, so the client sees the upstream's own error
        // rather than a synthetic one that hides an outage.
        assert_eq!(last.next, NextStep::SurfaceUpstreamError);
        assert_eq!(last.policy.transport_budget, 0);
    }

    #[test]
    fn an_exhausted_fleet_reports_capacity_rather_than_a_request_error() {
        // Nothing is wrong with the customer's request, so the error must be retryable and
        // must not blame them: an honest 429 with Retry-After, never an invented 503.
        let decision = decide(
            UpstreamVerdict::InsufficientBalance,
            Phase::BeforeCreate,
            policy(),
            0,
        );
        assert_eq!(decision.next, NextStep::SurfaceCapacityExhausted);
        assert_eq!(decision.effect, ProfileEffect::RestForBalance);
    }

    #[test]
    fn a_deterministic_client_error_neither_rotates_nor_blames_the_profile() {
        // The next profile would fail identically, and the subscription did nothing wrong.
        let decision = decide(UpstreamVerdict::ClientError, Phase::BeforeCreate, policy(), 5);
        assert_eq!(decision.next, NextStep::SurfaceUpstreamError);
        assert_eq!(decision.effect, ProfileEffect::None);
        assert_eq!(decision.policy, policy());
    }

    #[test]
    fn a_protocol_anomaly_never_rotates_into_a_possible_double_create() {
        // A lying or changed success envelope on a paid boundary needs review, not a retry:
        // the first attempt may already have created the task.
        let decision = decide(UpstreamVerdict::Protocol, Phase::BeforeCreate, policy(), 5);
        assert_eq!(decision.next, NextStep::SurfaceUpstreamError);
        assert_eq!(decision.effect, ProfileEffect::None);
    }

    #[test]
    fn success_creates_and_changes_nothing() {
        for phase in [Phase::BeforeCreate, Phase::TaskCreated] {
            let decision = decide(UpstreamVerdict::Ok, phase, policy(), 0);
            assert_eq!(decision.next, NextStep::Created);
            assert_eq!(decision.effect, ProfileEffect::None);
            assert_eq!(decision.policy, policy());
        }
    }

    #[test]
    fn the_loop_terminates_on_every_verdict() {
        // A decision that neither creates nor terminates would spin the request forever.
        let mut current = policy();
        for _ in 0..64 {
            let decision = decide(UpstreamVerdict::Transport, Phase::BeforeCreate, current, 5);
            current = decision.policy;
            if matches!(
                decision.next,
                NextStep::SurfaceUpstreamError | NextStep::SurfaceCapacityExhausted
            ) {
                return;
            }
        }
        panic!("attempt loop never terminated");
    }
}
