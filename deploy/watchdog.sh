#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$SCRIPT_DIR/watchdog-lib.sh"

SOURCE_REPO=/opt/apitoken/repo
REMOTE=origin
BRANCH=master
STATE_ROOT=/var/lib/apitoken/watchdog
CANDIDATE_ROOT=$STATE_ROOT/candidates
CI_USER=apitoken-ci
CI_HOME=$STATE_ROOT/ci-home
CI_TOOLCHAIN=/opt/apitoken-watchdog/rust-toolchain
CONTROLLER_ROOT=/usr/local/lib/apitoken-watchdog/controller
TEST_DB_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-test-db
WATCHDOG_LOCK=/run/lock/apitoken-watchdog.lock
DEPLOY_LOCK=/run/lock/apitoken-deploy.lock
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
COMMERCE_RELEASE_ROOT=/opt/apitoken/releases

PROCESSED_FILE=$STATE_ROOT/processed.sha
ENGINE_FILE=$STATE_ROOT/engine.sha
BACKEND_FILE=$STATE_ROOT/backend.sha
REJECTED_FILE=$STATE_ROOT/rejected.sha
PENDING_MIGRATION_FILE=$STATE_ROOT/pending-migration.sha
PENDING_INFRA_FILE=$STATE_ROOT/pending-infrastructure.sha
INFRA_APPROVED_FILE=$STATE_ROOT/infrastructure-approved.sha
DB_MANIFEST=$STATE_ROOT/database-migrations.manifest
STATUS_FILE=$STATE_ROOT/status

CURRENT_PHASE=starting
TEST_DB_STARTED=0

status() {
  local detail=$1
  wd_atomic_write "$STATUS_FILE" "phase=$CURRENT_PHASE sha=${CANDIDATE_SHA:-none} detail=$detail updated_at=$(date -u +%FT%TZ)"
}

fail() {
  local rc=$? line=${BASH_LINENO[0]:-unknown}
  trap - ERR EXIT INT TERM
  if (( TEST_DB_STARTED == 1 )); then
    sudo -n "$TEST_DB_HELPER" stop >/dev/null 2>&1 || true
  fi
  if [[ -n ${CANDIDATE_SHA:-} && $CANDIDATE_SHA =~ ^[0-9a-f]{40}$ ]]; then
    wd_atomic_write "$REJECTED_FILE" "$CANDIDATE_SHA"
  fi
  CURRENT_PHASE=failed
  status "command failed at line $line (exit $rc); candidate quarantined"
  wd_warn "candidate ${CANDIDATE_SHA:-unknown} failed at line $line and will not be retried automatically"
  exit "$rc"
}
trap fail ERR INT TERM

require_fixed_file() {
  local path=$1 owner
  [[ -f $path && ! -L $path ]] || wd_die "required regular file is missing: $path"
  owner=$(stat -c '%u' -- "$path")
  [[ $owner == 0 ]] || wd_die "required file must be root-owned: $path"
}

require_fixed_directory() {
  local path=$1 owner
  [[ -d $path && ! -L $path ]] || wd_die "required directory is missing: $path"
  owner=$(stat -c '%u' -- "$path")
  [[ $owner == 0 ]] || wd_die "required directory must be root-owned: $path"
}

marker_for() {
  printf '%s/%s.tested\n' "$STATE_ROOT" "$1"
}

candidate_for() {
  printf '%s/%s\n' "$CANDIDATE_ROOT" "$1"
}

candidate_is_tested() {
  local sha=$1 marker candidate marker_sha marker_digest actual_digest
  marker=$(marker_for "$sha")
  candidate=$(candidate_for "$sha")
  [[ -d $candidate && ! -L $candidate ]] || return 1
  marker_sha=$(wd_marker_value "$marker" sha 2>/dev/null) || return 1
  [[ $marker_sha == "$sha" ]] || return 1
  marker_digest=$(wd_marker_value "$marker" migration_digest 2>/dev/null) || return 1
  wd_migration_manifest "$candidate" >"$STATE_ROOT/.candidate-manifest.$$"
  actual_digest=$(wd_manifest_digest "$STATE_ROOT/.candidate-manifest.$$")
  rm -f -- "$STATE_ROOT/.candidate-manifest.$$"
  [[ $marker_digest == "$actual_digest" ]]
}

