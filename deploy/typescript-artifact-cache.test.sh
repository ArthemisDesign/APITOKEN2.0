#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
RUNNER=$ROOT/deploy/typescript-build-contexts.sh
TEMP=$(mktemp -d)
FIXTURE=$TEMP/repo
CACHE=$TEMP/cache
STATE=$TEMP/state
trap 'find "$TEMP" -type d -exec chmod u+w {} + 2>/dev/null || true; rm -rf "$TEMP"' EXIT

fail() { printf 'typescript-artifact-cache.test: %s\n' "$*" >&2; exit 1; }
cache_entry_matching() {
  local root=$1 relative=$2 expected=$3 entry
  for entry in "$root"/[0-9a-f]*; do
    [[ -d $entry && ! -L $entry ]] || continue
    cmp -s "$entry/$relative" "$expected" || continue
    printf '%s\n' "$entry"
    return 0
  done
  return 1
}

git clone --quiet --no-hardlinks "$ROOT" "$FIXTURE"
if ! cmp -s "$RUNNER" "$FIXTURE/deploy/typescript-build-contexts.sh"; then
  cp "$RUNNER" "$FIXTURE/deploy/typescript-build-contexts.sh"
  git -C "$FIXTURE" add deploy/typescript-build-contexts.sh
  git -C "$FIXTURE" -c user.name=test -c user.email=test@example.invalid \
    commit --quiet -m 'test current artifact cache helper'
fi

mkdir -p "$TEMP/bin" "$STATE"
cat >"$TEMP/bin/pnpm" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --version && $# -eq 1 ]]; then
  printf '10.0.0-test\n'
  exit 0
fi

args=" $* "
workspace=
previous=
for argument in "$@"; do
  if [[ $previous == --dir ]]; then workspace=$argument; break; fi
  previous=$argument
done
[[ -n $workspace ]] || { printf 'stub received no workspace\n' >&2; exit 80; }
state=${ARTIFACT_CACHE_TEST_STATE:?}

if [[ $args == *" --filter=@claude-api/commercial-api "* ]]; then
  label=commerce
elif [[ $args == *" --filter=@claude-api/sales-api "* ]]; then
  label=sales
elif [[ $args == *" --filter=@claude-api/openkeys "* ]]; then
  label=openkeys
elif [[ $args == *" --filter=@claude-api/web "* ]]; then
  label=web
else
  label=shared
fi

if [[ -n ${ONLY_BUILD_LABEL:-} && $label != "$ONLY_BUILD_LABEL" ]]; then
  printf 'unexpected build label: %s\n' "$label" >&2
  exit 81
fi
[[ ${FAIL_IF_BUILD_CALLED:-0} == 0 ]] \
  || { printf 'build unexpectedly ran: %s\n' "$label" >&2; exit 82; }
printf '%s\n' "$label" >>"$state/builds"

write_output() {
  local relative=$1 content=$2
  mkdir -p "$(dirname -- "$workspace/$relative")"
  printf '%s\n' "$content" >"$workspace/$relative"
}

if [[ $label == shared ]]; then
  for package in contracts db engine-client payments sales-db openkeys-db; do
    [[ $args == *" --filter=@claude-api/$package "* ]] || continue
    write_output "packages/$package/dist/index.js" "$package"
    case "$package" in
      db|sales-db|openkeys-db)
        write_output "packages/$package/dist/migrate.js" "$package migration"
        ;;
    esac
  done
elif [[ $label == commerce ]]; then
  write_output apps/api/dist/main.js api
  write_output apps/worker/dist/main.js worker
  write_output apps/content-studio/.next/BUILD_ID content-build
  write_output apps/content-studio/.next/server/runtime.js content-runtime
  write_output apps/content-studio/.next/cache/compiler.bin content-compiler-cache
  mkdir -p "$workspace/apps/content-studio/.next/standalone/node_modules/.pnpm/node_modules"
  ln -s ../semver@6.3.1/node_modules/semver \
    "$workspace/apps/content-studio/.next/standalone/node_modules/.pnpm/node_modules/semver"
