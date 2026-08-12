#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
POSTDROP=$ROOT/deploy/pricing-retirement-postdrop.sh

fail() {
  printf 'pricing retirement post-drop test failed: %s\n' "$*" >&2
  exit 1
}

# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"
# shellcheck source=deploy/pricing-retirement-postdrop.sh
source "$POSTDROP"

fixture=$(mktemp -d "${TMPDIR:-/tmp}/pricing-retirement-postdrop-test.XXXXXX") \
  || fail 'could not create fixture directory'
cleanup_fixture() {
  rm -rf -- "$fixture"
}
trap cleanup_fixture EXIT

# The processed..candidate range is the durable trigger. A failed post-drop delivery leaves
# processed.sha behind, so a forward fix still sees the same newly added contraction path.
repo=$fixture/repo
git init -q "$repo"
git -C "$repo" config user.name postdrop-test
git -C "$repo" config user.email postdrop-test@example.invalid
printf 'fixture\n' >"$repo/README"
git -C "$repo" add README
git -C "$repo" commit -q -m base
base=$(git -C "$repo" rev-parse HEAD)
[[ $(wd_pricing_retirement_postdrop_stage "$repo" "$base" "$base") == none ]] \
  || fail 'unchanged range selected a post-drop stage'

mkdir -p "$repo/packages/db/migrations"
printf 'SELECT 1;\n' >"$repo/packages/db/migrations/0049_retire_pricing_schema.sql"
git -C "$repo" add packages/db/migrations/0049_retire_pricing_schema.sql
git -C "$repo" commit -q -m commerce
commerce_sha=$(git -C "$repo" rev-parse HEAD)
[[ $(wd_pricing_retirement_postdrop_stage "$repo" "$base" "$commerce_sha") == commerce ]] \
  || fail 'commerce contraction addition did not select commerce post-drop proof'

mkdir -p "$repo/crates/registry/migrations_pg"
printf 'SELECT 1;\n' >"$repo/crates/registry/migrations_pg/0049_retire_pricing_schema.sql"
git -C "$repo" add crates/registry/migrations_pg/0049_retire_pricing_schema.sql
git -C "$repo" commit -q -m engine
engine_sha=$(git -C "$repo" rev-parse HEAD)
[[ $(wd_pricing_retirement_postdrop_stage "$repo" "$commerce_sha" "$engine_sha") == engine ]] \
  || fail 'engine contraction addition did not select engine post-drop proof'
set +e
wd_pricing_retirement_postdrop_stage "$repo" "$base" "$engine_sha" >/dev/null 2>&1
both_rc=$?
set -e
[[ $both_rc == 3 ]] || fail 'one range containing both contractions did not fail closed'

TEMP=$fixture/runtime
mkdir -p "$TEMP"
fixture_absent=(retired_table)
fixture_present=(live_table)
OBJECT_MODE=good
JOURNAL_MODE=good
ENGINE_SCHEMA_MODE=good
DEBT_MODE=none

prp_psql_ro() {
  local database=$1
  shift
  local query="$*"
  case $query in
    *pricing-retirement-postdrop:objects:fixture:table*|*pricing-retirement-postdrop:objects:fixture:function*)
      if [[ $OBJECT_MODE == good ]]; then
        printf 'absent\tretired_table\t0\npresent\tlive_table\t1\n'
      else
        printf 'absent\tretired_table\t1\npresent\tlive_table\t1\n'
      fi
      ;;
    *pricing-retirement-postdrop:commerce-journal*)
      if [[ $JOURNAL_MODE == good ]]; then
        if [[ -n ${JOURNAL_FORWARD_HASH:-} ]]; then
          printf '1\t1\t%s\t51\t51\t1788948001000\n' "$JOURNAL_FORWARD_HASH"
        else
          printf '1\t1\t%s\t50\t50\t1788948000000\n' "$COMMERCE_CONTRACTION_HASH"
        fi
      else
        printf '1\t1\t%s\t48\t49\t1788947000000\n' \
          aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
      fi
      ;;
    *pricing-retirement-postdrop:engine-schema:48*)
      if [[ $ENGINE_SCHEMA_MODE == good ]]; then printf '0\t48\t48\t48\t0\n'; else printf '0\t48\t47\t47\t1\n'; fi
      ;;
    *pricing-retirement-postdrop:engine-schema:49*)
      if [[ $ENGINE_SCHEMA_MODE == good ]]; then printf '1\t49\t49\t49\t0\n'; else printf '0\t48\t48\t48\t1\n'; fi
      ;;
    *pricing-retirement-postdrop:engine-schema:50*)
      if [[ $ENGINE_SCHEMA_MODE == good ]]; then printf '1\t50\t50\t50\t0\n'; else printf '1\t50\t49\t49\t1\n'; fi
      ;;
    *pricing-retirement-postdrop:below-floor-adjustment*) printf '1\n' ;;
    *pricing-retirement-postdrop:below-floor*)
      case $DEBT_MODE in
        none) printf '' ;;
        known) printf '%s\t%s\n' "$KNOWN_BELOW_FLOOR_ACCOUNT" "$KNOWN_BELOW_FLOOR_BALANCE" ;;
        unknown) printf 'acct_unreviewed\t-1000000001\n' ;;
      esac
      ;;
    *) fail "unexpected PostgreSQL fixture query for $database: $query" ;;
  esac
}

