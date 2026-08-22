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
rust_diagnostic_log="$TEMP/rust-candidate.log"
printf '%s\n' \
  'thread '\''pg_test'\'' panicked at crates/forward/src/proxy/tests.rs:42: ENGINE_CONTROL_KEY=sk-secret-abc123 host detail' \
  'error: test failed, to rerun pass `-p forward --lib`' >"$rust_diagnostic_log"
rust_diagnostic=$(wd_validation_failure_summary "$rust_diagnostic_log" 101 testing)
[[ $rust_diagnostic == *'panicked at'* && $rust_diagnostic == *'ENGINE_CONTROL_KEY=REDACTED'* \
    && $rust_diagnostic != *'sk-secret'* ]] \
  || wd_die "candidate diagnostic hid the concrete Rust failure or leaked a secret: $rust_diagnostic"
empty_diagnostic=$(wd_validation_failure_summary "$TEMP/missing.log" 7 shadow-validation)
[[ $empty_diagnostic == 'phase=shadow-validation; validator exited with code 7' ]] \
  || wd_die "candidate diagnostic fallback is unstable: $empty_diagnostic"

# Public GitHub failure text must name the fail-closed reason (payload-canary or last wd_die),
# stay inside 140 characters, redact secrets, and never use a bash `wait` line number.
(
  desc_sha=$(printf 'a%.0s' {1..40})
  CANDIDATE_SHA=$desc_sha
  WD_PAYLOAD_EVIDENCE_DIR=$TEMP
  WD_LAST_ERROR='unified router blue-green controller failed (exit 1)'
  printf 'payload-canary: status_ok=0 statuses=404,413\n' >"$TEMP/$desc_sha.reason"
  canary_desc=$(wd_github_failure_description deploying-components 1)
  [[ $canary_desc == 'phase=deploying-components; payload-canary: status_ok=0 statuses=404,413' ]] \
    || wd_die "payload-canary reason did not win the GitHub description: $canary_desc"
  [[ $canary_desc != *line=* && ${#canary_desc} -le 140 ]] \
    || wd_die "GitHub description still uses a line number or exceeds 140 characters"
  rm -f -- "$TEMP/$desc_sha.reason"
  fallback_desc=$(wd_github_failure_description deploying-engine 1)
  [[ $fallback_desc == 'phase=deploying-engine; unified router blue-green controller failed (exit 1)' ]] \
    || wd_die "GitHub description lost the last wd_die message: $fallback_desc"
  WD_LAST_ERROR='ENGINE_CONTROL_KEY=super-secret postgresql://user:pass@db/x'
  secret_desc=$(wd_github_failure_description deploying-engine 1)
  [[ $secret_desc == *REDACTED* && $secret_desc != *super-secret* && $secret_desc != *user:pass* ]] \
    || wd_die "GitHub description leaked a secret: $secret_desc"
  WD_LAST_ERROR=$(python3 -c 'print("x"*200)')
  long_desc=$(wd_github_failure_description testing 1)
  [[ ${#long_desc} -eq 140 && $long_desc == phase=testing'; '* ]] \
    || wd_die "GitHub description is not truncated to 140 characters: ${#long_desc}"
)

# A generic wrapper such as "lane failed (exit 1)" must lose to a concrete transcript marker so an
# agent can see the compiler/controller cause in the 140-character GitHub description. The longer
# redacted excerpt is what the Checks API carries.
(
  desc_sha=$(printf 'b%.0s' {1..40})
  CANDIDATE_SHA=$desc_sha
  WD_FAILURE_DIR=$TEMP/failures
  mkdir -p "$WD_FAILURE_DIR"
  WD_CYCLE_LOG=$WD_FAILURE_DIR/$desc_sha.cycle.log
  WD_LAST_ERROR='TypeScript candidate lane failed (exit 1)'
  printf '%s\n' \
    'pnpm install: done' \
    'error: could not compile crates/router' \
    'Authorization: Bearer super-secret-header' \
    'ENGINE_CONTROL_KEY=super-secret postgresql://user:pass@db/x' \
    'github_pat_abcdefghijklmnopqrstuvwxyz123456' \
    '    Finished test target' \
    >"$WD_CYCLE_LOG"
  marker_desc=$(wd_github_failure_description testing 1)
  [[ $marker_desc == *'could not compile crates/router'* ]] \
    || wd_die "transcript marker did not replace a generic lane wrapper: $marker_desc"
  [[ $marker_desc != *'TypeScript candidate lane failed'* ]] \
    || wd_die "generic wrapper still won the GitHub description: $marker_desc"
  excerpt=$(wd_extract_failure_excerpt "$WD_CYCLE_LOG")
  [[ $excerpt == *'could not compile crates/router'* ]] \
    || wd_die "failure excerpt dropped the compiler cause: $excerpt"
  [[ $excerpt == *'Authorization: REDACTED'* && $excerpt != *super-secret-header* ]] \
    || wd_die "failure excerpt leaked an Authorization header: $excerpt"
  [[ $excerpt == *REDACTED* && $excerpt != *user:pass* && $excerpt != *github_pat_* ]] \
    || wd_die "failure excerpt leaked a DSN or PAT: $excerpt"
  headline=$(wd_github_failure_description testing 1)
  wd_write_failure_report "$desc_sha" testing 1 "$headline"
  [[ -f $WD_FAILURE_DIR/$desc_sha.summary.md && -f $WD_FAILURE_DIR/$desc_sha.text ]] \
    || wd_die 'failure report files were not written'
  grep -Fq "$desc_sha" "$WD_FAILURE_DIR/$desc_sha.summary.md" \
    || wd_die 'failure summary omitted the SHA'
  grep -Fq 'could not compile' "$WD_FAILURE_DIR/$desc_sha.text" \
    || wd_die 'failure text omitted the excerpt'
  grep -Fq 'super-secret' "$WD_FAILURE_DIR/$desc_sha.text" \
    && wd_die 'failure text retained a secret'
  text_mode=$(stat -c '%a' "$WD_FAILURE_DIR/$desc_sha.text" 2>/dev/null \
    || stat -f '%OLp' "$WD_FAILURE_DIR/$desc_sha.text")
  [[ $text_mode == 600 || $text_mode == 0600 ]] \
    || wd_die "failure text is not mode 0600: $text_mode"
)

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
  'none 1 0 0 0 0|runtime,panel,monitoring,codex,gemini,kimi'
  'none 0 1 0 0 0|monitoring'
  'none 0 0 1 0 0|monitoring'
  'none 0 0 0 1 0|monitoring'
  'none 0 0 0 0 1|monitoring'
  'caddy 0 0 0 0 0|routing,monitoring,codex,gemini,kimi'
  'monitoring 0 0 0 0 0|monitoring'
  'controller+caddy+monitoring 0 0 0 0 0|routing,monitoring,codex,gemini,kimi'
  'systemd 0 0 0 0 0|runtime,panel,routing,monitoring,codex,gemini,kimi'
  'controller+systemd 0 0 0 0 0|runtime,panel,routing,monitoring,codex,gemini,kimi'
  'full 0 0 0 0 0|runtime,panel,routing,monitoring,codex,gemini,kimi'
  'full 1 1 1 1 1|runtime,panel,routing,monitoring,codex,gemini,kimi'
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

commit_status_context_re=$(sed -n "s/^commit_status_context_re='\\(.*\\)'$/\\1/p" \
  "$ROOT/deploy/watchdog-github.sh")
[[ -n $commit_status_context_re ]] || wd_die 'GitHub commit-status context validator is missing'
for valid_context in deploy/watchdog deploy/gpt-image-2-public-preflight \
  deploy/gpt-image-2-public-preflight-v2 deploy/gpt-image-2-public-preflight-v3 \
  deploy/gpt-image-2-public-paid-smoke deploy/gpt-image-2-public-paid-generation \
  deploy/gpt-image-2-public-paid-edit deploy/gpt-image-2-public-paid-inspect \
  deploy/gpt-image-2-settlement-diagnostic deploy/gpt-image-2-settlement-v2-diagnostic; do
  [[ $valid_context =~ $commit_status_context_re ]] \
    || wd_die "valid GitHub commit-status context was rejected: $valid_context"
done
for invalid_context in deploy/ deploy/-image deploy/Image-2 deploy/image_2 deploy/image/2 other/image-2; do
  if [[ $invalid_context =~ $commit_status_context_re ]]; then
    wd_die "invalid GitHub commit-status context was accepted: $invalid_context"
  fi
done

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
final_verify_kimi_surface() { verification_probe kimi; }

: >"$verification_barrier_log"
VERIFICATION_BARRIER_EXPECTED=6
run_final_verification_plan runtime,panel,routing,monitoring,codex,gemini,kimi deadbeef
[[ $(sed -n '1p' "$verification_barrier_log") == runtime ]] \
  || wd_die "read-only final probes started before runtime reconciliation"
for final_probe in panel routing monitoring codex gemini kimi; do
  grep -Fxq "$final_probe" "$verification_barrier_log" \
    || wd_die "final verification dispatcher omitted $final_probe"
done

: >"$verification_barrier_log"
VERIFICATION_BARRIER_EXPECTED=6
VERIFICATION_FAIL_CHECK=routing
if ( run_final_verification_plan runtime,panel,routing,monitoring,codex,gemini,kimi deadbeef ) \
    >/dev/null 2>&1; then
  wd_die "a failed final verifier did not fail the parent plan"
fi
[[ $(grep -Evc '^runtime$' "$verification_barrier_log") == 6 ]] \
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

# KIMI keeps the same explicit optional-provider contract on its backend-only loopback origin:
# disabled (the shipped default) is proven by the enabled gauge plus the bounded engine envelope;
# enabled additionally requires one live profile, matching the plane's own readiness contract;
# missing Prometheus series fail closed.
(
  # shellcheck disable=SC2091
  eval "$(sed -n '/^final_verify_kimi_surface()/,/^}/p' "$ROOT/deploy/watchdog.sh")"
  kimi_probe_log="$TEMP/kimi-probe.log"
  KIMI_PROBE_MODE=disabled
  # Invoked indirectly by the extracted verifier.
  # shellcheck disable=SC2329
  curl() {
    printf '%s\n' "$*" >>"$kimi_probe_log"
    case "$*" in
      *'query=claude_api_kimi_enabled'*)
        case "$KIMI_PROBE_MODE" in
          disabled) printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"0"]}]}}' ;;
          enabled|unauthenticated) printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"1"]}]}}' ;;
          missing) printf '%s\n' '{"status":"success","data":{"result":[]}}' ;;
        esac
        ;;
      *'claude_api_kimi_live_profiles'*)
        if [[ $KIMI_PROBE_MODE == unauthenticated ]]; then
          printf '%s\n' '{"status":"success","data":{"result":[]}}'
        else
          printf '%s\n' '{"status":"success","data":{"result":[{"value":[0,"1"]}]}}'
        fi
        ;;
      *'127.0.0.1:8803/v1/messages'*)
        printf '%s\n' '{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}'
        ;;
      *) return 2 ;;
    esac
  }
  sleep() { :; }

  : >"$kimi_probe_log"
  KIMI_PROBE_MODE=disabled
  final_verify_kimi_surface >/dev/null
  (( $(wc -l <"$kimi_probe_log") == 2 )) \
    || wd_die "disabled KIMI verification skipped the stable-origin envelope check"

  : >"$kimi_probe_log"
  KIMI_PROBE_MODE=enabled
  final_verify_kimi_surface >/dev/null
  (( $(wc -l <"$kimi_probe_log") == 3 )) \
    || wd_die "enabled KIMI verification skipped the live-profile or envelope checks"
  grep -Fq 'query=claude_api_kimi_live_profiles{provider="kimi"} >= 1' \
    "$kimi_probe_log" || wd_die "enabled KIMI verification did not require a live profile"

  : >"$kimi_probe_log"
  KIMI_PROBE_MODE=unauthenticated
  if ( final_verify_kimi_surface ) >/dev/null 2>&1; then
    wd_die "an enabled KIMI provider without a live profile passed verification"
  fi
  (( $(wc -l <"$kimi_probe_log") == 7 )) \
    || wd_die "missing KIMI runtime metrics did not use the bounded retry window"

  : >"$kimi_probe_log"
  KIMI_PROBE_MODE=missing
  if ( final_verify_kimi_surface ) >/dev/null 2>&1; then
    wd_die "missing KIMI enablement metrics were treated as disabled"
  fi
  (( $(wc -l <"$kimi_probe_log") == 6 )) \
    || wd_die "missing KIMI enablement metrics did not use the bounded retry window"
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

# Sales owns a separate Drizzle history and production database. Its flat SQL/journal artifacts
# receive the same byte-for-byte append-only protection as commerce before sales-deploy may migrate.
for fixture in sales-baseline sales-appended sales-tampered; do
  mkdir -p "$TEMP/$fixture/packages/sales-db"
  cp -R -- "$ROOT/packages/sales-db/migrations" "$TEMP/$fixture/packages/sales-db/migrations"
done
wd_sales_migration_manifest "$TEMP/sales-baseline" >"$TEMP/sales-baseline.manifest"
node - "$TEMP/sales-appended/packages/sales-db/migrations/meta/_journal.json" \
  "$TEMP/sales-appended/packages/sales-db/migrations/0016_watchdog_sales_manifest_test.sql" <<'NODE'
const fs = require("node:fs");
const [journalPath, sqlPath] = process.argv.slice(2);
const journal = JSON.parse(fs.readFileSync(journalPath, "utf8"));
const previous = journal.entries.at(-1);
journal.entries.push({
  idx: journal.entries.length,
  version: journal.version,
  when: previous.when + 1,
  tag: "0016_watchdog_sales_manifest_test",
  breakpoints: true,
});
fs.writeFileSync(journalPath, `${JSON.stringify(journal, null, 2)}\n`);
fs.writeFileSync(sqlPath, "CREATE TABLE watchdog_sales_manifest_test (id integer PRIMARY KEY);\n");
NODE
wd_sales_migration_manifest "$TEMP/sales-appended" >"$TEMP/sales-appended.manifest"
wd_manifest_is_append_only "$TEMP/sales-baseline.manifest" "$TEMP/sales-appended.manifest"
[[ $(wd_manifest_digest "$TEMP/sales-baseline.manifest") \
  != $(wd_manifest_digest "$TEMP/sales-appended.manifest") ]]
printf '\n-- forbidden historical edit\n' \
  >>"$TEMP/sales-tampered/packages/sales-db/migrations/0015_paid_funded_commission_v2.sql"
wd_sales_migration_manifest "$TEMP/sales-tampered" >"$TEMP/sales-tampered.manifest"
if wd_manifest_is_append_only "$TEMP/sales-baseline.manifest" "$TEMP/sales-tampered.manifest"; then
  wd_die "sales manifest accepted an edited historical SQL migration"
fi

grep -Fq 'wd_manifest_is_append_only "$SALES_DB_MANIFEST" "$MANIFEST_TMP"' \
  "$ROOT/deploy/sales-deploy.sh" \
  || wd_die "sales deploy lost its append-only admission gate"

# The installed component runner sits in controller/, while its fixed shared library and backup
# runner sit one level above it. Execute that real layout far enough to prove startup resolves the
# library, and pin the backup default to the same trusted root before the first Sales migration.
sales_installed_root="$TEMP/sales-installed-controller"
mkdir -p "$sales_installed_root/controller"
cp -- "$ROOT/deploy/sales-deploy.sh" "$sales_installed_root/controller/sales-deploy.sh"
cp -- "$ROOT/deploy/watchdog-lib.sh" "$sales_installed_root/watchdog-lib.sh"
sales_installed_error="$TEMP/sales-installed-controller.error"
if bash "$sales_installed_root/controller/sales-deploy.sh" >"$TEMP/sales-installed-controller.out" \
  2>"$sales_installed_error"; then
  wd_die "installed sales deploy unexpectedly accepted a missing SHA"
fi
grep -Fq 'usage: sales-deploy.sh <full-40-char-sha>' "$sales_installed_error" \
  || wd_die "installed sales deploy cannot source the fixed parent watchdog library"
grep -Fq 'BACKUP_RUNNER=${SALES_BACKUP_RUNNER:-$WATCHDOG_ROOT/watchdog-backup.sh}' \
  "$ROOT/deploy/sales-deploy.sh" \
  || wd_die "installed sales deploy does not resolve backup from the fixed watchdog root"
awk '
  /new append-only sales migration history detected/ { in_migration = 1 }
  in_migration && /"\$BACKUP_RUNNER" "\$SHA"/ { backup = NR }
  in_migration && /node "\$release\/packages\/sales-db\/dist\/migrate.js"/ { migrate = NR }
  in_migration && /committed tested sales migration manifest/ { commit = NR }
  END { exit !(backup > 0 && backup < migrate && migrate < commit) }
' "$ROOT/deploy/sales-deploy.sh" \
  || wd_die "sales deploy no longer orders backup before migrate before manifest commit"

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

# The CRM root names releases `crm-<sha>` (plus legacy plain `<sha>`), so its retention runs with an
# explicit name pattern. Prefixed releases get the same keep counting, current/previous protection,
# and explicit SHA protection; malformed prefixed names and links are never selected.
crm_root="$TEMP/crm-releases"
mkdir -p "$crm_root"
crm_shas=(
  1111111111111111111111111111111111111111
  2222222222222222222222222222222222222222
  3333333333333333333333333333333333333333
  4444444444444444444444444444444444444444
  5555555555555555555555555555555555555555
)
for crm_sha in "${crm_shas[@]}"; do
  mkdir -p "$crm_root/crm-$crm_sha"
done
mkdir -p "$crm_root/not-a-release" "$crm_root/crm-not-a-sha" "$crm_root/plain-non-sha"
ln -s "$crm_root/crm-${crm_shas[4]}" "$crm_root/current"
ln -s "$crm_root/crm-${crm_shas[3]}" "$crm_root/previous"
node - "$crm_root" "${crm_shas[@]}" <<'NODE'
const fs = require("node:fs");
const [root, ...shas] = process.argv.slice(2);
// Oldest first, so index 0 is the least recently modified prefixed release.
shas.forEach((sha, index) => {
  const when = 1700000000 + index;
  fs.utimesSync(`${root}/crm-${sha}`, when, when);
});
NODE

# keep=1 retains the newest unprotected prefixed release plus current/previous; the two oldest
# prefixed releases are selected and malformed names are ignored.
crm_prunable=()
while IFS= read -r -d '' crm_release; do
  crm_prunable+=("${crm_release##*/}")
done < <(wd_prunable_release_dirs "$crm_root" 1 --pattern '^(crm-)?[0-9a-f]{40}$')
[[ ${#crm_prunable[@]} -eq 2 ]] \
  || wd_die "crm release retention selected ${#crm_prunable[@]} directories, expected 2"
printf '%s\n' "${crm_prunable[@]}" | grep -Fxq "crm-${crm_shas[0]}" \
  || wd_die "crm release retention did not select the oldest prefixed release"
printf '%s\n' "${crm_prunable[@]}" | grep -Fxq "crm-${crm_shas[1]}" \
  || wd_die "crm release retention did not select the second-oldest prefixed release"
for protected_crm in "crm-${crm_shas[4]}" "crm-${crm_shas[3]}" "crm-${crm_shas[2]}" \
  crm-not-a-sha not-a-release plain-non-sha; do
  if printf '%s\n' "${crm_prunable[@]}" | grep -Fxq "$protected_crm"; then
    wd_die "crm release retention selected a protected or malformed entry: $protected_crm"
  fi
done

# An explicitly protected plain SHA must protect its prefixed CRM release.
crm_protected=()
while IFS= read -r -d '' crm_release; do
  crm_protected+=("${crm_release##*/}")
done < <(wd_prunable_release_dirs "$crm_root" 1 --pattern '^(crm-)?[0-9a-f]{40}$' "${crm_shas[0]}")
if printf '%s\n' "${crm_protected[@]}" | grep -Fxq "crm-${crm_shas[0]}"; then
  wd_die "crm release retention removed an explicitly protected prefixed release"
fi

# keep=0 with no protected list still must not touch prefixed current/previous.
crm_zero_keep=()
while IFS= read -r -d '' crm_release; do
  crm_zero_keep+=("${crm_release##*/}")
done < <(wd_prunable_release_dirs "$crm_root" 0 --pattern '^(crm-)?[0-9a-f]{40}$')
[[ ${#crm_zero_keep[@]} -eq 3 ]] \
  || wd_die "keep=0 crm retention must still protect prefixed current and previous"

# The default pattern never admits a prefixed name, even when prefixed directories exist.
plain_crm_root="$TEMP/plain-crm-releases"
mkdir -p "$plain_crm_root"
for plain_sha in 1111111111111111111111111111111111111111 2222222222222222222222222222222222222222; do
  mkdir -p "$plain_crm_root/$plain_sha" "$plain_crm_root/crm-$plain_sha"
done
while IFS= read -r -d '' plain_release; do
  [[ ${plain_release##*/} != crm-* ]] \
    || wd_die "default release retention accepted a prefixed name"
done < <(wd_prunable_release_dirs "$plain_crm_root" 1)

# Live release discovery requires complete exe+cwd observations, protects every distinct managed
# release, skips missing/inactive units, and routes non-dumpable authbot only through the root helper.
for retention_function in live_release_shas prune_selected_releases prune_expired_releases \
  prune_expired_releases_best_effort; do
  eval "$(sed -n "/^${retention_function}()/,/^}/p" "$ROOT/deploy/watchdog.sh")"
done
generic_engine_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
generic_commerce_sha=cccccccccccccccccccccccccccccccccccccccc
generic_crm_sha=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee
authbot_live_sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
ENGINE_RELEASE_ROOT="$TEMP/live-engine-releases"
COMMERCE_RELEASE_ROOT="$TEMP/live-commerce-releases"
CRM_RELEASE_ROOT="$TEMP/live-crm-releases"
mkdir -p "$ENGINE_RELEASE_ROOT/$generic_engine_sha" "$COMMERCE_RELEASE_ROOT/$generic_commerce_sha" \
  "$CRM_RELEASE_ROOT/crm-$generic_crm_sha"
engine_delete_sha=1111111111111111111111111111111111111111
commerce_delete_sha=2222222222222222222222222222222222222222
crm_delete_sha=3333333333333333333333333333333333333333
mkdir -p "$ENGINE_RELEASE_ROOT/$engine_delete_sha" "$COMMERCE_RELEASE_ROOT/$commerce_delete_sha" \
  "$CRM_RELEASE_ROOT/crm-$crm_delete_sha"
live_readlink_log="$TEMP/live-readlink.log"
release_rm_log="$TEMP/release-rm.log"
selector_log="$TEMP/prunable-release-args.log"
generic_state_count="$TEMP/generic-state-count"
generic_pid_count="$TEMP/generic-pid-count"
selector_count="$TEMP/selector-count"
mktemp_count="$TEMP/mktemp-count"
live_helper_mode=active
generic_live_mode=active
selector_mode=success
systemctl() {
  [[ $1 == show && $3 == -p && $5 == --value ]] || return 1
  if [[ $2 == apitoken-crm-web.service ]]; then
    case $4 in
      LoadState) printf '%s\n' loaded ;;
      ActiveState) printf '%s\n' active ;;
      MainPID) printf '%s\n' 4343 ;;
      *) return 1 ;;
    esac
    return 0
  fi
  if [[ $2 != claude-api.service ]]; then
    [[ $4 == LoadState ]] && printf '%s\n' not-found
    return 0
  fi
  case $4 in
    LoadState)
      [[ $generic_live_mode != systemctl-failure ]] || return 1
      printf '%s\n' loaded
      ;;
    ActiveState)
      count=$(<"$generic_state_count")
      count=$((count + 1))
      printf '%s\n' "$count" >"$generic_state_count"
      [[ $generic_live_mode != state-churn || $count -eq 1 ]] && printf '%s\n' active || printf '%s\n' inactive
      ;;
    MainPID)
      count=$(<"$generic_pid_count")
      count=$((count + 1))
      printf '%s\n' "$count" >"$generic_pid_count"
      [[ $generic_live_mode != pid-churn || $count -eq 1 ]] && printf '%s\n' 4242 || printf '%s\n' 4343
      ;;
    *) return 1 ;;
  esac
}
readlink() {
  printf '%s\n' "$3" >>"$live_readlink_log"
  case "$3" in
    /proc/4242/exe)
      [[ $generic_live_mode != exe-failure ]] || return 1
      case $generic_live_mode in
        outside-only) printf '%s\n' /usr/local/bin/claude-api ;;
        malformed-root) printf '%s/not-a-release/claude-api\n' "$ENGINE_RELEASE_ROOT" ;;
        *) printf '%s/%s/claude-api\n' "$ENGINE_RELEASE_ROOT" "$generic_engine_sha" ;;
      esac
      ;;
    /proc/4242/cwd)
      [[ $generic_live_mode != cwd-failure ]] || return 1
      [[ $generic_live_mode != outside-only ]] \
        && printf '%s/%s/apps/api\n' "$COMMERCE_RELEASE_ROOT" "$generic_commerce_sha" \
        || printf '%s\n' /var/lib/apitoken
      ;;
    /proc/4343/exe)
      printf '%s\n' /usr/bin/node
      ;;
    /proc/4343/cwd)
      printf '%s/crm-%s/apps/crm-web\n' "$CRM_RELEASE_ROOT" "$generic_crm_sha"
      ;;
    *) wd_die "generic live release inspection reached an unexpected procfs path: $3" ;;
  esac
}
flock() { return 0; }
sudo() {
  if [[ $1 == -n && $2 == fixed-authbot-helper && $3 == release-sha && $# -eq 3 ]]; then
    case "$live_helper_mode" in
      active) printf '%s\n' "$authbot_live_sha" ;;
      inactive) return 0 ;;
      malformed) printf '%s\n' not-a-sha ;;
      failure) return 1 ;;
      *) return 1 ;;
    esac
    return
  fi
  printf '%s\n' "$*" >>"$release_rm_log"
}
mktemp() {
  count=$(<"$mktemp_count")
  count=$((count + 1))
  printf '%s\n' "$count" >"$mktemp_count"
  path="$TEMP/release-selection-$count"
  : >"$path"
  printf '%s\n' "$path"
}
wd_prunable_release_dirs() {
  count=$(<"$selector_count")
  count=$((count + 1))
  printf '%s\n' "$count" >"$selector_count"
  printf '%s\n' "$*" >>"$selector_log"
  if [[ $selector_mode == first-failure && $count == 1 ]]; then
    printf '%s\0' "$ENGINE_RELEASE_ROOT/$engine_delete_sha"
    return 1
  fi
  if [[ $selector_mode == second-failure && $count == 2 ]]; then
    printf '%s\0' "$COMMERCE_RELEASE_ROOT/$commerce_delete_sha"
    return 1
  fi
  if [[ $selector_mode == third-failure && $count == 3 ]]; then
    printf '%s\0' "$CRM_RELEASE_ROOT/crm-$crm_delete_sha"
    return 1
  fi
  case $1 in
    "$ENGINE_RELEASE_ROOT") printf '%s\0' "$ENGINE_RELEASE_ROOT/$engine_delete_sha" ;;
    "$COMMERCE_RELEASE_ROOT") printf '%s\0' "$COMMERCE_RELEASE_ROOT/$commerce_delete_sha" ;;
    "$CRM_RELEASE_ROOT") printf '%s\0' "$CRM_RELEASE_ROOT/crm-$crm_delete_sha" ;;
    *) wd_die "selector reached an unexpected release root: $1" ;;
  esac
}
notification_failure=none
wd_warn() {
  printf '%s\n' "$*" >>"$TEMP/retention-warn.log"
  [[ $notification_failure != warn ]]
}
wd_log() { :; }
status() {
  printf '%s\n' "$*" >>"$TEMP/retention-status.log"
  [[ $notification_failure != status ]]
}
AUTHBOT_RUNTIME_STATE=fixed-authbot-helper
DEPLOY_LOCK="$TEMP/release-deploy.lock"
: >"$DEPLOY_LOCK"
RELEASE_RETENTION_KEEP=1
ENGINE_SHA=
BACKEND_SHA=
PROCESSED_SHA=
SALES_SHA=
OPENKEYS_SHA=
reset_live_fixture() {
  printf '0\n' >"$generic_state_count"
  printf '0\n' >"$generic_pid_count"
  printf '0\n' >"$selector_count"
  printf '0\n' >"$mktemp_count"
  : >"$live_readlink_log"
  : >"$release_rm_log"
  : >"$selector_log"
  : >"$TEMP/retention-warn.log"
  : >"$TEMP/retention-status.log"
  rm -f -- "$TEMP/release-selection-1" "$TEMP/release-selection-2" "$TEMP/release-selection-3"
}
reset_live_fixture
live_output=$(live_release_shas) || wd_die 'live release discovery rejected complete generic/authbot observations'
for live_sha in "$generic_engine_sha" "$generic_commerce_sha" "$generic_crm_sha" "$authbot_live_sha"; do
  printf '%s\n' "$live_output" | grep -Fxq "$live_sha" \
    || wd_die "live release discovery lost protected SHA $live_sha"
