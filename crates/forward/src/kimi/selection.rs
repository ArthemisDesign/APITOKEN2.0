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

impl Ineligible {
    /// Whether this reason came from our own reading of the environment rather than from a
    /// provider statement about capacity.
    ///
    /// An auth refusal on this plane arrives *after* a successful token refresh, so it cannot mean
    /// the credential died: the provider returns 401 for a proxy-side block and, per its own error
    /// reference, also for "capability above your plan". A wedged transport is likewise ours to
    /// rebuild. Neither says the subscription is out of quota, so neither may take the last
    /// capacity out of service — Gemini learned this in August 2026, when one refusal wave walked
    /// the roster and zeroed a healthy pool nine times.
    ///
    /// `QuotaWall` and `CapabilityNotInPlan` are deliberately absent: those are provider verdicts,
    /// and serving through them would burn a request that cannot succeed. `ModelCooling` is absent
    /// too — it is scoped to one model while the profile's other models stay eligible.
    pub fn is_environmental(self) -> bool {
        matches!(self, Self::AuthQuarantined | Self::TransportWedged)
    }
}

/// One quota window as the selector sees it (R7 of
/// `docs/engine/QUOTA_DISTRIBUTION_ANALYSIS.md`: the two windows must reach the selector
/// separately — a 90% five-hour window that resets in ten minutes and a 90% weekly window that
/// resets in days were indistinguishable in the old single-number fold).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowEvidence {
    /// Used fraction in `10^-8` units. `None` — the window was observed without a usable fraction
    /// (never zero-filled).
    pub used_fraction_units: Option<i64>,
    /// Seconds until the provider reset by the engine clock, clamped at 0. `None` — the provider
    /// did not name a reset for this window.
    pub reset_in_secs: Option<i64>,
}

/// A candidate profile as the selector sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub profile_id: String,
    /// `None` while the profile is serving.
    pub ineligible: Option<Ineligible>,
    /// Short rolling window (5h). `None` — never observed.
    pub window_5h: Option<WindowEvidence>,
    /// Weekly window. `None` — never observed.
    pub window_weekly: Option<WindowEvidence>,
    /// Observation age in seconds; larger is staler. `None` pairs with never-observed windows.
    pub quota_age_secs: Option<i64>,
    /// In-flight requests. A placement signal only — never a ceiling.
    pub inflight: u32,
}

impl Candidate {
    fn is_eligible(&self) -> bool {
        self.ineligible.is_none()
    }

    /// Whether the candidate carries any fresh-enough quota evidence at all (R7 reserve
    /// relaxation needs it to tell "no data" apart from "data that blocks").
    fn has_observation(&self) -> bool {
        self.window_5h.is_some() || self.window_weekly.is_some()
    }
}

/// Fixed-point scale shared with the calibration authority.
const FRACTION_SCALE: i64 = 100_000_000;
/// Quota steering only kicks in near the wall; below it, load balancing decides.
const STEERING_THRESHOLD: i64 = FRACTION_SCALE / 2;
/// An observation older than this is stale and loses to a fresh one.
const STALE_AFTER_SECS: i64 = 300;
/// R7: a five-hour window this close to its reset is almost free — spending quota that resets in
/// ten minutes cannot strand the profile for long. The discount fades linearly over the last hour:
/// `effective_used5 = used5 · min(1, time_to_reset5 / RESET_DISCOUNT_HORIZON_SECS)`. (The formula
/// direction was fixed in review #1 of the analysis: an inverted discount gave full weight to the
/// window right at the reset.)
const RESET_DISCOUNT_HORIZON_SECS: i64 = 3_600;
/// R7: soft weekly reserve. At or above it the profile serves sticky continuations only (the warm
/// cache is worth more than the last few percent of quota), while fresh placements move away.
const WEEKLY_SOFT_RESERVE: i64 = 95 * FRACTION_SCALE / 100;

/// R7: effective 5h steering value discounted by reset proximity, fixed-point integer math.
/// A window with an unknown reset keeps its full weight — unknown is not "about to reset".
fn discounted_5h(window: WindowEvidence) -> Option<i64> {
    let used = window.used_fraction_units?;
    let discount = match window.reset_in_secs {
        Some(secs) => secs.clamp(0, RESET_DISCOUNT_HORIZON_SECS),
        None => RESET_DISCOUNT_HORIZON_SECS,
    };
    Some(used * discount / RESET_DISCOUNT_HORIZON_SECS)
}

