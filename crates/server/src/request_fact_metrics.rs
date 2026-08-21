use forward::{RequestFactDeliverySnapshot, RequestFactPersistenceHealth};
use registry::request_facts::{
    request_fact_lifecycle_metrics, RequestFactLifecycleMetric,
    REQUEST_FACT_DURATION_BUCKETS_SECONDS,
};
use std::fmt::Write as _;

const DURATION_NAMES: [&str; 4] = [
    "admission_to_delivery",
    "admission_to_first_public_byte",
    "delivery_to_first_public_byte",
    "admission_to_terminal",
];

fn metric_labels(metric: &RequestFactLifecycleMetric) -> String {
    format!(
        "provider_plane=\"{}\",route_class=\"{}\",request_class=\"{}\",stream=\"{}\",provider_terminal_class=\"{}\"",
        metric.provider_plane,
        metric.route_class,
        metric.request_class,
        metric.stream,
        metric.provider_terminal_class,
    )
}

fn render_duration(
    body: &mut String,
    name: &str,
    labels: &str,
    buckets: &[u64; REQUEST_FACT_DURATION_BUCKETS_SECONDS.len()],
    sum: u64,
    count: u64,
) {
    for (upper, value) in REQUEST_FACT_DURATION_BUCKETS_SECONDS.iter().zip(buckets) {
        let _ = writeln!(
            body,
            "claude_api_request_fact_duration_seconds_bucket{{duration=\"{name}\",{labels},le=\"{upper}\"}} {value}"
        );
    }
    let _ = writeln!(
        body,
        "claude_api_request_fact_duration_seconds_bucket{{duration=\"{name}\",{labels},le=\"+Inf\"}} {count}\n\
         claude_api_request_fact_duration_seconds_sum{{duration=\"{name}\",{labels}}} {sum}\n\
         claude_api_request_fact_duration_seconds_count{{duration=\"{name}\",{labels}}} {count}"
    );
}

