#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/lib.sh
source "$SCRIPT_DIR/lib.sh"

usage() {
  printf '%s\n' \
    'Usage: deploy.sh [options] <full-commit-sha>' \
    '' \
    'Build and finalize an immutable, tested release.' \
    '' \
    'Options:' \
    '  --engine-only       Build, activate, restart, and probe only the Rust engine' \
    '  --api-only          Build, migrate, and point current for a later blue-green API cutover' \
    '  --bootstrap         Prepare the first release, create both current links, then install/start units' \
    '  --skip-migrate      Skip the commerce database migration (explicit override)' \
    '  --timeout SECONDS   Readiness deadline per service (default: 60)' \
    '  --dry-run           Print changes without fetching, building, swapping, or changing services' \
    '  -h, --help          Show this help'
}

DRY_RUN=0
DEPLOY_ENGINE=1
DEPLOY_API=1
BOOTSTRAP=0
SKIP_MIGRATE=0
READINESS_TIMEOUT=${READINESS_TIMEOUT_SECONDS:-60}
SHA=
MODE_SELECTED=

while [[ $# -gt 0 ]]; do
  case "$1" in
    --engine-only)
      [[ -z "$MODE_SELECTED" ]] || die "--engine-only and --api-only are mutually exclusive"
      DEPLOY_ENGINE=1
      DEPLOY_API=0
      MODE_SELECTED=engine
      shift
      ;;
    --api-only)
      [[ -z "$MODE_SELECTED" ]] || die "--engine-only and --api-only are mutually exclusive"
      DEPLOY_ENGINE=0
      DEPLOY_API=1
      MODE_SELECTED=api
      shift
      ;;
    --bootstrap)
      BOOTSTRAP=1
      shift
      ;;
    --skip-migrate)
      SKIP_MIGRATE=1
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

[[ -n "$SHA" ]] || { usage >&2; die "a full commit SHA is required"; }
validate_sha "$SHA"
validate_timeout "$READINESS_TIMEOUT"
validate_readiness_interval "${READINESS_INTERVAL_SECONDS:-2}"
if [[ "$BOOTSTRAP" == "1" ]]; then
  [[ "$DEPLOY_ENGINE" == "1" && "$DEPLOY_API" == "1" ]] || die "--bootstrap must prepare both engine and commerce current links atomically"
fi
if [[ "$DEPLOY_API" != "1" && "$SKIP_MIGRATE" == "1" ]]; then
  warn "--skip-migrate has no effect when the commerce API is not selected"
fi

