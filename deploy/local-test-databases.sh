#!/usr/bin/env bash
set -euo pipefail

# Local developer helper for the compose.yaml Postgres instance. Production and the watchdog
# use the same topology (one Postgres, extra databases). This script never talks to the host.

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
COMPOSE_FILE=$ROOT/compose.yaml
LOCAL_USER=commerce
LOCAL_COMMERCE_DSN='postgresql://commerce:commerce-local-only@127.0.0.1:5433/commerce'
LOCAL_SALES_DSN='postgresql://commerce:commerce-local-only@127.0.0.1:5433/sales'
LOCAL_OPENKEYS_DSN='postgresql://commerce:commerce-local-only@127.0.0.1:5433/openkeys'

die() { printf '[local-test-databases] ERROR: %s\n' "$*" >&2; exit 1; }
log() { printf '[local-test-databases] %s\n' "$*"; }

[[ -f $COMPOSE_FILE && ! -L $COMPOSE_FILE ]] || die "compose.yaml is missing"

compose() {
  docker compose -f "$COMPOSE_FILE" "$@"
}

postgres_ready() {
  compose exec -T commerce-postgres pg_isready -U "$LOCAL_USER" -d commerce >/dev/null 2>&1
}

listening_postgres_container() {
  docker ps --filter publish=5433 --format '{{.ID}}' | head -n1
}

tcp_postgres_ready() {
  local container=$1
  [[ -n $container ]] || return 1
  docker exec -i "$container" pg_isready -U "$LOCAL_USER" -d commerce >/dev/null 2>&1
}

psql_on() {
  local container=$1
  shift
  docker exec -i "$container" psql -U "$LOCAL_USER" "$@"
}

wait_for_postgres() {
  local _
  for _ in $(seq 1 30); do
    postgres_ready && return 0
    sleep 1
  done
  die "compose.yaml commerce-postgres did not become ready on 127.0.0.1:5433"
}

ensure_database() {
  local name=$1 container=$2 exists
  [[ $name == sales || $name == openkeys ]] \
    || die "refusing to create unexpected database: $name"
  exists=$(psql_on "$container" -d postgres -Atqc "SELECT 1 FROM pg_database WHERE datname='$name'") \
    || die "could not inspect local compose Postgres"
  if [[ $exists == 1 ]]; then
    log "database $name already exists"
    return 0
  fi
  log "creating database $name"
  psql_on "$container" -d postgres -v ON_ERROR_STOP=1 -c "CREATE DATABASE $name" \
    >/dev/null
}

ensure() {
  local container
  command -v docker >/dev/null || die "docker is required to create the local sales/openkeys databases"
  [[ -f $COMPOSE_FILE ]] || die "compose.yaml is missing"
  container=$(listening_postgres_container)
  if tcp_postgres_ready "$container"; then
    log "reusing the Postgres already listening on 127.0.0.1:5433"
  else
    log "starting compose.yaml commerce-postgres"
    if ! compose up -d commerce-postgres >/dev/null; then
      die "could not bind 127.0.0.1:5433; stop the other listener or reuse a healthy local commerce Postgres"
    fi
    wait_for_postgres
    container=$(listening_postgres_container)
    [[ -n $container ]] || die "compose.yaml commerce-postgres did not publish 127.0.0.1:5433"
  fi
  ensure_database sales "$container"
  ensure_database openkeys "$container"
}

uses_local_compose_dsn() {
  [[ ${TEST_DATABASE_URL:-} == "$LOCAL_COMMERCE_DSN" \
    || ${TEST_SALES_DATABASE_URL:-} == "$LOCAL_SALES_DSN" \
    || ${TEST_OPENKEYS_DATABASE_URL:-} == "$LOCAL_OPENKEYS_DSN" ]]
}

integration_test() {
  [[ -n ${TEST_DATABASE_URL:-} ]] \
    || die "TEST_DATABASE_URL is required (commerce Postgres DSN; see AGENTS.md)"
  [[ -n ${TEST_SALES_DATABASE_URL:-} ]] \
    || die "TEST_SALES_DATABASE_URL is required (sales database on the same local Postgres; see AGENTS.md)"
  if uses_local_compose_dsn; then
    ensure
  fi
  log "migrating commerce"
  DATABASE_URL="$TEST_DATABASE_URL" pnpm --dir "$ROOT" --filter @claude-api/db db:migrate
  log "migrating sales"
  SALES_DATABASE_URL="$TEST_SALES_DATABASE_URL" pnpm --dir "$ROOT" \
    --filter @claude-api/sales-db db:migrate
  log "running commerce integration tests"
  pnpm --dir "$ROOT" --filter @claude-api/db test:integration
  pnpm --dir "$ROOT" --filter @claude-api/commercial-api test
  log "running sales money tests"
  pnpm --dir "$ROOT" --filter @claude-api/sales-db test
  pnpm --dir "$ROOT" --filter @claude-api/sales-api test
  log "commerce and sales integration tests passed"
}

[[ $# -eq 1 ]] || die "usage: $0 ensure|integration-test"
case "$1" in
  ensure) ensure ;;
  integration-test) integration_test ;;
  *) die "usage: $0 ensure|integration-test" ;;
esac
