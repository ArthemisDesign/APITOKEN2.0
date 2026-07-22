#!/usr/bin/env bash
set -euo pipefail

# Export low-cardinality, aggregate state from the durable queues and all money-bearing databases.
# A failed run deliberately leaves the previous textfile in place so Prometheus alerts on staleness
# instead of silently replacing useful evidence with an empty/partial scrape.

COMPOSE_FILE=${MONITORING_POSTGRES_COMPOSE_FILE:-/usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml}
POSTGRES_ENV=${MONITORING_POSTGRES_ENV:-/etc/apitoken/postgres.env}
OUTPUT_DIR=${MONITORING_TEXTFILE_DIR:-/var/lib/apitoken/monitoring/textfile}
BACKUP_ROOT=${MONITORING_BACKUP_ROOT:-/var/lib/apitoken/backups}

[[ ${EUID:-$(id -u)} -eq 0 ]] || { printf 'monitoring collector must run as root\n' >&2; exit 1; }
[[ -f $COMPOSE_FILE && ! -L $COMPOSE_FILE ]] || { printf 'PostgreSQL compose definition is missing\n' >&2; exit 1; }
[[ -f $POSTGRES_ENV && ! -L $POSTGRES_ENV ]] || { printf 'PostgreSQL environment file is missing\n' >&2; exit 1; }

install -d -o root -g root -m 0755 "$OUTPUT_DIR"
temporary=$(mktemp "$OUTPUT_DIR/.apitoken.prom.XXXXXX")
cleanup() { rm -f -- "$temporary"; }
trap cleanup EXIT
chmod 0644 "$temporary"

psql_database() {
  local database=$1
  docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
    exec -T commerce-postgres psql \
      --username=commerce --dbname="$database" --no-psqlrc --tuples-only --no-align \
      --set ON_ERROR_STOP=1
}

database_exists() {
  local database=$1
  [[ $(printf '%s\n' "SELECT 1 FROM pg_database WHERE datname = '$database';" | psql_database postgres) == 1 ]]
}

cat >"$temporary" <<'METRICS'
# HELP apitoken_queue_ready Durable jobs waiting or eligible for retry.
# TYPE apitoken_queue_ready gauge
# HELP apitoken_queue_dead Durable jobs in a terminal failed state.
# TYPE apitoken_queue_dead gauge
# HELP apitoken_queue_oldest_ready_seconds Age of the oldest ready or retrying durable job.
# TYPE apitoken_queue_oldest_ready_seconds gauge
# HELP apitoken_webhook_failed Payment webhook events in failed state.
# TYPE apitoken_webhook_failed gauge
# HELP apitoken_checkout_stale Incomplete checkout sessions older than one hour.
# TYPE apitoken_checkout_stale gauge
# HELP apitoken_engine_settlement_pending Engine settlement outbox rows not committed.
# TYPE apitoken_engine_settlement_pending gauge
# HELP apitoken_engine_expired_active_leases Expired engine leases still marked active.
# TYPE apitoken_engine_expired_active_leases gauge
# HELP apitoken_sales_pending_referral_events Sales events buffered pending attribution reconciliation.
# TYPE apitoken_sales_pending_referral_events gauge
# HELP apitoken_sales_failed_payout_batches Sales payout batches in failed state.
# TYPE apitoken_sales_failed_payout_batches gauge
METRICS

