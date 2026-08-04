//! Profile selection for the GLM (Zhipu AI / Z.ai) Coding Plan plane.
//!
//! Pure state machine: no HTTP, no clock reads, no I/O. Contract:
//! `docs/engine/PROVIDER_ONBOARDING.md` §8.2 and §8.3.
//!
//! The rule that shapes everything here is that **there is no local concurrency limit**. Every
//! admitted request immediately starts an upstream attempt; `inflight` is a placement signal,
//! not a ceiling. A process-local semaphore or admission queue would manufacture a wait that
//! the provider never asked for, and would turn our own saturation into a customer-visible
//! error.

use std::collections::HashSet;

/// Why a profile cannot serve right now.
///
/// The cooling axes are deliberately separate (`docs/engine/PROVIDER_ONBOARDING.md` §8.4):
/// account health, provider quota, model scope and transport wedging clear independently and
/// must not share a timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ineligible {
    /// Provider said the plan quota is out; cooling until the window's reset. A capacity
    /// state, not a health failure.
    QuotaWall,
    /// The static key was refused or the plan expired. Out of rotation until the Auth Bot
    /// publishes a replacement key — there is no refresh, so nothing else can clear this.
    AccountDead,
    /// Risk-control fair-use or account anomaly. Out of rotation pending operator review, but
    /// recoverable — deliberately not dead.
    AccountSuspect,
    /// The requested model is outside this key's plan scope. The caller builds the candidate
    /// list per request, so the same profile can keep serving the models it does grant.
    ModelIneligible,
    /// Transport is wedged and being rebuilt.
    TransportWedged,
}

/// A candidate profile as the selector sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub profile_id: String,
    /// `None` while the profile is serving.
    pub ineligible: Option<Ineligible>,
    /// Used fraction of the tightest window, in `10^-8` units. `None` means never observed.
    pub used_fraction_units: Option<i64>,
    /// Observation age in seconds; larger is staler. `None` pairs with a never-observed quota.
    pub quota_age_secs: Option<i64>,
    /// In-flight requests. A placement signal only — never a ceiling.
    pub inflight: u32,
}

impl Candidate {
    fn is_eligible(&self) -> bool {
        self.ineligible.is_none()
    }
}

/// Fixed-point scale shared with the calibration authority.
const FRACTION_SCALE: i64 = 100_000_000;
/// Quota steering only kicks in near the wall; below it, load balancing decides.
const STEERING_THRESHOLD: i64 = FRACTION_SCALE / 2;
/// An observation older than this is stale and loses to a fresh one.
const STALE_AFTER_SECS: i64 = 300;

/// Choose a profile for a request.
///
/// `sticky` is a resolved conversation affinity. It wins outright while it is eligible,
/// because breaking affinity throws away the provider-side prompt cache the conversation has
/// built up — which costs real money on the very next turn.
///
/// `cursor` is an atomic rotation counter supplied by the caller; equal candidates alternate
/// through it so a burst of new conversations spreads instead of herding onto one
/// subscription.
pub fn select<'a>(
    candidates: &'a [Candidate],
    sticky: Option<&str>,
    cursor: u64,
) -> Option<&'a Candidate> {
    if let Some(sticky_id) = sticky {
        if let Some(pinned) = candidates
            .iter()
            .find(|candidate| candidate.profile_id == sticky_id && candidate.is_eligible())
        {
            return Some(pinned);
        }
    }

    let eligible: Vec<&Candidate> = candidates
        .iter()
        .filter(|candidate| candidate.is_eligible())
        .collect();
    if eligible.is_empty() {
        return None;
    }

    let best = eligible.iter().copied().map(rank).min()?;

    // Ties rotate, so identical candidates do not all receive the same burst. The tied set is
    // sorted by id purely to make the cursor's choice deterministic across processes; the id
    // must NOT take part in `rank` itself, or no two candidates would ever tie and the cursor
    // would never rotate at all.
    let mut tied: Vec<&Candidate> = eligible
        .into_iter()
        .filter(|candidate| rank(candidate) == best)
        .collect();
    tied.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    let index = (cursor % tied.len() as u64) as usize;
    Some(tied[index])
}

/// Ordering key. Lower is better; every component is an integer so ordering is total and
/// stable.
///
/// The profile id is deliberately absent. Including it would make every candidate distinct,
/// so nothing would ever tie and the rotation cursor below would be dead code — every burst
/// would land on the alphabetically first subscription.
fn rank(candidate: &Candidate) -> (u8, u32, i64) {
    // 1. Fresh evidence beats stale. A never-observed profile is neutral rather than worst: it
    //    may well be empty, and treating it as full would leave new capacity permanently idle.
    let freshness = match candidate.quota_age_secs {
        Some(age) if age <= STALE_AFTER_SECS => 0,
        None => 1,
        Some(_) => 2,
    };
    // 2. Then load, so a burst spreads across the fleet.
    // 3. Coarse quota steering applies only near the wall; below the threshold every profile
    //    scores equal so exact fractions cannot herd traffic onto whoever looks emptiest.
    let steering = match candidate.used_fraction_units {
        Some(used) if used >= STEERING_THRESHOLD => used,
        _ => 0,
    };
    (freshness, candidate.inflight, steering)
}