done
[[ $(wc -l <"$live_readlink_log" | tr -d ' ') == 4 ]] \
  || wd_die 'live release discovery did not require both exe and cwd observations'
live_helper_mode=inactive
reset_live_fixture
live_release_shas >/dev/null || wd_die 'live release discovery rejected inactive authbot'
for live_helper_mode in malformed failure; do
  reset_live_fixture
  live_release_shas >"$TEMP/live-helper-$live_helper_mode.log" 2>&1 \
    && wd_die "live release discovery accepted authbot helper $live_helper_mode"
done
live_helper_mode=active
for generic_live_mode in exe-failure cwd-failure outside-only malformed-root state-churn pid-churn systemctl-failure; do
  reset_live_fixture
  live_release_shas >"$TEMP/generic-live-$generic_live_mode.log" 2>&1 \
    && wd_die "live release discovery accepted generic inspection $generic_live_mode"
done
generic_live_mode=active
for selector_mode in first-failure second-failure third-failure; do
  reset_live_fixture
  prune_expired_releases && wd_die "release pruning accepted $selector_mode"
  [[ ! -s $release_rm_log ]] || wd_die "release pruning mutated a root after $selector_mode"
  [[ ! -e $TEMP/release-selection-1 && ! -e $TEMP/release-selection-2 \
      && ! -e $TEMP/release-selection-3 ]] \
    || wd_die "release pruning leaked selector files after $selector_mode"
done
selector_mode=success
reset_live_fixture
prune_expired_releases || wd_die 'release pruning rejected complete triple-root selections'
[[ $(grep -Fc "$authbot_live_sha" "$selector_log") -eq 3 ]] \
  || wd_die 'every release root did not use the same protected live snapshot'
grep -Fq "rm -rf --one-file-system -- $ENGINE_RELEASE_ROOT/$engine_delete_sha" "$release_rm_log" \
  || wd_die 'engine release selection was not removed after all selectors completed'
grep -Fq "rm -rf --one-file-system -- $COMMERCE_RELEASE_ROOT/$commerce_delete_sha" "$release_rm_log" \
  || wd_die 'commerce release selection was not removed after all selectors completed'
grep -Fq "rm -rf --one-file-system -- $CRM_RELEASE_ROOT/crm-$crm_delete_sha" "$release_rm_log" \
  || wd_die 'crm release selection was not removed after all selectors completed'
[[ ! -e $TEMP/release-selection-1 && ! -e $TEMP/release-selection-2 \
    && ! -e $TEMP/release-selection-3 ]] \
  || wd_die 'release pruning leaked selector files after success'
# A host-local observation failure is absorbed by the real wrapper under production-like errexit.
# Invoke it as a simple command, never through `fn || ...` (which suppresses errexit inside the
# function). Warning and status publication are independently best-effort: even when either fails,
# the ERR/quarantine equivalent must not run and subsequent candidate processing must execute.
live_helper_mode=failure
selector_mode=success
CANDIDATE_SHA=dddddddddddddddddddddddddddddddddddddddd
for notification_failure in none warn status; do
  reset_live_fixture
  retention_sentinel="$TEMP/retention-$notification_failure.continued"
  retention_failure_trap="$TEMP/retention-$notification_failure.failed"
  rm -f -- "$retention_sentinel" "$retention_failure_trap"
  (
    set -Ee
    trap 'printf "failure trap ran\n" >"$retention_failure_trap"; exit 97' ERR
    prune_expired_releases_best_effort
    printf 'candidate processing continued\n' >"$retention_sentinel"
  )
  [[ -e $retention_sentinel ]] \
    || wd_die "best-effort release pruning stopped candidate processing when $notification_failure notification failed"
  [[ ! -e $retention_failure_trap ]] \
    || wd_die "best-effort release pruning reached the failure trap when $notification_failure notification failed"
  [[ ! -s $release_rm_log ]] \
    || wd_die "best-effort release pruning deleted after $notification_failure notification failure"
  grep -Fq 'release retention skipped' "$TEMP/retention-warn.log" \
    || wd_die "best-effort release pruning lost its warning message in $notification_failure case"
  grep -Fq 'continuing candidate processing' "$TEMP/retention-status.log" \
    || wd_die "best-effort release pruning lost its status message in $notification_failure case"
done
unset CANDIDATE_SHA notification_failure
unset -f live_release_shas prune_selected_releases prune_expired_releases \
  prune_expired_releases_best_effort reset_live_fixture systemctl readlink flock sudo mktemp \
  wd_prunable_release_dirs wd_warn wd_log status

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
wd_path_is_typescript packages/opencode-router-plugin/apitoken-router.js \
  || wd_die "OpenCode plugin not classified for TypeScript validation"
wd_path_is_web packages/opencode-router-plugin/apitoken-router.js \
  || wd_die "OpenCode plugin not assigned to the web validation context"
wd_path_is_backend packages/opencode-router-plugin/apitoken-router.js \
  && wd_die "OpenCode plugin must not trigger a commerce backend deployment"
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
for control_api_path in \
  crates/server/src/admin.rs \
  crates/server/src/http.rs \
  crates/registry/src/pg.rs \
  crates/registry/migrations_pg/0062_request_usage_grafana_rollups.sql \
  packages/engine-client/src/index.ts \
  packages/contracts/src/index.ts \
  tests/control_api_engine_client_acceptance.sh; do
  wd_path_requires_control_api_acceptance "$control_api_path" \
    || wd_die "Control API acceptance trigger is missing: $control_api_path"
done
wd_path_requires_control_api_acceptance apps/web/src/app/page.tsx \
  && wd_die "unrelated frontend changes must not select Control API acceptance"
wd_path_is_backend packages/contracts/src/index.ts \
  || wd_die "contracts remain shared with the commerce backend"
wd_path_is_backend apps/content-studio/src/app/page.tsx || wd_die "content studio must trigger commerce deployment"
wd_path_is_engine tools/codex-native/probe-live.py \
  || wd_die "native Codex tooling must trigger an engine deployment"
for provider_unit in systemd/claude-api.service systemd/claude-api@.service \
  systemd/claude-api-anthropic@.service systemd/claude-api-openai.service \
  systemd/claude-api-openai@.service systemd/claude-api-gemini.service \
  systemd/claude-api-gemini@.service systemd/claude-api-kimi.service \
  systemd/claude-api-kimi@.service; do
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
  systemd/claude-api-kimi.service \
  systemd/claude-api-kimi@.service \
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
  deploy/agent-worktree.sh \
  deploy/DELETE_WORKTREE.sh \
  deploy/prune-merged.sh \
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
wd_path_is_systemd_definition systemd/claude-api-kimi.service
wd_path_is_systemd_definition systemd/claude-api-kimi@.service
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
  deploy/gpt-image-2-live-gate.sh \
  deploy/gpt-image-2-public-smoke-gate.sh \
  deploy/gpt-image-2-public-preflight-gate.sh \
  deploy/gpt-image-2-public-preflight-v2-gate.sh \
  deploy/gpt-image-2-public-preflight-v3-gate.sh \
  deploy/gpt-image-2-public-paid-smoke-gate.sh \
  deploy/gpt-image-2-public-paid-smoke-v2-gate.sh \
  deploy/gpt-image-2-public-paid-smoke-v3-gate.sh \
  deploy/gpt-image-2-public-paid-inspect-gate.sh \
  deploy/gpt-image-2-surface-probe-gate.sh \
  deploy/watchdog-infrastructure.sh \
  deploy/deploy.sh \
  deploy/authbot-runtime-state.sh \
  deploy/lib.sh \
  deploy/commerce-release-bundle.sh \
  deploy/release-tree-digest.mjs \
  deploy/content-studio-start.sh \
  deploy/api-bluegreen.sh \
  deploy/engine-bluegreen.sh \
  deploy/engine-migrate.sh \
  deploy/pricing-retirement-admission.sh \
  deploy/pricing-retirement-postdrop.sh \
  deploy/pricing-retired-schema-manifest.sh \
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

mkdir -p "$validation_repo/deploy"
printf 'format=1\ncommerce_requires=scalar-pricing-v1\nengine_provides=scalar-pricing-v1\n' \
  >"$validation_repo/deploy/engine-commerce-compatibility.contract"
git -C "$validation_repo" add deploy/engine-commerce-compatibility.contract
git -C "$validation_repo" commit --quiet -m compatibility-contract
validation_compatibility=$(git -C "$validation_repo" rev-parse HEAD)
mkdir -p "$validation_repo/packages/engine-client/acceptance"
printf 'acceptance\n' >"$validation_repo/packages/engine-client/acceptance/control-api.mjs"
git -C "$validation_repo" add packages/engine-client/acceptance/control-api.mjs
git -C "$validation_repo" commit --quiet -m control-api-acceptance
validation_control_api=$(git -C "$validation_repo" rev-parse HEAD)
control_api_plan=$(bash "$ROOT/deploy/validation-plan.sh" "$validation_repo" \
  "$validation_control_api" "$validation_compatibility" "$validation_compatibility" \
  "$validation_compatibility" "$validation_compatibility" "$validation_compatibility")
[[ $(plan_value "$control_api_plan" rust_required) == 1 \
   && $(plan_value "$control_api_plan" engine_artifacts_required) == 1 ]] \
  || wd_die "EngineClient acceptance changes did not require a production engine artifact"

compatibility_plan=$(bash "$ROOT/deploy/validation-plan.sh" "$validation_repo" \
  "$validation_compatibility" "$validation_unknown" "$validation_unknown" \
  "$validation_unknown" "$validation_unknown" "$validation_unknown")
for flag in typescript_required rust_required static_required engine_artifacts_required; do
  [[ $(plan_value "$compatibility_plan" "$flag") == 1 ]] \
    || wd_die "engine/commerce compatibility contract did not enable $flag"
done
[[ $(plan_value "$compatibility_plan" typescript_full) == 0 ]] \
  || wd_die "engine/commerce compatibility contract unnecessarily selected unrelated TypeScript contexts"
[[ $(wd_typescript_components_for_range "$validation_repo" "$validation_unknown" \
  "$validation_compatibility" 0) == commerce ]] \
  || wd_die "engine/commerce compatibility contract did not build the commerce release bundle"

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

# Cache affinity and Codex response history run as separate instances. maxmemory and
# maxmemory-policy are per-instance in Redis, so a single instance cannot give them independent
# budgets: a few dozen 256 MiB conversations could evict affinity, and affinity churn could delete
# conversations, which clients see as a permanent 400.
grep -Fq '127.0.0.1:6380:6379' "$ROOT/deploy/affinity-redis.compose.yaml" \
  || wd_die 'cache affinity does not have its own Redis instance'
grep -Fq '/var/lib/apitoken/affinity-redis-l2:/data' "$ROOT/deploy/affinity-redis.compose.yaml" \
  || wd_die 'the second Redis instance does not have its own data directory'
grep -Fq 'install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis-l2' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the second Redis data directory is not provisioned for the image uid/gid'
grep -Fq "[[ ! -L /var/lib/apitoken/affinity-redis-l2 ]]" "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the second Redis data directory is not symlink-fenced'
# Both provisioning blocks must live inside provision_redis_data_dirs, and every transaction that
# activates Redis must call it first. The narrow --systemd-only path also starts the containers,
# and Docker would create a missing bind-mount target as root — costing the redis uid write access
# to its own /data (MISCONF) while PING still answered.
provision_fn=$(sed -n '/^provision_redis_data_dirs()/,/^}/p' "$ROOT/deploy/install-watchdog.sh")
grep -Fq 'install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis' <<<"$provision_fn" \
  || wd_die 'the history Redis data directory provisioning is not in the shared function'
grep -Fq 'install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis-l2' \
  <<<"$provision_fn" \
  || wd_die 'the affinity Redis data directory provisioning is not in the shared function'
[[ $(grep -Ec '^[[:space:]]*provision_redis_data_dirs$' "$ROOT/deploy/install-watchdog.sh") -eq 2 ]] \
  || wd_die 'a path that starts Redis does not provision its data directories'
while read -r redis_start_line; do
  awk -v target="$redis_start_line" 'NR < target && /^[[:space:]]*provision_redis_data_dirs$/ { found = 1 }
    END { exit !found }' "$ROOT/deploy/install-watchdog.sh" \
    || wd_die 'Redis is started before its data directories are provisioned'
done < <(grep -n '^[[:space:]]*activate_redis_definition$' "$ROOT/deploy/install-watchdog.sh" \
  | cut -d: -f1)
grep -Fq 'CLAUDE_API_AFFINITY_REDIS_URL=redis://default:%s@127.0.0.1:6380/0' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the affinity Redis URL is not provisioned for the second instance'
# History keeps 6379 and the existing data directory. Moving that side instead would strand every
# stored conversation at cutover — the exact customer-visible failure this split prevents.
grep -F '  affinity-redis:' -A 24 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq '127.0.0.1:6379:6379' \
  || wd_die 'response history must keep the port that already holds its conversations'
grep -F '  affinity-redis:' -A 30 "$ROOT/deploy/affinity-redis.compose.yaml" \
  | grep -Fq '/var/lib/apitoken/affinity-redis:/data' \
  || wd_die 'response history must keep its existing data directory'
# The engine must fall back to the shared URL when the second instance is not yet provisioned:
# a shared instance is worse than a split one, but far better than affinity losing its L2 silently.
grep -Fq 'ev("CLAUDE_API_AFFINITY_REDIS_URL").or_else(|| redis_url.clone())' \
  "$ROOT/crates/server/src/config.rs" \
  || wd_die 'the engine does not fall back to the shared Redis URL before provisioning'
grep -Fq 'Wants=network-online.target apitoken-affinity-redis.service' \
  "$ROOT/systemd/claude-api-anthropic@.service"
! grep -Fq 'Requires=apitoken-affinity-redis.service' "$ROOT/systemd/claude-api-anthropic@.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-anthropic@.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api.service"
grep -Fq 'CLAUDE_API_PROVIDER=anthropic CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0 CLAUDE_API_GLM_ENABLED=0 CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1' \
  "$ROOT/systemd/claude-api-anthropic@.service"
! grep -Fq 'CLAUDE_API_PROVIDER=' "$ROOT/systemd/claude-api@.service"
! grep -Fq 'CLAUDE_API_PROVIDER=' "$ROOT/systemd/claude-api.service"
# GLM has no live subscription and its terms forbid the customer proxy shape. A staged keyring in
# the shared server env must therefore never activate it on the public Anthropic plane or either
# legacy combined rollback anchor. Activation needs a reviewed private boundary (or written
# permission) and a separate unit change after live evidence.
for anthropic_serving_unit in claude-api.service claude-api@.service \
  claude-api-anthropic@.service; do
  grep -Fq 'CLAUDE_API_GLM_ENABLED=0' "$ROOT/systemd/$anthropic_serving_unit" \
    || wd_die "$anthropic_serving_unit can inherit the dormant GLM preview from shared env"
done
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-openai.service"
grep -Fq 'CLAUDE_API_PROVIDER=openai CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0 CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=8793' \
  "$ROOT/systemd/claude-api-openai.service"
grep -Fq 'CLAUDE_API_INSTANCE_ID=%H:engine:openai' "$ROOT/systemd/claude-api-openai.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-openai@.service"
for non_anthropic_unit in claude-api-openai.service claude-api-openai@.service \
  claude-api-gemini.service claude-api-gemini@.service \
  claude-api-kimi.service claude-api-kimi@.service; do
  grep -Fq 'CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0' "$ROOT/systemd/$non_anthropic_unit" \
    || wd_die "$non_anthropic_unit can inherit the Anthropic-only ClaudeStore switch"
done
for non_openai_unit in claude-api-anthropic@.service \
  claude-api-gemini.service claude-api-gemini@.service \
  claude-api-kimi.service claude-api-kimi@.service; do
  grep -Fq 'CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0' "$ROOT/systemd/$non_openai_unit" \
    || wd_die "$non_openai_unit can inherit the OpenAI-only ClaudeStore switch"
done
for openai_unit in claude-api-openai.service claude-api-openai@.service; do
  ! grep -Fq 'CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0' "$ROOT/systemd/$openai_unit" \
    || wd_die "$openai_unit cannot inherit the OpenAI ClaudeStore switch"
done
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
grep -Fq 'CLAUDE_API_PROVIDER=gemini CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0 CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0 CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=8795' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_INSTANCE_ID=%H:engine:gemini' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_MODELS=gemini-3.1-flash-image,gemini-3.7-flash,gemini-3.6-flash,gemini-3.5-flash,gemini-3-flash-preview,gemini-3.1-pro-preview,gemini-3.1-flash-lite,gemini-2.5-flash,gemini-2.5-flash-lite' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_ANTIGRAVITY_VERSION=2.2.1' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_NODE_BINARY=/usr/bin/node' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_NODE_VERSION=v24.18.0' "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_NODE_SHA256=41a74efb34cbde5c7632cdac0cf8bd1a14d0b8d73dc1e82755014d9a9ce70f5c' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_PROFILES_FILE=/srv/claude-api/data/gemini/profiles.json' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fq 'CLAUDE_API_GEMINI_CREDENTIAL_LAYOUT=sealed-roster' \
  "$ROOT/systemd/claude-api-gemini.service" \
  || wd_die 'legacy Gemini unit does not pin the sealed credential layout after env files'
grep -Fq 'CLAUDE_API_GEMINI_UPSTREAM=https://daily-cloudcode-pa.sandbox.googleapis.com' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fxq 'ReadOnlyPaths=/srv/claude-api/data/gemini' \
  "$ROOT/systemd/claude-api-gemini.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-gemini@.service"
grep -Fq 'CLAUDE_API_GEMINI_BATCH_ENABLED=1 CLAUDE_API_GEMINI_BATCH_PUBLIC_ENABLED=1 CLAUDE_API_PRICING_BRIDGE_ENABLED=1' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slot template does not compose reviewed Batch runtime/public flags'
grep -Fq 'CLAUDE_API_PROVIDER=gemini CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0 CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0 CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=%i' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slot template does not pin fixed provider mode and its instance port'
! grep -Fq 'CLAUDE_API_GEMINI_BATCH_ENABLED=' "$ROOT/systemd/claude-api-gemini.service" \
  || wd_die 'legacy Gemini rollback unit must not activate Batch'
! grep -Fq 'CLAUDE_API_GEMINI_BATCH_PUBLIC_ENABLED=' "$ROOT/systemd/claude-api-gemini.service" \
  || wd_die 'legacy Gemini rollback unit must not publish Batch'
! grep -Fq 'CLAUDE_API_GEMINI_BATCH_DATA_KEYS=' "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slot template must rely on the server.env Batch data keyring'
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
grep -Fq 'CLAUDE_API_GEMINI_CREDENTIAL_LAYOUT=sealed-roster' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slots do not pin the sealed credential layout after env files'
grep -Fxq 'ReadOnlyPaths=/srv/claude-api/data/gemini' \
  "$ROOT/systemd/claude-api-gemini@.service" \
  || wd_die 'Gemini slots can mutate Auth Bot roster state'
legacy_gemini_exec=$(grep -F 'ExecStart=' "$ROOT/systemd/claude-api-gemini.service" \
  | sed -e 's/CLAUDE_API_PORT=8795/CLAUDE_API_PORT=%i/' \
    -e 's/CLAUDE_API_INSTANCE_ID=%H:engine:gemini /CLAUDE_API_INSTANCE_ID=%H:engine:gemini:%i /' \
    -e 's#CLAUDE_API_BODY_SPOOL_ROOT=/run/claude-api-gemini #CLAUDE_API_BODY_SPOOL_ROOT=/run/claude-api-gemini-%i #')
slot_gemini_exec=$(grep -F 'ExecStart=' "$ROOT/systemd/claude-api-gemini@.service" \
  | sed -e 's/CLAUDE_API_GEMINI_BATCH_ENABLED=1 CLAUDE_API_GEMINI_BATCH_PUBLIC_ENABLED=1 //' \
    -e 's/CLAUDE_API_TEXT_BODY_MAX_MIB=256/CLAUDE_API_TEXT_BODY_MAX_MIB=32/' \
    -e 's/CLAUDE_API_BODY_MEMORY_BUDGET_MIB=8192/CLAUDE_API_BODY_MEMORY_BUDGET_MIB=2048/' \
    -e 's/CLAUDE_API_BODY_SPOOL_BUDGET_MIB=16384/CLAUDE_API_BODY_SPOOL_BUDGET_MIB=2048/' \
    -e 's/CLAUDE_API_BODY_MEMORY_THRESHOLD_MIB=8/CLAUDE_API_BODY_MEMORY_THRESHOLD_MIB=32/' \
    -e 's/CLAUDE_API_NONSTREAM_RESPONSE_MAX_MIB=256/CLAUDE_API_NONSTREAM_RESPONSE_MAX_MIB=64/' \
    -e 's#CLAUDE_API_BODY_SPOOL_ROOT=/var/lib/apitoken/spool/gemini-%i #CLAUDE_API_BODY_SPOOL_ROOT=/run/claude-api-gemini-%i #')
