#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
CHECK=$ROOT/deploy/docs-check.py
fail() { printf 'docs-check.test: %s\n' "$*" >&2; exit 1; }

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT
REPO=$TEMP/repo
git init --quiet "$REPO"
git -C "$REPO" config user.name test
git -C "$REPO" config user.email test@example.invalid
mkdir -p "$REPO/deploy" "$REPO/docs/engine" "$REPO/docs/commerce" "$REPO/docs/sales" \
  "$REPO/docs/ops" "$REPO/observability/prometheus/rules" "$REPO/crates/server/src" \
  "$REPO/crates/metering/src" "$REPO/packages/payments/src" "$REPO/apps/api/src"
cp "$CHECK" "$REPO/deploy/docs-check.py"
printf '# docs\n\n- [Dependencies](DEPENDENCIES.md)\n- [CONTROL](engine/CONTROL_API.md)\n- [Pricing](commerce/PRICING_MODEL.md)\n- [Pay](commerce/PAY.md)\n- [Sales](sales/SALES_PORTAL.md)\n- [Monitoring](ops/MONITORING.md)\n' >"$REPO/docs/README.md"
printf '# Dependencies\n' >"$REPO/docs/DEPENDENCIES.md"
printf '# Control\n\n## Route\n' >"$REPO/docs/engine/CONTROL_API.md"
printf '# Pricing\n' >"$REPO/docs/commerce/PRICING_MODEL.md"
printf '# Pay\n' >"$REPO/docs/commerce/PAY.md"
printf '# Sales\n' >"$REPO/docs/sales/SALES_PORTAL.md"
printf '# Monitoring\n\n## Healthy\n' >"$REPO/docs/ops/MONITORING.md"
printf "groups:\n- rules:\n  - alert: Healthy\n    annotations:\n      runbook: 'docs/ops/MONITORING.md#healthy'\n" \
  >"$REPO/observability/prometheus/rules/application.yml"
printf 'base\n' >"$REPO/crates/server/src/http.rs"
printf 'base\n' >"$REPO/crates/server/src/admin.rs"
printf 'base\n' >"$REPO/crates/metering/src/lib.rs"
printf 'base\n' >"$REPO/packages/payments/src/pay.ts"
printf 'base\n' >"$REPO/apps/api/src/sales-feed.controller.ts"
git -C "$REPO" add .
git -C "$REPO" commit --quiet -m base
BASE=$(git -C "$REPO" rev-parse HEAD)

run_check() { (cd "$REPO" && DOCS_CHECK_ROOT="$REPO" python3 deploy/docs-check.py "$1" "$2"); }
commit_file() { # $1 path $2 content $3 message
  printf '%s\n' "$2" >>"$REPO/$1"
  git -C "$REPO" add "$1"
  git -C "$REPO" commit --quiet -m "$3"
  git -C "$REPO" rev-parse HEAD
}
expect_failure() { # $1 base $2 head $3 expected
  local output status=0
  output=$(run_check "$1" "$2" 2>&1) || status=$?
  (( status == 1 )) || fail "expected failure, got $status: $output"
  grep -Fq "$3" <<<"$output" || fail "missing diagnostic '$3': $output"
}

run_check "$BASE" "$BASE" >/dev/null

UNRELATED=$(commit_file docs/commerce/PAY.md unrelated unrelated-doc)
CONTROL=$(commit_file crates/server/src/http.rs contract control-with-unrelated-doc)
expect_failure "$BASE" "$CONTROL" 'Control API surface changed without required documentation'

commit_file docs/engine/CONTROL_API.md route control-owner >/dev/null
CONTROL_DOC=$(commit_file docs/DEPENDENCIES.md control-link control-dependencies)
run_check "$BASE" "$CONTROL_DOC" >/dev/null

BROKEN_LINK=$(commit_file docs/engine/CONTROL_API.md '[broken](missing.md)' broken-link)
expect_failure "$CONTROL_DOC" "$BROKEN_LINK" 'missing link target'

printf '# Control\n\n[bad](#missing)\n' >"$REPO/docs/engine/CONTROL_API.md"
git -C "$REPO" add docs/engine/CONTROL_API.md
git -C "$REPO" commit --quiet -m broken-anchor
BROKEN_ANCHOR=$(git -C "$REPO" rev-parse HEAD)
expect_failure "$BROKEN_LINK" "$BROKEN_ANCHOR" 'missing Markdown anchor #missing'

printf '# Extra\n' >"$REPO/docs/engine/EXTRA.md"
git -C "$REPO" add docs/engine/EXTRA.md
git -C "$REPO" commit --quiet -m unindexed
UNINDEXED=$(git -C "$REPO" rev-parse HEAD)
expect_failure "$BROKEN_ANCHOR" "$UNINDEXED" 'docs/README.md does not index docs/engine/EXTRA.md'

printf "\n  - alert: Broken\n    annotations:\n      runbook: 'docs/ops/MONITORING.md#broken'\n" \
  >>"$REPO/observability/prometheus/rules/application.yml"
git -C "$REPO" add observability/prometheus/rules/application.yml
git -C "$REPO" commit --quiet -m broken-runbook
BROKEN_RUNBOOK=$(git -C "$REPO" rev-parse HEAD)
expect_failure "$UNINDEXED" "$BROKEN_RUNBOOK" 'runbook anchor #broken is absent'

printf 'docs-check.test: passed\n'
