#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
ADMISSION=$ROOT/deploy/pricing-retirement-admission.sh

fail() {
  printf 'pricing retirement admission test failed: %s\n' "$*" >&2
  exit 1
}

# shellcheck source=deploy/pricing-retirement-admission.sh
source "$ADMISSION"

fixture=$(mktemp -d "${TMPDIR:-/tmp}/pricing-retirement-admission.XXXXXX") \
  || fail 'could not create fixture'
cleanup() {
  rm -rf -- "$fixture"
}
trap cleanup EXIT

repo=$fixture/repo
mkdir -p \
  "$repo/deploy" \
  "$repo/packages/db" \
  "$repo/crates/registry/migrations_pg" \
  "$repo/crates/registry/src" \
  "$repo/.deploy-artifacts/engine"
cp -R "$ROOT/packages/db/migrations" "$repo/packages/db/migrations"
cp "$ROOT/deploy/pricing-retirement-preflight.sh" "$repo/deploy/pricing-retirement-preflight.sh"
printf '%s\n' '-- fixture commerce contraction' \
  >"$repo/packages/db/migrations/0048_retire_pricing_schema.sql"
node - "$repo/packages/db/migrations/meta/_journal.json" <<'NODE'
const fs = require("node:fs");
const path = process.argv[2];
const journal = JSON.parse(fs.readFileSync(path, "utf8"));
journal.entries.push({
  idx: journal.entries.length,
  version: journal.version,
  when: journal.entries[journal.entries.length - 1].when + 1,
  tag: "0048_retire_pricing_schema",
  breakpoints: true,
});
fs.writeFileSync(path, `${JSON.stringify(journal, null, 2)}\n`);
NODE
printf '%s\n' '-- fixture engine contraction' \
  >"$repo/crates/registry/migrations_pg/0049_retire_pricing_schema.sql"
cat >"$repo/crates/registry/src/pg.rs" <<'EOF'
const MIGRATION_0049: &str =
    include_str!("../migrations_pg/0049_retire_pricing_schema.sql");
pub const CURRENT_SCHEMA_VERSION: i64 = 49;
const ENGINE_MIGRATIONS: &[(i64, &str)] = &[
    (49, MIGRATION_0049),
];
EOF
printf '%s\n' 'fixture engine binary' >"$repo/.deploy-artifacts/engine/claude-api"
chmod 0755 \
  "$repo/deploy/pricing-retirement-preflight.sh" \
  "$repo/.deploy-artifacts/engine/claude-api"

git init --quiet "$repo"
git -C "$repo" config user.name test
git -C "$repo" config user.email test@example.invalid
git -C "$repo" add \
  deploy/pricing-retirement-preflight.sh \
  packages/db/migrations \
  crates/registry/migrations_pg/0049_retire_pricing_schema.sql \
  crates/registry/src/pg.rs
git -C "$repo" commit --quiet -m 'fixture candidate'
sha=$(git -C "$repo" rev-parse HEAD)
tree=$(git -C "$repo" rev-parse 'HEAD^{tree}')

STATE_ROOT=$fixture/state
CANDIDATE_ROOT=$STATE_ROOT/candidates
COMMERCE_APPLIED_MANIFEST=$STATE_ROOT/database-migrations.manifest
ENGINE_RELEASE_ROOT=$fixture/engine-releases
mkdir -p "$CANDIDATE_ROOT" "$ENGINE_RELEASE_ROOT/$sha"
mv "$repo" "$CANDIDATE_ROOT/$sha"
CANDIDATE=$CANDIDATE_ROOT/$sha
cp "$CANDIDATE/.deploy-artifacts/engine/claude-api" "$ENGINE_RELEASE_ROOT/$sha/claude-api"
chmod 0755 "$ENGINE_RELEASE_ROOT/$sha/claude-api"

candidate_manifest=$fixture/candidate.manifest
wd_migration_manifest "$CANDIDATE" >"$candidate_manifest"
awk -v tag="$COMMERCE_CONTRACTION_TAG" -v path="$COMMERCE_CONTRACTION_REL" '
  !($1 == "entry=00000048" && $3 == tag) &&
  !(($1 ~ /^file=[0-9a-f]{64}$/) && $2 == path)