[[ $slot_gemini_exec == "$legacy_gemini_exec" ]] \
  || wd_die 'Gemini slots drifted from the reviewed roster, catalog, upstream, or wire identity pins beyond Batch activation'
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-kimi.service"
grep -Fq 'CLAUDE_API_PROVIDER=kimi CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0 CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0 CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=8804' \
  "$ROOT/systemd/claude-api-kimi.service"
grep -Fq 'CLAUDE_API_INSTANCE_ID=%H:engine:kimi' "$ROOT/systemd/claude-api-kimi.service"
grep -Fxq 'ReadOnlyPaths=/srv/claude-api/data/kimi' "$ROOT/systemd/claude-api-kimi.service"
grep -Fxq 'ReadWritePaths=/srv/claude-api/data/kimi/credentials' "$ROOT/systemd/claude-api-kimi.service"
grep -Fxq 'KillMode=mixed' "$ROOT/systemd/claude-api-kimi@.service"
grep -Fq 'CLAUDE_API_PROVIDER=kimi CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0 CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0 CLAUDE_API_TRUST_LOOPBACK=0 CLAUDE_API_HOST=127.0.0.1 CLAUDE_API_PORT=%i' \
  "$ROOT/systemd/claude-api-kimi@.service" \
  || wd_die 'KIMI slot template does not pin fixed provider mode and its instance port'
grep -Fq 'CLAUDE_API_INSTANCE_ID=%H:engine:kimi:%i' \
  "$ROOT/systemd/claude-api-kimi@.service" \
  || wd_die 'KIMI slot identities are not process-fenced by port'
grep -Fq 'ExecCondition=/usr/bin/test ! -L /srv/claude-api/releases/current/.kimi-bluegreen-v1' \
  "$ROOT/systemd/claude-api-kimi@.service" \
  || wd_die 'legacy releases can start through the KIMI slot template'
grep -Fq 'ExecCondition=/usr/bin/grep -Fxq kimi-bluegreen-v1' \
  "$ROOT/systemd/claude-api-kimi@.service" \
  || wd_die 'KIMI slot capability marker contents are not checked'
grep -Fxq 'ReadOnlyPaths=/srv/claude-api/data/kimi' \
  "$ROOT/systemd/claude-api-kimi@.service" \
  || wd_die 'KIMI slots can mutate Auth Bot roster state'
# The rotating refresh family legally reseals envelopes at runtime, so credentials/ must stay
# writable inside the read-only roster dir, or every token expiry reads as an auth death.
grep -Fxq 'ReadWritePaths=/srv/claude-api/data/kimi/credentials' \
  "$ROOT/systemd/claude-api-kimi@.service" \
  || wd_die 'KIMI slots cannot reseal rotated credential envelopes'
# The enablement pin must be argv-level in both incarnations: the plane's on/off state lives only
# in these reviewed units, never in a shared config.env value.
for kimi_unit in claude-api-kimi.service claude-api-kimi@.service; do
  grep -Fq 'CLAUDE_API_KIMI_ENABLED=1' "$ROOT/systemd/$kimi_unit" \
    || wd_die "$kimi_unit lost the argv-level KIMI enablement pin"
done
legacy_kimi_exec=$(grep -F 'ExecStart=' "$ROOT/systemd/claude-api-kimi.service" \
  | sed -e 's/CLAUDE_API_PORT=8804/CLAUDE_API_PORT=%i/' \
    -e 's/CLAUDE_API_INSTANCE_ID=%H:engine:kimi /CLAUDE_API_INSTANCE_ID=%H:engine:kimi:%i /' \
    -e 's#CLAUDE_API_BODY_SPOOL_ROOT=/run/claude-api-kimi #CLAUDE_API_BODY_SPOOL_ROOT=/run/claude-api-kimi-%i #')
slot_kimi_exec=$(grep -F 'ExecStart=' "$ROOT/systemd/claude-api-kimi@.service")
[[ $slot_kimi_exec == "$legacy_kimi_exec" ]] \
  || wd_die 'KIMI slots drifted from the reviewed provider pins'
grep -Fq 'systemctl_command kill --kill-whom=main -s SIGUSR1 "$KIMI_ACTIVE_UNIT"' \
  "$ROOT/deploy/engine-bluegreen.sh" \
  || wd_die 'KIMI blue-green cannot pre-drain its old slot'
grep -Fq 'kimi-provider-v1 >"$ENGINE_STAGE/.kimi-provider-v1"' "$ROOT/deploy/deploy.sh" \
  || wd_die "engine releases do not record KIMI provider capability"
grep -Fq 'kimi-bluegreen-v1 >"$ENGINE_STAGE/.kimi-bluegreen-v1"' "$ROOT/deploy/deploy.sh" \
  || wd_die "engine releases do not record KIMI blue-green capability"
grep -Fq '[[ -f "$directory/.kimi-bluegreen-v1" && ! -L "$directory/.kimi-bluegreen-v1" ]]' \
  "$ROOT/deploy/deploy.sh" \
  || wd_die "engine staging does not reject an unsafe KIMI blue-green capability marker"
grep -Fq '[[ $(<"$directory/.kimi-bluegreen-v1") == kimi-bluegreen-v1 ]]' \
  "$ROOT/deploy/deploy.sh" \
  || wd_die "engine staging does not validate KIMI blue-green marker contents"
grep -Fq 'KIMI_SUPPORTED' "$ROOT/deploy/engine-bluegreen.sh" \
  || wd_die 'engine blue-green does not gate the KIMI cutover on the provider capability marker'
grep -Fq '/srv/claude-api/data/kimi/credentials' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'claude-api-kimi.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'claude-api-kimi@.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq '/usr/bin/systemctl restart claude-api-kimi.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy"
grep -Fq '/usr/bin/systemctl --no-block stop claude-api-kimi.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "first KIMI slot cutover cannot retire the legacy singleton asynchronously"
grep -Fq '/usr/bin/systemctl --no-block stop claude-api-kimi@[0-9]*.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "KIMI blue-green cannot retire its old slot outside the deploy path"
grep -Fq '/usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api-kimi@8804.service' \
  "$ROOT/deploy/install-sudoers.sh" \
  || wd_die "KIMI slot drain signal is denied by the sudo policy self-check"
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
! grep -Fq 'systemctl restart apitoken-affinity-redis.service' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'Redis definition changes still stop live response history'
grep -Fq 'up -d --wait --remove-orphans' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'Redis definition changes do not use an in-place Compose reconcile'
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
# Exercise the installer's real activation fence with fake systemctl/docker commands. Unrelated
# transactions preserve the live container; an exact Redis definition change performs one in-place
# Compose reconcile after keeping the unit enabled.
redis_env_fixture="$TEMP/server.env"
redis_compose_fixture="$TEMP/affinity-redis.compose.yaml"
: >"$redis_env_fixture"
: >"$redis_compose_fixture"
redis_activation_definition=$(sed -n '/^activate_redis_definition()/,/^}/p' \
  "$ROOT/deploy/install-watchdog.sh")
redis_activation_definition=${redis_activation_definition//\/srv\/claude-api\/data\/server.env/$redis_env_fixture}
redis_activation_definition=${redis_activation_definition//\/usr\/local\/lib\/apitoken-watchdog\/controller\/affinity-redis.compose.yaml/$redis_compose_fixture}
eval "$redis_activation_definition"
redis_systemctl_log="$TEMP/redis-systemctl.log"
redis_docker_log="$TEMP/redis-docker.log"
systemctl() { printf '%s\n' "$*" >>"$redis_systemctl_log"; }
docker() { printf '%s\n' "$*" >>"$redis_docker_log"; }
REDIS_RESTART_REQUIRED=0
activate_redis_definition >/dev/null
[[ $(<"$redis_systemctl_log") == 'enable apitoken-affinity-redis.service' ]] \
  || wd_die 'unchanged infrastructure restarted Redis'
[[ ! -s $redis_docker_log ]] \
  || wd_die 'unchanged infrastructure reconciled Redis containers'
: >"$redis_systemctl_log"
REDIS_RESTART_REQUIRED=1
activate_redis_definition >/dev/null
[[ $(<"$redis_systemctl_log") == 'enable apitoken-affinity-redis.service' ]] \
  || wd_die 'changed Redis definitions restarted the systemd unit'
[[ $(<"$redis_docker_log") == "compose --env-file $redis_env_fixture -f $redis_compose_fixture up -d --wait --remove-orphans" ]] \
  || wd_die 'changed Redis definitions did not reconcile the exact Compose project in place'
redis_activation_line=$(grep -n '^activate_redis_definition$' "$ROOT/deploy/install-watchdog.sh" \
  | tail -n 1 | cut -d: -f1)
monitoring_install_line=$(grep -n '^install_monitoring_definitions$' "$ROOT/deploy/install-watchdog.sh" \
  | tail -n 1 | cut -d: -f1)
[[ $redis_activation_line -lt $monitoring_install_line ]] \
  || wd_die 'two-target monitoring is installed before the second Redis instance starts'
unset -f systemctl docker activate_redis_definition
! grep -Fq 'partners.panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"
! grep -Fq 'crm.panel.apitoken.sale {' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'import managed_admin_auth' "$ROOT/deploy/Caddyfile") -ge 5 ]]
grep -Fq 'forward_auth @managed_admin_request 127.0.0.1:8791' "$ROOT/deploy/Caddyfile"
grep -Fq 'order request_header before forward_auth' "$ROOT/deploy/Caddyfile"
grep -Fq 'order route before handle' "$ROOT/deploy/Caddyfile" \
  || wd_die 'managed admin auth route can run after a terminal application handle'
grep -Fq 'header_up Host 127.0.0.1:8791' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-Admin-Key "<ADMIN_AUTH_KEY_PLACEHOLDER>"' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-Admin-Domain {http.request.host}' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'import managed_admin_browser_auth' "$ROOT/deploy/Caddyfile") == 5 ]] \
  || wd_die 'not every managed-admin vhost exposes the same-origin browser auth projection'
grep -Fq 'handle_path /__admin-auth/* {' "$ROOT/deploy/Caddyfile"
grep -Fq 'rewrite * /v1/internal/admin-auth/browser{uri}' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'header_up X-Admin-Auth-Mode session-v1' "$ROOT/deploy/Caddyfile") == 2 ]] \
  || wd_die 'managed admin auth is not pinned to the session-v1 producer contract'
grep -Fq 'header_up X-Forwarded-Method {http.request.method}' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up X-Forwarded-Uri {http.request.uri}' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up Sec-Fetch-Dest {http.request.header.Sec-Fetch-Dest}' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up Accept {http.request.header.Accept}' "$ROOT/deploy/Caddyfile"
grep -Fq 'copy_headers X-Admin-Actor X-Admin-Account-Id Set-Cookie>X-Admin-Session-Set-Cookie' \
  "$ROOT/deploy/Caddyfile"
grep -Fq 'header @admin_session_cookie +Set-Cookie {http.request.header.X-Admin-Session-Set-Cookie}' \
  "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'request_header -X-Admin-Session-Set-Cookie' "$ROOT/deploy/Caddyfile") == 2 ]] \
  || wd_die 'temporary managed-admin cookie bridge is not cleared before and after forward auth'
grep -Fq 'request_header -Authorization' "$ROOT/deploy/Caddyfile" \
  || wd_die 'successful Basic migration still forwards credentials into application upstreams'
for browser_auth_spoof_header in X-Admin-Actor X-Admin-Account-Id \
  X-Admin-Session-Set-Cookie X-Admin-Login; do
  grep -Fq "header_up -$browser_auth_spoof_header" "$ROOT/deploy/Caddyfile" \
    || wd_die "browser auth projection accepts client-supplied $browser_auth_spoof_header"
done
[[ $(grep -Fc 'header_down -WWW-Authenticate' "$ROOT/deploy/Caddyfile") == 2 ]] \
  || wd_die 'session auth can leak a browser Basic challenge'
content_studio_vhost=$(sed -n \
  '/^content-studio\.apitoken\.sale {$/,/^monitoring\.apitoken\.sale {$/p' \
  "$ROOT/deploy/Caddyfile")
grep -Fq 'handle /v1/internal/admin-auth/* {' <<<"$content_studio_vhost" \
  || wd_die 'content studio exposes the internal browser-auth route outside its public projection'
! grep -Fqi 'X-Apitoken-Api-Plane' "$ROOT/deploy/Caddyfile"
strip_execution_identity_block=$(sed -n \
  '/^(strip_execution_identity) {$/,/^}$/p' "$ROOT/deploy/Caddyfile")
[[ $(grep -Fc 'request_header -X-Apitoken-Execution-Group' \
  <<<"$strip_execution_identity_block") == 1 ]] \
  || wd_die 'public ingress strip lost the execution-group header'
[[ $(grep -Fc 'request_header -X-Apitoken-Attempt' \
  <<<"$strip_execution_identity_block") == 1 ]] \
  || wd_die 'public ingress strip lost the execution-attempt header'
[[ $(grep -Fc 'request_header -X-Apitoken-Logical-Request-Id' \
  <<<"$strip_execution_identity_block") == 1 ]] \
  || wd_die 'public ingress strip lost the reserved logical-request header'
for execution_fenced_vhost in \
  api.apitoken.sale openai.api.apitoken.sale gemini.api.apitoken.sale router.apitoken.sale; do
  execution_fenced_block=$(sed -n "/^${execution_fenced_vhost//./\\.} {$/,/^}$/p" \
    "$ROOT/deploy/Caddyfile")
  [[ $(grep -Fc 'import strip_execution_identity' <<<"$execution_fenced_block") == 1 ]] \
    || wd_die "$execution_fenced_vhost must import the identity strip exactly once"
done
for internal_provider_origin in 8790 8792 8794 8803; do
  internal_provider_block=$(sed -n "/^http:\/\/127\.0\.0\.1:${internal_provider_origin} {$/,/^}$/p" \
    "$ROOT/deploy/Caddyfile")
  [[ -n $internal_provider_block ]] \
    || wd_die "stable provider origin $internal_provider_origin is missing"
  ! grep -Fq 'import strip_execution_identity' <<<"$internal_provider_block" \
    || wd_die "provider loopback origin $internal_provider_origin must preserve trusted internal identity"
done
internal_router_block=$(sed -n '/^http:\/\/127\.0\.0\.1:8802 {$/,/^}$/p' \
  "$ROOT/deploy/Caddyfile")
[[ -n $internal_router_block ]] || wd_die 'stable router origin 8802 is missing'
! grep -Fq 'import strip_execution_identity' <<<"$internal_router_block" \
  || wd_die 'router loopback origin 8802 must not import the public-ingress strip'
# Each backend snippet is imported exactly once — by its per-provider vhost. Since the stage-1b
# cutover the unified router.apitoken.sale vhost proxies to the claude-router process instead of
# importing plane backends (docs/engine/UNIFIED_ROUTER.md).
[[ $(grep -Fc 'import openai_engine_backend' "$ROOT/deploy/Caddyfile") == 1 ]]
[[ $(grep -Fc 'import gemini_engine_backend' "$ROOT/deploy/Caddyfile") == 1 ]]
grep -Fq 'reverse_proxy 127.0.0.1:8792' "$ROOT/deploy/Caddyfile"
grep -Fq 'http://127.0.0.1:8792 {' "$ROOT/deploy/Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8793 127.0.0.1:8797 {' "$ROOT/deploy/Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8794' "$ROOT/deploy/Caddyfile"
grep -Fq '@admin_gemini_data path /gemini-subs /gemini-subs/* /events/gemini' "$ROOT/deploy/Caddyfile"
# KIMI owns its runtime now: the dedicated default-off plane serves the sanitized projection from
# the stable loopback origin, so /kimi-subs leaves the Anthropic-origin matcher for its own route.
# GLM is still a backend inside the Anthropic runtime, so /glm-subs rides the Anthropic-origin
# matcher — never a separate origin or key while that is true.
grep -Fq '@admin_data path /overview /capacity /metrics /subs /spend-stats /fleet-history /settlement-health /glm-subs /events/engine' \
  "$ROOT/deploy/Caddyfile" || wd_die 'the Anthropic-origin admin matcher drifted'
if grep -Fq '@admin_data path /overview /capacity /metrics /subs /spend-stats /fleet-history /settlement-health /glm-subs /events/engine /kimi-subs' \
  "$ROOT/deploy/Caddyfile"; then
  wd_die 'KIMI admin projection still rides the Anthropic-origin route'
fi
! grep -Fq '@admin_glm_data' "$ROOT/deploy/Caddyfile" \
  || wd_die 'GLM must not grow a separate admin origin while it lives in the Anthropic runtime'
grep -Fq '@admin_kimi_data path /kimi-subs /events/kimi' "$ROOT/deploy/Caddyfile" \
  || wd_die 'KIMI admin projection lost its dedicated origin route'
grep -Fq '(kimi_engine_backend) {' "$ROOT/deploy/Caddyfile" \
  || wd_die 'KIMI lost its symmetric backend snippet'
grep -Fq 'http://127.0.0.1:8803 {' "$ROOT/deploy/Caddyfile" \
  || wd_die 'stable KIMI loopback origin is missing'
grep -Fq 'reverse_proxy 127.0.0.1:8804 127.0.0.1:8805 {' "$ROOT/deploy/Caddyfile" \
  || wd_die 'stable KIMI origin does not expose both blue-green slots'
kimi_stable_origin=$(sed -n '/^http:\/\/127\.0\.0\.1:8803 {$/,/^}$/p' "$ROOT/deploy/Caddyfile")
grep -Fq 'lb_policy first' <<<"$kimi_stable_origin" \
  || wd_die 'KIMI slots can round-robin the same roster during overlap'
grep -Fq 'health_uri /ready' <<<"$kimi_stable_origin" \
  || wd_die 'stable KIMI origin is not health-gated on slot readiness'
grep -Fq 'respond "No healthy KIMI runtime is available." 503' <<<"$kimi_stable_origin" \
  || wd_die 'stable KIMI origin lost its bounded no-upstream answer'
# Backend-only by contract: no public hostname and no request header may reach this plane.
if grep -Fq 'import kimi_engine_backend' "$ROOT/deploy/Caddyfile"; then
  wd_die 'backend-only KIMI grew a public vhost import'
fi
if grep -Eq '^[a-z0-9.-]*kimi[a-z0-9.-]* \{' "$ROOT/deploy/Caddyfile"; then
  wd_die 'backend-only KIMI grew a public hostname'
fi
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
grep -Fq 'managed admin session smoke failed: document_status=' \
  "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy installation does not safely diagnose a broken managed-admin session boundary'
grep -Fq 'restored and activated $backup' "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy installation can commit a broken managed-admin session boundary'
grep -Fq 'admin_document_location=absent-or-unexpected' "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy installer could leak an unexpected managed-admin redirect target'
admin_redirect_headers=$'HTTP/2 303\r\nlocation: /__admin-auth/login?return_to=%2F\r\n\r\n'
grep -Eiq '^location: /__admin-auth/login\?return_to=%2F[[:space:]]*$' \
  <<<"$admin_redirect_headers" \
  || wd_die 'managed-admin smoke rejects a valid CRLF-terminated Location header'
! grep -Eiq '^location: /__admin-auth/login\?return_to=%2F[[:space:]]*$' \
  <<<$'location: /__admin-auth/login?return_to=%2F.evil\r\n' \
  || wd_die 'managed-admin smoke accepts a Location suffix outside the login route'
grep -Fq 'https://crm.apitoken.sale/__admin-auth/login?return_to=%2F' \
  "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy installation does not exercise the public same-origin login projection'
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
gemini_api_vhost=$(sed -n '/^gemini\.api\.apitoken\.sale {$/,/^router\.apitoken\.sale {$/p' \
  "$ROOT/deploy/Caddyfile")
grep -Fq 'import engine_backend' <<<"$claude_api_vhost"
! grep -Fq 'openai_engine_backend' <<<"$claude_api_vhost"
! grep -Fq '/upload/v1beta/*' <<<"$claude_api_vhost" \
  || wd_die 'Anthropic public vhost exposes the Gemini upload perimeter'
grep -Fq 'import openai_engine_backend' <<<"$openai_api_vhost"
! grep -Fq '/upload/v1beta/*' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI public vhost exposes the Gemini upload perimeter'
grep -Fq '@public_core path /v1beta/* /upload/v1beta/* /health /balance' <<<"$gemini_api_vhost" \
  || wd_die 'Gemini public vhost lost its native Batch/Files perimeter'
grep -Fq 'import gemini_engine_backend' <<<"$gemini_api_vhost" \
  || wd_die 'Gemini Batch/Files public paths bypass the stable Gemini backend'
[[ $(grep -Fc 'encode zstd gzip {' <<<"$openai_api_vhost") == 1 ]] \
  || wd_die 'OpenAI public TLS boundary must have exactly one compression policy'
grep -Fq 'minimum_length 512' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI compression can spend CPU on tiny response bodies'
grep -Fq 'header Content-Type application/json*' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI compression is not restricted to complete JSON documents'
! grep -Fq 'text/event-stream' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI compression matcher can buffer SSE lifecycle frames'
# Unified router vhost and the stable loopback origin share one root-owned runtime backend snippet.
# The blue-green controller replaces that snippet atomically; the public path never lists slots or
# gains compression, and Prometheus never has to discover the active slot.
router_vhost=$(sed -n '/^router\.apitoken\.sale {$/,/^}$/p' "$ROOT/deploy/Caddyfile")
grep -Fq '@public_core path /v1/messages* /v1/responses* /v1/chat/completions /v1/images/* /v1/models* /v1beta/* /upload/v1beta/* /health /balance' \
  <<<"$router_vhost" \
  || wd_die 'unified router must forward exactly the documented public contract'
grep -Fq 'import router_backend' <<<"$router_vhost" \
  || wd_die 'unified router does not consume the atomically selected backend for Batch/Files paths'
grep -Fq 'import model_request_body' <<<"$claude_api_vhost" \
  || wd_die 'Anthropic public vhost lost the streaming 256 MiB request cap'
grep -Fq 'import model_request_body' <<<"$openai_api_vhost" \
  || wd_die 'OpenAI public vhost lost the streaming 256 MiB request cap'
grep -Fq 'import model_request_body' <<<"$gemini_api_vhost" \
  || wd_die 'Gemini public vhost lost the streaming 256 MiB request cap'
grep -Fq 'import model_request_body' <<<"$router_vhost" \
  || wd_die 'unified router lost the streaming 256 MiB request cap'
grep -Fq 'max_size 256MiB' "$ROOT/deploy/Caddyfile" \
  || wd_die 'model-vhost request cap is not the compile-time 256 MiB ceiling'
grep -Fq 'max_size 32KB' "$ROOT/deploy/Caddyfile" \
  || wd_die 'OpenKeys request cap is no longer 32KB'
! grep -Fq 'flush_interval' "$ROOT/deploy/Caddyfile" \
  || wd_die 'Caddy must not set flush_interval (SSE buffering risk)'
! grep -Fq 'reverse_proxy' <<<"$router_vhost" \
  || wd_die 'public router vhost bypasses the runtime backend selector'
grep -Fq 'import /etc/caddy/router-active.caddy' "$ROOT/deploy/Caddyfile" \
  || wd_die 'Caddy never loads the root-owned router backend state'
router_stable_origin=$(sed -n '/^http:\/\/127\.0\.0\.1:8802 {$/,/^}$/p' \
  "$ROOT/deploy/Caddyfile")
grep -Fq 'bind 127.0.0.1' <<<"$router_stable_origin"
grep -Fq 'import router_backend' <<<"$router_stable_origin" \
  || wd_die 'stable router origin can drift from the public active slot'
! grep -Eq '^[[:space:]]*import (engine_backend|openai_engine_backend|gemini_engine_backend)([[:space:]]|$)' \
  <<<"$router_vhost" \
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
grep -Fq 'handle /proxy-admin/* {' "$ROOT/deploy/Caddyfile"
grep -Fq 'rewrite /events/engine /admin-events' "$ROOT/deploy/Caddyfile"
grep -Fq 'rewrite /events/openai /admin-events' "$ROOT/deploy/Caddyfile"
grep -Fq 'rewrite /events/gemini /admin-events' "$ROOT/deploy/Caddyfile"
grep -Fq 'rewrite /events/kimi /admin-events' "$ROOT/deploy/Caddyfile"
grep -Fq 'reverse_proxy 127.0.0.1:8806' "$ROOT/deploy/Caddyfile"
grep -Fq 'header_up Host 127.0.0.1:8806' "$ROOT/deploy/Caddyfile"
[[ $(grep -Fc 'header_up X-Proxy-Admin-Key "<AUTH_BOT_PROXY_ADMIN_KEY_PLACEHOLDER>"' \
  "$ROOT/deploy/Caddyfile") == 1 ]] \
  || wd_die 'proxy-admin route must overwrite its exact header with one dedicated placeholder'
proxy_admin_route=$(sed -n '/^[[:space:]]*handle \/proxy-admin\/\* {$/,/^[[:space:]]*}$/p' \
  "$ROOT/deploy/Caddyfile")
