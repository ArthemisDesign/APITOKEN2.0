#!/usr/bin/env bash
set -euo pipefail

# Root-owned, read-only proof that one pricing-schema contraction left the intended production
# authorities intact. The watchdog invokes this only when the matching immutable migration path is
# newly added between processed.sha and the exact candidate. A failure is forward-only: it
# quarantines the candidate but never attempts to recreate schema or roll application binaries back.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$SCRIPT_DIR/watchdog-lib.sh"

MANIFEST=$SCRIPT_DIR/pricing-retired-schema-manifest.sh
AUTHORITY_DRIFT_AWK=$SCRIPT_DIR/monitoring-authority-drift.awk
STATE_ROOT=/var/lib/apitoken/watchdog
CANDIDATE_ROOT=$STATE_ROOT/candidates
PROCESSED_FILE=$STATE_ROOT/processed.sha
BACKUP_ROOT=/var/lib/apitoken/backups
COMPOSE_FILE=/usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml
POSTGRES_ENV=/etc/apitoken/postgres.env
BORG_CONFIG=/etc/borgmatic/config.yaml
COMMERCE_CONTRACTION_REL=packages/db/migrations/0048_retire_pricing_schema.sql
ENGINE_CONTRACTION_REL=crates/registry/migrations_pg/0049_retire_pricing_schema.sql
COMMERCE_CONTRACTION_HASH=
BACKUP_MAX_AGE_SECONDS=3600
PRICING_WATERMARK_LAG_SECONDS=120
PRICING_CURSOR_MAX_AGE_SECONDS=180
SALES_WATERMARK_LAG_SECONDS=120
PGOPTIONS_RO='-c default_transaction_read_only=on -c statement_timeout=120000 -c lock_timeout=5000 -c timezone=UTC -c datestyle=ISO'

KNOWN_BELOW_FLOOR_ACCOUNT=acct_d83edbc93d17247d216ce019
KNOWN_BELOW_FLOOR_BALANCE=-2522590000
KNOWN_BELOW_FLOOR_REF=bonus-revoke:18a95747-89e3-42fc-a81a-1d2e2dac549b
KNOWN_BELOW_FLOOR_ADJUSTMENT=-4000000000

TARGETED_ALERTS=(
  DurableQueueBacklog
  DurableQueueOldestItemStale
  DurableQueueDeadItems
  SalesSyncCursorStalled
  PricingMirrorDrift
  BusinessReconciliationUnavailable
  PricingAuthorityDrift
  PricingJobStaleConfirmed
  EngineAccountsBelowFloor
  SettlementUncollectedDetected
  SettlementUncollectedHigh
  UsageProviderAttributionMissing
  OpenKeysPricingDrift
  PositiveBalancePaymentRequired
  PricingChargeMismatch
  FailedWebhooksPresent
  EngineSettlementBacklog
  EngineExpiredLeasePresent
  BalanceDivergenceDetected
  BackupStale
  BackupMissing
  SalesReferralReconciliationBacklog
  SalesPayoutBatchFailed
)

[[ -f $MANIFEST && ! -L $MANIFEST ]] \
  || wd_die "pricing-retirement post-drop manifest is missing or unsafe: $MANIFEST"
# shellcheck source=deploy/pricing-retired-schema-manifest.sh
source "$MANIFEST"

CANDIDATE=
TESTED_AT=
TESTED_AT_EPOCH=
VERIFICATION_STARTED_EPOCH=
TEMP=

prp_die() {
  printf 'pricing retirement post-drop verification failed: %s\n' "$*" >&2
  exit 1
}

