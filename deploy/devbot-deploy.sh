#!/usr/bin/env bash
set -euo pipefail

# Deploy the dev notification bot (apps/devbot) from the watchdog's already tested+frozen
# candidate. Single-instance (not blue-green), modeled on admin-deploy.sh: promote an immutable
# release, atomically repoint the current symlink, restart the unit, health-gate, and roll the
# symlink back on failure. Invoked by watchdog.sh as root.
#
# Isolation: the bot has its OWN release root (/opt/apitoken/devbot-releases), decoupled from the
# commerce blue-green slots and from the sales/OpenKeys/admin release roots, so a devbot-only
# change never disturbs any serving plane. The app has no database, so there are no migrations.
#
# Disabled until provisioned: while the operator has not placed /etc/apitoken/devbot.env this
# script exits 0 without touching anything, so the watchdog lane never blocks the pipeline before
# secrets exist. The matching ConditionPathExists in systemd/apitoken-devbot.service keeps the
# unit itself cleanly inactive over the same window.

# Needs root (chown, systemctl). The watchdog runs as `deploy`, so self-elevate — robust whether
# invoked with or without sudo.
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then exec sudo -n -- "$0" "$@"; fi

SHA=${1:?usage: devbot-deploy.sh <full-40-char-sha>}
[[ $SHA =~ ^[0-9a-f]{40}$ ]] || { echo "devbot-deploy: SHA must be a full 40-char commit hash" >&2; exit 1; }

CANDIDATE_ROOT=${DEVBOT_CANDIDATE_ROOT:-/var/lib/apitoken/watchdog/candidates}
RELEASE_ROOT=${DEVBOT_RELEASE_ROOT:-/opt/apitoken/devbot-releases}
UNIT=apitoken-devbot.service
ENV_FILE=${DEVBOT_ENV_FILE:-/etc/apitoken/devbot.env}
# The health endpoint answers 200 in both the previous release and the candidate, so rollback
# health remains testable on the same stable path.
HEALTH=${DEVBOT_HEALTH:-http://127.0.0.1:3800/health}
ROLLBACK_HEALTH=${DEVBOT_ROLLBACK_HEALTH:-http://127.0.0.1:3800/health}
HEALTH_RETRIES=${DEVBOT_HEALTH_RETRIES:-30}
HEALTH_INTERVAL=${DEVBOT_HEALTH_INTERVAL:-2}

candidate="$CANDIDATE_ROOT/$SHA"
release="$RELEASE_ROOT/$SHA"
current_link="$RELEASE_ROOT/current"

log() { printf '[devbot-deploy] %s\n' "$*"; }
die() { printf '[devbot-deploy] ERROR: %s\n' "$*" >&2; exit 1; }

if [[ ! -f $ENV_FILE ]]; then
  log "devbot disabled: $ENV_FILE missing — skipping"
  exit 0
fi

[[ -d $candidate && ! -L $candidate ]] || die "tested candidate is missing: $candidate"
if [[ ! -d $candidate/apps/devbot ]]; then
  # The integration (unit, lane, this script) can reach production before the application code
  # itself. Rolling nothing is correct there; a present but unbuilt tree below is NOT tolerated.
  log "candidate carries no apps/devbot tree yet — skipping"
  exit 0
fi
[[ -f $candidate/apps/devbot/dist/main.js ]] || die "candidate has no built devbot: $candidate/apps/devbot/dist/main.js"

health_ok() {
  local url=$1 code
  code=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 5 "$url" 2>/dev/null || true)
  [[ $code == 200 ]]
}

wait_healthy() {
  local url=${1:-$HEALTH} i
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
[[ -f $release/apps/devbot/dist/main.js ]] || die "promoted release is incomplete: $release"

# 2) Atomically repoint current → new release, then restart the unit.
tmp_link="$RELEASE_ROOT/.current.$$"
ln -sfn -- "$release" "$tmp_link"
mv -T -- "$tmp_link" "$current_link"
log "devbot-current → $SHA; restarting unit"
systemctl restart "$UNIT"

# 3) Health-gate; roll back the symlink on failure.
if wait_healthy; then
  log "devbot $SHA healthy (200); deploy complete"
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
  systemctl restart "$UNIT"
  if wait_healthy "$ROLLBACK_HEALTH"; then
    die "devbot $SHA unhealthy; rolled back to $(basename -- "$previous_target")"
  fi
  die "devbot $SHA unhealthy AND rollback target also unhealthy — manual intervention required"
fi
die "devbot $SHA unhealthy and no previous release to roll back to"