run_as_ci() {
  sudo -n -u "$CI_USER" env \
    HOME="$CI_HOME" \
    CARGO_HOME="$CI_HOME/.cargo" \
    PATH="$CI_TOOLCHAIN/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$@"
}

prepare_and_test_candidate() {
  local sha=$1 candidate marker dsn manifest digest tree
  candidate=$(candidate_for "$sha")
  marker=$(marker_for "$sha")

  if candidate_is_tested "$sha"; then
    wd_log "reusing test-passed immutable candidate $sha"
    return 0
  fi

  CURRENT_PHASE=testing
  status "preparing isolated candidate"
  if [[ -e $candidate || -L $candidate ]]; then
    sudo -n chmod -R u+w -- "$candidate" 2>/dev/null || true
    sudo -n rm -rf --one-file-system -- "$candidate"
  fi
  rm -f -- "$marker"

  git clone --no-hardlinks --no-checkout "$SOURCE_REPO" "$candidate"
  git -C "$candidate" checkout --detach "$sha"
  [[ $(git -C "$candidate" rev-parse HEAD) == "$sha" ]] || wd_die "candidate checkout mismatch"
  sudo -n chown -R "$CI_USER:$CI_USER" -- "$candidate"

  wd_log "running frozen install and complete build for $sha as isolated user $CI_USER"
  run_as_ci pnpm --dir "$candidate" install --frozen-lockfile
  run_as_ci pnpm --dir "$candidate" build
  run_as_ci pnpm --dir "$candidate" typecheck

  dsn=$(sudo -n "$TEST_DB_HELPER" start)
  TEST_DB_STARTED=1
  wd_log "running commerce migrations and all TypeScript tests against disposable PostgreSQL"
  run_as_ci env DATABASE_URL="$dsn" node "$candidate/packages/db/dist/migrate.js"
  run_as_ci env TEST_DATABASE_URL="$dsn" pnpm --dir "$candidate" \
    -r --workspace-concurrency=1 --if-present test

  wd_log "running all Rust workspace tests"
  run_as_ci cargo test --locked --workspace --manifest-path "$candidate/Cargo.toml"

  wd_log "checking tracked whitespace and shell syntax"
  if [[ -n ${PROCESSED_SHA:-} && $PROCESSED_SHA != "$sha" ]]; then
    git -C "$SOURCE_REPO" diff --check "$PROCESSED_SHA..$sha"
  fi
  while IFS= read -r -d '' shell_file; do
    bash -n "$shell_file"
  done < <(find "$candidate/deploy" -type f -name '*.sh' -print0)

  sudo -n "$TEST_DB_HELPER" stop
  TEST_DB_STARTED=0

  [[ -z $(run_as_ci git -C "$candidate" status --porcelain --untracked-files=no) ]] \
    || wd_die "tests modified tracked candidate files"
  manifest="$STATE_ROOT/.candidate-manifest.$$"
  wd_migration_manifest "$candidate" >"$manifest"
  digest=$(wd_manifest_digest "$manifest")
  tree=$(run_as_ci git -C "$candidate" rev-parse 'HEAD^{tree}')
  {
    printf 'sha=%s\n' "$sha"
    printf 'tree=%s\n' "$tree"
    printf 'migration_digest=%s\n' "$digest"
    printf 'completed_at=%s\n' "$(date -u +%FT%TZ)"
  } >"${marker}.tmp.$$"
  chmod 0640 "${marker}.tmp.$$"
  mv -f -- "${marker}.tmp.$$" "$marker"
  rm -f -- "$manifest"

  # Manual migration consumes the exact tested build. Root ownership plus removed write bits keeps
  # it stable between the green test result and explicit operator approval.
  sudo -n chown -R root:root -- "$candidate"
  sudo -n chmod -R a-w -- "$candidate"
  wd_log "candidate $sha passed the complete isolated test gate"
}

