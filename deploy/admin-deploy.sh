#!/usr/bin/env bash
set -euo pipefail

# Deploy the admin panel bounded context (admin.apitoken.sale) from the watchdog's already
# tested+frozen candidate. Single-instance (not blue-green): promote an immutable release,
# atomically repoint the current symlink, restart the unit, health-gate, and roll the symlink
# back on failure. Invoked by watchdog.sh as root.
#
# Isolation: the admin panel has its OWN release root, decoupled from the commerce
# /opt/apitoken/releases/current and from the sales/OpenKeys release roots, so an admin-only
# change never disturbs the commerce blue-green API or the partner portal. The app has no
# database and no secrets, so there are no migrations and no env file.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ -f $SCRIPT_DIR/contour-config.sh ]]; then CONTOUR_ROOT=$SCRIPT_DIR; else CONTOUR_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd); fi
# shellcheck source=deploy/contour-config.sh
source "$CONTOUR_ROOT/contour-config.sh"

# Needs root (chown, systemctl). The watchdog runs as `deploy`, so self-elevate — robust whether
# invoked with or without sudo.
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then exec sudo -n -- "$0" "$@"; fi

SHA=${1:?usage: admin-deploy.sh <full-40-char-sha>}
[[ $SHA =~ ^[0-9a-f]{40}$ ]] || { echo "admin-deploy: SHA must be a full 40-char commit hash" >&2; exit 1; }

CANDIDATE_ROOT=${ADMIN_CANDIDATE_ROOT:-$CONTOUR_ROOTS_CANDIDATE}
RELEASE_ROOT=${ADMIN_RELEASE_ROOT:-$CONTOUR_ROOTS_ADMIN_RELEASE}
WEB_UNIT=$CONTOUR_UNITS_ADMIN
# The health endpoint returns 200 in both the previous release and the candidate, so rollback
# health remains testable on the same stable path.
WEB_HEALTH=${ADMIN_WEB_HEALTH:-$CONTOUR_ORIGINS_ADMIN/api/health}
WEB_ROLLBACK_HEALTH=${ADMIN_WEB_ROLLBACK_HEALTH:-$CONTOUR_ORIGINS_ADMIN/api/health}
HEALTH_RETRIES=${ADMIN_HEALTH_RETRIES:-30}
HEALTH_INTERVAL=${ADMIN_HEALTH_INTERVAL:-2}

candidate="$CANDIDATE_ROOT/$SHA"
release="$RELEASE_ROOT/$SHA"
current_link="$RELEASE_ROOT/current"

log() { printf '[admin-deploy] %s\n' "$*"; }
die() { printf '[admin-deploy] ERROR: %s\n' "$*" >&2; exit 1; }

[[ -d $candidate && ! -L $candidate ]] || die "tested candidate is missing: $candidate"
[[ -d $candidate/apps/admin/.next ]] || die "candidate has no built admin web"

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
[[ -d $release/apps/admin/.next ]] || die "promoted release is incomplete: $release"

# 2) Atomically repoint current → new release, then restart the unit.
tmp_link="$RELEASE_ROOT/.current.$$"
ln -sfn -- "$release" "$tmp_link"
mv -T -- "$tmp_link" "$current_link"
log "admin-current → $SHA; restarting unit"
systemctl restart "$WEB_UNIT"

# 3) Health-gate; roll back the symlink on failure.
if wait_healthy; then
  log "admin $SHA healthy (200); deploy complete"
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
    die "admin $SHA unhealthy; rolled back to $(basename -- "$previous_target")"
  fi
  die "admin $SHA unhealthy AND rollback target also unhealthy — manual intervention required"
fi
die "admin $SHA unhealthy and no previous release to roll back to"
