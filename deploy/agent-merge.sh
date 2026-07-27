#!/usr/bin/env bash
# Serialized, gated merge of the current branch into master.
#
# Several agents and contributors work on this repository at the same time, and a push to master
# is a production deployment trigger. This script is the only supported way to land work:
#
#   * it never checks out master, so it cannot disturb a co-resident working tree;
#   * it holds a machine-wide lock across the merge AND the deployment that follows, so two
#     candidates can never be tested or deployed on top of each other;
#   * it runs the local full gate and trusted production-host validation on the exact tree it
#     pushes, overlapping them when possible and repeating both only when the SHA changes;
#   * it refuses to stack work on a red or still-deploying master.
#
# Usage:  deploy/agent-merge.sh [--allow-primary-tree] [--dry-run] [--fix-red]
#
# Agents must run it from their own worktree with no arguments. Human contributors working in a
# plain clone pass --allow-primary-tree.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

AGENT_MERGE_REPO=${AGENT_MERGE_REPO:-3xcalibur-tech/Claude_API}
AGENT_MERGE_REMOTE=${AGENT_MERGE_REMOTE:-origin}
AGENT_MERGE_TARGET=${AGENT_MERGE_TARGET:-master}
AGENT_MERGE_LOCK=${AGENT_MERGE_LOCK:-$HOME/.claude-api/master-merge.lock}
AGENT_MERGE_LOCK_WAIT_S=${AGENT_MERGE_LOCK_WAIT_S:-3600}
AGENT_MERGE_STALE_S=${AGENT_MERGE_STALE_S:-5400}
AGENT_MERGE_DEPLOY_WAIT_S=${AGENT_MERGE_DEPLOY_WAIT_S:-2400}
AGENT_MERGE_POLL_S=${AGENT_MERGE_POLL_S:-5}
AGENT_MERGE_PUSH_ATTEMPTS=${AGENT_MERGE_PUSH_ATTEMPTS:-3}
# The context that means the production pipeline reached a verdict. Anything else is a partial view.
AGENT_MERGE_REQUIRED_CONTEXT=${AGENT_MERGE_REQUIRED_CONTEXT:-deploy/watchdog}
AGENT_MERGE_VALIDATION_ENVIRONMENT=${AGENT_MERGE_VALIDATION_ENVIRONMENT:-candidate-validation}

ALLOW_PRIMARY_TREE=0
DRY_RUN=0
FIX_RED=0
for argument in "$@"; do
  case "$argument" in
    --allow-primary-tree) ALLOW_PRIMARY_TREE=1 ;;
    --dry-run) DRY_RUN=1 ;;
    --fix-red) FIX_RED=1 ;;
    -h|--help) sed -n '2,16p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) printf 'agent-merge: unknown argument: %s\n' "$argument" >&2; exit 1 ;;
  esac
done

am_log() { printf '[agent-merge] %s\n' "$*"; }
am_die() { printf '[agent-merge] ERROR: %s\n' "$*" >&2; exit 1; }

# BSD (macOS) and GNU (Linux contributors, production host) disagree about stat, and they disagree
# dangerously: GNU reads -f as --file-system, then EXITS 0 while printing `File: "/tmp"` instead of a
# number. Feeding that to $(( )) makes bash treat `File` as a variable name, which under set -u kills
# the script. So try GNU first and validate that whatever came back is actually digits.
am_mtime() {
  local mtime
  mtime=$(stat -c %Y -- "$1" 2>/dev/null || printf '')
  [[ $mtime =~ ^[0-9]+$ ]] || mtime=$(stat -f %m -- "$1" 2>/dev/null || printf '')
  [[ $mtime =~ ^[0-9]+$ ]] || mtime=0
  printf '%s' "$mtime"
}
am_now() { date +%s; }

am_gate_typescript() (
  cd "$ROOT"
  pnpm install --frozen-lockfile
  pnpm build
  pnpm typecheck
  pnpm test
)

am_gate_rust() (
  cd "$ROOT"
  cargo test --locked --workspace
)

