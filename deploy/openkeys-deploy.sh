#!/usr/bin/env bash
set -euo pipefail

# Deploy the OpenKeys bounded context (openkeys.apitoken.sale) from the watchdog's already
# tested+frozen candidate. Single-instance (not blue-green): promote an immutable release,
# migrate the openkeys DB, atomically repoint the current symlink, restart the unit,
# health-gate, and roll the symlink back on failure. Invoked by watchdog.sh as root.
#
# Isolation: OpenKeys has its OWN release root, decoupled from the commerce
# /opt/apitoken/releases/current and from the sales release root, so an OpenKeys-only
# change never disturbs the commerce blue-green API or the partner portal.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ -f $SCRIPT_DIR/contour-config.sh ]]; then CONTOUR_ROOT=$SCRIPT_DIR; else CONTOUR_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd); fi
# shellcheck source=deploy/contour-config.sh
source "$CONTOUR_ROOT/contour-config.sh"

# Needs root (chown, read root-only openkeys.env, systemctl). The watchdog runs as `deploy`
# (NOPASSWD:ALL), so self-elevate — robust whether invoked with or without sudo.
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then exec sudo -n -- "$0" "$@"; fi

SHA=${1:?usage: openkeys-deploy.sh <full-40-char-sha>}
[[ $SHA =~ ^[0-9a-f]{40}$ ]] || { echo "openkeys-deploy: SHA must be a full 40-char commit hash" >&2; exit 1; }

CANDIDATE_ROOT=${OPENKEYS_CANDIDATE_ROOT:-$CONTOUR_ROOTS_CANDIDATE}
RELEASE_ROOT=${OPENKEYS_RELEASE_ROOT:-$CONTOUR_ROOTS_OPENKEYS_RELEASE}
ENV_FILE=${OPENKEYS_ENV_FILE:-$CONTOUR_ROOTS_CONFIG/openkeys.env}
WEB_UNIT=$CONTOUR_UNITS_OPENKEYS
# The product root may intentionally redirect (currently to /docs). Probe a stable page that
# returns 200 in both the previous release and the candidate, so a valid redirect is not mistaken
# for an unhealthy process and rollback health remains testable.
WEB_HEALTH=${OPENKEYS_WEB_HEALTH:-$CONTOUR_ORIGINS_OPENKEYS/api/ready}
WEB_ROLLBACK_HEALTH=${OPENKEYS_WEB_ROLLBACK_HEALTH:-$CONTOUR_ORIGINS_OPENKEYS/docs}
HEALTH_RETRIES=${OPENKEYS_HEALTH_RETRIES:-30}
HEALTH_INTERVAL=${OPENKEYS_HEALTH_INTERVAL:-2}

candidate="$CANDIDATE_ROOT/$SHA"
release="$RELEASE_ROOT/$SHA"
current_link="$RELEASE_ROOT/current"

log() { printf '[openkeys-deploy] %s\n' "$*"; }
die() { printf '[openkeys-deploy] ERROR: %s\n' "$*" >&2; exit 1; }

[[ -d $candidate && ! -L $candidate ]] || die "tested candidate is missing: $candidate"
[[ -d $candidate/apps/openkeys/.next ]] || die "candidate has no built openkeys web"
[[ -f $candidate/packages/openkeys-db/dist/migrate.js ]] || die "candidate has no built openkeys-db migrate"
[[ -f $ENV_FILE ]] || die "openkeys env file missing: $ENV_FILE"

health_ok() {
  local url=$1 code
  code=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 5 "$url" 2>/dev/null || true)
  [[ $code == 200 ]]
}

wait_healthy() {
  local url=${1:-$WEB_HEALTH} i
  for (( i = 0; i < HEALTH_RETRIES; i++ )); do
    health_ok "$url" && return 0
    sleep "$HEALTH_INTERVAL"
  done
  return 1
}

previous_target=""
if [[ -L $current_link ]]; then
  previous_target=$(readlink -f -- "$current_link" || true)
fi

# 1) Promote an immutable release by copying the tested candidate (idempotent).
install -d -o deploy -g deploy -m 0755 "$RELEASE_ROOT"
if [[ ! -d $release ]]; then
  stage="$RELEASE_ROOT/.${SHA}.tmp.$$"
  rm -rf --one-file-system -- "$stage" 2>/dev/null || true
  log "promoting tested candidate → $release"
  cp -a -- "$candidate" "$stage"
  chown -R deploy:deploy "$stage"
  mv -T -- "$stage" "$release"
fi
[[ -d $release/apps/openkeys/.next ]] || die "promoted release is incomplete: $release"

# 2) Migrate the openkeys DB from the NEW release before cutover (expand-only, advisory-locked).
log "applying openkeys-db migrations from $SHA"
# shellcheck disable=SC1090
( set -a; . "$ENV_FILE"; set +a; node "$release/packages/openkeys-db/dist/migrate.js" ) \
  || die "openkeys-db migration failed; leaving current symlink untouched ($previous_target)"

# 3) Atomically repoint current → new release, then restart the unit.
tmp_link="$RELEASE_ROOT/.current.$$"
ln -sfn -- "$release" "$tmp_link"
mv -T -- "$tmp_link" "$current_link"
log "openkeys-current → $SHA; restarting unit"
systemctl restart "$WEB_UNIT"

# 4) Health-gate; roll back the symlink on failure.
if wait_healthy; then
  log "openkeys $SHA healthy (200); deploy complete"
  # Prune older releases, keep the two most recent plus the live one.
  mapfile -t olds < <(cd "$RELEASE_ROOT" && ls -1dt -- */ 2>/dev/null | sed 's#/$##' | grep -E '^[0-9a-f]{40}$' || true)
  keep=0
  for dir in "${olds[@]}"; do
    [[ $dir == "$SHA" ]] && continue
    keep=$((keep + 1))
    (( keep <= 2 )) && continue
    rm -rf --one-file-system -- "${RELEASE_ROOT:?}/$dir" 2>/dev/null || true
  done
  exit 0
fi

log "health check FAILED for $SHA; rolling back"
if [[ -n $previous_target && -d $previous_target ]]; then
  ln -sfn -- "$previous_target" "$tmp_link"
  mv -T -- "$tmp_link" "$current_link"
  systemctl restart "$WEB_UNIT"
  if wait_healthy "$WEB_ROLLBACK_HEALTH"; then
    die "openkeys $SHA unhealthy; rolled back to $(basename -- "$previous_target")"
  fi
  die "openkeys $SHA unhealthy AND rollback target also unhealthy — manual intervention required"
fi
die "openkeys $SHA unhealthy and no previous release to roll back to"