/// Profiles that are out of rotation, for observability.
pub fn ineligible_ids(candidates: &[Candidate]) -> HashSet<&str> {
    candidates
        .iter()
        .filter(|candidate| !candidate.is_eligible())
        .map(|candidate| candidate.profile_id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str) -> Candidate {
        Candidate {
            profile_id: id.into(),
            ineligible: None,
            used_fraction_units: Some(0),
            quota_age_secs: Some(0),
            inflight: 0,
        }
    }

    #[test]
    fn resolved_affinity_wins_outright_while_it_is_eligible() {
        // Breaking affinity discards the provider-side prompt cache and costs money next turn.
        let mut busy = candidate("glm-01");
        busy.inflight = 50;
        let idle = candidate("glm-02");
        let candidates = vec![busy, idle];
        assert_eq!(
            select(&candidates, Some("glm-01"), 0).unwrap().profile_id,
            "glm-01"
        );
    }

    #[test]
    fn a_walled_sticky_profile_falls_through_instead_of_failing_the_request() {
        let mut walled = candidate("glm-01");
        walled.ineligible = Some(Ineligible::QuotaWall);
        let candidates = vec![walled, candidate("glm-02")];
        assert_eq!(
            select(&candidates, Some("glm-01"), 0).unwrap().profile_id,
            "glm-02"
        );
    }

    #[test]
    fn load_spreads_and_inflight_is_a_signal_never_a_ceiling() {
        let mut loaded = candidate("glm-01");
        loaded.inflight = 7;
        let candidates = vec![loaded, candidate("glm-02")];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "glm-02");

        // Even a heavily loaded fleet must still serve: no local concurrency limit exists, so
        // a request is never refused for being the Nth in flight.
        let mut a = candidate("glm-01");
        let mut b = candidate("glm-02");
        a.inflight = 10_000;
        b.inflight = 10_000;
        let candidates = vec![a, b];
        assert!(select(&candidates, None, 0).is_some());
    }

    #[test]
    fn fresh_evidence_beats_stale_and_a_never_seen_profile_stays_neutral() {
        let mut fresh = candidate("glm-fresh");
        fresh.quota_age_secs = Some(10);
        fresh.used_fraction_units = Some(90_000_000);
        let mut stale = candidate("glm-stale");
        stale.quota_age_secs = Some(10_000);
        stale.used_fraction_units = Some(0);
        let mut unseen = candidate("glm-unseen");
        unseen.quota_age_secs = None;
        unseen.used_fraction_units = None;

        // Fresh wins even though it looks fuller: stale emptiness is not evidence.
        let candidates = vec![stale.clone(), fresh.clone()];
        assert_eq!(
            select(&candidates, None, 0).unwrap().profile_id,
            "glm-fresh"
        );
        // Never-observed ranks above stale: treating it as full would idle new capacity
        // forever.
        let candidates = vec![stale, unseen];
        assert_eq!(
            select(&candidates, None, 0).unwrap().profile_id,
            "glm-unseen"
        );
    }

    #[test]
    fn quota_steering_only_applies_near_the_wall() {
        // Below the threshold exact fractions must not decide, or every request would herd
        // onto whichever subscription currently looks emptiest.
        let mut low = candidate("glm-01");
        low.used_fraction_units = Some(10_000_000);
        let mut lower = candidate("glm-02");
        lower.used_fraction_units = Some(1);
        lower.inflight = 1;
        let candidates = vec![low, lower];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "glm-01");

        // Near the wall it does decide, so a nearly exhausted profile is avoided.
        let mut nearly_full = candidate("glm-03");
        nearly_full.used_fraction_units = Some(95_000_000);
        let mut half = candidate("glm-04");
        half.used_fraction_units = Some(60_000_000);
        let candidates = vec![nearly_full, half];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "glm-04");
    }

    #[test]
    fn equal_candidates_rotate_through_the_cursor_and_ties_can_exist() {
        let candidates = vec![candidate("glm-01"), candidate("glm-02")];
        let first = select(&candidates, None, 0).unwrap().profile_id.clone();
        let second = select(&candidates, None, 1).unwrap().profile_id.clone();
        assert_ne!(first, second, "a burst must not herd onto one subscription");
        assert_eq!(select(&candidates, None, 2).unwrap().profile_id, first);
        // If the id took part in the rank key, no two candidates would ever tie and the cursor
        // would be dead code.
        assert_eq!(rank(&candidate("glm-aaa")), rank(&candidate("glm-zzz")));
    }

    #[test]
    fn every_ineligibility_reason_removes_a_profile_from_rotation() {
        for reason in [
            Ineligible::QuotaWall,
            Ineligible::AccountDead,
            Ineligible::AccountSuspect,
            Ineligible::ModelIneligible,
            Ineligible::TransportWedged,
        ] {
            let mut blocked = candidate("glm-01");
            blocked.ineligible = Some(reason);
            let candidates = vec![blocked];
            assert!(select(&candidates, None, 0).is_none(), "{reason:?}");
            assert_eq!(ineligible_ids(&candidates).len(), 1);
        }
    }

    #[test]
    fn a_model_ineligible_profile_still_serves_the_models_it_grants() {
        // The candidate list is built per request: a profile blocked for one model is simply
        // not marked for another.
        let mut blocked = candidate("glm-01");
        blocked.ineligible = Some(Ineligible::ModelIneligible);
        assert!(select(&[blocked], None, 0).is_none());
        assert_eq!(
            select(&[candidate("glm-01")], None, 0).unwrap().profile_id,
            "glm-01"
        );
    }

    #[test]
    fn an_empty_or_fully_walled_fleet_selects_nothing_and_one_usable_profile_is_capacity() {
        assert!(select(&[], None, 0).is_none());
        let mut walled = candidate("glm-01");
        walled.ineligible = Some(Ineligible::AccountDead);
        assert!(select(&[walled], None, 0).is_none());
        // No arbitrary minimum fleet size: a single working profile must serve.
        assert_eq!(
            select(&[candidate("glm-01")], None, 0).unwrap().profile_id,
            "glm-01"
        );
    }
}
