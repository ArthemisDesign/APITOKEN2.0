#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }

CONTAINER=${ENGINE_POSTGRES_CONTAINER:-deploy-commerce-postgres-1}
DB=${ENGINE_POSTGRES_DB:-claude_engine}
ROLE=${ENGINE_POSTGRES_ROLE:-claude_engine}
DATA_DIR=${ENGINE_DATA_DIR:-/srv/claude-api/data}
PENDING_ENV=$DATA_DIR/engine-postgres.pending
ACTIVE_ENV=$DATA_DIR/engine-postgres.env

command -v docker >/dev/null
command -v openssl >/dev/null
[[ $DB =~ ^[a-z_][a-z0-9_]{0,62}$ ]] || { echo "invalid engine database name" >&2; exit 1; }
[[ $ROLE =~ ^[a-z_][a-z0-9_]{0,62}$ ]] || { echo "invalid engine role name" >&2; exit 1; }
docker inspect "$CONTAINER" >/dev/null
install -d -m 0750 -o deploy -g deploy "$DATA_DIR"

if [[ -s "$ACTIVE_ENV" ]]; then
  echo "engine PostgreSQL is already active; refusing to rotate its credential" >&2
  exit 0
fi
if [[ -s "$PENDING_ENV" ]]; then
  echo "engine PostgreSQL is already provisioned and pending cutover"
  exit 0
fi

password=$(openssl rand -hex 32)
docker exec "$CONTAINER" psql -U commerce -d postgres -v ON_ERROR_STOP=1 \
  -c "DO \$\$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='$ROLE') THEN CREATE ROLE $ROLE LOGIN; END IF; END \$\$" \
  >/dev/null
docker exec "$CONTAINER" psql -U commerce -d postgres -v ON_ERROR_STOP=1 \
  -c "ALTER ROLE $ROLE NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD '$password'" \
  >/dev/null
if ! docker exec "$CONTAINER" psql -U commerce -d postgres -Atqc \
  "SELECT 1 FROM pg_database WHERE datname='$DB'" | grep -qx 1; then
  docker exec "$CONTAINER" createdb -U commerce -O "$ROLE" "$DB"
fi
docker exec "$CONTAINER" psql -U commerce -d postgres -v ON_ERROR_STOP=1 \
  -c "ALTER DATABASE $DB OWNER TO $ROLE" \
  -c "REVOKE ALL ON DATABASE $DB FROM PUBLIC" \
  -c "GRANT CONNECT,TEMPORARY ON DATABASE $DB TO $ROLE" \
  >/dev/null
docker exec "$CONTAINER" psql -U commerce -d "$DB" -v ON_ERROR_STOP=1 \
  -c "REVOKE CREATE ON SCHEMA public FROM PUBLIC" \
  -c "GRANT USAGE,CREATE ON SCHEMA public TO $ROLE" \
  >/dev/null

tmp=$(mktemp "$DATA_DIR/.engine-postgres.pending.XXXXXX")
trap 'rm -f "$tmp"' EXIT
printf 'CLAUDE_API_DATABASE_URL=postgresql://%s:%s@127.0.0.1:5433/%s\n' \
  "$ROLE" "$password" "$DB" >"$tmp"
chown root:root "$tmp"
chmod 0600 "$tmp"
mv -f "$tmp" "$PENDING_ENV"
trap - EXIT
echo "engine PostgreSQL role/database provisioned; credential staged for cutover"
