//! Window-capacity calibration for one Codex subscription window.
//!
//! The provider reports window utilisation as coarse integer `usedPercent` snapshots, while the
//! gateway knows the exact official-price cost of every turn it served. Correlating the two yields
//! the subscription's real sellable capacity in USD per full window — the same idea as the Claude
//! pool's `calib_window`, adapted to integer points instead of fractional utilisation headers.
//!
//! Two guards mirror the Claude incident lessons:
//!
//! - **foreign-usage rejection** — an interval calibrates only when gateway spend explains at least
//!   half of the observed window movement, otherwise the account owner's own Codex usage would be
//!   mistaken for pool capacity;
//! - **plausibility** — a sample outside `[0.25x, 4x]` of the configured prior is rejected, so a
//!   poisoned or misread snapshot can never inflate the estimate.
//!
//! Anchors move on every measured interval, accepted or not, so foreign consumption shifts the
//! baseline instead of accumulating into the next sample. Sub-threshold deltas accumulate and are
//! never lost.

/// Integer provider points the window must advance before an interval may calibrate. One point is
/// within the provider's own update quantisation, so single-point intervals are only accumulated.
const MIN_DELTA_POINTS: i64 = 2;
/// Gateway spend must explain at least this share of the observed movement (`ours >= share *
/// Δused * cap_baseline`); the rest is attributed to usage outside the gateway.
const MIN_OURS_SHARE: f64 = 0.5;
/// Plausibility bounds of one sample relative to the configured prior capacity.
const MIN_PRIOR_FACTOR: f64 = 0.25;
const MAX_PRIOR_FACTOR: f64 = 4.0;
/// EMA weight of the running estimate against one accepted sample.
const ALPHA_OLD: f64 = 0.7;
/// One accepted sample may move the estimate by at most this factor in either direction.
const MAX_JUMP: f64 = 2.0;

/// Calibration state for one window slot (`primary` or `secondary`) of one home.
#[derive(Clone, Debug)]
pub struct WindowCalibration {
    /// EMA of the window's official-price capacity in USD. `None` until the first snapshot fixes
    /// the window identity, because the duration-scaled prior is only known then.
    cap_usd: Option<f64>,
    /// Accepted samples; `> 0` distinguishes a measured estimate from a prior.
    samples: u32,
    anchor_used: Option<i64>,
    anchor_spend_usd: f64,
    /// Provider reset timestamp identifying the window; a change means the window rolled over.
    anchor_resets_at: Option<i64>,
}

impl WindowCalibration {
    pub fn new() -> Self {
        Self {
            cap_usd: None,
            samples: 0,
            anchor_used: None,
            anchor_spend_usd: 0.0,
            anchor_resets_at: None,
        }
    }

    /// Current capacity estimate in USD per full window. Falls back to `prior_usd` before the
    /// first observation (or if the estimate was never measured).
    pub fn cap_usd(&self, prior_usd: f64) -> f64 {
        self.cap_usd.unwrap_or(prior_usd)
    }