/// Ordering key. Lower is better; every component is an integer so ordering is total and stable.
///
/// The profile id is deliberately absent. Including it would make every candidate distinct, so
/// nothing would ever tie and the rotation cursor below would be dead code — every burst would
/// land on the alphabetically first subscription.
///
/// R7 changed the quota axes: the weekly window is its own rank key (the long resource decides
/// placement) instead of being folded into one number, and the 5h window steers through the
/// reset-proximity discount above. Both keep the 50% floor: below it exact fractions must not
/// decide, or every request would herd onto whichever subscription currently looks emptiest.
fn rank(candidate: &Candidate) -> (u8, u32, i64, i64) {
    // 1. Fresh evidence beats stale. A never-observed profile is neutral rather than worst: it
    //    may well be empty, and treating it as full would leave new capacity permanently idle.
    let freshness = match candidate.quota_age_secs {
        Some(age) if age <= STALE_AFTER_SECS => 0,
        None => 1,
        Some(_) => 2,
    };
    // 2. Then load, so a burst spreads across the fleet.
    // 3. Weekly steering — the long resource decides placement, as its own key.
    let steering_weekly = candidate
        .window_weekly
        .and_then(|window| window.used_fraction_units)
        .filter(|used| *used >= STEERING_THRESHOLD)
        .unwrap_or(0);
    // 4. 5h steering, discounted towards its reset.
    let steering_5h = candidate
        .window_5h
        .and_then(discounted_5h)
        .filter(|used| *used >= STEERING_THRESHOLD)
        .unwrap_or(0);
    (freshness, candidate.inflight, steering_weekly, steering_5h)
}

