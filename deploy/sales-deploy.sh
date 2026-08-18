#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ -f $SCRIPT_DIR/watchdog-lib.sh && ! -L $SCRIPT_DIR/watchdog-lib.sh ]]; then
  # Repository and extracted-candidate layout.
  WATCHDOG_ROOT=$SCRIPT_DIR
elif [[ -f $SCRIPT_DIR/../watchdog-lib.sh && ! -L $SCRIPT_DIR/../watchdog-lib.sh ]]; then
  # Installed controller layout: component runners live below the fixed shared helpers.
  WATCHDOG_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
else
  printf 'sales-deploy: fixed watchdog-lib.sh is unavailable\n' >&2
  exit 1
fi
# shellcheck source=deploy/watchdog-lib.sh
source "$WATCHDOG_ROOT/watchdog-lib.sh"

# Deploy the sales bounded context (partners.apitoken.sale) from the watchdog's already
# tested+frozen candidate. Single-instance (not blue-green): promote an immutable release,
# migrate the sales DB, atomically repoint the sales-current symlink, restart both units,
# health-gate, and roll the symlink back on failure. Invoked by watchdog.sh as root.
#
# Isolation guarantees (see docs/sales/SALES_PORTAL.md): the sales tier has its OWN release root, decoupled
# from the shared commerce /opt/apitoken/releases/current (which governs the commerce blue-green
# API and must not be disturbed by a sales-only change).

SHA=${1:?usage: sales-deploy.sh <full-40-char-sha>}
[[ $SHA =~ ^[0-9a-f]{40}$ ]] || { echo "sales-deploy: SHA must be a full 40-char commit hash" >&2; exit 1; }

# Needs root (chown, read root-only sales.env, systemctl). Validate the immutable identity before
# elevation so malformed/manual invocations never cross the privilege boundary. The watchdog runs
# as `deploy` (NOPASSWD:ALL), so self-elevate — robust whether invoked with or without sudo.
if [[ ${EUID:-$(id -u)} -ne 0 ]]; then exec sudo -n -- "$0" "$@"; fi

