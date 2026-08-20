//! Сбор per-профильных точек распределения квот для metrics.db (шаг 0 из
//! `docs/engine/QUOTA_DISTRIBUTION_ANALYSIS.md` §6): без этих рядов вторая волна правок и размер
//! эффекта R1 не измеримы. Селекторы сюда не смотрят — это чисто наблюдательный путь: читаем уже
//! кэшированные operational-статусы плоскостей, ни одного сетевого вызова.
//!
//! Семантика колонок `provider_sub_snapshots` (см. `metrics_store::ProviderQuotaPoint`):
//! - `used5h` — доля короткого окна: Codex primary, Kimi/GLM 5h-окно (18 000 с), у Gemini —
//!   `max(1 − remaining_fraction)` по всем бакетам (per-модельный официальный каталог Google не
//!   сводится к единой паре 5h/7d, поэтому публикуем худший остаток, а `used7d` не выдумываем);
//! - `used7d` — доля длинного окна: Codex secondary, Kimi/GLM weekly (604 800 с);
//! - `reset*_in` — секунды до reset по часам движка; окно без известного reset → NULL (GLM
//!   rolling-окно может его не называть, Gemini отдаёт reset строкой без точности к секунде —
//!   не парсим). Прошедший reset клампится в 0: отрицательного «времени до reset» не бывает.
//!
//! Окно определяется ближайшей длительностью к опорным константам, а не точным равенством —
//! провайдеры изредка сдвигают длительность на минуты (Kimi 300-минутное окно = 18 000 с ровно,
//! но снапшот с границы reset может отличаться).

use forward::gemini::GeminiOperationalStatus;
use forward::glm::GlmOperationalStatus;
use forward::kimi::KimiOperationalStatus;
use forward::CodexOperationalStatus;

use crate::metrics_store::ProviderQuotaPoint;

const KIMI_5H_SECS: i64 = registry::KIMI_ROLLING_WINDOW_SECS;
const KIMI_7D_SECS: i64 = registry::KIMI_WEEKLY_WINDOW_SECS;
const GLM_5H_SECS: i64 = registry::GLM_5H_WINDOW_SECS;
const GLM_7D_SECS: i64 = registry::GLM_WEEKLY_WINDOW_SECS;
/// Ближайшее окно признаём за своё в пределах ±10% опорной длительности.
const DURATION_TOLERANCE_BP: i64 = 1_000; // 10% в базисных пунктах

fn fraction_units_to_f64(units: i64, scale: i64) -> f64 {
    (units as f64 / scale as f64).clamp(0.0, 1.0)
}

fn reset_in(resets_at: Option<i64>, now: i64) -> Option<i64> {
    resets_at.map(|reset| reset.saturating_sub(now).max(0))
}

/// Ближайшая по длительности запись к `target`, если попала в допуск.
fn nearest_by_duration<'a, T>(
    windows: impl Iterator<Item = &'a T>,
    duration: impl Fn(&T) -> i64,
    target: i64,
) -> Option<&'a T> {
    let tolerance = target * DURATION_TOLERANCE_BP / 10_000;
    windows
        .filter(|window| (duration(window) - target).abs() <= tolerance)
        .min_by_key(|window| (duration(window) - target).abs())
}

/// Codex: провайдер сам нормализует проценты по окнам (primary 5h, secondary weekly).
pub fn codex_points(status: &CodexOperationalStatus, now: i64) -> Vec<ProviderQuotaPoint<'static>> {
    let mut out = Vec::with_capacity(status.homes.len());
    for home in &status.homes {
        let Some(limits) = &home.rate_limits else {
            continue;
        };
        let primary = limits.primary.as_ref();
        let secondary = limits.secondary.as_ref();
        out.push(ProviderQuotaPoint {
            plane: "codex",
            sub_id: Box::leak(home.id.clone().into_boxed_str()),
            used5h: primary.map(|window| window.used_fraction()),
            used7d: secondary.map(|window| window.used_fraction()),
            reset5h_in: primary.and_then(|window| reset_in(window.resets_at, now)),
            reset7d_in: secondary.and_then(|window| reset_in(window.resets_at, now)),
        });
    }
    out
}

/// Kimi: точные used/limit/resetTime по окнам 5h-rolling + weekly; берём оба раздельно.
pub fn kimi_points(status: &KimiOperationalStatus, now: i64) -> Vec<ProviderQuotaPoint<'static>> {
    let mut out = Vec::with_capacity(status.profiles.len());
    for profile in &status.profiles {
        let w5 = nearest_by_duration(
            profile.quota_windows.iter(),
            |w| w.duration_secs,
            KIMI_5H_SECS,
        );
        let w7 = nearest_by_duration(
            profile.quota_windows.iter(),
            |w| w.duration_secs,
            KIMI_7D_SECS,
        );
        if w5.is_none() && w7.is_none() {
            continue;
        }
        out.push(ProviderQuotaPoint {
            plane: "kimi",
            sub_id: Box::leak(profile.id.clone().into_boxed_str()),
            used5h: w5
                .map(|w| fraction_units_to_f64(w.used_fraction_units, registry::KIMI_FRACTION_SCALE)),
            used7d: w7
                .map(|w| fraction_units_to_f64(w.used_fraction_units, registry::KIMI_FRACTION_SCALE)),
            reset5h_in: w5.and_then(|w| reset_in(Some(w.resets_at), now)),
            reset7d_in: w7.and_then(|w| reset_in(Some(w.resets_at), now)),
        });
    }
    out
}

