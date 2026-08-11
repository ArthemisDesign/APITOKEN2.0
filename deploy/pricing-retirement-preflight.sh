#!/usr/bin/env bash
set -euo pipefail

# Read-only admission gate for the retired pricing-schema contraction. Diagnostic report mode is
# deliberately non-zero and can never authorize a drop. Final mode is intended to run under the
# watchdog migration lock after its exact-SHA backups have completed.

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
BASELINE="$ROOT/deploy/pricing-retired-schema-baseline.tsv"
MANIFEST="$ROOT/deploy/pricing-retired-schema-manifest.sh"
SOURCE_GUARD="$ROOT/deploy/pricing-retired-schema.test.sh"
AUTHORITY_DRIFT_AWK="$ROOT/deploy/monitoring-authority-drift.awk"

die() {
  printf 'pricing retirement preflight failed: %s\n' "$*" >&2
  exit 1
}

usage() {
  printf '%s\n' \
    'Usage:' \
    '  pricing-retirement-preflight.sh --report' \
    '  pricing-retirement-preflight.sh --final <commerce|engine> <full-commit-sha>' \
    '  pricing-retirement-preflight.sh --validate-manifest' \
    '' \
    '--report is diagnostic and always exits non-zero. Only --final can authorize one named' \
    'contraction, after retention, exact-SHA backups, source, rollback, watermark, dependency,' \
    'and business-health checks all pass.'
}

[[ -f $MANIFEST && ! -L $MANIFEST ]] || die "canonical manifest is missing or unsafe: $MANIFEST"
# shellcheck source=deploy/pricing-retired-schema-manifest.sh
source "$MANIFEST"