elif [[ $label == sales ]]; then
  write_output apps/sales-api/dist/main.js sales-api
  write_output apps/sales-web/.next/BUILD_ID sales-build
  write_output apps/sales-web/.next/server/runtime.js sales-runtime
  write_output apps/sales-web/.next/cache/compiler.bin sales-compiler-cache
elif [[ $label == openkeys ]]; then
  write_output apps/openkeys/.next/BUILD_ID openkeys-build
  write_output apps/openkeys/.next/server/runtime.js openkeys-runtime
  write_output apps/openkeys/.next/cache/compiler.bin openkeys-compiler-cache
  write_output node_modules/test-package/index.js installed-dependency
  mkdir -p "$workspace/apps/openkeys/.next/node_modules"
  ln -s ../../../../node_modules/test-package \
    "$workspace/apps/openkeys/.next/node_modules/test-package"
else
  write_output apps/web/.next/BUILD_ID "$(git -C "$workspace" rev-parse HEAD)"
  write_output apps/web/.next/server/runtime.js web-runtime
  write_output apps/web/.next/cache/compiler.bin web-compiler-cache
  if [[ ${INJECT_DANGLING_ESCAPE_LINK:-0} == 1 ]]; then
    mkdir -p "$workspace/node_modules"
    ln -s /artifact-cache-test-outside/missing \
      "$workspace/node_modules/cache-test-escape"
    ln -s ../../../../node_modules/cache-test-escape \
      "$workspace/apps/web/.next/server/escape"
  fi
fi
STUB
chmod +x "$TEMP/bin/pnpm"

run_cache() {
  PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" \
    TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" TYPESCRIPT_ARTIFACT_CACHE_KEEP="${CACHE_KEEP:-6}" \
    bash "$RUNNER" "$FIXTURE" "$@"
}

run_cache commerce sales openkeys web >"$STATE/cold.log"
[[ $(wc -l <"$STATE/builds" | tr -d ' ') == 5 ]] \
  || fail 'cold cache did not build the shared lane and four contexts exactly once'
for context in commerce sales openkeys web; do
  if [[ $(find "$CACHE/$context" -mindepth 1 -maxdepth 1 -type d ! -name '.*' | wc -l \
    | tr -d ' ') != 1 ]]; then
    cat "$STATE/cold.log" >&2
    fail "$context cache entry was not published"
  fi
done
if find "$CACHE" -path '*/.next/cache/*' -print -quit | grep -q .; then
  fail 'complete artifact entry duplicated the incremental Next cache'
fi
if find "$CACHE" -type l -print -quit | grep -q .; then
  fail 'workspace-relative link metadata leaked into the cache tree'
fi

: >"$STATE/builds"
if ! PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" FAIL_IF_BUILD_CALLED=1 \
  TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" \
  bash "$RUNNER" "$FIXTURE" commerce sales openkeys web >"$STATE/warm.log"; then
  cat "$STATE/warm.log" >&2
  fail 'warm exact cache command failed'
fi
[[ ! -s $STATE/builds ]] || fail 'warm exact cache ran a build'
[[ -f $FIXTURE/apps/web/.next/server/runtime.js ]] \
  || fail 'warm cache did not restore a complete Next output'
[[ -f $FIXTURE/apps/web/.next/cache/compiler.bin ]] \
  || fail 'complete artifact restore discarded the independent Next compiler cache'
[[ -L $FIXTURE/apps/openkeys/.next/node_modules/test-package ]] \
  || fail 'safe workspace-relative output link was not restored'
commerce_dangling=$FIXTURE/apps/content-studio/.next/standalone/node_modules/.pnpm/node_modules/semver
[[ -L $commerce_dangling ]] \
  || fail 'safe dangling standalone link was not restored'
[[ $(readlink "$commerce_dangling") == ../semver@6.3.1/node_modules/semver ]] \
  || fail 'safe dangling standalone link target changed during restore'

printf '\n' >>"$FIXTURE/apps/web/package.json"
git -C "$FIXTURE" add apps/web/package.json
git -C "$FIXTURE" -c user.name=test -c user.email=test@example.invalid \
  commit --quiet -m 'change only web input'
