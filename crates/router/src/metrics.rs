//! Compile-bounded Prometheus telemetry owned by the stateless router process.
//!
//! Every label is selected from a fixed enum. Request, model, credential and execution-group
//! identities never become labels, and no metric stores customer-controlled text.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::error::Lane;
use crate::proxy::RetryReason;

const NAMESPACE_COUNT: usize = 3;
const REASON_COUNT: usize = 2;
const AUTH_OUTCOME_COUNT: usize = 3;
const CATALOG_REFRESH_OUTCOME_COUNT: usize = 4;
const PRICING_FAILURE_COUNT: usize = 2;
const POLICY_FAILURE_COUNT: usize = 3;
const FALLBACK_SERIES_COUNT: usize = NAMESPACE_COUNT * NAMESPACE_COUNT * REASON_COUNT;
const LANE_MATRIX_COUNT: usize = NAMESPACE_COUNT * NAMESPACE_COUNT;

const NAMESPACES: [(Lane, &str); NAMESPACE_COUNT] = [
    (Lane::Anthropic, "anthropic"),
    (Lane::OpenAi, "openai"),
    (Lane::Gemini, "google"),
];
const REASONS: [(RetryReason, &str); REASON_COUNT] = [
    (RetryReason::NotStarted, "not_started"),
    (RetryReason::ConnectionRefused, "connect_refused"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthOutcome {
    Success,
    Unauthorized,
    Unavailable,
}

impl AuthOutcome {
    const ALL: [(Self, &'static str); AUTH_OUTCOME_COUNT] = [
        (Self::Success, "success"),
        (Self::Unauthorized, "unauthorized"),
        (Self::Unavailable, "unavailable"),
    ];

    fn index(self) -> usize {
        match self {
            Self::Success => 0,
            Self::Unauthorized => 1,
            Self::Unavailable => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogRefreshOutcome {
    Success,
    AuthRejected,
    Failed,
    Oversized,
}

impl CatalogRefreshOutcome {
    const ALL: [(Self, &'static str); CATALOG_REFRESH_OUTCOME_COUNT] = [
        (Self::Success, "success"),
        (Self::AuthRejected, "auth_rejected"),
        (Self::Failed, "failed"),
        (Self::Oversized, "oversized"),
    ];

    fn index(self) -> usize {
        match self {
            Self::Success => 0,
            Self::AuthRejected => 1,
            Self::Failed => 2,
            Self::Oversized => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingFailure {
    Unauthorized,
    Unavailable,
}

impl PricingFailure {
    const ALL: [(Self, &'static str); PRICING_FAILURE_COUNT] = [
        (Self::Unauthorized, "unauthorized"),
        (Self::Unavailable, "unavailable"),
    ];

    fn index(self) -> usize {
        match self {
            Self::Unauthorized => 0,
            Self::Unavailable => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyFailure {
    Unauthorized,
    Unavailable,
    Restricted,
}

impl PolicyFailure {
    const ALL: [(Self, &'static str); POLICY_FAILURE_COUNT] = [
        (Self::Unauthorized, "unauthorized"),
        (Self::Unavailable, "unavailable"),
        (Self::Restricted, "restricted"),
    ];

    fn index(self) -> usize {
        match self {
            Self::Unauthorized => 0,
            Self::Unavailable => 1,
            Self::Restricted => 2,
        }
    }
}

pub struct RouterMetrics {
    fallback: [AtomicU64; FALLBACK_SERIES_COUNT],
    active_universal_requests: AtomicU64,
    active_body_admission_units: AtomicU64,
    body_admission_overload: AtomicU64,
    body_read_timeout: AtomicU64,
    auth_outcomes: [AtomicU64; AUTH_OUTCOME_COUNT],
    auth_duration_micros: [AtomicU64; AUTH_OUTCOME_COUNT],
    catalog_cache_hits: [AtomicU64; NAMESPACE_COUNT],
    catalog_refreshes: [AtomicU64; NAMESPACE_COUNT * CATALOG_REFRESH_OUTCOME_COUNT],
    catalog_degraded: [AtomicU64; NAMESPACE_COUNT],
    pricing_failures: [AtomicU64; PRICING_FAILURE_COUNT],
    policy_failures: [AtomicU64; POLICY_FAILURE_COUNT],
    response_header_timeouts: [AtomicU64; NAMESPACE_COUNT],
    balance_failovers: [AtomicU64; LANE_MATRIX_COUNT],
}

impl Default for RouterMetrics {
    fn default() -> Self {
        Self {
            fallback: [const { AtomicU64::new(0) }; FALLBACK_SERIES_COUNT],
            active_universal_requests: AtomicU64::new(0),
            active_body_admission_units: AtomicU64::new(0),
            body_admission_overload: AtomicU64::new(0),
            body_read_timeout: AtomicU64::new(0),
            auth_outcomes: [const { AtomicU64::new(0) }; AUTH_OUTCOME_COUNT],
            auth_duration_micros: [const { AtomicU64::new(0) }; AUTH_OUTCOME_COUNT],
            catalog_cache_hits: [const { AtomicU64::new(0) }; NAMESPACE_COUNT],
            catalog_refreshes: [const { AtomicU64::new(0) };
                NAMESPACE_COUNT * CATALOG_REFRESH_OUTCOME_COUNT],
            catalog_degraded: [const { AtomicU64::new(0) }; NAMESPACE_COUNT],
            pricing_failures: [const { AtomicU64::new(0) }; PRICING_FAILURE_COUNT],
            policy_failures: [const { AtomicU64::new(0) }; POLICY_FAILURE_COUNT],
            response_header_timeouts: [const { AtomicU64::new(0) }; NAMESPACE_COUNT],
            balance_failovers: [const { AtomicU64::new(0) }; LANE_MATRIX_COUNT],
        }
    }
}

pub struct UniversalRequestGuard {
    metrics: Arc<RouterMetrics>,
}

impl Drop for UniversalRequestGuard {
    fn drop(&mut self) {
        self.metrics
            .active_universal_requests
            .fetch_sub(1, Ordering::Relaxed);
    }
}

impl RouterMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn universal_request(self: &Arc<Self>) -> UniversalRequestGuard {
        self.active_universal_requests
            .fetch_add(1, Ordering::Relaxed);
        UniversalRequestGuard {
            metrics: self.clone(),
        }
    }

    pub fn body_units_acquired(&self, units: u32) {
        self.active_body_admission_units
            .fetch_add(u64::from(units), Ordering::Relaxed);
    }

    pub fn body_units_released(&self, units: u32) {
        self.active_body_admission_units
            .fetch_sub(u64::from(units), Ordering::Relaxed);
    }

    pub fn body_admission_overload(&self) {
        self.body_admission_overload.fetch_add(1, Ordering::Relaxed);
    }

    pub fn body_read_timeout(&self) {
        self.body_read_timeout.fetch_add(1, Ordering::Relaxed);
    }

    pub fn auth(&self, outcome: AuthOutcome, duration: Duration) {
        let index = outcome.index();
        self.auth_outcomes[index].fetch_add(1, Ordering::Relaxed);
        self.auth_duration_micros[index].fetch_add(
            duration.as_micros().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
    }

    pub fn catalog_cache_hit(&self, lane: Lane) {
        self.catalog_cache_hits[namespace_index(lane)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn catalog_refresh(&self, lane: Lane, outcome: CatalogRefreshOutcome) {
        let index = namespace_index(lane) * CATALOG_REFRESH_OUTCOME_COUNT + outcome.index();
        self.catalog_refreshes[index].fetch_add(1, Ordering::Relaxed);
    }

    pub fn catalog_degraded(&self, lane: Lane) {
        self.catalog_degraded[namespace_index(lane)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn pricing_failure(&self, failure: PricingFailure) {
        self.pricing_failures[failure.index()].fetch_add(1, Ordering::Relaxed);
    }

    pub fn policy_failure(&self, failure: PolicyFailure) {
        self.policy_failures[failure.index()].fetch_add(1, Ordering::Relaxed);
    }

    pub fn response_header_timeout(&self, lane: Lane) {
        self.response_header_timeouts[namespace_index(lane)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn balance_failover(&self, from: Lane, to: Lane) {
        self.balance_failovers[lane_matrix_index(from, to)].fetch_add(1, Ordering::Relaxed);
    }

    /// Record the decision to continue exactly once, immediately before the next attempt starts.
    pub fn fallback(&self, from: Lane, to: Lane, reason: RetryReason) {
        self.fallback[fallback_index(from, to, reason)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn render(&self) -> String {
        use std::fmt::Write as _;

        let mut body = String::new();
        metric_header(
            &mut body,
            "claude_router_active_universal_requests",
            "gauge",
            "Universal requests currently handled by the router.",
        );
        sample(
            &mut body,
            "claude_router_active_universal_requests",
            self.active_universal_requests.load(Ordering::Relaxed),
        );
        metric_header(
            &mut body,
            "claude_router_active_body_admission_units",
            "gauge",
            "One-MiB universal request-body admission units currently held.",
        );
        sample(
            &mut body,
            "claude_router_active_body_admission_units",
            self.active_body_admission_units.load(Ordering::Relaxed),
        );
        metric_header(
            &mut body,
            "claude_router_body_admission_overload_total",
            "counter",
            "Universal requests rejected because the body budget had no capacity.",
        );
        sample(
            &mut body,
            "claude_router_body_admission_overload_total",
            self.body_admission_overload.load(Ordering::Relaxed),
        );
        metric_header(
            &mut body,
            "claude_router_body_read_timeout_total",
            "counter",
            "Universal request bodies rejected by the idle or maximum read deadline.",
        );
        sample(
            &mut body,
            "claude_router_body_read_timeout_total",
            self.body_read_timeout.load(Ordering::Relaxed),
        );

        metric_header(
            &mut body,
            "claude_router_auth_preflight_total",
            "counter",
            "Bodyless universal authentication preflight outcomes.",
        );
        metric_header(
            &mut body,
            "claude_router_auth_preflight_duration_seconds_sum",
            "counter",
            "Accumulated bodyless authentication preflight duration by outcome.",
        );
        for (outcome, label) in AuthOutcome::ALL {
            let index = outcome.index();
            let _ = writeln!(
                body,
                "claude_router_auth_preflight_total{{outcome=\"{label}\"}} {}",
                self.auth_outcomes[index].load(Ordering::Relaxed)
            );
            let seconds =
                self.auth_duration_micros[index].load(Ordering::Relaxed) as f64 / 1_000_000.0;
            let _ = writeln!(
                body,
                "claude_router_auth_preflight_duration_seconds_sum{{outcome=\"{label}\"}} {seconds:.6}"
            );
        }

        metric_header(
            &mut body,
            "claude_router_catalog_cache_hit_total",
            "counter",
            "Fresh catalog snapshots served without a provider refresh.",
        );
        metric_header(
            &mut body,
            "claude_router_catalog_refresh_total",
            "counter",
            "Catalog refresh outcomes for each fixed provider plane.",
        );
        metric_header(
            &mut body,
            "claude_router_catalog_degraded_total",
            "counter",
            "Catalog plane requests served stale or missing.",
        );
        metric_header(
            &mut body,
            "claude_router_response_header_timeout_total",
            "counter",
            "Bounded read-only balance attempts that exceeded the response-header deadline.",
        );
        for (lane, namespace) in NAMESPACES {
            let lane_index = namespace_index(lane);
            let _ = writeln!(
                body,
                "claude_router_catalog_cache_hit_total{{namespace=\"{namespace}\"}} {}",
                self.catalog_cache_hits[lane_index].load(Ordering::Relaxed)
            );
            let _ = writeln!(
                body,
                "claude_router_catalog_degraded_total{{namespace=\"{namespace}\"}} {}",
                self.catalog_degraded[lane_index].load(Ordering::Relaxed)
            );
            let _ = writeln!(
                body,
                "claude_router_response_header_timeout_total{{namespace=\"{namespace}\"}} {}",
                self.response_header_timeouts[lane_index].load(Ordering::Relaxed)
            );
            for (outcome, label) in CatalogRefreshOutcome::ALL {
                let index = lane_index * CATALOG_REFRESH_OUTCOME_COUNT + outcome.index();
                let _ = writeln!(body, "claude_router_catalog_refresh_total{{namespace=\"{namespace}\",outcome=\"{label}\"}} {}", self.catalog_refreshes[index].load(Ordering::Relaxed));
            }
        }

        metric_header(
            &mut body,
            "claude_router_pricing_failure_total",
            "counter",
            "Key-scoped pricing authority failures.",
        );
        for (failure, label) in PricingFailure::ALL {
            let _ = writeln!(
                body,
                "claude_router_pricing_failure_total{{reason=\"{label}\"}} {}",
                self.pricing_failures[failure.index()].load(Ordering::Relaxed)
            );
        }
        metric_header(
            &mut body,
            "claude_router_policy_failure_total",
            "counter",
            "Account routing-policy preflight failures.",
        );
        for (failure, label) in PolicyFailure::ALL {
            let _ = writeln!(
                body,
                "claude_router_policy_failure_total{{reason=\"{label}\"}} {}",
                self.policy_failures[failure.index()].load(Ordering::Relaxed)
            );
        }

        metric_header(
            &mut body,
            "claude_router_balance_failover_total",
            "counter",
            "Bodyless balance requests continued to another fixed provider plane.",
        );
        for (from, from_namespace) in NAMESPACES {
            for (to, to_namespace) in NAMESPACES {
                let _ = writeln!(body, "claude_router_balance_failover_total{{from_namespace=\"{from_namespace}\",to_namespace=\"{to_namespace}\"}} {}", self.balance_failovers[lane_matrix_index(from, to)].load(Ordering::Relaxed));
            }
        }

        metric_header(
            &mut body,
            "claude_router_fallback_total",
            "counter",
            "Serial fallback continuations started by the router.",
        );
        for (from, from_namespace) in NAMESPACES {
            for (to, to_namespace) in NAMESPACES {
                for (reason, reason_label) in REASONS {
                    let _ = writeln!(body, "claude_router_fallback_total{{from_namespace=\"{from_namespace}\",to_namespace=\"{to_namespace}\",reason=\"{reason_label}\"}} {}", self.fallback[fallback_index(from, to, reason)].load(Ordering::Relaxed));
                }
            }
        }
        body
    }
}

fn metric_header(body: &mut String, name: &str, kind: &str, help: &str) {
    use std::fmt::Write as _;
    let _ = writeln!(body, "# HELP {name} {help}");
    let _ = writeln!(body, "# TYPE {name} {kind}");
}

fn sample(body: &mut String, name: &str, value: u64) {
    use std::fmt::Write as _;
    let _ = writeln!(body, "{name} {value}");
}

fn namespace_index(lane: Lane) -> usize {
    match lane {
        Lane::Anthropic => 0,
        Lane::OpenAi => 1,
        Lane::Gemini => 2,
    }
}

fn reason_index(reason: RetryReason) -> usize {
    match reason {
        RetryReason::NotStarted => 0,
        RetryReason::ConnectionRefused => 1,
    }
}

fn lane_matrix_index(from: Lane, to: Lane) -> usize {
    namespace_index(from) * NAMESPACE_COUNT + namespace_index(to)
}

fn fallback_index(from: Lane, to: Lane, reason: RetryReason) -> usize {
    lane_matrix_index(from, to) * REASON_COUNT + reason_index(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_have_only_compile_fixed_label_matrices() {
        let metrics = RouterMetrics::new();
        metrics.fallback(Lane::Anthropic, Lane::OpenAi, RetryReason::NotStarted);
        metrics.balance_failover(Lane::Anthropic, Lane::OpenAi);
        metrics.catalog_refresh(Lane::Gemini, CatalogRefreshOutcome::Oversized);
        metrics.auth(AuthOutcome::Success, Duration::from_millis(12));

        let body = metrics.render();
        assert_eq!(
            body.lines()
                .filter(|line| line.starts_with("claude_router_fallback_total{"))
                .count(),
            FALLBACK_SERIES_COUNT
        );
        assert_eq!(
            body.lines()
                .filter(|line| line.starts_with("claude_router_balance_failover_total{"))
                .count(),
            LANE_MATRIX_COUNT
        );
        assert_eq!(
            body.lines()
                .filter(|line| line.starts_with("claude_router_catalog_refresh_total{"))
                .count(),
            NAMESPACE_COUNT * CATALOG_REFRESH_OUTCOME_COUNT
        );
        assert!(body.contains(
            "claude_router_catalog_refresh_total{namespace=\"google\",outcome=\"oversized\"} 1"
        ));
        assert!(body.contains("claude_router_auth_preflight_total{outcome=\"success\"} 1"));
        for forbidden in ["model=", "credential=", "group=", "request_id="] {
            assert!(!body.contains(forbidden));
        }
    }
}