' "$candidate_manifest" >"$COMMERCE_APPLIED_MANIFEST"
marker=$STATE_ROOT/$sha.tested
{
  printf 'sha=%s\n' "$sha"
  printf 'tree=%s\n' "$tree"
  printf 'migration_digest=%s\n' "$(wd_manifest_digest "$candidate_manifest")"
  printf 'engine_artifacts=1\n'
  printf 'engine_binary_sha256=%s\n' \
    "$(wd_sha256_file "$CANDIDATE/.deploy-artifacts/engine/claude-api")"
} >"$marker"

# Production requires a root-owned immutable candidate. The fixture exercises the same identity,
# marker, manifest and release checks while replacing only the host ownership predicate.
pra_require_root() { :; }
pra_require_root_owned_candidate() {
  [[ -d $1 && ! -L $1 ]] || wd_die "fixture candidate is missing"
}

TEST_PREFLIGHT_MODE=authorized
TEST_ENGINE_STATE=$'0\t48'
pra_execute_preflight() {
  local plane=$1 requested_sha=$2
  case $TEST_PREFLIGHT_MODE in
    authorized)
      printf 'evidence:fixture\n'
      printf 'AUTHORIZED:%s migration_sha=%s retention_epoch=1788948000 all_conjunctive_gates=passed\n' \
        "$plane" "$requested_sha"
      ;;
    rejected)
      printf 'NOT AUTHORIZED: fixture rejection\n' >&2
      return 3
      ;;
    malformed)
      printf 'AUTHORIZED:%s migration_sha=%s retention_epoch=not-an-epoch all_conjunctive_gates=passed\n' \
        "$plane" "$requested_sha"
      ;;
    duplicate)
      printf 'AUTHORIZED:%s migration_sha=%s retention_epoch=1788948000 all_conjunctive_gates=passed\n' \
        "$plane" "$requested_sha"
      printf 'AUTHORIZED:%s migration_sha=%s retention_epoch=1788948000 all_conjunctive_gates=passed\n' \
        "$plane" "$requested_sha"
      ;;
    *) return 99 ;;
  esac
}
pra_engine_schema_state() {
  printf '%s\n' "$TEST_ENGINE_STATE"
}

run_admission() {
  ( pricing_retirement_admission_main "$@" )
}

original_commerce_rel=$COMMERCE_CONTRACTION_REL
original_preflight_rel=$PREFLIGHT_REL
COMMERCE_CONTRACTION_REL=packages/db/migrations/0049_unrelated_expand.sql
PREFLIGHT_REL=deploy/removed-after-retirement.sh
TEST_PREFLIGHT_MODE=rejected
run_admission commerce "$sha" >"$fixture/unrelated.out" 2>&1 \
  || fail 'an unrelated commerce migration did not remain a no-op'
grep -Fq 'not applicable' "$fixture/unrelated.out" \
  || fail 'unrelated commerce migration lacked an explicit no-op verdict'
COMMERCE_CONTRACTION_REL=$original_commerce_rel
PREFLIGHT_REL=$original_preflight_rel

TEST_PREFLIGHT_MODE=authorized
run_admission commerce "$sha" >"$fixture/commerce-authorized.out" 2>&1 \
  || fail 'pending commerce contraction rejected an exact authorization'
grep -Fq "pricing-retirement commerce contraction admitted for exact candidate $sha" \
  "$fixture/commerce-authorized.out" \
  || fail 'commerce admission did not bind its success to the exact candidate'

for rejected_mode in rejected malformed duplicate; do
  TEST_PREFLIGHT_MODE=$rejected_mode
  if run_admission commerce "$sha" >"$fixture/commerce-$rejected_mode.out" 2>&1; then
    fail "commerce contraction accepted $rejected_mode preflight output"
  fi
done

PREFLIGHT_REL=deploy/removed-before-contraction.sh
TEST_PREFLIGHT_MODE=authorized
if run_admission commerce "$sha" >"$fixture/commerce-missing-preflight.out" 2>&1; then
  fail 'pending commerce contraction ran without its exact-candidate preflight'
fi
PREFLIGHT_REL=$original_preflight_rel

candidate_file=$(awk -v path="$COMMERCE_CONTRACTION_REL" \
  '$1 ~ /^file=[0-9a-f]{64}$/ && $2 == path && NF == 2 { print }' "$candidate_manifest")