pub(super) fn write_request_fact_metrics(
    body: &mut String,
    delivery: RequestFactDeliverySnapshot,
    stuck_lifecycles: Option<u64>,
) {
    if !delivery.enabled {
        return;
    }
    // A freshly started PostgreSQL inbox has not committed a batch yet, but it is available and has
    // no failure evidence. Only an observed persistence failure is unhealthy; the next successful
    // batch restores the gauge. SQLite never enables the inbox and publishes no PostgreSQL series.
    let health = match delivery.persistence_health {
        RequestFactPersistenceHealth::Unknown | RequestFactPersistenceHealth::Healthy => 1,
        RequestFactPersistenceHealth::Failed => 0,
    };
    let _ = writeln!(
        body,
        "# HELP claude_api_request_fact_inbox_capacity Fixed terminal request-fact inbox capacity.\n\
         # TYPE claude_api_request_fact_inbox_capacity gauge\n\
         claude_api_request_fact_inbox_capacity {}\n\
         # TYPE claude_api_request_fact_inbox_depth gauge\n\
         claude_api_request_fact_inbox_depth {}\n\
         # TYPE claude_api_request_fact_persistence_healthy gauge\n\
         claude_api_request_fact_persistence_healthy {}\n\
         # TYPE claude_api_request_fact_submissions_total counter\n\
         claude_api_request_fact_submissions_total{{outcome=\"accepted\"}} {}\n\
         claude_api_request_fact_submissions_total{{outcome=\"invalid\"}} {}\n\
         claude_api_request_fact_submissions_total{{outcome=\"full\"}} {}\n\
         claude_api_request_fact_submissions_total{{outcome=\"closed\"}} {}\n\
         claude_api_request_fact_submissions_total{{outcome=\"unsupported\"}} {}\n\
         # TYPE claude_api_request_fact_persistence_total counter\n\
         claude_api_request_fact_persistence_total{{outcome=\"persisted\"}} {}\n\
         claude_api_request_fact_persistence_total{{outcome=\"deduplicated\"}} {}\n\
         claude_api_request_fact_persistence_total{{outcome=\"failed\"}} {}",
        forward::TERMINAL_REQUEST_FACT_QUEUE_CAPACITY,
        delivery.queue_depth,
        health,
        delivery.accepted,
        delivery.dropped_invalid,
        delivery.dropped_full,
        delivery.dropped_closed,
        delivery.dropped_unsupported,
        delivery.persisted,
        delivery.deduplicated,
        delivery.persistence_failed,
    );
    if let Some(stuck_lifecycles) = stuck_lifecycles {
        let _ = writeln!(
            body,
            "# TYPE claude_api_request_fact_stuck_lifecycles gauge\n\
             claude_api_request_fact_stuck_lifecycles {stuck_lifecycles}"
        );
    }

    let metrics = request_fact_lifecycle_metrics();
    let _ = writeln!(
        body,
        "# TYPE claude_api_request_fact_lifecycle_total counter\n\
         # TYPE claude_api_request_fact_duration_seconds histogram"
    );
    for metric in metrics {
        let labels = metric_labels(&metric);
        let _ = writeln!(
            body,
            "claude_api_request_fact_lifecycle_total{{{labels}}} {}",
            metric.count
        );
        for (name, buckets, sum, count) in [
            (
                DURATION_NAMES[0],
                &metric.admission_to_delivery_buckets,
                metric.admission_to_delivery_sum_seconds,
                metric.admission_to_delivery_count,
            ),
            (
                DURATION_NAMES[1],
                &metric.admission_to_first_public_byte_buckets,
                metric.admission_to_first_public_byte_sum_seconds,
                metric.admission_to_first_public_byte_count,
            ),
            (
                DURATION_NAMES[2],
                &metric.delivery_to_first_public_byte_buckets,
                metric.delivery_to_first_public_byte_sum_seconds,
                metric.delivery_to_first_public_byte_count,
            ),
            (
                DURATION_NAMES[3],
                &metric.admission_to_terminal_buckets,
                metric.admission_to_terminal_sum_seconds,
                metric.admission_to_terminal_count,
            ),
        ] {
            render_duration(body, name, &labels, buckets, sum, count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_delivery_metrics_use_only_closed_outcomes() {
        let mut body = String::new();
        write_request_fact_metrics(
            &mut body,
            RequestFactDeliverySnapshot {
                enabled: true,
                queue_depth: 7,
                accepted: 11,
                persisted: 8,
                deduplicated: 2,
                dropped_invalid: 1,
                dropped_full: 3,
                dropped_closed: 4,
                dropped_unsupported: 5,
                persistence_failed: 6,
                persistence_health: RequestFactPersistenceHealth::Failed,
                process_started_at: Some(1),
            },
            Some(9),
        );
        assert!(body.contains("claude_api_request_fact_inbox_capacity 4096"));
        assert!(body.contains("claude_api_request_fact_inbox_depth 7"));
        assert!(body.contains("claude_api_request_fact_persistence_healthy 0"));
        assert!(body.contains("claude_api_request_fact_stuck_lifecycles 9"));
        for outcome in ["accepted", "invalid", "full", "closed", "unsupported"] {
            assert!(body.contains(&format!("outcome=\"{outcome}\"")));
        }
        assert!(!body.contains("account"));
        assert!(!body.contains("key_id"));
        assert!(!body.contains("model="));
    }

    #[test]
    fn fresh_postgres_is_healthy_and_disabled_authority_is_absent() {
        let fresh = RequestFactDeliverySnapshot {
            enabled: true,
            queue_depth: 0,
            accepted: 0,
            persisted: 0,
            deduplicated: 0,
            dropped_invalid: 0,
            dropped_full: 0,
            dropped_closed: 0,
            dropped_unsupported: 0,
            persistence_failed: 0,
            persistence_health: RequestFactPersistenceHealth::Unknown,
            process_started_at: Some(1),
        };
        let mut body = String::new();
        write_request_fact_metrics(&mut body, fresh, Some(0));
        assert!(body.contains("claude_api_request_fact_persistence_healthy 1"));

        body.clear();
        write_request_fact_metrics(
            &mut body,
            RequestFactDeliverySnapshot {
                enabled: false,
                ..fresh
            },
            None,
        );
        assert!(body.is_empty());
    }
}
