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

# Every operand in the watchdog's PromQL `and` expression must have the same empty label set.
# Aggregate collector freshness just like target and synthetic health, or vector matching yields
# no result even while every underlying series is healthy.
grep -Fq 'and min(time() - apitoken_monitoring_collector_last_success_unixtime) < 180' \
  "$ROOT/deploy/watchdog.sh"
! grep -Fq 'and (time() - apitoken_monitoring_collector_last_success_unixtime < 180)' \
  "$ROOT/deploy/watchdog.sh"

for dashboard in "$ROOT"/observability/grafana/dashboards/*.json; do
  node -e 'JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8"))' "$dashboard"
done

for pinned_image in \
  'quay.io/prometheus/prometheus:v3.12.0@sha256:69f5241418838263316593f7274a304b095c40bcf22e57272865da91bd60a8ac' \
  'quay.io/prometheus/alertmanager:v0.32.1@sha256:51a825c2a40acc3e338fdd00d622e01ec090f72be2b3ea46be0839cd47a4d286' \
  'grafana/grafana:13.1.0@sha256:121a7a9ece6dc10b969f1f96eed64b4f07dfac0d0b8abc070f7cb83bbde86f63' \
  'grafana/loki:3.7.2@sha256:191d4fdfb7264f16989f0a57f320872620a5a7c2ceeec6229212c4190ec49b86' \
  'grafana/alloy:v1.16.1@sha256:51aeb9d829239345070619dad3edd6873186f913c84f45b365b74574fcb38ec0' \
  'quay.io/prometheus/node-exporter:v1.11.1@sha256:0f422f62c15f154af8d8572b23d623aebfb10cec73a5c654d18f911f3f9df241' \
  'quay.io/prometheuscommunity/postgres-exporter:v0.19.1@sha256:e96064f876226d94bb6ce48a4c4b3dd76edba91168ec1ab024e5c4b959310b0f' \
  'quay.io/prometheus/blackbox-exporter:v0.28.0@sha256:e753ff9f3fc458d02cca5eddab5a77e1c175eee484a8925ac7d524f04366c2fc'; do
  grep -Fq "image: $pinned_image" "$ROOT/observability/compose.yaml"
done

[[ $(grep -Fc 'network_mode: host' "$ROOT/observability/compose.yaml") == 8 ]]
for listener in 127.0.0.1:9090 127.0.0.1:9093 127.0.0.1:12345 127.0.0.1:9100 \
  127.0.0.1:9187 127.0.0.1:9115; do
  grep -Fq "$listener" "$ROOT/observability/compose.yaml" "$ROOT/observability/loki/loki.yml"
done
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

printf '%s\n' \
  'EMAIL_FROM=no-reply@apitoken.sale' \
  'SMTP_HOST=smtp.example.test' \
  'SMTP_PORT=587' \
  'SMTP_USERNAME=monitoring-user' \
  'SMTP_PASSWORD=monitoring-password' >"$TEMP/worker.env"
printf '%s\n' \
  'GRAFANA_ADMIN_PASSWORD=irrelevant' \
  'MONITORING_POSTGRES_PASSWORD=irrelevant' \
  'ALERT_EMAIL_TO=alerts@example.test' >"$TEMP/monitoring.env"
node "$ROOT/deploy/render-alertmanager.mjs" \
  "$ROOT/observability/alertmanager/alertmanager.yml.template" \
  "$TEMP/worker.env" "$TEMP/monitoring.env" "$TEMP/alertmanager.yml"
grep -Fq 'smtp_smarthost: "smtp.example.test:587"' "$TEMP/alertmanager.yml"
grep -Fq 'to: "alerts@example.test"' "$TEMP/alertmanager.yml"
! grep -Eq '__[A-Z0-9_]+__' "$TEMP/alertmanager.yml"

grep -Fq 'apitoken_backup_present{database="%s"}' "$ROOT/deploy/collect-monitoring-metrics.sh"
grep -Fq 'apitoken_queue_dead{queue="commerce_email"}' "$ROOT/deploy/collect-monitoring-metrics.sh"
grep -Fq "FROM email_outbox WHERE status = 'failed'" "$ROOT/deploy/collect-monitoring-metrics.sh"
grep -Fq 'apitoken_queue_canceled{queue="commerce_email"}' "$ROOT/deploy/collect-monitoring-metrics.sh"
grep -Fq "FROM email_outbox WHERE status = 'canceled'" "$ROOT/deploy/collect-monitoring-metrics.sh"
for database in commerce claude_engine sales apitoken_crm; do
  grep -Fq "$database" "$ROOT/deploy/apitoken-db-dump"
done
grep -Fq 'observability/*' "$ROOT/deploy/watchdog-lib.sh"
grep -Fq 'install-monitoring.sh' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'apitoken-monitoring-collector.timer' "$ROOT/deploy/install-watchdog.sh"

if grep -R -E '(password|secret|token)[[:space:]]*[:=][[:space:]]*[A-Za-z0-9_-]{24,}' \
  "$ROOT/observability" --exclude='alertmanager.yml.template'; then
  printf 'observability configuration appears to contain a committed credential\n' >&2
  exit 1
fi

printf 'monitoring static configuration tests passed\n'