prp_verify_object_inventory table fixture fixture fixture_absent fixture_present >/dev/null
prp_verify_object_inventory function fixture fixture fixture_absent fixture_present >/dev/null
OBJECT_MODE=bad
if ( prp_verify_object_inventory table fixture fixture fixture_absent fixture_present ) \
    >/dev/null 2>&1; then
  fail 'object verifier accepted a retired table that still exists'
fi
OBJECT_MODE=good

CANDIDATE=$fixture/candidate
mkdir -p "$CANDIDATE/packages/db/migrations/meta" "$CANDIDATE/crates/registry/src"
printf 'DROP TABLE public.retired_table;\n' \
  >"$CANDIDATE/$COMMERCE_CONTRACTION_REL"
node - "$CANDIDATE/packages/db/migrations/meta/_journal.json" <<'NODE'
const fs = require("node:fs");
const destination = process.argv[2];
const entries = Array.from({ length: 50 }, (_, idx) => ({
  idx,
  version: "7",
  when: 1788947952000 + idx * 1000,
  tag: `fixture_${String(idx).padStart(4, "0")}`,
  breakpoints: true,
}));
entries[49].tag = "0049_retire_pricing_schema";
entries[49].when = 1788948000000;
fs.writeFileSync(destination, `${JSON.stringify({ version: "7", dialect: "postgresql", entries })}\n`);
NODE
fixture_contraction_hash=$(wd_sha256_file "$CANDIDATE/$COMMERCE_CONTRACTION_REL")
{
  printf 'entry=00000049 %s 0049_retire_pricing_schema\n' \
    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  printf 'file=%s %s\n' "$fixture_contraction_hash" "$COMMERCE_CONTRACTION_REL"
} >"$TEMP/candidate-migrations.manifest"
prp_verify_commerce_journal >/dev/null
JOURNAL_MODE=bad
if ( prp_verify_commerce_journal ) >/dev/null 2>&1; then
  fail 'commerce journal verifier accepted a non-latest contraction record'
fi
JOURNAL_MODE=good

printf 'SELECT 1;\n' >"$CANDIDATE/packages/db/migrations/0050_forward_fix.sql"
JOURNAL_FORWARD_HASH=$(wd_sha256_file \
  "$CANDIDATE/packages/db/migrations/0050_forward_fix.sql")
node - "$CANDIDATE/packages/db/migrations/meta/_journal.json" <<'NODE'
const fs = require("node:fs");
const path = process.argv[2];
const journal = JSON.parse(fs.readFileSync(path, "utf8"));
journal.entries.push({
  idx: 50,
  version: "7",
  when: 1788948001000,
  tag: "0050_forward_fix",
  breakpoints: true,
});
fs.writeFileSync(path, `${JSON.stringify(journal)}\n`);
NODE
{
  printf 'entry=00000050 %s 0050_forward_fix\n' \
    cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
  printf 'file=%s packages/db/migrations/0050_forward_fix.sql\n' "$JOURNAL_FORWARD_HASH"
} >>"$TEMP/candidate-migrations.manifest"
prp_verify_commerce_journal >/dev/null \
  || fail 'commerce journal verifier rejected an exact append-only forward fix after 0049'

{
  printf '%s\n' 'pub const CURRENT_SCHEMA_VERSION: i64 = 49;'
  printf '%s\n' 'const MIGRATION_0049: &str = include_str!("../migrations_pg/0049_retire_pricing_schema.sql");'
  printf '%s\n' 'const ENGINE_MIGRATIONS: &[(i64, &str)] = &[(49, MIGRATION_0049),];'
} >"$CANDIDATE/crates/registry/src/pg.rs"
[[ $(prp_candidate_engine_schema_version) == 49 ]] \
  || fail 'candidate engine schema version parser rejected canonical 0049 registration'
