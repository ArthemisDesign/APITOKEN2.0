#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

# PostgreSQL DO blocks must reach psql as $$, never as the backslash commands that psql
# interprets from \$\$. Exercise the same shell quoting used by the production installer.
role_sql=$(printf '%s\n' "DO \$\$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'apitoken_monitoring') THEN CREATE ROLE apitoken_monitoring LOGIN; END IF; END \$\$;")
[[ $role_sql == 'DO $$ BEGIN '* ]]
[[ $role_sql != *'\$\$'* ]]

# ProtectHome hides /root. Give the Docker CLI an accessible, empty config directory so it can
# still discover the system-wide Compose plugin used by the collector.
collector_unit="$ROOT/systemd/apitoken-monitoring-collector.service"
grep -Fxq 'RuntimeDirectory=apitoken-monitoring-collector' "$collector_unit"
grep -Fxq 'Environment=HOME=/run/apitoken-monitoring-collector' "$collector_unit"
grep -Fxq 'Environment=DOCKER_CONFIG=/run/apitoken-monitoring-collector' "$collector_unit"

backup_unit="$ROOT/systemd/claude-api-backup.service"
grep -Fxq 'RuntimeDirectory=apitoken-db-backup' "$backup_unit"
grep -Fxq 'Environment=HOME=/run/apitoken-db-backup' "$backup_unit"
grep -Fxq 'Environment=DOCKER_CONFIG=/run/apitoken-db-backup' "$backup_unit"
grep -Fq -- '-f /usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml' \
  "$ROOT/deploy/apitoken-db-dump"
! grep -Fq '/opt/apitoken/repo' "$ROOT/deploy/apitoken-db-dump"

# Collector freshness stays aggregated with min() so an empty label set cannot fail vector
# matching. The three operands are queried separately so a RED GitHub headline names the
# failing piece. The job filter matches MonitoringTargetDown; the 24x5s window matches
# that alert's 2m `for:` (6ef38441 and 289993c3 both quarantined after GREEN engine admission).
grep -Fq 'query=min(up{job!~"claude-router|devbot"}) == 1' "$ROOT/deploy/watchdog.sh"
grep -Fq 'query=min(probe_success{job=~"public-http|openai-http|gemini-http|protected-http|support-http|loopback-http"}) == 1' \
  "$ROOT/deploy/watchdog.sh"
grep -Fq 'query=min(time() - apitoken_monitoring_collector_last_success_unixtime) < 180' \
  "$ROOT/deploy/watchdog.sh"
! grep -Fq 'min(up) == 1' "$ROOT/deploy/watchdog.sh"
grep -Fq 'for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24; do' \
  "$ROOT/deploy/watchdog.sh"
! grep -Fq 'and (time() - apitoken_monitoring_collector_last_success_unixtime < 180)' \
  "$ROOT/deploy/watchdog.sh"

