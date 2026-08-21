#!/usr/bin/env bash
set -euo pipefail

EXPECTED_SHA=${REQUEST_OBSERVABILITY_EXPECTED_SHA:?set REQUEST_OBSERVABILITY_EXPECTED_SHA}
START_TS=${REQUEST_OBSERVABILITY_START_TS:?set REQUEST_OBSERVABILITY_START_TS}
PROMETHEUS_URL=${REQUEST_OBSERVABILITY_PROMETHEUS_URL:-http://127.0.0.1:9090}
NOW_TS=$(date -u +%s)
DURATION_SECS=${REQUEST_OBSERVABILITY_DURATION_SECS:-86400}
END_TS=$((START_TS + DURATION_SECS))

[[ $EXPECTED_SHA =~ ^[0-9a-f]{40}$ ]] || { echo 'expected SHA must be canonical' >&2; exit 2; }
[[ $START_TS =~ ^[0-9]+$ ]] || { echo 'start timestamp must be epoch seconds' >&2; exit 2; }
if (( DURATION_SECS != 86400 )); then
  [[ $DURATION_SECS == 75600 && ${REQUEST_OBSERVABILITY_OWNER_APPROVED_21H:-} == 1 ]] \
    || { echo 'only the recorded owner-approved 21-hour exception is valid' >&2; exit 2; }
fi
(( NOW_TS >= END_TS )) || { printf 'observation window incomplete: %ds remain\n' "$((END_TS - NOW_TS))" >&2; exit 3; }

query() {
  curl -fsSG "$PROMETHEUS_URL/api/v1/query" --data-urlencode "query=$1"
}
scalar() {
  query "$1" | jq -er '.data.result | if length == 0 then "0" else .[0].value[1] end'
}
range_samples() {
  curl -fsSG "$PROMETHEUS_URL/api/v1/query_range" \
    --data-urlencode "query=$1" \
    --data-urlencode "start=$START_TS" \
    --data-urlencode "end=$END_TS" \
    --data-urlencode 'step=60' \
    | jq -er '[.data.result[].values[]?] | length'
}

# The exact corrective release must have remained the selected engine release for the whole window.
release_sha=$(readlink -f /srv/claude-api/releases/current | awk -F/ '{print $NF}')
if [[ $release_sha != "$EXPECTED_SHA" ]]; then
  [[ ${REQUEST_OBSERVABILITY_ALLOW_DESCENDANT_SHA:-} == 1 ]] \
    || { printf 'active engine SHA changed: expected %s got %s\n' "$EXPECTED_SHA" "$release_sha" >&2; exit 1; }
  git merge-base --is-ancestor "$EXPECTED_SHA" "$release_sha" \
    || { echo 'active engine SHA is not a descendant of the observed producer' >&2; exit 1; }
  [[ -z $(git diff --name-only "$EXPECTED_SHA..$release_sha" -- \
    crates/registry/src/request_facts.rs crates/registry/src/pg/request_facts.rs \
    crates/forward/src/billing/request_facts.rs crates/server/src/request_fact_metrics.rs) ]] \
    || { echo 'a descendant changed the observed request-fact implementation' >&2; exit 1; }
fi

providers='provider=~"anthropic|openai|gemini"'
window="${DURATION_SECS}s"
[[ $(scalar "min_over_time(claude_api_request_fact_persistence_healthy{$providers}[$window])") == 1 ]] \
  || { echo 'request-fact persistence was unhealthy' >&2; exit 1; }
[[ $(scalar "max_over_time(claude_api_request_fact_stuck_lifecycles{$providers}[$window])") == 0 ]] \
  || { echo 'a request-fact lifecycle was stuck' >&2; exit 1; }
[[ $(scalar "sum(increase(claude_api_request_fact_submissions_total{outcome=~\"invalid|full|closed|unsupported\"}[$window])) + sum(increase(claude_api_request_fact_persistence_total{outcome=\"failed\"}[$window]))") == 0 ]] \
  || { echo 'request-fact loss was observed' >&2; exit 1; }
[[ $(scalar "sum(increase(claude_api_execution_group_double_winner_total[$window]))") == 0 ]] \
  || { echo 'execution-group winner invariant changed' >&2; exit 1; }
[[ $(scalar "max_over_time(apitoken_balance_divergence_nano[$window])") == 0 ]] \
  || { echo 'balance conservation diverged' >&2; exit 1; }
[[ $(scalar "sum(max_over_time(ALERTS{alertname=~\"RequestFact.*\",alertstate=\"firing\"}[$window]))") == 0 ]] \
  || { echo 'a request-fact alert fired' >&2; exit 1; }

# Require continuous scrape evidence rather than treating absent series as a passing zero.
for metric in \
  'claude_api_request_fact_persistence_healthy{provider=~"anthropic|openai|gemini"}' \
  'claude_api_request_fact_stuck_lifecycles{provider=~"anthropic|openai|gemini"}'; do
  samples=$(range_samples "$metric")
  minimum_samples=$((3 * (DURATION_SECS / 60 - 10)))
  (( samples >= minimum_samples )) || { printf 'insufficient continuous samples for %s: %s\n' "$metric" "$samples" >&2; exit 1; }
done

# The v1 gate cannot invent a latency or error comparison when the approved baseline series is
# absent. Require the operator to provide the approved PromQL expressions explicitly; then evaluate
# the locked dual threshold and +0.1 percentage-point error budget over every 15-minute interval.
: "${REQUEST_OBSERVABILITY_BASELINE_ADMISSION_P99_QUERY:?set baseline admission p99 query}"
: "${REQUEST_OBSERVABILITY_BASELINE_FIRST_BYTE_P99_QUERY:?set baseline first-byte p99 query}"
: "${REQUEST_OBSERVABILITY_BASELINE_ERROR_RATE_QUERY:?set baseline error-rate query}"

compare_query() {
  local current_query=$1 baseline_query=$2 absolute=$3 relative=$4 label=$5
  local breach
  breach=$(scalar "max_over_time(((($current_query) - ($baseline_query) > $absolute) and (($current_query) > ($baseline_query) * $relative))[$window:15m])")
  [[ $breach == 0 ]] || { printf '%s breached locked thresholds\n' "$label" >&2; exit 1; }
}
compare_query \
  'histogram_quantile(0.99,sum by (le)(rate(claude_api_request_fact_duration_seconds_bucket{duration="admission_to_delivery"}[15m])))' \
  "$REQUEST_OBSERVABILITY_BASELINE_ADMISSION_P99_QUERY" 0.005 1.10 admission-p99
compare_query \
  'histogram_quantile(0.99,sum by (le)(rate(claude_api_request_fact_duration_seconds_bucket{duration="admission_to_first_public_byte"}[15m])))' \
  "$REQUEST_OBSERVABILITY_BASELINE_FIRST_BYTE_P99_QUERY" 0.005 1.10 first-byte-p99
[[ $(scalar "max_over_time(((sum(rate(caddy_http_request_duration_seconds_count{host=~\"api.apitoken.sale|openai.api.apitoken.sale|gemini.api.apitoken.sale\",code=~\"4..|5..\"}[15m])) / clamp_min(sum(rate(caddy_http_request_duration_seconds_count{host=~\"api.apitoken.sale|openai.api.apitoken.sale|gemini.api.apitoken.sale\"}[15m])),0.000001)) - ($REQUEST_OBSERVABILITY_BASELINE_ERROR_RATE_QUERY) > 0.001)[$window:15m])") == 0 ]] \
  || { echo 'attributable request error rate increased by over 0.1 percentage points' >&2; exit 1; }

printf 'request observability gate GREEN: sha=%s duration=%ss window=[%s,%s)\n' "$EXPECTED_SHA" "$DURATION_SECS" "$START_TS" "$END_TS"
