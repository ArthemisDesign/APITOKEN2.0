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

# Anthropic, OpenAI and Gemini have independent stable origins and scrape labels. Without labels,
# provider-local zero gauges can collide and make Codex or Claude alerts evaluate against both.
for provider_target in \
  '127.0.0.1:8790"]|provider: anthropic' \
  '127.0.0.1:8792"]|provider: openai' \
  '127.0.0.1:8794"]|provider: gemini'; do
  target=${provider_target%%|*}
  label=${provider_target#*|}
  grep -F "$target" -A 1 "$ROOT/observability/prometheus/prometheus.yml" \
    | grep -Fq "$label" || { printf 'engine scrape %s lacks %s\n' "$target" "$label" >&2; exit 1; }
done
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
  'MONITORING_POSTGRES_PASSWORD=irrelevant' \
  'ALERT_EMAIL_TO=alerts@example.test' >"$TEMP/monitoring.env"
node "$ROOT/deploy/render-alertmanager.mjs" \
  "$ROOT/observability/alertmanager/alertmanager.yml.template" \
  "$TEMP/worker.env" "$TEMP/monitoring.env" "$TEMP/alertmanager.yml"
grep -Fq 'smtp_smarthost: "smtp.example.test:587"' "$TEMP/alertmanager.yml"
grep -Fq 'to: "alerts@example.test"' "$TEMP/alertmanager.yml"
! grep -Eq '__[A-Z0-9_]+__' "$TEMP/alertmanager.yml"

grep -Fq 'apitoken_backup_present{database="%s"}' "$ROOT/deploy/collect-monitoring-metrics.sh"

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
  grep -Fq "MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$gemini_alert" >&2; exit 1; }
  grep -Fqi "## $gemini_alert" "$ROOT/MONITORING.md" \
    || { printf 'MONITORING.md has no runbook section for %s\n' "$gemini_alert" >&2; exit 1; }
done
for watchdog_alert in DeployQuarantined DeployPipelineStale DeployStuckInPhase DeployMigrationUncommitted; do
  grep -Fq "alert: $watchdog_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing deployment alert %s\n' "$watchdog_alert" >&2; exit 1; }
  # Every alert must carry a runbook anchor that actually exists in MONITORING.md.
  anchor=$(printf '%s' "$watchdog_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$watchdog_alert" >&2; exit 1; }
  grep -Fqi "## $watchdog_alert" "$ROOT/MONITORING.md" \
    || { printf 'MONITORING.md has no runbook section for %s\n' "$watchdog_alert" >&2; exit 1; }
done

# Money conservation must be a closed collector -> alert -> runbook loop. Pin the operands as well
# as the metric name so a constant-zero replacement cannot satisfy this static contract.
reconciliation_metric=apitoken_balance_divergence_nano
for required in \
  "$reconciliation_metric" \
  "kind IN ('topup', 'adjust')" \
  'account.balance_nano::numeric' \
  'account.spent_nano::numeric' \
  'account.reserved_nano::numeric'; do
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
grep -Fq 'MONITORING.md#balancedivergencedetected' "$ROOT/observability/prometheus/rules/application.yml" \
  || { printf 'balance-divergence alert has no runbook anchor\n' >&2; exit 1; }
grep -Fqi '## BalanceDivergenceDetected' "$ROOT/MONITORING.md" \
  || { printf 'MONITORING.md has no balance-divergence runbook\n' >&2; exit 1; }

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
grep -Fq 'MONITORING.md#proxyupstreampairdown' "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'alert ProxyUpstreamPairDown has no runbook anchor\n' >&2; exit 1; }
grep -Fqi '## ProxyUpstreamPairDown' "$ROOT/MONITORING.md" \
  || { printf 'MONITORING.md has no runbook section for ProxyUpstreamPairDown\n' >&2; exit 1; }
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
  'claude_api_codex_home_rate_limit_used_percent'; do
  grep -Fq "$codex_metric" "$ROOT/crates/server/src/http.rs" \
    || { printf 'engine does not export %s\n' "$codex_metric" >&2; exit 1; }
  grep -Fq "$codex_metric" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'no alert rule consumes %s\n' "$codex_metric" >&2; exit 1; }
  grep -Fq "${codex_metric}{provider=\"openai\"}" \
    "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'Codex alert is not scoped to OpenAI: %s\n' "$codex_metric" >&2; exit 1; }
done
for codex_alert in CodexProviderDown CodexNoAvailableHomes CodexHomeUnauthenticated \
  CodexHomeNearRateLimit; do
  grep -Fq "alert: $codex_alert" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'missing Codex alert %s\n' "$codex_alert" >&2; exit 1; }
  anchor=$(printf '%s' "$codex_alert" | tr '[:upper:]' '[:lower:]')
  grep -Fq "MONITORING.md#$anchor" "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'alert %s has no runbook anchor\n' "$codex_alert" >&2; exit 1; }
  grep -Fqi "## $codex_alert" "$ROOT/MONITORING.md" \
    || { printf 'MONITORING.md has no runbook section for %s\n' "$codex_alert" >&2; exit 1; }
done
# Alerting on a disabled provider would page for a surface nobody is serving.
for gated_alert in CodexProviderDown CodexNoAvailableHomes; do
  grep -F "alert: $gated_alert" -A 2 "$ROOT/observability/prometheus/rules/application.yml" \
    | grep -Fq 'claude_api_codex_enabled{provider="openai"} == 1' \
    || { printf '%s is not gated on the provider being enabled\n' "$gated_alert" >&2; exit 1; }
done
for anthropic_metric in claude_api_breaker_open claude_api_subs claude_api_cooling \
  claude_api_upstream_429_total claude_api_upstream_auth_total claude_api_upstream_5xx_total \
  claude_api_affinity_redis_errors_total; do
  grep -Fq "${anthropic_metric}{provider=\"anthropic\"}" \
    "$ROOT/observability/prometheus/rules/application.yml" \
    || { printf 'Claude alert is not scoped to Anthropic: %s\n' "$anthropic_metric" >&2; exit 1; }
done
grep -Fq 'claude-(api(@.+|-anthropic@.+|-openai|-gemini)?|authbot)' \
  "$ROOT/observability/prometheus/rules/operations.yml" \
  || { printf 'systemd alerts omit a provider runtime unit\n' >&2; exit 1; }
grep -Fq 'claude-(api(@.+|-anthropic@.+|-openai|-gemini)?|authbot)' \
  "$ROOT/observability/grafana/dashboards/production-overview.json" \
  || { printf 'Grafana systemd panel omits a provider runtime unit\n' >&2; exit 1; }
[[ $(grep -Fc 'public-http|openai-http|gemini-http|protected-http|support-http|loopback-http' \
  "$ROOT/observability/grafana/dashboards/production-overview.json") -eq 2 ]] \
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
