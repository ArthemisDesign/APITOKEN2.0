#!/usr/bin/env bash
set -euo pipefail
set -E

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$SCRIPT_DIR/watchdog-lib.sh"
# shellcheck source=deploy/contour-config.sh
source "$SCRIPT_DIR/contour-config.sh"

SOURCE_REPO=$CONTOUR_ROOTS_SOURCE_REPO
REMOTE=$CONTOUR_GIT_REMOTE
BRANCH=$CONTOUR_GIT_BRANCH
STATE_ROOT=$CONTOUR_ROOTS_STATE
CANDIDATE_ROOT=$CONTOUR_ROOTS_CANDIDATE
CANDIDATE_RETENTION_SECONDS=$((24 * 60 * 60))
# Immutable releases and per-deployment dumps accumulate once per delivery and nothing else removes
# them. Keep enough history for multi-step rollback and forensics, bounded so disk use cannot grow
# without limit. `current`, `previous`, the recorded component SHAs, and any release backing a live
# process are always retained regardless of these counts.
RELEASE_RETENTION_KEEP=10
PREDEPLOY_DUMP_RETENTION_KEEP=10
CI_USER=$CONTOUR_IDENTITY_CI_USER
CI_HOME=$CONTOUR_ROOTS_CI_HOME
CI_CARGO_TARGET=$CONTOUR_ROOTS_CI_CARGO_TARGET
CI_NEXT_CACHE_ROOT=$CONTOUR_ROOTS_CI_NEXT_CACHE
CI_TYPESCRIPT_ARTIFACT_CACHE_ROOT=$CONTOUR_ROOTS_CI_TYPESCRIPT_ARTIFACT_CACHE
CI_TOOLCHAIN=$CONTOUR_ROOTS_CI_TOOLCHAIN
CONTROLLER_ROOT=$CONTOUR_ROOTS_CONTROLLER
CONTROLLER_ENTRYPOINT=${CONTOUR_ROOTS_CONTROLLER%/controller}/watchdog.sh
VALIDATION_PLANNER=$CONTROLLER_ROOT/validation-plan.sh
AUTHBOT_RUNTIME_STATE=$CONTROLLER_ROOT/authbot-runtime-state.sh
GPT_IMAGE_2_LIVE_GATE=$CONTROLLER_ROOT/gpt-image-2-live-gate.sh
GPT_IMAGE_2_IMPLEMENTATION_SHA=1c48e3769f0fe775e650f60ea3c5839458e5dfe2
GPT_IMAGE_2_PUBLIC_SMOKE_GATE=$CONTROLLER_ROOT/gpt-image-2-public-smoke-gate.sh
GPT_IMAGE_2_PUBLIC_PRODUCER_SHA=d2e345f2de75e0ee6c72797fdf315f12ab4bbeb6
GPT_IMAGE_2_PUBLIC_PREFLIGHT_GATE=$CONTROLLER_ROOT/gpt-image-2-public-preflight-gate.sh
GPT_IMAGE_2_PUBLIC_PREFLIGHT_PRODUCER_SHA=d42fc0e3290c0042a16797626326c250e0f6721c
GPT_IMAGE_2_PUBLIC_PREFLIGHT_V2_GATE=$CONTROLLER_ROOT/gpt-image-2-public-preflight-v2-gate.sh
GPT_IMAGE_2_PUBLIC_PREFLIGHT_V2_PRODUCER_SHA=6629ecd7b3725bcd7306ef7a1dc8675ef9160a43
GPT_IMAGE_2_PUBLIC_PREFLIGHT_V3_GATE=$CONTROLLER_ROOT/gpt-image-2-public-preflight-v3-gate.sh
GPT_IMAGE_2_PUBLIC_PREFLIGHT_V3_PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223
GPT_IMAGE_2_PUBLIC_PAID_SMOKE_GATE=$CONTROLLER_ROOT/gpt-image-2-public-paid-smoke-gate.sh
GPT_IMAGE_2_PUBLIC_PAID_SMOKE_PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223
GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V2_GATE=$CONTROLLER_ROOT/gpt-image-2-public-paid-smoke-v2-gate.sh
GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V2_PRODUCER_SHA=853fdc6c8d5be486c371b23df6772eeaf7a48029
GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V3_GATE=$CONTROLLER_ROOT/gpt-image-2-public-paid-smoke-v3-gate.sh
GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V3_PRODUCER_SHA=8b68d73a2a6ba6ffae2f24692b283059f15b7c63
GPT_IMAGE_2_SURFACE_PROBE_GATE=$CONTROLLER_ROOT/gpt-image-2-surface-probe-gate.sh
GPT_IMAGE_2_SURFACE_PROBE_PRODUCER_SHA=d69868fb700aaeb9b6723d8780bb29be4aab9c0d
GPT_IMAGE_2_PUBLIC_PAID_INSPECT_GATE=$CONTROLLER_ROOT/gpt-image-2-public-paid-inspect-gate.sh
GPT_IMAGE_2_PUBLIC_PAID_INSPECT_PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223
WATCHDOG_ROOT=${CONTOUR_ROOTS_CONTROLLER%/controller}
TEST_DB_HELPER=$WATCHDOG_ROOT/watchdog-test-db
BACKUP_RUNNER=$WATCHDOG_ROOT/watchdog-backup.sh
MIGRATION_RUNNER=$WATCHDOG_ROOT/watchdog-migrate.sh
PRICING_RETIREMENT_POSTDROP=$WATCHDOG_ROOT/pricing-retirement-postdrop.sh
INFRASTRUCTURE_RUNNER=$WATCHDOG_ROOT/watchdog-infrastructure.sh
RETENTION_HELPER=$WATCHDOG_ROOT/watchdog-retention.sh
SALES_RUNNER=$CONTROLLER_ROOT/sales-deploy.sh
OPENKEYS_RUNNER=$CONTROLLER_ROOT/openkeys-deploy.sh
ADMIN_RUNNER=$CONTROLLER_ROOT/admin-deploy.sh
DEVBOT_RUNNER=$CONTROLLER_ROOT/devbot-deploy.sh
GITHUB_HELPER=$CONTOUR_GITHUB_REPORTING_HELPER
WATCHDOG_LOCK=$CONTOUR_LOCKS_WATCHDOG
CANDIDATE_VALIDATOR_LOCK=$CONTOUR_LOCKS_CANDIDATE_VALIDATOR
SOURCE_FETCH_LOCK=$CONTOUR_LOCKS_SOURCE_FETCH
DEPLOY_LOCK=$CONTOUR_LOCKS_DEPLOY
ENGINE_RELEASE_ROOT=$CONTOUR_ROOTS_ENGINE_RELEASE
COMMERCE_RELEASE_ROOT=$CONTOUR_ROOTS_COMMERCE_RELEASE
CRM_RELEASE_ROOT=$CONTOUR_ROOTS_CRM_RELEASE
IFS=, read -r ANTHROPIC_PORT_A ANTHROPIC_PORT_B <<<"$CONTOUR_PORTS_ANTHROPIC_SLOTS"
IFS=, read -r OPENAI_PORT_A OPENAI_PORT_B <<<"$CONTOUR_PORTS_OPENAI_SLOTS"
IFS=, read -r GEMINI_PORT_A GEMINI_PORT_B <<<"$CONTOUR_PORTS_GEMINI_SLOTS"
IFS=, read -r KIMI_PORT_A KIMI_PORT_B <<<"$CONTOUR_PORTS_KIMI_SLOTS"
IFS=, read -r ROUTER_PORT_A ROUTER_PORT_B <<<"$CONTOUR_PORTS_ROUTER_SLOTS"
IFS=, read -r API_PORT_A API_PORT_B <<<"$CONTOUR_PORTS_COMMERCE_SLOTS"

PROCESSED_FILE=$STATE_ROOT/processed.sha
INFRASTRUCTURE_FILE=$STATE_ROOT/infrastructure.sha
ENGINE_FILE=$STATE_ROOT/engine.sha
BACKEND_FILE=$STATE_ROOT/backend.sha
SALES_FILE=$STATE_ROOT/sales.sha
OPENKEYS_FILE=$STATE_ROOT/openkeys.sha
ADMIN_FILE=$STATE_ROOT/admin.sha
DEVBOT_FILE=$STATE_ROOT/devbot.sha
# Presence of this operator-provisioned secret file is what enables the devbot lane and unit.
DEVBOT_ENV_FILE=$CONTOUR_ROOTS_CONFIG/devbot.env
REJECTED_FILE=$STATE_ROOT/rejected.sha
PENDING_MIGRATION_FILE=$STATE_ROOT/pending-migration.sha
ROUTER_SUCCESS_PROOF=$STATE_ROOT/router-proof/success
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
  # Repeating a status for the same SHA/context is safe: GitHub keeps the newest value. Absorb a
  # short API or network blip here so reporting availability cannot quarantine an otherwise
  # untested production candidate before any deployment work starts.
  contour_require_status_context "$2" || wd_die "GitHub status context is absent from the contour: $2"
  wd_retry 3 2 sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" "$1" "$2" "$3"
}

github_deployment_start() {
  local component=$1 environment=$2 url=$3
  contour_require_deployment_environment "$environment" \
    || wd_die "GitHub deployment environment is absent from the contour: $environment"
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
  run_github_status_lane pending "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" "Production pipeline started" &
  watchdog_pid=$!
  run_github_status_lane pending "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" "Path-aware isolated validation in progress" &
  tests_pid=$!
  wait "$watchdog_pid" || watchdog_rc=$?
  wait "$tests_pid" || tests_rc=$?
  (( watchdog_rc == 0 && tests_rc == 0 )) \
    || wd_die "could not publish pipeline start statuses (watchdog=$watchdog_rc tests=$tests_rc)"
}

publish_unchanged_component_statuses() {
  local engine_changed=$1 backend_changed=$2 sales_changed=$3 openkeys_changed=$4 admin_changed=$5
  local devbot_changed=$6
  local migration_pid='' engine_pid='' backend_pid='' sales_pid='' openkeys_pid='' admin_pid=''
  local devbot_pid=''
  local migration_rc=0 engine_rc=0 backend_rc=0 sales_rc=0 openkeys_rc=0 admin_rc=0 devbot_rc=0 flag
  for flag in "$engine_changed" "$backend_changed" "$sales_changed" "$openkeys_changed" \
    "$admin_changed" "$devbot_changed"; do
    [[ $flag == 0 || $flag == 1 ]] || wd_die "invalid component reporting flag: $flag"
  done

  if (( backend_changed == 0 )); then
    run_github_status_lane success "$CONTOUR_GITHUB_STATUS_CONTEXT_MIGRATION" "No commerce migration changes" &
    migration_pid=$!
  fi
  if (( engine_changed == 0 )); then
    run_github_status_lane success "$CONTOUR_GITHUB_STATUS_CONTEXT_ENGINE" "No engine changes" &
    engine_pid=$!
  fi
  if (( backend_changed == 0 )); then
    run_github_status_lane success "$CONTOUR_GITHUB_STATUS_CONTEXT_BACKEND" "No backend changes" &
    backend_pid=$!
  fi
  if (( sales_changed == 0 )); then
    run_github_status_lane success "$CONTOUR_GITHUB_STATUS_CONTEXT_SALES" "No sales changes" &
    sales_pid=$!
  fi
  if (( openkeys_changed == 0 )); then
    run_github_status_lane success "$CONTOUR_GITHUB_STATUS_CONTEXT_OPENKEYS" "No openkeys changes" &
    openkeys_pid=$!
  fi
  if (( admin_changed == 0 )); then
    run_github_status_lane success "$CONTOUR_GITHUB_STATUS_CONTEXT_ADMIN" "No admin changes" &
    admin_pid=$!
  fi
  if (( devbot_changed == 0 )); then
    run_github_status_lane success "$CONTOUR_GITHUB_STATUS_CONTEXT_DEVBOT" "No devbot changes" &
    devbot_pid=$!
  fi

  if [[ -n $migration_pid ]]; then wait "$migration_pid" || migration_rc=$?; fi
  if [[ -n $engine_pid ]]; then wait "$engine_pid" || engine_rc=$?; fi
  if [[ -n $backend_pid ]]; then wait "$backend_pid" || backend_rc=$?; fi
  if [[ -n $sales_pid ]]; then wait "$sales_pid" || sales_rc=$?; fi
  if [[ -n $openkeys_pid ]]; then wait "$openkeys_pid" || openkeys_rc=$?; fi
  if [[ -n $admin_pid ]]; then wait "$admin_pid" || admin_rc=$?; fi
  if [[ -n $devbot_pid ]]; then wait "$devbot_pid" || devbot_rc=$?; fi
  (( migration_rc == 0 && engine_rc == 0 && backend_rc == 0 \
      && sales_rc == 0 && openkeys_rc == 0 && admin_rc == 0 && devbot_rc == 0 )) \
    || wd_die "unchanged-component status publication failed (migration=$migration_rc engine=$engine_rc backend=$backend_rc sales=$sales_rc openkeys=$openkeys_rc admin=$admin_rc devbot=$devbot_rc)"
}

test_db() {
  sudo -n "$TEST_DB_HELPER" "$1" "$TEST_DB_SLOT"
}

repair_source_repo_permissions() {
  local objects="$SOURCE_REPO/.git/objects" unsafe unreadable
  [[ -d $objects && ! -L $objects ]] || wd_die "source Git object store is missing: $objects"

  unsafe=$(find "$objects" -type l -print -quit)
  [[ -z $unsafe ]] || wd_die "source Git object store contains an unexpected symlink: $unsafe"

  # The source checkout is shared by the deploy fetcher and the isolated CI reader. Git creates
  # loose objects and pack files according to the caller's umask; normalize only this object store
  # so a private validator transcript can never make history unreadable by apitoken-ci.
  find "$objects" -type d -exec chmod go+rx {} +
  find "$objects" -type f -exec chmod go+r {} +

  unreadable=$(find "$objects" \
    \( -type d ! -perm -0055 -o -type f ! -perm -0044 \) -print -quit)
  [[ -z $unreadable ]] || wd_die "source Git object store remains unreadable: $unreadable"
}

source_repo_readability_check() {
  local invalid errors
  repair_source_repo_permissions
  errors=$(mktemp)
  if ! invalid=$(run_as_ci git -c safe.directory="$SOURCE_REPO" -C "$SOURCE_REPO" \
      cat-file --batch-all-objects --batch-check='%(objectname) %(objecttype)' \
      2>"$errors" \
      | awk '$2 == "missing" || NF != 2 { if (bad == "") bad = $0 } END { if (bad != "") print bad }'); then
    rm -f -- "$errors"
    wd_die "source Git object store cannot be read by $CI_USER"
  fi
  if [[ -s $errors ]]; then
    rm -f -- "$errors"
    wd_die "source Git object store reported read errors for $CI_USER"
  fi
  rm -f -- "$errors"
  [[ -z $invalid ]] || wd_die "source Git object store contains an unreadable object: $invalid"
}

fetch_source_once() (
  exec 8<>"$SOURCE_FETCH_LOCK"
  flock 8
  git -C "$SOURCE_REPO" fetch --no-tags "$REMOTE" "$@"
  source_repo_readability_check
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
  local phase=$1 diagnostic=${2:-"deployment failed closed at $1"} log_url=${3:-}
  [[ -x $GITHUB_HELPER ]] || return 0
  if [[ -n $ACTIVE_DEPLOYMENT_ID ]]; then
    sudo -n "$GITHUB_HELPER" deployment-status "$ACTIVE_DEPLOYMENT_ID" failure \
      "$diagnostic" "$ACTIVE_DEPLOYMENT_ENV" "$ACTIVE_DEPLOYMENT_URL" \
      "$log_url" >/dev/null 2>&1 || true
  fi
  case $phase in
    testing) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
    migrating) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_MIGRATION" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
    deploying-engine) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_ENGINE" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
    deploying-backend) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_BACKEND" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
    deploying-sales) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_SALES" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
    deploying-openkeys) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_OPENKEYS" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
    deploying-admin) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_ADMIN" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
    deploying-devbot) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure "$CONTOUR_GITHUB_STATUS_CONTEXT_DEVBOT" "$diagnostic" "$log_url" >/dev/null 2>&1 || true ;;
  esac
}

