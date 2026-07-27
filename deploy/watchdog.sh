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
CANDIDATE_RETENTION_SECONDS=$((24 * 60 * 60))
# Immutable releases and per-deployment dumps accumulate once per delivery and nothing else removes
# them. Keep enough history for multi-step rollback and forensics, bounded so disk use cannot grow
# without limit. `current`, `previous`, the recorded component SHAs, and any release backing a live
# process are always retained regardless of these counts.
RELEASE_RETENTION_KEEP=10
PREDEPLOY_DUMP_RETENTION_KEEP=10
CI_USER=apitoken-ci
CI_HOME=$STATE_ROOT/ci-home
CI_CARGO_TARGET=$CI_HOME/cargo-target
CI_NEXT_CACHE_ROOT=$CI_HOME/next-cache
CI_TYPESCRIPT_ARTIFACT_CACHE_ROOT=$CI_HOME/typescript-artifacts
CI_TOOLCHAIN=/opt/apitoken-watchdog/rust-toolchain
CONTROLLER_ROOT=/usr/local/lib/apitoken-watchdog/controller
CONTROLLER_ENTRYPOINT=/usr/local/lib/apitoken-watchdog/watchdog.sh
VALIDATION_PLANNER=$CONTROLLER_ROOT/validation-plan.sh
TEST_DB_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-test-db
BACKUP_RUNNER=/usr/local/lib/apitoken-watchdog/watchdog-backup.sh
MIGRATION_RUNNER=/usr/local/lib/apitoken-watchdog/watchdog-migrate.sh
INFRASTRUCTURE_RUNNER=/usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh
RETENTION_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-retention.sh
SALES_RUNNER=/usr/local/lib/apitoken-watchdog/controller/sales-deploy.sh
OPENKEYS_RUNNER=/usr/local/lib/apitoken-watchdog/controller/openkeys-deploy.sh
GITHUB_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-github
WATCHDOG_LOCK=/run/lock/apitoken-watchdog.lock
CANDIDATE_VALIDATOR_LOCK=/run/lock/apitoken-candidate-validator.lock
SOURCE_FETCH_LOCK=/run/lock/apitoken-source-fetch.lock
DEPLOY_LOCK=/run/lock/apitoken-deploy.lock
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
COMMERCE_RELEASE_ROOT=/opt/apitoken/releases

PROCESSED_FILE=$STATE_ROOT/processed.sha
INFRASTRUCTURE_FILE=$STATE_ROOT/infrastructure.sha
ENGINE_FILE=$STATE_ROOT/engine.sha
BACKEND_FILE=$STATE_ROOT/backend.sha
SALES_FILE=$STATE_ROOT/sales.sha
OPENKEYS_FILE=$STATE_ROOT/openkeys.sha
REJECTED_FILE=$STATE_ROOT/rejected.sha
PENDING_MIGRATION_FILE=$STATE_ROOT/pending-migration.sha
DB_MANIFEST=$STATE_ROOT/database-migrations.manifest
STATUS_FILE=$STATE_ROOT/status
IDLE_MAINTENANCE_FILE=$STATE_ROOT/last-idle-maintenance.epoch
IDLE_MAINTENANCE_SECONDS=60

CURRENT_PHASE=starting
TEST_DB_STARTED=0
TEST_DB_SLOT=0
VALIDATION_BASE_SHA=
ACTIVE_DEPLOYMENT_ID=
ACTIVE_DEPLOYMENT_ENV=
ACTIVE_DEPLOYMENT_URL=
SHADOW_DEPLOYMENT_ID=

VALIDATION_TYPESCRIPT_REQUIRED=0
VALIDATION_TYPESCRIPT_FULL=0
VALIDATION_TYPESCRIPT_BASE_SHA=
VALIDATION_RUST_REQUIRED=0
VALIDATION_STATIC_REQUIRED=0
VALIDATION_ENGINE_ARTIFACTS_REQUIRED=0
VALIDATION_CODEX_ARTIFACTS_REQUIRED=0
VALIDATION_PLAN_FORMAT=1
VALIDATION_POLICY_SHA256=
VALIDATION_PLAN_SHA256=

github_status() {
  sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" "$1" "$2" "$3"
}

github_deployment_start() {
  local component=$1 environment=$2 url=$3
  ACTIVE_DEPLOYMENT_ENV=$environment
  ACTIVE_DEPLOYMENT_URL=$url
  ACTIVE_DEPLOYMENT_ID=$(sudo -n "$GITHUB_HELPER" deployment-create \
    "$CANDIDATE_SHA" "$environment" "Automatic $component deployment")
  sudo -n "$GITHUB_HELPER" deployment-status "$ACTIVE_DEPLOYMENT_ID" in_progress \
    "$component deployment started after validation" "$environment" "$url"
}

github_deployment_success() {
  local component=$1
  sudo -n "$GITHUB_HELPER" deployment-status "$ACTIVE_DEPLOYMENT_ID" success \
    "$component deployment verified in production" "$ACTIVE_DEPLOYMENT_ENV" "$ACTIVE_DEPLOYMENT_URL"
  ACTIVE_DEPLOYMENT_ID=
  ACTIVE_DEPLOYMENT_ENV=
  ACTIVE_DEPLOYMENT_URL=
}

run_github_status_lane() {
  # ERR/EXIT belong to the parent watchdog. A reporting worker returns only its own result so the
  # parent can join every request and publish one coherent overall failure.
  trap - ERR EXIT INT TERM
  github_status "$@"
}

publish_pipeline_start_statuses() {
  local watchdog_pid tests_pid watchdog_rc=0 tests_rc=0
  run_github_status_lane pending deploy/watchdog "Production pipeline started" &
  watchdog_pid=$!
  run_github_status_lane pending deploy/tests "Path-aware isolated validation in progress" &
  tests_pid=$!
  wait "$watchdog_pid" || watchdog_rc=$?
  wait "$tests_pid" || tests_rc=$?
  (( watchdog_rc == 0 && tests_rc == 0 )) \
    || wd_die "could not publish pipeline start statuses (watchdog=$watchdog_rc tests=$tests_rc)"
}

publish_unchanged_component_statuses() {
  local engine_changed=$1 backend_changed=$2 sales_changed=$3 openkeys_changed=$4
  local migration_pid='' engine_pid='' backend_pid='' sales_pid='' openkeys_pid=''
  local migration_rc=0 engine_rc=0 backend_rc=0 sales_rc=0 openkeys_rc=0 flag
  for flag in "$engine_changed" "$backend_changed" "$sales_changed" "$openkeys_changed"; do
    [[ $flag == 0 || $flag == 1 ]] || wd_die "invalid component reporting flag: $flag"
  done

  if (( backend_changed == 0 )); then
    run_github_status_lane success deploy/migration "No commerce migration changes" &
    migration_pid=$!
  fi
  if (( engine_changed == 0 )); then
    run_github_status_lane success deploy/engine "No engine changes" &
    engine_pid=$!
  fi
  if (( backend_changed == 0 )); then
    run_github_status_lane success deploy/backend "No backend changes" &
    backend_pid=$!
  fi
  if (( sales_changed == 0 )); then
    run_github_status_lane success deploy/sales "No sales changes" &
    sales_pid=$!
  fi
  if (( openkeys_changed == 0 )); then
    run_github_status_lane success deploy/openkeys "No openkeys changes" &
    openkeys_pid=$!
  fi

  if [[ -n $migration_pid ]]; then wait "$migration_pid" || migration_rc=$?; fi
  if [[ -n $engine_pid ]]; then wait "$engine_pid" || engine_rc=$?; fi
  if [[ -n $backend_pid ]]; then wait "$backend_pid" || backend_rc=$?; fi
  if [[ -n $sales_pid ]]; then wait "$sales_pid" || sales_rc=$?; fi
  if [[ -n $openkeys_pid ]]; then wait "$openkeys_pid" || openkeys_rc=$?; fi
  (( migration_rc == 0 && engine_rc == 0 && backend_rc == 0 \
      && sales_rc == 0 && openkeys_rc == 0 )) \
    || wd_die "unchanged-component status publication failed (migration=$migration_rc engine=$engine_rc backend=$backend_rc sales=$sales_rc openkeys=$openkeys_rc)"
}

test_db() {
  sudo -n "$TEST_DB_HELPER" "$1" "$TEST_DB_SLOT"
}

fetch_source_once() (
  exec 8<>"$SOURCE_FETCH_LOCK"
  flock 8
  git -C "$SOURCE_REPO" fetch --no-tags "$REMOTE" "$@"
)

fetch_source() {
  # Release the source-repository lock between retries. A transient candidate fetch must not keep
  # the production poll behind a five-second retry delay.
  wd_retry 3 5 fetch_source_once "$@"
}

status() {
  local detail=$1
  # World-readable: the monitoring collector runs with an empty CapabilityBoundingSet, so it has no
  # CAP_DAC_OVERRIDE and cannot read a 0640 file even as root. The status line contains only a
  # phase, a public commit SHA, a fixed detail string, and a timestamp — no secret.
  wd_atomic_write "$STATUS_FILE" "phase=$CURRENT_PHASE sha=${CANDIDATE_SHA:-none} detail=$detail updated_at=$(date -u +%FT%TZ)" 0644
}

github_phase_failure() {
  local phase=$1
  [[ -x $GITHUB_HELPER ]] || return 0
  if [[ -n $ACTIVE_DEPLOYMENT_ID ]]; then
    sudo -n "$GITHUB_HELPER" deployment-status "$ACTIVE_DEPLOYMENT_ID" failure \
      "deployment failed closed at $phase" "$ACTIVE_DEPLOYMENT_ENV" "$ACTIVE_DEPLOYMENT_URL" \
      >/dev/null 2>&1 || true
  fi
  case $phase in
    testing) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/tests "Validation failed; production unchanged" >/dev/null 2>&1 || true ;;
    migrating) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/migration "Migration failed; application deploy blocked" >/dev/null 2>&1 || true ;;
    deploying-engine) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/engine "Engine rollout failed closed" >/dev/null 2>&1 || true ;;
    deploying-backend) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/backend "Backend rollout failed closed" >/dev/null 2>&1 || true ;;
    deploying-sales) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/sales "Sales rollout failed closed" >/dev/null 2>&1 || true ;;
    deploying-openkeys) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/openkeys "OpenKeys rollout failed closed" >/dev/null 2>&1 || true ;;
  esac
}

fail() {
  local rc=$? line=${BASH_LINENO[0]:-unknown}
  local failed_phase=${CURRENT_PHASE_BEFORE_FAILURE:-$CURRENT_PHASE}
  # Clear first: this handler is registered on EXIT as well as ERR, and must never re-enter itself
  # when it exits below.
  trap - ERR EXIT INT TERM

  # The EXIT trap also fires on a successful cycle and on the deliberate `exit 0` paths (already
  # processed, quarantined, another cycle holding the lock). Those are not failures.
  if (( rc == 0 )); then
    return 0
  fi

  if (( TEST_DB_STARTED == 1 )); then
    test_db stop >/dev/null 2>&1 || true
  fi
  # A failure before a candidate is even identified (fetch, lock, state validation) is an
  # infrastructure problem, not a verdict on any commit. Quarantining nothing here would be
  # meaningless, and reporting a red status against a SHA we never tested would be wrong.
  if [[ -z ${CANDIDATE_SHA:-} || ! ${CANDIDATE_SHA:-} =~ ^[0-9a-f]{40}$ ]]; then
    CURRENT_PHASE=failed
    status "pre-candidate failure at line $line (exit $rc); no commit was evaluated"
    wd_warn "watchdog cycle failed at line $line before selecting a candidate; retrying next cycle"
    exit "$rc"
  fi
  wd_atomic_write "$REJECTED_FILE" "$CANDIDATE_SHA" 0644
  CURRENT_PHASE=failed
  status "command failed at line $line (exit $rc); candidate quarantined"
  if [[ -x $GITHUB_HELPER ]]; then
    github_phase_failure "$failed_phase"
    sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/watchdog \
      "Production pipeline failed closed; inspect watchdog logs" >/dev/null 2>&1 || true
  fi
  wd_warn "candidate ${CANDIDATE_SHA:-unknown} failed at line $line and will not be retried automatically"
  exit "$rc"
}
# Registered on EXIT as well as ERR. `wd_die` — used by 30+ validation call sites — terminates with
# `exit`, which does NOT run an ERR trap, so those failures previously bypassed quarantine and left
# no red commit status: the pipeline stopped correctly but reported nothing and blocked no retry.
# EXIT catches every abnormal termination path; `fail` returns immediately when the status is 0, so
# successful cycles and the deliberate `exit 0` paths are unaffected.
#
# Subshells do not inherit this trap (verified on bash 5.2), so a `wd_die` inside a subshell used as
# a condition — such as the post-admission backend verification — still fails only that subshell.
trap fail ERR EXIT INT TERM

