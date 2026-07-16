#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/lib.sh
source "$SCRIPT_DIR/lib.sh"

usage() {
  printf '%s\n' \
    'Usage: rollback.sh [options] [full-commit-sha]' \
    '' \
    'Repoint current to a previous immutable release. Without a SHA, each selected' \
    'component uses its recorded previous symlink. Database state is never changed.' \
    '' \
    'Options:' \
    '  --engine-only       Roll back and probe only the Rust engine' \
    '  --engine-bluegreen  Select only the engine rollback release; cut over slots separately' \
    '  --api-only          Select only the commerce API rollback release for blue-green cutover' \
    '  --timeout SECONDS   Readiness deadline per service (default: 60)' \
    '  --dry-run           Print changes without swapping or restarting' \
    '  -h, --help          Show this help'
}

DRY_RUN=0
ROLLBACK_ENGINE=1
ROLLBACK_API=1
RESTART_ENGINE=1
READINESS_TIMEOUT=${READINESS_TIMEOUT_SECONDS:-60}
SHA=
MODE_SELECTED=

while [[ $# -gt 0 ]]; do
  case "$1" in
    --engine-only)
      [[ -z "$MODE_SELECTED" ]] || die "--engine-only and --api-only are mutually exclusive"
      ROLLBACK_ENGINE=1
      ROLLBACK_API=0
      MODE_SELECTED=engine
      shift
      ;;
    --engine-bluegreen)
      [[ -z "$MODE_SELECTED" ]] || die "rollback modes are mutually exclusive"
      ROLLBACK_ENGINE=1
      ROLLBACK_API=0
      RESTART_ENGINE=0
      MODE_SELECTED=engine-bluegreen
      shift
      ;;
    --api-only)
      [[ -z "$MODE_SELECTED" ]] || die "--engine-only and --api-only are mutually exclusive"
      ROLLBACK_ENGINE=0
      ROLLBACK_API=1
      MODE_SELECTED=api
      shift
      ;;
    --timeout)
      [[ $# -ge 2 ]] || die "--timeout requires a value"
      READINESS_TIMEOUT=$2
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --*)
      die "unknown option: $1"
      ;;
    *)
      [[ -z "$SHA" ]] || die "only one release SHA may be supplied"
      SHA=$1
      shift
      ;;
  esac
done

if [[ -n "$SHA" ]]; then
  validate_sha "$SHA"
fi
validate_timeout "$READINESS_TIMEOUT"
validate_readiness_interval "${READINESS_INTERVAL_SECONDS:-2}"