for dashboard in "$ROOT"/observability/grafana/dashboards/*.json; do
  node -e 'JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8"))' "$dashboard"
  node - "$dashboard" <<'EOF'
const { readFileSync } = require('node:fs');
const file = process.argv[2];
const dashboard = JSON.parse(readFileSync(file, 'utf8'));
const ids = new Set();
for (const panel of dashboard.panels ?? []) {
  if (panel.id == null) continue;
  if (ids.has(panel.id)) throw new Error(`${file}: duplicate panel id ${panel.id}`);
  ids.add(panel.id);
}
EOF
done

# Exact client/model/tool dimensions stay in the read-only PostgreSQL reporting boundary. The
# datasource secret comes from monitoring.env, and the role receives only the aggregate views.
for reporting_view in request_fact_usage_daily request_fact_tool_usage_daily; do
  grep -Fq "CREATE VIEW $reporting_view" \
    "$ROOT/crates/registry/migrations_pg/0061_request_observability_views.sql"
  grep -Fq "$reporting_view" \
    "$ROOT/observability/grafana/dashboards/request-usage-dimensions.json"
done
grep -Fq 'uid: engine-request-analytics' \
  "$ROOT/observability/grafana/provisioning/datasources/datasources.yml"
# Grafana 12+ postgres plugin ignores top-level `database`. The default DB must
# live in jsonData or every panel shows "No data" with that plugin error.
python3 - "$ROOT/observability/grafana/provisioning/datasources/datasources.yml" <<'PY'
from pathlib import Path
import sys
text = Path(sys.argv[1]).read_text().splitlines()
in_ds = in_json = False
found = False
for line in text:
    if line.startswith('  - ') and in_ds:
        in_ds = in_json = False
    if 'uid: engine-request-analytics' in line:
        in_ds = True
        continue
    if in_ds and line.strip() == 'jsonData:':
        in_json = True
        continue
    if in_ds and in_json:
        if line.startswith('      database: claude_engine'):
            found = True
            break
        if line and not line.startswith('      '):
            break
    if in_ds and line.startswith('    database:'):
        sys.exit('engine-request-analytics still uses top-level database; Grafana 12+ ignores it')
if not found:
    sys.exit('engine-request-analytics jsonData.database is not claude_engine')
PY
for request_dashboard in production-overview.json request-usage-dimensions.json; do
  grep -Fq '"type":"grafana-postgresql-datasource","uid":"engine-request-analytics"' \
    "$ROOT/observability/grafana/dashboards/$request_dashboard" \
    || { printf 'request dashboard %s has the wrong PostgreSQL plugin reference\n' "$request_dashboard" >&2; exit 1; }
  ! grep -Fq '"type":"postgres","uid":"engine-request-analytics"' \
    "$ROOT/observability/grafana/dashboards/$request_dashboard"
done
grep -Fq '"type":"grafana-postgresql-datasource"' "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'password: ${MONITORING_POSTGRES_PASSWORD}' \
  "$ROOT/observability/grafana/provisioning/datasources/datasources.yml"
grep -Fq 'MONITORING_POSTGRES_PASSWORD: ${MONITORING_POSTGRES_PASSWORD:?MONITORING_POSTGRES_PASSWORD is required}' \
  "$ROOT/observability/compose.yaml"
grep -Fq 'GRANT SELECT ON request_fact_usage_daily, request_fact_tool_usage_daily TO apitoken_monitoring' \
  "$ROOT/deploy/install-monitoring.sh"
! grep -Eq 'GRANT SELECT ON (request_facts|usage_events|accounts|api_keys|ledger)' \
  "$ROOT/deploy/install-monitoring.sh"
for forbidden_dimension in logical_request_id billing_request_id execution_group_id \
  upstream_request_id client_version failure_class; do
  ! grep -Fq "$forbidden_dimension" \
    "$ROOT/observability/grafana/dashboards/request-usage-dimensions.json"
done
grep -Fq 'authbot|router)).service' "$ROOT/observability/grafana/dashboards/production-overview.json"
! grep -Fq 'authbot|router))\\.service' "$ROOT/observability/grafana/dashboards/production-overview.json"
# Schema 62 is GREEN in production. Overview cards consume the narrow 0062 rollups.
# The installer still grants them only when to_regclass finds the views, and the canary
# prefers request_fact_usage_top_model_daily with a 0061 fallback for first-boot hosts.
for reporting_view in request_fact_usage_top_customer_model_daily \
  request_fact_usage_top_client_daily request_fact_usage_top_model_daily \
  request_fact_usage_top_tool_daily; do
  grep -Fq "CREATE VIEW $reporting_view" \
    "$ROOT/crates/registry/migrations_pg/0062_request_usage_grafana_rollups.sql"
  grep -Fq "$reporting_view" \
    "$ROOT/observability/grafana/dashboards/production-overview.json" \
    || { printf 'production overview omits schema-62 rollup %s\n' \
      "$reporting_view" >&2; exit 1; }
done
! grep -Fq 'request_fact_usage_daily' \
  "$ROOT/observability/grafana/dashboards/production-overview.json" \
  || { printf 'production overview still queries the broad 0061 daily view\n' >&2; exit 1; }
! grep -Fq 'request_fact_tool_usage_daily' \
  "$ROOT/observability/grafana/dashboards/production-overview.json" \
  || { printf 'production overview still queries the broad 0061 tool view\n' >&2; exit 1; }
for overview_dimension in account_id key_id client_kind requested_model executable_model tool_class; do
  grep -Fq "$overview_dimension" \
    "$ROOT/observability/grafana/dashboards/production-overview.json" \
    || { printf 'production overview omits request-usage dimension %s\n' "$overview_dimension" >&2; exit 1; }
done
grep -Fq '/d/apitoken-request-usage/apitoken-sale-request-usage-dimensions' \
  "$ROOT/observability/grafana/dashboards/production-overview.json"
grep -Fq 'GRANT SELECT ON request_fact_usage_top_customer_model_daily, request_fact_usage_top_client_daily, request_fact_usage_top_model_daily, request_fact_usage_top_tool_daily TO apitoken_monitoring' \
  "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'Grafana request-analytics datasource returned no request usage rows' \
  "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'http://127.0.0.1:3600/api/ds/query' "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'grafana_query=$(jq -nc --arg sql "$grafana_sql"' \
  "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'grafana_response=$(curl --noproxy' "$ROOT/deploy/install-monitoring.sh"
! grep -Fq "grafana_query='" "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'FROM request_fact_usage_top_model_daily WHERE usage_day' "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'FROM request_fact_usage_daily WHERE usage_day' "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'grafana_sql=$grafana_sql_0062' "$ROOT/deploy/install-monitoring.sh"
grafana_sql="SELECT COUNT(*)::bigint AS rows, COALESCE(SUM(request_count),0)::bigint AS requests FROM request_fact_usage_top_model_daily WHERE usage_day >= CURRENT_DATE - INTERVAL '30 days'"
grafana_query=$(jq -nc --arg sql "$grafana_sql" \
  '{"from":"now-30d","to":"now","queries":[{"refId":"A","datasource":{"uid":"engine-request-analytics","type":"grafana-postgresql-datasource"},"format":"table","rawQuery":true,"rawSql":$sql,"intervalMs":30000,"maxDataPoints":100}]}')
jq --exit-status --arg needle "INTERVAL '30 days'" --arg sql "$grafana_sql" \
  '.queries[0].datasource.type == "grafana-postgresql-datasource" and .queries[0].rawSql == $sql and (.queries[0].rawSql | contains("request_fact_usage_top_model_daily")) and (.queries[0].rawSql | contains($needle))' \
  >/dev/null <<<"$grafana_query"

for pinned_image in \
  'quay.io/prometheus/prometheus:v3.12.0@sha256:69f5241418838263316593f7274a304b095c40bcf22e57272865da91bd60a8ac' \
  'quay.io/prometheus/alertmanager:v0.32.1@sha256:51a825c2a40acc3e338fdd00d622e01ec090f72be2b3ea46be0839cd47a4d286' \
  'grafana/grafana:13.1.0@sha256:121a7a9ece6dc10b969f1f96eed64b4f07dfac0d0b8abc070f7cb83bbde86f63' \
  'grafana/loki:3.7.2@sha256:191d4fdfb7264f16989f0a57f320872620a5a7c2ceeec6229212c4190ec49b86' \
  'grafana/alloy:v1.16.1@sha256:51aeb9d829239345070619dad3edd6873186f913c84f45b365b74574fcb38ec0' \
  'quay.io/prometheus/node-exporter:v1.11.1@sha256:0f422f62c15f154af8d8572b23d623aebfb10cec73a5c654d18f911f3f9df241' \
  'quay.io/prometheuscommunity/postgres-exporter:v0.19.1@sha256:e96064f876226d94bb6ce48a4c4b3dd76edba91168ec1ab024e5c4b959310b0f' \
  'quay.io/oliver006/redis_exporter:v1.67.0@sha256:120f7ec77293459ccfdad66bb1db75ab72e8bfeab99f58b1cc2564cadcd7a9e7' \
  'quay.io/prometheus/blackbox-exporter:v0.28.0@sha256:e753ff9f3fc458d02cca5eddab5a77e1c175eee484a8925ac7d524f04366c2fc'; do
  grep -Fq "image: $pinned_image" "$ROOT/observability/compose.yaml"
done

[[ $(grep -Fc 'network_mode: host' "$ROOT/observability/compose.yaml") == 10 ]]
for listener in 127.0.0.1:9090 127.0.0.1:9093 127.0.0.1:12345 127.0.0.1:9100 \
  127.0.0.1:9187 127.0.0.1:9121 127.0.0.1:9122 127.0.0.1:9115; do
  grep -Fq "$listener" "$ROOT/observability/compose.yaml" "$ROOT/observability/loki/loki.yml"
done

# Redis carries two consumers with very different blast radii: affinity digests (fail-open, losing
# one costs a prompt-cache hit) and Codex response history (losing one is a customer-visible 400).
# They now run as separate instances because maxmemory and maxmemory-policy are per-instance, so a
# shared one could not give them independent budgets. Both must stay measured independently.
grep -Fq -- '--redis.password-file=/run/secrets/affinity_redis_password' \
  "$ROOT/observability/compose.yaml"
grep -Fq './secrets/affinity_redis_password:/run/secrets/affinity_redis_password:ro' \
  "$ROOT/observability/compose.yaml"
grep -Fq 'job_name: redis' "$ROOT/observability/prometheus/prometheus.yml"
grep -Fq 'instance_role: history' "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'the history Redis instance is not labelled for alert routing\n' >&2; exit 1; }
grep -Fq 'instance_role: affinity' "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'the affinity Redis instance is not labelled for alert routing\n' >&2; exit 1; }
# redis_exporter reads a JSON address-to-password map, not a bare secret; a plain string makes it
# exit at startup. Verified against v1.67.0. Both instances share one map.
grep -Fq 'printf '"'"'{"redis://127.0.0.1:6379":"%s","redis://127.0.0.1:6380":"%s"}\n'"'"'' \
  "$ROOT/deploy/install-monitoring.sh"
grep -Fq 'chmod 0600 "$STAGE/secrets/affinity_redis_password"' "$ROOT/deploy/install-monitoring.sh"
for redis_exporter_service in 'redis-exporter:' 'redis-exporter-affinity:'; do
  grep -F "$redis_exporter_service" -A 16 "$ROOT/observability/compose.yaml" \
    | grep -Fq 'user: "0:0"' \
    || { printf 'Redis exporter cannot read the root-owned 0600 password map\n' >&2; exit 1; }
  grep -F "$redis_exporter_service" -A 16 "$ROOT/observability/compose.yaml" \
    | grep -Fq 'cap_drop:' \
    || { printf 'root Redis exporter does not drop Linux capabilities\n' >&2; exit 1; }
done
# History keeps the legacy service identity, 6379 and the existing data directory on purpose:
# changing that service would recreate the live container and strand conversations during rollout.
grep -F '  affinity-redis:' -A 24 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq '127.0.0.1:6379:6379' \
  || { printf 'response history must keep its existing port\n' >&2; exit 1; }
grep -F '  affinity-redis:' -A 30 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq '/var/lib/apitoken/affinity-redis:/data' \
  || { printf 'response history must keep its existing data directory\n' >&2; exit 1; }
grep -F '  affinity-redis:' -A 24 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq '8gb' \
  || { printf 'response history budget is not the 8 GiB first-rollout capacity\n' >&2; exit 1; }
grep -F '  cache-affinity-redis:' -A 24 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq '128mb' \
  || { printf 'cache affinity budget must stay 128 MiB\n' >&2; exit 1; }
grep -F '  affinity-redis:' -A 24 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq 'allkeys-lru' \
  || { printf 'response history eviction policy changed during the additive split\n' >&2; exit 1; }
grep -F '  cache-affinity-redis:' -A 24 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq '127.0.0.1:6380:6379' \
  || { printf 'cache affinity is not on its own instance\n' >&2; exit 1; }
for redis_listener in 'wait_http Redis-Exporter http://127.0.0.1:9121/metrics' \
  'wait_http Redis-Exporter-Affinity http://127.0.0.1:9122/metrics'; do
  grep -Fq "$redis_listener" "$ROOT/deploy/install-monitoring.sh" \
    || { printf 'monitoring install can commit before a Redis exporter starts\n' >&2; exit 1; }
done
grep -Fq "wait_prometheus_result Redis 'sum(redis_up == 1) == 2'" "$ROOT/deploy/install-monitoring.sh" \
  || { printf 'monitoring install does not prove both Redis instances are scraped\n' >&2; exit 1; }
grep -Fq 'sum(up{job=~"prometheus|node|postgres|redis|caddy|alertmanager|grafana|loki|alloy|blackbox-exporter"} == 1) == 11' \
  "$ROOT/deploy/install-monitoring.sh" \
  || { printf 'monitoring install target count omits a Redis exporter\n' >&2; exit 1; }
# The affinity alert must not be scoped to one plane. It was pinned to {provider="anthropic"},
# which excluded the OpenAI plane — the only plane where Redis also holds Codex history. It keeps a
# per-provider aggregation so the firing plane is still identifiable.
! grep -Fq 'claude_api_affinity_redis_errors_total{provider="anthropic"}' \
  "$ROOT/observability/prometheus/rules/application.yml"
grep -Fq 'sum by (provider) (increase(claude_api_affinity_redis_errors_total[10m]))' \
  "$ROOT/observability/prometheus/rules/application.yml"

# Redis memory pressure and Codex history loss were entirely unmeasured before these alerts.
# Pin the engine-side export too: an alert on a metric nothing emits is silently always-inactive.
for history_metric in \
  'claude_api_codex_history_write_failures_total' \
  'claude_api_codex_history_misses_total'; do
  grep -Fq "$history_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$history_metric" >&2; exit 1; }
  grep -Fq "$history_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$history_metric" >&2; exit 1; }
done
for redis_alert in AffinityRedisDown AffinityRedisEvictingKeys AffinityRedisMemoryHigh \
  CodexHistoryWriteFailures CodexHistoryMissesElevated; do
  grep -Fq "alert: $redis_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing shared-state alert %s\n' "$redis_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$redis_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$redis_alert" >&2; exit 1; }
  grep -Fqi "## $redis_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$redis_alert" >&2; exit 1; }
done
# A crashed exporter publishes no redis_up series at all, so `== 0` alone would stay silent. With
# two instances the check must be per-instance: a plain absent(redis_up) is satisfied by the
# surviving instance and would hide the dead one. This form covers both, and both-absent too.
grep -Fq 'redis_up == 0 or sum(absent(redis_up{instance_role="history"}) or absent(redis_up{instance_role="affinity"})) > 0' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'Redis-down alert misses a crashed exporter with no redis_up series\n' >&2; exit 1; }

# The billing single-writer hot path (reserve/settle/acquire_capacity plus the write queue) was
# the last unmeasured money surface. Pin export and consumption in both directions: an alert on a
# metric nothing emits is silently always-inactive, and an unalerted saturation signal is unseen.
for billing_metric in \
  'claude_api_billing_pg_command_duration_seconds' \
  'claude_api_billing_write_queue_depth'; do
  grep -Fq "$billing_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$billing_metric" >&2; exit 1; }
  grep -Fq "$billing_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$billing_metric" >&2; exit 1; }
done
for billing_alert in BillingPGCommandLatencyHigh BillingWriteQueueBacklog; do
  grep -Fq "alert: $billing_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing billing hot-path alert %s\n' "$billing_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$billing_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$billing_alert" >&2; exit 1; }
  grep -Fqi "## $billing_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$billing_alert" >&2; exit 1; }
done

# Request-fact operations are one closed metric -> alert -> dashboard -> runbook loop. Labels stay
# compile-bounded; a metric absent from the engine or an alert absent from the rules fails the gate.
for request_fact_metric in \
  claude_api_request_fact_inbox_capacity \
  claude_api_request_fact_inbox_depth \
  claude_api_request_fact_persistence_healthy \
  claude_api_request_fact_submissions_total \
  claude_api_request_fact_persistence_total \
  claude_api_request_fact_stuck_lifecycles \
  claude_api_request_fact_lifecycle_total \
  claude_api_request_fact_duration_seconds; do
  grep -Fq "$request_fact_metric" "$ROOT/crates/server/src/request_fact_metrics.rs" \
    || { printf 'engine does not export %s\n' "$request_fact_metric" >&2; exit 1; }
  grep -Fq "$request_fact_metric" "$ROOT/observability/grafana/dashboards/production-overview.json" \
    || { printf 'request-fact dashboard does not consume %s\n' "$request_fact_metric" >&2; exit 1; }
done
for request_fact_alert in RequestFactPersistenceUnhealthy RequestFactQueuePressure \
  RequestFactDropsHigh RequestFactLifecycleStuck; do
  grep -Fq "alert: $request_fact_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing request-fact alert %s\n' "$request_fact_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$request_fact_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'request-fact alert %s has no runbook anchor\n' "$request_fact_alert" >&2; exit 1; }
  grep -Fqi "## $request_fact_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$request_fact_alert" >&2; exit 1; }
done
grep -Fq 'claude_api_request_fact_inbox_depth / claude_api_request_fact_inbox_capacity >= 0.75' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'request-fact queue-pressure threshold drifted\n' >&2; exit 1; }
grep -Fq 'sum(increase(claude_api_request_fact_submissions_total[15m])) >= 100' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'request-fact drop alert lacks the minimum sample gate\n' >&2; exit 1; }

grep -Fq 'GF_SERVER_HTTP_ADDR: 127.0.0.1' "$ROOT/observability/compose.yaml"
grep -Fq 'GF_SERVER_HTTP_PORT: "3600"' "$ROOT/observability/compose.yaml"
grep -Fq 'http_listen_address: 127.0.0.1' "$ROOT/observability/loki/loki.yml"
grep -Fq 'http_listen_port: 3101' "$ROOT/observability/loki/loki.yml"

grep -Fq 'request_header -X-WEBAUTH-USER' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-WEBAUTH-USER {http.request.header.X-Admin-Actor}' "$ROOT/deploy/Caddyfile"
grep -Fq 'GF_AUTH_PROXY_ENABLED: "true"' "$ROOT/observability/compose.yaml"
grep -Fq 'GF_AUTH_BASIC_ENABLED: "false"' "$ROOT/observability/compose.yaml"
grep -A20 -F 'node-exporter:' "$ROOT/observability/compose.yaml" | grep -Fq 'apparmor=unconfined'
grep -Fq 'metrics {' "$ROOT/deploy/Caddyfile"
grep -Fq 'per_host' "$ROOT/deploy/Caddyfile"

# Anthropic, OpenAI, Gemini and KIMI have independent stable origins and scrape labels. Without
# labels, provider-local zero gauges can collide and make Codex or Claude alerts evaluate against
# both.
for provider_target in \
  '127.0.0.1:8790"]|provider: anthropic' \
  '127.0.0.1:8792"]|provider: openai' \
  '127.0.0.1:8794"]|provider: gemini' \
  '127.0.0.1:8803"]|provider: kimi'; do
  target=${provider_target%%|*}
  label=${provider_target#*|}
  grep -F "$target" -A 1 "$ROOT/observability/prometheus/prometheus.yml" \
    | grep -Fq "$label" || { printf 'engine scrape %s lacks %s\n' "$target" "$label" >&2; exit 1; }
done
# The stateless router owns a separate unauthenticated loopback endpoint. Its counter label matrix is
# compile-bounded, and the engine proof counter is consumed only after aggregation by fixed plane.
grep -F 'job_name: claude-router' -A 4 "$ROOT/observability/prometheus/prometheus.yml" \
  | grep -Fq 'targets: ["127.0.0.1:8802"]' \
  || { printf 'router metrics scrape is missing from stable loopback 8802\n' >&2; exit 1; }
! grep -F 'job_name: claude-router' -A 4 "$ROOT/observability/prometheus/prometheus.yml" \
  | grep -Fq 'authorization:' \
  || { printf 'router loopback metrics unexpectedly require a credential\n' >&2; exit 1; }
grep -Fq 'claude_router_fallback_total' "$ROOT/crates/router/src/metrics.rs" \
  || { printf 'router does not export fallback continuations\n' >&2; exit 1; }
[[ $(grep -Fc 'handle_errors 503 {' "$ROOT/deploy/Caddyfile") == 4 ]] \
  || { printf 'stable provider origins must sign exactly four Caddy no-upstream error paths\n' >&2; exit 1; }
[[ $(grep -Fc 'header X-Apitoken-Execution-State not_started' "$ROOT/deploy/Caddyfile") == 4 ]] \
  || { printf 'stable provider no-upstream paths are missing execution fencing\n' >&2; exit 1; }
[[ $(grep -Fc 'header_down -X-Apitoken-Execution-State' "$ROOT/deploy/Caddyfile") == 3 ]] \
  || { printf 'public provider proxies must strip internal execution fencing\n' >&2; exit 1; }
for router_metric in \
  claude_router_active_universal_requests \
  claude_router_active_body_admission_units \
  claude_router_body_admission_overload_total \
  claude_router_body_read_timeout_total \
  claude_router_request_body_bytes \
  claude_router_body_admission_rejections_total \
  claude_router_body_storage_bytes \
  claude_router_body_memory_cost_bytes \
  claude_router_body_spool_files \
  claude_router_auth_preflight_total \
  claude_router_catalog_refresh_total \
  claude_router_pricing_failure_total \
  claude_router_policy_failure_total \
  claude_router_response_header_timeout_total \
  claude_router_balance_failover_total; do
  grep -Fq "$router_metric" "$ROOT/crates/router/src/metrics.rs" \
    || { printf 'router metric is missing: %s\n' "$router_metric" >&2; exit 1; }
done
grep -Fq 'claude_api_execution_not_started_total' "$ROOT/crates/server/src/http.rs" \
  || { printf 'engine does not export exact not_started proofs\n' >&2; exit 1; }
grep -Fq 'sum by (plane) (rate(claude_api_execution_not_started_total[5m]))' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'no recording rule consumes exact not_started proofs by fixed plane\n' >&2; exit 1; }
for router_alert in RouterMetricsDown RouterFallbackRateHigh RouterConnectionRefusedFallback \
  RouterAdmissionFailures RouterBodyOversizePressure RouterBodySpoolLeak RouterAuthorityFailures \
  RouterResponseHeaderTimeout ProviderBodyAdmissionFailures ProviderBodySpoolLeak \
  GeminiIpcProtocolFailures; do
  grep -Fq "alert: $router_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing router fallback alert %s\n' "$router_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$router_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'router alert %s has no runbook anchor\n' "$router_alert" >&2; exit 1; }
  grep -Fqi "## $router_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$router_alert" >&2; exit 1; }
done
for body_surface in chat responses messages messages_count_tokens; do
  grep -Fq "$body_surface" "$ROOT/crates/router/src/metrics.rs" \
    || { printf 'router body histogram lacks fixed surface %s\n' "$body_surface" >&2; exit 1; }
done
for rejection_reason in oversized read_timeout admission_overload content_encoding; do
  grep -Fq "$rejection_reason" "$ROOT/crates/router/src/metrics.rs" \
    || { printf 'router body rejection metric lacks fixed reason %s\n' "$rejection_reason" >&2; exit 1; }
done
for forbidden_label in 'path=' 'key=' 'account=' 'model=' 'request_id='; do
  ! grep -F 'claude_router_request_body_bytes' "$ROOT/crates/router/src/metrics.rs" \
    | grep -Fq "$forbidden_label" \
    || { printf 'router body histogram contains forbidden label %s\n' "$forbidden_label" >&2; exit 1; }
done
oversize_alert=$(awk '/- alert: RouterBodyOversizePressure/{flag=1} flag{print} flag && /runbook:/{exit}' "$ROOT/observability/prometheus/rules/application.yml")
grep -Fq 'increase(claude_router_body_admission_rejections_total{reason="oversized"}[15m]) >= 10' <<<"$oversize_alert" \
  || { printf 'router oversize alert does not consume the fixed rejection signal\n' >&2; exit 1; }
! grep -Fq 'for:' <<<"$oversize_alert" \
  || { printf 'router oversize rolling-window alert has a contradictory extra for duration\n' >&2; exit 1; }
for quantile in 0.50 0.95 0.99; do
  grep -Fq "histogram_quantile($quantile, sum by (le, surface) (rate(claude_router_request_body_bytes_bucket[15m])))" \
    "$ROOT/observability/grafana/dashboards/production-overview.json" \
    || { printf 'router body dashboard lacks quantile %s\n' "$quantile" >&2; exit 1; }
done
grep -Fq 'claude_api_body_admission_rejections_total' "$ROOT/crates/forward/src/metrics.rs" \
  || { printf 'engine does not export body admission rejections\n' >&2; exit 1; }
grep -Fq 'claude_api_body_storage_bytes' "$ROOT/crates/server/src/http.rs" \
  || { printf 'engine does not export body storage gauges\n' >&2; exit 1; }
grep -Fq 'claude_api_body_spool_files' "$ROOT/crates/server/src/http.rs" \
  || { printf 'engine does not export live spool-file gauge\n' >&2; exit 1; }
grep -Fq 'claude_api_gemini_ipc_protocol_failures_total' "$ROOT/crates/forward/src/metrics.rs" \
  || { printf 'engine does not export Gemini IPC protocol failures\n' >&2; exit 1; }
grep -Fq 'sum by (reason) (increase(claude_api_body_admission_rejections_total[1h]))' \
  "$ROOT/observability/grafana/dashboards/production-overview.json" \
  || { printf 'provider body dashboard lacks rejection series\n' >&2; exit 1; }
grep -Fq 'claude_api_gemini_ipc_bytes_total' \
  "$ROOT/observability/grafana/dashboards/production-overview.json" \
  || { printf 'Gemini IPC dashboard panel is missing\n' >&2; exit 1; }
# The generic scrape alert must exclude jobs with their own scrape-health alerts:
# claude-router -> RouterMetricsDown, devbot -> DevBotMetricsDown.
grep -Fq 'up{job!~"claude-router|devbot"} == 0' "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'generic scrape alert would duplicate RouterMetricsDown/DevBotMetricsDown\n' >&2; exit 1; }
grep -Fq 'targets: ["https://openai.api.apitoken.sale/v1/responses"]' \
  "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'OpenAI public synthetic is missing\n' >&2; exit 1; }
grep -Fq 'module: [http_openai_surface]' "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'OpenAI synthetic does not verify its provider envelope\n' >&2; exit 1; }
grep -Fq 'targets: ["https://gemini.api.apitoken.sale/v1beta/models/gemini-provider-probe:generateContent"]' \
  "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'Gemini public synthetic is missing\n' >&2; exit 1; }
grep -Fq 'module: [http_gemini_surface]' "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'Gemini synthetic does not verify its provider envelope\n' >&2; exit 1; }
grep -Fq 'valid_status_codes: [400, 401, 404]' "$ROOT/observability/blackbox/blackbox.yml" \
  || { printf 'Gemini synthetic does not accept both enabled and kill-switch envelopes\n' >&2; exit 1; }
grep -Fq 'fail_if_body_not_matches_regexp:' "$ROOT/observability/blackbox/blackbox.yml" \
  || { printf 'OpenAI synthetic accepts a generic health response\n' >&2; exit 1; }
grep -Fq "body: '{}'" "$ROOT/observability/blackbox/blackbox.yml" \
  || { printf 'OpenAI synthetic can execute a real provider turn\n' >&2; exit 1; }
grep -Fq '"code"' "$ROOT/observability/blackbox/blackbox.yml" \
  || { printf 'OpenAI synthetic accepts a generic error envelope\n' >&2; exit 1; }

printf '%s\n' \
  'EMAIL_FROM=no-reply@apitoken.sale' \
  'SMTP_HOST=smtp.example.test' \
  'SMTP_PORT=587' \
  'SMTP_USERNAME=monitoring-user' \
  'SMTP_PASSWORD=monitoring-password' >"$TEMP/worker.env"
printf '%s\n' \
  'GRAFANA_ADMIN_PASSWORD=irrelevant' \
  'MONITORING_POSTGRES_PASSWORD=irrelevant' >"$TEMP/monitoring.env"
node "$ROOT/deploy/render-alertmanager.mjs" \
  "$ROOT/observability/alertmanager/alertmanager.yml.template" \
  "$TEMP/worker.env" "$TEMP/monitoring.env" "$TEMP/alertmanager.yml"
grep -Fq 'smtp_smarthost: "smtp.example.test:587"' "$TEMP/alertmanager.yml"
! grep -Fq 'email_configs' "$TEMP/alertmanager.yml"
! grep -Eq '__[A-Z0-9_]+__' "$TEMP/alertmanager.yml"

# Without DEVBOT_AM_SECRET the optional Telegram fan-out is stripped entirely, and the email
# receiver renders exactly as before (expand-only; the monitoring install must not break before
# the operator provisions the bot).
! grep -Fq 'devbot-telegram' "$TEMP/alertmanager.yml" \
  || { printf 'devbot block rendered without DEVBOT_AM_SECRET\n' >&2; exit 1; }
grep -Fq 'name: production-email' "$TEMP/alertmanager.yml" \
  || { printf 'email receiver lost while stripping the devbot block\n' >&2; exit 1; }
! grep -Fq '# DEVBOT-BEGIN' "$TEMP/alertmanager.yml" \
  || { printf 'devbot marker leaked into the rendered config\n' >&2; exit 1; }
# With the secret provisioned the webhook receiver renders with the URL-encoded path secret,
# fans out via continue: true, and keeps the email receiver beside it.
DEVBOT_AM_SECRET='devbot+secret/with=chars' node "$ROOT/deploy/render-alertmanager.mjs" \
  "$ROOT/observability/alertmanager/alertmanager.yml.template" \
  "$TEMP/worker.env" "$TEMP/monitoring.env" "$TEMP/alertmanager-devbot.yml"
grep -Fq 'name: devbot-telegram' "$TEMP/alertmanager-devbot.yml" \
  || { printf 'devbot receiver did not render with DEVBOT_AM_SECRET set\n' >&2; exit 1; }
grep -Fq 'url: http://127.0.0.1:3800/alerts/devbot%2Bsecret%2Fwith%3Dchars' \
  "$TEMP/alertmanager-devbot.yml" \
  || { printf 'devbot webhook secret is not URL-encoded into the path\n' >&2; exit 1; }
grep -Fq 'continue: true' "$TEMP/alertmanager-devbot.yml" \
  || { printf 'devbot route does not continue to the email tree\n' >&2; exit 1; }
grep -Fq 'name: production-email' "$TEMP/alertmanager-devbot.yml" \
  || { printf 'devbot rendering dropped the email receiver\n' >&2; exit 1; }
! grep -Eq '__[A-Z0-9_]+__' "$TEMP/alertmanager-devbot.yml" \
  || { printf 'unresolved placeholder in the devbot render\n' >&2; exit 1; }
# install-monitoring.sh sources the same secret file the bot reads, tolerating its absence.
grep -Fq '/etc/apitoken/devbot.env' "$ROOT/deploy/install-monitoring.sh" \
  || { printf 'monitoring installer does not source the devbot env file\n' >&2; exit 1; }
grep -Fq 'DEVBOT_AM_SECRET=$devbot_am_secret node' "$ROOT/deploy/install-monitoring.sh" \
  || { printf 'monitoring installer does not pass DEVBOT_AM_SECRET to the renderer\n' >&2; exit 1; }

grep -Fq 'apitoken_backup_present{database="%s"}' "$ROOT/deploy/collect-monitoring-metrics.sh"
[[ $(tail -n 3 "$ROOT/deploy/collect-monitoring-metrics.sh") == $'mv -f -- "$temporary" "$OUTPUT_DIR/apitoken.prom"\ncleanup\ntrap - EXIT' ]] \
  || { printf 'collector does not remove root-only authority snapshots after publish\n' >&2; exit 1; }

# Deployment-pipeline visibility. A failed or stalled delivery is otherwise only observable in the
# GitHub UI, so the collector must export it and the rules must alert on it.
for watchdog_metric in \
  'apitoken_watchdog_quarantined' \
  'apitoken_watchdog_status_age_seconds' \
  'apitoken_watchdog_phase' \
  'apitoken_watchdog_pending_migration'; do
  grep -Fq "$watchdog_metric" "$ROOT/deploy/collect-monitoring-metrics.sh" \
    || { printf 'collector does not export %s\n' "$watchdog_metric" >&2; exit 1; }
  grep -Fq "$watchdog_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$watchdog_metric" >&2; exit 1; }
done
# The native encrypted OAuth pool is independent from both established providers.
for gemini_metric in \
  'claude_api_gemini_profiles_available' \
  'claude_api_gemini_profiles_authenticated' \
  'claude_api_gemini_profile_authenticated'; do
  grep -Fq "$gemini_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$gemini_metric" >&2; exit 1; }
  grep -Fq "$gemini_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$gemini_metric" >&2; exit 1; }
  grep -Fq "${gemini_metric}{provider=\"gemini\"}" \
    "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'Gemini alert is not scoped to Gemini: %s\n' "$gemini_metric" >&2; exit 1; }
done
for gemini_alert in GeminiProviderDown GeminiNoAvailableProfiles GeminiProfileUnauthenticated \
  GeminiUpstreamRateLimited; do
  grep -Fq "alert: $gemini_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing Gemini alert %s\n' "$gemini_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$gemini_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$gemini_alert" >&2; exit 1; }
  grep -Fqi "## $gemini_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$gemini_alert" >&2; exit 1; }
done
for watchdog_alert in DeployQuarantined DeployPipelineStale DeployStuckInPhase DeployMigrationUncommitted; do
  grep -Fq "alert: $watchdog_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing deployment alert %s\n' "$watchdog_alert" >&2; exit 1; }
  # Every alert must carry a runbook anchor that actually exists in docs/ops/MONITORING.md.
  anchor=$(printf '%s' "$watchdog_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$watchdog_alert" >&2; exit 1; }
  grep -Fqi "## $watchdog_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$watchdog_alert" >&2; exit 1; }
done

# The devbot heartbeat alert is a closed unit -> alert -> runbook loop, warning-only. It must
# stay gated on the systemd unit being active (a disabled bot is intentionally silent, see the
# ConditionPathExists in the unit) and consume the textfile heartbeat the bot publishes.
grep -Fq 'alert: DevBotHeartbeatMissing' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'missing DevBotHeartbeatMissing alert\n' >&2; exit 1; }
grep -Fq 'severity: warning' \
  <(grep -F 'alert: DevBotHeartbeatMissing' -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
  || { printf 'DevBotHeartbeatMissing must stay a warning\n' >&2; exit 1; }
grep -Fq 'node_systemd_unit_state{name="apitoken-devbot.service",state="active"} == 1' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'DevBotHeartbeatMissing is not gated on the active devbot unit\n' >&2; exit 1; }
grep -Fq 'devbot_heartbeat_timestamp_seconds' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'DevBotHeartbeatMissing does not consume the heartbeat metric\n' >&2; exit 1; }
grep -Fq 'docs/ops/MONITORING.md#devbotheartbeatmissing' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'DevBotHeartbeatMissing has no runbook anchor\n' >&2; exit 1; }
grep -Fqi '## DevBotHeartbeatMissing' "$ROOT/docs/ops/MONITORING.md" \
  || { printf 'docs/ops/MONITORING.md has no runbook section for DevBotHeartbeatMissing\n' >&2; exit 1; }
# The bot writes its heartbeat next to the collector output, so the textfile directory must be
# group-writable by deploy from the monitoring installer.
grep -Fq 'install -d -o root -g deploy -m 0775 /var/lib/apitoken/monitoring/textfile' \
  "$ROOT/deploy/install-monitoring.sh" \
  || { printf 'textfile directory is not writable by the devbot service user\n' >&2; exit 1; }
# The root collector re-creates the same directory every minute; it must preserve the
# group-deploy writability instead of reverting it, or the heartbeat write fails with EACCES.
grep -Fq 'install -d -o root -g deploy -m 0775 "$OUTPUT_DIR"' \
  "$ROOT/deploy/collect-monitoring-metrics.sh" \
  || { printf 'collector reverts textfile directory ownership, breaking the devbot heartbeat\n' >&2; exit 1; }
# node-exporter reads the textfile as user nobody, so the bot must publish its heartbeat 0644
# regardless of the unit's UMask=0077.
grep -Fq 'await fs.chmod(tmp, 0o644);' "$ROOT/apps/devbot/src/am-webhook.ts" \
  || { printf 'devbot heartbeat is not world-readable for node-exporter\n' >&2; exit 1; }
# The devbot delivery alerts are a closed metric -> alert -> runbook loop. Every new devbot
# alert must stay a warning, carry a runbook anchor, and have a runbook section.
for devbot_alert in DevBotTelegramSendFailures DevBotWebhookDeliveryFailing DevBotWebhookSilent DevBotMetricsDown; do
  grep -Fq "alert: $devbot_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing devbot alert %s\n' "$devbot_alert" >&2; exit 1; }
  grep -Fq 'severity: warning' \
    <(grep -F "alert: $devbot_alert" -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
    || { printf 'devbot alert %s must stay a warning\n' "$devbot_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$devbot_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'devbot alert %s has no runbook anchor\n' "$devbot_alert" >&2; exit 1; }
  grep -Fqi "## $devbot_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$devbot_alert" >&2; exit 1; }
done
# The delivery signals live in the bot's own /metrics: Prometheus must scrape 127.0.0.1:3800.
grep -Fq 'job_name: devbot' "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'no devbot scrape job in prometheus.yml\n' >&2; exit 1; }
grep -Fq '127.0.0.1:3800' "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'devbot scrape job does not target 127.0.0.1:3800\n' >&2; exit 1; }
grep -Fq 'devbot_telegram_send_failures_total' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'no rule consumes devbot_telegram_send_failures_total\n' >&2; exit 1; }
grep -Fq 'devbot_last_webhook_seconds' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'no rule consumes devbot_last_webhook_seconds\n' >&2; exit 1; }
grep -Fq 'devbot_last_webhook_seconds' "$ROOT/apps/devbot/src/am-webhook.ts" \
  || { printf 'devbot does not export the last-webhook metric\n' >&2; exit 1; }
# Guardrail: lastWebhookTs must not initialize to 0. Prometheus then computes
# time()-0 > 86400 and DevBotWebhookSilent false-fires 30 min after every restart
# (2026-08-22 Chatwoot cutover).
if grep -Eq 'lastWebhookTs[[:space:]]*=[[:space:]]*0' "$ROOT/apps/devbot/src/am-webhook.ts"; then
  printf 'lastWebhookTs must not initialize to 0 (DevBotWebhookSilent epoch false-fire)\n' >&2
  exit 1
fi
grep -Fq 'Seeded to process start' "$ROOT/apps/devbot/src/am-webhook.ts" \
  || { printf 'lastWebhookTs seed comment is missing\n' >&2; exit 1; }
grep -Fq 'devbot_last_chatwoot_seconds' "$ROOT/apps/devbot/src/am-webhook.ts" \
  || { printf 'devbot does not export the last-chatwoot metric\n' >&2; exit 1; }
grep -Fq 'alertmanager_notifications_failed_total{integration="webhook"}' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'DevBotWebhookDeliveryFailing does not consume Alertmanager webhook failures\n' >&2; exit 1; }
# The devbot unit itself is part of the systemd failure and restart-loop coverage; a dead bot
# must page instead of silencing the channel. The heartbeat rule stays pinned to the node
# textfile series even though the bot /metrics now carries the same gauge name.
grep -Fq '|devbot)' "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'devbot is not covered by the systemd unit patterns\n' >&2; exit 1; }
grep -Fq 'devbot_heartbeat_timestamp_seconds{job="node"}' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'DevBotHeartbeatMissing is not pinned to the textfile series\n' >&2; exit 1; }
grep -Fq 'up{job!~"claude-router|devbot"}' "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'MonitoringTargetDown does not exclude the devbot job\n' >&2; exit 1; }

# Money conservation must be a closed collector -> alert -> runbook loop. Pin the operands as well
# as the metric name so a constant-zero replacement cannot satisfy this static contract.
reconciliation_metric=apitoken_balance_divergence_nano
for required in \
  "$reconciliation_metric" \
  "kind IN ('topup', 'adjust')" \
  'account.balance_nano::numeric' \
  'account.spent_nano::numeric' \
  'account.reserved_nano::numeric' \
  'account.uncollected_nano::numeric'; do
  grep -Fq "$required" "$ROOT/deploy/collect-monitoring-metrics.sh" \
    || { printf 'balance reconciliation collector is missing %s\n' "$required" >&2; exit 1; }
done
grep -Fq "$reconciliation_metric" "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'no alert rule consumes %s\n' "$reconciliation_metric" >&2; exit 1; }
grep -Fq 'alert: BalanceDivergenceDetected' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'missing balance-divergence alert\n' >&2; exit 1; }
grep -Fq 'severity: critical' \
  <(grep -F 'alert: BalanceDivergenceDetected' -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
  || { printf 'balance-divergence alert is not critical\n' >&2; exit 1; }
grep -Fq 'docs/ops/MONITORING.md#balancedivergencedetected' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'balance-divergence alert has no runbook anchor\n' >&2; exit 1; }
grep -Fqi '## BalanceDivergenceDetected' "$ROOT/docs/ops/MONITORING.md" \
  || { printf 'docs/ops/MONITORING.md has no balance-divergence runbook\n' >&2; exit 1; }

# A settlement that reaches the shared floor must preserve full usage as collected + explicit
# shortfall. Pin the collector operands, the bounded provider dimension, both severity thresholds,
# and both runbooks so a zero placeholder or a retired account-class join cannot hide money loss.
for settlement_evidence in \
  apitoken_settlement_uncollected_nano \
  'SUM(uncollected_nano)' \
  'amount_nano::numeric / official_nano' \
  "VALUES ('anthropic'), ('openai'), ('google'), ('kimi'), ('glm'), ('tripo3d'), ('suno')"; do
  grep -Fq "$settlement_evidence" "$ROOT/deploy/collect-monitoring-metrics.sh" \
    || { printf 'settlement evidence collector is missing %s\n' "$settlement_evidence" >&2; exit 1; }
done
for settlement_alert in SettlementUncollectedDetected SettlementUncollectedHigh; do
  lowercase_alert=$(printf '%s' "$settlement_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "alert: $settlement_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing %s alert\n' "$settlement_alert" >&2; exit 1; }
  grep -Fq "docs/ops/MONITORING.md#$lowercase_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf '%s alert has no runbook anchor\n' "$settlement_alert" >&2; exit 1; }
  grep -Fqi "## $settlement_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no %s runbook\n' "$settlement_alert" >&2; exit 1; }
done
grep -Fq 'severity: warning' \
  <(grep -F 'alert: SettlementUncollectedDetected' -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
  || { printf 'bounded settlement shortfall alert is not warning severity\n' >&2; exit 1; }
grep -Fq 'severity: critical' \
  <(grep -F 'alert: SettlementUncollectedHigh' -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
  || { printf 'high settlement shortfall alert is not critical severity\n' >&2; exit 1; }
if grep -Fq 'apitoken_pricing_charge_mismatch{account_class=' "$ROOT/deploy/collect-monitoring-metrics.sh"; then
  printf 'pricing mismatch still depends on retired account-class attribution\n' >&2
  exit 1
fi

# Both 2026-08 pricing incidents were silent because every component was individually healthy
# while two of them disagreed about one fact. Each drift detector must therefore be a closed
# collector -> alert -> runbook loop, with its operands pinned so a constant-zero replacement
# cannot satisfy the contract. The retired design's gauges (account_policy_bindings,
# engine_policy_jobs, engine_catalog_jobs, engine_switch_jobs) are gone and must not come back:
# nothing drains those lanes, so their counts described a state that can no longer advance.
for drift_metric in \
  apitoken_pricing_mirror_drift \
  apitoken_pricing_job_stale_confirmed \
  apitoken_sales_feed_head \
  apitoken_sales_attribution_feed_head \
  apitoken_sales_topups_feed_head \
  apitoken_sales_reversal_feed_head \
  apitoken_sales_cursor_age_seconds \
  apitoken_sales_sync_errors_recent \
  apitoken_sales_sync_journal_up \
  apitoken_sales_accounting_incomplete \
  apitoken_sales_partner_debt_nano \
  apitoken_engine_accounts_below_floor \
  apitoken_pricing_authority_drift \
  apitoken_business_reconciliation_up \
  apitoken_usage_provider_unresolved \
  apitoken_openkeys_pricing_drift \
  apitoken_openkeys_legacy_rows; do
  grep -Fq "$drift_metric" "$ROOT/deploy/collect-monitoring-metrics.sh" \
    || { printf 'collector does not export %s\n' "$drift_metric" >&2; exit 1; }
  grep -Fq "$drift_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$drift_metric" >&2; exit 1; }
done
for drift_operand in \
  'ea.mult_bp IS DISTINCT FROM cp.multiplier_bp' \
  "j.status = 'confirmed'" \
  'LEFT JOIN sync_cursors' \
  'balance_nano < -1000000000' \
  'FROM customer_provider_discounts' \
  'FROM account_provider_discounts' \
  'provider_id IS NULL OR provider_id IN' \
  "batch.pricing_contract = 'official_1_to_1'"; do
  grep -Fq "$drift_operand" "$ROOT/deploy/collect-monitoring-metrics.sh" \
    || { printf 'drift collector is missing %s\n' "$drift_operand" >&2; exit 1; }
done
for drift_alert in SalesSyncCursorStalled SalesAttributionSyncCursorStalled SalesTopupSyncCursorStalled \
  SalesFundingSyncCursorStalled SalesReversalSyncCursorStalled SalesPartnerAccountingIncomplete \
  SalesPartnerDebtPresent SalesSyncIterationFailing PricingMirrorDrift PricingJobStaleConfirmed \
  EngineAccountsBelowFloor BusinessReconciliationUnavailable PricingAuthorityDrift \
  UsageProviderAttributionMissing OpenKeysPricingDrift PositiveBalancePaymentRequired; do
  grep -Fq "alert: $drift_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing %s alert\n' "$drift_alert" >&2; exit 1; }
  lowercase_alert=$(printf '%s' "$drift_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$lowercase_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf '%s alert has no runbook anchor\n' "$drift_alert" >&2; exit 1; }
  grep -Fqi "## $drift_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no %s runbook\n' "$drift_alert" >&2; exit 1; }
done
grep -Fq 'claude_api_positive_balance_402_total' "$ROOT/crates/server/src/http.rs" \
  || { printf 'engine does not export positive-balance 402 observations\n' >&2; exit 1; }
grep -Fq 'increase(claude_api_positive_balance_402_total[10m]) > 0' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'positive-balance 402 observations do not alert\n' >&2; exit 1; }
grep -Fq 'engine_pricing_jobs' "$ROOT/docs/ops/MONITORING.md" \
  || { printf 'pricing queue runbook omits the only live scalar queue\n' >&2; exit 1; }
grep -Fq 'severity: critical' \
  <(grep -F 'alert: SalesSyncCursorStalled' -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
  || { printf 'sales-sync stall alert is not critical\n' >&2; exit 1; }
grep -Fq 'severity: critical' \
  <(grep -F 'alert: SalesTopupSyncCursorStalled' -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
  || { printf 'sales-topup-sync stall alert is not critical\n' >&2; exit 1; }
for partner_cursor_alert in SalesSyncCursorStalled SalesAttributionSyncCursorStalled \
  SalesTopupSyncCursorStalled SalesFundingSyncCursorStalled SalesReversalSyncCursorStalled; do
  alert_block=$(grep -F "alert: $partner_cursor_alert" -A 12 "$ROOT/observability/prometheus/rules/application.yml")
  grep -Fq '> 300' <<<"$alert_block" \
    || { printf '%s does not use the five-minute cursor age\n' "$partner_cursor_alert" >&2; exit 1; }
  grep -Fq 'for: 5m' <<<"$alert_block" \
    || { printf '%s does not alert after a five-minute hold\n' "$partner_cursor_alert" >&2; exit 1; }
done
grep -Fq "journalctl -q -u apitoken-sales-api.service --since '5 minutes ago' -n 1000" \
  "$ROOT/deploy/collect-monitoring-metrics.sh" \
  || { printf 'sales sync journal query is not bounded to five minutes\n' >&2; exit 1; }
grep -Fxq 'SupplementaryGroups=systemd-journal' "$collector_unit" \
  || { printf 'monitoring collector cannot read the sales service journal\n' >&2; exit 1; }
grep -Fq 'Partner sync intervention' "$ROOT/observability/grafana/dashboards/production-overview.json" \
  || { printf 'production dashboard omits the partner sync intervention panel\n' >&2; exit 1; }
for partner_dashboard_metric in apitoken_sales_pending_referral_events apitoken_sales_sync_errors_recent \
  apitoken_sales_sync_journal_up apitoken_sales_attribution_feed_head apitoken_sales_feed_head \
  apitoken_sales_topups_feed_head apitoken_sales_reversal_feed_head \
  apitoken_sales_accounting_incomplete apitoken_sales_partner_debt_nano; do
  grep -Fq "$partner_dashboard_metric" "$ROOT/observability/grafana/dashboards/production-overview.json" \
    || { printf 'partner sync dashboard omits %s\n' "$partner_dashboard_metric" >&2; exit 1; }
done
grep -Fq 'severity: critical' \
  <(grep -F 'alert: PricingMirrorDrift' -A 8 "$ROOT/observability/prometheus/rules/application.yml") \
  || { printf 'pricing mirror drift alert is not critical\n' >&2; exit 1; }
for retired_pricing_series in \
  account_policy_bindings \
  'queue="engine_policy_jobs"' \
  'queue="engine_catalog_jobs"' \
  'queue="engine_switch_jobs"' \
  apitoken_pricing_policy_pending; do
  if grep -Fq "$retired_pricing_series" "$ROOT/deploy/collect-monitoring-metrics.sh"; then
    printf 'collector still exports the retired pricing series %s\n' "$retired_pricing_series" >&2
    exit 1
  fi
done
for removed_pricing_queue in \
  pricing_release_control_jobs_v2 \
  pricing_funding_normalizations_v2 \
  pricing_shadow_policy_jobs_v2 \
  pricing_shadow_rollouts_v2 \
  pricing_stage8_capture_jobs_v2; do
  if grep -Fq "queue=\"$removed_pricing_queue\"" "$ROOT/deploy/collect-monitoring-metrics.sh"; then
    printf 'collector still exports the deleted release-cycle queue %s\n' "$removed_pricing_queue" >&2
    exit 1
  fi
done
for durable_alert in DurableQueueBacklog DurableQueueOldestItemStale DurableQueueDeadItems; do
  grep -Fqi "## $durable_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$durable_alert" >&2; exit 1; }
done

# The journal must be an explicit, bounded, persistent store. Under the default `Storage=auto`
# journald decided volatile-vs-persistent from whether the Docker-created /var/log/journal existed,
# put the journal in tmpfs, and lost it on every reboot; the SystemMaxUse default of 10% of the
# filesystem is also far too large for the production array.
journald_dropin=$ROOT/systemd/journald-apitoken.conf
for journald_setting in Storage=persistent SystemMaxUse=8G SystemMaxFileSize=512M MaxRetentionSec=90day; do
  grep -Fxq "$journald_setting" "$journald_dropin" \
    || { printf 'journald drop-in is missing %s\n' "$journald_setting" >&2; exit 1; }
done
# Installing the drop-in without applying it would leave the journal volatile until the next reboot,
# and applying it unconditionally would interrupt logging on every unrelated infrastructure commit.
grep -Fq 'journald.conf.d/10-apitoken.conf' "$ROOT/deploy/install-watchdog.sh" \
  || { printf 'the journald drop-in is never installed\n' >&2; exit 1; }
grep -Fq 'cmp -s "$ROOT/systemd/journald-apitoken.conf"' "$ROOT/deploy/install-watchdog.sh" \
  || { printf 'the journald restart is not guarded by a content comparison\n' >&2; exit 1; }
grep -Fq 'install -d -o root -g systemd-journal -m 2755 /var/log/journal' "$ROOT/deploy/install-watchdog.sh" \
  || { printf 'the persistent journal directory ownership is not corrected\n' >&2; exit 1; }

# Caddy no longer logs failed active health probes, because blue-green keeps one slot of each pair
# deliberately stopped. That removed signal must stay covered by an alert on upstream health.
grep -Fq 'alert: ProxyUpstreamPairDown' "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'the excluded health-check logger has no replacement alert\n' >&2; exit 1; }
grep -Fq 'docs/ops/MONITORING.md#proxyupstreampairdown' "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'alert ProxyUpstreamPairDown has no runbook anchor\n' >&2; exit 1; }
grep -Fqi '## ProxyUpstreamPairDown' "$ROOT/docs/ops/MONITORING.md" \
  || { printf 'docs/ops/MONITORING.md has no runbook section for ProxyUpstreamPairDown\n' >&2; exit 1; }
proxy_pair_rule=$(grep -F 'alert: ProxyUpstreamPairDown' -A 12 \
  "$ROOT/observability/prometheus/rules/operations.yml")
grep -Fq '3000|3001' <<<"$proxy_pair_rule" \
  || { printf 'commerce pair health alert is missing\n' >&2; exit 1; }
! grep -Fq '8787|8788' <<<"$proxy_pair_rule" \
  || { printf 'transitional OpenAI health aliases corrupt the Anthropic Caddy gauge\n' >&2; exit 1; }

# OpenKeys has a dedicated port and failure domain; do not attribute it to the legacy CRM service.
grep -Fq 'targets: ["http://127.0.0.1:3410/api/ready"]' "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'OpenKeys loopback readiness probe is missing\n' >&2; exit 1; }
grep -F 'targets: ["http://127.0.0.1:3410/api/ready"]' -A 1 \
  "$ROOT/observability/prometheus/prometheus.yml" | grep -Fq 'component: openkeys' \
  || { printf 'OpenKeys readiness probe has the wrong component label\n' >&2; exit 1; }
grep -Fq 'targets: ["https://openkeys.apitoken.sale/api/ready"]' \
  "$ROOT/observability/prometheus/prometheus.yml" \
  || { printf 'OpenKeys external readiness probe is missing\n' >&2; exit 1; }
[[ $(grep -c 'crm-web|openkeys' "$ROOT/observability/prometheus/rules/operations.yml") -eq 2 ]] \
  || { printf 'OpenKeys is missing from systemd failure or restart-loop alerts\n' >&2; exit 1; }
# The optional Codex provider is a separate failure domain: its homes can expire, cool or exhaust
# their subscription windows without any Claude signal moving. Every gauge the engine exports for it
# must be consumed by a rule, and every rule must have a runbook section.
for codex_metric in \
  'claude_api_codex_process_live' \
  'claude_api_codex_homes_available' \
  'claude_api_codex_home_authenticated' \
  'claude_api_codex_home_calibration_persistence_ok' \
  'claude_api_codex_home_rate_limit_used_percent' \
  'claude_api_codex_home_limit_reached'; do
  grep -Fq "$codex_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$codex_metric" >&2; exit 1; }
  grep -Fq "$codex_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$codex_metric" >&2; exit 1; }
  grep -Fq "${codex_metric}{provider=\"openai\"}" \
    "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'Codex alert is not scoped to OpenAI: %s\n' "$codex_metric" >&2; exit 1; }
done
for codex_alert in CodexProviderDown CodexNoAvailableHomes CodexHomeUnauthenticated \
  CodexHomeNearRateLimit CodexCalibrationPersistenceFailed; do
  grep -Fq "alert: $codex_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing Codex alert %s\n' "$codex_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$codex_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$codex_alert" >&2; exit 1; }
  grep -Fqi "## $codex_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$codex_alert" >&2; exit 1; }
done
# Alerting on a disabled provider would page for a surface nobody is serving.
for gated_alert in CodexProviderDown CodexNoAvailableHomes; do
  grep -F "alert: $gated_alert" -A 2 "$ROOT/observability/prometheus/rules/application.yml" \
    | grep -Fq 'claude_api_codex_enabled{provider="openai"} == 1' \
    || { printf '%s is not gated on the provider being enabled\n' "$gated_alert" >&2; exit 1; }
done
# The backend-only KIMI plane serves its own loopback origin: its aggregate series deliberately
# carry no provider label in the metric text — the scrape target attaches provider: kimi exactly
# like the other planes — so the claude_api_kimi_ name prefix and the enabled gate remain the
# discriminators, and the scoping pin here is the enabled gate, not a label selector.
for kimi_metric in \
  'claude_api_kimi_live_profiles' \
  'claude_api_kimi_available_profiles' \
  'claude_api_kimi_calibration_persistence_ok' \
  'claude_api_kimi_calibration_pending_events' \
  'claude_api_kimi_quota_last_observation_timestamp_seconds'; do
  grep -Fq "$kimi_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$kimi_metric" >&2; exit 1; }
  grep -Fq "$kimi_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$kimi_metric" >&2; exit 1; }
done
for kimi_alert in KimiNoLiveProfiles KimiNoAvailableProfiles KimiCalibrationPersistenceFailed \
  KimiCalibrationBacklog KimiQuotaStale; do
  grep -Fq "alert: $kimi_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing KIMI alert %s\n' "$kimi_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$kimi_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$kimi_alert" >&2; exit 1; }
  grep -Fqi "## $kimi_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$kimi_alert" >&2; exit 1; }
done
# Every KIMI rule is gated on the default-off plane being enabled, or it would page for a surface
# nobody is serving.
for gated_alert in KimiNoLiveProfiles KimiNoAvailableProfiles KimiCalibrationPersistenceFailed \
  KimiCalibrationBacklog KimiQuotaStale; do
  grep -F "alert: $gated_alert" -A 2 "$ROOT/observability/prometheus/rules/application.yml" \
    | grep -Fq 'claude_api_kimi_enabled == 1' \
    || { printf '%s is not gated on the provider being enabled\n' "$gated_alert" >&2; exit 1; }
done
# Customer KIMI traffic is the dedicated scrape (provider=kimi). Unscoped gauges let
# Anthropic-process zeros win Grafana lastNotNull.
grafana_overview="$ROOT/observability/grafana/dashboards/production-overview.json"
grep -Fq 'claude_api_kimi_requests_total{provider=' "$grafana_overview" \
  || { printf 'production overview omits scoped KIMI request traffic\n' >&2; exit 1; }
grep -Fq 'claude_api_kimi_inflight_requests{provider=' "$grafana_overview" \
  || { printf 'production overview omits scoped KIMI inflight\n' >&2; exit 1; }
grep -Fq 'claude_api_kimi_live_profiles{provider=' "$grafana_overview" \
  || { printf 'KIMI live-profile card is not scoped to the dedicated scrape\n' >&2; exit 1; }
grep -Fq 'claude_api_kimi_available_profiles{provider=' "$grafana_overview" \
  || { printf 'KIMI available-profile card is not scoped to the dedicated scrape\n' >&2; exit 1; }
! grep -Fq 'claude_api_kimi_live_profiles or vector(0)' "$grafana_overview" \
  || { printf 'KIMI live-profile card still mixes Anthropic zeros\n' >&2; exit 1; }
# The backend-only GLM plane runs inside the Anthropic runtime: its aggregate series are scraped
# on the anthropic target and deliberately carry no provider label — the claude_api_glm_ name
# prefix is the discriminator, so the scoping pin here is the enabled gate, not a label selector.
for glm_metric in \
  'claude_api_glm_live_profiles' \
  'claude_api_glm_available_profiles' \
  'claude_api_glm_calibration_persistence_ok' \
  'claude_api_glm_calibration_pending_events' \
  'claude_api_glm_quota_last_observation_timestamp_seconds' \
  'claude_api_glm_account_dead_profiles'; do
  grep -Fq "$glm_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$glm_metric" >&2; exit 1; }
  grep -Fq "$glm_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$glm_metric" >&2; exit 1; }
done
for glm_alert in GlmNoLiveProfiles GlmNoAvailableProfiles GlmCalibrationPersistenceFailed \
  GlmCalibrationBacklog GlmQuotaStale GlmAccountDead; do
  grep -Fq "alert: $glm_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing GLM alert %s\n' "$glm_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$glm_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$glm_alert" >&2; exit 1; }
  grep -Fqi "## $glm_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$glm_alert" >&2; exit 1; }
done
# Every GLM rule is gated on the default-off plane being enabled, or it would page for a surface
# nobody is serving.
for gated_alert in GlmNoLiveProfiles GlmNoAvailableProfiles GlmCalibrationPersistenceFailed \
  GlmCalibrationBacklog GlmQuotaStale GlmAccountDead; do
  grep -F "alert: $gated_alert" -A 2 "$ROOT/observability/prometheus/rules/application.yml" \
    | grep -Fq 'claude_api_glm_enabled == 1' \
    || { printf '%s is not gated on the provider being enabled\n' "$gated_alert" >&2; exit 1; }
done
# The backend-only Tripo3D plane runs on its own dedicated delivery mode: its aggregate series
# are scraped on the plane's target and deliberately carry no provider label — the
# claude_api_tripo3d_ name prefix is the discriminator, so the scoping pin here is the enabled
# gate, not a label selector.
for tripo3d_metric in \
  'claude_api_tripo3d_live_profiles' \
  'claude_api_tripo3d_available_profiles' \
  'claude_api_tripo3d_calibration_persistence_ok' \
  'claude_api_tripo3d_calibration_pending_events' \
  'claude_api_tripo3d_balance_last_observation_timestamp_seconds' \
  'claude_api_tripo3d_balance_walled_profiles' \
  'claude_api_tripo3d_requests_total' \
  'claude_api_tripo3d_failures_total'; do
  grep -Fq "$tripo3d_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$tripo3d_metric" >&2; exit 1; }
  grep -Fq "$tripo3d_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$tripo3d_metric" >&2; exit 1; }
done
for tripo3d_alert in Tripo3dNoLiveProfiles Tripo3dNoAvailableProfiles Tripo3dErrorShareHigh \
  Tripo3dCalibrationPersistenceFailed Tripo3dCalibrationBacklog Tripo3dBalanceStale \
  Tripo3dBalanceWalled; do
  grep -Fq "alert: $tripo3d_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing Tripo3D alert %s\n' "$tripo3d_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$tripo3d_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$tripo3d_alert" >&2; exit 1; }
  grep -Fqi "## $tripo3d_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$tripo3d_alert" >&2; exit 1; }
done
# Every Tripo3D rule is gated on the default-off plane being enabled, or it would page for a
# surface nobody is serving.
for gated_alert in Tripo3dNoLiveProfiles Tripo3dNoAvailableProfiles Tripo3dErrorShareHigh \
  Tripo3dCalibrationPersistenceFailed Tripo3dCalibrationBacklog Tripo3dBalanceStale \
  Tripo3dBalanceWalled; do
  grep -F "alert: $gated_alert" -A 2 "$ROOT/observability/prometheus/rules/application.yml" \
    | grep -Fq 'claude_api_tripo3d_enabled == 1' \
    || { printf '%s is not gated on the provider being enabled\n' "$gated_alert" >&2; exit 1; }
done
# The backend-only Suno plane runs on its own dedicated delivery mode: its aggregate series
# are scraped on the plane's target and deliberately carry no provider label — the
# claude_api_suno_ name prefix is the discriminator, so the scoping pin here is the enabled
# gate, not a label selector.
for suno_metric in \
  'claude_api_suno_live_profiles' \
  'claude_api_suno_available_profiles' \
  'claude_api_suno_calibration_persistence_ok' \
  'claude_api_suno_calibration_pending_events' \
  'claude_api_suno_quota_last_observation_timestamp_seconds' \
  'claude_api_suno_quota_walled_profiles' \
  'claude_api_suno_requests_total' \
  'claude_api_suno_failures_total'; do
  grep -Fq "$suno_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$suno_metric" >&2; exit 1; }
  grep -Fq "$suno_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$suno_metric" >&2; exit 1; }
done
for suno_alert in SunoNoLiveProfiles SunoNoAvailableProfiles SunoErrorShareHigh \
  SunoCalibrationPersistenceFailed SunoCalibrationBacklog SunoQuotaStale \
  SunoQuotaWalled; do
  grep -Fq "alert: $suno_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing Suno alert %s\n' "$suno_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$suno_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$suno_alert" >&2; exit 1; }
  grep -Fqi "## $suno_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$suno_alert" >&2; exit 1; }
done
# Every Suno rule is gated on the default-off plane being enabled, or it would page for a
# surface nobody is serving.
for gated_alert in SunoNoLiveProfiles SunoNoAvailableProfiles SunoErrorShareHigh \
  SunoCalibrationPersistenceFailed SunoCalibrationBacklog SunoQuotaStale \
  SunoQuotaWalled; do
  grep -F "alert: $gated_alert" -A 2 "$ROOT/observability/prometheus/rules/application.yml" \
    | grep -Fq 'claude_api_suno_enabled == 1' \
    || { printf '%s is not gated on the provider being enabled\n' "$gated_alert" >&2; exit 1; }
done
for anthropic_metric in claude_api_breaker_open claude_api_subs claude_api_cooling \
  claude_api_upstream_429_total claude_api_upstream_auth_total claude_api_upstream_5xx_total \
  claude_api_anthropic_auth_dead_subscriptions; do
  grep -Fq "${anthropic_metric}{provider=\"anthropic\"}" \
    "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'Claude alert is not scoped to Anthropic: %s\n' "$anthropic_metric" >&2; exit 1; }
done
for auth_health_metric in claude_api_anthropic_auth_suspect_subscriptions \
  claude_api_anthropic_auth_dead_subscriptions; do
  grep -Fq "$auth_health_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export Claude auth-health metric %s\n' "$auth_health_metric" >&2; exit 1; }
done
grep -Fq 'increase(claude_api_upstream_auth_total{provider="anthropic"}[10m]) > 10' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'request-path Claude auth rejections are not thresholded separately\n' >&2; exit 1; }
grep -F 'alert: EngineUpstreamRequestAuthRejected' -A 4 \
  "$ROOT/observability/prometheus/rules/application.yml" | grep -Fq 'severity: warning' \
  || { printf 'request-path Claude auth rejections must remain warning severity\n' >&2; exit 1; }
grep -Fq 'expr: claude_api_anthropic_auth_dead_subscriptions{provider="anthropic"} > 0' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'corroborated dead Claude subscriptions do not page\n' >&2; exit 1; }
grep -F 'alert: EngineSubscriptionAuthDead' -A 3 \
  "$ROOT/observability/prometheus/rules/application.yml" | grep -Fq 'severity: critical' \
  || { printf 'corroborated dead Claude subscriptions must remain critical\n' >&2; exit 1; }
for auth_alert in EngineUpstreamRequestAuthRejected EngineSubscriptionAuthDead; do
  anchor=$(printf '%s' "$auth_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "docs/ops/MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$auth_alert" >&2; exit 1; }
  grep -Fqi "## $auth_alert" "$ROOT/docs/ops/MONITORING.md" \
    || { printf 'docs/ops/MONITORING.md has no runbook section for %s\n' "$auth_alert" >&2; exit 1; }
done
! grep -Fq 'alert: EngineUpstreamAuthFailures' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'ambiguous EngineUpstreamAuthFailures rule still exists\n' >&2; exit 1; }
grep -Fq 'claude_api_execution_group_double_winner_total' "$ROOT/crates/server/src/http.rs" \
  || { printf 'engine does not export execution-group winner conflicts\n' >&2; exit 1; }
grep -Fq 'increase(claude_api_execution_group_double_winner_total[5m]) > 0' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'execution-group winner conflicts do not page\n' >&2; exit 1; }
for fallback_metric in \
  claude_api_claudestore_fallback_attempts_total \
  claude_api_claudestore_fallback_successes_total \
  claude_api_claudestore_fallback_failures_total; do
  grep -Fq "$fallback_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export ClaudeStore fallback metric %s\n' "$fallback_metric" >&2; exit 1; }
done
grep -Fq 'increase(claude_api_claudestore_fallback_failures_total{provider=~"anthropic|openai"}[10m]) > 0' \
  "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'ClaudeStore fallback failures do not alert on both eligible provider planes\n' >&2; exit 1; }
grep -Fq 'claude-(api(@.+|-anthropic@.+|-openai(@.+)?|-gemini(@.+)?|-kimi(@.+)?)?|authbot|router)' \
  "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'systemd alerts omit a provider runtime unit\n' >&2; exit 1; }
grep -Fq 'claude-(api(@.+|-anthropic@.+|-openai(@.+)?|-gemini(@.+)?|-kimi(@.+)?)?|authbot|router)' \
  "$ROOT/observability/grafana/dashboards/production-overview.json" \
  || { printf 'Grafana systemd panel omits a provider runtime unit\n' >&2; exit 1; }
[[ $(grep -Fc 'public-http|openai-http|gemini-http|protected-http|support-http|loopback-http|openkeys-http' \
  "$ROOT/observability/grafana/dashboards/production-overview.json") -ge 2 ]] \
  || { printf 'Grafana synthetic panels omit an independent provider probe\n' >&2; exit 1; }

# A missing status file must read as stale, never as a fresh zero.
grep -Fq 'WATCHDOG_STATUS_MISSING_AGE_SECONDS=86400' "$ROOT/deploy/collect-monitoring-metrics.sh" \
  || { printf 'a missing watchdog status must report a stale age\n' >&2; exit 1; }

# The collector unit runs with an empty CapabilityBoundingSet, so root has no CAP_DAC_OVERRIDE and
# cannot read a 0640 file. Deployment state must therefore be world-readable, and the collector must
# treat unreadable state as unknown rather than failing: it also exports the money-bearing queue and
# backup metrics, and must not be taken down by an observational read.
grep -Fq 'CapabilityBoundingSet=' "$ROOT/systemd/apitoken-monitoring-collector.service" \
  || { printf 'collector unit no longer drops capabilities; revisit the readability assumption\n' >&2; exit 1; }
for readable_guard in \
  '-r $WATCHDOG_STATE/rejected.sha' \
  '-r $WATCHDOG_STATE/pending-migration.sha' \
  '-r $status_file'; do
  grep -Fq -e "$readable_guard" "$ROOT/deploy/collect-monitoring-metrics.sh" \
    || { printf 'collector does not guard readability of %s\n' "$readable_guard" >&2; exit 1; }
done
grep -Fq '"$STATUS_FILE" "phase=' "$ROOT/deploy/watchdog.sh" \
  || { printf 'watchdog status write changed shape\n' >&2; exit 1; }
grep -Eq 'wd_atomic_write "\$STATUS_FILE".*0644' "$ROOT/deploy/watchdog.sh" \
  || { printf 'the watchdog status file must be collector-readable (0644)\n' >&2; exit 1; }
grep -Eq 'wd_atomic_write "\$REJECTED_FILE" "\$CANDIDATE_SHA" 0644' "$ROOT/deploy/watchdog.sh" \
  || { printf 'the quarantine marker must be collector-readable (0644)\n' >&2; exit 1; }
for observable in status candidate-validation-1.status candidate-validation-2.status \
  rejected.sha pending-migration.sha; do
  grep -Fq "$observable" "$ROOT/deploy/install-watchdog.sh" \
    || { printf 'the installer does not relax pre-existing %s state\n' "$observable" >&2; exit 1; }
done

grep -Fq 'apitoken_queue_dead{queue="commerce_email"}' "$ROOT/deploy/collect-monitoring-metrics.sh"
grep -Fq "FROM email_outbox WHERE status = 'failed'" "$ROOT/deploy/collect-monitoring-metrics.sh"
grep -Fq 'apitoken_queue_canceled{queue="commerce_email"}' "$ROOT/deploy/collect-monitoring-metrics.sh"
grep -Fq "FROM email_outbox WHERE status::text = 'canceled'" "$ROOT/deploy/collect-monitoring-metrics.sh"
for database in commerce claude_engine sales openkeys apitoken_crm; do
  grep -Fq "$database" "$ROOT/deploy/apitoken-db-dump"
  grep -Fq "$database" "$ROOT/deploy/collect-monitoring-metrics.sh"
  grep -Fq "$database" "$ROOT/deploy/install-monitoring.sh"
done
grep -Fq 'observability/*' "$ROOT/deploy/watchdog-lib.sh"
grep -Fq 'install-monitoring.sh' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'apitoken-monitoring-collector.timer' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'monitoring-authority-drift.awk' "$ROOT/deploy/install-watchdog.sh"
bash "$ROOT/deploy/monitoring-authority-drift.test.sh"

if grep -R -E '(password|secret|token)[[:space:]]*[:=][[:space:]]*[A-Za-z0-9_-]{24,}' \
  "$ROOT/observability" --exclude='alertmanager.yml.template'; then
  printf 'observability configuration appears to contain a committed credential\n' >&2
  exit 1
fi

# Every alert in both rules files must carry a runbook annotation, and every anchor must
# resolve to a real '## ' section in docs/ops/MONITORING.md. The named loops above pin extra
# properties of specific alerts; this generic pass extends the anchor guarantee to every
# alert, including ones nobody pinned yet.
node - "$ROOT" <<'EOF'
const { readFileSync } = require('node:fs');
const root = process.argv[2];
const rulesFiles = [
  `${root}/observability/prometheus/rules/application.yml`,
  `${root}/observability/prometheus/rules/operations.yml`,
];
const alerts = [];
const anchors = [];
for (const file of rulesFiles) {
  const text = readFileSync(file, 'utf8');
  for (const m of text.matchAll(/^\s*-\s*alert:\s*(\S+)\s*$/gm)) alerts.push(m[1]);
  for (const m of text.matchAll(/runbook:\s*['"]docs\/ops\/MONITORING\.md#([^'"]+)['"]/g)) anchors.push(m[1]);
}
const headingAnchors = new Set(
  readFileSync(`${root}/docs/ops/MONITORING.md`, 'utf8')
    .split('\n')
    .filter((l) => l.startsWith('## '))
    .map((l) => l.slice(3).trim().toLowerCase().replace(/[^\p{L}\p{N} _-]/gu, '').replace(/ /g, '-')),
);
const problems = [];
if (alerts.length !== anchors.length) {
  problems.push(`alert count ${alerts.length} != runbook anchor count ${anchors.length}`);
}
for (const anchor of new Set(anchors)) {
  if (!headingAnchors.has(anchor)) problems.push(`runbook anchor #${anchor} has no '## ' section in docs/ops/MONITORING.md`);
}
if (problems.length) {
  for (const p of problems) console.error(`monitoring-config: ${p}`);
  process.exit(1);
}
console.log(`monitoring-config: all ${alerts.length} alerts carry runbook anchors resolving to MONITORING.md sections`);
EOF

printf 'monitoring static configuration tests passed\n'