final_verify_engine() {
  local sha=$1 current
  current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
  [[ $current == "$ENGINE_RELEASE_ROOT/$sha" ]] || wd_die "engine current is not $sha after cutover"
  systemctl is-active --quiet claude-api@8787.service \
    || systemctl is-active --quiet claude-api@8788.service \
    || wd_die "no engine slot is active after cutover"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:8790/ready >/dev/null
}

final_verify_backend() {
  local sha=$1 current worker_pid worker_cwd
  current=$(readlink -f -- "$COMMERCE_RELEASE_ROOT/current")
  [[ $current == "$COMMERCE_RELEASE_ROOT/$sha" ]] || wd_die "commerce current is not $sha after cutover"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:3000/v1/ready >/dev/null \
    || curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:3001/v1/ready >/dev/null \
    || wd_die "no commerce API slot is ready after cutover"
  systemctl is-active --quiet apitoken-worker.service || wd_die "worker is not active after cutover"
  worker_pid=$(systemctl show apitoken-worker.service -p MainPID --value)
  [[ $worker_pid =~ ^[1-9][0-9]*$ ]] || wd_die "worker has no MainPID"
  worker_cwd=$(readlink -f -- "/proc/$worker_pid/cwd")
  [[ $worker_cwd == "$COMMERCE_RELEASE_ROOT/$sha/apps/worker" ]] \
    || wd_die "worker is not running immutable release $sha (cwd=$worker_cwd)"
}

deploy_engine() {
  local sha=$1
  CURRENT_PHASE=deploying-engine
  status "building and blue-green deploying engine"
  "$CONTROLLER_ROOT/deploy.sh" --engine-bluegreen "$sha"
  "$CONTROLLER_ROOT/engine-bluegreen.sh"
  final_verify_engine "$sha"
  wd_atomic_write "$ENGINE_FILE" "$sha"
  wd_log "engine $sha passed final production verification"
}

deploy_backend() {
  local sha=$1
  CURRENT_PHASE=deploying-backend
  status "building and blue-green deploying API plus worker; migrations explicitly skipped"
  "$CONTROLLER_ROOT/deploy.sh" --api-only --skip-migrate "$sha"
  "$CONTROLLER_ROOT/api-bluegreen.sh" --with-worker
  final_verify_backend "$sha"
  wd_atomic_write "$BACKEND_FILE" "$sha"
  wd_log "backend $sha passed final production verification"
}

require_migrations_applied() {
  local sha=$1 candidate manifest digest applied_digest
  candidate=$(candidate_for "$sha")
  manifest="$STATE_ROOT/.candidate-migrations.$$"
  wd_migration_manifest "$candidate" >"$manifest"

  if ! wd_manifest_is_append_only "$DB_MANIFEST" "$manifest"; then
    rm -f -- "$manifest"
    wd_die "candidate edits or deletes already-applied migration history"
  fi

  digest=$(wd_manifest_digest "$manifest")
  applied_digest=$(wd_manifest_digest "$DB_MANIFEST")
  rm -f -- "$manifest"
  if [[ $digest != "$applied_digest" ]]; then
    wd_atomic_write "$PENDING_MIGRATION_FILE" "$sha"
    CURRENT_PHASE=waiting-for-migration
    status "tests passed; run: sudo apitoken-watchdog migrate $sha"
    wd_log "candidate $sha contains unapplied migrations; automatic deployment is paused"
    wd_log "after review, run: sudo apitoken-watchdog migrate $sha"
    return 1
  fi
  rm -f -- "$PENDING_MIGRATION_FILE"
  return 0
}

