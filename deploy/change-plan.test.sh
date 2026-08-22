#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
PLAN=$ROOT/deploy/change-plan.sh
fail() { printf 'change-plan.test: %s\n' "$*" >&2; exit 1; }

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT
REPO=$TEMP/repo
git init --quiet "$REPO"
git -C "$REPO" config user.name test
git -C "$REPO" config user.email test@example.invalid
mkdir -p "$REPO/deploy"
cp "$ROOT/deploy/watchdog-lib.sh" "$REPO/deploy/watchdog-lib.sh"
cp "$PLAN" "$REPO/deploy/change-plan.sh"
printf 'base\n' >"$REPO/README.md"
git -C "$REPO" add deploy README.md
git -C "$REPO" commit --quiet -m base
BASE=$(git -C "$REPO" rev-parse HEAD)

run_plan() { (cd "$REPO" && bash deploy/change-plan.sh "$@"); }

mkdir -p "$REPO/docs"
printf 'docs\n' >"$REPO/docs/known.md"
git -C "$REPO" add docs/known.md
git -C "$REPO" commit --quiet -m docs
DOCS=$(git -C "$REPO" rev-parse HEAD)
output=$(run_plan --base "$BASE" --head "$DOCS")
grep -Fq 'lanes static=1 typescript=0 typescript_full=0 rust=0 deployment=0 unknown=0' <<<"$output" \
  || fail "documentation plan selected wrong lanes: $output"
grep -Fq 'docs/known.md [validation-neutral]' <<<"$output" \
  || fail "documentation path classification missing: $output"

mkdir -p "$REPO/apps/example"
printf 'typescript\n' >"$REPO/apps/example/index.ts"
git -C "$REPO" add apps/example/index.ts
git -C "$REPO" commit --quiet -m typescript
TS=$(git -C "$REPO" rev-parse HEAD)
output=$(run_plan --base "$DOCS" --head "$TS" --format json)
python3 - "$output" <<'PY'
import json, sys
plan=json.loads(sys.argv[1])
assert plan["format_version"] == 1
assert plan["lanes"] == {
    "deployment": False,
    "rust": False,
    "static": True,
    "typescript": True,
    "typescript_full": False,
    "unknown": False,
}
assert plan["typescript_components"] == ["commerce", "sales", "openkeys", "web", "admin", "devbot"]
assert plan["paths"] == ["apps/example/index.ts"]
PY

mkdir -p "$REPO/mystery"
printf 'unknown\n' >"$REPO/mystery/runtime.xyz"
git -C "$REPO" add mystery/runtime.xyz
git -C "$REPO" commit --quiet -m unknown
UNKNOWN=$(git -C "$REPO" rev-parse HEAD)
output=$(run_plan --base "$TS" --head "$UNKNOWN")
grep -Fq 'lanes static=1 typescript=1 typescript_full=1 rust=1 deployment=1 unknown=1' <<<"$output" \
  || fail "unknown path did not fail closed: $output"
grep -Fq 'mystery/runtime.xyz [unknown]' <<<"$output" \
  || fail "unknown path classification missing: $output"

status=0
run_plan --base missing --head "$UNKNOWN" >"$TEMP/bad.out" 2>&1 || status=$?
(( status == 2 )) || fail "missing base returned $status"
grep -Fq 'base does not resolve to a commit' "$TEMP/bad.out" \
  || fail "missing base diagnostic is unclear"

printf 'change-plan.test: passed\n'