    /// Whether at least one real sample was accepted; otherwise the figure is a prior.
    pub fn calibrated(&self) -> bool {
        self.samples > 0
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Estimated remaining sellable capacity of the current window in USD.
    pub fn remaining_usd(&self, used_percent: i64, prior_usd: f64) -> f64 {
        let left = (100 - used_percent.clamp(0, 100)) as f64 / 100.0;
        self.cap_usd(prior_usd) * left
    }

    /// Feed one snapshot point. `spend_usd_total` is the home's cumulative official-price spend
    /// served by the gateway (monotonic). `prior_usd` is the duration-scaled configured prior.
    pub fn observe(
        &mut self,
        used_percent: i64,
        resets_at: Option<i64>,
        spend_usd_total: f64,
        prior_usd: f64,
    ) {
        let used = used_percent.clamp(0, 100);
        if !spend_usd_total.is_finite() || !prior_usd.is_finite() || prior_usd <= 0.0 {
            return;
        }
        if self.cap_usd.is_none() {
            self.cap_usd = Some(prior_usd);
        }
        // A new window (including the first observation) only sets the baseline: measuring from a
        // window's unknown midpoint would mistake pre-gateway consumption for our own.
        if self.anchor_resets_at != resets_at || self.anchor_used.is_none() {
            self.reanchor(used, spend_usd_total, resets_at);
            return;
        }
        let anchor_used = self.anchor_used.unwrap_or(used);
        // The counter moved backwards inside the same window (provider accounting restart, plan
        // change): re-baseline without calibrating, exactly like a window rollover.
        if used < anchor_used {
            self.reanchor(used, spend_usd_total, resets_at);
            return;
        }
        let delta_used = used - anchor_used;
        if delta_used < MIN_DELTA_POINTS {
            return; // accumulate; nothing is lost because anchors stand still
        }
        let delta_spend = spend_usd_total - self.anchor_spend_usd;
        let cap = self.cap_usd.unwrap_or(prior_usd);
        let expected_total = delta_used as f64 / 100.0 * cap;
        let sample = delta_spend / (delta_used as f64 / 100.0);
        let ours_explains = delta_spend >= MIN_OURS_SHARE * expected_total;
        let plausible = sample.is_finite()
            && sample >= MIN_PRIOR_FACTOR * prior_usd
            && sample <= MAX_PRIOR_FACTOR * prior_usd;
        if ours_explains && plausible {
            let clamped = sample.clamp(cap / MAX_JUMP, cap * MAX_JUMP);
            self.cap_usd = Some(ALPHA_OLD * cap + (1.0 - ALPHA_OLD) * clamped);
            self.samples = self.samples.saturating_add(1);
        }
        // Re-anchor after every measured interval, accepted or not: foreign consumption moves the
        // baseline so it cannot leak into the next sample as phantom gateway capacity.
        self.reanchor(used, spend_usd_total, resets_at);
    }

    fn reanchor(&mut self, used: i64, spend_usd_total: f64, resets_at: Option<i64>) {
        self.anchor_used = Some(used);
        self.anchor_spend_usd = spend_usd_total;
        self.anchor_resets_at = resets_at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIOR: f64 = 1500.0;
    const WINDOW: Option<i64> = Some(1_800_000_000);

    /// A sequence of (used%, cumulative gateway spend) observations into one calibration.
    fn observe_all(cal: &mut WindowCalibration, points: &[(i64, f64)]) {
        for (used, spend) in points {
            cal.observe(*used, WINDOW, *spend, PRIOR);
        }
    }

    #[test]
    fn first_observation_only_anchors() {
        let mut cal = WindowCalibration::new();
        cal.observe(40, WINDOW, 100.0, PRIOR);
        assert!(!cal.calibrated());
        assert_eq!(cal.cap_usd(PRIOR), PRIOR);
        // A sub-threshold move right after the anchor accumulates but must not calibrate yet.
        cal.observe(41, WINDOW, 150.0, PRIOR);
        assert!(!cal.calibrated());
        assert_eq!(cal.cap_usd(PRIOR), PRIOR);
    }

    #[test]
    fn clean_isolated_sample_calibrates_with_visible_ema_blend() {
        let mut cal = WindowCalibration::new();
        // Anchor at 10%, then 4 points fully explained by $80 of gateway spend: sample = $2000,
        // deliberately different from the $1500 prior so the 0.7/0.3 blend is observable.
        observe_all(&mut cal, &[(10, 0.0), (14, 80.0)]);
        assert!(cal.calibrated());
        let expected = 0.7 * PRIOR + 0.3 * 2000.0;
        assert!(
            (cal.cap_usd(PRIOR) - expected).abs() < 1e-9,
            "cap {} != blended {expected}",
            cal.cap_usd(PRIOR)
        );
    }

    #[test]
    fn sub_threshold_deltas_accumulate_without_loss() {
        let mut cal = WindowCalibration::new();
        // Each single-point step is below MIN_DELTA_POINTS; together they form one valid interval.
        observe_all(&mut cal, &[(10, 0.0), (11, 20.0), (12, 40.0), (13, 60.0)]);
        assert!(cal.calibrated(), "accumulated interval must calibrate");
        let expected = 0.7 * PRIOR + 0.3 * 2000.0;
        assert!((cal.cap_usd(PRIOR) - expected).abs() < 1e-9);
    }

    #[test]
    fn foreign_usage_moves_the_anchor_without_calibrating() {
        let mut cal = WindowCalibration::new();
        // The account owner burns 10 points outside the gateway while gateway spend is ~0: ours
        // explains ~0 of the movement, so the interval is rejected and the baseline moves.
        observe_all(&mut cal, &[(10, 0.0), (20, 1.0)]);
        assert!(!cal.calibrated());
        assert_eq!(cal.cap_usd(PRIOR), PRIOR);
        // The following clean interval measures only gateway traffic from the shifted anchor.
        observe_all(&mut cal, &[(24, 81.0)]);
        assert!(cal.calibrated());
        let expected = 0.7 * PRIOR + 0.3 * 2000.0;
        assert!((cal.cap_usd(PRIOR) - expected).abs() < 1e-9);
    }

    #[test]
    fn implausibly_high_sample_is_rejected() {
        let mut cal = WindowCalibration::new();
        // $40_000 against 4 points = $1_000_000/window: beyond 4x prior, so it is quarantined
        // even though gateway spend "explains" it.
        observe_all(&mut cal, &[(10, 0.0), (14, 40_000.0)]);
        assert!(!cal.calibrated());
        assert_eq!(cal.cap_usd(PRIOR), PRIOR);
    }

    #[test]
    fn implausibly_low_sample_is_rejected() {
        let mut cal = WindowCalibration::new();
        // $4 against 4 points = $100/window: below 0.25x prior.
        // ours_explains passes (4 >= 0.5 * 0.04 * 1500 = 30 is false, so this sample is rejected by
        // BOTH guards; isolate plausibility by giving enough spend to satisfy ours_explains but a
        // tiny per-point price... that is impossible by construction, so assert the combined
        // rejection and verify the estimate is untouched).
        observe_all(&mut cal, &[(10, 0.0), (14, 4.0)]);
        assert!(!cal.calibrated());
        assert_eq!(cal.cap_usd(PRIOR), PRIOR);
    }

    #[test]
    fn one_sample_cannot_jump_the_estimate_more_than_the_clamp() {
        let mut cal = WindowCalibration::new();
        // Sample $5900 (within 4x prior) against the $1500 prior: the blend would land at $2820,
        // but the 2x jump clamp caps the sample at $3000 first.
        observe_all(&mut cal, &[(10, 0.0), (14, 236.0)]);
        assert!(cal.calibrated());
        let expected = 0.7 * PRIOR + 0.3 * (PRIOR * MAX_JUMP);
        assert!((cal.cap_usd(PRIOR) - expected).abs() < 1e-9);
    }

    #[test]
    fn window_rollover_reanchors_and_keeps_the_estimate() {
        let mut cal = WindowCalibration::new();
        observe_all(&mut cal, &[(10, 0.0), (14, 80.0)]);
        let measured = cal.cap_usd(PRIOR);
        assert!(cal.calibrated());
        // New reset identity: the fresh window only re-baselines, the measured estimate survives.
        cal.observe(3, Some(1_900_000_000), 200.0, PRIOR);
        assert_eq!(cal.samples(), 1);
        assert_eq!(cal.cap_usd(PRIOR), measured);
        cal.observe(6, Some(1_900_000_000), 260.0, PRIOR);
        assert_eq!(cal.samples(), 2, "clean interval in the new window calibrates");
    }

    #[test]
    fn backwards_counter_reanchors_without_calibrating() {
        let mut cal = WindowCalibration::new();
        observe_all(&mut cal, &[(40, 0.0), (25, 5.0)]);
        assert!(!cal.calibrated());
        // And the next forward interval measures from the re-based 25%.
        observe_all(&mut cal, &[(29, 85.0)]);
        assert!(cal.calibrated());
    }

    #[test]
    fn repeated_samples_converge_to_the_true_capacity() {
        let mut cal = WindowCalibration::new();
        let true_cap = 1800.0;
        let mut used = 0i64;
        let mut spend = 0.0f64;
        cal.observe(used, WINDOW, spend, PRIOR);
        for _ in 0..10 {
            used += 3;
            spend += 0.03 * true_cap;
            cal.observe(used, WINDOW, spend, PRIOR);
        }
        let cap = cal.cap_usd(PRIOR);
        assert!(
            (cap - true_cap).abs() / true_cap < 0.05,
            "cap {cap} should converge near {true_cap}"
        );
    }

    #[test]
    fn remaining_capacity_scales_with_used_percent() {
        let mut cal = WindowCalibration::new();
        observe_all(&mut cal, &[(0, 0.0), (4, 80.0)]);
        let cap = cal.cap_usd(PRIOR);
        assert!((cal.remaining_usd(50, PRIOR) - cap * 0.5).abs() < 1e-9);
        assert_eq!(cal.remaining_usd(100, PRIOR), 0.0);
        assert_eq!(cal.remaining_usd(140, PRIOR), 0.0, "clamped, never negative");
    }

    #[test]
    fn non_finite_or_nonpositive_prior_never_poisons_state() {
        let mut cal = WindowCalibration::new();
        cal.observe(10, WINDOW, 0.0, f64::NAN);
        cal.observe(14, WINDOW, 80.0, f64::INFINITY);
        cal.observe(18, WINDOW, 160.0, 0.0);
        assert!(!cal.calibrated());
        cal.observe(18, WINDOW, 160.0, PRIOR);
        cal.observe(22, WINDOW, 240.0, PRIOR);
        assert!(cal.calibrated(), "valid samples still calibrate after junk");
    }
}
