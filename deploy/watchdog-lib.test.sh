#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --assert-inherited-flock ]]; then
  [[ -e /proc/$$/fd/5 ]] || { printf 'lock descriptor was not inherited\n' >&2; exit 1; }
  flock -n 5 || { printf 'inherited descriptor no longer owns its lock\n' >&2; exit 1; }
  exit 0
fi

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

# Candidate diagnostics must identify the failing lane while never copying a DSN or credential to
# GitHub's public commit/deployment status descriptions.
diagnostic_log="$TEMP/candidate.log"
printf '%s\n' \
  '[watchdog] ERROR: TypeScript candidate lane failed (exit 1) postgresql://user:pass@db/openkeys ENGINE_CONTROL_KEY=super-secret' \
  >"$diagnostic_log"
diagnostic=$(wd_validation_failure_summary "$diagnostic_log" 1 testing)
[[ $diagnostic == *'phase=testing'* && $diagnostic == *'TypeScript candidate lane failed'* ]] \
  || wd_die "candidate diagnostic lost its phase or lane: $diagnostic"
[[ $diagnostic != *'user:pass'* && $diagnostic != *'super-secret'* \
    && $diagnostic == *'URL_REDACTED'* && $diagnostic == *'REDACTED'* ]] \
  || wd_die "candidate diagnostic leaked a secret or skipped redaction: $diagnostic"
empty_diagnostic=$(wd_validation_failure_summary "$TEMP/missing.log" 7 shadow-validation)
[[ $empty_diagnostic == 'phase=shadow-validation; validator exited with code 7' ]] \
  || wd_die "candidate diagnostic fallback is unstable: $empty_diagnostic"

# A restrictive fetch umask must not strand the shared source history from the isolated reader.
# Extract the production functions so this exercises the same normalization and CI read check that
# the watchdog uses, while the test itself runs as the isolated account in the trusted host gate.
permission_repair_body=$(sed -n '/^repair_source_repo_permissions()/,/^}/p' \
  "$ROOT/deploy/watchdog.sh")
permission_check_body=$(sed -n '/^source_repo_readability_check()/,/^}/p' \
  "$ROOT/deploy/watchdog.sh")
[[ -n $permission_repair_body && -n $permission_check_body ]] \
  || wd_die 'source repository permission helpers are missing'
eval "$permission_repair_body"
eval "$permission_check_body"
run_as_ci() {
  if [[ ${FORCE_CI_ERROR:-0} == 1 ]]; then
    printf 'synthetic unreadable Git object\n' >&2
    return 0
  fi
  # The test account owns the temporary checkout, so emulate the production reader boundary before
  # repair: an isolated user must reject any object-store path lacking group/other access.
  if [[ ${1:-} == git && -n ${SOURCE_REPO:-} ]]; then
    local blocked
    blocked=$(find "$SOURCE_REPO/.git/objects" \
      \( -type d ! -perm -0055 -o -type f ! -perm -0044 \) -print -quit)
    [[ -z $blocked ]] || return 1
  fi
  "$@"
}
CI_USER=apitoken-ci
permission_remote="$TEMP/source-permission-remote.git"
permission_seed="$TEMP/source-permission-seed"
permission_checkout="$TEMP/source-permission-checkout"
git -c init.defaultBranch=master init --bare --quiet "$permission_remote"
git -c init.defaultBranch=master init --quiet "$permission_seed"
git -C "$permission_seed" config user.name permission-test
git -C "$permission_seed" config user.email permission-test@example.invalid
git -C "$permission_seed" commit --quiet --allow-empty -m initial
git -C "$permission_seed" push --quiet "$permission_remote" HEAD:refs/heads/master
git clone --quiet --no-local "$permission_remote" "$permission_checkout"
git -C "$permission_seed" commit --quiet --allow-empty -m fetched
git -C "$permission_seed" push --quiet "$permission_remote" HEAD:refs/heads/master
(umask 077; git -C "$permission_checkout" fetch --quiet "$permission_remote" master)
permission_sha=$(git -C "$permission_checkout" rev-parse FETCH_HEAD)

# Make the old failure deterministic even when a Git build chooses a packed representation for the
# fetch. This is still a real fetch under umask 077; the explicit mode fence models its observed
# loose-object result and keeps the regression portable across Git storage strategies.
find "$permission_checkout/.git/objects" -type d -exec chmod go-rx {} +
find "$permission_checkout/.git/objects" -type f -exec chmod go-r {} +
permission_unreadable=$(find "$permission_checkout/.git/objects" \
  \( -type d ! -perm -0055 -o -type f ! -perm -0044 \) -print -quit)
[[ -n $permission_unreadable ]] || wd_die 'restrictive fetch test did not create an unreadable object store'
SOURCE_REPO="$permission_checkout"
if run_as_ci git -c safe.directory="$permission_checkout" -C "$permission_checkout" \
    cat-file --batch-all-objects --batch-check='%(objectname) %(objecttype)' >/dev/null 2>&1; then
  wd_die 'isolated reader accepted an unreadable object store before repair'
fi
source_repo_readability_check
git -c safe.directory="$permission_checkout" -C "$permission_checkout" \
  cat-file -e "$permission_sha^{commit}"
permission_unreadable=$(find "$permission_checkout/.git/objects" \
  \( -type d ! -perm -0055 -o -type f ! -perm -0044 \) -print -quit)
[[ -z $permission_unreadable ]] || wd_die 'source object repair left unreadable Git paths'
if ( FORCE_CI_ERROR=1 source_repo_readability_check >"$TEMP/source-permission-check.stdout" \
    2>"$TEMP/source-permission-check.stderr" ); then
  wd_die 'source reader stderr was accepted as a clean readability check'
fi
grep -Fq 'reported read errors' "$TEMP/source-permission-check.stderr" \
  || wd_die 'source reader stderr did not fail closed'

# Controller-only handoff uses exec, so the new process must retain the same open-file-description
# lock rather than reacquiring by pathname. Exercise that Linux contract when flock/procfs exist.
if command -v flock >/dev/null 2>&1 && [[ -d /proc/$$/fd ]]; then
  inherited_lock="$TEMP/inherited.lock"
  exec 5<>"$inherited_lock"
  flock -n 5
  bash "$0" --assert-inherited-flock
  flock -u 5
  exec 5>&-
fi

# Final production verification is derived from the surfaces that can actually have changed.
# Component-specific controllers already gate their own release, while full infrastructure keeps
# every cross-component smoke. The order is canonical so the controller can dispatch it safely.
final_plan_cases=(
  'none 0 0 0 0 0|none'
  'controller 0 0 0 0 0|none'
  'none 1 0 0 0 0|runtime,panel,monitoring,codex,gemini'
  'none 0 1 0 0 0|monitoring'
  'none 0 0 1 0 0|monitoring'
  'none 0 0 0 1 0|monitoring'
  'none 0 0 0 0 1|monitoring'
  'caddy 0 0 0 0 0|routing,monitoring,codex,gemini'
  'monitoring 0 0 0 0 0|monitoring'
  'controller+caddy+monitoring 0 0 0 0 0|routing,monitoring,codex,gemini'
  'systemd 0 0 0 0 0|runtime,panel,routing,monitoring,codex,gemini'
  'controller+systemd 0 0 0 0 0|runtime,panel,routing,monitoring,codex,gemini'
  'full 0 0 0 0 0|runtime,panel,routing,monitoring,codex,gemini'
  'full 1 1 1 1 1|runtime,panel,routing,monitoring,codex,gemini'
)
for final_plan_case in "${final_plan_cases[@]}"; do
  final_plan_args=${final_plan_case%%|*}
  final_plan_expected=${final_plan_case#*|}
  # shellcheck disable=SC2086 # The table intentionally supplies six positional scalar fields.
  final_plan_actual=$(wd_final_verification_plan $final_plan_args)
  [[ $final_plan_actual == "$final_plan_expected" ]] \
    || wd_die "final verification plan mismatch for $final_plan_args: $final_plan_actual"
done
if wd_final_verification_plan unknown 0 0 0 0 0 >/dev/null 2>&1 \
    || wd_final_verification_plan caddy+controller 0 0 0 0 0 >/dev/null 2>&1 \
    || wd_final_verification_plan none 2 0 0 0 0 >/dev/null 2>&1 \
    || wd_final_verification_plan none 0 0 0 0 2 >/dev/null 2>&1; then
  wd_die "invalid final verification inputs did not fail closed"
fi
for valid_scope in none controller caddy systemd monitoring \
  controller+caddy+systemd+monitoring full; do
  wd_infrastructure_scope_is_valid "$valid_scope" \
    || wd_die "valid infrastructure scope was rejected: $valid_scope"
done
for invalid_scope in '' caddy+controller controller+controller systemd+unknown; do
  if wd_infrastructure_scope_is_valid "$invalid_scope"; then
    wd_die "invalid infrastructure scope was accepted: $invalid_scope"
  fi
done
wd_infrastructure_scope_has controller+caddy+monitoring monitoring \
  || wd_die "composite infrastructure scope lost a member"
if wd_infrastructure_scope_has controller+caddy monitor; then
  wd_die "composite infrastructure scope accepted a partial member"
fi
wd_verification_plan_has runtime,panel,monitoring,codex,gemini monitoring \
  || wd_die "final verification plan membership lost an exact entry"
if wd_verification_plan_has runtime,panel,monitoring,codex,gemini monitor; then
  wd_die "final verification plan membership accepted a partial entry"
fi

# A transient GitHub 5xx while publishing the pipeline start is infrastructure noise, not a
# candidate verdict. Exercise the real github_status wrapper with a fake bridge that fails twice;
# replacing sleep keeps the retry contract deterministic and fast.
# shellcheck disable=SC2091
eval "$(sed -n '/^github_status()/,/^github_deployment_start()/p' "$ROOT/deploy/watchdog.sh" \
  | sed '$d')"
status_retry_log="$TEMP/status-retry.log"
(
  sleep() { :; }
  sudo() {
    printf '%s\n' "$*" >>"$status_retry_log"
    (( $(wc -l <"$status_retry_log") >= 3 ))
  }
  GITHUB_HELPER=/fixed/watchdog-github
  CANDIDATE_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  github_status pending deploy/watchdog "Production pipeline started"
)
(( $(wc -l <"$status_retry_log") == 3 )) \
  || wd_die "commit-status publication did not retry a transient bridge failure"
grep -Fq -- \
  '-n /fixed/watchdog-github commit-status aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa pending deploy/watchdog Production pipeline started' \
  "$status_retry_log" \
  || wd_die "commit-status retry changed the requested SHA, state, context, or description"

# Exercise the actual watchdog dispatchers with barrier-backed fakes. A serialized implementation
# deadlocks its first fake until the barrier deadline and fails, while the intended implementation
# starts every worker, joins every result, and still returns a single parent-owned verdict.
# shellcheck disable=SC2091
eval "$(sed -n '/^run_github_status_lane()/,/^test_db()/p' "$ROOT/deploy/watchdog.sh" \
  | sed '$d')"
status_barrier_log="$TEMP/status-barrier.log"
STATUS_BARRIER_EXPECTED=0
STATUS_FAIL_CONTEXT=
github_status() {
  local context=$2 observed
  printf '%s\n' "$context" >>"$status_barrier_log"
  for _ in $(seq 1 200); do
    observed=$(wc -l <"$status_barrier_log")
    if (( observed >= STATUS_BARRIER_EXPECTED )); then
      [[ $context != "$STATUS_FAIL_CONTEXT" ]]
      return
    fi
    sleep 0.01
  done
  return 98
}

: >"$status_barrier_log"
STATUS_BARRIER_EXPECTED=2
publish_pipeline_start_statuses
[[ $(sort -u "$status_barrier_log" | tr '\n' ',') == 'deploy/tests,deploy/watchdog,' ]] \
  || wd_die "pipeline start statuses were not both published"

: >"$status_barrier_log"
STATUS_BARRIER_EXPECTED=7
publish_unchanged_component_statuses 0 0 0 0 0 0
[[ $(sort -u "$status_barrier_log" | tr '\n' ',') \
    == 'deploy/admin,deploy/backend,deploy/devbot,deploy/engine,deploy/migration,deploy/openkeys,deploy/sales,' ]] \
  || wd_die "unchanged status publication lost a component context"

: >"$status_barrier_log"
STATUS_BARRIER_EXPECTED=4
publish_unchanged_component_statuses 1 1 0 0 0 0
[[ $(sort -u "$status_barrier_log" | tr '\n' ',') \
    == 'deploy/admin,deploy/devbot,deploy/openkeys,deploy/sales,' ]] \
  || wd_die "changed components received a false no-change status"

: >"$status_barrier_log"
STATUS_BARRIER_EXPECTED=7
STATUS_FAIL_CONTEXT=deploy/engine
if ( publish_unchanged_component_statuses 0 0 0 0 0 0 ) >/dev/null 2>&1; then
  wd_die "a failed status worker did not fail the parent publication batch"
fi
(( $(wc -l <"$status_barrier_log") == 7 )) \
  || wd_die "a failed status worker abandoned sibling publication requests"
STATUS_FAIL_CONTEXT=

# shellcheck disable=SC2091
eval "$(sed -n '/^run_final_verification_lane()/,/^# Post-admission recovery/p' \
  "$ROOT/deploy/watchdog.sh" | sed '$d')"
verification_barrier_log="$TEMP/verification-barrier.log"
VERIFICATION_BARRIER_EXPECTED=0
VERIFICATION_FAIL_CHECK=
verification_probe() {
  local check=$1 observed
  printf '%s\n' "$check" >>"$verification_barrier_log"
  for _ in $(seq 1 200); do
    observed=$(grep -Evc '^runtime$' "$verification_barrier_log" || true)
    if (( observed >= VERIFICATION_BARRIER_EXPECTED )); then
      [[ $check != "$VERIFICATION_FAIL_CHECK" ]]
      return
    fi
    sleep 0.01
  done
  return 98
}
reconcile_engine_runtime() { printf 'runtime\n' >>"$verification_barrier_log"; }
final_verify_admin_data() { verification_probe panel; }
final_verify_admin_routing() { verification_probe routing; }
final_verify_monitoring() { verification_probe monitoring; }
final_verify_codex_surface() { verification_probe codex; }
final_verify_gemini_surface() { verification_probe gemini; }

: >"$verification_barrier_log"
VERIFICATION_BARRIER_EXPECTED=5
run_final_verification_plan runtime,panel,routing,monitoring,codex,gemini deadbeef
[[ $(sed -n '1p' "$verification_barrier_log") == runtime ]] \
  || wd_die "read-only final probes started before runtime reconciliation"
for final_probe in panel routing monitoring codex gemini; do
  grep -Fxq "$final_probe" "$verification_barrier_log" \
    || wd_die "final verification dispatcher omitted $final_probe"
done

: >"$verification_barrier_log"
VERIFICATION_BARRIER_EXPECTED=5
VERIFICATION_FAIL_CHECK=routing
if ( run_final_verification_plan runtime,panel,routing,monitoring,codex,gemini deadbeef ) \
    >/dev/null 2>&1; then
  wd_die "a failed final verifier did not fail the parent plan"
fi
[[ $(grep -Evc '^runtime$' "$verification_barrier_log") == 5 ]] \
  || wd_die "a failed final verifier abandoned sibling checks"
VERIFICATION_FAIL_CHECK=

# The real Codex verifier must distinguish an explicit disabled gauge from a temporarily missing
# series. Disabled returns after one query; enabled continues through runtime and envelope checks;
# a missing metric exhausts the bounded retries and fails closed.
(
  # shellcheck disable=SC2091
  eval "$(sed -n '/^final_verify_codex_surface()/,/^}/p' "$ROOT/deploy/watchdog.sh")"
  codex_probe_log="$TEMP/codex-probe.log"
  CODEX_PROBE_MODE=disabled
  CODEX_PROBE_BODY=
  CODEX_PROBE_STATUS=
  # Invoked indirectly by the extracted verifier.
  # shellcheck disable=SC2329
  curl() {
    local response_body response_status
    printf '%s\n' "${*//$'\n'/\\n}" >>"$codex_probe_log"
    case "$*" in
      *'query=claude_api_codex_enabled'*)
        case "$CODEX_PROBE_MODE" in
          disabled) printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"0"]}]}}' ;;
          enabled|unauthenticated) printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"1"]}]}}' ;;
          missing) printf '%s\n' '{"status":"success","data":{"result":[]}}' ;;
        esac
        ;;
      *'claude_api_codex_process_live'*)
        if [[ $CODEX_PROBE_MODE == unauthenticated ]]; then
          printf '%s\n' '{"status":"success","data":{"result":[]}}'
        else
          printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"1"]}]}}'
        fi
        ;;
      *'openai.api.apitoken.sale'*)
        if [[ $CODEX_PROBE_MODE == disabled ]]; then
          response_body=$CODEX_PROBE_BODY
          response_status=${CODEX_PROBE_STATUS:-404}
          [[ -n $response_body ]] \
            || response_body='{"error":{"type":"invalid_request_error","code":"model_not_found","param":null}}'
        else
          response_body=$CODEX_PROBE_BODY
          response_status=${CODEX_PROBE_STATUS:-401}
          [[ -n $response_body ]] \
            || response_body='{"error":{"type":"invalid_request_error","code":"invalid_api_key","param":null}}'
        fi
        printf '%s\n%s\n' "$response_body" "$response_status"
        ;;
      *) return 2 ;;
    esac
  }
  sleep() { :; }

  : >"$codex_probe_log"
  CODEX_PROBE_MODE=disabled
  final_verify_codex_surface >/dev/null
  (( $(wc -l <"$codex_probe_log") == 2 )) \
    || wd_die "disabled Codex verification skipped its public provider-envelope check"

  : >"$codex_probe_log"
  CODEX_PROBE_MODE=enabled
  final_verify_codex_surface >/dev/null
  (( $(wc -l <"$codex_probe_log") == 3 )) \
    || wd_die "enabled Codex verification skipped runtime or public-envelope checks"
  grep -Fq 'query=claude_api_codex_process_live{provider="openai"} == 1 and claude_api_codex_homes_authenticated{provider="openai"} >= 1' \
    "$codex_probe_log" || wd_die "Codex verification did not require a live authenticated home"
  if grep -Fq 'claude_api_codex_homes_available' "$codex_probe_log"; then
    wd_die "transient Codex capacity still controls the deployment verdict"
  fi

  CODEX_PROBE_STATUS=200
  if ( final_verify_codex_surface ) >/dev/null 2>&1; then
    wd_die "a 200 OpenAI error envelope passed verification"
  fi
  CODEX_PROBE_STATUS=401
  CODEX_PROBE_BODY='{"error":{"type":"invalid_request_error","code":null,"param":null}}'
  if ( final_verify_codex_surface ) >/dev/null 2>&1; then
    wd_die "a generic 401 error envelope passed OpenAI verification"
  fi
  CODEX_PROBE_BODY=
  CODEX_PROBE_STATUS=

  : >"$codex_probe_log"
  CODEX_PROBE_MODE=unauthenticated
  if ( final_verify_codex_surface ) >/dev/null 2>&1; then
    wd_die "an enabled Codex provider without an authenticated runtime passed verification"
  fi
  (( $(wc -l <"$codex_probe_log") == 7 )) \
    || wd_die "missing Codex runtime metrics did not use the bounded retry window"

  : >"$codex_probe_log"
  CODEX_PROBE_MODE=missing
  if ( final_verify_codex_surface ) >/dev/null 2>&1; then
    wd_die "missing Codex enablement metrics were treated as disabled"
  fi
  (( $(wc -l <"$codex_probe_log") == 6 )) \
    || wd_die "missing Codex enablement metrics did not use the bounded retry window"
)

