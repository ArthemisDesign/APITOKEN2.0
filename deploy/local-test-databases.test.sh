#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
RUNNER=$ROOT/deploy/local-test-databases.sh

fail() { printf 'local-test-databases.test: %s\n' "$*" >&2; exit 1; }

[[ -x $RUNNER ]] || chmod +x "$RUNNER"

output=$(mktemp)
trap 'rm -f -- "$output"' EXIT

if env -u TEST_DATABASE_URL -u TEST_SALES_DATABASE_URL \
  bash "$RUNNER" integration-test >"$output" 2>&1; then
  fail 'integration-test succeeded without TEST_DATABASE_URL'
fi
grep -Fq 'TEST_DATABASE_URL is required' "$output" \
  || fail 'missing TEST_DATABASE_URL did not fail closed'

if env -u TEST_SALES_DATABASE_URL TEST_DATABASE_URL=postgresql://commerce-test \
  bash "$RUNNER" integration-test >"$output" 2>&1; then
  fail 'integration-test succeeded without TEST_SALES_DATABASE_URL'
fi
grep -Fq 'TEST_SALES_DATABASE_URL is required' "$output" \
  || fail 'missing TEST_SALES_DATABASE_URL did not fail closed'

if bash "$RUNNER" >/dev/null 2>"$output"; then
  fail 'runner accepted a missing command'
fi
grep -Fq 'usage:' "$output" || fail 'runner lost its usage error'

if bash "$RUNNER" unexpected >/dev/null 2>"$output"; then
  fail 'runner accepted an unknown command'
fi
grep -Fq 'usage:' "$output" || fail 'unknown command lost its usage error'

grep -Fq 'CREATE DATABASE sales' "$ROOT/deploy/local-postgres-init.sql" \
  || fail 'fresh-volume init SQL no longer creates sales'
grep -Fq 'CREATE DATABASE openkeys' "$ROOT/deploy/local-postgres-init.sql" \
  || fail 'fresh-volume init SQL no longer creates openkeys'
grep -Fq 'deploy/local-postgres-init.sql' "$ROOT/compose.yaml" \
  || fail 'compose.yaml no longer mounts the extra-database init SQL'
grep -Fq 'reusing the Postgres already listening on 127.0.0.1:5433' "$RUNNER" \
  || fail 'ensure no longer reuses a healthy listener on 5433'
grep -Fq 'docker exec -i' "$RUNNER" \
  || fail 'ensure no longer talks to a published local Postgres without a TTY'

printf 'local-test-databases.test: ok\n'
