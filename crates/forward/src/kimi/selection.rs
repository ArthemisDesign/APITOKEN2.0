//! Profile selection for the KIMI plane.
//!
//! Pure state machine: no HTTP, no clock reads, no I/O. Contract:
//! `docs/engine/PROVIDER_ONBOARDING.md` §8.2 and §8.3.
//!
//! The rule that shapes everything here is that **there is no local concurrency limit**. Every
//! admitted request immediately starts an upstream attempt; `inflight` is a placement signal, not
//! a ceiling. A process-local semaphore or admission queue would manufacture a wait that the
//! provider never asked for, and would turn our own saturation into a customer-visible error.

use std::collections::HashSet;

/// Why a profile cannot serve right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ineligible {
    /// Provider said no in a way that survives until a reset.
    QuotaWall,
    /// Auth is quarantined after repeated refusals.
    AuthQuarantined,
    /// Transport is wedged and being rebuilt.
    TransportWedged,
    /// The plan does not grant the requested capability.
    CapabilityNotInPlan,
    /// This model is failing on this profile while its other models stay eligible.
    ModelCooling,
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
/// `sticky` is a resolved conversation affinity. It wins outright while it is eligible, because
/// breaking affinity throws away the provider-side prompt cache the conversation has built up —
/// which costs real money on the very next turn.
///
/// `cursor` is an atomic rotation counter supplied by the caller; equal candidates alternate
/// through it so a burst of new conversations spreads instead of herding onto one subscription.
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
    // sorted by id purely to make the cursor's choice deterministic across processes; the id must
    // NOT take part in `rank` itself, or no two candidates would ever tie and the cursor would
    // never rotate at all.
    let mut tied: Vec<&Candidate> = eligible
        .into_iter()
        .filter(|candidate| rank(candidate) == best)
        .collect();
    tied.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    let index = (cursor % tied.len() as u64) as usize;
    Some(tied[index])
}

