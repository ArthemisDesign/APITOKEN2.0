#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$SCRIPT_DIR/watchdog-lib.sh"

BACKUP_SERVICE=claude-api-backup.service
BACKUP_ROOT=/var/lib/apitoken/backups
COMPOSE_FILE=/opt/apitoken/repo/deploy/commerce-postgres.compose.yaml
POSTGRES_ENV=/etc/apitoken/postgres.env

[[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "validated deployment backup must run as root"
[[ $# -eq 1 ]] || wd_die "usage: $0 <tested-full-sha>"
SHA=$1
wd_validate_sha "$SHA"
COMPLETE_MARKER=$BACKUP_ROOT/.pre-deploy-$SHA.complete

if [[ -f $COMPLETE_MARKER && ! -L $COMPLETE_MARKER ]]; then
  wd_log "validated pre-deployment PostgreSQL backup already exists for $SHA"
  exit 0
fi

temporary_files=()
cleanup() {
  ((${#temporary_files[@]} == 0)) || rm -f -- "${temporary_files[@]}"
}
trap cleanup EXIT

validate_and_preserve() {
  local database=$1 source=$BACKUP_ROOT/$1.dump temporary final
  [[ -f $source && ! -L $source ]] || wd_die "fresh $database backup is missing"
  [[ $(stat -c %Y -- "$source") -ge $backup_started ]] || wd_die "$database backup is stale"

  temporary=$BACKUP_ROOT/.$database.pre-deploy-$SHA.dump.$$
  final=$BACKUP_ROOT/$database.pre-deploy-$SHA.dump
  temporary_files+=("$temporary")
  cp --reflink=auto -- "$source" "$temporary"
  chmod 0600 "$temporary"

  # Rendering the entire custom archive (not just listing its table of contents) forces pg_restore
  # to read and decompress every archived object without changing a database.
  # Always validate with the PostgreSQL toolchain inside the production container. The host client
  # may be an older major version and cannot reliably read a newer custom archive format.
  docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
    exec -T commerce-postgres pg_restore --file=/dev/null <"$temporary"
  mv -f -- "$temporary" "$final"
}

install -d -o root -g root -m 0700 "$BACKUP_ROOT"
backup_started=$(date +%s)
systemctl reset-failed "$BACKUP_SERVICE" >/dev/null 2>&1 || true
systemctl start "$BACKUP_SERVICE"
[[ $(systemctl show "$BACKUP_SERVICE" -p Result --value) == success ]] \
  || wd_die "pre-deployment backup service did not succeed"

validate_and_preserve commerce
engine_exists=$(docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
  exec -T commerce-postgres psql -U commerce -d postgres -Atqc \
  "SELECT 1 FROM pg_database WHERE datname='claude_engine'")
if [[ $engine_exists == 1 ]]; then
  validate_and_preserve claude_engine
fi

marker_temporary=$COMPLETE_MARKER.tmp.$$
temporary_files+=("$marker_temporary")
printf 'completed_at=%s\n' "$(date -u +%FT%TZ)" >"$marker_temporary"
chmod 0600 "$marker_temporary"
mv -f -- "$marker_temporary" "$COMPLETE_MARKER"

wd_log "fresh validated pre-deployment PostgreSQL backup preserved for $SHA"