prp_cleanup() {
  [[ -n ${TEMP:-} && -d $TEMP ]] || return 0
  rm -f -- "$TEMP"/*
  rmdir -- "$TEMP"
}

prp_require_root() {
  [[ ${EUID:-$(id -u)} -eq 0 ]] \
    || prp_die 'post-drop verification must run as root through the fixed watchdog helper'
}

prp_require_fixed_file() {
  local path=$1 mode
  [[ -f $path && ! -L $path ]] || prp_die "required fixed file is missing or unsafe: $path"
  [[ $(stat -c %u -- "$path") == 0 ]] || prp_die "required fixed file is not root-owned: $path"
  mode=$(stat -c %a -- "$path")
  (( (8#$mode & 8#022) == 0 )) || prp_die "required fixed file is group/world-writable: $path"
}

prp_validate_host_contract() {
  local command fixed
  for command in awk curl date docker git journalctl jq node stat systemctl; do
    command -v "$command" >/dev/null 2>&1 || prp_die "required command is unavailable: $command"
  done
  for fixed in "$SCRIPT_DIR/watchdog-lib.sh" "$MANIFEST" "$AUTHORITY_DRIFT_AWK" \
    "$COMPOSE_FILE" "$POSTGRES_ENV" "$BORG_CONFIG"; do
    prp_require_fixed_file "$fixed"
  done
}

prp_validate_candidate() {
  local stage=$1 sha=$2 marker marker_sha marker_tree marker_digest candidate_sha candidate_tree
  local actual_digest candidate_mode processed detected completed_at

  CANDIDATE=$CANDIDATE_ROOT/$sha
  marker=$STATE_ROOT/$sha.tested
  [[ -d $CANDIDATE && ! -L $CANDIDATE ]] \
    || prp_die "tested post-drop candidate is missing or unsafe: $CANDIDATE"
  [[ $(stat -c %u -- "$CANDIDATE") == 0 ]] \
    || prp_die 'tested post-drop candidate must be root-owned'
  candidate_mode=$(stat -c %a -- "$CANDIDATE")
  (( (8#$candidate_mode & 8#222) == 0 )) \
    || prp_die 'tested post-drop candidate must be immutable'
  [[ -f $marker && ! -L $marker ]] \
    || prp_die 'tested post-drop candidate marker is missing or unsafe'

  marker_sha=$(wd_marker_value "$marker" sha) \
    || prp_die 'tested post-drop marker has no SHA'
  marker_tree=$(wd_marker_value "$marker" tree) \
    || prp_die 'tested post-drop marker has no tree'
  marker_digest=$(wd_marker_value "$marker" migration_digest) \
    || prp_die 'tested post-drop marker has no migration digest'
  completed_at=$(wd_marker_value "$marker" completed_at) \
    || prp_die 'tested post-drop marker has no completion time'
  [[ $marker_sha == "$sha" && $marker_tree =~ ^[0-9a-f]{40}$ \
      && $marker_digest =~ ^[0-9a-f]{64}$ ]] \
    || prp_die 'tested post-drop marker identity is malformed or belongs to another SHA'
  [[ $completed_at =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]] \
    || prp_die 'tested post-drop marker completion time is not canonical UTC'
  TESTED_AT_EPOCH=$(date -u -d "$completed_at" +%s 2>/dev/null) \
    || prp_die 'tested post-drop marker completion time cannot be parsed'
  [[ $TESTED_AT_EPOCH =~ ^[0-9]+$ && $TESTED_AT_EPOCH -le $(date -u +%s) ]] \
    || prp_die 'tested post-drop marker completion time is in the future'
  TESTED_AT=$completed_at

  candidate_sha=$(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" rev-parse 'HEAD^{commit}')
  candidate_tree=$(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" rev-parse 'HEAD^{tree}')
  [[ $candidate_sha == "$sha" && $candidate_tree == "$marker_tree" ]] \
    || prp_die 'tested post-drop candidate identity changed after validation'
  [[ -z $(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" \
    status --porcelain --untracked-files=no) ]] \
    || prp_die 'tested post-drop candidate has tracked modifications'

  wd_migration_manifest "$CANDIDATE" >"$TEMP/candidate-migrations.manifest"
  actual_digest=$(wd_manifest_digest "$TEMP/candidate-migrations.manifest")
  [[ $actual_digest == "$marker_digest" ]] \
    || prp_die 'tested post-drop candidate migrations changed after tests'

  processed=$(wd_read_sha "$PROCESSED_FILE") \
    || prp_die 'processed production SHA is unavailable'
  git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" \
    merge-base --is-ancestor "$processed" "$sha" \
    || prp_die 'processed production SHA is not an ancestor of the post-drop candidate'
  detected=$(wd_pricing_retirement_postdrop_stage "$CANDIDATE" "$processed" "$sha") \
    || prp_die 'candidate adds both contraction paths, an invalid path, or an unreadable range'
  [[ $detected == "$stage" ]] \
    || prp_die "requested post-drop stage $stage does not match exact candidate range $detected"

  [[ -f $CANDIDATE/$COMMERCE_CONTRACTION_REL \
      && ! -L $CANDIDATE/$COMMERCE_CONTRACTION_REL ]] \
    || prp_die 'exact commerce contraction artifact is absent or unsafe'
  if [[ $stage == commerce ]]; then
    [[ ! -e $CANDIDATE/$ENGINE_CONTRACTION_REL && ! -L $CANDIDATE/$ENGINE_CONTRACTION_REL ]] \
      || prp_die 'engine contraction artifact arrived in the commerce-only delivery'
  else
    [[ -f $CANDIDATE/$ENGINE_CONTRACTION_REL \
        && ! -L $CANDIDATE/$ENGINE_CONTRACTION_REL ]] \
      || prp_die 'exact engine contraction artifact is absent or unsafe'
  fi
}

prp_psql_ro() {
  local database=$1
  shift
  docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
    exec -T -e "PGOPTIONS=$PGOPTIONS_RO" commerce-postgres \
    psql -X -qAt -F $'\t' -v ON_ERROR_STOP=1 -U commerce -d "$database" "$@"
}

prp_verify_object_inventory() {
  local object_type=$1 database=$2 label=$3 absent_name=$4 present_name=$5
  local values='' separator='' object expected result_file diagnostic expression
  local -n absent_objects=$absent_name
  local -n present_objects=$present_name

  for object in "${absent_objects[@]}"; do
    [[ $object =~ ^[a-z0-9_(),[:space:]]+$ ]] \
      || prp_die "$label contains an unsafe absent $object_type identity"
    values+="${separator}('absent','$object')"
    separator=,
  done
  for object in "${present_objects[@]}"; do
    [[ $object =~ ^[a-z0-9_(),[:space:]]+$ ]] \
      || prp_die "$label contains an unsafe present $object_type identity"
    values+="${separator}('present','$object')"
    separator=,
  done
  expected=$((${#absent_objects[@]} + ${#present_objects[@]}))
  (( expected > 0 )) || prp_die "$label has an empty $object_type survival contract"

  case $object_type in
    table)
      expression="EXISTS (
        SELECT 1 FROM pg_class relation
        JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'public' AND relation.relname = object_identity
          AND relation.relkind IN ('r','p'))"
      ;;
    function)
      expression="EXISTS (
        SELECT 1 FROM pg_proc function_row
        WHERE function_row.oid = to_regprocedure('public.' || object_identity)
          AND function_row.prokind = 'f')"
      ;;
    *) prp_die "unknown post-drop object type: $object_type" ;;
  esac
  result_file=$TEMP/objects-$label-$object_type
  prp_psql_ro "$database" -c "
    /* pricing-retirement-postdrop:objects:$label:$object_type */
    WITH expected(disposition, object_identity) AS (VALUES $values)
    SELECT disposition, object_identity,
           CASE WHEN $expression THEN 1 ELSE 0 END
    FROM expected ORDER BY disposition, object_identity;" >"$result_file"

  diagnostic=$(awk -F '\t' -v expected="$expected" '
    NF != 3 || $1 !~ /^(absent|present)$/ || $3 !~ /^(0|1)$/ {
      print "malformed:" NR; next
    }
    $1 == "absent" && $3 != 0 { print "still-present:" $2 }
    $1 == "present" && $3 != 1 { print "missing-live:" $2 }
    END { if (NR != expected) print "row-count:" NR "/" expected }
  ' "$result_file")
  [[ -z $diagnostic ]] || prp_die "$label $object_type inventory mismatch: $diagnostic"
  printf 'postdrop-objects:%s:%s absent=%s present=%s\n' \
    "$label" "$object_type" "${#absent_objects[@]}" "${#present_objects[@]}"
}

prp_verify_commerce_journal() {
  local migration=$CANDIDATE/$COMMERCE_CONTRACTION_REL journal state matching latest_matching
  local database_latest_hash latest_id total latest_created extra manifest_file manifest_entry
  local latest_tag latest_migration expected_latest_hash expected_entries expected_when
  COMMERCE_CONTRACTION_HASH=$(wd_sha256_file "$migration")
  [[ $COMMERCE_CONTRACTION_HASH =~ ^[0-9a-f]{64}$ ]] \
    || prp_die 'commerce contraction artifact hash is malformed'
  manifest_file="file=$COMMERCE_CONTRACTION_HASH $COMMERCE_CONTRACTION_REL"
  manifest_entry=$(awk '
    $1 == "entry=00000048" && $3 == "0048_retire_pricing_schema" && NF == 3 { print }
  ' "$TEMP/candidate-migrations.manifest")
  [[ $(grep -Fxc -- "$manifest_file" "$TEMP/candidate-migrations.manifest" || true) == 1 \
      && $(printf '%s\n' "$manifest_entry" | awk 'NF { count++ } END { print count + 0 }') == 1 ]] \
    || prp_die 'candidate manifest does not bind the contraction to canonical commerce entry 0048'

  journal=$CANDIDATE/packages/db/migrations/meta/_journal.json
  [[ -f $journal && ! -L $journal ]] || prp_die 'candidate Drizzle journal is absent or unsafe'
  jq --exit-status '
    (.entries | type == "array" and length >= 49) and
    .entries[48].idx == 48 and .entries[48].tag == "0048_retire_pricing_schema" and
    (.entries[-1].idx == (.entries | length) - 1) and
    (.entries[-1].tag | type == "string" and test("^[A-Za-z0-9._-]+$")) and
    (.entries[-1].when | type == "number" and floor == . and . > 0)
  ' "$journal" >/dev/null || prp_die 'candidate Drizzle journal does not retain canonical entry 0048'
  latest_tag=$(jq -r '.entries[-1].tag' "$journal")
  expected_entries=$(jq -r '.entries | length' "$journal")
  expected_when=$(jq -r '.entries[-1].when' "$journal")
  latest_migration=$CANDIDATE/packages/db/migrations/$latest_tag.sql
  [[ -f $latest_migration && ! -L $latest_migration ]] \
    || prp_die 'candidate latest Drizzle migration artifact is absent or unsafe'
  expected_latest_hash=$(wd_sha256_file "$latest_migration")
  [[ $expected_latest_hash =~ ^[0-9a-f]{64}$ \
      && $(grep -Fxc -- "file=$expected_latest_hash packages/db/migrations/$latest_tag.sql" \
        "$TEMP/candidate-migrations.manifest" || true) == 1 ]] \
    || prp_die 'candidate manifest does not bind its latest Drizzle migration artifact'
  state=$(prp_psql_ro commerce -c "
    /* pricing-retirement-postdrop:commerce-journal */
    SELECT count(*) FILTER (WHERE hash = '$COMMERCE_CONTRACTION_HASH'),
           count(*) FILTER (WHERE hash = '$expected_latest_hash'),
           COALESCE((array_agg(hash ORDER BY id DESC))[1], ''),
           COALESCE(max(id), 0), count(*),
           COALESCE((array_agg(created_at ORDER BY id DESC))[1], 0)
    FROM drizzle.__drizzle_migrations;")
  IFS=$'\t' read -r matching latest_matching database_latest_hash latest_id total \
    latest_created extra <<<"$state"
  [[ $matching == 1 && $latest_matching == 1 \
      && $database_latest_hash == "$expected_latest_hash" \
      && $latest_id == "$expected_entries" && $total == "$expected_entries" \
      && $latest_created == "$expected_when" && -z ${extra:-} ]] \
    || prp_die 'commerce Drizzle journal lacks exact 0048 or does not end at the exact candidate migration'
  printf 'postdrop-journal:commerce contraction_sha256=%s latest_id=%s latest_tag=%s latest_sha256=%s created_at=%s\n' \
    "$COMMERCE_CONTRACTION_HASH" "$latest_id" "$latest_tag" "$expected_latest_hash" \
    "$latest_created"
}

prp_candidate_engine_schema_version() {
  local source=$CANDIDATE/crates/registry/src/pg.rs versions expected
  [[ -f $source && ! -L $source ]] || prp_die 'candidate engine migration registry is absent or unsafe'
  versions=$(sed -n 's/^pub const CURRENT_SCHEMA_VERSION: i64 = \([0-9][0-9]*\);$/\1/p' "$source")
  [[ $(printf '%s\n' "$versions" | awk 'NF { count++ } END { print count + 0 }') == 1 ]] \
    || prp_die 'candidate engine migration registry has no unique current schema version'
  expected=$versions
  [[ $expected =~ ^[0-9]+$ && $expected -ge 49 ]] \
    || prp_die 'candidate engine schema version is older than contraction 0049'
  grep -Fq 'include_str!("../migrations_pg/0049_retire_pricing_schema.sql")' "$source" \
    || prp_die 'candidate engine registry does not embed contraction 0049'
  grep -Fq '(49, MIGRATION_0049),' "$source" \
    || prp_die 'candidate engine registry does not register contraction 0049'
  printf '%s\n' "$expected"
}

prp_verify_engine_schema_version() {
  local expected=$1 state applied_49 max_version total distinct_versions missing extra
  state=$(prp_psql_ro claude_engine -c "
    /* pricing-retirement-postdrop:engine-schema:$expected */
    SELECT count(*) FILTER (WHERE version = 49), COALESCE(max(version), 0), count(*),
           count(DISTINCT version),
           (SELECT count(*) FROM generate_series(1, $expected) expected(version)
             WHERE NOT EXISTS (
               SELECT 1 FROM public.engine_schema_migrations actual
               WHERE actual.version = expected.version))
    FROM public.engine_schema_migrations;")
  IFS=$'\t' read -r applied_49 max_version total distinct_versions missing extra <<<"$state"
  [[ $applied_49 =~ ^[0-9]+$ && $max_version == "$expected" \
      && $total == "$expected" && $distinct_versions == "$expected" && $missing == 0 \
      && -z ${extra:-} ]] \
    || prp_die "engine migration journal is not a contiguous exact 1..$expected sequence"
  if [[ $expected == 48 ]]; then
    [[ $applied_49 == 0 ]] || prp_die 'engine contraction 0049 ran during commerce stage'
  else
    [[ $applied_49 == 1 ]] || prp_die 'engine contraction 0049 is not recorded exactly once'
  fi
  printf 'postdrop-journal:engine max_version=%s rows=%s contraction_0049=%s\n' \
    "$max_version" "$total" "$applied_49"
}

prp_verify_stage_schema() {
  local stage=$1 expected_engine_version
  local -a no_objects=()
  local -a engine_pre_tables=("${engine_tables[@]}" "${engine_live_tables[@]}")
  local -a engine_pre_functions=("${engine_retired_functions[@]}" "${engine_live_functions[@]}")

  case $stage in
    commerce)
      prp_verify_object_inventory table commerce commerce-postdrop commerce_tables commerce_live_tables
      prp_verify_object_inventory function commerce commerce-postdrop \
        commerce_retired_functions commerce_live_functions
      prp_verify_object_inventory table claude_engine engine-pre-engine no_objects engine_pre_tables
      prp_verify_object_inventory function claude_engine engine-pre-engine \
        no_objects engine_pre_functions
      prp_verify_commerce_journal
      prp_verify_engine_schema_version 48
      ;;
    engine)
      prp_verify_object_inventory table commerce commerce-postdrop commerce_tables commerce_live_tables
      prp_verify_object_inventory function commerce commerce-postdrop \
        commerce_retired_functions commerce_live_functions
      prp_verify_object_inventory table claude_engine engine-postdrop engine_tables engine_live_tables
      prp_verify_object_inventory function claude_engine engine-postdrop \
        engine_retired_functions engine_live_functions
      prp_verify_commerce_journal
      expected_engine_version=$(prp_candidate_engine_schema_version)
      prp_verify_engine_schema_version "$expected_engine_version"
      ;;
    *) prp_die "unknown post-drop schema stage: $stage" ;;
  esac
}

prp_pricing_watermark_snapshot() {
  local attempt=$1
  prp_psql_ro claude_engine >"$TEMP/engine-ledger-heads" <<SQL
/* pricing-retirement-postdrop:engine-ledger-heads */
SELECT account.id,
       COALESCE(max(ledger.id) FILTER (
         WHERE ledger.ts <= EXTRACT(EPOCH FROM now())::bigint - $PRICING_WATERMARK_LAG_SECONDS
       ), 0)
FROM accounts account
LEFT JOIN ledger ON ledger.account_id = account.id
GROUP BY account.id
ORDER BY account.id;
SQL
  prp_psql_ro commerce >"$TEMP/commerce-pricing-cursors" <<SQL
/* pricing-retirement-postdrop:commerce-pricing-cursors */
SELECT account.engine_account_id,
       CASE WHEN cursor.engine_account_id IS NULL THEN 0 ELSE 1 END,
       COALESCE(cursor.last_ledger_id, -1),
       COALESCE(cursor.topups_scanned_through_ledger_id, -1),
       CASE WHEN cursor.updated_at IS NOT NULL AND cursor.updated_at <> '-infinity'
                  AND cursor.updated_at >= now() - make_interval(secs => $PRICING_CURSOR_MAX_AGE_SECONDS)
            THEN 1 ELSE 0 END
FROM customer_profiles profile
JOIN engine_accounts account ON account.user_id = profile.user_id
JOIN users customer ON customer.id = profile.user_id
LEFT JOIN pricing_usage_cursors cursor
  ON cursor.user_id = profile.user_id AND cursor.engine_account_id = account.engine_account_id
WHERE profile.customer_type IN ('b2c', 'b2b')
  AND account.status = 'active' AND account.engine_account_id IS NOT NULL
  AND customer.status = 'active'
ORDER BY account.engine_account_id;
SQL
  awk -F '\t' -v attempt="$attempt" '
    NR == FNR { head[$1] = $2; next }
    {
      targets++
      if (!($1 in head)) missing_engine++
      if ($2 != 1) missing_cursor++
      if ($5 != 1) incomplete_or_stale++
      if ($3 < 0 || $4 < 0) invalid_cursor++
      if ($3 != $4) topup_gap++
      if (($1 in head) && $3 < (head[$1] + 0)) behind++
    }
    END {
      printf "postdrop-watermark:pricing-snapshot attempt=%d mapped_accounts=%d missing_engine=%d missing_cursor=%d incomplete_or_stale=%d invalid_cursor=%d topup_gap=%d behind_stable_head=%d\n",
        attempt, targets, missing_engine, missing_cursor, incomplete_or_stale, invalid_cursor,
        topup_gap, behind
      if (targets == 0 || missing_engine || missing_cursor || incomplete_or_stale ||
          invalid_cursor || topup_gap || behind) exit 1
    }
  ' "$TEMP/engine-ledger-heads" "$TEMP/commerce-pricing-cursors" \
    >"$TEMP/pricing-watermark-diagnostic"
}

prp_verify_pricing_watermarks() {
  local attempt
  for attempt in 1 2 3; do
    if prp_pricing_watermark_snapshot "$attempt"; then
      printf 'postdrop-watermark:pricing mapped_accounts=%s stable_cutoff_seconds=%s max_cursor_age_seconds=%s attempt=%s\n' \
        "$(wc -l <"$TEMP/commerce-pricing-cursors" | tr -d ' ')" \
        "$PRICING_WATERMARK_LAG_SECONDS" "$PRICING_CURSOR_MAX_AGE_SECONDS" "$attempt"
      return 0
    fi
    (( attempt == 3 )) || sleep 2
  done
  cat "$TEMP/pricing-watermark-diagnostic" >&2
  prp_die 'a live pricing cursor is missing, stale, incomplete, or behind its stable engine head'
}

prp_verify_sales_watermarks() {
  prp_psql_ro commerce >"$TEMP/sales-source-watermarks" <<SQL
/* pricing-retirement-postdrop:sales-source-watermarks */
SELECT 'attributions', COALESCE(max(id), 0)
FROM referral_attributions
WHERE created_at < now() - make_interval(secs => $SALES_WATERMARK_LAG_SECONDS)
UNION ALL
SELECT 'usage_events', COALESCE(max(feed_seq), 0)
FROM pricing_usage_events
WHERE created_at < now() - make_interval(secs => $SALES_WATERMARK_LAG_SECONDS)
UNION ALL
SELECT 'topups', COALESCE(max((EXTRACT(EPOCH FROM paid_at) * 1000000)::bigint), 0)
FROM payments
WHERE status = 'paid' AND paid_at < now() - make_interval(secs => $SALES_WATERMARK_LAG_SECONDS)
ORDER BY 1;
SQL
  prp_psql_ro sales >"$TEMP/sales-cursors" <<'SQL'
/* pricing-retirement-postdrop:sales-cursors */
WITH feeds(feed) AS (VALUES ('attributions'), ('usage_events'), ('topups'))
SELECT feeds.feed, COALESCE(cursor.last_id, -1)
FROM feeds LEFT JOIN sync_cursors cursor USING (feed)
ORDER BY feeds.feed;
SQL
  awk -F '\t' '
    NR == FNR { source[$1] = $2; source_count++; next }
    { cursor_count++; if (!($1 in source) || $2 < source[$1]) bad++ }
    END { if (source_count != 3 || cursor_count != 3 || bad != 0) exit 1 }
  ' "$TEMP/sales-source-watermarks" "$TEMP/sales-cursors" \
    || prp_die 'a sales cursor has not covered its stable source watermark'
  printf 'postdrop-watermark:sales feeds=3 stable_cutoff_seconds=%s\n' \
    "$SALES_WATERMARK_LAG_SECONDS"
}

prp_check_zero_query() {
  local database=$1 label=$2 sql=$3 result
  result=$(prp_psql_ro "$database" -c "/* pricing-retirement-postdrop:health:$label */ $sql")
  [[ $result =~ ^[0-9]+$ ]] || prp_die "could not parse business-health check $label"
  [[ $result == 0 ]] || prp_die "business-health check $label is non-zero: $result"
}

prp_verify_authority_drift() {
  prp_psql_ro commerce >"$TEMP/authority-commerce-accounts" <<'SQL'
/* pricing-retirement-postdrop:authority-commerce-accounts */
SELECT engine_account_id, mult_bp, status::text
FROM engine_accounts WHERE engine_account_id IS NOT NULL ORDER BY engine_account_id;
SQL
  prp_psql_ro claude_engine >"$TEMP/authority-engine-accounts" <<'SQL'
/* pricing-retirement-postdrop:authority-engine-accounts */
SELECT id, mult_bp, status FROM accounts ORDER BY id;
SQL
  prp_psql_ro commerce >"$TEMP/authority-commerce-overrides" <<'SQL'
/* pricing-retirement-postdrop:authority-commerce-overrides */
SELECT account.engine_account_id, discount.provider_id, discount.multiplier_bp
FROM customer_provider_discounts discount
JOIN engine_accounts account ON account.user_id = discount.user_id
WHERE account.engine_account_id IS NOT NULL
ORDER BY account.engine_account_id, discount.provider_id;
SQL
  prp_psql_ro claude_engine >"$TEMP/authority-engine-overrides" <<'SQL'
/* pricing-retirement-postdrop:authority-engine-overrides */
SELECT account_id, provider_id, mult_bp
FROM account_provider_discounts ORDER BY account_id, provider_id;
SQL
  awk \
    -v engine_accounts="$TEMP/authority-engine-accounts" \
    -v commerce_accounts="$TEMP/authority-commerce-accounts" \
    -v commerce_overrides="$TEMP/authority-commerce-overrides" \
    -v engine_overrides="$TEMP/authority-engine-overrides" \
    -f "$AUTHORITY_DRIFT_AWK" \
    "$TEMP/authority-engine-accounts" "$TEMP/authority-commerce-accounts" \
    "$TEMP/authority-commerce-overrides" "$TEMP/authority-engine-overrides" \
    >"$TEMP/authority-result"
  awk '$1 ~ /^apitoken_pricing_authority_drift/ && $2 != 0 { bad++ }
       /apitoken_business_reconciliation_up/ && $2 != 1 { bad++ }
       END { exit bad != 0 }' "$TEMP/authority-result" \
    || prp_die 'commerce and engine disagree on default, provider override, or account status'
}

prp_verify_known_below_floor_debt() {
  local rows adjustment_count
  adjustment_count=$(prp_psql_ro claude_engine -c "
    /* pricing-retirement-postdrop:below-floor-adjustment */
    SELECT count(*) FROM ledger
    WHERE account_id = '$KNOWN_BELOW_FLOOR_ACCOUNT' AND kind = 'adjust'
      AND ref = '$KNOWN_BELOW_FLOOR_REF' AND amount_nano = $KNOWN_BELOW_FLOOR_ADJUSTMENT;")
  [[ $adjustment_count == 1 ]] \
    || prp_die 'documented bonus-revocation debt has no unique matching ledger entry'
  rows=$(prp_psql_ro claude_engine -c \
    '/* pricing-retirement-postdrop:below-floor */ SELECT id, balance_nano FROM accounts WHERE balance_nano < -1000000000 ORDER BY id;')
  if [[ -z $rows ]]; then
    printf 'postdrop-health:below-floor accounts=0 documented_adjustment=verified\n'
    return 0
  fi
  [[ $rows == "$KNOWN_BELOW_FLOOR_ACCOUNT"$'\t'"$KNOWN_BELOW_FLOOR_BALANCE" ]] \
    || prp_die 'below-floor accounts differ from the one documented historical bonus-revocation debt'
  printf 'postdrop-health:below-floor accounts=1 documented_adjustment=verified\n'
}

prp_verify_business_health() {
  prp_check_zero_query commerce pricing_queue_ready \
    "SELECT count(*) FROM engine_pricing_jobs WHERE status IN ('pending','retry','processing');"
  prp_check_zero_query commerce credit_queue_ready \
    "SELECT count(*) FROM engine_credits WHERE status IN ('pending','retry','processing');"
  prp_check_zero_query commerce adjustment_queue_ready \
    "SELECT count(*) FROM engine_adjustments WHERE status IN ('pending','retry','processing');"
  prp_check_zero_query commerce credit_or_adjustment_dead \
    "SELECT (SELECT count(*) FROM engine_credits WHERE status = 'dead') + (SELECT count(*) FROM engine_adjustments WHERE status = 'dead');"
  prp_check_zero_query commerce stale_confirmed_pricing \
    "SELECT count(*) FROM engine_pricing_jobs job LEFT JOIN customer_profiles profile ON profile.user_id = job.user_id LEFT JOIN customer_provider_discounts discount ON discount.user_id = job.user_id AND discount.provider_id = job.provider_id WHERE job.status = 'confirmed' AND job.multiplier_bp IS DISTINCT FROM CASE WHEN job.provider_id IS NULL THEN profile.multiplier_bp ELSE discount.multiplier_bp END;"
  prp_check_zero_query commerce unresolved_provider_last_hour \
    "SELECT count(*) FROM pricing_usage_events WHERE created_at >= now() - interval '1 hour' AND (provider_id IS NULL OR provider_id IN ('unattributed','unavailable'));"
  prp_check_zero_query claude_engine settlement_backlog \
    "SELECT count(*) FROM settlement_outbox WHERE state <> 'done';"
  prp_check_zero_query claude_engine settlement_uncollected_last_hour \
    "SELECT COALESCE(sum(uncollected_nano), 0)::bigint FROM ledger WHERE kind = 'charge' AND ts > EXTRACT(EPOCH FROM now())::bigint - 3600;"
  prp_check_zero_query claude_engine pricing_charge_mismatch \
    "SELECT count(*) FROM ledger WHERE ts > EXTRACT(EPOCH FROM now())::bigint - 3600 AND official_nano > 0 AND amount_nano > 0 AND payable_multiplier_bp IS NOT NULL AND ABS(amount_nano::numeric / official_nano * 10000 - payable_multiplier_bp) > 1;"
  prp_check_zero_query claude_engine balance_divergence \
    "WITH funding AS (SELECT account_id, COALESCE(sum(amount_nano),0)::numeric funded_nano FROM ledger WHERE kind IN ('topup','adjust') GROUP BY account_id) SELECT COALESCE(max(ABS(account.balance_nano::numeric + account.spent_nano::numeric + account.reserved_nano::numeric - account.uncollected_nano::numeric - COALESCE(funding.funded_nano,0))),0)::bigint FROM accounts account LEFT JOIN funding ON funding.account_id = account.id;"
  prp_check_zero_query sales pending_referral_reconciliation \
    "SELECT (SELECT count(*) FROM pending_referral_events) + (SELECT count(*) FROM pending_referral_usage_events_v2);"
  prp_check_zero_query sales failed_payout_batches \
    "SELECT count(*) FROM payout_batches WHERE status = 'failed';"
  prp_check_zero_query openkeys openkeys_pricing_drift \
    "WITH drift AS (SELECT batch.id FROM openkeys_batches batch WHERE batch.pricing_contract IS NULL OR batch.pricing_contract NOT IN ('legacy','official_1_to_1') OR batch.mult_bp IS NULL OR batch.mult_bp NOT BETWEEN 1 AND 10000 OR (batch.pricing_contract = 'official_1_to_1' AND batch.mult_bp <> 10000) UNION ALL SELECT key_row.id FROM openkeys_keys key_row JOIN openkeys_batches batch ON batch.id = key_row.batch_id WHERE key_row.pricing_contract IS NULL OR key_row.pricing_contract NOT IN ('legacy','official_1_to_1') OR key_row.mult_bp IS NULL OR key_row.mult_bp NOT BETWEEN 1 AND 10000 OR (key_row.pricing_contract = 'official_1_to_1' AND key_row.mult_bp <> 10000) OR key_row.pricing_contract IS DISTINCT FROM batch.pricing_contract OR key_row.mult_bp IS DISTINCT FROM batch.mult_bp OR key_row.face_value_nano IS DISTINCT FROM batch.face_value_nano) SELECT count(*) FROM drift;"
  prp_verify_authority_drift
  prp_verify_known_below_floor_debt
  printf 'postdrop-health:queues money pricing settlement sales openkeys clear\n'
}

prp_export_service_journal() {
  local destination=$1
  journalctl -q --since "$TESTED_AT" --no-pager -o cat \
    -u 'claude-api*.service' \
    -u 'apitoken-api*.service' \
    -u apitoken-worker.service \
    -u apitoken-sales-api.service \
    -u apitoken-openkeys.service \
    -u apitoken-content-studio.service \
    >"$destination"
}

prp_verify_service_journal() {
  local undefined_errors sales_errors
  prp_export_service_journal "$TEMP/service-journal" \
    || prp_die 'could not inspect service journals after the exact candidate test marker'
  undefined_errors=$(awk '
    {
      line = tolower($0)
      if (line ~ /relation .* does not exist/ || line ~ /sqlstate[^[:alnum:]]*42p01/ ||
          line ~ /undefined_table/) count++
    }
    END { print count + 0 }
  ' "$TEMP/service-journal")
  sales_errors=$(awk '/sync iteration failed/ { count++ } END { print count + 0 }' \
    "$TEMP/service-journal")
  [[ $undefined_errors == 0 ]] \
    || prp_die "service journals contain $undefined_errors undefined-table error(s) since $TESTED_AT"
  [[ $sales_errors == 0 ]] \
    || prp_die "sales journal contains $sales_errors failed sync iteration(s) since $TESTED_AT"
  printf 'postdrop-journal:services since=%s undefined_table=0 sales_sync_errors=0\n' "$TESTED_AT"
}

prp_verify_recovery_evidence() {
  local sha=$1 marker=$BACKUP_ROOT/.pre-deploy-$sha.complete marker_line completed_at
  local completed_epoch marker_mtime marker_age now database dump owner mode mtime age size digest extra
  [[ -f $marker && ! -L $marker ]] \
    || prp_die "exact-SHA backup completion marker is absent: $marker"
  [[ $(stat -c '%u:%a' -- "$marker") == 0:600 ]] \
    || prp_die 'exact-SHA backup completion marker must be root-owned mode 0600'
  marker_mtime=$(stat -c %Y -- "$marker")
  {
    IFS= read -r marker_line || prp_die 'exact-SHA backup completion marker is unreadable'
    if IFS= read -r extra; then prp_die 'exact-SHA backup completion marker has extra lines'; fi
  } <"$marker"
  [[ $marker_line == completed_at=* ]] \
    || prp_die 'exact-SHA backup completion marker is malformed'
  completed_at=${marker_line#completed_at=}
  completed_epoch=$(date -u -d "$completed_at" +%s 2>/dev/null) \
    || prp_die 'exact-SHA backup completion time is invalid'
  now=$(date -u +%s)
  marker_age=$((now - completed_epoch))
  [[ $marker_age -ge 0 && $marker_age -le $BACKUP_MAX_AGE_SECONDS \
      && $marker_mtime -ge $completed_epoch && $marker_mtime -le $((completed_epoch + 5)) ]] \
    || prp_die "exact-SHA backup marker is stale, future-dated, or has inconsistent mtime: age=$marker_age"
  grep -Eq '^[[:space:]]*-[[:space:]]*/var/lib/apitoken(/backups)?[[:space:]]*$' "$BORG_CONFIG" \
    || prp_die 'Borg source directories do not cover /var/lib/apitoken recovery evidence'

  for database in commerce claude_engine; do
    dump=$BACKUP_ROOT/$database.pre-deploy-$sha.dump
    [[ -f $dump && ! -L $dump ]] || prp_die "exact-SHA $database dump is missing"
    owner=$(stat -c %u -- "$dump")
    mode=$(stat -c %a -- "$dump")
    mtime=$(stat -c %Y -- "$dump")
    size=$(stat -c %s -- "$dump")
    [[ $owner == 0 && $mode == 600 && $mtime =~ ^[0-9]+$ && $size =~ ^[1-9][0-9]*$ ]] \
      || prp_die "$database exact-SHA dump has unsafe ownership, mode, mtime, or size"
    age=$((now - mtime))
    [[ $age -ge 0 && $age -le $BACKUP_MAX_AGE_SECONDS && $mtime -le $marker_mtime ]] \
      || prp_die "$database exact-SHA dump is stale, future-dated, or newer than its marker"
    docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
      exec -T commerce-postgres pg_restore --list <"$dump" >/dev/null
    docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
      exec -T commerce-postgres pg_restore --file=/dev/null <"$dump"
    digest=$(wd_sha256_file "$dump")
    [[ $digest =~ ^[0-9a-f]{64}$ ]] || prp_die "could not hash exact-SHA $database dump"
    printf 'postdrop-recovery:%s bytes=%s sha256=%s age_seconds=%s marker_age_seconds=%s\n' \
      "$database" "$size" "$digest" "$age" "$marker_age"
  done
}

prp_prom_query() {
  local query=$1
  curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
    --data-urlencode "query=$query" http://127.0.0.1:9090/api/v1/query
}

prp_wait_for_fresh_monitoring_cycle() {
  local response attempt query
  query="count(apitoken_monitoring_collector_last_success_unixtime) == 1 and min(apitoken_monitoring_collector_last_success_unixtime) >= $VERIFICATION_STARTED_EPOCH and count(apitoken_business_reconciliation_up{scope=~\"pricing_authority|openkeys\"}) == 2 and min(apitoken_business_reconciliation_up{scope=~\"pricing_authority|openkeys\"}) == 1 and count(apitoken_backup_present) == 5 and min(apitoken_backup_present) == 1 and count(apitoken_backup_age_seconds) == 5 and max(apitoken_backup_age_seconds) < 5400"
  for ((attempt = 1; attempt <= 24; attempt++)); do
    response=$(prp_prom_query "$query" 2>/dev/null || true)
    if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
      >/dev/null 2>&1 <<<"$response"; then
      printf 'postdrop-monitoring:fresh collector_cycle_after=%s reconciliation=up backups=fresh\n' \
        "$VERIFICATION_STARTED_EPOCH"
      return 0
    fi
    (( attempt == 24 )) || sleep 5
  done
  prp_die 'monitoring did not complete a fresh healthy collector cycle after schema contraction'
}

prp_verify_targeted_alerts() {
  local IFS='|'
  local alert_regex=${TARGETED_ALERTS[*]} response names bad allowed_count
  response=$(prp_prom_query \
    "ALERTS{alertstate=~\"pending|firing\",alertname=~\"$alert_regex\"}") \
    || prp_die 'could not query targeted post-drop alerts'
  jq --exit-status '.status == "success" and (.data.result | type == "array")' \
    >/dev/null 2>&1 <<<"$response" || prp_die 'Prometheus returned a malformed targeted-alert response'
  names=$(jq -r '.data.result[].metric.alertname' <<<"$response") \
    || prp_die 'could not parse targeted post-drop alert names'
  bad=$(awk '$0 != "EngineAccountsBelowFloor" { print }' <<<"$names")
  [[ -z $bad ]] || prp_die "targeted post-drop alerts are pending or firing: $(tr '\n' ',' <<<"$bad" | sed 's/,$//')"
  allowed_count=$(awk '$0 == "EngineAccountsBelowFloor" { count++ } END { print count + 0 }' \
    <<<"$names")
  printf 'postdrop-monitoring:targeted_alerts clear allowed_engine_accounts_below_floor=%s\n' \
    "$allowed_count"
}

prp_report_database_sizes() {
  local database size
  for database in commerce claude_engine; do
    size=$(prp_psql_ro "$database" -c \
      '/* pricing-retirement-postdrop:database-size */ SELECT pg_database_size(current_database())::bigint;')
    [[ $size =~ ^[1-9][0-9]*$ ]] || prp_die "could not parse $database database size"
    printf 'postdrop-size:%s bytes=%s\n' "$database" "$size"
  done
}

pricing_retirement_postdrop_main() {
  local stage sha
  prp_require_root
  [[ $# -eq 3 && $1 == --stage ]] \
    || prp_die "usage: $0 --stage <commerce|engine> <tested-full-sha>"
  stage=$2
  sha=$3
  [[ $stage == commerce || $stage == engine ]] \
    || prp_die 'post-drop stage must be commerce or engine'
  wd_validate_sha "$sha"

  umask 077
  TEMP=$(mktemp -d /tmp/pricing-retirement-postdrop.XXXXXX) \
    || prp_die 'could not create a private post-drop verification directory'
  trap prp_cleanup EXIT
  VERIFICATION_STARTED_EPOCH=$(date -u +%s)
  [[ $VERIFICATION_STARTED_EPOCH =~ ^[0-9]+$ ]] \
    || prp_die 'could not record post-drop verification start time'

  prp_validate_host_contract
  prp_validate_candidate "$stage" "$sha"
  prp_verify_recovery_evidence "$sha"
  prp_verify_stage_schema "$stage"
  prp_verify_pricing_watermarks
  prp_verify_sales_watermarks
  prp_verify_business_health
  prp_verify_service_journal
  prp_wait_for_fresh_monitoring_cycle
  prp_verify_targeted_alerts
  prp_report_database_sizes
  printf 'POSTDROP VERIFIED stage=%s candidate_sha=%s read_only=1 all_required_evidence=passed\n' \
    "$stage" "$sha"

  prp_cleanup
  trap - EXIT
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  pricing_retirement_postdrop_main "$@"
fi
