//! Profile selection for the Suno (suno.com) subscription session-pool plane.
//!
//! Pure state machine: no HTTP, no clock reads, no I/O. Contract:
//! `docs/engine/PROVIDER_ONBOARDING.md` §8.2, §8.3 and §8.4.
//!
//! Two rules shape everything here:
//!
//! * **There is no local concurrency limit.** Every admitted request immediately starts an
//!   upstream attempt; `inflight` is a placement signal, not a ceiling. (The provider-side
//!   per-plan concurrency, manifest §1, surfaces as a 429 — the hard axis, not a local cap.)
//! * **The pool-must-not-empty invariant is structural.** Cooling splits into a HARD axis
//!   (provider verdicts: a 429 rate wall, explicit quota exhaustion visible via the billing
//!   probe zeroing, a proven quota observation that cannot cover the reserve) and a SOFT axis
//!   (our own inferences: a 401/403 after a successful mint, a CAPTCHA-required pre-check,
//!   transport faults). Only the hard axis may deny a request: when the strict pass is empty
//!   the caller re-selects through [`select_ignoring_soft`], bounded by its already-tried set.
//!   A subscription is removed from routable only on an unambiguous provider verdict — never
//!   on our own inference.

use std::collections::HashSet;

/// Hard ineligibility: the provider said so. Only this axis may deny admission on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hard {
    /// HTTP 429: a rate/concurrency wall (paid plans document bounded concurrency,
    /// manifest §1). A capacity state, not a health failure.
    RateLimited,
    /// Explicit quota exhaustion: the billing probe shows the profile cannot spend, or an
    /// upstream answer was proven to mean "no credits". The profile rests until a free billing
    /// probe shows credits again — no timer clears a money verdict.
    QuotaExhausted,
    /// The latest PROVEN quota observation cannot cover this request's reserve. Inert while
    /// the quota fields are unproven (the parsed halves are `None`, manifest §5.2) — unknown
    /// capacity is neutral, never treated as zero.
    QuotaShortfall,
}

/// Soft ineligibility: we inferred something about the environment. Recoverable on its own,
/// backs off exponentially, and can never deny admission by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Soft {
    /// The session was refused (401/403) after a successful JWT mint. JWT minting is keep-alive,
    /// not a repair for a dead session — but one refusal is never a verdict: bounded quarantine
    /// + probe.
    AuthCooling,
    /// The hCaptcha pre-check answered `required: true`. No CAPTCHA solving exists by design
    /// (manifest §4), so the profile rests briefly and the attempt rotates.
    CaptchaRequired,
    /// Transport faults, timeouts, failed probes.
    TransportWedged,
}

/// A candidate profile as the selector sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub profile_id: String,
    pub hard: Option<Hard>,
    pub soft: Option<Soft>,
    /// In-flight requests. A placement signal only — never a ceiling.
    pub inflight: u32,
}

impl Candidate {
    /// Strictly eligible: no axis of either kind is active.
    fn is_eligible(&self) -> bool {
        self.hard.is_none() && self.soft.is_none()
    }

    /// Eligible when only the hard axis is honored. The soft axis can slow a profile down but
    /// must never empty the pool (PROVIDER_ONBOARDING §8.4).
    fn is_hard_eligible(&self) -> bool {
        self.hard.is_none()
    }
}

/// Choose a profile for a request under the strict rule (both axes honored).
///
/// `cursor` is an atomic rotation counter supplied by the caller; equal candidates alternate
/// through it so a burst spreads instead of herding onto one subscription. There is no sticky
/// affinity at selection time: the generation binds to the creating account at creation, after
/// which the drain is pinned to its profile by construction.
pub fn select<'a>(candidates: &'a [Candidate], cursor: u64) -> Option<&'a Candidate> {
    pick(candidates.iter().filter(|c| c.is_eligible()), cursor)
}

/// Choose a profile honoring only the hard axis. The caller reaches this pass when the strict
/// pass returned nothing; it is what makes a full-soft-cooling fleet keep serving.
pub fn select_ignoring_soft<'a>(
    candidates: &'a [Candidate],
    cursor: u64,
) -> Option<&'a Candidate> {
    pick(candidates.iter().filter(|c| c.is_hard_eligible()), cursor)
}

fn pick<'a>(eligible: impl Iterator<Item = &'a Candidate>, cursor: u64) -> Option<&'a Candidate> {
    let eligible: Vec<&Candidate> = eligible.collect();
    if eligible.is_empty() {
        return None;
    }
    let best = eligible.iter().map(|candidate| candidate.inflight).min()?;
    // Ties rotate, so identical candidates do not all receive the same burst. The tied set is
    // sorted by id purely to make the cursor's choice deterministic across processes; the id
    // must NOT take part in the rank itself, or no two candidates would ever tie.
    let mut tied: Vec<&Candidate> = eligible
        .into_iter()
        .filter(|candidate| candidate.inflight == best)
        .collect();
    tied.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    let index = (cursor % tied.len() as u64) as usize;
    Some(tied[index])
}

