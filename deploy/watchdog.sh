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
CI_TOOLCHAIN=/opt/apitoken-watchdog/rust-toolchain
CONTROLLER_ROOT=/usr/local/lib/apitoken-watchdog/controller
TEST_DB_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-test-db
BACKUP_RUNNER=/usr/local/lib/apitoken-watchdog/watchdog-backup.sh
MIGRATION_RUNNER=/usr/local/lib/apitoken-watchdog/watchdog-migrate.sh
INFRASTRUCTURE_RUNNER=/usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh
RETENTION_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-retention.sh
SALES_RUNNER=/usr/local/lib/apitoken-watchdog/controller/sales-deploy.sh
GITHUB_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-github
WATCHDOG_LOCK=/run/lock/apitoken-watchdog.lock
DEPLOY_LOCK=/run/lock/apitoken-deploy.lock
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
COMMERCE_RELEASE_ROOT=/opt/apitoken/releases

PROCESSED_FILE=$STATE_ROOT/processed.sha
ENGINE_FILE=$STATE_ROOT/engine.sha
BACKEND_FILE=$STATE_ROOT/backend.sha
SALES_FILE=$STATE_ROOT/sales.sha
REJECTED_FILE=$STATE_ROOT/rejected.sha
PENDING_MIGRATION_FILE=$STATE_ROOT/pending-migration.sha
DB_MANIFEST=$STATE_ROOT/database-migrations.manifest
STATUS_FILE=$STATE_ROOT/status

CURRENT_PHASE=starting
TEST_DB_STARTED=0
ACTIVE_DEPLOYMENT_ID=
ACTIVE_DEPLOYMENT_ENV=
ACTIVE_DEPLOYMENT_URL=

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

status() {
  local detail=$1
  # World-readable: the monitoring collector runs with an empty CapabilityBoundingSet, so it has no
  # CAP_DAC_OVERRIDE and cannot read a 0640 file even as root. The status line contains only a
  # phase, a public commit SHA, a fixed detail string, and a timestamp — no secret.
  wd_atomic_write "$STATUS_FILE" "phase=$CURRENT_PHASE sha=${CANDIDATE_SHA:-none} detail=$detail updated_at=$(date -u +%FT%TZ)" 0644
}

fail() {
  local rc=$? line=${BASH_LINENO[0]:-unknown}
  trap - ERR EXIT INT TERM
  if (( TEST_DB_STARTED == 1 )); then
    sudo -n "$TEST_DB_HELPER" stop >/dev/null 2>&1 || true
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
    if [[ -n $ACTIVE_DEPLOYMENT_ID ]]; then
      sudo -n "$GITHUB_HELPER" deployment-status "$ACTIVE_DEPLOYMENT_ID" failure \
        "deployment failed closed at $CURRENT_PHASE" "$ACTIVE_DEPLOYMENT_ENV" "$ACTIVE_DEPLOYMENT_URL" \
        >/dev/null 2>&1 || true
    fi
    case ${CURRENT_PHASE_BEFORE_FAILURE:-$CURRENT_PHASE} in
      testing) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/tests "Validation failed; production unchanged" >/dev/null 2>&1 || true ;;
      migrating) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/migration "Migration failed; application deploy blocked" >/dev/null 2>&1 || true ;;
      deploying-engine) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/engine "Engine rollout failed closed" >/dev/null 2>&1 || true ;;
      deploying-backend) sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/backend "Backend rollout failed closed" >/dev/null 2>&1 || true ;;
    esac
    sudo -n "$GITHUB_HELPER" commit-status "$CANDIDATE_SHA" failure deploy/watchdog \
      "Production pipeline failed closed; inspect watchdog logs" >/dev/null 2>&1 || true
  fi
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
    if sudo -n rm -rf --one-file-system -- "$candidate"; then
      marker=$(marker_for "$sha")
      sudo -n rm -f -- "$marker" \
        || wd_warn "removed candidate $sha but could not remove its test marker"
      pruned=$((pruned + 1))
    else
      wd_warn "failed to remove expired candidate $sha"
      failed=$((failed + 1))
    fi
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
  for sha in "${ENGINE_SHA:-}" "${BACKEND_SHA:-}" "${PROCESSED_SHA:-}" "${SALES_SHA:-}"; do
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
    PATH="$CI_TOOLCHAIN/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    "$@"
}