[[ $(grep -Fc 'header_up x-api-key "<ADMIN_CONTROL_KEY_PLACEHOLDER>"' \
  <<<"$proxy_admin_route") == 1 ]] \
  || wd_die 'proxy-admin route lacks the old-binary rollback compatibility key'
grep -Fq 'new binary authenticates only X-Proxy-Admin-Key' "$ROOT/deploy/Caddyfile" \
  || wd_die 'proxy-admin compatibility header lacks an explicit new-binary boundary'
grep -Fq 'header_up X-Admin-Actor {http.request.header.X-Admin-Actor}' "$ROOT/deploy/Caddyfile"
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
grep -Fq 'request>headers>Authorization replace REDACTED' "$ROOT/deploy/Caddyfile" \
  || wd_die 'managed-admin Basic migration credentials can enter Caddy logs'
grep -Fq 'request>headers>Cookie replace REDACTED' "$ROOT/deploy/Caddyfile" \
  || wd_die 'managed-admin session cookies can enter Caddy logs'
grep -Fq 'request>headers>X-Admin-Session-Set-Cookie replace REDACTED' "$ROOT/deploy/Caddyfile" \
  || wd_die 'managed-admin cookie bridge can enter Caddy logs'
grep -Fq 'request>headers>X-Proxy-Admin-Key replace REDACTED' "$ROOT/deploy/Caddyfile" \
  || wd_die 'dedicated proxy-admin header is not redacted from Caddy runtime errors'
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
grep -Fq 'host_postgres_ready' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'test database helper never proves the published host listener'
grep -Fq '"/dev/tcp/127.0.0.1/$PORT"' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'test database helper does not probe the published host listener'
grep -Fq -- '--tmpfs /var/lib/postgresql:rw,noexec,nosuid,size=2g' \
  "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'test database data volume cannot hold the full candidate WAL and analytical matrices'
grep -Fq -- '--shm-size=256m' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'test database lacks bounded shared memory for parallel analytical tests'

# The shared cache-affinity L2 is the only place two engine slots agree on a prompt-cache home. Its
# single-winner and opaque-keyspace invariants were unprovable while the gate ran without Redis, so
# the disposable instance and its mandatory wiring are pinned here. Losing any of these silently
# returns ~350 lines of L2 code and the tenant-privacy assertions to zero executed coverage.
redis_base_port=$(sed -n 's/^REDIS_BASE_PORT=${WATCHDOG_REDIS_PORT:-\([0-9]*\)}$/\1/p' \
  "$ROOT/deploy/watchdog-test-db.sh")
[[ -n $redis_base_port ]] || wd_die 'could not read the disposable test Redis base port'
for redis_slot in 0 1 2; do
  redis_slot_port=$((redis_base_port + redis_slot))
  (( redis_slot_port < 32768 )) \
    || wd_die "test Redis port $redis_slot_port is inside the ephemeral range and will collide"
  (( redis_slot_port < test_db_base_port || redis_slot_port > test_db_base_port + 2 )) \
    || wd_die "test Redis port $redis_slot_port collides with the disposable PostgreSQL range"
done
grep -Fq 'REDIS_NAME=apitoken-watchdog-redis-$SLOT' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'parallel test Redis slots do not have distinct container names'
grep -Fq -- '--label apitoken.watchdog=test-redis' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'test Redis ownership is not labelled'
grep -Fq 'redis-url)' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'the test helper cannot hand out a disposable Redis URL'
# A --rm container must leave nothing behind, and the gate asserts protocol behaviour rather than
# persistence. Durability flags here would only make the lane slower and flakier.
grep -Fq -- "--save '' --appendonly no" "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'the disposable Redis must stay in-memory only'
grep -Fq 'if redis_is_owned; then' "$ROOT/deploy/watchdog-test-db.sh" \
  || wd_die 'the disposable Redis is not torn down with the database'
grep -Fq 'CLAUDE_API_TEST_REDIS_URL="$redis_url"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the Rust lane does not receive the disposable Redis URL'
grep -Fq 'wd_die "the Rust lane requires a disposable Redis URL"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the Rust lane accepts a missing Redis URL instead of failing closed'
grep -Fq 'redis_url=$(test_db redis-url)' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the candidate gate never resolves a disposable Redis URL'
# CI=1 converts the suite's local "no Redis" escape hatch into a hard failure. Without it the
# coverage could regress to silently skipped while every lane stayed green.
grep -Fq 'CI=1 \' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the Rust lane does not mark itself as CI for the mandatory-Redis assertion'
grep -Fq 'local candidate=$1 engine_dsn=$2 build_artifacts=$3 redis_url=$4 sha=$5' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the Rust lane does not accept the exact candidate SHA'
grep -Fq '"$redis_url" "$sha" &' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the candidate gate does not pass its exact SHA into the Rust lane'
grep -Fq 'run_as_ci env CLAUDE_API_IMPLEMENTATION_SHA="$sha" \' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the trusted claude-api build does not embed the validated candidate SHA'
grep -Fq 'run env CARGO_TARGET_DIR="$ENGINE_STAGE/target" CLAUDE_API_IMPLEMENTATION_SHA="$SHA" \' \
  "$ROOT/deploy/deploy.sh" \
  || wd_die 'the manual fallback claude-api build does not embed the validated release SHA'
[[ $(grep -Fc 'env -u CLAUDE_API_IMPLEMENTATION_SHA' "$ROOT/deploy/deploy.sh") -eq 2 ]] \
  || wd_die 'manual authbot/router builds can inherit the claude-api implementation SHA'
! grep -Fq '#[ignore' "$ROOT/crates/forward/src/affinity.rs" \
  || wd_die 'the shared affinity Redis proof is ignored again and the gate would never run it'
grep -Fq 'CLAUDE_API_TEST_REDIS_URL must be set in CI' "$ROOT/crates/forward/src/affinity.rs" \
  || wd_die 'the affinity suite lost its mandatory-Redis assertion'

# The authbot produces the subscriptions the engine serves from, so the production watchdog builds
# it once beside the tested engine and the release controller only promotes those exact binaries.
grep -Fq 'cargo build --locked --release -p claude-api --manifest-path "$candidate/Cargo.toml"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die "the candidate gate does not build the production engine"
grep -Fq 'cargo build --locked --release -p authbot -p claude-router \' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die "the candidate gate does not build the authbot and router artifacts"
grep -Fq 'run_as_ci env -u CLAUDE_API_IMPLEMENTATION_SHA \' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'authbot and router builds can inherit the claude-api implementation SHA'
grep -Fq '"$TESTED_CANDIDATE/.deploy-artifacts/engine/authbot"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the release controller does not promote the tested authbot"
grep -Fq '"$ENGINE_STAGE/authbot"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the authbot binary is not installed into the engine release"
grep -Fq 'staged authbot binary is missing' "$ROOT/deploy/deploy.sh" \
  || wd_die "a release without an authbot binary must fail closed"
authbot_exec_start='ExecStart=/usr/bin/env CLAUDE_BIN=/run/claude-authbot/claude AUTH_BOT_CLAUDE_BIN=/run/claude-authbot/claude AUTH_BOT_CODEX_HOMES_DIR=/srv/claude-api/data/codex-staging AUTH_BOT_CODEX_ROSTER_DIR=/srv/claude-api/data/codex AUTH_BOT_PROXY_ADMIN_KEY_FILE=%d/proxy-admin.key /srv/claude-api/releases/current/authbot'
grep -Fxq "$authbot_exec_start" "$ROOT/systemd/claude-authbot.service" \
  || wd_die 'the authbot unit must pin runtime paths and the credential path in its exact command'
grep -Fq 'ProtectHome=true' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot must keep the service user's home hidden"
grep -Fxq 'ProtectProc=invisible' "$ROOT/systemd/claude-authbot.service" \
  || wd_die 'the authbot can inspect unrelated host processes'
grep -Fxq 'ProcSubset=pid' "$ROOT/systemd/claude-authbot.service" \
  || wd_die 'the authbot procfs view exposes non-process kernel state'
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
grep -Fxq 'LoadCredential=proxy-admin.key:/etc/apitoken/proxy-admin.key' \
  "$ROOT/systemd/claude-authbot.service" \
  || wd_die 'the authbot unit does not load the root-owned raw proxy-admin key as a credential'
! grep -Eq '^Environment=AUTH_BOT_PROXY_ADMIN_KEY_FILE=' \
  "$ROOT/systemd/claude-authbot.service" \
  || wd_die 'an Environment directive lets environment files redirect the proxy-admin credential'
! grep -Eq '^(Environment|ExecStart)=.*AUTH_BOT_PROXY_ADMIN_KEY=' \
  "$ROOT/systemd/claude-authbot.service" \
  || wd_die 'the authbot unit still injects the proxy-admin secret as an environment value'
grep -Fq 'EnvironmentFile=/srv/claude-api/data/engine-postgres.env' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot can silently fall back from the engine PostgreSQL authority"
grep -Fq 'EnvironmentFile=/srv/claude-api/data/authbot.env' "$ROOT/systemd/claude-authbot.service" \
  || wd_die 'authbot.env is not retained for the bot other settings'
grep -Fq 'EnvironmentFile=-/srv/claude-api/data/server.env' "$ROOT/systemd/claude-authbot.service" \
  || wd_die "the authbot runtime status calls cannot receive the shared control key"
authbot_exec_line=$(grep -nFx "$authbot_exec_start" "$ROOT/systemd/claude-authbot.service" | cut -d: -f1)
authbot_last_env_line=$(grep -n '^EnvironmentFile=' "$ROOT/systemd/claude-authbot.service" \
  | tail -n 1 | cut -d: -f1)
[[ -n $authbot_exec_line && -n $authbot_last_env_line \
    && $authbot_last_env_line -lt $authbot_exec_line ]] \
  || wd_die 'the authbot command does not pin its credential path after environment-file loading'
! grep -Fq 'env_opt("AUTH_BOT_PROXY_ADMIN_KEY")' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die 'the authbot still accepts the proxy-admin secret from the process environment'
grep -Fq 'env_opt("AUTH_BOT_PROXY_ADMIN_KEY_FILE")' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die 'the authbot does not resolve the proxy-admin credential by path'
grep -Fq 'libc::PR_SET_DUMPABLE, 0' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die 'the authbot does not block same-UID process memory inspection before loading secrets'
hardening_line=$(grep -n '^[[:space:]]*harden_daemon_process()?;' "$ROOT/crates/authbot/src/main.rs" \
  | cut -d: -f1)
proxy_key_load_line=$(grep -n '^[[:space:]]*let proxy_admin_key = read_proxy_admin_key' \
  "$ROOT/crates/authbot/src/main.rs" | cut -d: -f1)
[[ -n $hardening_line && -n $proxy_key_load_line && $hardening_line -lt $proxy_key_load_line ]] \
  || wd_die 'authbot loads the proxy-admin key before disabling process dumpability'
grep -Fq 'provision_authbot_proxy_admin_key' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'watchdog installation never provisions the dedicated proxy-admin key'
grep -Fq '[[ -f $key_file && ! -L $key_file ]]' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'raw proxy-admin key provisioning is not regular-file/symlink fenced'
grep -Fq 'install -d -o root -g root -m 0755 "$key_dir"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'the proxy-admin key parent is not root-owned and non-writable by deploy'
grep -Fq 'chown root:root "$key_file"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'raw proxy-admin key root ownership is not enforced'
grep -Fq 'chmod 0600 "$key_file"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'raw proxy-admin key mode is not enforced'
grep -Fq '[[ -f $authbot_env && ! -L $authbot_env ]]' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'legacy authbot.env migration is not regular-file/symlink fenced'
grep -Fq 'chown root:root "$env_candidate"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'migrated authbot.env ownership is not enforced'
grep -Fq 'chmod 0600 "$authbot_env"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'migrated authbot.env mode is not enforced'
grep -Fxq 'PROXY_ADMIN_KEY_FILE=${PROXY_ADMIN_KEY_FILE:-/etc/apitoken/proxy-admin.key}' \
  "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy does not read the canonical root-owned proxy-admin key path'
grep -Fq 'awk -v proxy_admin_key_file="$PROXY_ADMIN_KEY_FILE" -v render_output="$tmp" \' \
  "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy renderer does not receive only the private raw-key path'
[[ $(grep -Ec '^[[:space:]]*awk -v ' "$ROOT/deploy/install-caddy.sh") == 1 ]] \
  || wd_die 'Caddy installer has an unexpected secret-bearing AWK invocation'
! grep -Eq -- '-v [[:alnum:]_]*(key|secret)=' "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy installer passes a secret value through AWK argv'
! grep -Fq 'AUTHBOT_ENV' "$ROOT/deploy/install-caddy.sh" \
  || wd_die 'Caddy still reads the mixed-settings authbot.env secret source'
provision_line=$(grep -n '^[[:space:]]*provision_authbot_proxy_admin_key$' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
authbot_unit_install_line=$(grep -n 'install -o root -g root -m 0644 "$ROOT/systemd/$unit"' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
[[ -n $provision_line && -n $authbot_unit_install_line \
    && $provision_line -lt $authbot_unit_install_line ]] \
  || wd_die 'authbot raw key is provisioned after systemd definitions'
full_install_line=$(grep -nF '"$CANDIDATE/deploy/install-watchdog.sh"' \
  "$ROOT/deploy/watchdog-infrastructure.sh" | head -n 1 | cut -d: -f1)
combined_systemd_line=$(grep -nF '"$CANDIDATE/deploy/install-watchdog.sh" --systemd-only' \
  "$ROOT/deploy/watchdog-infrastructure.sh" | cut -d: -f1)
combined_caddy_line=$(grep -nF 'CADDY_TEMPLATE="$CANDIDATE/deploy/Caddyfile" "$CANDIDATE/deploy/install-caddy.sh" --check' \
  "$ROOT/deploy/watchdog-infrastructure.sh" | cut -d: -f1)
[[ -n $full_install_line && -n $combined_systemd_line && -n $combined_caddy_line \
    && $full_install_line -lt $combined_caddy_line \
    && $combined_systemd_line -lt $combined_caddy_line ]] \
  || wd_die 'combined infrastructure can render Caddy before raw-key provisioning'
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
# Non-dumpability intentionally prevents same-UID /proc executable inspection. A fixed root helper
# exposes either a bounded comparison result or one canonical immutable release SHA.
authbot_runtime_helper="$ROOT/deploy/authbot-runtime-state.sh"
grep -Fq 'UNIT=claude-authbot.service' "$authbot_runtime_helper" \
  || wd_die 'authbot runtime verification does not hard-code its unit'
grep -Fq 'RELEASE_ROOT=/srv/claude-api/releases' "$authbot_runtime_helper" \
  || wd_die 'authbot release inspection does not hard-code its immutable root'
grep -Fq '/usr/bin/systemctl' "$authbot_runtime_helper" \
  || wd_die 'authbot runtime verification does not pin systemctl'
grep -Fq '/usr/bin/sha256sum -- "/proc/$pid/exe"' "$authbot_runtime_helper" \
  || wd_die 'authbot runtime verification does not hash only the live procfs executable'
grep -Fq '/usr/bin/readlink -f -- "/proc/$pid/exe"' "$authbot_runtime_helper" \
  || wd_die 'authbot release inspection does not canonically resolve only the live executable'
grep -Fq 'release-sha) mode=release-sha' "$authbot_runtime_helper" \
  || wd_die 'authbot runtime verification does not require literal release-sha mode'
grep -Fq '[[ $final_load_state == loaded && $final_state == active && $final_pid == "$pid" ]]' \
  "$authbot_runtime_helper" \
  || wd_die 'authbot runtime verification accepts service churn around inspection'
[[ $(grep -Ec "printf '%s\\\\n' (exact|different)" "$authbot_runtime_helper") -eq 2 \
    && $(grep -Fc "printf '%s\\n' inactive" "$authbot_runtime_helper") -eq 2 ]] \
  || wd_die 'authbot digest verification exposes an unexpected result vocabulary'
! grep -Eq '(readlink|cmp)[[:space:]]' "$ROOT/deploy/deploy.sh" \
  || wd_die 'deploy still dereferences a non-dumpable authbot process as deploy'
grep -Fq 'reconcile_authbot_release()' "$ROOT/deploy/lib.sh" \
  || wd_die 'authbot release reconciliation is not shared by deploy and rollback'
grep -Fq 'expected=$(sha256_file "$release/authbot")' "$ROOT/deploy/lib.sh" \
  || wd_die 'release reconciliation does not compute the expected authbot digest unprivileged'
grep -Fq 'privileged_command "$state_helper" "$expected"' "$ROOT/deploy/lib.sh" \
  || wd_die 'release reconciliation does not use the narrow root runtime verifier'
[[ $(grep -Fc 'privileged_command "$state_helper" "$expected"' "$ROOT/deploy/lib.sh") -eq 2 ]] \
  || wd_die 'authbot runtime verification must run exactly before and after a changed restart'
grep -Fq 'if [[ "$state" != exact ]]' "$ROOT/deploy/lib.sh" \
  || wd_die 'authbot post-restart verification is not exact-release only'
grep -Fq '$unit failed exact-release verification after restart' "$ROOT/deploy/lib.sh" \
  || wd_die "a crashed authbot can still produce a green deployment"
[[ $(grep -Fc 'reconcile_authbot_release "$ENGINE_RELEASE"' "$ROOT/deploy/deploy.sh") -eq 1 ]] \
  || wd_die 'normal engine activation does not use the shared authbot reconciliation'
[[ $(grep -Fc 'reconcile_authbot_after_engine_restore "$ENGINE_RELEASE_ROOT" "$ENGINE_ORIGINAL"' \
  "$ROOT/deploy/deploy.sh") -eq 1 ]] \
  || wd_die 'failed deploy activation does not guard original authbot reconciliation by restored current'
[[ $(grep -Fc 'reconcile_authbot_release "$ENGINE_TARGET"' "$ROOT/deploy/rollback.sh") -eq 1 ]] \
  || wd_die 'engine blue-green rollback does not reconcile authbot to its selected target'
[[ $(grep -Fc 'reconcile_authbot_after_engine_restore "$ENGINE_RELEASE_ROOT" "$ENGINE_ORIGINAL"' \
  "$ROOT/deploy/rollback.sh") -eq 1 ]] \
  || wd_die 'failed rollback selection does not guard original authbot reconciliation by restored current'
restore_links_line=$(grep -nF 'restore_activation_links || recovery_failed=1' "$ROOT/deploy/lib.sh" | cut -d: -f1)
recovery_callback_line=$(grep -nF 'if ! "$ACTIVATION_RECOVERY_CALLBACK"' "$ROOT/deploy/lib.sh" | cut -d: -f1)
[[ -n $restore_links_line && -n $recovery_callback_line && $restore_links_line -lt $recovery_callback_line ]] \
  || wd_die 'activation recovery can reconcile authbot before restoring engine links'
grep -A2 -F 'publish_authbot_runtime_helper()' "$ROOT/deploy/install-watchdog.sh" \
  | grep -Fq '"$ROOT/deploy/authbot-runtime-state.sh"' \
  || wd_die 'the root authbot runtime verifier is not installed with controller definitions'
authbot_helper_install_line=$(grep -nF '"$ROOT/deploy/authbot-runtime-state.sh"' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
authbot_deploy_install_line=$(grep -nF '"$ROOT/deploy/deploy.sh"' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
[[ -n $authbot_helper_install_line && -n $authbot_deploy_install_line \
    && $authbot_helper_install_line -lt $authbot_deploy_install_line ]] \
  || wd_die 'deploy controller is installed before its authbot runtime verifier'
! grep -Fq 'deferring adoption until the service is already inactive' "$ROOT/deploy/deploy.sh" \
  || wd_die "changed authbot code can remain undeployed forever"
# Asking for a world-readable unit file under sudo earns a policy denial that is indistinguishable
# from the unit being absent — which is exactly how the first attempt silently skipped the restart.
! grep -Fq 'privileged_command test -f "/etc/systemd/system/$unit"' "$ROOT/deploy/deploy.sh" \
  || wd_die "the authbot unit check must not require sudo"

# Execute the helper with fixed command stubs: malformed arguments and inspection failures are silent,
# digest mode has its three bounded states, and release mode emits only an exact immutable SHA.
authbot_helper_fixture="$TEMP/authbot-runtime-state.sh"
sed \
  -e 's#\[\[ ${EUID:-$(id -u)} -eq 0 \]\] || exit 1#:#' \
  -e 's#/usr/bin/systemctl#"$AUTHBOT_TEST_SYSTEMCTL"#g' \
  -e 's#/usr/bin/sha256sum#"$AUTHBOT_TEST_SHA256SUM"#g' \
  -e 's#/usr/bin/readlink#"$AUTHBOT_TEST_READLINK"#g' \
  "$authbot_runtime_helper" >"$authbot_helper_fixture"
chmod +x "$authbot_helper_fixture"
authbot_systemctl_stub="$TEMP/authbot-systemctl"
authbot_sha_stub="$TEMP/authbot-sha256sum"
authbot_readlink_stub="$TEMP/authbot-readlink"
cat >"$authbot_systemctl_stub" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
property=
for arg in "$@"; do
  case "$arg" in
    --property=*) property=${arg#--property=} ;;
  esac
done
case "$property" in
  LoadState)
    count=1
    if [[ -n ${AUTHBOT_TEST_LOAD_COUNT:-} ]]; then
      count=$(cat "$AUTHBOT_TEST_LOAD_COUNT")
      count=$((count + 1))
      printf '%s\n' "$count" >"$AUTHBOT_TEST_LOAD_COUNT"
    fi
    if [[ ${AUTHBOT_TEST_MODE:-exact} == load-churn && $count -gt 1 ]]; then
      printf '%s\n' not-found
    else
      printf '%s\n' "${AUTHBOT_TEST_LOAD_STATE:-loaded}"
    fi
    ;;
  ActiveState)
    count=$(cat "$AUTHBOT_TEST_ACTIVE_COUNT")
    count=$((count + 1))
    printf '%s\n' "$count" >"$AUTHBOT_TEST_ACTIVE_COUNT"
    if [[ ${AUTHBOT_TEST_MODE:-exact} == churn && $count -gt 1 ]]; then
      printf '%s\n' inactive
    else
      printf '%s\n' "${AUTHBOT_TEST_ACTIVE_STATE:-active}"
    fi
    ;;
  MainPID)
    count=1
    if [[ -n ${AUTHBOT_TEST_PID_COUNT:-} ]]; then
      count=$(cat "$AUTHBOT_TEST_PID_COUNT")
      count=$((count + 1))
      printf '%s\n' "$count" >"$AUTHBOT_TEST_PID_COUNT"
    fi
    if [[ ${AUTHBOT_TEST_MODE:-exact} == pid-churn && $count -gt 1 ]]; then
      printf '%s\n' 4343
    else
      printf '%s\n' "${AUTHBOT_TEST_PID:-4242}"
    fi
    ;;
  *) exit 1 ;;
