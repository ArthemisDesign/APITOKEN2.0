#!/usr/bin/env bash
set -euo pipefail

# Root-owned helper used by the watchdog. Candidate code never receives Docker access; it only
# receives credentials for this disposable loopback-only PostgreSQL container.

IMAGE=${WATCHDOG_POSTGRES_IMAGE:-postgres:18-alpine}
# Must stay BELOW the kernel ephemeral range (net.ipv4.ip_local_port_range, 32768-60999 here).
# The previous 55432 sat inside it, so any outbound loopback connection could be handed that exact
# source port and the container then failed to bind with "address already in use". That is what
# quarantined d3a3698 twice on 2026-07-25: Caddy's keep-alive to engine slot 8787 held
# 127.0.0.1:55432 while the test database tried to publish it.
BASE_PORT=${WATCHDOG_POSTGRES_PORT:-15432}
USER_NAME=watchdog
DATABASE=watchdog
ENGINE_DATABASE=watchdog_engine
SALES_DATABASE=watchdog_sales
OPENKEYS_DATABASE=watchdog_openkeys
PASSWORD=watchdog-local-disposable-only

die() {
  printf '[watchdog-test-db] ERROR: %s\n' "$*" >&2
  exit 1
}

[[ ${EUID:-$(id -u)} -eq 0 ]] || die "must run as root"
[[ $# -ge 1 && $# -le 2 ]] || die "usage: $0 start|engine-dsn|sales-dsn|openkeys-dsn|stop [0|1|2]"
COMMAND=$1
SLOT=${2:-0}
[[ $SLOT =~ ^[0-2]$ ]] || die "test database slot must be 0, 1, or 2"
[[ $BASE_PORT =~ ^[0-9]+$ ]] || die "WATCHDOG_POSTGRES_PORT must be numeric"
PORT=$((BASE_PORT + SLOT))
(( PORT >= 1024 && PORT < 32768 )) \
  || die "test database port must be unprivileged and below the ephemeral range"
if (( SLOT == 0 )); then
  NAME=apitoken-watchdog-postgres
else
  NAME=apitoken-watchdog-postgres-$SLOT
fi

database_is_owned() {
  [[ $(docker inspect -f \
    '{{ index .Config.Labels "apitoken.watchdog" }}:{{ index .Config.Labels "apitoken.watchdog.slot" }}' \
    "$NAME" 2>/dev/null || true) == "test-database:$SLOT" ]]
}

case "$COMMAND" in
  start)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run --detach --rm \
      --name "$NAME" \
      --label apitoken.watchdog=test-database \
      --label "apitoken.watchdog.slot=$SLOT" \
      --publish "127.0.0.1:${PORT}:5432" \
      --tmpfs /var/lib/postgresql:rw,noexec,nosuid,size=512m \
      --env "POSTGRES_DB=$DATABASE" \
      --env "POSTGRES_USER=$USER_NAME" \
      --env "POSTGRES_PASSWORD=$PASSWORD" \
      "$IMAGE" >/dev/null

    for _ in $(seq 1 60); do
      if docker exec "$NAME" pg_isready -U "$USER_NAME" -d "$DATABASE" >/dev/null 2>&1; then
        if ! docker exec "$NAME" createdb -U "$USER_NAME" "$ENGINE_DATABASE" >/dev/null 2>&1 \
          || ! docker exec "$NAME" createdb -U "$USER_NAME" "$SALES_DATABASE" >/dev/null 2>&1 \
          || ! docker exec "$NAME" createdb -U "$USER_NAME" "$OPENKEYS_DATABASE" >/dev/null 2>&1; then
          docker logs --tail 100 "$NAME" >&2 || true
          docker rm -f "$NAME" >/dev/null 2>&1 || true
          die "could not create the disposable engine, sales and openkeys PostgreSQL databases"
        fi
        printf 'postgresql://%s:%s@127.0.0.1:%s/%s\n' "$USER_NAME" "$PASSWORD" "$PORT" "$DATABASE"
        exit 0
      fi
      sleep 1
    done
    docker logs --tail 100 "$NAME" >&2 || true
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    die "disposable PostgreSQL did not become ready"
    ;;
  engine-dsn)
    if database_is_owned \
      && docker exec "$NAME" pg_isready -U "$USER_NAME" -d "$ENGINE_DATABASE" >/dev/null 2>&1; then
      printf 'postgresql://%s:%s@127.0.0.1:%s/%s\n' "$USER_NAME" "$PASSWORD" "$PORT" "$ENGINE_DATABASE"
      exit 0
    fi
    die "disposable engine PostgreSQL is not ready"
    ;;
  sales-dsn)
    if database_is_owned \
      && docker exec "$NAME" pg_isready -U "$USER_NAME" -d "$SALES_DATABASE" >/dev/null 2>&1; then
      printf 'postgresql://%s:%s@127.0.0.1:%s/%s\n' "$USER_NAME" "$PASSWORD" "$PORT" "$SALES_DATABASE"
      exit 0
    fi
    die "disposable sales PostgreSQL is not ready"
    ;;
  openkeys-dsn)
    if database_is_owned \
      && docker exec "$NAME" pg_isready -U "$USER_NAME" -d "$OPENKEYS_DATABASE" >/dev/null 2>&1; then
      printf 'postgresql://%s:%s@127.0.0.1:%s/%s\n' "$USER_NAME" "$PASSWORD" "$PORT" "$OPENKEYS_DATABASE"
      exit 0
    fi
    die "disposable openkeys PostgreSQL is not ready"
    ;;
  stop)
    if database_is_owned; then
      docker rm -f "$NAME" >/dev/null
    fi
    ;;
  *)
    die "usage: $0 start|engine-dsn|sales-dsn|openkeys-dsn|stop [0|1|2]"
    ;;
esac