printf '%s\n' "$candidate_file" >>"$COMMERCE_APPLIED_MANIFEST"
if run_admission commerce "$sha" >"$fixture/commerce-partial.out" 2>&1; then
  fail 'commerce contraction accepted a partial applied manifest'
fi
awk -v tag="$COMMERCE_CONTRACTION_TAG" -v path="$COMMERCE_CONTRACTION_REL" '
  !($1 == "entry=00000048" && $3 == tag) &&
  !(($1 ~ /^file=[0-9a-f]{64}$/) && $2 == path)
' "$candidate_manifest" >"$COMMERCE_APPLIED_MANIFEST"

cp "$candidate_manifest" "$COMMERCE_APPLIED_MANIFEST"
PREFLIGHT_REL=deploy/removed-after-retirement.sh
TEST_PREFLIGHT_MODE=rejected
run_admission commerce "$sha" >"$fixture/commerce-applied.out" 2>&1 \
  || fail 'already-applied commerce contraction invoked final preflight'
grep -Fq 'already recorded' "$fixture/commerce-applied.out" \
  || fail 'already-applied commerce contraction lacked a no-op verdict'
PREFLIGHT_REL=$original_preflight_rel
awk -v tag="$COMMERCE_CONTRACTION_TAG" -v path="$COMMERCE_CONTRACTION_REL" '
  !($1 == "entry=00000048" && $3 == tag) &&
  !(($1 ~ /^file=[0-9a-f]{64}$/) && $2 == path)
' "$candidate_manifest" >"$COMMERCE_APPLIED_MANIFEST"

cp "$marker" "$fixture/marker.good"
sed "s/^sha=.*/sha=0000000000000000000000000000000000000000/" \
  "$fixture/marker.good" >"$marker"
if run_admission commerce "$sha" >"$fixture/wrong-marker.out" 2>&1; then
  fail 'commerce contraction accepted a marker for another SHA'
fi
cp "$fixture/marker.good" "$marker"

sed "s/^tree=.*/tree=0000000000000000000000000000000000000000/" \
  "$fixture/marker.good" >"$marker"
if run_admission commerce "$sha" >"$fixture/wrong-tree.out" 2>&1; then
  fail 'commerce contraction accepted a candidate with the wrong tested tree'
fi
cp "$fixture/marker.good" "$marker"

cp "$CANDIDATE/deploy/pricing-retirement-preflight.sh" "$fixture/preflight.good"
printf '%s\n' '# tracked mutation' >>"$CANDIDATE/deploy/pricing-retirement-preflight.sh"
if run_admission commerce "$sha" >"$fixture/tracked-mutation.out" 2>&1; then
  fail 'commerce contraction accepted a candidate with tracked modifications'
fi
cp "$fixture/preflight.good" "$CANDIDATE/deploy/pricing-retirement-preflight.sh"
if run_admission commerce 0000000000000000000000000000000000000000 \
    >"$fixture/missing-candidate.out" 2>&1; then
  fail 'commerce contraction accepted a missing requested SHA candidate'
fi

TEST_PREFLIGHT_MODE=authorized
TEST_ENGINE_STATE=$'0\t48'
run_admission engine "$sha" >"$fixture/engine-authorized.out" 2>&1 \
  || fail 'pending engine contraction rejected an exact authorization'
grep -Fq "pricing-retirement engine contraction admitted for exact candidate $sha" \
  "$fixture/engine-authorized.out" \
  || fail 'engine admission did not bind its success to the exact candidate'

TEST_PREFLIGHT_MODE=rejected
TEST_ENGINE_STATE=$'1\t49'
PREFLIGHT_REL=deploy/removed-after-retirement.sh
run_admission engine "$sha" >"$fixture/engine-applied.out" 2>&1 \
  || fail 'already-applied engine contraction invoked final preflight'
grep -Fq 'already recorded' "$fixture/engine-applied.out" \
  || fail 'already-applied engine contraction lacked a no-op verdict'
PREFLIGHT_REL=$original_preflight_rel

# A post-drop failure is repaired forward. Once version 49 exists, admission must allow an exact
# candidate carrying a later append-only engine migration; before version 49, the initial
# contraction remains isolated and cannot smuggle that later version into the same failure domain.
cp "$CANDIDATE/crates/registry/src/pg.rs" "$fixture/engine-registry-v49"
sed 's/CURRENT_SCHEMA_VERSION: i64 = 49/CURRENT_SCHEMA_VERSION: i64 = 50/' \
  "$fixture/engine-registry-v49" >"$CANDIDATE/crates/registry/src/pg.rs"
