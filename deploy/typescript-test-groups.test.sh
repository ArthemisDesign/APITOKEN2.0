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

BIN=$TEMP/bin
mkdir -p "$BIN"
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
  *' --filter=@claude-api/engine-client '*|*' --filter=@claude-api/payments '*) group=pure ;;
  *) printf 'could not identify test group: %s\n' "$*" >&2; exit 91 ;;
esac
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
PNPM_CAPTURE=$full_capture PNPM_EXPECT_BARRIER=4 PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" >/dev/null
for group in pure commerce sales openkeys; do
  [[ -f $full_capture/$group.completed ]] || fail "$group group did not complete"
done
grep -Fq -- '--workspace-concurrency=4' "$full_capture/pure.args" \
  || fail 'pure group lost bounded internal parallelism'
for package in @claude-api/content-studio @claude-api/web \
  @claude-api/engine-client @claude-api/payments; do
  grep -Fq -- "--filter=$package" "$full_capture/pure.args" \
    || fail "pure group omitted $package"
done
for group in commerce sales openkeys; do
  grep -Fq -- '--workspace-concurrency=1' "$full_capture/$group.args" \
    || fail "$group database group is not serial internally"
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
PNPM_CAPTURE=$filtered_capture PNPM_EXPECT_BARRIER=2 PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" @claude-api/db @claude-api/web >/dev/null
[[ -f $filtered_capture/commerce.completed && -f $filtered_capture/pure.completed ]] \
  || fail 'filtered package scope did not run both selected groups'
[[ ! -e $filtered_capture/sales.started && ! -e $filtered_capture/openkeys.started ]] \
  || fail 'filtered package scope ran an unselected group'
grep -Fq -- '--filter=@claude-api/db' "$filtered_capture/commerce.args" \
  || fail 'filtered commerce group omitted its selected package'
if grep -Fq -- '--filter=@claude-api/commercial-api' "$filtered_capture/commerce.args"; then
  fail 'filtered commerce group widened the exact package scope'
fi

testless_capture=$TEMP/testless
mkdir -p "$testless_capture"
PNPM_CAPTURE=$testless_capture PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" @claude-api/sales-web >/dev/null
if find "$testless_capture" -type f -print -quit | grep -q .; then
  fail 'an explicitly testless package launched a test command'
fi

expect_failure 'unknown package selector' env PATH="$BIN:$PATH" \
  bash "$RUNNER" "$FIXTURE" @claude-api/not-classified
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

printf 'typescript-test-groups.test: ok\n'
