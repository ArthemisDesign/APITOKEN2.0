#!/usr/bin/env bash
set -euo pipefail

# Root entrypoint for the explicit engine schema migration. The deploy user may select only a
# finalized release SHA; the secret-bearing PostgreSQL environment and release root stay fixed here.
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/lib.sh
source "$SCRIPT_DIR/lib.sh"

ENGINE_RELEASE_ROOT=$CONTOUR_ROOTS_ENGINE_RELEASE
ENGINE_POSTGRES_ENV=$CONTOUR_ROOTS_DATA/engine-postgres.env
MIGRATION_LOCK_FILE=$CONTOUR_LOCKS_MIGRATION
PRICING_RETIREMENT_ADMISSION=${CONTOUR_ROOTS_CONTROLLER%/controller}/pricing-retirement-admission.sh

[[ ${EUID:-$(id -u)} -eq 0 ]] || die "engine schema migration must run as root"
[[ $# -eq 1 ]] || die "usage: $0 <engine-release-sha>"
SHA=$1
validate_sha "$SHA"

CANDIDATE=$ENGINE_RELEASE_ROOT/$SHA
validate_engine_release "$ENGINE_RELEASE_ROOT" "$CANDIDATE" "$SHA"
CURRENT=$(required_current_release_path "$ENGINE_RELEASE_ROOT")
[[ $CURRENT == "$CANDIDATE" ]] || die "current engine release is not the requested migration release"

[[ -f $ENGINE_POSTGRES_ENV && ! -L $ENGINE_POSTGRES_ENV && -s $ENGINE_POSTGRES_ENV ]] \
  || die "engine PostgreSQL environment is missing or unsafe"
[[ $(stat_owner_uid "$ENGINE_POSTGRES_ENV") == 0 ]] \
  || die "engine PostgreSQL environment must be root-owned"
[[ $(stat -c '%a' -- "$ENGINE_POSTGRES_ENV") == 600 ]] \
  || die "engine PostgreSQL environment must be mode 0600"
validate_fixed_lock_file "$MIGRATION_LOCK_FILE" "$CONTOUR_LOCKS_MIGRATION" migration

exec 8<>"$MIGRATION_LOCK_FILE"
flock -w 30 8 || die "timed out waiting for the engine PostgreSQL migration lock"

# Existing releases predate this one-time capability marker and remain valid recovery anchors.
# Every newly built release explicitly says whether it embeds contraction 0049, so a pruned
# watchdog candidate cannot either block an old restart or bypass the destructive gate.
pricing_retirement_marker=$CANDIDATE/$PRICING_RETIREMENT_ENGINE_ADMISSION_MARKER
if [[ -e $pricing_retirement_marker || -L $pricing_retirement_marker ]]; then
  pricing_retirement_capability=$(validate_pricing_retirement_engine_admission_marker \
    "$pricing_retirement_marker")
  if [[ $pricing_retirement_capability == contraction-0049 ]]; then
    # The watchdog has already preserved the exact-SHA backup before engine promotion.
    "$PRICING_RETIREMENT_ADMISSION" engine "$SHA"
  else
    log "engine release $SHA is explicitly pre-contraction; pricing-retirement admission is a no-op"
  fi
else
  log "engine release $SHA predates pricing-retirement capability markers; admission is a no-op"
fi

deploy_uid=$(id -u "$CONTOUR_IDENTITY_RUNTIME_USER")
deploy_gid=$(id -g "$CONTOUR_IDENTITY_RUNTIME_GROUP")
log "applying pending engine PostgreSQL migrations from release $SHA"
bash -c 'set -a; . "$1"; set +a; export HOME="$2"; exec setpriv --reuid="$3" --regid="$4" --init-groups --no-new-privs "$5" db migrate-engine' \
  engine-migrate "$ENGINE_POSTGRES_ENV" "/home/$CONTOUR_IDENTITY_RUNTIME_USER" \
  "$deploy_uid" "$deploy_gid" "$CANDIDATE/claude-api"
log "engine PostgreSQL migrations completed for release $SHA"