MODE=
PLANE=
SHA=
case ${1:-} in
  --report)
    [[ $# -eq 1 ]] || { usage >&2; die '--report accepts no other arguments'; }
    MODE=report
    ;;
  --final)
    [[ $# -eq 3 ]] || { usage >&2; die '--final requires a plane and full commit SHA'; }
    MODE=final
    PLANE=$2
    SHA=$3
    [[ $PLANE == commerce || $PLANE == engine ]] || die 'final plane must be commerce or engine'
    [[ $SHA =~ ^[0-9a-f]{40}$ ]] || die 'final migration SHA must be 40 lowercase hexadecimal characters'
    ;;
  --validate-manifest)
    [[ $# -eq 1 ]] || { usage >&2; die '--validate-manifest accepts no other arguments'; }
    exec bash "$SOURCE_GUARD"
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    die 'an explicit mode is required'
    ;;
esac

COMPOSE_FILE=${PRICING_RETIREMENT_POSTGRES_COMPOSE_FILE:-/usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml}
POSTGRES_ENV=${PRICING_RETIREMENT_POSTGRES_ENV:-/etc/apitoken/postgres.env}
POSTGRES_CONTAINER=${PRICING_RETIREMENT_POSTGRES_CONTAINER:-deploy-commerce-postgres-1}
BACKUP_ROOT=${PRICING_RETIREMENT_BACKUP_ROOT:-/var/lib/apitoken/backups}
WATCHDOG_STATE=${PRICING_RETIREMENT_WATCHDOG_STATE:-/var/lib/apitoken/watchdog}
ENGINE_RELEASE_ROOT=${PRICING_RETIREMENT_ENGINE_RELEASE_ROOT:-/srv/claude-api/releases}
COMMERCE_RELEASE_ROOT=${PRICING_RETIREMENT_COMMERCE_RELEASE_ROOT:-/opt/apitoken/releases}
SOURCE_REPO=${PRICING_RETIREMENT_SOURCE_REPO:-$ROOT}
BORG_CONFIG=${PRICING_RETIREMENT_BORG_CONFIG:-/etc/borgmatic/config.yaml}
AUTHBOT_RUNTIME_STATE=${PRICING_RETIREMENT_AUTHBOT_RUNTIME_STATE:-/usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh}
BACKUP_MAX_AGE_SECONDS=1800
PRICING_WATERMARK_LAG_SECONDS=120
PRICING_CURSOR_MAX_AGE_SECONDS=180
SALES_WATERMARK_LAG_SECONDS=120
PGOPTIONS_RO='-c default_transaction_read_only=on -c statement_timeout=120000 -c lock_timeout=5000 -c timezone=UTC -c datestyle=ISO'

if [[ $MODE == final && ${EUID:-$(id -u)} -ne 0 ]]; then
  die 'final admission must run as root through the fixed watchdog path'
fi
ENGINE_RELEASE_ROOT=$(realpath -- "$ENGINE_RELEASE_ROOT") \
  || die 'engine release root cannot be canonicalized'
COMMERCE_RELEASE_ROOT=$(realpath -- "$COMMERCE_RELEASE_ROOT") \
  || die 'commerce release root cannot be canonicalized'
WATCHDOG_STATE=$(realpath -- "$WATCHDOG_STATE") \
  || die 'watchdog state root cannot be canonicalized'
SOURCE_REPO=$(realpath -- "$SOURCE_REPO") || die 'source repository cannot be canonicalized'
[[ -d $SOURCE_REPO && ! -L $SOURCE_REPO ]] || die "source repository is missing or unsafe: $SOURCE_REPO"
command -v docker >/dev/null 2>&1 || die 'docker is required for read-only PostgreSQL inspection'
command -v git >/dev/null 2>&1 || die 'git is required for rollback-floor verification'
command -v awk >/dev/null 2>&1 || die 'awk is required for bounded snapshot comparison'

sha256_stream() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

sha256_file() {
  local path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$path" | awk '{print $1}'
  else
    shasum -a 256 -- "$path" | awk '{print $1}'
  fi
}

# Final mode uses the root-owned Compose/environment pair from the deployment contract. Report mode
# may fall back to the already-running fixed container because the deploy observer cannot read the
# root-only environment file. Both paths set PostgreSQL read-only before parsing any SQL.
psql_ro() {
  local database=$1
  shift
  if [[ -f $COMPOSE_FILE && ! -L $COMPOSE_FILE && -r $COMPOSE_FILE \
      && -f $POSTGRES_ENV && ! -L $POSTGRES_ENV && -r $POSTGRES_ENV ]]; then
    docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
      exec -T -e "PGOPTIONS=$PGOPTIONS_RO" commerce-postgres \
      psql -X -qAt -F $'\t' -v ON_ERROR_STOP=1 -U commerce -d "$database" "$@"
  else
    [[ $MODE == report ]] \
      || die 'final admission cannot use the unpinned PostgreSQL-container fallback'
    docker inspect "$POSTGRES_CONTAINER" >/dev/null 2>&1 \
      || die "production PostgreSQL container is unavailable: $POSTGRES_CONTAINER"
    docker exec -i -e "PGOPTIONS=$PGOPTIONS_RO" "$POSTGRES_CONTAINER" \
      psql -X -qAt -F $'\t' -v ON_ERROR_STOP=1 -U commerce -d "$database" "$@"
  fi
}

database_exists() {
  local database=$1 exists
  exists=$(psql_ro postgres -c "SELECT 1 FROM pg_database WHERE datname = '$database';")
  [[ $exists == 1 ]]
}

metadata_value() {
  local key=$1 values count
  values=$(sed -n "s/^# $key=//p" "$BASELINE")
  count=$(printf '%s\n' "$values" | awk 'NF { count++ } END { print count + 0 }')
  [[ $count == 1 ]] || die "baseline must define $key exactly once"
  printf '%s\n' "$values"
}

baseline_record() {
  local plane=$1 table=$2
  awk -F '\t' -v plane="$plane" -v table="$table" \
    '$1 == plane && $2 == "public" && $3 == table { print $4 "\t" $5 "\t" $6 }' "$BASELINE"
}

validate_source_contract() {
  [[ -f $BASELINE && ! -L $BASELINE ]] || die "production baseline is missing or unsafe: $BASELINE"
  [[ -x $SOURCE_GUARD || -f $SOURCE_GUARD ]] || die "source-reader guard is missing: $SOURCE_GUARD"
  bash "$SOURCE_GUARD" >/dev/null
  [[ $(metadata_value authoritative_max_epoch) =~ ^[0-9]+$ ]] \
    || die 'authoritative baseline epoch is malformed'
  [[ $(metadata_value conservative_drop_not_before_epoch) =~ ^[0-9]+$ ]] \
    || die 'conservative retention epoch is malformed'
  printf 'source-contract: manifest, baseline, Drizzle map, and runtime reader guard verified\n'
}

table_is_in_plane() {
  local plane=$1 needle=$2 table
  case $plane in
    engine)
      for table in "${engine_tables[@]}"; do [[ $table == "$needle" ]] && return 0; done
      ;;
    commerce)
      for table in "${commerce_tables[@]}"; do [[ $table == "$needle" ]] && return 0; done
      ;;
  esac
  return 1
}

function_is_in_list() {
  local needle=$1
  shift
  local item
  for item in "$@"; do [[ $item == "$needle" ]] && return 0; done
  return 1
}

verify_table_evidence() {
  local plane=$1 database table expected_rows expected_bytes expected_digest extra
  local stats actual_rows actual_bytes actual_digest count=0 total_rows=0 total_bytes=0 size_changes=0
  case $plane in
    engine) database=claude_engine; set -- "${engine_tables[@]}" ;;
    commerce) database=commerce; set -- "${commerce_tables[@]}" ;;
    *) die "unknown evidence plane: $plane" ;;
  esac
  database_exists "$database" || die "$database database is absent"

  for table in "$@"; do
    IFS=$'\t' read -r expected_rows expected_bytes expected_digest extra \
      <<<"$(baseline_record "$plane" "$table")"
    [[ $expected_rows =~ ^[0-9]+$ && $expected_bytes =~ ^[0-9]+$ \
        && $expected_digest =~ ^[0-9a-f]{64}$ && -z ${extra:-} ]] \
      || die "baseline record is missing or malformed for $plane.public.$table"
    stats=$(psql_ro "$database" -c \
      "SELECT count(*), pg_total_relation_size('public.$table'::regclass) FROM public.$table;")
    IFS=$'\t' read -r actual_rows actual_bytes extra <<<"$stats"
    [[ $actual_rows =~ ^[0-9]+$ && $actual_bytes =~ ^[0-9]+$ && -z ${extra:-} ]] \
      || die "could not parse table inventory for $plane.public.$table"
    [[ $actual_rows == "$expected_rows" ]] \
      || die "$plane.public.$table row count changed: baseline=$expected_rows actual=$actual_rows"

    actual_digest=$(psql_ro "$database" -c \
      "COPY (SELECT row_json FROM (SELECT row_to_json(t)::text AS row_json FROM public.$table AS t) rows ORDER BY row_json COLLATE \"C\") TO STDOUT;" \
      | sha256_stream)
    [[ $actual_digest == "$expected_digest" ]] \
      || die "$plane.public.$table content digest differs from the immutable production baseline"
    [[ $actual_bytes == "$expected_bytes" ]] || size_changes=$((size_changes + 1))
    count=$((count + 1))
    total_rows=$((total_rows + actual_rows))
    total_bytes=$((total_bytes + actual_bytes))
  done
  printf 'immutable-evidence:%s tables=%s rows=%s bytes=%s physical_size_changes=%s\n' \
    "$plane" "$count" "$total_rows" "$total_bytes" "$size_changes"
}