psql_database commerce >>"$temporary" <<'SQL'
SELECT 'apitoken_queue_ready{queue="engine_credits"} ' || count(*) FROM engine_credits WHERE status IN ('pending','retry');
SELECT 'apitoken_queue_dead{queue="engine_credits"} ' || count(*) FROM engine_credits WHERE status = 'dead';
SELECT 'apitoken_queue_oldest_ready_seconds{queue="engine_credits"} ' || COALESCE(GREATEST(0, EXTRACT(EPOCH FROM now() - min(created_at)))::bigint, 0) FROM engine_credits WHERE status IN ('pending','retry');
SELECT 'apitoken_queue_ready{queue="engine_adjustments"} ' || count(*) FROM engine_adjustments WHERE status IN ('pending','retry');
SELECT 'apitoken_queue_dead{queue="engine_adjustments"} ' || count(*) FROM engine_adjustments WHERE status = 'dead';
SELECT 'apitoken_queue_oldest_ready_seconds{queue="engine_adjustments"} ' || COALESCE(GREATEST(0, EXTRACT(EPOCH FROM now() - min(created_at)))::bigint, 0) FROM engine_adjustments WHERE status IN ('pending','retry');
SELECT 'apitoken_queue_ready{queue="engine_pricing"} ' || count(*) FROM engine_pricing_jobs WHERE status IN ('pending','retry');
SELECT 'apitoken_queue_dead{queue="engine_pricing"} 0';
SELECT 'apitoken_queue_oldest_ready_seconds{queue="engine_pricing"} ' || COALESCE(GREATEST(0, EXTRACT(EPOCH FROM now() - min(created_at)))::bigint, 0) FROM engine_pricing_jobs WHERE status IN ('pending','retry');
SELECT 'apitoken_queue_ready{queue="commerce_email"} ' || count(*) FROM email_outbox WHERE status = 'pending';
SELECT 'apitoken_queue_dead{queue="commerce_email"} ' || count(*) FROM email_outbox WHERE status = 'failed';
SELECT 'apitoken_queue_oldest_ready_seconds{queue="commerce_email"} ' || COALESCE(GREATEST(0, EXTRACT(EPOCH FROM now() - min(created_at)))::bigint, 0) FROM email_outbox WHERE status = 'pending';
SELECT 'apitoken_webhook_failed ' || count(*) FROM webhook_events WHERE status = 'failed';
SELECT 'apitoken_checkout_stale ' || count(*) FROM checkout_sessions WHERE status IN ('creating','pending') AND created_at < now() - interval '1 hour';
SQL

if database_exists claude_engine; then
  psql_database claude_engine >>"$temporary" <<'SQL'
SELECT 'apitoken_engine_settlement_pending ' || count(*) FROM settlement_outbox WHERE state <> 'done';
SELECT 'apitoken_engine_expired_active_leases ' || (
  (SELECT count(*) FROM capacity_leases WHERE state = 'active' AND lease_until < EXTRACT(EPOCH FROM now())::bigint)
  + (SELECT count(*) FROM reservations WHERE state IN ('reserved','delivering','settlement_pending') AND lease_until < EXTRACT(EPOCH FROM now())::bigint)
);
SQL
fi

if database_exists sales; then
  psql_database sales >>"$temporary" <<'SQL'
SELECT 'apitoken_queue_ready{queue="sales_email"} ' || count(*) FROM partner_email_outbox WHERE status = 'pending';
SELECT 'apitoken_queue_dead{queue="sales_email"} ' || count(*) FROM partner_email_outbox WHERE status = 'failed';
SELECT 'apitoken_queue_oldest_ready_seconds{queue="sales_email"} ' || COALESCE(GREATEST(0, EXTRACT(EPOCH FROM now() - min(created_at)))::bigint, 0) FROM partner_email_outbox WHERE status = 'pending';
SELECT 'apitoken_sales_pending_referral_events ' || count(*) FROM pending_referral_events;
SELECT 'apitoken_sales_failed_payout_batches ' || count(*) FROM payout_batches WHERE status = 'failed';
SQL
fi

{
  printf '# HELP apitoken_backup_present Whether a current custom-format database dump exists.\n'
  printf '# TYPE apitoken_backup_present gauge\n'
  printf '# HELP apitoken_backup_age_seconds Age of the current database dump.\n'
  printf '# TYPE apitoken_backup_age_seconds gauge\n'
  printf '# HELP apitoken_backup_size_bytes Size of the current database dump.\n'
  printf '# TYPE apitoken_backup_size_bytes gauge\n'
  now=$(date +%s)
  for database in commerce claude_engine sales apitoken_crm; do
    dump=$BACKUP_ROOT/$database.dump
    if [[ -f $dump && ! -L $dump ]]; then
      modified=$(stat -c %Y -- "$dump")
      size=$(stat -c %s -- "$dump")
      printf 'apitoken_backup_present{database="%s"} 1\n' "$database"
      printf 'apitoken_backup_age_seconds{database="%s"} %s\n' "$database" "$((now - modified))"
      printf 'apitoken_backup_size_bytes{database="%s"} %s\n' "$database" "$size"
    else
      printf 'apitoken_backup_present{database="%s"} 0\n' "$database"
      printf 'apitoken_backup_age_seconds{database="%s"} 0\n' "$database"
      printf 'apitoken_backup_size_bytes{database="%s"} 0\n' "$database"
    fi
  done
  printf '# HELP apitoken_monitoring_collector_last_success_unixtime Unix time of the last complete collector run.\n'
  printf '# TYPE apitoken_monitoring_collector_last_success_unixtime gauge\n'
  printf 'apitoken_monitoring_collector_last_success_unixtime %s\n' "$now"
} >>"$temporary"

mv -f -- "$temporary" "$OUTPUT_DIR/apitoken.prom"
trap - EXIT
