#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d)
FIXTURE="$TEMP/workspace"
trap 'rm -rf -- "$TEMP"' EXIT

mkdir -p "$FIXTURE/apps/app" "$FIXTURE/packages/core" "$FIXTURE/packages/isolated" \
  "$FIXTURE/packages/util" "$FIXTURE/docs"
git init --quiet "$FIXTURE"
git -C "$FIXTURE" config user.name typescript-scope-test
git -C "$FIXTURE" config user.email typescript-scope-test@example.invalid

printf '%s\n' '{"private":true,"packageManager":"pnpm@9.7.0"}' >"$FIXTURE/package.json"
printf '%s\n' 'packages:' '  - "apps/*"' '  - "packages/*"' >"$FIXTURE/pnpm-workspace.yaml"
printf '%s\n' 'lockfileVersion: "9.0"' >"$FIXTURE/pnpm-lock.yaml"
printf '%s\n' '# fixture' >"$FIXTURE/docs/readme.md"
printf '%s\n' \
  'require("node:fs").appendFileSync(process.env.SCOPE_TEST_LOG, process.env.npm_package_name + "\n");' \
  >"$FIXTURE/probe.cjs"

printf '%s\n' \
  '{"name":"@fixture/core","version":"1.0.0","scripts":{"probe":"node ../../probe.cjs"}}' \
  >"$FIXTURE/packages/core/package.json"
printf '%s\n' \
  '{"name":"@fixture/util","version":"1.0.0","scripts":{"probe":"node ../../probe.cjs"}}' \
  >"$FIXTURE/packages/util/package.json"
printf '%s\n' \
  '{"name":"@fixture/isolated","version":"1.0.0","scripts":{"probe":"node ../../probe.cjs"}}' \
  >"$FIXTURE/packages/isolated/package.json"
printf '%s\n' \
  '{"name":"@fixture/app","version":"1.0.0","dependencies":{"@fixture/core":"workspace:*","@fixture/util":"workspace:*"},"scripts":{"probe":"node ../../probe.cjs"}}' \
  >"$FIXTURE/apps/app/package.json"

git -C "$FIXTURE" add package.json pnpm-workspace.yaml pnpm-lock.yaml probe.cjs docs/readme.md \
  apps/app/package.json packages/core/package.json packages/isolated/package.json \
  packages/util/package.json
git -C "$FIXTURE" commit --quiet -m initial
initial=$(git -C "$FIXTURE" rev-parse HEAD)

scope() {
  node "$ROOT/deploy/typescript-scope.mjs" "$FIXTURE" "$1" "$2"
}

assert_scope() {
  local expected=$1 base=$2 target=$3 actual
  actual=$(scope "$base" "$target")
  [[ $actual == "$expected" ]] || {
    printf 'expected scope:\n%s\nactual scope:\n%s\n' "$expected" "$actual" >&2
    exit 1
  }
}

run_filtered_probe() {
  local base=$1 target=$2 output header package log="$TEMP/probe.log"
  local filters=()
  output=$(scope "$base" "$target")
  header=${output%%$'\n'*}
  [[ $header == filtered ]] || {
    printf 'probe expected a filtered scope, got: %s\n' "$output" >&2
    exit 1
  }
  while IFS= read -r package; do
    [[ $package == filtered || -z $package ]] && continue
    filters+=("--filter=$package")
  done <<<"$output"
  : >"$log"
  SCOPE_TEST_LOG="$log" pnpm --dir "$FIXTURE" "${filters[@]}" \
    -r --workspace-concurrency=1 --if-present --fail-if-no-match probe >/dev/null
  actual=$(sort "$log")
  expected=$(printf '%s\n' "${filters[@]#--filter=}" | sort)
  [[ $actual == "$expected" ]] || {
    printf 'pnpm filters ran unexpected projects:\nexpected:\n%s\nactual:\n%s\n' \
      "$expected" "$actual" >&2
    exit 1
  }
}

printf '%s\n' '// core change' >"$FIXTURE/packages/core/index.ts"
git -C "$FIXTURE" add packages/core/index.ts
git -C "$FIXTURE" commit --quiet -m core
core=$(git -C "$FIXTURE" rev-parse HEAD)
assert_scope $'filtered\n@fixture/app\n@fixture/core\n@fixture/util' "$initial" "$core"
run_filtered_probe "$initial" "$core"

printf '%s\n' '// app change' >"$FIXTURE/apps/app/index.ts"
git -C "$FIXTURE" add apps/app/index.ts
git -C "$FIXTURE" commit --quiet -m app
app=$(git -C "$FIXTURE" rev-parse HEAD)
assert_scope $'filtered\n@fixture/app\n@fixture/core\n@fixture/util' "$core" "$app"

printf '%s\n' '// isolated change' >"$FIXTURE/packages/isolated/index.ts"
git -C "$FIXTURE" add packages/isolated/index.ts
git -C "$FIXTURE" commit --quiet -m isolated
isolated=$(git -C "$FIXTURE" rev-parse HEAD)
assert_scope $'filtered\n@fixture/isolated' "$app" "$isolated"
run_filtered_probe "$app" "$isolated"

printf '%s\n' '# lockfile changed' >>"$FIXTURE/pnpm-lock.yaml"
git -C "$FIXTURE" add pnpm-lock.yaml
git -C "$FIXTURE" commit --quiet -m lockfile
lockfile=$(git -C "$FIXTURE" rev-parse HEAD)
assert_scope full "$isolated" "$lockfile"

rm -f -- "$FIXTURE/packages/core/index.ts"
git -C "$FIXTURE" add packages/core/index.ts
git -C "$FIXTURE" commit --quiet -m deletion
deletion=$(git -C "$FIXTURE" rev-parse HEAD)
assert_scope full "$lockfile" "$deletion"

mkdir -p "$FIXTURE/apps/ghost"
printf '%s\n' '// unknown workspace' >"$FIXTURE/apps/ghost/index.ts"
git -C "$FIXTURE" add apps/ghost/index.ts
git -C "$FIXTURE" commit --quiet -m unknown
unknown=$(git -C "$FIXTURE" rev-parse HEAD)
assert_scope full "$deletion" "$unknown"

printf '%s\n' '# docs only' >>"$FIXTURE/docs/readme.md"
git -C "$FIXTURE" add docs/readme.md
git -C "$FIXTURE" commit --quiet -m docs
docs=$(git -C "$FIXTURE" rev-parse HEAD)
assert_scope none "$unknown" "$docs"

if node "$ROOT/deploy/typescript-scope.mjs" "$FIXTURE" "$initial" "$unknown" \
  >/dev/null 2>&1; then
  printf 'scope selector accepted a target other than the checked-out HEAD\n' >&2
  exit 1
fi

printf 'typescript-scope.test: ok\n'
