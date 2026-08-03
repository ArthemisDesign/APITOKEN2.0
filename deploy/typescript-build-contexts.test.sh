#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
RUNNER=$ROOT/deploy/typescript-build-contexts.sh
TEMP=$(mktemp -d)
trap 'rm -rf "$TEMP"' EXIT

fail() { printf 'typescript-build-contexts.test: %s\n' "$*" >&2; exit 1; }
mkdir -p "$TEMP/bin" "$TEMP/state/started"

cat >"$TEMP/bin/pnpm" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
args=" $* "
state=${BUILD_CONTEXT_TEST_STATE:?}
if [[ $args == *" --filter=@claude-api/commercial-api "* ]]; then
  label=commerce
elif [[ $args == *" --filter=@claude-api/sales-api "* ]]; then
  label=sales
elif [[ $args == *" --filter=@claude-api/openkeys "* ]]; then
  label=openkeys
elif [[ $args == *" --filter=@claude-api/web "* ]]; then
  label=web
elif [[ $args == *" --filter=@claude-api/admin "* ]]; then
  label=admin
elif [[ $args == *" --filter=@claude-api/devbot "* ]]; then
  label=devbot
else
  printf '%s\n' "$*" >"$state/shared.args"
  : >"$state/shared.done"
  exit 0
fi
[[ -f $state/shared.done || $label == web || $label == admin || $label == devbot ]] \
  || { printf 'context started before shared packages: %s\n' "$label" >&2; exit 71; }
printf '%s\n' "$*" >"$state/$label.args"
mkdir "$state/started/$label"
for _ in $(seq 1 100); do
  count=$(find "$state/started" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
  (( count >= EXPECTED_CONTEXT_JOBS )) && break
  sleep 0.02
done
(( count >= EXPECTED_CONTEXT_JOBS )) \
  || { printf 'context builds did not overlap\n' >&2; exit 72; }
[[ ${FAIL_BUILD_CONTEXT:-} != "$label" ]]
STUB
chmod +x "$TEMP/bin/pnpm"

PATH="$TEMP/bin:$PATH" BUILD_CONTEXT_TEST_STATE="$TEMP/state" EXPECTED_CONTEXT_JOBS=6 \
  bash "$RUNNER" "$ROOT" commerce sales openkeys web admin devbot
for context in commerce sales openkeys web admin devbot; do
  [[ -f $TEMP/state/$context.args ]] || fail "$context context was not built"
done
grep -Fq -- '--filter=@claude-api/contracts' "$TEMP/state/shared.args" \
  || fail 'shared contracts package was not built'
grep -Fq -- '--filter=@claude-api/openkeys-db' "$TEMP/state/shared.args" \
  || fail 'shared OpenKeys database package was not built'
[[ $(grep -o -- '--filter=@claude-api/contracts' "$TEMP/state/shared.args" | wc -l | tr -d ' ') == 1 ]] \
  || fail 'a shared package was built more than once'

rm -rf "$TEMP/state"
mkdir -p "$TEMP/state/started"
PATH="$TEMP/bin:$PATH" BUILD_CONTEXT_TEST_STATE="$TEMP/state" EXPECTED_CONTEXT_JOBS=1 \
  bash "$RUNNER" "$ROOT" sales
grep -Fq -- '--filter=@claude-api/sales-db' "$TEMP/state/shared.args" \
  || fail 'sales-only build omitted its database package'
[[ -f $TEMP/state/sales.args && ! -e $TEMP/state/commerce.args ]] \
  || fail 'sales-only build started an unrelated context'

rm -rf "$TEMP/state"
mkdir -p "$TEMP/state/started"
PATH="$TEMP/bin:$PATH" BUILD_CONTEXT_TEST_STATE="$TEMP/state" EXPECTED_CONTEXT_JOBS=1 \
  bash "$RUNNER" "$ROOT" web
grep -Fq -- '--filter=@claude-api/opencode-router-plugin' "$TEMP/state/web.args" \
  || fail 'web-only build omitted the OpenCode plugin package'
[[ -f $TEMP/state/web.args && ! -e $TEMP/state/shared.args && ! -e $TEMP/state/commerce.args ]] \
  || fail 'web-only build started an unrelated context'

rm -rf "$TEMP/state"
mkdir -p "$TEMP/state/started"
PATH="$TEMP/bin:$PATH" BUILD_CONTEXT_TEST_STATE="$TEMP/state" EXPECTED_CONTEXT_JOBS=1 \
  bash "$RUNNER" "$ROOT" admin
[[ -f $TEMP/state/admin.args && ! -e $TEMP/state/shared.args && ! -e $TEMP/state/commerce.args ]] \
  || fail 'admin-only build started shared or unrelated contexts'

rm -rf "$TEMP/state"
mkdir -p "$TEMP/state/started"
PATH="$TEMP/bin:$PATH" BUILD_CONTEXT_TEST_STATE="$TEMP/state" EXPECTED_CONTEXT_JOBS=1 \
  bash "$RUNNER" "$ROOT" devbot
[[ -f $TEMP/state/devbot.args && ! -e $TEMP/state/shared.args && ! -e $TEMP/state/commerce.args ]] \
  || fail 'devbot-only build started shared or unrelated contexts'

rm -rf "$TEMP/state"
mkdir -p "$TEMP/state/started"
if PATH="$TEMP/bin:$PATH" BUILD_CONTEXT_TEST_STATE="$TEMP/state" EXPECTED_CONTEXT_JOBS=2 \
  FAIL_BUILD_CONTEXT=openkeys bash "$RUNNER" "$ROOT" openkeys web >/dev/null 2>&1; then
  fail 'a failing context build was accepted'
fi
[[ -f $TEMP/state/openkeys.args && -f $TEMP/state/web.args ]] \
  || fail 'the runner did not wait for every context after one failed'

if PATH="$TEMP/bin:$PATH" BUILD_CONTEXT_TEST_STATE="$TEMP/state" EXPECTED_CONTEXT_JOBS=1 \
  bash "$RUNNER" "$ROOT" unknown >/dev/null 2>&1; then
  fail 'an unknown context was accepted'
fi

printf 'typescript-build-contexts.test: ok\n'
