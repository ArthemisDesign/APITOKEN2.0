#!/usr/bin/env bash
set -euo pipefail

# Root-only, idempotent installer for the host-local monitoring stack. All listeners bind to
# loopback. Caddy is the only ingress and exposes only Grafana behind managed admin auth.

[[ ${EUID:-$(id -u)} -eq 0 ]] || { printf 'run as root\n' >&2; exit 1; }
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
SOURCE=$ROOT/observability
LIVE=/etc/apitoken/monitoring
ENV_FILE=/etc/apitoken/monitoring.env
WORKER_ENV=/etc/apitoken/worker.env
ENGINE_ENV=/srv/claude-api/data/server.env
POSTGRES_ENV=/etc/apitoken/postgres.env
POSTGRES_COMPOSE=/usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml
TIMESTAMP=$(date -u +%Y%m%dT%H%M%SZ)
STAGE=/etc/apitoken/.monitoring.stage.$$
BACKUP=
ACTIVATED=0
COMMITTED=0

log() { printf '[monitoring] %s\n' "$*"; }
die() { printf '[monitoring] ERROR: %s\n' "$*" >&2; exit 1; }

rollback() {
  local failed
  if (( COMMITTED == 0 && ACTIVATED == 1 )); then
    failed="/etc/apitoken/monitoring.failed.$TIMESTAMP"
    log "activation failed; restoring the previous monitoring configuration"
    mv -T -- "$LIVE" "$failed" 2>/dev/null || true
    if [[ -n $BACKUP && -d $BACKUP ]]; then
      mv -T -- "$BACKUP" "$LIVE"
      docker compose --env-file "$ENV_FILE" -f "$LIVE/compose.yaml" up -d --force-recreate >/dev/null 2>&1 || true
    else
      docker compose --env-file "$ENV_FILE" -f "$failed/compose.yaml" down >/dev/null 2>&1 || true
    fi
  fi
  [[ ! -d $STAGE ]] || rm -rf --one-file-system -- "$STAGE"
}
trap rollback EXIT

for command in docker curl jq node openssl systemctl; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done
[[ -d $SOURCE && ! -L $SOURCE ]] || die "observability source tree is missing"
for fixed_file in "$WORKER_ENV" "$ENGINE_ENV" "$POSTGRES_ENV" "$POSTGRES_COMPOSE"; do
  [[ -f $fixed_file && ! -L $fixed_file ]] || die "required fixed file is missing: $fixed_file"
done

install -d -o root -g root -m 0755 /etc/apitoken /var/lib/apitoken/monitoring/textfile
if [[ ! -e $ENV_FILE ]]; then
  umask 077
  {
    printf 'GRAFANA_ADMIN_PASSWORD=%s\n' "$(openssl rand -hex 32)"
    printf 'MONITORING_POSTGRES_PASSWORD=%s\n' "$(openssl rand -hex 32)"
    printf 'ALERT_EMAIL_TO=apitokensale@gmail.com\n'
  } >"$ENV_FILE"
fi
[[ -f $ENV_FILE && ! -L $ENV_FILE ]] || die "$ENV_FILE must be a regular file"
chown root:root "$ENV_FILE"
chmod 0600 "$ENV_FILE"
for required_key in GRAFANA_ADMIN_PASSWORD MONITORING_POSTGRES_PASSWORD ALERT_EMAIL_TO; do
  [[ $(awk -F= -v key="$required_key" '$1 == key { count++ } END { print count + 0 }' "$ENV_FILE") == 1 ]] \
    || die "$ENV_FILE must contain exactly one $required_key assignment"
done

engine_key_count=$(awk -F= '$1 == "CLAUDE_API_PANEL_KEY" { count++ } END { print count + 0 }' "$ENGINE_ENV")
(( engine_key_count <= 1 )) || die "$ENGINE_ENV contains duplicate CLAUDE_API_PANEL_KEY assignments"
if (( engine_key_count == 0 )); then
  engine_key=$(openssl rand -hex 32)
  engine_tmp=$(mktemp "$(dirname -- "$ENGINE_ENV")/.server-env.XXXXXX")
  cp -- "$ENGINE_ENV" "$engine_tmp"
  printf '\n# Read-only Prometheus scrape credential managed by install-monitoring.sh.\nCLAUDE_API_PANEL_KEY=%s\n' "$engine_key" >>"$engine_tmp"
  chown --reference="$ENGINE_ENV" "$engine_tmp"
  chmod --reference="$ENGINE_ENV" "$engine_tmp"
  mv -f -- "$engine_tmp" "$ENGINE_ENV"