esac
EOF
cat >"$authbot_sha_stub" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ ${AUTHBOT_TEST_HASH_FAIL:-0} != 1 ]] || exit 1
[[ $# -eq 2 && $1 == -- && $2 == /proc/4242/exe ]] || exit 1
printf '%s  %s\n' "$AUTHBOT_TEST_OBSERVED" "$2"
EOF
cat >"$authbot_readlink_stub" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ ${AUTHBOT_TEST_READLINK_FAIL:-0} != 1 ]] || exit 1
[[ $# -eq 3 && $1 == -f && $2 == -- && $3 == /proc/4242/exe ]] || exit 1
printf '%s\n' "$AUTHBOT_TEST_RESOLVED"
EOF
chmod +x "$authbot_systemctl_stub" "$authbot_sha_stub" "$authbot_readlink_stub"
authbot_expected=$(printf expected | wd_sha256_stdin)
authbot_different=$(printf different | wd_sha256_stdin)
authbot_active_count="$TEMP/authbot-active-count"
authbot_pid_count="$TEMP/authbot-pid-count"
authbot_load_count="$TEMP/authbot-load-count"
run_authbot_helper() {
  printf '0\n' >"$authbot_active_count"
  printf '0\n' >"$authbot_pid_count"
  printf '0\n' >"$authbot_load_count"
  AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub \
    AUTHBOT_TEST_SHA256SUM=$authbot_sha_stub AUTHBOT_TEST_READLINK=$authbot_readlink_stub \
    AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count AUTHBOT_TEST_PID_COUNT=$authbot_pid_count \
    AUTHBOT_TEST_LOAD_COUNT=$authbot_load_count \
    AUTHBOT_TEST_OBSERVED=$authbot_expected "$authbot_helper_fixture" "$@"
}
[[ $(run_authbot_helper "$authbot_expected") == exact ]] \
  || wd_die 'authbot runtime helper did not recognize the exact live binary'
printf '0\n' >"$authbot_load_count"
[[ $(AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub AUTHBOT_TEST_LOAD_COUNT=$authbot_load_count \
  AUTHBOT_TEST_LOAD_STATE=not-found "$authbot_helper_fixture" "$authbot_expected") == inactive ]] \
  || wd_die 'authbot runtime helper did not treat a missing unit as inactive'
printf '0\n' >"$authbot_load_count"
authbot_missing_release_output=$(AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub \
  AUTHBOT_TEST_LOAD_COUNT=$authbot_load_count AUTHBOT_TEST_LOAD_STATE=not-found \
  "$authbot_helper_fixture" release-sha) \
  || wd_die 'authbot release helper rejected a missing unit'
[[ -z $authbot_missing_release_output ]] \
  || wd_die 'authbot release helper emitted output for a missing unit'
AUTHBOT_TEST_OBSERVED=$authbot_different
printf '0\n' >"$authbot_active_count"
[[ $(AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub \
  AUTHBOT_TEST_SHA256SUM=$authbot_sha_stub AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count \
  AUTHBOT_TEST_OBSERVED=$authbot_different "$authbot_helper_fixture" "$authbot_expected") == different ]] \
  || wd_die 'authbot runtime helper did not report a changed live binary'
printf '0\n' >"$authbot_active_count"
[[ $(AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub \
  AUTHBOT_TEST_SHA256SUM=$authbot_sha_stub AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count \
  AUTHBOT_TEST_OBSERVED=$authbot_expected AUTHBOT_TEST_ACTIVE_STATE=inactive \
  "$authbot_helper_fixture" "$authbot_expected") == inactive ]] \
  || wd_die 'authbot runtime helper did not report an inactive service'
for malformed_args in '' 'abc' 'release-path' 'release-sha extra' "$authbot_expected extra"; do
  read -r -a helper_args <<<"$malformed_args"
  helper_log="$TEMP/authbot-helper-malformed.log"
  if run_authbot_helper "${helper_args[@]}" >"$helper_log" 2>&1; then
    wd_die "authbot runtime helper accepted malformed arguments: $malformed_args"
  fi
  [[ ! -s $helper_log ]] || wd_die 'authbot runtime helper leaked malformed input details'
done
for helper_failure in hash churn; do
  printf '0\n' >"$authbot_active_count"
  helper_log="$TEMP/authbot-helper-$helper_failure.log"
  if AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub \
      AUTHBOT_TEST_SHA256SUM=$authbot_sha_stub AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count \
      AUTHBOT_TEST_OBSERVED=$authbot_expected AUTHBOT_TEST_HASH_FAIL=$([[ $helper_failure == hash ]] && printf 1 || printf 0) \
      AUTHBOT_TEST_MODE=$helper_failure "$authbot_helper_fixture" "$authbot_expected" \
      >"$helper_log" 2>&1; then
    wd_die "authbot runtime helper accepted $helper_failure during inspection"
  fi
  [[ ! -s $helper_log ]] || wd_die 'authbot runtime helper leaked a digest or procfs path on failure'
done

authbot_release_sha=0123456789abcdef0123456789abcdef01234567
printf '0\n' >"$authbot_active_count"
[[ $(AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub AUTHBOT_TEST_READLINK=$authbot_readlink_stub \
  AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count \
  AUTHBOT_TEST_RESOLVED="/srv/claude-api/releases/$authbot_release_sha/authbot" \
  "$authbot_helper_fixture" release-sha) == "$authbot_release_sha" ]] \
  || wd_die 'authbot runtime helper did not emit the exact canonical live release SHA'
printf '0\n' >"$authbot_active_count"
authbot_inactive_release_output=$(AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub \
  AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count AUTHBOT_TEST_ACTIVE_STATE=inactive \
  "$authbot_helper_fixture" release-sha) \
  || wd_die 'authbot runtime helper rejected an inactive release inspection'
[[ -z $authbot_inactive_release_output ]] \
  || wd_die 'authbot runtime helper emitted output for an inactive release inspection'
for rejected_path in \
  "/srv/claude-api/releases/current/authbot" \
  "/srv/claude-api/releases/${authbot_release_sha^^}/authbot" \
  "/srv/claude-api/releases/$authbot_release_sha/bin/authbot" \
  "/opt/apitoken/releases/$authbot_release_sha/authbot"; do
  printf '0\n' >"$authbot_active_count"
  helper_log="$TEMP/authbot-release-path.log"
  if AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub AUTHBOT_TEST_READLINK=$authbot_readlink_stub \
      AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count AUTHBOT_TEST_RESOLVED=$rejected_path \
      "$authbot_helper_fixture" release-sha >"$helper_log" 2>&1; then
    wd_die "authbot runtime helper accepted a non-canonical release path: $rejected_path"
  fi
  [[ ! -s $helper_log ]] || wd_die 'authbot release helper leaked a rejected path'
done
for release_failure in readlink churn pid-churn load-churn; do
  printf '0\n' >"$authbot_active_count"
  printf '0\n' >"$authbot_pid_count"
  printf '0\n' >"$authbot_load_count"
  helper_log="$TEMP/authbot-release-$release_failure.log"
  if AUTHBOT_TEST_SYSTEMCTL=$authbot_systemctl_stub AUTHBOT_TEST_READLINK=$authbot_readlink_stub \
      AUTHBOT_TEST_ACTIVE_COUNT=$authbot_active_count AUTHBOT_TEST_PID_COUNT=$authbot_pid_count \
      AUTHBOT_TEST_LOAD_COUNT=$authbot_load_count \
      AUTHBOT_TEST_RESOLVED="/srv/claude-api/releases/$authbot_release_sha/authbot" \
      AUTHBOT_TEST_READLINK_FAIL=$([[ $release_failure == readlink ]] && printf 1 || printf 0) \
      AUTHBOT_TEST_MODE=$release_failure "$authbot_helper_fixture" release-sha \
      >"$helper_log" 2>&1; then
    wd_die "authbot runtime helper accepted $release_failure during release inspection"
  fi
  [[ ! -s $helper_log ]] || wd_die 'authbot release helper leaked a path or SHA on failure'
done

# Exercise the shared reconciliation state machine independently from host systemd. Exact state
# preserves the process; changed/inactive state restarts once; helper errors, unknown states, and
# non-exact post verification all fail closed.
authbot_reconcile_body=$(sed -n '/^reconcile_authbot_release()/,/^}/p' "$ROOT/deploy/lib.sh" \
  | sed \
      -e 's#local state_helper=/usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh#local state_helper=fixed-helper#' \
      -e 's#\[\[ ! -f /etc/systemd/system/$unit \]\]#\[\[ ! -f "$AUTHBOT_TEST_UNIT" \]\]#')
eval "$authbot_reconcile_body"
authbot_test_unit="$TEMP/claude-authbot.service"
authbot_state_queue="$TEMP/authbot-state-queue"
authbot_restart_log="$TEMP/authbot-restart.log"
authbot_release="$TEMP/release"
mkdir -p "$authbot_release"
printf '#!/usr/bin/env bash\n' >"$authbot_release/authbot"
chmod +x "$authbot_release/authbot"
touch "$authbot_test_unit"
log() { :; }
warn() { :; }
sha256_file() { printf '%s\n' "$authbot_expected"; }
sleep() { :; }
privileged_command() {
  if [[ $1 == systemctl ]]; then
    [[ $# -eq 3 && $2 == restart && $3 == claude-authbot.service ]] || return 1
    printf 'restart\n' >>"$authbot_restart_log"
    return "${AUTHBOT_TEST_RESTART_FAIL:-0}"
  fi
  [[ $1 == fixed-helper && $# -eq 2 && $2 == "$authbot_expected" ]] || return 1
  local response
  IFS= read -r response <"$authbot_state_queue" || return 1
  tail -n +2 "$authbot_state_queue" >"$authbot_state_queue.next"
  mv -- "$authbot_state_queue.next" "$authbot_state_queue"
  [[ $response != failure ]] || return 1
  printf '%s\n' "$response"
}
run_authbot_reconcile_case() {
  local responses=$1 expected_status=$2 expected_restarts=$3
  : >"$authbot_restart_log"
  printf '%b' "$responses" >"$authbot_state_queue"
  if (DRY_RUN=0 AUTHBOT_TEST_UNIT=$authbot_test_unit \
      reconcile_authbot_release "$authbot_release") >"$TEMP/authbot-reconcile-case.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  restarts=$(wc -l <"$authbot_restart_log" | tr -d '[:space:]')
  [[ $status == "$expected_status" && $restarts == "$expected_restarts" ]] \
    || wd_die "authbot reconciliation mismatch for $responses: status=$status restarts=$restarts"
}
run_authbot_reconcile_case 'exact\n' 0 0
run_authbot_reconcile_case 'different\nexact\n' 0 1
run_authbot_reconcile_case 'inactive\nexact\n' 0 1
run_authbot_reconcile_case 'failure\n' 1 0
run_authbot_reconcile_case 'unexpected\n' 1 0
run_authbot_reconcile_case 'different\ndifferent\n' 1 1
run_authbot_reconcile_case 'different\nfailure\n' 1 1
unset -f reconcile_authbot_release log warn sha256_file sleep privileged_command run_authbot_reconcile_case

# Both recovery callbacks must guard ENGINE_ORIGINAL reconciliation with exact restoration of engine
# current, including blue-green mode where provider services are deliberately untouched.
deploy_authbot_recovery=$(sed -n '/^recovery_restart_selected_services()/,/^}/p' \
  "$ROOT/deploy/deploy.sh" | sed '1s/recovery_restart_selected_services/deploy_authbot_recovery/')
rollback_authbot_recovery=$(sed -n '/^recovery_restart_selected_services()/,/^}/p' \
  "$ROOT/deploy/rollback.sh" | sed '1s/recovery_restart_selected_services/rollback_authbot_recovery/')
eval "$deploy_authbot_recovery"
eval "$rollback_authbot_recovery"
reconcile_authbot_after_engine_restore() { printf '%s|%s\n' "$1" "$2" >>"$TEMP/authbot-recovery"; }
best_effort_restart_units() { wd_die 'blue-green authbot recovery restarted provider services'; }
DEPLOY_ENGINE=1 ROLLBACK_ENGINE=1 RESTART_ENGINE=0
ENGINE_RELEASE_ROOT="$TEMP/engine-root" ENGINE_ORIGINAL="$TEMP/original-engine"
deploy_authbot_recovery || wd_die 'deploy authbot recovery rejected its restored original release'
rollback_authbot_recovery || wd_die 'rollback authbot recovery rejected its restored original release'
[[ $(grep -Fc "$ENGINE_RELEASE_ROOT|$ENGINE_ORIGINAL" "$TEMP/authbot-recovery") == 2 ]] \
  || wd_die 'deploy and rollback recovery did not guard the captured original authbot by engine current'
reconcile_authbot_after_engine_restore() { return 1; }
if deploy_authbot_recovery || rollback_authbot_recovery; then
  wd_die 'an activation recovery callback hid failed guarded authbot reconciliation'
fi
unset -f deploy_authbot_recovery rollback_authbot_recovery \
  reconcile_authbot_after_engine_restore best_effort_restart_units

# Explicit --engine-bluegreen rollback selects links without touching provider slots, but authbot is
# a singleton and must converge to ENGINE_TARGET in that same activation transaction.
rollback_activate_body=$(sed -n '/^activate_rollback_links()/,/^}/p' "$ROOT/deploy/rollback.sh" \
  | sed '1s/activate_rollback_links/test_activate_rollback_links/')
eval "$rollback_activate_body"
set_journaled_release_link() { :; }
log() { :; }
die() { exit 1; }
reconcile_authbot_release() { printf '%s\n' "$1" >>"$TEMP/rollback-authbot-target"; }
ROLLBACK_ENGINE=1 ROLLBACK_API=0 RESTART_ENGINE=0 DRY_RUN=1
ENGINE_RELEASE_ROOT="$TEMP/engine-root"
ENGINE_ORIGINAL="$TEMP/original-engine" ENGINE_TARGET="$TEMP/rollback-engine"
test_activate_rollback_links || wd_die 'blue-green rollback rejected authbot target reconciliation'
[[ $(cat "$TEMP/rollback-authbot-target") == "$ENGINE_TARGET" ]] \
  || wd_die 'blue-green rollback did not reconcile authbot to ENGINE_TARGET'
reconcile_authbot_release() { return 1; }
if (test_activate_rollback_links) >"$TEMP/rollback-authbot-failure.log" 2>&1; then
  wd_die 'blue-green rollback ignored failed authbot reconciliation'
fi
unset -f test_activate_rollback_links set_journaled_release_link log die reconcile_authbot_release

# The unified router is a third engine artifact. It is promoted through two fixed slots only after
# direct readiness and exact-binary checks; Caddy switches new requests atomically before the old
# process receives SIGTERM, so long SSE streams drain without a deployment 502 window.
grep -Fq -- '-p authbot -p claude-router' "$ROOT/deploy/watchdog.sh" \
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
! grep -Fq 'restart_router_if_changed' "$ROOT/deploy/deploy.sh" \
  || wd_die "release selection still restarts the public router singleton"
grep -Fq 'CLAUDE_ROUTER_PORT=%i' "$ROOT/systemd/claude-router@.service" \
  || wd_die "router slots do not bind their fixed instance port"
grep -Fxq 'TimeoutStopSec=660' "$ROOT/systemd/claude-router@.service" \
  || wd_die "router slots cannot drain provider-grade SSE sessions"
grep -Fxq 'TimeoutStopSec=660' "$ROOT/systemd/claude-router.service" \
  || wd_die "first singleton-to-slot handoff still truncates long SSE sessions"
router_payload_pins='CLAUDE_ROUTER_MAX_BODY_MIB=256 CLAUDE_ROUTER_BODY_MEMORY_BUDGET_MIB=4096 CLAUDE_ROUTER_BODY_SPOOL_BUDGET_MIB=16384 CLAUDE_ROUTER_BODY_MEMORY_THRESHOLD_MIB=8 CLAUDE_ROUTER_BODY_IDLE_SECS=120 CLAUDE_ROUTER_BODY_MAX_SECS=1800'
for router_unit in claude-router.service claude-router@.service; do
  if [[ $router_unit == claude-router@.service ]]; then
    grep -Fq "$router_payload_pins" "$ROOT/systemd/$router_unit"
  else
    grep -Fq 'CLAUDE_ROUTER_MAX_BODY_MIB=32' "$ROOT/systemd/$router_unit"
  fi
  if [[ $router_unit == claude-router@.service ]]; then
    grep -Fxq 'MemoryHigh=6G' "$ROOT/systemd/$router_unit"
    grep -Fxq 'MemoryMax=8G' "$ROOT/systemd/$router_unit"
  else
    grep -Fxq 'MemoryMax=512M' "$ROOT/systemd/$router_unit"
  fi
done
for resource_unit in claude-router@.service claude-api-anthropic@.service claude-api-openai@.service claude-api-gemini@.service; do
  grep -Fxq 'LimitNOFILE=262144' "$ROOT/systemd/$resource_unit" \
    || wd_die "$resource_unit lacks the large-payload fd envelope"
  grep -Fxq 'TasksMax=8192' "$ROOT/systemd/$resource_unit" \
    || wd_die "$resource_unit lacks the large-payload task envelope"
  grep -Fxq 'OOMPolicy=stop' "$ROOT/systemd/$resource_unit" \
    || wd_die "$resource_unit does not stop its cgroup after OOM"
done
grep -Fxq 'MemoryHigh=12G' "$ROOT/systemd/claude-api-gemini@.service"
grep -Fxq 'MemoryMax=16G' "$ROOT/systemd/claude-api-gemini@.service"
grep -Fq 'large-payload-headroom.sh' "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die 'router cutover lacks a fail-closed headroom gate'
grep -Fq 'large-payload-headroom.sh' "$ROOT/deploy/engine-bluegreen.sh" \
  || wd_die 'provider cutover lacks a fail-closed headroom gate'
grep -Fq 'CLAUDE_ROUTER_BODY_SPOOL_ROOT=/var/lib/apitoken/spool/router-%i' "$ROOT/systemd/claude-router@.service" \
  || wd_die 'router slots do not use separate instance-private spool roots'
grep -Fxq 'StateDirectory=apitoken/spool/router-%i' "$ROOT/systemd/claude-router@.service" \
  || wd_die 'router slot spool directory is not instance-scoped'
grep -Fxq 'StateDirectoryMode=0700' "$ROOT/systemd/claude-router@.service" \
  || wd_die 'router slot spool root is not private'
grep -Fq 'CLAUDE_ROUTER_BODY_SPOOL_ROOT=/run/claude-router-8798' "$ROOT/systemd/claude-router.service" \
  || wd_die 'legacy router does not have its own spool root'
provider_payload_pins='CLAUDE_API_TEXT_BODY_MAX_MIB=32 CLAUDE_API_BODY_MEMORY_BUDGET_MIB=2048 CLAUDE_API_BODY_SPOOL_BUDGET_MIB=2048 CLAUDE_API_BODY_MEMORY_THRESHOLD_MIB=32 CLAUDE_API_NONSTREAM_RESPONSE_MAX_MIB=64'
for provider_unit in claude-api.service claude-api@.service claude-api-anthropic@.service \
  claude-api-openai.service claude-api-openai@.service claude-api-gemini.service \
  claude-api-gemini@.service claude-api-kimi.service claude-api-kimi@.service; do
  if [[ $provider_unit == claude-api-gemini@.service ]]; then
    grep -Fq 'CLAUDE_API_TEXT_BODY_MAX_MIB=256 CLAUDE_API_BODY_MEMORY_BUDGET_MIB=8192 CLAUDE_API_BODY_SPOOL_BUDGET_MIB=16384 CLAUDE_API_BODY_MEMORY_THRESHOLD_MIB=8 CLAUDE_API_NONSTREAM_RESPONSE_MAX_MIB=256' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit does not argv-pin the 256 MiB Gemini envelope"
    grep -Fq 'CLAUDE_API_BODY_SPOOL_ROOT=/var/lib/apitoken/spool/gemini-%i' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit does not argv-pin a disk-backed Gemini spool root"
    grep -Fxq 'StateDirectoryMode=0700' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit spool root is not mode 0700"
  elif [[ $provider_unit == claude-api-openai@.service ]]; then
    grep -Fq 'CLAUDE_API_TEXT_BODY_MAX_MIB=256 CLAUDE_API_BODY_MEMORY_BUDGET_MIB=4096 CLAUDE_API_BODY_SPOOL_BUDGET_MIB=16384 CLAUDE_API_BODY_MEMORY_THRESHOLD_MIB=8 CLAUDE_API_NONSTREAM_RESPONSE_MAX_MIB=256' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit does not argv-pin the 256 MiB OpenAI envelope"
    grep -Fq 'CLAUDE_API_BODY_SPOOL_ROOT=/var/lib/apitoken/spool/openai-%i' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit does not argv-pin a disk-backed OpenAI spool root"
    grep -Fxq 'StateDirectoryMode=0700' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit spool root is not mode 0700"
  elif [[ $provider_unit == claude-api-anthropic@.service ]]; then
    grep -Fq 'CLAUDE_API_TEXT_BODY_MAX_MIB=32 CLAUDE_API_BODY_MEMORY_BUDGET_MIB=2048 CLAUDE_API_BODY_SPOOL_BUDGET_MIB=2048 CLAUDE_API_BODY_MEMORY_THRESHOLD_MIB=32 CLAUDE_API_NONSTREAM_RESPONSE_MAX_MIB=256' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit does not argv-pin the Anthropic 32 MiB request / 256 MiB response envelope"
    grep -Fq 'CLAUDE_API_BODY_SPOOL_ROOT=/run/claude-api' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit does not argv-pin a private provider spool root"
    grep -Fxq 'RuntimeDirectoryMode=0700' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit spool root is not mode 0700"
  else
    grep -Fq "$provider_payload_pins" "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit changed outside the Gemini/OpenAI canary"
    grep -Fq 'CLAUDE_API_BODY_SPOOL_ROOT=/run/claude-api' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit does not argv-pin a private provider spool root"
    grep -Fxq 'RuntimeDirectoryMode=0700' "$ROOT/systemd/$provider_unit" \
      || wd_die "$provider_unit spool root is not mode 0700"
  fi
  case "$provider_unit" in
    claude-api-anthropic@.service|claude-api-openai@.service)
      grep -Fxq 'MemoryHigh=6G' "$ROOT/systemd/$provider_unit"
      grep -Fxq 'MemoryMax=8G' "$ROOT/systemd/$provider_unit" ;;
    claude-api-gemini@.service)
      grep -Fxq 'MemoryHigh=12G' "$ROOT/systemd/$provider_unit"
      grep -Fxq 'MemoryMax=16G' "$ROOT/systemd/$provider_unit" ;;
    *) grep -Fxq 'MemoryMax=2G' "$ROOT/systemd/$provider_unit" ;;
  esac
done
! grep -Fq "trap 'abort_cutover \"\$?\" EXIT' EXIT" "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die 'router success path is still converted into a cutover failure by EXIT trap'
grep -Fq "trap 'rc=\$?; (( rc == 0 )) || abort_cutover \"\$rc\" EXIT' EXIT" \
  "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die 'explicit die/exit failures bypass router cutover recovery'
grep -Fq '/usr/local/lib/apitoken-watchdog/tests/large_payload_mock_load.py' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'payload canary load harness is never installed for the candidate gate'
grep -Fq '/usr/local/lib/apitoken-watchdog/tests/large_payload_candidate_gate.py' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'payload canary evaluator is never installed for the candidate gate'
! grep -Fq 'anthropic/test' "$ROOT/tests/large_payload_mock_load.py" \
  || wd_die 'payload canary still forwards a namespaced model to a narrower plane'
grep -Fq '{"messages":[{"role":"user","content":"' "$ROOT/tests/large_payload_mock_load.py" \
  || wd_die 'payload canary is not a router-local missing-model JSON body'
grep -Fq 'verdict_rc' "$ROOT/deploy/large-payload-candidate-gate.sh" \
  || wd_die 'candidate gate drops the verdict when the evaluator fails'
grep -Fq '$sha.reason' "$ROOT/deploy/large-payload-candidate-gate.sh" \
  || wd_die 'candidate gate does not persist a content-free payload-canary reason'
grep -Fq 'payload_canary_reason' "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die 'router cutover does not surface the payload-canary reason on failure'
grep -Fq 'wd_payload_canary_reason' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'engine deploy does not promote the payload-canary reason into wd_die'
bash "$ROOT/deploy/large-payload-candidate-gate.test.sh"
bash "$ROOT/deploy/large-payload-headroom.test.sh"
grep -Fq 'cd /var/lib/apitoken/watchdog/router-proof' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'router proof provisioning does not pin the opened directory handle'
grep -Fq '[[ $(pwd -P) == /var/lib/apitoken/watchdog/router-proof ]]' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'router proof provisioning does not verify the physical directory path'
grep -Fq 'chown deploy:deploy .' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'router proof directory is not securely provisioned for the deploy controller'
grep -Fxq 'ROUTER_SUCCESS_PROOF=/var/lib/apitoken/watchdog/router-proof/success' \
  "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die 'router controller proof does not use the fixed deploy-writable state path'
grep -Fq 'mv -fT -- "$PROOF_CANDIDATE" "$ROUTER_SUCCESS_PROOF"' \
  "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die 'router success proof is not atomically published in its secure parent'
grep -Fq '[[ -z $PROOF_CANDIDATE ]] || rm -f -- "$PROOF_CANDIDATE"' \
  "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die 'router proof staging file is not removed by cutover recovery'
proof_publish_line=$(grep -nF 'mv -fT -- "$PROOF_CANDIDATE" "$ROUTER_SUCCESS_PROOF"' \
  "$ROOT/deploy/router-bluegreen.sh" | cut -d: -f1)
proof_commit_line=$(grep -nF 'commit_cutover' "$ROOT/deploy/router-bluegreen.sh" | tail -n 1 | cut -d: -f1)
[[ -n $proof_publish_line && -n $proof_commit_line && $proof_publish_line -lt $proof_commit_line ]] \
  || wd_die 'router controller clears recovery traps before exact success proof is durable'
grep -Fxq 'ROUTER_SUCCESS_PROOF=$STATE_ROOT/router-proof/success' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog and router controller disagree on the exact proof path'
grep -Fq '[[ $proof_mode == 600 && $proof_owner == "$controller_identity"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog accepts router proof without ownership and mode validation'
grep -Fq 'router-bluegreen.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "router blue-green controller is never installed"
grep -Fq 'router-promote.sh' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "root-owned Caddy promotion helper is never installed"
grep -Fq '"$CONTROLLER_ROOT/router-bluegreen.sh"' "$ROOT/deploy/watchdog.sh" \
  || wd_die "watchdog can select a new router binary without cutting over its slot"
grep -Fq 'executable == "$release/claude-router"' "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die "router candidate admission lacks exact-binary verification"
grep -Fq 'ready_port "$port" && startup_port "$port"' "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die "router candidate admission lacks the provider data-path contract probe"
grep -Fq 'stable_startup || die' "$ROOT/deploy/router-bluegreen.sh" \
  || wd_die "router promotion never verifies the data path through the stable origin"
target_ready_line=$(grep -nF 'wait_target || die' "$ROOT/deploy/router-bluegreen.sh" | cut -d: -f1)
target_enable_line=$(grep -nF 'systemctl_command enable "$TARGET_UNIT"' \
  "$ROOT/deploy/router-bluegreen.sh" | head -n 1 | cut -d: -f1)
promotion_line=$(grep -nF 'privileged_command "$PROMOTE_HELPER" "$TARGET_PORT"' \
  "$ROOT/deploy/router-bluegreen.sh" | cut -d: -f1)
old_stop_line=$(grep -nF 'stop_unit "$ACTIVE_UNIT"' "$ROOT/deploy/router-bluegreen.sh" | cut -d: -f1)
[[ -n $target_ready_line && -n $target_enable_line && -n $promotion_line && -n $old_stop_line \
    && $target_ready_line -lt $target_enable_line && $target_enable_line -lt $promotion_line \
    && $promotion_line -lt $old_stop_line ]] \
  || wd_die "router cutover does not verify, boot-fence, promote, and drain in that order"
grep -Fq 'mv -f -- "$candidate" "$SNIPPET"' "$ROOT/deploy/router-promote.sh" \
  || wd_die "router backend selection is not an atomic same-directory rename"
grep -Fq 'restore || true' "$ROOT/deploy/router-promote.sh" \
  || wd_die "failed router promotion cannot restore the previous Caddy backend"
grep -Fq 'claude-router@.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "router slot template is never installed"
! grep -Fq 'systemctl try-restart claude-router.service' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die "systemd definition rollout still interrupts the legacy router anchor"
grep -Fq '/usr/bin/systemctl start claude-router@[0-9]*.service' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die "deploy user cannot start a router slot"
grep -Fq 'systemd/claude-router@.service' "$ROOT/deploy/watchdog-lib.sh" \
  || wd_die "router slot definition is not scoped as systemd infrastructure"

# Exercise the production convergence predicate with mocked systemd/PID/HTTP state. Static script
# ordering above protects the cutover protocol; this matrix protects the final GREEN verdict from
# accepting two slots, the wrong executable, the legacy backend, or a dead stable origin.
eval "$(sed -n '/^router_runtime_aligned()/,/^}/p' "$ROOT/deploy/watchdog.sh")"
ROUTER_TEST_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
ROUTER_TEST_ACTIVE_PORT=8800
ROUTER_TEST_BOTH_ACTIVE=0
ROUTER_TEST_WRONG_BINARY=0
ROUTER_TEST_STABLE_STATUS=200
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
router_active_backend_port() { printf '%s\n' "$ROUTER_TEST_ACTIVE_PORT"; }
systemctl() {
  local verb=$1 unit=${3:-}
  case "$verb" in
    is-active|is-enabled)
      unit=${3:-${2:-}}
      case "$unit" in
        claude-router@8800.service)
          [[ $ROUTER_TEST_ACTIVE_PORT == 8800 || $ROUTER_TEST_BOTH_ACTIVE == 1 ]]
          ;;
        claude-router@8801.service)
          [[ $ROUTER_TEST_ACTIVE_PORT == 8801 || $ROUTER_TEST_BOTH_ACTIVE == 1 ]]
          ;;
        *) return 1 ;;
      esac
      ;;
    show)
      unit=$2
      case "$unit" in
        claude-router@8800.service) printf '8800\n' ;;
        claude-router@8801.service) printf '8801\n' ;;
        *) printf '0\n' ;;
      esac
      ;;
    *) return 1 ;;
  esac
}
curl() {
  local arg url=
  for arg in "$@"; do [[ $arg == http://* ]] && url=$arg; done
  case "$url" in
    http://127.0.0.1:8802/ready) printf '%s' "$ROUTER_TEST_STABLE_STATUS" ;;
    http://127.0.0.1:8800/ready)
      [[ $ROUTER_TEST_ACTIVE_PORT == 8800 || $ROUTER_TEST_BOTH_ACTIVE == 1 ]] && printf '200' || printf '000'
      ;;
    http://127.0.0.1:8801/ready)
      [[ $ROUTER_TEST_ACTIVE_PORT == 8801 || $ROUTER_TEST_BOTH_ACTIVE == 1 ]] && printf '200' || printf '000'
      ;;
    *) printf '000' ;;
  esac
}
readlink() {
  local path=${@: -1}
  case "$path" in
    /proc/8800/exe|/proc/8801/exe)
      if [[ $ROUTER_TEST_WRONG_BINARY == 1 ]]; then
        printf '/srv/claude-api/releases/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/claude-router\n'
      else
        printf '/srv/claude-api/releases/%s/claude-router\n' "$ROUTER_TEST_SHA"
      fi
      ;;
    *) return 1 ;;
  esac
}
router_runtime_aligned "$ROUTER_TEST_SHA" \
  || wd_die 'one exact active router slot was rejected by the production verifier'
