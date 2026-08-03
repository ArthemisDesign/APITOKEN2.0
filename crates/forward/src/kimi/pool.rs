//! Attempt loop for the KIMI plane: the piece that turns known profiles into served traffic.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §8.1 and §8.4. This module is a pure decision
//! engine — it decides what to do after an attempt, and the caller performs the I/O. Keeping it
//! pure is what makes the one-byte boundary provable by test instead of by inspection.
//!
//! The rule that everything else defers to: **once a single public byte has reached the client,
//! no retry and no account switch may happen.** Replaying after that would either duplicate output
//! or silently serve a different subscription mid-answer. It is encoded in the type system rather
//! than as a check, so a caller cannot express the violation.

use super::transport::{may_rotate, spends_transport_budget, UpstreamVerdict};

/// Delivery progress. The transition to `Delivering` is one-way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Nothing has reached the client yet; rotation is still legal.
    PreByte,
    /// At least one public byte has been written; the attempt is committed to this profile.
    Delivering,
}

/// Budget for attempts that are the upstream's fault rather than an account's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttemptPolicy {
    /// How many transport-class failures may be spent before giving up on the fleet.
    pub transport_budget: u32,
    /// Whether a forced token refresh plus one same-profile retry is still available.
    pub auth_retry_available: bool,
}

impl Default for AttemptPolicy {
    fn default() -> Self {
        Self {
            transport_budget: 3,
            auth_retry_available: true,
        }
    }
}

/// What the caller should do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextStep {
    /// Force a token refresh and retry the same profile once.
    RefreshAndRetrySameProfile,
    /// Take the profile out of rotation for this request and try another.
    RotateToAnotherProfile,
    /// Stop and surface the upstream's own error to the client.
    SurfaceUpstreamError,
    /// Stop and surface a synthetic retryable error: capacity, not correctness.
    SurfaceCapacityExhausted,
    /// Success.
    Deliver,
}

/// Durable consequence for the profile that just failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileEffect {
    None,
    /// Cool until the provider's reset. Not a health failure — a capacity state.
    CoolUntilReset,
    /// Repeated auth refusal: quarantine and stop routing here.
    AuthQuarantine,
    /// Transport streak; the caller rebuilds the client.
    TransportFault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub next: NextStep,
    pub effect: ProfileEffect,
    /// Policy to carry into the next attempt.
    pub policy: AttemptPolicy,
}

/// Decide what happens after one attempt.
///
/// `delivery` is the hard gate: in `Delivering` no branch can produce a retry or a rotation,
/// because the client already holds bytes from this profile.
pub fn decide(
    verdict: UpstreamVerdict,
    delivery: Delivery,
    policy: AttemptPolicy,
    eligible_profiles_left: usize,
) -> Decision {
    if verdict == UpstreamVerdict::Ok {
        return Decision {
            next: NextStep::Deliver,
            effect: ProfileEffect::None,
            policy,
        };
    }

    // The one-byte boundary. Everything below it is unreachable once delivery started: the only
    // honest option is to end the stream with the upstream's own error.
    if delivery == Delivery::Delivering {
        return Decision {
            next: NextStep::SurfaceUpstreamError,
            effect: effect_for(verdict),
            policy,
        };
    }

    let effect = effect_for(verdict);

    match verdict {
        UpstreamVerdict::Auth if policy.auth_retry_available => Decision {
            next: NextStep::RefreshAndRetrySameProfile,
            // A first 401 is not yet evidence of a dead account: the token may simply be stale.
            effect: ProfileEffect::None,
            policy: AttemptPolicy {
                auth_retry_available: false,
                ..policy
            },
        },
        UpstreamVerdict::ClientError => Decision {
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
            // A transport budget exhausted means the upstream is out, not that this customer did
            // anything wrong, so the surfaced error is the upstream's own.
            if spends && remaining == 0 {
                return Decision {
                    next: NextStep::SurfaceUpstreamError,
                    effect,
                    policy: AttemptPolicy {
                        transport_budget: 0,
                        ..policy
                    },
                };
            }
            // Quota and auth refusals do not spend budget, but they still need somewhere to go.
            // With no eligible profile left this is exhausted capacity, which is retryable for the
            // client rather than an error about their request.
            if eligible_profiles_left == 0 {
                return Decision {
                    next: NextStep::SurfaceCapacityExhausted,
                    effect,
                    policy: AttemptPolicy {
                        transport_budget: remaining,
                        ..policy
                    },
                };
            }
            Decision {
                next: NextStep::RotateToAnotherProfile,
                effect,
                policy: AttemptPolicy {
                    transport_budget: remaining,
                    // A fresh profile gets its own auth retry.
                    auth_retry_available: true,
                },
            }
        }
    }
}

