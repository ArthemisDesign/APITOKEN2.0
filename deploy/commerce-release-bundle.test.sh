#!/usr/bin/env bash
set -eEuo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(realpath -- "$(mktemp -d)")
cleanup() {
  chmod -R u+w "$TEMP" 2>/dev/null || true
  rm -rf -- "$TEMP"
}
trap cleanup EXIT

fail() {
  printf '[commerce-release-bundle-test] ERROR: %s\n' "$*" >&2
  exit 1
}

FIXTURE=$TEMP/candidate
BIN=$TEMP/bin
mkdir -p "$BIN" "$FIXTURE"
for relative_dir in \
  apps/api \
  apps/worker \
  apps/content-studio \
  packages/contracts \
  packages/db \
  packages/engine-client \
  packages/payments; do
  mkdir -p "$FIXTURE/$relative_dir"
  printf '{"name":"fixture-%s","private":true,"type":"module"}\n' \
    "${relative_dir//\//-}" >"$FIXTURE/$relative_dir/package.json"
done
printf '{"name":"fixture","private":true,"packageManager":"pnpm@9.7.0"}\n' \
  >"$FIXTURE/package.json"
printf 'lockfileVersion: "9.0"\n' >"$FIXTURE/pnpm-lock.yaml"
printf 'packages:\n  - "apps/*"\n  - "packages/*"\n' >"$FIXTURE/pnpm-workspace.yaml"

for relative_file in \
  apps/api/dist/main.js \
  apps/worker/dist/main.js \
  packages/contracts/dist/index.js \
  packages/db/dist/migrate.js \
  packages/engine-client/dist/index.js \
  packages/payments/dist/index.js; do
  mkdir -p "$FIXTURE/$(dirname -- "$relative_file")"
  printf 'export default %q;\n' "$relative_file" >"$FIXTURE/$relative_file"
done
mkdir -p \
  "$FIXTURE/packages/db/migrations" \
  "$FIXTURE/apps/content-studio/.next/static" \
  "$FIXTURE/apps/content-studio/.next/standalone/apps/content-studio" \
  "$FIXTURE/apps/content-studio/.next/standalone/node_modules/runtime"
printf 'migration\n' >"$FIXTURE/packages/db/migrations/0001.sql"
printf 'build-id\n' >"$FIXTURE/apps/content-studio/.next/BUILD_ID"
printf 'static\n' >"$FIXTURE/apps/content-studio/.next/static/app.js"
printf 'server\n' \
  >"$FIXTURE/apps/content-studio/.next/standalone/apps/content-studio/server.js"
printf 'runtime\n' \
  >"$FIXTURE/apps/content-studio/.next/standalone/node_modules/runtime/index.js"
ln -s runtime \
  "$FIXTURE/apps/content-studio/.next/standalone/node_modules/runtime-link"
mkdir -p "$FIXTURE/apps/api/src"
printf 'source must not ship\n' >"$FIXTURE/apps/api/src/private.ts"

