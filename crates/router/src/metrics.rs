//! Compile-bounded Prometheus counters owned by the stateless router process.
//!
//! Metrics deliberately expose only the three catalog namespaces and the two reviewed continuation
//! reasons. Request, model, credential and execution-group identities never become labels.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::Lane;
use crate::proxy::RetryReason;

const NAMESPACE_COUNT: usize = 3;
const REASON_COUNT: usize = 2;
const FALLBACK_SERIES_COUNT: usize = NAMESPACE_COUNT * NAMESPACE_COUNT * REASON_COUNT;

const NAMESPACES: [(Lane, &str); NAMESPACE_COUNT] = [
    (Lane::Anthropic, "anthropic"),
    (Lane::OpenAi, "openai"),
    (Lane::Gemini, "google"),
];
const REASONS: [(RetryReason, &str); REASON_COUNT] = [
    (RetryReason::NotStarted, "not_started"),
    (RetryReason::ConnectionRefused, "connect_refused"),
];

pub struct RouterMetrics {
    fallback: [AtomicU64; FALLBACK_SERIES_COUNT],
}

impl Default for RouterMetrics {
    fn default() -> Self {
        Self {
            fallback: [const { AtomicU64::new(0) }; FALLBACK_SERIES_COUNT],
        }
    }
}

impl RouterMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the decision to continue exactly once, immediately before the next attempt starts.
    pub fn fallback(&self, from: Lane, to: Lane, reason: RetryReason) {
        self.fallback[index(from, to, reason)].fetch_add(1, Ordering::Relaxed);
    }

    fn fallback_count(&self, from: Lane, to: Lane, reason: RetryReason) -> u64 {
        self.fallback[index(from, to, reason)].load(Ordering::Relaxed)
    }

    pub fn render(&self) -> String {
        let mut body = String::from(
            "# HELP claude_router_fallback_total Serial fallback continuations started by the router.\n\
             # TYPE claude_router_fallback_total counter\n",
        );
        use std::fmt::Write as _;
        for (from, from_namespace) in NAMESPACES {
            for (to, to_namespace) in NAMESPACES {
                for (reason, reason_label) in REASONS {
                    let _ = writeln!(
                        body,
                        "claude_router_fallback_total{{from_namespace=\"{from_namespace}\",to_namespace=\"{to_namespace}\",reason=\"{reason_label}\"}} {}",
                        self.fallback_count(from, to, reason),
                    );
                }
            }
        }
        body
    }
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

fn index(from: Lane, to: Lane, reason: RetryReason) -> usize {
    (namespace_index(from) * NAMESPACE_COUNT + namespace_index(to)) * REASON_COUNT
        + reason_index(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_metrics_have_only_the_compile_fixed_label_matrix() {
        let metrics = RouterMetrics::new();
        metrics.fallback(Lane::Anthropic, Lane::OpenAi, RetryReason::NotStarted);
        metrics.fallback(Lane::Anthropic, Lane::OpenAi, RetryReason::NotStarted);
        metrics.fallback(Lane::OpenAi, Lane::Gemini, RetryReason::ConnectionRefused);

        let body = metrics.render();
        let samples: Vec<_> = body
            .lines()
            .filter(|line| line.starts_with("claude_router_fallback_total{"))
            .collect();
        assert_eq!(samples.len(), FALLBACK_SERIES_COUNT);
        assert!(body.contains(
            "claude_router_fallback_total{from_namespace=\"anthropic\",to_namespace=\"openai\",reason=\"not_started\"} 2"
        ));
        assert!(body.contains(
            "claude_router_fallback_total{from_namespace=\"openai\",to_namespace=\"google\",reason=\"connect_refused\"} 1"
        ));
        for forbidden in ["model=", "credential=", "group=", "request_id="] {
            assert!(!body.contains(forbidden));
        }
    }
}