CANDIDATE_ROOT=${SALES_CANDIDATE_ROOT:-/var/lib/apitoken/watchdog/candidates}
RELEASE_ROOT=${SALES_RELEASE_ROOT:-/opt/apitoken/sales-releases}
ENV_FILE=${SALES_ENV_FILE:-/etc/apitoken/sales.env}
API_UNIT=apitoken-sales-api.service
WEB_UNIT=apitoken-sales-web.service
API_HEALTH=${SALES_API_HEALTH:-http://127.0.0.1:3100/v1/health}
WEB_HEALTH=${SALES_WEB_HEALTH:-http://127.0.0.1:3200/}
WEB_LOGIN=${SALES_WEB_LOGIN:-http://127.0.0.1:3200/login}
API_TELEGRAM_CONFIG=${SALES_API_TELEGRAM_CONFIG:-http://127.0.0.1:3100/v1/auth/telegram/config}
COMMERCE_BALANCER_URL=${COMMERCE_BALANCER_URL:-http://127.0.0.1:8791}
HEALTH_RETRIES=${SALES_HEALTH_RETRIES:-30}
HEALTH_INTERVAL=${SALES_HEALTH_INTERVAL:-2}
STATE_ROOT=${SALES_STATE_ROOT:-/var/lib/apitoken/watchdog}
SALES_DB_MANIFEST=$STATE_ROOT/sales-database-migrations.manifest
BACKUP_RUNNER=${SALES_BACKUP_RUNNER:-$WATCHDOG_ROOT/watchdog-backup.sh}

candidate="$CANDIDATE_ROOT/$SHA"
release="$RELEASE_ROOT/$SHA"
current_link="$RELEASE_ROOT/current"

log() { printf '[sales-deploy] %s\n' "$*"; }
die() { printf '[sales-deploy] ERROR: %s\n' "$*" >&2; exit 1; }
ENV_TMP=
MANIFEST_TMP=
cleanup() {
  [[ -z $ENV_TMP ]] || rm -f -- "$ENV_TMP"
  [[ -z $MANIFEST_TMP ]] || rm -f -- "$MANIFEST_TMP"
}
trap cleanup EXIT

[[ -d $candidate && ! -L $candidate ]] || die "tested candidate is missing: $candidate"
[[ -f $candidate/apps/sales-api/dist/main.js ]] || die "candidate has no built sales-api"
[[ -d $candidate/apps/sales-web/.next ]] || die "candidate has no built sales-web"
[[ -f $candidate/packages/sales-db/dist/migrate.js ]] || die "candidate has no built sales-db migrate"
[[ -f $ENV_FILE ]] || die "sales env file missing: $ENV_FILE"

configure_commerce_balancer() {
  local status assignments tmp
  [[ $COMMERCE_BALANCER_URL =~ ^http://127\.0\.0\.1:[0-9]+$ ]] \
    || die "COMMERCE_BALANCER_URL must be a loopback HTTP origin with an explicit port"
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 3 \
    "$COMMERCE_BALANCER_URL/v1/ready" 2>/dev/null || true)
  [[ $status == 200 ]] || die "stable commerce balancer is not ready (HTTP ${status:-000})"

  assignments=$(awk -F= '$1 == "COMMERCE_BASE_URL" { found++ } END { print found + 0 }' "$ENV_FILE")
  (( assignments <= 1 )) || die "$ENV_FILE contains duplicate COMMERCE_BASE_URL assignments"
  if (( assignments == 1 )) && awk -F= -v expected="$COMMERCE_BALANCER_URL" '
    $1 == "COMMERCE_BASE_URL" { ok = ($2 == expected) }
    END { exit ok ? 0 : 1 }
  ' "$ENV_FILE"; then
    log "sales API already uses the stable commerce balancer"
    return 0
  fi

  tmp=$(mktemp "$(dirname -- "$ENV_FILE")/.sales-env.XXXXXX")
  ENV_TMP=$tmp
  chmod 0600 "$tmp"
  awk -F= -v url="$COMMERCE_BALANCER_URL" '
    BEGIN { replaced = 0 }
    $1 == "COMMERCE_BASE_URL" { print "COMMERCE_BASE_URL=" url; replaced = 1; next }
    { print }
    END { if (!replaced) print "COMMERCE_BASE_URL=" url }
  ' "$ENV_FILE" >"$tmp"
  chown --reference="$ENV_FILE" "$tmp"
  chmod --reference="$ENV_FILE" "$tmp"
  mv -f -- "$tmp" "$ENV_FILE"
  ENV_TMP=
  log "configured sales API to use the stable commerce balancer"
}

configure_commerce_balancer

health_ok() {
  local url=$1 code
  code=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 5 "$url" 2>/dev/null || true)
  [[ $code == 200 ]]
}

# The sign-in page returning 200 is NOT proof that a partner can sign in: the page renders its
# own shell, and the Telegram Login Widget is injected by a third-party script. A CSP that forbids
# that script leaves a perfectly healthy 200 page with no sign-in button at all — the exact way a
# broken login once shipped unnoticed. Gate on the two preconditions the button needs.
login_ok() {
  local headers policy
  headers=$(curl --noproxy '*' -sS -D - -o /dev/null --max-time 5 "$WEB_LOGIN" 2>/dev/null || true)
  grep -qiE '^HTTP/[0-9.]+ 200' <<<"$headers" || return 1
  policy=$(grep -i '^content-security-policy:' <<<"$headers" || true)
  # Exactly one policy: two headers make the browser enforce their intersection, which would
  # silently re-block the widget even though each header looks correct on its own.
  [[ $(grep -ci '^content-security-policy:' <<<"$headers") == 1 ]] || return 1
  grep -q "telegram.org" <<<"$policy" || return 1
  grep -q "unsafe-eval" <<<"$policy" || return 1
  # And the backend must actually name a bot, otherwise the widget has nothing to render.
  curl --noproxy '*' -sS --max-time 5 "$API_TELEGRAM_CONFIG" 2>/dev/null | grep -q '"botUsername":"[A-Za-z0-9_]\{5,\}"'
}

wait_healthy() {
  local i
  for (( i = 0; i < HEALTH_RETRIES; i++ )); do
    if health_ok "$API_HEALTH" && health_ok "$WEB_HEALTH" && login_ok; then
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

commit_sales_manifest() {
  local source=$1 temporary=${SALES_DB_MANIFEST}.tmp.$$
  cp -- "$source" "$temporary"
  chown root:deploy "$temporary"
  chmod 0640 "$temporary"
  mv -f -- "$temporary" "$SALES_DB_MANIFEST"
}

bootstrap_sales_manifest() {
  local previous_name previous_parent
  if [[ -e $SALES_DB_MANIFEST || -L $SALES_DB_MANIFEST ]]; then
    [[ -f $SALES_DB_MANIFEST && ! -L $SALES_DB_MANIFEST ]] \
      || die "sales migration manifest is not a regular file: $SALES_DB_MANIFEST"
    return 0
  fi
  [[ -n $previous_target && -d $previous_target && ! -L $previous_target ]] \
    || die "cannot bootstrap sales migration history without a live immutable release"
  previous_name=${previous_target##*/}
  previous_parent=${previous_target%/*}
  [[ $previous_parent == "$RELEASE_ROOT" && $previous_name =~ ^[0-9a-f]{40}$ ]] \
    || die "unsafe live release while bootstrapping sales migration history: $previous_target"
  MANIFEST_TMP=$(mktemp "$STATE_ROOT/.sales-migrations.bootstrap.XXXXXX")
  wd_sales_migration_manifest "$previous_target" >"$MANIFEST_TMP"
  commit_sales_manifest "$MANIFEST_TMP"
  rm -f -- "$MANIFEST_TMP"
  MANIFEST_TMP=
  log "bootstrapped append-only sales migration baseline from $previous_name"
}

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

# 2) Admit and apply only append-only sales history. A fresh validated all-database backup is
# mandatory before the first schema command for this exact SHA; app-only releases skip migrate.
bootstrap_sales_manifest
MANIFEST_TMP=$(mktemp "$STATE_ROOT/.sales-migrations.candidate.XXXXXX")
wd_sales_migration_manifest "$release" >"$MANIFEST_TMP"
wd_manifest_is_append_only "$SALES_DB_MANIFEST" "$MANIFEST_TMP" \
  || die "candidate edits or deletes already-applied sales migration history"
if [[ $(wd_manifest_digest "$SALES_DB_MANIFEST") != $(wd_manifest_digest "$MANIFEST_TMP") ]]; then
  [[ -x $BACKUP_RUNNER && ! -L $BACKUP_RUNNER ]] || die "validated backup runner is unavailable"
  log "new append-only sales migration history detected; creating validated backup"
  "$BACKUP_RUNNER" "$SHA"
  log "applying sales-db migrations from $SHA"
  # shellcheck disable=SC1090
  ( set -a; . "$ENV_FILE"; set +a; node "$release/packages/sales-db/dist/migrate.js" ) \
    || die "sales-db migration failed; leaving current symlink untouched ($previous_target)"
  commit_sales_manifest "$MANIFEST_TMP"
  log "committed tested sales migration manifest for $SHA"
else
  log "sales migration history is unchanged; skipping schema command"
fi
rm -f -- "$MANIFEST_TMP"
MANIFEST_TMP=

# 3) Atomically repoint current → new release, then restart units.
tmp_link="$RELEASE_ROOT/.current.$$"
ln -sfn -- "$release" "$tmp_link"
mv -T -- "$tmp_link" "$current_link"
log "sales-current → $SHA; restarting units"
restart_units

# 4) Health-gate; roll back the symlink on failure.
if wait_healthy; then
  log "sales $SHA healthy (api+web 200, sign-in page can load the Telegram widget); deploy complete"
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