time_column_is_authoritative() {
  case $1 in
    ts|created_at|created_ts|updated_at|updated_ts|normalized_at|normalized_ts|recorded_at|recorded_ts|\
    captured_at|captured_ts|occurred_at|occurred_ts|activated_at|activated_ts|completed_at|completed_ts|\
    finished_at|finished_ts|requested_at|requested_ts|decided_at|decided_ts|evaluated_at|evaluated_ts|\
    assigned_at|assigned_ts|recovered_at|recovered_ts|retired_at|retired_ts|published_at|published_ts|\
    committed_at|committed_ts|applied_at|applied_ts|started_at|started_ts|closed_at|closed_ts|\
    cutover_at|cutover_ts|promoted_at|promoted_ts|generated_at|generated_ts|accepted_at|accepted_ts|\
    observed_at|observed_ts|measured_at|measured_ts|prepared_at|prepared_ts|refreshed_at|refreshed_ts|\
    released_at|released_ts|written_at|written_ts|enqueued_at|enqueued_ts|admission_at|admission_ts|\
    tariff_priced_at|tariff_priced_ts|*_created_at|*_created_ts|*_updated_at|*_updated_ts|\
    *_normalized_at|*_normalized_ts|*_recorded_at|*_recorded_ts|*_captured_at|*_captured_ts|\
    *_occurred_at|*_occurred_ts|*_activated_at|*_activated_ts|*_completed_at|*_completed_ts|\
    *_finished_at|*_finished_ts|*_requested_at|*_requested_ts|*_decided_at|*_decided_ts|\
    *_evaluated_at|*_evaluated_ts|*_assigned_at|*_assigned_ts|*_recovered_at|*_recovered_ts|\
    *_retired_at|*_retired_ts|*_published_at|*_published_ts|*_committed_at|*_committed_ts|\
    *_applied_at|*_applied_ts|*_started_at|*_started_ts|*_closed_at|*_closed_ts|*_cutover_at|\
    *_cutover_ts|*_promoted_at|*_promoted_ts|*_generated_at|*_generated_ts|*_accepted_at|\
    *_accepted_ts|*_observed_at|*_observed_ts|*_measured_at|*_measured_ts|*_prepared_at|\
    *_prepared_ts|*_refreshed_at|*_refreshed_ts|*_released_at|*_released_ts|*_written_at|\
    *_written_ts|*_enqueued_at|*_enqueued_ts|*_admission_at|*_admission_ts|*_tariff_priced_at|\
    *_tariff_priced_ts) return 0 ;;
  esac
  return 1
}

sql_table_list() {
  local first=1 table
  for table in "$@"; do
    (( first == 1 )) || printf ','
    printf "'%s'" "$table"
    first=0
  done
}

recompute_authoritative_max() {
  local plane=$1 database table_list columns table column type value max=0 max_source=none
  case $plane in
    engine) database=claude_engine; table_list=$(sql_table_list "${engine_tables[@]}") ;;
    commerce) database=commerce; table_list=$(sql_table_list "${commerce_tables[@]}") ;;
    *) die "unknown timestamp plane: $plane" ;;
  esac
  columns=$(psql_ro "$database" -c "
    SELECT table_name, column_name, data_type
    FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name IN ($table_list)
      AND data_type IN ('smallint', 'integer', 'bigint',
                        'timestamp without time zone', 'timestamp with time zone', 'date')
    ORDER BY table_name, ordinal_position;")
  while IFS=$'\t' read -r table column type; do
    [[ -n $table ]] || continue
    time_column_is_authoritative "$column" || continue
    case $type in
      smallint|integer|bigint)
        value=$(psql_ro "$database" -c \
          "SELECT COALESCE(max($column), 0)::bigint FROM public.$table;")
        ;;
      *)
        value=$(psql_ro "$database" -c \
          "SELECT COALESCE(EXTRACT(EPOCH FROM max($column))::bigint, 0) FROM public.$table;")
        ;;
    esac
    [[ $value =~ ^-?[0-9]+$ ]] || die "could not parse $plane.public.$table.$column maximum"
    if (( value > max )); then max=$value; max_source=$table.$column; fi
  done <<<"$columns"
  (( max > 0 )) || die "no authoritative timestamp found for retired $plane evidence"
  printf '%s\t%s\n' "$max" "$max_source"
}

verify_retention_boundary() {
  local plane max source baseline_max not_before now required
  baseline_max=$(metadata_value authoritative_max_epoch)
  not_before=$(metadata_value conservative_drop_not_before_epoch)
  now=$(date -u +%s)
  for plane in "$@"; do
    IFS=$'\t' read -r max source <<<"$(recompute_authoritative_max "$plane")"
    (( max <= baseline_max )) \
      || die "$plane retired evidence has a newer authoritative timestamp at $source: $max"
    required=$((max + 30 * 24 * 60 * 60))
    (( required <= not_before )) || not_before=$required
    printf 'retention-evidence:%s max_epoch=%s source=%s\n' "$plane" "$max" "$source"
  done
  RETENTION_REQUIRED_EPOCH=$not_before
  RETENTION_NOW_EPOCH=$now
}

check_sha_floor() {
  local component=$1 sha=$2 floor
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || die "$component rollback record is not a full SHA"
  case $component in
    engine) floor=e8cf49ae121b581042c582ddb3621ee29fae8103 ;;
    commerce) floor=0c236aa2334f539786f53429d815d6b7c791adbe ;;
    *) die "unknown rollback-floor component: $component" ;;
  esac
  git -c safe.directory="$SOURCE_REPO" -C "$SOURCE_REPO" cat-file -e "$floor^{commit}" 2>/dev/null \
    || die "Git history lacks the $component rollback floor $floor"
  git -c safe.directory="$SOURCE_REPO" -C "$SOURCE_REPO" cat-file -e "$sha^{commit}" 2>/dev/null \
    || die "Git history cannot classify $component release $sha"
  git -c safe.directory="$SOURCE_REPO" -C "$SOURCE_REPO" merge-base --is-ancestor "$floor" "$sha" \
    || die "$component release predates retired-reader removal floor: $sha"
}

