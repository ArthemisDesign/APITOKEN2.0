#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
RUNNER=$ROOT/deploy/typescript-test-groups.sh
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

fail() { printf 'typescript-test-groups.test: %s\n' "$*" >&2; exit 1; }
expect_failure() {
  local description=$1
  shift
  if "$@" >/dev/null 2>&1; then
    fail "$description unexpectedly succeeded"
  fi
}

FIXTURE=$TEMP/workspace
mkdir -p "$FIXTURE/apps" "$FIXTURE/packages"
for manifest in "$ROOT"/apps/*/package.json "$ROOT"/packages/*/package.json; do
  relative=${manifest#"$ROOT"/}
  mkdir -p "$FIXTURE/${relative%/*}"
  cp -- "$manifest" "$FIXTURE/$relative"
done
cp -- "$ROOT/package.json" "$FIXTURE/package.json"
for database_package in db sales-db openkeys-db; do
  mkdir -p "$FIXTURE/packages/$database_package/dist"
  printf 'migration fixture\n' >"$FIXTURE/packages/$database_package/dist/migrate.js"
done

BIN=$TEMP/bin
mkdir -p "$BIN"
REAL_NODE=$(command -v node)
export REAL_NODE
cat >"$BIN/node" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ ${1:-} == - ]]; then
  exec "$REAL_NODE" "$@"
fi
case "${1:-}" in
  */packages/db/dist/migrate.js)
    group=commerce
    [[ ${DATABASE_URL:-} == postgresql://commerce-test ]] || exit 83
    ;;
  */packages/sales-db/dist/migrate.js)
    group=sales
    [[ ${SALES_DATABASE_URL:-} == postgresql://sales-test ]] || exit 84
    ;;
  */packages/openkeys-db/dist/migrate.js)
    group=openkeys
    [[ ${OPENKEYS_DATABASE_URL:-} == postgresql://openkeys-test ]] || exit 85
    ;;
  *)
    printf 'unexpected node invocation: %s\n' "$*" >&2
    exit 86
    ;;
esac
capture=${PNPM_CAPTURE:?}
: >"$capture/$group.migration.started"
expected=${MIGRATION_EXPECT_BARRIER:-0}
if (( expected > 0 )); then
  for _ in $(seq 1 200); do
    started=$(find "$capture" -name '*.migration.started' | wc -l | tr -d ' ')
    (( started >= expected )) && break
    sleep 0.01
  done
  (( started >= expected )) \
    || { printf '%s migration did not overlap its peers\n' "$group" >&2; exit 87; }
fi
[[ ${MIGRATION_FAIL_GROUP:-} != "$group" ]] || exit 24
: >"$capture/$group.migration.completed"
STUB
chmod +x "$BIN/node"

cat >"$BIN/pnpm" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
args=" $* "
case "$args" in
  *' --filter=@claude-api/commercial-api '*|*' --filter=@claude-api/db '*|\
  *' --filter=@claude-api/payment-worker '*) group=commerce ;;
  *' --filter=@claude-api/sales-api '*|*' --filter=@claude-api/sales-db '*) group=sales ;;
  *' --filter=@claude-api/openkeys '*) group=openkeys ;;
  *' --filter=@claude-api/content-studio '*|*' --filter=@claude-api/web '*|\
  *' --filter=@claude-api/opencode-router-plugin '*|\
  *' --filter=@claude-api/engine-client '*|*' --filter=@claude-api/payments '*|\
  *' --filter=@claude-api/admin '*|*' --filter=@claude-api/devbot '*|\
  *' --filter=@claude-api/sales-web '*) group=pure ;;
  *) printf 'could not identify test group: %s\n' "$*" >&2; exit 91 ;;
esac
if [[ $group != pure && -n ${TYPESCRIPT_TEST_COMPONENTS:-} ]]; then
  [[ -f $PNPM_CAPTURE/$group.migration.completed ]] \
    || { printf '%s tests started before their migration\n' "$group" >&2; exit 93; }