main() {
  local remote_ref rejected approved infra_changed=0 engine_changed=0 backend_changed=0

  [[ $(id -un) == deploy ]] || wd_die "watchdog service must run as deploy"
  [[ -d $SOURCE_REPO/.git ]] || wd_die "source repository is missing: $SOURCE_REPO"
  [[ -d $STATE_ROOT && ! -L $STATE_ROOT ]] || wd_die "watchdog state is not installed"
  require_fixed_file "$WATCHDOG_LOCK"
  require_fixed_file "$DEPLOY_LOCK"
  require_fixed_directory "$CONTROLLER_ROOT"
  require_fixed_file "$TEST_DB_HELPER"
  require_fixed_directory "$CI_TOOLCHAIN"
  [[ -f $DB_MANIFEST && ! -L $DB_MANIFEST ]] || wd_die "database migration baseline is missing"

  exec 7<>"$WATCHDOG_LOCK"
  if ! flock -n 7; then
    wd_log "another watchdog cycle is still running"
    exit 0
  fi

  CURRENT_PHASE=fetching
  status "fetching $REMOTE/$BRANCH"
  git -C "$SOURCE_REPO" fetch --no-tags "$REMOTE" \
    "+refs/heads/$BRANCH:refs/remotes/$REMOTE/$BRANCH"
  remote_ref="refs/remotes/$REMOTE/$BRANCH"
  CANDIDATE_SHA=$(git -C "$SOURCE_REPO" rev-parse "$remote_ref^{commit}")
  wd_validate_sha "$CANDIDATE_SHA"

  PROCESSED_SHA=$(wd_read_sha "$PROCESSED_FILE")
  ENGINE_SHA=$(wd_read_sha "$ENGINE_FILE")
  BACKEND_SHA=$(wd_read_sha "$BACKEND_FILE")
  wd_require_ancestor "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" processed
  wd_require_ancestor "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" engine
  wd_require_ancestor "$SOURCE_REPO" "$BACKEND_SHA" "$CANDIDATE_SHA" backend

  if rejected=$(wd_read_sha "$REJECTED_FILE" 2>/dev/null) && [[ $rejected == "$CANDIDATE_SHA" ]]; then
    CURRENT_PHASE=quarantined
    status "failed candidate remains blocked; run: sudo apitoken-watchdog retry $CANDIDATE_SHA"
    wd_log "candidate $CANDIDATE_SHA is quarantined; waiting for a newer commit or explicit retry"
    exit 0
  fi

  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" wd_path_is_infrastructure \
    && infra_changed=1
  wd_range_has_class "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" wd_path_is_engine \
    && engine_changed=1
  wd_range_has_class "$SOURCE_REPO" "$BACKEND_SHA" "$CANDIDATE_SHA" wd_path_is_backend \
    && backend_changed=1

  if [[ $PROCESSED_SHA == "$CANDIDATE_SHA" && $engine_changed == 0 && $backend_changed == 0 ]]; then
    CURRENT_PHASE=idle
    status "master already processed; no component drift"
    wd_log "master $CANDIDATE_SHA is already processed and production is aligned"
    exit 0
  fi

  prepare_and_test_candidate "$CANDIDATE_SHA"
  rm -f -- "$REJECTED_FILE"

  if (( infra_changed == 1 )); then
    approved=$(wd_read_sha "$INFRA_APPROVED_FILE" 2>/dev/null || true)
    if [[ $approved != "$CANDIDATE_SHA" ]]; then
      wd_atomic_write "$PENDING_INFRA_FILE" "$CANDIDATE_SHA"
      CURRENT_PHASE=waiting-for-infrastructure-review
      status "tests passed; operational files require manual review and approval"
      wd_log "operational files changed; automatic component deployment is paused"
      wd_log "after installing/reviewing them, run: sudo apitoken-watchdog approve-infrastructure $CANDIDATE_SHA"
      exit 0
    fi
  fi
  rm -f -- "$PENDING_INFRA_FILE"

  if (( backend_changed == 1 )) && ! require_migrations_applied "$CANDIDATE_SHA"; then
    exit 0
  fi

  if (( engine_changed == 1 )); then
    deploy_engine "$CANDIDATE_SHA"
  fi
  if (( backend_changed == 1 )); then
    deploy_backend "$CANDIDATE_SHA"
  fi

  wd_atomic_write "$PROCESSED_FILE" "$CANDIDATE_SHA"
  rm -f -- "$INFRA_APPROVED_FILE" "$PENDING_INFRA_FILE" "$PENDING_MIGRATION_FILE"
  CURRENT_PHASE=idle
  status "candidate tested and all selected components verified in production"
  wd_log "watchdog completed $CANDIDATE_SHA (engine=$engine_changed backend=$backend_changed)"
}

main "$@"
trap - ERR EXIT INT TERM
