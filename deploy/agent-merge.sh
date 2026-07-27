#!/usr/bin/env bash
# Serialized, gated merge of the current branch into master.
#
# Several agents and contributors work on this repository at the same time, and a push to master
# is a production deployment trigger. This script is the only supported way to land work:
#
#   * it never checks out master, so it cannot disturb a co-resident working tree;
#   * it holds a machine-wide lock across the merge AND the deployment that follows, so two
#     candidates can never be tested or deployed on top of each other;
#   * it runs the full gate on the exact tree it pushes, after the rebase;
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

am_gate() {
  if [[ -n ${AGENT_MERGE_GATE_CMD:-} ]]; then
    ( cd "$ROOT" && eval "$AGENT_MERGE_GATE_CMD" )
    return
  fi
  # Keep in step with the complete gate in CONTRIBUTING.md.
  cd "$ROOT"
  pnpm install --frozen-lockfile
  pnpm build
  pnpm typecheck
  pnpm test
  cargo test --locked --workspace
  # The merge path tests itself on every merge, strictly. It is deliberately not enforced in the
  # production gate: the watchdog installed on the host still calls deploy/agent-merge.test.sh, now a
  # report-only shim, so a host-environment difference cannot quarantine a SHA and trap its own fix.
  bash "$ROOT/deploy/agent-merge.suite.sh"
  # shellcheck disable=SC2046
  bash -n $(find "$ROOT/deploy" -type f -name '*.sh') "$ROOT/deploy/apitoken-db-dump"
  git -C "$ROOT" diff --check
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
  [[ -n ${AGENT_MERGE_STATUS_CMD:-} ]] && return
  local token
  token=$(am_token)
  [[ -n $token ]] || am_die 'autonomous GitHub deployment-status access is unavailable: git has no
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

# Check production before spending time on the local gate. The same verdict is checked again under
# the merge lock because master can move while the gate runs.
am_require_status_access
git -C "$ROOT" fetch "$AGENT_MERGE_REMOTE"
previous=$(git -C "$ROOT" rev-parse "$AGENT_MERGE_REMOTE/$AGENT_MERGE_TARGET")
am_log "checking $AGENT_MERGE_REQUIRED_CONTEXT for existing $AGENT_MERGE_TARGET $previous before the gate"
am_wait_for_target_ready "$previous"

# Fail fast, before queueing behind anyone, on a tree that cannot pass anyway.
am_log "running the full gate on $(git -C "$ROOT" rev-parse --short HEAD) before queueing"
am_gate
GATED_SHA=$(git -C "$ROOT" rev-parse HEAD)

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
  if [[ $candidate == "$GATED_SHA" ]]; then
    am_log "reusing the full gate already passed by unchanged SHA $(git -C "$ROOT" rev-parse --short HEAD)"
  else
    am_log "re-running the gate because the rebased SHA changed to $(git -C "$ROOT" rev-parse --short HEAD)"
    am_gate
    GATED_SHA=$candidate
  fi
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