# Persist a redacted cycle excerpt and, when the host PAT has Checks: write, upload it as a
# GitHub check run. Never fail-closed on reporting: quarantine and the 140-character status are
# the verdict, and a missing Checks permission must not hide them.
wd_publish_github_failure_log() {
  local sha=$1 phase=$2 rc=$3 diagnostic=$4 html_url=''
  wd_write_failure_report "$sha" "$phase" "$rc" "$diagnostic" || return 1
  [[ -x ${GITHUB_HELPER:-} ]] || return 1
  html_url=$(sudo -n "$GITHUB_HELPER" check-run "$sha" failure \
    "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG_LOG" "$diagnostic") \
    || return 1
  [[ $html_url == "$CONTOUR_ORIGINS_GITHUB_WEB"/* ]] || return 1
  printf '%s' "$html_url"
}

wd_start_cycle_transcript() {
  local sha=$1
  wd_prepare_cycle_log "$sha" >/dev/null
  if command -v stdbuf >/dev/null 2>&1; then
    exec > >(stdbuf -oL -eL tee -a "$WD_CYCLE_LOG") 2>&1
  else
    exec > >(tee -a "$WD_CYCLE_LOG") 2>&1
  fi
}

fail() {
  local rc=$? line=${BASH_LINENO[0]:-unknown}
  local failed_phase=${CURRENT_PHASE_BEFORE_FAILURE:-$CURRENT_PHASE}
  local diagnostic failure_log_url=
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
  diagnostic=$(wd_github_failure_description "$failed_phase" "$rc")
  status "candidate quarantined ($diagnostic)"
  failure_log_url=$(wd_publish_github_failure_log "$CANDIDATE_SHA" "$failed_phase" "$rc" "$diagnostic" || true)
  wd_discard_cycle_log || true
  if [[ -x $GITHUB_HELPER ]]; then
    github_phase_failure "$failed_phase" "$diagnostic" "$failure_log_url"
    sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure \
      "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" \
      "$diagnostic" "$failure_log_url" >/dev/null 2>&1 || true
  fi
  wd_warn "candidate ${CANDIDATE_SHA:-unknown} failed ($diagnostic) and will not be retried automatically"
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
  github_phase_failure "$phase" "$(wd_github_failure_description "$phase" "$rc")"
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

require_fixed_root_executable() {
  local path=$1 parent=${1%/*}
  [[ -d $parent && ! -L $parent ]] \
    || wd_die "required fixed helper parent is missing: $parent"
  [[ $(stat -c '%u:%g:%a' -- "$parent") == 0:0:755 ]] \
    || wd_die "required fixed helper parent must be root:root mode 0755: $parent"
  [[ -f $path && ! -L $path ]] || wd_die "required fixed helper is missing: $path"
  [[ $(stat -c '%u:%g:%a' -- "$path") == 0:0:755 ]] \
    || wd_die "required fixed helper must be root:root mode 0755: $path"
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
      for component in commerce sales openkeys web admin devbot; do
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
    expected_hash=$(wd_marker_value "$marker" router_binary_sha256 2>/dev/null) || return 1
    actual_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/engine/claude-router") || return 1
    [[ $actual_hash == "$expected_hash" ]] || return 1
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
# reach them. Resolve normal active units directly from procfs and fail if their immutable release
# cannot be identified; authbot is non-dumpable, so only the fixed root helper may resolve its
# executable and expose the validated immutable release SHA.
live_release_shas() {
  local unit pid resolved relative name authbot_sha load_state state final_load_state final_state
  local final_pid found exe cwd
  local anthropic_a anthropic_b openai_a openai_b gemini_a gemini_b kimi_a kimi_b
  local router_a router_b api_a api_b
  IFS=, read -r anthropic_a anthropic_b <<<"$CONTOUR_PORTS_ANTHROPIC_SLOTS"
  IFS=, read -r openai_a openai_b <<<"$CONTOUR_PORTS_OPENAI_SLOTS"
  IFS=, read -r gemini_a gemini_b <<<"$CONTOUR_PORTS_GEMINI_SLOTS"
  IFS=, read -r kimi_a kimi_b <<<"$CONTOUR_PORTS_KIMI_SLOTS"
  IFS=, read -r router_a router_b <<<"$CONTOUR_PORTS_ROUTER_SLOTS"
  IFS=, read -r api_a api_b <<<"$CONTOUR_PORTS_COMMERCE_SLOTS"
  for unit in "$CONTOUR_UNITS_ENGINE_LEGACY" \
    "${CONTOUR_UNITS_ENGINE_BRIDGE_TEMPLATE/@.service/@$anthropic_a.service}" \
    "${CONTOUR_UNITS_ENGINE_BRIDGE_TEMPLATE/@.service/@$anthropic_b.service}" \
    "${CONTOUR_UNITS_ANTHROPIC_TEMPLATE/@.service/@$anthropic_a.service}" \
    "${CONTOUR_UNITS_ANTHROPIC_TEMPLATE/@.service/@$anthropic_b.service}" \
    "$CONTOUR_UNITS_OPENAI_LEGACY" \
    "${CONTOUR_UNITS_OPENAI_TEMPLATE/@.service/@$openai_a.service}" \
    "${CONTOUR_UNITS_OPENAI_TEMPLATE/@.service/@$openai_b.service}" \
    "$CONTOUR_UNITS_GEMINI_LEGACY" \
    "${CONTOUR_UNITS_GEMINI_TEMPLATE/@.service/@$gemini_a.service}" \
    "${CONTOUR_UNITS_GEMINI_TEMPLATE/@.service/@$gemini_b.service}" \
    "$CONTOUR_UNITS_KIMI_LEGACY" \
    "${CONTOUR_UNITS_KIMI_TEMPLATE/@.service/@$kimi_a.service}" \
    "${CONTOUR_UNITS_KIMI_TEMPLATE/@.service/@$kimi_b.service}" \
    "$CONTOUR_UNITS_ROUTER_LEGACY" \
    "${CONTOUR_UNITS_ROUTER_TEMPLATE/@.service/@$router_a.service}" \
    "${CONTOUR_UNITS_ROUTER_TEMPLATE/@.service/@$router_b.service}" \
    "${CONTOUR_UNITS_COMMERCE_TEMPLATE/@.service/@$api_a.service}" \
    "${CONTOUR_UNITS_COMMERCE_TEMPLATE/@.service/@$api_b.service}" \
    "$CONTOUR_UNITS_WORKER" "$CONTOUR_UNITS_CONTENT_STUDIO" \
    "$CONTOUR_UNITS_CRM_API" "$CONTOUR_UNITS_CRM_WEB"; do
    load_state=''
    if ! load_state=$(systemctl show "$unit" -p LoadState --value 2>/dev/null); then
      [[ $load_state == not-found ]] && continue
      return 1
    fi
    [[ $load_state != not-found ]] || continue
    [[ $load_state == loaded ]] || return 1
    state=$(systemctl show "$unit" -p ActiveState --value) || return 1
    case $state in
      inactive|failed) continue ;;
      active) ;;
      *) return 1 ;;
    esac
    pid=$(systemctl show "$unit" -p MainPID --value) || return 1
    [[ $pid =~ ^[1-9][0-9]*$ ]] || return 1
    exe=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null) || return 1
    cwd=$(readlink -f -- "/proc/$pid/cwd" 2>/dev/null) || return 1
    found=0
    for resolved in "$exe" "$cwd"; do
      case $resolved in
        "$ENGINE_RELEASE_ROOT"/*) relative=${resolved#"$ENGINE_RELEASE_ROOT"/} ;;
        "$COMMERCE_RELEASE_ROOT"/*) relative=${resolved#"$COMMERCE_RELEASE_ROOT"/} ;;
        "$CRM_RELEASE_ROOT"/*) relative=${resolved#"$CRM_RELEASE_ROOT"/} ;;
        *) continue ;;
      esac
      name=${relative%%/*}
      name=${name#crm-}
      [[ $name =~ ^[0-9a-f]{40}$ ]] || return 1
      printf '%s\n' "$name"
      found=1
    done
    final_load_state=$(systemctl show "$unit" -p LoadState --value) || return 1
    final_state=$(systemctl show "$unit" -p ActiveState --value) || return 1
    final_pid=$(systemctl show "$unit" -p MainPID --value) || return 1
    [[ $final_load_state == loaded && $final_state == active \
        && $final_pid == "$pid" && $found == 1 ]] || return 1
  done

  authbot_sha=$(sudo -n "$AUTHBOT_RUNTIME_STATE" release-sha) || return 1
  [[ -z $authbot_sha || $authbot_sha =~ ^[0-9a-f]{40}$ ]] || return 1
  [[ -z $authbot_sha ]] || printf '%s\n' "$authbot_sha"
}

prune_selected_releases() {
  local root=$1 label=$2 selection=$3 release sha removed=0 failed=0
  while IFS= read -r -d '' release; do
    # The checked selector excludes links, non-SHA names, and protected releases. Revalidate at the
    # destructive boundary as defence in depth; the CRM root's `crm-` name prefix is normalized
    # before the SHA check, matching the selector's pattern.
    sha=${release##*/}
    sha=${sha#crm-}
    if [[ ${release%/*} != "$root" || ! $sha =~ ^[0-9a-f]{40}$ || ! -d $release || -L $release ]]; then
      wd_warn "unsafe $label retention target skipped: $release"
      failed=$((failed + 1))
      continue
    fi
    sudo -n chmod -R u+w -- "$release" 2>/dev/null || true
    if sudo -n rm -rf --one-file-system -- "$release"; then
      removed=$((removed + 1))
    else
      wd_warn "failed to remove expired $label release $sha"
      failed=$((failed + 1))
    fi
  done <"$selection"
  if (( removed > 0 || failed > 0 )); then
    wd_log "$label release retention finished: removed=$removed failed=$failed keep=$RELEASE_RETENTION_KEEP"
  fi
  return 0
}

prune_expired_releases() {
  local protected=() sha live_shas engine_selection commerce_selection crm_selection
  exec 8<>"$DEPLOY_LOCK" || return 1
  if ! flock -n 8; then
    exec 8>&-
    return 1
  fi
  engine_selection=$(mktemp) || { flock -u 8; exec 8>&-; return 1; }
  commerce_selection=$(mktemp) \
    || { rm -f -- "$engine_selection"; flock -u 8; exec 8>&-; return 1; }
  crm_selection=$(mktemp) \
    || { rm -f -- "$engine_selection" "$commerce_selection"; flock -u 8; exec 8>&-; return 1; }

  # Build one complete live snapshot and all complete NUL selections before mutating either root.
  # No process substitution is used here: producer status is checked directly.
  if ! live_shas=$(live_release_shas); then
    rm -f -- "$engine_selection" "$commerce_selection" "$crm_selection"
    flock -u 8
    exec 8>&-
    return 1
  fi
  while IFS= read -r sha; do
    [[ -z $sha ]] && continue
    if [[ ! $sha =~ ^[0-9a-f]{40}$ ]]; then
      rm -f -- "$engine_selection" "$commerce_selection" "$crm_selection"
      flock -u 8
      exec 8>&-
      return 1
    fi
    protected+=("$sha")
  done <<<"$live_shas"
  for sha in "${ENGINE_SHA:-}" "${BACKEND_SHA:-}" "${PROCESSED_SHA:-}" \
    "${SALES_SHA:-}" "${OPENKEYS_SHA:-}"; do
    [[ $sha =~ ^[0-9a-f]{40}$ ]] && protected+=("$sha")
  done
  if ! wd_prunable_release_dirs "$ENGINE_RELEASE_ROOT" "$RELEASE_RETENTION_KEEP" \
      "${protected[@]}" >"$engine_selection" \
      || ! wd_prunable_release_dirs "$COMMERCE_RELEASE_ROOT" "$RELEASE_RETENTION_KEEP" \
        "${protected[@]}" >"$commerce_selection" \
      || ! wd_prunable_release_dirs "$CRM_RELEASE_ROOT" "$RELEASE_RETENTION_KEEP" \
        --pattern '^(crm-)?[0-9a-f]{40}$' "${protected[@]}" >"$crm_selection"; then
    rm -f -- "$engine_selection" "$commerce_selection" "$crm_selection"
    flock -u 8
    exec 8>&-
    return 1
  fi

  prune_selected_releases "$ENGINE_RELEASE_ROOT" engine "$engine_selection"
  prune_selected_releases "$COMMERCE_RELEASE_ROOT" commerce "$commerce_selection"
  prune_selected_releases "$CRM_RELEASE_ROOT" crm "$crm_selection"
  rm -f -- "$engine_selection" "$commerce_selection" "$crm_selection"
  flock -u 8
  exec 8>&-
  return 0
}

prune_expired_releases_best_effort() {
  if prune_expired_releases; then
    return 0
  fi
  wd_warn "release retention skipped: live process observation or release selection was incomplete" \
    || true
  status "release retention skipped safely; continuing candidate processing" || true
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
    lane_components=commerce,sales,openkeys,web,admin,devbot
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
  local candidate=$1 engine_dsn=$2 build_artifacts=$3 redis_url=$4 sha=$5
  wd_validate_sha "$sha"
  wd_log "running the locked Rust workspace tests from the shared target cache"
  # CLAUDE_API_TEST_REDIS_URL is mandatory here, not optional. The shared cache-affinity L2 is the
  # only place where two engine slots agree on a prompt-cache home, and its single-winner and
  # opaque-keyspace invariants are unprovable without a real Redis. Passing an empty value would
  # silently return the suite to the state where ~350 lines of L2 code were never executed.
  [[ -n $redis_url ]] || wd_die "the Rust lane requires a disposable Redis URL"
  # CI=1 turns the suite's "no Redis configured" escape hatch into a hard failure. Locally that
  # escape hatch keeps `cargo test` usable without Docker; here it must never be reachable, or the
  # L2 coverage could silently regress to skipped without any lane turning red.
  run_as_ci env -u CLAUDE_API_IMPLEMENTATION_SHA \
    CLAUDE_API_TEST_DATABASE_URL="$engine_dsn" \
    CLAUDE_API_TEST_REDIS_URL="$redis_url" \
    CI=1 \
    cargo test --locked --workspace --manifest-path "$candidate/Cargo.toml"
  if (( build_artifacts == 1 )); then
    wd_log "building the production engine, authbot and router once, from the tested candidate"
    run_as_ci env CLAUDE_API_IMPLEMENTATION_SHA="$sha" \
      cargo build --locked --release -p claude-api --manifest-path "$candidate/Cargo.toml"
    run_as_ci env -u CLAUDE_API_IMPLEMENTATION_SHA \
      cargo build --locked --release -p authbot -p claude-router \
      --manifest-path "$candidate/Cargo.toml"
    run_as_ci install -d -m 0755 "$candidate/.deploy-artifacts/engine"
    run_as_ci install -m 0755 "$CI_CARGO_TARGET/release/claude-api" \
      "$candidate/.deploy-artifacts/engine/claude-api"
    run_as_ci install -m 0755 "$CI_CARGO_TARGET/release/authbot" \
      "$candidate/.deploy-artifacts/engine/authbot"
    run_as_ci install -m 0755 "$CI_CARGO_TARGET/release/claude-router" \
      "$candidate/.deploy-artifacts/engine/claude-router"
  fi
}

test_control_api_acceptance() {
  local candidate=$1 engine_dsn=$2
  local binary="$candidate/.deploy-artifacts/engine/claude-api"
  [[ -x $binary ]] || wd_die "Control API acceptance requires the tested release claude-api binary"
  wd_log "building the real EngineClient package export for assembled Control API acceptance"
  run_as_ci pnpm --dir "$candidate" install --frozen-lockfile
  run_as_ci pnpm --dir "$candidate" --filter @claude-api/contracts \
    --filter @claude-api/engine-client -r --if-present --fail-if-no-match build
  wd_log "running built claude-api and EngineClient against disposable PostgreSQL"
  run_as_ci env CLAUDE_API_BIN="$binary" CLAUDE_API_TEST_DATABASE_URL="$engine_dsn" \
    CONTROL_API_ACCEPTANCE_PORT="$((17480 + TEST_DB_SLOT))" \
    bash "$candidate/tests/control_api_engine_client_acceptance.sh"
}

test_router_engine_replay() {
  local candidate=$1
  local engine="$candidate/.deploy-artifacts/engine/claude-api"
  local router="$candidate/.deploy-artifacts/engine/claude-router"
  [[ -x $engine && -x $router ]] \
    || wd_die "router-engine replay requires both tested release binaries"
  wd_log "running deterministic keyless router→engine→mock replay"
  run_as_ci env CI=1 CLAUDE_API_BIN="$engine" CLAUDE_ROUTER_BIN="$router" \
    python3 "$candidate/tests/router_engine_replay.py"
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
  run_as_ci python3 "$candidate/deploy/repository-invariants.py" "$candidate"
  if [[ -n $diff_base ]]; then
    run_as_ci bash "$candidate/deploy/docs-check.sh" "$diff_base" "$sha"
  fi
  if (( run_regression_suites == 1 )); then
    wd_log "running deployment and merge-workflow regression suites"
    run_as_ci bash "$candidate/deploy/lib.test.sh"
    run_as_ci bash "$candidate/deploy/shutdown-ladder.test.sh"
    run_as_ci bash "$candidate/deploy/codex-homes-migrate.test.sh"
    run_as_ci bash "$candidate/deploy/watchdog-backup.test.sh"
    run_as_ci bash "$candidate/deploy/contour-config.test.sh"
    run_as_ci bash "$candidate/deploy/stage-unit-renderer.test.sh"
    run_as_ci bash "$candidate/deploy/staging-foundation.test.sh"
    run_as_ci bash "$candidate/deploy/watchdog-lib.test.sh"
    run_as_ci bash "$candidate/deploy/monitoring-config.test.sh"
    run_as_ci bash "$candidate/deploy/sccache-cargo.test.sh"
    run_as_ci bash "$candidate/deploy/agent-worktree.test.sh"
    run_as_ci bash "$candidate/deploy/delete-worktree-agent.test.sh"
    run_as_ci bash "$candidate/deploy/next-cache.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-scope.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-build-contexts.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-artifact-cache.test.sh"
    run_as_ci bash "$candidate/deploy/typescript-test-groups.test.sh"
    run_as_ci bash "$candidate/deploy/commerce-release-bundle.test.sh"
    run_as_ci bash "$candidate/deploy/change-plan.test.sh"
    run_as_ci bash "$candidate/deploy/repository-invariants.test.sh"
    run_as_ci bash "$candidate/deploy/docs-check.test.sh"
    run_as_ci bash "$candidate/deploy/apitoken-observe.test.sh"
    run_as_ci bash "$candidate/deploy/host-image-gate.test.sh"
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
  local candidate marker dsn= engine_dsn= sales_dsn= openkeys_dsn= redis_url= manifest digest tree
  local typescript_pid= rust_pid= static_pid=
  local typescript_rc=0 rust_rc=0 codex_rc=0 static_rc=0 acceptance_rc=0 replay_rc=0
  local typescript_components=none typescript_digest=none component
  local typescript_digest_commerce=none typescript_digest_sales=none
  local typescript_digest_openkeys=none typescript_digest_web=none
  local typescript_digest_admin=none typescript_digest_devbot=none
  local commerce_release_bundle_hash=none
  local engine_hash=none authbot_hash=none router_hash=none
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
    redis_url=$(test_db redis-url)
  fi

  # Resolve every fallible prerequisite before starting children. Once launched, the parent reaches
  # every wait and the database cleanup path even when any individual lane fails.
  if (( typescript_required == 1 )); then
    run_candidate_lane test_typescript_lane "$candidate" "$dsn" "$sales_dsn" "$openkeys_dsn" \
      "$typescript_base" "$sha" "$typescript_full" "$typescript_components" &
    typescript_pid=$!
  fi
  if (( rust_required == 1 )); then
    run_candidate_lane test_rust_lane "$candidate" "$engine_dsn" "$engine_artifacts_required" \
      "$redis_url" "$sha" &
    rust_pid=$!
  fi
  run_candidate_lane test_static_lane "$candidate" "$sha" "$static_required" &
  static_pid=$!

  # The language and static suites are independent once their prerequisites exist. Wait for every
  # child even when one fails so no candidate-owned process survives into cleanup or a later cycle.
  if [[ -n $typescript_pid ]]; then wait "$typescript_pid" || typescript_rc=$?; fi
  if [[ -n $rust_pid ]]; then wait "$rust_pid" || rust_rc=$?; fi
  wait "$static_pid" || static_rc=$?
  if (( rust_rc == 0 && static_rc == 0 && engine_artifacts_required == 1 )); then
    test_control_api_acceptance "$candidate" "$engine_dsn" || acceptance_rc=$?
    (( acceptance_rc != 0 )) || test_router_engine_replay "$candidate" || replay_rc=$?
  fi
  if (( TEST_DB_STARTED == 1 )); then
    test_db stop
    TEST_DB_STARTED=0
  fi
  (( typescript_rc == 0 )) || wd_die "TypeScript candidate lane failed (exit $typescript_rc)"
  (( rust_rc == 0 )) || wd_die "Rust candidate lane failed (exit $rust_rc)"
  (( codex_rc == 0 )) || wd_die "Codex candidate lane failed (exit $codex_rc)"
  (( static_rc == 0 )) || wd_die "Static candidate lane failed (exit $static_rc)"
  (( acceptance_rc == 0 )) || wd_die "Control API assembled acceptance failed (exit $acceptance_rc)"
  (( replay_rc == 0 )) || wd_die "Router-engine replay failed (exit $replay_rc)"

  [[ -z $(run_as_ci git -C "$candidate" status --porcelain --untracked-files=no) ]] \
    || wd_die "tests modified tracked candidate files"
  manifest="$STATE_ROOT/.candidate-manifest.${BASHPID:-$$}"
  wd_migration_manifest "$candidate" >"$manifest"
  digest=$(wd_manifest_digest "$manifest")
  tree=$(run_as_ci git -C "$candidate" rev-parse 'HEAD^{tree}')
  if (( typescript_required == 1 )); then
    for component in commerce sales openkeys web admin devbot; do
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
        admin)
          typescript_digest_admin=$(wd_typescript_component_artifact_digest \
            "$candidate" "$component")
          ;;
        devbot)
          typescript_digest_devbot=$(wd_typescript_component_artifact_digest \
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
    router_hash=$(wd_sha256_file "$candidate/.deploy-artifacts/engine/claude-router")
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
    printf 'typescript_artifact_digest_admin=%s\n' "$typescript_digest_admin"
    printf 'typescript_artifact_digest_devbot=%s\n' "$typescript_digest_devbot"
    printf 'commerce_release_bundle_sha256=%s\n' "$commerce_release_bundle_hash"
    printf 'engine_binary_sha256=%s\n' "$engine_hash"
    printf 'authbot_binary_sha256=%s\n' "$authbot_hash"
    printf 'router_binary_sha256=%s\n' "$router_hash"
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
  # The native Codex provider has no pinned sidecar artifact: its wire identity ships inside
  # the tested engine binary, so the engine lane already proves every Codex-affecting change.
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
  ADMIN_SHA=$(wd_read_sha "$ADMIN_FILE" 2>/dev/null || printf '')
  DEVBOT_SHA=$(wd_read_sha "$DEVBOT_FILE" 2>/dev/null || printf '')
}

shadow_validation_exit() {
  local rc=$1 summary
  trap - ERR EXIT INT TERM

  # Preserve the full mode-0600 validator transcript in journald. The GitHub commit-status
  # description stays short; the same transcript is also reduced into a redacted check-run log.
  if [[ -f ${SHADOW_LOG_FILE:-} && ! -L ${SHADOW_LOG_FILE:-} ]]; then
    cat -- "$SHADOW_LOG_FILE" >&8 || true
  fi
  (( rc != 0 )) || return 0

  if (( TEST_DB_STARTED == 1 )); then
    test_db stop >/dev/null 2>&1 || true
  fi
  summary=$(wd_validation_failure_summary "$SHADOW_LOG_FILE" "$rc" \
    "${CURRENT_PHASE_BEFORE_FAILURE:-${CURRENT_PHASE:-unknown}}")
  WD_CYCLE_LOG=$SHADOW_LOG_FILE
  failure_log_url=$(wd_publish_github_failure_log "$CANDIDATE_SHA" \
    "${CURRENT_PHASE_BEFORE_FAILURE:-${CURRENT_PHASE:-unknown}}" "$rc" "$summary" || true)
  sudo -n "$GITHUB_HELPER" deployment-status "$SHADOW_DEPLOYMENT_ID" failure \
    "$summary" "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_CANDIDATE_VALIDATION" "" "$failure_log_url" \
    >/dev/null 2>&1 || true
  sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure \
    "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" "$summary" "$failure_log_url" >/dev/null 2>&1 || true
  wd_warn "trusted shadow validation failed for $CANDIDATE_SHA: $summary; production remains unchanged" >&8 2>&8
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
  local committed_master=$4 current_master previous_umask
  CI_CARGO_TARGET="$CI_HOME/cargo-target-shadow-$TEST_DB_SLOT"
  STATUS_FILE="$STATE_ROOT/candidate-validation-$TEST_DB_SLOT.status"
  SHADOW_LOG_FILE="$STATE_ROOT/candidate-validation-$TEST_DB_SLOT.log"
  WD_CYCLE_LOG=$SHADOW_LOG_FILE
  VALIDATION_BASE_SHA=$PROCESSED_SHA
  TEST_DB_STARTED=0
  exec 8>&1 9>&2
  rm -f -- "$SHADOW_LOG_FILE"
  previous_umask=$(umask)
  umask 077
  : >"$SHADOW_LOG_FILE"
  chmod 0600 "$SHADOW_LOG_FILE"
  umask "$previous_umask"
  exec >>"$SHADOW_LOG_FILE" 2>&1
  trap 'exit 130' INT
  trap 'exit 143' TERM
  trap 'shadow_validation_exit "$?"' EXIT

  CURRENT_PHASE=shadow-validation
  CURRENT_PHASE_BEFORE_FAILURE=shadow-validation
  status "validating an exact pre-merge candidate in isolated slot $TEST_DB_SLOT"
  wd_retry 3 5 sudo -n "$GITHUB_HELPER" deployment-status "$SHADOW_DEPLOYMENT_ID" in_progress \
    "Trusted production-host candidate validation started" "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_CANDIDATE_VALIDATION" ""
  github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" "Trusted production-host candidate validation in progress"

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
  [[ -z $ADMIN_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$ADMIN_SHA" "$CANDIDATE_SHA" shadow-admin
  [[ -z $DEVBOT_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$DEVBOT_SHA" "$CANDIDATE_SHA" shadow-devbot

  # Validate from the real production component baselines. The committed parent can be red when
  # agent-merge is repairing it; treating that undeployed parent as a production baseline both
  # misses runtime drift and makes production discard/rebuild this exact green candidate after
  # merge. A later successful parent only narrows these requirements, and candidate_is_tested
  # deliberately accepts that already-tested superset.
  select_candidate_validation_requirements "$CANDIDATE_SHA"
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

  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" "Trusted production-host candidate validation passed"
  wd_retry 3 5 sudo -n "$GITHUB_HELPER" deployment-status "$SHADOW_DEPLOYMENT_ID" success \
    "Exact candidate passed trusted production-host validation" "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_CANDIDATE_VALIDATION" ""
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

  [[ $(id -un) == "$CONTOUR_IDENTITY_RUNTIME_USER" ]] \
    || wd_die "candidate validator service must run as $CONTOUR_IDENTITY_RUNTIME_USER"
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
  [[ -z $ADMIN_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$ADMIN_SHA" "$master_sha" validator-admin
  [[ -z $DEVBOT_SHA ]] \
    || wd_require_ancestor "$SOURCE_REPO" "$DEVBOT_SHA" "$master_sha" validator-devbot

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
  router_runtime_aligned "$sha" \
    || wd_die "router runtime is not in a single-slot steady state after cutover: $ROUTER_RUNTIME_DETAIL"
}

engine_runtime_aligned() {
  # $2 is a retained no-op: the native provider needs no daemon-cohort convergence phase.
  local sha=$1 codex_alignment=${2:-converged}
  local expected current legacy_active=0 legacy_enabled=0 stable_status
  local active_8787=0 ready_8787=0 current_8787=0 enabled_8787=0
  local active_8788=0 ready_8788=0 current_8788=0 enabled_8788=0
  local openai_active_8793=0 openai_ready_8793=0 openai_current_8793=0 openai_enabled_8793=0
  local openai_active_8797=0 openai_ready_8797=0 openai_current_8797=0 openai_enabled_8797=0
  local openai_legacy_active=0 openai_legacy_ready=0 openai_legacy_current=0
  local openai_legacy_enabled=0 openai_shared_supported=0 openai_stable_status
  local gemini_active_8795=0 gemini_ready_8795=0 gemini_current_8795=0 gemini_enabled_8795=0
  local gemini_active_8799=0 gemini_ready_8799=0 gemini_current_8799=0 gemini_enabled_8799=0
  local gemini_legacy_active=0 gemini_legacy_ready=0 gemini_legacy_current=0
  local gemini_legacy_enabled=0 gemini_supported=0 gemini_shared_supported=0 gemini_stable_status
  local kimi_active_8804=0 kimi_ready_8804=0 kimi_current_8804=0 kimi_enabled_8804=0
  local kimi_active_8805=0 kimi_ready_8805=0 kimi_current_8805=0 kimi_enabled_8805=0
  local kimi_legacy_active=0 kimi_legacy_ready=0 kimi_legacy_current=0
  local kimi_legacy_enabled=0 kimi_supported=0 kimi_shared_supported=0 kimi_stable_status
  local port unit pid executable status combined_unit environment

  [[ $codex_alignment == serving || $codex_alignment == converged ]] || return 2

  expected="$ENGINE_RELEASE_ROOT/$sha"
  current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
  if [[ $current != "$expected" ]]; then
    ENGINE_RUNTIME_DETAIL="current=$current expected=$expected"
    return 1
  fi

  for port in "$ANTHROPIC_PORT_A" "$ANTHROPIC_PORT_B"; do
    unit="${CONTOUR_UNITS_ANTHROPIC_TEMPLATE/@.service/@$port.service}"
    local active=0 ready=0 selected=0 enabled=0
    systemctl is-active --quiet "$unit" && active=1
    systemctl is-enabled --quiet "$unit" && enabled=1
    status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
      "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$port/ready" 2>/dev/null || true)
    [[ $status == 200 ]] && ready=1
    if (( active == 1 )); then
      pid=$(systemctl show "$unit" -p MainPID --value)
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
        if [[ $executable == "$expected/claude-api" ]] \
            && tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null \
              | grep -Fxq 'CLAUDE_API_PROVIDER=anthropic'; then
          selected=1
        fi
      fi
    fi
    if [[ $port == "$ANTHROPIC_PORT_A" ]]; then
      active_8787=$active; ready_8787=$ready; current_8787=$selected; enabled_8787=$enabled
    else
      active_8788=$active; ready_8788=$ready; current_8788=$selected; enabled_8788=$enabled
    fi
  done
  for combined_unit in "$CONTOUR_UNITS_ENGINE_LEGACY" \
    "${CONTOUR_UNITS_ENGINE_BRIDGE_TEMPLATE/@.service/@$ANTHROPIC_PORT_A.service}" \
    "${CONTOUR_UNITS_ENGINE_BRIDGE_TEMPLATE/@.service/@$ANTHROPIC_PORT_B.service}"; do
    systemctl is-active --quiet "$combined_unit" && legacy_active=1
    systemctl is-enabled --quiet "$combined_unit" && legacy_enabled=1
  done
  for port in "$OPENAI_PORT_A" "$OPENAI_PORT_B"; do
    unit="${CONTOUR_UNITS_OPENAI_TEMPLATE/@.service/@$port.service}"
    local openai_active=0 openai_ready=0 openai_selected=0 openai_enabled=0
    systemctl is-active --quiet "$unit" && openai_active=1
    systemctl is-enabled --quiet "$unit" && openai_enabled=1
    status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
      "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$port/ready" 2>/dev/null || true)
    [[ $status == 200 ]] && openai_ready=1
    if (( openai_active == 1 )); then
      pid=$(systemctl show "$unit" -p MainPID --value)
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
        environment=$(tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null || true)
        if [[ $executable == "$expected/claude-api" ]] \
            && grep -Fxq 'CLAUDE_API_PROVIDER=openai' <<<"$environment" \
            && ! grep -Fq 'CLAUDE_API_CODEX_TRANSPORT' <<<"$environment"; then
          openai_selected=1
        fi
      fi
    fi
    if [[ $port == "$OPENAI_PORT_A" ]]; then
      openai_active_8793=$openai_active; openai_ready_8793=$openai_ready
      openai_current_8793=$openai_selected; openai_enabled_8793=$openai_enabled
    else
      openai_active_8797=$openai_active; openai_ready_8797=$openai_ready
      openai_current_8797=$openai_selected; openai_enabled_8797=$openai_enabled
    fi
  done
  systemctl is-active --quiet "$CONTOUR_UNITS_OPENAI_LEGACY" && openai_legacy_active=1
  systemctl is-enabled --quiet "$CONTOUR_UNITS_OPENAI_LEGACY" && openai_legacy_enabled=1
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$OPENAI_PORT_A/ready" 2>/dev/null || true)
  [[ $status == 200 && $openai_legacy_active == 1 ]] && openai_legacy_ready=1
  if (( openai_legacy_active == 1 )); then
    pid=$(systemctl show "$CONTOUR_UNITS_OPENAI_LEGACY" -p MainPID --value)
    if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
      executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
      if [[ $executable == "$expected/claude-api" ]] \
          && tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null \
            | grep -Fxq 'CLAUDE_API_PROVIDER=openai'; then
        openai_legacy_current=1
      fi
    fi
  fi
  stable_status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$CONTOUR_ORIGINS_ANTHROPIC_STABLE/ready" 2>/dev/null || true)
  openai_stable_status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$CONTOUR_ORIGINS_OPENAI_STABLE/ready" 2>/dev/null || true)
  if [[ -e "$expected/.openai-bluegreen-v1" || -L "$expected/.openai-bluegreen-v1" ]]; then
    [[ -f "$expected/.openai-bluegreen-v1" && ! -L "$expected/.openai-bluegreen-v1" \
        && $(<"$expected/.openai-bluegreen-v1") == openai-bluegreen-v1 ]] || return 1
    openai_shared_supported=1
  fi
  if [[ -f "$expected/.gemini-provider-v1" && ! -L "$expected/.gemini-provider-v1" \
      && $(<"$expected/.gemini-provider-v1") == gemini-provider-v1 ]]; then
    gemini_supported=1
  fi
  if [[ -e "$expected/.gemini-bluegreen-v1" || -L "$expected/.gemini-bluegreen-v1" ]]; then
    [[ -f "$expected/.gemini-bluegreen-v1" && ! -L "$expected/.gemini-bluegreen-v1" \
        && $(<"$expected/.gemini-bluegreen-v1") == gemini-bluegreen-v1 ]] || return 1
    (( gemini_supported == 1 )) || return 1
    gemini_shared_supported=1
  fi
  if [[ -f "$expected/.kimi-provider-v1" && ! -L "$expected/.kimi-provider-v1" \
      && $(<"$expected/.kimi-provider-v1") == kimi-provider-v1 ]]; then
    kimi_supported=1
  fi
  if [[ -e "$expected/.kimi-bluegreen-v1" || -L "$expected/.kimi-bluegreen-v1" ]]; then
    [[ -f "$expected/.kimi-bluegreen-v1" && ! -L "$expected/.kimi-bluegreen-v1" \
        && $(<"$expected/.kimi-bluegreen-v1") == kimi-bluegreen-v1 ]] || return 1
    (( kimi_supported == 1 )) || return 1
    kimi_shared_supported=1
  fi
  for port in "$GEMINI_PORT_A" "$GEMINI_PORT_B"; do
    unit="${CONTOUR_UNITS_GEMINI_TEMPLATE/@.service/@$port.service}"
    local gemini_active=0 gemini_ready=0 gemini_selected=0 gemini_enabled=0
    systemctl is-active --quiet "$unit" && gemini_active=1
    systemctl is-enabled --quiet "$unit" && gemini_enabled=1
    status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
      "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$port/ready" 2>/dev/null || true)
    [[ $status == 200 ]] && gemini_ready=1
    if (( gemini_active == 1 )); then
      pid=$(systemctl show "$unit" -p MainPID --value)
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
        if [[ $executable == "$expected/claude-api" ]] \
            && tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null \
              | grep -Fxq 'CLAUDE_API_PROVIDER=gemini'; then
          gemini_selected=1
        fi
      fi
    fi
    if [[ $port == "$GEMINI_PORT_A" ]]; then
      gemini_active_8795=$gemini_active; gemini_ready_8795=$gemini_ready
      gemini_current_8795=$gemini_selected; gemini_enabled_8795=$gemini_enabled
    else
      gemini_active_8799=$gemini_active; gemini_ready_8799=$gemini_ready
      gemini_current_8799=$gemini_selected; gemini_enabled_8799=$gemini_enabled
    fi
  done
  systemctl is-active --quiet "$CONTOUR_UNITS_GEMINI_LEGACY" && gemini_legacy_active=1
  systemctl is-enabled --quiet "$CONTOUR_UNITS_GEMINI_LEGACY" && gemini_legacy_enabled=1
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$GEMINI_PORT_A/ready" 2>/dev/null || true)
  [[ $status == 200 && $gemini_legacy_active == 1 ]] && gemini_legacy_ready=1
  if (( gemini_legacy_active == 1 )); then
    pid=$(systemctl show "$CONTOUR_UNITS_GEMINI_LEGACY" -p MainPID --value)
    if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
      executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
      if [[ $executable == "$expected/claude-api" ]] \
          && tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null \
            | grep -Fxq 'CLAUDE_API_PROVIDER=gemini'; then
        gemini_legacy_current=1
      fi
    fi
  fi
  gemini_stable_status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$CONTOUR_ORIGINS_GEMINI_STABLE/ready" 2>/dev/null || true)
  for port in "$KIMI_PORT_A" "$KIMI_PORT_B"; do
    unit="${CONTOUR_UNITS_KIMI_TEMPLATE/@.service/@$port.service}"
    local kimi_active=0 kimi_ready=0 kimi_selected=0 kimi_enabled=0
    systemctl is-active --quiet "$unit" && kimi_active=1
    systemctl is-enabled --quiet "$unit" && kimi_enabled=1
    status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
      "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$port/ready" 2>/dev/null || true)
    [[ $status == 200 ]] && kimi_ready=1
    if (( kimi_active == 1 )); then
      pid=$(systemctl show "$unit" -p MainPID --value)
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
        if [[ $executable == "$expected/claude-api" ]] \
            && tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null \
              | grep -Fxq 'CLAUDE_API_PROVIDER=kimi'; then
          kimi_selected=1
        fi
      fi
    fi
    if [[ $port == "$KIMI_PORT_A" ]]; then
      kimi_active_8804=$kimi_active; kimi_ready_8804=$kimi_ready
      kimi_current_8804=$kimi_selected; kimi_enabled_8804=$kimi_enabled
    else
      kimi_active_8805=$kimi_active; kimi_ready_8805=$kimi_ready
      kimi_current_8805=$kimi_selected; kimi_enabled_8805=$kimi_enabled
    fi
  done
  systemctl is-active --quiet "$CONTOUR_UNITS_KIMI_LEGACY" && kimi_legacy_active=1
  systemctl is-enabled --quiet "$CONTOUR_UNITS_KIMI_LEGACY" && kimi_legacy_enabled=1
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$KIMI_PORT_A/ready" 2>/dev/null || true)
  [[ $status == 200 && $kimi_legacy_active == 1 ]] && kimi_legacy_ready=1
  if (( kimi_legacy_active == 1 )); then
    pid=$(systemctl show "$CONTOUR_UNITS_KIMI_LEGACY" -p MainPID --value)
    if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
      executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
      if [[ $executable == "$expected/claude-api" ]] \
          && tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null \
            | grep -Fxq 'CLAUDE_API_PROVIDER=kimi'; then
        kimi_legacy_current=1
      fi
    fi
  fi
  kimi_stable_status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$CONTOUR_ORIGINS_KIMI_STABLE/ready" 2>/dev/null || true)
  ENGINE_RUNTIME_DETAIL="anthropic[8787=$active_8787:$ready_8787:$current_8787:$enabled_8787 8788=$active_8788:$ready_8788:$current_8788:$enabled_8788 stable=${stable_status:-unreachable}] openai[shared=$openai_shared_supported 8793=$openai_active_8793:$openai_ready_8793:$openai_current_8793:$openai_enabled_8793 8797=$openai_active_8797:$openai_ready_8797:$openai_current_8797:$openai_enabled_8797 stable=${openai_stable_status:-unreachable} legacy=$openai_legacy_active:$openai_legacy_ready:$openai_legacy_current:$openai_legacy_enabled] gemini[supported=$gemini_supported shared=$gemini_shared_supported 8795=$gemini_active_8795:$gemini_ready_8795:$gemini_current_8795:$gemini_enabled_8795 8799=$gemini_active_8799:$gemini_ready_8799:$gemini_current_8799:$gemini_enabled_8799 stable=${gemini_stable_status:-unreachable} legacy=$gemini_legacy_active:$gemini_legacy_ready:$gemini_legacy_current:$gemini_legacy_enabled] kimi[supported=$kimi_supported shared=$kimi_shared_supported 8804=$kimi_active_8804:$kimi_ready_8804:$kimi_current_8804:$kimi_enabled_8804 8805=$kimi_active_8805:$kimi_ready_8805:$kimi_current_8805:$kimi_enabled_8805 stable=${kimi_stable_status:-unreachable} legacy=$kimi_legacy_active:$kimi_legacy_ready:$kimi_legacy_current:$kimi_legacy_enabled] legacy=$legacy_active:$legacy_enabled"
  [[ $stable_status == 200 ]] || return 1
  [[ $openai_stable_status == 200 ]] || return 1
  if (( openai_shared_supported == 0 )); then
    (( openai_legacy_active == 1 && openai_legacy_ready == 1 \
      && openai_legacy_current == 1 && openai_legacy_enabled == 1 \
      && openai_active_8793 == 0 && openai_current_8793 == 0 && openai_enabled_8793 == 0 \
      && openai_active_8797 == 0 && openai_ready_8797 == 0 \
      && openai_current_8797 == 0 && openai_enabled_8797 == 0 )) || return 1
  else
    (( openai_legacy_active == 0 && openai_legacy_enabled == 0 )) || return 1
    if (( openai_active_8793 == 1 )); then
      (( openai_ready_8793 == 1 && openai_current_8793 == 1 && openai_enabled_8793 == 1 \
        && openai_active_8797 == 0 && openai_ready_8797 == 0 \
        && openai_current_8797 == 0 && openai_enabled_8797 == 0 )) || return 1
    else
      (( openai_active_8797 == 1 && openai_ready_8797 == 1 \
        && openai_current_8797 == 1 && openai_enabled_8797 == 1 \
        && openai_active_8793 == 0 && openai_ready_8793 == 0 \
        && openai_current_8793 == 0 && openai_enabled_8793 == 0 )) || return 1
    fi
  fi
  if (( gemini_supported == 1 )); then
    [[ $gemini_stable_status == 200 ]] || return 1
    if (( gemini_shared_supported == 0 )); then
      (( gemini_legacy_active == 1 && gemini_legacy_ready == 1 \
        && gemini_legacy_current == 1 && gemini_legacy_enabled == 1 \
        && gemini_active_8795 == 0 && gemini_ready_8795 == 0 \
        && gemini_current_8795 == 0 && gemini_enabled_8795 == 0 \
        && gemini_active_8799 == 0 && gemini_ready_8799 == 0 \
        && gemini_current_8799 == 0 && gemini_enabled_8799 == 0 )) || return 1
    else
      (( gemini_legacy_active == 0 && gemini_legacy_enabled == 0 )) || return 1
      if (( gemini_active_8795 == 1 )); then
        (( gemini_ready_8795 == 1 && gemini_current_8795 == 1 && gemini_enabled_8795 == 1 \
          && gemini_active_8799 == 0 && gemini_ready_8799 == 0 \
          && gemini_current_8799 == 0 && gemini_enabled_8799 == 0 )) || return 1
      else
        (( gemini_active_8799 == 1 && gemini_ready_8799 == 1 \
          && gemini_current_8799 == 1 && gemini_enabled_8799 == 1 \
          && gemini_active_8795 == 0 && gemini_ready_8795 == 0 \
          && gemini_current_8795 == 0 && gemini_enabled_8795 == 0 )) || return 1
      fi
    fi
  else
    (( gemini_legacy_active == 0 && gemini_legacy_enabled == 0 \
      && gemini_active_8795 == 0 && gemini_enabled_8795 == 0 \
      && gemini_active_8799 == 0 && gemini_enabled_8799 == 0 )) || return 1
  fi
  if (( kimi_supported == 1 )); then
    [[ $kimi_stable_status == 200 ]] || return 1
    if (( kimi_shared_supported == 0 )); then
      (( kimi_legacy_active == 1 && kimi_legacy_ready == 1 \
        && kimi_legacy_current == 1 && kimi_legacy_enabled == 1 \
        && kimi_active_8804 == 0 && kimi_ready_8804 == 0 \
        && kimi_current_8804 == 0 && kimi_enabled_8804 == 0 \
        && kimi_active_8805 == 0 && kimi_ready_8805 == 0 \
        && kimi_current_8805 == 0 && kimi_enabled_8805 == 0 )) || return 1
    else
      (( kimi_legacy_active == 0 && kimi_legacy_enabled == 0 )) || return 1
      if (( kimi_active_8804 == 1 )); then
        (( kimi_ready_8804 == 1 && kimi_current_8804 == 1 && kimi_enabled_8804 == 1 \
          && kimi_active_8805 == 0 && kimi_ready_8805 == 0 \
          && kimi_current_8805 == 0 && kimi_enabled_8805 == 0 )) || return 1
      else
        (( kimi_active_8805 == 1 && kimi_ready_8805 == 1 \
          && kimi_current_8805 == 1 && kimi_enabled_8805 == 1 \
          && kimi_active_8804 == 0 && kimi_ready_8804 == 0 \
          && kimi_current_8804 == 0 && kimi_enabled_8804 == 0 )) || return 1
      fi
    fi
  else
    (( kimi_legacy_active == 0 && kimi_legacy_enabled == 0 \
      && kimi_active_8804 == 0 && kimi_enabled_8804 == 0 \
      && kimi_active_8805 == 0 && kimi_enabled_8805 == 0 )) || return 1
  fi
  wd_engine_topology_is_steady \
    "$active_8787" "$ready_8787" "$current_8787" "$enabled_8787" \
    "$active_8788" "$ready_8788" "$current_8788" "$enabled_8788" \
    "$legacy_active" "$legacy_enabled"
}

router_active_backend_port() {
  local snippet=$CONTOUR_ROOTS_ROUTER_ACTIVE ports
  [[ -f $snippet && ! -L $snippet ]] || return 1
  [[ $(stat -c '%u' -- "$snippet" 2>/dev/null) == 0 ]] || return 1
  ports=$(awk -v host="$CONTOUR_NETWORK_LOOPBACK_HOST" '
    $1 == "reverse_proxy" {
      prefix = host ":"
      if (index($2, prefix) == 1) print substr($2, length(prefix) + 1)
    }
  ' "$snippet") || return 1
  [[ $ports != *$'\n'* ]] || return 1
  case "$ports" in "$ROUTER_PORT_A"|"$ROUTER_PORT_B") printf '%s\n' "$ports" ;; *) return 1 ;; esac
}

router_runtime_aligned() {
  local sha=$1 expected active_port port unit pid executable status
  local active_8800=0 ready_8800=0 current_8800=0 enabled_8800=0
  local active_8801=0 ready_8801=0 current_8801=0 enabled_8801=0
  local legacy_active=0 legacy_ready=0 legacy_enabled=0 stable_status
  expected="$ENGINE_RELEASE_ROOT/$sha"
  active_port=$(router_active_backend_port 2>/dev/null || true)
  for port in "$ROUTER_PORT_A" "$ROUTER_PORT_B"; do
    unit="${CONTOUR_UNITS_ROUTER_TEMPLATE/@.service/@$port.service}"
    local active=0 ready=0 selected=0 enabled=0
    systemctl is-active --quiet "$unit" && active=1
    systemctl is-enabled --quiet "$unit" && enabled=1
    status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
      "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$port/ready" 2>/dev/null || true)
    [[ $status == 200 ]] && ready=1
    if (( active == 1 )); then
      pid=$(systemctl show "$unit" -p MainPID --value)
      if [[ $pid =~ ^[1-9][0-9]*$ ]]; then
        executable=$(readlink -f -- "/proc/$pid/exe" 2>/dev/null || true)
        [[ $executable == "$expected/claude-router" ]] && selected=1
      fi
    fi
    if [[ $port == "$ROUTER_PORT_A" ]]; then
      active_8800=$active; ready_8800=$ready; current_8800=$selected; enabled_8800=$enabled
    else
      active_8801=$active; ready_8801=$ready; current_8801=$selected; enabled_8801=$enabled
    fi
  done
  systemctl is-active --quiet "$CONTOUR_UNITS_ROUTER_LEGACY" && legacy_active=1
  systemctl is-enabled --quiet "$CONTOUR_UNITS_ROUTER_LEGACY" && legacy_enabled=1
  status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$CONTOUR_PORTS_ROUTER_LEGACY/ready" 2>/dev/null || true)
  [[ $status == 200 ]] && legacy_ready=1
  stable_status=$(curl --noproxy '*' -sS -o /dev/null -w '%{http_code}' --max-time 2 \
    "$CONTOUR_ORIGINS_ROUTER_STABLE/ready" 2>/dev/null || true)
  ROUTER_RUNTIME_DETAIL="active_backend=${active_port:-invalid} stable=${stable_status:-unreachable} 8800=$active_8800:$ready_8800:$current_8800:$enabled_8800 8801=$active_8801:$ready_8801:$current_8801:$enabled_8801 legacy=$legacy_active:$legacy_ready:$legacy_enabled"
  [[ $stable_status == 200 && $legacy_active == 0 && $legacy_ready == 0 && $legacy_enabled == 0 ]] || return 1
  case "$active_port" in
    "$ROUTER_PORT_A")
      (( active_8800 == 1 && ready_8800 == 1 && current_8800 == 1 && enabled_8800 == 1 \
        && active_8801 == 0 && ready_8801 == 0 && current_8801 == 0 && enabled_8801 == 0 ))
      ;;
    "$ROUTER_PORT_B")
      (( active_8801 == 1 && ready_8801 == 1 && current_8801 == 1 && enabled_8801 == 1 \
        && active_8800 == 0 && ready_8800 == 0 && current_8800 == 0 && enabled_8800 == 0 ))
      ;;
    *) return 1 ;;
  esac
}

reconcile_engine_runtime() {
  local sha=$1 current expected engine_ok=0 router_ok=0
  expected="$ENGINE_RELEASE_ROOT/$sha"
  engine_runtime_aligned "$sha" && engine_ok=1
  router_runtime_aligned "$sha" && router_ok=1
  if (( engine_ok == 1 && router_ok == 1 )); then return 0; fi
  current=$(readlink -f -- "$ENGINE_RELEASE_ROOT/current")
  [[ $current == "$expected" ]] \
    || wd_die "refusing slot-only repair while engine release selection is wrong: engine=$ENGINE_RUNTIME_DETAIL router=$ROUTER_RUNTIME_DETAIL"
  CURRENT_PHASE=reconciling-engine
  if (( engine_ok == 0 )); then
    status "repairing engine single-slot runtime drift: $ENGINE_RUNTIME_DETAIL"
    wd_warn "engine runtime drift detected; converging through the health-gated controller: $ENGINE_RUNTIME_DETAIL"
    "$CONTROLLER_ROOT/engine-bluegreen.sh"
  fi
  if (( router_ok == 0 )); then
    status "repairing router single-slot runtime drift: $ROUTER_RUNTIME_DETAIL"
    wd_warn "router runtime drift detected; converging through the atomic Caddy controller: $ROUTER_RUNTIME_DETAIL"
    "$CONTROLLER_ROOT/router-bluegreen.sh"
  fi
  final_verify_engine "$sha"
  wd_log "engine and router runtime drift repaired; each has exactly one current active, ready, and enabled slot"
}

final_verify_backend() {
  local sha=$1 current worker_pid worker_cwd studio_pid studio_cwd expected_studio_cwd
  current=$(readlink -f -- "$COMMERCE_RELEASE_ROOT/current")
  [[ $current == "$COMMERCE_RELEASE_ROOT/$sha" ]] || wd_die "commerce current is not $sha after cutover"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$API_PORT_A/v1/ready" >/dev/null \
    || curl --noproxy '*' --fail --silent --show-error --max-time 5 "http://$CONTOUR_NETWORK_LOOPBACK_HOST:$API_PORT_B/v1/ready" >/dev/null \
    || wd_die "no commerce API slot is ready after cutover"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 \
    "$CONTOUR_ORIGINS_COMMERCE_STABLE/v1/ready" >/dev/null \
    || wd_die "stable commerce balancer is not ready after cutover"
  systemctl is-active --quiet "$CONTOUR_UNITS_WORKER" || wd_die "worker is not active after cutover"
  worker_pid=$(systemctl show "$CONTOUR_UNITS_WORKER" -p MainPID --value)
  [[ $worker_pid =~ ^[1-9][0-9]*$ ]] || wd_die "worker has no MainPID"
  worker_cwd=$(readlink -f -- "/proc/$worker_pid/cwd")
  [[ $worker_cwd == "$COMMERCE_RELEASE_ROOT/$sha/apps/worker" ]] \
    || wd_die "worker is not running immutable release $sha (cwd=$worker_cwd)"
  systemctl is-active --quiet "$CONTOUR_UNITS_CONTENT_STUDIO" || wd_die "content studio is not active after cutover"
  studio_pid=$(systemctl show "$CONTOUR_UNITS_CONTENT_STUDIO" -p MainPID --value)
  [[ $studio_pid =~ ^[1-9][0-9]*$ ]] || wd_die "content studio has no MainPID"
  studio_cwd=$(readlink -f -- "/proc/$studio_pid/cwd")
  expected_studio_cwd=$(wd_content_studio_runtime_directory "$COMMERCE_RELEASE_ROOT/$sha")
  [[ $studio_cwd == "$expected_studio_cwd" ]] \
    || wd_die "content studio is not running immutable release $sha (cwd=$studio_cwd)"
  curl --noproxy '*' --fail --silent --show-error --max-time 5 \
    "$CONTOUR_ORIGINS_CONTENT_STUDIO/api/health" >/dev/null \
    || wd_die "content studio health endpoint is not ready after cutover"
}

https_vhost_status() {
  local host=$1
  curl --noproxy '*' --insecure --silent --show-error --max-time 5 \
    --resolve "$host:443:127.0.0.1" -o /dev/null -w '%{http_code}' \
    "https://$host/" 2>/dev/null || true
}

require_admin_auth_vhost() {
  local host=$1 response headers status login_response login_headers login_status form_response form_body form_status
  response=$(curl --noproxy '*' --insecure --silent --show-error --max-time 5 \
    --resolve "$host:443:127.0.0.1" -H 'Accept: text/html' -D - -o /dev/null \
    -w $'\n%{http_code}' "https://$host/" 2>/dev/null || true)
  status=${response##*$'\n'}
  headers=${response%$'\n'*}
  [[ $status == 303 ]] \
    || wd_die "$host document navigation is not redirected by managed admin auth (HTTP ${status:-unreachable})"
  grep -Eiq '^location: /__admin-auth/login\?return_to=%2F[[:space:]]*$' <<<"$headers" \
    || wd_die "$host managed admin auth returned an unsafe or missing login location"
  ! grep -Eiq '^www-authenticate:' <<<"$headers" \
    || wd_die "$host managed session auth leaked a Basic browser challenge"
  login_response=$(curl --noproxy '*' --insecure --silent --show-error --max-time 5 \
    --resolve "$host:443:127.0.0.1" -H 'Accept: text/html' -D - -o /dev/null \
    -w $'\n%{http_code}' "https://$host/__admin-auth/login?return_to=%2F" 2>/dev/null || true)
  login_status=${login_response##*$'\n'}
  login_headers=${login_response%$'\n'*}
  [[ $login_status == 200 ]] \
    || wd_die "$host same-origin managed admin login is unavailable (HTTP ${login_status:-unreachable})"
  grep -Eiq '^referrer-policy: same-origin[[:space:]]*$' <<<"$login_headers" \
    || wd_die "$host managed admin login cannot supply the no-Origin CSRF fallback"

  # Some browsers omit Origin on a same-origin HTML form POST. Exercise the exact credential-free
  # fallback through public TLS: the login page's same-origin Referer must reach password validation,
  # while a cross-origin Referer must still fail before credentials are checked.
  form_response=$(curl --noproxy '*' --insecure --silent --show-error --max-time 5 \
    --resolve "$host:443:127.0.0.1" \
    -H "Referer: https://$host/__admin-auth/login?return_to=%2F" \
    -H 'Content-Type: application/x-www-form-urlencoded' \
    --data 'username=managed-auth-smoke-invalid&password=invalid&return_to=%2F' \
    -w $'\n%{http_code}' "https://$host/__admin-auth/login" 2>/dev/null || true)
  form_status=${form_response##*$'\n'}
  form_body=${form_response%$'\n'*}
  [[ $form_status == 401 ]] \
    || wd_die "$host no-Origin managed admin form is rejected before password validation (HTTP ${form_status:-unreachable})"
  grep -Fq 'Неверный логин или пароль' <<<"$form_body" \
    || wd_die "$host no-Origin managed admin form did not return the login error page"
  form_status=$(curl --noproxy '*' --insecure --silent --show-error --max-time 5 \
    --resolve "$host:443:127.0.0.1" \
    -H 'Referer: https://attacker.invalid/' \
    -H 'Content-Type: application/x-www-form-urlencoded' \
    --data 'username=managed-auth-smoke-invalid&password=invalid&return_to=%2F' \
    -o /dev/null -w '%{http_code}' "https://$host/__admin-auth/login" 2>/dev/null || true)
  [[ $form_status == 403 ]] \
    || wd_die "$host managed admin form accepts a cross-origin Referer (HTTP ${form_status:-unreachable})"
}

require_retired_vhost() {
  local host=$1 status
  status=$(https_vhost_status "$host")
  case "$status" in
    ''|000|404|421) ;;
    *) wd_die "retired hostname $host is still served (HTTP $status)" ;;
  esac
}

final_verify_admin_data() {
  local status matched=0 streak=0
  # The admin UI is the standalone Next.js app behind admin.apitoken.sale (own deploy lane); the
  # engine now serves only the data routes that app polls. Prove the deployed engine still exposes
  # that surface: /overview without the control key must be rejected by the engine's own auth
  # gate. The engine answers 401 with its JSON envelope, which the provider fallback (upstream
  # proxy) cannot produce, so this fails if the route is ever dropped from the router.
  # The stable listener round-robins both engine slots with a 2s active-health interval, so for
  # several seconds after a cutover the retiring slot can still answer. Require a short streak of
  # expected answers over a window comfortably longer than health convergence plus drain, so this
  # asserts the steady state rather than racing the cutover. The window must stay well above
  # Caddy's 2s active-health convergence: a one-second window quarantined a correct promotion on
  # 2026-07-25.
  for _ in $(seq 1 20); do
    status=$(curl --noproxy '*' --silent --show-error --max-time 5 \
      -o /dev/null -w '%{http_code}' \
      "$CONTOUR_ORIGINS_ANTHROPIC_STABLE/overview" 2>/dev/null || true)
    if [[ $status == 401 ]]; then
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
  [[ $matched == 1 ]] || wd_die "deployed engine does not serve the admin data routes (/overview must reject an unauthenticated probe with 401)"
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
  local response monitoring_ready=0 grafana_ready=0 prometheus_ready=0
  # Grafana and Prometheus answer their loopback health probes single-threaded and can drop a
  # 5-second curl while reloading configuration or rules; a single timed-out probe quarantined an
  # otherwise green master on 2026-08-07. Give both the same bounded retry window the target
  # aggregation below already uses instead of dying on the first flake.
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
    if (( grafana_ready == 0 )) && curl --noproxy '*' --fail --silent --show-error --max-time 5 \
      "$CONTOUR_ORIGINS_GRAFANA/api/health" >/dev/null 2>&1; then
      grafana_ready=1
    fi
    if (( prometheus_ready == 0 )) && curl --noproxy '*' --fail --silent --show-error --max-time 5 \
      "$CONTOUR_ORIGINS_PROMETHEUS/-/ready" >/dev/null 2>&1; then
      prometheus_ready=1
    fi
    if (( grafana_ready == 1 && prometheus_ready == 1 )); then
      break
    fi
    sleep 5
  done
  [[ $grafana_ready == 1 ]] || wd_die "Grafana is not healthy on its loopback listener"
  [[ $prometheus_ready == 1 ]] || wd_die "Prometheus is not ready on its loopback listener"
  # Wait across the same 2-minute `for:` window as MonitoringTargetDown / PublicEndpointDown /
  # BusinessCollectorStale. Engine blue-green plus a 60s collector timer can keep a scrape or
  # collector sample stale for more than the previous 60s retry; 6ef38441 and 289993c3 both
  # quarantined on this combined query after a GREEN engine admission. Exclude the same jobs as
  # the alert: an unprovisioned devbot has no listener, and RouterMetricsDown owns 8802.
  local up_ready=0 probe_ready=0 collector_ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24; do
    if (( up_ready == 0 )); then
      response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
        --data-urlencode 'query=min(up{job!~"claude-router|devbot"}) == 1' \
        "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
      if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
        >/dev/null 2>&1 <<<"$response"; then
        up_ready=1
      fi
    fi
    if (( probe_ready == 0 )); then
      response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
        --data-urlencode 'query=min(probe_success{job=~"public-http|openai-http|gemini-http|protected-http|support-http|loopback-http"}) == 1' \
        "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
      if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
        >/dev/null 2>&1 <<<"$response"; then
        probe_ready=1
      fi
    fi
    if (( collector_ready == 0 )); then
      response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
        --data-urlencode 'query=min(time() - apitoken_monitoring_collector_last_success_unixtime) < 180' \
        "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
      if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
        >/dev/null 2>&1 <<<"$response"; then
        collector_ready=1
      fi
    fi
    if (( up_ready == 1 && probe_ready == 1 && collector_ready == 1 )); then
      monitoring_ready=1
      break
    fi
    sleep 5
  done
  [[ $up_ready == 1 ]] \
    || wd_die "Prometheus scrape targets are not all up (MonitoringTargetDown job set)"
  [[ $probe_ready == 1 ]] || wd_die "HTTP synthetic probes are not all succeeding"
  [[ $collector_ready == 1 ]] || wd_die "business collector last success is older than 180s"
}

# Post-promotion smoke for the optional OpenAI-compatible surface.
#
# The Claude path is verified by /ready and the admin data check, but a Codex regression is invisible to
# both: the provider can be enabled while its app-server is dead, every home is unauthenticated, or
# the public routes have silently fallen back to the Anthropic path. None needs an API key to detect.
# Routable headroom is deliberately not a deployment invariant: subscription windows can exhaust
# without a code change, and `CodexNoAvailableHomes` owns that operational alert.
final_verify_codex_surface() {
  local response envelope enabled=0 determined=0 enabled_state='' attempt openai_host

  for attempt in 1 2 3 4 5 6; do
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode 'query=claude_api_codex_enabled{provider="openai"}' \
      "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
    enabled_state=$(jq --exit-status --raw-output \
      'select(.status == "success" and (.data.result | length) == 1)
       | .data.result[0].value[1]
       | select(. == "0" or . == "1")' <<<"$response" 2>/dev/null || true)
    case "$enabled_state" in
      0) determined=1; break ;;
      1) enabled=1; determined=1; break ;;
    esac
    (( attempt == 6 )) || sleep 5
  done
  (( determined == 1 )) \
    || wd_die "could not determine whether the Codex provider is enabled"

  if (( enabled == 1 )); then
    # A home may be cooling or outside configured headroom; that is capacity, not evidence of a bad
    # release. Enabled mode must nevertheless retain one authenticated live app-server.
    local provider_ready=0
    for attempt in 1 2 3 4 5 6; do
      response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
        --data-urlencode 'query=claude_api_codex_process_live{provider="openai"} == 1 and claude_api_codex_homes_authenticated{provider="openai"} >= 1' \
        "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
      if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
        >/dev/null 2>&1 <<<"$response"; then
        provider_ready=1
        break
      fi
      (( attempt == 6 )) || sleep 5
    done
    (( provider_ready == 1 )) \
      || wd_die "Codex provider is enabled but has no live authenticated app-server"
  fi

  # Prove the public OpenAI hostname is actually served by the Codex adapter rather than falling
  # through to the Anthropic path: only the adapter answers with an OpenAI-shaped error envelope
  # carrying `code`/`param`. Resolve the public hostname to loopback so this validates Caddy's
  # hostname boundary and provider-specific origin without depending on external DNS or hairpin routing.
  # The request omits the required model, so even a legacy loopback-trusted bridge rejects it before
  # reservation/provider execution. A fixed unauthenticated engine answers `invalid_api_key` first.
  openai_host=${CONTOUR_ORIGINS_PUBLIC_OPENAI#https://}
  envelope=$(curl --noproxy '*' --silent --show-error --max-time 5 \
    --resolve "$openai_host:443:$CONTOUR_NETWORK_LOOPBACK_HOST" \
    -H 'content-type: application/json' \
    -d '{}' \
    -w $'\n%{http_code}' "$CONTOUR_ORIGINS_PUBLIC_OPENAI/v1/responses" 2>/dev/null || true)
  envelope_status=${envelope##*$'\n'}
  envelope=${envelope%$'\n'*}
  if (( enabled == 1 )); then
    jq --exit-status '.error.type == "invalid_request_error"
        and $status == "401" and .error.code == "invalid_api_key"' \
      --arg status "$envelope_status" \
      >/dev/null 2>&1 <<<"$envelope" \
      || wd_die "/v1/responses did not answer with the enabled OpenAI-compatible envelope"
  else
    jq --exit-status '.error.type == "invalid_request_error"
        and $status == "404" and .error.code == "model_not_found"' \
      --arg status "$envelope_status" \
      >/dev/null 2>&1 <<<"$envelope" \
      || wd_die "disabled /v1/responses did not answer with the OpenAI-compatible envelope"
  fi

  wd_log "Codex OpenAI-compatible surface verified (enabled=$enabled)"
}

# Post-promotion smoke for the separate native Gemini service. It proves three independent facts:
# Prometheus scrapes the Gemini process and the public hostname reaches the native router. Enabled
# mode additionally requires one authenticated paid project; disabled mode is the provider-only
# kill switch and must keep a stable native 404 envelope instead of crashing the service.
final_verify_gemini_surface() {
  local response envelope enabled=0 determined=0 enabled_state='' attempt gemini_host
  for attempt in 1 2 3 4 5 6; do
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode 'query=claude_api_gemini_enabled{provider="gemini"}' \
      "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
    enabled_state=$(jq --exit-status --raw-output \
      'select(.status == "success" and (.data.result | length) == 1)
       | .data.result[0].value[1]
       | select(. == "0" or . == "1")' <<<"$response" 2>/dev/null || true)
    case "$enabled_state" in
      0) determined=1; break ;;
      1) enabled=1; determined=1; break ;;
    esac
    (( attempt == 6 )) || sleep 5
  done
  (( determined == 1 )) || wd_die "could not determine whether the Gemini provider is enabled"

  if (( enabled == 1 )); then
    # Seller-onboarding rollout: an enabled Gemini surface with zero authenticated paid projects is
    # a valid pre-onboarding state — it answers a native 401 until the first seller completes OAuth,
    # so the deploy must not fail closed on an empty roster. Record the count for observability; the
    # runtime GeminiNoAvailableProfiles alert covers a surface that stays empty after onboarding.
    local authenticated
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode 'query=claude_api_gemini_profiles_authenticated{provider="gemini"}' \
      "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
    authenticated=$(jq --raw-output \
      'select(.status == "success" and (.data.result | length) == 1) | .data.result[0].value[1]' \
      <<<"$response" 2>/dev/null || true)
    wd_log "Gemini enabled with ${authenticated:-0} authenticated paid project(s) (0 = pre-onboarding)"
  fi

  gemini_host=${CONTOUR_ORIGINS_PUBLIC_GEMINI#https://}
  envelope=$(curl --noproxy '*' --silent --show-error --max-time 5 \
    --resolve "$gemini_host:443:$CONTOUR_NETWORK_LOOPBACK_HOST" \
    -H 'content-type: application/json' -d '{}' \
    "$CONTOUR_ORIGINS_PUBLIC_GEMINI/v1beta/models/gemini-provider-probe:generateContent" \
    2>/dev/null || true)
  if (( enabled == 1 )); then
    jq --exit-status '.error.status == "INVALID_ARGUMENT" and .error.code == 400
      and (.error.details // [] | any(.reason? == "API_KEY_INVALID"))' \
      >/dev/null 2>&1 <<<"$envelope" \
      || wd_die "enabled public Gemini hostname did not answer with the native provider envelope"
  else
    jq --exit-status '.error.status == "NOT_FOUND" and .error.code == 404' \
      >/dev/null 2>&1 <<<"$envelope" \
      || wd_die "disabled public Gemini hostname did not answer with the native provider envelope"
  fi
  wd_log "native Gemini subscription-pool surface verified (enabled=$enabled)"
}

# Post-promotion smoke for the backend-only KIMI plane. There is no public hostname to probe, so
# it proves two independent facts instead: Prometheus scrapes the kimi provider target on the
# stable loopback origin, and that origin answers with the engine's own bounded Anthropic-shaped
# envelope rather than Caddy's plain-text no-upstream 503. Default-off is the expected steady
# state; a deliberately enabled plane must additionally keep one live profile, matching the
# plane's own readiness contract.
final_verify_kimi_surface() {
  local response envelope enabled=0 determined=0 enabled_state='' attempt
  for attempt in 1 2 3 4 5 6; do
    response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
      --data-urlencode 'query=claude_api_kimi_enabled{provider="kimi"}' \
      "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
    enabled_state=$(jq --exit-status --raw-output \
      'select(.status == "success" and (.data.result | length) == 1)
       | .data.result[0].value[1]
       | select(. == "0" or . == "1")' <<<"$response" 2>/dev/null || true)
    case "$enabled_state" in
      0) determined=1; break ;;
      1) enabled=1; determined=1; break ;;
    esac
    (( attempt == 6 )) || sleep 5
  done
  (( determined == 1 )) || wd_die "could not determine whether the KIMI provider is enabled"

  if (( enabled == 1 )); then
    # An enabled KIMI plane whose gateway lost every live profile takes its own slots out of
    # readiness, so the runtime check would already have failed. Assert the same contract here:
    # deliberate enablement must keep at least one live profile.
    local provider_ready=0
    for attempt in 1 2 3 4 5 6; do
      response=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 --get \
        --data-urlencode 'query=claude_api_kimi_live_profiles{provider="kimi"} >= 1' \
        "$CONTOUR_ORIGINS_PROMETHEUS/api/v1/query" 2>/dev/null || true)
      if jq --exit-status '.status == "success" and (.data.result | length) > 0' \
        >/dev/null 2>&1 <<<"$response"; then
        provider_ready=1
        break
      fi
      (( attempt == 6 )) || sleep 5
    done
    (( provider_ready == 1 )) \
      || wd_die "KIMI provider is enabled but has no live profile"
  fi

  envelope=$(curl --noproxy '*' --silent --show-error --max-time 5 \
    -H 'content-type: application/json' -d '{}' \
    "$CONTOUR_ORIGINS_KIMI_STABLE/v1/messages" 2>/dev/null || true)
  jq --exit-status '.error.type == "authentication_error"' \
    >/dev/null 2>&1 <<<"$envelope" \
    || wd_die "stable KIMI origin did not answer with the bounded engine envelope"
  wd_log "backend-only KIMI subscription-pool surface verified (enabled=$enabled)"
}

run_final_verification_lane() {
  # Verification workers are read-only and independent. The parent joins every selected check and
  # owns quarantine/overall status so one fast failure never abandons another in-flight probe.
  trap - ERR EXIT INT TERM
  "$@"
}

run_final_verification_plan() {
  local verification_plan=$1 engine_sha=$2
  local panel_pid='' routing_pid='' monitoring_pid='' codex_pid='' gemini_pid='' kimi_pid=''
  local panel_rc=0 routing_rc=0 monitoring_rc=0 codex_rc=0 gemini_rc=0 kimi_rc=0

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
    run_final_verification_lane final_verify_admin_data &
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
  if wd_verification_plan_has "$verification_plan" gemini; then
    run_final_verification_lane final_verify_gemini_surface &
    gemini_pid=$!
  fi
  if wd_verification_plan_has "$verification_plan" kimi; then
    run_final_verification_lane final_verify_kimi_surface &
    kimi_pid=$!
  fi

  if [[ -n $panel_pid ]]; then wait "$panel_pid" || panel_rc=$?; fi
  if [[ -n $routing_pid ]]; then wait "$routing_pid" || routing_rc=$?; fi
  if [[ -n $monitoring_pid ]]; then wait "$monitoring_pid" || monitoring_rc=$?; fi
  if [[ -n $codex_pid ]]; then wait "$codex_pid" || codex_rc=$?; fi
  if [[ -n $gemini_pid ]]; then wait "$gemini_pid" || gemini_rc=$?; fi
  if [[ -n $kimi_pid ]]; then wait "$kimi_pid" || kimi_rc=$?; fi
  (( panel_rc == 0 && routing_rc == 0 && monitoring_rc == 0 && codex_rc == 0 && gemini_rc == 0 \
      && kimi_rc == 0 )) \
    || wd_die "final verification lanes failed (panel=$panel_rc routing=$routing_rc monitoring=$monitoring_rc codex=$codex_rc gemini=$gemini_rc kimi=$kimi_rc)"
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
  local previous_sha
  previous_sha=$(basename -- "$(readlink -f -- "$ENGINE_RELEASE_ROOT/previous")")
  attempt_rollback engine --engine-bluegreen engine-bluegreen.sh "$ENGINE_RELEASE_ROOT/previous" \
    || return 1
  if ! "$CONTROLLER_ROOT/router-bluegreen.sh"; then
    wd_warn "engine providers rolled back, but router rollback cutover failed"
    return 1
  fi
  wd_atomic_write "$ENGINE_FILE" "$previous_sha"
  ENGINE_SHA=$previous_sha
}

rollback_backend() {
  local previous_sha
  previous_sha=$(basename -- "$(readlink -f -- "$COMMERCE_RELEASE_ROOT/previous")")
  attempt_rollback backend --api-only api-bluegreen.sh "$COMMERCE_RELEASE_ROOT/previous" \
    || return 1
  wd_atomic_write "$BACKEND_FILE" "$previous_sha"
  BACKEND_SHA=$previous_sha
}

deploy_engine() {
  local sha=$1 codex_changed=$2
  local controller_rc=0 proof_mode proof_owner controller_identity
  local deploy_args=(--engine-bluegreen --tested-candidate "$(candidate_for "$sha")")
  CURRENT_PHASE=deploying-engine
  CURRENT_PHASE_BEFORE_FAILURE=deploying-engine
  status "promoting and blue-green deploying the tested engine"
  github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_ENGINE" "Engine blue-green deployment in progress"
  github_deployment_start engine "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_ENGINE" \
    "$CONTOUR_ORIGINS_PUBLIC_ENGINE/health"
  "$CONTROLLER_ROOT/deploy.sh" "${deploy_args[@]}" "$sha"
  "$CONTROLLER_ROOT/engine-bluegreen.sh" || controller_rc=$?
  if (( controller_rc != 0 )); then
    rollback_engine || true
    wd_die "engine provider controller failed (exit $controller_rc)"
  fi
  rm -f -- "$ROUTER_SUCCESS_PROOF"
  if "$CONTROLLER_ROOT/router-bluegreen.sh"; then
    controller_rc=0
  else
    controller_rc=$?
  fi
  if (( controller_rc != 0 )) && [[ -f $ROUTER_SUCCESS_PROOF && ! -L $ROUTER_SUCCESS_PROOF ]] \
      && [[ $(<"$ROUTER_SUCCESS_PROOF") == "$sha" ]]; then
    proof_mode=$(stat -c '%a' -- "$ROUTER_SUCCESS_PROOF" 2>/dev/null || true)
    proof_owner=$(stat -c '%u:%g' -- "$ROUTER_SUCCESS_PROOF" 2>/dev/null || true)
    controller_identity=$(id -u):$(id -g)
    if [[ $proof_mode == 600 && $proof_owner == "$controller_identity" \
        && $(stat -c '%u:%g:%a' -- "${ROUTER_SUCCESS_PROOF%/*}" 2>/dev/null) \
          == "$controller_identity:700" ]]; then
      wd_warn "router controller returned $controller_rc after publishing exact success proof; accepting proof"
      controller_rc=0
    fi
  fi
  rm -f -- "$ROUTER_SUCCESS_PROOF"
  if (( controller_rc != 0 )); then
    rollback_engine || true
    wd_die "$(wd_payload_canary_reason "$sha" \
      || printf 'unified router blue-green controller failed (exit %s)' "$controller_rc")"
  fi
  # HTTP admission must never queue behind a stateful daemon roll. First prove that the committed
  # target is the sole ready/enabled gateway and can use the currently serving authenticated
  # cohort, then converge a changed pin after traffic already has a healthy availability anchor.
  if ! engine_runtime_aligned "$sha" serving || ! router_runtime_aligned "$sha"; then
    rollback_engine || true
    wd_die "engine/router runtime has no admitted single-slot serving state after cutover: engine=$ENGINE_RUNTIME_DETAIL router=$ROUTER_RUNTIME_DETAIL"
  fi
  if ! engine_runtime_aligned "$sha" converged || ! router_runtime_aligned "$sha"; then
    rollback_engine || true
    wd_die "engine/router runtime is not converged after cutover: engine=$ENGINE_RUNTIME_DETAIL router=$ROUTER_RUNTIME_DETAIL"
  fi
  wd_atomic_write "$ENGINE_FILE" "$sha"
  github_deployment_success engine
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_ENGINE" "Engine verified in production"
  wd_log "engine $sha passed final production verification"
}

deploy_backend() {
  local sha=$1
  CURRENT_PHASE=deploying-backend
  CURRENT_PHASE_BEFORE_FAILURE=deploying-backend
  status "promoting and blue-green deploying tested API, worker, and Content Studio artifacts"
  github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_BACKEND" "Backend blue-green deployment in progress"
  github_deployment_start backend "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_BACKEND" \
    "$CONTOUR_ORIGINS_PUBLIC_BACKEND/v1/ready"
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
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_BACKEND" "Backend and worker verified in production"
  wd_log "backend and Content Studio $sha passed final production verification"
}

deploy_sales() {
  local sha=$1
  CURRENT_PHASE=deploying-sales
  CURRENT_PHASE_BEFORE_FAILURE=deploying-sales
  status "promoting and health-gating the sales partner portal (own release lifecycle)"
  github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_SALES" "Sales partner portal deployment in progress"
  github_deployment_start sales "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_SALES" \
    "$CONTOUR_ORIGINS_PUBLIC_SALES/v1/health"
  "$SALES_RUNNER" "$sha"
  wd_atomic_write "$SALES_FILE" "$sha"
  github_deployment_success sales
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_SALES" "Sales partner portal verified in production"
  wd_log "sales $sha promoted and verified (partners.apitoken.sale)"
}

deploy_openkeys() {
  local sha=$1
  CURRENT_PHASE=deploying-openkeys
  CURRENT_PHASE_BEFORE_FAILURE=deploying-openkeys
  status "promoting and health-gating the OpenKeys portal (own release lifecycle)"
  github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_OPENKEYS" "OpenKeys portal deployment in progress"
  github_deployment_start openkeys "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_OPENKEYS" \
    "$CONTOUR_ORIGINS_PUBLIC_OPENKEYS/"
  "$OPENKEYS_RUNNER" "$sha"
  wd_atomic_write "$OPENKEYS_FILE" "$sha"
  github_deployment_success openkeys
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_OPENKEYS" "OpenKeys portal verified in production"
  wd_log "openkeys $sha promoted and verified (openkeys.apitoken.sale)"
}

deploy_admin() {
  local sha=$1
  CURRENT_PHASE=deploying-admin
  CURRENT_PHASE_BEFORE_FAILURE=deploying-admin
  status "promoting and health-gating the admin panel (own release lifecycle)"
  github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_ADMIN" "Admin panel deployment in progress"
  github_deployment_start admin "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_ADMIN" \
    "$CONTOUR_ORIGINS_PUBLIC_ADMIN/"
  "$ADMIN_RUNNER" "$sha"
  wd_atomic_write "$ADMIN_FILE" "$sha"
  github_deployment_success admin
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_ADMIN" "Admin panel verified in production"
  wd_log "admin $sha promoted and verified (admin.apitoken.sale)"
}

deploy_devbot() {
  local sha=$1
  CURRENT_PHASE=deploying-devbot
  CURRENT_PHASE_BEFORE_FAILURE=deploying-devbot
  status "promoting and health-gating the dev notification bot (own release lifecycle)"
  github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_DEVBOT" "Devbot deployment in progress"
  if [[ ! -f $DEVBOT_ENV_FILE ]]; then
    # Disabled until the operator provisions secrets: keep the pipeline green, but deliberately
    # do NOT advance devbot.sha. The lane therefore keeps triggering on later cycles and rolls
    # the first release as soon as /etc/apitoken/devbot.env exists. devbot-deploy.sh re-checks
    # the same condition as root, so a standalone invocation skips identically.
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_DEVBOT" \
      "Devbot disabled until $DEVBOT_ENV_FILE is provisioned"
    wd_log "devbot disabled: $DEVBOT_ENV_FILE missing — skipping (devbot.sha not advanced)"
    return 0
  fi
  local candidate
  candidate=$(candidate_for "$sha")
  if [[ ! -f $candidate/apps/devbot/dist/main.js ]]; then
    # A candidate without the TypeScript lane (deploy/observability/engine-only diff) carries no
    # built devbot. By classifier construction its devbot code is identical to the running
    # release, so there is nothing to roll. Do NOT advance devbot.sha here: while the baseline
    # is missing the lane would otherwise keep quarantining every TypeScript-less master after
    # provisioning (devbot-deploy.sh dies on the missing dist). Deferring retries on the next
    # TypeScript-bearing master, which is guaranteed to carry a built devbot.
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_DEVBOT" "No built devbot in this candidate; rollout deferred"
    wd_log "candidate $sha carries no built devbot (TypeScript lane not selected) — rollout deferred (devbot.sha not advanced)"
    return 0
  fi
  github_deployment_start devbot "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_DEVBOT" \
    "$CONTOUR_ORIGINS_DEVBOT/health"
  "$DEVBOT_RUNNER" "$sha"
  wd_atomic_write "$DEVBOT_FILE" "$sha"
  github_deployment_success devbot
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_DEVBOT" "Devbot verified in production"
  wd_log "devbot $sha promoted and verified (127.0.0.1:3800)"
}

deploy_core_components() {
  local sha=$1 engine_changed=$2 backend_changed=$3 codex_changed=$4
  # The engine and commerce controllers deliberately share apitoken-deploy.lock. Keep their
  # cutovers ordered inside one lane while sales/OpenKeys/admin use their independent roots and units.
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
    github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_MIGRATION" "Backup and automatic migration in progress"
    github_deployment_start database "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENT_DATABASE" ""
    wd_log "candidate $sha contains new migration history; applying it before any application cutover"
    sudo -n "$MIGRATION_RUNNER" "$sha"
    [[ $(wd_manifest_digest "$DB_MANIFEST") == "$digest" ]] \
      || wd_die "automatic migration returned without committing the tested manifest"
    github_deployment_success database
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_MIGRATION" "Tested migration applied before application rollout"
  else
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_MIGRATION" "No commerce migration changes"
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
  local infra_changed=0 engine_changed=0 backend_changed=0 sales_changed=0
  local openkeys_changed=0 admin_changed=0 devbot_changed=0 codex_changed=0
  local gpt_image_2_live_gate=0 gpt_image_2_public_smoke_gate=0
  local gpt_image_2_public_preflight_gate=0 gpt_image_2_public_preflight_v2_gate=0
  local gpt_image_2_public_preflight_v3_gate=0 gpt_image_2_public_paid_smoke_gate=0
  local gpt_image_2_public_paid_smoke_v2_gate=0 gpt_image_2_public_paid_smoke_v3_gate=0
  local gpt_image_2_surface_probe_gate=0
  local gpt_image_2_public_paid_inspect_gate=0
  local typescript_required=0 typescript_full=0 typescript_base= rust_required=0 static_required=0
  local engine_artifacts_required=0 codex_artifacts_required=0
  local validation_policy_sha256='' validation_plan_sha256='' final_verification_plan=''
  local pricing_retirement_postdrop_stage=none
  local public_image_summary='' public_image_preflight_summary='' public_image_preflight_v2_summary=''
  local public_image_preflight_v3_summary='' public_image_paid_summary=''
  local public_image_generation_status='' public_image_edit_status='' public_image_paid_inspect_summary=''
  local core_pid= sales_pid= openkeys_pid= admin_pid= devbot_pid= core_rc=0 sales_rc=0 openkeys_rc=0 admin_rc=0 devbot_rc=0

  [[ $(id -un) == "$CONTOUR_IDENTITY_RUNTIME_USER" ]] \
    || wd_die "watchdog service must run as $CONTOUR_IDENTITY_RUNTIME_USER"
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
  require_fixed_root_executable "$PRICING_RETIREMENT_POSTDROP"
  require_fixed_file "$INFRASTRUCTURE_RUNNER"
  require_fixed_file "$RETENTION_HELPER"
  require_fixed_file "$SALES_RUNNER"
  require_fixed_file "$OPENKEYS_RUNNER"
  require_fixed_file "$ADMIN_RUNNER"
  require_fixed_file "$DEVBOT_RUNNER"
  require_fixed_file "$VALIDATION_PLANNER"
  require_fixed_root_executable "$AUTHBOT_RUNTIME_STATE"
  require_fixed_file "$GPT_IMAGE_2_LIVE_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_SMOKE_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V2_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V3_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_PAID_SMOKE_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V2_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V3_GATE"
  require_fixed_file "$GPT_IMAGE_2_PUBLIC_PAID_INSPECT_GATE"
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
  [[ -z $ADMIN_SHA ]] || wd_require_ancestor "$SOURCE_REPO" "$ADMIN_SHA" "$CANDIDATE_SHA" admin
  [[ -z $DEVBOT_SHA ]] || wd_require_ancestor "$SOURCE_REPO" "$DEVBOT_SHA" "$CANDIDATE_SHA" devbot

  if rejected=$(wd_read_sha "$REJECTED_FILE" 2>/dev/null) && [[ $rejected == "$CANDIDATE_SHA" ]]; then
    CURRENT_PHASE=quarantined
    status "failed candidate remains blocked; run: sudo apitoken-watchdog retry $CANDIDATE_SHA"
    wd_log "candidate $CANDIDATE_SHA is quarantined; waiting for a newer commit or explicit retry"
    exit 0
  fi

  pricing_retirement_postdrop_stage=$(wd_pricing_retirement_postdrop_stage \
    "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA") \
    || wd_die "pricing-retirement contraction range must add at most one exact migration path"

  infra_scope=$(wd_infrastructure_install_scope \
    "$SOURCE_REPO" "$INFRASTRUCTURE_SHA" "$CANDIDATE_SHA")
  wd_infrastructure_scope_is_valid "$infra_scope" \
    || wd_die "invalid infrastructure install scope: $infra_scope"
  [[ $infra_scope == none ]] || infra_changed=1
  # A systemd or full installer deliberately hands off to a fresh invocation after recording its
  # infrastructure SHA. Preserve the delivery-wide scope from the last completed candidate so the
  # next invocation still runs the final checks required by the infrastructure just changed.
  delivery_infra_scope=$(wd_infrastructure_install_scope \
    "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA")
  wd_infrastructure_scope_is_valid "$delivery_infra_scope" \
    || wd_die "invalid delivery infrastructure scope: $delivery_infra_scope"
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_live_gate_trigger && gpt_image_2_live_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_smoke_gate_trigger && gpt_image_2_public_smoke_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_preflight_gate_trigger && gpt_image_2_public_preflight_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_preflight_v2_gate_trigger && gpt_image_2_public_preflight_v2_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_preflight_v3_gate_trigger && gpt_image_2_public_preflight_v3_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_paid_smoke_gate_trigger && gpt_image_2_public_paid_smoke_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_paid_smoke_v2_gate_trigger && gpt_image_2_public_paid_smoke_v2_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_paid_smoke_v3_gate_trigger && gpt_image_2_public_paid_smoke_v3_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_public_paid_inspect_gate_trigger && gpt_image_2_public_paid_inspect_gate=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" \
    wd_path_is_gpt_image_2_surface_probe_gate_trigger && gpt_image_2_surface_probe_gate=1
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
  # Admin panel держит собственную релизную базу. Как и у OpenKeys: пока admin.sha не
  # существует, первый запуск деплоим безусловно — иначе контекст никогда не получит
  # свой первый релиз.
  if [[ -z ${ADMIN_SHA:-} ]]; then
    admin_changed=1
  else
    wd_range_has_class "$SOURCE_REPO" "$ADMIN_SHA" "$CANDIDATE_SHA" wd_path_is_admin \
      && admin_changed=1
  fi
  # Devbot держит собственную релизную базу по той же схеме. Пока devbot.sha не существует,
  # lane срабатывает безусловно; до provisioning /etc/apitoken/devbot.env deploy_devbot
  # завершается зелёным skip'ом и базу НЕ двигает, поэтому после появления секретов первый
  # релиз раскатывается автоматически на ближайшем цикле.
  if [[ -z ${DEVBOT_SHA:-} ]]; then
    devbot_changed=1
  else
    wd_range_has_class "$SOURCE_REPO" "$DEVBOT_SHA" "$CANDIDATE_SHA" wd_path_is_devbot \
      && devbot_changed=1
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

  if [[ $PROCESSED_SHA != "$CANDIDATE_SHA" ]]; then
    CURRENT_PHASE=admitting
    status "checking host-owned promotion or hotfix admission"
    if ! admission_out=$(sudo -n /usr/local/lib/apitoken-watchdog/promotion-admission.sh "$CANDIDATE_SHA" 2>&1); then
      wd_die "${admission_out:-promotion-admission rejected unattested master SHA $CANDIDATE_SHA}"
    fi
    [[ -z $admission_out ]] || wd_log "$admission_out"
  fi

  if [[ $PROCESSED_SHA == "$CANDIDATE_SHA" && $infra_changed == 0 \
        && $engine_changed == 0 && $backend_changed == 0 \
        && $sales_changed == 0 && $openkeys_changed == 0 && $admin_changed == 0 \
        && $devbot_changed == 0 && $codex_changed == 0 && $gpt_image_2_live_gate == 0 \
        && $gpt_image_2_public_smoke_gate == 0 \
        && $gpt_image_2_public_preflight_gate == 0 ]]; then
    if idle_maintenance_due; then
      CURRENT_PHASE=maintaining
      status "running periodic retention and production-alignment checks"
      prune_expired_candidates
      prune_expired_releases_best_effort
      prune_expired_dumps
      final_verification_plan=$(wd_final_verification_plan full 0 0 0 0 0) \
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
  prune_expired_releases_best_effort
  prune_expired_dumps
  wd_prune_failure_cycle_logs
  wd_start_cycle_transcript "$CANDIDATE_SHA"

  publish_pipeline_start_statuses

  prepare_and_test_candidate "$CANDIDATE_SHA" "$typescript_required" "$typescript_full" \
    "$typescript_base" "$rust_required" "$static_required" "$engine_artifacts_required" \
    "$validation_policy_sha256" "$validation_plan_sha256" "$codex_artifacts_required"
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_TESTS" "Selected isolated validation lanes passed"
  rm -f -- "$REJECTED_FILE"

  if (( infra_changed == 1 )); then
    CURRENT_PHASE=installing-infrastructure
    CURRENT_PHASE_BEFORE_FAILURE=installing-infrastructure
    status "installing exact tested operational definitions ($infra_scope)"
    github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" "Installing exact tested operational definitions ($infra_scope)"
    # The fixed root bridge independently derives and verifies this exact scope before installing
    # it, so the candidate controller cannot omit a changed concern.
    sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA"
    [[ $(wd_read_sha "$INFRASTRUCTURE_FILE") == "$CANDIDATE_SHA" ]] \
      || wd_die "infrastructure installer did not record the exact installed candidate"
    if [[ $infra_scope == full ]] \
        || wd_infrastructure_scope_has "$infra_scope" systemd; then
      # A systemd transaction may change this service's sandbox. The current process still has the
      # old namespace, so only a manager-spawned next cycle may consume the new privileges.
      CURRENT_PHASE=handoff
      status "system definitions installed; next five-second poll starts the updated service"
      github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" "System definitions installed; continuing on next poll"
      wd_log "system infrastructure transaction installed; deferring to a fresh systemd invocation"
      exit 0
    elif wd_infrastructure_scope_has "$infra_scope" controller; then
      CURRENT_PHASE=handoff
      status "operational definitions installed; continuing immediately with the new controller"
      github_status pending "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" "New controller installed; continuing immediately"
      wd_log "exact tested controller installed; transferring the held lock to the new controller"
      require_fixed_file "$CONTROLLER_ENTRYPOINT"
      exec "$CONTROLLER_ENTRYPOINT" --resume "$CANDIDATE_SHA"
    else
      wd_log "exact tested $infra_scope definitions installed; continuing the same deployment cycle"
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
  if (( admin_changed == 0 )) && [[ -z ${ADMIN_SHA:-} ]]; then
    # First run before admin.sha exists: adopt the current commit as the baseline.
    wd_atomic_write "$ADMIN_FILE" "$CANDIDATE_SHA"
  fi
  if (( devbot_changed == 0 )) && [[ -z ${DEVBOT_SHA:-} ]]; then
    # First run before devbot.sha exists: adopt the current commit as the baseline.
    wd_atomic_write "$DEVBOT_FILE" "$CANDIDATE_SHA"
  fi
  publish_unchanged_component_statuses \
    "$engine_changed" "$backend_changed" "$sales_changed" "$openkeys_changed" "$admin_changed" \
    "$devbot_changed"

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
  if (( admin_changed == 1 )); then
    run_rollout_lane deploy_admin "$CANDIDATE_SHA" &
    admin_pid=$!
  fi
  if (( devbot_changed == 1 )); then
    run_rollout_lane deploy_devbot "$CANDIDATE_SHA" &
    devbot_pid=$!
  fi

  # Always join every started lane before quarantine/final verification. A failed lane owns its
  # component-specific failure status; this parent owns the single overall verdict.
  if [[ -n $core_pid ]]; then wait "$core_pid" || core_rc=$?; fi
  if [[ -n $sales_pid ]]; then wait "$sales_pid" || sales_rc=$?; fi
  if [[ -n $openkeys_pid ]]; then wait "$openkeys_pid" || openkeys_rc=$?; fi
  if [[ -n $admin_pid ]]; then wait "$admin_pid" || admin_rc=$?; fi
  if [[ -n $devbot_pid ]]; then wait "$devbot_pid" || devbot_rc=$?; fi
  if (( core_rc != 0 || sales_rc != 0 || openkeys_rc != 0 || admin_rc != 0 || devbot_rc != 0 )); then
    wd_die "component rollout lanes failed (core=$core_rc sales=$sales_rc openkeys=$openkeys_rc admin=$admin_rc devbot=$devbot_rc)"
  fi
  if (( engine_changed == 1 )); then ENGINE_SHA=$CANDIDATE_SHA; fi

  CURRENT_PHASE=verifying
  CURRENT_PHASE_BEFORE_FAILURE=verifying
  # The final plan's last flag only drives the monitoring re-probe shared by every application
  # lane; a devbot rollout needs that same check, so it is folded into the admin argument.
  final_verification_plan=$(wd_final_verification_plan "$delivery_infra_scope" "$engine_changed" \
    "$backend_changed" "$sales_changed" "$openkeys_changed" \
    "$(( admin_changed || devbot_changed ))") \
    || wd_die "could not derive the final production verification plan"
  status "running selected final production verification ($final_verification_plan)"
  if ! ( run_final_verification_plan "$final_verification_plan" "$ENGINE_SHA" ); then
    (( engine_changed == 0 )) || rollback_engine || true
    (( backend_changed == 0 )) || rollback_backend || true
    wd_die "selected final production verification failed after component admission"
  fi

  if [[ $pricing_retirement_postdrop_stage != none ]]; then
    # Both contractions are forward-only. This proof runs after the selected applications are
    # serving and deliberately has no rollback branch: a failure quarantines the SHA and requires
    # a forward fix against the already-contracted schema.
    CURRENT_PHASE=verifying-pricing-retirement-postdrop
    CURRENT_PHASE_BEFORE_FAILURE=verifying-pricing-retirement-postdrop
    status "verifying $pricing_retirement_postdrop_stage pricing-schema contraction in production"
    sudo -n "$PRICING_RETIREMENT_POSTDROP" --stage \
      "$pricing_retirement_postdrop_stage" "$CANDIDATE_SHA"
  fi

  if (( gpt_image_2_live_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2
    status "running bounded GPT Image 2 edit through the sealed Codex OAuth pool"
    # This is evidence for one already deployed implementation, not whichever unrelated engine
    # release happens to be current when the one-shot controller lands.
    sudo -n "$GPT_IMAGE_2_LIVE_GATE" "$GPT_IMAGE_2_IMPLEMENTATION_SHA"
  fi

  if (( gpt_image_2_public_smoke_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2-public
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2-public
    status "inspecting the permanently fenced GPT Image 2 public attempt without network access"
    public_image_summary=
    if ! public_image_summary=$(sudo -n "$GPT_IMAGE_2_PUBLIC_SMOKE_GATE" \
        "$GPT_IMAGE_2_PUBLIC_PRODUCER_SHA" --inspect); then
      [[ $public_image_summary =~ ^gpt-image-public:[a-z_]{1,64}:g=(true|false):e=(true|false)$ ]] \
        && CURRENT_PHASE_BEFORE_FAILURE=$public_image_summary
      false
    fi
  fi

  if (( gpt_image_2_public_preflight_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2-public-preflight
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2-public-preflight
    status "inspecting the fenced GPT Image 2 public preflight without network access"
    public_image_preflight_summary=$(sudo -n "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_GATE" \
      "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_PRODUCER_SHA" --inspect)
    [[ $public_image_preflight_summary =~ ^gpt-image-preflight:[a-z_]{1,64}:g=false:e=false$ ]] \
      || wd_die "GPT Image 2 public preflight inspector returned an invalid summary"
    CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_summary
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PUBLIC_PREFLIGHT" \
      "$public_image_preflight_summary"
  fi

  if (( gpt_image_2_public_preflight_v2_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2-public-preflight-v2
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2-public-preflight-v2
    status "running free GPT Image 2 public preflight v2 without image dispatch"
    public_image_preflight_v2_summary=
    if ! public_image_preflight_v2_summary=$(sudo -n "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V2_GATE" \
        "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V2_PRODUCER_SHA"); then
      [[ $public_image_preflight_v2_summary =~ \
          ^gpt-image-preflight-v2:[a-z_]{1,64}:g=false:e=false$ ]] \
        && CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_v2_summary
      false
    fi
    [[ $public_image_preflight_v2_summary == \
        gpt-image-preflight-v2:preflight_success:g=false:e=false ]] \
      || wd_die "GPT Image 2 public preflight v2 returned an invalid summary"
    CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_v2_summary
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PUBLIC_PREFLIGHT_V2" \
      "$public_image_preflight_v2_summary"
  fi

  if (( gpt_image_2_public_preflight_v3_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2-public-preflight-v3
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2-public-preflight-v3
    status "running free GPT Image 2 public preflight v3 without image dispatch"
    public_image_preflight_v3_summary=
    if ! public_image_preflight_v3_summary=$(sudo -n "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V3_GATE" \
        "$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V3_PRODUCER_SHA"); then
      [[ $public_image_preflight_v3_summary =~ \
          ^gpt-image-preflight-v3:[a-z_]{1,64}:g=false:e=false$ ]] \
        && CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_v3_summary
      false
    fi
    [[ $public_image_preflight_v3_summary == \
        gpt-image-preflight-v3:preflight_success:g=false:e=false ]] \
      || wd_die "GPT Image 2 public preflight v3 returned an invalid summary"
    CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_v3_summary
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PUBLIC_PREFLIGHT_V3" \
      "$public_image_preflight_v3_summary"
  fi

  if (( gpt_image_2_public_paid_smoke_gate == 1 )); then
    wd_die "retired GPT Image 2 paid smoke root cannot be dispatched again"
  fi

  if (( gpt_image_2_public_paid_smoke_v2_gate == 1 )); then
    wd_die "retired GPT Image 2 paid smoke v2 root cannot be dispatched again"
  fi

  if (( gpt_image_2_public_paid_smoke_v3_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2-public-paid-v3
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2-public-paid-v3
    status "running fresh GPT Image 2 generation and edit through the sealed pool"
    public_image_paid_summary=
    if ! public_image_paid_summary=$(sudo -n "$GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V3_GATE" \
        "$GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V3_PRODUCER_SHA"); then
      [[ $public_image_paid_summary =~ ^gpt-image-paid:[a-z_]{1,64}:g=(true|false):e=(true|false)$ ]] \
        && CURRENT_PHASE_BEFORE_FAILURE=$public_image_paid_summary
      false
    fi
    jq -e '
      def operation($image_input_required):
        (keys | sort) == ([
          "charge_nano", "height", "image_input_tokens", "image_output_tokens", "png_sha256",
          "real_nano", "text_input_tokens", "width"
        ] | sort) and
        (.width | type == "number" and floor == . and . >= 1 and . <= 3840) and
        (.height | type == "number" and floor == . and . >= 1 and . <= 3840) and
        (.png_sha256 | type == "string" and test("^sha256:[0-9a-f]{64}$")) and
        ([.text_input_tokens, .image_input_tokens, .image_output_tokens, .real_nano,
          .charge_nano] | all(.[]; type == "number" and floor == . and . >= 0)) and
        .image_output_tokens > 0 and .real_nano > 0 and .charge_nano == 0 and
        (if $image_input_required then .image_input_tokens > 0 else .image_input_tokens == 0 end);
      (keys | sort) == (["edit", "generation", "state"] | sort) and
      .state == "green" and (.generation | operation(false)) and (.edit | operation(true)) and
      .generation.png_sha256 != .edit.png_sha256
    ' <<<"$public_image_paid_summary" >/dev/null \
      || wd_die "GPT Image 2 public paid smoke v3 returned invalid evidence"
    public_image_generation_status=$(jq -jr '
      .generation | "\(.width)x\(.height) \(.png_sha256) real=\(.real_nano) charge=\(.charge_nano)"
    ' <<<"$public_image_paid_summary")
    public_image_edit_status=$(jq -jr '
      .edit | "\(.width)x\(.height) \(.png_sha256) image_in=\(.image_input_tokens) real=\(.real_nano) charge=\(.charge_nano)"
    ' <<<"$public_image_paid_summary")
    (( ${#public_image_generation_status} <= 140 && ${#public_image_edit_status} <= 140 )) \
      || wd_die "GPT Image 2 public paid smoke v3 status exceeds the GitHub bound"
    CURRENT_PHASE_BEFORE_FAILURE=gpt-image-paid-v3:success:g=true:e=true
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PUBLIC_GENERATION" \
      "$public_image_generation_status"
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PUBLIC_EDIT" \
      "$public_image_edit_status"
  fi

  if (( gpt_image_2_public_paid_inspect_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2-public-paid-inspect
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2-public-paid-inspect
    status "inspecting the fenced GPT Image 2 generation without network access"
    public_image_paid_inspect_summary=$(sudo -n "$GPT_IMAGE_2_PUBLIC_PAID_INSPECT_GATE" \
      "$GPT_IMAGE_2_PUBLIC_PAID_INSPECT_PRODUCER_SHA" --inspect)
    jq -e '
      (keys | sort) == ([
        "edit_dispatched", "generation", "generation_dispatched", "state"
      ] | sort) and
      .state == "generation_received" and .generation_dispatched == true and
      .edit_dispatched == false and
      (.generation | keys | sort) == (["height", "png_bytes", "png_sha256", "width"] | sort) and
      (.generation.width | type == "number" and floor == . and . > 0) and
      (.generation.height | type == "number" and floor == . and . > 0) and
      (.generation.png_bytes | type == "number" and floor == . and . > 0 and . <= 16777216) and
      (.generation.png_sha256 | type == "string" and test("^sha256:[0-9a-f]{64}$"))
    ' <<<"$public_image_paid_inspect_summary" >/dev/null \
      || wd_die "GPT Image 2 public paid inspector returned invalid evidence"
    public_image_generation_status=$(jq -jr '
      .generation | "\(.width)x\(.height) \(.png_sha256) bytes=\(.png_bytes); edit_not_dispatched"
    ' <<<"$public_image_paid_inspect_summary")
    (( ${#public_image_generation_status} <= 140 )) \
      || wd_die "GPT Image 2 inspected generation description exceeds the GitHub status bound"
    CURRENT_PHASE_BEFORE_FAILURE=gpt-image-paid:generation_received:g=true:e=false
    github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PUBLIC_PAID_INSPECT" \
      "$public_image_generation_status"
  fi

  if (( gpt_image_2_surface_probe_gate == 1 )); then
    CURRENT_PHASE=verifying-gpt-image-2-surface-probe
    CURRENT_PHASE_BEFORE_FAILURE=verifying-gpt-image-2-surface-probe
    status "probing GPT Image 2 quality tiers and multi-reference edits on the native wire"
    surface_probe_summary=$(sudo -n "$GPT_IMAGE_2_SURFACE_PROBE_GATE" \
      "$GPT_IMAGE_2_SURFACE_PROBE_PRODUCER_SHA")
    jq -e '
      def probe:
        (keys | sort) == (["image_input_tokens", "output_tokens", "returned_quality", "verdict"] | sort) and
        (.verdict | IN("honored", "normalized", "rejected")) and
        ((.returned_quality == null) or (.returned_quality | IN("auto", "low", "medium", "high"))) and
        (.output_tokens | type == "number" and floor == . and . >= 0 and . <= 100000) and
        (.image_input_tokens | type == "number" and floor == . and . >= 0 and . <= 100000000) and
        (if .verdict == "honored" then .output_tokens > 0
         elif .verdict == "rejected" then .returned_quality == null and .output_tokens == 0
         else .returned_quality != null end);
      (keys | sort) == (["high", "medium", "multi-ref"] | sort) and
      (.medium | probe) and (.high | probe) and (.["multi-ref"] | probe)
    ' <<<"$surface_probe_summary" >/dev/null \
      || wd_die "GPT Image 2 surface probe returned invalid evidence"
    for surface_probe_name in medium high multi-ref; do
      surface_probe_status=$(jq -jr --arg name "$surface_probe_name" '
        .[$name] | "\($name): \(.verdict)" +
          (if .returned_quality == null then "" else " returned=\(.returned_quality)" end) +
          " out=\(.output_tokens)" +
          (if .image_input_tokens > 0 then " in=\(.image_input_tokens)" else "" end)
      ' <<<"$surface_probe_summary")
      (( ${#surface_probe_status} <= 140 )) \
        || wd_die "GPT Image 2 surface probe status exceeds the GitHub bound"
      case "$surface_probe_name" in
        medium) surface_probe_context=$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PROBE_MEDIUM ;;
        high) surface_probe_context=$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PROBE_HIGH ;;
        multi-ref) surface_probe_context=$CONTOUR_GITHUB_STATUS_CONTEXT_GPT_IMAGE_PROBE_MULTI_REF ;;
        *) wd_die "unknown GPT Image 2 surface probe context: $surface_probe_name" ;;
      esac
      github_status success "$surface_probe_context" "$surface_probe_status"
    done
  fi

  wd_atomic_write "$PROCESSED_FILE" "$CANDIDATE_SHA"
  if [[ ! -f $STATE_ROOT/staging-admission.enabled ]]; then
    printf '%s\n' "$CANDIDATE_SHA" >"$STATE_ROOT/staging-admission.enabled"
    chmod 0644 "$STATE_ROOT/staging-admission.enabled"
  fi
  wd_discard_cycle_log || true
  rm -f -- "$PENDING_MIGRATION_FILE"
  CURRENT_PHASE=idle
  status "candidate tested and all selected components verified in production"
  github_status success "$CONTOUR_GITHUB_STATUS_CONTEXT_WATCHDOG" "All selected production components verified"
  wd_log "watchdog completed $CANDIDATE_SHA (engine=$engine_changed codex=$codex_changed backend=$backend_changed sales=$sales_changed openkeys=$openkeys_changed admin=$admin_changed devbot=$devbot_changed)"
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