am_gate_deployment() (
  cd "$ROOT"
  bash "$ROOT/deploy/lib.test.sh"
  # The merge path tests itself on every merge, strictly. It is deliberately not enforced in the
  # production gate: the watchdog installed on the host still calls deploy/agent-merge.test.sh, now a
  # report-only shim, so a host-environment difference cannot quarantine a SHA and trap its own fix.
  bash "$ROOT/deploy/agent-merge.suite.sh"
  # shellcheck disable=SC2046
  bash -n $(find "$ROOT/deploy" -type f -name '*.sh') "$ROOT/deploy/apitoken-db-dump"
  git -C "$ROOT" diff --check
)

am_gate() {
  if [[ -n ${AGENT_MERGE_GATE_CMD:-} ]]; then
    ( cd "$ROOT" && eval "$AGENT_MERGE_GATE_CMD" )
    return
  fi

  # Keep these lanes in step with the complete gate in CONTRIBUTING.md. They are independent, so
  # run them concurrently but always reap all three before reporting a failure.
  local typescript_pid rust_pid deployment_pid
  local typescript_rc=0 rust_rc=0 deployment_rc=0
  am_log 'running local TypeScript, Rust, and deployment gates in parallel'
  am_gate_typescript & typescript_pid=$!
  am_gate_rust & rust_pid=$!
  am_gate_deployment & deployment_pid=$!
  wait "$typescript_pid" || typescript_rc=$?
  wait "$rust_pid" || rust_rc=$?
  wait "$deployment_pid" || deployment_rc=$?
  (( typescript_rc == 0 && rust_rc == 0 && deployment_rc == 0 )) \
    || am_die "local gate lanes failed (typescript=$typescript_rc rust=$rust_rc deployment=$deployment_rc)"
}