prepare_and_test_candidate() {
  local sha=$1 candidate marker dsn engine_dsn sales_dsn manifest digest tree
  candidate=$(candidate_for "$sha")
  marker=$(marker_for "$sha")

  if candidate_is_tested "$sha"; then
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

  wd_log "running frozen install and complete build for $sha as isolated user $CI_USER"
  run_as_ci pnpm --dir "$candidate" install --frozen-lockfile
  run_as_ci pnpm --dir "$candidate" build
  run_as_ci pnpm --dir "$candidate" typecheck

  dsn=$(sudo -n "$TEST_DB_HELPER" start)
  TEST_DB_STARTED=1
  wd_log "running commerce migrations against disposable PostgreSQL"
  run_as_ci env DATABASE_URL="$dsn" node "$candidate/packages/db/dist/migrate.js"
  sales_dsn=$(sudo -n "$TEST_DB_HELPER" sales-dsn)
  wd_log "running sales migrations against a separate disposable PostgreSQL database"
  run_as_ci env SALES_DATABASE_URL="$sales_dsn" node "$candidate/packages/sales-db/dist/migrate.js"
  wd_log "running all TypeScript tests against their migrated disposable databases"
  run_as_ci env TEST_DATABASE_URL="$dsn" TEST_SALES_DATABASE_URL="$sales_dsn" pnpm --dir "$candidate" \
    -r --workspace-concurrency=1 --if-present test

  engine_dsn=$(sudo -n "$TEST_DB_HELPER" engine-dsn)
  wd_log "running all Rust workspace tests against separate disposable PostgreSQL"
  run_as_ci env CLAUDE_API_TEST_DATABASE_URL="$engine_dsn" \
    cargo test --locked --workspace --manifest-path "$candidate/Cargo.toml"

  wd_log "checking tracked whitespace and shell syntax"
  if [[ -n ${PROCESSED_SHA:-} && $PROCESSED_SHA != "$sha" ]]; then
    git -C "$SOURCE_REPO" diff --check "$PROCESSED_SHA..$sha"
  fi
  while IFS= read -r -d '' shell_file; do
    bash -n "$shell_file"
  done < <(find "$candidate/deploy" -type f -name '*.sh' -print0)
  run_as_ci bash "$candidate/deploy/watchdog-lib.test.sh"
  run_as_ci bash "$candidate/deploy/monitoring-config.test.sh"

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
  local sha=$1 current worker_pid worker_cwd studio_pid studio_cwd
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
  [[ $studio_cwd == "$COMMERCE_RELEASE_ROOT/$sha/apps/content-studio" ]] \
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
  local panel matched=0 response monitoring_ready=0
  # A just-stopped old slot can finish one keep-alive response while Caddy converges. Retry through
  # one active-health window before declaring that the stable listener serves the wrong panel.
  for _ in 1 2 3 4 5 6; do
    panel=$(curl --noproxy '*' --fail --silent --show-error --max-time 5 \
      http://127.0.0.1:8790/admin-panel 2>/dev/null || true)
    if grep -Fq 'data-admin-panel-version="6"' <<<"$panel"; then
      matched=1
      break
    fi
    sleep 1
  done
  [[ $matched == 1 ]] || wd_die "deployed engine does not contain the current admin panel"
  require_admin_auth_vhost admin.apitoken.sale
  require_admin_auth_vhost admin.partners.apitoken.sale
  require_admin_auth_vhost crm.apitoken.sale
  require_admin_auth_vhost content-studio.apitoken.sale
  require_admin_auth_vhost monitoring.apitoken.sale
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
  require_retired_vhost panel.apitoken.sale
  require_retired_vhost partners.panel.apitoken.sale
  require_retired_vhost crm.panel.apitoken.sale
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
  local sha=$1
  CURRENT_PHASE=deploying-engine
  CURRENT_PHASE_BEFORE_FAILURE=deploying-engine
  status "backing up PostgreSQL before building and blue-green deploying engine"
  github_status pending deploy/engine "Engine blue-green deployment in progress"
  github_deployment_start engine production-engine https://api.apitoken.sale/health
  sudo -n "$BACKUP_RUNNER" "$sha"
  "$CONTROLLER_ROOT/deploy.sh" --engine-bluegreen "$sha"
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
  status "building and blue-green deploying API, worker, and Content Studio; migrations explicitly skipped"
  github_status pending deploy/backend "Backend blue-green deployment in progress"
  github_deployment_start backend production-backend https://backend.apitoken.sale/v1/ready
  "$CONTROLLER_ROOT/deploy.sh" --api-only --skip-migrate "$sha"
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

main() {
  local remote_ref rejected infra_changed=0 caddy_changed=0 engine_changed=0 backend_changed=0 sales_changed=0

  [[ $(id -un) == deploy ]] || wd_die "watchdog service must run as deploy"
  [[ -d $SOURCE_REPO/.git ]] || wd_die "source repository is missing: $SOURCE_REPO"
  [[ -d $STATE_ROOT && ! -L $STATE_ROOT ]] || wd_die "watchdog state is not installed"
  [[ -d $CANDIDATE_ROOT && ! -L $CANDIDATE_ROOT ]] \
    || wd_die "candidate root is not a regular directory: $CANDIDATE_ROOT"
  require_fixed_file "$WATCHDOG_LOCK"
  require_fixed_file "$DEPLOY_LOCK"
  require_fixed_directory "$CONTROLLER_ROOT"
  require_fixed_file "$TEST_DB_HELPER"
  require_fixed_file "$BACKUP_RUNNER"
  require_fixed_file "$MIGRATION_RUNNER"
  require_fixed_file "$INFRASTRUCTURE_RUNNER"
  require_fixed_file "$RETENTION_HELPER"
  require_fixed_file "$SALES_RUNNER"
  require_fixed_file "$GITHUB_HELPER"
  require_fixed_directory "$CI_TOOLCHAIN"
  [[ -f $DB_MANIFEST && ! -L $DB_MANIFEST ]] || wd_die "database migration baseline is missing"

  exec 7<>"$WATCHDOG_LOCK"
  if ! flock -n 7; then
    wd_log "another watchdog cycle is still running"
    exit 0
  fi

  # Candidate trees are build/test workspaces, not production releases. The exclusive lock makes
  # every directory idle here, so expired read-only trees can be removed without racing a test,
  # migration, infrastructure install, or deployment. A quarantined SHA can still be retried after
  # expiry; candidate_is_tested will rebuild it from the source repository.
  prune_expired_candidates

  CURRENT_PHASE=fetching
  status "fetching $REMOTE/$BRANCH"
  # GitHub reachability is not a property of the candidate. Absorb transient DNS/TLS/network
  # failures here so they never reach the failure path at all.
  wd_retry 3 5 git -C "$SOURCE_REPO" fetch --no-tags "$REMOTE" \
    "+refs/heads/$BRANCH:refs/remotes/$REMOTE/$BRANCH"
  remote_ref="refs/remotes/$REMOTE/$BRANCH"
  CANDIDATE_SHA=$(git -C "$SOURCE_REPO" rev-parse "$remote_ref^{commit}")
  wd_validate_sha "$CANDIDATE_SHA"

  PROCESSED_SHA=$(wd_read_sha "$PROCESSED_FILE")
  ENGINE_SHA=$(wd_read_sha "$ENGINE_FILE")
  BACKEND_SHA=$(wd_read_sha "$BACKEND_FILE")
  SALES_SHA=$(wd_read_sha "$SALES_FILE" 2>/dev/null || printf '')
  wd_require_ancestor "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" processed
  wd_require_ancestor "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" engine
  wd_require_ancestor "$SOURCE_REPO" "$BACKEND_SHA" "$CANDIDATE_SHA" backend
  [[ -z $SALES_SHA ]] || wd_require_ancestor "$SOURCE_REPO" "$SALES_SHA" "$CANDIDATE_SHA" sales

  if rejected=$(wd_read_sha "$REJECTED_FILE" 2>/dev/null) && [[ $rejected == "$CANDIDATE_SHA" ]]; then
    CURRENT_PHASE=quarantined
    status "failed candidate remains blocked; run: sudo apitoken-watchdog retry $CANDIDATE_SHA"
    wd_log "candidate $CANDIDATE_SHA is quarantined; waiting for a newer commit or explicit retry"
    exit 0
  fi

  # Releases and per-deployment dumps are pruned here, after the recorded component SHAs are loaded
  # (they are protected inputs) and while the exclusive watchdog lock guarantees no deploy, rollback,
  # or migration is in flight. Anything backing a live process is protected independently of counts.
  CURRENT_PHASE=pruning
  status "applying immutable release and pre-deploy dump retention"
  prune_expired_releases "$ENGINE_RELEASE_ROOT" engine
  prune_expired_releases "$COMMERCE_RELEASE_ROOT" commerce
  prune_expired_dumps

  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" wd_path_is_infrastructure \
    && infra_changed=1
  wd_range_has_class "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" wd_path_is_caddy \
    && caddy_changed=1
  wd_range_has_class "$SOURCE_REPO" "$ENGINE_SHA" "$CANDIDATE_SHA" wd_path_is_engine \
    && engine_changed=1
  wd_range_has_class "$SOURCE_REPO" "$BACKEND_SHA" "$CANDIDATE_SHA" wd_path_is_backend \
    && backend_changed=1
  # Sales has its own release baseline; fall back to processed on first run (before sales.sha exists).
  wd_range_has_class "$SOURCE_REPO" "${SALES_SHA:-$PROCESSED_SHA}" "$CANDIDATE_SHA" wd_path_is_sales \
    && sales_changed=1

  if [[ $PROCESSED_SHA == "$CANDIDATE_SHA" && $engine_changed == 0 && $backend_changed == 0 && $sales_changed == 0 ]]; then
    reconcile_engine_runtime "$ENGINE_SHA"
    final_verify_admin_panel
    CURRENT_PHASE=idle
    status "master already processed; production runtime aligned"
    wd_log "master $CANDIDATE_SHA is already processed and production is aligned"
    exit 0
  fi

  github_status pending deploy/watchdog "Production pipeline started"
  github_status pending deploy/tests "Complete isolated validation in progress"

  prepare_and_test_candidate "$CANDIDATE_SHA"
  github_status success deploy/tests "Complete isolated validation passed"
  rm -f -- "$REJECTED_FILE"

  if (( infra_changed == 1 )); then
    CURRENT_PHASE=installing-infrastructure
    status "installing exact tested operational definitions"
    github_status pending deploy/watchdog "Installing exact tested operational definitions"
    if (( caddy_changed == 1 )); then
      sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA" --apply-caddy
    else
      sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA"
    fi
    wd_log "exact tested operational definitions installed automatically"
  fi

  if (( backend_changed == 1 )); then
    apply_migrations_before_deploy "$CANDIDATE_SHA"
  else
    github_status success deploy/migration "No commerce migration changes"
  fi

  if (( engine_changed == 1 )); then
    deploy_engine "$CANDIDATE_SHA"
    ENGINE_SHA=$CANDIDATE_SHA
  else
    github_status success deploy/engine "No engine changes"
  fi
  if (( backend_changed == 1 )); then
    deploy_backend "$CANDIDATE_SHA"
  else
    github_status success deploy/backend "No backend changes"
  fi

  if (( sales_changed == 1 )); then
    deploy_sales "$CANDIDATE_SHA"
  else
    github_status success deploy/sales "No sales changes"
    # First run before sales.sha exists: adopt the current commit as the sales baseline.
    if [[ -z ${SALES_SHA:-} ]]; then wd_atomic_write "$SALES_FILE" "$CANDIDATE_SHA"; fi
  fi

  reconcile_engine_runtime "$ENGINE_SHA"
  final_verify_admin_panel

  wd_atomic_write "$PROCESSED_FILE" "$CANDIDATE_SHA"
  rm -f -- "$PENDING_MIGRATION_FILE"
  CURRENT_PHASE=idle
  status "candidate tested and all selected components verified in production"
  github_status success deploy/watchdog "All selected production components verified"
  wd_log "watchdog completed $CANDIDATE_SHA (engine=$engine_changed backend=$backend_changed sales=$sales_changed)"
}

main "$@"
trap - ERR EXIT INT TERM
