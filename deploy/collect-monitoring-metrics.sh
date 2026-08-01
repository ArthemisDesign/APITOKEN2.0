#!/usr/bin/env bash
set -euo pipefail

# Export low-cardinality, aggregate state from the durable queues and all money-bearing databases.
# A failed run deliberately leaves the previous textfile in place so Prometheus alerts on staleness
# instead of silently replacing useful evidence with an empty/partial scrape.

COMPOSE_FILE=${MONITORING_POSTGRES_COMPOSE_FILE:-/usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml}
POSTGRES_ENV=${MONITORING_POSTGRES_ENV:-/etc/apitoken/postgres.env}
OUTPUT_DIR=${MONITORING_TEXTFILE_DIR:-/var/lib/apitoken/monitoring/textfile}
BACKUP_ROOT=${MONITORING_BACKUP_ROOT:-/var/lib/apitoken/backups}
WATCHDOG_STATE=${MONITORING_WATCHDOG_STATE:-/var/lib/apitoken/watchdog}
# Deliberately larger than any alert threshold: a missing status file must read as "stale", never
# as a fresh zero.
WATCHDOG_STATUS_MISSING_AGE_SECONDS=86400

[[ ${EUID:-$(id -u)} -eq 0 ]] || { printf 'monitoring collector must run as root\n' >&2; exit 1; }
[[ -f $COMPOSE_FILE && ! -L $COMPOSE_FILE ]] || { printf 'PostgreSQL compose definition is missing\n' >&2; exit 1; }
[[ -f $POSTGRES_ENV && ! -L $POSTGRES_ENV ]] || { printf 'PostgreSQL environment file is missing\n' >&2; exit 1; }

# Must stay group-deploy writable, matching install-monitoring.sh: apitoken-devbot.service
# (User=deploy) publishes its devbot.prom heartbeat next to this collector's output, and this
# line runs every minute — re-rooting the directory breaks the heartbeat write with EACCES.
install -d -o root -g deploy -m 0775 "$OUTPUT_DIR"
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
# HELP apitoken_queue_canceled Durable jobs intentionally canceled because their work became obsolete.
# TYPE apitoken_queue_canceled gauge
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
# HELP apitoken_balance_divergence_nano Maximum absolute per-account divergence between durable funding and balance plus charged or held nanodollars.
# TYPE apitoken_balance_divergence_nano gauge
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
-- Infrastructure installs before application migrations. Cast the enum to text so this collector
-- remains valid both before and after the canceled status is introduced.
SELECT 'apitoken_queue_canceled{queue="commerce_email"} ' || count(*) FROM email_outbox WHERE status::text = 'canceled';
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
-- Every commerce-originated credit or debit is recorded as an idempotent topup/adjust ledger row.
-- Account aggregates must conserve that funding across completed charges and in-flight holds.
WITH funding AS (
  SELECT account_id, COALESCE(SUM(amount_nano), 0)::numeric AS funded_nano
  FROM ledger
  WHERE kind IN ('topup', 'adjust')
  GROUP BY account_id
), divergence AS (
  SELECT ABS(
    account.balance_nano::numeric
    + account.spent_nano::numeric
    + account.reserved_nano::numeric
    - COALESCE(funding.funded_nano, 0)
  ) AS amount_nano
  FROM accounts account
  LEFT JOIN funding ON funding.account_id = account.id
)
SELECT 'apitoken_balance_divergence_nano ' || COALESCE(MAX(amount_nano), 0)
FROM divergence;
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

# Deployment pipeline state. The watchdog reports per-commit results to GitHub, but a quarantined
# candidate or a stalled poll loop is otherwise invisible from the host. Export it so a failed
# delivery pages the operator instead of waiting to be noticed in a browser.
{
  printf '# HELP apitoken_watchdog_quarantined Whether a candidate commit failed and is blocked from retry.\n'
  printf '# TYPE apitoken_watchdog_quarantined gauge\n'
  printf '# HELP apitoken_watchdog_status_age_seconds Age of the watchdog status file.\n'
  printf '# TYPE apitoken_watchdog_status_age_seconds gauge\n'
  printf '# HELP apitoken_watchdog_phase Current watchdog phase, as a label on a constant series.\n'
  printf '# TYPE apitoken_watchdog_phase gauge\n'
  printf '# HELP apitoken_watchdog_pending_migration Whether a migration was started but not committed.\n'
  printf '# TYPE apitoken_watchdog_pending_migration gauge\n'

  # Deployment state is observational: it must never fail the collector, which also exports the
  # money-bearing queue and backup metrics. The unit runs with an empty CapabilityBoundingSet, so
  # root has no CAP_DAC_OVERRIDE and an unexpectedly restrictive mode reads as unreadable rather
  # than as an error. Treat anything unreadable as unknown, exactly like a missing file.
  if [[ -f $WATCHDOG_STATE/rejected.sha && ! -L $WATCHDOG_STATE/rejected.sha && -r $WATCHDOG_STATE/rejected.sha ]]; then
    printf 'apitoken_watchdog_quarantined 1\n'
  else
    printf 'apitoken_watchdog_quarantined 0\n'
  fi
  if [[ -f $WATCHDOG_STATE/pending-migration.sha && ! -L $WATCHDOG_STATE/pending-migration.sha && -r $WATCHDOG_STATE/pending-migration.sha ]]; then
    printf 'apitoken_watchdog_pending_migration 1\n'
  else
    printf 'apitoken_watchdog_pending_migration 0\n'
  fi

  status_file=$WATCHDOG_STATE/status
  if [[ -f $status_file && ! -L $status_file && -r $status_file ]]; then
    status_modified=$(stat -c %Y -- "$status_file" 2>/dev/null || printf '0')
    if [[ $status_modified =~ ^[0-9]+$ ]] && (( status_modified > 0 )); then
      printf 'apitoken_watchdog_status_age_seconds %s\n' "$((now - status_modified))"
    else
      printf 'apitoken_watchdog_status_age_seconds %s\n' "$WATCHDOG_STATUS_MISSING_AGE_SECONDS"
    fi
    # phase=<value> is the first field written by the watchdog's atomic status write.
    phase=$(sed -n 's/^phase=\([A-Za-z-]\{1,32\}\).*/\1/p' "$status_file" 2>/dev/null | head -n 1)
    printf 'apitoken_watchdog_phase{phase="%s"} 1\n' "${phase:-unknown}"
  else
    # No readable status file means the watchdog has never completed a cycle here, or the file is
    # not visible to this sandbox. Report a large age rather than omitting the series, so the
    # staleness alert fires instead of going blind.
    printf 'apitoken_watchdog_status_age_seconds %s\n' "$WATCHDOG_STATUS_MISSING_AGE_SECONDS"
    printf 'apitoken_watchdog_phase{phase="unknown"} 1\n'
  fi
} >>"$temporary"

mv -f -- "$temporary" "$OUTPUT_DIR/apitoken.prom"
trap - EXIT