# Gemini has the same explicit optional-provider contract, with one seller-onboarding relaxation:
# disabled is a stable native 404; enabled requires a native 401 and only records the authenticated
# project count (zero authenticated projects is a valid pre-onboarding state, not a deploy failure);
# missing Prometheus series still fail closed.
(
  # shellcheck disable=SC2091
  eval "$(sed -n '/^final_verify_gemini_surface()/,/^}/p' "$ROOT/deploy/watchdog.sh")"
  gemini_probe_log="$TEMP/gemini-probe.log"
  GEMINI_PROBE_MODE=disabled
  # Invoked indirectly by the extracted verifier.
  # shellcheck disable=SC2329
  curl() {
    printf '%s\n' "$*" >>"$gemini_probe_log"
    case "$*" in
      *'query=claude_api_gemini_enabled'*)
        case "$GEMINI_PROBE_MODE" in
          disabled) printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"0"]}]}}' ;;
          enabled|unauthenticated) printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"1"]}]}}' ;;
          missing) printf '%s\n' '{"status":"success","data":{"result":[]}}' ;;
        esac
        ;;
      *'claude_api_gemini_profiles_authenticated'*)
        if [[ $GEMINI_PROBE_MODE == unauthenticated ]]; then
          printf '%s\n' '{"status":"success","data":{"result":[]}}'
        else
          printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"1"]}]}}'
        fi
        ;;
      *'gemini.api.apitoken.sale'*)
        if [[ $GEMINI_PROBE_MODE == disabled ]]; then
          printf '%s\n' '{"error":{"code":404,"status":"NOT_FOUND"}}'
        else
          printf '%s\n' '{"error":{"code":400,"status":"INVALID_ARGUMENT","details":[{"reason":"API_KEY_INVALID"}]}}'
        fi
        ;;
      *) return 2 ;;
    esac
  }
  sleep() { :; }

  : >"$gemini_probe_log"
  GEMINI_PROBE_MODE=disabled
  final_verify_gemini_surface >/dev/null
  (( $(wc -l <"$gemini_probe_log") == 2 )) \
    || wd_die "disabled Gemini verification skipped its public native-envelope check"

  : >"$gemini_probe_log"
  GEMINI_PROBE_MODE=enabled
  final_verify_gemini_surface >/dev/null
  (( $(wc -l <"$gemini_probe_log") == 3 )) \
    || wd_die "enabled Gemini verification skipped project or public-envelope checks"

  : >"$gemini_probe_log"
  GEMINI_PROBE_MODE=unauthenticated
  final_verify_gemini_surface >/dev/null \
    || wd_die "an enabled Gemini provider with zero authenticated projects must pass as pre-onboarding"
  (( $(wc -l <"$gemini_probe_log") == 3 )) \
    || wd_die "pre-onboarding Gemini verification skipped project count or public-envelope check"

  : >"$gemini_probe_log"
  GEMINI_PROBE_MODE=missing
  if ( final_verify_gemini_surface ) >/dev/null 2>&1; then
    wd_die "missing Gemini enablement metrics were treated as disabled"
  fi
  (( $(wc -l <"$gemini_probe_log") == 6 )) \
    || wd_die "missing Gemini enablement metrics did not use the bounded retry window"
)

# Atomic state writes are shared by parallel rollout lanes. Bash keeps `$$` constant in asynchronous
# subshells, so this test proves each writer uses a unique temporary path and leaves one valid value.
parallel_state="$TEMP/parallel-state"
parallel_pids=()
for value in $(seq 1 32); do
  wd_atomic_write "$parallel_state" "$value" 0644 &
  parallel_pids+=("$!")
done
for parallel_pid in "${parallel_pids[@]}"; do wait "$parallel_pid"; done
parallel_value=$(<"$parallel_state")
[[ $parallel_value =~ ^([1-9]|[12][0-9]|3[0-2])$ ]] \
  || wd_die "parallel atomic writes left an invalid state value: $parallel_value"
if find "$TEMP" -maxdepth 1 -name 'parallel-state.tmp.*' -print -quit | grep -q .; then
  wd_die "parallel atomic writes left a temporary file behind"
fi

# Candidate retention selects only direct, real SHA directories strictly older than the cutoff.
# It must not follow symlinks or touch malformed entries.
candidate_root="$TEMP/candidates"
mkdir -p "$candidate_root"
old_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
boundary_sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
new_sha=cccccccccccccccccccccccccccccccccccccccc
symlink_sha=dddddddddddddddddddddddddddddddddddddddd
marked_new_sha=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
marked_old_sha=ffffffffffffffffffffffffffffffffffffffff
mkdir -p "$candidate_root/$old_sha" "$candidate_root/$boundary_sha" \
  "$candidate_root/$new_sha" "$candidate_root/$marked_new_sha" \
  "$candidate_root/$marked_old_sha" "$candidate_root/not-a-sha" "$TEMP/outside-candidate"
ln -s -- "$TEMP/outside-candidate" "$candidate_root/$symlink_sha"
touch "$TEMP/$marked_new_sha.tested" "$TEMP/$marked_old_sha.tested"
candidate_cutoff=1800000000
node - "$candidate_root/$old_sha" "$candidate_root/$boundary_sha" \
  "$candidate_root/$new_sha" "$candidate_root/$marked_new_sha" \
  "$candidate_root/$marked_old_sha" "$candidate_root/not-a-sha" "$TEMP/outside-candidate" \
  "$TEMP/$marked_new_sha.tested" "$TEMP/$marked_old_sha.tested" "$candidate_cutoff" <<'NODE'
const fs = require("node:fs");
const [oldPath, boundaryPath, newPath, markedNewPath, markedOldPath, malformedPath, outsidePath,
  markedNewMarker, markedOldMarker, cutoffText] = process.argv.slice(2);
const cutoff = Number(cutoffText);
fs.utimesSync(oldPath, cutoff - 1, cutoff - 1);
fs.utimesSync(boundaryPath, cutoff, cutoff);
fs.utimesSync(newPath, cutoff + 1, cutoff + 1);
fs.utimesSync(markedNewPath, cutoff - 1, cutoff - 1);
fs.utimesSync(markedOldPath, cutoff + 1, cutoff + 1);
fs.utimesSync(malformedPath, cutoff - 1, cutoff - 1);
fs.utimesSync(outsidePath, cutoff - 1, cutoff - 1);
fs.utimesSync(markedNewMarker, cutoff + 1, cutoff + 1);
fs.utimesSync(markedOldMarker, cutoff - 1, cutoff - 1);
NODE
expired_candidates=()
while IFS= read -r -d '' expired_candidate; do
  expired_candidates+=("$expired_candidate")