/// GLM: зеркалит Kimi, но доля окна может быть недоступна (`None`) — тогда только длительность.
pub fn glm_points(status: &GlmOperationalStatus, now: i64) -> Vec<ProviderQuotaPoint<'static>> {
    let mut out = Vec::with_capacity(status.profiles.len());
    for profile in &status.profiles {
        let w5 = nearest_by_duration(profile.quota_windows.iter(), |w| w.duration_secs, GLM_5H_SECS);
        let w7 = nearest_by_duration(profile.quota_windows.iter(), |w| w.duration_secs, GLM_7D_SECS);
        if w5.is_none() && w7.is_none() {
            continue;
        }
        out.push(ProviderQuotaPoint {
            plane: "glm",
            sub_id: Box::leak(profile.id.clone().into_boxed_str()),
            used5h: w5
                .and_then(|w| w.used_fraction_units)
                .map(|units| fraction_units_to_f64(units, registry::GLM_FRACTION_SCALE)),
            used7d: w7
                .and_then(|w| w.used_fraction_units)
                .map(|units| fraction_units_to_f64(units, registry::GLM_FRACTION_SCALE)),
            reset5h_in: w5.and_then(|w| reset_in(w.resets_at, now)),
            reset7d_in: w7.and_then(|w| reset_in(w.resets_at, now)),
        });
    }
    out
}

/// Gemini: per-модельные бакеты официального квота-каталога. Публикуем худший остаток как
/// used5h (это и есть «tightest» для селектора); единой недели у каталога нет.
pub fn gemini_points(status: &GeminiOperationalStatus) -> Vec<ProviderQuotaPoint<'static>> {
    let mut out = Vec::with_capacity(status.profiles.len());
    for profile in &status.profiles {
        let tightest_used = profile
            .quotas
            .iter()
            .filter_map(|bucket| bucket.remaining_fraction)
            .map(|remaining| 1.0 - remaining.clamp(0.0, 1.0))
            .reduce(f64::max);
        let Some(tightest_used) = tightest_used else {
            continue;
        };
        out.push(ProviderQuotaPoint {
            plane: "gemini",
            sub_id: Box::leak(profile.id.clone().into_boxed_str()),
            used5h: Some(tightest_used),
            used7d: None,
            reset5h_in: None,
            reset7d_in: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use forward::kimi::{KimiOperationalStatus, KimiProfileStatus, KimiQuotaWindowStatus};

    fn kimi_status(windows: Vec<KimiQuotaWindowStatus>) -> KimiOperationalStatus {
        KimiOperationalStatus {
            total_profiles: 1,
            live_profiles: 1,
            available_profiles: 1,
            auth_quarantined_profiles: 0,
            transport_cooling_profiles: 0,
            quota_cooling_profiles: 0,
            unreviewed_plan_profiles: 0,
            inflight_requests: 0,
            profiles: vec![KimiProfileStatus {
                id: "kimi-test01".into(),
                plan: "reviewed",
                live: true,
                auth_quarantined_until: None,
                transport_cool_until: None,
                quota_cool_until: None,
                inflight: 0,
                quota_observed_at: Some(1_000),
                quota_windows: windows,
            }],
            delivery: Default::default(),
        }
    }

    #[test]
    fn kimi_points_split_the_two_windows_and_convert_reset_to_remaining_secs() {
        let status = kimi_status(vec![
            KimiQuotaWindowStatus {
                duration_secs: KIMI_5H_SECS,
                used_units: 1,
                limit_units: 4,
                used_fraction_units: 25_000_000,
                measurement_resolution_fraction_units: 1,
                resets_at: 1_000 + 3_600,
                observed_at: 1_000,
            },
            KimiQuotaWindowStatus {
                duration_secs: KIMI_7D_SECS,
                used_units: 1,
                limit_units: 2,
                used_fraction_units: 50_000_000,
                measurement_resolution_fraction_units: 1,
                resets_at: 1_000 + 86_400,
                observed_at: 1_000,
            },
        ]);
        let points = kimi_points(&status, 1_000);
        assert_eq!(points.len(), 1);
        let point = &points[0];
        assert_eq!(point.plane, "kimi");
        assert_eq!(point.used5h, Some(0.25));
        assert_eq!(point.used7d, Some(0.5));
        assert_eq!(point.reset5h_in, Some(3_600));
        assert_eq!(point.reset7d_in, Some(86_400));
    }

    #[test]
    fn kimi_points_tolerate_a_shifted_duration_and_a_missing_window() {
        // Провайдер сдвинул 5h-окно на пару минут — всё ещё наше окно (±10%).
        let status = kimi_status(vec![KimiQuotaWindowStatus {
            duration_secs: KIMI_5H_SECS - 120,
            used_units: 0,
            limit_units: 1,
            used_fraction_units: 10_000_000,
            measurement_resolution_fraction_units: 1,
            resets_at: 500, // reset в прошлом → кламп в 0, отрицательного не бывает
            observed_at: 1_000,
        }]);
        let points = kimi_points(&status, 1_000);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].used5h, Some(0.1));
        assert_eq!(points[0].used7d, None, "weekly-окно не наблюдалось → NULL, не 0");
        assert_eq!(points[0].reset5h_in, Some(0));
        // Чужая длительность (часовое окно) не признаётся ни за одно из наших.
        let alien = kimi_status(vec![KimiQuotaWindowStatus {
            duration_secs: 3_600,
            used_units: 0,
            limit_units: 1,
            used_fraction_units: 1,
            measurement_resolution_fraction_units: 1,
            resets_at: 2_000,
            observed_at: 1_000,
        }]);
        assert!(kimi_points(&alien, 1_000).is_empty());
    }
}