ROUTER_TEST_ACTIVE_PORT=8801
router_runtime_aligned "$ROUTER_TEST_SHA" \
  || wd_die 'reverse router slot was rejected by the production verifier'
ROUTER_TEST_BOTH_ACTIVE=1
if router_runtime_aligned "$ROUTER_TEST_SHA"; then
  wd_die 'production verifier accepted two active router slots'
fi
ROUTER_TEST_BOTH_ACTIVE=0
ROUTER_TEST_WRONG_BINARY=1
if router_runtime_aligned "$ROUTER_TEST_SHA"; then
  wd_die 'production verifier accepted a router from the wrong immutable release'
fi
ROUTER_TEST_WRONG_BINARY=0
ROUTER_TEST_ACTIVE_PORT=8798
if router_runtime_aligned "$ROUTER_TEST_SHA"; then
  wd_die 'production verifier accepted the legacy singleton as steady state'
fi
ROUTER_TEST_ACTIVE_PORT=8800
ROUTER_TEST_STABLE_STATUS=503
if router_runtime_aligned "$ROUTER_TEST_SHA"; then
  wd_die 'production verifier accepted a dead stable router origin'
fi
unset -f router_runtime_aligned router_active_backend_port systemctl curl readlink

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
grep -Fq 'if "$CONTROLLER_ROOT/router-bluegreen.sh"; then' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'engine rollout does not capture router verdict directly'
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
grep -Fq '$unit runs a different binary; restarting onto the selected release' \
  "$ROOT/deploy/lib.sh" \
  || wd_die "deployment can leave changed authbot code unadopted"
grep -Fq 'recover_interrupted_handoffs' "$ROOT/crates/authbot/src/main.rs" \
  || wd_die "an authbot code restart can strand sellers in a dead in-memory OAuth session"
for retained_engine_unit in claude-api-openai.service claude-api-openai@8793.service \
  claude-api-openai@8797.service claude-api-gemini.service \
  claude-api-gemini@8795.service claude-api-gemini@8799.service \
  claude-router.service claude-router@8800.service claude-router@8801.service \
  apitoken-crm-api.service apitoken-crm-web.service; do
  grep -Fq "$retained_engine_unit" "$ROOT/deploy/watchdog.sh" \
    || wd_die "release retention can unlink the executable backing $retained_engine_unit"
done
grep -Fq 'AUTHBOT_RUNTIME_STATE=$CONTROLLER_ROOT/authbot-runtime-state.sh' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'release retention has no fixed privileged authbot inspection path'
grep -Fq 'require_fixed_root_executable "$AUTHBOT_RUNTIME_STATE"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the watchdog does not preflight its privileged authbot inspection helper'
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
  'run_as_ci bash "$candidate/deploy/sccache-cargo.test.sh"'
  'run_as_ci bash "$candidate/deploy/agent-worktree.test.sh"'
  'run_as_ci bash "$candidate/deploy/delete-worktree-agent.test.sh"'
  'run_as_ci bash "$candidate/deploy/next-cache.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-scope.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-build-contexts.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-artifact-cache.test.sh"'
  'run_as_ci bash "$candidate/deploy/typescript-test-groups.test.sh"'
  'run_as_ci bash "$candidate/deploy/commerce-release-bundle.test.sh"'
  'run_as_ci bash "$candidate/deploy/agent-merge.suite.sh"'
  'test_control_api_acceptance "$candidate" "$engine_dsn"'
  'bash "$candidate/tests/control_api_engine_client_acceptance.sh"'
  'CONTROL_API_ACCEPTANCE_PORT="$((17480 + TEST_DB_SLOT))"'
  'status --porcelain --untracked-files=no'
  'run_candidate_lane test_typescript_lane "$candidate" "$dsn" "$sales_dsn" "$openkeys_dsn"'
  'run_candidate_lane test_rust_lane "$candidate" "$engine_dsn" "$engine_artifacts_required" \'
  'run_candidate_lane test_static_lane "$candidate" "$sha" "$static_required" &'
  'wait "$typescript_pid"'
  'wait "$rust_pid"'
  'wait "$codex_pid"'
  'wait "$static_pid"'
  'Control API assembled acceptance failed'
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
  "apps/sales-web",
  "apps/web",
  "apps/worker",
  "packages/db",
  "packages/engine-client",
  "packages/opencode-router-plugin",
  "packages/payments",
  "packages/sales-db",
]);
const explicitlyTestless = new Set([
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

grep -Fxq 'SuccessExitStatus=143' "$ROOT/systemd/apitoken-sales-web.service" \
  || wd_die 'a normal Next.js SIGTERM is reported as a failed Sales Web rollout'

grep -Fq 'CANDIDATE_RETENTION_SECONDS=$((24 * 60 * 60))' "$ROOT/deploy/watchdog.sh"
grep -Fq 'prune_expired_candidates' "$ROOT/deploy/watchdog.sh"

# Retention, retry, and post-admission recovery must stay wired into the watchdog itself.
[[ $(grep -Fc 'prune_expired_releases_best_effort' "$ROOT/deploy/watchdog.sh") -ge 3 ]] \
  || wd_die 'combined fail-local release retention is not wired into both watchdog paths'
grep -Fq '/usr/bin/rm -rf --one-file-system -- /opt/apitoken/crm-releases/[0-9a-f]*' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'crm release retention deletion is denied by sudo policy'
grep -Fq "require_permitted 'crm release removal'" "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudo policy self-check never verifies crm release removal'
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
# A TypeScript-less candidate carries no built devbot; the lane must defer green WITHOUT advancing
# devbot.sha, or every deploy/observability/engine-only master is quarantined after provisioning.
grep -Fq 'rollout deferred (devbot.sha not advanced)' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'the devbot lane does not defer TypeScript-less candidates'

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
# Exercise the real cleanup and signal traps without touching host policy. An interrupt after the
# mutation point must restore the prior policy and remain non-zero so the outer installer cannot
# publish a dependent watchdog entrypoint.
sudoers_cleanup_body=$(sed -n '/^cleanup()/,/^}/p' "$ROOT/deploy/install-sudoers.sh")
sudoers_signal_traps=$(sed -n '/^trap cleanup EXIT$/,/^trap '\''exit 143'\'' TERM$/p' \
  "$ROOT/deploy/install-sudoers.sh")
[[ -n $sudoers_cleanup_body && -n $sudoers_signal_traps ]] \
  || wd_die 'the sudoers installer cleanup or signal traps are missing'
for signal_case in INT:130 TERM:143; do
  signal=${signal_case%%:*}
  expected_rc=${signal_case#*:}
  signal_restore_log="$TEMP/sudoers-$signal.restore"
  signal_staging="$TEMP/sudoers-$signal.staging"
  : >"$signal_staging"
  if (
    staging=$signal_staging
    policy_mutated=1
    policy_committed=0
    restore() { printf 'restored\n' >"$signal_restore_log"; }
    eval "$sudoers_cleanup_body"
    eval "$sudoers_signal_traps"
    kill -s "$signal" "$BASHPID"
    exit 0
  ); then
    signal_rc=0
  else
    signal_rc=$?
  fi
  [[ $signal_rc == "$expected_rc" ]] \
    || wd_die "sudoers $signal interruption exited $signal_rc instead of $expected_rc"
  grep -Fxq restored "$signal_restore_log" \
    || wd_die "sudoers $signal interruption did not restore the prior policy"
  [[ ! -e $signal_staging ]] \
    || wd_die "sudoers $signal interruption did not remove its staging file"
done
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
authbot_sudo_pattern=$(printf '[0-9a-f]%.0s' {1..64})
grep -Fq "/usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh $authbot_sudo_pattern" \
  "$sudoers_policy" \
  || wd_die 'the sudo policy does not restrict authbot runtime verification to one exact SHA-256'
grep -Fq "require_permitted 'authbot exact-runtime verifier'" "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not live-verify authbot runtime inspection'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh release-sha' \
  "$sudoers_policy" \
  || wd_die 'the sudo policy does not permit only literal authbot release-sha inspection'
grep -Fq "require_permitted 'authbot live release inspector'" "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not live-verify authbot release inspection'
grep -Fq 'MISSING or unsafe required fixed authbot helper or parent' "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer can skip a missing authbot trust anchor'
# Exercise the trust predicate itself: only a non-symlink root:root 0755 helper under an equally
# constrained fixed parent may pass. Stub stat metadata so this remains runnable without root.
fixed_root_helper_body=$(sed -n '/^fixed_root_helper_is_trusted()/,/^}/p' \
  "$ROOT/deploy/install-sudoers.sh")
eval "$fixed_root_helper_body"
trust_root="$TEMP/authbot-helper-trust"
trust_parent="$trust_root/controller"
trust_helper="$trust_parent/authbot-runtime-state.sh"
mkdir -p "$trust_parent"
printf '#!/usr/bin/env bash\n' >"$trust_helper"
chmod 0755 "$trust_parent" "$trust_helper"
trust_parent_meta=0:0:755
trust_helper_meta=0:0:755
stat() {
  case $4 in
    "$trust_parent") printf '%s\n' "$trust_parent_meta" ;;
    "$trust_helper") printf '%s\n' "$trust_helper_meta" ;;
    *) return 1 ;;
  esac
}
fixed_root_helper_is_trusted "$trust_helper" \
  || wd_die 'fixed authbot helper trust rejected root:root 0755 metadata'
for trust_case in writable-helper wrong-helper-group writable-parent wrong-parent-group; do
  trust_parent_meta=0:0:755
  trust_helper_meta=0:0:755
  case $trust_case in
    writable-helper) trust_helper_meta=0:0:775 ;;
    wrong-helper-group) trust_helper_meta=0:1:755 ;;
    writable-parent) trust_parent_meta=0:0:775 ;;
    wrong-parent-group) trust_parent_meta=0:1:755 ;;
  esac
  fixed_root_helper_is_trusted "$trust_helper" \
    && wd_die "fixed authbot helper trust accepted $trust_case"
done
rm -f -- "$trust_helper"
fixed_root_helper_is_trusted "$trust_helper" \
  && wd_die 'fixed authbot helper trust accepted a missing helper'
printf '#!/usr/bin/env bash\n' >"$trust_root/target"
ln -s "$trust_root/target" "$trust_helper"
fixed_root_helper_is_trusted "$trust_helper" \
  && wd_die 'fixed authbot helper trust accepted a symlink helper'
unset -f fixed_root_helper_is_trusted stat
grep -Fq "require_denied 'malformed authbot runtime digest'" "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not reject malformed authbot verifier arguments'
grep -Fq "require_denied 'extra authbot runtime verifier argument'" \
  "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not reject extra authbot verifier arguments'
grep -Fq "require_denied 'extra authbot release inspector argument'" \
  "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not reject extra authbot release arguments'