fi
printf '%s\n' "$*" >"$PNPM_CAPTURE/$group.args"
: >"$PNPM_CAPTURE/$group.started"
expected=${PNPM_EXPECT_BARRIER:-0}
if (( expected > 0 )); then
  for _ in $(seq 1 200); do
    started=$(find "$PNPM_CAPTURE" -name '*.started' | wc -l | tr -d ' ')
    (( started >= expected )) && break
    sleep 0.01
  done
  (( started >= expected )) \
    || { printf '%s group did not overlap its peers\n' "$group" >&2; exit 92; }
fi
: >"$PNPM_CAPTURE/$group.completed"
[[ ${PNPM_FAIL_GROUP:-} != "$group" ]] || exit 23
STUB
chmod +x "$BIN/pnpm"

full_capture=$TEMP/full
mkdir -p "$full_capture"
PNPM_CAPTURE=$full_capture PNPM_EXPECT_BARRIER=4 MIGRATION_EXPECT_BARRIER=3 \
  TYPESCRIPT_TEST_COMPONENTS=commerce,sales,openkeys,web,admin,devbot \
  TEST_DATABASE_URL=postgresql://commerce-test \
  TEST_SALES_DATABASE_URL=postgresql://sales-test \
  TEST_OPENKEYS_DATABASE_URL=postgresql://openkeys-test PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" >/dev/null
for group in pure commerce sales openkeys; do
  [[ -f $full_capture/$group.completed ]] || fail "$group group did not complete"
done
# The admin component is selected but database-free: its tests join the database-free pure lane
# and no disposable migration may run for it.
grep -Fq -- '--filter=@claude-api/admin' "$full_capture/pure.args" \
  || fail 'pure group omitted @claude-api/admin'
if find "$full_capture" -name 'admin.migration.*' -print -quit | grep -q .; then
  fail 'the database-free admin component launched a migration lane'
fi
# The devbot component follows the same database-free contract as admin.
grep -Fq -- '--filter=@claude-api/devbot' "$full_capture/pure.args" \
  || fail 'pure group omitted @claude-api/devbot'
if find "$full_capture" -name 'devbot.migration.*' -print -quit | grep -q .; then
  fail 'the database-free devbot component launched a migration lane'
fi
for group in commerce sales openkeys; do
  [[ -f $full_capture/$group.migration.completed ]] \
    || fail "$group migration did not complete"
done
grep -Fq -- '--workspace-concurrency=4' "$full_capture/pure.args" \
  || fail 'pure group lost bounded internal parallelism'
for package in @claude-api/content-studio @claude-api/web @claude-api/opencode-router-plugin \
  @claude-api/engine-client @claude-api/payments @claude-api/sales-web; do
  grep -Fq -- "--filter=$package" "$full_capture/pure.args" \
    || fail "pure group omitted $package"
done
for group in commerce sales openkeys; do
  grep -Fq -- '--workspace-concurrency=1' "$full_capture/$group.args" \
    || fail "$group database group is not serial internally"
done
for group in pure commerce sales openkeys; do
  grep -Fq -- '--config.enable-pre-post-scripts=false' "$full_capture/$group.args" \
    || fail "$group tests did not suppress redundant package pretest builds"
done
for package in @claude-api/db @claude-api/commercial-api @claude-api/payment-worker; do
  grep -Fq -- "--filter=$package" "$full_capture/commerce.args" \
    || fail "commerce group omitted $package"
done
for package in @claude-api/sales-db @claude-api/sales-api; do
  grep -Fq -- "--filter=$package" "$full_capture/sales.args" \
    || fail "sales group omitted $package"
done
grep -Fq -- '--filter=@claude-api/openkeys' "$full_capture/openkeys.args" \
  || fail 'OpenKeys group omitted its application tests'

filtered_capture=$TEMP/filtered
mkdir -p "$filtered_capture"
PNPM_CAPTURE=$filtered_capture PNPM_EXPECT_BARRIER=2 MIGRATION_EXPECT_BARRIER=1 \
  TYPESCRIPT_TEST_COMPONENTS=commerce TEST_DATABASE_URL=postgresql://commerce-test \
  PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" @claude-api/db @claude-api/web >/dev/null
[[ -f $filtered_capture/commerce.completed && -f $filtered_capture/pure.completed ]] \
  || fail 'filtered package scope did not run both selected groups'