else
  engine_key=$(awk -F= '$1 == "CLAUDE_API_PANEL_KEY" { print substr($0, index($0, "=") + 1) }' "$ENGINE_ENV")
  engine_key=${engine_key%\"}; engine_key=${engine_key#\"}
  engine_key=${engine_key%\'}; engine_key=${engine_key#\'}
fi
[[ $engine_key =~ ^[A-Za-z0-9._~-]{32,256}$ ]] || die "CLAUDE_API_PANEL_KEY has an unsafe format"

monitoring_postgres_password=$(awk -F= '$1 == "MONITORING_POSTGRES_PASSWORD" { print substr($0, index($0, "=") + 1) }' "$ENV_FILE")
[[ $monitoring_postgres_password =~ ^[a-f0-9]{64}$ ]] || die "MONITORING_POSTGRES_PASSWORD must be 64 lowercase hex characters"
escaped_postgres_password=${monitoring_postgres_password//\'/\'\'}
monitoring_role_sql=$({
  printf '%s\n' "DO \$\$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'apitoken_monitoring') THEN CREATE ROLE apitoken_monitoring LOGIN; END IF; END \$\$;"
  printf "ALTER ROLE apitoken_monitoring WITH LOGIN PASSWORD '%s';\n" "$escaped_postgres_password"
  printf 'GRANT pg_monitor TO apitoken_monitoring;\n'
  printf 'GRANT CONNECT ON DATABASE commerce TO apitoken_monitoring;\n'
  for database in claude_engine sales apitoken_crm; do
    if docker compose --env-file "$POSTGRES_ENV" -f "$POSTGRES_COMPOSE" exec -T commerce-postgres \
      psql -U commerce -d postgres -Atqc "SELECT 1 FROM pg_database WHERE datname='$database'" | grep -qx 1; then
      printf 'GRANT CONNECT ON DATABASE %s TO apitoken_monitoring;\n' "$database"
    fi
  done
})
[[ $monitoring_role_sql == 'DO $$ BEGIN '* ]] || die 'generated PostgreSQL role bootstrap has invalid dollar quoting'
[[ $monitoring_role_sql != *'\$\$'* ]] || die 'generated PostgreSQL role bootstrap contains escaped dollar quoting'
printf '%s\n' "$monitoring_role_sql" | docker compose --env-file "$POSTGRES_ENV" -f "$POSTGRES_COMPOSE" exec -T commerce-postgres \
  psql -U commerce -d postgres --no-psqlrc --set ON_ERROR_STOP=1 >/dev/null

install -d -o root -g root -m 0755 "$STAGE"
cp -a -- "$SOURCE/." "$STAGE/"
install -d -o root -g root -m 0700 "$STAGE/secrets" "$STAGE/rendered"
printf '%s\n' "$engine_key" >"$STAGE/secrets/engine_metrics_token"
chmod 0600 "$STAGE/secrets/engine_metrics_token"
node "$ROOT/deploy/render-alertmanager.mjs" \
  "$STAGE/alertmanager/alertmanager.yml.template" "$WORKER_ENV" "$ENV_FILE" \
  "$STAGE/rendered/alertmanager.yml"
find "$STAGE" -type d -exec chmod 0755 {} +
find "$STAGE" -type f ! -path "$STAGE/secrets/*" ! -path "$STAGE/rendered/*" -exec chmod 0644 {} +
chmod 0700 "$STAGE/secrets" "$STAGE/rendered"
chmod 0600 "$STAGE/secrets/engine_metrics_token" "$STAGE/rendered/alertmanager.yml"
chown -R root:root "$STAGE"

log 'pulling pinned monitoring images'
docker compose --env-file "$ENV_FILE" -f "$STAGE/compose.yaml" pull --quiet
docker compose --env-file "$ENV_FILE" -f "$STAGE/compose.yaml" config --quiet

log 'validating Prometheus, Alertmanager, Loki, Alloy, Blackbox, and Grafana provisioning'
docker run --rm --user 0:0 --entrypoint /bin/promtool \
  -v "$STAGE/prometheus:/etc/prometheus:ro" \
  -v "$STAGE/secrets/engine_metrics_token:/run/secrets/engine_metrics_token:ro" \
  quay.io/prometheus/prometheus:v3.12.0@sha256:69f5241418838263316593f7274a304b095c40bcf22e57272865da91bd60a8ac \
  check config /etc/prometheus/prometheus.yml >/dev/null
docker run --rm --user 0:0 --entrypoint /bin/amtool -v "$STAGE/rendered/alertmanager.yml:/etc/alertmanager/alertmanager.yml:ro" \
  quay.io/prometheus/alertmanager:v0.32.1@sha256:51a825c2a40acc3e338fdd00d622e01ec090f72be2b3ea46be0839cd47a4d286 \
  check-config /etc/alertmanager/alertmanager.yml >/dev/null
docker run --rm -v "$STAGE/loki/loki.yml:/etc/loki/loki.yml:ro" \
  grafana/loki:3.7.2@sha256:191d4fdfb7264f16989f0a57f320872620a5a7c2ceeec6229212c4190ec49b86 \
  -verify-config=true -config.file=/etc/loki/loki.yml >/dev/null
docker run --rm -v "$STAGE/alloy/config.alloy:/etc/alloy/config.alloy:ro" \
  grafana/alloy:v1.16.1@sha256:51aeb9d829239345070619dad3edd6873186f913c84f45b365b74574fcb38ec0 \
  validate /etc/alloy/config.alloy >/dev/null
docker run --rm -v "$STAGE/blackbox/blackbox.yml:/etc/blackbox/blackbox.yml:ro" \
  quay.io/prometheus/blackbox-exporter:v0.28.0@sha256:e753ff9f3fc458d02cca5eddab5a77e1c175eee484a8925ac7d524f04366c2fc \
  --config.file=/etc/blackbox/blackbox.yml --config.check >/dev/null
while IFS= read -r -d '' dashboard; do jq --exit-status . "$dashboard" >/dev/null; done \
  < <(find "$STAGE/grafana/dashboards" -type f -name '*.json' -print0)

if [[ -d $LIVE ]]; then
  BACKUP="$LIVE.pre-$TIMESTAMP"
  mv -T -- "$LIVE" "$BACKUP"
fi
mv -T -- "$STAGE" "$LIVE"
ACTIVATED=1

log 'starting the monitoring stack'
docker compose --env-file "$ENV_FILE" -f "$LIVE/compose.yaml" up -d --force-recreate --remove-orphans

wait_http() {
  local name=$1 url=$2 attempt
  for attempt in {1..60}; do
    if curl --noproxy '*' --fail --silent --show-error --max-time 3 "$url" >/dev/null 2>&1; then
      log "$name is ready"
      return 0
    fi
    sleep 1
  done
  die "$name did not become ready at $url"
}
wait_http Prometheus http://127.0.0.1:9090/-/ready
wait_http Alertmanager http://127.0.0.1:9093/-/ready
wait_http Grafana http://127.0.0.1:3600/api/health
wait_http Loki http://127.0.0.1:3101/ready
wait_http Alloy http://127.0.0.1:12345/-/ready
wait_http Node-Exporter http://127.0.0.1:9100/metrics
wait_http Postgres-Exporter http://127.0.0.1:9187/metrics
wait_http Blackbox-Exporter http://127.0.0.1:9115/metrics

systemctl enable --now apitoken-monitoring-collector.timer
systemctl start apitoken-monitoring-collector.service

wait_prometheus_result() {
  local name=$1 query=$2 attempt response
  for attempt in {1..30}; do
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode "query=$query" http://127.0.0.1:9090/api/v1/query 2>/dev/null || true)
    if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
      >/dev/null 2>&1 <<<"$response"; then
      log "$name is present in Prometheus"
      return 0
    fi
    sleep 2
  done
  die "$name did not appear in Prometheus"
}
wait_prometheus_result business-collector 'time() - apitoken_monitoring_collector_last_success_unixtime < 180'
wait_prometheus_result systemd-collector 'node_scrape_collector_success{collector="systemd"} == 1'
wait_prometheus_result PostgreSQL 'pg_up == 1'
wait_prometheus_result monitoring-targets \
  'sum(up{job=~"prometheus|node|postgres|caddy|alertmanager|grafana|loki|alloy|blackbox-exporter"} == 1) == 9'

COMMITTED=1
trap - EXIT
log "monitoring stack installed successfully${BACKUP:+; rollback copy: $BACKUP}"