release_sha_from_link() {
  local root=$1 link=$2 target sha
  [[ -L $link ]] || die "release selector is not a symlink: $link"
  target=$(realpath -- "$link") || die "release selector is broken: $link"
  [[ ${target%/*} == "$root" && -d $target && ! -L $target ]] \
    || die "release selector leaves its immutable root: $link"
  sha=${target##*/}
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || die "release selector does not name an immutable SHA: $link"
  printf '%s\n' "$sha"
}

recorded_sha() {
  local file=$1 sha extra
  [[ -f $file && ! -L $file ]] || die "recorded deployment SHA is missing or unsafe: $file"
  {
    IFS= read -r sha || die "recorded deployment SHA is unreadable: $file"
    if IFS= read -r extra; then die "recorded deployment SHA has extra lines: $file"; fi
  } <"$file"
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || die "recorded deployment SHA is malformed: $file"
  printf '%s\n' "$sha"
}

verify_active_unit_floor() {
  local component=$1 root=$2 unit=$3 load active pid before after final_load final_active runtime release sha
  load=$(systemctl show "$unit" -p LoadState --value 2>/dev/null || true)
  [[ $load != not-found && -n $load ]] || return 0
  [[ $load == loaded ]] || die "$unit has unexpected load state $load"
  active=$(systemctl show "$unit" -p ActiveState --value)
  case $active in inactive|failed) return 0 ;; active) ;; *) die "$unit is in transitional state $active" ;; esac
  pid=$(systemctl show "$unit" -p MainPID --value)
  [[ $pid =~ ^[1-9][0-9]*$ ]] || die "$unit has invalid MainPID"
  before=$pid
  case $component in
    engine)
      runtime=$(realpath -- "/proc/$pid/exe" 2>/dev/null) \
        || die "cannot resolve active engine executable for $unit"
      release=${runtime%/*}
      [[ $runtime == "$release/claude-api" ]] || die "$unit is not running an immutable claude-api binary"
      ;;
    commerce)
      runtime=$(realpath -- "/proc/$pid/cwd" 2>/dev/null) \
        || die "cannot resolve active commerce working directory for $unit"
      release=${runtime%/apps/api}
      [[ $release != "$runtime" ]] || release=${runtime%/apps/worker}
      [[ $release != "$runtime" ]] || die "$unit is not running from an API/worker release"
      ;;
  esac
  [[ ${release%/*} == "$root" && -d $release && ! -L $release ]] \
    || die "$unit runs outside the immutable $component release root"
  sha=${release##*/}
  check_sha_floor "$component" "$sha"
  after=$(systemctl show "$unit" -p MainPID --value)
  final_active=$(systemctl show "$unit" -p ActiveState --value)
  final_load=$(systemctl show "$unit" -p LoadState --value)
  [[ $before == "$after" && $final_active == active && $final_load == loaded ]] \
    || die "$unit changed while its rollback floor was inspected"
}

verify_rollback_floors() {
  local sha unit authbot_sha
  for sha in \
    "$(release_sha_from_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/current")" \
    "$(release_sha_from_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/previous")"; do
    check_sha_floor engine "$sha"
  done
  for sha in \
    "$(release_sha_from_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/current")" \
    "$(release_sha_from_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/previous")"; do
    check_sha_floor commerce "$sha"
  done
  check_sha_floor engine "$(recorded_sha "$WATCHDOG_STATE/engine.sha")"
  check_sha_floor commerce "$(recorded_sha "$WATCHDOG_STATE/backend.sha")"
  sha=$(recorded_sha "$WATCHDOG_STATE/processed.sha")
  check_sha_floor engine "$sha"
  check_sha_floor commerce "$sha"

  for unit in claude-api.service claude-api@8787.service claude-api@8788.service \
    claude-api-anthropic@8787.service claude-api-anthropic@8788.service \
    claude-api-openai.service claude-api-openai@8793.service claude-api-openai@8797.service \
    claude-api-gemini.service claude-api-gemini@8795.service claude-api-gemini@8799.service \
    claude-api-kimi.service claude-api-kimi@8804.service claude-api-kimi@8805.service; do
    verify_active_unit_floor engine "$ENGINE_RELEASE_ROOT" "$unit"
  done
  for unit in apitoken-api@3000.service apitoken-api@3001.service apitoken-worker.service; do
    verify_active_unit_floor commerce "$COMMERCE_RELEASE_ROOT" "$unit"
  done
  [[ -x $AUTHBOT_RUNTIME_STATE && ! -L $AUTHBOT_RUNTIME_STATE ]] \
    || die 'fixed authbot runtime-release inspector is missing or unsafe'
  if [[ ${EUID:-$(id -u)} -eq 0 ]]; then
    authbot_sha=$("$AUTHBOT_RUNTIME_STATE" release-sha)
  else
    authbot_sha=$(sudo -n "$AUTHBOT_RUNTIME_STATE" release-sha)
  fi
  [[ -z $authbot_sha || $authbot_sha =~ ^[0-9a-f]{40}$ ]] \
    || die 'authbot runtime inspector returned a malformed release SHA'
  [[ -z $authbot_sha ]] || check_sha_floor engine "$authbot_sha"
  printf 'rollback-floor: current, previous, recorded, and active engine/commerce releases verified\n'
}

