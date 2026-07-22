#!/usr/bin/env bash
set -euo pipefail

# Root-owned helper used by the watchdog. Candidate code never receives Docker access; it only
# receives credentials for this disposable loopback-only PostgreSQL container.

NAME=apitoken-watchdog-postgres
IMAGE=${WATCHDOG_POSTGRES_IMAGE:-postgres:18-alpine}
PORT=${WATCHDOG_POSTGRES_PORT:-55432}
USER_NAME=watchdog
DATABASE=watchdog
ENGINE_DATABASE=watchdog_engine
SALES_DATABASE=watchdog_sales
PASSWORD=watchdog-local-disposable-only

die() {
  printf '[watchdog-test-db] ERROR: %s\n' "$*" >&2
  exit 1
}

[[ ${EUID:-$(id -u)} -eq 0 ]] || die "must run as root"

case "${1:-}" in
  start)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run --detach --rm \
      --name "$NAME" \
      --label apitoken.watchdog=test-database \
      --publish "127.0.0.1:${PORT}:5432" \
      --tmpfs /var/lib/postgresql:rw,noexec,nosuid,size=512m \
      --env "POSTGRES_DB=$DATABASE" \
      --env "POSTGRES_USER=$USER_NAME" \
      --env "POSTGRES_PASSWORD=$PASSWORD" \
      "$IMAGE" >/dev/null

    for _ in $(seq 1 60); do
      if docker exec "$NAME" pg_isready -U "$USER_NAME" -d "$DATABASE" >/dev/null 2>&1; then
        if ! docker exec "$NAME" createdb -U "$USER_NAME" "$ENGINE_DATABASE" >/dev/null 2>&1 \
          || ! docker exec "$NAME" createdb -U "$USER_NAME" "$SALES_DATABASE" >/dev/null 2>&1; then
          docker logs --tail 100 "$NAME" >&2 || true
          docker rm -f "$NAME" >/dev/null 2>&1 || true
          die "could not create the disposable engine and sales PostgreSQL databases"
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
    if docker inspect -f '{{ index .Config.Labels "apitoken.watchdog" }}' "$NAME" 2>/dev/null \
      | grep -qx 'test-database' \
      && docker exec "$NAME" pg_isready -U "$USER_NAME" -d "$ENGINE_DATABASE" >/dev/null 2>&1; then
      printf 'postgresql://%s:%s@127.0.0.1:%s/%s\n' "$USER_NAME" "$PASSWORD" "$PORT" "$ENGINE_DATABASE"
      exit 0
    fi
    die "disposable engine PostgreSQL is not ready"
    ;;
  sales-dsn)
    if docker inspect -f '{{ index .Config.Labels "apitoken.watchdog" }}' "$NAME" 2>/dev/null \
      | grep -qx 'test-database' \
      && docker exec "$NAME" pg_isready -U "$USER_NAME" -d "$SALES_DATABASE" >/dev/null 2>&1; then
      printf 'postgresql://%s:%s@127.0.0.1:%s/%s\n' "$USER_NAME" "$PASSWORD" "$PORT" "$SALES_DATABASE"
      exit 0
    fi
    die "disposable sales PostgreSQL is not ready"
    ;;
  stop)
    if docker inspect -f '{{ index .Config.Labels "apitoken.watchdog" }}' "$NAME" 2>/dev/null \
      | grep -qx 'test-database'; then
      docker rm -f "$NAME" >/dev/null
    fi
    ;;
  *)
    die "usage: $0 start|engine-dsn|sales-dsn|stop"
    ;;
esac