MARKER=$marker
TEST_ENGINE_STATE=$'1\t49'
set +e
( pra_engine_is_pending "$sha" ) >"$fixture/engine-forward-noop.out" 2>&1
engine_forward_rc=$?
set -e
[[ $engine_forward_rc == 1 ]] \
  || fail 'already-contracted engine rejected a later append-only forward-fix version'
grep -Fq 'already recorded' "$fixture/engine-forward-noop.out" \
  || { cat "$fixture/engine-forward-noop.out" >&2; fail 'later engine forward-fix candidate lacked an explicit contraction no-op verdict'; }
TEST_ENGINE_STATE=$'0\t48'
if ( pra_engine_is_pending "$sha" ) >"$fixture/engine-combined-version.out" 2>&1; then
  fail 'initial engine contraction accepted a combined version-49-plus-forward-fix delivery'
fi
cp "$fixture/engine-registry-v49" "$CANDIDATE/crates/registry/src/pg.rs"

TEST_ENGINE_STATE=$'0\t47'
if run_admission engine "$sha" >"$fixture/engine-gap.out" 2>&1; then
  fail 'engine contraction accepted a non-contiguous predecessor schema'
fi
TEST_ENGINE_STATE=$'0\t49'
if run_admission engine "$sha" >"$fixture/engine-missing-version.out" 2>&1; then
  fail 'engine contraction accepted a schema that skipped the exact version-49 record'
fi

cp "$ENGINE_RELEASE_ROOT/$sha/claude-api" "$fixture/engine-binary.good"
printf '%s\n' 'tampered' >>"$ENGINE_RELEASE_ROOT/$sha/claude-api"
TEST_ENGINE_STATE=$'0\t48'
if run_admission engine "$sha" >"$fixture/engine-binary-mismatch.out" 2>&1; then
  fail 'engine contraction accepted a release binary different from the tested artifact'
fi
cp "$fixture/engine-binary.good" "$ENGINE_RELEASE_ROOT/$sha/claude-api"
chmod 0755 "$ENGINE_RELEASE_ROOT/$sha/claude-api"

# Top-level order plus `set -e` proves a rejected admission cannot reach either migrator.
commerce_backup_line=$(grep -nF '"$BACKUP_RUNNER" "$SHA"' \
  "$ROOT/deploy/watchdog-migrate.sh" | tail -n 1 | cut -d: -f1)
commerce_admission_line=$(grep -nF '"$PRICING_RETIREMENT_ADMISSION" commerce "$SHA"' \
  "$ROOT/deploy/watchdog-migrate.sh" | cut -d: -f1)
commerce_migrator_line=$(grep -nF '"$CANDIDATE/packages/db/dist/migrate.js"' \
  "$ROOT/deploy/watchdog-migrate.sh" | tail -n 1 | cut -d: -f1)
[[ -n $commerce_backup_line && -n $commerce_admission_line && -n $commerce_migrator_line \
    && $commerce_backup_line -lt $commerce_admission_line \
    && $commerce_admission_line -lt $commerce_migrator_line ]] \
  || fail 'commerce admission is not strictly between fresh backup and migrator'

engine_lock_line=$(grep -nF 'flock -w 30 8' "$ROOT/deploy/engine-migrate.sh" | cut -d: -f1)
engine_admission_line=$(grep -nF '"$PRICING_RETIREMENT_ADMISSION" engine "$SHA"' \
  "$ROOT/deploy/engine-migrate.sh" | cut -d: -f1)
engine_migrator_line=$(grep -nF 'engine-migrate "$ENGINE_POSTGRES_ENV"' \
  "$ROOT/deploy/engine-migrate.sh" | cut -d: -f1)
[[ -n $engine_lock_line && -n $engine_admission_line && -n $engine_migrator_line \
    && $engine_lock_line -lt $engine_admission_line \
    && $engine_admission_line -lt $engine_migrator_line ]] \
  || fail 'engine admission is not strictly between migration lock and migrator'

printf 'pricing retirement admission tests passed\n'