TEMP=$(mktemp -d "${TMPDIR:-/tmp}/pricing-retirement-preflight.XXXXXX") \
  || die 'could not create a private preflight directory'
cleanup() {
  rm -f -- "$TEMP"/*
  rmdir -- "$TEMP"
}
trap cleanup EXIT

pricing_watermark_snapshot() {
  local attempt=$1
  psql_ro claude_engine >"$TEMP/engine-ledger-heads" <<SQL \
    || die 'could not read the stable engine ledger heads'
SELECT account.id,
       COALESCE(max(ledger.id) FILTER (
         WHERE ledger.ts <= EXTRACT(EPOCH FROM now())::bigint - $PRICING_WATERMARK_LAG_SECONDS
       ), 0)
FROM accounts account
LEFT JOIN ledger ON ledger.account_id = account.id
GROUP BY account.id
ORDER BY account.id;
SQL
  psql_ro commerce >"$TEMP/commerce-pricing-cursors" <<SQL \
    || die 'could not read the commerce pricing cursors'
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
      printf "watermark:pricing-snapshot attempt=%d mapped_accounts=%d missing_engine=%d missing_cursor=%d incomplete_or_stale=%d invalid_cursor=%d topup_gap=%d behind_stable_head=%d\n",
        attempt, targets, missing_engine, missing_cursor, incomplete_or_stale, invalid_cursor,
        topup_gap, behind
      if (targets == 0 || missing_engine || missing_cursor || incomplete_or_stale ||
          invalid_cursor || topup_gap || behind) exit 1
    }
  ' "$TEMP/engine-ledger-heads" "$TEMP/commerce-pricing-cursors" \
    >"$TEMP/pricing-watermark-diagnostic"
}

verify_pricing_watermarks() {
  local attempt
  for attempt in 1 2 3; do
    if pricing_watermark_snapshot "$attempt"; then
      printf 'watermark:pricing mapped_accounts=%s stable_ledger_cutoff_seconds=%s max_cursor_age_seconds=%s snapshot_attempt=%s\n' \
        "$(wc -l <"$TEMP/commerce-pricing-cursors" | tr -d ' ')" \
        "$PRICING_WATERMARK_LAG_SECONDS" "$PRICING_CURSOR_MAX_AGE_SECONDS" "$attempt"
      return 0
    fi
    (( attempt == 3 )) || sleep 2
  done
  cat "$TEMP/pricing-watermark-diagnostic" >&2
  die 'a mapped pricing cursor is missing, stale, incomplete, behind its stable engine head, or its top-up watermark'
}

verify_sales_watermarks() {
  psql_ro commerce >"$TEMP/sales-source-watermarks" <<SQL
SELECT 'attributions', COALESCE(max(id), 0)
FROM referral_attributions
WHERE created_at < now() - make_interval(secs => $SALES_WATERMARK_LAG_SECONDS)
UNION ALL
SELECT 'usage_events', COALESCE(max(feed_seq), 0)
FROM pricing_usage_events
WHERE created_at < now() - make_interval(secs => $SALES_WATERMARK_LAG_SECONDS)
UNION ALL
SELECT 'topups_v2', COALESCE(max(feed_seq), 0)
FROM payments
WHERE created_at < now() - make_interval(secs => $SALES_WATERMARK_LAG_SECONDS)
ORDER BY 1;
SQL
  psql_ro sales >"$TEMP/sales-cursors" <<'SQL'
WITH feeds(feed) AS (VALUES ('attributions'), ('usage_events'), ('topups_v2'))
SELECT feeds.feed, COALESCE(cursor.last_id, -1)
FROM feeds LEFT JOIN sync_cursors cursor USING (feed)
ORDER BY feeds.feed;
SQL
  awk -F '\t' '
    NR == FNR { source[$1] = $2; source_count++; next }
    { cursor_count++; if (!($1 in source) || $2 < source[$1]) bad++ }
    END { if (source_count != 3 || cursor_count != 3 || bad != 0) exit 1 }
  ' "$TEMP/sales-source-watermarks" "$TEMP/sales-cursors" \
    || die 'a sales cursor has not covered its stable source watermark'

  systemctl is-active --quiet apitoken-sales-api.service \
    || die 'sales API is not active while its cursor evidence is inspected'
  local started errors
  started=$(systemctl show apitoken-sales-api.service -p ActiveEnterTimestamp --value)
  [[ -n $started ]] || die 'sales API activation time is unavailable'
  errors=$(journalctl -q -u apitoken-sales-api.service --since "$started" --no-pager -o cat \
    | awk '/sync iteration failed/ { count++ } END { print count + 0 }')
  [[ $errors == 0 ]] || die "sales sync logged $errors failed iteration(s) since serving activation"
  printf 'watermark:sales feeds=3 stable_source_cutoff_seconds=%s parser_or_sync_errors_since_activation=0\n' \
    "$SALES_WATERMARK_LAG_SECONDS"
}

check_zero_query() {
  local database=$1 label=$2 sql=$3 result
  result=$(psql_ro "$database" -c "$sql")
  [[ $result =~ ^[0-9]+$ ]] || die "could not parse business-health check $label"
  [[ $result == 0 ]] || die "business-health check $label is non-zero: $result"
}

verify_authority_drift() {
  psql_ro commerce >"$TEMP/authority-commerce-accounts" <<'SQL'
SELECT engine_account_id, mult_bp, status::text
FROM engine_accounts WHERE engine_account_id IS NOT NULL ORDER BY engine_account_id;
SQL
  psql_ro claude_engine >"$TEMP/authority-engine-accounts" <<'SQL'
SELECT id, mult_bp, status FROM accounts ORDER BY id;
SQL
  psql_ro commerce >"$TEMP/authority-commerce-overrides" <<'SQL'
SELECT account.engine_account_id, discount.provider_id, discount.multiplier_bp
FROM customer_provider_discounts discount
JOIN engine_accounts account ON account.user_id = discount.user_id
WHERE account.engine_account_id IS NOT NULL
ORDER BY account.engine_account_id, discount.provider_id;
SQL
  psql_ro claude_engine >"$TEMP/authority-engine-overrides" <<'SQL'
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
    || die 'commerce and engine disagree on default, provider override, or account status'
}

verify_business_health() {
  check_zero_query commerce pricing_queue_ready \
    "SELECT count(*) FROM engine_pricing_jobs WHERE status IN ('pending','retry','processing');"
  check_zero_query commerce credit_queue_ready \
    "SELECT count(*) FROM engine_credits WHERE status IN ('pending','retry','processing');"
  check_zero_query commerce adjustment_queue_ready \
    "SELECT count(*) FROM engine_adjustments WHERE status IN ('pending','retry','processing');"
  check_zero_query commerce credit_or_adjustment_dead \
    "SELECT (SELECT count(*) FROM engine_credits WHERE status = 'dead') + (SELECT count(*) FROM engine_adjustments WHERE status = 'dead');"
  check_zero_query commerce stale_confirmed_pricing \
    "SELECT count(*) FROM engine_pricing_jobs job LEFT JOIN customer_profiles profile ON profile.user_id = job.user_id LEFT JOIN customer_provider_discounts discount ON discount.user_id = job.user_id AND discount.provider_id = job.provider_id WHERE job.status = 'confirmed' AND job.multiplier_bp IS DISTINCT FROM CASE WHEN job.provider_id IS NULL THEN profile.multiplier_bp ELSE discount.multiplier_bp END;"
  check_zero_query commerce unresolved_provider_last_hour \
    "SELECT count(*) FROM pricing_usage_events WHERE created_at >= now() - interval '1 hour' AND (provider_id IS NULL OR provider_id IN ('unattributed','unavailable'));"
  check_zero_query claude_engine settlement_backlog \
    "SELECT count(*) FROM settlement_outbox WHERE state <> 'done';"
  check_zero_query claude_engine settlement_uncollected_last_hour \
    "SELECT COALESCE(sum(uncollected_nano), 0)::bigint FROM ledger WHERE kind = 'charge' AND ts > EXTRACT(EPOCH FROM now())::bigint - 3600;"
  check_zero_query claude_engine pricing_charge_mismatch \
    "SELECT count(*) FROM ledger WHERE ts > EXTRACT(EPOCH FROM now())::bigint - 3600 AND official_nano > 0 AND amount_nano > 0 AND payable_multiplier_bp IS NOT NULL AND ABS(amount_nano::numeric / official_nano * 10000 - payable_multiplier_bp) > 1;"
  check_zero_query claude_engine balance_divergence \
    "WITH funding AS (SELECT account_id, COALESCE(sum(amount_nano),0)::numeric funded_nano FROM ledger WHERE kind IN ('topup','adjust') GROUP BY account_id) SELECT COALESCE(max(ABS(account.balance_nano::numeric + account.spent_nano::numeric + account.reserved_nano::numeric - account.uncollected_nano::numeric - COALESCE(funding.funded_nano,0))),0)::bigint FROM accounts account LEFT JOIN funding ON funding.account_id = account.id;"
  check_zero_query sales pending_referral_reconciliation \
    "SELECT (SELECT count(*) FROM pending_referral_events) + (SELECT count(*) FROM pending_referral_usage_events_v2);"
  check_zero_query sales failed_payout_batches \
    "SELECT count(*) FROM payout_batches WHERE status = 'failed';"
  check_zero_query openkeys openkeys_pricing_drift \
    "WITH drift AS (SELECT batch.id FROM openkeys_batches batch WHERE batch.pricing_contract IS NULL OR batch.pricing_contract NOT IN ('legacy','official_1_to_1') OR batch.mult_bp IS NULL OR batch.mult_bp NOT BETWEEN 1 AND 10000 OR (batch.pricing_contract = 'official_1_to_1' AND batch.mult_bp <> 10000) UNION ALL SELECT key_row.id FROM openkeys_keys key_row JOIN openkeys_batches batch ON batch.id = key_row.batch_id WHERE key_row.pricing_contract IS NULL OR key_row.pricing_contract NOT IN ('legacy','official_1_to_1') OR key_row.mult_bp IS NULL OR key_row.mult_bp NOT BETWEEN 1 AND 10000 OR (key_row.pricing_contract = 'official_1_to_1' AND key_row.mult_bp <> 10000) OR key_row.pricing_contract IS DISTINCT FROM batch.pricing_contract OR key_row.mult_bp IS DISTINCT FROM batch.mult_bp OR key_row.face_value_nano IS DISTINCT FROM batch.face_value_nano) SELECT count(*) FROM drift;"
  verify_authority_drift
  printf 'business-health: live queues, money invariants, pricing authority, sales, and OpenKeys clear\n'
}

verify_dependency_graph() {
  local plane=$1 database table_list functions retired_functions external views
  local trigger_table_schema trigger_table trigger_name trigger_function_schema trigger_function
  local table function
  case $plane in
    engine)
      database=claude_engine
      table_list=$(sql_table_list "${engine_tables[@]}")
      functions=("${engine_retired_functions[@]}" "${engine_live_functions[@]}")
      retired_functions=("${engine_retired_functions[@]}")
      ;;
    commerce)
      database=commerce
      table_list=$(sql_table_list "${commerce_tables[@]}")
      functions=("${commerce_retired_functions[@]}" "${commerce_live_functions[@]}")
      retired_functions=("${commerce_retired_functions[@]}")
      ;;
    *) die "unknown dependency plane: $plane" ;;
  esac

  printf '%s\n' "${functions[@]}" | LC_ALL=C sort >"$TEMP/$plane-functions-expected"
  psql_ro "$database" >"$TEMP/$plane-functions-actual" <<'SQL'
SELECT function.proname || '(' || oidvectortypes(function.proargtypes) || ')'
FROM pg_proc function
JOIN pg_namespace namespace ON namespace.oid = function.pronamespace
WHERE namespace.nspname = 'public' AND function.prokind = 'f'
ORDER BY 1;
SQL
  LC_ALL=C sort -o "$TEMP/$plane-functions-actual" "$TEMP/$plane-functions-actual"
  cmp -s -- "$TEMP/$plane-functions-expected" "$TEMP/$plane-functions-actual" \
    || die "$plane public-function inventory differs from the retired/live allowlists"

  external=$(psql_ro "$database" -c "
    SELECT constraint_row.conname, child_ns.nspname, child.relname,
           parent_ns.nspname, parent.relname
    FROM pg_constraint constraint_row
    JOIN pg_class child ON child.oid = constraint_row.conrelid
    JOIN pg_namespace child_ns ON child_ns.oid = child.relnamespace
    JOIN pg_class parent ON parent.oid = constraint_row.confrelid
    JOIN pg_namespace parent_ns ON parent_ns.oid = parent.relnamespace
    WHERE constraint_row.contype = 'f'
      AND parent_ns.nspname = 'public' AND parent.relname IN ($table_list)
      AND NOT (child_ns.nspname = 'public' AND child.relname IN ($table_list))
    ORDER BY 1,2,3,4,5;")
  if [[ $plane == engine ]]; then
    [[ $external == $'api_keys_activation_policy_fk\tpublic\tapi_keys\tpublic\taccount_policy_versions' ]] \
      || die "engine external foreign-key edge differs from the one reviewed exception: ${external:-none}"
  else
    [[ -z $external ]] || die "commerce has an external foreign-key edge into retired schema: $external"
  fi

  views=$(psql_ro "$database" -c "
    SELECT DISTINCT dependent_ns.nspname, dependent.relname
    FROM pg_depend dependency
    JOIN pg_rewrite rewrite ON rewrite.oid = dependency.objid
    JOIN pg_class dependent ON dependent.oid = rewrite.ev_class
    JOIN pg_namespace dependent_ns ON dependent_ns.oid = dependent.relnamespace
    JOIN pg_class referenced ON referenced.oid = dependency.refobjid
    JOIN pg_namespace referenced_ns ON referenced_ns.oid = referenced.relnamespace
    WHERE dependent.relkind IN ('v','m')
      AND referenced_ns.nspname = 'public'
      AND referenced.relname IN ($table_list)
    ORDER BY 1,2;")
  [[ -z $views ]] || die "$plane retired tables are referenced by a view or materialized view: $views"

  psql_ro "$database" >"$TEMP/$plane-triggers" <<SQL
SELECT relation_ns.nspname, relation.relname, trigger_row.tgname,
       function_ns.nspname,
       function.proname || '(' || oidvectortypes(function.proargtypes) || ')'
FROM pg_trigger trigger_row
JOIN pg_class relation ON relation.oid = trigger_row.tgrelid
JOIN pg_namespace relation_ns ON relation_ns.oid = relation.relnamespace
JOIN pg_proc function ON function.oid = trigger_row.tgfoid
JOIN pg_namespace function_ns ON function_ns.oid = function.pronamespace
WHERE NOT trigger_row.tgisinternal
  AND ((relation_ns.nspname = 'public' AND relation.relname IN ($table_list))
       OR (function_ns.nspname = 'public'
       AND function.proname || '(' || oidvectortypes(function.proargtypes) || ')' IN
          ($(for function in "${retired_functions[@]}"; do printf "'%s'," "$function"; done | sed 's/,$//'))))
ORDER BY 1,2,3,4,5;
SQL
  while IFS=$'\t' read -r trigger_table_schema trigger_table trigger_name \
      trigger_function_schema trigger_function; do
    [[ -n $trigger_table ]] || continue
    [[ $trigger_table_schema == public ]] \
      || die "$plane retired function $trigger_function is attached outside public: $trigger_table_schema.$trigger_table"
    table_is_in_plane "$plane" "$trigger_table" \
      || die "$plane retired function $trigger_function is attached to live table $trigger_table"
    [[ $trigger_function_schema == public ]] \
      || die "$plane retired table $trigger_table uses non-public trigger function $trigger_function_schema.$trigger_function"
    case $plane in
      engine) function_is_in_list "$trigger_function" "${engine_retired_functions[@]}" ;;
      commerce) function_is_in_list "$trigger_function" "${commerce_retired_functions[@]}" ;;
    esac || die "$plane retired table $trigger_table uses non-retired trigger function $trigger_function"
    [[ -n $trigger_name ]] || die "$plane returned a malformed trigger inventory row"
  done <"$TEMP/$plane-triggers"
  printf 'dependency-graph:%s external_fk=%s views=0 function_inventory=%s triggers_reviewed=%s\n' \
    "$plane" "$([[ $plane == engine ]] && printf 1 || printf 0)" "${#functions[@]}" \
    "$(wc -l <"$TEMP/$plane-triggers" | tr -d ' ')"
}

verify_exact_candidate() {
  local actual
  actual=$(git -c safe.directory="$SOURCE_REPO" -C "$SOURCE_REPO" rev-parse HEAD)
  [[ $actual == "$SHA" ]] || die "final preflight source is $actual, not migration SHA $SHA"
}

verify_backups() {
  local database dump size digest owner mode mtime age marker marker_line completed_at
  local marker_owner marker_mode marker_mtime marker_age completed_epoch now extra
  marker=$BACKUP_ROOT/.pre-deploy-$SHA.complete
  [[ -f $marker && ! -L $marker ]] || die "exact-SHA backup completion marker is absent: $marker"
  marker_owner=$(stat -c %u -- "$marker")
  marker_mode=$(stat -c %a -- "$marker")
  marker_mtime=$(stat -c %Y -- "$marker")
  [[ $marker_owner == 0 && $marker_mode == 600 && $marker_mtime =~ ^[0-9]+$ ]] \
    || die 'exact-SHA backup marker must be root-owned mode 0600 with a valid mtime'
  {
    IFS= read -r marker_line || die 'exact-SHA backup marker is unreadable'
    if IFS= read -r extra; then die 'exact-SHA backup marker has extra lines'; fi
  } <"$marker"
  [[ $marker_line == completed_at=* && ${marker_line#completed_at=} != "$marker_line" ]] \
    || die 'exact-SHA backup marker has malformed completion evidence'
  completed_at=${marker_line#completed_at=}
  completed_epoch=$(date -u -d "$completed_at" +%s 2>/dev/null) \
    || die 'exact-SHA backup marker has an invalid UTC completion timestamp'
  now=$(date -u +%s)
  [[ $completed_epoch =~ ^[0-9]+$ && $now =~ ^[0-9]+$ ]] \
    || die 'could not evaluate exact-SHA backup freshness'
  marker_age=$((now - completed_epoch))
  [[ $marker_age -ge 0 && $marker_age -le $BACKUP_MAX_AGE_SECONDS ]] \
    || die "exact-SHA backup marker is not fresh: age_seconds=$marker_age max=$BACKUP_MAX_AGE_SECONDS"
  [[ $marker_mtime -ge $completed_epoch && $marker_mtime -le $((completed_epoch + 5)) ]] \
    || die 'exact-SHA backup marker mtime disagrees with its completion timestamp'
  [[ -r $BORG_CONFIG && ! -L $BORG_CONFIG ]] || die 'Borg configuration is unavailable for path verification'
  grep -Eq '^[[:space:]]*-[[:space:]]*/var/lib/apitoken(/backups)?[[:space:]]*$' "$BORG_CONFIG" \
    || die 'Borg source directories do not include /var/lib/apitoken or its backup root'
  for database in commerce claude_engine; do
    dump=$BACKUP_ROOT/$database.pre-deploy-$SHA.dump
    [[ -f $dump && ! -L $dump ]] || die "fresh exact-SHA $database dump is missing"
    owner=$(stat -c %u -- "$dump")
    mode=$(stat -c %a -- "$dump")
    mtime=$(stat -c %Y -- "$dump")
    size=$(stat -c %s -- "$dump")
    [[ $owner == 0 && $mode == 600 && $mtime =~ ^[0-9]+$ && $size =~ ^[1-9][0-9]*$ ]] \
      || die "$database dump must be root-owned mode 0600, non-empty, and have a valid mtime"
    age=$((now - mtime))
    [[ $age -ge 0 && $age -le $BACKUP_MAX_AGE_SECONDS && $mtime -le $marker_mtime ]] \
      || die "$database exact-SHA dump is stale, future-dated, or newer than its completion marker"
    if [[ -f $COMPOSE_FILE && ! -L $COMPOSE_FILE && -r $COMPOSE_FILE \
        && -f $POSTGRES_ENV && ! -L $POSTGRES_ENV && -r $POSTGRES_ENV ]]; then
      docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
        exec -T commerce-postgres pg_restore --list <"$dump" >/dev/null
      docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
        exec -T commerce-postgres pg_restore --file=/dev/null <"$dump"
    else
      die 'exact-SHA dumps cannot be validated without the fixed production Compose definition'
    fi
    digest=$(sha256_file "$dump")
    [[ $digest =~ ^[0-9a-f]{64}$ ]] || die "could not hash $database exact-SHA dump"
    printf 'recovery-evidence:%s path=%s bytes=%s sha256=%s migration_sha=%s mtime_epoch=%s age_seconds=%s marker_age_seconds=%s\n' \
      "$database" "$dump" "$size" "$digest" "$SHA" "$mtime" "$age" "$marker_age"
  done
}