/// Profiles that are out of strict rotation, for observability.
pub fn ineligible_ids(candidates: &[Candidate]) -> HashSet<&str> {
    candidates
        .iter()
        .filter(|candidate| !candidate.is_eligible())
        .map(|candidate| candidate.profile_id.as_str())
        .collect()
}

/// Profiles even the hard-only pass cannot use: the fleet is genuinely out when this covers
/// every roster profile, and the honest answer is a 429 with `Retry-After`.
pub fn hard_ineligible_ids(candidates: &[Candidate]) -> HashSet<&str> {
    candidates
        .iter()
        .filter(|candidate| !candidate.is_hard_eligible())
        .map(|candidate| candidate.profile_id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str) -> Candidate {
        Candidate {
            profile_id: id.into(),
            hard: None,
            soft: None,
            inflight: 0,
        }
    }

    #[test]
    fn load_spreads_and_inflight_is_a_signal_never_a_ceiling() {
        let mut loaded = candidate("suno-01");
        loaded.inflight = 7;
        let candidates = vec![loaded, candidate("suno-02")];
        assert_eq!(select(&candidates, 0).unwrap().profile_id, "suno-02");

        // Even a heavily loaded fleet must still serve: no local concurrency limit exists, so
        // a request is never refused for being the Nth in flight.
        let mut a = candidate("suno-01");
        let mut b = candidate("suno-02");
        a.inflight = 10_000;
        b.inflight = 10_000;
        let candidates = vec![a, b];
        assert!(select(&candidates, 0).is_some());
    }

    #[test]
    fn equal_candidates_rotate_through_the_cursor_and_ties_can_exist() {
        let candidates = vec![candidate("suno-01"), candidate("suno-02")];
        let first = select(&candidates, 0).unwrap().profile_id.clone();
        let second = select(&candidates, 1).unwrap().profile_id.clone();
        assert_ne!(first, second, "a burst must not herd onto one subscription");
        assert_eq!(select(&candidates, 2).unwrap().profile_id, first);
    }

    #[test]
    fn a_full_soft_cooling_fleet_still_serves_through_the_hard_only_pass() {
        // THE pool-must-not-empty invariant: soft axes may never empty the pool. The strict
        // pass is empty, the hard-only pass still selects.
        let mut a = candidate("suno-01");
        a.soft = Some(Soft::AuthCooling);
        let mut b = candidate("suno-02");
        b.soft = Some(Soft::CaptchaRequired);
        let mut c = candidate("suno-03");
        c.soft = Some(Soft::TransportWedged);
        let candidates = vec![a, b, c];
        assert!(select(&candidates, 0).is_none());
        assert!(select_ignoring_soft(&candidates, 0).is_some());
    }

    #[test]
    fn a_full_hard_wall_fleet_selects_nothing_on_both_passes() {
        // Real provider limits: the honest answer upstream is a 429 with Retry-After, never a
        // 503 invented from an environmental guess.
        for hard in [Hard::RateLimited, Hard::QuotaExhausted, Hard::QuotaShortfall] {
            let mut walled = candidate("suno-01");
            walled.hard = Some(hard);
            let candidates = vec![walled];
            assert!(select(&candidates, 0).is_none(), "{hard:?}");
            assert!(select_ignoring_soft(&candidates, 0).is_none(), "{hard:?}");
        }
    }

    #[test]
    fn hard_and_soft_axes_clear_independently() {
        // A profile under both axes becomes available to the strict pass only when BOTH clear;
        // clearing the soft axis alone must not bypass a real provider wall.
        let mut both = candidate("suno-01");
        both.hard = Some(Hard::QuotaExhausted);
        both.soft = Some(Soft::CaptchaRequired);
        let candidates = vec![both];
        assert!(select_ignoring_soft(&candidates, 0).is_none());
        assert_eq!(ineligible_ids(&candidates).len(), 1);
        assert_eq!(hard_ineligible_ids(&candidates).len(), 1);
    }

    #[test]
    fn an_empty_fleet_selects_nothing_and_one_usable_profile_is_capacity() {
        assert!(select(&[], 0).is_none());
        assert!(select_ignoring_soft(&[], 0).is_none());
        // No arbitrary minimum fleet size: a single working profile must serve.
        assert_eq!(
            select(&[candidate("suno-01")], 0).unwrap().profile_id,
            "suno-01"
        );
    }
}