COMMERCE_RELEASE_ROOT=$(canonicalize_release_root "${COMMERCE_RELEASE_ROOT:-/opt/apitoken/releases}" /opt/apitoken commerce)
ENGINE_RELEASE_ROOT=$(canonicalize_release_root "${ENGINE_RELEASE_ROOT:-/srv/claude-api/releases}" /srv/claude-api engine)
DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}
ENGINE_READY_URL=${ENGINE_READY_URL:-http://127.0.0.1:8787/ready}
ENGINE_POSTGRES_ENV=${ENGINE_POSTGRES_ENV:-/srv/claude-api/data/engine-postgres.env}
ENGINE_SERVICE=${ENGINE_SERVICE:-claude-api.service}

validate_service_unit "$ENGINE_SERVICE"

ENGINE_ORIGINAL=
ENGINE_TARGET=
API_ORIGINAL=
API_TARGET=

restart_selected_services() {
  if [[ "$ROLLBACK_ENGINE" == "1" && "$RESTART_ENGINE" == "1" ]]; then
    restart_units "$ENGINE_SERVICE"
  fi
}

recovery_restart_selected_services() {
  local failed=0
  if [[ "$ROLLBACK_ENGINE" == "1" && "$RESTART_ENGINE" == "1" ]]; then
    best_effort_restart_units "$ENGINE_SERVICE" || failed=1
  fi
  return "$failed"
}

readiness_ok() {
  local ok=0
  if [[ "$ROLLBACK_ENGINE" == "1" && "$RESTART_ENGINE" == "1" ]]; then
    wait_for_release_service engine engine "$ENGINE_SERVICE" "$ENGINE_RELEASE_ROOT" "$ENGINE_TARGET" "$ENGINE_READY_URL" "$READINESS_TIMEOUT" || ok=1
  fi
  return "$ok"
}

preflight_engine() {
  local previous
  capture_release_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/current"
  capture_release_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/previous"
  ENGINE_ORIGINAL=$(captured_link_target "$ENGINE_RELEASE_ROOT/current")
  if [[ -n "$ENGINE_ORIGINAL" ]]; then
    validate_engine_release "$ENGINE_RELEASE_ROOT" "$ENGINE_ORIGINAL" "$(basename -- "$ENGINE_ORIGINAL")"
  fi

  if [[ -n "$SHA" ]]; then
    ENGINE_TARGET="$ENGINE_RELEASE_ROOT/$SHA"
  else
    previous=$(captured_link_target "$ENGINE_RELEASE_ROOT/previous")
    [[ -n "$previous" ]] || die "no recorded previous engine release under $ENGINE_RELEASE_ROOT"
    ENGINE_TARGET=$previous
  fi
  validate_engine_release "$ENGINE_RELEASE_ROOT" "$ENGINE_TARGET" "$(basename -- "$ENGINE_TARGET")"
}

preflight_api() {
  local previous
  capture_release_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/current"
  capture_release_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/previous"
  API_ORIGINAL=$(captured_link_target "$COMMERCE_RELEASE_ROOT/current")
  if [[ -n "$API_ORIGINAL" ]]; then
    validate_commerce_release "$COMMERCE_RELEASE_ROOT" "$API_ORIGINAL" "$(basename -- "$API_ORIGINAL")"
  fi

  if [[ -n "$SHA" ]]; then
    API_TARGET="$COMMERCE_RELEASE_ROOT/$SHA"
  else
    previous=$(captured_link_target "$COMMERCE_RELEASE_ROOT/previous")
    [[ -n "$previous" ]] || die "no recorded previous commerce release under $COMMERCE_RELEASE_ROOT"
    API_TARGET=$previous
  fi
  validate_commerce_release "$COMMERCE_RELEASE_ROOT" "$API_TARGET" "$(basename -- "$API_TARGET")"
}

activate_rollback_links() {
  if [[ "$ROLLBACK_ENGINE" == "1" && "$ENGINE_TARGET" != "$ENGINE_ORIGINAL" ]]; then
    if [[ -n "$ENGINE_ORIGINAL" ]]; then
      set_journaled_release_link "$ENGINE_ORIGINAL" "$ENGINE_RELEASE_ROOT/previous"
    fi
    set_journaled_release_link "$ENGINE_TARGET" "$ENGINE_RELEASE_ROOT/current"
    log "engine current -> $(basename -- "$ENGINE_TARGET")"
  elif [[ "$ROLLBACK_ENGINE" == "1" ]]; then
    log "engine already runs $(basename -- "$ENGINE_TARGET"); preserving the real previous link"
  fi

  if [[ "$ROLLBACK_API" == "1" && "$API_TARGET" != "$API_ORIGINAL" ]]; then
    if [[ -n "$API_ORIGINAL" ]]; then
      set_journaled_release_link "$API_ORIGINAL" "$COMMERCE_RELEASE_ROOT/previous"
    fi
    set_journaled_release_link "$API_TARGET" "$COMMERCE_RELEASE_ROOT/current"
    log "commerce API current -> $(basename -- "$API_TARGET")"
  elif [[ "$ROLLBACK_API" == "1" ]]; then
    log "commerce API already runs $(basename -- "$API_TARGET"); preserving the real previous link"
  fi

  if [[ "$DRY_RUN" != "1" ]]; then
    if [[ "$ROLLBACK_ENGINE" == "1" ]]; then
      [[ "$(release_path_from_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/current")" == "$ENGINE_TARGET" ]] || die "engine rollback link did not reach its target"
    fi
    if [[ "$ROLLBACK_API" == "1" ]]; then
      [[ "$(release_path_from_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/current")" == "$API_TARGET" ]] || die "commerce rollback link did not reach its target"
    fi
  fi
}

log "rolling back immutable releases (engine=$ROLLBACK_ENGINE engine-restart=$RESTART_ENGINE api=$ROLLBACK_API dry-run=$DRY_RUN)"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
if [[ "$ROLLBACK_ENGINE" == "1" && "$RESTART_ENGINE" == "1" ]] \
  && privileged_command test -s "$ENGINE_POSTGRES_ENV"; then
  die "PostgreSQL engine slots are active; use --engine-bluegreen, then engine-bluegreen.sh"
fi

# Full preflight for every selected component happens before any current/previous link mutation.
if [[ "$ROLLBACK_ENGINE" == "1" ]]; then
  preflight_engine
fi
if [[ "$ROLLBACK_API" == "1" ]]; then
  preflight_api
fi

begin_activation recovery_restart_selected_services
activate_rollback_links
restart_selected_services
if ! readiness_ok; then
  die "rollback target failed exact-unit readiness; original links and services are being restored"
fi

commit_activation
if [[ "$DRY_RUN" == "1" ]]; then
  log "dry-run complete; no symlink, service, lock, or database state changed"
else
  if [[ "$ROLLBACK_ENGINE" == "1" && "$RESTART_ENGINE" == "1" ]]; then
    log "engine rollback completed and its exact unit serves the requested release"
  elif [[ "$ROLLBACK_ENGINE" == "1" ]]; then
    log "engine rollback release selected by releases/current; engine slots were not restarted"
    log "run $SCRIPT_DIR/engine-bluegreen.sh to cut the PostgreSQL engine over without downtime"
  fi
  if [[ "$ROLLBACK_API" == "1" ]]; then
    log "commerce rollback release selected by releases/current; API slots were not restarted"
    log "run $SCRIPT_DIR/api-bluegreen.sh to cut the commerce API over without downtime"
  fi
  log "database migrations were not changed"
fi