: >"$STATE/builds"
PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" ONLY_BUILD_LABEL=web \
  TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" \
  bash "$RUNNER" "$FIXTURE" commerce sales openkeys web >/dev/null
[[ $(cat "$STATE/builds") == web ]] \
  || fail 'a component-only input change rebuilt an unrelated context'

web_entry=$(cache_entry_matching "$CACHE/web" apps/web/.next/BUILD_ID \
  "$FIXTURE/apps/web/.next/BUILD_ID")
[[ -n $web_entry ]] || fail 'could not select the latest web cache entry'
printf 'corrupt\n' >"$web_entry/apps/web/.next/BUILD_ID"
: >"$STATE/builds"
PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" ONLY_BUILD_LABEL=web \
  TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" \
  bash "$RUNNER" "$FIXTURE" web >/dev/null
[[ $(cat "$STATE/builds") == web ]] || fail 'a corrupt entry did not rebuild'

web_entry=$(cache_entry_matching "$CACHE/web" apps/web/.next/BUILD_ID \
  "$FIXTURE/apps/web/.next/BUILD_ID")
rm "$web_entry/apps/web/.next/server/runtime.js"
ln -s /etc/passwd "$web_entry/apps/web/.next/server/runtime.js"
: >"$STATE/builds"
PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" ONLY_BUILD_LABEL=web \
  TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" \
  bash "$RUNNER" "$FIXTURE" web >/dev/null
[[ $(cat "$STATE/builds") == web ]] || fail 'a symlink-injected entry did not rebuild'

web_entries_before=$(find "$CACHE/web" -mindepth 1 -maxdepth 1 -type d ! -name '.*' \
  | wc -l | tr -d ' ')
: >"$STATE/builds"
PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" ONLY_BUILD_LABEL=web \
  INJECT_DANGLING_ESCAPE_LINK=1 NEXT_PUBLIC_DOCS_URL=https://escape.example.invalid \
  TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" \
  bash "$RUNNER" "$FIXTURE" web >"$STATE/escape.log"
[[ $(cat "$STATE/builds") == web ]] || fail 'unsafe dangling link did not run the normal build'
web_entries_after=$(find "$CACHE/web" -mindepth 1 -maxdepth 1 -type d ! -name '.*' \
  | wc -l | tr -d ' ')
[[ $web_entries_after == "$web_entries_before" ]] \
  || fail 'a dangling link through an external intermediary was cached'
grep -q 'path escaped cache root.*continuing without cache save' "$STATE/escape.log" \
  || fail 'unsafe dangling-link cache rejection was not reported'
rm -- "$FIXTURE/apps/web/.next/server/escape" "$FIXTURE/node_modules/cache-test-escape"

for version in one two three; do
  : >"$STATE/builds"
  PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" ONLY_BUILD_LABEL=web \
    NEXT_PUBLIC_DOCS_URL="https://$version.example.invalid" \
    TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" TYPESCRIPT_ARTIFACT_CACHE_KEEP=2 \
    bash "$RUNNER" "$FIXTURE" web >/dev/null
  [[ $(cat "$STATE/builds") == web ]] || fail 'build environment did not invalidate the cache key'
done
[[ $(find "$CACHE/web" -mindepth 1 -maxdepth 1 -type d ! -name '.*' | wc -l \
  | tr -d ' ') == 2 ]] || fail 'content cache retention was not bounded'

: >"$STATE/builds"
PATH="$TEMP/bin:$PATH" ARTIFACT_CACHE_TEST_STATE="$STATE" FAIL_IF_BUILD_CALLED=1 \
  NEXT_PUBLIC_DOCS_URL=https://three.example.invalid \
  TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CACHE" TYPESCRIPT_ARTIFACT_CACHE_KEEP=2 \
  bash "$RUNNER" "$FIXTURE" web >/dev/null
[[ ! -s $STATE/builds ]] || fail 'same build environment did not reuse its exact cache entry'

printf 'typescript-artifact-cache.test: ok\n'