grep -Fq "require_denied 'arbitrary authbot runtime mode'" "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'the sudoers installer does not reject arbitrary authbot runtime modes'
! grep -Eq '(^|[[:space:],])(readlink|cmp|sha256sum)([[:space:],]|$)' "$sudoers_policy" \
  || wd_die 'the sudo policy grants generic process or file comparison access'
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
sudoers_start_line=$(grep -nF 'if ! systemctl start apitoken-sudoers-install.service' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
helper_publish_line=$(grep -nF 'publish_authbot_runtime_helper' "$ROOT/deploy/install-watchdog.sh" \
  | cut -d: -f1 | head -n 2 | tail -n 1)
watchdog_publish_line=$(grep -nF 'mv -f -- "$watchdog_staged" "$watchdog_target"' \
  "$ROOT/deploy/install-watchdog.sh" | cut -d: -f1)
[[ -n $helper_publish_line && -n $sudoers_start_line && -n $watchdog_publish_line \
    && $helper_publish_line -lt $sudoers_start_line && $sudoers_start_line -lt $watchdog_publish_line ]] \
  || wd_die 'helper and sudo policy are not verified before atomic watchdog publication'
grep -Fq 'mv -f -- "$authbot_backup" "$authbot_helper"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'failed sudo policy installation does not restore the prior authbot helper'
# Bash expands every assignment word in one `local` command before applying any of them. Keep the
# staged path assignment separate so `set -u` cannot dereference `target` before it exists.
publish_fixed_helper_body=$(sed -n '/^publish_fixed_helper()/,/^}/p' \
  "$ROOT/deploy/install-watchdog.sh")
printf '%s\n' "$publish_fixed_helper_body" \
  | grep -Fq 'local source=$1 target=$2 staged' \
  || wd_die 'fixed-helper publisher does not declare its locals before deriving the staged path'
printf '%s\n' "$publish_fixed_helper_body" \
  | grep -Fq 'staged=${target}.tmp.$$' \
  || wd_die 'fixed-helper publisher does not derive its staged path after local initialization'
if printf '%s\n' "$publish_fixed_helper_body" \
    | grep -Eq 'local .*staged=\$\{target\}'; then
  wd_die 'fixed-helper publisher dereferences target in the same local command that declares it'
fi
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
grep -Fq "Referer: https://\$host/__admin-auth/login?return_to=%2F" "$ROOT/deploy/watchdog.sh" \
  || wd_die 'managed-admin final verification does not exercise the no-Origin same-origin form fallback'
grep -Fq "Referer: https://attacker.invalid/" "$ROOT/deploy/watchdog.sh" \
  || wd_die 'managed-admin final verification does not prove a foreign Referer stays forbidden'
grep -Fq "'^referrer-policy: same-origin[[:space:]]*\$'" "$ROOT/deploy/watchdog.sh" \
  || wd_die 'managed-admin final verification does not require a browser-usable Referrer policy'
grep -Fq "''|000|404|421" "$ROOT/deploy/watchdog.sh"
# Встроенной панели в движке больше нет: UI — standalone Next.js app на :3700 (свой deploy
# lane), а engine отдаёт только data routes. Ни HTML/JS-панели, ни её version marker в
# candidate быть не должно; watchdog проверяет data surface через /overview.
[[ ! -e "$ROOT/crates/server/src/admin-panel.html" ]]
[[ ! -e "$ROOT/crates/server/src/admin-panel.js" ]]
! grep -Fq 'data-admin-panel-version' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog must not verify the retired embedded panel version marker'
[[ ! -e "$ROOT/crates/server/src/panel.html" ]]

# Exercise the exact raw-key provisioner with host ownership changes stubbed into the test directory.
# Generation, migration, rejection, and reruns must never print the credential.
authbot_key_fixture="$TEMP/apitoken/proxy-admin.key"
authbot_env_fixture="$TEMP/authbot/authbot.env"
authbot_server_env_fixture="$TEMP/server.env"
authbot_key_log="$TEMP/authbot-key.log"
authbot_chown_log="$TEMP/authbot-key.chown.log"
authbot_install_log="$TEMP/authbot-key.install.log"
canonical_proxy_key=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
divergent_proxy_key=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
AUTHBOT_PROXY_ADMIN_KEY_CREATED=0
for provision_function in proxy_admin_key_file_is_valid proxy_admin_key_files_equal \
  provision_authbot_proxy_admin_key; do
  provision_definition=$(sed -n "/^$provision_function()/,/^}/p" \
    "$ROOT/deploy/install-watchdog.sh")
  [[ -n $provision_definition ]] || wd_die "missing installer helper: $provision_function"
  eval "$provision_definition"
done
install() {
  local arg previous= target=
  printf '%s\n' "$*" >>"$authbot_install_log"
  for arg in "$@"; do previous=$target; target=$arg; done
  if [[ " $* " == *' -d '* ]]; then mkdir -p "$target"; else cp "$previous" "$target"; fi
}
chown() { printf '%s\n' "$*" >>"$authbot_chown_log"; }
openssl() {
  [[ $* == 'rand -hex 32' ]] || return 1
  printf '%064d\n' 0
}
command() {
  if [[ ${1:-} == -v && ${2:-} == openssl ]]; then printf 'openssl\n'; else builtin command "$@"; fi
}
AUTHBOT_LN_COLLISION=0
ln() {
  if [[ $AUTHBOT_LN_COLLISION == 1 && ${1:-} == -- ]]; then
    printf '%s\n' "$divergent_proxy_key" >"${3}"
    return 1
  fi
  command ln "$@"
}
: >"$authbot_chown_log"
: >"$authbot_install_log"
printf 'OTHER_SERVER_SETTING=preserved\n' >"$authbot_server_env_fixture"
PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
  SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
  >"$authbot_key_log" 2>&1
[[ $(wc -c <"$authbot_key_fixture" | tr -d '[:space:]') == 65 \
    && $(LC_ALL=C awk 'NR == 1 && length($0) == 64 && $0 ~ /^[0-9a-f]+$/ { valid=1 } END { print valid+0 ":" NR }' \
      "$authbot_key_fixture") == 1:1 ]] \
  || wd_die 'proxy-admin bootstrap did not create the exact raw lowercase-hex format'
[[ ! -e $authbot_env_fixture ]] \
  || wd_die 'raw proxy-admin bootstrap created a mixed-settings authbot.env'
[[ $AUTHBOT_PROXY_ADMIN_KEY_CREATED == 1 ]] \
  || wd_die 'proxy-admin bootstrap does not request service adoption'
grep -Fxq -- "-d -o root -g root -m 0755 ${authbot_key_fixture%/*}" "$authbot_install_log" \
  || wd_die 'proxy-admin bootstrap did not enforce a root-owned, non-deploy-writable key parent'
grep -Fq "root:root $authbot_key_fixture" "$authbot_chown_log" \
  || wd_die 'proxy-admin bootstrap did not enforce raw-key ownership'
[[ $(LC_ALL=C ls -ld "$authbot_key_fixture" | cut -c1-10) == -rw------- ]] \
  || wd_die 'proxy-admin bootstrap did not enforce raw-key mode'
[[ ! -s $authbot_key_log ]] || wd_die 'proxy-admin bootstrap emitted output'
AUTHBOT_PROXY_ADMIN_KEY_CREATED=0
authbot_key_before=$(shasum -a 256 "$authbot_key_fixture" | cut -d' ' -f1)
PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
  SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
  >>"$authbot_key_log" 2>&1
authbot_key_after=$(shasum -a 256 "$authbot_key_fixture" | cut -d' ' -f1)
[[ $authbot_key_before == "$authbot_key_after" ]] \
  || wd_die 'proxy-admin provisioning rotated or rewrote an existing valid raw key'
[[ $AUTHBOT_PROXY_ADMIN_KEY_CREATED == 0 ]] \
  || wd_die 'valid raw proxy-admin key falsely requests another authbot restart'
[[ ! -s $authbot_key_log ]] || wd_die 'proxy-admin stable rerun emitted output'

assert_proxy_admin_provision_fails() {
  local description=$1 key_contents=$2 env_contents=${3-__absent__}
  rm -f -- "$authbot_key_fixture" "$authbot_env_fixture" "$authbot_key_log"
  printf '%b' "$key_contents" >"$authbot_key_fixture"
  printf 'OTHER_SERVER_SETTING=preserved\n' >"$authbot_server_env_fixture"
  if [[ $env_contents != __absent__ ]]; then printf '%b' "$env_contents" >"$authbot_env_fixture"; fi
  if PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
      SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
      >"$authbot_key_log" 2>&1; then
    wd_die "proxy-admin provisioning accepted $description"
  fi
  ! grep -Eq '[0-9a-fA-F]{64}' "$authbot_key_log" \
    || wd_die "proxy-admin provisioning leaked a key while rejecting $description"
}
assert_proxy_admin_provision_fails 'a short raw key' 'short\n'
assert_proxy_admin_provision_fails 'an uppercase raw key' \
  'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n'
assert_proxy_admin_provision_fails 'a CRLF raw key' \
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n'
assert_proxy_admin_provision_fails 'a multi-line raw key' \
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nextra\n'
assert_proxy_admin_provision_fails 'an oversized raw key' \
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
rm -f -- "$authbot_key_fixture" "$authbot_env_fixture"
printf '%s\n' "$canonical_proxy_key" >"$TEMP/proxy-admin-target.key"
ln -s "$TEMP/proxy-admin-target.key" "$authbot_key_fixture"
if PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
    SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
    >"$authbot_key_log" 2>&1; then
  wd_die 'proxy-admin provisioning followed a raw-key symlink'
fi
! grep -Fq "$canonical_proxy_key" "$authbot_key_log" \
  || wd_die 'proxy-admin symlink rejection leaked the key'
rm -f -- "$authbot_key_fixture" "$authbot_env_fixture"
printf '%s\n' "$canonical_proxy_key" >"$authbot_key_fixture"
printf 'OTHER_SETTING=preserved\n' >"$TEMP/authbot-target.env"
ln -s "$TEMP/authbot-target.env" "$authbot_env_fixture"
if PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
    SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
    >"$authbot_key_log" 2>&1; then
  wd_die 'proxy-admin migration followed an authbot.env symlink'
fi

# Shared server settings may not shadow or redirect the dedicated credential authority.
for stale_server_setting in \
  "AUTH_BOT_PROXY_ADMIN_KEY=$divergent_proxy_key" \
  'AUTH_BOT_PROXY_ADMIN_KEY_FILE=/tmp/deploy-controlled-proxy-admin.key'; do
  rm -f -- "$authbot_env_fixture" "$authbot_key_log"
  printf '%s\n' "$canonical_proxy_key" >"$authbot_key_fixture"
  printf '%s\n' "$stale_server_setting" >"$authbot_server_env_fixture"
  authbot_key_before=$(shasum -a 256 "$authbot_key_fixture" | cut -d' ' -f1)
  if PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
      SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
      >"$authbot_key_log" 2>&1; then
    wd_die "proxy-admin provisioning accepted stale server.env setting: ${stale_server_setting%%=*}"
  fi
  [[ $(shasum -a 256 "$authbot_key_fixture" | cut -d' ' -f1) == "$authbot_key_before" ]] \
    || wd_die 'server.env rejection modified the canonical proxy-admin key'
  ! grep -Eq '[0-9a-fA-F]{64}' "$authbot_key_log" \
    || wd_die 'server.env rejection leaked a proxy-admin key'
done
printf 'OTHER_SERVER_SETTING=preserved\n' >"$authbot_server_env_fixture"

# An exact legacy assignment is migrated into the raw file while unrelated authbot settings remain.
rm -f -- "$authbot_key_fixture" "$authbot_env_fixture" "$authbot_key_log"
printf 'AUTH_BOT_TOKEN=telegram-setting\nAUTH_BOT_PROXY_ADMIN_KEY=%s\nAUTH_BOT_ADMIN=42\n' \
  "$canonical_proxy_key" >"$authbot_env_fixture"
AUTHBOT_PROXY_ADMIN_KEY_CREATED=0
PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
  SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
  >"$authbot_key_log" 2>&1
[[ $(tr -d '\n' <"$authbot_key_fixture") == "$canonical_proxy_key" ]] \
  || wd_die 'legacy proxy-admin assignment was not migrated into the raw key file'
[[ $(grep -Ec '^AUTH_BOT_PROXY_ADMIN_KEY=' "$authbot_env_fixture") == 0 \
    && $(grep -Fc 'AUTH_BOT_TOKEN=telegram-setting' "$authbot_env_fixture") == 1 \
    && $(grep -Fc 'AUTH_BOT_ADMIN=42' "$authbot_env_fixture") == 1 ]] \
  || wd_die 'legacy migration did not preserve other authbot.env settings exactly once'
[[ $AUTHBOT_PROXY_ADMIN_KEY_CREATED == 1 ]] \
  || wd_die 'legacy proxy-admin migration does not request service adoption'
[[ ! -s $authbot_key_log ]] || wd_die 'legacy proxy-admin migration emitted output'

# Equal canonical and legacy values converge by deleting only the obsolete assignment.
printf '%s' "$canonical_proxy_key" >"$authbot_key_fixture"
printf 'AUTH_BOT_TOKEN=keep\nAUTH_BOT_PROXY_ADMIN_KEY=%s\n' "$canonical_proxy_key" \
  >"$authbot_env_fixture"
AUTHBOT_PROXY_ADMIN_KEY_CREATED=0
PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
  SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
  >"$authbot_key_log" 2>&1
[[ $(cat "$authbot_env_fixture") == 'AUTH_BOT_TOKEN=keep' \
    && $(cat "$authbot_key_fixture") == "$canonical_proxy_key" \
    && $AUTHBOT_PROXY_ADMIN_KEY_CREATED == 0 ]] \
  || wd_die 'equal canonical and legacy proxy-admin keys did not converge safely'
[[ ! -s $authbot_key_log ]] || wd_die 'equal-key proxy-admin convergence emitted output'

# Divergent canonical and legacy values fail without modifying either authority.
printf '%s\n' "$canonical_proxy_key" >"$authbot_key_fixture"
printf 'AUTH_BOT_TOKEN=keep\nAUTH_BOT_PROXY_ADMIN_KEY=%s\n' "$divergent_proxy_key" \
  >"$authbot_env_fixture"
authbot_key_before=$(shasum -a 256 "$authbot_key_fixture" | cut -d' ' -f1)
authbot_env_before=$(shasum -a 256 "$authbot_env_fixture" | cut -d' ' -f1)
if PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
    SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
    >"$authbot_key_log" 2>&1; then
  wd_die 'proxy-admin provisioning accepted divergent canonical and legacy keys'
fi
[[ $(shasum -a 256 "$authbot_key_fixture" | cut -d' ' -f1) == "$authbot_key_before" \
    && $(shasum -a 256 "$authbot_env_fixture" | cut -d' ' -f1) == "$authbot_env_before" ]] \
  || wd_die 'divergent proxy-admin migration modified canonical or legacy state'
! grep -Eq '[0-9a-f]{64}' "$authbot_key_log" \
  || wd_die 'divergent proxy-admin migration leaked a key'

# A destination appearing between candidate creation and hard-link publication wins unchanged.
rm -f -- "$authbot_key_fixture" "$authbot_env_fixture" "$authbot_key_log"
AUTHBOT_PROXY_ADMIN_KEY_CREATED=0
AUTHBOT_LN_COLLISION=1
if PROXY_ADMIN_KEY_FILE=$authbot_key_fixture AUTHBOT_ENV=$authbot_env_fixture \
    SERVER_ENV=$authbot_server_env_fixture provision_authbot_proxy_admin_key \
    >"$authbot_key_log" 2>&1; then
  wd_die 'proxy-admin provisioning overwrote an atomic destination collision'
fi
AUTHBOT_LN_COLLISION=0
[[ $(tr -d '\n' <"$authbot_key_fixture") == "$divergent_proxy_key" \
    && $AUTHBOT_PROXY_ADMIN_KEY_CREATED == 0 ]] \
  || wd_die 'proxy-admin collision did not preserve the independently published destination'
! grep -Eq '[0-9a-f]{64}' "$authbot_key_log" \
  || wd_die 'proxy-admin collision rejection leaked a key'
unset -f assert_proxy_admin_provision_fails install chown openssl command ln \
  proxy_admin_key_file_is_valid proxy_admin_key_files_equal provision_authbot_proxy_admin_key
unset AUTHBOT_PROXY_ADMIN_KEY_CREATED AUTHBOT_LN_COLLISION

render_live="$TEMP/live.Caddyfile"
rendered_once="$TEMP/rendered-once.Caddyfile"
rendered_twice="$TEMP/rendered-twice.Caddyfile"
render_proxy_key="$TEMP/render-proxy-admin.key"
{
  printf '(panel_admins) {\n\tbasic_auth {\n\t\tadmin $2y$12$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q\n\t}\n}\n'
  printf '(crm_admins) {\n\tbasic_auth {\n\t\tcrm $2a$14$GkwhyxjgFuLvnJRxUDO5POFWymIfHL9NKsdtLIHo3lvrXIhvPaO2q\n\t}\n}\n'
  printf 'admin.apitoken.sale {\n'
  printf '\theader_up x-api-key "test-control-secret"\n'
  printf '\theader_up x-admin-key "test-commerce-secret"\n'
  printf '\theader_up x-sales-admin-key "test-sales-secret"\n}\n'
} >"$render_live"
printf '%s' "$canonical_proxy_key" >"$render_proxy_key"
awk -v proxy_admin_key_file="$render_proxy_key" -v render_output="$rendered_once" \
  -f "$ROOT/deploy/render-caddy.awk" "$render_live" "$ROOT/deploy/Caddyfile"
# The same raw-key parser accepts the provisioner's single LF and requires live Caddy to agree.
printf '%s\n' "$canonical_proxy_key" >"$render_proxy_key"
awk -v proxy_admin_key_file="$render_proxy_key" -v render_output="$rendered_twice" \
  -f "$ROOT/deploy/render-caddy.awk" "$rendered_once" "$ROOT/deploy/Caddyfile"
for rendered in "$rendered_once" "$rendered_twice"; do
  ! grep -Fq 'basic_auth' "$rendered"
  ! grep -Fq '$2y$' "$rendered"
  grep -Fq 'forward_auth @managed_admin_request 127.0.0.1:8791' "$rendered"
  [[ $(grep -Fc 'header_up x-api-key "test-control-secret"' "$rendered") == 5 ]]
  [[ $(grep -Fc 'header_up X-OpenKeys-Control-Key "test-control-secret"' "$rendered") == 1 ]]
  [[ $(grep -Fc 'header_up x-admin-key "test-commerce-secret"' "$rendered") == 2 ]]
  [[ $(grep -Fc 'header_up X-Admin-Key "test-commerce-secret"' "$rendered") == 2 ]]
  [[ $(grep -Fc 'header_up x-sales-admin-key "test-sales-secret"' "$rendered") == 1 ]]
  [[ $(grep -Fc "header_up X-Proxy-Admin-Key \"$canonical_proxy_key\"" "$rendered") == 1 ]]
  if grep -Eq '<[A-Z_]*PLACEHOLDER>' "$rendered"; then
    wd_die "rendered Caddy fixture retained a secret placeholder"
  fi
done

assert_proxy_admin_render_fails() {
  local description=$1 key_contents=$2 live_input=${3:-$render_live}
  local failure_output="$TEMP/render-failure.out" failure_render="$TEMP/render-failure.Caddyfile"
  if [[ $key_contents == __missing__ ]]; then
    rm -f -- "$render_proxy_key"
  else
    printf '%b' "$key_contents" >"$render_proxy_key"
  fi
  rm -f -- "$failure_render"
  if awk -v proxy_admin_key_file="$render_proxy_key" -v render_output="$failure_render" \
      -f "$ROOT/deploy/render-caddy.awk" "$live_input" "$ROOT/deploy/Caddyfile" \
      >"$failure_output" 2>&1; then
    wd_die "Caddy renderer accepted $description proxy-admin state"
  fi
  [[ ! -e $failure_render ]] \
    || wd_die "failed Caddy render published a partial candidate for $description state"
  ! grep -Eq '[0-9a-fA-F]{64}' "$failure_output" \
    || wd_die "failed Caddy render leaked a proxy-admin key for $description state"
}
assert_proxy_admin_render_fails 'empty' ''
assert_proxy_admin_render_fails 'short' 'short\n'
assert_proxy_admin_render_fails 'uppercase' \
  'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n'
assert_proxy_admin_render_fails 'CRLF' \
  'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n'
assert_proxy_admin_render_fails 'multi-line' \
  'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\ndddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\n'
assert_proxy_admin_render_fails 'missing raw-key file' __missing__

# Header names follow HTTP case-insensitive semantics; alternate case must still match and deduplicate.
render_alternate_case="$TEMP/render-alternate-case.Caddyfile"
sed "s/header_up X-Proxy-Admin-Key \"$canonical_proxy_key\"/header_up x-PrOxY-aDmIn-KeY \"$canonical_proxy_key\"/" \
  "$rendered_once" >"$render_alternate_case"
printf '%s\n' "$canonical_proxy_key" >"$render_proxy_key"
awk -v proxy_admin_key_file="$render_proxy_key" -v render_output="$rendered_twice" \
  -f "$ROOT/deploy/render-caddy.awk" "$render_alternate_case" "$ROOT/deploy/Caddyfile"
[[ $(grep -Fci "header_up X-Proxy-Admin-Key \"$canonical_proxy_key\"" "$rendered_twice") == 1 ]] \
  || wd_die 'Caddy renderer did not case-insensitively converge the live proxy-admin header'
render_duplicate_case="$TEMP/render-duplicate-case.Caddyfile"
cp "$render_alternate_case" "$render_duplicate_case"
printf 'header_up X-PROXY-ADMIN-KEY "%s"\n' "$canonical_proxy_key" \
  >>"$render_duplicate_case"
assert_proxy_admin_render_fails 'duplicate alternate-case live header' \
  "$canonical_proxy_key\n" "$render_duplicate_case"
assert_proxy_admin_render_fails 'divergent alternate-case live header' \
  "$divergent_proxy_key\n" "$render_alternate_case"
unset -f assert_proxy_admin_render_fails

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

# The retired Gemini 3.7 root bridge and paid branch-protection dependency must stay absent.
! grep -Fq 'production-head-is' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'GitHub helper still requires paid branch protection for production delivery'
! grep -Fq 'DIRECT_ADMISSION_' "$ROOT/deploy/watchdog-infrastructure.sh" \
  || wd_die 'privileged infrastructure runner still contains Gemini direct-admission state'
! grep -Fq 'claude-api-gemini-3-7-admission.service' \
  "$ROOT/systemd/apitoken-deploy-watchdog.service" \
  || wd_die 'watchdog sandbox still provisions the retired Gemini admission unit'
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

postdrop_line=$(grep -nF 'sudo -n "$PRICING_RETIREMENT_POSTDROP" --stage' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
final_verification_line=$(grep -nF \
  'if ! ( run_final_verification_plan "$final_verification_plan" "$ENGINE_SHA" ); then' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $final_verification_line && -n $postdrop_line && -n $processed_line \
    && $final_verification_line -lt $postdrop_line && $postdrop_line -lt $processed_line ]] \
  || wd_die 'pricing-retirement post-drop proof is not between final verification and processed/green'
grep -Fq 'PRICING_RETIREMENT_POSTDROP=/usr/local/lib/apitoken-watchdog/pricing-retirement-postdrop.sh' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the fixed pricing-retirement post-drop helper'
grep -Fq 'pricing-retirement-postdrop.sh --stage commerce [0-9a-f]*' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot invoke commerce post-drop proof with one exact candidate SHA'
grep -Fq 'pricing-retirement-postdrop.sh --stage engine [0-9a-f]*' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot invoke engine post-drop proof with one exact candidate SHA'
grep -Fq '/usr/local/lib/apitoken-watchdog/pricing-retirement-postdrop.sh' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'controller installer does not publish the fixed post-drop helper'
grep -Fq '/usr/local/lib/apitoken-watchdog/pricing-retired-schema-manifest.sh' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'controller installer does not publish the post-drop survival manifest'

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
  'github_phase_failure "$phase" "$(wd_github_failure_description "$phase" "$rc")"'
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
grep -Fq 'PRICING_RETIREMENT_ENGINE_ADMISSION_MARKER=.pricing-retirement-admission-v1' \
  "$ROOT/deploy/lib.sh" \
  || wd_die 'engine releases have no immutable pricing-retirement capability marker'
grep -Fq 'pricing_retirement_value=contraction-0049' "$ROOT/deploy/deploy.sh" \
  || wd_die 'engine release promotion does not mark contraction 0049'
grep -Fq 'pricing_retirement_value=pre-contraction' "$ROOT/deploy/deploy.sh" \
  || wd_die 'ordinary engine releases are not explicitly fenced from contraction 0049'
grep -Fq 'if [[ $pricing_retirement_capability == contraction-0049 ]]' \
  "$ROOT/deploy/engine-migrate.sh" \
  || wd_die 'engine migrator does not condition destructive admission on immutable release metadata'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh [0-9a-f]*' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot invoke the fixed engine schema migration helper'
grep -Fq '/usr/bin/test -x /usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'deploy user cannot probe the fixed engine schema migration helper'

pricing_marker_fixture="$TEMP/pricing-retirement-engine-marker"
for pricing_marker_value in pre-contraction contraction-0049; do
  printf '%s\n' "$pricing_marker_value" >"$pricing_marker_fixture"
  [[ $(bash -c 'source "$1"; validate_pricing_retirement_engine_admission_marker "$2"' \
      _ "$ROOT/deploy/lib.sh" "$pricing_marker_fixture") == "$pricing_marker_value" ]] \
    || wd_die "engine release marker rejected $pricing_marker_value"
done
printf '%s\n' bypass >"$pricing_marker_fixture"
if bash -c 'source "$1"; validate_pricing_retirement_engine_admission_marker "$2"' \
    _ "$ROOT/deploy/lib.sh" "$pricing_marker_fixture" >/dev/null 2>&1; then
  wd_die 'engine release marker accepted an unknown contraction capability'
fi

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
grep -Fq 'outputFileTracingRoot: workspaceRoot' "$ROOT/apps/content-studio/next.config.ts" \
  || wd_die 'Content Studio standalone trace is no longer rooted at the stable workspace path'
grep -Fq 'turbopack: { root: workspaceRoot }' "$ROOT/apps/content-studio/next.config.ts" \
  || wd_die 'Content Studio Turbopack root can drift with the checkout parent directory'
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
[[ $(grep -Fc 'production-(database|engine|backend|sales|openkeys|admin|devbot)' \
  "$ROOT/deploy/watchdog-github.sh") == 2 ]] \
  || wd_die 'GitHub deployment reporting does not allow the admin and devbot environments'

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
grep -Fq 'wd_publish_github_failure_log "$CANDIDATE_SHA"' <<<"$shadow_body" \
  || wd_die 'trusted candidate failures are published without a redacted check-run log'
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
grep -Fq 'wd_github_failure_description' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'production failures do not publish a bounded GitHub fail-closed reason'
grep -Fq 'wd_publish_github_failure_log' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'production failures do not persist and publish a redacted cycle excerpt'
grep -Fq 'wd_start_cycle_transcript' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'production watchdog does not capture a cycle transcript for failure reports'
grep -Fq 'check-run)' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'GitHub bridge cannot upload a redacted failure check run'
grep -Fq 'escaped its directory' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'GitHub check-run helper does not refuse a path-escaped failure report'
grep -Fq '/var/lib/apitoken/watchdog/failures' "$ROOT/deploy/watchdog-github.sh" \
  || wd_die 'GitHub check-run helper does not pin the failure-report directory'
grep -Fq 'am_failure_log' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge client does not fetch the redacted host failure log'
grep -Fq 'deploy/watchdog-log' "$ROOT/deploy/agent-merge.sh" \
  || wd_die 'the merge client does not look up the deploy/watchdog-log check run'
grep -Eq "GitHub failure-log bridge'.*watchdog-github check-run" \
  "$ROOT/deploy/install-sudoers.sh" \
  || wd_die 'sudo policy installer does not verify failure-log check-run access'
if grep -Fq 'diagnostic="phase=$failed_phase; line=$line; exit=$rc; candidate quarantined"' \
  "$ROOT/deploy/watchdog.sh"; then
  wd_die 'production failures still publish a bash wait line number as the GitHub reason'
fi
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

# GPT Image 2 edit is one exact-SHA, bounded production gate. It consumes only the immutable
# successful generation artifact, requires authoritative image-input usage, and closes a terminal
# success-or-withdrawal fence before overall GREEN advances the processed baseline.
wd_path_is_gpt_image_2_live_gate_trigger deploy/gpt-image-2-live-gate.sh \
  || wd_die 'GPT Image 2 live gate file does not trigger its one-shot delivery range'
if wd_path_is_gpt_image_2_live_gate_trigger deploy/watchdog.sh; then
  wd_die 'ordinary controller updates trigger a paid GPT Image 2 edit'
fi
grep -Fq '"$ROOT/deploy/gpt-image-2-live-gate.sh"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 live gate is not installed as a fixed controller'
grep -Fxq 'EXPECTED_IMPLEMENTATION_SHA=1c48e3769f0fe775e650f60ea3c5839458e5dfe2' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate is not pinned to its watchdog-green implementation SHA'
grep -Fxq 'GENERATION_IMPLEMENTATION_SHA=df58715abb4f1ac52b6c46b1ea6f830c6e11178f' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate lost the immutable generation provenance SHA'
grep -Fxq 'EDIT_BUDGET_NANOUSD=64022330000' "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate lost its numeric authorization ceiling'
grep -Fq 'reference=$generation_root/generation.png' "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate does not bind the owned generation reference'
grep -Fq 'cmp -s -- "$reference" "$generation_internal_output"' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate does not verify generation reference recovery provenance'
grep -Fq '! cmp -s -- "$output" "$reference"' "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate can accept the unmodified reference bytes as output'
grep -Fq '(.provider.size == "auto" or .provider.size == "\(.width)x\(.height)")' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate does not enforce the bounded auto-size contract'
grep -Fq '(.width * .height) >= 655360 and (.width * .height) <= 8294400' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate does not enforce the output pixel envelope'
grep -Fq '.usage.input_tokens_details.image_tokens | type == "number"' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate does not require authoritative image-input usage'
grep -Fq '.usage.output_tokens_details.image_tokens | type == "number"' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit gate does not require authoritative image-output usage'
grep -Fq '.state == "evidence_home_mismatch"' "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit diagnostic omits a terminal evidence state'
grep -Fq '.state == "rejected" or .state == "outcome_unknown"' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit diagnostic omits terminal transport outcomes'
grep -Fq 'wd_die "prior GPT Image 2 edit attempt was withdrawn; publication remains blocked"' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 edit withdrawal can incorrectly advance overall GREEN'
grep -Fq '(.returned.output_sha256 | type == "string"' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 mismatch journal does not require a bounded output digest'
grep -Fq '[[ ${#recovery_entries[@]} -eq 1 && ${recovery_entries[0]} == journal.json ]]' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 diagnostic accepts unexpected recovery artifacts'
grep -Fq '(([.. | numbers]) as $values |' "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 diagnostic usage verifier lost valid jq value binding syntax'
recovery_fence_line=$(grep -nF 'if [[ -e $recovery || -L $recovery ]]; then' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" | cut -d: -f1)
runtime_env_line=$(grep -nF 'load_openai_runtime_environment() {' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" | cut -d: -f1)
[[ -n $recovery_fence_line && -n $runtime_env_line && $recovery_fence_line -lt $runtime_env_line ]] \
  || wd_die 'GPT Image 2 terminal recovery can reach runtime credentials or network dispatch'
grep -Fq 'for forbidden in "$internal_output" "$internal_checkpoint"' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 diagnostic does not verify rejected image absence'
grep -Fq '"$binary" openai-image-canary' "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 gate cannot dispatch the exact private canary'
grep -Fq -- '--reference "$reference"' "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 gate cannot dispatch the owned reference edit'
grep -Fq 'CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 gate does not force the external Codex fallback off'
! grep -Eiq 'laozhang|aihubproxy|apixo|whataicc' \
  "$ROOT/deploy/gpt-image-2-live-gate.sh" \
  || wd_die 'GPT Image 2 live gate contains a third-party image relay'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-live-gate.sh 1c48e3769f0fe775e650f60ea3c5839458e5dfe2' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 live gate lacks an exact-SHA sudo bridge'
grep -A2 -F "require_permitted 'GPT Image 2 exact-SHA live gate'" \
  "$ROOT/deploy/install-sudoers.sh" \
  | grep -Fq '1c48e3769f0fe775e650f60ea3c5839458e5dfe2' \
  || wd_die 'GPT Image 2 exact-SHA sudo bridge self-check is not aligned with policy'
grep -Fxq 'GPT_IMAGE_2_IMPLEMENTATION_SHA=1c48e3769f0fe775e650f60ea3c5839458e5dfe2' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 gate to its immutable implementation release'
live_gate_line=$(grep -nF \
  'sudo -n "$GPT_IMAGE_2_LIVE_GATE" "$GPT_IMAGE_2_IMPLEMENTATION_SHA"' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $live_gate_line && -n $processed_line && $live_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 exact implementation withdrawal fence is not verified before processed/green'
! grep -Fq 'sudo -n "$GPT_IMAGE_2_LIVE_GATE" "$ENGINE_SHA"' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 gate follows the mutable current engine baseline instead of its pinned implementation'

# The first public generation+edit attempt owns its producer-SHA fence. After a RED delivery this
# controller becomes a permanently non-network inspector: exact success or the observed exact pre-dispatch
# withdrawal may pass, while anything else reports only bounded journal state and dispatch flags.
wd_path_is_gpt_image_2_public_smoke_gate_trigger deploy/gpt-image-2-public-smoke-gate.sh \
  || wd_die 'GPT Image 2 public evidence inspector file does not trigger corrective verification'
if wd_path_is_gpt_image_2_public_smoke_gate_trigger deploy/watchdog.sh; then
  wd_die 'ordinary controller updates trigger GPT Image 2 public evidence inspection'
fi
grep -Fq '"$ROOT/deploy/gpt-image-2-public-smoke-gate.sh"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 public evidence inspector is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=d2e345f2de75e0ee6c72797fdf315f12ab4bbeb6' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public evidence inspector is not pinned to the withdrawn producer SHA'
grep -Fq 'OUTPUT=$EVIDENCE_PARENT/$PRODUCER_SHA' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public evidence inspector does not use the producer SHA fence'
grep -Fq '[[ $# -eq 2 && $2 == --inspect ]]' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public evidence inspector accepts a dispatch-capable invocation'
! grep -Eq 'load_openai_runtime_environment|/proc/\$pid/environ|setpriv|timeout .*1500|openai-image-public-smoke' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public corrective inspector can still reach runtime credentials or dispatch'
grep -Fq 'mapfile -t entries < <(find "$OUTPUT" -mindepth 1 -maxdepth 1' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public verifier accepts unexpected retained artifacts'
grep -Fq '.settlement.release_billing_mode == "meter_only"' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public verifier does not require meter-only settlement'
grep -Fq '.settlement.real_nano == (.settlement.input_nano + .settlement.output_nano)' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public verifier lost exact official cost reconciliation'
grep -Fq '.settlement.charge_nano == 0' "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public verifier permits customer debit'
grep -Fq '.usage.input_tokens_details.image_tokens > 0' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public edit does not require authoritative image input usage'
grep -Fq '! cmp -s -- "$generation" "$edit"' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public verifier accepts a byte-identical edit'
grep -Fq 'gpt-image-public:\($journal.state):g=\($journal.generation_dispatched):e=\($journal.edit_dispatched)' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public inspector does not emit bounded retained journal classification'
grep -Fq '[[ $summary == gpt-image-public:preflight:g=false:e=false ]]' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public inspector does not accept the observed exact pre-dispatch withdrawal'
grep -Fq ".generation_request_id == null and .edit_request_id == null" \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public withdrawal can pass with a request identity'
! grep -Eiq 'laozhang|aihubproxy|apixo|whataicc' \
  "$ROOT/deploy/gpt-image-2-public-smoke-gate.sh" \
  || wd_die 'GPT Image 2 public inspector contains a third-party image relay'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-smoke-gate.sh d2e345f2de75e0ee6c72797fdf315f12ab4bbeb6 --inspect' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 public inspector lacks an exact-producer sudo bridge'
grep -A2 -F "require_permitted 'GPT Image 2 exact-producer public evidence inspector'" \
  "$ROOT/deploy/install-sudoers.sh" \
  | grep -Fq 'd2e345f2de75e0ee6c72797fdf315f12ab4bbeb6 --inspect' \
  || wd_die 'GPT Image 2 public inspector sudo self-check is not aligned with policy'
grep -Fxq 'GPT_IMAGE_2_PUBLIC_PRODUCER_SHA=d2e345f2de75e0ee6c72797fdf315f12ab4bbeb6' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 public producer release'
public_gate_line=$(grep -nF '"$GPT_IMAGE_2_PUBLIC_PRODUCER_SHA" --inspect' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $public_gate_line && -n $processed_line && $public_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 public evidence is not inspected before processed/green'
grep -Fq 'CURRENT_PHASE_BEFORE_FAILURE=$public_image_summary' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 retained journal classification is absent from the RED status'

# The free production attempt is fenced. Its corrective controller can only classify the retained
# one-file no-dispatch journal; it cannot load an environment, credential, binary, or network path.
wd_path_is_gpt_image_2_public_preflight_gate_trigger deploy/gpt-image-2-public-preflight-gate.sh \
  || wd_die 'GPT Image 2 public preflight inspector file does not trigger corrective verification'
if wd_path_is_gpt_image_2_public_preflight_gate_trigger deploy/watchdog.sh; then
  wd_die 'ordinary controller updates trigger GPT Image 2 public preflight inspection'
fi
grep -Fq '"$ROOT/deploy/gpt-image-2-public-preflight-gate.sh"' "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight inspector is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=d42fc0e3290c0042a16797626326c250e0f6721c' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector is not pinned to the GREEN producer'
grep -Fq '[[ $# -eq 2 && $2 == --inspect ]]' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector accepts a dispatch-capable invocation'
! grep -Eq '/proc/|systemctl|setpriv|timeout|env -i|CLAUDE_API_DATABASE_URL|openai-image-public-smoke|https?://' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector can reach environment, binary, or network'
grep -Fq '.generation_dispatched == false and .edit_dispatched == false' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector accepts an image dispatch'
grep -Fq '.generation_request_id == null and .edit_request_id == null' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector accepts a request identity'
grep -Fq '[[ ${#entries[@]} -eq 1 && ${entries[0]} == journal.json ]]' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector accepts image/evidence artifacts'
grep -Fq 'gpt-image-preflight:\($journal.state):g=false:e=false' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector omits exact retained stage'
! grep -Eiq 'APIYI|laozhang|aihubproxy|apixo|whataicc' \
  "$ROOT/deploy/gpt-image-2-public-preflight-gate.sh" \
  || wd_die 'GPT Image 2 public preflight inspector contains a third-party image relay'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-preflight-gate.sh d42fc0e3290c0042a16797626326c250e0f6721c --inspect' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 public preflight inspector lacks an exact-producer sudo bridge'
grep -A2 -F "require_permitted 'GPT Image 2 exact-producer public preflight inspector'" \
  "$ROOT/deploy/install-sudoers.sh" \
  | grep -Fq 'd42fc0e3290c0042a16797626326c250e0f6721c --inspect' \
  || wd_die 'GPT Image 2 public preflight inspector sudo self-check is not aligned with policy'
grep -Fxq 'GPT_IMAGE_2_PUBLIC_PREFLIGHT_PRODUCER_SHA=d42fc0e3290c0042a16797626326c250e0f6721c' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 public preflight producer'
preflight_gate_line=$(grep -nF '"$GPT_IMAGE_2_PUBLIC_PREFLIGHT_PRODUCER_SHA" --inspect' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $preflight_gate_line && -n $processed_line && $preflight_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 public preflight is not inspected before processed/green'
grep -Fq 'CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_summary' "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight stage is absent from the watchdog status'
grep -Fq 'github_status success deploy/gpt-image-2-public-preflight "$public_image_preflight_summary"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight stage has no sanitized GREEN status'

# The v2 root is permanently fenced after its one delivery. No later path, including the retained
# historical controller itself, may trigger another execution against that root.
for path in deploy/gpt-image-2-public-preflight-v2-gate.sh deploy/watchdog.sh deploy/watchdog-lib.sh; do
  if wd_path_is_gpt_image_2_public_preflight_v2_gate_trigger "$path"; then
    wd_die "retired GPT Image 2 public preflight v2 trigger accepts $path"
  fi
done
grep -Fq '"$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight v2 is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=6629ecd7b3725bcd7306ef7a1dc8675ef9160a43' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 is not pinned to the corrected selector producer'
grep -Fxq 'EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-preflight-v2' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 reuses a historical evidence root'
grep -Fq '[[ $# -eq 1 ]]' "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 accepts an unbounded invocation'
grep -Fq '"$binary" openai-image-public-smoke --output "$OUTPUT" --preflight-only' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 does not run the exact no-image CLI mode'
! grep -Fq -- '--execute' "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 can dispatch an image'
grep -Fq '.generation_dispatched == false and .edit_dispatched == false' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 accepts image dispatch flags'
grep -Fq '.generation_request_id == null and .edit_request_id == null' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 accepts image request identities'
grep -Fq '[[ ${#entries[@]} -eq 1 && ${entries[0]} == journal.json ]]' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 accepts image/evidence artifacts'
! grep -Eiq 'APIYI|laozhang|aihubproxy|apixo|whataicc|https?://' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v2-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v2 contains a third-party or direct network origin'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-preflight-v2-gate.sh 6629ecd7b3725bcd7306ef7a1dc8675ef9160a43' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 public preflight v2 lacks an exact-producer sudo bridge'
grep -A2 -F "require_permitted 'GPT Image 2 exact-producer public preflight v2 gate'" \
  "$ROOT/deploy/install-sudoers.sh" \
  | grep -Fq '6629ecd7b3725bcd7306ef7a1dc8675ef9160a43' \
  || wd_die 'GPT Image 2 public preflight v2 sudo self-check is not aligned with policy'
grep -Fxq 'GPT_IMAGE_2_PUBLIC_PREFLIGHT_V2_PRODUCER_SHA=6629ecd7b3725bcd7306ef7a1dc8675ef9160a43' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 public preflight v2 producer'
preflight_v2_gate_line=$(grep -nF '"$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V2_PRODUCER_SHA")' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $preflight_v2_gate_line && -n $processed_line && $preflight_v2_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 public preflight v2 does not run before processed/green'
grep -Fq 'CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_v2_summary' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight v2 result is absent from a later RED status'
grep -Fq 'github_status success deploy/gpt-image-2-public-preflight-v2' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight v2 has no sanitized GREEN status'

# V3 is the fresh-root, no-dispatch consumer of the GREEN handle-based selector producer.
wd_path_is_gpt_image_2_public_preflight_v3_gate_trigger \
  deploy/gpt-image-2-public-preflight-v3-gate.sh \
  || wd_die 'GPT Image 2 public preflight v3 file does not trigger the one-shot gate'
for path in deploy/watchdog.sh deploy/watchdog-lib.sh deploy/gpt-image-2-public-preflight-v2-gate.sh; do
  if wd_path_is_gpt_image_2_public_preflight_v3_gate_trigger "$path"; then
    wd_die "unrelated path triggers GPT Image 2 public preflight v3: $path"
  fi
done
grep -Fq '"$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight v3 is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 is not pinned to the handle-based selector producer'
grep -Fxq 'EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-preflight-v3' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 reuses a historical evidence root'
grep -Fq '[[ $# -eq 1 ]]' "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 accepts an unbounded invocation'
grep -Fq '"$binary" openai-image-public-smoke --output "$OUTPUT" --preflight-only' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 does not run the exact no-image CLI mode'
grep -Fq 'if verify_preflight_success; then' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 rejects terminal evidence after process teardown timeout'
! grep -Fq -- '--execute' "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 can dispatch an image'
grep -Fq '.generation_dispatched == false and .edit_dispatched == false' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 accepts image dispatch flags'
grep -Fq '.generation_request_id == null and .edit_request_id == null' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 accepts image request identities'
grep -Fq '[[ ${#entries[@]} -eq 1 && ${entries[0]} == journal.json ]]' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 accepts image/evidence artifacts'
! grep -Eiq 'APIYI|laozhang|aihubproxy|apixo|whataicc|https?://' \
  "$ROOT/deploy/gpt-image-2-public-preflight-v3-gate.sh" \
  || wd_die 'GPT Image 2 public preflight v3 contains a third-party or direct network origin'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-preflight-v3-gate.sh 63972f2ddfd5906d7c30a87406053eb3782f4223' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 public preflight v3 lacks an exact-producer sudo bridge'
grep -A2 -F "require_permitted 'GPT Image 2 exact-producer public preflight v3 gate'" \
  "$ROOT/deploy/install-sudoers.sh" \
  | grep -Fq '63972f2ddfd5906d7c30a87406053eb3782f4223' \
  || wd_die 'GPT Image 2 public preflight v3 sudo self-check is not aligned with policy'
grep -Fxq 'GPT_IMAGE_2_PUBLIC_PREFLIGHT_V3_PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 public preflight v3 producer'
preflight_v3_gate_line=$(grep -nF '"$GPT_IMAGE_2_PUBLIC_PREFLIGHT_V3_PRODUCER_SHA")' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $preflight_v3_gate_line && -n $processed_line && $preflight_v3_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 public preflight v3 does not run before processed/green'
grep -Fq 'CURRENT_PHASE_BEFORE_FAILURE=$public_image_preflight_v3_summary' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight v3 result is absent from a later RED status'
grep -Fq 'github_status success deploy/gpt-image-2-public-preflight-v3' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public preflight v3 has no sanitized GREEN status'

# The first paid root is fenced at generation_received and can only be inspected without network.
if wd_path_is_gpt_image_2_public_paid_smoke_gate_trigger \
    deploy/gpt-image-2-public-paid-smoke-gate.sh; then
  wd_die 'retired GPT Image 2 public paid smoke can dispatch again'
fi
if wd_path_is_gpt_image_2_public_paid_smoke_v2_gate_trigger \
    deploy/gpt-image-2-public-paid-smoke-v2-gate.sh; then
  wd_die 'retired GPT Image 2 public paid smoke v2 can dispatch again'
fi
grep -Fq '"$ROOT/deploy/gpt-image-2-public-paid-smoke-v2-gate.sh"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 public paid smoke v2 is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=853fdc6c8d5be486c371b23df6772eeaf7a48029' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v2-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v2 is not pinned to the GREEN producer'
grep -Fxq 'EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-paid-smoke-v2' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v2-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v2 does not use a fresh evidence root'
grep -Fq '"$binary" openai-image-public-smoke --output "$OUTPUT" --execute' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v2-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v2 does not run the exact one-shot CLI'
[[ $(grep -Fc -- '--execute' "$ROOT/deploy/gpt-image-2-public-paid-smoke-v2-gate.sh") -eq 1 ]] \
  || wd_die 'GPT Image 2 public paid smoke v2 has multiple execute paths'
! grep -Eiq 'APIYI|laozhang|aihubproxy|apixo|whataicc' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v2-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v2 contains a reseller path'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-paid-smoke-v2-gate.sh 853fdc6c8d5be486c371b23df6772eeaf7a48029' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 public paid smoke v2 lacks an exact-producer sudo bridge'
grep -Fxq 'GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V2_PRODUCER_SHA=853fdc6c8d5be486c371b23df6772eeaf7a48029' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 public paid smoke v2 producer'
grep -Fq 'retired GPT Image 2 paid smoke v2 root cannot be dispatched again' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public paid smoke v2 is not fail-closed retired'
wd_path_is_gpt_image_2_public_paid_smoke_v3_gate_trigger \
  deploy/gpt-image-2-public-paid-smoke-v3-gate.sh \
  || wd_die 'fresh GPT Image 2 public paid smoke v3 file does not trigger its gate'
for path in deploy/watchdog.sh deploy/watchdog-lib.sh deploy/gpt-image-2-public-paid-smoke-v2-gate.sh; do
  if wd_path_is_gpt_image_2_public_paid_smoke_v3_gate_trigger "$path"; then
    wd_die "unrelated path triggers GPT Image 2 public paid smoke v3: $path"
  fi
done
grep -Fq '"$ROOT/deploy/gpt-image-2-public-paid-smoke-v3-gate.sh"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 public paid smoke v3 is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=8b68d73a2a6ba6ffae2f24692b283059f15b7c63' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v3-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v3 is not pinned to the GREEN producer'
grep -Fxq 'EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-paid-smoke-v3' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v3-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v3 does not use a fresh evidence root'
grep -Fq '"$binary" openai-image-public-smoke --output "$OUTPUT" --execute' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v3-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v3 does not run the exact one-shot CLI'
[[ $(grep -Fc -- '--execute' "$ROOT/deploy/gpt-image-2-public-paid-smoke-v3-gate.sh") -eq 1 ]] \
  || wd_die 'GPT Image 2 public paid smoke v3 has multiple execute paths'
! grep -Eiq 'APIYI|laozhang|aihubproxy|apixo|whataicc' \
  "$ROOT/deploy/gpt-image-2-public-paid-smoke-v3-gate.sh" \
  || wd_die 'GPT Image 2 public paid smoke v3 contains a reseller path'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-paid-smoke-v3-gate.sh 8b68d73a2a6ba6ffae2f24692b283059f15b7c63' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 public paid smoke v3 lacks an exact-producer sudo bridge'
grep -Fxq 'GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V3_PRODUCER_SHA=8b68d73a2a6ba6ffae2f24692b283059f15b7c63' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 public paid smoke v3 producer'
paid_v3_gate_line=$(grep -nF '"$GPT_IMAGE_2_PUBLIC_PAID_SMOKE_V3_PRODUCER_SHA")' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $paid_v3_gate_line && -n $processed_line && $paid_v3_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 public paid smoke v3 does not run before processed/green'
grep -Fq 'CURRENT_PHASE_BEFORE_FAILURE=gpt-image-paid-v3:success:g=true:e=true' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 public paid smoke v3 terminal state is absent from later RED status'
wd_path_is_gpt_image_2_public_paid_inspect_gate_trigger \
  deploy/gpt-image-2-public-paid-inspect-gate.sh \
  || wd_die 'GPT Image 2 public paid inspector file does not trigger the corrective gate'
for path in deploy/watchdog.sh deploy/watchdog-lib.sh deploy/gpt-image-2-public-paid-smoke-gate.sh; do
  if wd_path_is_gpt_image_2_public_paid_inspect_gate_trigger "$path"; then
    wd_die "unrelated path triggers GPT Image 2 public paid inspector: $path"
  fi
done
grep -Fq '"$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 public paid inspector is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector is not pinned to the fenced producer'
grep -Fxq 'EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-public-paid-smoke' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector reads the wrong fence'
grep -Fq '[[ $# -eq 2 && $2 == --inspect ]]' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector accepts a dispatch-capable invocation'
! grep -Eq 'openai-image-public-smoke|setpriv|systemctl|/proc/|timeout ' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector can access runtime or network credentials'
grep -Fq '.state == "generation_received"' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector accepts another journal stage'
grep -Fq '.generation_dispatched == true and .edit_dispatched == false' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector does not require generation-only dispatch'
grep -Fq '[[ ${#entries[@]} -eq 2 && ${entries[0]} == generation.png && ${entries[1]} == journal.json ]]' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector accepts unexpected artifacts'
! grep -Eiq 'APIYI|laozhang|aihubproxy|apixo|whataicc|https?://' \
  "$ROOT/deploy/gpt-image-2-public-paid-inspect-gate.sh" \
  || wd_die 'GPT Image 2 public paid inspector contains a network origin'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-public-paid-inspect-gate.sh 63972f2ddfd5906d7c30a87406053eb3782f4223 --inspect' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 public paid inspector lacks an exact read-only sudo bridge'
grep -A2 -F "require_permitted 'GPT Image 2 exact-producer public paid inspector'" \
  "$ROOT/deploy/install-sudoers.sh" \
  | grep -Fq '63972f2ddfd5906d7c30a87406053eb3782f4223 --inspect' \
  || wd_die 'GPT Image 2 public paid inspector sudo self-check is not aligned with policy'
grep -Fxq 'GPT_IMAGE_2_PUBLIC_PAID_INSPECT_PRODUCER_SHA=63972f2ddfd5906d7c30a87406053eb3782f4223' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 public paid inspector producer'
paid_inspect_gate_line=$(grep -nF '"$GPT_IMAGE_2_PUBLIC_PAID_INSPECT_PRODUCER_SHA" --inspect)' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $paid_inspect_gate_line && -n $processed_line && $paid_inspect_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 public paid inspector does not run before processed/green'
grep -Fq 'CURRENT_PHASE_BEFORE_FAILURE=gpt-image-paid:generation_received:g=true:e=false' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 generation-only withdrawal is absent from later RED status'
grep -Fq 'github_status success deploy/gpt-image-2-public-paid-inspect "$public_image_generation_status"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 generation inspection has no sanitized GREEN status'

# The two one-shot settlement diagnostics are terminal historical evidence. Their status records
# remain on the immutable commits; no current runtime/controller/sudo path may read retired pricing
# snapshots after the retention cleanup begins.
for retired_diagnostic in \
  gpt-image-2-settlement-diagnostic-gate.sh \
  gpt-image-2-settlement-v2-diagnostic-gate.sh; do
  [[ ! -e "$ROOT/deploy/$retired_diagnostic" ]] \
    || wd_die "retired GPT Image 2 diagnostic remains in the source tree: $retired_diagnostic"
  grep -Fq "/usr/local/lib/apitoken-watchdog/controller/$retired_diagnostic" \
    "$ROOT/deploy/install-watchdog.sh" \
    || wd_die "retired GPT Image 2 diagnostic is not removed from the installed controller root"
  ! grep -Fq "$retired_diagnostic" \
    "$ROOT/deploy/watchdog.sh" "$ROOT/deploy/watchdog-lib.sh" \
    "$ROOT/deploy/install-sudoers.sh" "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
    || wd_die "retired GPT Image 2 diagnostic still has a watchdog or sudo execution path"
done

# The surface probe is the only paid multi-control gate: medium/high generations and a
# two-reference edit, each under its exact official authorization ceiling, with sanitized verdicts.
wd_path_is_gpt_image_2_surface_probe_gate_trigger \
  deploy/gpt-image-2-surface-probe-gate.sh \
  || wd_die 'GPT Image 2 surface probe file does not trigger its gate'
for path in deploy/watchdog.sh deploy/watchdog-lib.sh deploy/gpt-image-2-public-paid-smoke-v3-gate.sh; do
  if wd_path_is_gpt_image_2_surface_probe_gate_trigger "$path"; then
    wd_die "unrelated path triggers GPT Image 2 surface probe: $path"
  fi
done
grep -Fq '"$ROOT/deploy/gpt-image-2-surface-probe-gate.sh"' \
  "$ROOT/deploy/install-watchdog.sh" \
  || wd_die 'GPT Image 2 surface probe is not installed as a fixed controller'
grep -Fxq 'PRODUCER_SHA=d69868fb700aaeb9b6723d8780bb29be4aab9c0d' \
  "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
  || wd_die 'GPT Image 2 surface probe is not pinned to the probe-capable producer'
grep -Fxq 'EVIDENCE_PARENT=$STATE_ROOT/gpt-image-2-surface-probe' \
  "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
  || wd_die 'GPT Image 2 surface probe does not use its own evidence root'
[[ $(grep -Fc -- '--execute' "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh") -eq 1 ]] \
  || wd_die 'GPT Image 2 surface probe does not funnel execution through one flag path'
grep -Fxq 'MEDIUM_BUDGET_NANOUSD=180460000' "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
  || wd_die 'GPT Image 2 surface probe medium ceiling drifted from the official formula'
grep -Fxq 'HIGH_BUDGET_NANOUSD=714130000' "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
  || wd_die 'GPT Image 2 surface probe high ceiling drifted from the official formula'
grep -Fxq 'MULTI_REF_BUDGET_NANOUSD=128022330000' "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
  || wd_die 'GPT Image 2 surface probe multi-reference envelope drifted'
! grep -Eiq 'APIYI|laozhang|aihubproxy|apixo|whataicc' \
  "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
  || wd_die 'GPT Image 2 surface probe contains a reseller path'
grep -Fq '/usr/local/lib/apitoken-watchdog/controller/gpt-image-2-surface-probe-gate.sh d69868fb700aaeb9b6723d8780bb29be4aab9c0d' \
  "$ROOT/deploy/sudoers.d/95-apitoken-deploy" \
  || wd_die 'GPT Image 2 surface probe lacks an exact-producer sudo bridge'
grep -A2 -F "require_permitted 'GPT Image 2 exact-producer surface probe gate'" \
  "$ROOT/deploy/install-sudoers.sh" \
  | grep -Fq 'd69868fb700aaeb9b6723d8780bb29be4aab9c0d' \
  || wd_die 'GPT Image 2 surface probe sudo self-check is not aligned with policy'
grep -Fxq 'GPT_IMAGE_2_SURFACE_PROBE_PRODUCER_SHA=d69868fb700aaeb9b6723d8780bb29be4aab9c0d' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'watchdog does not pin the GPT Image 2 surface probe producer'
surface_probe_gate_line=$(grep -nF '"$GPT_IMAGE_2_SURFACE_PROBE_PRODUCER_SHA")' \
  "$ROOT/deploy/watchdog.sh" | cut -d: -f1)
[[ -n $surface_probe_gate_line && -n $processed_line && \
   $surface_probe_gate_line -lt $processed_line ]] \
  || wd_die 'GPT Image 2 surface probe does not run before processed/green'
grep -Fq 'github_status success "deploy/gpt-image-2-probe-$surface_probe_name"' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 surface probe has no per-probe GREEN statuses'
grep -Fq 'for surface_probe_name in medium high multi-ref; do' \
  "$ROOT/deploy/watchdog.sh" \
  || wd_die 'GPT Image 2 surface probe names must stay valid status contexts (hyphenated)'
grep -Fq '"multi-ref":$multi_ref' "$ROOT/deploy/gpt-image-2-surface-probe-gate.sh" \
  || wd_die 'GPT Image 2 surface probe summary key must match the hyphenated status name'

grep -Fq 'tokio-postgres-rustls' "$ROOT/crates/registry/Cargo.toml" \
  || wd_die 'engine PostgreSQL transport must use rustls alongside the BoringSSL forward transport'
if grep -Eq '^[[:space:]]*(postgres-native-tls|native-tls)[[:space:]]*=' \
  "$ROOT/crates/registry/Cargo.toml"; then
  wd_die 'OpenSSL-compatible PostgreSQL TLS cannot be linked with the BoringSSL forward transport'
fi

bash "$ROOT/deploy/pricing-retired-schema.test.sh"
bash "$ROOT/deploy/pricing-retirement-preflight.test.sh"
bash "$ROOT/deploy/pricing-retirement-admission.test.sh"
bash "$ROOT/deploy/pricing-retirement-postdrop.test.sh"

printf 'watchdog retention, migration, and engine topology tests passed\n'