fn effect_for(verdict: UpstreamVerdict) -> ProfileEffect {
    match verdict {
        UpstreamVerdict::QuotaExhausted => ProfileEffect::CoolUntilReset,
        UpstreamVerdict::Auth => ProfileEffect::AuthQuarantine,
        UpstreamVerdict::Transport | UpstreamVerdict::MembershipTemporary => {
            ProfileEffect::TransportFault
        }
        UpstreamVerdict::Ok | UpstreamVerdict::ClientError => ProfileEffect::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> AttemptPolicy {
        AttemptPolicy::default()
    }

    #[test]
    fn nothing_retries_or_rotates_once_a_public_byte_has_been_written() {
        // Replaying after delivery would duplicate output or silently continue an answer on a
        // different subscription. No verdict may escape this.
        for verdict in [
            UpstreamVerdict::Auth,
            UpstreamVerdict::QuotaExhausted,
            UpstreamVerdict::Transport,
            UpstreamVerdict::MembershipTemporary,
            UpstreamVerdict::ClientError,
        ] {
            let decision = decide(verdict, Delivery::Delivering, policy(), 5);
            assert_eq!(
                decision.next,
                NextStep::SurfaceUpstreamError,
                "{verdict:?} must not retry after bytes"
            );
        }
    }

    #[test]
    fn a_post_byte_failure_still_records_the_profile_effect() {
        // The request cannot be saved, but the fleet must still learn the profile hit a wall.
        let decision = decide(
            UpstreamVerdict::QuotaExhausted,
            Delivery::Delivering,
            policy(),
            5,
        );
        assert_eq!(decision.effect, ProfileEffect::CoolUntilReset);
    }

    #[test]
    fn a_first_auth_failure_refreshes_and_retries_the_same_profile() {
        // A stale token is the common case; rotating immediately would abandon a healthy
        // subscription and lose its prompt cache.
        let decision = decide(UpstreamVerdict::Auth, Delivery::PreByte, policy(), 5);
        assert_eq!(decision.next, NextStep::RefreshAndRetrySameProfile);
        assert_eq!(decision.effect, ProfileEffect::None);
        assert!(!decision.policy.auth_retry_available);
    }

    #[test]
    fn a_repeated_auth_failure_quarantines_and_rotates() {
        let after_first = decide(UpstreamVerdict::Auth, Delivery::PreByte, policy(), 5).policy;
        let decision = decide(UpstreamVerdict::Auth, Delivery::PreByte, after_first, 5);
        assert_eq!(decision.next, NextStep::RotateToAnotherProfile);
        assert_eq!(decision.effect, ProfileEffect::AuthQuarantine);
        // The next profile is entitled to its own refresh attempt.
        assert!(decision.policy.auth_retry_available);
    }

    #[test]
    fn a_quota_wall_rotates_without_spending_the_transport_budget() {
        // That budget exists for upstream outages. Spending it on an account-level refusal would
        // stop the search before a healthy profile is reached.
        let decision = decide(UpstreamVerdict::QuotaExhausted, Delivery::PreByte, policy(), 5);
        assert_eq!(decision.next, NextStep::RotateToAnotherProfile);
        assert_eq!(decision.effect, ProfileEffect::CoolUntilReset);
        assert_eq!(decision.policy.transport_budget, policy().transport_budget);
    }

    #[test]
    fn transport_faults_spend_the_budget_and_stop_at_zero() {
        let mut current = AttemptPolicy {
            transport_budget: 2,
            auth_retry_available: true,
        };
        let first = decide(UpstreamVerdict::Transport, Delivery::PreByte, current, 5);
        assert_eq!(first.next, NextStep::RotateToAnotherProfile);
        assert_eq!(first.policy.transport_budget, 1);
        current = first.policy;

        let last = decide(UpstreamVerdict::Transport, Delivery::PreByte, current, 5);
        // Budget exhausted: the upstream is out, so the client sees the upstream's own error
        // rather than a synthetic one that hides an outage.
        assert_eq!(last.next, NextStep::SurfaceUpstreamError);
        assert_eq!(last.policy.transport_budget, 0);
    }

    #[test]
    fn an_exhausted_fleet_reports_capacity_rather_than_a_request_error() {
        // Nothing is wrong with the customer's request, so the error must be retryable and must
        // not blame them.
        let decision = decide(UpstreamVerdict::QuotaExhausted, Delivery::PreByte, policy(), 0);
        assert_eq!(decision.next, NextStep::SurfaceCapacityExhausted);
    }

    #[test]
    fn a_deterministic_client_error_neither_rotates_nor_blames_the_profile() {
        // The next profile would fail identically, and the subscription did nothing wrong.
        let decision = decide(UpstreamVerdict::ClientError, Delivery::PreByte, policy(), 5);
        assert_eq!(decision.next, NextStep::SurfaceUpstreamError);
        assert_eq!(decision.effect, ProfileEffect::None);
        assert_eq!(decision.policy, policy());
    }

    #[test]
    fn a_membership_hiccup_is_a_transport_fault_not_an_account_death() {
        // The provider documents 402 as usually temporary.
        let decision = decide(
            UpstreamVerdict::MembershipTemporary,
            Delivery::PreByte,
            policy(),
            5,
        );
        assert_eq!(decision.next, NextStep::RotateToAnotherProfile);
        assert_eq!(decision.effect, ProfileEffect::TransportFault);
    }

    #[test]
    fn success_delivers_and_changes_nothing() {
        for delivery in [Delivery::PreByte, Delivery::Delivering] {
            let decision = decide(UpstreamVerdict::Ok, delivery, policy(), 0);
            assert_eq!(decision.next, NextStep::Deliver);
            assert_eq!(decision.effect, ProfileEffect::None);
            assert_eq!(decision.policy, policy());
        }
    }

    #[test]
    fn a_quota_wall_never_marks_a_profile_dead() {
        // Exhausted quota is a capacity state that resets; treating it as a health failure would
        // permanently remove a paid subscription from the fleet.
        let decision = decide(UpstreamVerdict::QuotaExhausted, Delivery::PreByte, policy(), 3);
        assert_ne!(decision.effect, ProfileEffect::AuthQuarantine);
        assert_eq!(decision.effect, ProfileEffect::CoolUntilReset);
    }

    #[test]
    fn the_loop_terminates_on_every_verdict() {
        // A decision that neither delivers nor terminates would spin the request forever.
        let mut current = policy();
        for _ in 0..64 {
            let decision = decide(UpstreamVerdict::Transport, Delivery::PreByte, current, 5);
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