done < <(wd_candidate_dirs_older_than "$candidate_root" "$TEMP" "$candidate_cutoff")
[[ ${#expired_candidates[@]} -eq 2 ]] \
  || wd_die "candidate retention selected an unsafe or non-expired directory"
printf '%s\n' "${expired_candidates[@]}" | grep -Fxq "$candidate_root/$old_sha" \
  || wd_die "candidate retention did not select an expired untested workspace"
printf '%s\n' "${expired_candidates[@]}" | grep -Fxq "$candidate_root/$marked_old_sha" \
  || wd_die "candidate retention did not use the test-completion marker age"

for fixture in baseline appended tampered; do
  mkdir -p "$TEMP/$fixture/packages/db"
  cp -R -- "$ROOT/packages/db/migrations" "$TEMP/$fixture/packages/db/migrations"
done

wd_migration_manifest "$TEMP/baseline" >"$TEMP/baseline.manifest"

node - "$TEMP/appended/packages/db/migrations/meta/_journal.json" "$TEMP/appended/packages/db/migrations/0012_watchdog_manifest_test.sql" <<'NODE'
const fs = require("node:fs");
const [journalPath, sqlPath] = process.argv.slice(2);
const journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
const previous = journal.entries.at(-1);
journal.entries.push({
  idx: journal.entries.length,
  version: journal.version,
  when: previous.when + 1,
  tag: "0012_watchdog_manifest_test",
  breakpoints: true,
});
fs.writeFileSync(journalPath, `${JSON.stringify(journal, null, 2)}\n`);
fs.writeFileSync(sqlPath, "CREATE TABLE watchdog_manifest_test (id integer PRIMARY KEY);\n");
NODE
wd_migration_manifest "$TEMP/appended" >"$TEMP/appended.manifest"
wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/appended.manifest"
[[ $(wd_manifest_digest "$TEMP/baseline.manifest") != $(wd_manifest_digest "$TEMP/appended.manifest") ]]

printf '\n-- forbidden historical edit\n' >>"$TEMP/tampered/packages/db/migrations/0000_curved_skrulls.sql"
wd_migration_manifest "$TEMP/tampered" >"$TEMP/tampered.manifest"
if wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/tampered.manifest"; then
  wd_die "manifest accepted an edited historical SQL migration"
fi

node - "$TEMP/tampered/packages/db/migrations/meta/_journal.json" <<'NODE'
const fs = require("node:fs");
const journalPath = process.argv[2];
const journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
journal.entries[0].when += 1;
fs.writeFileSync(journalPath, JSON.stringify(journal));
NODE
wd_migration_manifest "$TEMP/tampered" >"$TEMP/tampered-journal.manifest"
if wd_manifest_is_append_only "$TEMP/baseline.manifest" "$TEMP/tampered-journal.manifest"; then
  wd_die "manifest accepted an edited historical journal entry"
fi

# Bounded retry: transient failures are absorbed, permanent ones still surface their exit status.
retry_attempts_file="$TEMP/retry-attempts"
printf '0\n' >"$retry_attempts_file"
flaky_command() {
  local count
  count=$(<"$retry_attempts_file")
  count=$((count + 1))
  printf '%s\n' "$count" >"$retry_attempts_file"
  (( count >= 3 ))
}
wd_retry 3 0 flaky_command || wd_die "retry did not absorb a transient failure"
[[ $(<"$retry_attempts_file") == 3 ]] || wd_die "retry did not stop at the first success"
if wd_retry 2 0 false; then
  wd_die "retry reported success for a permanently failing command"
fi

# Release retention: current/previous and explicitly protected SHAs survive regardless of age, the
# newest `keep` are retained, and only genuine SHA directories are ever selected.
release_root="$TEMP/releases"
mkdir -p "$release_root"
release_shas=(
  1111111111111111111111111111111111111111
  2222222222222222222222222222222222222222
  3333333333333333333333333333333333333333
  4444444444444444444444444444444444444444
  5555555555555555555555555555555555555555
)
for release_sha in "${release_shas[@]}"; do
  mkdir -p "$release_root/$release_sha"
done
mkdir -p "$release_root/not-a-release"
ln -s "$release_root/${release_shas[4]}" "$release_root/current"
ln -s "$release_root/${release_shas[3]}" "$release_root/previous"
node - "$release_root" "${release_shas[@]}" <<'NODE'
const fs = require("node:fs");
const [root, ...shas] = process.argv.slice(2);
// Oldest first, so index 0 is the least recently modified release.
shas.forEach((sha, index) => {
  const when = 1700000000 + index;
  fs.utimesSync(`${root}/${sha}`, when, when);
});
NODE

# keep=1 retains the newest unprotected release plus current/previous. Of the three unprotected
# releases (…111, …222, …333), the newest (…333) is kept and the two oldest are selected.
prunable_releases=()
while IFS= read -r -d '' prunable_release; do
  prunable_releases+=("${prunable_release##*/}")
done < <(wd_prunable_release_dirs "$release_root" 1)
[[ ${#prunable_releases[@]} -eq 2 ]] \
  || wd_die "release retention selected ${#prunable_releases[@]} directories, expected 2"
printf '%s\n' "${prunable_releases[@]}" | grep -Fxq "${release_shas[0]}" \
  || wd_die "release retention did not select the oldest release"
printf '%s\n' "${prunable_releases[@]}" | grep -Fxq "${release_shas[1]}" \
  || wd_die "release retention did not select the second-oldest release"
for protected_release in "${release_shas[4]}" "${release_shas[3]}" "${release_shas[2]}" not-a-release; do
  if printf '%s\n' "${prunable_releases[@]}" | grep -Fxq "$protected_release"; then
    wd_die "release retention selected a protected or non-release entry: $protected_release"
  fi
done

# An explicitly protected SHA (a live PID's release, or a recorded component baseline) must survive
# even when retention counting would otherwise reach it.
protected_selection=()
while IFS= read -r -d '' prunable_release; do
  protected_selection+=("${prunable_release##*/}")
done < <(wd_prunable_release_dirs "$release_root" 1 "${release_shas[0]}")
if printf '%s\n' "${protected_selection[@]}" | grep -Fxq "${release_shas[0]}"; then
  wd_die "release retention removed an explicitly protected live release"
fi

# keep=0 with no protected list still must not touch current/previous.
zero_keep_selection=()
while IFS= read -r -d '' prunable_release; do
  zero_keep_selection+=("${prunable_release##*/}")
done < <(wd_prunable_release_dirs "$release_root" 0)
[[ ${#zero_keep_selection[@]} -eq 3 ]] \
  || wd_die "keep=0 retention must still protect current and previous"

# Pre-deploy dump retention is per-database and must never select the hourly rotation artifact.
dump_root="$TEMP/backups"
mkdir -p "$dump_root"
for database in commerce claude_engine; do
  : >"$dump_root/$database.dump"
  for index in 1 2 3; do
    dump_sha=$(printf '%040d' "$index")
    : >"$dump_root/$database.pre-deploy-$dump_sha.dump"
    node -e 'const fs=require("node:fs");const w=1700000000+Number(process.argv[2]);fs.utimesSync(process.argv[1],w,w);' \
      "$dump_root/$database.pre-deploy-$dump_sha.dump" "$index"
  done
done
: >"$dump_root/commerce-pre-offboard-20260715T102931Z.dump"

prunable_dumps=()
while IFS= read -r -d '' prunable_dump; do
  prunable_dumps+=("${prunable_dump##*/}")
done < <(wd_prunable_predeploy_dumps "$dump_root" 2)
# Two databases, three snapshots each, keep the newest two: exactly one per database is selected.
[[ ${#prunable_dumps[@]} -eq 2 ]] \
  || wd_die "dump retention selected ${#prunable_dumps[@]} files, expected 2"
for retained_dump in commerce.dump claude_engine.dump commerce-pre-offboard-20260715T102931Z.dump; do
  if printf '%s\n' "${prunable_dumps[@]}" | grep -Fxq "$retained_dump"; then
    wd_die "dump retention selected a non-pre-deploy artifact: $retained_dump"
  fi
done
oldest_dump_sha=$(printf '%040d' 1)
printf '%s\n' "${prunable_dumps[@]}" | grep -Fxq "commerce.pre-deploy-$oldest_dump_sha.dump" \
  || wd_die "dump retention did not select the oldest commerce snapshot"

# Path classifiers: sales vs backend/engine/infra separation.
wd_path_is_typescript apps/web/src/app/page.tsx || wd_die "web app not classified for TypeScript validation"
wd_path_is_typescript packages/contracts/src/index.ts || wd_die "workspace package not classified for TypeScript validation"
wd_path_is_merge_workflow .claude/hooks/guard-git.sh || wd_die "git guard not classified as merge workflow"
wd_path_is_validation_neutral docs/engine/STAGE2_POSTGRES_AUTHORITY.md \
  || wd_die "documentation should be validation-neutral"
wd_path_is_sales apps/sales-api/src/main.ts || wd_die "sales-api not classified as sales"
wd_path_is_sales apps/sales-web/src/app/page.tsx || wd_die "sales-web not classified as sales"
wd_path_is_sales packages/sales-db/src/schema.ts || wd_die "sales-db not classified as sales"
wd_path_is_sales apps/api/src/main.ts && wd_die "commerce api wrongly classified as sales"
wd_path_is_sales crates/server/src/http.rs && wd_die "engine wrongly classified as sales"
wd_path_is_backend packages/sales-db/src/schema.ts \
  && wd_die "sales-db must not trigger the independent commerce backend"
wd_path_is_backend packages/openkeys-db/src/schema.ts \
  && wd_die "openkeys-db must not trigger the independent commerce backend"
wd_path_is_admin apps/admin/src/app/page.tsx || wd_die "admin app not classified as admin"
wd_path_is_admin apps/api/src/main.ts && wd_die "commerce api wrongly classified as admin"
wd_path_is_admin crates/server/src/http.rs && wd_die "engine wrongly classified as admin"
wd_path_is_devbot apps/devbot/src/main.ts || wd_die "devbot app not classified as devbot"
wd_path_is_devbot systemd/apitoken-devbot.service \
  || wd_die "devbot unit not classified as devbot"
wd_path_is_devbot deploy/devbot-deploy.sh || wd_die "devbot deploy script not classified as devbot"
wd_path_is_devbot apps/api/src/main.ts && wd_die "commerce api wrongly classified as devbot"
wd_path_is_devbot crates/server/src/http.rs && wd_die "engine wrongly classified as devbot"
wd_path_is_devbot apps/admin/src/app/page.tsx && wd_die "admin app wrongly classified as devbot"
wd_path_is_admin apps/devbot/src/main.ts && wd_die "devbot app wrongly classified as admin"
wd_path_is_backend apps/devbot/src/main.ts \
  && wd_die "apps/devbot must not trigger the independent commerce backend"
wd_path_is_backend apps/admin/src/app/page.tsx \
  && wd_die "apps/admin must not trigger the independent commerce backend"
wd_path_is_sales apps/admin/src/app/page.tsx && wd_die "admin app wrongly classified as sales"
wd_path_is_openkeys apps/admin/src/app/page.tsx \
  && wd_die "admin app wrongly classified as openkeys"
wd_path_is_backend packages/engine-client/src/index.ts \
  || wd_die "engine-client remains shared with the commerce backend"
wd_path_is_backend packages/contracts/src/index.ts \
  || wd_die "contracts remain shared with the commerce backend"
wd_path_is_backend apps/content-studio/src/app/page.tsx || wd_die "content studio must trigger commerce deployment"
wd_path_is_engine tools/codex-native/probe-live.py \
  || wd_die "native Codex tooling must trigger an engine deployment"
for provider_unit in systemd/claude-api.service systemd/claude-api@.service \
  systemd/claude-api-anthropic@.service systemd/claude-api-openai.service \
  systemd/claude-api-openai@.service systemd/claude-api-gemini.service \
  systemd/claude-api-gemini@.service; do
  wd_path_is_engine "$provider_unit" \
    || wd_die "provider unit change does not force runtime adoption: $provider_unit"
done
wd_path_is_codex_tooling tools/codex-native/probe-live.py \
  || wd_die "native Codex tooling must stay on the engine deployment path"
if wd_path_is_codex_tooling crates/forward/src/codex/api.rs; then
  wd_die "gateway-only changes must not rebuild any sidecar Codex artifact"
fi
grep -Fq 'WEB_HEALTH=${OPENKEYS_WEB_HEALTH:-http://127.0.0.1:3410/api/ready}' \
  "$ROOT/deploy/openkeys-deploy.sh" \
  || wd_die "OpenKeys rollout must gate on dependency readiness"
grep -Fq 'WEB_ROLLBACK_HEALTH=${OPENKEYS_WEB_ROLLBACK_HEALTH:-http://127.0.0.1:3410/docs}' \
  "$ROOT/deploy/openkeys-deploy.sh" \
  || wd_die "OpenKeys rollback health must remain compatible with the previous release"
grep -Fq 'WEB_HEALTH=${ADMIN_WEB_HEALTH:-http://127.0.0.1:3700/api/health}' \
  "$ROOT/deploy/admin-deploy.sh" \
  || wd_die "admin rollout must gate on the application health endpoint"
grep -Fq 'WEB_ROLLBACK_HEALTH=${ADMIN_WEB_ROLLBACK_HEALTH:-http://127.0.0.1:3700/api/health}' \
  "$ROOT/deploy/admin-deploy.sh" \
  || wd_die "admin rollback health must remain compatible with the previous release"
if grep -Fq 'migrate.js' "$ROOT/deploy/admin-deploy.sh"; then
  wd_die "the admin panel has no database and must not run migrations"
fi
grep -Fq 'HEALTH=${DEVBOT_HEALTH:-http://127.0.0.1:3800/health}' \
  "$ROOT/deploy/devbot-deploy.sh" \
  || wd_die "devbot rollout must gate on the application health endpoint"
grep -Fq 'ROLLBACK_HEALTH=${DEVBOT_ROLLBACK_HEALTH:-http://127.0.0.1:3800/health}' \
  "$ROOT/deploy/devbot-deploy.sh" \
  || wd_die "devbot rollback health must remain compatible with the previous release"
grep -Fq 'devbot disabled: ' "$ROOT/deploy/devbot-deploy.sh" \
  || wd_die "devbot rollout must skip cleanly while secrets are not provisioned"
grep -Fq 'missing — skipping' "$ROOT/deploy/devbot-deploy.sh" \
  || wd_die "devbot disabled skip must keep its operator-visible log line"
if grep -Fq 'migrate.js' "$ROOT/deploy/devbot-deploy.sh"; then
  wd_die "the devbot has no database and must not run migrations"
fi

wd_engine_topology_is_steady 1 1 1 1 0 0 0 0 0 0
wd_engine_topology_is_steady 0 0 0 0 1 1 1 1 0 0
for invalid_topology in \
  "1 1 1 1 1 1 1 1 0 0" \
  "1 1 1 0 0 0 0 1 0 0" \
  "1 0 1 1 0 0 0 0 0 0" \
  "1 1 0 1 0 0 0 0 0 0" \
  "1 1 1 1 0 0 0 0 1 0" \
  "1 1 1 1 0 0 0 0 0 1"; do
  # Each fixture intentionally expands to ten arguments.
  # shellcheck disable=SC2086
  if wd_engine_topology_is_steady $invalid_topology; then
    wd_die "engine topology accepted an invalid steady state: $invalid_topology"
  fi
done

wd_path_is_infrastructure deploy/watchdog.sh
wd_path_is_infrastructure systemd/apitoken-deploy-watchdog.service
wd_path_is_infrastructure systemd/apitoken-candidate-validator.service
wd_path_is_infrastructure compose.yaml
wd_path_is_infrastructure observability/prometheus/prometheus.yml
wd_path_is_infrastructure deploy/affinity-redis.compose.yaml
if wd_path_is_infrastructure .github/workflows/indexnow.yml; then
  wd_die "GitHub-only workflow changes must not require a production-host infrastructure install"
fi
for runtime_definition in \
  deploy/watchdog.sh \
  deploy/watchdog-lib.sh \
  deploy/validation-plan.sh \
  deploy/install-watchdog.sh \
  deploy/Caddyfile \
  deploy/install-caddy.sh \
  deploy/install-monitoring.sh \
  deploy/install-tmpfiles.sh \
  systemd/apitoken-deploy-watchdog.service \
  systemd/apitoken-candidate-validator.service \
  systemd/apitoken-tmpfiles-install.service \
  systemd/claude-api.service \
  systemd/claude-api@.service \
  systemd/claude-api-anthropic@.service \
  systemd/claude-api-openai.service \
  systemd/claude-api-openai@.service \
  systemd/claude-api-gemini.service \
  systemd/claude-api-gemini@.service \
  systemd/apitoken-tmpfiles.conf \
  observability/prometheus/prometheus.yml; do
  wd_path_requires_infrastructure_install "$runtime_definition" \
    || wd_die "runtime definition did not request infrastructure installation: $runtime_definition"
done
for validation_only_path in \
  deploy/README.md \
  deploy/lib.test.sh \
  deploy/watchdog-lib.test.sh \
  deploy/monitoring-config.test.sh \
  deploy/agent-merge.sh \
  deploy/agent-merge.suite.sh \
  deploy/test-stage2-e2e.sh \
  deploy/sccache-cargo.sh \
  deploy/next-cache.sh \
  deploy/typescript-scope.mjs \
  deploy/typescript-build-contexts.sh \
  deploy/typescript-test-groups.sh \
  deploy/commerce-release-bundle.test.sh \
  compose.yaml; do
  wd_path_is_infrastructure "$validation_only_path" \
    || wd_die "deployment tooling path escaped operational validation: $validation_only_path"
  if wd_path_requires_infrastructure_install "$validation_only_path"; then
    wd_die "validation-only path requested a production-host reinstall: $validation_only_path"
  fi
done
wd_path_is_caddy deploy/Caddyfile
wd_path_is_caddy deploy/install-caddy.sh
wd_path_is_caddy deploy/render-caddy.awk
if wd_path_is_caddy deploy/watchdog.sh; then
  wd_die "non-Caddy infrastructure change requested a Caddy reload"
fi
wd_path_is_systemd_definition systemd/apitoken-deploy-watchdog.service
wd_path_is_systemd_definition systemd/apitoken-tmpfiles-install.service
wd_path_is_systemd_definition systemd/apitoken-sysctl-install.service
wd_path_is_systemd_definition systemd/sysctl-apitoken-redis.conf
wd_path_is_systemd_definition deploy/install-sysctl.sh
wd_path_is_systemd_definition deploy/install-tmpfiles.sh
wd_path_is_systemd_definition systemd/claude-api.service
wd_path_is_systemd_definition systemd/claude-api-openai.service
wd_path_is_systemd_definition systemd/claude-api-gemini.service
wd_path_is_systemd_definition systemd/claude-api-gemini@.service
wd_path_is_systemd_definition systemd/apitoken-admin.service \
  || wd_die "the admin panel unit escaped the narrow systemd installer"
if wd_path_is_systemd_definition systemd/future-uninstalled.service; then
  wd_die "unknown systemd definition entered the narrow installer"
fi
wd_path_is_monitoring_definition observability/prometheus/prometheus.yml
wd_path_is_monitoring_definition deploy/install-monitoring.sh
if wd_path_is_monitoring_definition deploy/affinity-redis.compose.yaml; then
  wd_die "stateful Redis definition entered the monitoring installer"
fi
for controller_definition in \
  deploy/watchdog.sh \
  deploy/watchdog-lib.sh \
  deploy/validation-plan.sh \
  deploy/watchdog-infrastructure.sh \
  deploy/deploy.sh \
  deploy/lib.sh \
  deploy/commerce-release-bundle.sh \
  deploy/release-tree-digest.mjs \
  deploy/content-studio-start.sh \
  deploy/api-bluegreen.sh \
  deploy/engine-bluegreen.sh \
  deploy/engine-migrate.sh \
  deploy/codex-homes-migrate.sh \
  deploy/rollback.sh \
  deploy/sales-deploy.sh \
  deploy/openkeys-deploy.sh \
  deploy/admin-deploy.sh; do
  wd_path_is_controller_definition "$controller_definition" \
    || wd_die "fixed controller definition escaped the narrow installer: $controller_definition"
done
for full_definition in \
  deploy/install-watchdog.sh \
  deploy/install-sudoers.sh \
  deploy/affinity-redis.compose.yaml; do
  if wd_path_is_controller_definition "$full_definition"; then
    wd_die "stateful or privileged definition entered the narrow installer: $full_definition"
  fi
done

# The root transaction is selected from the exact range. Independent narrow concerns compose;
# privileged/stateful definitions, unknown files, and deletions fail closed to the complete
# installer.
infrastructure_repo="$TEMP/infrastructure-repo"
git init --quiet "$infrastructure_repo"
git -C "$infrastructure_repo" config user.name test
git -C "$infrastructure_repo" config user.email test@example.invalid
mkdir -p "$infrastructure_repo/deploy" "$infrastructure_repo/systemd" \
  "$infrastructure_repo/observability"
printf 'controller\n' >"$infrastructure_repo/deploy/watchdog.sh"
printf 'caddy\n' >"$infrastructure_repo/deploy/Caddyfile"
printf 'cache\n' >"$infrastructure_repo/deploy/next-cache.sh"
printf 'unit\n' >"$infrastructure_repo/systemd/apitoken-deploy-watchdog.service"
printf 'monitoring\n' >"$infrastructure_repo/observability/config.yml"
printf 'local compose\n' >"$infrastructure_repo/compose.yaml"
git -C "$infrastructure_repo" add deploy/watchdog.sh deploy/Caddyfile \
  deploy/next-cache.sh systemd/apitoken-deploy-watchdog.service \
  observability/config.yml compose.yaml
git -C "$infrastructure_repo" commit --quiet -m base
infrastructure_base=$(git -C "$infrastructure_repo" rev-parse HEAD)

assert_infrastructure_scope() {
  local expected=$1 base=$2 target=$3 actual
  actual=$(wd_infrastructure_install_scope "$infrastructure_repo" "$base" "$target")
  [[ $actual == "$expected" ]] \
    || wd_die "infrastructure scope was $actual, expected $expected for $base..$target"
}

printf 'edit\n' >>"$infrastructure_repo/deploy/watchdog.sh"
git -C "$infrastructure_repo" add deploy/watchdog.sh
git -C "$infrastructure_repo" commit --quiet -m controller
infrastructure_controller=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope controller "$infrastructure_base" "$infrastructure_controller"

printf 'edit\n' >>"$infrastructure_repo/deploy/next-cache.sh"
git -C "$infrastructure_repo" add deploy/next-cache.sh
git -C "$infrastructure_repo" commit --quiet -m validation-only
infrastructure_validation=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope none "$infrastructure_controller" "$infrastructure_validation"

printf 'edit\n' >>"$infrastructure_repo/deploy/Caddyfile"
git -C "$infrastructure_repo" add deploy/Caddyfile
git -C "$infrastructure_repo" commit --quiet -m caddy
infrastructure_caddy=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope caddy "$infrastructure_validation" "$infrastructure_caddy"

printf 'mixed\n' >>"$infrastructure_repo/deploy/watchdog.sh"
printf 'mixed\n' >>"$infrastructure_repo/deploy/Caddyfile"
git -C "$infrastructure_repo" add deploy/watchdog.sh deploy/Caddyfile
git -C "$infrastructure_repo" commit --quiet -m mixed
infrastructure_mixed=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope controller+caddy "$infrastructure_caddy" "$infrastructure_mixed"

printf 'edit\n' >>"$infrastructure_repo/systemd/apitoken-deploy-watchdog.service"
git -C "$infrastructure_repo" add systemd/apitoken-deploy-watchdog.service
git -C "$infrastructure_repo" commit --quiet -m systemd
infrastructure_systemd=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope systemd "$infrastructure_mixed" "$infrastructure_systemd"

printf 'edit\n' >>"$infrastructure_repo/observability/config.yml"
git -C "$infrastructure_repo" add observability/config.yml
git -C "$infrastructure_repo" commit --quiet -m monitoring
infrastructure_monitoring=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope monitoring "$infrastructure_systemd" "$infrastructure_monitoring"

printf 'edit\n' >>"$infrastructure_repo/compose.yaml"
git -C "$infrastructure_repo" add compose.yaml
git -C "$infrastructure_repo" commit --quiet -m local-compose
infrastructure_local_compose=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope none "$infrastructure_monitoring" "$infrastructure_local_compose"

printf 'all\n' >>"$infrastructure_repo/deploy/watchdog.sh"
printf 'all\n' >>"$infrastructure_repo/deploy/Caddyfile"
printf 'all\n' >>"$infrastructure_repo/systemd/apitoken-deploy-watchdog.service"
printf 'all\n' >>"$infrastructure_repo/observability/config.yml"
git -C "$infrastructure_repo" add deploy/watchdog.sh deploy/Caddyfile \
  systemd/apitoken-deploy-watchdog.service observability/config.yml
git -C "$infrastructure_repo" commit --quiet -m all-narrow
infrastructure_all_narrow=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope controller+caddy+systemd+monitoring \
  "$infrastructure_local_compose" "$infrastructure_all_narrow"

git -C "$infrastructure_repo" rm --quiet deploy/watchdog.sh
git -C "$infrastructure_repo" commit --quiet -m deletion
infrastructure_deletion=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope full "$infrastructure_all_narrow" "$infrastructure_deletion"

printf 'unknown\n' >"$infrastructure_repo/deploy/future-runtime.sh"
git -C "$infrastructure_repo" add deploy/future-runtime.sh
git -C "$infrastructure_repo" commit --quiet -m unknown
infrastructure_unknown=$(git -C "$infrastructure_repo" rev-parse HEAD)
assert_infrastructure_scope full "$infrastructure_deletion" "$infrastructure_unknown"

# Documentation-only ranges stay cheap, but any new unclassified area fails safe into the complete
# validation set until its owner adds an explicit classifier.
validation_repo="$TEMP/validation-repo"
git init --quiet "$validation_repo"
git -C "$validation_repo" config user.name test
git -C "$validation_repo" config user.email test@example.invalid
git -C "$validation_repo" commit --quiet --allow-empty -m base
validation_base=$(git -C "$validation_repo" rev-parse HEAD)
mkdir -p "$validation_repo/docs"
printf 'known\n' >"$validation_repo/docs/known.md"
git -C "$validation_repo" add docs/known.md
git -C "$validation_repo" commit --quiet -m docs
validation_docs=$(git -C "$validation_repo" rev-parse HEAD)
if wd_range_has_unknown_validation_path "$validation_repo" "$validation_base" "$validation_docs"; then
  wd_die "documentation-only range was treated as unknown code"
fi
mkdir -p "$validation_repo/mystery"
printf 'unknown\n' >"$validation_repo/mystery/runtime.xyz"
git -C "$validation_repo" add mystery/runtime.xyz
git -C "$validation_repo" commit --quiet -m unknown
validation_unknown=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_has_unknown_validation_path "$validation_repo" "$validation_docs" "$validation_unknown" \
  || wd_die "an unclassified path did not fail safe into complete validation"

# The versioned planner is an executable contract, not only a collection of classifiers. Verify the
# cheap known-path envelope and the fail-closed unknown-path envelope with explicit baselines.
plan_value() {
  local plan=$1 key=$2 value
  value=$(grep -E "^${key}=" <<<"$plan")
  [[ $(grep -Ec "^${key}=" <<<"$plan") == 1 ]] \
    || wd_die "validation plan did not contain exactly one $key"
  printf '%s\n' "${value#*=}"
}

docs_plan=$(bash "$ROOT/deploy/validation-plan.sh" "$validation_repo" "$validation_docs" \
  "$validation_base" "$validation_base" "$validation_base" "$validation_base" "$validation_base")
[[ $(plan_value "$docs_plan" validation_plan_format) == 1 ]] \
  || wd_die "validation plan format is not versioned"
[[ $(plan_value "$docs_plan" validation_policy_sha256) =~ ^[0-9a-f]{64}$ ]] \
  || wd_die "validation plan policy is not content-addressed"
for flag in typescript_required typescript_full rust_required static_required engine_artifacts_required; do
  [[ $(plan_value "$docs_plan" "$flag") == 0 ]] \
    || wd_die "documentation-only validation plan enabled $flag"
done

installed_planner_root="$TEMP/installed-planner"
mkdir -p "$installed_planner_root/controller"
cp "$ROOT/deploy/validation-plan.sh" "$installed_planner_root/controller/validation-plan.sh"
cp "$ROOT/deploy/watchdog-lib.sh" "$installed_planner_root/watchdog-lib.sh"
installed_docs_plan=$(bash "$installed_planner_root/controller/validation-plan.sh" \
  "$validation_repo" "$validation_docs" "$validation_base" "$validation_base" \
  "$validation_base" "$validation_base" "$validation_base")
[[ $installed_docs_plan == "$docs_plan" ]] \
  || wd_die "installed and repository planner layouts produced different policies"

unknown_plan=$(bash "$ROOT/deploy/validation-plan.sh" "$validation_repo" "$validation_unknown" \
  "$validation_docs" "$validation_docs" "$validation_docs" "$validation_docs" "$validation_docs")
for flag in typescript_required typescript_full rust_required static_required engine_artifacts_required; do
  [[ $(plan_value "$unknown_plan" "$flag") == 1 ]] \
    || wd_die "unknown-path validation plan did not fail closed for $flag"
done

# Package edits stay filterable, while shared inputs, selector changes, and deleted package paths
# force a complete TypeScript workspace check.
mkdir -p "$validation_repo/apps/example"
printf 'base\n' >"$validation_repo/apps/example/index.ts"
git -C "$validation_repo" add apps/example/index.ts
git -C "$validation_repo" commit --quiet -m typescript-scope-base
validation_typescript_base=$(git -C "$validation_repo" rev-parse HEAD)
printf 'edit\n' >>"$validation_repo/apps/example/index.ts"
git -C "$validation_repo" add apps/example/index.ts
git -C "$validation_repo" commit --quiet -m typescript-scope-edit
validation_typescript_edit=$(git -C "$validation_repo" rev-parse HEAD)
if wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_typescript_base" "$validation_typescript_edit"; then
  wd_die "an ordinary package edit unnecessarily forced full TypeScript validation"
fi
printf 'lock\n' >"$validation_repo/pnpm-lock.yaml"
git -C "$validation_repo" add pnpm-lock.yaml
git -C "$validation_repo" commit --quiet -m typescript-shared-input
validation_typescript_shared=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_typescript_edit" "$validation_typescript_shared" \
  || wd_die "a shared TypeScript input did not force full validation"
mkdir -p "$validation_repo/deploy"
printf 'selector\n' >"$validation_repo/deploy/typescript-scope.mjs"
git -C "$validation_repo" add deploy/typescript-scope.mjs
git -C "$validation_repo" commit --quiet -m typescript-selector
validation_typescript_selector=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_changes_typescript_gate "$validation_repo" \
  "$validation_typescript_shared" "$validation_typescript_selector" \
  || wd_die "a TypeScript selector change did not identify gate machinery"
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_typescript_shared" "$validation_typescript_selector" \
  || wd_die "a TypeScript selector change did not force full validation"
printf 'cache\n' >"$validation_repo/deploy/next-cache.sh"
git -C "$validation_repo" add deploy/next-cache.sh
git -C "$validation_repo" commit --quiet -m next-cache-helper
validation_next_cache=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_changes_typescript_gate "$validation_repo" \
  "$validation_typescript_selector" "$validation_next_cache" \
  || wd_die "a Next.js cache helper change did not identify gate machinery"
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_typescript_selector" "$validation_next_cache" \
  || wd_die "a Next.js cache helper change did not force full validation"
printf 'contexts\n' >"$validation_repo/deploy/typescript-build-contexts.sh"
git -C "$validation_repo" add deploy/typescript-build-contexts.sh
git -C "$validation_repo" commit --quiet -m typescript-build-contexts
validation_build_contexts=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_changes_typescript_gate "$validation_repo" \
  "$validation_next_cache" "$validation_build_contexts" \
  || wd_die "a TypeScript context-build change did not identify gate machinery"
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_next_cache" "$validation_build_contexts" \
  || wd_die "a TypeScript context-build change did not force full validation"
printf 'groups\n' >"$validation_repo/deploy/typescript-test-groups.sh"
git -C "$validation_repo" add deploy/typescript-test-groups.sh
git -C "$validation_repo" commit --quiet -m typescript-test-groups
validation_test_groups=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_changes_typescript_gate "$validation_repo" \
  "$validation_build_contexts" "$validation_test_groups" \
  || wd_die "a TypeScript test-group change did not identify gate machinery"
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_build_contexts" "$validation_test_groups" \
  || wd_die "a TypeScript test-group change did not force full validation"
printf 'bundle\n' >"$validation_repo/deploy/commerce-release-bundle.sh"
git -C "$validation_repo" add deploy/commerce-release-bundle.sh
git -C "$validation_repo" commit --quiet -m commerce-release-bundle
validation_release_bundle=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_changes_typescript_gate "$validation_repo" \
  "$validation_test_groups" "$validation_release_bundle" \
  || wd_die "a commerce release-bundle change did not identify gate machinery"
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_test_groups" "$validation_release_bundle" \
  || wd_die "a commerce release-bundle change did not force full validation"
printf 'digest\n' >"$validation_repo/deploy/release-tree-digest.mjs"
git -C "$validation_repo" add deploy/release-tree-digest.mjs
git -C "$validation_repo" commit --quiet -m release-tree-digest
validation_release_digest=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_changes_typescript_gate "$validation_repo" \
  "$validation_release_bundle" "$validation_release_digest" \
  || wd_die "a release-tree digest change did not identify gate machinery"
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_release_bundle" "$validation_release_digest" \
  || wd_die "a release-tree digest change did not force full validation"
git -C "$validation_repo" rm --quiet apps/example/index.ts
git -C "$validation_repo" commit --quiet -m typescript-deletion
validation_typescript_deletion=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_requires_full_typescript_scope "$validation_repo" \
  "$validation_release_digest" "$validation_typescript_deletion" \
  || wd_die "a deleted TypeScript workspace path did not force full validation"

# Runtime build contexts remain independently selectable, while a full/unknown TypeScript scope
# always produces the canonical complete list.
mkdir -p "$validation_repo/apps/web"
printf 'web\n' >"$validation_repo/apps/web/page.ts"
git -C "$validation_repo" add apps/web/page.ts
git -C "$validation_repo" commit --quiet -m web-context
validation_web_context=$(git -C "$validation_repo" rev-parse HEAD)
[[ $(wd_typescript_components_for_range "$validation_repo" \
  "$validation_typescript_deletion" "$validation_web_context" 0) == web ]] \
  || wd_die "a web-only range selected unrelated runtime contexts"
mkdir -p "$validation_repo/apps/admin"
printf 'admin\n' >"$validation_repo/apps/admin/page.ts"
git -C "$validation_repo" add apps/admin/page.ts
git -C "$validation_repo" commit --quiet -m admin-context
validation_admin_context=$(git -C "$validation_repo" rev-parse HEAD)
[[ $(wd_typescript_components_for_range "$validation_repo" \
  "$validation_web_context" "$validation_admin_context" 0) == admin ]] \
  || wd_die "an admin-only range selected unrelated runtime contexts"
mkdir -p "$validation_repo/apps/devbot"
printf 'devbot\n' >"$validation_repo/apps/devbot/main.ts"
git -C "$validation_repo" add apps/devbot/main.ts
git -C "$validation_repo" commit --quiet -m devbot-context
validation_devbot_context=$(git -C "$validation_repo" rev-parse HEAD)
[[ $(wd_typescript_components_for_range "$validation_repo" \
  "$validation_admin_context" "$validation_devbot_context" 0) == devbot ]] \
  || wd_die "a devbot-only range selected unrelated runtime contexts"
mkdir -p "$validation_repo/packages/contracts"
printf 'contracts\n' >"$validation_repo/packages/contracts/index.ts"
git -C "$validation_repo" add packages/contracts/index.ts
git -C "$validation_repo" commit --quiet -m shared-context
validation_shared_context=$(git -C "$validation_repo" rev-parse HEAD)
[[ $(wd_typescript_components_for_range "$validation_repo" \
  "$validation_devbot_context" "$validation_shared_context" 0) == commerce,sales,openkeys ]] \
  || wd_die "the contracts package did not select every host consumer context"
[[ $(wd_typescript_components_for_range "$validation_repo" \
  "$validation_devbot_context" "$validation_shared_context" 1) == commerce,sales,openkeys,web,admin,devbot ]] \
  || wd_die "full TypeScript validation did not select every runtime context"

# The canonical-list validator accepts any strictly rank-ordered subset and rejects duplicates,
# unknown components, wrong order, and empty fields without enumerating combinations.
for canonical_list in devbot commerce,devbot web,admin,devbot \
  commerce,sales,openkeys,web,admin,devbot; do
  wd_typescript_component_list_is_canonical "$canonical_list" \
    || wd_die "a canonical TypeScript component list was rejected: $canonical_list"
done
for malformed_list in '' devbot,admin admin,devbot,admin devbot, ,devbot commerce,unknown; do
  if wd_typescript_component_list_is_canonical "$malformed_list"; then
    wd_die "a malformed TypeScript component list was accepted: $malformed_list"
  fi
done

# A deleted component file still requires that component's lane. A rename is deliberately exposed
# as an old-path deletion plus a new-path addition, so moving code cannot escape its former owner.
mkdir -p "$validation_repo/crates"
printf 'removed\n' >"$validation_repo/crates/deleted.rs"
git -C "$validation_repo" add crates/deleted.rs
git -C "$validation_repo" commit --quiet -m deletion-base
validation_deletion_base=$(git -C "$validation_repo" rev-parse HEAD)
git -C "$validation_repo" rm --quiet crates/deleted.rs
git -C "$validation_repo" commit --quiet -m deletion
validation_deletion=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_has_class "$validation_repo" "$validation_deletion_base" "$validation_deletion" \
  wd_path_is_engine || wd_die "a deleted engine path did not require Rust validation"

mkdir -p "$validation_repo/crates"
printf 'renamed\n' >"$validation_repo/crates/renamed.rs"
git -C "$validation_repo" add crates/renamed.rs
git -C "$validation_repo" commit --quiet -m rename-base
validation_rename_base=$(git -C "$validation_repo" rev-parse HEAD)
git -C "$validation_repo" mv crates/renamed.rs docs/renamed.md
git -C "$validation_repo" commit --quiet -m rename
validation_rename=$(git -C "$validation_repo" rev-parse HEAD)
wd_range_has_class "$validation_repo" "$validation_rename_base" "$validation_rename" \
  wd_path_is_engine || wd_die "a renamed-away engine path did not require Rust validation"
wd_range_has_class "$validation_repo" "$validation_rename_base" "$validation_rename" \
  wd_path_is_validation_neutral || wd_die "a renamed documentation destination was not classified"

# Both the gate and release promoter must hash the same mandatory runtime entrypoints.
artifact_tree="$TEMP/artifact-tree"
artifact_paths=(
  apps/api/dist/main.js
  apps/worker/dist/main.js
  apps/content-studio/.next/BUILD_ID
  apps/sales-api/dist/main.js
  apps/sales-web/.next/BUILD_ID
  apps/openkeys/.next/BUILD_ID
  apps/web/.next/BUILD_ID
  apps/admin/.next/BUILD_ID
  apps/devbot/dist/main.js
  packages/db/dist/migrate.js
  packages/sales-db/dist/migrate.js
  packages/openkeys-db/dist/migrate.js
)
for artifact_path in "${artifact_paths[@]}"; do
  mkdir -p "$artifact_tree/$(dirname -- "$artifact_path")"
  printf '%s\n' "$artifact_path" >"$artifact_tree/$artifact_path"
done
watchdog_artifact_digest=$(wd_typescript_artifact_digest "$artifact_tree")
deploy_artifact_digest=$(bash -c \
  'source "$1"; tested_typescript_artifact_digest "$2"' _ "$ROOT/deploy/lib.sh" "$artifact_tree")
[[ $watchdog_artifact_digest == "$deploy_artifact_digest" ]] \
  || wd_die "watchdog and release promoter disagree on the TypeScript artifact identity"
for artifact_component in commerce sales openkeys web admin devbot; do
  watchdog_component_digest=$(wd_typescript_component_artifact_digest \
    "$artifact_tree" "$artifact_component")
  deploy_component_digest=$(bash -c \
    'source "$1"; tested_typescript_component_artifact_digest "$2" "$3"' \
    _ "$ROOT/deploy/lib.sh" "$artifact_tree" "$artifact_component")
  [[ $watchdog_component_digest == "$deploy_component_digest" ]] \
    || wd_die "watchdog and release promoter disagree on the $artifact_component artifact identity"
done
printf 'tampered\n' >>"$artifact_tree/apps/api/dist/main.js"
[[ $(wd_typescript_artifact_digest "$artifact_tree") != "$watchdog_artifact_digest" ]] \
  || wd_die "artifact identity did not detect a changed runtime entrypoint"

# Exercise the release-side marker check without requiring root in this hermetic fixture. Production
# still supplies the real stat_owner_uid implementation; only the fixture's owner result is stubbed.
tested_candidate="$TEMP/tested-candidate"
cp -R "$artifact_tree" "$tested_candidate"
git init --quiet "$tested_candidate"
git -C "$tested_candidate" config user.name test
git -C "$tested_candidate" config user.email test@example.invalid
git -C "$tested_candidate" commit --quiet --allow-empty -m candidate
tested_sha=$(git -C "$tested_candidate" rev-parse HEAD)
tested_tree=$(git -C "$tested_candidate" rev-parse 'HEAD^{tree}')
mkdir -p "$tested_candidate/.deploy-artifacts/engine"
mkdir -p "$tested_candidate/.deploy-artifacts/codex"
mkdir -p "$tested_candidate/.deploy-artifacts/commerce-release/apps/api"
mkdir -p "$tested_candidate/deploy"
cp "$ROOT/deploy/release-tree-digest.mjs" "$tested_candidate/deploy/release-tree-digest.mjs"
printf 'tested bundle\n' \
  >"$tested_candidate/.deploy-artifacts/commerce-release/apps/api/main.js"
printf '#!/usr/bin/env bash\nexit 0\n' >"$tested_candidate/.deploy-artifacts/engine/claude-api"
printf '#!/usr/bin/env bash\nexit 0\n' >"$tested_candidate/.deploy-artifacts/engine/authbot"
printf '#!/usr/bin/env bash\nexit 0\n' >"$tested_candidate/.deploy-artifacts/engine/claude-router"
printf '#!/usr/bin/env bash\nexit 0\n' >"$tested_candidate/.deploy-artifacts/codex/codex"
chmod +x "$tested_candidate/.deploy-artifacts/engine/claude-api" \
  "$tested_candidate/.deploy-artifacts/engine/authbot" \
  "$tested_candidate/.deploy-artifacts/engine/claude-router" \
  "$tested_candidate/.deploy-artifacts/codex/codex"
tested_marker="$TEMP/tested-candidate.marker"
{
  printf 'sha=%s\n' "$tested_sha"
  printf 'tree=%s\n' "$tested_tree"
  printf 'typescript_tested=1\n'
  printf 'typescript_full=1\n'
  printf 'typescript_base=%s\n' "$tested_sha"
  printf 'rust_tested=1\n'
  printf 'engine_artifacts=1\n'
  printf 'codex_artifacts=1\n'
  printf 'typescript_artifact_digest=%s\n' "$(wd_typescript_artifact_digest "$tested_candidate")"
  printf 'commerce_release_bundle_sha256=%s\n' \
    "$(wd_commerce_release_bundle_digest "$tested_candidate")"
  printf 'engine_binary_sha256=%s\n' \
    "$(wd_sha256_file "$tested_candidate/.deploy-artifacts/engine/claude-api")"
  printf 'authbot_binary_sha256=%s\n' \
    "$(wd_sha256_file "$tested_candidate/.deploy-artifacts/engine/authbot")"
  printf 'router_binary_sha256=%s\n' \
    "$(wd_sha256_file "$tested_candidate/.deploy-artifacts/engine/claude-router")"
  printf 'codex_binary_sha256=%s\n' \
    "$(wd_sha256_file "$tested_candidate/.deploy-artifacts/codex/codex")"
} >"$tested_marker"
validate_candidate_fixture() {
  bash -c '
    source "$1"
    stat_owner_uid(){ printf "0\n"; }
    validate_tested_candidate "$2" "$3" "$4" 1 1 commerce
  ' _ "$ROOT/deploy/lib.sh" "$tested_candidate" "$tested_marker" "$tested_sha"
}
validate_candidate_fixture || wd_die "release promoter rejected an intact tested candidate"
printf 'post-marker mutation\n' >>"$tested_candidate/apps/api/dist/main.js"
if validate_candidate_fixture >/dev/null 2>&1; then
  wd_die "release promoter accepted a runtime artifact changed after the test marker"
fi

# A component marker must be sufficient on its own: commerce promotion neither requires nor hashes
# artifacts from sales, OpenKeys, or the Vercel-only web app.
component_candidate="$TEMP/component-candidate"
mkdir -p "$component_candidate"
for artifact_path in \
  apps/api/dist/main.js \
  apps/worker/dist/main.js \
  apps/content-studio/.next/BUILD_ID \
  packages/db/dist/migrate.js; do
  mkdir -p "$component_candidate/$(dirname -- "$artifact_path")"
  printf '%s\n' "$artifact_path" >"$component_candidate/$artifact_path"
done
git init --quiet "$component_candidate"
git -C "$component_candidate" config user.name test
git -C "$component_candidate" config user.email test@example.invalid
git -C "$component_candidate" commit --quiet --allow-empty -m component-candidate
mkdir -p "$component_candidate/.deploy-artifacts/commerce-release/apps/api"
mkdir -p "$component_candidate/deploy"
cp "$ROOT/deploy/release-tree-digest.mjs" "$component_candidate/deploy/release-tree-digest.mjs"
printf 'component bundle\n' \
  >"$component_candidate/.deploy-artifacts/commerce-release/apps/api/main.js"
component_sha=$(git -C "$component_candidate" rev-parse HEAD)
component_tree=$(git -C "$component_candidate" rev-parse 'HEAD^{tree}')
component_marker="$TEMP/component-candidate.marker"
{
  printf 'sha=%s\n' "$component_sha"
  printf 'tree=%s\n' "$component_tree"
  printf 'typescript_tested=1\n'
  printf 'typescript_components=commerce\n'
  printf 'typescript_artifact_digest_commerce=%s\n' \
    "$(wd_typescript_component_artifact_digest "$component_candidate" commerce)"
  printf 'commerce_release_bundle_sha256=%s\n' \
    "$(wd_commerce_release_bundle_digest "$component_candidate")"
} >"$component_marker"
bash -c '
  source "$1"
  stat_owner_uid(){ printf "0\n"; }
  validate_tested_candidate "$2" "$3" "$4" 1 0 commerce
' _ "$ROOT/deploy/lib.sh" "$component_candidate" "$component_marker" "$component_sha" \
  || wd_die "release promoter rejected an intact component-scoped candidate"
printf 'tampered\n' >>"$component_candidate/apps/worker/dist/main.js"
if bash -c '
  source "$1"
  stat_owner_uid(){ printf "0\n"; }
  validate_tested_candidate "$2" "$3" "$4" 1 0 commerce
' _ "$ROOT/deploy/lib.sh" "$component_candidate" "$component_marker" "$component_sha" \
  >/dev/null 2>&1; then
  wd_die "release promoter accepted a changed component-scoped artifact"
fi

grep -Fq 'admin.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'api.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'openai.api.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'gemini.api.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'router.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'admin.partners.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'crm.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'content-studio.apitoken.sale {' "$ROOT/deploy/Caddyfile"
grep -Fq 'monitoring.apitoken.sale {' "$ROOT/deploy/Caddyfile"
! grep -Fq 'panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"

# Shared cache affinity must remain host-local, durable enough for cache continuity, and optional
# for engine availability. PostgreSQL continues to own all financial/capacity correctness.
grep -Fq '127.0.0.1:6379:6379' "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq 'redis:7.4.2-alpine@sha256:02419de7eddf55aa5bcf49efb74e88fa8d931b4d77c07eff8a6b2144472b6952' \
  "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq -- '--appendonly' "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq 'everysec' "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq 'set apitoken:healthcheck' "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq "grep -qx 'aof_enabled:1'" "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq "grep -qx 'aof_last_bgrewrite_status:ok'" "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq "grep -qx 'rdb_last_bgsave_status:ok'" "$ROOT/deploy/affinity-redis.compose.yaml"
grep -Fq 'Wants=network-online.target apitoken-affinity-redis.service' \
  "$ROOT/systemd/claude-api-anthropic@.service"
! grep -Fq 'Requires=apitoken-affinity-redis.service' "$ROOT/systemd/claude-api-anthropic@.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-anthropic@.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api.service"
grep -Fq 'CLAUDE_API_PROVIDER=anthropic CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1' \
  "$ROOT/systemd/claude-api-anthropic@.service"
! grep -Fq 'CLAUDE_API_PROVIDER=' "$ROOT/systemd/claude-api@.service"
! grep -Fq 'CLAUDE_API_PROVIDER=' "$ROOT/systemd/claude-api.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-openai.service"
grep -Fq 'CLAUDE_API_PROVIDER=openai CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=8793' \
  "$ROOT/systemd/claude-api-openai.service"
grep -Fq 'CLAUDE_API_INSTANCE_ID=%H:engine:openai' "$ROOT/systemd/claude-api-openai.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-openai@.service"
! grep -Fq 'CLAUDE_API_CODEX_TRANSPORT' "$ROOT/systemd/claude-api-openai@.service" \
  || wd_die 'OpenAI blue-green slots still pin the removed app-server transport'
! grep -Fq 'claude-api-codex-app-servers' "$ROOT/systemd/claude-api-openai@.service" \
  || wd_die 'OpenAI slots still wait on the removed daemon boot barrier'
grep -Fq 'ExecCondition=/usr/bin/test ! -L /srv/claude-api/releases/current/.openai-bluegreen-v1' \
  "$ROOT/systemd/claude-api-openai@.service" \
  || wd_die 'legacy releases can start pre-migration binaries through the shared slot'
grep -Fq 'ExecCondition=/usr/bin/grep -Fxq openai-bluegreen-v1' \
  "$ROOT/systemd/claude-api-openai@.service" \
  || wd_die 'shared release marker contents are not checked by the OpenAI slot'
for removed_unit in systemd/claude-api-codex-app-server@.service \
  systemd/claude-api-codex-app-servers.service \
  systemd/claude-api-codex-app-servers-ready.target \
  systemd/claude-api-codex-app-servers.timer; do
  [[ ! -e $ROOT/$removed_unit ]] \
    || wd_die "app-server unit still exists: $removed_unit"
done
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_PROVIDER=gemini CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=8795' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_INSTANCE_ID=%H:engine:gemini' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_MODELS=gemini-3.1-flash-image,gemini-3.6-flash,gemini-3.5-flash,gemini-3.1-pro-preview,gemini-3.1-flash-lite,gemini-2.5-flash,gemini-2.5-flash-lite' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_ANTIGRAVITY_VERSION=2.2.1' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_NODE_BINARY=/usr/bin/node' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_NODE_VERSION=v24.18.0' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_NODE_SHA256=41a74efb34cbde5c7632cdac0cf8bd1a14d0b8d73dc1e82755014d9a9ce70f5c' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_PROFILES_FILE=/srv/claude-api/data/gemini/profiles.json' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_UPSTREAM=https://daily-cloudcode-pa.sandbox.googleapis.com' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fxq 'ReadOnlyPaths=/srv/claude-api/data/gemini' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-gemini@.service"
grep -Fq 'CLAUDE_API_PROVIDER=gemini CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=%i' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slot template does not pin fixed provider mode and its instance port'
grep -Fq 'CLAUDE_API_INSTANCE_ID=%H:engine:gemini:%i' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slot identities are not process-fenced by port'
grep -Fq 'ExecCondition=/usr/bin/test ! -L /srv/claude-api/releases/current/.gemini-bluegreen-v1' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'legacy releases can start through the Gemini slot template'
grep -Fq 'ExecCondition=/usr/bin/grep -Fxq gemini-bluegreen-v1' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slot capability marker contents are not checked'
grep -Fq 'CLAUDE_API_GEMINI_PROFILES_FILE=/srv/claude-api/data/gemini/profiles.json' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slots do not share the sealed profile roster'
grep -Fxq 'ReadOnlyPaths=/srv/claude-api/data/gemini' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slots can mutate Auth Bot roster state'
legacy_gemini_exec=$(grep -F 'ExecStart=' "$ROOT/systemd/claude-api-gemini.service" \
  | sed -e 's/CLAUDE_API_PORT=8795/CLAUDE_API_PORT=%i/' \
    -e 's/CLAUDE_API_INSTANCE_ID=%H:engine:gemini /CLAUDE_API_INSTANCE_ID=%H:engine:gemini:%i /')
slot_gemini_exec=$(grep -F 'ExecStart=' "$ROOT/systemd/claude-api-gemini@.service")
[[ $slot_gemini_exec == "$legacy_gemini_exec" ]] \
  || wd_die 'Gemini slots drifted from the reviewed roster, catalog, upstream, or wire identity pins'
grep -Fq '/srv/claude-api/data/gemini/credentials' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'claude-api-openai.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'claude-api-gemini.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'claude-api-gemini@.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq '/usr/bin/systemctl restart claude-api-openai.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy"
grep -Fq '/usr/bin/systemctl --no-block stop claude-api-openai@[0-9]*.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "OpenAI blue-green cannot retire its old HTTP slot outside the deploy path"
grep -Fq '/usr/bin/systemctl restart claude-api-gemini.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy"
grep -Fq '/usr/bin/systemctl --no-block stop claude-api-gemini.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "first Gemini slot cutover cannot retire the legacy singleton asynchronously"
grep -Fq '/usr/bin/systemctl --no-block stop claude-api-gemini@[0-9]*.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "Gemini blue-green cannot retire its old slot outside the deploy path"
grep -Fq '/usr/bin/systemctl restart claude-api.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "combined bridge recovery restart is denied by sudo policy"
grep -Fq '/usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "combined bridge pre-drain is denied by sudo policy"
grep -Fq 'systemctl_command kill --kill-whom=main -s SIGUSR1 "$ACTIVE_UNIT"' \
  "$ROOT/deploy/engine-bluegreen.sh"
grep -Fq 'provider-runtime-v1 >"$ENGINE_STAGE/.provider-runtime-v1"' "$ROOT/deploy/deploy.sh" \
  || wd_die "engine releases do not record fixed-provider capability"
grep -Fq 'openai-bluegreen-v1 >"$ENGINE_STAGE/.openai-bluegreen-v1"' "$ROOT/deploy/deploy.sh" \
  || wd_die "engine releases do not record OpenAI blue-green capability"
grep -Fq 'gemini-bluegreen-v1 >"$ENGINE_STAGE/.gemini-bluegreen-v1"' "$ROOT/deploy/deploy.sh" \
  || wd_die "engine releases do not record Gemini blue-green capability"
grep -Fq '[[ -f "$directory/.gemini-bluegreen-v1" && ! -L "$directory/.gemini-bluegreen-v1" ]]' \
  "$ROOT/deploy/deploy.sh" \
  || wd_die "engine staging does not reject an unsafe Gemini blue-green capability marker"
grep -Fq '[[ $(<"$directory/.gemini-bluegreen-v1") == gemini-bluegreen-v1 ]]' \
  "$ROOT/deploy/deploy.sh" \
  || wd_die "engine staging does not validate Gemini blue-green marker contents"
! grep -Fq 'CLAUDE_API_CODEX_ENABLED=1 is required when CLAUDE_API_PROVIDER=openai' \
  "$ROOT/crates/server/src/config.rs" \
  || wd_die "disabled fixed OpenAI mode cannot serve a stable kill-switch envelope"
grep -Fq '/usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api-anthropic@8787.service' \
  "$ROOT/deploy/install-sudoers.sh"
grep -Fxq 'd /run/apitoken 0755 root root -' "$ROOT/systemd/apitoken-tmpfiles.conf"
! grep -Fq 'codex-home.lock' "$ROOT/systemd/apitoken-tmpfiles.conf" \
  || wd_die 'the removed Codex ownership lock still has a runtime entry'
! grep -Fq 'codex-app-server' "$ROOT/systemd/apitoken-tmpfiles.conf" \
  || wd_die 'removed Codex app-server runtime directories still exist'
for codex_owner_unit in systemd/claude-api.service systemd/claude-api@.service \
  systemd/claude-api-openai.service; do
  ! grep -Fq 'codex-home.lock' "$ROOT/$codex_owner_unit" \
    || wd_die "removed Codex ownership lock still referenced in $codex_owner_unit"
  grep -Fq 'ReadWritePaths=/srv/claude-api/data' "$ROOT/$codex_owner_unit" \
    || wd_die "provider data root is not writable in $codex_owner_unit"
done
grep -Fq 'systemd-tmpfiles --create "$TARGET"' "$ROOT/deploy/install-tmpfiles.sh"
grep -Fq 'CLAUDE_API_AFFINITY_SECRET' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'apitoken-affinity-redis.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis' \
  "$ROOT/deploy/install-watchdog.sh"
grep -Fq "[[ ! -L /var/lib/apitoken/affinity-redis ]]" "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'if (( REDIS_RESTART_REQUIRED )); then' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'Redis activation is not fenced by an exact definition change'
[[ $(grep -Fc 'systemctl restart apitoken-affinity-redis.service' \
  "$ROOT/deploy/install-watchdog.sh") -eq 1 ]] \
  || wd_die 'Redis has an unconditional or duplicate restart path'
grep -Fq 'cmp -s "$ROOT/deploy/affinity-redis.compose.yaml" "$redis_compose_target"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'Redis compose changes do not request a restart'
grep -Fq 'cmp -s "$ROOT/systemd/$unit" "/etc/systemd/system/$unit"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'Redis unit changes do not request a restart'
grep -Fxq 'vm.overcommit_memory = 1' "$ROOT/systemd/sysctl-apitoken-redis.conf" \
  || wd_die 'Redis overcommit policy is not pinned'
grep -Fq '"$SYSCTL" --load "$TARGET"' "$ROOT/deploy/install-sysctl.sh" \
  || wd_die 'Redis sysctl installer does not apply the persistent policy immediately'
# Exercise the installer's real activation fence with a fake systemctl. Unrelated full/systemd
# transactions must preserve the live container, while an exact Redis definition change performs
# one restart after keeping the unit enabled.
eval "$(sed -n '/^activate_redis_definition()/,/^}/p' "$ROOT/deploy/install-watchdog.sh")"
redis_systemctl_log="$TEMP/redis-systemctl.log"
systemctl() { printf '%s\n' "$*" >>"$redis_systemctl_log"; }
REDIS_RESTART_REQUIRED=0
activate_redis_definition >/dev/null
[[ $(<"$redis_systemctl_log") == 'enable apitoken-affinity-redis.service' ]] \
  || wd_die 'unchanged infrastructure restarted Redis'
: >"$redis_systemctl_log"
REDIS_RESTART_REQUIRED=1
activate_redis_definition >/dev/null
[[ $(<"$redis_systemctl_log") == $'enable apitoken-affinity-redis.service\nrestart apitoken-affinity-redis.service' ]] \
  || wd_die 'changed Redis definitions did not enable and restart exactly once'
unset -f systemctl activate_redis_definition
! grep -Fq 'partners.panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"
! grep -Fq 'crm.panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'import managed_admin_auth' "$ROOT/deploy/Caddyfile") -ge 5 ]]
grep -Fq 'forward_auth 127.0.0.1:8791' "$ROOT/deploy/Caddyfile"
grep -Fq 'order request_header before forward_auth' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up Host 127.0.0.1:8791' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-Admin-Key "<ADMIN_AUTH_KEY_PLACEHOLDER>"' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-Admin-Domain {http.request.host}' "$ROOT/deploy/Caddyfile"
! grep -Fqi 'X-Apitoken-Api-Plane' "$ROOT/deploy/Caddyfile"
# Each backend snippet is imported exactly once — by its per-provider vhost. Since the stage-1b
# cutover the unified router.apitoken.sale vhost proxies to the claude-router process instead of
# importing plane backends (docs/engine/UNIFIED_ROUTER.md).
[[ $(grep -Fc 'import openai_engine_backend' "$ROOT/deploy/Caddyfile") == 1 ]]
[[ $(grep -Fc 'import gemini_engine_backend' "$ROOT/deploy/Caddyfile") == 1 ]]
grep -Fq 'reverse_proxy 127.0.0.1:8792' "$ROOT/deploy/Caddyfile"
grep -Fq 'http://127.0.0.1:8792 {' "$ROOT/deploy/Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8793 127.0.0.1:8797 {' "$ROOT/deploy/Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8794' "$ROOT/deploy/Caddyfile"
grep -Fq '@admin_gemini_data path /gemini-subs' "$ROOT/deploy/Caddyfile"
grep -Fq 'http://127.0.0.1:8794 {' "$ROOT/deploy/Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8795 127.0.0.1:8799 {' "$ROOT/deploy/Caddyfile" \
  || wd_die 'stable Gemini origin does not expose both blue-green slots'
gemini_stable_origin=$(sed -n '/^http:\/\/127\.0\.0\.1:8794 {$/,/^}$/p' "$ROOT/deploy/Caddyfile")
grep -Fq 'lb_policy first' <<<"$gemini_stable_origin" \
  || wd_die 'Gemini slots can round-robin the same OAuth roster during overlap'
[[ $(grep -Fc 'health_uri /ready' "$ROOT/deploy/Caddyfile") -ge 3 ]]
! grep -Fq 'health_method POST' "$ROOT/deploy/Caddyfile"
! grep -Fq 'health_status 4xx' "$ROOT/deploy/Caddyfile"
! grep -Fq 'health_body invalid_request_error' "$ROOT/deploy/Caddyfile"
grep -Fq 'OpenAI hostname smoke failed; restored' "$ROOT/deploy/install-caddy.sh" \
  || wd_die "Caddy installation can commit a syntactically valid but misrouted OpenAI hostname"
grep -Fq -- "-d '{}'" "$ROOT/deploy/install-caddy.sh" \
  || wd_die "Caddy smoke can execute a real OpenAI provider turn"
grep -Fq -- "-d '{}'" "$ROOT/deploy/watchdog.sh" \
  || wd_die "final OpenAI verification can execute a real provider turn"
grep -Fq 'systemctl restart caddy' "$ROOT/deploy/install-caddy.sh" \
  || wd_die "Caddy rollback cannot recover from an admin reload failure"
grep -Fq 'mv -f -- "$tmp" "$LIVE"' "$ROOT/deploy/install-caddy.sh" \
  || wd_die "Caddy candidate publication is not an atomic same-directory rename"
grep -Fq 'mv -f -- "$rollback_tmp" "$LIVE"' "$ROOT/deploy/install-caddy.sh" \
  || wd_die "Caddy rollback publication is not an atomic same-directory rename"
! grep -Fq 'caddy reload --adapter caddyfile --config "$LIVE" || true' \
  "$ROOT/deploy/install-caddy.sh" \
  || wd_die "Caddy rollback reload failures are silently ignored"
claude_api_vhost=$(sed -n '/^api\.apitoken\.sale {$/,/^openai\.api\.apitoken\.sale {$/p' \
  "$ROOT/deploy/Caddyfile")
openai_api_vhost=$(sed -n '/^openai\.api\.apitoken\.sale {$/,/^gemini\.api\.apitoken\.sale {$/p' \
  "$ROOT/deploy/Caddyfile")
grep -Fq 'import engine_backend' <<<"$claude_api_vhost"
! grep -Fq 'openai_engine_backend' <<<"$claude_api_vhost"
grep -Fq 'import openai_engine_backend' <<<"$openai_api_vhost"
[[ $(grep -Fc 'encode zstd gzip {' <<<"$openai_api_vhost") == 1 ]] \
  || wd_die 'OpenAI public TLS boundary must have exactly one compression policy'
grep -Fq 'minimum_length 512' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI compression can spend CPU on tiny response bodies'
grep -Fq 'header Content-Type application/json*' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI compression is not restricted to complete JSON documents'
! grep -Fq 'text/event-stream' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI compression matcher can buffer SSE lifecycle frames'
# Unified stage-1b cutover: the vhost terminates TLS and forwards the whole public contract —
# now including the aggregated /v1/models* — to the claude-router singleton on loopback 8798,
# which owns lane routing, the catalog and lane-shaped errors. No lane may gain compression.
router_vhost=$(sed -n '/^router\.apitoken\.sale {$/,/^}$/p' "$ROOT/deploy/Caddyfile")
grep -Fq '@public_core path /v1/messages* /v1/responses* /v1/chat/completions /v1/models* /v1beta/* /health /balance' \
  <<<"$router_vhost" \
  || wd_die 'unified router must forward exactly the documented public contract'
grep -Fq 'reverse_proxy 127.0.0.1:8798' <<<"$router_vhost" \
  || wd_die 'unified router must proxy to the claude-router loopback origin'
[[ $(grep -Fc 'reverse_proxy' <<<"$router_vhost") == 1 ]] \
  || wd_die 'unified router must have exactly one upstream: the router process'
! grep -Eq '^[[:space:]]*import ' <<<"$router_vhost" \
  || wd_die 'unified router must not import plane backends after the stage-1b cutover'
! grep -Eq '^[[:space:]]*encode ' <<<"$router_vhost" \
  || wd_die 'unified router must not compress any lane (SSE buffering risk)'
grep -Fq 'respond 404' <<<"$router_vhost" \
  || wd_die 'unified router must 404 every path outside the public contract'
grep -Fq 'targets: ["https://router.apitoken.sale/health"]' \
  "$ROOT/observability/prometheus/prometheus.yml" \
  || wd_die 'unified router public endpoint lost its blackbox probe'
grep -Fq 'import gemini_engine_backend' "$ROOT/deploy/Caddyfile"
grep -Fq '@oauth_callback path /oauth/callback' "$ROOT/deploy/Caddyfile"
grep -Fq 'log_skip @oauth_callback' "$ROOT/deploy/Caddyfile"
grep -Fq 'handle @oauth_callback {' "$ROOT/deploy/Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8796' "$ROOT/deploy/Caddyfile"
grep -Fq -- '--resolve openai.api.apitoken.sale:443:127.0.0.1' "$ROOT/deploy/watchdog.sh"
grep -Fq 'https://openai.api.apitoken.sale/v1/responses' "$ROOT/deploy/watchdog.sh"
grep -Fq 'https://gemini.api.apitoken.sale/v1beta/models/gemini-provider-probe:generateContent' \
  "$ROOT/deploy/watchdog.sh"
! grep -Fq -- "-H 'X-Apitoken-Api-Plane: openai'" "$ROOT/deploy/watchdog.sh"
grep -Fq '@commerce_admin path /admin/*' "$ROOT/deploy/Caddyfile"
grep -Fq 'handle_path /openkeys-admin/*' "$ROOT/deploy/Caddyfile"
grep -Fq 'handle /api/internal/*' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-OpenKeys-Control-Key "<OPENKEYS_INTERNAL_KEY_PLACEHOLDER>"' "$ROOT/deploy/Caddyfile"
grep -Fq 'handle_path /partner-admin/*' "$ROOT/deploy/Caddyfile"
grep -Fq 'copy_headers X-Admin-Actor X-Admin-Account-Id' "$ROOT/deploy/Caddyfile"
grep -Fq 'encode zstd gzip' "$ROOT/deploy/Caddyfile"
grep -Fq 'Permissions-Policy "camera=(), microphone=(), geolocation=(), payment=(), usb=()"' \
  "$ROOT/deploy/Caddyfile"
# The admin UI is the standalone Next.js app on :3700 (own deploy lane) and sends its own
# security headers via next.config.ts; the vhost must not pin a CSP for the retired embedded
# panel, and no hand-maintained inline script hash may exist anywhere.
! grep -Fq 'Content-Security-Policy' "$ROOT/deploy/Caddyfile" \
  || wd_die 'admin vhost must not carry the retired embedded-panel CSP'
! grep -Fq "script-src 'sha256-" "$ROOT/deploy/Caddyfile" \
  || wd_die 'admin CSP must not depend on a hand-maintained inline script hash'
! grep -Fq 'admin-panel' "$ROOT/deploy/Caddyfile" \
  || wd_die 'admin vhost must not route the retired embedded panel'
! grep -Fq 'admin_panel' "$ROOT/crates/server/src/http.rs" \
  || wd_die 'engine must not serve the retired embedded panel'
grep -Fq 'reverse_proxy 127.0.0.1:3700' "$ROOT/deploy/Caddyfile" \
  || wd_die 'admin vhost must proxy the UI to the standalone Next.js app on :3700'
! grep -Fq 'header_up x-admin-actor' "$ROOT/deploy/Caddyfile"
! grep -Fq 'header_up x-admin-account-id' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'reverse_proxy 127.0.0.1:3000 127.0.0.1:3001' "$ROOT/deploy/Caddyfile") == 1 ]]
[[ $(grep -Fc 'reverse_proxy 127.0.0.1:8787 127.0.0.1:8788' "$ROOT/deploy/Caddyfile") == 1 ]]
! grep -Fq 'unhealthy_status 503' "$ROOT/deploy/Caddyfile"
! grep -Fq 'fail_duration' "$ROOT/deploy/Caddyfile"
! grep -Fq 'max_fails' "$ROOT/deploy/Caddyfile"
grep -Fq 'request>headers>X-Admin-Key replace REDACTED' "$ROOT/deploy/Caddyfile"
# Both slots of each pair stay listed as upstreams while exactly one runs, so the active health
# checker fails against a deliberately stopped address once every two seconds forever. Excluding
# that logger is what keeps the journal and the Grafana error panel readable; losing the line
# silently reintroduces roughly one junk entry per second.
[[ $(grep -Fc 'exclude http.handlers.reverse_proxy.health_checker.active' "$ROOT/deploy/Caddyfile") == 1 ]]
grep -Fq 'COMMERCE_BASE_URL=http://127.0.0.1:8791' "$ROOT/apps/sales-api/.env.example"
grep -Fq 'COMMERCE_BALANCER_URL=${COMMERCE_BALANCER_URL:-http://127.0.0.1:8791}' "$ROOT/deploy/sales-deploy.sh"
grep -Fq 'configure_commerce_balancer' "$ROOT/deploy/sales-deploy.sh"
grep -Fq 'COMMERCE_BALANCER_READY_URL=${COMMERCE_BALANCER_READY_URL:-http://127.0.0.1:8791/v1/ready}' "$ROOT/deploy/api-bluegreen.sh"
[[ $(grep -Fc 'balancer_is_ready' "$ROOT/deploy/api-bluegreen.sh") -ge 6 ]]
# Each concurrent candidate owns a stable disposable-database slot. All three loopback ports must
# stay below the kernel ephemeral range, or unrelated outbound traffic can intermittently take one.
test_db_base_port=$(sed -n 's/^BASE_PORT=${WATCHDOG_POSTGRES_PORT:-\([0-9]*\)}$/\1/p' \
  "$ROOT/deploy/watchdog-test-db.sh")
[[ -n $test_db_base_port ]] \
  || wd_die "could not read the disposable test database base port"
for test_db_slot in 0 1 2; do
  test_db_port=$((test_db_base_port + test_db_slot))
  (( test_db_port < 32768 )) \
    || wd_die "test database port $test_db_port is inside the ephemeral range and will collide"
done
grep -Fq 'SLOT=${2:-0}' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'test database helper does not default production to slot zero'
grep -Fq 'NAME=apitoken-watchdog-postgres-$SLOT' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'parallel test database slots do not have distinct container names'
grep -Fq -- '--label "apitoken.watchdog.slot=$SLOT"' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'test database ownership is not fenced by slot'

# The authbot produces the subscriptions the engine serves from, so the production watchdog builds
# it once beside the tested engine and the release controller only promotes those exact binaries.
grep -Fq 'cargo build --locked --release -p claude-api -p authbot -p claude-router' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the candidate gate does not build all production engine artifacts"
grep -Fq '"$TESTED_CANDIDATE/.deploy-artifacts/engine/authbot"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the release controller does not promote the tested authbot"
grep -Fq '"$ENGINE_STAGE/authbot"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the authbot binary is not installed into the engine release"
grep -Fq 'staged authbot binary is missing' "$ROOT/deploy/deploy.sh" \
  || wd_die "a release without an authbot binary must fail closed"
grep -Fq 'ExecStart=/usr/bin/env CLAUDE_BIN=/run/claude-authbot/claude AUTH_BOT_CLAUDE_BIN=/run/claude-authbot/claude AUTH_BOT_CODEX_HOMES_DIR=/srv/claude-api/data/codex-staging AUTH_BOT_CODEX_ROSTER_DIR=/srv/claude-api/data/codex /srv/claude-api/releases/current/authbot' \
  "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot unit must override legacy CLI/home paths and run the current release"
grep -Fq 'ProtectHome=true' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot must keep the service user's home hidden"
grep -Fq 'Environment=AUTH_BOT_CLAUDE_BIN=/run/claude-authbot/claude' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the protected authbot still tries to execute Claude below hidden /home"
grep -Fq 'env_opt("AUTH_BOT_CLAUDE_BIN")' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die "legacy authbot.env can override the sandbox-safe Claude CLI path"
grep -Fq 'BindReadOnlyPaths=/home/deploy/.local/bin/claude:/run/claude-authbot/claude' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the protected authbot cannot execute the installed Claude CLI from its runtime directory"
grep -Fq 'RuntimeDirectory=claude-authbot' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot has no private mount destination for the Claude CLI"
grep -Fq 'Environment=HOME=/srv/claude-api/data/authbot' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the protected authbot has no writable home for an already-running old binary"
grep -Fq 'EnvironmentFile=/srv/claude-api/data/engine-postgres.env' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot can silently fall back from the engine PostgreSQL authority"
grep -Fq 'AuthorityConfig::Postgres' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die "the authbot registry is not pinned to PostgreSQL"
! grep -Fq 'subscriptions.db' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die "the authbot still has a SQLite subscription-registry fallback"
grep -Fq '/srv/claude-api/data/authbot' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die "the authbot stores Claude account state below the writable production data root"
grep -Fq 'systemctl try-restart claude-authbot.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "an updated authbot sandbox is not adopted by the running service"
grep -Fq 'claude-authbot.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "the authbot unit is never installed"
grep -Fq '/usr/bin/systemctl restart claude-authbot.service' "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "the deploy user cannot restart the authbot"
# Unchanged authbot binaries preserve live handoffs; changed ones restart and recover persisted state.
grep -Fq 'cmp -s "$exe" "$current/authbot"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the authbot restart must compare the running binary, not release paths"
grep -Fq '$unit failed exact-release verification after restart' "$ROOT/deploy/deploy.sh" \
  || wd_die "a crashed authbot can still produce a green deployment"
! grep -Fq 'deferring adoption until the service is already inactive' "$ROOT/deploy/deploy.sh" \
  || wd_die "changed authbot code can remain undeployed forever"
# Asking for a world-readable unit file under sudo earns a policy denial that is indistinguishable
# from the unit being absent — which is exactly how the first attempt silently skipped the restart.
! grep -Fq 'privileged_command test -f "/etc/systemd/system/$unit"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the authbot unit check must not require sudo"

# The unified router is a third engine artifact: the gate builds it once beside the tested engine,
# the marker pins its digest, the promoter verifies it, and the release controller restarts the
# singleton only on an actual binary change, then requires /ready before reporting green.
grep -Fq -- '-p claude-api -p authbot -p claude-router' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the candidate gate does not build the production router artifact"
grep -Fq 'router_binary_sha256' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the tested marker does not pin the router binary digest"
grep -Fq 'claude-router) digest_key=router_binary_sha256' "$ROOT/deploy/lib.sh" \
  || wd_die "the promoter does not verify the router digest from the marker"
grep -Fq '"$TESTED_CANDIDATE/.deploy-artifacts/engine/claude-router"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the release controller does not promote the tested router"
grep -Fq '"$ENGINE_STAGE/claude-router"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the router binary is not installed into the engine release"
grep -Fq 'staged router binary is missing' "$ROOT/deploy/deploy.sh" \
  || wd_die "a release without a router binary must fail closed"
grep -Fq 'restart_router_if_changed "$ENGINE_RELEASE"' "$ROOT/deploy/deploy.sh" \
  || wd_die "deployment can leave changed router code unadopted"
grep -Fq 'cmp -s "$exe" "$current/claude-router"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the router restart must compare the running binary, not release paths"
grep -Fq 'never became ready on 127.0.0.1:8798' "$ROOT/deploy/deploy.sh" \
  || wd_die "a router that never answers /ready must fail the deployment"
grep -Fq 'CLAUDE_ROUTER_PORT=8798' "$ROOT/systemd/claude-router.service" \
  || wd_die "the router unit does not pin the deploy-verified loopback port"
grep -Fq 'systemctl try-restart claude-router.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "an updated router unit contract is not adopted by the running service"
grep -Fq 'claude-router.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "the router unit is never installed"
grep -Fq '/usr/bin/systemctl restart claude-router.service' "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "the deploy user cannot restart the router"
grep -Fq 'systemd/claude-router.service' "$ROOT/deploy/watchdog-lib.sh" \
  || wd_die "the router unit definition is not scoped as systemd infrastructure"

# The native Codex provider ships its wire identity inside the tested engine binary: there is no
# sidecar artifact, no isolated build lane and no promotion helper left.
! grep -Fq 'test_codex_lane' "$ROOT/deploy/watchdog.sh" \
  || wd_die "a sidecar Codex build lane survived the native migration"
! grep -Fq 'VALIDATION_CODEX_ARTIFACTS_REQUIRED=1' "$ROOT/deploy/watchdog.sh" \
  || wd_die "Codex tooling changes still request a sidecar production artifact"
! grep -Fq 'CODEX_PROMOTION_HELPER' "$ROOT/deploy/deploy.sh" \
  || wd_die "release controller still promotes a sidecar Codex artifact"
! grep -Fq 'watchdog-codex-promote.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "removed Codex promotion helper is still installed"
! grep -Fq 'codex-promote' "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "deploy user can still invoke a removed Codex promotion helper"
! grep -Fq "require_permitted 'Codex promotion helper'" \
  "$ROOT/deploy/install-sudoers.sh" \
  || wd_die "sudo policy installer still verifies a removed Codex promotion helper"

grep -Fq 'final_verify_admin_data' "$ROOT/deploy/watchdog.sh"
# The admin data check runs immediately after cutover, while the stable listener still round-robins
# the retiring slot. It must require a streak of expected answers rather than accepting the first
# one, and its window must stay well above Caddy's 2s active-health convergence: a one-second
# window quarantined a correct promotion on 2026-07-25.
grep -Fq 'streak >= 3' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the admin data check must require consecutive expected answers, not a single one"
grep -Fq 'for _ in $(seq 1 20); do' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the admin data convergence window must outlast blue-green cutover and health checks"
grep -Fq 'http://127.0.0.1:8790/overview' "$ROOT/deploy/watchdog.sh" \
  || wd_die "the admin data check must probe the engine data route the admin app polls"
for scoped_verifier in final_verify_admin_routing final_verify_monitoring \
  final_verify_codex_surface final_verify_gemini_surface; do
  grep -Fq "$scoped_verifier" "$ROOT/deploy/watchdog.sh" \
    || wd_die "final verification lost scoped check $scoped_verifier"
done
grep -Fq -- "--data-urlencode 'query=claude_api_codex_enabled{provider=\"openai\"}'" \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die "disabled Codex detection is not scoped to the OpenAI provider process"

# OpenAI and Gemini are health-gated two-slot cohorts. Candidate readiness over each shared sealed
# roster is the admission gate; Gemini additionally keeps new routing active/passive in Caddy.
provider_controller="$ROOT/deploy/engine-bluegreen.sh"
old_stop_line=$(grep -nF 'systemctl_command stop "$ACTIVE_UNIT"' "$provider_controller" | head -1 | cut -d: -f1)
openai_start_line=$(grep -nF 'systemctl_command start "$OPENAI_TARGET_UNIT"' "$provider_controller" | head -1 | cut -d: -f1)
openai_enable_line=$(grep -nF 'systemctl_command enable "$OPENAI_TARGET_UNIT"' \
  "$provider_controller" | head -1 | cut -d: -f1)
openai_drain_line=$(grep -nF 'systemctl_command kill --kill-whom=main -s SIGUSR1 "$OPENAI_ACTIVE_UNIT"' \
  "$provider_controller" | head -1 | cut -d: -f1)
openai_async_stop_line=$(grep -nF 'systemctl_command --no-block stop "$OPENAI_ACTIVE_UNIT"' \
  "$provider_controller" | head -1 | cut -d: -f1)
[[ -n $old_stop_line && -n $openai_start_line && $old_stop_line -lt $openai_start_line ]] \
  || wd_die 'OpenAI can start before the old combined engine cgroup has stopped'
[[ -n $openai_start_line && -n $openai_enable_line && -n $openai_drain_line \
    && -n $openai_async_stop_line \
    && $openai_start_line -lt $openai_enable_line \
    && $openai_enable_line -lt $openai_drain_line \
    && $openai_drain_line -lt $openai_async_stop_line ]] \
  || wd_die 'OpenAI does not admit and boot-durable the target before draining the old generation'
! grep -Fq 'CODEX_APP_SERVERS_HELPER' "$provider_controller" \
  || wd_die 'HTTP blue-green still drives a stateful Codex daemon cohort'
! grep -Fq 'CLAUDE_API_CODEX_TRANSPORT' "$provider_controller" \
  || wd_die 'OpenAI slots still pin the removed app-server transport mode'
! grep -Fq 'claude-api-codex-app-servers' "$provider_controller" \
  || wd_die 'provider controller still references the removed daemon units'
! grep -Fq 'CODEX_HOME_MIGRATION_HELPER' "$provider_controller" \
  || wd_die 'provider controller still runs the legacy CODEX_HOME relocation'
grep -Fq 'unit_release_binding_ok engine "$unit" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" anthropic' \
  "$provider_controller" || wd_die "Anthropic slot gate does not prove provider mode"
grep -Fq 'unit_release_binding_ok engine "$OPENAI_LEGACY_UNIT" "$ENGINE_RELEASE_ROOT"' \
  "$provider_controller" || wd_die 'legacy OpenAI rollback gate does not prove the exact release'
grep -Fq 'preserving it without another cutover' "$provider_controller" \
  || wd_die 'same-release OpenAI drift can trigger a needless second slot cutover'
grep -Fq 'OPENAI_TARGET_PREEXISTING=1' "$provider_controller" \
  || wd_die 'same-release recovery can stop the only pre-existing current OpenAI slot'
grep -Fq 'recovery retains the pre-existing current OpenAI target' "$provider_controller" \
  || wd_die 'pre-drain recovery can destroy a previously admitted OpenAI target'
grep -Fq 'wait_openai_retirement_ack "$OPENAI_ACTIVE_UNIT" "$OPENAI_ACTIVE_PORT"' \
  "$provider_controller" \
  || wd_die 'OpenAI async retirement is not acknowledged before ownership commit'
grep -Fq 'openai_slot_retired "$OPENAI_OTHER_UNIT" "$OPENAI_OTHER_PORT"' \
  "$provider_controller" \
  || wd_die 'OpenAI final verification rejects a safely draining old HTTP generation'
gemini_start_line=$(grep -nF 'systemctl_command start "$GEMINI_TARGET_UNIT"' \
  "$provider_controller" | head -1 | cut -d: -f1)
gemini_enable_line=$(grep -nF 'systemctl_command enable "$GEMINI_TARGET_UNIT"' \
  "$provider_controller" | head -1 | cut -d: -f1)
gemini_drain_line=$(grep -nF 'systemctl_command kill --kill-whom=main -s SIGUSR1 "$GEMINI_ACTIVE_UNIT"' \
  "$provider_controller" | head -1 | cut -d: -f1)
gemini_async_stop_line=$(grep -nF 'systemctl_command --no-block stop "$GEMINI_ACTIVE_UNIT"' \
  "$provider_controller" | head -1 | cut -d: -f1)
[[ -n $gemini_start_line && -n $gemini_enable_line && -n $gemini_drain_line \
    && -n $gemini_async_stop_line \
    && $gemini_start_line -lt $gemini_enable_line \
    && $gemini_enable_line -lt $gemini_drain_line \
    && $gemini_drain_line -lt $gemini_async_stop_line ]] \
  || wd_die 'Gemini does not admit and boot-durable the target before draining the old generation'
grep -Fq 'unit_release_binding_ok engine "$GEMINI_LEGACY_UNIT" "$ENGINE_RELEASE_ROOT"' \
  "$provider_controller" || wd_die "legacy Gemini rollback gate does not prove the exact release"
grep -Fq 'unit_release_binding_ok engine "$unit" "$ENGINE_RELEASE_ROOT" "$CURRENT_RELEASE" gemini' \
  "$provider_controller" || wd_die "Gemini slot gate does not prove provider mode"
grep -Fq 'GEMINI_BLUEGREEN_MARKER=.gemini-bluegreen-v1' "$provider_controller" \
  || wd_die 'provider controller cannot distinguish slot-safe Gemini releases from legacy binaries'
grep -Fq '[[ -f "$expected/.gemini-provider-v1" && ! -L "$expected/.gemini-provider-v1"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog accepts a symlinked Gemini provider capability marker'
grep -Fq 'GEMINI_TARGET_PREEXISTING=1' "$provider_controller" \
  || wd_die 'same-release recovery can stop the only pre-existing current Gemini slot'
grep -Fq 'recovery retains the pre-existing current Gemini target' "$provider_controller" \
  || wd_die 'pre-drain recovery can destroy a previously admitted Gemini target'
grep -Fq 'as the release-rollback anchor' "$provider_controller" \
  || wd_die 'failed first Gemini cutover can remove the only alternate-port rollback anchor'
grep -Fq 'wait_gemini_retirement_ack "$GEMINI_ACTIVE_UNIT" "$GEMINI_ACTIVE_PORT"' \
  "$provider_controller" \
  || wd_die 'Gemini async retirement is not acknowledged before commit'
grep -Fq 'gemini_slot_retired "$GEMINI_OTHER_UNIT" "$GEMINI_OTHER_PORT"' \
  "$provider_controller" \
  || wd_die 'Gemini final verification rejects a safely draining old generation'
grep -Fq 'for old_unit in "$LEGACY_UNIT" "$(legacy_slot_unit 8787)" "$(legacy_slot_unit 8788)"' \
  "$provider_controller" || wd_die "active-but-unready engine cgroups can survive the OpenAI handoff"
grep -Fq 'OPENAI_CAPABILITY_MARKER=.openai-bluegreen-v1' "$provider_controller" \
  || wd_die 'provider controller cannot distinguish safe shared releases from legacy binaries'
grep -Fq 'recovery commits verified OpenAI target' "$provider_controller" \
  || wd_die 'post-drain shared recovery can leave the admitted target boot-fragile'
grep -Fq 'systemctl_raw enable "$OPENAI_TARGET_UNIT"' "$provider_controller" \
  || wd_die 'post-drain recovery does not make the admitted OpenAI target boot-durable'
! grep -Fq 'codex-app-servers' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'systemd installation still installs or arms the removed daemon reconciliation'
grep -Fq 'reverse_proxy 127.0.0.1:8793 127.0.0.1:8797' "$ROOT/deploy/Caddyfile" \
  || wd_die 'stable OpenAI origin does not health-balance two HTTP generations'
! grep -Fq 'codex-app-servers.sh' "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user can still invoke the removed daemon reconciler'
grep -Fq 'codex-homes-migrate.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "Codex seal-migration helper is not installed with its controller"
grep -Fq 'CLAUDE_API_CODEX_PROFILES_FILE=/srv/claude-api/data/codex/profiles.json' \
  "$ROOT/systemd/claude-api-openai.service" \
  || wd_die "OpenAI unit does not pin the sealed Codex roster"
! grep -Fq 'CODEX_HOMES' "$ROOT/systemd/claude-api-openai.service" \
  || wd_die "OpenAI unit still scans legacy CODEX_HOME directories"
grep -Fq 'AUTH_BOT_CODEX_ROSTER_DIR=/srv/claude-api/data/codex' \
  "$ROOT/systemd/claude-authbot.service" \
  || wd_die "authbot and OpenAI provider do not share the sealed Codex roster"
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/codex-homes-migrate.sh --apply' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "deploy user cannot invoke the fixed Codex seal-migration helper"
grep -Fq "require_permitted 'legacy Codex home migration'" \
  "$ROOT/deploy/install-sudoers.sh" \
  || wd_die "sudo policy installer does not verify Codex home migration access"
grep -Fq 'codex_credential::CredentialKeyring' \
  "$ROOT/crates/forward/src/codex/config.rs" \
  || wd_die "Codex provider has no sealed credential keyring"
grep -Fq 'chatgpt.com/backend-api/codex' \
  "$ROOT/crates/codex-credential/src/lib.rs" \
  || wd_die "Codex native backend is not pinned in the credential crate"
grep -Fq 'track_background_task' "$ROOT/crates/forward/src/codex/api.rs" \
  || wd_die "detached Codex response streams bypass the shutdown barrier"
grep -Fq 'track_background_task' "$ROOT/crates/forward/src/codex/chat.rs" \
  || wd_die "detached Codex chat streams bypass the shutdown barrier"
grep -Fq 'PROVIDER_CAPABILITY_MARKER=.provider-runtime-v1' "$provider_controller" \
  || wd_die "provider controller can accept a release without fixed-provider rollback support"
grep -Fq 'exit 2' "$provider_controller" \
  || wd_die "post-admission provider failures are not distinguishable for rollback"
grep -Fq 'if (( controller_rc != 0 )); then' "$ROOT/deploy/watchdog.sh" \
  || wd_die "watchdog does not detect provider controller failure"
controller_failure_body=$(sed -n '/if (( controller_rc != 0 )); then/,/^[[:space:]]*fi$/p' \
  "$ROOT/deploy/watchdog.sh")
grep -Fq 'rollback_engine || true' <<<"$controller_failure_body" \
  || wd_die "watchdog does not restore the release after every provider controller failure"
grep -Fq 'codex_credential::CredentialKeyring' \
  "$ROOT/crates/forward/src/codex/config.rs" || wd_die "Codex provider has no sealed credential keyring"
grep -Fq 'CLAUDE_API_CODEX_CREDENTIAL_KEYS' \
  "$ROOT/crates/server/src/config.rs" || wd_die "Codex credential keyring is not wired into the server config"
grep -Fq 'track_background_task' "$ROOT/crates/forward/src/codex/api.rs" \
  || wd_die "detached Codex response streams bypass the shutdown barrier"
grep -Fq 'track_background_task' "$ROOT/crates/forward/src/codex/chat.rs" \
  || wd_die "detached Codex chat streams bypass the shutdown barrier"
grep -Fq '$unit runs a different binary; restarting onto the tested release' \
  "$ROOT/deploy/deploy.sh" \
  || wd_die "deployment can leave changed authbot code unadopted"
grep -Fq 'recover_interrupted_handoffs' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die "an authbot code restart can strand sellers in a dead in-memory OAuth session"
for retained_engine_unit in claude-api-openai.service claude-api-openai@8793.service \
  claude-api-openai@8797.service claude-api-gemini.service \
  claude-api-gemini@8795.service claude-api-gemini@8799.service claude-authbot.service \
  claude-router.service; do
  grep -Fq "$retained_engine_unit" "$ROOT/deploy/watchdog.sh" \
    || wd_die "release retention can unlink the executable backing $retained_engine_unit"
done
grep -Fq 'shutdown_until(shutdown_deadline)' "$ROOT/crates/server/src/main.rs" \
  || wd_die "Codex shutdown is not bounded by the server drain deadline"
grep -Fq 'self.abort_active_turns();' "$ROOT/crates/forward/src/codex/mod.rs" \
  || wd_die "Codex shutdown cannot cancel turns left at the drain deadline"
grep -Fq 'self.abort_active_streams();' "$ROOT/crates/forward/src/gemini/pool.rs" \
  || wd_die "Gemini shutdown cannot settle and cancel streams left at the drain deadline"
grep -Fq 'self.detached.wait_idle().await;' "$ROOT/crates/forward/src/billing.rs" \
  || wd_die "billing flush can overtake a backpressured detached settlement"
grep -Fq 'track_detached_work()' "$ROOT/crates/forward/src/meter.rs" \
  || wd_die "billing flush can overtake an Anthropic disconnect drain before settlement exists"
[[ $(grep -Fc '.env_clear()' "$ROOT/crates/authbot/src/codex_login.rs") -eq 2 ]] \
  || wd_die "Codex login children inherit unrelated authbot secrets"

# Independent final smokes and no-change GitHub contexts run concurrently, but every worker is
# joined before the overall green verdict. Runtime reconciliation stays ahead of read-only probes
# because it may perform a corrective cutover.
final_verification_contract=(
  'publish_pipeline_start_statuses'
  'publish_unchanged_component_statuses'
  'run_github_status_lane success deploy/migration'
  'run_github_status_lane success deploy/engine'
  'run_github_status_lane success deploy/backend'
  'run_github_status_lane success deploy/sales'
  'run_github_status_lane success deploy/openkeys'
  'run_github_status_lane success deploy/admin'
  'wd_final_verification_plan "$delivery_infra_scope" "$engine_changed"'
  'run_final_verification_lane final_verify_admin_data &'
  'run_final_verification_lane final_verify_admin_routing &'
  'run_final_verification_lane final_verify_monitoring &'
  'run_final_verification_lane final_verify_codex_surface &'
  'run_final_verification_lane final_verify_gemini_surface &'
  'wait "$panel_pid"'
  'wait "$routing_pid"'
  'wait "$monitoring_pid"'
  'wait "$codex_pid"'
  'wait "$gemini_pid"'
  'final verification lanes failed'
)
for final_stage in "${final_verification_contract[@]}"; do
  grep -Fq -- "$final_stage" "$ROOT/deploy/watchdog.sh" \
    || wd_die "scoped final verification lost required stage: $final_stage"
done
final_verification_body=$(sed -n '/^run_final_verification_plan()/,/^}/p' \
  "$ROOT/deploy/watchdog.sh")
runtime_line=$(grep -nF 'reconcile_engine_runtime "$engine_sha"' \
  <<<"$final_verification_body" | cut -d: -f1)
panel_lane_line=$(grep -nF 'run_final_verification_lane final_verify_admin_data &' \
  <<<"$final_verification_body" | cut -d: -f1)
[[ -n $runtime_line && -n $panel_lane_line && $runtime_line -lt $panel_lane_line ]] \
  || wd_die "runtime reconciliation can race the read-only final verification lanes"

# The path-aware candidate gate must keep every lane and its selection/fallback contract. Language
# and static validation run concurrently when selected; unknown paths select every expensive lane.
gate_contract=(
  'pnpm --dir "$candidate" install --frozen-lockfile'
  'NEXT_CACHE_ROOT="$CI_NEXT_CACHE_ROOT"'
  'bash "$candidate/deploy/next-cache.sh" restore "$candidate"'
  'TYPESCRIPT_ARTIFACT_CACHE_ROOT="$CI_TYPESCRIPT_ARTIFACT_CACHE_ROOT"'
  'typescript-build-contexts.sh'
  '"$candidate" "${build_contexts[@]}"'
  'bash "$candidate/deploy/next-cache.sh" save "$candidate"'
  'pnpm --dir "$candidate" typecheck'
  'typescript-scope.mjs'
  '"${filters[@]}"'
  '--fail-if-no-match typecheck'
  'TEST_DATABASE_URL="$dsn" TEST_SALES_DATABASE_URL="$sales_dsn"'
  'TYPESCRIPT_TEST_COMPONENTS="$lane_components"'
  'typescript-test-groups.sh" "$candidate" "${test_packages[@]}"'
  'commerce-release-bundle.sh" "$candidate"'
  'CLAUDE_API_TEST_DATABASE_URL="$engine_dsn"'
  'cargo test --locked --workspace --manifest-path "$candidate/Cargo.toml"'
  'git -C "$SOURCE_REPO" diff --check "$diff_base..$sha"'
  'find "$candidate/deploy" -type f -name '\''*.sh'\'' -print0'
  'bash -n "$shell_file"'
  'run_as_ci bash "$candidate/deploy/lib.test.sh"'
  'run_as_ci bash "$candidate/deploy/watchdog-lib.test.sh"'
  'run_as_ci bash "$candidate/deploy/monitoring-config.test.sh"'
  'run_as_ci bash "$candidate/deploy/next-cache.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-scope.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-build-contexts.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-artifact-cache.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-test-groups.test.sh"'
  'run_as_ci bash "$candidate/deploy/commerce-release-bundle.test.sh"'
  'run_as_ci bash "$candidate/deploy/agent-merge.suite.sh"'
  'status --porcelain --untracked-files=no'
  'run_candidate_lane test_typescript_lane "$candidate" "$dsn" "$sales_dsn" "$openkeys_dsn"'
  'run_candidate_lane test_rust_lane "$candidate" "$engine_dsn" "$engine_artifacts_required" &'
  'run_candidate_lane test_static_lane "$candidate" "$sha" "$static_required" &'
  'wait "$typescript_pid"'
  'wait "$rust_pid"'
  'wait "$codex_pid"'
  'wait "$static_pid"'
  'Static candidate lane failed'
  'wd_infrastructure_install_scope'
  'select_candidate_validation_requirements "$CANDIDATE_SHA"'
  'typescript_tested=%s'
  'typescript_full=%s'
  'typescript_base=%s'
  'rust_tested=%s'
  'static_tested=%s'
  'engine_artifacts=%s'
  'codex_artifacts=%s'
  'validation_plan_format=%s'
  'validation_policy_sha256=%s'
  'validation_plan_sha256=%s'
  'typescript_components=%s'
  'typescript_artifact_digest_commerce=%s'
  'typescript_artifact_digest_sales=%s'
  'typescript_artifact_digest_openkeys=%s'
  'typescript_artifact_digest_web=%s'
  'typescript_artifact_digest_admin=%s'
  'typescript_artifact_digest_devbot=%s'
  'commerce_release_bundle_sha256=%s'
)
for required_stage in "${gate_contract[@]}"; do
  grep -Fq -- "$required_stage" "$ROOT/deploy/watchdog.sh" \
    || wd_die "candidate gate contract lost required stage: $required_stage"
done
if grep -Eq '^bash "\$ROOT/deploy/(sccache-cargo|typescript-build-contexts|typescript-artifact-cache)\.test\.sh"$' \
  "$ROOT/deploy/watchdog-lib.test.sh"; then
  wd_die 'watchdog library suite duplicated a helper regression already owned by the static gate'
fi

# `pnpm -r --if-present test` deliberately tolerates packages with no suite. Keep that tolerance
# explicit: deleting a test script from a covered package, or adding a new workspace package without
# deciding whether it needs a suite, must fail this structural test.
node - "$ROOT" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const root = process.argv[2];
const required = new Set([
  "apps/admin",
  "apps/api",
  "apps/content-studio",
  "apps/devbot",
  "apps/openkeys",
  "apps/sales-api",
  "apps/web",
  "apps/worker",
  "packages/db",
  "packages/engine-client",
  "packages/payments",
  "packages/sales-db",
]);
const explicitlyTestless = new Set([
  "apps/sales-web",
  "packages/contracts",
  // Только схема, пул и раннер миграций: собственной логики, которую можно
  // проверить в отрыве от PostgreSQL, здесь нет. Денежная арифметика OpenKeys
  // живёт в apps/openkeys и покрыта там.
  "packages/openkeys-db",
]);
const discovered = [];
for (const parent of ["apps", "packages"]) {
  for (const entry of fs.readdirSync(path.join(root, parent), { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const relative = `${parent}/${entry.name}`;
    const manifestPath = path.join(root, relative, "package.json");
    if (!fs.existsSync(manifestPath)) continue;
    discovered.push(relative);
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    const testScript = manifest.scripts?.test;
    if (required.has(relative) && (typeof testScript !== "string" || testScript.trim() === "")) {
      throw new Error(`${relative} lost its required test script`);
    }
    if (explicitlyTestless.has(relative) && typeof testScript === "string" && testScript.trim() !== "") {
      throw new Error(`${relative} now has tests; move it into the required-test set`);
    }
    if (!required.has(relative) && !explicitlyTestless.has(relative)) {
      throw new Error(`${relative} has no declared gate-test policy`);
    }
  }
}
for (const relative of [...required, ...explicitlyTestless]) {
  if (!discovered.includes(relative)) {
    throw new Error(`declared workspace package is missing: ${relative}`);
  }
}
NODE

grep -Fq 'CANDIDATE_RETENTION_SECONDS=$((24 * 60 * 60))' "$ROOT/deploy/watchdog.sh"
grep -Fq 'prune_expired_candidates' "$ROOT/deploy/watchdog.sh"

# Retention, retry, and post-admission recovery must stay wired into the watchdog itself.
grep -Fq 'prune_expired_releases "$ENGINE_RELEASE_ROOT" engine' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'engine release retention is not wired into the watchdog cycle'
grep -Fq 'prune_expired_releases "$COMMERCE_RELEASE_ROOT" commerce' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'commerce release retention is not wired into the watchdog cycle'
grep -Fq 'prune_expired_dumps' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'pre-deploy dump retention is not wired into the watchdog cycle'
grep -Fq 'wd_retry 3 5 fetch_source_once' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the GitHub fetch is not retried before failing a cycle'
grep -Fq 'rollback_engine' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'engine post-admission rollback is not wired into the watchdog'
grep -Fq 'rollback_backend' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'backend post-admission rollback is not wired into the watchdog'
# Rollback recovery requires the controller to be installed alongside the blue-green scripts.
grep -Fq 'controller/rollback.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the rollback controller is not installed for automatic recovery'
grep -Fq 'watchdog-retention.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the dump retention helper is not installed'

# The admin lane is installed exactly like the other independent release lanes: its deploy
# controller, its systemd unit, and a least-privilege sudo grant for the fixed script path.
grep -Fq 'controller/admin-deploy.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the admin deploy controller is not installed'
grep -Fq 'apitoken-admin.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the admin panel unit is not installed'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/admin-deploy.sh [0-9a-f]*' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot invoke the fixed admin deploy controller'
grep -Fxq 'WorkingDirectory=/opt/apitoken/admin-releases/current/apps/admin' \
  "$ROOT/systemd/apitoken-admin.service" \
  || wd_die 'the admin unit does not run from the immutable admin release'
grep -Fxq 'ExecStart=/opt/apitoken/admin-releases/current/apps/admin/node_modules/.bin/next start -H 127.0.0.1 -p 3700' \
  "$ROOT/systemd/apitoken-admin.service" \
  || wd_die 'the admin unit does not serve the fixed loopback port 3700'
grep -Fq 'RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK' \
  "$ROOT/systemd/apitoken-admin.service" \
  || wd_die 'the admin unit lost AF_NETLINK and Next.js will crash on uv_interface_addresses'
if grep -Fq 'EnvironmentFile' "$ROOT/systemd/apitoken-admin.service"; then
  wd_die 'the admin panel has no secrets and must not load an environment file'
fi

# The devbot lane is installed exactly like the other independent release lanes: its deploy
# controller, its systemd unit, and a least-privilege sudo grant for the fixed script path. The
# unit and the lane stay deliberately inert until the operator provisions /etc/apitoken/devbot.env.
grep -Fq 'controller/devbot-deploy.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the devbot deploy controller is not installed'
grep -Fq 'apitoken-devbot.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the devbot unit is not installed'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/devbot-deploy.sh [0-9a-f]*' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot invoke the fixed devbot deploy controller'
grep -Fxq 'WorkingDirectory=/opt/apitoken/devbot-releases/current/apps/devbot' \
  "$ROOT/systemd/apitoken-devbot.service" \
  || wd_die 'the devbot unit does not run from the immutable devbot release'
grep -Fxq 'ConditionPathExists=/etc/apitoken/devbot.env' \
  "$ROOT/systemd/apitoken-devbot.service" \
  || wd_die 'the devbot unit must stay inactive until secrets are provisioned'
grep -Fxq 'EnvironmentFile=/etc/apitoken/devbot.env' \
  "$ROOT/systemd/apitoken-devbot.service" \
  || wd_die 'the devbot unit does not load its environment file'
grep -Fxq 'Environment=DEVBOT_PORT=3800' \
  "$ROOT/systemd/apitoken-devbot.service" \
  || wd_die 'the devbot unit does not serve the fixed loopback port 3800'
grep -Fxq 'ProtectSystem=strict' "$ROOT/systemd/apitoken-devbot.service" \
  || wd_die 'the devbot unit lost its filesystem hardening'
grep -Fxq 'ReadWritePaths=/var/lib/apitoken/devbot /var/lib/apitoken/monitoring/textfile' \
  "$ROOT/systemd/apitoken-devbot.service" \
  || wd_die 'the devbot unit cannot write its state directory or heartbeat textfile'
grep -Fq 'DEVBOT_FILE=$STATE_ROOT/devbot.sha' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the watchdog has no devbot baseline state file'
grep -Fq 'run_rollout_lane deploy_devbot' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the devbot rollout lane is not wired into the watchdog'
grep -Fq 'github_deployment_start devbot production-devbot' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the devbot lane does not publish its deployment environment'
grep -Fq 'success deploy/devbot' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the devbot lane does not publish its status context'

# A pre-candidate failure must never quarantine a commit: no SHA has been evaluated at that point.
grep -Fq 'no commit was evaluated' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'pre-candidate failures are not separated from candidate quarantine'

# `wd_die` terminates with `exit`, which does not run an ERR trap. Without EXIT in the trap list,
# every wd_die validation failure would fail closed but silently: no quarantine, no red status.
grep -Eq '^trap fail ERR EXIT INT TERM$' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the watchdog failure handler must be registered on EXIT so wd_die quarantines'
# ...but an EXIT trap also fires on success, so it must return early on a zero status or every
# successful cycle would report itself as a failure.
grep -Fq '(( rc == 0 ))' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the failure handler does not exempt successful exits from quarantine'

# Behavioural check of the exact handler shape, rather than trusting the greps above. Subshells must
# not inherit the trap, or the post-admission rollback path (which runs its verifier in a subshell)
# would quarantine from inside the subshell instead of recovering.
trap_fixture="$TEMP/trap-fixture.sh"
cat >"$trap_fixture" <<'FIXTURE'
#!/usr/bin/env bash
set -euo pipefail
set -E
wd_die(){ printf 'ERROR: %s\n' "$*" >&2; exit 1; }
fail() {
  local rc=$?
  trap - ERR EXIT INT TERM
  if (( rc == 0 )); then return 0; fi
  if [[ -z ${CANDIDATE_SHA:-} ]]; then printf 'PRECANDIDATE\n'; exit "$rc"; fi
  printf 'QUARANTINED\n'
  exit "$rc"
}
trap fail ERR EXIT INT TERM
case "$1" in
  success) CANDIDATE_SHA=abc; printf 'OK\n' ;;
  exit0) CANDIDATE_SHA=abc; printf 'OK\n'; exit 0 ;;
  die_precandidate) wd_die 'missing required file' ;;
  die_candidate) CANDIDATE_SHA=abc; wd_die 'candidate checkout mismatch' ;;
  command_failure) CANDIDATE_SHA=abc; false ;;
  subshell) CANDIDATE_SHA=abc
    verify(){ wd_die 'verification failed'; }
    if ! ( verify ); then printf 'RECOVERED\n'; fi
    printf 'CONTINUED\n' ;;
esac
FIXTURE
chmod +x "$trap_fixture"

trap_case_output() { "$trap_fixture" "$1" 2>/dev/null || true; }
[[ $(trap_case_output success) == OK ]] \
  || wd_die 'a successful cycle must not report a failure'
[[ $(trap_case_output exit0) == OK ]] \
  || wd_die 'a deliberate exit 0 (already processed / quarantined) must not report a failure'
[[ $(trap_case_output die_precandidate) == PRECANDIDATE ]] \
  || wd_die 'wd_die before a candidate is selected must not quarantine'
[[ $(trap_case_output die_candidate) == QUARANTINED ]] \
  || wd_die 'wd_die after a candidate is selected must quarantine it'
[[ $(trap_case_output command_failure) == QUARANTINED ]] \
  || wd_die 'a failing command must still quarantine the candidate'
# The subshell must be caught by its caller so recovery runs; it must NOT quarantine from inside.
subshell_result=$(trap_case_output subshell)
[[ $subshell_result == $'RECOVERED\nCONTINUED' ]] \
  || wd_die "a wd_die inside a condition subshell must not quarantine (got: $subshell_result)"


# Operator visibility: independent controller and application baselines must appear in status.
grep -Fq 'for entry in processed infrastructure engine backend sales openkeys admin rejected pending-migration' \
  "$ROOT/deploy/watchdog-control.sh" \
  || wd_die 'watchdog status omits an independent deployment baseline'

# The least-privilege sudo policy must exist, deny the reporting credential, and be installed by a
# validating installer rather than hand-edited.
sudoers_policy="$ROOT/deploy/sudoers.d/95-apitoken-deploy"
[[ -f $sudoers_policy ]] || wd_die 'the least-privilege sudo policy is missing'
if grep -Eq '^[^#]*NOPASSWD:[[:space:]]*ALL' "$sudoers_policy"; then
  wd_die 'the sudo policy grants unrestricted NOPASSWD:ALL'
fi
grep -Fq 'visudo -c -f' "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not validate before installing'
grep -Fq 'github-watchdog.env' "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not verify the reporting credential stays unreadable'
# The policy must permit re-running its own installer. Without this, removing the unrestricted
# grant is irreversible without console access.
grep -Fq '/usr/local/lib/apitoken-watchdog/install-sudoers.sh' "$sudoers_policy" \
  || wd_die 'the sudo policy is not self-repairable: the installer path is not permitted'
grep -Fq 'APITOKEN_POLICY' "$sudoers_policy" \
  || wd_die 'the policy self-management alias is missing'
grep -Fq '/usr/bin/systemctl enable apitoken-content-studio.service' "$sudoers_policy" \
  || wd_die 'the sudo policy cannot enable the content studio during blue-green cutover'
grep -Fq "require_permitted 'content studio enable'" "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not live-verify content studio enablement'
# Operator tooling must survive the restriction.
grep -Fq '/usr/local/bin/apitoken-watchdog status' "$sudoers_policy" \
  || wd_die 'the sudo policy breaks apitoken-watchdog status'
# Every Cmnd_Alias must be referenced by a grant line, or the privilege is silently not granted.
while IFS= read -r declared_alias; do
  grep -Fq "$declared_alias" <<<"$(grep -E '^deploy ALL=' -A2 "$sudoers_policy")" \
    || wd_die "sudo policy declares unused alias $declared_alias"
done < <(grep -oE '^Cmnd_Alias [A-Z_]+' "$sudoers_policy" | awk '{print $2}')
# The installer and its policy are delivered together with the other operational definitions.
grep -Fq 'install-sudoers.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the sudoers installer is not delivered to the host'
grep -Fq 'apitoken-sudoers-install.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the isolated sudoers installer unit is not delivered to the host'
grep -Fxq 'ExecStart=/usr/local/lib/apitoken-watchdog/install-sudoers.sh' \
  "$ROOT/systemd/apitoken-sudoers-install.service" \
  || wd_die 'the isolated sudoers installer unit does not run the fixed root-owned installer'
grep -Fxq 'ExecStart=/usr/local/lib/apitoken-watchdog/install-tmpfiles.sh' \
  "$ROOT/systemd/apitoken-tmpfiles-install.service" \
  || wd_die 'the isolated tmpfiles installer unit does not run the fixed root-owned installer'
grep -Fxq 'ExecStart=/usr/local/lib/apitoken-watchdog/install-sysctl.sh' \
  "$ROOT/systemd/apitoken-sysctl-install.service" \
  || wd_die 'the isolated sysctl installer unit does not run the fixed root-owned installer'
grep -Fxq 'ReadWritePaths=/etc/sysctl.d' \
  "$ROOT/systemd/apitoken-sysctl-install.service" \
  || wd_die 'the isolated sysctl installer cannot publish its fixed definition'
grep -Fxq 'ReadWritePaths=/etc/tmpfiles.d' \
  "$ROOT/systemd/apitoken-tmpfiles-install.service" \
  || wd_die 'the isolated tmpfiles installer cannot publish its fixed definition'
grep -Fq '/usr/local/lib/apitoken-watchdog/apitoken-tmpfiles.conf' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the tested tmpfiles definition is not staged outside the watchdog namespace'
if grep -Fq '/etc/tmpfiles.d/apitoken.conf' "$ROOT/deploy/install-watchdog.sh"; then
  wd_die 'the watchdog installer still writes tmpfiles inside its read-only namespace'
fi
tmpfiles_reload_line=$(grep -nF 'systemctl daemon-reload' "$ROOT/deploy/install-watchdog.sh" \
  | cut -d: -f1 | head -n 1)
tmpfiles_start_line=$(grep -nF 'systemctl start apitoken-tmpfiles-install.service' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
[[ -n $tmpfiles_reload_line && -n $tmpfiles_start_line \
    && $tmpfiles_start_line -gt $tmpfiles_reload_line ]] \
  || wd_die 'the isolated tmpfiles installer is not started after daemon-reload'
sysctl_start_line=$(grep -nF 'systemctl start apitoken-sysctl-install.service' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
[[ -n $tmpfiles_reload_line && -n $sysctl_start_line \
    && $sysctl_start_line -gt $tmpfiles_reload_line ]] \
  || wd_die 'the isolated sysctl installer is not started after daemon-reload'
if grep -Fxq '/usr/local/lib/apitoken-watchdog/install-sudoers.sh' \
  "$ROOT/deploy/install-watchdog.sh"; then
  wd_die 'the sudoers installer runs inside the watchdog read-only mount namespace'
fi
sudoers_reload_line=$(grep -nF 'systemctl daemon-reload' "$ROOT/deploy/install-watchdog.sh" \
  | cut -d: -f1 | tail -n 1)
sudoers_start_line=$(grep -nF 'systemctl start apitoken-sudoers-install.service' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
[[ -n $sudoers_reload_line && -n $sudoers_start_line && $sudoers_start_line -gt $sudoers_reload_line ]] \
  || wd_die 'the isolated sudoers installer is not started after daemon-reload'
# install-watchdog.sh must never re-add apitoken-ci to the deploy group: that would silently undo
# the isolation fix on the next infrastructure install, and the two installers would fight.
if grep -Eq 'usermod -a -G deploy apitoken-ci' "$ROOT/deploy/install-watchdog.sh"; then
  wd_die 'the watchdog installer re-adds apitoken-ci to the deploy group'
fi
grep -Fq 'gpasswd -d apitoken-ci deploy' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the watchdog installer does not enforce apitoken-ci group isolation'
grep -Fq -- '--controller-only' "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'the sudo policy cannot run the narrow controller transaction'
grep -Fq -- '--caddy-only' "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'the sudo policy cannot run the narrow Caddy transaction'

grep -Fq 'sales-dsn)' "$ROOT/deploy/watchdog-test-db.sh"
grep -Fq 'require_retired_vhost panel.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq 'require_admin_auth_vhost content-studio.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq 'require_admin_auth_vhost monitoring.apitoken.sale' "$ROOT/deploy/watchdog.sh"
grep -Fq "''|000|404|421" "$ROOT/deploy/watchdog.sh"
# Встроенной панели в движке больше нет: UI — standalone Next.js app на :3700 (свой deploy
# lane), а engine отдаёт только data routes. Ни HTML/JS-панели, ни её version marker в
# candidate быть не должно; watchdog проверяет data surface через /overview.
[[ ! -e "$ROOT/crates/server/src/admin-panel.html" ]]
[[ ! -e "$ROOT/crates/server/src/admin-panel.js" ]]
! grep -Fq 'data-admin-panel-version' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog must not verify the retired embedded panel version marker'
[[ ! -e "$ROOT/crates/server/src/panel.html" ]]

render_live="$TEMP/live.Caddyfile"
rendered_once="$TEMP/rendered-once.Caddyfile"
rendered_twice="$TEMP/rendered-twice.Caddyfile"
{
  printf '(panel_admins) {\n\tbasic_auth {\n\t\tadmin $2y$12$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q\n\t}\n}\n'
  printf '(crm_admins) {\n\tbasic_auth {\n\t\tcrm $2a$14$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q\n\t}\n}\n'
  printf 'admin.apitoken.sale {\n'
  printf '\theader_up x-api-key "test-control-secret"\n'
  printf '\theader_up x-admin-key "test-commerce-secret"\n'
  printf '\theader_up x-sales-admin-key "test-sales-secret"\n}\n'
} >"$render_live"
awk -f "$ROOT/deploy/render-caddy.awk" "$render_live" "$ROOT/deploy/Caddyfile" >"$rendered_once"
awk -f "$ROOT/deploy/render-caddy.awk" "$rendered_once" "$ROOT/deploy/Caddyfile" >"$rendered_twice"
for rendered in "$rendered_once" "$rendered_twice"; do
  ! grep -Fq 'basic_auth' "$rendered"
  ! grep -Fq '$2y$' "$rendered"
  grep -Fq 'forward_auth 127.0.0.1:8791' "$rendered"
  [[ $(grep -Fc 'header_up x-api-key "test-control-secret"' "$rendered") == 3 ]]
  [[ $(grep -Fc 'header_up X-OpenKeys-Control-Key "test-control-secret"' "$rendered") == 1 ]]
  [[ $(grep -Fc 'header_up x-admin-key "test-commerce-secret"' "$rendered") == 2 ]]
  [[ $(grep -Fc 'header_up X-Admin-Key "test-commerce-secret"' "$rendered") == 1 ]]
  [[ $(grep -Fc 'header_up x-sales-admin-key "test-sales-secret"' "$rendered") == 1 ]]
  if grep -Eq '<[A-Z_]*PLACEHOLDER>' "$rendered"; then
    wd_die "rendered Caddy fixture retained a secret placeholder"
  fi
done

legacy_export="$TEMP/legacy-admins.json"
awk -f "$ROOT/deploy/export-legacy-admins.awk" "$render_live" >"$legacy_export"
node - "$legacy_export" <<'NODE'
const fs = require("node:fs");
const value = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
if (value.accounts.length !== 2) process.exit(1);
const panel = value.accounts.find((account) => account.username === "admin");
const crm = value.accounts.find((account) => account.username === "crm");
if (!panel || panel.domains.length !== 4 || !panel.domains.includes("admin.apitoken.sale") ||
    !panel.domains.includes("monitoring.apitoken.sale")) process.exit(1);
if (!crm || crm.domains.length !== 1 || crm.domains[0] !== "crm.apitoken.sale") process.exit(1);
NODE

watchdog_writable_paths=$(sed -n 's/^ReadWritePaths=//p' "$ROOT/systemd/apitoken-deploy-watchdog.service")
for required_path in \
  /var/lib/apitoken/watchdog /opt/apitoken /srv/claude-api/releases /run/lock \
  /usr/local/lib/apitoken-watchdog /usr/local/bin /etc/systemd/system /etc/caddy \
  /etc/apitoken /srv/claude-api/data /var/lib/apitoken/monitoring; do
  if ! tr ' ' '\n' <<<"$watchdog_writable_paths" | grep -Fxq "$required_path"; then
    wd_die "watchdog service cannot update required operational path: $required_path"
  fi
done

for cache_environment in \
  'Environment=CARGO_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/cargo' \
  'Environment=XDG_CACHE_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/xdg-cache' \
  'Environment=XDG_CONFIG_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/xdg-config' \
  'Environment=XDG_DATA_HOME=/var/lib/apitoken/watchdog/deploy-build-cache/xdg-data'; do
  grep -Fxq "$cache_environment" "$ROOT/systemd/apitoken-deploy-watchdog.service" \
    || wd_die "watchdog service is missing writable build cache environment: $cache_environment"
done
grep -Fq 'DEPLOY_BUILD_CACHE_ROOT=/var/lib/apitoken/watchdog/deploy-build-cache' \
  "$ROOT/deploy/deploy.sh" || wd_die 'release builder does not pin the writable build cache'
grep -Fq '/var/lib/apitoken/watchdog/deploy-build-cache/cargo' \
  "$ROOT/deploy/install-watchdog.sh" || wd_die 'watchdog installer does not create the release build cache'
grep -Fq 'CARGO_TARGET_DIR="$CI_CARGO_TARGET"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'candidate Rust builds do not share one persistent target cache'
grep -Fq '/var/lib/apitoken/watchdog/ci-home/cargo-target' \
  "$ROOT/deploy/install-watchdog.sh" || wd_die 'watchdog installer does not create the shared CI target'
grep -Fq '/var/lib/apitoken/watchdog/ci-home/next-cache' \
  "$ROOT/deploy/install-watchdog.sh" || wd_die 'watchdog installer does not create the shared Next.js cache'
for shadow_slot in 1 2; do
  grep -Fq "/var/lib/apitoken/watchdog/ci-home/cargo-target-shadow-$shadow_slot" \
    "$ROOT/deploy/install-watchdog.sh" \
    || wd_die "watchdog installer does not create candidate target slot $shadow_slot"
done

# Five-second update detection must not turn retention and deep production probes into five-second
# busy work. Those checks remain on a separate minute maintenance cadence.
grep -Fxq 'OnUnitInactiveSec=5s' "$ROOT/systemd/apitoken-deploy-watchdog.timer" \
  || wd_die 'production update polling is not five seconds'
grep -Fxq 'OnUnitInactiveSec=5s' "$ROOT/systemd/apitoken-candidate-validator.timer" \
  || wd_die 'candidate validation polling is not five seconds'
grep -Fq 'AGENT_MERGE_POLL_S=${AGENT_MERGE_POLL_S:-5}' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'merge/deployment status polling is not five seconds'
grep -Fq 'IDLE_MAINTENANCE_SECONDS=60' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'deep idle maintenance is not decoupled from the fast update poll'

# Narrow concerns compose inside one root transaction. Controller self-updates transfer the
# already-held lock directly into the new root-owned controller, while any systemd concern still
# requires a fresh manager invocation because the current process retains its old mount namespace.
grep -Fq 'wd_atomic_write "$STATE_ROOT/infrastructure.sha" "$SHA"' \
  "$ROOT/deploy/watchdog-infrastructure.sh" \
  || wd_die 'infrastructure transaction does not record its exact SHA'
grep -Fq 'install_controller_definitions' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'watchdog installer has no narrow controller transaction'
grep -Fq 'install_systemd_definitions' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'watchdog installer has no narrow systemd transaction'
grep -Fq 'install_monitoring_definitions' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'watchdog installer has no narrow monitoring transaction'
grep -Fq '"$ROOT/deploy/validation-plan.sh"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'watchdog installer does not install the versioned validation planner'
narrow_dispatch_line=$(grep -nF 'case "$INSTALL_MODE" in' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
bootstrap_line=$(grep -nF "command -v curl >/dev/null" \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
[[ -n $narrow_dispatch_line && -n $bootstrap_line && $narrow_dispatch_line -lt $bootstrap_line ]] \
  || wd_die 'narrow transactions are not fenced before full bootstrap provisioning'
for narrow_option in --controller-only --systemd-only --monitoring-only; do
  grep -Fq -- "$narrow_option" "$ROOT/deploy/install-watchdog.sh" \
    || wd_die "watchdog installer rejects narrow option $narrow_option"
  grep -Fq -- "$narrow_option" "$ROOT/deploy/watchdog-infrastructure.sh" \
    || wd_die "root infrastructure bridge never selects narrow option $narrow_option"
done
grep -Fq 'INSTALL_SCOPE=$(wd_infrastructure_install_scope "$CANDIDATE" "$BASE" "$SHA")' \
  "$ROOT/deploy/watchdog-infrastructure.sh" \
  || wd_die 'fixed root bridge does not derive the exact candidate scope itself'
grep -Fq 'sudo -n "$INFRASTRUCTURE_RUNNER" "$CANDIDATE_SHA"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not delegate one exact-SHA automatic infrastructure transaction'
grep -Fq 'exec "$CONTROLLER_ENTRYPOINT" --resume "$CANDIDATE_SHA"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'controller-only update does not continue in the installed controller'
grep -Fq 'controller resume requires the inherited watchdog lock' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'new controller can resume without the inherited deployment lock'
grep -Fq 'System definitions installed; continuing on next poll' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'full system update does not defer to the refreshed systemd sandbox'
handoff_line=$(grep -nF 'exec "$CONTROLLER_ENTRYPOINT" --resume "$CANDIDATE_SHA"' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
processed_line=$(grep -nF 'wd_atomic_write "$PROCESSED_FILE" "$CANDIDATE_SHA"' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $handoff_line && -n $processed_line && $handoff_line -lt $processed_line ]] \
  || wd_die 'self-update handoff is not fenced before the processed/green path'

# Stateful component rollouts use joined lanes. Engine and commerce remain ordered behind
# their shared deploy lock; bounded sales/OpenKeys/admin roots can make progress concurrently.
rollout_contract=(
  'run_rollout_lane deploy_core_components "$CANDIDATE_SHA" "$engine_changed"'
  'run_rollout_lane deploy_sales "$CANDIDATE_SHA" &'
  'run_rollout_lane deploy_openkeys "$CANDIDATE_SHA" &'
  'run_rollout_lane deploy_admin "$CANDIDATE_SHA" &'
  'wait "$core_pid"'
  'wait "$sales_pid"'
  'wait "$openkeys_pid"'
  'wait "$admin_pid"'
  'component rollout lanes failed'
  'github_phase_failure "$phase"'
)
for required_stage in "${rollout_contract[@]}"; do
  grep -Fq -- "$required_stage" "$ROOT/deploy/watchdog.sh" \
    || wd_die "parallel rollout contract lost required stage: $required_stage"
done
core_body=$(sed -n '/^deploy_core_components()/,/^}/p' "$ROOT/deploy/watchdog.sh")
core_engine_line=$(grep -nF 'deploy_engine "$sha"' <<<"$core_body" | cut -d: -f1)
core_backend_line=$(grep -nF 'deploy_backend "$sha"' <<<"$core_body" | cut -d: -f1)
[[ -n $core_engine_line && -n $core_backend_line && $core_engine_line -lt $core_backend_line ]] \
  || wd_die 'engine and backend escaped their serial shared-lock lane'
backup_line=$(grep -nF 'sudo -n "$BACKUP_RUNNER" "$CANDIDATE_SHA"' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
core_start_line=$(grep -nF \
  'run_rollout_lane deploy_core_components "$CANDIDATE_SHA" "$engine_changed"' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $backup_line && -n $core_start_line && $backup_line -lt $core_start_line ]] \
  || wd_die 'production backup can race an independent database rollout'
grep -Fq 'DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}' \
  "$ROOT/deploy/engine-bluegreen.sh" \
  || wd_die 'engine controller lost the shared deploy lock'
grep -Fq 'DEPLOY_LOCK_FILE=${DEPLOY_LOCK_FILE:-/run/lock/apitoken-deploy.lock}' \
  "$ROOT/deploy/api-bluegreen.sh" \
  || wd_die 'backend controller lost the shared deploy lock'
grep -Fq 'ENGINE_MIGRATION_HELPER=/usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh' \
  "$ROOT/deploy/engine-bluegreen.sh" \
  || wd_die 'engine controller has no fixed schema migration helper'
grep -Fq 'controller/engine-migrate.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'watchdog installer does not install the engine schema migration helper'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh [0-9a-f]*' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot invoke the fixed engine schema migration helper'
grep -Fq '/usr/bin/test -x /usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot probe the fixed engine schema migration helper'

# Core releases promote the frozen candidate, while manual deployments retain their fallback build.
grep -Fq -- '--tested-candidate "$(candidate_for "$sha")"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'core deployments do not consume the tested candidate'
grep -Fq 'reflink-promoting the exact tested compact commerce bundle' "$ROOT/deploy/deploy.sh" \
  || wd_die 'commerce is rebuilt after the candidate gate'
grep -Fq 'TESTED_CANDIDATE/.deploy-artifacts/commerce-release' "$ROOT/deploy/deploy.sh" \
  || wd_die 'commerce promotion copies the complete candidate instead of its compact bundle'
grep -Fq -- '--reflink=auto' "$ROOT/deploy/deploy.sh" \
  || wd_die 'compact commerce promotion lost same-filesystem reflink acceleration'
! grep -Fq 'chmod -R u+w -- "$COMMERCE_STAGE"' "$ROOT/deploy/deploy.sh" \
  || wd_die 'compact commerce promotion still traverses and rewrites every release mode'
grep -Fq 'output: "standalone"' "$ROOT/apps/content-studio/next.config.ts" \
  || wd_die 'Content Studio no longer emits its minimal standalone runtime'
grep -Fxq 'ExecStart=/usr/local/lib/apitoken-watchdog/controller/content-studio-start.sh' \
  "$ROOT/systemd/apitoken-content-studio.service" \
  || wd_die 'Content Studio does not use the standalone-compatible fixed launcher'
grep -Fq 'content_studio_runtime_directory "$CURRENT_RELEASE"' \
  "$ROOT/deploy/api-bluegreen.sh" \
  || wd_die 'blue-green readiness does not verify the selected Content Studio runtime directory'
grep -Fq 'wd_content_studio_runtime_directory "$COMMERCE_RELEASE_ROOT/$sha"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'final verification does not verify the selected Content Studio runtime directory'
grep -Fq 'promoting exact tested engine binaries' "$ROOT/deploy/deploy.sh" \
  || wd_die 'engine is rebuilt after the candidate gate'
[[ $(grep -Fc 'production-(database|engine|backend|sales|openkeys|admin)' \
  "$ROOT/deploy/watchdog-github.sh") == 2 ]] \
  || wd_die 'GitHub deployment reporting does not allow the admin environment'

# Trusted pre-merge validation is host-owned and SHA-keyed. A separate low-priority service can
# validate two distinct descendants while production is active, but it shares only the exact-SHA
# candidate cache and never the production quarantine or overall deploy/watchdog verdict.
grep -Fq 'validation-next)' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'GitHub bridge cannot read the trusted candidate validation queue'
grep -Eq "GitHub candidate queue bridge'.*watchdog-github validation-next 2" \
  "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'sudo policy installer does not verify candidate queue access'
grep -Fq 'deployments(last:100,environments:[$environment]' \
  "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'candidate validation queue is not restricted to its dedicated environment'
grep -Fq 'latestStatus{state}' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'five-second candidate polling does not fetch queue states in one API request'
grep -Fq '^(candidate-validation|production-' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'GitHub bridge cannot report trusted candidate validation results'
grep -Fq '(.state == "IN_PROGRESS")' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'queued or interrupted candidate validations cannot be claimed'
grep -Fq 'auto_inactive:($environment != "candidate-validation")' \
  "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'parallel candidate verdicts can auto-inactivate one another'

shadow_contract=(
  'fetch_source "$CANDIDATE_SHA"'
  'wd_require_ancestor "$SOURCE_REPO" "$committed_master" "$CANDIDATE_SHA" shadow-committed-master'
  'wd_require_ancestor "$SOURCE_REPO" "$PROCESSED_SHA" "$CANDIDATE_SHA" shadow-processed'
  'VALIDATION_BASE_SHA=$PROCESSED_SHA'
  'select_candidate_validation_requirements "$CANDIDATE_SHA"'
  'prepare_and_test_candidate "$CANDIDATE_SHA" "$VALIDATION_TYPESCRIPT_REQUIRED"'
  '"$VALIDATION_TYPESCRIPT_FULL" "$VALIDATION_TYPESCRIPT_BASE_SHA"'
  'wd_require_ancestor "$SOURCE_REPO" "$current_master" "$CANDIDATE_SHA" shadow-current-master'
  'Trusted production-host candidate validation passed'
  'validation_output=$(sudo -n "$GITHUB_HELPER" validation-next 2)'
  'slot=$((index + 1))'
  'wait "${validation_pids[$index]}"'
)
for required_stage in "${shadow_contract[@]}"; do
  grep -Fq -- "$required_stage" "$ROOT/deploy/watchdog.sh" \
    || wd_die "trusted shadow validation lost required stage: $required_stage"
done
shadow_body=$(sed -n '/^shadow_validation_exit()/,/^final_verify_engine()/p' \
  "$ROOT/deploy/watchdog.sh")
grep -Fq 'wd_validation_failure_summary "$SHADOW_LOG_FILE" "$rc"' <<<"$shadow_body" \
  || wd_die 'trusted candidate failures are published without a diagnostic summary'
grep -Fq 'candidate-validation-$TEST_DB_SLOT.log' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'trusted candidate validation does not retain its bounded slot transcript'
grep -Fq 'previous_umask=$(umask)' <<<"$shadow_body" \
  || wd_die 'trusted candidate validation does not preserve the controller umask'
grep -Fq 'umask "$previous_umask"' <<<"$shadow_body" \
  || wd_die 'trusted candidate validation leaks its transcript-only umask into Git fetches'
fetch_body=$(sed -n '/^fetch_source_once()/,/^}/p' "$ROOT/deploy/watchdog.sh")
grep -Fq 'source_repo_readability_check' <<<"$fetch_body" \
  || wd_die 'source fetch does not repair and validate shared Git object readability'
grep -Fq "cat-file --batch-all-objects --batch-check='%(objectname) %(objecttype)'" \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'source repository check does not resolve every Git object as CI'
grep -Fq '== "missing"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'source repository check does not reject missing or unreadable Git objects'
grep -Fq 'validation-plan-unreadable-target-v1' "$ROOT/deploy/validation-plan.sh" \
  || wd_die 'an unreadable freshly fetched candidate cannot request a fail-closed repair gate'
if grep -Fq 'validation-plan baseline is unavailable' "$ROOT/deploy/validation-plan.sh"; then
  wd_die 'an unreadable trusted baseline still blocks the fail-closed repair gate'
fi
grep -Fq -- '-type d -exec chmod go+rx {} + -o -type f -exec chmod go+r {} +' \
  "$ROOT/deploy/lib.test.sh" \
  || wd_die 'the first trusted suite cannot repair a restrictive candidate checkout umask'
for conservative_field in \
  'typescript_required=1' 'typescript_full=1' 'rust_required=1' \
  'static_required=1' 'engine_artifacts_required=1'; do
  grep -Fq "printf '$conservative_field\\n'" "$ROOT/deploy/validation-plan.sh" \
    || wd_die "unreadable-target fallback omitted $conservative_field"
done
grep -Fq 'selected?.description' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge client discards trusted-host failure descriptions'
grep -Fq 'diagnostic="phase=$failed_phase; line=$line; exit=$rc; candidate quarantined"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'production failures do not publish their phase, line, and exit code'
if grep -Fq 'REJECTED_FILE' <<<"$shadow_body"; then
  wd_die 'a failed feature validation can quarantine production'
fi
if grep -Fq 'commit-status "$CANDIDATE_SHA" failure deploy/watchdog' <<<"$shadow_body"; then
  wd_die 'a failed feature validation can mark the healthy production SHA red'
fi
production_body=$(sed -n '/^main()/,/^case "${1:-}" in/p' "$ROOT/deploy/watchdog.sh")
if grep -Fq 'validation-next' <<<"$production_body"; then
  wd_die 'production watchdog still consumes candidate-validation work'
fi
grep -Fq 'ExecStart=/usr/local/lib/apitoken-watchdog/watchdog.sh --candidate-validator' \
  "$ROOT/systemd/apitoken-candidate-validator.service" \
  || wd_die 'candidate validation does not run in its own service'
grep -Fq 'CPUWeight=10' "$ROOT/systemd/apitoken-candidate-validator.service" \
  || wd_die 'candidate validation is not scheduled below production'
for candidate_unit in apitoken-candidate-validator.service apitoken-candidate-validator.timer; do
  grep -Fq "$candidate_unit" "$ROOT/deploy/install-watchdog.sh" \
    || wd_die "candidate validator unit is not installed: $candidate_unit"
done
grep -Fq 'systemctl enable --now apitoken-candidate-validator.timer' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'candidate validation timer is not enabled'
grep -Fq 'SOURCE_FETCH_LOCK=/run/lock/apitoken-source-fetch.lock' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'concurrent Git fetches are not serialized'
grep -Fq 'exec 9>"$STATE_ROOT/$sha.candidate.lock"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'production and candidate validation can mutate one SHA concurrently'
grep -Fq 'CI_CARGO_TARGET="$CI_HOME/cargo-target-shadow-$TEST_DB_SLOT"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'parallel Rust candidates share a writable build target'
grep -Fq 'TEST_DB_SLOT=$3' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'parallel candidates do not select isolated database slots'
[[ $(grep -Fc 'select_candidate_validation_requirements "$CANDIDATE_SHA"' \
  "$ROOT/deploy/watchdog.sh") == 2 ]] \
  || wd_die 'production and shadow validation do not share one requirement selector'
! grep -Fq 'codex_runtime_matches_candidate' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog still attests a sidecar Codex runtime against Git baselines'
! grep -Fq 'CODEX_RUNTIME_ATTESTATION' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog still fences a sidecar Codex runtime attestation'
! grep -Fq 'CODEX_APP_SERVERS_HELPER' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog still waits on a Codex daemon cohort before reporting green'

grep -Fq 'tokio-postgres-rustls' "$ROOT/crates/registry/Cargo.toml" \
  || wd_die 'engine PostgreSQL transport must use rustls alongside the BoringSSL forward transport'
if grep -Eq '^[[:space:]]*(postgres-native-tls|native-tls)[[:space:]]*=' \
  "$ROOT/crates/registry/Cargo.toml"; then
  wd_die 'OpenSSL-compatible PostgreSQL TLS cannot be linked with the BoringSSL forward transport'
fi

printf 'watchdog retention, migration, and engine topology tests passed\n'