SOURCE_REPO=${SOURCE_REPO:-/opt/apitoken/repo}
COMMERCE_RELEASE_ROOT=$(canonicalize_release_root "${COMMERCE_RELEASE_ROOT:-/opt/apitoken/releases}" /opt/apitoken commerce)
ENGINE_RELEASE_ROOT=$(canonicalize_release_root "${ENGINE_RELEASE_ROOT:-/srv/claude-api/releases}" /srv/claude-api engine)
API_ENV_FILE=${API_ENV_FILE:-/etc/apitoken/api.env}
DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}
MIGRATION_LOCK_FILE=${MIGRATION_LOCK_FILE:-/run/lock/apitoken-db-migrate.lock}
API_READY_URL=${API_READY_URL:-http://127.0.0.1:3000/v1/ready}
ENGINE_READY_URL=${ENGINE_READY_URL:-http://127.0.0.1:8787/ready}
API_SERVICE=${API_SERVICE:-apitoken-api@3000.service}
ENGINE_SERVICE=${ENGINE_SERVICE:-claude-api.service}
LEGACY_API_SERVICE=${LEGACY_API_SERVICE:-apitoken-api.service}
SYSTEMD_UNIT_DIR=${SYSTEMD_UNIT_DIR:-/etc/systemd/system}

[[ "$SOURCE_REPO" == /* ]] || die "SOURCE_REPO must be absolute"
[[ "$API_ENV_FILE" == /* ]] || die "API_ENV_FILE must be absolute"
[[ "$SYSTEMD_UNIT_DIR" == "/etc/systemd/system" ]] || die "SYSTEMD_UNIT_DIR is fixed at /etc/systemd/system"
validate_service_unit "$API_SERVICE"
validate_service_unit "$ENGINE_SERVICE"
validate_service_unit "$LEGACY_API_SERVICE"
[[ "$API_SERVICE" != "$LEGACY_API_SERVICE" ]] || die "template and legacy API units must be different"

COMMERCE_RELEASE="$COMMERCE_RELEASE_ROOT/$SHA"
ENGINE_RELEASE="$ENGINE_RELEASE_ROOT/$SHA"
COMMERCE_STAGE=
ENGINE_STAGE=
ENGINE_SOURCE_STAGE=
ENGINE_SOURCE_DIR=
ENGINE_ORIGINAL=
API_ORIGINAL=

cleanup() {
  local path
  for path in "$COMMERCE_STAGE" "$ENGINE_STAGE" "$ENGINE_SOURCE_STAGE"; do
    [[ -n "$path" && -e "$path" ]] || continue
    case "$(basename -- "$path")" in
      *.tmp."$$") run rm -rf -- "$path" ;;
      *) warn "refusing to remove unexpected staging path $path" ;;
    esac
  done
}
trap cleanup EXIT

write_release_marker() {
  local directory=$1
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run: would write $directory/.release-sha"
  else
    printf '%s\n' "$SHA" >"$directory/.release-sha"
  fi
}

validate_commerce_stage() {
  local directory=$1
  [[ -r "$directory/apps/api/dist/main.js" ]] || die "staged commerce API artifact is missing: $directory/apps/api/dist/main.js"
  [[ -r "$directory/packages/db/dist/migrate.js" ]] || die "staged prebuilt migration artifact is missing: $directory/packages/db/dist/migrate.js"
}

validate_engine_stage() {
  local directory=$1
  [[ -x "$directory/claude-api" ]] || die "staged engine binary is missing: $directory/claude-api"
}

fetch_release_commit() {
  log "fetching tested commit $SHA from origin"
  run git -C "$SOURCE_REPO" fetch --no-tags origin "$SHA"
  if [[ "$DRY_RUN" != "1" ]]; then
    local fetched
    fetched=$(git -C "$SOURCE_REPO" rev-parse 'FETCH_HEAD^{commit}')
    [[ "$fetched" == "$SHA" ]] || die "origin returned $fetched instead of requested $SHA"
  fi
}

checkout_stage() {
  local destination=$1
  run git clone --no-hardlinks --no-checkout "$SOURCE_REPO" "$destination"
  run git -C "$destination" checkout --detach "$SHA"
  if [[ "$DRY_RUN" != "1" ]]; then
    [[ "$(git -C "$destination" rev-parse HEAD)" == "$SHA" ]] || die "checkout verification failed in $destination"
  fi
}

prepare_commerce_release() {
  if [[ -e "$COMMERCE_RELEASE" || -L "$COMMERCE_RELEASE" ]]; then
    [[ -d "$COMMERCE_RELEASE" && ! -L "$COMMERCE_RELEASE" ]] || die "commerce release path exists but is not an immutable directory: $COMMERCE_RELEASE"
    validate_commerce_release "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE" "$SHA"
    log "reusing validated immutable commerce release $COMMERCE_RELEASE"
    return 0
  fi

  COMMERCE_STAGE="$COMMERCE_RELEASE_ROOT/.${SHA}.tmp.$$"
  [[ ! -e "$COMMERCE_STAGE" && ! -L "$COMMERCE_STAGE" ]] || die "staging path already exists: $COMMERCE_STAGE"
  log "creating commerce release $COMMERCE_RELEASE"
  checkout_stage "$COMMERCE_STAGE"
  run pnpm --dir "$COMMERCE_STAGE" install --frozen-lockfile
  run pnpm --dir "$COMMERCE_STAGE" build
  # db:migrate itself builds; deployment must instead prebuild this artifact before finalization.
  run pnpm --dir "$COMMERCE_STAGE" --filter @claude-api/db build
  if [[ "$DRY_RUN" != "1" ]]; then
    validate_commerce_stage "$COMMERCE_STAGE"
  fi
  write_release_marker "$COMMERCE_STAGE"
  freeze_release_tree "$COMMERCE_STAGE"
  run mv -- "$COMMERCE_STAGE" "$COMMERCE_RELEASE"
  COMMERCE_STAGE=
  if [[ "$DRY_RUN" != "1" ]]; then
    validate_commerce_release "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE" "$SHA"
  fi
}

prepare_engine_source() {
  if [[ "$DEPLOY_API" == "1" ]]; then
    ENGINE_SOURCE_DIR=$COMMERCE_RELEASE
    return 0
  fi

  ENGINE_SOURCE_STAGE="${TMPDIR:-/tmp}/apitoken-engine-${SHA}.tmp.$$"
  [[ ! -e "$ENGINE_SOURCE_STAGE" && ! -L "$ENGINE_SOURCE_STAGE" ]] || die "staging path already exists: $ENGINE_SOURCE_STAGE"
  checkout_stage "$ENGINE_SOURCE_STAGE"
  ENGINE_SOURCE_DIR=$ENGINE_SOURCE_STAGE
}

prepare_engine_release() {
  if [[ -e "$ENGINE_RELEASE" || -L "$ENGINE_RELEASE" ]]; then
    [[ -d "$ENGINE_RELEASE" && ! -L "$ENGINE_RELEASE" ]] || die "engine release path exists but is not an immutable directory: $ENGINE_RELEASE"
    validate_engine_release "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE" "$SHA"
    log "reusing validated immutable engine release $ENGINE_RELEASE"
    return 0
  fi

  prepare_engine_source
  ENGINE_STAGE="$ENGINE_RELEASE_ROOT/.${SHA}.tmp.$$"
  [[ ! -e "$ENGINE_STAGE" && ! -L "$ENGINE_STAGE" ]] || die "staging path already exists: $ENGINE_STAGE"
  run install -d -- "$ENGINE_STAGE"
  log "building engine release $ENGINE_RELEASE"
  run env CARGO_TARGET_DIR="$ENGINE_STAGE/target" cargo build --locked --release -p claude-api --manifest-path "$ENGINE_SOURCE_DIR/Cargo.toml"
  run install -m 0755 -- "$ENGINE_STAGE/target/release/claude-api" "$ENGINE_STAGE/claude-api"
  run rm -rf -- "$ENGINE_STAGE/target"
  if [[ "$DRY_RUN" != "1" ]]; then
    validate_engine_stage "$ENGINE_STAGE"
  fi
  write_release_marker "$ENGINE_STAGE"
  freeze_release_tree "$ENGINE_STAGE"
  run mv -- "$ENGINE_STAGE" "$ENGINE_RELEASE"
  ENGINE_STAGE=
  if [[ "$DRY_RUN" != "1" ]]; then
    validate_engine_release "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE" "$SHA"
  fi
}

run_locked_migration() {
  if [[ "$SKIP_MIGRATE" == "1" ]]; then
    warn "commerce migration skipped by explicit --skip-migrate override"
    return 0
  fi

  if [[ "$DRY_RUN" == "1" ]]; then
    acquire_migration_lock "$MIGRATION_LOCK_FILE"
    log "dry-run: would load $API_ENV_FILE without printing secrets"
    print_command node "$COMMERCE_RELEASE/packages/db/dist/migrate.js"
    return 0
  fi

  # api.env is root-only (0600, root:root) — the commercial service reads it via systemd
  # EnvironmentFile as root, and the deploy user must NOT need read access to the secret file.
  # So both the existence check AND the migration run happen as root via privileged_command; the
  # OS deploy-serialization lock is still held by this (deploy) shell through fd 8.
  privileged_command test -f "$API_ENV_FILE" || die "API environment file not found: $API_ENV_FILE"
  validate_commerce_release "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE" "$SHA"
  log "running prebuilt commerce database migration before API switchover"
  (
    acquire_migration_lock "$MIGRATION_LOCK_FILE"
    privileged_command bash -c 'set -a; . "$0"; set +a; exec node "$1"' \
      "$API_ENV_FILE" "$COMMERCE_RELEASE/packages/db/dist/migrate.js"
  )
}

restart_selected_services() {
  # Normal commerce API lifecycle is owned exclusively by api-bluegreen.sh. Moving
  # releases/current must not restart the still-serving slot onto the new release.
  if [[ "$DEPLOY_ENGINE" == "1" ]]; then
    restart_units "$ENGINE_SERVICE"
  fi
}

recovery_restart_selected_services() {
  local failed=0
  if [[ "$DEPLOY_ENGINE" == "1" ]]; then
    best_effort_restart_units "$ENGINE_SERVICE" || failed=1
  fi
  return "$failed"
}

bootstrap_recovery_services() {
  local failed=0

  if systemctl_raw disable --now "$API_SERVICE"; then
    log "recovery disabled failed bootstrap unit $API_SERVICE"
  else
    warn "recovery failed to disable/stop $API_SERVICE"
    failed=1
  fi
  if systemctl_raw stop "$ENGINE_SERVICE"; then
    log "recovery stopped $ENGINE_SERVICE because bootstrap restored an absent engine current link"
  else
    warn "recovery failed to stop $ENGINE_SERVICE"
    failed=1
  fi
  if systemctl_raw enable "$LEGACY_API_SERVICE"; then
    log "recovery enabled $LEGACY_API_SERVICE"
  else
    warn "recovery failed to enable $LEGACY_API_SERVICE"
    failed=1
  fi
  if systemctl_raw restart "$LEGACY_API_SERVICE"; then
    log "recovery restarted $LEGACY_API_SERVICE"
  else
    warn "recovery failed to restart $LEGACY_API_SERVICE"
    failed=1
  fi
  return "$failed"
}

require_unit_active_now() {
  local unit=$1
  validate_service_unit "$unit"
  if [[ "$DRY_RUN" == "1" ]]; then
    log "dry-run: would immediately require $unit to be active"
    return 0
  fi
  systemctl_raw is-active --quiet "$unit" || die "$unit is not active immediately after start/restart"
}

readiness_ok() {
  local ok=0
  if [[ "$DEPLOY_ENGINE" == "1" ]]; then
    wait_for_release_service engine engine "$ENGINE_SERVICE" "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE" "$ENGINE_READY_URL" "$READINESS_TIMEOUT" || ok=1
  fi
  return "$ok"
}

install_bootstrap_units() {
  if [[ "$DRY_RUN" != "1" ]]; then
    [[ -f "$COMMERCE_RELEASE/systemd/apitoken-api@.service" ]] || die "bootstrap API unit artifact is missing from release"
    [[ -f "$COMMERCE_RELEASE/systemd/claude-api.service" ]] || die "bootstrap engine unit artifact is missing from release"
  fi
  privileged_command install -m 0644 -- "$COMMERCE_RELEASE/systemd/apitoken-api@.service" "$SYSTEMD_UNIT_DIR/apitoken-api@.service"
  privileged_command install -m 0644 -- "$COMMERCE_RELEASE/systemd/claude-api.service" "$SYSTEMD_UNIT_DIR/claude-api.service"
  systemctl_command daemon-reload
}

bootstrap_services() {
  install_bootstrap_units

  # The engine unit keeps its name (claude-api.service) and is already running the pre-bootstrap
  # binary, so `enable --now` would NOT reload it. Enable for boot, then restart to bind the new
  # ExecStart (releases/current -> new release). SIGTERM drains + releases the flock before the new
  # process binds, so there is no flock/port overlap.
  systemctl_command enable "$ENGINE_SERVICE"
  restart_units "$ENGINE_SERVICE"
  require_unit_active_now "$ENGINE_SERVICE"
  wait_for_release_service engine engine "$ENGINE_SERVICE" "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE" "$ENGINE_READY_URL" "$READINESS_TIMEOUT"

  # The legacy API remains serving until both current links exist, validate, and systemd has reloaded.
  systemctl_command stop "$LEGACY_API_SERVICE"
  systemctl_command enable --now "$API_SERVICE"
  require_unit_active_now "$API_SERVICE"
  wait_for_release_service commerce-api api "$API_SERVICE" "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE" "$API_READY_URL" "$READINESS_TIMEOUT"
  systemctl_command disable "$LEGACY_API_SERVICE"
}

preflight_links() {
  if [[ "$DEPLOY_ENGINE" == "1" ]]; then
    capture_release_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/current"
    capture_release_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/previous"
    ENGINE_ORIGINAL=$(captured_link_target "$ENGINE_RELEASE_ROOT/current")
  fi
  if [[ "$DEPLOY_API" == "1" ]]; then
    capture_release_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/current"
    capture_release_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/previous"
    API_ORIGINAL=$(captured_link_target "$COMMERCE_RELEASE_ROOT/current")
  fi

  if [[ "$BOOTSTRAP" == "1" ]]; then
    [[ -z "$ENGINE_ORIGINAL" && -z "$API_ORIGINAL" ]] || die "--bootstrap requires both current links to be genuinely absent"
    if [[ "$DRY_RUN" != "1" ]]; then
      systemctl_raw is-active --quiet "$LEGACY_API_SERVICE" || die "$LEGACY_API_SERVICE must be active before bootstrap handoff"
    else
      log "dry-run: would require legacy unit $LEGACY_API_SERVICE active before handoff"
    fi
  else
    if [[ "$DEPLOY_ENGINE" == "1" ]]; then
      [[ -n "$ENGINE_ORIGINAL" ]] || die "engine current is absent; use a full --bootstrap deploy first"
      validate_engine_release "$ENGINE_RELEASE_ROOT" "$ENGINE_ORIGINAL" "$(basename -- "$ENGINE_ORIGINAL")"
    fi
    if [[ "$DEPLOY_API" == "1" ]]; then
      [[ -n "$API_ORIGINAL" ]] || die "commerce current is absent; use a full --bootstrap deploy first"
      validate_commerce_release "$COMMERCE_RELEASE_ROOT" "$API_ORIGINAL" "$(basename -- "$API_ORIGINAL")"
    fi
  fi
}

activate_release_links() {
  if [[ "$DEPLOY_ENGINE" == "1" && "$ENGINE_ORIGINAL" != "$ENGINE_RELEASE" ]]; then
    if [[ -n "$ENGINE_ORIGINAL" ]]; then
      set_journaled_release_link "$ENGINE_ORIGINAL" "$ENGINE_RELEASE_ROOT/previous"
    fi
    log "activating engine release $SHA"
    set_journaled_release_link "$ENGINE_RELEASE" "$ENGINE_RELEASE_ROOT/current"
  elif [[ "$DEPLOY_ENGINE" == "1" ]]; then
    log "engine already runs release $SHA; preserving the real previous link"
  fi

  if [[ "$DEPLOY_API" == "1" && "$API_ORIGINAL" != "$COMMERCE_RELEASE" ]]; then
    if [[ -n "$API_ORIGINAL" ]]; then
      set_journaled_release_link "$API_ORIGINAL" "$COMMERCE_RELEASE_ROOT/previous"
    fi
    log "activating commerce API release $SHA"
    set_journaled_release_link "$COMMERCE_RELEASE" "$COMMERCE_RELEASE_ROOT/current"
  elif [[ "$DEPLOY_API" == "1" ]]; then
    log "commerce API already runs release $SHA; preserving the real previous link"
  fi

  if [[ "$DRY_RUN" != "1" ]]; then
    if [[ "$DEPLOY_ENGINE" == "1" ]]; then
      [[ "$(release_path_from_link "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE_ROOT/current")" == "$ENGINE_RELEASE" ]] || die "engine current did not activate target release"
    fi
    if [[ "$DEPLOY_API" == "1" ]]; then
      [[ "$(release_path_from_link "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE_ROOT/current")" == "$COMMERCE_RELEASE" ]] || die "commerce current did not activate target release"
    fi
  fi
}

log "deploying immutable release $SHA (engine=$DEPLOY_ENGINE api=$DEPLOY_API bootstrap=$BOOTSTRAP dry-run=$DRY_RUN)"
acquire_deploy_lock "$DEPLOY_LOCK_FILE"
# Reject invalid current/previous links before builds, migrations, or any activation mutation.
preflight_links
if [[ "$DEPLOY_API" == "1" ]]; then
  run install -d -- "$COMMERCE_RELEASE_ROOT"
fi
if [[ "$DEPLOY_ENGINE" == "1" ]]; then
  run install -d -- "$ENGINE_RELEASE_ROOT"
fi
fetch_release_commit

if [[ "$DEPLOY_API" == "1" ]]; then
  prepare_commerce_release
fi
if [[ "$DEPLOY_ENGINE" == "1" ]]; then
  prepare_engine_release
fi
if [[ "$DEPLOY_API" == "1" ]]; then
  run_locked_migration
fi

if [[ "$DRY_RUN" != "1" ]]; then
  if [[ "$DEPLOY_API" == "1" ]]; then
    validate_commerce_release "$COMMERCE_RELEASE_ROOT" "$COMMERCE_RELEASE" "$SHA"
  fi
  if [[ "$DEPLOY_ENGINE" == "1" ]]; then
    validate_engine_release "$ENGINE_RELEASE_ROOT" "$ENGINE_RELEASE" "$SHA"
  fi
fi

# No staging path remains by the time activation traps replace the cleanup trap.
cleanup
trap - EXIT
if [[ "$BOOTSTRAP" == "1" ]]; then
  begin_activation bootstrap_recovery_services
else
  begin_activation recovery_restart_selected_services
fi

activate_release_links
if [[ "$BOOTSTRAP" == "1" ]]; then
  bootstrap_services
else
  restart_selected_services
  if ! readiness_ok; then
    die "release $SHA failed exact-unit readiness; automatic link and service recovery has started"
  fi
fi

commit_activation
if [[ "$DRY_RUN" == "1" ]]; then
  log "dry-run complete; no release, symlink, unit, service, lock, or database state changed"
else
  if [[ "$DEPLOY_ENGINE" == "1" ]]; then
    log "engine release $SHA is active on its exact unit and passed readiness"
  fi
  if [[ "$DEPLOY_API" == "1" && "$BOOTSTRAP" == "1" ]]; then
    log "commerce bootstrap handoff completed on $API_SERVICE for release $SHA"
  elif [[ "$DEPLOY_API" == "1" ]]; then
    log "commerce release $SHA is finalized, migrated, and selected by releases/current; API slots were not restarted"
    log "run $SCRIPT_DIR/api-bluegreen.sh to cut the commerce API over without downtime"
  fi
fi