# Resolves a GitHub token without asking anybody to configure one. $GITHUB_TOKEN wins when set;
# otherwise we reuse the credential git already uses to push to this remote, which every
# contributor who can push normally already has. GIT_TERMINAL_PROMPT=0 keeps a contributor without
# a credential helper (an SSH remote, say) from being prompted or hung. Missing status access fails
# closed before the gate or merge; it is never delegated to a human as a token/proof request.
am_token() {
  if [[ -n ${GITHUB_TOKEN:-} ]]; then
    printf '%s' "$GITHUB_TOKEN"
    return
  fi
  local remote_url host=github.com
  remote_url=$(git -C "$ROOT" remote get-url "$AGENT_MERGE_REMOTE" 2>/dev/null || printf '')
  if [[ $remote_url == https://* ]]; then
    # An https remote names its own host, which may be a GitHub Enterprise instance.
    host=${remote_url#https://}
    host=${host%%/*}
    host=${host#*@}
  fi
  GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true SSH_ASKPASS=true \
    git -C "$ROOT" credential fill 2>/dev/null <<EOF | sed -n 's/^password=//p' | head -n 1 || printf ''
protocol=https
host=$host

EOF
}

am_require_status_access() {
  if [[ -n ${AGENT_MERGE_STATUS_CMD:-} \
        && -n ${AGENT_MERGE_VALIDATION_REQUEST_CMD:-} \
        && -n ${AGENT_MERGE_VALIDATION_STATUS_CMD:-} ]]; then
    return
  fi
  local token
  token=$(am_token)
  [[ -n $token ]] || am_die 'autonomous GitHub deployment/validation access is unavailable: git has no
  reusable HTTPS credential for this remote. No gate or merge was attempted. Repair the local git
  credential helper/remote authentication and rerun; never ask a human for a token or green proof.'
}

# Prints the verdict for a SHA: success, pending, failure, error or unknown.
#
# Deliberately NOT the combined status. GitHub reports a combined state of "success" as soon as
# every context posted so far is green, and the fast ones (Vercel) post a minute before the host
# posts deploy/tests. Polling the combined state therefore reports a green deployment for a SHA
# whose gate has not started, which is exactly how 316691e was announced as green and then failed.
# The verdict is the state of the one context that means the pipeline finished; its absence is
# pending, never success.
#
# The token is passed to curl through -K so it never reaches argv, a log line, or the process list,
# the same discipline deploy/watchdog-github.sh uses on the production host.
am_status() {
  if [[ -n ${AGENT_MERGE_STATUS_CMD:-} ]]; then
    eval "$AGENT_MERGE_STATUS_CMD $1" || printf 'unknown'
    return
  fi
  local token
  token=$(am_token)
  [[ -n $token ]] || { printf 'unknown'; return; }
  curl -fsSL --max-time 30 -K - \
    "https://api.github.com/repos/$AGENT_MERGE_REPO/commits/$1/status" 2>/dev/null <<EOF \
    | node -e '
let raw = "";
process.stdin.on("data", (chunk) => { raw += chunk; });
process.stdin.on("end", () => {
  const required = process.argv[1];
  try {
    const payload = JSON.parse(raw);
    const verdict = (payload.statuses || []).find((s) => s.context === required);
    process.stdout.write(verdict ? String(verdict.state) : "pending");
  } catch { process.stdout.write("unknown"); }
});
' "$AGENT_MERGE_REQUIRED_CONTEXT" 2>/dev/null || printf 'unknown'
header = "Authorization: Bearer $token"
header = "Accept: application/vnd.github+json"
EOF
}

am_validation_request() {
  local sha=$1 token body response deployment_id
  if [[ -n ${AGENT_MERGE_VALIDATION_REQUEST_CMD:-} ]]; then
    eval "$AGENT_MERGE_VALIDATION_REQUEST_CMD $sha"
    return
  fi
  token=$(am_token)
  [[ -n $token ]] || return 1
  body=$(node -e '
const [ref, environment, branch] = process.argv.slice(1);
process.stdout.write(JSON.stringify({
  ref,
  environment,
  description: `Trusted pre-merge validation for ${branch}`,
  auto_merge: false,
  required_contexts: [],
  transient_environment: true,
  production_environment: false,
}));
' "$sha" "$AGENT_MERGE_VALIDATION_ENVIRONMENT" "$BRANCH")
  response=$(curl -fsSL --max-time 30 -K - -X POST \
    "https://api.github.com/repos/$AGENT_MERGE_REPO/deployments" -d "$body" <<EOF
header = "Authorization: Bearer $token"
header = "Accept: application/vnd.github+json"
header = "X-GitHub-Api-Version: 2022-11-28"
header = "Content-Type: application/json"
EOF
  ) || return 1
  deployment_id=$(node -e '
let raw = "";
process.stdin.on("data", (chunk) => { raw += chunk; });
process.stdin.on("end", () => {
  try {
    const id = JSON.parse(raw).id;
    if (!Number.isSafeInteger(id) || id < 1) process.exit(1);
    process.stdout.write(String(id));
  } catch { process.exit(1); }
});
' <<<"$response") || return 1
  [[ $deployment_id =~ ^[1-9][0-9]*$ ]] || return 1
  printf '%s' "$deployment_id"
}

am_validation_status() {
  local deployment_id=$1 token response
  if [[ -n ${AGENT_MERGE_VALIDATION_STATUS_CMD:-} ]]; then
    eval "$AGENT_MERGE_VALIDATION_STATUS_CMD $deployment_id" || printf 'unknown'
    return
  fi
  token=$(am_token)
  [[ -n $token ]] || { printf 'unknown'; return; }
  response=$(curl -fsSL --max-time 30 -K - \
    "https://api.github.com/repos/$AGENT_MERGE_REPO/deployments/$deployment_id/statuses?per_page=100" <<EOF
header = "Authorization: Bearer $token"
header = "Accept: application/vnd.github+json"
header = "X-GitHub-Api-Version: 2022-11-28"
EOF
  ) || { printf 'unknown'; return; }
  node -e '
let raw = "";
process.stdin.on("data", (chunk) => { raw += chunk; });
process.stdin.on("end", () => {
  try {
    const statuses = JSON.parse(raw);
    if (!Array.isArray(statuses)) throw new Error("not an array");
    const states = statuses.map((status) => String(status.state || ""));
    // A later successful request may auto-inactivate an older environment deployment. An exact
    // deployment that recorded success still passed even if its newest status is now inactive.
    if (states.includes("success")) process.stdout.write("success");
    else process.stdout.write(states[0] || "queued");
  } catch { process.stdout.write("unknown"); }
});
' <<<"$response" 2>/dev/null || printf 'unknown'
}

VALIDATION_FAILURE_DETAIL=
am_wait_for_validation() {
  local deployment_id=$1 sha=$2 waited=0 verdict
  VALIDATION_FAILURE_DETAIL=
  while (( waited < AGENT_MERGE_DEPLOY_WAIT_S )); do
    verdict=$(am_validation_status "$deployment_id")
    case "$verdict" in
      success)
        am_log "trusted host validation is GREEN for $sha (deployment $deployment_id)"
        return 0
        ;;
      failure|error|inactive)
        VALIDATION_FAILURE_DETAIL="reported $verdict"
        return 1
        ;;
      queued|pending|in_progress)
        am_log "trusted host validation is $verdict for $sha; waiting autonomously (${waited}s)"
        ;;
      unknown)
        am_log "trusted validation lookup for $sha is temporarily unavailable; retrying autonomously (${waited}s)"
        ;;
      *)
        am_log "unexpected trusted validation status '$verdict' for $sha; retrying autonomously (${waited}s)"
        ;;
    esac
    sleep "$AGENT_MERGE_POLL_S"
    waited=$(( waited + AGENT_MERGE_POLL_S ))
  done
  VALIDATION_FAILURE_DETAIL="did not settle within ${AGENT_MERGE_DEPLOY_WAIT_S}s"
  return 1
}

am_publish_validation_sha() {
  local sha=$1 remote_sha
  [[ $(git -C "$ROOT" rev-parse HEAD) == "$sha" ]] \
    || am_die "refusing to publish a validation ref for a SHA other than HEAD"
  remote_sha=$(git -C "$ROOT" rev-parse "$AGENT_MERGE_REMOTE/$BRANCH" 2>/dev/null || printf '')
  [[ $remote_sha != "$sha" ]] || return 0
  am_log "publishing exact candidate $sha on its feature branch for trusted host validation"
  git -C "$ROOT" push --force-with-lease "$AGENT_MERGE_REMOTE" \
    "HEAD:refs/heads/$BRANCH"
  remote_sha=$(git -C "$ROOT" rev-parse "$AGENT_MERGE_REMOTE/$BRANCH" 2>/dev/null || printf '')
  [[ $remote_sha == "$sha" ]] \
    || am_die "feature branch did not publish the exact candidate required for host validation"
}

am_require_target_gateable() {
  local sha=$1 waited=0 verdict
  while (( waited < AGENT_MERGE_DEPLOY_WAIT_S )); do
    verdict=$(am_status "$sha")
    case "$verdict" in
      success)
        am_log "$AGENT_MERGE_REQUIRED_CONTEXT is GREEN for existing $AGENT_MERGE_TARGET $sha"
        return
        ;;
      pending)
        # The immutable target commit is already available even though its production rollout is
        # not finished. It is safe to rebase and test a descendant speculatively; the locked check
        # below still refuses to push until this exact target reports green.
        am_log "$AGENT_MERGE_TARGET $sha is committed and still deploying; starting speculative exact-SHA gates"
        return
        ;;
      failure|error)
        if (( FIX_RED )); then
          am_log "WARNING: $AGENT_MERGE_TARGET is RED at $sha; proceeding because --fix-red was given"
          return
        fi
        am_die "$AGENT_MERGE_TARGET is RED at $sha: land the repair for that failure with
  --fix-red, or wait. Never stack unrelated work on a red target, and never retry the red SHA."
        ;;
      unknown)
        am_log "deployment-status lookup for $sha is temporarily unavailable; retrying autonomously (${waited}s)"
        ;;
      *)
        am_log "unexpected deployment status '$verdict' for $sha; retrying autonomously (${waited}s)"
        ;;
    esac
    sleep "$AGENT_MERGE_POLL_S"
    waited=$(( waited + AGENT_MERGE_POLL_S ))
  done
  am_die "could not establish whether existing $AGENT_MERGE_TARGET $sha is safe to test against
  within ${AGENT_MERGE_DEPLOY_WAIT_S}s. No gate or merge was attempted."
}

am_wait_for_target_ready() {
  local sha=$1 waited=0 verdict
  while (( waited < AGENT_MERGE_DEPLOY_WAIT_S )); do
    verdict=$(am_status "$sha")
    case "$verdict" in
      success)
        am_log "$AGENT_MERGE_REQUIRED_CONTEXT is GREEN for existing $AGENT_MERGE_TARGET $sha"
        return ;;
      failure|error)
        # Recovering a red master means merging a new commit on top of it, so this is refusable but
        # not forbidden. --fix-red is for the commit that repairs the failure, never unrelated work.
        if (( FIX_RED )); then
          am_log "WARNING: $AGENT_MERGE_TARGET is RED at $sha; proceeding because --fix-red was given"
          return
        fi
        am_die "$AGENT_MERGE_TARGET is RED at $sha: land the repair for that failure with
  --fix-red, or wait. Never stack unrelated work on a red target, and never retry the red SHA." ;;
      pending)
        am_log "$AGENT_MERGE_TARGET is still deploying at $sha; waiting for $AGENT_MERGE_REQUIRED_CONTEXT autonomously (${waited}s)" ;;
      unknown)
        am_log "deployment-status lookup for $sha is temporarily unavailable; retrying autonomously (${waited}s)" ;;
      *)
        am_log "unexpected deployment status '$verdict' for $sha; retrying autonomously (${waited}s)" ;;
    esac
    sleep "$AGENT_MERGE_POLL_S"
    waited=$(( waited + AGENT_MERGE_POLL_S ))
  done
  am_die "could not verify a green $AGENT_MERGE_REQUIRED_CONTEXT for existing $AGENT_MERGE_TARGET
  $sha within ${AGENT_MERGE_DEPLOY_WAIT_S}s. No merge was attempted. Diagnose the status API or
  credential helper and rerun; never ask a human for a token or deployment proof."
}

# --- preflight ---------------------------------------------------------------------------------
git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 || am_die 'not a git repository'

git_dir=$(git -C "$ROOT" rev-parse --absolute-git-dir)
common_dir=$(cd -- "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null \
  || git -C "$ROOT" rev-parse --git-common-dir)" && pwd)
if [[ $git_dir == "$common_dir" && $ALLOW_PRIMARY_TREE -eq 0 ]]; then
  am_die 'refusing to merge from the primary working tree: agents must work in their own
  worktree (git worktree add ~/wt/<task> -b <type>/<task> origin/master). A human contributor in
  a plain clone may pass --allow-primary-tree.'
fi

BRANCH=$(git -C "$ROOT" rev-parse --abbrev-ref HEAD)
[[ $BRANCH != HEAD ]] || am_die 'detached HEAD: check out your own branch first'
[[ $BRANCH != "$AGENT_MERGE_TARGET" ]] \
  || am_die "refusing to run on $AGENT_MERGE_TARGET: work happens on your own branch"
[[ -z $(git -C "$ROOT" status --porcelain) ]] \
  || am_die 'working tree is dirty: commit your own paths (never git add -A) or clean it up'
git -C "$ROOT" rev-parse --verify -q "$BRANCH@{upstream}" >/dev/null \
  || am_die "branch has no upstream: git push -u $AGENT_MERGE_REMOTE HEAD"

# Reject a red production baseline before spending time on the gates. A pending baseline is already
# an immutable commit, so its descendant can be tested speculatively while that rollout finishes.
# The same target must be green under the merge lock before anything is pushed.
am_require_status_access
git -C "$ROOT" fetch "$AGENT_MERGE_REMOTE"
previous=$(git -C "$ROOT" rev-parse "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET")
am_log "checking whether existing $AGENT_MERGE_TARGET $previous is safe to test against"
am_require_target_gateable "$previous"

# Rebase before either expensive gate. This makes their exact SHA a descendant of the latest
# committed target and avoids knowingly validating a SHA that the locked rebase would replace.
am_log "rebasing the candidate onto committed $AGENT_MERGE_TARGET $previous before parallel gates"
git -C "$ROOT" rebase "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET"

# Fail fast, before queueing behind anyone, on a tree that cannot pass anyway. The host receives
# the exact already-pushed SHA first, so its path-aware gate runs concurrently with this local full
# gate. Neither result is trusted for a different SHA.
candidate=$(git -C "$ROOT" rev-parse HEAD)
am_publish_validation_sha "$candidate"
validation_id=
if (( DRY_RUN )); then
  am_log "dry run: skipping the external trusted-host validation request"
else
  validation_id=$(am_validation_request "$candidate") \
    || am_die "could not create trusted host validation for $candidate"
  [[ $validation_id =~ ^[1-9][0-9]*$ ]] \
    || am_die "GitHub returned an invalid trusted validation deployment id"
  am_log "requested trusted host validation for $candidate (deployment $validation_id)"
fi
am_log "running the full gate on $(git -C "$ROOT" rev-parse --short HEAD) before queueing"
am_gate
GATED_SHA=$(git -C "$ROOT" rev-parse HEAD)
[[ $GATED_SHA == "$candidate" ]] || am_die "the candidate SHA changed while its local gate ran"
VALIDATED_SHA=
if (( DRY_RUN )); then
  VALIDATED_SHA=$candidate
elif am_wait_for_validation "$validation_id" "$candidate"; then
  VALIDATED_SHA=$candidate
else
  # Another serialized merge may land while our two gates run. Its new production baseline can
  # legitimately make this old request fail the ancestry fence; keep the local result, then rebase
  # and request validation for the new exact SHA under the lock.
  git -C "$ROOT" fetch "$AGENT_MERGE_REMOTE"
  latest_target=$(git -C "$ROOT" rev-parse "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET")
  if [[ $latest_target == "$previous" ]]; then
    am_die "trusted host validation for $candidate $VALIDATION_FAILURE_DETAIL; no merge was attempted"
  fi
  am_log "discarding stale host validation for $candidate because $AGENT_MERGE_TARGET moved to $latest_target"
fi

# --- lock --------------------------------------------------------------------------------------
mkdir -p -- "$(dirname -- "$AGENT_MERGE_LOCK")"
waited=0
until mkdir -- "$AGENT_MERGE_LOCK" 2>/dev/null; do
  owner=$(cat -- "$AGENT_MERGE_LOCK/pid" 2>/dev/null || printf '')
  owner_host=$(cat -- "$AGENT_MERGE_LOCK/host" 2>/dev/null || printf '')
  age=$(( $(am_now) - $(am_mtime "$AGENT_MERGE_LOCK") ))
  if [[ -n $owner && $owner_host == "$(hostname)" ]] && ! kill -0 "$owner" 2>/dev/null \
    && (( age > AGENT_MERGE_STALE_S )); then
    am_log "breaking a stale lock left by dead pid $owner (${age}s old)"
    rm -rf -- "$AGENT_MERGE_LOCK"
    continue
  fi
  (( waited < AGENT_MERGE_LOCK_WAIT_S )) \
    || am_die "another merge held the lock for ${AGENT_MERGE_LOCK_WAIT_S}s (owner pid ${owner:-?}); try again later"
  am_log "waiting for the merge lock, held by ${owner:-?} on ${owner_host:-?} (${waited}s)"
  sleep "$AGENT_MERGE_POLL_S"
  waited=$(( waited + AGENT_MERGE_POLL_S ))
done
printf '%s\n' "$$" >"$AGENT_MERGE_LOCK/pid"
hostname >"$AGENT_MERGE_LOCK/host"
printf '%s\n' "$BRANCH" >"$AGENT_MERGE_LOCK/branch"
trap 'rm -rf -- "$AGENT_MERGE_LOCK"' EXIT
am_log 'merge lock acquired'

# --- never stack onto a red or in-flight target --------------------------------------------------
git -C "$ROOT" fetch "$AGENT_MERGE_REMOTE"
previous=$(git -C "$ROOT" rev-parse "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET")
am_log "rechecking $AGENT_MERGE_REQUIRED_CONTEXT for locked $AGENT_MERGE_TARGET $previous"
am_wait_for_target_ready "$previous"

# --- rebase, re-gate the exact tree we push, push ------------------------------------------------
pushed=''
for attempt in $(seq 1 "$AGENT_MERGE_PUSH_ATTEMPTS"); do
  git -C "$ROOT" rebase "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET"
  candidate=$(git -C "$ROOT" rev-parse HEAD)
  if [[ $candidate == "$GATED_SHA" && $candidate == "$VALIDATED_SHA" ]]; then
    am_log "reusing the local and trusted host gates already passed by unchanged SHA $(git -C "$ROOT" rev-parse --short HEAD)"
  else
    am_publish_validation_sha "$candidate"
    validation_id=
    if (( ! DRY_RUN )); then
      validation_id=$(am_validation_request "$candidate") \
        || am_die "could not create trusted host validation for rebased SHA $candidate"
      [[ $validation_id =~ ^[1-9][0-9]*$ ]] \
        || am_die "GitHub returned an invalid trusted validation deployment id"
      am_log "requested trusted host validation for rebased SHA $candidate (deployment $validation_id)"
    fi
    if [[ $candidate == "$GATED_SHA" ]]; then
      am_log "reusing the local full gate for $candidate while renewing trusted host validation"
    else
      am_log "re-running the local gate because the rebased SHA changed to $(git -C "$ROOT" rev-parse --short HEAD)"
      am_gate
      [[ $(git -C "$ROOT" rev-parse HEAD) == "$candidate" ]] \
        || am_die "the rebased candidate SHA changed while its local gate ran"
      GATED_SHA=$candidate
    fi
    if (( DRY_RUN )); then
      VALIDATED_SHA=$candidate
    elif am_wait_for_validation "$validation_id" "$candidate"; then
      VALIDATED_SHA=$candidate
    else
      am_die "trusted host validation for rebased SHA $candidate $VALIDATION_FAILURE_DETAIL"
    fi
  fi
  [[ $candidate == "$GATED_SHA" && $candidate == "$VALIDATED_SHA" ]] \
    || am_die "refusing to push a candidate that did not pass both exact-SHA gates"
  if (( DRY_RUN )); then
    am_log "dry run: would push $candidate to $AGENT_MERGE_TARGET"
    exit 0
  fi
  if git -C "$ROOT" push "$AGENT_MERGE_REMOTE" "HEAD:$AGENT_MERGE_TARGET"; then
    pushed=$(git -C "$ROOT" rev-parse HEAD)
    break
  fi
  am_log "$AGENT_MERGE_TARGET moved under us, retrying ($attempt/$AGENT_MERGE_PUSH_ATTEMPTS)"
  git -C "$ROOT" fetch "$AGENT_MERGE_REMOTE"
  previous=$(git -C "$ROOT" rev-parse "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET")
  am_log "checking $AGENT_MERGE_REQUIRED_CONTEXT for newly moved $AGENT_MERGE_TARGET $previous"
  am_wait_for_target_ready "$previous"
done
[[ -n $pushed ]] \
  || am_die "could not fast-forward $AGENT_MERGE_TARGET after $AGENT_MERGE_PUSH_ATTEMPTS attempts"
git -C "$ROOT" push --force-with-lease "$AGENT_MERGE_REMOTE" "HEAD:$BRANCH" || true
am_log "pushed $pushed to $AGENT_MERGE_TARGET"

# --- hold the lock until our own deployment settles ----------------------------------------------
waited=0
while (( waited < AGENT_MERGE_DEPLOY_WAIT_S )); do
  case "$(am_status "$pushed")" in
    success)
      am_log "deploy/watchdog is GREEN for $pushed"
      exit 0 ;;
    failure|error)
      am_die "deploy/watchdog is RED for $pushed: fix it on a NEW branch with a NEW commit;
  never retry this SHA" ;;
    unknown)
      am_log "deployment-status lookup for $pushed is temporarily unavailable; retrying autonomously (${waited}s)" ;;
  esac
  am_log "waiting for deploy/watchdog on $pushed (${waited}s)"
  sleep "$AGENT_MERGE_POLL_S"
  waited=$(( waited + AGENT_MERGE_POLL_S ))
done
am_die "deploy/watchdog did not settle for $pushed within ${AGENT_MERGE_DEPLOY_WAIT_S}s:
  diagnose the status API or watchdog logs before another merge; never ask a human for a token or
  deployment proof"
