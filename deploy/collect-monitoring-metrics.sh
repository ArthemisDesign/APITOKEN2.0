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
AUTHORITY_DRIFT_AWK=${MONITORING_AUTHORITY_DRIFT_AWK:-/usr/local/lib/apitoken-watchdog/monitoring-authority-drift.awk}
# Deliberately larger than any alert threshold: a missing status file must read as "stale", never
# as a fresh zero.
WATCHDOG_STATUS_MISSING_AGE_SECONDS=86400

[[ ${EUID:-$(id -u)} -eq 0 ]] || { printf 'monitoring collector must run as root\n' >&2; exit 1; }
[[ -f $COMPOSE_FILE && ! -L $COMPOSE_FILE ]] || { printf 'PostgreSQL compose definition is missing\n' >&2; exit 1; }
[[ -f $POSTGRES_ENV && ! -L $POSTGRES_ENV ]] || { printf 'PostgreSQL environment file is missing\n' >&2; exit 1; }
[[ -f $AUTHORITY_DRIFT_AWK && ! -L $AUTHORITY_DRIFT_AWK ]] \
  || { printf 'pricing authority reconciliation program is missing\n' >&2; exit 1; }

# Must stay group-deploy writable, matching install-monitoring.sh: apitoken-devbot.service
# (User=deploy) publishes its devbot.prom heartbeat next to this collector's output, and this
# line runs every minute — re-rooting the directory breaks the heartbeat write with EACCES.
install -d -o root -g deploy -m 0775 "$OUTPUT_DIR"
temporary=$(mktemp "$OUTPUT_DIR/.apitoken.prom.XXXXXX")
authority_commerce_accounts=$(mktemp "$OUTPUT_DIR/.authority-commerce-accounts.XXXXXX")
authority_engine_accounts=$(mktemp "$OUTPUT_DIR/.authority-engine-accounts.XXXXXX")
authority_commerce_overrides=$(mktemp "$OUTPUT_DIR/.authority-commerce-overrides.XXXXXX")
authority_engine_overrides=$(mktemp "$OUTPUT_DIR/.authority-engine-overrides.XXXXXX")
cleanup() {
  rm -f -- "$temporary" "$authority_commerce_accounts" "$authority_engine_accounts" \
    "$authority_commerce_overrides" "$authority_engine_overrides"
}
trap cleanup EXIT
chmod 0644 "$temporary"

