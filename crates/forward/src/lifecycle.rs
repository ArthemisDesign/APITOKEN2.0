const SECONDS_PER_DAY: i64 = 86_400;
const UNIX_EPOCH_DAY_OFFSET: i128 = 719_468;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SubscriptionLifecycle {
    pub acquired_at: Option<i64>,
    pub subscription_expires_at: Option<i64>,
    pub subscription_days_left: Option<f64>,
}

impl SubscriptionLifecycle {
    const EMPTY: Self = Self {
        acquired_at: None,
        subscription_expires_at: None,
        subscription_days_left: None,
    };

    fn from_expiry(acquired_at: i64, subscription_expires_at: Option<i64>, now: i64) -> Self {
        if acquired_at <= 0 {
            return Self::EMPTY;
        }
        Self {
            acquired_at: Some(acquired_at),
            subscription_expires_at,
            subscription_days_left: subscription_expires_at.map(|expires_at| {
                (i128::from(expires_at) - i128::from(now)) as f64 / SECONDS_PER_DAY as f64
            }),
        }
    }
}

pub(crate) fn fixed_days(acquired_at: i64, days: i64, now: i64) -> SubscriptionLifecycle {
    let expires_at = (acquired_at > 0)
        .then(|| days.checked_mul(SECONDS_PER_DAY))
        .flatten()
        .and_then(|lifetime| acquired_at.checked_add(lifetime));
    SubscriptionLifecycle::from_expiry(acquired_at, expires_at, now)
}

pub(crate) fn gemini(acquired_at: i64, plan: &str, now: i64) -> SubscriptionLifecycle {
    if acquired_at <= 0 || !gemini_credential::is_supported_paid_plan(plan) {
        return SubscriptionLifecycle::EMPTY;
    }
    if plan == "google_ai_pro" {
        return SubscriptionLifecycle::from_expiry(
            acquired_at,
            add_utc_calendar_months(acquired_at, 18),
            now,
        );
    }
    fixed_days(acquired_at, 30, now)
}

/// Add Gregorian calendar months in UTC, preserving time-of-day and clamping an end-of-month day
/// to the last valid day of the target month.
fn add_utc_calendar_months(timestamp: i64, months: u32) -> Option<i64> {
    let days = timestamp.div_euclid(SECONDS_PER_DAY);
    let seconds_of_day = timestamp.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(i128::from(days));
    let month_index = year
        .checked_mul(12)?
        .checked_add(i128::from(month - 1))?
        .checked_add(i128::from(months))?;
    let target_year = month_index.div_euclid(12);
    let target_month = u32::try_from(month_index.rem_euclid(12) + 1).ok()?;
    let target_day = day.min(days_in_month(target_year, target_month));
    let target_days = days_from_civil(target_year, target_month, target_day)?;
    let target = target_days
        .checked_mul(i128::from(SECONDS_PER_DAY))?
        .checked_add(i128::from(seconds_of_day))?;
    i64::try_from(target).ok()
}

fn is_leap_year(year: i128) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

fn days_in_month(year: i128, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(year: i128, month: u32, day: u32) -> Option<i128> {
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let adjusted_year = year - i128::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = i128::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i128::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(UNIX_EPOCH_DAY_OFFSET)
}

fn civil_from_days(days: i128) -> (i128, u32, u32) {
    let shifted = days + UNIX_EPOCH_DAY_OFFSET;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = shifted_month + if shifted_month < 10 { 3 } else { -9 };
    year += i128::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(year: i128, month: u32, day: u32, seconds_of_day: i64) -> i64 {
        i64::try_from(
            days_from_civil(year, month, day).unwrap() * i128::from(SECONDS_PER_DAY)
                + i128::from(seconds_of_day),
        )
        .unwrap()
    }

    fn date_time(timestamp: i64) -> (i128, u32, u32, i64) {
        let (year, month, day) = civil_from_days(i128::from(timestamp.div_euclid(SECONDS_PER_DAY)));
        (year, month, day, timestamp.rem_euclid(SECONDS_PER_DAY))
    }

    #[test]
    fn calendar_months_clamp_month_end_and_preserve_utc_time() {
        let time = 17 * 3_600 + 23 * 60 + 41;
        let leap_january = timestamp(2024, 1, 31, time);
        let ordinary_january = timestamp(2023, 1, 31, time);

        assert_eq!(
            date_time(add_utc_calendar_months(leap_january, 1).unwrap()),
            (2024, 2, 29, time)
        );
        assert_eq!(
            date_time(add_utc_calendar_months(ordinary_january, 1).unwrap()),
            (2023, 2, 28, time)
        );
    }

    #[test]
    fn calendar_months_cross_years_and_clamp_leap_day() {
        assert_eq!(
            date_time(add_utc_calendar_months(timestamp(2024, 2, 29, 0), 18).unwrap()),
            (2025, 8, 29, 0)
        );
        assert_eq!(
            date_time(add_utc_calendar_months(timestamp(2024, 8, 31, 0), 18).unwrap()),
            (2026, 2, 28, 0)
        );
    }

    #[test]
    fn lifecycle_is_optional_for_invalid_sources_and_supports_expired_values() {
        assert_eq!(fixed_days(0, 30, 100), SubscriptionLifecycle::EMPTY);
        assert_eq!(gemini(100, "unreviewed", 200), SubscriptionLifecycle::EMPTY);

        let lifecycle = fixed_days(100, 30, 100 + 31 * SECONDS_PER_DAY);
        assert_eq!(lifecycle.acquired_at, Some(100));
        assert_eq!(
            lifecycle.subscription_expires_at,
            Some(100 + 30 * SECONDS_PER_DAY)
        );
        assert_eq!(lifecycle.subscription_days_left, Some(-1.0));
    }

    #[test]
    fn google_ai_pro_uses_eighteen_calendar_months() {
        let issued_at = timestamp(2024, 8, 31, 12_345);
        let lifecycle = gemini(issued_at, "google_ai_pro", issued_at);
        assert_eq!(
            date_time(lifecycle.subscription_expires_at.unwrap()),
            (2026, 2, 28, 12_345)
        );
        assert!(lifecycle.subscription_days_left.unwrap() > 500.0);
    }

    #[test]
    fn every_other_canonical_gemini_plan_uses_thirty_days() {
        for plan in [
            "google_ai_ultra",
            "code_assist_standard",
            "code_assist_enterprise",
            "workspace_ai_ultra",
        ] {
            let lifecycle = gemini(100, plan, 100);
            assert_eq!(
                lifecycle.subscription_expires_at,
                Some(100 + 30 * SECONDS_PER_DAY),
                "{plan}"
            );
            assert_eq!(lifecycle.subscription_days_left, Some(30.0), "{plan}");
        }
    }
}