/// Ordering key. Lower is better; every component is an integer so ordering is total and stable.
///
/// The profile id is deliberately absent. Including it would make every candidate distinct, so
/// nothing would ever tie and the rotation cursor below would be dead code — every burst would
/// land on the alphabetically first subscription.
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
        let mut busy = candidate("kimi-01");
        busy.inflight = 50;
        let idle = candidate("kimi-02");
        let candidates = vec![busy, idle];
        assert_eq!(
            select(&candidates, Some("kimi-01"), 0).unwrap().profile_id,
            "kimi-01"
        );
    }

    #[test]
    fn a_walled_sticky_profile_falls_through_instead_of_failing_the_request() {
        let mut walled = candidate("kimi-01");
        walled.ineligible = Some(Ineligible::QuotaWall);
        let candidates = vec![walled, candidate("kimi-02")];
        assert_eq!(
            select(&candidates, Some("kimi-01"), 0).unwrap().profile_id,
            "kimi-02"
        );
    }

    #[test]
    fn load_spreads_an_unbound_burst() {
        let mut loaded = candidate("kimi-01");
        loaded.inflight = 7;
        let candidates = vec![loaded, candidate("kimi-02")];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-02");
    }

    #[test]
    fn inflight_is_a_placement_signal_and_never_a_ceiling() {
        // Even a heavily loaded fleet must still serve: no local concurrency limit exists, so a
        // request is never refused for being the Nth in flight.
        let mut a = candidate("kimi-01");
        let mut b = candidate("kimi-02");
        a.inflight = 10_000;
        b.inflight = 10_000;
        let candidates = vec![a, b];
        assert!(select(&candidates, None, 0).is_some());
    }

    #[test]
    fn fresh_evidence_beats_stale_and_a_never_seen_profile_stays_neutral() {
        let mut fresh = candidate("kimi-fresh");
        fresh.quota_age_secs = Some(10);
        fresh.used_fraction_units = Some(90_000_000);
        let mut stale = candidate("kimi-stale");
        stale.quota_age_secs = Some(10_000);
        stale.used_fraction_units = Some(0);
        let mut unseen = candidate("kimi-unseen");
        unseen.quota_age_secs = None;
        unseen.used_fraction_units = None;

        // Fresh wins even though it looks fuller: stale emptiness is not evidence.
        let candidates = vec![stale.clone(), fresh.clone()];
        assert_eq!(
            select(&candidates, None, 0).unwrap().profile_id,
            "kimi-fresh"
        );
        // Never-observed ranks above stale: treating it as full would idle new capacity forever.
        let candidates = vec![stale, unseen];
        assert_eq!(
            select(&candidates, None, 0).unwrap().profile_id,
            "kimi-unseen"
        );
    }

    #[test]
    fn quota_steering_only_applies_near_the_wall() {
        // Below the threshold exact fractions must not decide, or every request would herd onto
        // whichever subscription currently looks emptiest.
        let mut low = candidate("kimi-01");
        low.used_fraction_units = Some(10_000_000);
        let mut lower = candidate("kimi-02");
        lower.used_fraction_units = Some(1);
        lower.inflight = 1;
        let candidates = vec![low, lower];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-01");

        // Near the wall it does decide, so a nearly exhausted profile is avoided.
        let mut nearly_full = candidate("kimi-03");
        nearly_full.used_fraction_units = Some(95_000_000);
        let mut half = candidate("kimi-04");
        half.used_fraction_units = Some(60_000_000);
        let candidates = vec![nearly_full, half];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-04");
    }

    #[test]
    fn equal_candidates_rotate_through_the_cursor() {
        let candidates = vec![candidate("kimi-01"), candidate("kimi-02")];
        let first = select(&candidates, None, 0).unwrap().profile_id.clone();
        let second = select(&candidates, None, 1).unwrap().profile_id.clone();
        assert_ne!(first, second, "a burst must not herd onto one subscription");
        assert_eq!(select(&candidates, None, 2).unwrap().profile_id, first);
    }

    #[test]
    fn the_ranking_key_excludes_the_profile_id_so_ties_can_exist() {
        // If the id took part in the key, no two candidates would ever tie, the rotation cursor
        // would be dead code, and every burst would land on the alphabetically first
        // subscription. This asserts the property directly rather than only through rotation.
        let left = candidate("kimi-aaa");
        let right = candidate("kimi-zzz");
        assert_eq!(rank(&left), rank(&right));
    }

    #[test]
    fn selection_is_deterministic_for_the_same_cursor() {
        let candidates = vec![candidate("kimi-01"), candidate("kimi-02")];
        for _ in 0..8 {
            assert_eq!(
                select(&candidates, None, 3).unwrap().profile_id,
                select(&candidates, None, 3).unwrap().profile_id
            );
        }
    }

    #[test]
    fn every_ineligibility_reason_removes_a_profile_from_rotation() {
        for reason in [
            Ineligible::QuotaWall,
            Ineligible::AuthQuarantined,
            Ineligible::TransportWedged,
            Ineligible::CapabilityNotInPlan,
        ] {
            let mut blocked = candidate("kimi-01");
            blocked.ineligible = Some(reason);
            let candidates = vec![blocked];
            assert!(select(&candidates, None, 0).is_none(), "{reason:?}");
            assert_eq!(ineligible_ids(&candidates).len(), 1);
        }
    }

    #[test]
    fn an_empty_or_fully_walled_fleet_selects_nothing() {
        assert!(select(&[], None, 0).is_none());
        let mut walled = candidate("kimi-01");
        walled.ineligible = Some(Ineligible::QuotaWall);
        assert!(select(&[walled], None, 0).is_none());
    }

    #[test]
    fn one_usable_subscription_is_real_capacity() {
        // No arbitrary minimum fleet size: a single working profile must serve.
        let candidates = vec![candidate("kimi-01")];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-01");
    }
}