/// Choose a profile for a request.
///
/// `sticky` is a resolved conversation affinity. It wins outright while it is eligible, because
/// breaking affinity throws away the provider-side prompt cache the conversation has built up —
/// which costs real money on the very next turn. R7 keeps this absolute: even a profile sitting
/// on the soft weekly reserve serves its sticky continuations; the reserve deflects only fresh
/// placements, and only while some other observed profile has weekly room (an all-reserved or
/// all-unobserved fleet falls through to the ordinary selection instead of manufacturing an
/// outage — the same service-floor lesson as the Gemini reserve).
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

    let at_reserve =
        |candidate: &Candidate| matches!(candidate.window_weekly.and_then(|w| w.used_fraction_units), Some(used) if used >= WEEKLY_SOFT_RESERVE);
    // The reserve holds only while a reserve-free alternative exists among profiles that carry
    // quota evidence at all; a fleet where nobody observed a weekly window has nothing to relax.
    let observed: Vec<&Candidate> = eligible
        .iter()
        .copied()
        .filter(|candidate| candidate.has_observation())
        .collect();
    let filtered: Vec<&Candidate> = if observed.iter().any(|candidate| !at_reserve(candidate)) {
        eligible
            .iter()
            .copied()
            .filter(|candidate| !at_reserve(candidate))
            .collect()
    } else {
        eligible.clone()
    };
    if filtered.is_empty() {
        return None;
    }

    let best = filtered.iter().copied().map(rank).min()?;

    // Ties rotate, so identical candidates do not all receive the same burst. The tied set is
    // sorted by id purely to make the cursor's choice deterministic across processes; the id must
    // NOT take part in `rank` itself, or no two candidates would ever tie and the cursor would
    // never rotate at all.
    let mut tied: Vec<&Candidate> = filtered
        .into_iter()
        .filter(|candidate| rank(candidate) == best)
        .collect();
    tied.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    let index = (cursor % tied.len() as u64) as usize;
    Some(tied[index])
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

    fn window(used_percent: i64, reset_in_secs: Option<i64>) -> WindowEvidence {
        WindowEvidence {
            used_fraction_units: Some(used_percent * FRACTION_SCALE / 100),
            reset_in_secs,
        }
    }

    fn candidate(id: &str) -> Candidate {
        Candidate {
            profile_id: id.into(),
            ineligible: None,
            window_5h: Some(window(0, None)),
            window_weekly: Some(window(0, None)),
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
        fresh.window_5h = Some(window(90, None));
        let mut stale = candidate("kimi-stale");
        stale.quota_age_secs = Some(10_000);
        let mut unseen = candidate("kimi-unseen");
        unseen.quota_age_secs = None;
        unseen.window_5h = None;
        unseen.window_weekly = None;

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
        low.window_weekly = Some(window(10, None));
        let mut lower = candidate("kimi-02");
        lower.window_weekly = Some(window(1, None));
        lower.inflight = 1;
        let candidates = vec![low, lower];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-01");

        // Near the wall it does decide, so a nearly exhausted profile is avoided.
        let mut nearly_full = candidate("kimi-03");
        nearly_full.window_weekly = Some(window(90, None));
        let mut half = candidate("kimi-04");
        half.window_weekly = Some(window(60, None));
        let candidates = vec![nearly_full, half];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-04");
    }

    #[test]
    fn the_weekly_window_is_its_own_rank_key() {
        // R7: 90% weekly with an empty 5h loses to a profile at 60% weekly even when its 5h is
        // hot — the old fold into one number could not tell these apart.
        let mut burnt_week = candidate("kimi-01");
        burnt_week.window_weekly = Some(window(90, None));
        let mut hot_five = candidate("kimi-02");
        hot_five.window_5h = Some(window(80, Some(7_200)));
        hot_five.window_weekly = Some(window(60, None));
        let candidates = vec![burnt_week, hot_five];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-02");
    }

    #[test]
    fn the_5h_window_is_discounted_towards_its_reset() {
        // R7: a 90% five-hour window that resets in 10 minutes is cheaper than a plain 60%
        // window (90 · 600/3600 = 15%), because that quota cannot be stranded for long.
        let mut near_reset = candidate("kimi-01");
        near_reset.window_5h = Some(window(90, Some(600)));
        let mut plain = candidate("kimi-02");
        plain.window_5h = Some(window(60, Some(7_200)));
        let candidates = vec![near_reset, plain.clone()];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-01");

        // Without a reset the same 90% keeps full weight: unknown is not "about to reset".
        let mut unknown_reset = candidate("kimi-03");
        unknown_reset.window_5h = Some(window(90, None));
        let candidates = vec![unknown_reset, plain];
        assert_eq!(select(&candidates, None, 0).unwrap().profile_id, "kimi-02");
    }

    #[test]
    fn the_soft_weekly_reserve_deflects_new_placements_but_not_sticky() {
        // R7: at weekly ≥ 95% the profile serves sticky continuations (the warm cache is worth
        // more) while fresh placements move to a profile with room.
        let mut reserved = candidate("kimi-01");
        reserved.window_weekly = Some(window(96, None));
        let roomy = candidate("kimi-02");
        let candidates = vec![reserved.clone(), roomy];
        assert_eq!(
            select(&candidates, None, 0).unwrap().profile_id,
            "kimi-02",
            "fresh placements stay off the reserved profile"
        );
        assert_eq!(
            select(&candidates, Some("kimi-01"), 0).unwrap().profile_id,
            "kimi-01",
            "sticky continuations keep the warm home even at the reserve"
        );
    }

    #[test]
    fn the_soft_weekly_reserve_relaxes_on_a_fully_reserved_fleet() {
        // Same service floor as the Gemini reserve: when nobody has weekly room (or nobody
        // observed a weekly window), the reserve must not manufacture an outage.
        let mut a = candidate("kimi-01");
        let mut b = candidate("kimi-02");
        a.window_weekly = Some(window(96, None));
        b.window_weekly = Some(window(99, None));
        assert!(select(&[a, b], None, 0).is_some());

        let mut c = candidate("kimi-03");
        c.window_weekly = None;
        assert!(select(&[c], None, 0).is_some(), "never-observed weekly must not look reserved");
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

#[cfg(test)]
mod escape_hatch_tests {
    use super::*;

    fn candidate(id: &str, reason: Option<Ineligible>) -> Candidate {
        Candidate {
            profile_id: id.into(),
            ineligible: reason,
            window_5h: None,
            window_weekly: None,
            quota_age_secs: Some(0),
            inflight: 0,
        }
    }

    #[test]
    fn only_environment_derived_reasons_may_be_relaxed() {
        // The split is the whole point: our own reading of the environment may be overridden when
        // it is the last capacity, a provider verdict may not.
        assert!(Ineligible::AuthQuarantined.is_environmental());
        assert!(Ineligible::TransportWedged.is_environmental());
        assert!(!Ineligible::QuotaWall.is_environmental());
        assert!(!Ineligible::CapabilityNotInPlan.is_environmental());
        assert!(!Ineligible::ModelCooling.is_environmental());
    }

    #[test]
    fn a_fleet_held_only_by_auth_refusals_still_serves() {
        // One 401 wave walking the roster must not zero the plane: after relaxing the
        // environmental reasons a profile is selectable again.
        let held = vec![
            candidate("a", Some(Ineligible::AuthQuarantined)),
            candidate("b", Some(Ineligible::TransportWedged)),
        ];
        assert!(select(&held, None, 0).is_none(), "strict pass must find nothing");

        let relaxed: Vec<Candidate> = held
            .iter()
            .cloned()
            .map(|mut candidate| {
                if candidate.ineligible.is_some_and(Ineligible::is_environmental) {
                    candidate.ineligible = None;
                }
                candidate
            })
            .collect();
        assert!(select(&relaxed, None, 0).is_some(), "relaxed pass must serve");
    }

    #[test]
    fn a_quota_wall_is_never_relaxed_into_a_doomed_request() {
        let walled = vec![
            candidate("a", Some(Ineligible::QuotaWall)),
            candidate("b", Some(Ineligible::CapabilityNotInPlan)),
        ];
        let relaxed: Vec<Candidate> = walled
            .iter()
            .cloned()
            .map(|mut candidate| {
                if candidate.ineligible.is_some_and(Ineligible::is_environmental) {
                    candidate.ineligible = None;
                }
                candidate
            })
            .collect();
        assert!(
            select(&relaxed, None, 0).is_none(),
            "a provider verdict must stay a wall even when it is the last profile"
        );
    }
}