rollout_lane_exit() {
  local rc=$1 phase=${CURRENT_PHASE_BEFORE_FAILURE:-$CURRENT_PHASE}
  trap - ERR EXIT INT TERM
  (( rc != 0 )) || return 0
  set +e
  github_phase_failure "$phase"
  wd_warn "parallel rollout lane failed during $phase (exit $rc); waiting lanes will still be joined"
  exit "$rc"
}

run_rollout_lane() {
  # Each asynchronous lane owns its deployment ID and component phase. Report its specific failure
  # from that subshell, but leave quarantine and the overall watchdog verdict to the parent after
  # every sibling has reached a safe terminal state.
  trap - ERR EXIT INT TERM
  trap 'rollout_lane_exit "$?"' EXIT
  "$@"
}

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
  local sha=$1 typescript_required=$2 typescript_full=$3 typescript_base=$4
  local rust_required=$5 static_required=$6 engine_artifacts_required=$7
  local validation_policy_sha256=$8 required_typescript_components=$9
  local codex_artifacts_required=${10}
  local marker candidate marker_sha marker_tree candidate_sha candidate_tree marker_digest actual_digest
  local marker_flag marker_typescript_full expected_hash actual_hash manifest
  local marker_plan_format marker_policy marker_plan marker_plan_actual
  local marker_typescript_required marker_typescript_base marker_rust_required
  local marker_static_required marker_engine_artifacts marker_typescript_components component
  marker=$(marker_for "$sha")
  candidate=$(candidate_for "$sha")
  [[ -d $candidate && ! -L $candidate ]] || return 1
  [[ $(stat -c '%u' -- "$candidate" 2>/dev/null) == 0 ]] || return 1
  marker_sha=$(wd_marker_value "$marker" sha 2>/dev/null) || return 1
  [[ $marker_sha == "$sha" ]] || return 1
  marker_tree=$(wd_marker_value "$marker" tree 2>/dev/null) || return 1
  candidate_sha=$(git -c safe.directory="$candidate" -C "$candidate" rev-parse 'HEAD^{commit}' 2>/dev/null) \
    || return 1
  candidate_tree=$(git -c safe.directory="$candidate" -C "$candidate" rev-parse 'HEAD^{tree}' 2>/dev/null) \
    || return 1
  [[ $candidate_sha == "$sha" && $candidate_tree == "$marker_tree" ]] || return 1
  [[ -z $(git -c safe.directory="$candidate" -C "$candidate" \
    status --porcelain --untracked-files=no 2>/dev/null) ]] || return 1
  marker_digest=$(wd_marker_value "$marker" migration_digest 2>/dev/null) || return 1
  manifest="$STATE_ROOT/.candidate-manifest.${BASHPID:-$$}"
  wd_migration_manifest "$candidate" >"$manifest"
  actual_digest=$(wd_manifest_digest "$manifest")
  rm -f -- "$manifest"
  [[ $marker_digest == "$actual_digest" ]] || return 1

  # Markers written by this controller are bound to both the candidate policy implementation and
  # the exact union plan. A legacy marker can be admitted once during the staged controller
  # upgrade, but only through the same lane/artifact checks below; a partially upgraded marker is
  # never accepted.
  marker_plan_format=$(wd_marker_value "$marker" validation_plan_format 2>/dev/null || printf '')
  marker_policy=$(wd_marker_value "$marker" validation_policy_sha256 2>/dev/null || printf '')
  marker_plan=$(wd_marker_value "$marker" validation_plan_sha256 2>/dev/null || printf '')
  if [[ -n $marker_plan_format || -n $marker_policy || -n $marker_plan ]]; then
    [[ $marker_plan_format == "$VALIDATION_PLAN_FORMAT" ]] || return 1
    [[ $marker_policy == "$validation_policy_sha256" ]] || return 1
    [[ $marker_plan =~ ^[0-9a-f]{64}$ ]] || return 1
    marker_typescript_required=$(wd_marker_value "$marker" typescript_tested 2>/dev/null) \
      || return 1
    marker_typescript_full=$(wd_marker_value "$marker" typescript_full 2>/dev/null) || return 1
    marker_typescript_base=$(wd_marker_value "$marker" typescript_base 2>/dev/null) || return 1
    marker_rust_required=$(wd_marker_value "$marker" rust_tested 2>/dev/null) || return 1
    marker_static_required=$(wd_marker_value "$marker" static_tested 2>/dev/null) || return 1
    marker_engine_artifacts=$(wd_marker_value "$marker" engine_artifacts 2>/dev/null) || return 1
    for marker_flag in "$marker_typescript_required" "$marker_typescript_full" \
      "$marker_rust_required" "$marker_static_required" "$marker_engine_artifacts"; do
      [[ $marker_flag == 0 || $marker_flag == 1 ]] || return 1
    done
    [[ $marker_typescript_base =~ ^[0-9a-f]{40}$ ]] || return 1
    marker_plan_actual=$(validation_plan_digest_values \
      "$marker_plan_format" "$marker_policy" "$marker_typescript_required" \
      "$marker_typescript_full" "$marker_typescript_base" "$marker_rust_required" \
      "$marker_static_required" "$marker_engine_artifacts")
    [[ $marker_plan == "$marker_plan_actual" ]] || return 1
  fi

  for marker_flag in \
    "typescript_tested:$typescript_required" \
    "typescript_full:$typescript_full" \
    "rust_tested:$rust_required" \
    "static_tested:$static_required" \
    "engine_artifacts:$engine_artifacts_required" \
    "codex_artifacts:$codex_artifacts_required"; do
    if [[ ${marker_flag#*:} == 1 ]]; then
      [[ $(wd_marker_value "$marker" "${marker_flag%%:*}" 2>/dev/null) == 1 ]] || return 1
    fi
  done

  if (( typescript_required == 1 )); then
    marker_typescript_full=$(wd_marker_value "$marker" typescript_full 2>/dev/null) || return 1
    if [[ $marker_typescript_full != 1 ]]; then
      marker_typescript_base=$(wd_marker_value "$marker" typescript_base 2>/dev/null) || return 1
      git -C "$SOURCE_REPO" merge-base --is-ancestor "$marker_typescript_base" "$typescript_base" \
        2>/dev/null || return 1
    fi
    marker_typescript_components=$(wd_marker_value "$marker" typescript_components 2>/dev/null \
      || printf '')
    if [[ -z $marker_typescript_components ]]; then
      # Staged upgrade compatibility: the previous controller always built every host component
      # and recorded one aggregate digest.
      expected_hash=$(wd_marker_value "$marker" typescript_artifact_digest 2>/dev/null) || return 1
      actual_hash=$(wd_typescript_artifact_digest "$candidate") || return 1
      [[ $actual_hash == "$expected_hash" ]] || return 1
    else
      wd_typescript_component_list_is_canonical "$marker_typescript_components" || return 1
      for component in commerce sales openkeys web; do
        wd_typescript_component_list_contains "$required_typescript_components" "$component" \
          || continue
        wd_typescript_component_list_contains "$marker_typescript_components" "$component" \
          || return 1
        expected_hash=$(wd_marker_value "$marker" \
          "typescript_artifact_digest_$component" 2>/dev/null) || return 1
        actual_hash=$(wd_typescript_component_artifact_digest "$candidate" "$component") || return 1
        [[ $actual_hash == "$expected_hash" ]] || return 1
      done
    fi
    if wd_typescript_component_list_contains "$required_typescript_components" commerce; then
      expected_hash=$(wd_marker_value "$marker" commerce_release_bundle_sha256 2>/dev/null) \
        || return 1
      [[ $expected_hash =~ ^[0-9a-f]{64}$ ]] || return 1
      actual_hash=$(wd_commerce_release_bundle_digest "$candidate") || return 1
      [[ $actual_hash == "$expected_hash" ]] || return 1
    fi
  fi
  if (( engine_artifacts_required == 1 )); then
    expected_hash=$(wd_marker_value "$marker" engine_binary_sha256 2>/dev/null) || return 1
    actual_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/engine/claude-api") || return 1
    [[ $actual_hash == "$expected_hash" ]] || return 1
    expected_hash=$(wd_marker_value "$marker" authbot_binary_sha256 2>/dev/null) || return 1
    actual_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/engine/authbot") || return 1
    [[ $actual_hash == "$expected_hash" ]] || return 1
  fi
  if (( codex_artifacts_required == 1 )); then
    [[ -f $candidate/.deploy-artifacts/codex/codex \
        && ! -L $candidate/.deploy-artifacts/codex/codex \
        && -x $candidate/.deploy-artifacts/codex/codex ]] || return 1
    expected_hash=$(wd_marker_value "$marker" codex_binary_sha256 2>/dev/null) || return 1
    [[ $expected_hash =~ ^[0-9a-f]{64}$ ]] || return 1
    actual_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/codex/codex") || return 1
    [[ $actual_hash == "$expected_hash" ]] || return 1
    [[ $(wd_marker_value "$marker" codex_source_commit 2>/dev/null) =~ ^[0-9a-f]{40}$ ]] \
      || return 1
    [[ $(wd_marker_value "$marker" codex_version 2>/dev/null) \
        =~ ^codex-cli\ [0-9]+\.[0-9]+\.[0-9]+$ ]] || return 1
  fi
  return 0
}

prune_expired_candidates() {
  local now cutoff candidate sha marker pruned=0 failed=0 phase_set=0
  now=$(date +%s)
  [[ $now =~ ^[0-9]+$ ]] || {
    wd_warn "cannot determine current epoch; candidate retention skipped"
    return 0
  }
  cutoff=$((now - CANDIDATE_RETENTION_SECONDS))

  while IFS= read -r -d '' candidate; do
    # wd_candidate_dirs_older_than already restricts output to direct, non-symlink SHA
    # directories. Revalidate at the destructive boundary as defence in depth.
    sha=${candidate##*/}
    if [[ ${candidate%/*} != "$CANDIDATE_ROOT" || ! $sha =~ ^[0-9a-f]{40}$ || \
          ! -d $candidate || -L $candidate ]]; then
      wd_warn "unsafe candidate retention target skipped: $candidate"
      failed=$((failed + 1))
      continue
    fi
    if (( phase_set == 0 )); then
      CURRENT_PHASE=pruning
      status "removing watchdog build candidates older than 24 hours"
      phase_set=1
    fi
    exec 9>"$STATE_ROOT/$sha.candidate.lock"
    if ! flock -n 9; then
      wd_log "candidate $sha is being validated; retention skipped it"
      exec 9>&-
      continue
    fi
    if sudo -n rm -rf --one-file-system -- "$candidate"; then
      marker=$(marker_for "$sha")
      sudo -n rm -f -- "$marker" \
        || wd_warn "removed candidate $sha but could not remove its test marker"
      pruned=$((pruned + 1))
    else
      wd_warn "failed to remove expired candidate $sha"
      failed=$((failed + 1))
    fi
    flock -u 9
    exec 9>&-
  done < <(wd_candidate_dirs_older_than "$CANDIDATE_ROOT" "$STATE_ROOT" "$cutoff")

  if (( pruned > 0 || failed > 0 )); then
    wd_log "candidate retention finished: removed=$pruned failed=$failed max_age_seconds=$CANDIDATE_RETENTION_SECONDS"
  fi
  return 0
}

# Releases backing a live process must never be removed, even if retention counting would otherwise
# reach them. Resolve every relevant unit's MainPID to the release directory it actually executes
# from, exactly like the readiness gates do, rather than trusting the symlinks alone.
live_release_shas() {
  local unit pid resolved name
  for unit in claude-api@8787.service claude-api@8788.service \
    apitoken-api@3000.service apitoken-api@3001.service \
    apitoken-worker.service apitoken-content-studio.service; do
    systemctl is-active --quiet "$unit" || continue
    pid=$(systemctl show "$unit" -p MainPID --value)
    [[ $pid =~ ^[1-9][0-9]*$ ]] || continue
    for resolved in "$(readlink -f -- "/proc/$pid/exe" 2>/dev/null)" \
      "$(readlink -f -- "/proc/$pid/cwd" 2>/dev/null)"; do
      [[ -n $resolved ]] || continue
      # Walk up to the SHA-named release directory under either release root.
      while [[ $resolved == /*/* ]]; do
        name=${resolved##*/}
        if [[ $name =~ ^[0-9a-f]{40}$ ]]; then
          printf '%s\n' "$name"
          break
        fi
        resolved=${resolved%/*}
      done
    done
  done
}

prune_expired_releases() {
  local root=$1 label=$2 protected=() sha removed=0 failed=0 release

  mapfile -t protected < <(live_release_shas)
  for sha in "${ENGINE_SHA:-}" "${BACKEND_SHA:-}" "${PROCESSED_SHA:-}" "${SALES_SHA:-}" "${OPENKEYS_SHA:-}"; do
    [[ $sha =~ ^[0-9a-f]{40}$ ]] && protected+=("$sha")
  done

  while IFS= read -r -d '' release; do
    # wd_prunable_release_dirs already excludes links, non-SHA names, and protected releases.
    # Revalidate at the destructive boundary as defence in depth.
    sha=${release##*/}
    if [[ ${release%/*} != "$root" || ! $sha =~ ^[0-9a-f]{40}$ || ! -d $release || -L $release ]]; then
      wd_warn "unsafe $label retention target skipped: $release"
      failed=$((failed + 1))
      continue
    fi
    # Releases are finalized read-only; restore write bits on the tree before removing it.
    sudo -n chmod -R u+w -- "$release" 2>/dev/null || true
    if sudo -n rm -rf --one-file-system -- "$release"; then
      removed=$((removed + 1))
    else
      wd_warn "failed to remove expired $label release $sha"
      failed=$((failed + 1))
    fi
  done < <(wd_prunable_release_dirs "$root" "$RELEASE_RETENTION_KEEP" "${protected[@]}")

  if (( removed > 0 || failed > 0 )); then
    wd_log "$label release retention finished: removed=$removed failed=$failed keep=$RELEASE_RETENTION_KEEP"
  fi
  return 0
}

prune_expired_dumps() {
  # The backup root is root-only (0700) and unreadable by deploy, so both selection and deletion
  # happen inside the fixed root helper. It prints its own summary.
  sudo -n "$RETENTION_HELPER" "$PREDEPLOY_DUMP_RETENTION_KEEP" \
    || wd_warn "pre-deploy dump retention did not complete"
  return 0
}

run_as_ci() {
  sudo -n -u "$CI_USER" env \
    HOME="$CI_HOME" \
    CARGO_HOME="$CI_HOME/.cargo" \
    CARGO_TARGET_DIR="$CI_CARGO_TARGET" \
    PATH="$CI_TOOLCHAIN/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$@"
}

test_typescript_lane() {
  local candidate=$1 dsn=$2 sales_dsn=$3 openkeys_dsn=$4
  local base=$5 target=$6 force_full=$7 lane_components=$8
  local scope_output='' mode=full package
  local filters=() test_packages=() build_contexts=()
  wd_log "running frozen TypeScript install, build, typecheck, migrations, and tests"
  run_as_ci pnpm --dir "$candidate" install --frozen-lockfile
  run_as_ci env NEXT_CACHE_ROOT="$CI_NEXT_CACHE_ROOT" \
    bash "$candidate/deploy/next-cache.sh" restore "$candidate"
  if (( force_full == 0 )) \
    && scope_output=$(run_as_ci node "$candidate/deploy/typescript-scope.mjs" \
      "$candidate" "$base" "$target"); then
    mode=${scope_output%%$'\n'*}
    if [[ $mode == filtered ]]; then
      while IFS= read -r package; do
        [[ $package == filtered || -z $package ]] && continue
        filters+=("--filter=$package")
        test_packages+=("$package")
      done <<<"$scope_output"
    fi
  fi
  if [[ $mode != filtered ]]; then
    # A shared/full/failed package scope needs every clean runtime context. This keeps the fallback
    # fail-closed and aligns full typecheck/tests with the artifacts they exercise.
    lane_components=commerce,sales,openkeys,web
  fi
  IFS=, read -r -a build_contexts <<<"$lane_components"
  wd_log "building complete TypeScript context(s): $lane_components"
  run_as_ci env TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CI_TYPESCRIPT_ARTIFACT_CACHE_ROOT" \
    bash "$candidate/deploy/typescript-build-contexts.sh" "$candidate" "${build_contexts[@]}"
  run_as_ci env NEXT_CACHE_ROOT="$CI_NEXT_CACHE_ROOT" \
    bash "$candidate/deploy/next-cache.sh" save "$candidate"
  if (( ${#filters[@]} == 0 )); then
    wd_log "TypeScript scope is shared, empty, or unavailable; checking the full workspace"
    run_as_ci pnpm --dir "$candidate" typecheck
  else
    wd_log "TypeScript scope selected ${#filters[@]} workspace package(s)"
    run_as_ci pnpm --dir "$candidate" "${filters[@]}" \
      -r --if-present --fail-if-no-match typecheck
  fi
  run_as_ci env TEST_DATABASE_URL="$dsn" TEST_SALES_DATABASE_URL="$sales_dsn" \
    TEST_OPENKEYS_DATABASE_URL="$openkeys_dsn" \
    TYPESCRIPT_TEST_COMPONENTS="$lane_components" \
    bash "$candidate/deploy/typescript-test-groups.sh" "$candidate" "${test_packages[@]}"
  if wd_typescript_component_list_contains "$lane_components" commerce; then
    wd_log "assembling the tested compact commerce release bundle"
    run_as_ci bash "$candidate/deploy/commerce-release-bundle.sh" "$candidate"
  fi
}

test_rust_lane() {
  local candidate=$1 engine_dsn=$2 build_artifacts=$3
  wd_log "running the locked Rust workspace tests from the shared target cache"
  run_as_ci env CLAUDE_API_TEST_DATABASE_URL="$engine_dsn" \
    cargo test --locked --workspace --manifest-path "$candidate/Cargo.toml"
  if (( build_artifacts == 1 )); then
    wd_log "building the production engine and authbot once, from the tested candidate"
    run_as_ci cargo build --locked --release -p claude-api -p authbot \
      --manifest-path "$candidate/Cargo.toml"
    run_as_ci install -d -m 0755 "$candidate/.deploy-artifacts/engine"
    run_as_ci install -m 0755 "$CI_CARGO_TARGET/release/claude-api" \
      "$candidate/.deploy-artifacts/engine/claude-api"
    run_as_ci install -m 0755 "$CI_CARGO_TARGET/release/authbot" \
      "$candidate/.deploy-artifacts/engine/authbot"
  fi
}

test_codex_lane() {
  local candidate=$1 stage artifact_root built_binary
  stage="$candidate/.deploy-artifacts/codex-stage"
  artifact_root="$candidate/.deploy-artifacts/codex"
  wd_log "building and testing the pinned official Codex binary from its attested source"
  run_as_ci "$candidate/tools/codex-app-server/build-pinned.sh" --install-dir "$stage"
  built_binary=$(readlink -f -- "$stage/codex")
  [[ $built_binary == "$stage"/codex-* && -f $built_binary && ! -L $built_binary ]] \
    || wd_die "pinned Codex builder produced an unsafe artifact"
  run_as_ci install -d -m 0755 "$artifact_root"
  run_as_ci install -m 0555 "$built_binary" "$artifact_root/codex"
  run_as_ci rm -rf -- "$stage"
}

test_static_lane() {
  local candidate=$1 sha=$2 run_regression_suites=$3 shell_file
  local diff_base=${VALIDATION_BASE_SHA:-${PROCESSED_SHA:-}}
  wd_log "checking tracked whitespace and shell syntax"
  if [[ -n $diff_base && $diff_base != "$sha" ]]; then
    git -C "$SOURCE_REPO" diff --check "$diff_base..$sha"
  fi
  while IFS= read -r -d '' shell_file; do
    bash -n "$shell_file"
  done < <(find "$candidate/deploy" -type f -name '*.sh' -print0)
  if (( run_regression_suites == 1 )); then
    wd_log "running deployment and merge-workflow regression suites"
    run_as_ci bash "$candidate/deploy/lib.test.sh"
    run_as_ci bash "$candidate/deploy/watchdog-lib.test.sh"
    run_as_ci bash "$candidate/deploy/watchdog-codex-promote.test.sh"
    run_as_ci bash "$candidate/deploy/monitoring-config.test.sh"
    run_as_ci bash "$candidate/deploy/sccache-cargo.test.sh"
    run_as_ci bash "$candidate/deploy/next-cache.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-scope.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-build-contexts.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-artifact-cache.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-test-groups.test.sh"
    run_as_ci bash "$candidate/deploy/commerce-release-bundle.test.sh"
    run_as_ci bash "$candidate/deploy/agent-merge.suite.sh"
  fi
}

run_candidate_lane() {
  # ERR is inherited by asynchronous functions under `set -E`. The parent owns quarantine and
  # database cleanup after it has joined every lane; a child must only return its lane status.
  trap - ERR EXIT INT TERM
  "$@"
}

prepare_and_test_candidate_unlocked() {
  local sha=$1 typescript_required=$2 typescript_full=$3 typescript_base=$4
  local rust_required=$5 static_required=$6 engine_artifacts_required=$7
  local validation_policy_sha256=$8 validation_plan_sha256=$9
  local codex_artifacts_required=${10}
  local candidate marker dsn= engine_dsn= sales_dsn= openkeys_dsn= manifest digest tree
  local typescript_pid= rust_pid= codex_pid= static_pid=
  local typescript_rc=0 rust_rc=0 codex_rc=0 static_rc=0
  local typescript_components=none typescript_digest=none component
  local typescript_digest_commerce=none typescript_digest_sales=none
  local typescript_digest_openkeys=none typescript_digest_web=none
  local commerce_release_bundle_hash=none
  local engine_hash=none authbot_hash=none codex_hash=none
  local codex_source_commit=none codex_version=none
  candidate=$(candidate_for "$sha")
  marker=$(marker_for "$sha")

  (( engine_artifacts_required == 0 || rust_required == 1 )) \
    || wd_die "engine artifacts cannot be prepared without the Rust validation lane"
  if (( typescript_required == 1 )); then
    typescript_components=$(wd_typescript_components_for_range \
      "$SOURCE_REPO" "$typescript_base" "$sha" "$typescript_full")
    wd_typescript_component_list_is_canonical "$typescript_components" \
      || wd_die "derived a malformed TypeScript component list: $typescript_components"
  fi

  if candidate_is_tested "$sha" "$typescript_required" "$typescript_full" "$typescript_base" \
    "$rust_required" "$static_required" "$engine_artifacts_required" \
    "$validation_policy_sha256" "$typescript_components" "$codex_artifacts_required"; then
    wd_log "reusing test-passed immutable candidate $sha"
    return 0
  fi

  CURRENT_PHASE=testing
  CURRENT_PHASE_BEFORE_FAILURE=testing
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

  if (( typescript_required == 1 || rust_required == 1 )); then
    dsn=$(test_db start)
    TEST_DB_STARTED=1
  fi
  if (( typescript_required == 1 )); then
    sales_dsn=$(test_db sales-dsn)
    openkeys_dsn=$(test_db openkeys-dsn)
  fi
  if (( rust_required == 1 )); then
    engine_dsn=$(test_db engine-dsn)
  fi

  # Resolve every fallible prerequisite before starting children. Once launched, the parent reaches
  # every wait and the database cleanup path even when any individual lane fails.
  if (( typescript_required == 1 )); then
    run_candidate_lane test_typescript_lane "$candidate" "$dsn" "$sales_dsn" "$openkeys_dsn" \
      "$typescript_base" "$sha" "$typescript_full" "$typescript_components" &
    typescript_pid=$!
  fi
  if (( rust_required == 1 )); then
    run_candidate_lane test_rust_lane "$candidate" "$engine_dsn" "$engine_artifacts_required" &
    rust_pid=$!
  fi
  if (( codex_artifacts_required == 1 )); then
    run_candidate_lane test_codex_lane "$candidate" &
    codex_pid=$!
  fi
  run_candidate_lane test_static_lane "$candidate" "$sha" "$static_required" &
  static_pid=$!

  # The language and static suites are independent once their prerequisites exist. Wait for every
  # child even when one fails so no candidate-owned process survives into cleanup or a later cycle.
  if [[ -n $typescript_pid ]]; then wait "$typescript_pid" || typescript_rc=$?; fi
  if [[ -n $rust_pid ]]; then wait "$rust_pid" || rust_rc=$?; fi
  if [[ -n $codex_pid ]]; then wait "$codex_pid" || codex_rc=$?; fi
  wait "$static_pid" || static_rc=$?
  if (( TEST_DB_STARTED == 1 )); then
    test_db stop
    TEST_DB_STARTED=0
  fi
  (( typescript_rc == 0 )) || wd_die "TypeScript candidate lane failed (exit $typescript_rc)"
  (( rust_rc == 0 )) || wd_die "Rust candidate lane failed (exit $rust_rc)"
  (( codex_rc == 0 )) || wd_die "Codex candidate lane failed (exit $codex_rc)"
  (( static_rc == 0 )) || wd_die "Static candidate lane failed (exit $static_rc)"

  [[ -z $(run_as_ci git -C "$candidate" status --porcelain --untracked-files=no) ]] \
    || wd_die "tests modified tracked candidate files"
  manifest="$STATE_ROOT/.candidate-manifest.${BASHPID:-$$}"
  wd_migration_manifest "$candidate" >"$manifest"
  digest=$(wd_manifest_digest "$manifest")
  tree=$(run_as_ci git -C "$candidate" rev-parse 'HEAD^{tree}')
  if (( typescript_required == 1 )); then
    for component in commerce sales openkeys web; do
      wd_typescript_component_list_contains "$typescript_components" "$component" || continue
      case "$component" in
        commerce)
          typescript_digest_commerce=$(wd_typescript_component_artifact_digest \
            "$candidate" "$component")
          ;;
        sales)
          typescript_digest_sales=$(wd_typescript_component_artifact_digest \
            "$candidate" "$component")
          ;;
        openkeys)
          typescript_digest_openkeys=$(wd_typescript_component_artifact_digest \
            "$candidate" "$component")
          ;;
        web)
          typescript_digest_web=$(wd_typescript_component_artifact_digest \
            "$candidate" "$component")
          ;;
      esac
    done
    if wd_typescript_component_list_contains "$typescript_components" commerce \
      && wd_typescript_component_list_contains "$typescript_components" sales \
      && wd_typescript_component_list_contains "$typescript_components" openkeys; then
      typescript_digest=$(wd_typescript_artifact_digest "$candidate")
    fi
    if wd_typescript_component_list_contains "$typescript_components" commerce; then
      commerce_release_bundle_hash=$(wd_commerce_release_bundle_digest "$candidate")
    fi
  fi
  if (( engine_artifacts_required == 1 )); then
    engine_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/engine/claude-api")
    authbot_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/engine/authbot")
  fi
  if (( codex_artifacts_required == 1 )); then
    codex_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/codex/codex")
    codex_version=$(run_as_ci "$candidate/.deploy-artifacts/codex/codex" --version)
    [[ $codex_version =~ ^codex-cli\ [0-9]+\.[0-9]+\.[0-9]+$ ]] \
      || wd_die "tested Codex binary reported a malformed version"
    codex_source_commit=$(sed -n \
      "s/^CODEX_GIT_COMMIT='\\([0-9a-f]\\{40\\}\\)'$/\\1/p" \
      "$candidate/tools/codex-app-server/UPSTREAM.pin")
    [[ $codex_source_commit =~ ^[0-9a-f]{40}$ ]] \
      || wd_die "Codex source pin is malformed"
  fi
  {
    printf 'sha=%s\n' "$sha"
    printf 'tree=%s\n' "$tree"
    printf 'migration_digest=%s\n' "$digest"
    printf 'typescript_tested=%s\n' "$typescript_required"
    printf 'typescript_full=%s\n' "$typescript_full"
    printf 'typescript_base=%s\n' "${typescript_base:-none}"
    printf 'rust_tested=%s\n' "$rust_required"
    printf 'static_tested=%s\n' "$static_required"
    printf 'engine_artifacts=%s\n' "$engine_artifacts_required"
    printf 'codex_artifacts=%s\n' "$codex_artifacts_required"
    printf 'validation_plan_format=%s\n' "$VALIDATION_PLAN_FORMAT"
    printf 'validation_policy_sha256=%s\n' "$validation_policy_sha256"
    printf 'validation_plan_sha256=%s\n' "$validation_plan_sha256"
    printf 'typescript_components=%s\n' "$typescript_components"
    printf 'typescript_artifact_digest=%s\n' "$typescript_digest"
    printf 'typescript_artifact_digest_commerce=%s\n' "$typescript_digest_commerce"
    printf 'typescript_artifact_digest_sales=%s\n' "$typescript_digest_sales"
    printf 'typescript_artifact_digest_openkeys=%s\n' "$typescript_digest_openkeys"
    printf 'typescript_artifact_digest_web=%s\n' "$typescript_digest_web"
    printf 'commerce_release_bundle_sha256=%s\n' "$commerce_release_bundle_hash"
    printf 'engine_binary_sha256=%s\n' "$engine_hash"
    printf 'authbot_binary_sha256=%s\n' "$authbot_hash"
    printf 'codex_binary_sha256=%s\n' "$codex_hash"
    printf 'codex_source_commit=%s\n' "$codex_source_commit"
    printf 'codex_version=%s\n' "$codex_version"
    printf 'completed_at=%s\n' "$(date -u +%FT%TZ)"
  } >"${marker}.tmp.${BASHPID:-$$}"
  chmod 0640 "${marker}.tmp.${BASHPID:-$$}"
  mv -f -- "${marker}.tmp.${BASHPID:-$$}" "$marker"
  rm -f -- "$manifest"

  # Every deployment consumes this exact build. Root ownership plus removed write bits keeps it
  # stable between the green test result and release promotion.
  sudo -n chown -R root:root -- "$candidate"
  sudo -n chmod -R a-w -- "$candidate"
  wd_log "candidate $sha passed every selected isolated validation lane"
}

prepare_and_test_candidate() {
  local sha=$1
  wd_validate_sha "$sha"
  # Candidate workspaces and markers are SHA-addressed and shared by production and speculative
  # validators. Serialize only the same SHA: unrelated candidates can still validate in parallel,
  # while production can wait for and reuse an already-running exact build.
  exec 9>"$STATE_ROOT/$sha.candidate.lock"
  flock 9
  prepare_and_test_candidate_unlocked "$@"
  flock -u 9
  exec 9>&-
}

PARSED_PLAN_FORMAT=
PARSED_PLAN_POLICY=
PARSED_PLAN_TYPESCRIPT_REQUIRED=
PARSED_PLAN_TYPESCRIPT_FULL=
PARSED_PLAN_TYPESCRIPT_BASE=
PARSED_PLAN_RUST_REQUIRED=
PARSED_PLAN_STATIC_REQUIRED=
PARSED_PLAN_ENGINE_ARTIFACTS_REQUIRED=

parse_validation_plan() {
  local output=$1 line key value
  local seen_format=0 seen_policy=0 seen_typescript_required=0 seen_typescript_full=0
  local seen_typescript_base=0 seen_rust_required=0 seen_static_required=0
  local seen_engine_artifacts=0

  PARSED_PLAN_FORMAT=
  PARSED_PLAN_POLICY=
  PARSED_PLAN_TYPESCRIPT_REQUIRED=
  PARSED_PLAN_TYPESCRIPT_FULL=
  PARSED_PLAN_TYPESCRIPT_BASE=
  PARSED_PLAN_RUST_REQUIRED=
  PARSED_PLAN_STATIC_REQUIRED=
  PARSED_PLAN_ENGINE_ARTIFACTS_REQUIRED=

  while IFS= read -r line; do
    [[ $line == *=* ]] || return 1
    key=${line%%=*}
    value=${line#*=}
    case "$key" in
      validation_plan_format)
        (( seen_format == 0 )) || return 1
        PARSED_PLAN_FORMAT=$value
        seen_format=1
        ;;
      validation_policy_sha256)
        (( seen_policy == 0 )) || return 1
        PARSED_PLAN_POLICY=$value
        seen_policy=1
        ;;
      typescript_required)
        (( seen_typescript_required == 0 )) || return 1
        PARSED_PLAN_TYPESCRIPT_REQUIRED=$value
        seen_typescript_required=1
        ;;
      typescript_full)
        (( seen_typescript_full == 0 )) || return 1
        PARSED_PLAN_TYPESCRIPT_FULL=$value
        seen_typescript_full=1
        ;;
      typescript_base)
        (( seen_typescript_base == 0 )) || return 1
        PARSED_PLAN_TYPESCRIPT_BASE=$value
        seen_typescript_base=1
        ;;
      rust_required)
        (( seen_rust_required == 0 )) || return 1
        PARSED_PLAN_RUST_REQUIRED=$value
        seen_rust_required=1
        ;;
      static_required)
        (( seen_static_required == 0 )) || return 1
        PARSED_PLAN_STATIC_REQUIRED=$value
        seen_static_required=1
        ;;
      engine_artifacts_required)
        (( seen_engine_artifacts == 0 )) || return 1
        PARSED_PLAN_ENGINE_ARTIFACTS_REQUIRED=$value
        seen_engine_artifacts=1
        ;;
      *) return 1 ;;
    esac
  done <<<"$output"

  (( seen_format == 1 && seen_policy == 1 && seen_typescript_required == 1 \
    && seen_typescript_full == 1 && seen_typescript_base == 1 \
    && seen_rust_required == 1 && seen_static_required == 1 \
    && seen_engine_artifacts == 1 )) || return 1
  [[ $PARSED_PLAN_FORMAT == "$VALIDATION_PLAN_FORMAT" ]] || return 1
  [[ $PARSED_PLAN_POLICY =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ $PARSED_PLAN_TYPESCRIPT_BASE =~ ^[0-9a-f]{40}$ ]] || return 1
  for value in \
    "$PARSED_PLAN_TYPESCRIPT_REQUIRED" "$PARSED_PLAN_TYPESCRIPT_FULL" \
    "$PARSED_PLAN_RUST_REQUIRED" "$PARSED_PLAN_STATIC_REQUIRED" \
    "$PARSED_PLAN_ENGINE_ARTIFACTS_REQUIRED"; do
    [[ $value == 0 || $value == 1 ]] || return 1
  done
  (( PARSED_PLAN_TYPESCRIPT_FULL == 0 || PARSED_PLAN_TYPESCRIPT_REQUIRED == 1 )) || return 1
  (( PARSED_PLAN_ENGINE_ARTIFACTS_REQUIRED == 0 || PARSED_PLAN_RUST_REQUIRED == 1 )) || return 1
}

candidate_validation_plan() {
  local target=$1 processed_base=$2 engine_base=$3 backend_base=$4 sales_base=$5 openkeys_base=$6
  local temporary output rc=0
  temporary=$(mktemp -d "$STATE_ROOT/.validation-plan.XXXXXX")
  chmod 0755 "$temporary"
  if ! git -C "$SOURCE_REPO" archive "$target" \
    deploy/validation-plan.sh deploy/watchdog-lib.sh | tar -x -C "$temporary"; then
    rm -rf -- "$temporary"
    return 1
  fi
  if [[ ! -f $temporary/deploy/validation-plan.sh \
        || -L $temporary/deploy/validation-plan.sh \
        || ! -f $temporary/deploy/watchdog-lib.sh \
        || -L $temporary/deploy/watchdog-lib.sh ]]; then
    rm -rf -- "$temporary"
    return 1
  fi
  chmod 0755 "$temporary/deploy" "$temporary/deploy/validation-plan.sh"
  chmod 0644 "$temporary/deploy/watchdog-lib.sh"
  output=$(
    run_as_ci timeout --signal=TERM --kill-after=5s 30s \
      env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.directory \
      GIT_CONFIG_VALUE_0="$SOURCE_REPO" bash "$temporary/deploy/validation-plan.sh" \
      "$SOURCE_REPO" "$target" "$processed_base" "$engine_base" "$backend_base" \
      "$sales_base" "$openkeys_base"
  ) || rc=$?
  rm -rf -- "$temporary"
  (( rc == 0 )) || return "$rc"
  printf '%s\n' "$output"
}

validation_plan_digest_values() {
  [[ $# -eq 8 ]] || wd_die "validation plan digest requires eight fields"
  printf '%s\n' \
    "validation_plan_format=$1" \
    "validation_policy_sha256=$2" \
    "typescript_required=$3" \
    "typescript_full=$4" \
    "typescript_base=$5" \
    "rust_required=$6" \
    "static_required=$7" \
    "engine_artifacts_required=$8" \
    | wd_sha256_stdin
}

validation_plan_digest() {
  validation_plan_digest_values \
    "$VALIDATION_PLAN_FORMAT" "$VALIDATION_POLICY_SHA256" \
    "$VALIDATION_TYPESCRIPT_REQUIRED" "$VALIDATION_TYPESCRIPT_FULL" \
    "$VALIDATION_TYPESCRIPT_BASE_SHA" "$VALIDATION_RUST_REQUIRED" \
    "$VALIDATION_STATIC_REQUIRED" "$VALIDATION_ENGINE_ARTIFACTS_REQUIRED"
}

select_candidate_validation_requirements() {
  local target=$1 committed_base=${2:-}
  local processed_base=${committed_base:-$PROCESSED_SHA}
  local engine_base=${committed_base:-$ENGINE_SHA}
  local backend_base=${committed_base:-$BACKEND_SHA}
  local sales_base=${committed_base:-${SALES_SHA:-$PROCESSED_SHA}}
  local openkeys_base=${committed_base:-${OPENKEYS_SHA:-$PROCESSED_SHA}}
  local installed_output candidate_output
  local installed_typescript_required installed_typescript_full installed_typescript_base
  local installed_rust_required installed_static_required installed_engine_artifacts_required

  installed_output=$(
    "$VALIDATION_PLANNER" "$SOURCE_REPO" "$target" "$processed_base" "$engine_base" \
      "$backend_base" "$sales_base" "$openkeys_base"
  ) || wd_die "installed validation planner failed for candidate $target"
  parse_validation_plan "$installed_output" \
    || wd_die "installed validation planner returned a malformed plan for $target"
  installed_typescript_required=$PARSED_PLAN_TYPESCRIPT_REQUIRED
  installed_typescript_full=$PARSED_PLAN_TYPESCRIPT_FULL
  installed_typescript_base=$PARSED_PLAN_TYPESCRIPT_BASE
  installed_rust_required=$PARSED_PLAN_RUST_REQUIRED
  installed_static_required=$PARSED_PLAN_STATIC_REQUIRED
  installed_engine_artifacts_required=$PARSED_PLAN_ENGINE_ARTIFACTS_REQUIRED

  # Execute only the tiny pure planner from the exact candidate as the unprivileged CI account.
  # The host plan remains a floor; candidate policy can add work but cannot remove it.
  candidate_output=$(candidate_validation_plan "$target" "$processed_base" "$engine_base" \
    "$backend_base" "$sales_base" "$openkeys_base") \
    || wd_die "exact candidate validation planner failed for $target"
  parse_validation_plan "$candidate_output" \
    || wd_die "exact candidate validation planner returned a malformed plan for $target"

  VALIDATION_TYPESCRIPT_REQUIRED=$((installed_typescript_required \
    || PARSED_PLAN_TYPESCRIPT_REQUIRED))
  VALIDATION_TYPESCRIPT_FULL=$((installed_typescript_full || PARSED_PLAN_TYPESCRIPT_FULL))
  VALIDATION_RUST_REQUIRED=$((installed_rust_required || PARSED_PLAN_RUST_REQUIRED))
  VALIDATION_STATIC_REQUIRED=$((installed_static_required || PARSED_PLAN_STATIC_REQUIRED))
  VALIDATION_ENGINE_ARTIFACTS_REQUIRED=$((installed_engine_artifacts_required \
    || PARSED_PLAN_ENGINE_ARTIFACTS_REQUIRED))
  VALIDATION_CODEX_ARTIFACTS_REQUIRED=0
  VALIDATION_POLICY_SHA256=$PARSED_PLAN_POLICY

  if (( installed_typescript_required == 0 )); then
    VALIDATION_TYPESCRIPT_BASE_SHA=$PARSED_PLAN_TYPESCRIPT_BASE
  elif (( PARSED_PLAN_TYPESCRIPT_REQUIRED == 0 )); then
    VALIDATION_TYPESCRIPT_BASE_SHA=$installed_typescript_base
  elif git -C "$SOURCE_REPO" merge-base --is-ancestor \
    "$PARSED_PLAN_TYPESCRIPT_BASE" "$installed_typescript_base"; then
    VALIDATION_TYPESCRIPT_BASE_SHA=$PARSED_PLAN_TYPESCRIPT_BASE
  elif git -C "$SOURCE_REPO" merge-base --is-ancestor \
    "$installed_typescript_base" "$PARSED_PLAN_TYPESCRIPT_BASE"; then
    VALIDATION_TYPESCRIPT_BASE_SHA=$installed_typescript_base
  else
    VALIDATION_TYPESCRIPT_BASE_SHA=$processed_base
    VALIDATION_TYPESCRIPT_FULL=1
  fi
  # Keep the public plan envelope at format 1 during this staged controller upgrade. Older
  # production controllers can validate the feature candidate, while the newly installed
  # controller adds this artifact requirement before any production promotion.
  if wd_range_has_class "$SOURCE_REPO" "$engine_base" "$target" wd_path_is_codex_tooling; then
    VALIDATION_CODEX_ARTIFACTS_REQUIRED=1
  fi
  (( VALIDATION_ENGINE_ARTIFACTS_REQUIRED == 0 )) || VALIDATION_RUST_REQUIRED=1
  (( VALIDATION_TYPESCRIPT_FULL == 0 )) || VALIDATION_TYPESCRIPT_REQUIRED=1
  VALIDATION_PLAN_SHA256=$(validation_plan_digest)
}

load_production_baselines() {
  PROCESSED_SHA=$(wd_read_sha "$PROCESSED_FILE")
  INFRASTRUCTURE_SHA=$(wd_read_sha "$INFRASTRUCTURE_FILE" 2>/dev/null || printf '%s\n' "$PROCESSED_SHA")
  ENGINE_SHA=$(wd_read_sha "$ENGINE_FILE")
  BACKEND_SHA=$(wd_read_sha "$BACKEND_FILE")
  SALES_SHA=$(wd_read_sha "$SALES_FILE" 2>/dev/null || printf '')
  OPENKEYS_SHA=$(wd_read_sha "$OPENKEYS_FILE" 2>/dev/null || printf '')
}

shadow_validation_exit() {
  local rc=$1
  trap - ERR EXIT INT TERM
  (( rc != 0 )) || return 0

  if (( TEST_DB_STARTED == 1 )); then
    test_db stop >/dev/null 2>&1 || true
  fi
  sudo -n "$GITHUB_HELPER" deployment-status "$SHADOW_DEPLOYMENT_ID" failure \
    "Trusted candidate validation failed; production unchanged" candidate-validation "" \
    >/dev/null 2>&1 || true
  sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/tests \
    "Trusted candidate validation failed; production unchanged" >/dev/null 2>&1 || true
  wd_warn "trusted shadow validation failed for $CANDIDATE_SHA; production remains unchanged"
  exit "$rc"
}

run_shadow_candidate_validation() (
  # This is deliberately isolated from the production failure trap. A feature SHA can receive a
  # red validation verdict, but it must never write rejected.sha or mark deploy/watchdog red for
  # the already healthy production SHA.
  trap - ERR EXIT INT TERM
  set -euo pipefail
  SHADOW_DEPLOYMENT_ID=$1
  CANDIDATE_SHA=$2
  TEST_DB_SLOT=$3
  local committed_master=$4 current_master
  CI_CARGO_TARGET="$CI_HOME/cargo-target-shadow-$TEST_DB_SLOT"
  STATUS_FILE="$STATE_ROOT/candidate-validation-$TEST_DB_SLOT.status"
  VALIDATION_BASE_SHA=$committed_master
  TEST_DB_STARTED=0
  trap 'exit 130' INT
  trap 'exit 143' TERM
  trap 'shadow_validation_exit "$?"' EXIT

  CURRENT_PHASE=shadow-validation
  CURRENT_PHASE_BEFORE_FAILURE=shadow-validation
  status "validating an exact pre-merge candidate in isolated slot $TEST_DB_SLOT"
  wd_retry 3 5 sudo -n "$GITHUB_HELPER" deployment-status "$SHADOW_DEPLOYMENT_ID" in_progress \
    "Trusted production-host candidate validation started" candidate-validation ""
  github_status pending deploy/tests "Trusted production-host candidate validation in progress"

  # GitHub permits fetching a raw object ID when it is reachable from the already-pushed feature
  # branch. The merge client therefore requests validation only for a remotely visible exact SHA.
  fetch_source "$CANDIDATE_SHA"
  git -C "$SOURCE_REPO" cat-file -e "$CANDIDATE_SHA^{commit}"
  wd_require_ancestor "$SOURCE_REPO" "$committed_master" "$CANDIDATE_SHA" shadow-committed-master
  wd_require_ancestor "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" shadow-processed
  wd_require_ancestor "$SOURCE_REPO" "$INFRASTRUCTURE_SHA" "$CANDIDATE_SHA" shadow-infrastructure
  wd_require_ancestor "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" shadow-engine
  wd_require_ancestor "$SOURCE_REPO" "$BACKEND_SHA" "$CANDIDATE_SHA" shadow-backend
  [[ -z $SALES_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$SALES_SHA" "$CANDIDATE_SHA" shadow-sales
  [[ -z $OPENKEYS_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$OPENKEYS_SHA" "$CANDIDATE_SHA" shadow-openkeys

  # The merge client rebases before requesting validation. Select only its delta from that exact
  # committed master: the parent's own immutable candidate is already being deployed and the
  # locked merge cannot land this SHA unless that parent becomes green.
  select_candidate_validation_requirements "$CANDIDATE_SHA" "$committed_master"
  prepare_and_test_candidate "$CANDIDATE_SHA" "$VALIDATION_TYPESCRIPT_REQUIRED" \
    "$VALIDATION_TYPESCRIPT_FULL" "$VALIDATION_TYPESCRIPT_BASE_SHA" \
    "$VALIDATION_RUST_REQUIRED" "$VALIDATION_STATIC_REQUIRED" \
    "$VALIDATION_ENGINE_ARTIFACTS_REQUIRED" "$VALIDATION_POLICY_SHA256" \
    "$VALIDATION_PLAN_SHA256" "$VALIDATION_CODEX_ARTIFACTS_REQUIRED"

  # A different agent may have landed while this gate ran. Accept an advanced master only when it
  # is still an ancestor of this exact candidate; otherwise the request is stale and the merge
  # client will rebase, produce a new SHA, and request both exact-SHA gates again.
  fetch_source "+refs/heads/$BRANCH:refs/remotes/$REMOTE/$BRANCH"
  current_master=$(git -C "$SOURCE_REPO" rev-parse "refs/remotes/$REMOTE/$BRANCH^{commit}")
  wd_validate_sha "$current_master"
  wd_require_ancestor "$SOURCE_REPO" "$current_master" "$CANDIDATE_SHA" shadow-current-master

  github_status success deploy/tests "Trusted production-host candidate validation passed"
  wd_retry 3 5 sudo -n "$GITHUB_HELPER" deployment-status "$SHADOW_DEPLOYMENT_ID" success \
    "Exact candidate passed trusted production-host validation" candidate-validation ""
  wd_log "trusted shadow validation passed for $CANDIDATE_SHA"
)

candidate_validator_main() {
  # This service is a separate failure domain. Queue/API faults may fail this oneshot and feature
  # candidates may receive red verdicts, but neither path can invoke the production quarantine trap.
  trap - ERR EXIT INT TERM
  trap 'exit 130' INT
  trap 'exit 143' TERM

  local remote_ref master_sha validation_output validation_id validation_sha validation_extra
  local index slot validation_rc
  local validation_ids=() validation_shas=() validation_pids=()

  [[ $(id -un) == deploy ]] || wd_die "candidate validator service must run as deploy"
  [[ -d $SOURCE_REPO/.git ]] || wd_die "source repository is missing: $SOURCE_REPO"
  [[ -d $STATE_ROOT && ! -L $STATE_ROOT ]] || wd_die "watchdog state is not installed"
  [[ -d $CANDIDATE_ROOT && ! -L $CANDIDATE_ROOT ]] \
    || wd_die "candidate root is not a regular directory: $CANDIDATE_ROOT"
  require_fixed_file "$CANDIDATE_VALIDATOR_LOCK"
  require_fixed_file "$SOURCE_FETCH_LOCK"
  require_fixed_file "$TEST_DB_HELPER"
  require_fixed_file "$VALIDATION_PLANNER"
  require_fixed_file "$GITHUB_HELPER"
  require_fixed_directory "$CI_TOOLCHAIN"
  [[ -f $DB_MANIFEST && ! -L $DB_MANIFEST ]] || wd_die "database migration baseline is missing"

  exec 6<>"$CANDIDATE_VALIDATOR_LOCK"
  if ! flock -n 6; then
    wd_log "another candidate-validator cycle is still running"
    return 0
  fi

  if ! validation_output=$(sudo -n "$GITHUB_HELPER" validation-next 2); then
    wd_warn "candidate validation queue lookup failed; retrying on the next five-second poll"
    return 0
  fi
  [[ -n $validation_output ]] || return 0

  while IFS=$'\t' read -r validation_id validation_sha validation_extra; do
    if [[ ! $validation_id =~ ^[1-9][0-9]*$ || ! $validation_sha =~ ^[0-9a-f]{40}$ \
          || -n $validation_extra || ${#validation_ids[@]} -ge 2 ]]; then
      wd_die "GitHub helper returned a malformed candidate validation batch"
    fi
    validation_ids[${#validation_ids[@]}]=$validation_id
    validation_shas[${#validation_shas[@]}]=$validation_sha
  done <<<"$validation_output"
  (( ${#validation_ids[@]} > 0 )) || return 0

  remote_ref="refs/remotes/$REMOTE/$BRANCH"
  fetch_source "+refs/heads/$BRANCH:$remote_ref"
  master_sha=$(git -C "$SOURCE_REPO" rev-parse "$remote_ref^{commit}")
  wd_validate_sha "$master_sha"
  load_production_baselines
  wd_require_ancestor "$SOURCE_REPO" "$PROCESSED_SHA" "$master_sha" validator-processed
  wd_require_ancestor "$SOURCE_REPO" "$INFRASTRUCTURE_SHA" "$master_sha" validator-infrastructure
  wd_require_ancestor "$SOURCE_REPO" "$ENGINE_SHA" "$master_sha" validator-engine
  wd_require_ancestor "$SOURCE_REPO" "$BACKEND_SHA" "$master_sha" validator-backend
  [[ -z $SALES_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$SALES_SHA" "$master_sha" validator-sales
  [[ -z $OPENKEYS_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$OPENKEYS_SHA" "$master_sha" validator-openkeys

  for index in "${!validation_ids[@]}"; do
    slot=$((index + 1))
    run_shadow_candidate_validation "${validation_ids[$index]}" "${validation_shas[$index]}" \
      "$slot" "$master_sha" &
    validation_pids[$index]=$!
  done

  # Candidate verdicts are request outcomes, not service-health failures. Join every worker so its
  # isolated database is cleaned up, report each result, and leave the timer healthy for new work.
  for index in "${!validation_pids[@]}"; do
    validation_rc=0
    wait "${validation_pids[$index]}" || validation_rc=$?
    if (( validation_rc == 0 )); then
      wd_log "completed trusted validation ${validation_ids[$index]} for ${validation_shas[$index]}"
    else
      wd_warn "trusted validation ${validation_ids[$index]} failed (exit $validation_rc)"
    fi
  done
}

final_verify_engine() {
  local sha=$1
  engine_runtime_aligned "$sha" \
    || wd_die "engine runtime is not in a single-slot steady state after cutover: $ENGINE_RUNTIME_DETAIL"
}

engine_runtime_aligned() {
  local sha=$1 expected current legacy_active=0 legacy_enabled=0 stable_status
  local active_8787=0 ready_8787=0 current_8787=0 enabled_8787=0
  local active_8788=0 ready_8788=0 current_8788=0 enabled_8788=0
  local port unit pid executable status

  expected="$ENGINE_RELEASE_ROOT/$sha"
  current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
  if [[ $current != "$expected" ]]; then
    ENGINE_RUNTIME_DETAIL="current=$current expected=$expected"
    return 1
  fi

  for port in 8787 8788; do
    unit="claude-api@$port.service"
    local active=0 ready=0 selected=0 enabled=0
    systemctl is-active --quiet "$unit" && active=1
    systemctl is-enabled --quiet "$unit" && enabled=1
    status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
      "http://127.0.0.1:$port/ready" 2>/dev/null || true)
    [[ $status == 200 ]] && ready=1
    if (( active == 1 )); then
      pid=$(systemctl show "$unit" -p MainPID --value)
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
        [[ $executable == "$expected/claude-api" ]] && selected=1
      fi
    fi
    if [[ $port == 8787 ]]; then
      active_8787=$active; ready_8787=$ready; current_8787=$selected; enabled_8787=$enabled
    else
      active_8788=$active; ready_8788=$ready; current_8788=$selected; enabled_8788=$enabled
    fi
  done
  systemctl is-active --quiet claude-api.service && legacy_active=1
  systemctl is-enabled --quiet claude-api.service && legacy_enabled=1
  stable_status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    http://127.0.0.1:8790/ready 2>/dev/null || true)
  ENGINE_RUNTIME_DETAIL="8787=$active_8787:$ready_8787:$current_8787:$enabled_8787 8788=$active_8788:$ready_8788:$current_8788:$enabled_8788 legacy=$legacy_active:$legacy_enabled stable=${stable_status:-unreachable}"
  [[ $stable_status == 200 ]] || return 1
  wd_engine_topology_is_steady \
    "$active_8787" "$ready_8787" "$current_8787" "$enabled_8787" \
    "$active_8788" "$ready_8788" "$current_8788" "$enabled_8788" \
    "$legacy_active" "$legacy_enabled"
}

reconcile_engine_runtime() {
  local sha=$1 current expected
  expected="$ENGINE_RELEASE_ROOT/$sha"
  if engine_runtime_aligned "$sha"; then return 0; fi
  current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
  [[ $current == "$expected" ]] \
    || wd_die "refusing slot-only repair while engine release selection is wrong: $ENGINE_RUNTIME_DETAIL"
  CURRENT_PHASE=reconciling-engine
  status "repairing engine single-slot runtime drift: $ENGINE_RUNTIME_DETAIL"
  wd_warn "engine runtime drift detected; converging through the health-gated controller: $ENGINE_RUNTIME_DETAIL"
  "$CONTROLLER_ROOT/engine-bluegreen.sh"
  final_verify_engine "$sha"
  wd_log "engine runtime drift repaired; exactly one current slot is active, ready, and enabled"
}

final_verify_backend() {
  local sha=$1 current worker_pid worker_cwd studio_pid studio_cwd expected_studio_cwd
  current=$(readlink -f -- "$COMMERCE_RELEASE_ROOT/current")
  [[ $current == "$COMMERCE_RELEASE_ROOT/$sha" ]] || wd_die "commerce current is not $sha after cutover"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:3000/v1/ready >/dev/null \
    || curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:3001/v1/ready >/dev/null \
    || wd_die "no commerce API slot is ready after cutover"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:8791/v1/ready >/dev/null \
    || wd_die "stable commerce balancer is not ready after cutover"
  systemctl is-active --quiet apitoken-worker.service || wd_die "worker is not active after cutover"
  worker_pid=$(systemctl show apitoken-worker.service -p MainPID --value)
  [[ $worker_pid =~ ^[1-9][0-9]*$ ]] || wd_die "worker has no MainPID"
  worker_cwd=$(readlink -f -- "/proc/$worker_pid/cwd")
  [[ $worker_cwd == "$COMMERCE_RELEASE_ROOT/$sha/apps/worker" ]] \
    || wd_die "worker is not running immutable release $sha (cwd=$worker_cwd)"
  systemctl is-active --quiet apitoken-content-studio.service || wd_die "content studio is not active after cutover"
  studio_pid=$(systemctl show apitoken-content-studio.service -p MainPID --value)
  [[ $studio_pid =~ ^[1-9][0-9]*$ ]] || wd_die "content studio has no MainPID"
  studio_cwd=$(readlink -f -- "/proc/$studio_pid/cwd")
  expected_studio_cwd=$(wd_content_studio_runtime_directory "$COMMERCE_RELEASE_ROOT/$sha")
  [[ $studio_cwd == "$expected_studio_cwd" ]] \
    || wd_die "content studio is not running immutable release $sha (cwd=$studio_cwd)"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:3500/api/health >/dev/null \
    || wd_die "content studio health endpoint is not ready after cutover"
}

https_vhost_status() {
  local host=$1
  curl --noproxy '*' --insecure --silent --show-error --max-time 5 \
    --resolve "$host:443:127.0.0.1" -o /dev/null -w '%{http_code}' \
    "https://$host/" 2>/dev/null || true
}

require_admin_auth_vhost() {
  local host=$1 status
  status=$(https_vhost_status "$host")
  [[ $status == 401 ]] \
    || wd_die "$host is not reachable behind managed admin auth (HTTP ${status:-unreachable})"
}

require_retired_vhost() {
  local host=$1 status
  status=$(https_vhost_status "$host")
  case "$status" in
    ''|000|404|421) ;;
    *) wd_die "retired hostname $host is still served (HTTP $status)" ;;
  esac
}

final_verify_admin_panel() {
  local panel matched=0 streak=0 expected_version candidate_panel
  # Ожидаемую версию берём из самого кандидата, а не из константы здесь: версия панели
  # уже живёт в HTML и в тесте крейта, и третья её копия в watchdog означала, что любой
  # бамп версии валит выкат, который на деле корректен (так и случилось на b6b048c).
  candidate_panel="$(candidate_for "$CANDIDATE_SHA")/crates/server/src/admin-panel.html"
  [[ -f $candidate_panel ]] || wd_die "candidate admin panel is missing: $candidate_panel"
  expected_version=$(sed -n 's/.*data-admin-panel-version="\([0-9]\{1,\}\)".*/\1/p' "$candidate_panel" | head -1)
  [[ -n $expected_version ]] || wd_die "candidate admin panel has no version marker"
  # The stable listener round-robins both engine slots with a 2s active-health interval, so for
  # several seconds after a cutover the retiring slot can still answer 200 with the previous panel.
  # One matching answer is therefore not proof: require a short streak of them, over a window
  # comfortably longer than health convergence plus drain, so this asserts the old slot has left
  # rotation rather than racing it.
  #
  # Observed on 2026-07-25: cutover completed at 08:53:19 and the previous 6x1s window gave up at
  # 08:53:27, quarantining a promotion that was in fact correct — the panel served the new version
  # moments later. The check only ever fires on a panel-version bump, which is exactly when it has
  # to be trustworthy.
  for _ in $(seq 1 20); do
    panel=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 \
      http://127.0.0.1:8790/admin-panel 2>/dev/null || true)
    if grep -Fq "data-admin-panel-version=\"$expected_version\"" <<<"$panel"; then
      streak=$((streak + 1))
      if (( streak >= 3 )); then
        matched=1
        break
      fi
    else
      streak=0
    fi
    sleep 3
  done
  [[ $matched == 1 ]] || wd_die "deployed engine does not contain the current admin panel"
}

final_verify_admin_routing() {
  require_admin_auth_vhost admin.apitoken.sale
  require_admin_auth_vhost admin.partners.apitoken.sale
  require_admin_auth_vhost crm.apitoken.sale
  require_admin_auth_vhost content-studio.apitoken.sale
  require_admin_auth_vhost monitoring.apitoken.sale
  require_retired_vhost panel.apitoken.sale
  require_retired_vhost partners.panel.apitoken.sale
  require_retired_vhost crm.panel.apitoken.sale
}

final_verify_monitoring() {
  local response monitoring_ready=0
  curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:3600/api/health >/dev/null \
    || wd_die "Grafana is not healthy on its loopback listener"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 http://127.0.0.1:9090/-/ready >/dev/null \
    || wd_die "Prometheus is not ready on its loopback listener"
  # Wait across several scrape intervals: the engine may have restarted after monitoring itself,
  # and Caddy may still be completing first-certificate activation for the new protected hostname.
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode 'query=min(up) == 1 and min(probe_success{job=~"public-http|protected-http|support-http|loopback-http"}) == 1 and min(time() - apitoken_monitoring_collector_last_success_unixtime) < 180' \
      http://127.0.0.1:9090/api/v1/query 2>/dev/null || true)
    if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
      >/dev/null 2>&1 <<<"$response"; then
      monitoring_ready=1
      break
    fi
    sleep 5
  done
  [[ $monitoring_ready == 1 ]] || wd_die "monitoring targets, synthetics, or business collector are not healthy"
}

# Post-promotion smoke for the optional OpenAI-compatible surface.
#
# The Claude path is verified by /ready and the panel check, but a Codex regression is invisible to
# both: the provider can be enabled, attested and started while its home pool has no usable account,
# or while the public routes have silently fallen back to the Anthropic path. Neither needs an API
# key to detect. The provider is optional, so everything here is skipped when it is switched off.
final_verify_codex_surface() {
  local response envelope enabled=0 enabled_state='' attempt

  for attempt in 1 2 3 4 5 6; do
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode 'query=claude_api_codex_enabled' \
      http://127.0.0.1:9090/api/v1/query 2>/dev/null || true)
    enabled_state=$(jq --exit-status --raw-output \
      'select(.status == "success" and (.data.result | length) == 1)
       | .data.result[0].value[1]
       | select(. == "0" or . == "1")' <<<"$response" 2>/dev/null || true)
    case "$enabled_state" in
      0)
        wd_log "Codex provider is disabled; skipping OpenAI-compatible surface verification"
        return 0
        ;;
      1)
        enabled=1
        break
        ;;
    esac
    (( attempt == 6 )) || sleep 5
  done
  (( enabled == 1 )) \
    || wd_die "could not determine whether the Codex provider is enabled"

  # A live child is not enough: every home may be quarantined or out of window headroom, which
  # serves customers nothing but retryable errors.
  local pool_ready=0
  for attempt in 1 2 3 4 5 6; do
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode 'query=claude_api_codex_process_live == 1 and claude_api_codex_homes_available >= 1' \
      http://127.0.0.1:9090/api/v1/query 2>/dev/null || true)
    if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
      >/dev/null 2>&1 <<<"$response"; then
      pool_ready=1
      break
    fi
    (( attempt == 6 )) || sleep 5
  done
  (( pool_ready == 1 )) \
    || wd_die "Codex provider is enabled but no authenticated home can accept a request"

  # Prove the public OpenAI hostname is actually served by the Codex adapter rather than falling
  # through to the Anthropic path: only the adapter answers with an OpenAI-shaped error envelope
  # carrying `code`/`param`. Resolve the public hostname to loopback so this validates Caddy's
  # hostname boundary and marker injection without depending on external DNS or hairpin routing.
  # The request names a parameter the adapter rejects by contract, so it can never start a turn,
  # reach a ChatGPT subscription or spend quota. An unauthenticated engine answers
  # `invalid_api_key` before request validation for the same reason.
  envelope=$(curl --noproxy '*' --silent --show-error --max-time 5 \
    --resolve openai.api.apitoken.sale:443:127.0.0.1 \
    -H 'content-type: application/json' \
    -d '{"model":"gpt-5.6","input":"ping","temperature":0.5}' \
    https://openai.api.apitoken.sale/v1/responses 2>/dev/null || true)
  jq --exit-status '.error.type == "invalid_request_error"
      and (.error.code == "invalid_api_key" or .error.param != null)' \
    >/dev/null 2>&1 <<<"$envelope" \
    || wd_die "/v1/responses did not answer with the OpenAI-compatible error envelope"

  wd_log "Codex OpenAI-compatible surface verified: pool has capacity and serves its own envelope"
}

run_final_verification_lane() {
  # Verification workers are read-only and independent. The parent joins every selected check and
  # owns quarantine/overall status so one fast failure never abandons another in-flight probe.
  trap - ERR EXIT INT TERM
  "$@"
}

run_final_verification_plan() {
  local verification_plan=$1 engine_sha=$2
  local panel_pid='' routing_pid='' monitoring_pid='' codex_pid=''
  local panel_rc=0 routing_rc=0 monitoring_rc=0 codex_rc=0

  if [[ $verification_plan == none ]]; then
    wd_log "no serving runtime changed; final component smokes are already satisfied"
    return 0
  fi

  # Runtime reconciliation may perform a health-gated cutover when it finds drift. Complete that
  # possible mutation before launching the read-only smokes against the resulting steady state.
  if wd_verification_plan_has "$verification_plan" runtime; then
    reconcile_engine_runtime "$engine_sha"
  fi
  if wd_verification_plan_has "$verification_plan" panel; then
    run_final_verification_lane final_verify_admin_panel &
    panel_pid=$!
  fi
  if wd_verification_plan_has "$verification_plan" routing; then
    run_final_verification_lane final_verify_admin_routing &
    routing_pid=$!
  fi
  if wd_verification_plan_has "$verification_plan" monitoring; then
    run_final_verification_lane final_verify_monitoring &
    monitoring_pid=$!
  fi
  if wd_verification_plan_has "$verification_plan" codex; then
    run_final_verification_lane final_verify_codex_surface &
    codex_pid=$!
  fi

  if [[ -n $panel_pid ]]; then wait "$panel_pid" || panel_rc=$?; fi
  if [[ -n $routing_pid ]]; then wait "$routing_pid" || routing_rc=$?; fi
  if [[ -n $monitoring_pid ]]; then wait "$monitoring_pid" || monitoring_rc=$?; fi
  if [[ -n $codex_pid ]]; then wait "$codex_pid" || codex_rc=$?; fi
  (( panel_rc == 0 && routing_rc == 0 && monitoring_rc == 0 && codex_rc == 0 )) \
    || wd_die "final verification lanes failed (panel=$panel_rc routing=$routing_rc monitoring=$monitoring_rc codex=$codex_rc)"
}

# Post-admission recovery. Before admission the blue-green controllers already fail closed and
# leave the old slot serving, so rollback would be both unnecessary and riskier than doing nothing.
# Once the new slot has been admitted and the old one drained, however, a failed final verification
# means a possibly-bad release is serving traffic with no automatic way back. `previous` still
# points at the last verified release, so re-selecting it and re-running the same health-gated
# controller is the safest available action.
#
# This is best-effort by design: it never masks the original failure. The candidate stays
# quarantined and the pipeline still fails closed either way; a successful rollback only changes
# what is serving while the operator investigates.
attempt_rollback() {
  local component=$1 selector=$2 controller=$3 previous_link=$4 previous_sha

  if [[ ! -L $previous_link ]]; then
    wd_warn "$component rollback unavailable: no previous release is recorded"
    return 1
  fi
  previous_sha=$(basename -- "$(readlink -f -- "$previous_link")")
  if [[ ! $previous_sha =~ ^[0-9a-f]{40}$ ]]; then
    wd_warn "$component rollback unavailable: previous release is not a SHA release ($previous_sha)"
    return 1
  fi

  wd_warn "$component verification failed after traffic was committed; rolling back to $previous_sha"
  if ! "$CONTROLLER_ROOT/rollback.sh" "$selector"; then
    wd_warn "$component rollback selection failed; production still serves the unverified release"
    return 1
  fi
  if ! "$CONTROLLER_ROOT/$controller"; then
    wd_warn "$component rollback cutover failed; inspect slots before any further mutation"
    return 1
  fi
  wd_log "$component rolled back to previously verified release $previous_sha"
  return 0
}

rollback_engine() {
  attempt_rollback engine --engine-bluegreen engine-bluegreen.sh "$ENGINE_RELEASE_ROOT/previous"
}

rollback_backend() {
  attempt_rollback backend --api-only api-bluegreen.sh "$COMMERCE_RELEASE_ROOT/previous"
}

deploy_engine() {
  local sha=$1 codex_changed=$2
  local deploy_args=(--engine-bluegreen --tested-candidate "$(candidate_for "$sha")")
  CURRENT_PHASE=deploying-engine
  CURRENT_PHASE_BEFORE_FAILURE=deploying-engine
  status "promoting and blue-green deploying the tested engine"
  github_status pending deploy/engine "Engine blue-green deployment in progress"
  github_deployment_start engine production-engine https://api.apitoken.sale/health
  if (( codex_changed == 1 )); then
    deploy_args+=(--promote-codex)
  fi
  "$CONTROLLER_ROOT/deploy.sh" "${deploy_args[@]}" "$sha"
  "$CONTROLLER_ROOT/engine-bluegreen.sh"
  # The controller has admitted the new slot and drained the old one by this point. A verification
  # failure now leaves an unverified release serving, so recover before failing closed.
  # `engine_runtime_aligned` is the non-fatal predicate; `final_verify_engine` would exit here.
  if ! engine_runtime_aligned "$sha"; then
    rollback_engine || true
    wd_die "engine runtime is not in a single-slot steady state after cutover: $ENGINE_RUNTIME_DETAIL"
  fi
  wd_atomic_write "$ENGINE_FILE" "$sha"
  github_deployment_success engine
  github_status success deploy/engine "Engine verified in production"
  wd_log "engine $sha passed final production verification"
}

deploy_backend() {
  local sha=$1
  CURRENT_PHASE=deploying-backend
  CURRENT_PHASE_BEFORE_FAILURE=deploying-backend
  status "promoting and blue-green deploying tested API, worker, and Content Studio artifacts"
  github_status pending deploy/backend "Backend blue-green deployment in progress"
  github_deployment_start backend production-backend https://backend.apitoken.sale/v1/ready
  "$CONTROLLER_ROOT/deploy.sh" --api-only --skip-migrate \
    --tested-candidate "$(candidate_for "$sha")" "$sha"
  "$CONTROLLER_ROOT/api-bluegreen.sh" --with-worker
  # As with the engine: traffic is already committed to the new slot here. Run the verifier in a
  # subshell so its internal wd_die becomes a testable status instead of exiting this process, then
  # recover before failing closed. The message is re-emitted by the subshell on stderr.
  if ! ( final_verify_backend "$sha" ); then
    rollback_backend || true
    wd_die "commerce API, worker, or Content Studio failed verification after cutover"
  fi
  wd_atomic_write "$BACKEND_FILE" "$sha"
  github_deployment_success backend
  github_status success deploy/backend "Backend and worker verified in production"
  wd_log "backend and Content Studio $sha passed final production verification"
}

deploy_sales() {
  local sha=$1
  CURRENT_PHASE=deploying-sales
  CURRENT_PHASE_BEFORE_FAILURE=deploying-sales
  status "promoting and health-gating the sales partner portal (own release lifecycle)"
  github_status pending deploy/sales "Sales partner portal deployment in progress"
  github_deployment_start sales production-sales https://partners.apitoken.sale/v1/health
  "$SALES_RUNNER" "$sha"
  wd_atomic_write "$SALES_FILE" "$sha"
  github_deployment_success sales
  github_status success deploy/sales "Sales partner portal verified in production"
  wd_log "sales $sha promoted and verified (partners.apitoken.sale)"
}

deploy_openkeys() {
  local sha=$1
  CURRENT_PHASE=deploying-openkeys
  CURRENT_PHASE_BEFORE_FAILURE=deploying-openkeys
  status "promoting and health-gating the OpenKeys portal (own release lifecycle)"
  github_status pending deploy/openkeys "OpenKeys portal deployment in progress"
  github_deployment_start openkeys production-openkeys https://openkeys.apitoken.sale/
  "$OPENKEYS_RUNNER" "$sha"
  wd_atomic_write "$OPENKEYS_FILE" "$sha"
  github_deployment_success openkeys
  github_status success deploy/openkeys "OpenKeys portal verified in production"
  wd_log "openkeys $sha promoted and verified (openkeys.apitoken.sale)"
}

deploy_core_components() {
  local sha=$1 engine_changed=$2 backend_changed=$3 codex_changed=$4
  # The engine and commerce controllers deliberately share apitoken-deploy.lock. Keep their
  # cutovers ordered inside one lane while sales/OpenKeys use their independent roots and units.
  if (( engine_changed == 1 )); then deploy_engine "$sha" "$codex_changed"; fi
  if (( backend_changed == 1 )); then deploy_backend "$sha"; fi
}

apply_migrations_before_deploy() {
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
    wd_atomic_write "$PENDING_MIGRATION_FILE" "$sha" 0644
    CURRENT_PHASE=migrating
    CURRENT_PHASE_BEFORE_FAILURE=migrating
    status "tests passed; backing up and applying tested database migrations before application deploy"
    github_status pending deploy/migration "Backup and automatic migration in progress"
    github_deployment_start database production-database ""
    wd_log "candidate $sha contains new migration history; applying it before any application cutover"
    sudo -n "$MIGRATION_RUNNER" "$sha"
    [[ $(wd_manifest_digest "$DB_MANIFEST") == "$digest" ]] \
      || wd_die "automatic migration returned without committing the tested manifest"
    github_deployment_success database
    github_status success deploy/migration "Tested migration applied before application rollout"
  else
    github_status success deploy/migration "No commerce migration changes"
  fi
  rm -f -- "$PENDING_MIGRATION_FILE"
  return 0
}

idle_maintenance_due() {
  local now last
  now=$(date +%s)
  [[ $now =~ ^[0-9]+$ ]] || return 0
  if ! wd_read_line "$IDLE_MAINTENANCE_FILE"; then
    return 0
  fi
  last=$REPLY
  [[ $last =~ ^[0-9]+$ ]] || return 0
  (( now - last >= IDLE_MAINTENANCE_SECONDS ))
}

mark_idle_maintenance_complete() {
  wd_atomic_write "$IDLE_MAINTENANCE_FILE" "$(date +%s)" 0644
}

main() {
  local resume_sha=${1:-}
  local remote_ref rejected infra_scope=none delivery_infra_scope=none
  local infra_changed=0 caddy_changed=0 engine_changed=0 backend_changed=0 sales_changed=0
  local openkeys_changed=0 codex_changed=0 typescript_required=0 typescript_full=0 typescript_base=
  local rust_required=0 static_required=0
  local engine_artifacts_required=0 codex_artifacts_required=0
  local validation_policy_sha256='' validation_plan_sha256='' final_verification_plan=''
  local core_pid= sales_pid= openkeys_pid= core_rc=0 sales_rc=0 openkeys_rc=0

  [[ $(id -un) == deploy ]] || wd_die "watchdog service must run as deploy"
  [[ -d $SOURCE_REPO/.git ]] || wd_die "source repository is missing: $SOURCE_REPO"
  [[ -d $STATE_ROOT && ! -L $STATE_ROOT ]] || wd_die "watchdog state is not installed"
  [[ -d $CANDIDATE_ROOT && ! -L $CANDIDATE_ROOT ]] \
    || wd_die "candidate root is not a regular directory: $CANDIDATE_ROOT"
  require_fixed_file "$WATCHDOG_LOCK"
  require_fixed_file "$SOURCE_FETCH_LOCK"
  require_fixed_file "$DEPLOY_LOCK"
  require_fixed_directory "$CONTROLLER_ROOT"
  require_fixed_file "$TEST_DB_HELPER"
  require_fixed_file "$BACKUP_RUNNER"
  require_fixed_file "$MIGRATION_RUNNER"
  require_fixed_file "$INFRASTRUCTURE_RUNNER"
  require_fixed_file "$RETENTION_HELPER"
  require_fixed_file "$SALES_RUNNER"
  require_fixed_file "$OPENKEYS_RUNNER"
  require_fixed_file "$VALIDATION_PLANNER"
  require_fixed_file "$GITHUB_HELPER"
  require_fixed_directory "$CI_TOOLCHAIN"
  [[ -f $DB_MANIFEST && ! -L $DB_MANIFEST ]] || wd_die "database migration baseline is missing"

  if [[ -n $resume_sha ]]; then
    wd_validate_sha "$resume_sha"
    [[ -e /proc/$$/fd/7 && $(readlink -f -- "/proc/$$/fd/7") == "$WATCHDOG_LOCK" ]] \
      || wd_die "controller resume requires the inherited watchdog lock"
    flock -n 7 || wd_die "controller resume no longer owns the inherited watchdog lock"
  else
    exec 7<>"$WATCHDOG_LOCK"
    if ! flock -n 7; then
      wd_log "another watchdog cycle is still running"
      exit 0
    fi
  fi

  CURRENT_PHASE=fetching
  status "fetching $REMOTE/$BRANCH"
  # GitHub reachability is not a property of the candidate. Absorb transient DNS/TLS/network
  # failures here so they never reach the failure path at all.
  fetch_source "+refs/heads/$BRANCH:refs/remotes/$REMOTE/$BRANCH"
  remote_ref="refs/remotes/$REMOTE/$BRANCH"
  CANDIDATE_SHA=$(git -C "$SOURCE_REPO" rev-parse "$remote_ref^{commit}")
  wd_validate_sha "$CANDIDATE_SHA"
  if [[ -n $resume_sha && $CANDIDATE_SHA != "$resume_sha" ]]; then
    CURRENT_PHASE=handoff
    status "master advanced during controller handoff; next poll will select the newer candidate"
    wd_log "controller handoff expected $resume_sha but master is now $CANDIDATE_SHA"
    exit 0
  fi

  load_production_baselines
  if [[ -n $resume_sha && $INFRASTRUCTURE_SHA != "$resume_sha" ]]; then
    wd_die "controller handoff fence does not match the installed candidate"
  fi
  wd_require_ancestor "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" processed
  wd_require_ancestor "$SOURCE_REPO" "$INFRASTRUCTURE_SHA" "$CANDIDATE_SHA" infrastructure
  wd_require_ancestor "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" engine
  wd_require_ancestor "$SOURCE_REPO" "$BACKEND_SHA" "$CANDIDATE_SHA" backend
  [[ -z $SALES_SHA ]] || wd_require_ancestor "$SOURCE_REPO" "$SALES_SHA" "$CANDIDATE_SHA" sales
  [[ -z $OPENKEYS_SHA ]] || wd_require_ancestor "$SOURCE_REPO" "$OPENKEYS_SHA" "$CANDIDATE_SHA" openkeys

  if rejected=$(wd_read_sha "$REJECTED_FILE" 2>/dev/null) && [[ $rejected == "$CANDIDATE_SHA" ]]; then
    CURRENT_PHASE=quarantined
    status "failed candidate remains blocked; run: sudo apitoken-watchdog retry $CANDIDATE_SHA"
    wd_log "candidate $CANDIDATE_SHA is quarantined; waiting for a newer commit or explicit retry"
    exit 0
  fi

  infra_scope=$(wd_infrastructure_install_scope \
    "$SOURCE_REPO" "$INFRASTRUCTURE_SHA" "$CANDIDATE_SHA")
  case "$infra_scope" in
    none) ;;
    controller|caddy|full) infra_changed=1 ;;
    *) wd_die "invalid infrastructure install scope: $infra_scope" ;;
  esac
  # A full installer deliberately hands off to a fresh systemd invocation after recording its
  # infrastructure SHA. Preserve the delivery-wide scope from the last completed candidate so
  # that next invocation still runs the final checks required by the infrastructure just changed.
  delivery_infra_scope=$(wd_infrastructure_install_scope \
    "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA")
  case "$delivery_infra_scope" in
    none|controller|caddy|full) ;;
    *) wd_die "invalid delivery infrastructure scope: $delivery_infra_scope" ;;
  esac
  wd_range_has_class "$SOURCE_REPO" "$INFRASTRUCTURE_SHA" "$CANDIDATE_SHA" wd_path_is_caddy \
    && caddy_changed=1
  wd_range_has_class "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" wd_path_is_engine \
    && engine_changed=1
  wd_range_has_class "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" wd_path_is_codex_tooling \
    && codex_changed=1
  wd_range_has_class "$SOURCE_REPO" "$BACKEND_SHA" "$CANDIDATE_SHA" wd_path_is_backend \
    && backend_changed=1
  # Sales has its own release baseline; fall back to processed on first run (before sales.sha exists).
  wd_range_has_class "$SOURCE_REPO" "${SALES_SHA:-$PROCESSED_SHA}" "$CANDIDATE_SHA" wd_path_is_sales \
    && sales_changed=1
  # OpenKeys держит собственную релизную базу. Пока openkeys.sha не существует,
  # диапазон processed..candidate может быть уже пустым (инфраструктурный шаг
  # прошёл раньше), поэтому первый запуск деплоим безусловно — иначе контекст
  # никогда не получит свой первый релиз.
  if [[ -z ${OPENKEYS_SHA:-} ]]; then
    openkeys_changed=1
  else
    wd_range_has_class "$SOURCE_REPO" "$OPENKEYS_SHA" "$CANDIDATE_SHA" wd_path_is_openkeys \
      && openkeys_changed=1
  fi

  select_candidate_validation_requirements "$CANDIDATE_SHA"
  typescript_required=$VALIDATION_TYPESCRIPT_REQUIRED
  typescript_full=$VALIDATION_TYPESCRIPT_FULL
  typescript_base=$VALIDATION_TYPESCRIPT_BASE_SHA
  rust_required=$VALIDATION_RUST_REQUIRED
  static_required=$VALIDATION_STATIC_REQUIRED
  engine_artifacts_required=$VALIDATION_ENGINE_ARTIFACTS_REQUIRED
  codex_artifacts_required=$VALIDATION_CODEX_ARTIFACTS_REQUIRED
  validation_policy_sha256=$VALIDATION_POLICY_SHA256
  validation_plan_sha256=$VALIDATION_PLAN_SHA256

  if [[ $PROCESSED_SHA == "$CANDIDATE_SHA" && $infra_changed == 0 \
        && $engine_changed == 0 && $backend_changed == 0 \
        && $sales_changed == 0 && $openkeys_changed == 0 ]]; then
    if idle_maintenance_due; then
      CURRENT_PHASE=maintaining
      status "running periodic retention and production-alignment checks"
      prune_expired_candidates
      prune_expired_releases "$ENGINE_RELEASE_ROOT" engine
      prune_expired_releases "$COMMERCE_RELEASE_ROOT" commerce
      prune_expired_dumps
      final_verification_plan=$(wd_final_verification_plan full 0 0 0 0) \
        || wd_die "could not derive the idle-maintenance verification plan"
      run_final_verification_plan "$final_verification_plan" "$ENGINE_SHA"
      mark_idle_maintenance_complete
    fi
    CURRENT_PHASE=idle
    status "master already processed; production runtime aligned"
    wd_log "master $CANDIDATE_SHA is already processed"
    exit 0
  fi

  # Candidate trees and immutable releases are pruned only for real delivery work or the periodic
  # maintenance pass above. The five-second update poll therefore stays cheap while master is idle.
  CURRENT_PHASE=pruning
  status "applying candidate, release, and pre-deploy dump retention"
  prune_expired_candidates
  prune_expired_releases "$ENGINE_RELEASE_ROOT" engine
  prune_expired_releases "$COMMERCE_RELEASE_ROOT" commerce
  prune_expired_dumps

  publish_pipeline_start_statuses

  prepare_and_test_candidate "$CANDIDATE_SHA" "$typescript_required" "$typescript_full" \
    "$typescript_base" "$rust_required" "$static_required" "$engine_artifacts_required" \
    "$validation_policy_sha256" "$validation_plan_sha256" "$codex_artifacts_required"
  github_status success deploy/tests "Selected isolated validation lanes passed"
  rm -f -- "$REJECTED_FILE"

  if (( infra_changed == 1 )); then
    CURRENT_PHASE=installing-infrastructure
    CURRENT_PHASE_BEFORE_FAILURE=installing-infrastructure
    status "installing exact tested operational definitions ($infra_scope)"
    github_status pending deploy/watchdog "Installing exact tested operational definitions ($infra_scope)"
    if [[ $infra_scope == controller ]]; then
      sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA" --controller-only
    elif [[ $infra_scope == caddy ]]; then
      sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA" --caddy-only
    elif (( caddy_changed == 1 )); then
      sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA" --apply-caddy
    else
      sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA"
    fi
    [[ $(wd_read_sha "$INFRASTRUCTURE_FILE") == "$CANDIDATE_SHA" ]] \
      || wd_die "infrastructure installer did not record the exact installed candidate"
    if [[ $infra_scope == controller ]]; then
      CURRENT_PHASE=handoff
      status "operational definitions installed; continuing immediately with the new controller"
      github_status pending deploy/watchdog "New controller installed; continuing immediately"
      wd_log "exact tested controller installed; transferring the held lock to the new controller"
      require_fixed_file "$CONTROLLER_ENTRYPOINT"
      exec "$CONTROLLER_ENTRYPOINT" --resume "$CANDIDATE_SHA"
    elif [[ $infra_scope == caddy ]]; then
      wd_log "exact tested Caddy configuration installed; continuing the same deployment cycle"
    else
      # A full transaction may change this service's systemd sandbox. The current process still has
      # the old namespace, so only a manager-spawned next cycle may consume the new privileges.
      CURRENT_PHASE=handoff
      status "system definitions installed; next five-second poll starts the updated service"
      github_status pending deploy/watchdog "System definitions installed; continuing on next poll"
      wd_log "full infrastructure transaction installed; deferring to a fresh systemd invocation"
      exit 0
    fi
  fi

  if (( backend_changed == 1 )); then
    apply_migrations_before_deploy "$CANDIDATE_SHA"
  fi

  # The validated backup includes engine and sales databases. Complete it before any independent
  # lane can migrate sales while the engine lane snapshots production.
  if (( engine_changed == 1 )); then
    CURRENT_PHASE=deploying-engine
    CURRENT_PHASE_BEFORE_FAILURE=deploying-engine
    status "creating the validated backup before concurrent component rollouts"
    sudo -n "$BACKUP_RUNNER" "$CANDIDATE_SHA"
  fi

  if (( sales_changed == 0 )) && [[ -z ${SALES_SHA:-} ]]; then
    # First run before sales.sha exists: adopt the current commit as the sales baseline.
    wd_atomic_write "$SALES_FILE" "$CANDIDATE_SHA"
  fi
  if (( openkeys_changed == 0 )) && [[ -z ${OPENKEYS_SHA:-} ]]; then
    # First run before openkeys.sha exists: adopt the current commit as the baseline.
    wd_atomic_write "$OPENKEYS_FILE" "$CANDIDATE_SHA"
  fi
  publish_unchanged_component_statuses \
    "$engine_changed" "$backend_changed" "$sales_changed" "$openkeys_changed"

  CURRENT_PHASE=deploying-components
  CURRENT_PHASE_BEFORE_FAILURE=deploying-components
  status "deploying independent production component lanes in parallel"
  if (( engine_changed == 1 || backend_changed == 1 )); then
    run_rollout_lane deploy_core_components "$CANDIDATE_SHA" "$engine_changed" \
      "$backend_changed" "$codex_changed" &
    core_pid=$!
  fi
  if (( sales_changed == 1 )); then
    run_rollout_lane deploy_sales "$CANDIDATE_SHA" &
    sales_pid=$!
  fi
  if (( openkeys_changed == 1 )); then
    run_rollout_lane deploy_openkeys "$CANDIDATE_SHA" &
    openkeys_pid=$!
  fi

  # Always join every started lane before quarantine/final verification. A failed lane owns its
  # component-specific failure status; this parent owns the single overall verdict.
  if [[ -n $core_pid ]]; then wait "$core_pid" || core_rc=$?; fi
  if [[ -n $sales_pid ]]; then wait "$sales_pid" || sales_rc=$?; fi
  if [[ -n $openkeys_pid ]]; then wait "$openkeys_pid" || openkeys_rc=$?; fi
  if (( core_rc != 0 || sales_rc != 0 || openkeys_rc != 0 )); then
    wd_die "component rollout lanes failed (core=$core_rc sales=$sales_rc openkeys=$openkeys_rc)"
  fi
  if (( engine_changed == 1 )); then ENGINE_SHA=$CANDIDATE_SHA; fi

  CURRENT_PHASE=verifying
  CURRENT_PHASE_BEFORE_FAILURE=verifying
  final_verification_plan=$(wd_final_verification_plan "$delivery_infra_scope" "$engine_changed" \
    "$backend_changed" "$sales_changed" "$openkeys_changed") \
    || wd_die "could not derive the final production verification plan"
  status "running selected final production verification ($final_verification_plan)"
  run_final_verification_plan "$final_verification_plan" "$ENGINE_SHA"

  wd_atomic_write "$PROCESSED_FILE" "$CANDIDATE_SHA"
  rm -f -- "$PENDING_MIGRATION_FILE"
  CURRENT_PHASE=idle
  status "candidate tested and all selected components verified in production"
  github_status success deploy/watchdog "All selected production components verified"
  wd_log "watchdog completed $CANDIDATE_SHA (engine=$engine_changed codex=$codex_changed backend=$backend_changed sales=$sales_changed openkeys=$openkeys_changed)"
}

case "${1:-}" in
  --candidate-validator)
    [[ $# -eq 1 ]] || wd_die "usage: watchdog.sh --candidate-validator"
    candidate_validator_main
    ;;
  --resume)
    [[ $# -eq 2 ]] || wd_die "usage: watchdog.sh --resume <installed-sha>"
    main "$2"
    ;;
  '')
    main
    ;;
  *)
    wd_die "usage: watchdog.sh [--candidate-validator|--resume <installed-sha>]"
    ;;
esac
trap - ERR EXIT INT TERM
