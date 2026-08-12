//! Attempt loop for the Suno (suno.com) subscription session-pool plane: the piece that turns
//! known profiles into served generations.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §8.1 and §8.4, adapted to a task-based media
//! API (`docs/engine/SUNO_PROVIDER.md` §4). This module is a pure decision engine — it decides
//! what to do after an attempt, and the caller performs the I/O.
//!
//! Two rules everything else defers to:
//!
//! * **The money boundary is a successful generation creation, not a byte.** Until the provider
//!   accepts the create call the provider created nothing, so retry/rotation across profiles is
//!   legal. After it, the generation belongs to the creating profile's account, the reservation
//!   is delivering, and no rotation can ever happen — the phase transition is one-way and
//!   encoded in the type system.
//! * **A CAPTCHA gate is a soft axis, never a customer error and never something to solve.**
//!   When `/api/c/check` answers `required: true` the profile soft-cools and the attempt
//!   rotates (manifest §4); a persistent gate is an operational state. Likewise a 401/403 after
//!   a successful JWT mint is SOFT (`docs/engine/SUNO_PROVIDER.md` §4.1): it quarantines with
//!   exponential backoff and never, on its own, removes the profile from the fleet.

use super::transport::{may_rotate, spends_transport_budget, UpstreamVerdict};

/// Lifecycle phase of one request. The transition to `GenerationCreated` is one-way: the
/// upstream generation exists on the creating account, and from now on the drain (poll →
/// download → settle) is detached and never cancelled by a client disconnect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing was created upstream yet; rotation is still legal.
    BeforeCreate,
    /// The provider accepted the create call; the reservation is committed to this profile.
    GenerationCreated,
}

/// Budget for attempts that are the upstream's fault rather than an account's.
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
    /// The generation was created; proceed to the detached drain.
    Created,
}

/// Consequence for the profile that just answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileEffect {
    None,
    /// HARD axis: a 429 rate wall; the caller cools the profile for a bounded window.
    CoolRateLimited,
    /// HARD axis: explicit quota exhaustion. Rest until a free billing probe shows credits —
    /// no timer clears a money verdict.
    RestForQuota,
    /// SOFT axis: a 401/403 after a successful mint. Exponential backoff from a small base,
    /// reset on any proven success. Never removes the profile from the fleet on its own.
    SoftAuthFault,
    /// SOFT axis: the hCaptcha pre-check answered `required: true`. Brief cool + rotate; no
    /// CAPTCHA is ever solved.
    SoftCaptchaGate,
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
/// `phase` is the hard gate: in `GenerationCreated` no branch can produce a retry or a
/// rotation, because the provider already owns a generation for this request on this profile.
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

    // The money boundary. Everything below it is unreachable once the generation exists: the
    // only honest option is to surface the failure — the detached drain owns the generation's
    // settlement.
    if phase == Phase::GenerationCreated {
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
        UpstreamVerdict::QuotaExhausted => ProfileEffect::RestForQuota,
        UpstreamVerdict::AuthRefused => ProfileEffect::SoftAuthFault,
        UpstreamVerdict::CaptchaRequired => ProfileEffect::SoftCaptchaGate,
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
    fn nothing_retries_or_rotates_once_the_generation_exists() {
        // The generation lives on the creating account: after creation no other profile can
        // even SEE it, so rotation is impossible by construction.
        for verdict in [
            UpstreamVerdict::RateLimitedHard,
            UpstreamVerdict::QuotaExhausted,
            UpstreamVerdict::AuthRefused,
            UpstreamVerdict::CaptchaRequired,
            UpstreamVerdict::Transport,
            UpstreamVerdict::ClientError,
            UpstreamVerdict::Protocol,
        ] {
            let decision = decide(verdict, Phase::GenerationCreated, policy(), 5);
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
            UpstreamVerdict::QuotaExhausted,
            Phase::GenerationCreated,
            policy(),
            5,
        );
        assert_eq!(decision.effect, ProfileEffect::RestForQuota);
    }

    #[test]
    fn hard_verdicts_rotate_without_spending_the_transport_budget() {
        for (verdict, effect) in [
            (UpstreamVerdict::RateLimitedHard, ProfileEffect::CoolRateLimited),
            (UpstreamVerdict::QuotaExhausted, ProfileEffect::RestForQuota),
        ] {
            let decision = decide(verdict, Phase::BeforeCreate, policy(), 5);
            assert_eq!(decision.next, NextStep::RotateToAnotherProfile, "{verdict:?}");
            assert_eq!(decision.effect, effect, "{verdict:?}");
            assert_eq!(decision.policy.transport_budget, policy().transport_budget);
        }
    }

    #[test]
    fn a_captcha_gate_soft_cools_and_rotates_without_solving() {
        // No CAPTCHA solving exists by design: `required: true` cools the profile briefly and
        // the attempt rotates; a persistent gate is operational, never a customer error.
        let decision = decide(
            UpstreamVerdict::CaptchaRequired,
            Phase::BeforeCreate,
            policy(),
            5,
        );
        assert_eq!(decision.next, NextStep::RotateToAnotherProfile);
        assert_eq!(decision.effect, ProfileEffect::SoftCaptchaGate);
        assert_eq!(decision.policy.transport_budget, policy().transport_budget);
    }

    #[test]
    fn a_first_auth_refusal_rotates_immediately_and_stays_soft() {
        // The mint succeeded, so the refusal may belong to the request path: rotate away, and
        // the axis stays soft — never a fleet-removing verdict.
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
            UpstreamVerdict::QuotaExhausted,
            Phase::BeforeCreate,
            policy(),
            0,
        );
        assert_eq!(decision.next, NextStep::SurfaceCapacityExhausted);
        assert_eq!(decision.effect, ProfileEffect::RestForQuota);
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
        // the first attempt may already have created the generation.
        let decision = decide(UpstreamVerdict::Protocol, Phase::BeforeCreate, policy(), 5);
        assert_eq!(decision.next, NextStep::SurfaceUpstreamError);
        assert_eq!(decision.effect, ProfileEffect::None);
    }

    #[test]
    fn success_creates_and_changes_nothing() {
        for phase in [Phase::BeforeCreate, Phase::GenerationCreated] {
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
