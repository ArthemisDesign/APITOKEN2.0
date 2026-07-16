#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$SCRIPT_DIR/watchdog-lib.sh"

STATE_ROOT=/var/lib/apitoken/watchdog
CANDIDATE_ROOT=$STATE_ROOT/candidates
PENDING_FILE=$STATE_ROOT/pending-migration.sha
DB_MANIFEST=$STATE_ROOT/database-migrations.manifest
API_ENV_FILE=/etc/apitoken/api.env
DEPLOY_LOCK=/run/lock/apitoken-deploy.lock
MIGRATION_LOCK=/run/lock/apitoken-db-migrate.lock

[[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "manual migration must run as root"
[[ $# -eq 1 ]] || wd_die "usage: $0 <tested-full-sha>"
SHA=$1
wd_validate_sha "$SHA"

pending=$(wd_read_sha "$PENDING_FILE") || wd_die "there is no pending manual migration"
[[ $pending == "$SHA" ]] || wd_die "pending migration is $pending, not $SHA"

CANDIDATE=$CANDIDATE_ROOT/$SHA
MARKER=$STATE_ROOT/$SHA.tested
[[ -d $CANDIDATE && ! -L $CANDIDATE ]] || wd_die "tested candidate is missing: $CANDIDATE"
marker_sha=$(wd_marker_value "$MARKER" sha) || wd_die "test-success marker is missing"
marker_digest=$(wd_marker_value "$MARKER" migration_digest) || wd_die "test marker lacks migration digest"
[[ $marker_sha == "$SHA" ]] || wd_die "test-success marker belongs to another SHA"
[[ -f $API_ENV_FILE && ! -L $API_ENV_FILE ]] || wd_die "API environment file is missing"
[[ -f $CANDIDATE/packages/db/dist/migrate.js ]] || wd_die "tested migration build is missing"

MANIFEST=$STATE_ROOT/.manual-migration.$$
cleanup() {
  rm -f -- "$MANIFEST" "${DB_MANIFEST}.tmp.$$"
}
trap cleanup EXIT
wd_migration_manifest "$CANDIDATE" >"$MANIFEST"
actual_digest=$(wd_manifest_digest "$MANIFEST")
[[ $actual_digest == "$marker_digest" ]] || wd_die "candidate migrations changed after tests"
wd_manifest_is_append_only "$DB_MANIFEST" "$MANIFEST" \
  || wd_die "candidate edits or deletes already-applied migration history"

exec 9<>"$DEPLOY_LOCK"
flock -n 9 || wd_die "another deployment is running"
exec 8<>"$MIGRATION_LOCK"
flock -x 8

wd_log "creating a fresh validated PostgreSQL backup before manual migration"
systemctl start claude-api-backup.service
systemctl is-failed --quiet claude-api-backup.service \
  && wd_die "pre-migration backup failed"

wd_log "manually applying the exact tested migration build for $SHA"
bash -c 'set -a; . "$0"; set +a; exec node "$1"' \
  "$API_ENV_FILE" "$CANDIDATE/packages/db/dist/migrate.js"

cp -- "$MANIFEST" "${DB_MANIFEST}.tmp.$$"
chown root:deploy "${DB_MANIFEST}.tmp.$$"
chmod 0640 "${DB_MANIFEST}.tmp.$$"
mv -f -- "${DB_MANIFEST}.tmp.$$" "$DB_MANIFEST"
rm -f -- "$PENDING_FILE"
wd_log "migration $SHA committed; the watchdog may now deploy that tested candidate"