validate_source_contract
verify_rollback_floors
verify_pricing_watermarks
verify_sales_watermarks
verify_business_health

planes=()
case "$MODE:$PLANE" in
  report:|final:commerce) planes=(engine commerce) ;;
  final:engine) planes=(engine) ;;
esac
for target_plane in "${planes[@]}"; do
  verify_table_evidence "$target_plane"
  verify_dependency_graph "$target_plane"
done
verify_retention_boundary "${planes[@]}"

if [[ $MODE == report ]]; then
  printf 'NOT AUTHORIZED: diagnostic report only; retention gate requires epoch %s (now %s), and exact-SHA backups were not admitted.\n' \
    "$RETENTION_REQUIRED_EPOCH" "$RETENTION_NOW_EPOCH" >&2
  exit 3
fi

verify_exact_candidate
(( RETENTION_NOW_EPOCH >= RETENTION_REQUIRED_EPOCH )) \
  || die "retention has not elapsed: required_epoch=$RETENTION_REQUIRED_EPOCH now_epoch=$RETENTION_NOW_EPOCH"
verify_backups
printf 'AUTHORIZED:%s migration_sha=%s retention_epoch=%s all_conjunctive_gates=passed\n' \
  "$PLANE" "$SHA" "$RETENTION_REQUIRED_EPOCH"
