#!/usr/bin/env bash
set -euo pipefail

# Deploy the sales bounded context (partners.apitoken.sale) from the watchdog's already
# tested+frozen candidate. Single-instance (not blue-green): promote an immutable release,
# migrate the sales DB, atomically repoint the sales-current symlink, restart both units,
# health-gate, and roll the symlink back on failure. Invoked by watchdog.sh as root.
#
# Isolation guarantees (see SALES_PORTAL.md): the sales tier has its OWN release root, decoupled
# from the shared commerce /opt/apitoken/releases/current (which governs the commerce blue-green
# API and must not be disturbed by a sales-only change).

# Needs root (chown, read root-only sales.env, systemctl). The watchdog runs as `deploy`
# (NOPASSWD:ALL), so self-elevate — robust whether invoked with or without sudo.
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then exec sudo -n -- "$0" "$@"; fi

SHA=${1:?usage: sales-deploy.sh <full-40-char-sha>}
[[ $SHA =~ ^[0-9a-f]{40}$ ]] || { echo "sales-deploy: SHA must be a full 40-char commit hash" >&2; exit 1; }

CANDIDATE_ROOT=${SALES_CANDIDATE_ROOT:-/var/lib/apitoken/watchdog/candidates}
RELEASE_ROOT=${SALES_RELEASE_ROOT:-/opt/apitoken/sales-releases}
ENV_FILE=${SALES_ENV_FILE:-/etc/apitoken/sales.env}
API_UNIT=apitoken-sales-api.service
WEB_UNIT=apitoken-sales-web.service
API_HEALTH=${SALES_API_HEALTH:-http://127.0.0.1:3100/v1/health}
WEB_HEALTH=${SALES_WEB_HEALTH:-http://127.0.0.1:3200/}
HEALTH_RETRIES=${SALES_HEALTH_RETRIES:-30}
HEALTH_INTERVAL=${SALES_HEALTH_INTERVAL:-2}

candidate="$CANDIDATE_ROOT/$SHA"
release="$RELEASE_ROOT/$SHA"
current_link="$RELEASE_ROOT/current"

log() { printf '[sales-deploy] %s\n' "$*"; }
die() { printf '[sales-deploy] ERROR: %s\n' "$*" >&2; exit 1; }

[[ -d $candidate && ! -L $candidate ]] || die "tested candidate is missing: $candidate"
[[ -f $candidate/apps/sales-api/dist/main.js ]] || die "candidate has no built sales-api"
[[ -d $candidate/apps/sales-web/.next ]] || die "candidate has no built sales-web"
[[ -f $candidate/packages/sales-db/dist/migrate.js ]] || die "candidate has no built sales-db migrate"
[[ -f $ENV_FILE ]] || die "sales env file missing: $ENV_FILE"

health_ok() {
  local url=$1 code
  code=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 5 "$url" 2>/dev/null || true)
  [[ $code == 200 ]]
}

wait_healthy() {
  local i
  for (( i = 0; i < HEALTH_RETRIES; i++ )); do
    if health_ok "$API_HEALTH" && health_ok "$WEB_HEALTH"; then
      return 0
    fi
    sleep "$HEALTH_INTERVAL"
  done
  return 1
}

restart_units() {
  systemctl restart "$API_UNIT" "$WEB_UNIT"
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
[[ -f $release/apps/sales-api/dist/main.js ]] || die "promoted release is incomplete: $release"

# 2) Migrate the sales DB from the NEW release before cutover (expand-only, advisory-locked).
log "applying sales-db migrations from $SHA"
# shellcheck disable=SC1090
( set -a; . "$ENV_FILE"; set +a; node "$release/packages/sales-db/dist/migrate.js" ) \
  || die "sales-db migration failed; leaving current symlink untouched ($previous_target)"

# 3) Atomically repoint current → new release, then restart units.
tmp_link="$RELEASE_ROOT/.current.$$"
ln -sfn -- "$release" "$tmp_link"
mv -T -- "$tmp_link" "$current_link"
log "sales-current → $SHA; restarting units"
restart_units

# 4) Health-gate; roll back the symlink on failure.
if wait_healthy; then
  log "sales $SHA healthy (api+web 200); deploy complete"
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
  restart_units
  if wait_healthy; then
    die "sales $SHA unhealthy; rolled back to $(basename -- "$previous_target")"
  fi
  die "sales $SHA unhealthy AND rollback target also unhealthy — manual intervention required"
fi
die "sales $SHA unhealthy and no previous release to roll back to"
