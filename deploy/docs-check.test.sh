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

# The repository contract supports the macOS system Python 3.9 used by agent-merge; importing and
# executing an archive snapshot must not require the Python 3.12-only extractall(filter=) keyword.
grep -Fq 'bundle.extractall(destination)' "$CHECK" || fail 'Python 3.9 extraction path is absent'

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

# A Drizzle migration necessarily modifies its existing journal. Accept only an exact tail append
# whose new tag has a matching newly-added SQL file; historical SQL and journal entries stay frozen.
MIGRATION_REPO=$TEMP/migration-repo
git init --quiet "$MIGRATION_REPO"
git -C "$MIGRATION_REPO" config user.name test
git -C "$MIGRATION_REPO" config user.email test@example.invalid
mkdir -p "$MIGRATION_REPO/deploy" "$MIGRATION_REPO/docs/sales" \
  "$MIGRATION_REPO/docs/ops" "$MIGRATION_REPO/observability/prometheus/rules" \
  "$MIGRATION_REPO/packages/sales-db/migrations/meta"
cp "$CHECK" "$MIGRATION_REPO/deploy/docs-check.py"
printf '# docs\n\n- [Sales](sales/SALES_PORTAL.md)\n- [Monitoring](ops/MONITORING.md)\n' \
  >"$MIGRATION_REPO/docs/README.md"
printf '# Sales\n' >"$MIGRATION_REPO/docs/sales/SALES_PORTAL.md"
printf '# Monitoring\n' >"$MIGRATION_REPO/docs/ops/MONITORING.md"
printf 'CREATE TABLE base (id integer PRIMARY KEY);\n' \
  >"$MIGRATION_REPO/packages/sales-db/migrations/0000_base.sql"
printf '%s\n' \
  '{"version":"7","dialect":"postgresql","entries":[{"idx":0,"version":"7","when":1,"tag":"0000_base","breakpoints":true}]}' \
  >"$MIGRATION_REPO/packages/sales-db/migrations/meta/_journal.json"
git -C "$MIGRATION_REPO" add deploy/docs-check.py docs/README.md \
  docs/sales/SALES_PORTAL.md docs/ops/MONITORING.md \
  packages/sales-db/migrations/0000_base.sql \
  packages/sales-db/migrations/meta/_journal.json
git -C "$MIGRATION_REPO" commit --quiet -m base
MIGRATION_BASE=$(git -C "$MIGRATION_REPO" rev-parse HEAD)

printf 'CREATE TABLE appended (id integer PRIMARY KEY);\n' \
  >"$MIGRATION_REPO/packages/sales-db/migrations/0001_appended.sql"
python3 - "$MIGRATION_REPO/packages/sales-db/migrations/meta/_journal.json" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
journal = json.loads(path.read_text())
journal["entries"].append({
    "idx": 1,
    "version": "7",
    "when": 2,
    "tag": "0001_appended",
    "breakpoints": True,
})
path.write_text(json.dumps(journal) + "\n")
PY
printf '\nNew migration.\n' >>"$MIGRATION_REPO/docs/sales/SALES_PORTAL.md"
git -C "$MIGRATION_REPO" add docs/sales/SALES_PORTAL.md \
  packages/sales-db/migrations/0001_appended.sql \
  packages/sales-db/migrations/meta/_journal.json
git -C "$MIGRATION_REPO" commit --quiet -m append
MIGRATION_APPEND=$(git -C "$MIGRATION_REPO" rev-parse HEAD)
DOCS_CHECK_ROOT="$MIGRATION_REPO" python3 "$MIGRATION_REPO/deploy/docs-check.py" \
  "$MIGRATION_BASE" "$MIGRATION_APPEND" >/dev/null

printf '\n-- forbidden edit\n' >>"$MIGRATION_REPO/packages/sales-db/migrations/0000_base.sql"
git -C "$MIGRATION_REPO" add packages/sales-db/migrations/0000_base.sql
git -C "$MIGRATION_REPO" commit --quiet -m tamper-sql
MIGRATION_SQL_TAMPER=$(git -C "$MIGRATION_REPO" rev-parse HEAD)
output=$(DOCS_CHECK_ROOT="$MIGRATION_REPO" python3 "$MIGRATION_REPO/deploy/docs-check.py" \
  "$MIGRATION_APPEND" "$MIGRATION_SQL_TAMPER" 2>&1) && fail 'historical SQL edit passed'
grep -Fq 'existing migration is immutable' <<<"$output" \
  || fail "missing immutable SQL diagnostic: $output"

python3 - "$MIGRATION_REPO/packages/sales-db/migrations/meta/_journal.json" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
journal = json.loads(path.read_text())
journal["entries"][0]["when"] = 99
path.write_text(json.dumps(journal) + "\n")
PY
git -C "$MIGRATION_REPO" add packages/sales-db/migrations/meta/_journal.json
git -C "$MIGRATION_REPO" commit --quiet -m tamper-journal
MIGRATION_JOURNAL_TAMPER=$(git -C "$MIGRATION_REPO" rev-parse HEAD)
output=$(DOCS_CHECK_ROOT="$MIGRATION_REPO" python3 "$MIGRATION_REPO/deploy/docs-check.py" \
  "$MIGRATION_SQL_TAMPER" "$MIGRATION_JOURNAL_TAMPER" 2>&1) \
  && fail 'historical journal edit passed'
grep -Fq 'migration journal is not append-only' <<<"$output" \
  || fail "missing append-only journal diagnostic: $output"

printf 'docs-check.test: passed\n'
