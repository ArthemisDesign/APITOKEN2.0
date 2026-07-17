#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT
for fixture in baseline appended tampered; do
  mkdir -p "$TEMP/$fixture/packages/db"
  cp -R -- "$ROOT/packages/db/migrations" "$TEMP/$fixture/packages/db/migrations"
done

wd_migration_manifest "$TEMP/baseline" >"$TEMP/baseline.manifest"

node - "$TEMP/appended/packages/db/migrations/meta/_journal.json" "$TEMP/appended/packages/db/migrations/0012_watchdog_manifest_test.sql" <<'NODE'
const fs = require("node:fs");
const [journalPath, sqlPath] = process.argv.slice(2);
const journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
const previous = journal.entries.at(-1);
journal.entries.push({
  idx: journal.entries.length,
  version: journal.version,
  when: previous.when + 1,
  tag: "0012_watchdog_manifest_test",
  breakpoints: true,
});
fs.writeFileSync(journalPath, `${JSON.stringify(journal, null, 2)}\n`);
fs.writeFileSync(sqlPath, "CREATE TABLE watchdog_manifest_test (id integer PRIMARY KEY);\n");
NODE
wd_migration_manifest "$TEMP/appended" >"$TEMP/appended.manifest"
wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/appended.manifest"
[[ $(wd_manifest_digest "$TEMP/baseline.manifest") != $(wd_manifest_digest "$TEMP/appended.manifest") ]]

printf '\n-- forbidden historical edit\n' >>"$TEMP/tampered/packages/db/migrations/0000_curved_skrulls.sql"
wd_migration_manifest "$TEMP/tampered" >"$TEMP/tampered.manifest"
if wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/tampered.manifest"; then
  wd_die "manifest accepted an edited historical SQL migration"
fi

node - "$TEMP/tampered/packages/db/migrations/meta/_journal.json" <<'NODE'
const fs = require("node:fs");
const journalPath = process.argv[2];
const journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
journal.entries[0].when += 1;
fs.writeFileSync(journalPath, JSON.stringify(journal));
NODE
wd_migration_manifest "$TEMP/tampered" >"$TEMP/tampered-journal.manifest"
if wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/tampered-journal.manifest"; then
  wd_die "manifest accepted an edited historical journal entry"
fi

wd_engine_topology_is_steady 1 1 1 1 0 0 0 0 0 0
wd_engine_topology_is_steady 0 0 0 0 1 1 1 1 0 0
for invalid_topology in \
  "1 1 1 1 1 1 1 1 0 0" \
  "1 1 1 0 0 0 0 1 0 0" \
  "1 0 1 1 0 0 0 0 0 0" \
  "1 1 0 1 0 0 0 0 0 0" \
  "1 1 1 1 0 0 0 0 1 0" \
  "1 1 1 1 0 0 0 0 0 1"; do
  # shellcheck disable=SC2086 -- each fixture intentionally expands to ten arguments.
  if wd_engine_topology_is_steady $invalid_topology; then
    wd_die "engine topology accepted an invalid steady state: $invalid_topology"
  fi
done

printf 'watchdog migration and engine topology tests passed\n'