psql_database() {
  local database=$1
  docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
    exec -T commerce-postgres psql \
      --username=commerce --dbname="$database" --no-psqlrc --tuples-only --no-align \
      --field-separator=$'\t' --set ON_ERROR_STOP=1
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
# HELP apitoken_balance_divergence_nano Maximum absolute per-account divergence between durable funding and balance plus full billed usage and holds, net of explicit pool-funded shortfall.
# TYPE apitoken_balance_divergence_nano gauge
# HELP apitoken_sales_pending_referral_events Sales events buffered pending attribution reconciliation.
# TYPE apitoken_sales_pending_referral_events gauge
# HELP apitoken_sales_failed_payout_batches Sales payout batches in failed state.
# TYPE apitoken_sales_failed_payout_batches gauge
# HELP apitoken_sales_failed_payout_rows Sales payout rows in a definitive failed state awaiting retry or release.
# TYPE apitoken_sales_failed_payout_rows gauge
# HELP apitoken_sales_stale_broadcast_payouts Sales payout transactions unresolved for more than ten minutes after broadcast reservation.
# TYPE apitoken_sales_stale_broadcast_payouts gauge
# HELP apitoken_pricing_mirror_drift Customers whose commerce default multiplier disagrees with the engine mirror.
# TYPE apitoken_pricing_mirror_drift gauge
# HELP apitoken_pricing_job_stale_confirmed Pricing jobs marked confirmed while carrying a value commerce no longer wants.
# TYPE apitoken_pricing_job_stale_confirmed gauge
# HELP apitoken_sales_feed_head Highest usage-event sequence commerce has published to the partner feed.
# TYPE apitoken_sales_feed_head gauge
# HELP apitoken_sales_attribution_feed_head Highest referral-attribution sequence commerce has published to the partner feed.
# TYPE apitoken_sales_attribution_feed_head gauge
# HELP apitoken_sales_topups_feed_head Highest payment sequence commerce has published to the partner feed.
# TYPE apitoken_sales_topups_feed_head gauge
# HELP apitoken_sales_reversal_feed_head Highest terminal payment-reversal audit sequence commerce has published to Sales.
# TYPE apitoken_sales_reversal_feed_head gauge
# HELP apitoken_sales_cursor Partner-portal sync cursor per feed; a gap to its feed head that stops closing means partner data is stale.
# TYPE apitoken_sales_cursor gauge
# HELP apitoken_sales_cursor_age_seconds Time since the partner-portal sync cursor last advanced.
# TYPE apitoken_sales_cursor_age_seconds gauge
# HELP apitoken_sales_sync_errors_recent Partner sync iterations that failed or terminated unexpectedly in the last five minutes.
# TYPE apitoken_sales_sync_errors_recent gauge
# HELP apitoken_sales_sync_journal_up Whether the bounded partner sync journal query completed.
# TYPE apitoken_sales_sync_journal_up gauge
# HELP apitoken_sales_accounting_incomplete Durable partner accounting facts that are incomplete, by fixed invariant.
# TYPE apitoken_sales_accounting_incomplete gauge
# HELP apitoken_sales_partner_debt_nano Partner commission committed for payout beyond signed net earnings.
# TYPE apitoken_sales_partner_debt_nano gauge
# HELP apitoken_engine_accounts_below_floor Accounts whose balance is below the $1 overdraft floor.
# TYPE apitoken_engine_accounts_below_floor gauge
# HELP apitoken_pricing_charge_mismatch Settled full billed amounts whose value does not match the reserve-time provider multiplier.
# TYPE apitoken_pricing_charge_mismatch gauge
# HELP apitoken_settlement_uncollected_nano Full billed usage the shared account floor prevented customer balances from collecting, by bounded observation window.
# TYPE apitoken_settlement_uncollected_nano gauge
# HELP apitoken_pricing_output_overage_absorbed_nano Provider cost of output generated past the ceiling a customer asked for, absorbed by the pool in the last hour.
# TYPE apitoken_pricing_output_overage_absorbed_nano gauge
# HELP apitoken_pricing_effective_multiplier_bp Basis points of full billed usage against official price, by bounded provider.
# TYPE apitoken_pricing_effective_multiplier_bp gauge
# HELP apitoken_pricing_authority_drift Mapped commerce accounts whose engine authority disagrees on a fixed pricing dimension.
# TYPE apitoken_pricing_authority_drift gauge
# HELP apitoken_business_reconciliation_up Whether a required cross-database reconciliation completed against both authorities.
# TYPE apitoken_business_reconciliation_up gauge
# HELP apitoken_usage_provider_unresolved Usage rows without exact provider evidence, by bounded observation window.
# TYPE apitoken_usage_provider_unresolved gauge
# HELP apitoken_openkeys_pricing_drift OpenKeys batch or key rows that violate the stored pricing contract.
# TYPE apitoken_openkeys_pricing_drift gauge
# HELP apitoken_openkeys_legacy_rows Historical OpenKeys batch and key rows still carrying the legacy pricing contract.
# TYPE apitoken_openkeys_legacy_rows gauge
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
-- The retired pricing design's queues (policy, catalog and switch control lanes) and its account
-- binding gauges are gone from here. Nothing drains those lanes any more, so their counts
-- described a concept that no longer prices anything — the pending gauge sat at 2 for a state that
-- cannot advance, which is worse than no gauge at all. engine_pricing_jobs above is the one live
-- delivery queue.
--
-- What replaces them are the drift detectors. Both 2026-08 incidents were silent because every
-- component was individually healthy while two of them disagreed, so what needs watching is the
-- disagreement itself.
SELECT 'apitoken_pricing_mirror_drift ' || count(*)
FROM customer_profiles cp JOIN engine_accounts ea ON ea.user_id = cp.user_id
WHERE ea.engine_account_id IS NOT NULL AND ea.mult_bp IS DISTINCT FROM cp.multiplier_bp;
-- A job marked confirmed whose value is not what commerce currently wants means the queue believes
-- a price was delivered that nobody asked for. Zero is the only healthy value.
SELECT 'apitoken_pricing_job_stale_confirmed ' || count(*)
FROM engine_pricing_jobs j
LEFT JOIN customer_profiles cp ON cp.user_id = j.user_id
LEFT JOIN customer_provider_discounts d ON d.user_id = j.user_id AND d.provider_id = j.provider_id
WHERE j.status = 'confirmed'
  AND j.multiplier_bp IS DISTINCT FROM
      CASE WHEN j.provider_id IS NULL THEN cp.multiplier_bp ELSE d.multiplier_bp END;
-- The head of the sales usage feed. Paired with apitoken_sales_cursor below: a growing gap is the
-- partner sync falling behind or refusing pages, which is exactly how five hours of commission
-- went unaccrued on 2026-08-10 without a single alert.
SELECT 'apitoken_sales_feed_head ' || COALESCE(max(feed_seq), 0) FROM pricing_usage_events;
SELECT 'apitoken_sales_attribution_feed_head ' || COALESCE(max(id), 0) FROM referral_attributions;
SELECT 'apitoken_sales_topups_feed_head ' || COALESCE(max(feed_seq), 0) FROM payments;
SELECT 'apitoken_sales_reversal_feed_head ' || COALESCE(max(id), 0)
FROM audit_log WHERE action = 'payment.reversed';
SELECT 'apitoken_usage_provider_unresolved{window="all"} ' || count(*)
FROM pricing_usage_events
WHERE provider_id IS NULL OR provider_id IN ('unattributed', 'unavailable');
SELECT 'apitoken_usage_provider_unresolved{window="1h"} ' || count(*)
FROM pricing_usage_events
WHERE created_at >= now() - interval '1 hour'
  AND (provider_id IS NULL OR provider_id IN ('unattributed', 'unavailable'));
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
-- Admission and settlement now share the same atomic −$1 account fence. A row below it therefore
-- means a later negative adjustment/clawback created recorded account debt; settlement overshoot is
-- represented separately by the immutable uncollected metrics below.
SELECT 'apitoken_engine_accounts_below_floor ' || count(*) FROM accounts WHERE balance_nano < -1000000000;
SELECT 'apitoken_settlement_uncollected_nano{window="all"} '
       || COALESCE(SUM(uncollected_nano)::bigint, 0)
FROM accounts;
SELECT 'apitoken_settlement_uncollected_nano{window="1h"} '
       || COALESCE(SUM(uncollected_nano)::bigint, 0)
FROM ledger
WHERE kind = 'charge' AND ts > EXTRACT(EPOCH FROM now())::bigint - 3600;
-- What a customer was actually charged, checked against the multiplier the same settled row
-- declares. This replaces the shadow evaluation lane, which computed the comparison ahead of a
-- rollout, wrote 2654 rows that nothing ever read, and was not running during the cutover it
-- existed to protect. This reads settled money instead of a dry run, so it cannot be skipped.
-- One basis point of tolerance absorbs integer rounding on sub-cent charges.
WITH providers(provider) AS (
  VALUES ('anthropic'), ('openai'), ('google'), ('kimi'), ('glm'), ('tripo3d'), ('suno')
), settled AS (
  SELECT provider,
         amount_nano::numeric / official_nano AS charged_ratio,
         payable_multiplier_bp
  FROM ledger
  WHERE ts > EXTRACT(EPOCH FROM now())::bigint - 3600
    AND official_nano > 0 AND amount_nano > 0
    AND payable_multiplier_bp IS NOT NULL
    AND provider IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d', 'suno')
)
SELECT 'apitoken_pricing_charge_mismatch{provider="' || providers.provider || '"} '
       || count(*) FILTER (WHERE ABS(charged_ratio * 10000 - payable_multiplier_bp) > 1)
FROM providers
LEFT JOIN settled USING (provider)
GROUP BY providers.provider;
WITH settled AS (
  SELECT provider,
         amount_nano::numeric / official_nano AS charged_ratio
  FROM ledger
  WHERE ts > EXTRACT(EPOCH FROM now())::bigint - 3600
    AND official_nano > 0 AND amount_nano > 0
    AND provider IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d', 'suno')
)
SELECT 'apitoken_pricing_effective_multiplier_bp{provider="' || provider || '"} '
       || round(avg(charged_ratio) * 10000)
FROM settled GROUP BY provider;
-- A customer that sends `max_tokens` is billed only up to that ceiling, exactly as the emulated
-- API would bill it, but this transport cannot stop generation: the provider may overshoot and the
-- pool eats the difference. That absorption used to be invisible, and the only thing that surfaced
-- it was PricingChargeMismatch firing on rows where both numbers were individually correct. Measure
-- it directly instead, as the gap between what the provider produced and what was billable.
SELECT 'apitoken_pricing_output_overage_absorbed_nano ' || COALESCE(SUM(real_nano - charge_basis)::bigint, 0)
FROM (
  SELECT u.real_nano, LEAST(l.official_nano, u.real_nano) AS charge_basis
  FROM usage_events u JOIN ledger l ON l.request_id = u.request_id AND l.kind = 'charge'
  WHERE u.ts > EXTRACT(EPOCH FROM now())::bigint - 3600 AND u.real_nano > 0
) absorbed;
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
    - account.uncollected_nano::numeric
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
SELECT 'apitoken_sales_pending_referral_events ' || (
  (SELECT count(*) FROM pending_referral_events)
  + (SELECT count(*) FROM pending_referral_usage_events_v2)
);
SELECT 'apitoken_sales_failed_payout_batches ' || count(*) FROM payout_batches WHERE status = 'failed';
SELECT 'apitoken_sales_failed_payout_rows ' || count(*) FROM payouts WHERE status = 'requested' AND chain_status = 'failed';
SELECT 'apitoken_sales_stale_broadcast_payouts ' || count(*)
FROM payouts p
LEFT JOIN payout_batches b ON b.id = p.batch_id
WHERE p.status = 'requested'
  AND p.chain_status = 'broadcast'
  AND COALESCE(b.sent_at, p.requested_at) < now() - interval '10 minutes';
-- Payout preparation uses these exact predicates under the partner-accounting lock. Monitoring
-- evaluates the same durable proof continuously so a replay/completeness regression pages before
-- the next payout window. Fixed invariant labels keep customer/payment identities out of metrics.
WITH all_usage AS (
  SELECT 1::int AS source_schema, usage.id, usage.amount_nano AS basis_nano
  FROM partner_usage_events usage
  UNION ALL
  SELECT 2::int AS source_schema, usage.id, usage.paid_funded_nano AS basis_nano
  FROM partner_usage_events_v2 usage
), accounting_health AS (
  SELECT
    (SELECT count(*)
     FROM all_usage usage
     WHERE COALESCE((
       SELECT sum(allocation.allocated_paid_nano)
       FROM partner_usage_funding_allocations allocation
       WHERE (usage.source_schema = 1 AND allocation.usage_event_id = usage.id)
          OR (usage.source_schema = 2 AND allocation.usage_event_v2_id = usage.id)
     ), 0) <> usage.basis_nano) AS usage_funding,
    (SELECT count(*) FROM (
       SELECT 1
       FROM partner_usage_funding_allocations usage_allocation
       JOIN commission_entries entry ON entry.usage_event_id = usage_allocation.usage_event_id
       WHERE NOT EXISTS (
         SELECT 1 FROM partner_commission_funding_allocations commission_allocation
         WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
           AND commission_allocation.commission_entry_id = entry.id
       )
       UNION ALL
       SELECT 1
       FROM partner_usage_funding_allocations usage_allocation
       JOIN commission_entries_v2 entry ON entry.usage_event_id = usage_allocation.usage_event_v2_id
       WHERE NOT EXISTS (
         SELECT 1 FROM partner_commission_funding_allocations commission_allocation
         WHERE commission_allocation.usage_funding_allocation_id = usage_allocation.id
           AND commission_allocation.commission_entry_v2_id = entry.id
       )
     ) missing) AS commission_funding,
    (SELECT count(*)
     FROM partner_payment_reversals reversal
     WHERE EXISTS (
       SELECT 1
       FROM partner_commission_funding_allocations commission_allocation
       JOIN partner_usage_funding_allocations usage_allocation
         ON usage_allocation.id = commission_allocation.usage_funding_allocation_id
       WHERE usage_allocation.funding_lot_id = reversal.funding_lot_id
         AND commission_allocation.allocated_commission_nano > 0
         AND NOT EXISTS (
           SELECT 1 FROM partner_commission_adjustments adjustment
           WHERE adjustment.reversal_id = reversal.id
             AND adjustment.commission_funding_allocation_id = commission_allocation.id
             AND adjustment.amount_nano = -commission_allocation.allocated_commission_nano
         )
     )) AS reversal_adjustments,
    (SELECT count(*)
     FROM payout_batches
     WHERE status IN ('preparing', 'prepared', 'sending') AND earned_before IS NULL)
      AS payout_boundary
)
SELECT 'apitoken_sales_accounting_incomplete{invariant="usage_funding"} ' || usage_funding
FROM accounting_health
UNION ALL
SELECT 'apitoken_sales_accounting_incomplete{invariant="commission_funding"} ' || commission_funding
FROM accounting_health
UNION ALL
SELECT 'apitoken_sales_accounting_incomplete{invariant="reversal_adjustments"} ' || reversal_adjustments
FROM accounting_health
UNION ALL
SELECT 'apitoken_sales_accounting_incomplete{invariant="payout_boundary"} ' || payout_boundary
FROM accounting_health;
WITH gross AS (
  SELECT partner_id, sum(amount_nano)::bigint AS amount_nano
  FROM (
    SELECT partner_id, amount_nano FROM commission_entries
    UNION ALL
    SELECT partner_id, amount_nano FROM commission_entries_v2
  ) commission
  GROUP BY partner_id
), adjustments AS (
  SELECT partner_id, sum(amount_nano)::bigint AS amount_nano
  FROM partner_commission_adjustments
  GROUP BY partner_id
), committed AS (
  SELECT partner_id, sum(amount_nano)::bigint AS amount_nano
  FROM payouts WHERE status IN ('requested', 'approved', 'paid')
  GROUP BY partner_id
)
SELECT 'apitoken_sales_partner_debt_nano ' || COALESCE(sum(GREATEST(
  COALESCE(committed.amount_nano, 0) - COALESCE(gross.amount_nano, 0)
    - COALESCE(adjustments.amount_nano, 0), 0
)), 0)
FROM partners partner
LEFT JOIN gross ON gross.partner_id = partner.id
LEFT JOIN adjustments ON adjustments.partner_id = partner.id
LEFT JOIN committed ON committed.partner_id = partner.id;
-- Where the partner sync actually stands. Compared against apitoken_sales_feed_head from commerce,
-- a gap that stops closing is the whole signal: on 2026-08-10 this cursor stood still for five
-- hours while every service was up and healthy, and no commission accrued.
WITH feeds(feed) AS (VALUES
  ('attributions'), ('usage_events'), ('topups_v2'),
  ('topup_funding_lots'), ('payment_reversals')
)
SELECT 'apitoken_sales_cursor{feed="' || feeds.feed || '"} ' || COALESCE(cursor.last_id, 0)
FROM feeds LEFT JOIN sync_cursors cursor USING (feed);
WITH feeds(feed) AS (VALUES
  ('attributions'), ('usage_events'), ('topups_v2'),
  ('topup_funding_lots'), ('payment_reversals')
)
SELECT 'apitoken_sales_cursor_age_seconds{feed="' || feeds.feed || '"} '
       || CASE WHEN cursor.updated_at IS NULL THEN 86400
               ELSE GREATEST(0, EXTRACT(EPOCH FROM now() - cursor.updated_at))::bigint END
FROM feeds LEFT JOIN sync_cursors cursor USING (feed);
SQL
fi

# Cursor gaps detect a stalled feed once backlog exists. This bounded journal signal catches the
# first rejected page/network/DB iteration without waiting for a gap to age. Journal access is
# observational and failure-local: losing it must not suppress the SQL money metrics.
sales_sync_journal_up=0
sales_sync_errors_recent=0
sales_sync_journal=$(journalctl -q -u apitoken-sales-api.service --since '5 minutes ago' -n 1000 --no-pager -o cat 2>/dev/null) && sales_sync_journal_up=1 || true
if (( sales_sync_journal_up == 1 )); then
  sales_sync_errors_recent=$(printf '%s\n' "$sales_sync_journal" \
    | awk '/sync iteration failed|sync loop terminated unexpectedly/ { count += 1 } END { print count + 0 }')
fi
printf 'apitoken_sales_sync_errors_recent %s\n' "$sales_sync_errors_recent" >>"$temporary"
printf 'apitoken_sales_sync_journal_up %s\n' "$sales_sync_journal_up" >>"$temporary"

# Compare the actual engine authority to commerce's durable mapped intent without exporting a
# customer/account/provider identifier. PostgreSQL cannot join databases directly and enabling
# dblink would create a second production dependency, so the collector takes four individual
# statement snapshots and performs one bounded in-memory join. A ten-minute alert delay absorbs the
# short interval in which a worker may legitimately reconcile state between those snapshots. Any
# query failure aborts the collector and leaves the previous textfile in place; a missing engine
# database exports an explicit reconciliation-down series instead of a healthy-looking zero drift.
if database_exists claude_engine; then
  psql_database commerce >"$authority_commerce_accounts" <<'SQL'
SELECT engine_account_id, mult_bp, status::text
FROM engine_accounts
WHERE engine_account_id IS NOT NULL
ORDER BY engine_account_id;
SQL
  psql_database claude_engine >"$authority_engine_accounts" <<'SQL'
SELECT id, mult_bp, status FROM accounts ORDER BY id;
SQL
  psql_database commerce >"$authority_commerce_overrides" <<'SQL'
SELECT account.engine_account_id, discount.provider_id, discount.multiplier_bp
FROM customer_provider_discounts discount
JOIN engine_accounts account ON account.user_id = discount.user_id
WHERE account.engine_account_id IS NOT NULL
ORDER BY account.engine_account_id, discount.provider_id;
SQL
  psql_database claude_engine >"$authority_engine_overrides" <<'SQL'
SELECT account_id, provider_id, mult_bp
FROM account_provider_discounts
ORDER BY account_id, provider_id;
SQL

  awk \
    -v engine_accounts="$authority_engine_accounts" \
    -v commerce_accounts="$authority_commerce_accounts" \
    -v commerce_overrides="$authority_commerce_overrides" \
    -v engine_overrides="$authority_engine_overrides" \
    -f "$AUTHORITY_DRIFT_AWK" \
    "$authority_engine_accounts" "$authority_commerce_accounts" \
    "$authority_commerce_overrides" "$authority_engine_overrides" >>"$temporary"
else
  printf '%s\n' \
    'apitoken_pricing_authority_drift{dimension="default"} 0' \
    'apitoken_pricing_authority_drift{dimension="provider"} 0' \
    'apitoken_pricing_authority_drift{dimension="status"} 0' \
    'apitoken_business_reconciliation_up{scope="pricing_authority"} 0' >>"$temporary"
fi

# OpenKeys keeps historical legacy inventory intentionally, so its baseline is not an error. The
# integrity gauge catches malformed rows, while the monotonic legacy-row baseline lets Prometheus
# alert only when a new issuance regresses to the old contract.
if database_exists openkeys; then
  psql_database openkeys >>"$temporary" <<'SQL'
WITH drift AS (
  SELECT batch.id
  FROM openkeys_batches batch
  WHERE batch.pricing_contract IS NULL
     OR batch.pricing_contract NOT IN ('legacy', 'official_1_to_1')
     OR batch.mult_bp IS NULL
     OR batch.mult_bp NOT BETWEEN 1 AND 10000
     OR (batch.pricing_contract = 'official_1_to_1' AND batch.mult_bp <> 10000)
  UNION ALL
  SELECT key_row.id
  FROM openkeys_keys key_row
  JOIN openkeys_batches batch ON batch.id = key_row.batch_id
  WHERE key_row.pricing_contract IS NULL
     OR key_row.pricing_contract NOT IN ('legacy', 'official_1_to_1')
     OR key_row.mult_bp IS NULL
     OR key_row.mult_bp NOT BETWEEN 1 AND 10000
     OR (key_row.pricing_contract = 'official_1_to_1' AND key_row.mult_bp <> 10000)
     OR key_row.pricing_contract IS DISTINCT FROM batch.pricing_contract
     OR key_row.mult_bp IS DISTINCT FROM batch.mult_bp
     OR key_row.face_value_nano IS DISTINCT FROM batch.face_value_nano
)
SELECT 'apitoken_openkeys_pricing_drift ' || count(*) FROM drift;
SELECT 'apitoken_openkeys_legacy_rows ' || (
  (SELECT count(*) FROM openkeys_batches WHERE pricing_contract = 'legacy')
  + (SELECT count(*) FROM openkeys_keys WHERE pricing_contract = 'legacy')
);
SELECT 'apitoken_business_reconciliation_up{scope="openkeys"} 1';
SQL
else
  printf '%s\n' \
    'apitoken_openkeys_pricing_drift 0' \
    'apitoken_openkeys_legacy_rows 0' \
    'apitoken_business_reconciliation_up{scope="openkeys"} 0' >>"$temporary"
fi

{
  printf '# HELP apitoken_backup_present Whether a current custom-format database dump exists.\n'
  printf '# TYPE apitoken_backup_present gauge\n'
  printf '# HELP apitoken_backup_age_seconds Age of the current database dump.\n'
  printf '# TYPE apitoken_backup_age_seconds gauge\n'
  printf '# HELP apitoken_backup_size_bytes Size of the current database dump.\n'
  printf '# TYPE apitoken_backup_size_bytes gauge\n'
  now=$(date +%s)
  for database in commerce claude_engine sales openkeys apitoken_crm; do
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
cleanup
trap - EXIT