cat >"$BIN/pnpm" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"$BUNDLE_TEST_PNPM_LOG"
stage=
while [[ $# -gt 0 ]]; do
  if [[ $1 == --dir ]]; then
    stage=$2
    break
  fi
  shift
done
[[ -n $stage ]]
mkdir -p "$stage/node_modules/runtime"
printf 'installed\n' >"$stage/node_modules/runtime/index.js"
STUB
chmod +x "$BIN/pnpm"

export PATH="$BIN:$PATH"
export BUNDLE_TEST_PNPM_LOG=$TEMP/pnpm.log
built=$(bash "$ROOT/deploy/commerce-release-bundle.sh" "$FIXTURE")
BUNDLE=$FIXTURE/.deploy-artifacts/commerce-release
[[ $built == "$BUNDLE" && -d $BUNDLE ]] || fail "builder did not publish the fixed artifact"
for required in \
  .release-bundle-format \
  apps/api/dist/main.js \
  apps/worker/dist/main.js \
  packages/db/dist/migrate.js \
  packages/db/migrations/0001.sql \
  apps/content-studio/.next/BUILD_ID \
  apps/content-studio/.next/standalone/apps/content-studio/server.js \
  apps/content-studio/.next/standalone/apps/content-studio/.next/static/app.js \
  node_modules/runtime/index.js; do
  [[ -e $BUNDLE/$required || -L $BUNDLE/$required ]] \
    || fail "compact bundle omitted $required"
done
[[ ! -e $BUNDLE/apps/api/src ]] || fail "source tree leaked into compact bundle"
[[ ! -w $BUNDLE && ! -w $BUNDLE/apps/api/dist/main.js ]] \
  || fail "published compact bundle is writable"
for required_argument in \
  '--prod' \
  '--offline' \
  '--ignore-scripts' \
  '--frozen-lockfile' \
  '--filter @claude-api/commercial-api...' \
  '--filter @claude-api/payment-worker...' \
  '--filter @claude-api/db...'; do
  grep -Fq -- "$required_argument" "$BUNDLE_TEST_PNPM_LOG" \
    || fail "production install lost argument: $required_argument"
done

digest_before=$(node "$ROOT/deploy/release-tree-digest.mjs" "$BUNDLE")
[[ $digest_before =~ ^[0-9a-f]{64}$ ]] || fail "tree digest is malformed"
chmod u+w "$BUNDLE/apps/api/dist/main.js"
printf 'tamper\n' >>"$BUNDLE/apps/api/dist/main.js"
digest_after=$(node "$ROOT/deploy/release-tree-digest.mjs" "$BUNDLE")
[[ $digest_after =~ ^[0-9a-f]{64}$ && $digest_after != "$digest_before" ]] \
  || fail "tree digest did not bind file content and mode"

unsafe=$TEMP/unsafe
mkdir -p "$unsafe"
ln -s ../../outside "$unsafe/escape"
if node "$ROOT/deploy/release-tree-digest.mjs" "$unsafe" >"$TEMP/unsafe.out" 2>&1; then
  fail "tree digest accepted an escaping symlink"
fi
grep -Fq 'symlink escapes release tree' "$TEMP/unsafe.out" \
  || fail "escaping symlink failure was not explicit"

APP=$TEMP/content-app
RUN_LOG=$TEMP/runtime.log
mkdir -p "$APP/.next/standalone/apps/content-studio"
printf 'server\n' >"$APP/.next/standalone/apps/content-studio/server.js"
cat >"$BIN/node-probe" <<'STUB'
#!/usr/bin/env bash
printf 'standalone|%s|%s|%s\n' "$HOSTNAME" "$PORT" "$*" >"$BUNDLE_TEST_RUNTIME_LOG"
STUB
chmod +x "$BIN/node-probe"
CONTENT_STUDIO_APP_ROOT=$APP \
CONTENT_STUDIO_NODE_BIN=$BIN/node-probe \
HOSTNAME=127.0.0.1 \
PORT=13500 \
BUNDLE_TEST_RUNTIME_LOG=$RUN_LOG \
  bash "$ROOT/deploy/content-studio-start.sh"
grep -Fq "standalone|127.0.0.1|13500|$APP/.next/standalone/apps/content-studio/server.js" \
  "$RUN_LOG" || fail "Content Studio launcher did not select standalone runtime"

rm -f "$APP/.next/standalone/apps/content-studio/server.js"
mkdir -p "$APP/node_modules/.bin"
cat >"$APP/node_modules/.bin/next" <<'STUB'
#!/usr/bin/env bash
printf 'legacy|%s\n' "$*" >"$BUNDLE_TEST_RUNTIME_LOG"
STUB
chmod +x "$APP/node_modules/.bin/next"
CONTENT_STUDIO_APP_ROOT=$APP \
HOSTNAME=127.0.0.1 \
PORT=13500 \
BUNDLE_TEST_RUNTIME_LOG=$RUN_LOG \
  bash "$ROOT/deploy/content-studio-start.sh"
grep -Fq 'legacy|start -H 127.0.0.1 -p 13500' "$RUN_LOG" \
  || fail "Content Studio launcher did not preserve legacy rollback compatibility"

printf '[commerce-release-bundle-test] compact bundle, digest, and launcher checks passed\n'