[[ ! -e $filtered_capture/sales.started && ! -e $filtered_capture/openkeys.started ]] \
  || fail 'filtered package scope ran an unselected group'
[[ -f $filtered_capture/commerce.migration.completed \
  && ! -e $filtered_capture/sales.migration.started \
  && ! -e $filtered_capture/openkeys.migration.started ]] \
  || fail 'filtered component scope ran the wrong disposable migration'
grep -Fq -- '--filter=@claude-api/db' "$filtered_capture/commerce.args" \
  || fail 'filtered commerce group omitted its selected package'
if grep -Fq -- '--filter=@claude-api/commercial-api' "$filtered_capture/commerce.args"; then
  fail 'filtered commerce group widened the exact package scope'
fi

sales_web_capture=$TEMP/sales-web
mkdir -p "$sales_web_capture"
PNPM_CAPTURE=$sales_web_capture PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" @claude-api/sales-web >/dev/null
grep -Fq -- '--filter=@claude-api/sales-web' "$sales_web_capture/pure.args" \
  || fail 'Sales Web tests did not join the database-free pure lane'

migration_only_capture=$TEMP/migration-only
mkdir -p "$migration_only_capture"
PNPM_CAPTURE=$migration_only_capture MIGRATION_EXPECT_BARRIER=1 \
  TYPESCRIPT_TEST_COMPONENTS=sales TEST_SALES_DATABASE_URL=postgresql://sales-test \
  PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" @claude-api/sales-web >/dev/null
[[ -f $migration_only_capture/sales.migration.completed ]] \
  || fail 'a selected Sales component omitted its migration smoke'
grep -Fq -- '--filter=@claude-api/sales-web' "$migration_only_capture/pure.args" \
  || fail 'the Sales component omitted selected Sales Web tests'

expect_failure 'unknown package selector' env PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" @claude-api/not-classified
expect_failure 'non-canonical component selector' env \
  TYPESCRIPT_TEST_COMPONENTS=sales,commerce PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE"
expect_failure 'duplicate component selector' env \
  TYPESCRIPT_TEST_COMPONENTS=commerce,commerce PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE"
mkdir -p "$FIXTURE/apps/new-surface"
printf '%s\n' '{"name":"@claude-api/new-surface","scripts":{"test":"vitest run"}}' \
  >"$FIXTURE/apps/new-surface/package.json"
expect_failure 'unclassified workspace package' env PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE"

rm -rf -- "$FIXTURE/apps/new-surface"
failure_capture=$TEMP/failure
mkdir -p "$failure_capture"
expect_failure 'a failed parallel group' env \
  PNPM_CAPTURE="$failure_capture" PNPM_EXPECT_BARRIER=4 PNPM_FAIL_GROUP=sales PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE"
for group in pure commerce sales openkeys; do
  [[ -f $failure_capture/$group.completed ]] \
    || fail "runner did not reap $group after a peer failed"
done

migration_failure_capture=$TEMP/migration-failure
mkdir -p "$migration_failure_capture"
expect_failure 'a failed parallel migration' env \
  PNPM_CAPTURE="$migration_failure_capture" MIGRATION_EXPECT_BARRIER=3 \
  MIGRATION_FAIL_GROUP=sales TYPESCRIPT_TEST_COMPONENTS=commerce,sales,openkeys \
  TEST_DATABASE_URL=postgresql://commerce-test \
  TEST_SALES_DATABASE_URL=postgresql://sales-test \
  TEST_OPENKEYS_DATABASE_URL=postgresql://openkeys-test PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE"
for group in commerce sales openkeys; do
  [[ -f $migration_failure_capture/$group.migration.started ]] \
    || fail "runner did not start $group migration after a peer failed"
done
[[ -f $migration_failure_capture/commerce.completed \
  && -f $migration_failure_capture/openkeys.completed \
  && -f $migration_failure_capture/pure.completed ]] \
  || fail 'runner did not reap successful lanes after a migration failed'
[[ ! -e $migration_failure_capture/sales.started ]] \
  || fail 'sales tests ran after their migration failed'

printf 'typescript-test-groups.test: ok\n'
