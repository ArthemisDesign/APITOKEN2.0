#!/usr/bin/env bash
set -euo pipefail

[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo "run as root" >&2; exit 1; }

DATA_DIR=${ENGINE_DATA_DIR:-/srv/claude-api/data}
RELEASE_ROOT=${ENGINE_RELEASE_ROOT:-/srv/claude-api/releases}
PENDING_ENV=$DATA_DIR/engine-postgres.pending
ACTIVE_ENV=$DATA_DIR/engine-postgres.env
SQLITE_DB=${SUBS_DB:-$DATA_DIR/subscriptions.db}
UNIT_SOURCE=${ENGINE_UNIT_SOURCE:-/opt/apitoken/repo/systemd/claude-api.service}
UNIT_TARGET=/etc/systemd/system/claude-api.service
TEMPLATE_SOURCE=${ENGINE_TEMPLATE_SOURCE:-/opt/apitoken/repo/systemd/claude-api@.service}
TEMPLATE_TARGET=/etc/systemd/system/claude-api@.service
READY_URL=${ENGINE_READY_URL:-http://127.0.0.1:8787/ready}
CURRENT_BIN=$RELEASE_ROOT/current/claude-api

[[ -s "$PENDING_ENV" ]] || { echo "missing $PENDING_ENV; provision first" >&2; exit 1; }
[[ ! -e "$ACTIVE_ENV" ]] || { echo "PostgreSQL cutover is already active" >&2; exit 1; }
[[ -x "$CURRENT_BIN" ]]
[[ -f "$UNIT_SOURCE" ]]
[[ -f "$TEMPLATE_SOURCE" ]]

backup_dir=$DATA_DIR/backups/pre-postgres-cutover
install -d -m 0700 -o deploy -g deploy "$backup_dir"
env -u CLAUDE_API_DATABASE_URL "$CURRENT_BIN" backup --out "$backup_dir" --keep 4

unit_backup=$(mktemp /etc/systemd/system/.claude-api.service.stage2.XXXXXX)
cp -a "$UNIT_TARGET" "$unit_backup"
template_backup=$(mktemp /etc/systemd/system/.claude-api-at.service.stage2.XXXXXX)
template_was_present=0
if [[ -f "$TEMPLATE_TARGET" ]]; then
  cp -a "$TEMPLATE_TARGET" "$template_backup"
  template_was_present=1
fi
activated=0
recover() {
  local status=$?
  if [[ $status -eq 0 || $activated -eq 1 ]]; then return; fi
  echo "cutover failed; restoring SQLite unit/environment" >&2
  systemctl stop claude-api.service 2>/dev/null || true
  if [[ -e "$ACTIVE_ENV" ]]; then mv -f "$ACTIVE_ENV" "$PENDING_ENV"; fi
  cp -a "$unit_backup" "$UNIT_TARGET"
  if [[ $template_was_present -eq 1 ]]; then
    cp -a "$template_backup" "$TEMPLATE_TARGET"
  else
    rm -f "$TEMPLATE_TARGET"
  fi
  systemctl daemon-reload
  systemctl start claude-api.service
}
trap recover EXIT

# The old singleton drains all streams and its billing FIFO before returning from stop.
systemctl stop claude-api.service

set -a
# shellcheck disable=SC1090
source "$PENDING_ENV"
set +a
"$CURRENT_BIN" db migrate-postgres --sqlite "$SQLITE_DB"
"$CURRENT_BIN" db verify-postgres

install -m 0644 "$UNIT_SOURCE" "$UNIT_TARGET"
install -m 0644 "$TEMPLATE_SOURCE" "$TEMPLATE_TARGET"
mv -f "$PENDING_ENV" "$ACTIVE_ENV"
systemctl daemon-reload
systemctl start claude-api.service

for _ in $(seq 1 60); do
  if [[ $(curl -fsS -o /dev/null -w '%{http_code}' "$READY_URL" 2>/dev/null || true) == 200 ]]; then
    activated=1
    break
  fi
  sleep 1
done
[[ $activated -eq 1 ]] || { journalctl -u claude-api.service -n 40 --no-pager >&2; exit 1; }

touch "$DATA_DIR/.stage2-postgres-cutover"
chown root:root "$DATA_DIR/.stage2-postgres-cutover"
chmod 0600 "$DATA_DIR/.stage2-postgres-cutover"
rm -f "$unit_backup" "$template_backup"
trap - EXIT
echo "engine PostgreSQL authority is active and ready; SQLite retained as rollback-era audit snapshot"