sed 's/CURRENT_SCHEMA_VERSION: i64 = 49/CURRENT_SCHEMA_VERSION: i64 = 50/' \
  "$CANDIDATE/crates/registry/src/pg.rs" >"$TEMP/pg-v50.rs"
mv "$TEMP/pg-v50.rs" "$CANDIDATE/crates/registry/src/pg.rs"
[[ $(prp_candidate_engine_schema_version) == 50 ]] \
  || fail 'candidate engine schema version parser rejected an append-only forward-fix version'

prp_verify_engine_schema_version 48 >/dev/null
prp_verify_engine_schema_version 49 >/dev/null
prp_verify_engine_schema_version 50 >/dev/null
ENGINE_SCHEMA_MODE=bad
if ( prp_verify_engine_schema_version 49 ) >/dev/null 2>&1; then
  fail 'engine journal verifier accepted a missing contraction version'
fi
ENGINE_SCHEMA_MODE=good

prp_verify_known_below_floor_debt >/dev/null
DEBT_MODE=known
prp_verify_known_below_floor_debt >/dev/null
DEBT_MODE=unknown
if ( prp_verify_known_below_floor_debt ) >/dev/null 2>&1; then
  fail 'below-floor verifier accepted an undocumented account'
fi
DEBT_MODE=none

TESTED_AT=2026-08-11T00:00:00Z
JOURNAL_EXPORT_MODE=good
prp_export_service_journal() {
  local destination=$1
  case $JOURNAL_EXPORT_MODE in
    good) printf 'service ready\n' >"$destination" ;;
    undefined) printf 'ERROR: relation "pricing_months" does not exist (SQLSTATE 42P01)\n' >"$destination" ;;
    sales) printf 'sync iteration failed\n' >"$destination" ;;
  esac
}
prp_verify_service_journal >/dev/null
JOURNAL_EXPORT_MODE=undefined
if ( prp_verify_service_journal ) >/dev/null 2>&1; then
  fail 'service-journal verifier accepted an undefined-table error'
fi
JOURNAL_EXPORT_MODE=sales
if ( prp_verify_service_journal ) >/dev/null 2>&1; then
  fail 'service-journal verifier accepted a sales sync failure'
fi

PROM_MODE=fresh
ALERT_MODE=clear
prp_prom_query() {
  local query=$1
  case $query in
    *apitoken_monitoring_collector_last_success_unixtime*)
      case $PROM_MODE in
        fresh) printf '{"status":"success","data":{"result":[{"metric":{},"value":[1786472104,"1"]}]}}\n' ;;
        stale) printf '{"status":"success","data":{"result":[]}}\n' ;;
      esac
      ;;
    *'ALERTS{'*)
      case $ALERT_MODE in
        clear) printf '{"status":"success","data":{"result":[]}}\n' ;;
        known) printf '{"status":"success","data":{"result":[{"metric":{"alertname":"EngineAccountsBelowFloor"}}]}}\n' ;;
        bad) printf '{"status":"success","data":{"result":[{"metric":{"alertname":"PricingMirrorDrift"}}]}}\n' ;;
      esac
      ;;
    *) fail "unexpected Prometheus fixture query: $query" ;;
  esac
}
VERIFICATION_STARTED_EPOCH=1786472100
prp_wait_for_fresh_monitoring_cycle >/dev/null
PROM_MODE=stale
sleep() { :; }
if ( prp_wait_for_fresh_monitoring_cycle ) >/dev/null 2>&1; then
  fail 'monitoring verifier accepted the absence of a fresh healthy collector cycle'
fi
PROM_MODE=alerts
prp_verify_targeted_alerts >/dev/null
ALERT_MODE=known
prp_verify_targeted_alerts >/dev/null
ALERT_MODE=bad
if ( prp_verify_targeted_alerts ) >/dev/null 2>&1; then
  fail 'targeted-alert verifier accepted active pricing drift'
fi

grep -Fq "PGOPTIONS_RO='-c default_transaction_read_only=on" "$POSTDROP" \
  || fail 'post-drop verifier does not force every PostgreSQL session read-only'
[[ $(grep -Fc 'pg_restore --' "$POSTDROP") == 2 ]] \
  || fail 'post-drop verifier does not list and fully render both exact-SHA dumps'
grep -Fq 'apitoken_monitoring_collector_last_success_unixtime' "$POSTDROP" \
  || fail 'post-drop verifier does not wait for a collector cycle after contraction'
grep -Fq 'POSTDROP VERIFIED stage=%s candidate_sha=%s read_only=1 all_required_evidence=passed' \
  "$POSTDROP" || fail 'post-drop verifier lacks one bounded success verdict'

printf 'pricing retirement post-drop contract tests passed\n'
