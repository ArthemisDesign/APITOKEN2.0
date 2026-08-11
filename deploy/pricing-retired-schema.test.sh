#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
RUNBOOK="$ROOT/docs/ops/PRICING_RETIREMENT.md"
BASELINE="$ROOT/deploy/pricing-retired-schema-baseline.tsv"
PREFLIGHT="$ROOT/deploy/pricing-retirement-preflight.sh"

die() {
  printf 'pricing retired-schema contract failed: %s\n' "$*" >&2
  exit 1
}

# shellcheck source=deploy/pricing-retired-schema-manifest.sh
source "$ROOT/deploy/pricing-retired-schema-manifest.sh"

retired_route_fragments=(
  'pricing/v2'
  'pricing-stage5-v2'
  'pricing-stage6-v2'
  'pricing-stage8-capture-v2'
  'pricing-release-activation-v2'
  'pricing-release-orchestration-v2'
  'pricing-shadow-rollout-v2'
  'pricing-policy-delivery-repairs'
  'pricing-catalog-jobs'
  'pricing-switch-jobs'
  'policy-enforcement-cutover'
  'strict-backfill'
)

(( ${#engine_tables[@]} == 31 )) || die "engine manifest has ${#engine_tables[@]} tables, expected 31"
(( ${#commerce_tables[@]} == 43 )) || die "commerce manifest has ${#commerce_tables[@]} tables, expected 43"
(( ${#commerce_symbols[@]} == ${#commerce_tables[@]} )) \
  || die 'commerce Drizzle symbol manifest is not paired with the table manifest'
(( ${#engine_retired_functions[@]} == 48 )) || die 'engine retired-function manifest must contain 48 functions'
(( ${#engine_live_functions[@]} == 4 )) || die 'engine live-function allowlist must contain four functions'
(( ${#commerce_retired_functions[@]} == 5 )) \
  || die 'commerce retired-function manifest must contain five functions'
(( ${#commerce_live_functions[@]} == 2 )) || die 'commerce live-function allowlist must contain two functions'
[[ -f $RUNBOOK ]] || die "runbook is missing: $RUNBOOK"
[[ -f $BASELINE ]] || die "production baseline is missing: $BASELINE"
[[ -x $PREFLIGHT && ! -L $PREFLIGHT ]] || die "read-only preflight is missing or unsafe: $PREFLIGHT"

TEMP=$(mktemp -d "${TMPDIR:-/tmp}/pricing-retired-schema.XXXXXX") \
  || die 'could not create a temporary scan directory'
cleanup() {
  rm -f -- "$TEMP/engine-patterns" "$TEMP/commerce-patterns" "$TEMP/route-patterns" \
    "$TEMP/baseline-expected" "$TEMP/baseline-actual"
  rmdir -- "$TEMP"
}
trap cleanup EXIT

printf '%s\n' "${engine_tables[@]}" >"$TEMP/engine-patterns"
printf '%s\n' "${commerce_tables[@]}" "${commerce_symbols[@]}" >"$TEMP/commerce-patterns"
printf '%s\n' "${retired_route_fragments[@]}" >"$TEMP/route-patterns"

grep_count() {
  local needle=$1 file=$2 count status
  set +e
  count=$(grep -Fxc -- "$needle" "$file")
  status=$?
  set -e
  case $status in
    0|1) printf '%s\n' "$count" ;;
    *) die "could not inspect $file" ;;
  esac
}

text_count() {
  local needle=$1 body=$2 count status
  set +e
  count=$(grep -Fxc -- "$needle" <<<"$body")
  status=$?
  set -e
  case $status in
    0|1) printf '%s\n' "$count" ;;
    *) die 'could not inspect a runbook manifest section' ;;
  esac
}

manifest_line_count() {
  local body=$1 count status
  set +e
  count=$(grep -Ec '^- `public\.[^`()]+`$' <<<"$body")
  status=$?
  set -e
  case $status in
    0|1) printf '%s\n' "$count" ;;
    *) die 'could not count runbook manifest lines' ;;
  esac
}

validate_baseline() {
  local format_count boundary_count not_before_count rows bytes invalid
  format_count=$(grep_count '# format=1' "$BASELINE")
  boundary_count=$(grep -Ec '^# authoritative_max_epoch=[0-9]+$' "$BASELINE" || true)
  not_before_count=$(grep -Ec '^# conservative_drop_not_before_epoch=[0-9]+$' "$BASELINE" || true)
  [[ $format_count == 1 && $boundary_count == 1 && $not_before_count == 1 ]] \
    || die 'production baseline metadata is missing, duplicated, or malformed'

  invalid=$(awk -F '\t' '
    /^#/ { next }
    !header_seen {
      header_seen = 1
      if ($0 != "plane\tschema\ttable\trows\tbytes\tsha256") print "header"
      next
    }
    {
      rows += 1
      if (NF != 6 || $1 !~ /^(engine|commerce)$/ || $2 != "public" ||
          $3 !~ /^[a-z][a-z0-9_]*$/ || $4 !~ /^[0-9]+$/ || $5 !~ /^[0-9]+$/ ||
          $6 !~ /^[0-9a-f]{64}$/) print "row:" rows
    }
    END {
      if (!header_seen) print "missing-header"
      if (rows != 74) print "row-count:" rows
    }
  ' "$BASELINE")
  [[ -z $invalid ]] || die "production baseline has invalid records: $invalid"

  {
    local table
    for table in "${engine_tables[@]}"; do printf 'engine\tpublic\t%s\n' "$table"; done
    for table in "${commerce_tables[@]}"; do printf 'commerce\tpublic\t%s\n' "$table"; done
  } >"$TEMP/baseline-expected"
  awk -F '\t' '!/^#/ && $1 != "plane" { print $1 "\t" $2 "\t" $3 }' "$BASELINE" \
    >"$TEMP/baseline-actual"
  cmp -s -- "$TEMP/baseline-expected" "$TEMP/baseline-actual" \
    || die 'production baseline object order differs from the canonical manifest'

  rows=$(awk -F '\t' '!/^#/ && $1 != "plane" { sum += $4 } END { printf "%.0f", sum }' "$BASELINE")
  [[ $rows == 284032 ]] || die "production baseline row total changed: $rows"
  bytes=$(awk -F '\t' '!/^#/ && $1 != "plane" { sum += $5 } END { printf "%.0f", sum }' "$BASELINE")
  [[ $bytes == 259588096 ]] || die "production baseline byte total changed: $bytes"
}

scan_tracked_source() {
  [[ $# -ge 3 ]] || die 'tracked-source scan requires a mode, pattern file, and path'
  local mode=$1 pattern_file=$2 path output status matches=''
  shift 2
  while IFS= read -r -d '' path; do
    case $mode in
      engine)
        [[ $path == crates/*/src/* ]] || continue
        case $path in
          */tests.rs|*/tests/*) continue ;;
        esac
        ;;
      commerce)
        case $path in
          packages/db/src/schema.ts|*/migrations/*|*.test.*|*.spec.*|*/__tests__/*|*/dist/*|*.md)
            continue
            ;;
        esac
        ;;
      routes)
        case $path in
          */migrations/*|*.test.*|*.spec.*|*/tests.rs|*/tests/*|*/__tests__/*|*/dist/*|*.md)
            continue
            ;;
        esac
        ;;
      *) die "unknown tracked-source scan mode: $mode" ;;
    esac

    set +e
    output=$(LC_ALL=C grep -IHnF -f "$pattern_file" -- "$path")
    status=$?
    set -e
    case $status in
      0)
        [[ -z $matches ]] || matches+=$'\n'
        matches+=$output
        ;;
      1) ;;
      *) die "could not inspect tracked source file $path" ;;
    esac
  done < <(git ls-files -z -- "$@")
  printf '%s' "$matches"
}

validate_baseline

grep -Fq "PGOPTIONS_RO='-c default_transaction_read_only=on" "$PREFLIGHT" \
  || die 'preflight does not force every PostgreSQL session read-only'
grep -Fq "printf 'NOT AUTHORIZED:" "$PREFLIGHT" \
  || die 'diagnostic preflight lacks an explicit non-authorization verdict'
grep -Fq 'exit 3' "$PREFLIGHT" \
  || die 'diagnostic preflight can return a successful admission status'
grep -Fq "printf 'AUTHORIZED:%s" "$PREFLIGHT" \
  || die 'final preflight lacks a bounded plane-specific authorization verdict'
[[ $(grep -Fc 'pg_restore --' "$PREFLIGHT") == 2 ]] \
  || die 'final preflight must list and fully render every exact-SHA dump'
grep -Fq 'BACKUP_MAX_AGE_SECONDS=1800' "$PREFLIGHT" \
  || die 'final preflight does not enforce the fixed 30-minute recovery-evidence window'
grep -Fq 'PRICING_WATERMARK_LAG_SECONDS=120' "$PREFLIGHT" \
  || die 'pricing watermark gate does not cover two complete default sweep intervals'
grep -Fq 'PRICING_CURSOR_MAX_AGE_SECONDS=180' "$PREFLIGHT" \
  || die 'pricing watermark gate does not reject stale completed sweeps'
grep -Fq 'SALES_WATERMARK_LAG_SECONDS=120' "$PREFLIGHT" \
  || die 'sales watermark gate does not cover two complete default sync intervals'
grep -Fq '$final_active == active && $final_load == loaded' "$PREFLIGHT" \
  || die 'active release inspection does not recheck both systemd active and load state'
for required_text in \
  'deploy/pricing-retired-schema-baseline.tsv' \
  'deploy/pricing-retirement-preflight.sh --report' \
  '120-second stable source boundary' \
  '30 minutes' \
  'AUTHORIZED:<plane>'; do
  grep -Fq "$required_text" "$RUNBOOK" \
    || die "runbook is missing automated preflight contract: $required_text"
done

engine_manifest=$(sed -n '/^## Engine drop set /,/^## Commerce drop set /p' "$RUNBOOK")
commerce_manifest=$(sed -n '/^## Commerce drop set /,/^## Mandatory pre-drop gates$/p' "$RUNBOOK")
[[ -n $engine_manifest && -n $commerce_manifest ]] || die 'runbook manifest sections are missing'

[[ $(manifest_line_count "$engine_manifest") == 31 ]] \
  || die 'runbook engine manifest contains extra, missing, or malformed table lines'
[[ $(manifest_line_count "$commerce_manifest") == 43 ]] \
  || die 'runbook commerce manifest contains extra, missing, or malformed table lines'

for table in "${engine_tables[@]}"; do
  [[ $(text_count "- \`public.$table\`" "$engine_manifest") == 1 ]] \
    || die "runbook engine manifest must name public.$table exactly once"
done
for table in "${commerce_tables[@]}"; do
  [[ $(text_count "- \`public.$table\`" "$commerce_manifest") == 1 ]] \
    || die "runbook commerce manifest must name public.$table exactly once"
done

for function in "${engine_retired_functions[@]}" "${engine_live_functions[@]}"; do
  [[ $(text_count "- \`public.$function\`" "$engine_manifest") == 1 ]] \
    || die "runbook engine function manifest must name public.$function exactly once"
done
for function in "${commerce_retired_functions[@]}"; do
  [[ $(text_count "- \`public.$function\`" "$commerce_manifest") == 1 ]] \
    || die "runbook commerce function manifest must name public.$function exactly once"
done
for function in "${commerce_live_functions[@]}"; do
  [[ $commerce_manifest == *"\`public.$function\`"* ]] \
    || die "runbook commerce live-function allowlist is missing public.$function"
done

for index in "${!commerce_tables[@]}"; do
  table=${commerce_tables[$index]}
  symbol=${commerce_symbols[$index]}
  [[ $(grep_count "export const $symbol = pgTable(\"$table\", {" \
    "$ROOT/packages/db/src/schema.ts") == 1 ]] \
    || die "Drizzle manifest mismatch for $symbol -> $table"
done

cd "$ROOT"

engine_matches=$(scan_tracked_source engine "$TEMP/engine-patterns" crates)
if [[ -n $engine_matches ]]; then
  expected_engine_include='const MIGRATION_0041: &str = include_str!("../migrations_pg/0041_strict_funding_lots_v2.sql");'
  [[ $(printf '%s\n' "$engine_matches" | wc -l | tr -d ' ') == 1 \
    && $engine_matches == crates/registry/src/pg.rs:*":$expected_engine_include" ]] \
    || die "engine runtime source references retired tables:\n$engine_matches"
fi

commerce_matches=$(scan_tracked_source commerce "$TEMP/commerce-patterns" apps packages)
[[ -z $commerce_matches ]] \
  || die "commerce runtime source references retired tables:\n$commerce_matches"

# A future controller/client must not resurrect the withdrawn policy/release endpoints as a
# rollback shim. Historical migrations and tests may name them; deployable source may not.
route_matches=$(scan_tracked_source routes "$TEMP/route-patterns" apps packages crates)
[[ -z $route_matches ]] || die "runtime source resurrects retired pricing routes:\n$route_matches"

printf 'pricing retired-schema manifest and source-reader checks passed\n'
